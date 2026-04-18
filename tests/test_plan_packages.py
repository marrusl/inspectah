"""Plan item tests: packages, repos, dependency classification, version patterns."""

import tempfile
from pathlib import Path

import pytest

from inspectah.schema import (
    EnabledModuleStream,
    FleetPrevalence,
    InspectionSnapshot,
    NonRpmItem,
    NonRpmSoftwareSection,
    OsRelease,
    PackageEntry,
    RepoFile,
    RpmSection,
    VersionLockEntry,
)
from inspectah.renderers.containerfile import render as render_containerfile
from inspectah.renderers.audit_report import render as render_audit

from conftest import _env


class TestLeafAutoSlimming:

    def test_only_leaf_packages_in_dnf_install(self):
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.6", id="rhel"),
            rpm=RpmSection(
                base_image="registry.redhat.io/rhel9/rhel-bootc:9.6",
                packages_added=[
                    PackageEntry(name="httpd", epoch="0", version="2.4", release="1", arch="x86_64"),
                    PackageEntry(name="httpd-core", epoch="0", version="2.4", release="1", arch="x86_64"),
                    PackageEntry(name="httpd-filesystem", epoch="0", version="2.4", release="1", arch="x86_64"),
                    PackageEntry(name="apr", epoch="0", version="1.7", release="1", arch="x86_64"),
                    PackageEntry(name="apr-util", epoch="0", version="1.6", release="1", arch="x86_64"),
                    PackageEntry(name="nginx", epoch="0", version="1.24", release="1", arch="x86_64"),
                ],
                leaf_packages=["httpd", "nginx"],
                auto_packages=["apr", "apr-util", "httpd-core", "httpd-filesystem"],
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_containerfile(snapshot, _env(), Path(tmp))
            cf = (Path(tmp) / "Containerfile").read_text()
        install_block = cf.split("dnf install")[1].split("dnf clean")[0]
        assert "httpd" in install_block
        assert "nginx" in install_block
        assert "apr" not in install_block
        assert "httpd-core" not in install_block
        assert "4 additional package" in cf

    def test_fallback_when_no_leaf_data(self):
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.6", id="rhel"),
            rpm=RpmSection(
                base_image="registry.redhat.io/rhel9/rhel-bootc:9.6",
                packages_added=[
                    PackageEntry(name="httpd", epoch="0", version="2.4", release="1", arch="x86_64"),
                    PackageEntry(name="apr", epoch="0", version="1.7", release="1", arch="x86_64"),
                ],
                leaf_packages=None,
                auto_packages=None,
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_containerfile(snapshot, _env(), Path(tmp))
            cf = (Path(tmp) / "Containerfile").read_text()
        assert "httpd" in cf
        assert "apr" in cf
        assert "additional package" not in cf

    def test_audit_report_shows_both_groups(self):
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.6", id="rhel"),
            rpm=RpmSection(
                base_image="registry.redhat.io/rhel9/rhel-bootc:9.6",
                packages_added=[
                    PackageEntry(name="httpd", epoch="0", version="2.4", release="1", arch="x86_64"),
                    PackageEntry(name="apr", epoch="0", version="1.7", release="1", arch="x86_64"),
                ],
                leaf_packages=["httpd"],
                auto_packages=["apr"],
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_audit(snapshot, _env(), Path(tmp))
            report = (Path(tmp) / "audit-report.md").read_text()
        assert "Explicitly installed" in report
        assert "Dependencies" in report
        assert "httpd" in report
        assert "apr" in report


class TestSourceRepo:
    """PackageEntry.source_repo field."""

    def test_source_repo_field_populated(self):
        p = PackageEntry(name="htop", version="3.2", release="1", arch="x86_64", source_repo="epel")
        assert p.source_repo == "epel"
        d = p.model_dump()
        assert d["source_repo"] == "epel"
        p2 = PackageEntry.model_validate(d)
        assert p2.source_repo == "epel"

    def test_source_repo_defaults_empty(self):
        p = PackageEntry(name="x", version="1", release="1", arch="x86_64")
        assert p.source_repo == ""


class TestRepoFileClassification:
    """is_default_repo classification logic."""

    def test_default_repo_redhat(self):
        from inspectah.inspectors.rpm import _classify_default_repo
        rf = RepoFile(path="etc/yum.repos.d/redhat.repo", content="[rhel-baseos]\nbaseurl=http://x\n")
        assert _classify_default_repo(rf) is True

    def test_non_default_repo_epel(self):
        from inspectah.inspectors.rpm import _classify_default_repo
        rf = RepoFile(path="etc/yum.repos.d/epel.repo", content="[epel]\nbaseurl=http://x\n")
        assert _classify_default_repo(rf) is False

    def test_default_repo_appstream_section(self):
        from inspectah.inspectors.rpm import _classify_default_repo
        rf = RepoFile(path="etc/yum.repos.d/centos.repo", content="[appstream]\nbaseurl=http://x\n")
        assert _classify_default_repo(rf) is True

    def test_non_default_repo_copr(self):
        from inspectah.inspectors.rpm import _classify_default_repo
        rf = RepoFile(path="etc/yum.repos.d/copr-myrepo.repo", content="[copr:user:project]\nbaseurl=http://x\n")
        assert _classify_default_repo(rf) is False

    def test_default_repo_fedora_section(self):
        from inspectah.inspectors.rpm import _classify_default_repo
        rf = RepoFile(path="etc/yum.repos.d/fedora.repo", content="[fedora]\nbaseurl=http://x\n")
        assert _classify_default_repo(rf) is True


class TestRepoCascadeContainerfile:
    """When repo include=False and its packages also have include=False, both are excluded."""

    def test_excluded_repo_and_its_packages(self):
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.6", id="rhel"),
            rpm=RpmSection(
                base_image="registry.redhat.io/rhel9/rhel-bootc:9.6",
                packages_added=[
                    PackageEntry(name="httpd", version="2.4", release="1", arch="x86_64", source_repo="baseos"),
                    PackageEntry(name="htop", version="3.2", release="1", arch="x86_64", source_repo="epel", include=False),
                ],
                leaf_packages=["httpd", "htop"],
                auto_packages=[],
                repo_files=[
                    RepoFile(path="etc/yum.repos.d/epel.repo", content="[epel]\nbaseurl=http://x\n",
                             is_default_repo=False, include=False),
                ],
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_containerfile(snapshot, _env(), Path(tmp))
            cf = (Path(tmp) / "Containerfile").read_text()
        assert "httpd" in cf
        assert "htop" not in cf
        assert "# Excluded repo: etc/yum.repos.d/epel.repo" in cf
        assert not (Path(tmp) / "config" / "etc" / "yum.repos.d" / "epel.repo").exists()


class TestDeepVersionPatterns:

    def _match(self, data: bytes, expected: bytes):
        from inspectah.inspectors.non_rpm_software import DEEP_VERSION_PATTERNS
        for pat in DEEP_VERSION_PATTERNS:
            m = pat.search(data)
            if m and m.group(1) == expected:
                return
        pytest.fail(f"No pattern matched {data!r} → {expected!r}")

    def test_go(self):
        self._match(b"go1.21.5 linux/amd64", b"1.21.5")

    def test_rust(self):
        self._match(b"rustc 1.75.0 (82e1608df 2023-12-21)", b"1.75.0")

    def test_openssl(self):
        self._match(b"OpenSSL 3.0.12 24 Oct 2023", b"3.0.12")

    def test_deep_is_superset_of_base(self):
        from inspectah.inspectors.non_rpm_software import VERSION_PATTERNS, DEEP_VERSION_PATTERNS
        for pat in VERSION_PATTERNS:
            assert pat in DEEP_VERSION_PATTERNS


class TestVersionChangeSchema:
    """VersionChange model and RpmSection.version_changes field."""

    def test_version_change_model(self):
        from inspectah.schema import VersionChange, VersionChangeDirection
        vc = VersionChange(
            name="httpd",
            arch="x86_64",
            host_version="2.4.57-5.el9",
            base_version="2.4.53-11.el9",
            host_epoch="0",
            base_epoch="0",
            direction=VersionChangeDirection.DOWNGRADE,
        )
        assert vc.name == "httpd"
        assert vc.direction == VersionChangeDirection.DOWNGRADE
        d = vc.model_dump()
        assert d["direction"] == "downgrade"
        vc2 = VersionChange.model_validate(d)
        assert vc2.direction == VersionChangeDirection.DOWNGRADE

    def test_version_changes_on_rpm_section(self):
        from inspectah.schema import RpmSection, VersionChange, VersionChangeDirection
        section = RpmSection()
        assert section.version_changes == []
        section.version_changes.append(VersionChange(
            name="curl", arch="x86_64",
            host_version="7.76.1-29.el9", base_version="7.76.1-26.el9",
            direction=VersionChangeDirection.DOWNGRADE,
        ))
        assert len(section.version_changes) == 1

    def test_version_changes_empty_by_default_roundtrip(self):
        from inspectah.schema import RpmSection
        data = {"packages_added": [], "base_image_only": []}
        section = RpmSection.model_validate(data)
        assert section.version_changes == []

    def test_schema_version_bumped(self):
        from inspectah.schema import SCHEMA_VERSION
        assert SCHEMA_VERSION >= 7


class TestVersionChangesHtmlReport:
    """Version Changes subsection in the HTML packages tab."""

    def _render_html(self, snapshot):
        """Helper: render HTML report and return the HTML string."""
        from inspectah.renderers.html_report import render as render_html
        from jinja2 import Environment, FileSystemLoader
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            (tmp_path / "Containerfile").write_text("FROM test")
            templates_dir = Path(__file__).parent.parent / "src" / "inspectah" / "templates"
            env = Environment(loader=FileSystemLoader(str(templates_dir)), autoescape=True)
            render_html(snapshot, env, tmp_path)
            return (tmp_path / "report.html").read_text()

    def test_version_changes_table_present(self):
        from inspectah.schema import (
            InspectionSnapshot, OsRelease, RpmSection, PackageEntry,
            VersionChange, VersionChangeDirection,
        )
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.6", id="rhel"),
            rpm=RpmSection(
                base_image="registry.redhat.io/rhel9/rhel-bootc:9.6",
                packages_added=[
                    PackageEntry(name="httpd", epoch="0", version="2.4.57", release="5.el9", arch="x86_64"),
                ],
                version_changes=[
                    VersionChange(
                        name="bash", arch="x86_64",
                        host_version="5.2.15-2.el9", base_version="5.1.8-9.el9",
                        direction=VersionChangeDirection.DOWNGRADE,
                    ),
                    VersionChange(
                        name="curl", arch="x86_64",
                        host_version="7.76.1-26.el9", base_version="7.76.1-29.el9",
                        direction=VersionChangeDirection.UPGRADE,
                    ),
                ],
                leaf_packages=["httpd"],
                auto_packages=[],
            ),
        )
        html = self._render_html(snapshot)
        assert "Version Changes" in html
        assert "bash" in html
        assert "5.2.15-2.el9" in html
        assert "5.1.8-9.el9" in html
        assert "downgrade" in html.lower()
        assert "upgrade" in html.lower()

    def test_version_column_on_dependency_tree(self):
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.6", id="rhel"),
            rpm=RpmSection(
                base_image="registry.redhat.io/rhel9/rhel-bootc:9.6",
                packages_added=[
                    PackageEntry(name="httpd", epoch="0", version="2.4.57", release="5.el9", arch="x86_64"),
                ],
                leaf_packages=["httpd"],
                auto_packages=[],
                leaf_dep_tree={"httpd": []},
            ),
        )
        html = self._render_html(snapshot)
        assert "2.4.57-5.el9" in html

    def test_version_changes_absent_when_empty(self):
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.6", id="rhel"),
            rpm=RpmSection(
                base_image="registry.redhat.io/rhel9/rhel-bootc:9.6",
                packages_added=[
                    PackageEntry(name="httpd", epoch="0", version="2.4.57", release="5.el9", arch="x86_64"),
                ],
                leaf_packages=["httpd"],
                auto_packages=[],
            ),
        )
        html = self._render_html(snapshot)
        assert "Version Changes" not in html


class TestVersionChangesAuditReport:
    """Version drift summary in the audit report."""

    def test_audit_report_shows_version_drift(self):
        from inspectah.schema import (
            InspectionSnapshot, OsRelease, RpmSection, PackageEntry,
            VersionChange, VersionChangeDirection,
        )
        from inspectah.renderers.audit_report import render as render_audit
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.6", id="rhel"),
            rpm=RpmSection(
                base_image="registry.redhat.io/rhel9/rhel-bootc:9.6",
                packages_added=[
                    PackageEntry(name="httpd", epoch="0", version="2.4.57", release="5.el9", arch="x86_64"),
                ],
                version_changes=[
                    VersionChange(
                        name="bash", arch="x86_64",
                        host_version="5.2.15-2.el9", base_version="5.1.8-9.el9",
                        direction=VersionChangeDirection.DOWNGRADE,
                    ),
                ],
                leaf_packages=["httpd"],
                auto_packages=[],
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_audit(snapshot, _env(), Path(tmp))
            report = (Path(tmp) / "audit-report.md").read_text()
        assert "Version" in report
        assert "bash" in report
        assert "downgrade" in report.lower()

    def test_audit_report_no_version_drift_when_empty(self):
        from inspectah.schema import InspectionSnapshot, OsRelease, RpmSection, PackageEntry
        from inspectah.renderers.audit_report import render as render_audit
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.6", id="rhel"),
            rpm=RpmSection(
                base_image="registry.redhat.io/rhel9/rhel-bootc:9.6",
                packages_added=[
                    PackageEntry(name="httpd", epoch="0", version="2.4.57", release="5.el9", arch="x86_64"),
                ],
                leaf_packages=["httpd"],
                auto_packages=[],
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_audit(snapshot, _env(), Path(tmp))
            report = (Path(tmp) / "audit-report.md").read_text()
        assert "Version Changes" not in report


class TestVersionChangeRoundtrip:

    def test_version_changes_survive_json_roundtrip(self):
        from inspectah.schema import (
            InspectionSnapshot, OsRelease, RpmSection,
            VersionChange, VersionChangeDirection,
        )
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.6", id="rhel"),
            rpm=RpmSection(
                version_changes=[
                    VersionChange(
                        name="bash", arch="x86_64",
                        host_version="5.2.15-2.el9", base_version="5.1.8-9.el9",
                        direction=VersionChangeDirection.DOWNGRADE,
                    ),
                ],
            ),
        )
        json_str = snapshot.model_dump_json()
        loaded = InspectionSnapshot.model_validate_json(json_str)
        assert len(loaded.rpm.version_changes) == 1
        assert loaded.rpm.version_changes[0].name == "bash"
        assert loaded.rpm.version_changes[0].direction == VersionChangeDirection.DOWNGRADE


class TestPythonVersionMap:

    def test_rhel10_uses_python312(self):
        items = [
            NonRpmItem(name="cryptography", version="41.0.0", method="pip dist-info",
                       has_c_extensions=True, confidence="high",
                       path="usr/lib/python3.12/site-packages/cryptography-41.0.0.dist-info"),
        ]
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="10.0", id="rhel"),
            rpm=RpmSection(base_image="registry.redhat.io/rhel10/rhel-bootc:10.0"),
            non_rpm_software=NonRpmSoftwareSection(items=items),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_containerfile(snapshot, _env(), Path(tmp))
            cf = (Path(tmp) / "Containerfile").read_text()
        assert "python3.12" in cf
        assert "python3.X" not in cf

    def test_fedora_uses_python312(self):
        items = [
            NonRpmItem(name="numpy", version="1.26.0", method="pip dist-info",
                       has_c_extensions=True, confidence="high",
                       path="usr/lib/python3.12/site-packages/numpy-1.26.0.dist-info"),
        ]
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="Fedora", version_id="41", id="fedora"),
            rpm=RpmSection(base_image="quay.io/fedora/fedora-bootc:41"),
            non_rpm_software=NonRpmSoftwareSection(items=items),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_containerfile(snapshot, _env(), Path(tmp))
            cf = (Path(tmp) / "Containerfile").read_text()
        assert "python3.12" in cf


