"""
Tests verifying every CLI flag is parsed and wired through to behavior.
"""

import sys
import unittest.mock
from pathlib import Path

import pytest

from yoinkc.cli import parse_args


def test_defaults():
    args = parse_args([])
    assert args.host_root == Path("/host")
    assert args.output_file is None
    assert args.output_dir is None
    assert args.no_subscription is False
    assert args.from_snapshot is None
    assert args.inspect_only is False
    assert args.baseline_packages is None
    assert args.no_baseline is False
    assert args.config_diffs is False
    assert args.deep_binary_scan is False
    assert args.query_podman is False
    assert args.validate is False
    assert args.push_to_github is None
    assert args.public is False
    assert args.yes is False


def test_from_snapshot_flags():
    """Flags compatible with --from-snapshot parse correctly."""
    args = parse_args([
        "--host-root", "/mnt/host",
        "--output-dir", "/tmp/out",
        "--from-snapshot", "/tmp/snap.json",
        "--validate",
        "--push-to-github", "owner/repo",
        "--public",
        "--yes",
    ])
    assert args.host_root == Path("/mnt/host")
    assert args.output_dir == Path("/tmp/out")
    assert args.from_snapshot == Path("/tmp/snap.json")
    assert args.inspect_only is False
    assert args.validate is True
    assert args.push_to_github == "owner/repo"
    assert args.public is True
    assert args.yes is True


def test_output_file_short_flag():
    """'-o' sets the tarball output path."""
    args = parse_args(["-o", "/tmp/out.tar.gz"])
    assert args.output_file == Path("/tmp/out.tar.gz")
    assert args.output_dir is None


def test_output_dir_long_flag():
    """'--output-dir' sets directory output mode."""
    args = parse_args(["--output-dir", "/tmp/outdir"])
    assert args.output_dir == Path("/tmp/outdir")
    assert args.output_file is None


def test_output_file_and_output_dir_mutually_exclusive():
    """-o and --output-dir together must be rejected."""
    with pytest.raises(SystemExit):
        parse_args(["-o", "/tmp/out.tar.gz", "--output-dir", "/tmp/outdir"])


def test_no_subscription_flag():
    args = parse_args(["--no-subscription"])
    assert args.no_subscription is True


def test_validate_requires_output_dir():
    """--validate without --output-dir must be rejected."""
    with pytest.raises(SystemExit):
        parse_args(["--validate"])


def test_push_to_github_requires_output_dir():
    """--push-to-github without --output-dir must be rejected."""
    with pytest.raises(SystemExit):
        parse_args(["--push-to-github", "owner/repo"])


def test_validate_with_output_dir_accepted():
    args = parse_args(["--output-dir", "/tmp/out", "--validate"])
    assert args.validate is True
    assert args.output_dir == Path("/tmp/out")


def test_push_to_github_with_output_dir_accepted():
    args = parse_args(["--output-dir", "/tmp/out", "--push-to-github", "owner/repo"])
    assert args.push_to_github == "owner/repo"


def test_inspect_only_flags():
    """Flags compatible with --inspect-only parse correctly."""
    args = parse_args([
        "--host-root", "/mnt/host",
        "--inspect-only",
        "--baseline-packages", "/tmp/pkgs.txt",
        "--config-diffs",
        "--deep-binary-scan",
        "--query-podman",
    ])
    assert args.host_root == Path("/mnt/host")
    assert args.from_snapshot is None
    assert args.inspect_only is True
    assert args.baseline_packages == Path("/tmp/pkgs.txt")
    assert args.config_diffs is True
    assert args.deep_binary_scan is True
    assert args.query_podman is True


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


def test_no_baseline_reaches_inspectors():
    """--no-baseline is parsed and passed through to run_all as no_baseline_opt_in."""
    import unittest.mock
    args = parse_args(["--no-baseline"])
    assert args.no_baseline is True

    with unittest.mock.patch("yoinkc.inspectors.run_all") as mock_run_all:
        mock_run_all.return_value = unittest.mock.MagicMock()
        from yoinkc.__main__ import _run_inspectors
        _run_inspectors(Path("/host"), args)
        mock_run_all.assert_called_once()
        call_kwargs = mock_run_all.call_args
        assert call_kwargs.kwargs.get("no_baseline_opt_in") is True


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


def test_from_snapshot_and_inspect_only_are_mutually_exclusive():
    """--from-snapshot and --inspect-only together must be rejected."""
    with pytest.raises(SystemExit):
        parse_args(["--from-snapshot", "snap.json", "--inspect-only"])


def test_no_baseline_and_baseline_packages_are_mutually_exclusive():
    """--no-baseline and --baseline-packages together must be rejected."""
    with pytest.raises(SystemExit):
        parse_args(["--no-baseline", "--baseline-packages", "/tmp/pkgs.txt"])
