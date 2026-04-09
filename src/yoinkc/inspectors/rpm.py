"""
RPM inspector: package list, rpm -Va, repo files, dnf history removed.
Baseline is the target bootc base image package list (or --baseline-packages file).
"""

import json
import os
import re
from pathlib import Path
from typing import Dict, List, Optional, Set, Tuple

from .._util import debug as _debug_fn, detect_rpmdb_path, make_warning, run_rpm_query as _util_run_rpm_query, _RPM_LOCK_DEFINE as _UTIL_RPM_LOCK_DEFINE


def _debug(msg: str) -> None:
    _debug_fn("rpm", msg)


from ..baseline import BaselineResolver, load_baseline_packages_file
from ..executor import Executor
from ..schema import (
    EnabledModuleStream,
    OstreePackageOverride,
    PackageEntry,
    PackageState,
    RepoFile,
    RpmSection,
    RpmVaEntry,
    SystemType,
    VersionLockEntry,
)


RPM_QA_QUERYFORMAT = r"%{EPOCH}:%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}"

_RPM_LOCK_DEFINE = _UTIL_RPM_LOCK_DEFINE

_VIRTUAL_PACKAGES: Set[str] = {
    "gpg-pubkey",
    "gpg-pubkey-release",
}


def _parse_nevr(nevra: str) -> Optional[PackageEntry]:
    """Parse a single NEVRA line from rpm -qa --queryformat.

    Format: epoch:name-version-release.arch
    Epoch is numeric or ``(none)`` when the package has no explicit epoch tag.
    """
    s = nevra.strip()
    if ":" not in s:
        return None
    epoch_part, rest = s.split(":", 1)
    if epoch_part.isdigit():
        epoch = epoch_part
    elif epoch_part == "(none)":
        epoch = "0"
    else:
        return None
    if "." not in rest:
        return None
    base, arch = rest.rsplit(".", 1)
    parts = base.split("-")
    if len(parts) < 3:
        return None
    release = parts[-1]
    version = parts[-2]
    name = "-".join(parts[:-2])
    return PackageEntry(
        name=name,
        epoch=epoch,
        version=version,
        release=release,
        arch=arch,
        state=PackageState.ADDED,
    )


def _parse_rpm_qa(stdout: str, warnings: Optional[list] = None) -> List[PackageEntry]:
    packages = []
    failed = []
    for line in stdout.strip().splitlines():
        line = line.strip()
        if not line:
            continue
        pkg = _parse_nevr(line)
        if pkg:
            packages.append(pkg)
        else:
            failed.append(line)
    total = len(packages) + len(failed)
    if failed and total > 0:
        pct = len(failed) / total * 100
        _debug(f"NEVRA parse failures: {len(failed)} lines ({pct:.0f}%)")
        for f in failed[:10]:
            _debug(f"  failed to parse: {f!r}")
        if warnings is not None:
            severity = "warning" if pct >= 5 else "info"
            warnings.append(make_warning(
                "rpm",
                f"rpm -qa: {len(failed)} package line(s) could not be parsed ({pct:.0f}% of output) — package list may be incomplete.",
                severity,
            ))
    _debug(f"parsed {len(packages)} packages from rpm -qa "
           f"(first 5 names: {[p.name for p in packages[:5]]})")
    return packages


def _detect_multiarch(installed: List[PackageEntry]) -> List[str]:
    """Return actionable ``name.arch`` entries for packages installed in multiple architectures.

    When a package has an ``x86_64`` build plus other architectures, only the
    non-``x86_64`` variants are returned (for example ``zlib.i686``). If there
    is no obvious native architecture in the group, return all affected
    ``name.arch`` pairs so the operator sees the full picture.
    """
    packages_by_name: dict[str, list[PackageEntry]] = {}
    for pkg in installed:
        packages_by_name.setdefault(pkg.name, []).append(pkg)

    flagged: set[str] = set()
    for name, entries in packages_by_name.items():
        arches = {pkg.arch for pkg in entries}
        if len(arches) <= 1:
            continue
        if "x86_64" in arches:
            flagged.update(f"{name}.{pkg.arch}" for pkg in entries if pkg.arch != "x86_64")
            continue
        flagged.update(f"{name}.{pkg.arch}" for pkg in entries)
    return sorted(flagged)


def _detect_duplicates(installed: List[PackageEntry]) -> List[str]:
    """Return name.arch keys that have more than one version installed."""
    counts: dict = {}
    for pkg in installed:
        key = f"{pkg.name}.{pkg.arch}"
        counts[key] = counts.get(key, 0) + 1
    return sorted(key for key, count in counts.items() if count > 1)


def _parse_rpm_va(stdout: str) -> List[RpmVaEntry]:
    """Parse rpm -Va output. Format: flags type path (e.g. S.5....T.  c /etc/foo)."""
    entries = []
    for line in stdout.strip().splitlines():
        line = line.strip()
        if not line:
            continue
        if len(line) < 11:
            continue
        flags = line[:9].strip()
        rest = line[9:].lstrip()
        if rest.startswith("c ") or rest.startswith("d "):
            path = rest[2:].strip()
        else:
            path = rest.strip()
        if path and not path.startswith("/boot/"):
            entries.append(RpmVaEntry(path=path, flags=flags, package=None))
    return entries


