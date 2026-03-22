# tests/test_fleet_cli.py
"""Tests for yoinkc fleet subcommand CLI and end-to-end behaviour."""

import io
import json
import re
import tarfile
from pathlib import Path

import pytest

from yoinkc.schema import InspectionSnapshot, OsRelease, RpmSection, PackageEntry


def _make_snapshot(hostname, packages):
    """Create a minimal InspectionSnapshot for testing."""
    return InspectionSnapshot(
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


def _make_tarball(tmp_path, hostname, packages):
    """Create a test tarball with the given packages."""
    snap = _make_snapshot(hostname, packages)
    tarball_path = tmp_path / f"{hostname}.tar.gz"
    with tarfile.open(tarball_path, "w:gz") as tar:
        data = snap.model_dump_json().encode()
        info = tarfile.TarInfo(name="inspection-snapshot.json")
        info.size = len(data)
        tar.addfile(info, io.BytesIO(data))
    return tarball_path


def _make_snapshot_json(tmp_path, hostname, packages):
    """Write a minimal test snapshot as a JSON file."""
    snap = _make_snapshot(hostname, packages)
    path = tmp_path / f"{hostname}.json"
    path.write_text(snap.model_dump_json())
    return path


class TestFleetCliParsing:
    def test_requires_input_dir(self):
        from yoinkc.cli import parse_args
        with pytest.raises(SystemExit):
            parse_args(["fleet"])

    def test_basic(self, tmp_path):
        from yoinkc.cli import parse_args
        args = parse_args(["fleet", str(tmp_path)])
        assert args.input_dir == tmp_path
        assert args.min_prevalence == 100
        assert args.no_hosts is False

    def test_with_prevalence(self, tmp_path):
        from yoinkc.cli import parse_args
        args = parse_args(["fleet", str(tmp_path), "-p", "80"])
        assert args.min_prevalence == 80

    def test_with_output(self, tmp_path):
        from yoinkc.cli import parse_args
        args = parse_args(["fleet", str(tmp_path), "-o", "/tmp/merged.json"])
        assert args.output_file == Path("/tmp/merged.json")

    def test_no_hosts_flag(self, tmp_path):
        from yoinkc.cli import parse_args
        args = parse_args(["fleet", str(tmp_path), "--no-hosts"])
        assert args.no_hosts is True

    def test_prevalence_out_of_range(self, tmp_path):
        from yoinkc.cli import parse_args
        with pytest.raises(SystemExit):
            parse_args(["fleet", str(tmp_path), "-p", "0"])
        with pytest.raises(SystemExit):
            parse_args(["fleet", str(tmp_path), "-p", "101"])

    def test_json_only_flag(self, tmp_path):
        from yoinkc.cli import parse_args
        args = parse_args(["fleet", str(tmp_path), "--json-only"])
        assert args.json_only is True

    def test_output_dir_flag(self, tmp_path):
        from yoinkc.cli import parse_args
        args = parse_args(["fleet", str(tmp_path), "--output-dir", str(tmp_path)])
        assert args.output_dir == tmp_path


class TestFleetEndToEnd:
    def test_aggregate_produces_valid_snapshot(self, tmp_path):
        from yoinkc.fleet.__main__ import main
        _make_tarball(tmp_path, "web-01", ["httpd", "php"])
        _make_tarball(tmp_path, "web-02", ["httpd", "mod_ssl"])

        output = tmp_path / "merged.json"
        exit_code = main([str(tmp_path), "-o", str(output), "-p", "50", "--json-only"])
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

        exit_code = main([str(tmp_path), "--json-only"])
        assert exit_code == 0
        assert (tmp_path / "fleet-snapshot.json").exists()

    def test_aggregate_fewer_than_two_exits(self, tmp_path):
        from yoinkc.fleet.__main__ import main
        _make_tarball(tmp_path, "web-01", ["httpd"])
        exit_code = main([str(tmp_path)])
        assert exit_code == 1

    def test_aggregate_empty_dir_exits(self, tmp_path):
        from yoinkc.fleet.__main__ import main
        exit_code = main([str(tmp_path)])
        assert exit_code == 1

    def test_fleet_tarball_output(self, tmp_path):
        from yoinkc.fleet.__main__ import main
        _make_snapshot_json(tmp_path, "srv-01", ["nginx"])
        _make_snapshot_json(tmp_path, "srv-02", ["nginx", "curl"])

        tarball = tmp_path / "fleet.tar.gz"
        exit_code = main([str(tmp_path), "-o", str(tarball)])
        assert exit_code == 0
        assert tarball.exists()

        with tarfile.open(tarball, "r:gz") as tf:
            names = tf.getnames()
        assert any("Containerfile" in n for n in names)
        assert any("report.html" in n for n in names)
        assert any("inspection-snapshot.json" in n for n in names)

    def test_fleet_json_only(self, tmp_path):
        from yoinkc.fleet.__main__ import main
        _make_snapshot_json(tmp_path, "db-01", ["postgresql"])
        _make_snapshot_json(tmp_path, "db-02", ["postgresql", "pg_dump"])

        exit_code = main([str(tmp_path), "--json-only"])
        assert exit_code == 0
        assert (tmp_path / "fleet-snapshot.json").exists()
        assert not list(tmp_path.glob("*.tar.gz"))

    def test_fleet_output_dir(self, tmp_path):
        from yoinkc.fleet.__main__ import main
        _make_snapshot_json(tmp_path, "app-01", ["java"])
        _make_snapshot_json(tmp_path, "app-02", ["java", "tomcat"])

        out_dir = tmp_path / "rendered"
        out_dir.mkdir()
        exit_code = main([str(tmp_path), "--output-dir", str(out_dir)])
        assert exit_code == 0
        assert (out_dir / "Containerfile").exists()
        assert (out_dir / "report.html").exists()

    def test_output_and_output_dir_mutually_exclusive(self, tmp_path):
        from yoinkc.fleet.__main__ import main
        _make_snapshot_json(tmp_path, "mx-01", ["postfix"])
        _make_snapshot_json(tmp_path, "mx-02", ["postfix"])

        exit_code = main([str(tmp_path), "-o", "out.tar.gz", "--output-dir", str(tmp_path / "out")])
        assert exit_code == 1

    def test_json_only_and_output_dir_mutually_exclusive(self, tmp_path):
        from yoinkc.fleet.__main__ import main
        _make_snapshot_json(tmp_path, "mx-03", ["postfix"])
        _make_snapshot_json(tmp_path, "mx-04", ["postfix"])

        exit_code = main([str(tmp_path), "--json-only", "--output-dir", str(tmp_path / "out")])
        assert exit_code == 1

    def test_fleet_tarball_naming(self, tmp_path, monkeypatch):
        from yoinkc.fleet.__main__ import main
        snap_dir = tmp_path / "snapshots"
        snap_dir.mkdir()
        _make_snapshot_json(snap_dir, "cache-01", ["redis"])
        _make_snapshot_json(snap_dir, "cache-02", ["redis", "memcached"])

        # CWD intentionally differs from snap_dir to verify output lands in input_dir
        other_dir = tmp_path / "other"
        other_dir.mkdir()
        monkeypatch.chdir(other_dir)

        exit_code = main([str(snap_dir)])
        assert exit_code == 0

        tarballs = list(snap_dir.glob("*.tar.gz"))
        assert len(tarballs) == 1, "output tarball should be in the input directory"
        assert re.search(r".+-\d{8}-\d{6}\.tar\.gz", tarballs[0].name)
        assert not list(other_dir.glob("*.tar.gz")), "nothing should land in CWD"

    def test_fleet_main_cwd_override(self, tmp_path):
        from yoinkc.fleet.__main__ import main
        snap_dir = tmp_path / "snapshots"
        snap_dir.mkdir()
        _make_snapshot_json(snap_dir, "cache-01", ["redis"])
        _make_snapshot_json(snap_dir, "cache-02", ["redis", "memcached"])

        out_dir = tmp_path / "override"
        out_dir.mkdir()

        exit_code = main([str(snap_dir)], cwd=out_dir)
        assert exit_code == 0

        tarballs = list(out_dir.glob("*.tar.gz"))
        assert len(tarballs) == 1, "cwd override should control default output location"
        assert not list(snap_dir.glob("*.tar.gz")), "default output should move to cwd override"
