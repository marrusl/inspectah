"""Tests for RPM preflight check: schema, install set, and preflight module."""

from pathlib import Path
from yoinkc.executor import RunResult
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


class TestRepoProvidingPackages:
    def test_detects_repo_providing_packages(self, host_root, fixture_executor):
        """Packages that own .repo files in /etc/yum.repos.d/ are detected."""
        from yoinkc.inspectors.rpm import _detect_repo_providing_packages

        def executor(cmd, cwd=None):
            cmd_str = " ".join(cmd) if isinstance(cmd, list) else cmd
            if "rpm" in cmd_str and "-qf" in cmd_str and "yum.repos.d" in cmd_str:
                return RunResult(
                    stdout="epel-release\nepel-release\nrpmfusion-free-release\n",
                    stderr="", returncode=0,
                )
            return fixture_executor(cmd, cwd=cwd)

        result = _detect_repo_providing_packages(executor, host_root)
        assert "epel-release" in result
        assert "rpmfusion-free-release" in result

    def test_no_repo_files(self, tmp_path):
        """When no repo files exist, returns empty list."""
        from yoinkc.inspectors.rpm import _detect_repo_providing_packages

        def executor(cmd, cwd=None):
            return RunResult(stdout="", stderr="", returncode=1)

        result = _detect_repo_providing_packages(executor, tmp_path)
        assert result == []

    def test_rpm_qf_failure_returns_empty(self, host_root):
        """When rpm -qf fails, returns empty list gracefully."""
        from yoinkc.inspectors.rpm import _detect_repo_providing_packages

        def executor(cmd, cwd=None):
            return RunResult(stdout="", stderr="error", returncode=1)

        result = _detect_repo_providing_packages(executor, host_root)
        assert result == []


# --- Preflight module tests ---

from datetime import datetime, timezone
from unittest.mock import patch, MagicMock

from yoinkc.rpm_preflight import run_package_preflight
from yoinkc.schema import PackageEntry, PackageState, RepoFile


def _make_preflight_snapshot(
    packages=None,
    base_image="quay.io/fedora/fedora-bootc:44",
    repo_providing_packages=None,
):
    """Build a snapshot suitable for preflight testing."""
    entries = []
    for name in (packages or []):
        entries.append(PackageEntry(
            name=name, epoch="0", version="1.0", release="1.fc44",
            arch="x86_64", state=PackageState.ADDED, include=True, source_repo="baseos",
        ))
    section = RpmSection(
        packages_added=entries, no_baseline=True, base_image=base_image,
        repo_providing_packages=repo_providing_packages or [],
    )
    return InspectionSnapshot(rpm=section)


def _make_preflight_executor(
    repoquery_stdout="",
    repoquery_rc=0,
    repoquery_stderr="",
    pull_rc=0,
    bootstrap_rc=0,
    repos_stdout="fedora\nupdates\n",
):
    """Build a mock executor for preflight subprocess calls.
    repoquery_stdout should contain plain package names (one per line),
    matching the --queryformat "%{name}" output format.
    """
    def executor(cmd, cwd=None):
        cmd_str = " ".join(cmd) if isinstance(cmd, list) else cmd
        if "podman" in cmd_str and "pull" in cmd_str:
            return RunResult(stdout="", stderr="" if pull_rc == 0 else "pull failed", returncode=pull_rc)
        if "podman" in cmd_str and "run" in cmd_str and "dnf install" in cmd_str:
            return RunResult(stdout="", stderr="" if bootstrap_rc == 0 else "install failed", returncode=bootstrap_rc)
        if "podman" in cmd_str and "run" in cmd_str and "repoquery" in cmd_str:
            return RunResult(stdout=repoquery_stdout, stderr=repoquery_stderr, returncode=repoquery_rc)
        if "podman" in cmd_str and "run" in cmd_str and "repolist" in cmd_str:
            return RunResult(stdout=repos_stdout, stderr="", returncode=0)
        if "podman" in cmd_str and "rm" in cmd_str:
            return RunResult(stdout="", stderr="", returncode=0)
        return RunResult(stdout="", stderr="unknown cmd", returncode=1)
    return executor