def _rpmvercmp(a: str, b: str) -> int:
    """Compare two RPM version/release strings using the rpmvercmp algorithm.

    Returns negative if a < b, 0 if equal, positive if a > b.
    Handles ~(pre-release) and ^(post-release snapshot) markers.
    Pure-Python implementation of RPM's rpmvercmp (see lib/rpmvercmp.c).
    """
    if a == b:
        return 0

    i, j = 0, 0
    while i < len(a) or j < len(b):
        # Skip non-alphanumeric, non-tilde, non-caret separators
        while i < len(a) and not a[i].isalnum() and a[i] not in ("~", "^"):
            i += 1
        while j < len(b) and not b[j].isalnum() and b[j] not in ("~", "^"):
            j += 1

        if i >= len(a) and j >= len(b):
            return 0
        if i >= len(a):
            return -1 if b[j] == "^" else (1 if b[j] == "~" else -1)
        if j >= len(b):
            return 1 if a[i] == "^" else (-1 if a[i] == "~" else 1)

        # Tilde sorts before everything (pre-release)
        if a[i] == "~":
            if b[j] != "~":
                return -1
            i += 1
            j += 1
            continue
        if b[j] == "~":
            return 1

        # Caret sorts after empty but before any alphanumeric
        if a[i] == "^":
            if b[j] != "^":
                return 1 if j >= len(b) or not b[j].isalnum() else -1
            i += 1
            j += 1
            continue
        if b[j] == "^":
            return -1 if i >= len(a) or not a[i].isalnum() else 1

        # Extract contiguous digit or alpha segment
        if a[i].isdigit():
            si = i
            while i < len(a) and a[i].isdigit():
                i += 1
            seg_a = a[si:i]

            sj = j
            if j < len(b) and b[j].isdigit():
                while j < len(b) and b[j].isdigit():
                    j += 1
                seg_b = b[sj:j]
            else:
                return 1  # numeric > alpha

            na = int(seg_a) if seg_a else 0
            nb = int(seg_b) if seg_b else 0
            if na != nb:
                return 1 if na > nb else -1
        else:
            si = i
            while i < len(a) and a[i].isalpha():
                i += 1
            seg_a = a[si:i]

            sj = j
            if j < len(b) and b[j].isalpha():
                while j < len(b) and b[j].isalpha():
                    j += 1
                seg_b = b[sj:j]
            else:
                return -1  # alpha < numeric

            if seg_a != seg_b:
                return 1 if seg_a > seg_b else -1

    return 0


def _compare_evr(host_pkg: "PackageEntry", base_pkg: "PackageEntry") -> int:
    """Compare epoch:version-release between two PackageEntry objects.

    Returns negative if host < base, 0 if equal, positive if host > base.
    Pure-Python implementation of RPM's EVR comparison (see lib/rpmvercmp.c).
    """
    h_epoch = int(host_pkg.epoch or "0")
    b_epoch = int(base_pkg.epoch or "0")
    if h_epoch != b_epoch:
        return 1 if h_epoch > b_epoch else -1

    vc = _rpmvercmp(host_pkg.version, base_pkg.version)
    if vc != 0:
        return vc

    return _rpmvercmp(host_pkg.release, base_pkg.release)


def _read_os_id_version(host_root: Path) -> tuple[str, str]:
    """Read ID and VERSION_ID from host os-release. Returns (id, version_id) or ('', '')."""
    os_release = host_root / "etc" / "os-release"
    if not os_release.exists():
        return "", ""
    id_val = ""
    version_id = ""
    for line in os_release.read_text().splitlines():
        if line.startswith("ID="):
            id_val = line.split("=", 1)[1].strip().strip('"')
        elif line.startswith("VERSION_ID="):
            version_id = line.split("=", 1)[1].strip().strip('"')
    return id_val, version_id


def _populate_source_repos(
    executor: Executor,
    host_root: Path,
    packages: List["PackageEntry"],
) -> None:
    """Set source_repo on each PackageEntry.

    Primary: dnf repoquery --installed (reliable across RHEL 9/10).
    Fallback: rpm -qi (checking both "From repo" and "Repository").
    """
    if not packages:
        return
    name_set = {p.name for p in packages}
    names = sorted(name_set)
    repo_map: dict = {}

    # --- Primary: dnf repoquery ---
    def _try_dnf() -> bool:
        # from_repo is stored in dnf's own database on the host, not in the raw
        # RPM DB that --installroot accesses.  Run on the host via nsenter when
        # inside a container; plain dnf when already on the host.
        if str(host_root) == "/":
            cmd_base = ["dnf", "repoquery", "--installed", "--queryformat", "%{name} %{from_repo}\n"]
        else:
            cmd_base = ["nsenter", "-t", "1", "-m", "-u", "-i", "-n", "--",
                        "dnf", "repoquery", "--installed", "--queryformat", "%{name} %{from_repo}\n"]
        # Probe with the first package
        probe = executor(cmd_base + [names[0]])
        if probe.returncode != 0:
            _debug(f"dnf repoquery probe failed (rc={probe.returncode}), falling back to rpm -qi")
            return False
        # Parse probe result
        for line in probe.stdout.strip().splitlines():
            parts = line.strip().split(None, 1)
            if len(parts) == 2 and parts[0] in name_set:
                repo_map[parts[0]] = parts[1]
        # Process remaining in batches
        batch_size = 100
        remaining = names[1:]
        for i in range(0, len(remaining), batch_size):
            batch = remaining[i:i + batch_size]
            result = executor(cmd_base + batch)
            if result.returncode != 0:
                continue
            for line in result.stdout.strip().splitlines():
                parts = line.strip().split(None, 1)
                if len(parts) == 2 and parts[0] in name_set and parts[0] not in repo_map:
                    repo_map[parts[0]] = parts[1]
        return True

    # --- Fallback: rpm -qi ---
    def _try_rpm() -> None:
        batch_size = 100
        for i in range(0, len(names), batch_size):
            batch = names[i:i + batch_size]
            result = _run_rpm_query(executor, host_root, ["-qi"] + batch)
            if result.returncode != 0:
                continue
            cur_name = ""
            for line in result.stdout.splitlines():
                if line.startswith("Name"):
                    parts = line.split(":", 1)
                    if len(parts) == 2:
                        cur_name = parts[1].strip()
                elif line.startswith("From repo") or line.startswith("Repository"):
                    parts = line.split(":", 1)
                    if len(parts) == 2 and cur_name and cur_name not in repo_map:
                        repo_map[cur_name] = parts[1].strip()

    if not _try_dnf():
        _try_rpm()

    for p in packages:
        p.source_repo = repo_map.get(p.name, "")
    _debug(f"source_repo populated for {len(repo_map)}/{len(names)} packages")


