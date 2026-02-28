"""
Inspectors produce structured data that is merged into the inspection snapshot.
Each inspector receives host_root and an executor; returns a section for the snapshot.
"""

import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Callable, List, Optional, TypeVar

from ..executor import Executor, make_executor
from ..schema import InspectionSnapshot, OsRelease

T = TypeVar("T")

# Directories whose entire subtrees should be pruned when doing recursive
# scans under /home, /opt, /srv.  Presence of any of these as an immediate
# child of a directory causes the scanner to skip that directory and everything
# below it — this prevents source-code checkouts, build trees, and IDE
# metadata from appearing as operator-deployed software.
_PRUNE_MARKERS = frozenset({".git", ".svn", ".hg"})

# Additional directory names that are always skipped (never descended into).
_SKIP_DIR_NAMES = frozenset({
    "__pycache__", ".mypy_cache", ".pytest_cache", ".tox", ".nox",
    "node_modules", ".eggs",
    ".vscode", ".idea", ".cursor",
})


def is_dev_artifact(path: Path) -> bool:
    """Return True if any component of *path* sits inside a dev/build directory."""
    for part in path.parts:
        if part in _SKIP_DIR_NAMES or part in _PRUNE_MARKERS:
            return True
    return False


def filtered_rglob(root: Path, pattern: str) -> List[Path]:
    """Like Path.rglob but prunes source-code checkouts and build dirs.

    A directory is pruned (not descended into) when it:
      - contains a VCS marker (.git, .svn, .hg), or
      - has a name in the skip list (node_modules, __pycache__, …).

    Only files matching *pattern* (a simple glob like ``*.yml``) from
    non-pruned subtrees are yielded.
    """
    import fnmatch
    results: List[Path] = []

    def _walk(d: Path) -> None:
        try:
            entries = sorted(d.iterdir())
        except (PermissionError, OSError):
            return

        child_names = {e.name for e in entries}
        if child_names & _PRUNE_MARKERS:
            return

        for entry in entries:
            if entry.is_file() and fnmatch.fnmatch(entry.name, pattern):
                results.append(entry)
            elif entry.is_dir() and entry.name not in _SKIP_DIR_NAMES:
                _walk(entry)

    _walk(root)
    return results


def _safe_run(name: str, fn: Callable[[], T], default: T, warnings: list) -> T:
    """Run an inspector; on PermissionError/OSError log a warning and return *default*."""
    try:
        return fn()
    except (PermissionError, OSError) as exc:
        warnings.append({
            "source": name,
            "message": f"{name} inspector: {exc}",
            "severity": "warning",
        })
        print(f"WARNING: {name} inspector skipped: {exc}", file=sys.stderr)
        return default

from .rpm import run as run_rpm
from .config import run as run_config
from .service import run as run_service
from .network import run as run_network
from .storage import run as run_storage
from .scheduled_tasks import run as run_scheduled_tasks
from .container import run as run_container
from .non_rpm_software import run as run_non_rpm_software
from .kernel_boot import run as run_kernel_boot
from .selinux import run as run_selinux
from .users_groups import run as run_users_groups


def _read_os_release(host_root: Path) -> Optional[OsRelease]:
    p = host_root / "etc" / "os-release"
    if not p.exists():
        return None
    data = {}
    for line in p.read_text().splitlines():
        if "=" in line:
            k, v = line.split("=", 1)
            data[k] = v.strip().strip('"')
    return OsRelease(
        name=data.get("NAME", ""),
        version_id=data.get("VERSION_ID", ""),
        version=data.get("VERSION", ""),
        id=data.get("ID", ""),
        id_like=data.get("ID_LIKE", ""),
        pretty_name=data.get("PRETTY_NAME", ""),
    )


