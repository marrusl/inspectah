"""
Inspectors produce structured data that is merged into the inspection snapshot.
Each inspector receives host_root and an executor; returns a section for the snapshot.
"""

import os
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Callable, List, Optional, TypeVar

from ..executor import Executor, make_executor
from ..schema import InspectionSnapshot, OsRelease, SystemType
from ..system_type import detect_system_type, map_ostree_base_image, OstreeDetectionError
from .._util import make_warning, section_banner as _section_banner, status as _status_fn

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


def is_dev_artifact(path: Path, host_root: Optional[Path] = None) -> bool:
    """Return True if any component of *path* sits inside a dev/build directory.

    When *host_root* is provided, only the path components relative to
    host_root are checked — this prevents false positives from the
    workspace or container mount path (e.g. ``.cursor`` in a worktree
    path like ``/home/user/.cursor/worktrees/...``).
    """
    if host_root is not None:
        try:
            rel = path.relative_to(host_root)
            parts = rel.parts
        except ValueError:
            parts = path.parts
    else:
        parts = path.parts
    for part in parts:
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
        warnings.append(make_warning(name, f"{name} inspector: {exc}"))
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
        variant_id=data.get("VARIANT_ID", ""),
    )


_SUPPORTED_RHEL_MAJORS = {"9", "10"}
_SUPPORTED_CENTOS_MAJORS = {"9", "10"}


def _validate_supported_host(os_release: Optional[OsRelease]) -> Optional[str]:
    """Return error message if host is not supported, else None."""
    if not os_release or not os_release.version_id:
        return None
    vid = os_release.version_id
    major = vid.split(".")[0]
    os_id = os_release.id.lower()
    supported_list = (
        ", ".join(f"RHEL {m}.x" for m in sorted(_SUPPORTED_RHEL_MAJORS))
        + ", "
        + ", ".join(f"CentOS Stream {m}" for m in sorted(_SUPPORTED_CENTOS_MAJORS))
        + ", and Fedora"
    )
    if os_id == "rhel":
        if major not in _SUPPORTED_RHEL_MAJORS:
            return (
                f"Host is running RHEL {vid}. This version of yoinkc supports "
                f"{supported_list}."
            )
    elif "centos" in os_id:
        if major not in _SUPPORTED_CENTOS_MAJORS:
            return (
                f"Host is running CentOS {vid}. This version of yoinkc supports "
                f"{supported_list}."
            )
    return None




def _baseline_fail_fast(base_image: Optional[str]) -> None:
    """Print a clear error about missing baseline and exit."""
    lines = [
        "ERROR: Could not query the base image package list.",
        "",
        "Without a baseline, the generated Containerfile would include every",
        "installed package — not just the ones added by the operator. This is",
        "almost certainly not what you want.",
        "",
        "To fix this, try one of:",
    ]
    step = 1
    if base_image and "registry.redhat.io" in base_image:
        lines.append(f"  {step}. Log in to the registry on the host:")
        lines.append("       sudo podman login registry.redhat.io")
        step += 1
    lines.append(f"  {step}. Ensure the container has host access:")
    lines.append("       --pid=host and --privileged")
    step += 1
    lines.append(f"  {step}. Provide a pre-exported package list (for air-gapped environments):")
    lines.append("       --baseline-packages FILE")
    step += 1
    lines.append(f"  {step}. Explicitly opt in to degraded all-packages mode (not recommended):")
    lines.append("       --no-baseline")
    print("\n".join(lines), file=sys.stderr)
    sys.exit(1)


_COMMON_BASES = [
    "quay.io/fedora-ostree-desktops/silverblue:41",
    "quay.io/fedora-ostree-desktops/kinoite:41",
    "quay.io/fedora/fedora-bootc:41",
    "quay.io/centos-bootc/centos-bootc:stream10",
]


def _ostree_unknown_base_fail(system_type: "SystemType", os_release: Optional[OsRelease]) -> None:
    """Print refusal message for unmappable ostree system and exit."""
    type_label = "bootc" if system_type == SystemType.BOOTC else "rpm-ostree"
    identity = (os_release.pretty_name or os_release.id) if os_release else "unknown"
    lines = [
        f"Detected {type_label} system: {identity}",
        "Could not map to a known bootc base image.",
        "",
        "Specify one with: yoinkc --target-image <registry/image:tag>",
        "",
        "Common bases:",
    ]
    for base in _COMMON_BASES:
        lines.append(f"  {base}")
    print("\n".join(lines), file=sys.stderr)
    sys.exit(1)