def _query_user_installed(
    executor: Executor,
    host_root: Path,
) -> Optional[Set[str]]:
    """Query dnf for the set of user-installed (explicitly requested) package names.

    Returns None if the query fails (e.g. dnf unavailable).
    """
    if str(host_root) == "/":
        cmd = ["dnf", "repoquery", "--userinstalled", "--queryformat", "%{name}\n"]
    else:
        cmd = ["nsenter", "-t", "1", "-m", "-u", "-i", "-n", "--",
               "dnf", "repoquery", "--userinstalled", "--queryformat", "%{name}\n"]
    result = executor(cmd)
    if result.returncode != 0:
        _debug(f"dnf repoquery --userinstalled failed (rc={result.returncode})")
        return None
    names: Set[str] = set()
    for line in result.stdout.splitlines():
        name = line.strip()
        if name:
            names.add(name)
    _debug(f"dnf repoquery --userinstalled returned {len(names)} packages")
    return names


_DEFAULT_REPO_FILENAME_PATTERNS = ("redhat.repo", "redhat-rhui", "redhat.redhat")
_DEFAULT_REPO_ID_PREFIXES = (
    "rhel-", "baseos", "appstream", "rhui-", "crb", "codeready",
    "fedora", "updates",
)


def _classify_default_repo(repo: RepoFile) -> bool:
    """Return True if *repo* looks like a default distro repository."""
    basename = repo.path.rsplit("/", 1)[-1] if "/" in repo.path else repo.path
    for pat in _DEFAULT_REPO_FILENAME_PATTERNS:
        if pat in basename:
            return True
    for line in repo.content.splitlines():
        stripped = line.strip()
        if stripped.startswith("[") and stripped.endswith("]"):
            section_id = stripped[1:-1]
            for prefix in _DEFAULT_REPO_ID_PREFIXES:
                if section_id.startswith(prefix):
                    return True
    return False


def _collect_repo_files(host_root: Path) -> List[RepoFile]:
    """Read repo files from host_root/etc/yum.repos.d and host_root/etc/dnf."""
    repo_files = []
    for subdir in ("etc/yum.repos.d", "etc/dnf"):
        d = host_root / subdir
        try:
            if not d.exists():
                continue
            entries = sorted(d.iterdir())
        except (PermissionError, OSError):
            continue
        for f in entries:
            if f.is_file() and f.suffix != ".module" and (f.suffix in (".repo", ".conf") or subdir == "etc/dnf"):
                try:
                    content = f.read_text()
                except Exception:
                    content = ""
                rf = RepoFile(path=str(f.relative_to(host_root)), content=content)
                rf.is_default_repo = _classify_default_repo(rf)
                repo_files.append(rf)
    return repo_files


def _resolve_dnf_vars(path: str, host_root: Path) -> str:
    """Replace dnf variable references ($releasever, $releasever_major, $basearch) in *path*.

    Values are read from the host's /etc/os-release and platform.machine().
    Returns the original path unchanged if it contains no variables.
    """
    if "$" not in path:
        return path

    os_release: dict = {}
    try:
        for line in (host_root / "etc/os-release").read_text().splitlines():
            if "=" in line:
                k, v = line.split("=", 1)
                os_release[k] = v.strip().strip('"')
    except (FileNotFoundError, PermissionError, OSError):
        pass

    version_id = os_release.get("VERSION_ID", "")
    major = version_id.split(".")[0] if version_id else ""
    path = path.replace("$releasever_major", major)
    path = path.replace("$releasever", version_id)

    import platform as _platform
    path = path.replace("$basearch", _platform.machine())
    return path


def _collect_gpg_keys(host_root: Path, repo_files: List[RepoFile]) -> List[RepoFile]:
    """Read GPG key files referenced by gpgkey=file:///... in repo configs.

    Handles comma-separated URLs on a single line and INI-style continuation
    lines (indented lines following ``gpgkey=``).  https:// URLs are skipped
    since dnf will fetch them at build time.
    """
    seen: dict = {}
    for repo in repo_files:
        lines = repo.content.splitlines()
        i = 0
        while i < len(lines):
            stripped = lines[i].strip()
            if not stripped.startswith("gpgkey"):
                i += 1
                continue
            _, _, value = stripped.partition("=")
            # Accumulate continuation lines (indented, not a new key or section)
            parts = [value]
            while i + 1 < len(lines):
                next_line = lines[i + 1]
                if next_line and next_line[0] in (" ", "\t") and "=" not in next_line:
                    parts.append(next_line.strip())
                    i += 1
                else:
                    break
            combined = " ".join(parts)
            for token in re.split(r"[,\s]+", combined.strip()):
                token = token.strip()
                if not token.startswith("file://"):
                    continue
                abs_path = token[len("file://"):]
                if "$" in abs_path:
                    abs_path = _resolve_dnf_vars(abs_path, host_root)
                rel_path = abs_path.lstrip("/")
                if rel_path in seen:
                    continue
                key_path = host_root / rel_path
                try:
                    content = key_path.read_text()
                except (FileNotFoundError, PermissionError, OSError):
                    _debug(f"gpgkey file not found or unreadable: {key_path}")
                    continue
                seen[rel_path] = content
            i += 1
    return [RepoFile(path=p, content=c) for p, c in sorted(seen.items())]


