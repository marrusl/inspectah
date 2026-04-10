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

    Matches the persistent container pattern:
      podman pull → pull response
      podman run -d → container name (stdout)
      podman exec ... dnf install → bootstrap response
      podman exec ... repoquery → repoquery response
      podman exec ... repolist → repolist response
      podman rm → cleanup response

    repoquery_stdout should contain plain package names (one per line),
    matching the --queryformat "%{name}" output format.
    """
    def executor(cmd, cwd=None):
        cmd_str = " ".join(cmd) if isinstance(cmd, list) else cmd
        if "podman" in cmd_str and "pull" in cmd_str:
            return RunResult(stdout="", stderr="" if pull_rc == 0 else "pull failed", returncode=pull_rc)
        if "podman" in cmd_str and "run" in cmd_str and "-d" in cmd_str:
            # Persistent container creation
            return RunResult(stdout="yoinkc-preflight-test1234\n", stderr="", returncode=0)
        if "podman" in cmd_str and "exec" in cmd_str and "dnf install" in cmd_str:
            return RunResult(stdout="", stderr="" if bootstrap_rc == 0 else "install failed", returncode=bootstrap_rc)
        if "podman" in cmd_str and "exec" in cmd_str and "repoquery" in cmd_str:
            return RunResult(stdout=repoquery_stdout, stderr=repoquery_stderr, returncode=repoquery_rc)
        if "podman" in cmd_str and "exec" in cmd_str and "repolist" in cmd_str:
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

    def test_unreachable_repo_packages_not_in_unavailable(self):
        """Packages from unreachable repos stay OUT of unavailable
        (they remain in the Containerfile with a warning)."""
        snapshot = _make_preflight_snapshot(packages=["httpd", "internal-app"])
        snapshot.rpm.packages_added[1].source_repo = "internal-mirror"
        executor = _make_preflight_executor(
            repoquery_stdout="httpd\n",
            repoquery_stderr="Failed to synchronize cache for repo 'internal-mirror': connection timed out\n",
        )
        result = run_package_preflight(snapshot=snapshot, executor=executor)
        assert result.status == "partial"
        assert "internal-app" not in result.unavailable  # NOT excluded
        assert "httpd" in result.available
        assert result.repo_unreachable[0].repo_id == "internal-mirror"

    def test_all_repos_unreachable_returns_failed(self):
        """When ALL repos are unreachable, status is 'failed', not 'partial'."""
        snapshot = _make_preflight_snapshot(packages=["httpd"])
        snapshot.rpm.packages_added[0].source_repo = "fedora"
        executor = _make_preflight_executor(
            repoquery_stdout="",
            repoquery_rc=1,
            repoquery_stderr="Failed to synchronize cache for repo 'fedora': connection timed out\nFailed to synchronize cache for repo 'updates': timeout\n",
            repos_stdout="",
        )
        result = run_package_preflight(snapshot=snapshot, executor=executor)
        assert result.status == "failed"
        assert "all repos unreachable" in result.status_reason.lower() or "no meaningful" in result.status_reason.lower()

    def test_persistent_container_used(self):
        """Preflight uses podman run -d + exec pattern (not --rm)."""
        commands_seen = []

        def tracking_executor(cmd, cwd=None):
            cmd_str = " ".join(cmd) if isinstance(cmd, list) else cmd
            commands_seen.append(cmd_str)
            if "podman" in cmd_str and "pull" in cmd_str:
                return RunResult(stdout="", stderr="", returncode=0)
            if "podman" in cmd_str and "run" in cmd_str and "-d" in cmd_str:
                return RunResult(stdout="test-container\n", stderr="", returncode=0)
            if "podman" in cmd_str and "exec" in cmd_str and "repoquery" in cmd_str:
                return RunResult(stdout="httpd\n", stderr="", returncode=0)
            if "podman" in cmd_str and "exec" in cmd_str and "repolist" in cmd_str:
                return RunResult(stdout="fedora\n", stderr="", returncode=0)
            if "podman" in cmd_str and "rm" in cmd_str:
                return RunResult(stdout="", stderr="", returncode=0)
            return RunResult(stdout="", stderr="", returncode=0)

        snapshot = _make_preflight_snapshot(packages=["httpd"])
        result = run_package_preflight(snapshot=snapshot, executor=tracking_executor)
        assert result.status == "completed"

        # Verify persistent container pattern
        run_cmds = [c for c in commands_seen if "podman" in c and "run" in c]
        assert any("-d" in c for c in run_cmds), "Expected podman run -d for persistent container"
        assert not any("--rm" in c for c in run_cmds), "Should not use --rm with persistent container"
        exec_cmds = [c for c in commands_seen if "podman" in c and "exec" in c]
        assert len(exec_cmds) >= 1, "Expected at least one podman exec command"
        rm_cmds = [c for c in commands_seen if "podman" in c and " rm " in c]
        assert len(rm_cmds) == 1, "Expected one podman rm -f for cleanup"

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


class TestPreflightIntegration:
    def test_skip_unavailable_sets_skipped(self, fixture_executor, host_root):
        """When skip_unavailable=True, snapshot.preflight.status is 'skipped'."""
        from yoinkc.inspectors import run_all

        snapshot = run_all(
            host_root,
            executor=fixture_executor,
            no_baseline_opt_in=True,
            skip_unavailable=True,
        )
        assert snapshot.preflight.status == "skipped"
        assert "skip-unavailable" in snapshot.preflight.status_reason

    def test_preflight_runs_after_all_inspectors(self, fixture_executor, host_root):
        """Preflight runs after all inspectors, so it sees config and kernel_boot."""
        from unittest.mock import patch

        preflight_snapshot_state = {}

        def spy_preflight(*, snapshot, executor):
            # Record what snapshot state preflight sees at call time
            preflight_snapshot_state['has_config'] = snapshot.config is not None
            preflight_snapshot_state['has_kernel_boot'] = snapshot.kernel_boot is not None
            preflight_snapshot_state['has_services'] = snapshot.services is not None
            # Return a minimal result so preflight doesn't actually run podman
            return PreflightResult(status="failed", status_reason="spy")

        with patch('yoinkc.rpm_preflight.run_package_preflight', side_effect=spy_preflight):
            from yoinkc.inspectors import run_all

            snapshot = run_all(
                host_root,
                executor=fixture_executor,
                no_baseline_opt_in=True,
            )

        # Preflight should have seen all inspector outputs
        assert preflight_snapshot_state.get('has_config') is True
        assert preflight_snapshot_state.get('has_kernel_boot') is True
        assert preflight_snapshot_state.get('has_services') is True


from yoinkc.renderers.containerfile.packages import section_lines


class TestRendererPreflightConsumption:
    def _make_renderer_snapshot(self, packages, unavailable=None, direct_install=None, unverifiable=None):
        """Build a snapshot with preflight data for renderer testing."""
        entries = []
        for name in packages:
            entries.append(PackageEntry(
                name=name, epoch="0", version="1.0", release="1",
                arch="x86_64", state=PackageState.ADDED, include=True,
                source_repo="baseos",
            ))
        section = RpmSection(
            packages_added=entries, no_baseline=True,
            base_image="quay.io/fedora/fedora-bootc:44",
        )
        preflight = PreflightResult(
            status="completed",
            available=[p for p in packages if p not in (unavailable or []) and p not in (direct_install or [])],
            unavailable=unavailable or [],
            direct_install=direct_install or [],
            unverifiable=[UnverifiablePackage(name=n, reason="test") for n in (unverifiable or [])],
            base_image="quay.io/fedora/fedora-bootc:44",
            repos_queried=["fedora", "updates"],
            timestamp="2026-04-09T17:00:00Z",
        )
        return InspectionSnapshot(rpm=section, preflight=preflight)

    def test_unavailable_excluded_from_dnf_install(self):
        """Unavailable packages are NOT in the dnf install line."""
        snapshot = self._make_renderer_snapshot(
            packages=["httpd", "mcelog", "nginx"], unavailable=["mcelog"],
        )
        lines = section_lines(
            snapshot, base="quay.io/fedora/fedora-bootc:44",
            c_ext_pip=[], needs_multistage=False,
        )
        install_block = [l.strip().rstrip(" \\") for l in lines
                         if l.startswith("    ") and not l.strip().startswith("#")
                         and not l.strip().startswith("&&")]
        assert "mcelog" not in install_block
        assert "httpd" in install_block
        assert "nginx" in install_block

    def test_direct_install_excluded_from_dnf_install(self):
        """Direct-install RPMs are NOT in the dnf install line."""
        snapshot = self._make_renderer_snapshot(
            packages=["httpd", "custom-agent"], direct_install=["custom-agent"],
        )
        lines = section_lines(
            snapshot, base="quay.io/fedora/fedora-bootc:44",
            c_ext_pip=[], needs_multistage=False,
        )
        install_block = [l.strip().rstrip(" \\") for l in lines
                         if l.startswith("    ") and not l.strip().startswith("#")
                         and not l.strip().startswith("&&")]
        assert "custom-agent" not in install_block

    def test_skipped_preflight_includes_all(self):
        """With preflight skipped, all packages are included."""
        entries = [
            PackageEntry(name=n, epoch="0", version="1.0", release="1",
                         arch="x86_64", state=PackageState.ADDED, include=True)
            for n in ["httpd", "mcelog", "nginx"]
        ]
        snapshot = InspectionSnapshot(
            rpm=RpmSection(packages_added=entries, no_baseline=True,
                           base_image="quay.io/fedora/fedora-bootc:44"),
            preflight=PreflightResult(status="skipped",
                                      status_reason="user passed --skip-unavailable"),
        )
        lines = section_lines(
            snapshot, base="quay.io/fedora/fedora-bootc:44",
            c_ext_pip=[], needs_multistage=False,
        )
        joined = "\n".join(lines)
        assert "httpd" in joined
        assert "mcelog" in joined
        assert "nginx" in joined


from yoinkc.architect.analyzer import FleetInput


class TestArchitectPreflightAggregation:
    def test_fleet_input_has_preflight_fields(self):
        fi = FleetInput(
            name="fleet-1", packages=["httpd"], configs=[],
            unavailable_packages=["mcelog"],
            direct_install_packages=["custom-agent"],
            preflight_status="completed",
            base_image="quay.io/fedora/fedora-bootc:44",
        )
        assert fi.unavailable_packages == ["mcelog"]
        assert fi.direct_install_packages == ["custom-agent"]
        assert fi.preflight_status == "completed"


class TestEndToEnd:
    def test_preflight_roundtrip_via_snapshot(self, tmp_path):
        """Preflight data survives: inspect -> save snapshot -> load -> render."""
        from yoinkc.pipeline import save_snapshot, load_snapshot

        snapshot = InspectionSnapshot(
            rpm=RpmSection(
                packages_added=[
                    PackageEntry(name="httpd", epoch="0", version="1.0",
                                 release="1", arch="x86_64", source_repo="baseos"),
                    PackageEntry(name="mcelog", epoch="0", version="1.0",
                                 release="1", arch="x86_64", source_repo="baseos"),
                ],
                base_image="quay.io/fedora/fedora-bootc:44",
                no_baseline=True,
            ),
            preflight=PreflightResult(
                status="completed",
                available=["httpd"],
                unavailable=["mcelog"],
                base_image="quay.io/fedora/fedora-bootc:44",
                repos_queried=["fedora"],
                timestamp="2026-04-09T17:00:00Z",
            ),
        )

        # Save and reload
        path = tmp_path / "snapshot.json"
        save_snapshot(snapshot, path)
        loaded = load_snapshot(path)

        assert loaded.preflight.status == "completed"
        assert loaded.preflight.unavailable == ["mcelog"]
        assert loaded.preflight.available == ["httpd"]

        # Render — mcelog should be excluded
        lines = section_lines(
            loaded, base="quay.io/fedora/fedora-bootc:44",
            c_ext_pip=[], needs_multistage=False,
        )
        install_block = [l.strip().rstrip(" \\") for l in lines
                         if l.startswith("    ") and not l.strip().startswith("#")
                         and not l.strip().startswith("&&")]
        assert "mcelog" not in install_block
        assert "httpd" in install_block

    def test_skip_unavailable_preserves_all_packages(self):
        """With skipped preflight, renderer includes all packages."""
        snapshot = InspectionSnapshot(
            rpm=RpmSection(
                packages_added=[
                    PackageEntry(name="httpd", epoch="0", version="1.0",
                                 release="1", arch="x86_64"),
                    PackageEntry(name="mcelog", epoch="0", version="1.0",
                                 release="1", arch="x86_64"),
                ],
                base_image="quay.io/fedora/fedora-bootc:44",
                no_baseline=True,
            ),
            preflight=PreflightResult(
                status="skipped",
                status_reason="user passed --skip-unavailable",
            ),
        )
        lines = section_lines(
            snapshot, base="quay.io/fedora/fedora-bootc:44",
            c_ext_pip=[], needs_multistage=False,
        )
        joined = "\n".join(lines)
        assert "httpd" in joined
        assert "mcelog" in joined
