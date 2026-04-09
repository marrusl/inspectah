"""System type detection and ostree base image mapping tests."""

from yoinkc.schema import SystemType, FlatpakApp, OstreePackageOverride


def test_system_type_enum_values():
    assert SystemType.PACKAGE_MODE == "package-mode"
    assert SystemType.RPM_OSTREE == "rpm-ostree"
    assert SystemType.BOOTC == "bootc"


def test_flatpak_app_model():
    app = FlatpakApp(app_id="org.mozilla.firefox", origin="flathub", branch="stable")
    assert app.app_id == "org.mozilla.firefox"
    assert app.origin == "flathub"


def test_ostree_package_override_model():
    ovr = OstreePackageOverride(
        name="kernel",
        from_nevra="kernel-5.14.0-1.el9",
        to_nevra="kernel-5.14.0-2.el9",
    )
    assert ovr.name == "kernel"


def test_os_release_has_variant_id():
    from yoinkc.schema import OsRelease
    osr = OsRelease(name="Fedora", version_id="41", variant_id="silverblue")
    assert osr.variant_id == "silverblue"


def test_snapshot_system_type_default():
    from yoinkc.schema import InspectionSnapshot
    snap = InspectionSnapshot()
    assert snap.system_type == SystemType.PACKAGE_MODE


import pytest
from pathlib import Path
from yoinkc.executor import RunResult
from yoinkc.system_type import detect_system_type, OstreeDetectionError


def _mock_executor(bootc_rc=1, rpmostree_rc=1):
    """Return an executor that fakes bootc/rpm-ostree status commands."""
    def executor(cmd, *, cwd=None):
        if cmd == ["bootc", "status"]:
            if bootc_rc == 0:
                return RunResult(stdout="ok", stderr="", returncode=0)
            return RunResult(stdout="", stderr="not found", returncode=bootc_rc)
        if cmd == ["rpm-ostree", "status"]:
            if rpmostree_rc == 0:
                return RunResult(stdout="State: idle", stderr="", returncode=0)
            return RunResult(stdout="", stderr="not found", returncode=rpmostree_rc)
        return RunResult(stdout="", stderr="", returncode=1)
    return executor


def test_detect_package_mode(tmp_path):
    """No /ostree directory -> package-mode."""
    assert detect_system_type(tmp_path, _mock_executor()) == SystemType.PACKAGE_MODE


def test_detect_bootc_system(tmp_path):
    """/ostree exists + bootc status succeeds -> bootc."""
    (tmp_path / "ostree").mkdir()
    assert detect_system_type(tmp_path, _mock_executor(bootc_rc=0)) == SystemType.BOOTC


def test_detect_rpm_ostree_system(tmp_path):
    """/ostree exists + bootc fails + rpm-ostree succeeds -> rpm-ostree."""
    (tmp_path / "ostree").mkdir()
    assert detect_system_type(
        tmp_path, _mock_executor(bootc_rc=1, rpmostree_rc=0)
    ) == SystemType.RPM_OSTREE


def test_detect_unknown_ostree_raises(tmp_path):
    """/ostree exists + both commands fail -> OstreeDetectionError."""
    (tmp_path / "ostree").mkdir()
    with pytest.raises(OstreeDetectionError, match="could not determine"):
        detect_system_type(tmp_path, _mock_executor(bootc_rc=1, rpmostree_rc=1))


def test_detect_bootc_preferred_over_rpmostree(tmp_path):
    """When both succeed, bootc wins."""
    (tmp_path / "ostree").mkdir()
    assert detect_system_type(
        tmp_path, _mock_executor(bootc_rc=0, rpmostree_rc=0)
    ) == SystemType.BOOTC