def _dnf_history_removed(executor: Executor, host_root: Path, warnings: Optional[list] = None) -> List[str]:
    """Run dnf history and collect package names from Remove transactions."""
    result = executor(["dnf", "history", "list", "-q"], cwd=str(host_root))
    if result.returncode != 0:
        if warnings is not None:
            warnings.append(make_warning(
                "rpm",
                "dnf history unavailable — orphaned config detection (packages removed after install) is incomplete.",
            ))
        return []
    removed = []
    for line in result.stdout.splitlines():
        parts = line.split("|")
        if len(parts) >= 4 and "Removed" in (parts[3].strip() if len(parts) > 3 else ""):
            try:
                tid = int(parts[0].strip())
            except ValueError:
                continue
            info_result = executor(["dnf", "history", "info", str(tid), "-q"], cwd=str(host_root))
            if info_result.returncode != 0:
                continue
            for iline in info_result.stdout.splitlines():
                if "Removed" in iline:
                    pkg_part = iline.split("Removed", 1)[-1].strip().split()
                    if pkg_part:
                        nevra = pkg_part[0]
                        name = re.match(r"^([^-]+(?:-[^-]+)*?)-\d", nevra)
                        if name:
                            removed.append(name.group(1))
                        else:
                            removed.append(nevra.split("-")[0] if "-" in nevra else nevra)
    return removed


def _parse_rpmostree_package_state(
    executor: Executor,
    section: "RpmSection",
    warnings: Optional[list] = None,
    system_type: SystemType = SystemType.PACKAGE_MODE,
) -> None:
    """Parse rpm-ostree status --json for layered, removed, and overridden packages.

    Mutates *section* in place:
    - requested-packages → section.packages_added (as PackageEntry with name only,
      skipping names already present)
    - base-removals → section.ostree_removals
    - base-local-replacements → section.ostree_overrides (as OstreePackageOverride)

    Only the booted deployment is inspected.
    """
    result = executor(["rpm-ostree", "status", "--json"])
    if result.returncode != 0:
        _debug(f"rpm-ostree status failed (rc={result.returncode}), skipping ostree package state")
        if system_type == SystemType.BOOTC and warnings is not None:
            warnings.append(make_warning(
                "rpm",
                "Package diff is approximate -- rpm-ostree status is not available on this bootc system. "
                "Package detection used rpm -qa against the base image, which may differ due to tag drift "
                "or NVR skew. Results require manual review.",
                "warning",
            ))
        return

    try:
        data = json.loads(result.stdout)
    except (json.JSONDecodeError, ValueError) as exc:
        _debug(f"rpm-ostree status returned invalid JSON: {exc}")
        return

    deployments = data.get("deployments", [])
    booted = None
    for dep in deployments:
        if dep.get("booted"):
            booted = dep
            break
    if booted is None:
        _debug("no booted deployment found in rpm-ostree status")
        return

    # Layered packages
    existing_names = {p.name for p in section.packages_added}
    for pkg_name in booted.get("requested-packages", []):
        if pkg_name not in existing_names:
            section.packages_added.append(PackageEntry(
                name=pkg_name,
                epoch="0",
                version="",
                release="",
                arch="noarch",
                state=PackageState.ADDED,
            ))
            existing_names.add(pkg_name)

    # Removed packages
    for removal in booted.get("base-removals", []):
        name = removal.get("name", "")
        if name:
            section.ostree_removals.append(name)

    # Overridden packages
    for replacement in booted.get("base-local-replacements", []):
        name = replacement.get("name", "")
        if name:
            section.ostree_overrides.append(OstreePackageOverride(
                name=name,
                to_nevra=replacement.get("nevra", ""),
                from_nevra=replacement.get("base-nevra", ""),
            ))

    _debug(f"rpm-ostree state: {len(booted.get('requested-packages', []))} layered, "
           f"{len(section.ostree_removals)} removals, "
           f"{len(section.ostree_overrides)} overrides")


def _parse_module_ini(text: str) -> Dict[str, str]:
    """Parse concatenated module INI text and return ``{module_name: stream}``
    for sections whose state is ``enabled`` or ``installed``.

    Used by both the file-based inspector and the podman-based baseline query.
    """
    import configparser

    parser = configparser.ConfigParser()
    try:
        parser.read_string(text)
    except Exception as exc:
        _debug(f"failed to parse module INI: {exc}")
        return {}

    result: Dict[str, str] = {}
    for section in parser.sections():
        state = parser.get(section, "state", fallback="").strip().lower()
        if state not in ("enabled", "installed"):
            continue
        stream = parser.get(section, "stream", fallback="").strip()
        if stream:
            result[section] = stream
    return result


def _collect_module_streams(host_root: Path) -> List[EnabledModuleStream]:
    """Parse enabled/installed DNF module streams from /etc/dnf/modules.d/*.module."""
    import configparser

    modules_dir = host_root / "etc" / "dnf" / "modules.d"
    if not modules_dir.exists():
        return []

    try:
        module_files = sorted(modules_dir.glob("*.module"))
    except (PermissionError, OSError) as exc:
        _debug(f"cannot read modules.d: {exc}")
        return []

    result: List[EnabledModuleStream] = []
    for mf in module_files:
        parser = configparser.ConfigParser()
        try:
            parser.read_string(mf.read_text())
        except Exception as exc:
            _debug(f"skipping malformed module file {mf.name}: {exc}")
            continue

        for section in parser.sections():
            state = parser.get(section, "state", fallback="").strip().lower()
            if state not in ("enabled", "installed"):
                continue
            stream = parser.get(section, "stream", fallback="").strip()
            if not stream:
                _debug(f"module section [{section}] in {mf.name} has no stream, skipping")
                continue
            profiles_raw = parser.get(section, "profiles", fallback="").strip()
            profiles = [p.strip() for p in profiles_raw.split(",") if p.strip()] if profiles_raw else []
            result.append(EnabledModuleStream(
                module_name=section,
                stream=stream,
                profiles=profiles,
            ))

    return result


