"""
Tests verifying every CLI flag is parsed and wired through to behavior.
"""

import sys
import unittest.mock
from pathlib import Path

import pytest

from inspectah.cli import parse_args


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


def test_host_root_equal_syntax_counts_as_explicit():
    """`--host-root=/path` must count as an explicit override."""
    args = parse_args(["inspect", "--host-root=/host"])
    assert args.host_root == Path("/host")
    assert args.host_root_explicit is True


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

    with unittest.mock.patch("inspectah.inspectors.run_all") as mock_run_all:
        mock_run_all.return_value = unittest.mock.MagicMock()
        from inspectah.__main__ import _run_inspectors
        _run_inspectors(Path("/host"), args)
        mock_run_all.assert_called_once()
        call_kwargs = mock_run_all.call_args
        assert call_kwargs.kwargs.get("baseline_packages_file") == Path("/tmp/pkgs.txt")


def test_no_baseline_reaches_inspectors():
    """--no-baseline is parsed and passed through to run_all as no_baseline_opt_in."""
    import unittest.mock
    args = parse_args(["--no-baseline"])
    assert args.no_baseline is True

    with unittest.mock.patch("inspectah.inspectors.run_all") as mock_run_all:
        mock_run_all.return_value = unittest.mock.MagicMock()
        from inspectah.__main__ import _run_inspectors
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
    """Unhandled exceptions print a debug hint when INSPECTAH_DEBUG is unset."""
    monkeypatch.delenv("INSPECTAH_DEBUG", raising=False)
    with unittest.mock.patch("inspectah.__main__.run_pipeline", side_effect=RuntimeError("boom")):
        from inspectah.__main__ import main
        rc = main(["--skip-preflight"])
    assert rc == 1
    err = capsys.readouterr().err
    assert "boom" in err
    assert "INSPECTAH_DEBUG" in err


def test_main_exception_prints_traceback_in_debug_mode(capsys, monkeypatch):
    """Full traceback is printed when INSPECTAH_DEBUG=1."""
    monkeypatch.setenv("INSPECTAH_DEBUG", "1")
    with unittest.mock.patch("inspectah.__main__.run_pipeline", side_effect=RuntimeError("kaboom")):
        from inspectah.__main__ import main
        rc = main(["--skip-preflight"])
    assert rc == 1
    err = capsys.readouterr().err
    assert "kaboom" in err
    assert "Traceback" in err


def test_main_git_init_failure_returns_error(capsys, tmp_path, monkeypatch):
    """When github deps are available but init_git_repo fails, main() exits 1 with a helpful message."""
    monkeypatch.delenv("INSPECTAH_DEBUG", raising=False)
    snap = _make_main_snapshot()
    # The packaged-install guard checks whether github/git are importable. Mock them
    # as available so the test can reach the init_git_repo failure path it exercises.
    fake_github = unittest.mock.MagicMock()
    fake_git = unittest.mock.MagicMock()
    with (
        unittest.mock.patch("inspectah.__main__.run_pipeline", return_value=snap),
        unittest.mock.patch("inspectah.git_github.init_git_repo", return_value=False) as mock_init,
        unittest.mock.patch("inspectah.git_github.add_and_commit") as mock_commit,
        unittest.mock.patch.dict(sys.modules, {"github": fake_github, "git": fake_git}),
    ):
        from inspectah.__main__ import main
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


# --- Subcommand routing tests ---


