"""Tests for RPM preflight check: schema, install set, and preflight module."""

from yoinkc.schema import (
    InspectionSnapshot,
    PreflightResult,
    RepoStatus,
    RpmSection,
    UnverifiablePackage,
)


class TestPreflightSchema:
    def test_preflight_result_completed(self):
        result = PreflightResult(
            status="completed",
            available=["httpd", "nginx"],
            unavailable=["mcelog"],
            direct_install=["custom-agent"],
            base_image="quay.io/fedora/fedora-bootc:44",
            repos_queried=["fedora", "updates"],
            timestamp="2026-04-09T17:00:00Z",
        )
        assert result.status == "completed"
        assert result.status_reason is None
        assert result.unavailable == ["mcelog"]
        assert result.unverifiable == []
        assert result.repo_unreachable == []

    def test_preflight_result_partial_with_unverifiable(self):
        result = PreflightResult(
            status="partial",
            status_reason="repo-providing package epel-release unavailable",
            available=["httpd"],
            unavailable=["mcelog"],
            unverifiable=[
                UnverifiablePackage(
                    name="some-epel-pkg",
                    reason="repo-providing package epel-release unavailable",
                )
            ],
            base_image="quay.io/fedora/fedora-bootc:44",
            repos_queried=["fedora", "updates"],
            timestamp="2026-04-09T17:00:00Z",
        )
        assert result.status == "partial"
        assert len(result.unverifiable) == 1
        assert result.unverifiable[0].name == "some-epel-pkg"

    def test_preflight_result_skipped(self):
        result = PreflightResult(
            status="skipped",
            status_reason="user passed --skip-unavailable",
        )
        assert result.status == "skipped"
        assert result.available == []

    def test_preflight_result_failed(self):
        result = PreflightResult(
            status="failed",
            status_reason="base image could not be pulled",
        )
        assert result.status == "failed"

    def test_repo_status(self):
        rs = RepoStatus(
            repo_id="internal-mirror",
            repo_name="Internal Mirror",
            error="connection timed out",
            affected_packages=["internal-app", "internal-lib"],
        )
        assert rs.repo_id == "internal-mirror"
        assert len(rs.affected_packages) == 2

    def test_snapshot_has_preflight_field(self):
        snapshot = InspectionSnapshot()
        assert snapshot.preflight is not None
        assert snapshot.preflight.status == "skipped"

    def test_snapshot_preflight_roundtrip(self):
        """Preflight data survives JSON serialization/deserialization."""
        snapshot = InspectionSnapshot(
            preflight=PreflightResult(
                status="completed",
                available=["httpd"],
                unavailable=["mcelog"],
                direct_install=["custom-agent"],
                base_image="quay.io/fedora/fedora-bootc:44",
                repos_queried=["fedora"],
                timestamp="2026-04-09T17:00:00Z",
            )
        )
        json_str = snapshot.model_dump_json()
        loaded = InspectionSnapshot.model_validate_json(json_str)
        assert loaded.preflight.status == "completed"
        assert loaded.preflight.unavailable == ["mcelog"]
        assert loaded.preflight.direct_install == ["custom-agent"]

    def test_rpm_section_has_repo_providing_packages(self):
        section = RpmSection()
        assert section.repo_providing_packages == []

    def test_rpm_section_repo_providing_packages_roundtrip(self):
        section = RpmSection(repo_providing_packages=["epel-release", "rpmfusion-free-release"])
        data = section.model_dump()
        loaded = RpmSection.model_validate(data)
        assert loaded.repo_providing_packages == ["epel-release", "rpmfusion-free-release"]
