"""Plan item tests: packages, repos, dependency classification, version patterns."""

import tempfile
from pathlib import Path

import pytest

from yoinkc.schema import (
    InspectionSnapshot,
    NonRpmItem,
    NonRpmSoftwareSection,
    OsRelease,
    PackageEntry,
    RepoFile,
    RpmSection,
)
from yoinkc.renderers.containerfile import render as render_containerfile
from yoinkc.renderers.audit_report import render as render_audit

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
        from yoinkc.inspectors.rpm import _classify_default_repo
        rf = RepoFile(path="etc/yum.repos.d/redhat.repo", content="[rhel-baseos]\nbaseurl=http://x\n")
        assert _classify_default_repo(rf) is True

    def test_non_default_repo_epel(self):
        from yoinkc.inspectors.rpm import _classify_default_repo
        rf = RepoFile(path="etc/yum.repos.d/epel.repo", content="[epel]\nbaseurl=http://x\n")
        assert _classify_default_repo(rf) is False

    def test_default_repo_appstream_section(self):
        from yoinkc.inspectors.rpm import _classify_default_repo
        rf = RepoFile(path="etc/yum.repos.d/centos.repo", content="[appstream]\nbaseurl=http://x\n")
        assert _classify_default_repo(rf) is True

    def test_non_default_repo_copr(self):
        from yoinkc.inspectors.rpm import _classify_default_repo
        rf = RepoFile(path="etc/yum.repos.d/copr-myrepo.repo", content="[copr:user:project]\nbaseurl=http://x\n")
        assert _classify_default_repo(rf) is False

    def test_default_repo_fedora_section(self):
        from yoinkc.inspectors.rpm import _classify_default_repo
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
        from yoinkc.inspectors.non_rpm_software import DEEP_VERSION_PATTERNS
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
        from yoinkc.inspectors.non_rpm_software import VERSION_PATTERNS, DEEP_VERSION_PATTERNS
        for pat in VERSION_PATTERNS:
            assert pat in DEEP_VERSION_PATTERNS


class TestVersionChangeSchema:
    """VersionChange model and RpmSection.version_changes field."""

    def test_version_change_model(self):
        from yoinkc.schema import VersionChange, VersionChangeDirection
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
        from yoinkc.schema import RpmSection, VersionChange, VersionChangeDirection
        section = RpmSection()
        assert section.version_changes == []
        section.version_changes.append(VersionChange(
            name="curl", arch="x86_64",
            host_version="7.76.1-29.el9", base_version="7.76.1-26.el9",
            direction=VersionChangeDirection.DOWNGRADE,
        ))
        assert len(section.version_changes) == 1

    def test_version_changes_empty_by_default_roundtrip(self):
        from yoinkc.schema import RpmSection
        data = {"packages_added": [], "base_image_only": []}
        section = RpmSection.model_validate(data)
        assert section.version_changes == []

    def test_schema_version_bumped(self):
        from yoinkc.schema import SCHEMA_VERSION
        assert SCHEMA_VERSION >= 7


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
