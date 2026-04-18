"""Tests for fleet snapshot loader."""

import json
import io
import tarfile
import tempfile
from pathlib import Path

import pytest

from inspectah.schema import InspectionSnapshot, OsRelease, SCHEMA_VERSION


class TestDiscoverSnapshots:
    """Test snapshot discovery from a directory."""

    def test_finds_json_files(self, tmp_path):
        from inspectah.fleet.loader import discover_snapshots
        snap = InspectionSnapshot(meta={"hostname": "web-01"})
        (tmp_path / "web-01.json").write_text(snap.model_dump_json())
        (tmp_path / "web-02.json").write_text(snap.model_dump_json())
        results = discover_snapshots(tmp_path)
        assert len(results) == 2

    def test_finds_tarballs(self, tmp_path):
        from inspectah.fleet.loader import discover_snapshots
        snap = InspectionSnapshot(meta={"hostname": "web-01"})
        tarball_path = tmp_path / "web-01.tar.gz"
        with tarfile.open(tarball_path, "w:gz") as tar:
            data = snap.model_dump_json().encode()
            info = tarfile.TarInfo(name="inspection-snapshot.json")
            info.size = len(data)
            tar.addfile(info, io.BytesIO(data))
        results = discover_snapshots(tmp_path)
        assert len(results) == 1

    def test_empty_directory_returns_empty(self, tmp_path):
        from inspectah.fleet.loader import discover_snapshots
        results = discover_snapshots(tmp_path)
        assert results == []

    def test_skips_non_snapshot_files(self, tmp_path):
        from inspectah.fleet.loader import discover_snapshots
        (tmp_path / "readme.txt").write_text("not a snapshot")
        (tmp_path / "data.csv").write_text("a,b,c")
        results = discover_snapshots(tmp_path)
        assert results == []

    def test_skips_fleet_snapshot_output(self, tmp_path):
        """Prevent re-runs from ingesting previous output as input."""
        from inspectah.fleet.loader import discover_snapshots
        snap = InspectionSnapshot(meta={"hostname": "web-01"})
        (tmp_path / "web-01.json").write_text(snap.model_dump_json())
        (tmp_path / "fleet-snapshot.json").write_text(snap.model_dump_json())
        results = discover_snapshots(tmp_path)
        assert len(results) == 1  # fleet-snapshot.json excluded

    def test_skips_invalid_json_with_warning(self, tmp_path):
        from inspectah.fleet.loader import discover_snapshots
        (tmp_path / "bad.json").write_text("not valid json {{{")
        with pytest.warns(UserWarning, match="Skipping invalid JSON"):
            results = discover_snapshots(tmp_path)
        assert results == []

    def test_skips_tarball_without_snapshot_json(self, tmp_path):
        from inspectah.fleet.loader import discover_snapshots
        tarball_path = tmp_path / "empty.tar.gz"
        with tarfile.open(tarball_path, "w:gz") as tar:
            data = b"FROM scratch"
            info = tarfile.TarInfo(name="Containerfile")
            info.size = len(data)
            tar.addfile(info, io.BytesIO(data))
        with pytest.warns(UserWarning, match="no inspection-snapshot.json"):
            results = discover_snapshots(tmp_path)
        assert results == []


class TestValidateSnapshots:
    """Test input validation across loaded snapshots."""

    def _make_snap(self, hostname="web-01", os_id="rhel", os_version="9.4",
                   base_image="quay.io/centos-bootc/centos-bootc:stream9"):
        from inspectah.schema import RpmSection
        return InspectionSnapshot(
            meta={"hostname": hostname},
            os_release=OsRelease(name="RHEL", version_id=os_version, id=os_id),
            rpm=RpmSection(base_image=base_image),
        )

    def test_valid_snapshots_pass(self):
        from inspectah.fleet.loader import validate_snapshots
        snaps = [self._make_snap("web-01"), self._make_snap("web-02")]
        validate_snapshots(snaps)  # should not raise

    def test_schema_version_mismatch_raises(self):
        from inspectah.fleet.loader import validate_snapshots
        s1 = self._make_snap("web-01")
        s2 = self._make_snap("web-02")
        s2.schema_version = 999
        with pytest.raises(SystemExit):
            validate_snapshots([s1, s2])

    def test_os_release_mismatch_raises(self):
        from inspectah.fleet.loader import validate_snapshots
        s1 = self._make_snap("web-01", os_id="rhel")
        s2 = self._make_snap("web-02", os_id="centos")
        with pytest.raises(SystemExit):
            validate_snapshots([s1, s2])

    def test_base_image_mismatch_raises(self):
        from inspectah.fleet.loader import validate_snapshots
        s1 = self._make_snap("web-01", base_image="image-a")
        s2 = self._make_snap("web-02", base_image="image-b")
        with pytest.raises(SystemExit):
            validate_snapshots([s1, s2])

    def test_fewer_than_two_raises(self):
        from inspectah.fleet.loader import validate_snapshots
        with pytest.raises(SystemExit):
            validate_snapshots([self._make_snap()])

    def test_duplicate_hostnames_warns(self):
        from inspectah.fleet.loader import validate_snapshots
        snaps = [self._make_snap("web-01"), self._make_snap("web-01")]
        with pytest.warns(UserWarning, match="Duplicate hostname"):
            validate_snapshots(snaps)  # warns but does not error

    def test_missing_os_release_raises(self):
        from inspectah.fleet.loader import validate_snapshots
        s1 = InspectionSnapshot(meta={"hostname": "web-01"})
        s2 = self._make_snap("web-02")
        with pytest.raises(SystemExit):
            validate_snapshots([s1, s2])


class TestComputeDisplayNames:
    @pytest.mark.parametrize(
        ("hostnames", "expected"),
        [
            (
                ["web-01.east.example.com", "web-02.west.example.com"],
                ["web-01", "web-02"],
            ),
            (
                ["web-01.east.example.com", "web-01.west.example.com"],
                ["web-01.east", "web-01.west"],
            ),
            (
                ["web-01.east.example.com", "web-01.east.internal.com"],
                ["web-01.east.example", "web-01.east.internal"],
            ),
            (
                ["web-01", "web-02"],
                ["web-01", "web-02"],
            ),
            (
                ["web-01", "web-01.east.example.com"],
                ["web-01", "web-01.east"],
            ),
            (
                ["web-01.example.com", "web-01.example.com"],
                ["web-01 (1)", "web-01 (2)"],
            ),
        ],
    )
    def test_progressively_disambiguates_colliding_hostnames(self, hostnames, expected):
        from inspectah.fleet.loader import compute_display_names

        assert compute_display_names(hostnames) == expected
