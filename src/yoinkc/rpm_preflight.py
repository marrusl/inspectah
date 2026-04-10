"""
RPM package availability preflight check.

Validates that packages in the install set exist in the target base image's
repos before rendering the Containerfile. Runs a two-phase check inside a
temporary container:
  Phase 1: Bootstrap repo-providing packages (e.g., epel-release)
  Phase 2: dnf repoquery --available to check package existence

Results are stored as PreflightResult in the InspectionSnapshot.
"""

import configparser
import shutil
import tempfile
from datetime import datetime, timezone
from pathlib import Path
from typing import List, Optional

from ._util import debug as _debug_fn
from .executor import Executor
from .install_set import resolve_install_set
from .schema import (
    InspectionSnapshot,
    PreflightResult,
    RepoStatus,
    UnverifiablePackage,
)

_DIRECT_INSTALL_REPOS = frozenset({"", "(none)", "commandline", "(commandline)", "installed"})

# DNF stderr patterns that indicate a repo failed to sync/download metadata.
_REPO_FAILURE_PATTERNS = (
    "Failed to synchronize cache for repo",
    "Failed to download metadata for repo",
    "Cannot download repomd.xml",
    "Errors during downloading metadata for repository",
)


def _debug(msg: str) -> None:
    _debug_fn("rpm_preflight", msg)


def _stage_config_tree(snapshot: InspectionSnapshot) -> Optional[Path]:
    """Stage snapshot repo files, GPG keys, and dnf config to a temp directory.

    Returns the temp directory path (caller must clean up), or None if
    the snapshot has no custom config to stage.

    The layout mirrors what the renderer's ``write_config_tree`` produces:
      staging/etc/yum.repos.d/*.repo
      staging/etc/pki/rpm-gpg/RPM-GPG-KEY-*
      staging/etc/dnf/...
    """
    has_repos = snapshot.rpm and snapshot.rpm.repo_files and any(r.include for r in snapshot.rpm.repo_files)
    has_gpg = snapshot.rpm and snapshot.rpm.gpg_keys and any(k.include for k in snapshot.rpm.gpg_keys)
    # dnf config files live in snapshot.config with paths starting "etc/dnf/"
    has_dnf_conf = (
        snapshot.config and snapshot.config.files
        and any(f.include and f.path.startswith("etc/dnf/") for f in snapshot.config.files)
    )

    if not has_repos and not has_gpg and not has_dnf_conf:
        return None

    staging = Path(tempfile.mkdtemp(prefix="yoinkc-preflight-"))

    if has_repos:
        for repo in snapshot.rpm.repo_files:
            if not repo.include or not repo.path:
                continue
            dest = staging / repo.path
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_text(repo.content or "")

    if has_gpg:
        for key in snapshot.rpm.gpg_keys:
            if not key.include or not key.path:
                continue
            dest = staging / key.path
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_text(key.content or "")

    if has_dnf_conf:
        for f in snapshot.config.files:
            if not f.include or not f.path.startswith("etc/dnf/"):
                continue
            dest = staging / f.path
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_text(f.content or "")

    return staging


def _classify_direct_installs(snapshot: InspectionSnapshot) -> tuple[list[str], list[str]]:
    """Split install set into (repo_packages, direct_install_packages).

    Direct-install packages have no repo origin (source_repo is empty,
    "(none)", "commandline", etc.) and cannot be installed from a repo.

    Synthetic packages injected by resolve_install_set() (e.g., tuned)
    that are not in packages_added are treated as repo packages — they
    must be validated by repoquery, not skipped as direct-install.
    """
    if not snapshot.rpm or not snapshot.rpm.packages_added:
        return [], []

    install_set = set(resolve_install_set(snapshot))
    direct: list[str] = []
    repo_pkgs: list[str] = []

    # Build a lookup of source_repo by package name (only real packages)
    source_repos = {}
    for p in snapshot.rpm.packages_added:
        if p.name not in source_repos:
            source_repos[p.name] = p.source_repo

    for name in sorted(install_set):
        if name not in source_repos:
            # Synthetic injection (e.g., tuned) — not in packages_added,
            # so no source_repo to check. Treat as repo package so
            # repoquery validates it.
            repo_pkgs.append(name)
        elif source_repos[name].strip().lower() in _DIRECT_INSTALL_REPOS:
            direct.append(name)
        else:
            repo_pkgs.append(name)

    return repo_pkgs, direct


def _provider_repo_ids(snapshot: InspectionSnapshot) -> dict[str, set[str]]:
    """Map each repo-providing package to the repo IDs it provides.

    Parses [reponame] headers from .repo files owned by each provider.
    Returns {provider_pkg_name: {repo_id, ...}}.
    """
    if not snapshot.rpm:
        return {}

    # Build mapping: .repo file path -> owning package
    # We know repo_providing_packages and repo_files; cross-reference
    # by checking which repo files are non-default (user-added).
    result: dict[str, set[str]] = {}
    for repo_file in (snapshot.rpm.repo_files or []):
        if repo_file.is_default_repo:
            continue  # Base-image repo, not from a provider package
        # Parse repo IDs from the .repo content
        repo_ids: set[str] = set()
        if repo_file.content:
            parser = configparser.ConfigParser()
            try:
                parser.read_string(repo_file.content)
                repo_ids = set(parser.sections())
            except configparser.Error:
                pass
        if repo_ids:
            # Attribute to all repo-providing packages (conservative)
            # In practice, each .repo file is owned by one provider
            for provider in (snapshot.rpm.repo_providing_packages or []):
                result.setdefault(provider, set()).update(repo_ids)
    return result


