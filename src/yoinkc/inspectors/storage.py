"""Storage inspector: fstab, mount points, LVM, NFS/CIFS, multipath, /var scan. File-based + executor under host_root."""

from pathlib import Path
from typing import List, Optional

from ..executor import Executor
from ..schema import StorageSection, FstabEntry, MountPoint, LvmVolume, VarDirectory


# Directories under /var to scan for application data.
# Each gets a category and default recommendation.
_VAR_SCAN_DIRS = [
    ("var/lib", "application data"),
    ("var/log", "log retention"),
    ("var/data", "application data"),
    ("var/www", "web content"),
    ("var/opt", "add-on packages"),
]

# Known non-application directories under /var/lib that are managed by
# the OS or by bootc and should not appear in the migration plan.
_VAR_LIB_SKIP = frozenset({
    "alternatives", "authselect", "dbus", "dnf", "logrotate", "misc",
    "NetworkManager", "os-prober", "plymouth", "polkit-1", "portables",
    "private", "rpm", "rpm-state", "selinux", "sss", "systemd",
    "tuned", "unbound", "tpm2-tss",
})


def _scan_var_directories(host_root: Path) -> List[VarDirectory]:
    """Scan /var for non-empty directories that likely contain application data."""
    results: List[VarDirectory] = []

    for subdir, category in _VAR_SCAN_DIRS:
        d = host_root / subdir
        try:
            if not d.exists() or not d.is_dir():
                continue
        except (PermissionError, OSError):
            continue

        for entry in sorted(_safe_iterdir(d)):
            if not entry.is_dir() or entry.name.startswith("."):
                continue

            # Skip known OS-managed dirs under var/lib
            if subdir == "var/lib" and entry.name in _VAR_LIB_SKIP:
                continue

            # Check if non-empty (has at least one file)
            has_file = False
            total_size = 0
            try:
                for f in entry.rglob("*"):
                    if f.is_file():
                        has_file = True
                        try:
                            total_size += f.stat().st_size
                        except (PermissionError, OSError):
                            pass
                        if total_size > 10 * 1024 * 1024:
                            break  # stop counting after 10 MB
            except (PermissionError, OSError):
                pass

            if not has_file:
                continue

            # Human-readable size estimate
            if total_size > 1024 * 1024 * 1024:
                size_str = f"~{total_size / (1024**3):.1f} GB"
            elif total_size > 1024 * 1024:
                size_str = f"~{total_size / (1024**2):.0f} MB"
            elif total_size > 1024:
                size_str = f"~{total_size / 1024:.0f} KB"
            else:
                size_str = f"{total_size} bytes"

            rel_path = str(entry.relative_to(host_root))
            rec = _var_recommendation(rel_path, category)
            results.append(VarDirectory(
                path=rel_path,
                size_estimate=size_str,
                recommendation=rec,
            ))

    return results


def _var_recommendation(path: str, category: str) -> str:
    """Map a /var directory to a migration recommendation."""
    p = "/" + path

    if "mysql" in p or "pgsql" in p or "postgres" in p or "mongodb" in p or "mariadb" in p:
        return "PVC / volume mount — database storage, must persist independently"
    if "containers" in p or "docker" in p:
        return "PVC / volume mount — container storage"
    if "/var/log" in p:
        return "PVC / volume mount — log retention (or ship to external logging)"
    if "/var/www" in p:
        return "Image-embedded or PVC — depends on whether content is static"
    if "cache" in p.lower():
        return "Ephemeral — rebuilds on next run, no migration needed"
    if "spool" in p:
        return "PVC / volume mount — spool data (mail, print, at jobs)"
    return f"PVC / volume mount — {category}, review application needs"


def _safe_iterdir(d: Path):
    try:
        return list(d.iterdir())
    except (PermissionError, OSError):
        return []


def run(
    host_root: Path,
    executor: Optional[Executor],
) -> StorageSection:
    section = StorageSection()
    host_root = Path(host_root)

    fstab = host_root / "etc/fstab"
    try:
        fstab_lines = fstab.read_text().splitlines() if fstab.exists() else []
    except (PermissionError, OSError):
        fstab_lines = []
    if fstab_lines:
        for line in fstab_lines:
            line = line.strip()
            if line and not line.startswith("#"):
                parts = line.split()
                if len(parts) >= 3:
                    section.fstab_entries.append(
                        FstabEntry(device=parts[0], mount_point=parts[1], fstype=parts[2])
                    )

    if executor:
        r = executor(["findmnt", "--json", "--real"])
        if r.returncode == 0 and r.stdout.strip():
            try:
                import json
                data = json.loads(r.stdout)
                for fs in data.get("filesystems", []):
                    section.mount_points.append(MountPoint(
                        target=fs.get("target", ""),
                        source=fs.get("source", ""),
                        fstype=fs.get("fstype", ""),
                        options=fs.get("options", ""),
                    ))
            except Exception:
                pass

        r = executor(["lvs", "--reportformat", "json", "--units", "g"])
        if r.returncode == 0 and r.stdout.strip():
            try:
                import json
                data = json.loads(r.stdout)
                for lv in data.get("report", [{}])[0].get("lv", []):
                    section.lvm_info.append(LvmVolume(
                        lv_name=lv.get("lv_name", ""),
                        vg_name=lv.get("vg_name", ""),
                        lv_size=lv.get("lv_size", ""),
                    ))
            except Exception:
                pass

    try:
        iscsi_conf = host_root / "etc/iscsi/initiatorname.iscsi"
        if iscsi_conf.exists():
            section.mount_points.append(MountPoint(target="iSCSI", source="etc/iscsi/initiatorname.iscsi", fstype="iscsi"))
    except (PermissionError, OSError):
        pass

    try:
        multipath = host_root / "etc/multipath.conf"
        if multipath.exists():
            section.mount_points.append(MountPoint(target="multipath", source="etc/multipath.conf", fstype="dm-multipath"))
    except (PermissionError, OSError):
        pass

    # /var directory scan for data migration plan
    section.var_directories = _scan_var_directories(host_root)

    return section
