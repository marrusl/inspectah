"""Tests for multi-arch and duplicate package detection (items 1 & 2 of detection quick wins)."""

import tempfile
from pathlib import Path

from jinja2 import Environment

from yoinkc.inspectors.rpm import _detect_duplicates, _detect_multiarch
from yoinkc.renderers.audit_report import render as render_audit_report
from yoinkc.renderers.containerfile.packages import section_lines
from yoinkc.schema import (
    InspectionSnapshot,
    OsRelease,
    PackageEntry,
    PackageState,
    RpmSection,
)


def _pkg(name: str, arch: str, version: str = "1.0", release: str = "1.el9") -> PackageEntry:
    return PackageEntry(name=name, arch=arch, version=version, release=release, state=PackageState.ADDED)


# --- _detect_multiarch ---

class TestDetectMultiarch:

    def test_same_name_different_arch_flagged(self):
        pkgs = [_pkg("zlib", "x86_64"), _pkg("zlib", "i686")]
        assert _detect_multiarch(pkgs) == ["zlib.i686"]

    def test_same_name_same_arch_not_flagged(self):
        pkgs = [_pkg("zlib", "x86_64"), _pkg("zlib", "x86_64", version="1.1")]
        assert _detect_multiarch(pkgs) == []

    def test_single_package_not_flagged(self):
        pkgs = [_pkg("bash", "x86_64")]
        assert _detect_multiarch(pkgs) == []

    def test_empty_list(self):
        assert _detect_multiarch([]) == []

    def test_multiple_multiarch_packages_sorted(self):
        pkgs = [
            _pkg("openssl", "x86_64"),
            _pkg("openssl", "i686"),
            _pkg("curl", "x86_64"),
            _pkg("curl", "i686"),
            _pkg("bash", "x86_64"),
        ]
        result = _detect_multiarch(pkgs)
        assert result == ["curl.i686", "openssl.i686"]
        assert "bash.i686" not in result

    def test_multiarch_includes_all_variants_when_no_native_arch_present(self):
        pkgs = [_pkg("libfoo", "ppc64le"), _pkg("libfoo", "s390x")]
        assert _detect_multiarch(pkgs) == ["libfoo.ppc64le", "libfoo.s390x"]

    def test_noarch_not_counted_as_multiarch(self):
        pkgs = [_pkg("python3-pip", "noarch"), _pkg("python3-pip", "noarch", version="2.0")]
        assert _detect_multiarch(pkgs) == []


# --- _detect_duplicates ---

class TestDetectDuplicates:

    def test_same_name_arch_different_versions_flagged(self):
        pkgs = [_pkg("curl", "x86_64", "7.76"), _pkg("curl", "x86_64", "7.88")]
        assert _detect_duplicates(pkgs) == ["curl.x86_64"]

    def test_different_names_not_flagged(self):
        pkgs = [_pkg("curl", "x86_64"), _pkg("wget", "x86_64")]
        assert _detect_duplicates(pkgs) == []

    def test_single_package_not_flagged(self):
        pkgs = [_pkg("bash", "x86_64")]
        assert _detect_duplicates(pkgs) == []

    def test_empty_list(self):
        assert _detect_duplicates([]) == []

    def test_different_arches_not_same_key(self):
        # x86_64 and i686 are different name.arch keys — each has count 1
        pkgs = [_pkg("zlib", "x86_64"), _pkg("zlib", "i686")]
        assert _detect_duplicates(pkgs) == []

    def test_multiple_duplicates_sorted(self):
        pkgs = [
            _pkg("zlib", "x86_64", "1.2.11"),
            _pkg("zlib", "x86_64", "1.2.12"),
            _pkg("curl", "x86_64", "7.76"),
            _pkg("curl", "x86_64", "7.88"),
        ]
        result = _detect_duplicates(pkgs)
        assert result == ["curl.x86_64", "zlib.x86_64"]


# --- Containerfile FIXME blocks ---

def _make_snapshot(multiarch=None, duplicates=None) -> InspectionSnapshot:
    rpm = RpmSection(
        packages_added=[_pkg("httpd", "x86_64")],
        multiarch_packages=multiarch or [],
        duplicate_packages=duplicates or [],
        no_baseline=True,
    )
    return InspectionSnapshot(
        meta={},
        os_release=OsRelease(name="RHEL", version_id="9", pretty_name="RHEL 9"),
        rpm=rpm,
    )


