"""Containerfile section: packages (build stage, FROM, repos, GPG keys, dnf install)."""

import functools
import re
from collections import defaultdict
from pathlib import Path
from typing import List

from ...schema import InspectionSnapshot, VersionLockEntry
from ._helpers import _sanitize_shell_value, _TUNED_PROFILE_RE


def _format_nevra(entry: VersionLockEntry) -> str:
    """Format a VersionLockEntry as an installable NEVRA string for dnf."""
    ver_rel = f"{entry.version}-{entry.release}"
    arch_suffix = f".{entry.arch}" if entry.arch and entry.arch != "*" else ""
    if entry.epoch > 0:
        return f"{entry.epoch}:{entry.name}-{ver_rel}{arch_suffix}"
    return f"{entry.name}-{ver_rel}{arch_suffix}"


def _partition_version_locks(
    locks: List[VersionLockEntry],
    min_prevalence: int,
) -> tuple:
    """Partition fleet version lock entries into ``(pinned, fixme)`` lists.

    *pinned* — one winner per ``(name, arch)`` group that is above the
    prevalence threshold; versioned install + versionlock add are emitted.
    *fixme* — below threshold, no fleet data, or tied-out losers.
    """
    from ...inspectors.rpm import _rpmvercmp

    def _evr_cmp(a: VersionLockEntry, b: VersionLockEntry) -> int:
        ep_cmp = (a.epoch > b.epoch) - (a.epoch < b.epoch)
        if ep_cmp != 0:
            return ep_cmp
        vc = _rpmvercmp(a.version, b.version)
        if vc != 0:
            return vc
        return _rpmvercmp(a.release, b.release)

    groups: dict = defaultdict(list)
    for lock in locks:
        groups[(lock.name, lock.arch)].append(lock)

    pinned: List[VersionLockEntry] = []
    fixme: List[VersionLockEntry] = []

    for entries in groups.values():
        no_fleet = [e for e in entries if e.fleet is None]
        with_fleet = [e for e in entries if e.fleet is not None]
        fixme.extend(no_fleet)

        if not with_fleet:
            continue

        total = with_fleet[0].fleet.total
        above = [e for e in with_fleet if e.fleet.count / total * 100 >= min_prevalence]
        fixme.extend(e for e in with_fleet if e not in above)

        if not above:
            continue

        if len(above) == 1:
            pinned.append(above[0])
            continue

        # Multiple entries above threshold: highest count wins, tie-break newer EVR
        max_count = max(e.fleet.count for e in above)
        tied = [e for e in above if e.fleet.count == max_count]
        winner = max(tied, key=functools.cmp_to_key(_evr_cmp))
        pinned.append(winner)
        fixme.extend(e for e in above if e is not winner)

    return pinned, fixme


