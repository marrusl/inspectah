"""SELinux/Security inspector: mode, modules, booleans, audit rules, FIPS, PAM. File-based + executor."""

import os
import re
import sys
from pathlib import Path
from typing import List, Optional

from ..executor import Executor
from ..schema import SelinuxSection

_DEBUG = bool(os.environ.get("YOINKC_DEBUG", ""))


def _debug(msg: str) -> None:
    if _DEBUG:
        print(f"[yoinkc] selinux: {msg}", file=sys.stderr)


def _safe_iterdir(d: Path) -> List[Path]:
    try:
        return list(d.iterdir())
    except (PermissionError, OSError):
        return []


def _policy_type(host_root: Path) -> str:
    """Read SELINUXTYPE from /etc/selinux/config, default to 'targeted'."""
    cfg = host_root / "etc/selinux/config"
    try:
        if cfg.exists():
            for line in cfg.read_text().splitlines():
                line = line.strip()
                if line.startswith("SELINUXTYPE="):
                    return line.split("=", 1)[1].strip()
    except (PermissionError, OSError):
        pass
    return "targeted"


def _discover_custom_modules(host_root: Path, policy_type: str) -> List[str]:
    """Discover custom modules from the priority-400 module store.

    Modules at priority 400 were installed locally via ``semodule -i``.
    This is purely filesystem-based — no need for ``semodule`` command.
    """
    local_store = (
        host_root / "etc/selinux" / policy_type / "active/modules/400"
    )
    _debug(f"custom modules: checking {local_store}")
    try:
        if not local_store.is_dir():
            _debug(f"custom modules: {local_store} not a directory or missing")
            return []
    except (PermissionError, OSError) as e:
        _debug(f"custom modules: {local_store} access error: {e}")
        return []

    local_names = []
    for child in _safe_iterdir(local_store):
        if child.is_dir():
            local_names.append(child.name)

    _debug(f"custom modules: found {len(local_names)} in priority-400 store: {local_names}")
    return sorted(local_names)


_BOOL_RE = re.compile(
    r"^(\S+)\s+\((\w+)\s*,\s*(\w+)\)\s+(.*)"
)


def _parse_semanage_booleans(text: str) -> List[dict]:
    """Parse ``semanage boolean -l`` output.

    Returns all booleans where current state differs from the default,
    each as ``{"name": ..., "current": ..., "default": ..., "description": ...}``.
    """
    results: List[dict] = []
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith("SELinux boolean"):
            continue
        m = _BOOL_RE.match(line)
        if not m:
            continue
        name, current, default, desc = m.group(1), m.group(2), m.group(3), m.group(4).strip()
        results.append({
            "name": name,
            "current": current,
            "default": default,
            "non_default": current != default,
            "description": desc,
        })
    return results


def _read_booleans_from_fs(host_root: Path) -> List[dict]:
    """Fallback: read boolean runtime values from /sys/fs/selinux/booleans/.

    Returns only booleans whose current value differs from the pending
    (policy-loaded) value.  Without semanage we cannot get descriptions.
    """
    booldir = host_root / "sys/fs/selinux/booleans"
    _debug(f"boolean fs fallback: checking {booldir}")
    if not booldir.is_dir():
        _debug(f"boolean fs fallback: {booldir} not found")
        return []

    results: List[dict] = []
    for f in _safe_iterdir(booldir):
        if not f.is_file():
            continue
        try:
            parts = f.read_text().strip().split()
        except (PermissionError, OSError):
            continue
        if len(parts) >= 2:
            current = "on" if parts[0] == "1" else "off"
            pending = "on" if parts[1] == "1" else "off"
            if current != pending:
                results.append({
                    "name": f.name,
                    "current": current,
                    "default": pending,
                    "non_default": True,
                    "description": "",
                })
    _debug(f"boolean fs fallback: found {len(results)} non-default booleans")
    return results


def run(
    host_root: Path,
    executor: Optional[Executor],
    warnings: Optional[list] = None,
) -> SelinuxSection:
    section = SelinuxSection()
    host_root = Path(host_root)

    selinux_config = host_root / "etc/selinux/config"
    try:
        if selinux_config.exists():
            for line in selinux_config.read_text().splitlines():
                line = line.strip()
                if line.startswith("SELINUX="):
                    section.mode = line.split("=", 1)[1].strip()
                    break
    except (PermissionError, OSError):
        pass

    ptype = _policy_type(host_root)
    _debug(f"policy type: {ptype}")

    # --- Custom modules from priority-400 store (filesystem only) ---
    section.custom_modules = _discover_custom_modules(host_root, ptype)

    # --- Boolean overrides via chroot semanage, with filesystem fallback ---
    if executor:
        # Try the host's own semanage via chroot (works against host policy)
        _debug("trying: chroot /host semanage boolean -l")
        out = executor(["chroot", str(host_root), "semanage", "boolean", "-l"])
        if out.returncode == 0 and out.stdout.strip():
            _debug(f"semanage boolean -l succeeded ({len(out.stdout.splitlines())} lines)")
            section.boolean_overrides = _parse_semanage_booleans(out.stdout)
        else:
            _debug(f"semanage failed (rc={out.returncode}): {out.stderr.strip()[:200]}")
            # Fallback: try reading /sys/fs/selinux/booleans/ from the host
            fallback = _read_booleans_from_fs(host_root)
            section.boolean_overrides = fallback
            booldir = host_root / "sys/fs/selinux/booleans"
            if not booldir.is_dir() and warnings is not None:
                warnings.append({
                    "source": "selinux",
                    "message": "SELinux boolean override detection unavailable — semanage failed and /sys/fs/selinux/booleans not accessible.",
                    "severity": "warning",
                })

    audit_d = host_root / "etc/audit/rules.d"
    if audit_d.exists():
        for f in _safe_iterdir(audit_d):
            if f.is_file():
                section.audit_rules.append(str(f.relative_to(host_root)))

    fips_path = host_root / "proc/sys/crypto/fips_enabled"
    try:
        if fips_path.exists():
            section.fips_mode = fips_path.read_text().strip() == "1"
    except (PermissionError, OSError):
        pass

    pam_d = host_root / "etc/pam.d"
    if pam_d.exists():
        for f in _safe_iterdir(pam_d):
            if f.is_file():
                section.pam_configs.append(str(f.relative_to(host_root)))

    return section