def _parse_nevra_pattern(raw_line: str) -> VersionLockEntry:
    """Parse a versionlock NEVRA pattern into a VersionLockEntry.

    Handles: ``1:curl-7.76.1-26.el9.x86_64``, ``curl-7.76.1-26.el9.x86_64``,
    ``curl-7.76.1-26.el9.*``, ``python3-urllib3-1.26.5-3.el9.noarch``.
    """
    s = raw_line.strip()

    # Split arch at last dot
    if "." in s:
        rest, arch = s.rsplit(".", 1)
    else:
        rest, arch = s, ""

    # Split epoch if a colon appears before the first dash
    epoch = 0
    colon_pos = rest.find(":")
    dash_pos = rest.find("-")
    if colon_pos != -1 and (dash_pos == -1 or colon_pos < dash_pos):
        epoch = int(rest[:colon_pos])
        rest = rest[colon_pos + 1:]

    # Name/version boundary: first '-' followed immediately by a digit
    match = re.search(r"-(\d)", rest)
    if not match:
        raise ValueError(f"cannot locate name/version boundary in {raw_line!r}")

    name = rest[: match.start()]
    ver_rel = rest[match.start() + 1:]

    if "-" in ver_rel:
        version, release = ver_rel.rsplit("-", 1)
    else:
        version, release = ver_rel, ""

    return VersionLockEntry(
        raw_pattern=s,
        name=name,
        epoch=epoch,
        version=version,
        release=release,
        arch=arch,
    )


def _collect_version_locks(
    executor: Optional[Executor],
    host_root: Path,
) -> tuple:
    """Collect version-lock pins from versionlock.list and dnf versionlock list.

    Returns ``(entries, command_output)`` where *entries* is a list of
    :class:`VersionLockEntry` and *command_output* is the raw string from
    ``dnf versionlock list`` (or ``None`` if unavailable).
    """
    dnf_path = host_root / "etc" / "dnf" / "plugins" / "versionlock.list"
    yum_path = host_root / "etc" / "yum" / "pluginconf.d" / "versionlock.list"

    lock_file = dnf_path if dnf_path.exists() else (yum_path if yum_path.exists() else None)

    entries: List[VersionLockEntry] = []
    if lock_file is not None:
        try:
            for line in lock_file.read_text().splitlines():
                line = line.strip()
                if not line or line.startswith("#"):
                    continue
                try:
                    entries.append(_parse_nevra_pattern(line))
                except ValueError as exc:
                    _debug(f"skipping unparseable versionlock line {line!r}: {exc}")
        except (PermissionError, OSError) as exc:
            _debug(f"cannot read versionlock file {lock_file}: {exc}")

    command_output: Optional[str] = None
    if executor is not None:
        result = executor(["dnf", "versionlock", "list"])
        if result.returncode == 0:
            command_output = result.stdout
        else:
            _debug(f"dnf versionlock list failed (rc={result.returncode})")

    return entries, command_output


def _run_rpm_query(executor: Executor, host_root: Path, args: List[str]):
    """Run an rpm query with --dbpath fallback to --root."""
    return _util_run_rpm_query(executor, host_root, args)


def _classify_deps_via_rpm(
    executor: Executor,
    host_root: Path,
    added_names: Set[str],
) -> dict:
    """Build dependency graph using rpm -qR + --whatprovides.

    Returns ``depends_on`` where ``depends_on[A] = {B, C}`` means A directly
    requires B and C (within *added_names*).
    """
    depends_on: dict = {name: set() for name in added_names}
    name_list = sorted(added_names)
    batch_size = 50

    for i in range(0, len(name_list), batch_size):
        batch = name_list[i:i + batch_size]

        for pkg_name in batch:
            result = _run_rpm_query(executor, host_root, ["-qR", pkg_name])
            if result.returncode != 0:
                continue

            caps = set()
            for line in result.stdout.splitlines():
                cap = line.strip()
                if cap and not cap.startswith("rpmlib(") and not cap.startswith("/"):
                    caps.add(cap.split()[0])

            if not caps:
                continue

            cap_list = sorted(caps)
            for j in range(0, len(cap_list), batch_size):
                cap_batch = cap_list[j:j + batch_size]
                wp_result = _run_rpm_query(executor, host_root, ["-q", "--whatprovides"] + cap_batch)
                if wp_result.returncode != 0:
                    continue
                for pline in wp_result.stdout.splitlines():
                    pline = pline.strip()
                    if not pline or "no package provides" in pline:
                        continue
                    match = re.match(r"^(.+?)-\d", pline)
                    provider = match.group(1) if match else pline.split("-")[0]
                    if provider in added_names and provider != pkg_name:
                        depends_on[pkg_name].add(provider)

    return depends_on


