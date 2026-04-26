"""Regression tests for 2026-04-08 bug fixes.

Bug 1 (8ca8ffa): Config directory/file collision in rendering
  - _safe_write_file() must handle dest-is-directory and parent-is-file gracefully
  - Config inspector must filter rpm_va_by_path to /etc-only paths
  - Kinoite-like scenarios with dir/file collisions must not crash

Bug 2 (8240b18): inspectah-build Containerfile path
  - RETIRED: standalone inspectah-build script removed. Regression coverage
    moved to Go tests in cmd/inspectah/internal/cli/build_test.go
    (TestBuildCmd_AbsoluteContainerfilePath).
"""

import sys
import tempfile
from pathlib import Path
from unittest.mock import patch

import pytest

from inspectah.renderers.containerfile._config_tree import _safe_write_file, write_config_tree
from inspectah.inspectors.config import run as run_config_inspector
from inspectah.schema import (
    ConfigFileEntry,
    ConfigFileKind,
    ConfigSection,
    InspectionSnapshot,
    RpmSection,
    RpmVaEntry,
)


# ---------------------------------------------------------------------------
# Bug 1: _safe_write_file — directory/file collision handling
# ---------------------------------------------------------------------------


class TestSafeWriteFileCollisions:
    """Verify _safe_write_file gracefully skips on path collisions."""

    def test_skips_when_dest_is_directory(self, tmp_path, capsys):
        """When dest already exists as a directory, skip the write and warn."""
        dest = tmp_path / "etc" / "foo"
        dest.mkdir(parents=True)

        _safe_write_file(dest, "should not be written")

        # dest should still be a directory, not a file
        assert dest.is_dir()
        assert not dest.is_file()

        # Warning should have been printed to stderr
        captured = capsys.readouterr()
        assert "skipping config file write" in captured.err
        assert "already a directory" in captured.err

    def test_skips_when_parent_component_is_file(self, tmp_path, capsys):
        """When an ancestor of dest is already a regular file, skip and warn."""
        # Create a file at a path that would need to be a directory
        blocker = tmp_path / "etc" / "foo"
        blocker.parent.mkdir(parents=True, exist_ok=True)
        blocker.write_text("I am a file")

        # Now try to write to a path underneath the blocker
        dest = tmp_path / "etc" / "foo" / "bar" / "baz.conf"
        _safe_write_file(dest, "should not be written")

        # The blocker should still be a file
        assert blocker.is_file()
        assert blocker.read_text() == "I am a file"

        # The nested file should not exist
        assert not dest.exists()

        # Warning should have been printed to stderr
        captured = capsys.readouterr()
        assert "skipping config file write" in captured.err
        assert "parent path conflict" in captured.err

    def test_normal_write_succeeds(self, tmp_path):
        """Normal case: no collision, file is written."""
        dest = tmp_path / "etc" / "new" / "config.conf"
        _safe_write_file(dest, "hello world")

        assert dest.is_file()
        assert dest.read_text() == "hello world"

    def test_overwrites_existing_file(self, tmp_path):
        """An existing regular file at dest should be overwritten normally."""
        dest = tmp_path / "config.conf"
        dest.write_text("original")

        _safe_write_file(dest, "updated")

        assert dest.read_text() == "updated"


# ---------------------------------------------------------------------------
# Bug 1: Config inspector /etc filter for rpm_va_by_path
# ---------------------------------------------------------------------------


class TestConfigInspectorEtcFilter:
    """Verify config inspector filters rpm_va to /etc-only paths."""

    def test_rpm_va_non_etc_paths_excluded(self, tmp_path):
        """rpm_va entries outside /etc must not appear in config output."""
        # Set up minimal host_root with /etc
        etc = tmp_path / "etc"
        etc.mkdir()
        # Place a file the inspector would find for an /etc entry
        (etc / "httpd.conf").write_text("ServerRoot /etc/httpd")

        rpm_section = RpmSection(
            rpm_va=[
                RpmVaEntry(path="/etc/httpd.conf", flags="S.5....T.", package="httpd"),
                RpmVaEntry(path="/usr/lib/systemd/system/httpd.service", flags="S.5....T.", package="httpd"),
                RpmVaEntry(path="/var/log/something.log", flags="..5....T.", package="some-pkg"),
            ],
        )

        section = run_config_inspector(
            host_root=tmp_path,
            executor=None,
            rpm_section=rpm_section,
            rpm_owned_paths_override=set(),
            config_diffs=False,
        )

        # Only /etc paths should produce config entries of kind RPM_OWNED_MODIFIED
        modified_paths = [
            f.path for f in section.files
            if f.kind == ConfigFileKind.RPM_OWNED_MODIFIED
        ]
        assert "/etc/httpd.conf" in modified_paths
        assert "/usr/lib/systemd/system/httpd.service" not in modified_paths
        assert "/var/log/something.log" not in modified_paths

    def test_rpm_va_etc_paths_still_work(self, tmp_path):
        """rpm_va entries under /etc are still captured correctly."""
        etc = tmp_path / "etc" / "sysconfig"
        etc.mkdir(parents=True)
        (etc / "httpd").write_text("OPTION=value")

        rpm_section = RpmSection(
            rpm_va=[
                RpmVaEntry(path="/etc/sysconfig/httpd", flags="S.5....T.", package="httpd"),
            ],
        )

        section = run_config_inspector(
            host_root=tmp_path,
            executor=None,
            rpm_section=rpm_section,
            rpm_owned_paths_override=set(),
            config_diffs=False,
        )

        modified = [f for f in section.files if f.kind == ConfigFileKind.RPM_OWNED_MODIFIED]
        assert len(modified) == 1
        assert modified[0].path == "/etc/sysconfig/httpd"
        assert modified[0].content == "OPTION=value"


