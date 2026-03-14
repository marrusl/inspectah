# tests/test_fleet_cli.py
"""Tests for yoinkc-fleet CLI."""

import pytest


class TestFleetCliParsing:
    def test_aggregate_requires_input_dir(self):
        from yoinkc.fleet.cli import parse_args
        with pytest.raises(SystemExit):
            parse_args([])

    def test_aggregate_basic(self, tmp_path):
        from yoinkc.fleet.cli import parse_args
        args = parse_args(["aggregate", str(tmp_path)])
        assert args.input_dir == tmp_path
        assert args.min_prevalence == 100
        assert args.no_hosts is False

    def test_aggregate_with_prevalence(self, tmp_path):
        from yoinkc.fleet.cli import parse_args
        args = parse_args(["aggregate", str(tmp_path), "-p", "80"])
        assert args.min_prevalence == 80

    def test_aggregate_with_output(self, tmp_path):
        from yoinkc.fleet.cli import parse_args
        args = parse_args(["aggregate", str(tmp_path), "-o", "/tmp/merged.json"])
        assert str(args.output) == "/tmp/merged.json"

    def test_aggregate_no_hosts_flag(self, tmp_path):
        from yoinkc.fleet.cli import parse_args
        args = parse_args(["aggregate", str(tmp_path), "--no-hosts"])
        assert args.no_hosts is True

    def test_prevalence_out_of_range(self, tmp_path):
        from yoinkc.fleet.cli import parse_args
        with pytest.raises(SystemExit):
            parse_args(["aggregate", str(tmp_path), "-p", "0"])
        with pytest.raises(SystemExit):
            parse_args(["aggregate", str(tmp_path), "-p", "101"])