def _classify_deps_via_dnf(
    executor: Executor,
    host_root: Path,
    added_names: Set[str],
) -> Optional[dict]:
    """Build transitive dependency graph using dnf repoquery.

    Returns ``depends_on`` where ``depends_on[A]`` contains the full transitive
    set of dependencies of A (within *added_names*), or ``None`` if dnf
    repoquery is unavailable.
    """
    if not added_names:
        return {name: set() for name in added_names}

    cmd_base = ["dnf", "repoquery"]
    if str(host_root) != "/":
        cmd_base += ["--installroot", str(host_root)]
    cmd_base += ["--requires", "--resolve", "--recursive", "--installed",
                 "--queryformat", "%{name}\n"]

    name_list = sorted(added_names)

    first_result = executor(cmd_base + [name_list[0]])
    if first_result.returncode != 0:
        _debug(f"dnf repoquery unavailable (rc={first_result.returncode}), "
               "will fall back to rpm")
        return None

    depends_on: dict = {name: set() for name in added_names}

    for line in first_result.stdout.splitlines():
        dep_name = line.strip()
        if dep_name and dep_name in added_names and dep_name != name_list[0]:
            depends_on[name_list[0]].add(dep_name)

    for pkg_name in name_list[1:]:
        result = executor(cmd_base + [pkg_name])
        if result.returncode != 0:
            continue
        for line in result.stdout.splitlines():
            dep_name = line.strip()
            if dep_name and dep_name in added_names and dep_name != pkg_name:
                depends_on[pkg_name].add(dep_name)

    return depends_on


def _classify_leaf_auto(
    executor: Executor,
    host_root: Path,
    packages_added: List["PackageEntry"],
) -> tuple:
    """Split added packages into leaf vs auto with per-leaf dependency tree.

    Returns ``(leaf_list, auto_list, leaf_dep_tree)`` where *leaf_dep_tree*
    maps each leaf package name to the sorted list of auto packages it
    pulls in (transitively, within the added set).

    Tries ``dnf repoquery --recursive`` first for accurate transitive
    resolution (handles weak deps, rich boolean deps, etc).  Falls back
    to ``rpm -qR`` + ``--whatprovides`` if dnf is unavailable.
    """
    added_names = {p.name for p in packages_added}

    # Use dnf's user-installed tracking when available — it directly tells us
    # which packages the operator explicitly requested vs pulled in as deps.
    user_installed = _query_user_installed(executor, host_root)

    depends_on = _classify_deps_via_dnf(executor, host_root, added_names)
    transitive = depends_on is not None

    if depends_on is None:
        depends_on = _classify_deps_via_rpm(executor, host_root, added_names)

    if user_installed is not None:
        leaf_set = user_installed & added_names
        if not leaf_set and added_names:
            _debug("dnf --userinstalled returned no overlap with added packages; "
                   "falling back to graph-based classification")
            user_installed = None
        else:
            auto_set_raw = added_names - leaf_set
            leaf = sorted(leaf_set)
            auto = sorted(auto_set_raw)
            _debug(f"using dnf --userinstalled for leaf classification: "
                   f"{len(leaf)} leaf, {len(auto)} auto")
    if user_installed is None:
        _debug("dnf --userinstalled unavailable, falling back to graph-based classification")
        depended_on: Set[str] = set()
        for deps in depends_on.values():
            depended_on.update(deps)
        leaf = sorted(added_names - depended_on)
        auto = sorted(depended_on)

    # Build per-leaf transitive dependency tree
    auto_set = set(auto)
    leaf_dep_tree: dict = {}
    if transitive:
        # dnf repoquery --recursive already gave us the transitive closure
        for lf in leaf:
            leaf_dep_tree[lf] = sorted(depends_on.get(lf, set()) & auto_set)
    else:
        # rpm gives only direct deps; walk the graph
        for lf in leaf:
            reachable: Set[str] = set()
            stack = list(depends_on.get(lf, set()))
            while stack:
                dep = stack.pop()
                if dep in reachable:
                    continue
                reachable.add(dep)
                stack.extend(depends_on.get(dep, set()) - reachable)
            leaf_dep_tree[lf] = sorted(reachable & auto_set)

    return leaf, auto, leaf_dep_tree


def _apply_module_stream_baseline(
    module_streams: List[EnabledModuleStream],
    baseline_streams: Dict[str, str],
    module_stream_conflicts: List[str],
    warnings: Optional[list] = None,
) -> None:
    """Set ``baseline_match`` on each stream entry and populate conflict lists.

    - Same stream as baseline → ``baseline_match = True``
    - Module absent from baseline → ``baseline_match = False``
    - Module present with different stream → conflict message added, ``baseline_match = False``
    """
    for ms in module_streams:
        base_stream = baseline_streams.get(ms.module_name)
        if base_stream is None:
            ms.baseline_match = False
        elif base_stream == ms.stream:
            ms.baseline_match = True
        else:
            ms.baseline_match = False
            msg = f"{ms.module_name}: host={ms.stream}, base_image={base_stream}"
            module_stream_conflicts.append(msg)
            if warnings is not None:
                warnings.append(make_warning("rpm", msg, "warning"))


