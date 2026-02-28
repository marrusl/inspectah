"""Kernel/Boot inspector: cmdline, grub, sysctl, modules-load.d, modprobe.d, dracut.

File-based under host_root, plus ``lsmod`` via executor.
"""

from pathlib import Path
from typing import Dict, List, Optional, Set, Tuple

from ..executor import Executor
from ..schema import KernelBootSection


def _safe_iterdir(d: Path) -> List[Path]:
    try:
        return list(d.iterdir())
    except (PermissionError, OSError):
        return []


def _safe_read(p: Path) -> str:
    try:
        return p.read_text()
    except (PermissionError, OSError):
        return ""


# ---------------------------------------------------------------------------
# Module helpers
# ---------------------------------------------------------------------------

def _parse_lsmod(text: str) -> List[dict]:
    """Parse ``lsmod`` output into a list of {name, size, used_by}."""
    results: List[dict] = []
    for line in text.splitlines()[1:]:  # skip header
        parts = line.split()
        if len(parts) < 2:
            continue
        name = parts[0]
        size = parts[1] if len(parts) > 1 else "0"
        used_by = parts[3] if len(parts) > 3 else ""
        results.append({"name": name, "size": size, "used_by": used_by})
    return results


def _collect_expected_modules(host_root: Path) -> Set[str]:
    """Gather module names that are explicitly configured to load.

    Sources: ``/usr/lib/modules-load.d/*.conf`` and ``/etc/modules-load.d/*.conf``.
    """
    expected: Set[str] = set()
    for base in ("usr/lib/modules-load.d", "etc/modules-load.d"):
        d = host_root / base
        if not d.exists():
            continue
        for f in _safe_iterdir(d):
            if f.is_file() and f.suffix == ".conf":
                for line in _safe_read(f).splitlines():
                    line = line.strip()
                    if line and not line.startswith("#"):
                        expected.add(line)
    return expected


def _collect_dependency_modules(loaded: List[dict]) -> Set[str]:
    """Build the set of modules that were loaded as dependencies of other modules.

    A module with a non-empty ``used_by`` column was pulled in because
    another module requires it — it is a dependency, not a top-level load.
    """
    deps: Set[str] = set()
    for mod in loaded:
        if mod.get("used_by", "").strip():
            deps.add(mod["name"])
    return deps


def _diff_modules(
    loaded: List[dict], expected: Set[str],
) -> List[dict]:
    """Return loaded modules that are neither explicitly configured nor a dependency."""
    dep_names = _collect_dependency_modules(loaded)
    loaded_names = {m["name"] for m in loaded}

    # Modules referenced in used_by of OTHER modules are built-in deps
    non_default: List[dict] = []
    for mod in loaded:
        name = mod["name"]
        if name in expected:
            continue
        if name in dep_names:
            continue
        non_default.append(mod)
    return non_default


# ---------------------------------------------------------------------------
# Sysctl helpers
# ---------------------------------------------------------------------------

def _parse_sysctl_conf(text: str) -> Dict[str, str]:
    """Parse a sysctl .conf file into {key: value}."""
    result: Dict[str, str] = {}
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith("#") or line.startswith(";"):
            continue
        if "=" not in line:
            continue
        key, _, val = line.partition("=")
        result[key.strip()] = val.strip()
    return result


def _collect_sysctl_defaults(host_root: Path) -> Dict[str, Tuple[str, str]]:
    """Read shipped sysctl defaults from ``/usr/lib/sysctl.d/``.

    Returns ``{dotted.key: (value, source_file)}``.
    Later files (sorted by name) override earlier ones, matching systemd behaviour.
    """
    defaults: Dict[str, Tuple[str, str]] = {}
    d = host_root / "usr/lib/sysctl.d"
    if not d.exists():
        return defaults
    for f in sorted(_safe_iterdir(d), key=lambda p: p.name):
        if f.is_file() and f.suffix == ".conf":
            rel = str(f.relative_to(host_root))
            for k, v in _parse_sysctl_conf(_safe_read(f)).items():
                defaults[k] = (v, rel)
    return defaults


def _sysctl_key_to_proc_path(host_root: Path, key: str) -> Path:
    """Convert ``net.ipv4.ip_forward`` to ``<host_root>/proc/sys/net/ipv4/ip_forward``."""
    return host_root / "proc/sys" / key.replace(".", "/")