def section_lines(
    snapshot: InspectionSnapshot,
    *,
    base: str,
    c_ext_pip: list,
    needs_multistage: bool,
) -> list[str]:
    """Return Containerfile lines for build stage, FROM, repos, and packages."""
    lines: list[str] = []

    if needs_multistage:
        lines.append("# === Build stage: compile pip packages with C extensions ===")
        lines.append(f"FROM {base} AS builder")
        lines.append("RUN dnf install -y gcc python3-devel make && dnf clean all && rm -rf /var/cache/dnf /var/lib/dnf/history* /var/log/dnf* /var/log/hawkey.log /var/log/rhsm")
        lines.append("RUN python3 -m venv /tmp/pip-build")
        c_ext_pip.sort()
        specs = " ".join(f"{n}=={v}" for n, v in c_ext_pip)
        lines.append(f"RUN /tmp/pip-build/bin/pip install {specs}")
        lines.append("")

    lines.append("# === Base Image ===")
    os_desc = "unknown"
    if snapshot.os_release:
        os_desc = snapshot.os_release.pretty_name or snapshot.os_release.name or os_desc
    lines.append(f"# Detected: {os_desc}")
    lines.append(f"FROM {base}")

    # Cross-major-version migration warning
    if snapshot.os_release and snapshot.os_release.version_id and snapshot.rpm and snapshot.rpm.base_image:
        source_major = snapshot.os_release.version_id.split(".")[0]
        target_tag = snapshot.rpm.base_image.rsplit(":", 1)[-1] if ":" in snapshot.rpm.base_image else ""
        target_major = re.sub(r"^stream", "", target_tag).split(".")[0] if target_tag else ""
        if source_major and target_major and source_major != target_major:
            lines.append("")
            lines.append("# !! CROSS-MAJOR-VERSION MIGRATION !!")
            lines.append(f"# Source: {os_desc} ({snapshot.os_release.version_id})")
            lines.append(f"# Target: {snapshot.rpm.base_image}")
            lines.append("# Package names, service names, and config formats may have changed.")
            lines.append("# This Containerfile requires heavier manual review than a same-version migration.")

    lines.append("")

    _PYTHON_VERSION_MAP = {"9": "3.9", "10": "3.12"}

    if needs_multistage:
        lines.append("# === Install pre-built pip packages with C extensions ===")
        py_ver = ""
        if snapshot.os_release:
            vid = snapshot.os_release.version_id or ""
            major = vid.split(".")[0]
            os_id = snapshot.os_release.id.lower()
            py_ver = _PYTHON_VERSION_MAP.get(major, "")
            if not py_ver and os_id == "fedora":
                py_ver = "3.12"
        if py_ver:
            lines.append(f"COPY --from=builder /tmp/pip-build/lib/python{py_ver}/site-packages/ "
                         f"/usr/lib/python{py_ver}/site-packages/")
        else:
            lines.append("# FIXME: replace python3.X with the actual Python version in the base image")
            lines.append("COPY --from=builder /tmp/pip-build/lib/python3.X/site-packages/ "
                         "/usr/lib/python3.X/site-packages/")
        lines.append("")

    # Repository Configuration
    included_gpg = [k for k in snapshot.rpm.gpg_keys if k.include] if snapshot.rpm else []
    gpg_copy_dirs = sorted(set(str(Path(k.path).parent) for k in included_gpg)) if included_gpg else []

    if snapshot.rpm and snapshot.rpm.repo_files:
        included_repos = [r for r in snapshot.rpm.repo_files if r.include]
        if included_repos:
            lines.append("# === Repository Configuration ===")
            for d in gpg_copy_dirs:
                lines.append(f"COPY config/{d}/ /{d}/")
            has_yum_repos = any(r.path.startswith("etc/yum.repos.d/") for r in included_repos)
            has_dnf_conf   = any(r.path.startswith("etc/dnf/")         for r in included_repos)
            if has_yum_repos:
                lines.append("COPY config/etc/yum.repos.d/ /etc/yum.repos.d/")
            if has_dnf_conf:
                lines.append("COPY config/etc/dnf/ /etc/dnf/")
            key_note = f" + {len(included_gpg)} GPG key(s)" if included_gpg else ""
            lines.append(f"# {len(included_repos)} repo file(s){key_note} — also included in consolidated COPY config/etc/ below")
            lines.append("")
    elif gpg_copy_dirs:
        lines.append("# === GPG Keys ===")
        for d in gpg_copy_dirs:
            lines.append(f"COPY config/{d}/ /{d}/")
        lines.append("")

    rpm = snapshot.rpm

    # DNF Module Stream Enables (after repo COPYs, before dnf install)
    if rpm and rpm.module_streams:
        emit_streams = sorted(
            [ms for ms in rpm.module_streams if ms.include and not ms.baseline_match],
            key=lambda ms: ms.module_name,
        )
        if emit_streams:
            stream_pairs = " ".join(f"{ms.module_name}:{ms.stream}" for ms in emit_streams)
            lines.append("# --- Enabled DNF module streams ---")
            lines.append(f"RUN dnf module enable -y {stream_pairs}")
            for msg in (rpm.module_stream_conflicts or []):
                lines.append(f"# WARNING: {msg}")
            lines.append("")

    # Compute included version locks (used below regardless of packages_added)
    included_version_locks = [vl for vl in (rpm.version_locks if rpm else []) if vl.include]
    has_pkgs = bool(rpm and rpm.packages_added)

    if rpm and rpm.multiarch_packages:
        lines.append("# FIXME: The following package variants are installed alongside another architecture.")
        lines.append("# Verify whether these 32-bit or otherwise non-native variants are required; they may not be available")
        lines.append("# in the base image repositories.")
        for pkg_nevra in sorted(rpm.multiarch_packages):
            lines.append(f"#   {pkg_nevra}")
        lines.append("")

    if rpm and rpm.duplicate_packages:
        lines.append("# FIXME: The following packages have multiple versions installed (duplicate name.arch).")
        lines.append("# Resolve which version should be installed before building the image.")
        for key in sorted(rpm.duplicate_packages):
            lines.append(f"#   {key}")
        lines.append("")

    # Determine if tuned needs to be injected as a prerequisite package
    needs_tuned_pkg = bool(
        snapshot.kernel_boot and snapshot.kernel_boot.tuned_active
        and _TUNED_PROFILE_RE.match(snapshot.kernel_boot.tuned_active)
    )

    # Package Installation
    if has_pkgs or included_version_locks or needs_tuned_pkg:
        from ...install_set import resolve_install_set

        # Leaf/auto classification (only relevant when packages exist)
        install_names: List[str] = []
        auto_count = 0
        if has_pkgs:
            # FIXME comments for unsafe package names
            included_pkgs = [p for p in rpm.packages_added if p.include]
            for p in included_pkgs:
                if _sanitize_shell_value(p.name, "dnf install") is None:
                    lines.append(f"# FIXME: package name contains unsafe characters, skipped: {p.name!r}")

            # Compute auto_count for the comment (resolve_install_set handles filtering)
            leaf_set = set(rpm.leaf_packages) if rpm.leaf_packages is not None else None
            dep_tree = rpm.leaf_dep_tree or {}
            if leaf_set is not None and not getattr(rpm, "no_baseline", False):
                included_name_set = set(p.name for p in included_pkgs)
                included_leaf_names = leaf_set & included_name_set
                if dep_tree:
                    remaining_auto: set = set()
                    for lf in included_leaf_names:
                        remaining_auto.update(dep_tree.get(lf, []))
                    auto_count = len(remaining_auto)
                else:
                    all_auto = set(rpm.auto_packages) if rpm.auto_packages else set()
                    auto_count = len(all_auto & included_name_set)

        # Use resolve_install_set for all package filtering + tuned injection
        install_names = resolve_install_set(snapshot)

        # Snapshot-derived count excludes synthetically injected tuned
        # (reflects host-observed packages only)
        if has_pkgs:
            included_pkgs = [p for p in rpm.packages_added if p.include]
            tuned_on_host = any(p.name == "tuned" for p in included_pkgs)
            observed_count = len(install_names) if tuned_on_host else len(install_names) - (1 if "tuned" in install_names else 0)
        else:
            observed_count = 0

        lines.append("# === Package Installation ===")
        if has_pkgs:
            if getattr(rpm, "no_baseline", False):
                lines.append("# No baseline — including all installed packages")
            elif auto_count:
                lines.append(f"# Detected: {observed_count} explicitly installed packages "
                             f"(+{auto_count} dependencies pulled in automatically)")
            else:
                lines.append(f"# Detected: {observed_count} packages added beyond base image")

        # Version lock content (fleet-pinned + FIXME, above the main dnf install)
        if included_version_locks:
            fleet_meta = snapshot.meta.get("fleet") if snapshot.meta else None
            if fleet_meta:
                pinned, fixme_locks = _partition_version_locks(
                    included_version_locks, fleet_meta["min_prevalence"]
                )
            else:
                pinned = []
                fixme_locks = included_version_locks

            if pinned:
                for entry in sorted(pinned, key=lambda e: e.name):
                    nevra = _format_nevra(entry)
                    lines.append(f"RUN dnf install -y {nevra}")
                    lines.append(f"RUN dnf versionlock add {nevra}")
                lines.append("")

            if fixme_locks:
                lines.append("# FIXME: The following packages were version-locked on the source system.")
                lines.append("# dnf install will pull the latest available version instead.")
                for entry in sorted(fixme_locks, key=lambda e: e.name):
                    lines.append(f"#   {_format_nevra(entry)}")
                lines.append("")

        if install_names:
            lines.append("RUN dnf install -y \\")
            for n in install_names[:-1]:
                lines.append(f"    {n} \\")
            lines.append(f"    {install_names[-1]} \\")
            lines.append("    && dnf clean all && rm -rf /var/cache/dnf /var/lib/dnf/history* /var/log/dnf* /var/log/hawkey.log /var/log/rhsm")
        if auto_count:
            lines.append(f"# {auto_count} additional package(s) will be pulled in as dependencies")
            lines.append("# See audit-report.md for full package list")
        lines.append("")

    # ostree package overrides (rpm-ostree override replace)
    if rpm and rpm.ostree_overrides:
        lines.append("# === Package Overrides (from rpm-ostree) ===")
        lines.append("# These packages were overridden on the source system.")
        for ovr in sorted(rpm.ostree_overrides, key=lambda o: o.name):
            lines.append(f"# Override: {ovr.name}  base: {ovr.from_nevra} -> {ovr.to_nevra}")
        lines.append("")

    # ostree removed packages (rpm-ostree override remove)
    if rpm and rpm.ostree_removals:
        lines.append("# === Removed Packages (from rpm-ostree) ===")
        removals = sorted(rpm.ostree_removals)
        lines.append("RUN dnf remove -y \\")
        for name in removals[:-1]:
            lines.append(f"    {name} \\")
        lines.append(f"    {removals[-1]} \\")
        lines.append("    || true  # Some packages may not be in base image")
        lines.append("")

    return lines
