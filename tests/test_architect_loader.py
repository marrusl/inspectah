"""Tests for architect fleet tarball loader."""

import json
import sys
import tarfile
import pytest
from io import BytesIO
from pathlib import Path

from yoinkc.architect.loader import load_refined_fleets
from yoinkc.architect.analyzer import FleetInput


def _make_snapshot(hostname: str, packages: list[str], fleet_name: str = "test-fleet") -> dict:
    return {
        "schema_version": 6,
        "meta": {
            "hostname": hostname,
            "fleet": {
                "source_hosts": [hostname],
                "total_hosts": 3,
            },
        },
        "os_release": {"name": "RHEL", "version_id": "9.4", "id": "rhel"},
        "rpm": {
            "base_image": "registry.redhat.io/rhel9/rhel-bootc:9.4",
            "packages_added": [
                {"name": pkg, "nvra": f"{pkg}-1.0-1.el9.x86_64", "source": "dnf"}
                for pkg in packages
            ],
        },
        "config": {"files": [{"path": "/etc/test.conf", "content": "test"}]},
    }


def _write_tarball(directory: Path, name: str, snapshot: dict) -> Path:
    tarball_path = directory / f"{name}.tar.gz"
    snap_json = json.dumps(snapshot).encode()
    with tarfile.open(tarball_path, "w:gz") as tar:
        info = tarfile.TarInfo(name="inspection-snapshot.json")
        info.size = len(snap_json)
        tar.addfile(info, BytesIO(snap_json))
    return tarball_path


class TestLoadRefinedFleets:
    def test_loads_tarballs_from_directory(self, tmp_path):
        snap1 = _make_snapshot("web-fleet", ["httpd", "openssl"])
        snap2 = _make_snapshot("db-fleet", ["postgresql", "openssl"])
        _write_tarball(tmp_path, "web-fleet", snap1)
        _write_tarball(tmp_path, "db-fleet", snap2)

        fleets = load_refined_fleets(tmp_path)
        assert len(fleets) == 2
        assert all(isinstance(f, FleetInput) for f in fleets)

    def test_fleet_name_from_hostname(self, tmp_path):
        snap = _make_snapshot("web-servers", ["httpd"])
        _write_tarball(tmp_path, "web-servers", snap)

        fleets = load_refined_fleets(tmp_path)
        assert fleets[0].name == "web-servers"

    def test_packages_extracted(self, tmp_path):
        snap = _make_snapshot("web", ["httpd", "openssl", "bash"])
        _write_tarball(tmp_path, "web", snap)

        fleets = load_refined_fleets(tmp_path)
        assert set(fleets[0].packages) == {"httpd-1.0-1.el9.x86_64", "openssl-1.0-1.el9.x86_64", "bash-1.0-1.el9.x86_64"}

    def test_host_count_from_fleet_meta(self, tmp_path):
        snap = _make_snapshot("web", ["httpd"])
        snap["meta"]["fleet"]["total_hosts"] = 42
        _write_tarball(tmp_path, "web", snap)

        fleets = load_refined_fleets(tmp_path)
        assert fleets[0].host_count == 42

    def test_configs_extracted(self, tmp_path):
        snap = _make_snapshot("web", ["httpd"])
        _write_tarball(tmp_path, "web", snap)

        fleets = load_refined_fleets(tmp_path)
        assert len(fleets[0].configs) >= 1

    def test_base_image_extracted(self, tmp_path):
        snap = _make_snapshot("web", ["httpd"])
        _write_tarball(tmp_path, "web", snap)

        fleets = load_refined_fleets(tmp_path)
        assert fleets[0].base_image == "registry.redhat.io/rhel9/rhel-bootc:9.4"

    def test_base_image_defaults_to_empty(self, tmp_path):
        snap = _make_snapshot("web", ["httpd"])
        del snap["rpm"]["base_image"]
        _write_tarball(tmp_path, "web", snap)

        fleets = load_refined_fleets(tmp_path)
        assert fleets[0].base_image == ""

    def test_empty_directory(self, tmp_path):
        fleets = load_refined_fleets(tmp_path)
        assert fleets == []

    def test_skips_non_tarball_files(self, tmp_path):
        (tmp_path / "readme.md").write_text("not a tarball")
        snap = _make_snapshot("web", ["httpd"])
        _write_tarball(tmp_path, "web", snap)

        fleets = load_refined_fleets(tmp_path)
        assert len(fleets) == 1

    def test_loads_from_tarball_of_tarballs(self, tmp_path):
        """A .tar.gz bundle containing fleet tarballs should be extractable
        and loadable via the standard loader path."""
        # Create fleet tarballs in a staging directory
        staging = tmp_path / "staging"
        staging.mkdir()
        snap1 = _make_snapshot("web-fleet", ["httpd", "openssl"])
        snap2 = _make_snapshot("db-fleet", ["postgresql", "openssl"])
        _write_tarball(staging, "web-fleet", snap1)
        _write_tarball(staging, "db-fleet", snap2)

        # Bundle them into a tarball-of-tarballs
        bundle_path = tmp_path / "architect-demo-bundle.tar.gz"
        with tarfile.open(bundle_path, "w:gz") as bundle:
            for child in sorted(staging.iterdir()):
                bundle.add(child, arcname=child.name)

        # Extract and load through the normal loader
        extract_dir = tmp_path / "extracted"
        extract_dir.mkdir()
        with tarfile.open(bundle_path, "r:gz") as bundle:
            if sys.version_info >= (3, 12):
                bundle.extractall(extract_dir, filter="data")
            else:
                bundle.extractall(extract_dir)

        fleets = load_refined_fleets(extract_dir)
        assert len(fleets) == 2
        names = {f.name for f in fleets}
        assert names == {"web-fleet", "db-fleet"}