def _validate_supported_host(os_release: Optional[OsRelease]) -> Optional[str]:
    """Return error message if host is not supported, else None."""
    if not os_release or not os_release.version_id:
        return None
    vid = os_release.version_id
    if os_release.id == "rhel":
        if not vid.startswith("9."):
            return (
                f"Host is running RHEL {vid}. This version of yoinkc only supports "
                "RHEL 9.x and CentOS Stream 9."
            )
    elif "centos" in os_release.id.lower():
        if vid != "9":
            return (
                f"Host is running CentOS {vid}. This version of yoinkc only supports "
                "CentOS Stream 9."
            )
    else:
        return None
    return None




def run_all(
    host_root: Path,
    executor: Optional[Executor] = None,
    config_diffs: bool = False,
    deep_binary_scan: bool = False,
    query_podman: bool = False,
    baseline_packages_file: Optional[Path] = None,
) -> InspectionSnapshot:
    """Run all inspectors and return a merged snapshot."""
    host_root = Path(host_root)
    if executor is None:
        executor = make_executor(str(host_root))

    meta = {"host_root": str(host_root), "timestamp": datetime.now(timezone.utc).isoformat()}
    hostname_path = host_root / "etc" / "hostname"
    try:
        if hostname_path.exists():
            lines = hostname_path.read_text().strip().splitlines()
            if lines:
                meta["hostname"] = lines[0]
    except (PermissionError, OSError):
        pass
    os_release = _read_os_release(host_root)
    err = _validate_supported_host(os_release)
    if err:
        raise ValueError(err)
    snapshot = InspectionSnapshot(
        meta=meta,
        os_release=os_release,
    )

    # Create one BaselineResolver per run — shares the nsenter probe cache
    # across the package query (rpm inspector) and the presets query (service baseline).
    from ..baseline import BaselineResolver
    resolver = BaselineResolver(executor)

    w = snapshot.warnings
    snapshot.rpm = _safe_run("rpm", lambda: run_rpm(host_root, executor, baseline_packages_file=baseline_packages_file, warnings=w, resolver=resolver), None, w)
    if snapshot.rpm and snapshot.rpm.no_baseline:
        w.append({
            "source": "rpm",
            "message": (
                "Could not query base image package list. "
                "No baseline available — all installed packages will be included in the Containerfile. "
                "Common fixes: (1) run with 'sudo podman run …' (rootless containers cannot "
                "nsenter the host), (2) ensure --pid=host is set, or (3) provide a package "
                "list via --baseline-packages FILE."
            ),
            "severity": "warning",
        })
    snapshot.config = _safe_run("config", lambda: run_config(host_root, executor, rpm_section=snapshot.rpm, rpm_owned_paths_override=None, config_diffs=config_diffs, warnings=w), None, w)

    # Query base image for systemd presets (service baseline) — reuses the same
    # resolver so the nsenter probe is not repeated.
    base_image_preset_text = None
    if snapshot.rpm and snapshot.rpm.base_image and executor is not None:
        base_image_preset_text = resolver.query_presets(snapshot.rpm.base_image)
    snapshot.services = _safe_run("service", lambda: run_service(host_root, executor, base_image_preset_text=base_image_preset_text, warnings=w), None, w)
    snapshot.network = _safe_run("network", lambda: run_network(host_root, executor, warnings=w), None, w)
    snapshot.storage = _safe_run("storage", lambda: run_storage(host_root, executor), None, w)
    snapshot.scheduled_tasks = _safe_run("scheduled_tasks", lambda: run_scheduled_tasks(host_root, executor), None, w)
    snapshot.containers = _safe_run("containers", lambda: run_container(host_root, executor, query_podman=query_podman, warnings=w), None, w)
    snapshot.non_rpm_software = _safe_run("non_rpm_software", lambda: run_non_rpm_software(host_root, executor, deep_binary_scan=deep_binary_scan, warnings=w), None, w)
    snapshot.kernel_boot = _safe_run("kernel_boot", lambda: run_kernel_boot(host_root, executor, warnings=w), None, w)
    snapshot.selinux = _safe_run("selinux", lambda: run_selinux(host_root, executor, warnings=w), None, w)
    snapshot.users_groups = _safe_run("users_groups", lambda: run_users_groups(host_root, executor), None, w)

    return snapshot