# ---------------------------------------------------------------------------
# Bug 1: Kinoite-like scenario — write_config_tree survives collisions
# ---------------------------------------------------------------------------


class TestConfigTreeCollisionScenario:
    """Kinoite-like scenario: config entries create dir/file name collisions.

    For example, one entry writes to /etc/foo (a file) and another needs
    /etc/foo/bar.conf (requiring /etc/foo to be a directory).  The old code
    would crash; the fix must skip the conflicting write gracefully.
    """

    def test_file_then_subdir_collision(self, tmp_path):
        """Writing /etc/foo as file then /etc/foo/bar.conf must not crash."""
        snapshot = InspectionSnapshot(
            config=ConfigSection(files=[
                ConfigFileEntry(
                    path="/etc/foo",
                    kind=ConfigFileKind.UNOWNED,
                    content="I am a file",
                    include=True,
                ),
                ConfigFileEntry(
                    path="/etc/foo/bar.conf",
                    kind=ConfigFileKind.UNOWNED,
                    content="nested under foo",
                    include=True,
                ),
            ]),
        )

        # Must not raise
        write_config_tree(snapshot, tmp_path)

        config_dir = tmp_path / "config"
        # The first entry should have been written as a file
        assert (config_dir / "etc" / "foo").is_file()
        # The second entry should have been skipped (parent is a file)
        assert not (config_dir / "etc" / "foo" / "bar.conf").exists()

    def test_subdir_then_file_collision(self, tmp_path):
        """Writing /etc/foo/bar.conf first, then /etc/foo as file, must not crash."""
        snapshot = InspectionSnapshot(
            config=ConfigSection(files=[
                ConfigFileEntry(
                    path="/etc/foo/bar.conf",
                    kind=ConfigFileKind.UNOWNED,
                    content="nested first",
                    include=True,
                ),
                ConfigFileEntry(
                    path="/etc/foo",
                    kind=ConfigFileKind.UNOWNED,
                    content="file second",
                    include=True,
                ),
            ]),
        )

        # Must not raise
        write_config_tree(snapshot, tmp_path)

        config_dir = tmp_path / "config"
        # /etc/foo should be a directory (from mkdir for bar.conf)
        assert (config_dir / "etc" / "foo").is_dir()
        # bar.conf should exist
        assert (config_dir / "etc" / "foo" / "bar.conf").is_file()
        # The file write to /etc/foo should have been skipped (it's a dir)

    def test_no_crash_on_multiple_collisions(self, tmp_path):
        """Multiple collision scenarios in one snapshot must all be handled."""
        snapshot = InspectionSnapshot(
            config=ConfigSection(files=[
                ConfigFileEntry(
                    path="/etc/a",
                    kind=ConfigFileKind.UNOWNED,
                    content="file a",
                    include=True,
                ),
                ConfigFileEntry(
                    path="/etc/a/nested.conf",
                    kind=ConfigFileKind.UNOWNED,
                    content="nested under a",
                    include=True,
                ),
                ConfigFileEntry(
                    path="/etc/b/deep/config.conf",
                    kind=ConfigFileKind.UNOWNED,
                    content="deep config",
                    include=True,
                ),
                ConfigFileEntry(
                    path="/etc/b",
                    kind=ConfigFileKind.UNOWNED,
                    content="file b",
                    include=True,
                ),
            ]),
        )

        # Must not raise
        write_config_tree(snapshot, tmp_path)


# ---------------------------------------------------------------------------
# Bug 2: inspectah-build Containerfile path — RETIRED
# Standalone inspectah-build script removed. Regression coverage moved to
# Go tests: cmd/inspectah/internal/cli/build_test.go
#   - TestBuildCmd_AbsoluteContainerfilePath
# ---------------------------------------------------------------------------