def _detect_unreachable_repos(stderr: str) -> list[RepoStatus]:
    """Parse dnf stderr for repo failure messages.

    Returns a list of RepoStatus for repos that could not be queried.
    """
    unreachable: list[RepoStatus] = []
    seen: set[str] = set()
    for line in stderr.splitlines():
        for pattern in _REPO_FAILURE_PATTERNS:
            if pattern in line:
                # Extract repo ID — typically quoted or at end of message
                # e.g., "Failed to synchronize cache for repo 'epel'"
                repo_id = ""
                if "'" in line:
                    parts = line.split("'")
                    if len(parts) >= 2:
                        repo_id = parts[1]
                elif '"' in line:
                    parts = line.split('"')
                    if len(parts) >= 2:
                        repo_id = parts[1]
                if repo_id and repo_id not in seen:
                    seen.add(repo_id)
                    unreachable.append(RepoStatus(
                        repo_id=repo_id,
                        repo_name=repo_id,
                        error=line.strip(),
                    ))
                break
    return unreachable


def run_package_preflight(
    *,
    snapshot: InspectionSnapshot,
    executor: Executor,
) -> PreflightResult:
    """Run the package availability preflight check.

    Parameters
    ----------
    snapshot : InspectionSnapshot
        Snapshot with RPM data populated (packages_added, leaf_packages, etc.).
    executor : Executor
        Command executor (real or mock).

    Returns
    -------
    PreflightResult
        Structured result with available/unavailable/unverifiable packages.
    """
    timestamp = datetime.now(timezone.utc).isoformat()

    # Check prerequisites
    base_image = snapshot.rpm.base_image if snapshot.rpm else None
    if not base_image:
        return PreflightResult(
            status="failed",
            status_reason="No base image configured — cannot run preflight check",
            timestamp=timestamp,
        )

    # Classify direct installs vs repo packages
    repo_packages, direct_installs = _classify_direct_installs(snapshot)
    _debug(f"install set: {len(repo_packages)} repo packages, {len(direct_installs)} direct installs")

    if not repo_packages:
        return PreflightResult(
            status="completed",
            direct_install=direct_installs,
            base_image=base_image,
            timestamp=timestamp,
        )

    # Pull the base image
    pull_result = executor(["podman", "pull", "-q", base_image])
    if pull_result.returncode != 0:
        return PreflightResult(
            status="failed",
            status_reason=f"Base image {base_image} could not be pulled: {pull_result.stderr.strip()[:200]}",
            direct_install=direct_installs,
            base_image=base_image,
            timestamp=timestamp,
        )

    # Stage custom repo/GPG/dnf config from snapshot
    staging_dir = _stage_config_tree(snapshot)

    try:
        return _run_checks(
            snapshot=snapshot,
            executor=executor,
            base_image=base_image,
            repo_packages=repo_packages,
            direct_installs=direct_installs,
            staging_dir=staging_dir,
            timestamp=timestamp,
        )
    finally:
        if staging_dir:
            shutil.rmtree(staging_dir, ignore_errors=True)


