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


# =====================================================================
# ostree base image mapping tests
# =====================================================================

import json
from yoinkc.system_type import map_ostree_base_image


def _make_os_release(**kwargs):
    from yoinkc.schema import OsRelease
    defaults = {
        "name": "Fedora Linux", "version_id": "41",
        "id": "fedora", "variant_id": "",
    }
    defaults.update(kwargs)
    return OsRelease(**defaults)


# --- rpm-ostree systems: VARIANT_ID mapping ---

def test_map_silverblue(tmp_path):
    os_rel = _make_os_release(variant_id="silverblue", version_id="41")
    result = map_ostree_base_image(tmp_path, os_rel, SystemType.RPM_OSTREE, executor=None)
    assert result == "quay.io/fedora-ostree-desktops/silverblue:41"


def test_map_kinoite(tmp_path):
    os_rel = _make_os_release(variant_id="kinoite", version_id="42")
    result = map_ostree_base_image(tmp_path, os_rel, SystemType.RPM_OSTREE, executor=None)
    assert result == "quay.io/fedora-ostree-desktops/kinoite:42"


# --- Universal Blue ---

def test_map_universal_blue(tmp_path):
    ublue_dir = tmp_path / "usr" / "share" / "ublue-os"
    ublue_dir.mkdir(parents=True)
    info = {
        "image-name": "bluefin", "image-vendor": "ublue-os",
        "image-ref": "ghcr.io/ublue-os/bluefin:41", "image-tag": "41",
    }
    (ublue_dir / "image-info.json").write_text(json.dumps(info))
    os_rel = _make_os_release(variant_id="silverblue", version_id="41")
    result = map_ostree_base_image(tmp_path, os_rel, SystemType.RPM_OSTREE, executor=None)
    assert result == "ghcr.io/ublue-os/bluefin:41"


def test_map_ublue_synthesis_from_vendor_name_tag(tmp_path):
    """UBlue image-info.json without image-ref but with vendor/name/tag -> synthesized ref."""
    ublue_dir = tmp_path / "usr" / "share" / "ublue-os"
    ublue_dir.mkdir(parents=True)
    info = {
        "image-name": "aurora",
        "image-vendor": "ublue-os",
        "image-tag": "42",
        # No image-ref -- synthesis should kick in
    }
    (ublue_dir / "image-info.json").write_text(json.dumps(info))
    os_rel = _make_os_release(variant_id="kinoite", version_id="42")
    result = map_ostree_base_image(tmp_path, os_rel, SystemType.RPM_OSTREE, executor=None)
    assert result == "ghcr.io/ublue-os/aurora:42"


def test_map_ublue_malformed_json_missing_fields(tmp_path):
    """Missing required image-name -> None."""
    ublue_dir = tmp_path / "usr" / "share" / "ublue-os"
    ublue_dir.mkdir(parents=True)
    (ublue_dir / "image-info.json").write_text(json.dumps({"image-vendor": "ublue-os"}))
    os_rel = _make_os_release(variant_id="silverblue", version_id="41")
    result = map_ostree_base_image(tmp_path, os_rel, SystemType.RPM_OSTREE, executor=None)
    assert result is None


def test_map_ublue_invalid_json(tmp_path):
    ublue_dir = tmp_path / "usr" / "share" / "ublue-os"
    ublue_dir.mkdir(parents=True)
    (ublue_dir / "image-info.json").write_text("not valid json{{{")
    os_rel = _make_os_release(variant_id="silverblue", version_id="41")
    result = map_ostree_base_image(tmp_path, os_rel, SystemType.RPM_OSTREE, executor=None)
    assert result is None


# --- bootc systems ---

def test_map_fedora_bootc_from_status(tmp_path):
    os_rel = _make_os_release(id="fedora", version_id="41")
    def executor(cmd, *, cwd=None):
        if cmd == ["bootc", "status", "--json"]:
            return RunResult(
                stdout=json.dumps({"status": {"booted": {"image": {"image": {"image": "quay.io/fedora/fedora-bootc:41"}}}}}),
                stderr="", returncode=0,
            )
        return RunResult(stdout="", stderr="", returncode=1)
    result = map_ostree_base_image(tmp_path, os_rel, SystemType.BOOTC, executor=executor)
    assert result == "quay.io/fedora/fedora-bootc:41"


def test_map_bootc_status_fails_falls_back_to_os_release(tmp_path):
    os_rel = _make_os_release(id="fedora", version_id="41")
    def executor(cmd, *, cwd=None):
        return RunResult(stdout="", stderr="error", returncode=1)
    result = map_ostree_base_image(tmp_path, os_rel, SystemType.BOOTC, executor=executor)
    assert result == "quay.io/fedora/fedora-bootc:41"


def test_map_centos_bootc(tmp_path):
    os_rel = _make_os_release(id="centos", name="CentOS Stream", version_id="10")
    result = map_ostree_base_image(tmp_path, os_rel, SystemType.BOOTC, executor=None)
    assert result == "quay.io/centos-bootc/centos-bootc:stream10"


def test_map_rhel_bootc(tmp_path):
    os_rel = _make_os_release(id="rhel", version_id="9.4")
    result = map_ostree_base_image(tmp_path, os_rel, SystemType.BOOTC, executor=None)
    assert result == "registry.redhat.io/rhel9/rhel-bootc:9.4"


def test_map_unknown_returns_none(tmp_path):
    os_rel = _make_os_release(id="custom-os", version_id="1.0")
    result = map_ostree_base_image(tmp_path, os_rel, SystemType.RPM_OSTREE, executor=None)
    assert result is None