class TestPackagePreflight:
    def test_all_available(self):
        snapshot = _make_preflight_snapshot(packages=["httpd", "nginx"])
        executor = _make_preflight_executor(repoquery_stdout="httpd\nnginx\n")
        result = run_package_preflight(snapshot=snapshot, executor=executor)
        assert result.status == "completed"
        assert sorted(result.available) == ["httpd", "nginx"]
        assert result.unavailable == []

    def test_some_unavailable(self):
        snapshot = _make_preflight_snapshot(packages=["httpd", "mcelog", "nginx"])
        executor = _make_preflight_executor(repoquery_stdout="httpd\nnginx\n")
        result = run_package_preflight(snapshot=snapshot, executor=executor)
        assert result.status == "completed"
        assert result.unavailable == ["mcelog"]
        assert sorted(result.available) == ["httpd", "nginx"]

    def test_base_image_pull_fails(self):
        snapshot = _make_preflight_snapshot(packages=["httpd"])
        executor = _make_preflight_executor(pull_rc=1)
        result = run_package_preflight(snapshot=snapshot, executor=executor)
        assert result.status == "failed"
        assert "pull" in result.status_reason.lower()

    def test_direct_install_excluded(self):
        entries = [
            PackageEntry(name="httpd", epoch="0", version="1.0", release="1", arch="x86_64", source_repo="baseos"),
            PackageEntry(name="custom-agent", epoch="0", version="1.0", release="1", arch="x86_64", source_repo=""),
            PackageEntry(name="local-tool", epoch="0", version="1.0", release="1", arch="x86_64", source_repo="(none)"),
        ]
        snapshot = InspectionSnapshot(rpm=RpmSection(
            packages_added=entries, no_baseline=True, base_image="quay.io/fedora/fedora-bootc:44",
        ))
        executor = _make_preflight_executor(repoquery_stdout="httpd\n")
        result = run_package_preflight(snapshot=snapshot, executor=executor)
        assert sorted(result.direct_install) == ["custom-agent", "local-tool"]
        assert "custom-agent" not in result.available
        assert "custom-agent" not in result.unavailable

    def test_repo_provider_bootstrap_failure_classifies_correctly(self):
        snapshot = _make_preflight_snapshot(
            packages=["httpd", "some-epel-pkg"],
            repo_providing_packages=["epel-release"],
        )
        snapshot.rpm.packages_added[0].source_repo = "baseos"
        snapshot.rpm.packages_added[1].source_repo = "epel"
        snapshot.rpm.repo_files = [
            RepoFile(path="etc/yum.repos.d/epel.repo", content="[epel]\nname=EPEL\n", is_default_repo=False),
        ]
        executor = _make_preflight_executor(bootstrap_rc=1, repoquery_stdout="httpd\n")
        result = run_package_preflight(snapshot=snapshot, executor=executor)
        assert result.status == "partial"
        assert "epel-release" in result.status_reason
        assert "httpd" in result.available
        unverifiable_names = [uv.name for uv in result.unverifiable]
        assert "some-epel-pkg" in unverifiable_names
        assert "some-epel-pkg" not in result.unavailable

    def test_repo_unreachable_detected(self):
        snapshot = _make_preflight_snapshot(packages=["httpd", "internal-app"])
        executor = _make_preflight_executor(
            repoquery_stdout="httpd\n",
            repoquery_stderr="Failed to synchronize cache for repo 'internal-mirror': connection timed out\n",
        )
        result = run_package_preflight(snapshot=snapshot, executor=executor)
        assert result.status == "partial"
        assert len(result.repo_unreachable) == 1
        assert result.repo_unreachable[0].repo_id == "internal-mirror"

    def test_empty_install_set_returns_completed(self):
        snapshot = InspectionSnapshot(rpm=RpmSection(base_image="quay.io/fedora/fedora-bootc:44"))
        result = run_package_preflight(snapshot=snapshot, executor=_make_preflight_executor())
        assert result.status == "completed"
        assert result.available == []

    def test_no_base_image_returns_failed(self):
        snapshot = _make_preflight_snapshot(packages=["httpd"])
        snapshot.rpm.base_image = None
        result = run_package_preflight(snapshot=snapshot, executor=_make_preflight_executor())
        assert result.status == "failed"
        assert "base image" in result.status_reason.lower()

    def test_synthetic_tuned_not_classified_as_direct_install(self):
        """Synthetic tuned (from resolve_install_set, not in packages_added)
        must go through repoquery, not be classified as direct-install."""
        from yoinkc.schema import KernelBootSection

        snapshot = _make_preflight_snapshot(packages=["httpd"])
        snapshot.rpm.packages_added[0].source_repo = "baseos"
        snapshot.kernel_boot = KernelBootSection(tuned_active="throughput-performance")
        # tuned is NOT in packages_added, but resolve_install_set will inject it

        executor = _make_preflight_executor(repoquery_stdout="httpd\ntuned\n")
        result = run_package_preflight(snapshot=snapshot, executor=executor)

        assert "tuned" not in result.direct_install
        assert "tuned" in result.available