def run(
    host_root: Path,
    executor: Optional[Executor],
    baseline_packages_file: Optional[Path] = None,
    warnings: Optional[list] = None,
    resolver: Optional[BaselineResolver] = None,
    target_version: Optional[str] = None,
    target_image: Optional[str] = None,
    preflight_baseline: Optional[Tuple[Optional[Dict[str, "PackageEntry"]], Optional[str], bool]] = None,
    system_type: SystemType = SystemType.PACKAGE_MODE,
) -> RpmSection:
    """Run RPM inspection.

    Baseline comes from querying the target bootc base image via podman,
    or from ``--baseline-packages`` file.  If neither is available,
    ``no_baseline=True`` and all installed packages are treated as added.
    """
    host_root = Path(host_root)
    section = RpmSection()

    # 1) rpm -qa
    if executor is not None:
        dbpath = detect_rpmdb_path(host_root)
        cmd_qa = ["rpm", "--dbpath", dbpath, "-qa", "--queryformat", RPM_QA_QUERYFORMAT + "\\n"]
        result_qa = executor(cmd_qa)
        used_root_fallback = False
        if result_qa.returncode != 0:
            cmd_qa = ["rpm", "--root", str(host_root)] + _RPM_LOCK_DEFINE + ["-qa", "--queryformat", RPM_QA_QUERYFORMAT + "\\n"]
            result_qa = executor(cmd_qa)
            used_root_fallback = True
        if used_root_fallback and result_qa.returncode == 0 and warnings is not None:
            warnings.append(make_warning(
                "rpm",
                "rpm -qa used --root fallback (--dbpath query failed); results are correct but may be slower.",
                "info",
            ))
        installed = [p for p in _parse_rpm_qa(result_qa.stdout, warnings=warnings)
                     if p.name not in _VIRTUAL_PACKAGES]
    else:
        installed = []

    # 1b) Detect multi-arch and duplicate packages on the full installed list
    if installed:
        section.multiarch_packages = _detect_multiarch(installed)
        section.duplicate_packages = _detect_duplicates(installed)
        multiarch_names = sorted({variant.rsplit(".", 1)[0] for variant in section.multiarch_packages})
        for name in multiarch_names:
            if warnings is not None:
                warnings.append(make_warning(
                    "rpm",
                    f"Package '{name}' is installed in multiple architectures — verify affected variants are needed.",
                    "warning",
                ))
        for key in section.duplicate_packages:
            if warnings is not None:
                warnings.append(make_warning(
                    "rpm",
                    f"Package '{key}' has multiple versions installed — possible upgrade inconsistency.",
                    "warning",
                ))

    # 2) Baseline from base image (or file, or fallback)
    baseline_packages: Optional[Dict[str, "PackageEntry"]] = None
    section.no_baseline = False

    if preflight_baseline is not None:
        baseline_set, base_image, no_baseline = preflight_baseline
        section.base_image = base_image
        if no_baseline:
            section.no_baseline = True
            baseline_packages = {}
        else:
            baseline_packages = baseline_set
    else:
        id_val, version_id = _read_os_id_version(host_root)
        if id_val and version_id:
            _resolver = resolver if resolver is not None else BaselineResolver(executor)
            if target_image:
                section.base_image = target_image
                if baseline_packages_file:
                    baseline_set = load_baseline_packages_file(baseline_packages_file)
                    no_baseline = not baseline_set
                elif _resolver._executor is not None:
                    baseline_set = _resolver.query_packages(target_image)
                    no_baseline = baseline_set is None
                else:
                    baseline_set, no_baseline = None, True
            else:
                baseline_set, base_image, no_baseline = _resolver.get_baseline_packages(
                    host_root, id_val, version_id,
                    baseline_packages_file=baseline_packages_file,
                    target_version=target_version,
                )
                section.base_image = base_image
            if no_baseline:
                section.no_baseline = True
                baseline_packages = {}
            else:
                baseline_packages = baseline_set
        else:
            section.no_baseline = True
            baseline_packages = {}

    if installed:
        installed_names = {p.name for p in installed}
        _debug(f"installed package count: {len(installed_names)}")
        _prereq_exclude: Set[str] = set()
        _prereq_raw = os.environ.get("YOINKC_EXCLUDE_PREREQS", "").split()
        if _prereq_raw:
            _prereq_exclude = set(_prereq_raw)
            _debug(f"YOINKC_EXCLUDE_PREREQS: will exclude tool prerequisites: {sorted(_prereq_exclude)}")
        if baseline_packages is not None and not section.no_baseline:
            baseline_name_set = {p.name for p in baseline_packages.values()}
            added_names = installed_names - baseline_name_set
            if _prereq_exclude:
                _excluded = added_names & _prereq_exclude
                if _excluded:
                    _debug(f"excluded tool prerequisites from added set: {sorted(_excluded)}")
                    added_names -= _excluded
            base_only_names = baseline_name_set - installed_names
            matched_names = installed_names & baseline_name_set
            _debug(f"baseline has {len(baseline_name_set)} names, "
                   f"installed has {len(installed_names)} names")
            _debug(f"matched={len(matched_names)}, "
                   f"added (installed-baseline, after prereq exclusion)={len(added_names)}, "
                   f"base-image-only (baseline-installed)={len(base_only_names)}")
            section.baseline_package_names = sorted(baseline_name_set)
            for p in installed:
                if p.name in added_names:
                    p.state = PackageState.ADDED
                    section.packages_added.append(p)

            # Populate base_image_only with full NEVRA from baseline when available
            baseline_by_name: dict = {}
            for bp in baseline_packages.values():
                if bp.name not in baseline_by_name:
                    baseline_by_name[bp.name] = bp
            for name in sorted(base_only_names):
                bio_pkg = baseline_by_name.get(name)
                if bio_pkg and bio_pkg.version:
                    section.base_image_only.append(
                        PackageEntry(name=bio_pkg.name, epoch=bio_pkg.epoch,
                                     version=bio_pkg.version, release=bio_pkg.release,
                                     arch=bio_pkg.arch, state=PackageState.BASE_IMAGE_ONLY)
                    )
                else:
                    section.base_image_only.append(
                        PackageEntry(name=name, epoch="0", version="", release="",
                                     arch="noarch", state=PackageState.BASE_IMAGE_ONLY)
                    )

            # Version comparison for matched packages (only with NEVRA baseline)
            _has_nevra = any(p.version for p in baseline_packages.values())
            if _has_nevra:
                from ..schema import VersionChange, VersionChangeDirection
                installed_by_key = {f"{p.name}.{p.arch}": p for p in installed}
                matched_keys = installed_by_key.keys() & baseline_packages.keys()
                for key in sorted(matched_keys):
                    host_pkg = installed_by_key[key]
                    base_pkg = baseline_packages[key]
                    cmp = _compare_evr(host_pkg, base_pkg)
                    if cmp != 0:
                        direction = (VersionChangeDirection.DOWNGRADE if cmp > 0
                                     else VersionChangeDirection.UPGRADE)
                        section.version_changes.append(VersionChange(
                            name=host_pkg.name,
                            arch=host_pkg.arch,
                            host_version=f"{host_pkg.version}-{host_pkg.release}",
                            base_version=f"{base_pkg.version}-{base_pkg.release}",
                            host_epoch=host_pkg.epoch,
                            base_epoch=base_pkg.epoch,
                            direction=direction,
                        ))
                if section.version_changes:
                    n_down = sum(1 for vc in section.version_changes
                                 if vc.direction == VersionChangeDirection.DOWNGRADE)
                    n_up = len(section.version_changes) - n_down
                    _debug(f"version changes: {n_down} downgrades, {n_up} upgrades")
                    section.version_changes.sort(
                        key=lambda vc: (0 if vc.direction == VersionChangeDirection.DOWNGRADE else 1, vc.name)
                    )
                    if n_down > 0 and warnings is not None:
                        warnings.append(make_warning(
                            "rpm",
                            f"{n_down} package(s) will be downgraded by the base image — "
                            "review the Version Changes section.",
                            "warning",
                        ))
        else:
            section.baseline_package_names = None
            for p in installed:
                if p.name not in _prereq_exclude:
                    p.state = PackageState.ADDED
                    section.packages_added.append(p)
            if _prereq_exclude:
                _skipped = [p.name for p in installed if p.name in _prereq_exclude]
                if _skipped:
                    _debug(f"(no-baseline) excluded tool prerequisites: {sorted(_skipped)}")

    # 2b) Source repo per added package
    if executor is not None and section.packages_added:
        _populate_source_repos(executor, host_root, section.packages_added)

    # 3) rpm -Va (rc != 0 is normal — it means files were modified)
    #    --root tells rpm where to verify files; --dbpath tells it where the
    #    database lives.  Both are needed when the container's rpm binary uses
    #    a different default dbpath than the host (Fedora uses
    #    /usr/lib/sysimage/rpm, RHEL 9 uses /var/lib/rpm).
    #    SKIP on ostree/bootc: rpm -Va floods false positives on immutable /usr.
    if system_type != SystemType.PACKAGE_MODE:
        _debug("skipping rpm -Va on ostree/bootc system (immutable /usr)")
        section.rpm_va = []
    elif executor is not None:
        if str(host_root) == "/":
            cmd_va = ["rpm", "-Va", "--nodeps", "--noscripts"]
        else:
            cmd_va = ["rpm", "--root", str(host_root), "--dbpath", detect_rpmdb_path(host_root, relative=True)] + _RPM_LOCK_DEFINE + ["-Va", "--nodeps", "--noscripts"]
        _debug(f"running: {' '.join(cmd_va)}")
        result_va = executor(cmd_va)
        if result_va.stderr and "cannot open Packages database" in result_va.stderr:
            _debug("rpm -Va --dbpath failed, retrying with --root only")
            cmd_va = ["rpm", "--root", str(host_root)] + _RPM_LOCK_DEFINE + ["-Va", "--nodeps", "--noscripts"]
            result_va = executor(cmd_va)
        _debug(f"rpm -Va: rc={result_va.returncode}, stdout={len(result_va.stdout)} bytes, stderr={result_va.stderr[:200] if result_va.stderr else ''}")
        section.rpm_va = _parse_rpm_va(result_va.stdout)
    else:
        section.rpm_va = []

    # 3b) rpm-ostree package state (layered, removed, overridden)
    if system_type != SystemType.PACKAGE_MODE and executor is not None:
        _parse_rpmostree_package_state(executor, section, warnings=warnings, system_type=system_type)

    # 4) Leaf/auto package classification
    if executor is not None and section.packages_added and not section.no_baseline:
        leaf, auto, dep_tree = _classify_leaf_auto(executor, host_root, section.packages_added)
        section.leaf_packages = leaf
        section.auto_packages = auto
        section.leaf_dep_tree = dep_tree
        _debug(f"leaf/auto split: {len(leaf)} leaf, {len(auto)} auto")

    # 5) Repo files
    section.repo_files = _collect_repo_files(host_root)
    section.gpg_keys = _collect_gpg_keys(host_root, section.repo_files)

    # 5a) DNF module streams
    section.module_streams = _collect_module_streams(host_root)
    _debug(f"module streams: {len(section.module_streams)} enabled")

    # 5a-compare) Module stream baseline comparison
    if section.module_streams and not section.no_baseline and section.base_image and executor is not None:
        _ms_resolver = resolver if resolver is not None else BaselineResolver(executor)
        baseline_streams = _ms_resolver.query_module_streams(section.base_image)
        section.baseline_module_streams = baseline_streams
        _apply_module_stream_baseline(
            section.module_streams,
            baseline_streams,
            section.module_stream_conflicts,
            warnings=warnings,
        )
        _debug(f"module stream baseline: {sum(1 for ms in section.module_streams if ms.baseline_match)} matched, "
               f"{len(section.module_stream_conflicts)} conflicts")

    # 5b) Version locks
    version_locks, vl_output = _collect_version_locks(executor, host_root)
    section.version_locks = version_locks
    section.versionlock_command_output = vl_output
    _debug(f"version locks: {len(version_locks)} pins")

    # 6) dnf history removed
    if executor is not None:
        section.dnf_history_removed = _dnf_history_removed(executor, host_root, warnings=warnings)
    else:
        section.dnf_history_removed = []

    return section
