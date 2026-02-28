"""
Service inspector: systemd unit state vs baseline (enabled/disabled/masked).
Baseline is derived from systemd preset files on the host, not static manifests.
"""

import fnmatch
import os
import sys
from pathlib import Path
from typing import Dict, List, Optional, Set, Tuple

from ..executor import Executor
from ..schema import ServiceSection, ServiceStateChange

_DEBUG = bool(os.environ.get("YOINKC_DEBUG", ""))


def _debug(msg: str) -> None:
    if _DEBUG:
        print(f"[yoinkc] service: {msg}", file=sys.stderr)


def _parse_preset_lines(lines: List[str]) -> Tuple[Set[str], Set[str], bool]:
    """Parse preset content lines into (enabled, disabled, has_disable_all)."""
    default_enabled: Set[str] = set()
    default_disabled: Set[str] = set()
    already_matched: Set[str] = set()
    has_disable_all = False
    glob_rules: List[Tuple[str, str]] = []

    for raw in lines:
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        parts = line.split()
        if len(parts) < 2:
            continue
        action = parts[0].lower()
        pattern = parts[1]

        if "*" in pattern or "?" in pattern:
            if pattern == "*" and action == "disable":
                has_disable_all = True
            glob_rules.append((action, pattern))
            continue

        if pattern in already_matched:
            continue
        already_matched.add(pattern)

        if action == "enable":
            default_enabled.add(pattern)
        elif action == "disable":
            default_disabled.add(pattern)

    _debug(f"presets: {len(default_enabled)} explicit enable, "
           f"{len(default_disabled)} explicit disable, "
           f"disable_all={has_disable_all}, {len(glob_rules)} glob rules")
    return default_enabled, default_disabled, has_disable_all


def _parse_preset_files(
    host_root: Path,
    base_image_preset_text: Optional[str] = None,
) -> Tuple[Set[str], Set[str], bool]:
    """Parse systemd preset files to determine default-enabled and default-disabled services.

    If *base_image_preset_text* is provided (from querying the base image), it is
    used as the authoritative source.  Otherwise falls back to reading the host's
    own preset files from /usr/lib/systemd/system-preset/ and /etc/systemd/system-preset/.
    """
    if base_image_preset_text:
        _debug("using base image preset data for service defaults")
        return _parse_preset_lines(base_image_preset_text.splitlines())

    preset_dirs = [
        host_root / "etc/systemd/system-preset",
        host_root / "usr/lib/systemd/system-preset",
    ]
    all_lines: List[str] = []
    file_count = 0
    for d in preset_dirs:
        try:
            if not d.exists():
                _debug(f"preset dir not found: {d}")
                continue
            entries = sorted(d.iterdir())
        except (PermissionError, OSError) as exc:
            _debug(f"preset dir not accessible: {d}: {exc}")
            entries = []
        for f in entries:
            if f.is_file() and f.suffix == ".preset":
                try:
                    all_lines.extend(f.read_text().splitlines())
                    file_count += 1
                except Exception:
                    continue

    _debug(f"found {file_count} preset files")
    return _parse_preset_lines(all_lines)


def _parse_systemctl_list_unit_files(stdout: str) -> Dict[str, str]:
    """Parse output of systemctl list-unit-files. Returns unit -> state."""
    units = {}
    for line in stdout.strip().splitlines():
        parts = line.split()
        if len(parts) >= 2:
            unit = parts[0]
            state = parts[1]
            units[unit] = state
    return units