class TestSubcommandRouting:
    """Verify the subcommand-based CLI structure."""

    def test_no_args_defaults_to_inspect(self):
        args = parse_args([])
        assert args.command in (None, "inspect")

    def test_explicit_inspect_subcommand(self):
        args = parse_args(["inspect"])
        assert args.command == "inspect"

    def test_inspect_with_from_snapshot(self):
        args = parse_args(["inspect", "--from-snapshot", "foo.json"])
        assert args.command == "inspect"
        assert args.from_snapshot == Path("foo.json")

    def test_bare_flags_parsed_as_inspect(self):
        """Bare `inspectah --from-snapshot foo.json` should parse as inspect."""
        args = parse_args(["--from-snapshot", "foo.json"])
        assert args.command in (None, "inspect")
        assert args.from_snapshot == Path("foo.json")

    def test_fleet_subcommand_recognized(self):
        args = parse_args(["fleet", "/some/dir"])
        assert args.command == "fleet"

    def test_refine_subcommand_recognized(self):
        args = parse_args(["refine", "dummy.tar.gz"])
        assert args.command == "refine"

    def test_top_level_help_lists_subcommands(self, capsys):
        with pytest.raises(SystemExit) as exc_info:
            parse_args(["--help"])
        assert exc_info.value.code == 0
        out = capsys.readouterr().out
        assert "fleet" in out
        assert "refine" in out

    def test_inspect_help_shows_inspect_flags(self, capsys):
        with pytest.raises(SystemExit) as exc_info:
            parse_args(["inspect", "--help"])
        assert exc_info.value.code == 0
        out = capsys.readouterr().out
        assert "--from-snapshot" in out

    @pytest.mark.parametrize("flags,attr,expected", [
        (["inspect", "--host-root", "/mnt"], "host_root", Path("/mnt")),
        (["inspect", "-o", "/tmp/out.tar.gz"], "output_file", Path("/tmp/out.tar.gz")),
        (["inspect", "--output-dir", "/tmp/d"], "output_dir", Path("/tmp/d")),
        (["inspect", "--no-subscription"], "no_subscription", True),
        (["inspect", "--inspect-only"], "inspect_only", True),
        (["inspect", "--config-diffs"], "config_diffs", True),
        (["inspect", "--deep-binary-scan"], "deep_binary_scan", True),
        (["inspect", "--query-podman"], "query_podman", True),
        (["inspect", "--skip-preflight"], "skip_preflight", True),
        (["inspect", "--output-dir", "/d", "--validate"], "validate", True),
        (["inspect", "--target-version", "9.6"], "target_version", "9.6"),
        (["inspect", "--target-image", "reg/img:1"], "target_image", "reg/img:1"),
        (["inspect", "--no-baseline"], "no_baseline", True),
        (["inspect", "--baseline-packages", "/f"], "baseline_packages", Path("/f")),
        (["inspect", "--user-strategy", "sysusers"], "user_strategy", "sysusers"),
    ], ids=lambda v: str(v) if not isinstance(v, list) else " ".join(v))
    def test_inspect_flags_under_subcommand(self, flags, attr, expected):
        """All inspect flags work under the explicit 'inspect' subcommand."""
        args = parse_args(flags)
        assert getattr(args, attr) == expected

    @pytest.mark.parametrize("flags,attr,expected", [
        (["--host-root", "/mnt"], "host_root", Path("/mnt")),
        (["-o", "/tmp/out.tar.gz"], "output_file", Path("/tmp/out.tar.gz")),
        (["--output-dir", "/tmp/d"], "output_dir", Path("/tmp/d")),
        (["--no-subscription"], "no_subscription", True),
        (["--inspect-only"], "inspect_only", True),
        (["--config-diffs"], "config_diffs", True),
        (["--deep-binary-scan"], "deep_binary_scan", True),
        (["--query-podman"], "query_podman", True),
        (["--skip-preflight"], "skip_preflight", True),
        (["--output-dir", "/d", "--validate"], "validate", True),
        (["--target-version", "9.6"], "target_version", "9.6"),
        (["--target-image", "reg/img:1"], "target_image", "reg/img:1"),
        (["--no-baseline"], "no_baseline", True),
        (["--baseline-packages", "/f"], "baseline_packages", Path("/f")),
        (["--user-strategy", "sysusers"], "user_strategy", "sysusers"),
    ], ids=lambda v: str(v) if not isinstance(v, list) else " ".join(v))
    def test_inspect_flags_bare(self, flags, attr, expected):
        """All inspect flags work without explicit 'inspect' subcommand (backwards compat)."""
        args = parse_args(flags)
        assert getattr(args, attr) == expected