def test_map_target_image_override(tmp_path):
    os_rel = _make_os_release(variant_id="silverblue", version_id="41")
    result = map_ostree_base_image(
        tmp_path, os_rel, SystemType.RPM_OSTREE, executor=None,
        target_image_override="quay.io/my-custom/image:latest",
    )
    assert result == "quay.io/my-custom/image:latest"


# =====================================================================
# Pipeline wiring tests (run_all integration)
# =====================================================================

from unittest.mock import patch
from yoinkc.inspectors import run_all, _read_os_release
import yoinkc.preflight as preflight_mod


def test_read_os_release_captures_variant_id(tmp_path):
    etc = tmp_path / "etc"
    etc.mkdir()
    (etc / "os-release").write_text(
        'NAME="Fedora Linux"\nVERSION_ID=41\nID=fedora\n'
        'VARIANT_ID=silverblue\nPRETTY_NAME="Fedora Linux 41 (Silverblue)"\n'
    )
    os_rel = _read_os_release(tmp_path)
    assert os_rel is not None
    assert os_rel.variant_id == "silverblue"


def _silverblue_executor(cmd, *, cwd=None):
    """Executor simulating a Silverblue system."""
    if cmd == ["bootc", "status"]:
        return RunResult(stdout="", stderr="not found", returncode=1)
    if cmd == ["rpm-ostree", "status"]:
        return RunResult(stdout="State: idle", stderr="", returncode=0)
    return RunResult(stdout="", stderr="", returncode=1)


def _setup_silverblue_host(tmp_path):
    (tmp_path / "ostree").mkdir()
    etc = tmp_path / "etc"
    etc.mkdir()
    (etc / "os-release").write_text(
        'NAME="Fedora Linux"\nVERSION_ID=41\nID=fedora\n'
        'VARIANT_ID=silverblue\nPRETTY_NAME="Fedora Linux 41 (Silverblue)"\n'
    )
    (etc / "hostname").write_text("test-host\n")
    return tmp_path


def test_run_all_detects_rpm_ostree(tmp_path):
    host_root = _setup_silverblue_host(tmp_path)
    with patch.object(preflight_mod, "in_user_namespace", return_value=False):
        snapshot = run_all(host_root, executor=_silverblue_executor, no_baseline_opt_in=True)
    assert snapshot.system_type == SystemType.RPM_OSTREE


def test_run_all_package_mode_unchanged(tmp_path):
    """No /ostree -> package-mode, existing behavior intact."""
    etc = tmp_path / "etc"
    etc.mkdir()
    (etc / "os-release").write_text(
        'NAME="Fedora Linux"\nVERSION_ID=41\nID=fedora\n'
    )
    (etc / "hostname").write_text("test\n")
    no_ostree_exec = _mock_executor()
    with patch.object(preflight_mod, "in_user_namespace", return_value=False):
        snapshot = run_all(tmp_path, executor=no_ostree_exec, no_baseline_opt_in=True)
    assert snapshot.system_type == SystemType.PACKAGE_MODE


def test_run_all_unknown_ostree_refuses_without_no_baseline(tmp_path):
    """Unknown ostree without --target-image and without --no-baseline -> hard exit."""
    (tmp_path / "ostree").mkdir()
    etc = tmp_path / "etc"
    etc.mkdir()
    (etc / "os-release").write_text(
        'NAME="CustomOS"\nVERSION_ID=1.0\nID=custom-os\n'
        'VARIANT_ID=custom\nPRETTY_NAME="Custom OS"\n'
    )
    (etc / "hostname").write_text("test\n")
    def unknown_exec(cmd, *, cwd=None):
        if cmd == ["bootc", "status"]:
            return RunResult(stdout="", stderr="", returncode=1)
        if cmd == ["rpm-ostree", "status"]:
            return RunResult(stdout="ok", stderr="", returncode=0)
        return RunResult(stdout="", stderr="", returncode=1)
    with patch.object(preflight_mod, "in_user_namespace", return_value=False):
        with pytest.raises(SystemExit):
            run_all(tmp_path, executor=unknown_exec)


def test_run_all_unknown_ostree_proceeds_with_no_baseline(tmp_path):
    """Unknown ostree + --no-baseline -> warn but proceed."""
    (tmp_path / "ostree").mkdir()
    etc = tmp_path / "etc"
    etc.mkdir()
    (etc / "os-release").write_text(
        'NAME="CustomOS"\nVERSION_ID=1.0\nID=custom-os\n'
        'VARIANT_ID=custom\nPRETTY_NAME="Custom OS"\n'
    )
    (etc / "hostname").write_text("test\n")
    def unknown_exec(cmd, *, cwd=None):
        if cmd == ["bootc", "status"]:
            return RunResult(stdout="", stderr="", returncode=1)
        if cmd == ["rpm-ostree", "status"]:
            return RunResult(stdout="ok", stderr="", returncode=0)
        return RunResult(stdout="", stderr="", returncode=1)
    with patch.object(preflight_mod, "in_user_namespace", return_value=False):
        snapshot = run_all(tmp_path, executor=unknown_exec, no_baseline_opt_in=True)
    assert snapshot.system_type == SystemType.RPM_OSTREE
    warning_msgs = [w.get("message", "") for w in snapshot.warnings]
    assert any("could not map" in m.lower() or "unknown" in m.lower() for m in warning_msgs)


def test_run_all_target_image_overrides_ostree_mapping(tmp_path):
    """--target-image overrides auto-mapping on ostree systems."""
    host_root = _setup_silverblue_host(tmp_path)
    with patch.object(preflight_mod, "in_user_namespace", return_value=False):
        snapshot = run_all(
            host_root, executor=_silverblue_executor,
            target_image="quay.io/my-custom/image:latest",
            no_baseline_opt_in=True,
        )
    assert snapshot.system_type == SystemType.RPM_OSTREE
