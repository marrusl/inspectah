"""Source system type detection and ostree base image mapping."""

from pathlib import Path

from .executor import Executor
from .schema import SystemType
from ._util import debug as _debug_fn


def _debug(msg: str) -> None:
    _debug_fn("system_type", msg)


class OstreeDetectionError(Exception):
    """Raised when an ostree system cannot be classified."""
    pass


def detect_system_type(host_root: Path, executor: Executor) -> SystemType:
    """Detect whether the source system is package-mode, rpm-ostree, or bootc.

    Detection order per spec:
    1. No /ostree -> package-mode
    2. /ostree + bootc status succeeds -> bootc
    3. /ostree + rpm-ostree status succeeds -> rpm-ostree
    4. /ostree + both fail -> OstreeDetectionError (never fall back to package-mode)
    """
    ostree_dir = host_root / "ostree"
    if not ostree_dir.exists():
        return SystemType.PACKAGE_MODE

    result = executor(["bootc", "status"])
    if result.returncode == 0:
        return SystemType.BOOTC

    result = executor(["rpm-ostree", "status"])
    if result.returncode == 0:
        return SystemType.RPM_OSTREE

    raise OstreeDetectionError(
        "Detected ostree system (/ostree exists) but could not determine\n"
        "system type -- both 'bootc status' and 'rpm-ostree status' failed.\n"
        "\n"
        "This system may use an ostree configuration yoinkc does not yet support."
    )