def _run_checks(
    *,
    snapshot: InspectionSnapshot,
    executor: Executor,
    base_image: str,
    repo_packages: list[str],
    direct_installs: list[str],
    staging_dir: Optional[Path],
    timestamp: str,
) -> PreflightResult:
    """Run the two-phase check (extracted for staging_dir cleanup)."""

    # Build the podman run command with volume mounts for custom repos
    run_base = ["podman", "run", "--rm"]

    if staging_dir:
        repo_dir = staging_dir / "etc" / "yum.repos.d"
        gpg_dir = staging_dir / "etc" / "pki" / "rpm-gpg"
        dnf_dir = staging_dir / "etc" / "dnf"
        if repo_dir.is_dir():
            run_base += ["-v", f"{repo_dir}:/etc/yum.repos.d/:Z"]
        if gpg_dir.is_dir():
            run_base += ["-v", f"{gpg_dir}:/etc/pki/rpm-gpg/:Z"]
        if dnf_dir.is_dir():
            run_base += ["-v", f"{dnf_dir}:/etc/dnf/:Z"]

    # Phase 1: Bootstrap repo-providing packages
    repo_providers = snapshot.rpm.repo_providing_packages if snapshot.rpm else []
    unverifiable: list[UnverifiablePackage] = []
    bootstrap_failed_providers: set[str] = set()

    if repo_providers:
        _debug(f"phase 1: bootstrapping {repo_providers}")
        bootstrap_cmd = run_base + [base_image, "dnf", "install", "-y"] + list(repo_providers)
        bootstrap_result = executor(bootstrap_cmd)
        if bootstrap_result.returncode != 0:
            _debug(f"repo-provider bootstrap failed: {bootstrap_result.stderr[:200]}")
            bootstrap_failed_providers = set(repo_providers)

    # Phase 2: Check availability via dnf repoquery
    # Use --queryformat "%{name}" for unambiguous name extraction.
    # Avoids NEVRA parsing pitfalls with hyphenated package names.
    _debug(f"phase 2: checking {len(repo_packages)} packages")

    repoquery_cmd = run_base + [
        base_image, "dnf", "repoquery", "--available",
        "--queryformat", "%{name}",
    ] + repo_packages

    repoquery_result = executor(repoquery_cmd)

    # Detect unreachable repos from stderr
    repo_unreachable = _detect_unreachable_repos(repoquery_result.stderr or "")

    if repoquery_result.returncode != 0 and not repoquery_result.stdout.strip():
        # Total failure — no results at all
        if repo_unreachable:
            # Some repos failed — report partial, not failed
            return PreflightResult(
                status="partial",
                status_reason=f"{len(repo_unreachable)} repo(s) unreachable",
                direct_install=direct_installs,
                repo_unreachable=repo_unreachable,
                base_image=base_image,
                timestamp=timestamp,
            )
        return PreflightResult(
            status="failed",
            status_reason=f"dnf repoquery failed: {repoquery_result.stderr.strip()[:200]}",
            direct_install=direct_installs,
            base_image=base_image,
            timestamp=timestamp,
        )

    # Parse results — each line is a plain package name
    found_names: set[str] = set()
    for line in repoquery_result.stdout.splitlines():
        name = line.strip()
        if name:
            found_names.add(name)

    available = sorted(n for n in repo_packages if n in found_names)
    not_found = sorted(n for n in repo_packages if n not in found_names)

    # Classify not-found packages: unavailable vs unverifiable.
    # If repo-provider bootstrap failed, packages whose source_repo
    # matches the failed provider's repos are unverifiable (we couldn't
    # check their repos). Packages from base repos that weren't found
    # are genuinely unavailable.
    unavailable: list[str] = []
    if bootstrap_failed_providers and not_found:
        provider_repos = _provider_repo_ids(snapshot)
        failed_repo_ids: set[str] = set()
        for provider in bootstrap_failed_providers:
            failed_repo_ids.update(provider_repos.get(provider, set()))

        # Pre-compute lowered repo IDs for case-insensitive comparison
        failed_repo_ids_lower = {r.lower() for r in failed_repo_ids}

        # Build source_repo lookup
        source_repos = {}
        for p in snapshot.rpm.packages_added:
            if p.name not in source_repos:
                source_repos[p.name] = p.source_repo

        for pkg in not_found:
            pkg_source = source_repos.get(pkg, "").strip().lower()
            if pkg_source in failed_repo_ids or pkg_source in failed_repo_ids_lower:
                unverifiable.append(UnverifiablePackage(
                    name=pkg,
                    reason=f"repo-providing package(s) {', '.join(sorted(bootstrap_failed_providers))} unavailable",
                ))
            else:
                unavailable.append(pkg)
    else:
        unavailable = not_found

    # Populate affected_packages on unreachable repos
    if repo_unreachable:
        source_repos = {}
        for p in snapshot.rpm.packages_added:
            if p.name not in source_repos:
                source_repos[p.name] = p.source_repo
        for repo_status in repo_unreachable:
            repo_status.affected_packages = sorted(
                name for name in repo_packages
                if source_repos.get(name, "") == repo_status.repo_id
            )

    # Query available repo IDs
    repolist_cmd = run_base + [base_image, "dnf", "repolist", "--quiet"]
    repolist_result = executor(repolist_cmd)
    repos_queried: list[str] = []
    if repolist_result.returncode == 0:
        for line in repolist_result.stdout.splitlines():
            repo_id = line.strip().split()[0] if line.strip() else ""
            if repo_id:
                repos_queried.append(repo_id)

    # Determine status
    if unverifiable or repo_unreachable:
        status = "partial"
        reasons = []
        if bootstrap_failed_providers:
            reasons.append(
                f"repo-providing package(s) {', '.join(sorted(bootstrap_failed_providers))} "
                f"unavailable; {len(unverifiable)} package(s) unverifiable"
            )
        if repo_unreachable:
            reasons.append(f"{len(repo_unreachable)} repo(s) unreachable")
        status_reason = "; ".join(reasons) if reasons else None
    else:
        status = "completed"
        status_reason = None

    return PreflightResult(
        status=status,
        status_reason=status_reason,
        available=available,
        unavailable=unavailable,
        unverifiable=unverifiable,
        direct_install=direct_installs,
        repo_unreachable=repo_unreachable,
        base_image=base_image,
        repos_queried=repos_queried,
        timestamp=timestamp,
    )