def run_all(
    host_root: Path,
    executor: Optional[Executor] = None,
    config_diffs: bool = False,
    deep_binary_scan: bool = False,
    query_podman: bool = False,
    baseline_packages_file: Optional[Path] = None,
    target_version: Optional[str] = None,
    target_image: Optional[str] = None,
    user_strategy: Optional[str] = None,
    no_baseline_opt_in: bool = False,
) -> InspectionSnapshot:
    """Run all inspectors and return a merged snapshot."""
    host_root = Path(host_root)
    if executor is None:
        executor = make_executor(str(host_root))

    meta = {"host_root": str(host_root), "timestamp": datetime.now(timezone.utc).isoformat()}
    # Hostname priority: YOINKC_HOSTNAME env var (set by wrapper on host)
    #                  → /etc/hostname
    #                  → hostnamectl hostname
    env_hostname = os.environ.get("YOINKC_HOSTNAME", "").strip()
    if env_hostname:
        meta["hostname"] = env_hostname
    else:
        hostname_path = host_root / "etc" / "hostname"
        name = ""
        try:
            if hostname_path.exists():
                lines = hostname_path.read_text().splitlines()
                if lines:
                    name = lines[0].strip()
        except (PermissionError, OSError):
            pass
        if not name:
            try:
                result = executor(["hostnamectl", "hostname"])
            except OSError:
                result = None
            if result and result.returncode == 0 and result.stdout.strip():
                name = result.stdout.strip().splitlines()[0].strip()
        if name:
            meta["hostname"] = name
    os_release = _read_os_release(host_root)
    err = _validate_supported_host(os_release)
    if err:
        raise ValueError(err)
    # -- System type detection --
    try:
        system_type = detect_system_type(host_root, executor)
    except OstreeDetectionError as exc:
        print(str(exc), file=sys.stderr)
        sys.exit(1)

    snapshot = InspectionSnapshot(
        meta=meta,
        os_release=os_release,
        system_type=system_type,
    )

    w = snapshot.warnings

    # Gate message for ostree systems
    if system_type != SystemType.PACKAGE_MODE:
        type_label = "bootc" if system_type == SystemType.BOOTC else "rpm-ostree"
        print(f"Detected {type_label} system, adapting inspection", file=sys.stderr)

    # Ostree base image mapping
    if system_type != SystemType.PACKAGE_MODE:
        mapped_image = map_ostree_base_image(
            host_root, os_release, system_type,
            executor=executor, target_image_override=target_image,
        )
        if mapped_image is None:
            if not no_baseline_opt_in:
                _ostree_unknown_base_fail(system_type, os_release)
            else:
                w.append(make_warning(
                    "pipeline",
                    "Could not map ostree system to a known base image. "
                    "Running without baseline.",
                ))
        else:
            target_image = mapped_image

    # Cross-major-version warning
    if target_version and os_release and os_release.version_id:
        source_major = os_release.version_id.split(".")[0]
        target_major = target_version.split(".")[0]
        if source_major != target_major:
            msg = (
                f"Source host is {os_release.id.upper()} {os_release.version_id} "
                f"but target image is version {target_version}. "
                "Cross-major-version migration may require significant manual adjustment. "
                "Package names, service names, and config formats may have changed."
            )
            print(f"WARNING: {msg}", file=sys.stderr)
            w.append(make_warning("pipeline", msg, "error"))

    # Create one BaselineResolver per run — shares the nsenter probe cache
    # across the package query (rpm inspector) and the presets query (service baseline).
    from ..baseline import BaselineResolver
    resolver = BaselineResolver(executor)

    # Preflight: resolve baseline before inspectors start so the user gets a
    # clear error in seconds rather than after a long inspection run.
    preflight_baseline = None
    host_os_id = os_release.id if os_release else ""
    host_version_id = os_release.version_id if os_release else ""
    if host_os_id and host_version_id:
        preflight_baseline = resolver.resolve(
            host_root, host_os_id, host_version_id,
            baseline_packages_file=baseline_packages_file,
            target_version=target_version,
            target_image=target_image,
        )
        _, resolved_image, no_baseline = preflight_baseline
        if no_baseline:
            if not no_baseline_opt_in:
                _baseline_fail_fast(resolved_image)
            w.append(make_warning(
                "rpm",
                "Running without baseline (--no-baseline). All installed packages "
                "will be included in the Containerfile.",
            ))

    _TOTAL_STEPS = 11
    _status_fn("Starting inspection…")

    _section_banner("Packages", 1, _TOTAL_STEPS)
    def _run_rpm_inspector():
        return run_rpm(
            host_root, executor,
            baseline_packages_file=baseline_packages_file,
            warnings=w, resolver=resolver,
            target_version=target_version,
            target_image=target_image,
            preflight_baseline=preflight_baseline,
            system_type=system_type,
        )
    snapshot.rpm = _safe_run("rpm", _run_rpm_inspector, None, w)

    # Post-inspector fallback: if the preflight was skipped (e.g. os-release
    # missing or incomplete) but the RPM inspector still ended up without a
    # baseline, apply the same fail-fast / warn logic.
    if preflight_baseline is None and snapshot.rpm and snapshot.rpm.no_baseline:
        if not no_baseline_opt_in:
            _baseline_fail_fast(None)
        w.append(make_warning(
            "rpm",
            "Running without baseline (--no-baseline). All installed packages "
            "will be included in the Containerfile.",
        ))

    # Build RPM-owned path set once; shared by config and scheduled_tasks inspectors
    # to avoid issuing two separate rpm -qa queries.
    from .config import _rpm_owned_paths as _build_rpm_owned_paths
    rpm_owned = _build_rpm_owned_paths(executor, host_root, warnings=w)

    _section_banner("Config files", 2, _TOTAL_STEPS)
    snapshot.config = _safe_run("config", lambda: run_config(host_root, executor, rpm_section=snapshot.rpm, rpm_owned_paths_override=rpm_owned, config_diffs=config_diffs, warnings=w, system_type=system_type), None, w)

    _section_banner("Services", 3, _TOTAL_STEPS)
    base_image_preset_text = None
    if snapshot.rpm and snapshot.rpm.base_image and executor is not None:
        base_image_preset_text = resolver.query_presets(snapshot.rpm.base_image)
    snapshot.services = _safe_run("service", lambda: run_service(host_root, executor, base_image_preset_text=base_image_preset_text, warnings=w), None, w)

    _section_banner("Network", 4, _TOTAL_STEPS)
    snapshot.network = _safe_run("network", lambda: run_network(host_root, executor, warnings=w), None, w)

    _section_banner("Storage", 5, _TOTAL_STEPS)
    snapshot.storage = _safe_run("storage", lambda: run_storage(host_root, executor), None, w)

    _section_banner("Scheduled tasks", 6, _TOTAL_STEPS)
    snapshot.scheduled_tasks = _safe_run("scheduled_tasks", lambda: run_scheduled_tasks(host_root, executor, rpm_owned_paths=rpm_owned), None, w)

    _section_banner("Containers", 7, _TOTAL_STEPS)
    snapshot.containers = _safe_run("containers", lambda: run_container(host_root, executor, query_podman=query_podman, warnings=w), None, w)

    _section_banner("Non-RPM software", 8, _TOTAL_STEPS)
    snapshot.non_rpm_software = _safe_run("non_rpm_software", lambda: run_non_rpm_software(host_root, executor, deep_binary_scan=deep_binary_scan, warnings=w), None, w)

    _section_banner("Kernel / boot", 9, _TOTAL_STEPS)
    snapshot.kernel_boot = _safe_run("kernel_boot", lambda: run_kernel_boot(host_root, executor, warnings=w), None, w)

    _section_banner("SELinux / security", 10, _TOTAL_STEPS)
    snapshot.selinux = _safe_run("selinux", lambda: run_selinux(host_root, executor, warnings=w, rpm_owned_paths=rpm_owned), None, w)

    _section_banner("Users / groups", 11, _TOTAL_STEPS)
    snapshot.users_groups = _safe_run("users_groups", lambda: run_users_groups(host_root, executor, user_strategy_override=user_strategy), None, w)

    _status_fn("Inspection complete.")

    return snapshot
