"""Slice 2 inspector adaptation tests: storage, kernel/boot, scheduled tasks."""
from pathlib import Path
from yoinkc.executor import RunResult
from yoinkc.schema import SystemType, MountPoint


def test_ostree_mounts_filtered_from_storage(tmp_path):
    from yoinkc.inspectors.storage import run as run_storage
    etc = tmp_path / "etc"
    etc.mkdir()
    (etc / "fstab").write_text("/dev/sda1  /boot  ext4  defaults  0 2\n")
    proc = tmp_path / "proc"
    proc.mkdir()
    (proc / "self").mkdir(parents=True)

    def executor(cmd, *, cwd=None):
        cmd_str = " ".join(cmd)
        if "findmnt" in cmd_str:
            return RunResult(
                stdout=(
                    '{"filesystems": ['
                    '{"target": "/", "source": "/dev/sda2", "fstype": "ext4", "options": "rw"},'
                    '{"target": "/sysroot", "source": "/dev/sda2", "fstype": "ext4", "options": "ro"},'
                    '{"target": "/ostree", "source": "/dev/sda2", "fstype": "ext4", "options": "ro"},'
                    '{"target": "/boot", "source": "/dev/sda1", "fstype": "ext4", "options": "rw"},'
                    '{"target": "/var", "source": "/dev/sda2", "fstype": "ext4", "options": "rw"}'
                    ']}'
                ),
                stderr="", returncode=0,
            )
        if "lsblk" in cmd_str:
            return RunResult(stdout="", stderr="", returncode=1)
        if "pvs" in cmd_str or "lvs" in cmd_str or "vgs" in cmd_str:
            return RunResult(stdout="", stderr="", returncode=1)
        return RunResult(stdout="", stderr="", returncode=1)

    section = run_storage(tmp_path, executor, system_type=SystemType.RPM_OSTREE)
    mount_targets = [m.target for m in section.mount_points]
    assert "/sysroot" not in mount_targets
    assert "/ostree" not in mount_targets
    assert "/boot" in mount_targets or "/" in mount_targets  # At least non-ostree mounts remain


def test_ostree_grub_defaults_suppressed(tmp_path):
    from yoinkc.inspectors.kernel_boot import run as run_kernel_boot
    etc = tmp_path / "etc"
    etc.mkdir()
    (etc / "default").mkdir()
    (etc / "default" / "grub").write_text("GRUB_TIMEOUT=5\n")
    proc = tmp_path / "proc"
    proc.mkdir()
    (proc / "cmdline").write_text("root=/dev/sda2 ro rhgb quiet custom.option=foo")
    def executor(cmd, *, cwd=None):
        if "lsmod" in " ".join(cmd):
            return RunResult(stdout="Module  Size  Used\nvfat  20480  1\n", stderr="", returncode=0)
        return RunResult(stdout="", stderr="", returncode=1)
    section = run_kernel_boot(tmp_path, executor, system_type=SystemType.RPM_OSTREE)
    assert section.grub_defaults == ""


def test_ostree_cmdline_still_captured(tmp_path):
    from yoinkc.inspectors.kernel_boot import run as run_kernel_boot
    proc = tmp_path / "proc"
    proc.mkdir()
    (proc / "cmdline").write_text(
        "BOOT_IMAGE=/vmlinuz root=/dev/sda2 ro rhgb quiet mitigations=off custom.opt=1"
    )
    def executor(cmd, *, cwd=None):
        if "lsmod" in " ".join(cmd):
            return RunResult(stdout="", stderr="", returncode=0)
        return RunResult(stdout="", stderr="", returncode=1)
    section = run_kernel_boot(tmp_path, executor, system_type=SystemType.RPM_OSTREE)
    assert section.cmdline is not None
    assert "mitigations=off" in section.cmdline
    assert "custom.opt=1" in section.cmdline
