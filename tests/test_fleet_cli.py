# tests/test_fleet_cli.py
"""Tests for yoinkc-fleet CLI."""

import io
import json
import tarfile
from pathlib import Path

import pytest

from yoinkc.schema import InspectionSnapshot, OsRelease, RpmSection, PackageEntry


def _make_tarball(tmp_path, hostname, packages):
    """Create a test tarball with the given packages."""
    snap = InspectionSnapshot(
        meta={"hostname": hostname},
        os_release=OsRelease(name="RHEL", version_id="9.4", id="rhel"),
        rpm=RpmSection(
            packages_added=[
                PackageEntry(name=n, version="1.0", release="1", arch="x86_64")
                for n in packages
            ],
            base_image="quay.io/centos-bootc/centos-bootc:stream9",
        ),
    )
    tarball_path = tmp_path / f"{hostname}.tar.gz"
    with tarfile.open(tarball_path, "w:gz") as tar:
        data = snap.model_dump_json().encode()
        info = tarfile.TarInfo(name="inspection-snapshot.json")
        info.size = len(data)
        tar.addfile(info, io.BytesIO(data))
    return tarball_path


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


class TestFleetEndToEnd:
    def test_aggregate_produces_valid_snapshot(self, tmp_path):
        from yoinkc.fleet.__main__ import main
        _make_tarball(tmp_path, "web-01", ["httpd", "php"])
        _make_tarball(tmp_path, "web-02", ["httpd", "mod_ssl"])

        output = tmp_path / "merged.json"
        exit_code = main(["aggregate", str(tmp_path), "-o", str(output), "-p", "50"])
        assert exit_code == 0

        data = json.loads(output.read_text())
        snap = InspectionSnapshot(**data)
        assert snap.meta["fleet"]["total_hosts"] == 2
        pkg_names = {p.name for p in snap.rpm.packages_added}
        assert "httpd" in pkg_names
        assert "php" in pkg_names
        assert "mod_ssl" in pkg_names

        httpd = next(p for p in snap.rpm.packages_added if p.name == "httpd")
        assert httpd.fleet.count == 2
        assert httpd.include is True

    def test_aggregate_default_output_path(self, tmp_path):
        from yoinkc.fleet.__main__ import main
        _make_tarball(tmp_path, "web-01", ["httpd"])
        _make_tarball(tmp_path, "web-02", ["httpd"])

        exit_code = main(["aggregate", str(tmp_path)])
        assert exit_code == 0
        assert (tmp_path / "fleet-snapshot.json").exists()

    def test_aggregate_fewer_than_two_exits(self, tmp_path):
        from yoinkc.fleet.__main__ import main
        _make_tarball(tmp_path, "web-01", ["httpd"])
        with pytest.raises(SystemExit):
            main(["aggregate", str(tmp_path)])

    def test_aggregate_empty_dir_exits(self, tmp_path):
        from yoinkc.fleet.__main__ import main
        with pytest.raises(SystemExit):
            main(["aggregate", str(tmp_path)])
