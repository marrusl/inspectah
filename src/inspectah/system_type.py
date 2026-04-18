"""Source system type detection and ostree base image mapping."""

import json
from pathlib import Path
from typing import Optional

from .executor import Executor
from .schema import OsRelease, SystemType
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


# =====================================================================
# ostree / bootc base image mapping
# =====================================================================

_FEDORA_OSTREE_DESKTOPS = {
    "silverblue",
    "kinoite",
    "sway-atomic",
    "budgie-atomic",
    "lxqt-atomic",
    "xfce-atomic",
    "cosmic-atomic",
}


_UBLUE_NOT_PRESENT = "not_present"


def _read_ublue_image_info(host_root: Path) -> str | None:
    """Read Universal Blue image-info.json and return the image ref.

    Returns:
        - The image ref string on success
        - None if the file is present but malformed (caller should refuse)
        - _UBLUE_NOT_PRESENT sentinel if the file doesn't exist (caller falls through)
    """
    info_path = host_root / "usr" / "share" / "ublue-os" / "image-info.json"
    if not info_path.exists():
        return _UBLUE_NOT_PRESENT

    try:
        data = json.loads(info_path.read_text())
    except (json.JSONDecodeError, OSError) as exc:
        _debug(f"UBlue image-info.json unreadable: {exc}")
        return None

    # Require image-name and image-vendor for validity
    if "image-name" not in data or "image-vendor" not in data:
        _debug("UBlue image-info.json missing required fields (image-name, image-vendor)")
        return None

    ref = data.get("image-ref")
    if ref:
        _debug(f"UBlue detected: {ref}")
        return ref

    # Synthesis fallback: construct from vendor/name/tag
    vendor = data.get("image-vendor", "")
    name = data.get("image-name", "")
    tag = data.get("image-tag", "")
    if vendor and name and tag:
        synthesized = f"ghcr.io/{vendor}/{name}:{tag}"
        _debug(f"UBlue: synthesized ref from vendor/name/tag: {synthesized}")
        return synthesized

    _debug("UBlue image-info.json has no image-ref and insufficient fields for synthesis")
    return None


def _bootc_status_image_ref(executor: Executor) -> Optional[str]:
    """Parse ``bootc status --json`` for the booted image ref."""
    try:
        result = executor(["bootc", "status", "--json"])
    except Exception as exc:
        _debug(f"bootc status --json failed: {exc}")
        return None

    if result.returncode != 0:
        _debug(f"bootc status --json exited {result.returncode}")
        return None

    try:
        data = json.loads(result.stdout)
        ref = data["status"]["booted"]["image"]["image"]["image"]
        _debug(f"bootc status image ref: {ref}")
        return ref
    except (json.JSONDecodeError, KeyError, TypeError) as exc:
        _debug(f"Failed to parse bootc status JSON: {exc}")
        return None


def _map_bootc_from_os_release(os_release: OsRelease) -> Optional[str]:
    """Fallback bootc image mapping from os-release fields."""
    os_id = os_release.id
    ver = os_release.version_id

    if os_id == "fedora":
        return f"quay.io/fedora/fedora-bootc:{ver}"
    if os_id == "centos":
        major = ver.split(".")[0]
        return f"quay.io/centos-bootc/centos-bootc:stream{major}"
    if os_id == "rhel":
        major = ver.split(".")[0]
        return f"registry.redhat.io/rhel{major}/rhel-bootc:{ver}"

    _debug(f"Unknown bootc OS id={os_id!r}, cannot map base image")
    return None


def map_ostree_base_image(
    host_root: Path,
    os_release: OsRelease,
    system_type: SystemType,
    *,
    executor: Optional[Executor] = None,
    target_image_override: Optional[str] = None,
) -> Optional[str]:
    """Map an ostree/bootc source system to its container base image ref.

    Returns the base image reference string, or None if the system is
    unknown (caller should handle refusal).

    Resolution order:
    1. ``target_image_override`` (from --target-image CLI flag)
    2. Universal Blue image-info.json
    3. System-type-specific mapping (rpm-ostree variant or bootc status/os-release)
    """
    if target_image_override:
        _debug(f"Using target image override: {target_image_override}")
        return target_image_override

    # Universal Blue check (applies to both rpm-ostree and bootc UBlue systems)
    ublue_ref = _read_ublue_image_info(host_root)
    if ublue_ref is _UBLUE_NOT_PRESENT:
        pass  # Not a UBlue system, fall through to standard mapping
    elif ublue_ref is not None:
        return ublue_ref
    else:
        # UBlue file exists but is malformed — refuse rather than guess
        return None

    if system_type == SystemType.RPM_OSTREE:
        variant = os_release.variant_id
        if variant in _FEDORA_OSTREE_DESKTOPS:
            ref = f"quay.io/fedora-ostree-desktops/{variant}:{os_release.version_id}"
            _debug(f"rpm-ostree variant mapping: {ref}")
            return ref
        _debug(f"Unknown rpm-ostree variant_id={variant!r}")
        return None

    if system_type == SystemType.BOOTC:
        # Try bootc status --json first
        if executor is not None:
            ref = _bootc_status_image_ref(executor)
            if ref is not None:
                return ref
        # Fall back to os-release mapping
        return _map_bootc_from_os_release(os_release)

    _debug(f"map_ostree_base_image called with unexpected system_type={system_type}")
    return None
