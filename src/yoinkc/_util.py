"""Shared utilities for yoinkc: debug logging, safe filesystem helpers."""

import os
import sys
from pathlib import Path
from typing import List

_DEBUG = bool(os.environ.get("YOINKC_DEBUG", ""))


def debug(label: str, msg: str) -> None:
    """Print a debug message to stderr when YOINKC_DEBUG is set."""
    if _DEBUG:
        print(f"[yoinkc] {label}: {msg}", file=sys.stderr)


def is_debug() -> bool:
    return _DEBUG


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