# ---------------------------------------------------------------------------
# Module stream renderer tests
# ---------------------------------------------------------------------------

def _pkg(name="httpd"):
    return PackageEntry(name=name, epoch="0", version="2.4", release="1", arch="x86_64")


def _render_cf(snapshot):
    with tempfile.TemporaryDirectory() as tmp:
        render_containerfile(snapshot, _env(), Path(tmp))
        return (Path(tmp) / "Containerfile").read_text()


def _snap_with_streams(module_streams, conflicts=None, no_baseline=False):
    return InspectionSnapshot(
        meta={},
        os_release=OsRelease(name="RHEL", version_id="9.6", id="rhel"),
        rpm=RpmSection(
            base_image="registry.redhat.io/rhel9/rhel-bootc:9.6",
            packages_added=[_pkg()],
            module_streams=module_streams,
            module_stream_conflicts=conflicts or [],
            no_baseline=no_baseline,
        ),
    )


class TestModuleStreamRenderer:

    def test_two_streams_emitted(self):
        streams = [
            EnabledModuleStream(module_name="nodejs", stream="18"),
            EnabledModuleStream(module_name="postgresql", stream="15"),
        ]
        cf = _render_cf(_snap_with_streams(streams))
        assert "RUN dnf module enable -y" in cf
        assert "nodejs:18" in cf
        assert "postgresql:15" in cf

    def test_alphabetical_order(self):
        streams = [
            EnabledModuleStream(module_name="postgresql", stream="15"),
            EnabledModuleStream(module_name="nodejs", stream="18"),
        ]
        cf = _render_cf(_snap_with_streams(streams))
        enable_line = next(l for l in cf.splitlines() if "dnf module enable" in l)
        assert enable_line.index("nodejs") < enable_line.index("postgresql")

    def test_baseline_match_true_skipped(self):
        streams = [EnabledModuleStream(module_name="postgresql", stream="15", baseline_match=True)]
        cf = _render_cf(_snap_with_streams(streams))
        assert "dnf module enable" not in cf

    def test_include_false_skipped(self):
        streams = [EnabledModuleStream(module_name="postgresql", stream="15", include=False)]
        cf = _render_cf(_snap_with_streams(streams))
        assert "dnf module enable" not in cf

    def test_no_streams_no_output(self):
        cf = _render_cf(_snap_with_streams([]))
        assert "dnf module enable" not in cf

    def test_conflict_warning_comment(self):
        streams = [EnabledModuleStream(module_name="postgresql", stream="15")]
        conflicts = ["postgresql: host=15, base_image=13"]
        cf = _render_cf(_snap_with_streams(streams, conflicts=conflicts))
        assert "# WARNING: postgresql: host=15, base_image=13" in cf

    def test_no_baseline_all_included_streams_emitted(self):
        """--no-baseline: baseline_match stays False (default), so all included streams emit."""
        streams = [
            EnabledModuleStream(module_name="postgresql", stream="15"),
            EnabledModuleStream(module_name="nodejs", stream="18"),
        ]
        cf = _render_cf(_snap_with_streams(streams, no_baseline=True))
        assert "postgresql:15" in cf
        assert "nodejs:18" in cf

    def test_module_enable_appears_before_dnf_install(self):
        streams = [EnabledModuleStream(module_name="nodejs", stream="18")]
        cf = _render_cf(_snap_with_streams(streams))
        enable_pos = cf.index("dnf module enable")
        install_pos = cf.index("dnf install")
        assert enable_pos < install_pos