class TestContainerfileFixme:

    def _lines(self, snapshot: InspectionSnapshot) -> list[str]:
        return section_lines(
            snapshot,
            base="quay.io/centos-bootc/centos-bootc:stream9",
            c_ext_pip=[],
            needs_multistage=False,
        )

    def test_multiarch_fixme_emitted_when_non_empty(self):
        snap = _make_snapshot(multiarch=["zlib.i686"])
        lines = self._lines(snap)
        text = "\n".join(lines)
        assert "FIXME" in text
        assert "32-bit" in text
        assert "zlib.i686" in text

    def test_multiarch_fixme_absent_when_empty(self):
        snap = _make_snapshot(multiarch=[])
        lines = self._lines(snap)
        text = "\n".join(lines)
        assert "32-bit" not in text

    def test_duplicate_fixme_emitted_when_non_empty(self):
        snap = _make_snapshot(duplicates=["curl.x86_64"])
        lines = self._lines(snap)
        text = "\n".join(lines)
        assert "FIXME" in text
        assert "multiple versions" in text
        assert "curl.x86_64" in text

    def test_duplicate_fixme_absent_when_empty(self):
        snap = _make_snapshot(duplicates=[])
        lines = self._lines(snap)
        text = "\n".join(lines)
        assert "multiple versions" not in text

    def test_both_fixme_blocks_present(self):
        snap = _make_snapshot(multiarch=["zlib.i686"], duplicates=["curl.x86_64"])
        lines = self._lines(snap)
        text = "\n".join(lines)
        assert "32-bit" in text
        assert "multiple versions" in text
        assert "zlib.i686" in text
        assert "curl.x86_64" in text

    def test_fixme_appears_before_dnf_install(self):
        snap = _make_snapshot(multiarch=["zlib.i686"])
        lines = self._lines(snap)
        fixme_idx = next(i for i, ln in enumerate(lines) if "FIXME" in ln and "package variants" in ln)
        dnf_idx = next((i for i, ln in enumerate(lines) if ln.startswith("RUN dnf install")), None)
        if dnf_idx is not None:
            assert fixme_idx < dnf_idx

    def test_multiarch_fixme_emitted_when_packages_added_empty(self):
        snap = _make_snapshot(multiarch=["zlib.i686"])
        assert snap.rpm is not None
        snap.rpm.packages_added = []
        lines = self._lines(snap)
        text = "\n".join(lines)
        assert "FIXME" in text
        assert "zlib.i686" in text

    def test_duplicate_fixme_emitted_when_packages_added_empty(self):
        snap = _make_snapshot(duplicates=["curl.x86_64"])
        assert snap.rpm is not None
        snap.rpm.packages_added = []
        lines = self._lines(snap)
        text = "\n".join(lines)
        assert "FIXME" in text
        assert "curl.x86_64" in text


# --- Audit report summary lines ---

class TestAuditReportSummary:

    def _render(self, snapshot: InspectionSnapshot) -> str:
        with tempfile.TemporaryDirectory() as tmpdir:
            render_audit_report(snapshot, Environment(), Path(tmpdir))
            return (Path(tmpdir) / "audit-report.md").read_text()

    def test_multiarch_summary_present(self):
        snap = _make_snapshot(multiarch=["zlib.i686", "openssl.i686"])
        md = self._render(snap)
        assert "Multi-arch" in md
        assert "2" in md

    def test_multiarch_summary_absent_when_empty(self):
        snap = _make_snapshot(multiarch=[])
        md = self._render(snap)
        assert "Multi-arch" not in md

    def test_duplicates_summary_present(self):
        snap = _make_snapshot(duplicates=["curl.x86_64"])
        md = self._render(snap)
        assert "Duplicates" in md
        assert "1" in md

    def test_duplicates_summary_absent_when_empty(self):
        snap = _make_snapshot(duplicates=[])
        md = self._render(snap)
        assert "Duplicates" not in md

    def test_both_summaries_in_rpm_section(self):
        snap = _make_snapshot(multiarch=["zlib.i686"], duplicates=["curl.x86_64"])
        md = self._render(snap)
        assert "Multi-arch" in md
        assert "Duplicates" in md
