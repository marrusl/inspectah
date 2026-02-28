"""
RPM inspector: package list, rpm -Va, repo files, dnf history removed.
Baseline is the target bootc base image package list (or --baseline-packages file).
"""

import os
import re
import sys
from pathlib import Path
from typing import List, Optional, Set

_DEBUG = bool(os.environ.get("YOINKC_DEBUG", ""))


def _debug(msg: str) -> None:
    if _DEBUG:
        print(f"[yoinkc] rpm: {msg}", file=sys.stderr)


from ..baseline import BaselineResolver
from ..executor import Executor
from ..schema import (
    PackageEntry,
    PackageState,
    RepoFile,
    RpmSection,
    RpmVaEntry,
)


RPM_QA_QUERYFORMAT = r"%{EPOCH}:%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}"

_RPM_LOCK_DEFINE = ["--define", "_rpmlock_path /var/tmp/.rpm.lock"]

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
            warnings.append({
                "source": "rpm",
                "message": f"rpm -qa: {len(failed)} package line(s) could not be parsed ({pct:.0f}% of output) — package list may be incomplete.",
                "severity": severity,
            })
    _debug(f"parsed {len(packages)} packages from rpm -qa "
           f"(first 5 names: {[p.name for p in packages[:5]]})")
    return packages


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
        if path:
            entries.append(RpmVaEntry(path=path, flags=flags, package=None))
    return entries


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
            if f.is_file() and (f.suffix in (".repo", ".conf") or subdir == "etc/dnf"):
                try:
                    content = f.read_text()
                except Exception:
                    content = ""
                repo_files.append(RepoFile(path=str(f.relative_to(host_root)), content=content))
    return repo_files


def _dnf_history_removed(executor: Executor, host_root: Path, warnings: Optional[list] = None) -> List[str]:
    """Run dnf history and collect package names from Remove transactions."""
    result = executor(["dnf", "history", "list", "-q"], cwd=str(host_root))
    if result.returncode != 0:
        if warnings is not None:
            warnings.append({
                "source": "rpm",
                "message": "dnf history unavailable — orphaned config detection (packages removed after install) is incomplete.",
                "severity": "warning",
            })
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


def run(
    host_root: Path,
    executor: Optional[Executor],
    baseline_packages_file: Optional[Path] = None,
    warnings: Optional[list] = None,
    resolver: Optional[BaselineResolver] = None,
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
        dbpath = str(host_root / "var" / "lib" / "rpm")
        cmd_qa = ["rpm", "--dbpath", dbpath, "-qa", "--queryformat", RPM_QA_QUERYFORMAT + "\\n"]
        result_qa = executor(cmd_qa)
        used_root_fallback = False
        if result_qa.returncode != 0:
            cmd_qa = ["rpm", "--root", str(host_root)] + _RPM_LOCK_DEFINE + ["-qa", "--queryformat", RPM_QA_QUERYFORMAT + "\\n"]
            result_qa = executor(cmd_qa)
            used_root_fallback = True
        if used_root_fallback and result_qa.returncode == 0 and warnings is not None:
            warnings.append({
                "source": "rpm",
                "message": "rpm -qa used --root fallback (--dbpath query failed); results are correct but may be slower.",
                "severity": "info",
            })
        installed = [p for p in _parse_rpm_qa(result_qa.stdout, warnings=warnings)
                     if p.name not in _VIRTUAL_PACKAGES]
    else:
        installed = []

    # 2) Baseline from base image (or file, or fallback)
    id_val, version_id = _read_os_id_version(host_root)
    baseline_names: Optional[Set[str]] = None
    section.no_baseline = False

    if id_val and version_id:
        # Use the shared resolver if provided, else create a transient one
        _resolver = resolver if resolver is not None else BaselineResolver(executor)
        baseline_set, base_image, no_baseline = _resolver.get_baseline_packages(
            host_root, id_val, version_id,
            baseline_packages_file=baseline_packages_file,
        )
        section.base_image = base_image
        if no_baseline:
            section.no_baseline = True
            baseline_names = set()
        else:
            baseline_names = baseline_set
    else:
        section.no_baseline = True
        baseline_names = set()

    if installed:
        installed_names = {p.name for p in installed}
        _debug(f"installed package count: {len(installed_names)}")
        if baseline_names is not None and not section.no_baseline:
            added_names = installed_names - baseline_names
            removed_names = baseline_names - installed_names
            matched_names = installed_names & baseline_names
            _debug(f"baseline has {len(baseline_names)} names, "
                   f"installed has {len(installed_names)} names")
            _debug(f"matched={len(matched_names)}, "
                   f"added (installed-baseline)={len(added_names)}, "
                   f"removed (baseline-installed)={len(removed_names)}")
            section.baseline_package_names = sorted(baseline_names)
            for p in installed:
                if p.name in added_names:
                    p.state = PackageState.ADDED
                    section.packages_added.append(p)
            for name in sorted(removed_names):
                section.packages_removed.append(
                    PackageEntry(name=name, epoch="0", version="", release="", arch="noarch", state=PackageState.REMOVED)
                )
        else:
            section.baseline_package_names = None
            for p in installed:
                p.state = PackageState.ADDED
                section.packages_added.append(p)

    # 3) rpm -Va
    if executor is not None:
        cmd_va = ["rpm", "-Va", "--nodeps", "--noscripts"]
        if str(host_root) != "/":
            cmd_va = ["rpm", "--root", str(host_root)] + _RPM_LOCK_DEFINE + ["-Va", "--nodeps", "--noscripts"]
        result_va = executor(cmd_va)
        section.rpm_va = _parse_rpm_va(result_va.stdout)
    else:
        section.rpm_va = []

    # 4) Repo files
    section.repo_files = _collect_repo_files(host_root)

    # 5) dnf history removed
    if executor is not None:
        section.dnf_history_removed = _dnf_history_removed(executor, host_root, warnings=warnings)
    else:
        section.dnf_history_removed = []

    return section