def _read_runtime_sysctl(host_root: Path, key: str) -> Optional[str]:
    p = _sysctl_key_to_proc_path(host_root, key)
    try:
        if p.exists():
            return p.read_text().strip()
    except (PermissionError, OSError):
        pass
    return None


def _collect_sysctl_overrides(host_root: Path) -> Dict[str, Tuple[str, str]]:
    """Read operator sysctl overrides from ``/etc/sysctl.d/`` and ``/etc/sysctl.conf``.

    Returns ``{dotted.key: (value, source_file)}``.
    """
    overrides: Dict[str, Tuple[str, str]] = {}
    d = host_root / "etc/sysctl.d"
    if d.exists():
        for f in sorted(_safe_iterdir(d), key=lambda p: p.name):
            if f.is_file() and f.suffix == ".conf":
                rel = str(f.relative_to(host_root))
                for k, v in _parse_sysctl_conf(_safe_read(f)).items():
                    overrides[k] = (v, rel)
    try:
        sysctl_conf = host_root / "etc/sysctl.conf"
        if sysctl_conf.exists():
            for k, v in _parse_sysctl_conf(_safe_read(sysctl_conf)).items():
                overrides[k] = (v, "etc/sysctl.conf")
    except (PermissionError, OSError):
        pass
    return overrides


def _diff_sysctl(
    host_root: Path,
    defaults: Dict[str, Tuple[str, str]],
    overrides: Dict[str, Tuple[str, str]],
) -> List[dict]:
    """Compare runtime sysctl values against shipped defaults.

    For every key found in both defaults and overrides, or only in overrides,
    check the actual runtime value from ``/proc/sys/``.  Return entries
    where runtime differs from the shipped default.
    """
    all_keys = set(defaults) | set(overrides)
    results: List[dict] = []
    for key in sorted(all_keys):
        default_val, default_src = defaults.get(key, (None, ""))
        override_val, override_src = overrides.get(key, (None, ""))

        runtime = _read_runtime_sysctl(host_root, key)
        if runtime is None:
            runtime = override_val if override_val is not None else default_val

        if default_val is not None and runtime == default_val:
            continue

        results.append({
            "key": key,
            "runtime": runtime,
            "default": default_val or "",
            "source": override_src or default_src,
        })
    return results


# ---------------------------------------------------------------------------
# Main entry point
# ---------------------------------------------------------------------------

def run(
    host_root: Path,
    executor: Optional[Executor],
) -> KernelBootSection:
    section = KernelBootSection()
    host_root = Path(host_root)

    # --- cmdline ---
    try:
        cmdline = host_root / "proc/cmdline"
        if cmdline.exists():
            section.cmdline = cmdline.read_text().strip()
    except (PermissionError, OSError):
        pass

    # --- GRUB ---
    try:
        grub = host_root / "etc/default/grub"
        if grub.exists():
            section.grub_defaults = grub.read_text().strip()[:500]
    except (PermissionError, OSError):
        pass

    # --- sysctl diff ---
    defaults = _collect_sysctl_defaults(host_root)
    overrides = _collect_sysctl_overrides(host_root)
    section.sysctl_overrides = _diff_sysctl(host_root, defaults, overrides)

    # --- modules-load.d / modprobe.d / dracut ---
    for dirname, target_list in [
        ("etc/modules-load.d", section.modules_load_d),
        ("etc/modprobe.d", section.modprobe_d),
        ("etc/dracut.conf.d", section.dracut_conf),
    ]:
        d = host_root / dirname
        if d.exists():
            for f in _safe_iterdir(d):
                if f.is_file() and f.suffix == ".conf":
                    target_list.append({
                        "path": str(f.relative_to(host_root)),
                        "content": _safe_read(f),
                    })

    # --- lsmod + diff ---
    if executor:
        try:
            out = executor(["lsmod"])
            if out.returncode == 0 and out.stdout:
                section.loaded_modules = _parse_lsmod(out.stdout)
                expected = _collect_expected_modules(host_root)
                section.non_default_modules = _diff_modules(
                    section.loaded_modules, expected,
                )
        except Exception:
            pass

    return section
