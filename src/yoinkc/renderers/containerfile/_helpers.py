"""Shared helpers for the containerfile renderer package."""

import re
from typing import List, Optional

from ...schema import InspectionSnapshot


def _summarise_diff(diff_text: str) -> List[str]:
    """Produce human-readable change summaries from a unified diff.

    For simple key=value changes, produces "key: old → new".
    Falls back to raw +/- lines for complex changes.
    """
    additions: dict = {}
    removals: dict = {}
    other: List[str] = []

    for line in diff_text.strip().splitlines():
        if line.startswith("---") or line.startswith("+++") or line.startswith("@@"):
            continue
        stripped = line[1:].strip() if len(line) > 1 else ""
        if not stripped or stripped.startswith("#"):
            continue
        if line.startswith("-"):
            if "=" in stripped or ":" in stripped:
                sep = "=" if "=" in stripped else ":"
                key = stripped.split(sep, 1)[0].strip()
                removals[key] = stripped.split(sep, 1)[1].strip()
            else:
                other.append(f"removed: {stripped}")
        elif line.startswith("+"):
            if "=" in stripped or ":" in stripped:
                sep = "=" if "=" in stripped else ":"
                key = stripped.split(sep, 1)[0].strip()
                additions[key] = stripped.split(sep, 1)[1].strip()
            else:
                other.append(f"added: {stripped}")

    results: List[str] = []
    matched_keys = set()
    for key in additions:
        if key in removals:
            results.append(f"{key}: {removals[key]} → {additions[key]}")
            matched_keys.add(key)
        else:
            results.append(f"{key}: added ({additions[key]})")
    for key in removals:
        if key not in matched_keys:
            results.append(f"{key}: removed")
    results.extend(other)
    return results or ["(diff available — see audit report)"]


# Characters that would change shell semantics if injected into a RUN command.
# The data comes from RPM databases / systemd on an operator-controlled host,
# so this is a safety net against corrupted snapshots, not a security boundary.
_SHELL_UNSAFE_RE = re.compile(r'[\n\r;`|]|\$\(')


def _sanitize_shell_value(value: str, context: str) -> Optional[str]:
    """Return *value* if it is safe to embed in a shell RUN command, else None.

    Rejects values containing newlines, semicolons, backticks, ``$(...)``, or
    pipe characters — the characters that materially change shell semantics.
    When None is returned the caller should emit a FIXME comment instead.
    """
    if _SHELL_UNSAFE_RE.search(value):
        return None
    return value


# ---------------------------------------------------------------------------
# Kernel argument filtering
# ---------------------------------------------------------------------------

# Exact bare-word kernel parameters that are always managed by the bootloader
# or base image and must never appear in a kargs.d image drop-in.
_KARGS_BOOTLOADER_EXACT: frozenset = frozenset({
    "ro", "rw", "rhgb", "quiet", "splash",
})

# Prefixes whose matching kargs are likewise bootloader/installer-owned.
_KARGS_BOOTLOADER_PREFIXES: tuple = (
    "BOOT_IMAGE=",
    "root=",
    "rootflags=",
    "rootfstype=",
    "initrd=",
    "initramfs=",
    "crashkernel=",
    "resume=",
    "rd.lvm.lv=",
    "rd.luks.uuid=",
    "rd.luks.name=",
    "rd.md.uuid=",
    "LANG=",
)


def _is_bootloader_karg(karg: str) -> bool:
    """Return True if *karg* is a standard bootloader/installer parameter.

    These are managed by the bootloader or base image and should not appear
    in a kargs.d TOML drop-in.  Only operator-added kargs belong there.
    """
    if karg in _KARGS_BOOTLOADER_EXACT:
        return True
    for prefix in _KARGS_BOOTLOADER_PREFIXES:
        if karg.startswith(prefix):
            return True
    return False


def _operator_kargs(cmdline: str) -> List[str]:
    """Return the operator-added kernel arguments from a raw cmdline string.

    Filters out both standard bootloader-managed parameters and any kargs
    that contain shell-unsafe characters.
    """
    result: List[str] = []
    for karg in cmdline.split():
        if _is_bootloader_karg(karg):
            continue
        if _sanitize_shell_value(karg, "kargs") is None:
            continue
        result.append(karg)
    return result


def _base_image_from_snapshot(snapshot: InspectionSnapshot) -> str:
    """Return FROM line base image, preferring the one stored in the snapshot."""
    from ...baseline import base_image_for_snapshot
    return base_image_for_snapshot(snapshot)


def _dhcp_connection_paths(snapshot: InspectionSnapshot) -> set:
    """Return relative paths of NM profiles that are NOT static (DHCP/other).

    These belong in the kickstart, not baked into the image.
    """
    paths: set = set()
    if snapshot.network:
        for c in snapshot.network.connections:
            if c.method != "static" and c.path:
                paths.add(c.path)
    return paths