class TestFleetSubcommand:
    """Verify fleet subcommand parsing."""

    def test_fleet_input_dir_positional(self):
        args = parse_args(["fleet", "/some/dir"])
        assert args.command == "fleet"
        assert args.input_dir == Path("/some/dir")

    def test_fleet_min_prevalence(self):
        args = parse_args(["fleet", "/some/dir", "-p", "75"])
        assert args.min_prevalence == 75

    def test_fleet_min_prevalence_long(self):
        args = parse_args(["fleet", "/some/dir", "--min-prevalence", "50"])
        assert args.min_prevalence == 50

    def test_fleet_json_only(self):
        args = parse_args(["fleet", "/some/dir", "--json-only"])
        assert args.json_only is True

    def test_fleet_output_file(self):
        args = parse_args(["fleet", "/some/dir", "-o", "out.tar.gz"])
        assert args.output_file == Path("out.tar.gz")

    def test_fleet_output_dir(self):
        args = parse_args(["fleet", "/some/dir", "--output-dir", "/tmp/out"])
        assert args.output_dir == Path("/tmp/out")

    def test_fleet_no_hosts(self):
        args = parse_args(["fleet", "/some/dir", "--no-hosts"])
        assert args.no_hosts is True

    def test_fleet_defaults(self):
        args = parse_args(["fleet", "/some/dir"])
        assert args.min_prevalence == 100
        assert args.json_only is False
        assert args.no_hosts is False
        assert args.output_file is None
        assert args.output_dir is None

    def test_fleet_help_exits_cleanly(self, capsys):
        with pytest.raises(SystemExit) as exc_info:
            parse_args(["fleet", "--help"])
        assert exc_info.value.code == 0
        out = capsys.readouterr().out
        assert "input" in out.lower() or "dir" in out.lower()


class TestRefineSubcommand:
    """Verify refine subcommand parsing."""

    def test_refine_tarball_positional(self):
        args = parse_args(["refine", "foo.tar.gz"])
        assert args.command == "refine"
        assert args.tarball == Path("foo.tar.gz")

    def test_refine_no_browser_flag(self):
        args = parse_args(["refine", "foo.tar.gz", "--no-browser"])
        assert args.command == "refine"
        assert args.no_browser is True

    def test_refine_port_flag(self):
        args = parse_args(["refine", "foo.tar.gz", "--port", "9000"])
        assert args.command == "refine"
        assert args.port == 9000

    def test_refine_defaults(self):
        args = parse_args(["refine", "foo.tar.gz"])
        assert args.tarball == Path("foo.tar.gz")
        assert args.no_browser is False
        assert args.port == 8642

    def test_refine_help_exits_cleanly(self, capsys):
        with pytest.raises(SystemExit) as exc_info:
            parse_args(["refine", "--help"])
        assert exc_info.value.code == 0
        out = capsys.readouterr().out
        assert "tarball" in out.lower()


def test_skip_unavailable_flag():
    """--skip-unavailable is parsed correctly."""
    from inspectah.cli import parse_args
    args = parse_args(["inspect", "--skip-unavailable"])
    assert args.skip_unavailable is True


def test_skip_unavailable_default_false():
    """--skip-unavailable defaults to False."""
    from inspectah.cli import parse_args
    args = parse_args(["inspect"])
    assert args.skip_unavailable is False


class TestMainModule:
    """Verify `python -m inspectah` works via __main__.py guard."""

    def test_module_help(self):
        """python3 -m inspectah --help produces output and exits 0."""
        import subprocess

        result = subprocess.run(
            [sys.executable, "-m", "inspectah", "--help"],
            capture_output=True, text=True, timeout=10,
        )
        assert result.returncode == 0
        assert "inspect" in result.stdout
