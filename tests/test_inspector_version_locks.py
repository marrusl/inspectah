"""Unit tests for _parse_nevra_pattern and _collect_version_locks (rpm inspector)."""

import pytest

from inspectah.executor import RunResult
from inspectah.inspectors.rpm import _collect_version_locks, _parse_nevra_pattern


# ---------------------------------------------------------------------------
# _parse_nevra_pattern
# ---------------------------------------------------------------------------

class TestParseNevraPattern:
    def test_full_nevra_with_epoch(self):
        e = _parse_nevra_pattern("1:curl-7.76.1-26.el9.x86_64")
        assert e.name == "curl"
        assert e.epoch == 1
        assert e.version == "7.76.1"
        assert e.release == "26.el9"
        assert e.arch == "x86_64"
        assert e.raw_pattern == "1:curl-7.76.1-26.el9.x86_64"

    def test_no_epoch(self):
        e = _parse_nevra_pattern("curl-7.76.1-26.el9.x86_64")
        assert e.name == "curl"
        assert e.epoch == 0
        assert e.version == "7.76.1"
        assert e.release == "26.el9"
        assert e.arch == "x86_64"

    def test_wildcard_arch(self):
        e = _parse_nevra_pattern("curl-7.76.1-26.el9.*")
        assert e.name == "curl"
        assert e.arch == "*"
        assert e.version == "7.76.1"
        assert e.release == "26.el9"

    def test_multi_part_name(self):
        """python3-urllib3 has a hyphenated name with digits in the suffix."""
        e = _parse_nevra_pattern("python3-urllib3-1.26.5-3.el9.noarch")
        assert e.name == "python3-urllib3"
        assert e.epoch == 0
        assert e.version == "1.26.5"
        assert e.release == "3.el9"
        assert e.arch == "noarch"

    def test_name_with_leading_digits(self):
        """lib2to3 contains digits but the name boundary must still be found correctly."""
        e = _parse_nevra_pattern("lib2to3-2.0.0-2.el9.noarch")
        assert e.name == "lib2to3"
        assert e.version == "2.0.0"
        assert e.release == "2.el9"
        assert e.arch == "noarch"

    def test_epoch_zero_explicit(self):
        """Explicit 0: epoch normalises to integer 0."""
        e = _parse_nevra_pattern("0:openssl-3.0.7-24.el9.x86_64")
        assert e.epoch == 0
        assert e.name == "openssl"

    def test_raw_pattern_preserved(self):
        raw = "  1:curl-7.76.1-26.el9.x86_64  "
        e = _parse_nevra_pattern(raw)
        assert e.raw_pattern == "1:curl-7.76.1-26.el9.x86_64"


# ---------------------------------------------------------------------------
# _collect_version_locks
# ---------------------------------------------------------------------------

def _make_executor(stdout="", returncode=0):
    def executor(cmd, **kwargs):
        return RunResult(stdout=stdout, stderr="", returncode=returncode)
    return executor


class TestCollectVersionLocks:
    def test_multiple_entries_comments_and_blanks_skipped(self, tmp_path):
        vl = tmp_path / "etc" / "dnf" / "plugins"
        vl.mkdir(parents=True)
        (vl / "versionlock.list").write_text(
            "# versionlock list\n"
            "\n"
            "curl-7.76.1-26.el9.x86_64\n"
            "  # another comment\n"
            "openssl-3.0.7-24.el9.x86_64\n"
        )
        entries, _ = _collect_version_locks(None, tmp_path)
        names = [e.name for e in entries]
        assert names == ["curl", "openssl"]

    def test_dnf_path_takes_priority_over_yum(self, tmp_path):
        dnf_dir = tmp_path / "etc" / "dnf" / "plugins"
        dnf_dir.mkdir(parents=True)
        (dnf_dir / "versionlock.list").write_text("curl-7.76.1-26.el9.x86_64\n")

        yum_dir = tmp_path / "etc" / "yum" / "pluginconf.d"
        yum_dir.mkdir(parents=True)
        (yum_dir / "versionlock.list").write_text("wget-1.21.1-7.el9.x86_64\n")

        entries, _ = _collect_version_locks(None, tmp_path)
        assert len(entries) == 1
        assert entries[0].name == "curl"

    def test_yum_fallback_when_dnf_absent(self, tmp_path):
        yum_dir = tmp_path / "etc" / "yum" / "pluginconf.d"
        yum_dir.mkdir(parents=True)
        (yum_dir / "versionlock.list").write_text("wget-1.21.1-7.el9.x86_64\n")

        entries, _ = _collect_version_locks(None, tmp_path)
        assert len(entries) == 1
        assert entries[0].name == "wget"

    def test_neither_path_exists(self, tmp_path):
        entries, output = _collect_version_locks(None, tmp_path)
        assert entries == []
        assert output is None

    def test_command_output_stored(self, tmp_path):
        raw_output = "curl-7.76.1-26.el9.x86_64\n"
        executor = _make_executor(stdout=raw_output, returncode=0)
        _, output = _collect_version_locks(executor, tmp_path)
        assert output == raw_output

    def test_command_failure_yields_none_output(self, tmp_path):
        executor = _make_executor(stdout="", returncode=1)
        _, output = _collect_version_locks(executor, tmp_path)
        assert output is None

    def test_no_executor_yields_none_output(self, tmp_path):
        _, output = _collect_version_locks(None, tmp_path)
        assert output is None

    def test_epoch_in_versionlock_file(self, tmp_path):
        vl = tmp_path / "etc" / "dnf" / "plugins"
        vl.mkdir(parents=True)
        (vl / "versionlock.list").write_text("1:curl-7.76.1-26.el9.x86_64\n")
        entries, _ = _collect_version_locks(None, tmp_path)
        assert len(entries) == 1
        assert entries[0].epoch == 1
        assert entries[0].name == "curl"
