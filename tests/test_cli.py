"""
Tests verifying every CLI flag is parsed and wired through to behavior.
"""

import sys
import unittest.mock
from pathlib import Path

from yoinkc.cli import parse_args


def test_defaults():
    args = parse_args([])
    assert args.host_root == Path("/host")
    assert args.output_dir == Path("./output")
    assert args.from_snapshot is None
    assert args.inspect_only is False
    assert args.baseline_packages is None
    assert args.config_diffs is False
    assert args.deep_binary_scan is False
    assert args.query_podman is False
    assert args.validate is False
    assert args.push_to_github is None
    assert args.public is False
    assert args.yes is False


def test_all_flags_set():
    args = parse_args([
        "--host-root", "/mnt/host",
        "--output-dir", "/tmp/out",
        "--from-snapshot", "/tmp/snap.json",
        "--inspect-only",
        "--baseline-packages", "/tmp/pkgs.txt",
        "--config-diffs",
        "--deep-binary-scan",
        "--query-podman",
        "--validate",
        "--push-to-github", "owner/repo",
        "--public",
        "--yes",
    ])
    assert args.host_root == Path("/mnt/host")
    assert args.output_dir == Path("/tmp/out")
    assert args.from_snapshot == Path("/tmp/snap.json")
    assert args.inspect_only is True
    assert args.baseline_packages == Path("/tmp/pkgs.txt")
    assert args.config_diffs is True
    assert args.deep_binary_scan is True
    assert args.query_podman is True
    assert args.validate is True
    assert args.push_to_github == "owner/repo"
    assert args.public is True
    assert args.yes is True


def test_baseline_packages_reaches_inspectors():
    """--baseline-packages is parsed and passed through __main__._run_inspectors to run_all."""
    import unittest.mock
    args = parse_args(["--baseline-packages", "/tmp/pkgs.txt"])
    assert args.baseline_packages == Path("/tmp/pkgs.txt")

    with unittest.mock.patch("yoinkc.inspectors.run_all") as mock_run_all:
        mock_run_all.return_value = unittest.mock.MagicMock()
        from yoinkc.__main__ import _run_inspectors
        _run_inspectors(Path("/host"), args)
        mock_run_all.assert_called_once()
        call_kwargs = mock_run_all.call_args
        assert call_kwargs.kwargs.get("baseline_packages_file") == Path("/tmp/pkgs.txt")


def _make_main_snapshot():
    """Minimal snapshot mock for main() tests."""
    snap = unittest.mock.MagicMock()
    snap.redactions = []
    return snap


def test_main_exception_prints_hint(capsys, monkeypatch):
    """Unhandled exceptions print a debug hint when YOINKC_DEBUG is unset."""
    monkeypatch.delenv("YOINKC_DEBUG", raising=False)
    with unittest.mock.patch("yoinkc.__main__.run_pipeline", side_effect=RuntimeError("boom")):
        from yoinkc.__main__ import main
        rc = main([])
    assert rc == 1
    err = capsys.readouterr().err
    assert "boom" in err
    assert "YOINKC_DEBUG" in err


def test_main_exception_prints_traceback_in_debug_mode(capsys, monkeypatch):
    """Full traceback is printed when YOINKC_DEBUG=1."""
    monkeypatch.setenv("YOINKC_DEBUG", "1")
    with unittest.mock.patch("yoinkc.__main__.run_pipeline", side_effect=RuntimeError("kaboom")):
        from yoinkc.__main__ import main
        rc = main([])
    assert rc == 1
    err = capsys.readouterr().err
    assert "kaboom" in err
    assert "Traceback" in err


def test_main_git_init_failure_returns_error(capsys, tmp_path, monkeypatch):
    """When init_git_repo returns False, main() exits with code 1 and a helpful message."""
    monkeypatch.delenv("YOINKC_DEBUG", raising=False)
    snap = _make_main_snapshot()
    with (
        unittest.mock.patch("yoinkc.__main__.run_pipeline", return_value=snap),
        unittest.mock.patch("yoinkc.git_github.init_git_repo", return_value=False) as mock_init,
        unittest.mock.patch("yoinkc.git_github.add_and_commit") as mock_commit,
    ):
        from yoinkc.__main__ import main
        rc = main(["--output-dir", str(tmp_path), "--push-to-github", "owner/repo",
                   "--skip-preflight", "--yes"])
    assert rc == 1
    err = capsys.readouterr().err
    assert "git" in err.lower()
    assert "pip install" in err
    mock_commit.assert_not_called()
