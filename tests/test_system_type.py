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