def _scan_unit_files_from_fs(host_root: Path) -> Dict[str, str]:
    """Determine unit file states by scanning the filesystem directly.

    This is the fallback when systemctl --root is unavailable or broken.
    Scans /usr/lib/systemd/system/ for vendor units and /etc/systemd/system/
    for local overrides/enables/masks.

    Returns unit_name -> state (enabled, disabled, masked, static).
    """
    vendor_dir = host_root / "usr/lib/systemd/system"
    admin_dir = host_root / "etc/systemd/system"
    units: Dict[str, str] = {}

    enabled_units: Set[str] = set()
    masked_units: Set[str] = set()

    # Scan .wants/ directories under /etc/systemd/system/ for enabled units
    try:
        if admin_dir.exists():
            for entry in admin_dir.iterdir():
                if entry.is_dir() and entry.name.endswith(".wants"):
                    try:
                        for link in entry.iterdir():
                            if link.is_symlink() or link.is_file():
                                enabled_units.add(link.name)
                    except (PermissionError, OSError):
                        continue
                # Direct symlink to /dev/null = masked
                if entry.is_symlink():
                    try:
                        target = os.readlink(str(entry))
                        if target == "/dev/null":
                            masked_units.add(entry.name)
                    except (OSError, ValueError):
                        pass
    except (PermissionError, OSError) as exc:
        _debug(f"cannot scan admin dir {admin_dir}: {exc}")

    _debug(f"fs scan: {len(enabled_units)} enabled via .wants/, {len(masked_units)} masked")

    # Collect all vendor unit files
    vendor_units: Set[str] = set()
    try:
        if vendor_dir.exists():
            for f in vendor_dir.iterdir():
                if f.name.endswith((".service", ".timer")) and (f.is_file() or f.is_symlink()):
                    vendor_units.add(f.name)
    except (PermissionError, OSError) as exc:
        _debug(f"cannot scan vendor dir {vendor_dir}: {exc}")

    _debug(f"fs scan: {len(vendor_units)} vendor unit files")

    # Determine state for each unit
    all_known = vendor_units | enabled_units | masked_units
    for unit in sorted(all_known):
        if not unit.endswith((".service", ".timer")):
            continue
        if unit in masked_units:
            units[unit] = "masked"
        elif unit in enabled_units:
            units[unit] = "enabled"
        else:
            # Check if unit has [Install] section (static vs disabled)
            unit_path = vendor_dir / unit
            has_install = False
            try:
                if unit_path.exists():
                    text = unit_path.read_text()
                    has_install = "[Install]" in text
            except (PermissionError, OSError):
                pass
            units[unit] = "disabled" if has_install else "static"

    return units


def run(
    host_root: Path,
    executor: Optional[Executor],
    base_image_preset_text: Optional[str] = None,
    warnings: Optional[list] = None,
) -> ServiceSection:
    host_root = Path(host_root)
    section = ServiceSection()

    current: Dict[str, str] = {}

    # Try systemctl first, fall back to filesystem scan
    if executor is not None:
        cmd = ["systemctl", "list-unit-files", "--no-pager", "--no-legend"]
        if str(host_root) != "/":
            cmd = ["systemctl", "--root", str(host_root), "list-unit-files", "--no-pager", "--no-legend"]
        _debug(f"running: {' '.join(cmd)}")
        result = executor(cmd)
        _debug(f"returncode={result.returncode}, stdout={len(result.stdout)} bytes, stderr={result.stderr[:200] if result.stderr else ''}")
        if result.returncode == 0 and result.stdout.strip():
            current = _parse_systemctl_list_unit_files(result.stdout)
            _debug(f"parsed {len(current)} unit files from systemctl output")

    if not current:
        _debug("systemctl unavailable or failed, falling back to filesystem scan")
        current = _scan_unit_files_from_fs(host_root)
        _debug(f"fs scan found {len(current)} unit files")

    if _DEBUG and current:
        sample = list(current.items())[:5]
        _debug(f"sample: {sample}")

    if not current:
        return section

    default_enabled, default_disabled, has_disable_all = _parse_preset_files(
        host_root, base_image_preset_text=base_image_preset_text,
    )
    if base_image_preset_text is None and warnings is not None:
        warnings.append({
            "source": "service",
            "message": (
                "No base image service presets available — service state changes are "
                "reported without comparison to base image defaults. "
                "All non-default-enabled units will appear as changes."
            ),
            "severity": "warning",
        })

    for unit, state in current.items():
        if not unit.endswith(".service") and not unit.endswith(".timer"):
            continue
        if unit in default_enabled:
            default_state = "enabled"
        elif unit in default_disabled:
            default_state = "disabled"
        elif has_disable_all:
            default_state = "disabled"
        else:
            default_state = "unknown"

        action = "unchanged"
        if state == "enabled" and default_state != "enabled":
            action = "enable"
            section.enabled_units.append(unit)
        elif state == "disabled" and default_state == "enabled":
            action = "disable"
            section.disabled_units.append(unit)
        elif state == "masked":
            action = "mask"
        section.state_changes.append(
            ServiceStateChange(
                unit=unit,
                current_state=state,
                default_state=default_state,
                action=action,
            )
        )

    _debug(f"result: {len(section.enabled_units)} enabled changes, "
           f"{len(section.disabled_units)} disabled changes, "
           f"{len(section.state_changes)} total tracked")
    return section