# ---------------------------------------------------------------------------
# Version lock renderer tests
# ---------------------------------------------------------------------------

def _snap_with_locks(version_locks, fleet_meta=None):
    meta = {"fleet": fleet_meta} if fleet_meta else {}
    return InspectionSnapshot(
        meta=meta,
        os_release=OsRelease(name="RHEL", version_id="9.6", id="rhel"),
        rpm=RpmSection(
            base_image="registry.redhat.io/rhel9/rhel-bootc:9.6",
            packages_added=[_pkg()],
            version_locks=version_locks,
        ),
    )


def _lock(name, version, release="1.el9", arch="x86_64", epoch=0, include=True, fleet=None):
    return VersionLockEntry(
        raw_pattern=f"{name}-{version}-{release}.{arch}",
        name=name, version=version, release=release, arch=arch,
        epoch=epoch, include=include, fleet=fleet,
    )


class TestVersionLockRendererSingleHost:

    def test_fixme_block_present(self):
        locks = [_lock("curl", "7.76.1", "26.el9")]
        cf = _render_cf(_snap_with_locks(locks))
        assert "FIXME: The following packages were version-locked on the source system." in cf
        assert "curl-7.76.1-26.el9.x86_64" in cf

    def test_include_false_skipped(self):
        locks = [_lock("curl", "7.76.1", "26.el9", include=False)]
        cf = _render_cf(_snap_with_locks(locks))
        assert "version-locked on the source system" not in cf

    def test_no_locks_no_version_lock_output(self):
        cf = _render_cf(_snap_with_locks([]))
        assert "version-locked" not in cf

    def test_multiple_locks_all_listed(self):
        locks = [_lock("curl", "7.76.1", "26.el9"), _lock("openssl", "3.0.7", "24.el9")]
        cf = _render_cf(_snap_with_locks(locks))
        assert "curl-7.76.1-26.el9.x86_64" in cf
        assert "openssl-3.0.7-24.el9.x86_64" in cf


