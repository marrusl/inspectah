"""Config inspector tests for ostree/bootc source systems.

Verifies:
- Tier 1: /usr/etc -> /etc diff detection (modified, unmodified, volatile)
- Tier 2: /etc-only files (unowned, RPM-owned post-install)
- Symlink target change detection
- SELinux context comparison (when xattr available)
"""

import os
from pathlib import Path

from yoinkc.executor import RunResult
from yoinkc.inspectors.config import run as run_config
from yoinkc.schema import ConfigFileKind, SystemType


def _config_executor(cmd, *, cwd=None):
    """Executor for config inspector ostree tests."""
    if "rpm" in cmd and "-qf" in cmd:
        if any("custom-app.conf" in a for a in cmd):
            return RunResult(stdout="", stderr="not owned", returncode=1)
        if any("rpm-post.conf" in a for a in cmd):
            return RunResult(stdout="some-rpm\n", stderr="", returncode=0)
        return RunResult(stdout="", stderr="not owned", returncode=1)
    if "rpm" in cmd and "-V" in cmd:
        if any("rpm-post.conf" in a for a in cmd) or "some-rpm" in cmd:
            return RunResult(stdout="S.5....T.  c /etc/rpm-post.conf\n", stderr="", returncode=1)
        return RunResult(stdout="", stderr="", returncode=0)
    if "rpm" in cmd and "-qa" in cmd and "--queryformat" in cmd:
        return RunResult(stdout="/etc/rpm-post.conf\n", stderr="", returncode=0)
    return RunResult(stdout="", stderr="", returncode=1)


def _setup_ostree_config(tmp_path):
    """Set up a mock ostree filesystem with /usr/etc and /etc."""
    usr_etc = tmp_path / "usr" / "etc"
    (usr_etc / "ssh").mkdir(parents=True)
    (usr_etc / "ssh" / "sshd_config").write_text(
        "Port 22\nPermitRootLogin yes\nPasswordAuthentication yes\n"
    )
    (usr_etc / "httpd" / "conf").mkdir(parents=True)
    (usr_etc / "httpd" / "conf" / "httpd.conf").write_text("ServerRoot /etc/httpd\nListen 80\n")

    etc = tmp_path / "etc"
    (etc / "ssh").mkdir(parents=True)
    (etc / "ssh" / "sshd_config").write_text(
        "Port 2222\nPermitRootLogin no\nPasswordAuthentication yes\n"
    )
    (etc / "httpd" / "conf").mkdir(parents=True)
    (etc / "httpd" / "conf" / "httpd.conf").write_text("ServerRoot /etc/httpd\nListen 80\n")

    # Tier 2: etc-only files
    (etc / "custom-app.conf").write_text("key=value\n")
    (etc / "rpm-post.conf").write_text("modified\n")

    # usr/etc-only file (should NOT be reported)
    (usr_etc / "unmodified.conf").write_text("defaults\n")

    # Volatile files (should be skipped)
    (etc / "resolv.conf").write_text("nameserver 8.8.8.8\n")
    (etc / "hostname").write_text("test-host\n")
    (etc / "machine-id").write_text("abc123\n")

    # os-release (should be skipped in Tier 2)
    (etc / "os-release").write_text(
        'NAME="Fedora Linux"\nVERSION_ID=41\nID=fedora\nVARIANT_ID=silverblue\n'
    )

    return tmp_path


class TestOstreeModifiedConfig:
    """Tier 1: modified configs in /etc vs /usr/etc are detected."""

    def test_ostree_modified_config_detected(self, tmp_path):
        host_root = _setup_ostree_config(tmp_path)
        section = run_config(
            host_root,
            _config_executor,
            system_type=SystemType.RPM_OSTREE,
        )
        paths = [e.path for e in section.files]
        assert "etc/ssh/sshd_config" in paths, (
            f"sshd_config should be detected as modified, got paths: {paths}"
        )
        # Check that it has diff content
        sshd_entry = [e for e in section.files if e.path == "etc/ssh/sshd_config"][0]
        assert sshd_entry.diff_against_rpm is not None, "Modified config should have a diff"
        assert "Port 2222" in sshd_entry.diff_against_rpm

    def test_ostree_unmodified_config_not_reported(self, tmp_path):
        host_root = _setup_ostree_config(tmp_path)
        section = run_config(
            host_root,
            _config_executor,
            system_type=SystemType.RPM_OSTREE,
        )
        paths = [e.path for e in section.files]
        assert "etc/httpd/conf/httpd.conf" not in paths, (
            f"Unmodified httpd.conf should NOT be reported, got paths: {paths}"
        )

    def test_ostree_usr_etc_only_file_not_reported(self, tmp_path):
        host_root = _setup_ostree_config(tmp_path)
        section = run_config(
            host_root,
            _config_executor,
            system_type=SystemType.RPM_OSTREE,
        )
        paths = [e.path for e in section.files]
        assert "etc/unmodified.conf" not in paths, (
            f"File only in /usr/etc should NOT be reported, got paths: {paths}"
        )


