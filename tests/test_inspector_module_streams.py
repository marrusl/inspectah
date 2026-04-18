"""Unit tests for _collect_module_streams and _apply_module_stream_baseline (rpm inspector)."""

import textwrap
from pathlib import Path

import pytest

from inspectah.inspectors.rpm import _apply_module_stream_baseline, _collect_module_streams
from inspectah.schema import EnabledModuleStream


def _write_module_file(directory: Path, filename: str, content: str) -> None:
    directory.mkdir(parents=True, exist_ok=True)
    (directory / filename).write_text(textwrap.dedent(content))


def _modules_dir(host_root: Path) -> Path:
    return host_root / "etc" / "dnf" / "modules.d"


# ---------------------------------------------------------------------------
# Happy-path cases
# ---------------------------------------------------------------------------

def test_well_formed_enabled(tmp_path):
    d = _modules_dir(tmp_path)
    _write_module_file(d, "postgresql.module", """\
        [postgresql]
        name=postgresql
        stream=15
        profiles=server
        state=enabled
    """)
    result = _collect_module_streams(tmp_path)
    assert len(result) == 1
    m = result[0]
    assert m.module_name == "postgresql"
    assert m.stream == "15"
    assert m.profiles == ["server"]
    assert m.include is True
    assert m.baseline_match is False


def test_state_installed_is_captured(tmp_path):
    """state=installed implies the stream is enabled and must be reproduced."""
    d = _modules_dir(tmp_path)
    _write_module_file(d, "nodejs.module", """\
        [nodejs]
        name=nodejs
        stream=18
        profiles=common
        state=installed
    """)
    result = _collect_module_streams(tmp_path)
    assert len(result) == 1
    assert result[0].module_name == "nodejs"
    assert result[0].stream == "18"


def test_state_disabled_is_skipped(tmp_path):
    d = _modules_dir(tmp_path)
    _write_module_file(d, "nginx.module", """\
        [nginx]
        name=nginx
        stream=mainline
        profiles=
        state=disabled
    """)
    result = _collect_module_streams(tmp_path)
    assert result == []


def test_multiple_modules_across_files(tmp_path):
    d = _modules_dir(tmp_path)
    _write_module_file(d, "postgresql.module", """\
        [postgresql]
        name=postgresql
        stream=15
        profiles=server
        state=enabled
    """)
    _write_module_file(d, "nodejs.module", """\
        [nodejs]
        name=nodejs
        stream=18
        profiles=common,development
        state=installed
    """)
    _write_module_file(d, "nginx.module", """\
        [nginx]
        name=nginx
        stream=mainline
        profiles=
        state=disabled
    """)
    result = _collect_module_streams(tmp_path)
    names = {m.module_name for m in result}
    assert names == {"postgresql", "nodejs"}
    nodejs = next(m for m in result if m.module_name == "nodejs")
    assert set(nodejs.profiles) == {"common", "development"}


def test_multiple_sections_in_one_file(tmp_path):
    """A single .module file may contain multiple module sections."""
    d = _modules_dir(tmp_path)
    _write_module_file(d, "multi.module", """\
        [module_a]
        name=module_a
        stream=1
        profiles=
        state=enabled

        [module_b]
        name=module_b
        stream=2
        profiles=
        state=disabled

        [module_c]
        name=module_c
        stream=3
        profiles=default
        state=installed
    """)
    result = _collect_module_streams(tmp_path)
    names = {m.module_name for m in result}
    assert names == {"module_a", "module_c"}


# ---------------------------------------------------------------------------
# Edge / error cases
# ---------------------------------------------------------------------------

def test_empty_directory(tmp_path):
    d = _modules_dir(tmp_path)
    d.mkdir(parents=True)
    result = _collect_module_streams(tmp_path)
    assert result == []


def test_missing_directory(tmp_path):
    result = _collect_module_streams(tmp_path)
    assert result == []


def test_malformed_file_missing_stream_key(tmp_path):
    """A section without a 'stream' key is silently skipped; other sections still parsed."""
    d = _modules_dir(tmp_path)
    _write_module_file(d, "bad.module", """\
        [broken]
        name=broken
        profiles=
        state=enabled

        [good]
        name=good
        stream=5
        profiles=
        state=enabled
    """)
    result = _collect_module_streams(tmp_path)
    assert len(result) == 1
    assert result[0].module_name == "good"
    assert result[0].stream == "5"


def test_no_profiles_field(tmp_path):
    """profiles field is optional; defaults to empty list."""
    d = _modules_dir(tmp_path)
    _write_module_file(d, "minimal.module", """\
        [mymod]
        name=mymod
        stream=1.0
        state=enabled
    """)
    result = _collect_module_streams(tmp_path)
    assert len(result) == 1
    assert result[0].profiles == []


# ---------------------------------------------------------------------------
# _apply_module_stream_baseline — comparison logic
# ---------------------------------------------------------------------------

def _make_streams(*specs):
    """Build a list of EnabledModuleStream objects from (name, stream) pairs."""
    return [EnabledModuleStream(module_name=name, stream=stream) for name, stream in specs]


class TestApplyModuleStreamBaseline:
    def test_match_sets_baseline_match_true(self):
        streams = _make_streams(("postgresql", "15"))
        _apply_module_stream_baseline(streams, {"postgresql": "15"}, [], warnings=None)
        assert streams[0].baseline_match is True

    def test_missing_from_baseline_is_false(self):
        streams = _make_streams(("nodejs", "18"))
        _apply_module_stream_baseline(streams, {}, [], warnings=None)
        assert streams[0].baseline_match is False

    def test_conflict_different_stream(self):
        streams = _make_streams(("postgresql", "15"))
        conflicts: list = []
        warnings: list = []
        _apply_module_stream_baseline(streams, {"postgresql": "13"}, conflicts, warnings=warnings)
        assert streams[0].baseline_match is False
        assert len(conflicts) == 1
        assert "postgresql" in conflicts[0]
        assert "host=15" in conflicts[0]
        assert "base_image=13" in conflicts[0]
        assert len(warnings) == 1

    def test_conflict_adds_to_warnings(self):
        streams = _make_streams(("postgresql", "15"))
        warnings: list = []
        _apply_module_stream_baseline(streams, {"postgresql": "13"}, [], warnings=warnings)
        assert any("postgresql" in str(w) for w in warnings)

    def test_no_baseline_all_remain_false(self):
        """Passing an empty baseline dict (no-baseline mode) leaves all streams False."""
        streams = _make_streams(("postgresql", "15"), ("nodejs", "18"))
        _apply_module_stream_baseline(streams, {}, [], warnings=None)
        assert all(not ms.baseline_match for ms in streams)

    def test_mixed_match_missing_conflict(self):
        streams = _make_streams(
            ("postgresql", "15"),   # conflict: base has 13
            ("nodejs", "18"),        # match
            ("nginx", "mainline"),   # missing from baseline
        )
        conflicts: list = []
        _apply_module_stream_baseline(
            streams,
            {"postgresql": "13", "nodejs": "18"},
            conflicts,
        )
        by_name = {ms.module_name: ms for ms in streams}
        assert by_name["postgresql"].baseline_match is False
        assert by_name["nodejs"].baseline_match is True
        assert by_name["nginx"].baseline_match is False
        assert len(conflicts) == 1