class TestVersionLockRendererFleet:

    def _fleet_meta(self, total=3, min_prev=67):
        return {"source_hosts": [f"h{i}" for i in range(total)],
                "total_hosts": total, "min_prevalence": min_prev}

    def test_above_threshold_pinned(self):
        locks = [_lock("sudo", "1.9.5p2", "10.el9",
                        fleet=FleetPrevalence(count=3, total=3))]
        cf = _render_cf(_snap_with_locks(locks, fleet_meta=self._fleet_meta()))
        assert "RUN dnf install -y sudo-1.9.5p2-10.el9.x86_64" in cf
        assert "RUN dnf versionlock add sudo-1.9.5p2-10.el9.x86_64" in cf
        assert "FIXME: The following packages were version-locked" not in cf

    def test_epoch_included_in_install_line(self):
        locks = [_lock("curl", "7.76.1", "26.el9", epoch=1,
                        fleet=FleetPrevalence(count=3, total=3))]
        cf = _render_cf(_snap_with_locks(locks, fleet_meta=self._fleet_meta()))
        assert "RUN dnf install -y 1:curl-7.76.1-26.el9.x86_64" in cf

    def test_below_threshold_gets_fixme(self):
        # 1/3 = 33%, below 67% threshold
        locks = [_lock("curl", "7.76.1", "26.el9",
                        fleet=FleetPrevalence(count=1, total=3))]
        cf = _render_cf(_snap_with_locks(locks, fleet_meta=self._fleet_meta()))
        assert "FIXME: The following packages were version-locked" in cf
        assert "dnf versionlock add" not in cf

    def test_tiebreak_newer_version_wins(self):
        # Both at 50% prevalence (2/4), tie-broken by rpmvercmp
        locks = [
            _lock("curl", "7.76.1", "26.el9", fleet=FleetPrevalence(count=2, total=4)),
            _lock("curl", "7.76.0", "20.el9", fleet=FleetPrevalence(count=2, total=4)),
        ]
        cf = _render_cf(_snap_with_locks(locks, fleet_meta=self._fleet_meta(total=4, min_prev=50)))
        assert "RUN dnf install -y curl-7.76.1-26.el9.x86_64" in cf  # newer wins
        assert "FIXME: The following packages were version-locked" in cf  # loser gets FIXME