class TestOstreeEtcOnlyFiles:
    """Tier 2: files only in /etc (no /usr/etc counterpart)."""

    def test_ostree_etc_only_unowned_file_detected(self, tmp_path):
        host_root = _setup_ostree_config(tmp_path)
        section = run_config(
            host_root,
            _config_executor,
            system_type=SystemType.RPM_OSTREE,
        )
        paths = [e.path for e in section.files]
        assert "etc/custom-app.conf" in paths, (
            f"Unowned etc-only file should be detected, got paths: {paths}"
        )
        entry = [e for e in section.files if e.path == "etc/custom-app.conf"][0]
        assert entry.kind == ConfigFileKind.UNOWNED

    def test_ostree_etc_only_rpm_owned_post_detected(self, tmp_path):
        host_root = _setup_ostree_config(tmp_path)
        section = run_config(
            host_root,
            _config_executor,
            system_type=SystemType.RPM_OSTREE,
        )
        paths = [e.path for e in section.files]
        assert "etc/rpm-post.conf" in paths, (
            f"RPM-owned post-install config should be detected, got paths: {paths}"
        )
        entry = [e for e in section.files if e.path == "etc/rpm-post.conf"][0]
        assert entry.kind == ConfigFileKind.RPM_OWNED_MODIFIED

    def test_ostree_rpm_v_other_file_modified_not_false_positive(self, tmp_path):
        """When rpm -V reports another file in the same package as modified,
        our specific file should NOT be incorrectly classified as modified."""
        host_root = _setup_ostree_config(tmp_path)

        # Add a file that is RPM-owned but NOT listed in rpm -V output
        (tmp_path / "etc" / "clean-rpm.conf").write_text("clean config\n")

        def executor_other_file_modified(cmd, *, cwd=None):
            if "rpm" in cmd and "-qf" in cmd:
                if any("custom-app.conf" in a for a in cmd):
                    return RunResult(stdout="", stderr="not owned", returncode=1)
                if any("rpm-post.conf" in a for a in cmd):
                    return RunResult(stdout="some-rpm\n", stderr="", returncode=0)
                if any("clean-rpm.conf" in a for a in cmd):
                    return RunResult(stdout="some-rpm\n", stderr="", returncode=0)
                return RunResult(stdout="", stderr="not owned", returncode=1)
            if "rpm" in cmd and "-V" in cmd:
                # rpm -V returns rc=1 because ANOTHER file in the package is
                # modified, but /etc/clean-rpm.conf is NOT in the output
                return RunResult(
                    stdout="S.5....T.  c /etc/rpm-post.conf\n",
                    stderr="", returncode=1,
                )
            if "rpm" in cmd and "-qa" in cmd and "--queryformat" in cmd:
                return RunResult(
                    stdout="/etc/rpm-post.conf\n/etc/clean-rpm.conf\n",
                    stderr="", returncode=0,
                )
            return RunResult(stdout="", stderr="", returncode=1)

        section = run_config(
            host_root,
            executor_other_file_modified,
            system_type=SystemType.RPM_OSTREE,
        )
        paths = [e.path for e in section.files]
        # rpm-post.conf IS in rpm -V output -> should be RPM_OWNED_MODIFIED
        assert "etc/rpm-post.conf" in paths
        rpm_post = [e for e in section.files if e.path == "etc/rpm-post.conf"][0]
        assert rpm_post.kind == ConfigFileKind.RPM_OWNED_MODIFIED

        # clean-rpm.conf is NOT in rpm -V output -> should NOT appear as modified
        assert "etc/clean-rpm.conf" not in paths, (
            f"clean-rpm.conf should not be reported as modified (it's not in rpm -V output), "
            f"got paths: {paths}"
        )


class TestOstreeVolatileFiltering:
    """Volatile files that change every boot are skipped."""

    def test_ostree_volatile_files_skipped(self, tmp_path):
        host_root = _setup_ostree_config(tmp_path)
        section = run_config(
            host_root,
            _config_executor,
            system_type=SystemType.RPM_OSTREE,
        )
        paths = [e.path for e in section.files]
        for volatile in ["etc/resolv.conf", "etc/hostname", "etc/machine-id"]:
            assert volatile not in paths, (
                f"Volatile file {volatile} should be skipped, got paths: {paths}"
            )

    def test_ostree_os_release_skipped(self, tmp_path):
        """os-release is always present on ostree systems, not a customization."""
        host_root = _setup_ostree_config(tmp_path)
        section = run_config(
            host_root,
            _config_executor,
            system_type=SystemType.RPM_OSTREE,
        )
        paths = [e.path for e in section.files]
        assert "etc/os-release" not in paths, (
            f"os-release should be skipped, got paths: {paths}"
        )


class TestOstreeSymlinkChange:
    """Symlink target changes are detected."""

    def test_ostree_symlink_target_change_detected(self, tmp_path):
        usr_etc = tmp_path / "usr" / "etc"
        usr_etc.mkdir(parents=True, exist_ok=True)
        etc = tmp_path / "etc"
        etc.mkdir(parents=True, exist_ok=True)

        # Create symlinks with different targets
        (usr_etc / "localtime").symlink_to("/usr/share/zoneinfo/UTC")
        (etc / "localtime").symlink_to("/usr/share/zoneinfo/America/New_York")

        section = run_config(
            tmp_path,
            _config_executor,
            system_type=SystemType.RPM_OSTREE,
        )
        paths = [e.path for e in section.files]
        assert "etc/localtime" in paths, (
            f"Symlink target change should be detected, got paths: {paths}"
        )
        entry = [e for e in section.files if e.path == "etc/localtime"][0]
        assert "New_York" in (entry.content or ""), (
            f"Content should mention new symlink target, got: {entry.content}"
        )
