"""Shared utilities for inspectah: debug logging, safe filesystem helpers."""

import os
import sys
from pathlib import Path
from typing import List, Optional, Tuple

# UID range treated as "non-system" (operator-created) accounts.
# Used by both the users_groups and container inspectors.
NON_SYSTEM_UID_MIN: int = 1000
NON_SYSTEM_UID_MAX: int = 60000  # exclusive

# RPM fallback args shared between rpm and config inspectors
_RPM_LOCK_DEFINE: List[str] = ["--define", "_rpmlock_path /var/tmp/.rpm.lock"]

_DEBUG = bool(os.environ.get("INSPECTAH_DEBUG", ""))


def debug(label: str, msg: str) -> None:
    """Print a debug message to stderr when INSPECTAH_DEBUG is set."""
    if _DEBUG:
        print(f"[inspectah] {label}: {msg}", file=sys.stderr)


def is_debug() -> bool:
    return _DEBUG


# ---------------------------------------------------------------------------
# User-facing progress output (distinct from debug logging)
# ---------------------------------------------------------------------------

class _C:
    """ANSI colour constants.  All blanked when stderr is not a TTY."""
    BOLD   = "\033[1m"
    DIM    = "\033[2m"
    GREEN  = "\033[32m"
    YELLOW = "\033[33m"
    CYAN   = "\033[36m"
    RESET  = "\033[0m"


if not sys.stderr.isatty():
    _C.BOLD = _C.DIM = _C.GREEN = _C.YELLOW = _C.CYAN = _C.RESET = ""


def status(msg: str) -> None:
    """Print a user-facing progress line to stderr."""
    print(f"  {_C.GREEN}\uf00c{_C.RESET}  {msg}", file=sys.stderr)


def section_banner(title: str, step: int, total: int) -> None:
    """Print a section header with a [step/total] counter to stderr."""
    counter = f"{_C.DIM}[{step}/{total}]{_C.RESET}"
    rule = f"{_C.DIM}{'─' * (42 - len(title))}{_C.RESET}"
    print(f"{_C.CYAN}──{_C.RESET} {counter} {_C.BOLD}{title}{_C.RESET} {rule}", file=sys.stderr)


def safe_iterdir(d: Path) -> List[Path]:
    """List directory contents, returning [] on permission/OS errors."""
    try:
        return sorted(d.iterdir())
    except (PermissionError, OSError):
        return []


def make_warning(source: str, message: str, severity: str = "warning") -> dict:
    """Build a structured warning dict with consistent keys."""
    return {"source": source, "message": message, "severity": severity}


def safe_read(p: Path, label: str = "") -> str:
    """Read a text file, returning '' on permission/OS errors."""
    try:
        return p.read_text()
    except (PermissionError, OSError) as exc:
        if label:
            debug(label, f"cannot read {p}: {exc}")
        return ""


def parse_dist_info_name(stem: str) -> Tuple[str, str]:
    """Parse a dist-info directory stem (``name-version``) into ``(name, version)``.

    Canonical wheel naming splits on the first component whose first character
    is a digit.  Returns ``(stem, "")`` if no version component is found.
    """
    parts = stem.split("-")
    for idx, part in enumerate(parts):
        if part and part[0].isdigit():
            return "-".join(parts[:idx]), "-".join(parts[idx:])
    return stem, ""


def detect_rpmdb_path(host_root: Path, *, relative: bool = False) -> str:
    """Return the rpmdb path for *host_root*.

    Probes ``<host_root>/usr/lib/sysimage/rpm/`` first (Fedora 33+, modern
    default), then falls back to ``<host_root>/var/lib/rpm/`` (RHEL 9,
    CentOS, older Fedora).  Returns the first directory that exists and
    is non-empty; defaults to the traditional path if neither qualifies.

    When *relative* is True, the returned path is relative to *host_root*
    (e.g. ``/var/lib/rpm``).  This is useful for ``rpm --root`` + ``--dbpath``
    combos where rpm interprets the dbpath relative to the root.
    """
    # In-container paths (used for probing and for relative return value).
    _RPMDB_CANDIDATES = [
        Path("/usr/lib/sysimage/rpm"),
        Path("/var/lib/rpm"),
    ]
    for in_container in _RPMDB_CANDIDATES:
        on_disk = host_root / in_container.relative_to("/")
        if on_disk.is_dir() and safe_iterdir(on_disk):
            return str(in_container) if relative else str(on_disk)
    # Default to the traditional location when neither directory exists
    # (the subsequent rpm call will fail and trigger --root fallback).
    fallback = _RPMDB_CANDIDATES[-1]
    return str(fallback) if relative else str(host_root / fallback.relative_to("/"))


def run_rpm_query(executor, host_root: Path, args: List[str]):
    """Run an rpm query against *host_root* with ``--dbpath`` fallback to ``--root``.

    On RHEL/CentOS the ``--dbpath`` form is preferred because it avoids
    chroot limitations.  If it fails (e.g. locked DB), we retry with
    ``--root`` plus the lock-path override.
    """
    if str(host_root) == "/":
        prefix: List[str] = ["rpm"]
    else:
        prefix = ["rpm", "--dbpath", detect_rpmdb_path(host_root)]
    result = executor(prefix + args)
    if result.returncode != 0 and str(host_root) != "/":
        result = executor(
            ["rpm", "--root", str(host_root)] + _RPM_LOCK_DEFINE + args
        )
    return result
