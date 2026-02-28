"""Storage inspector: fstab, mount points, LVM, NFS/CIFS, multipath. File-based + executor under host_root."""

from pathlib import Path
from typing import Optional

from ..executor import Executor
from ..schema import StorageSection, FstabEntry, MountPoint, LvmVolume


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

    return section
