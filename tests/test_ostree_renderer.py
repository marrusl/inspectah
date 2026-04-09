"""Containerfile renderer tests for ostree/bootc systems."""
from pathlib import Path
from yoinkc.schema import (
    InspectionSnapshot, OsRelease, SystemType, RpmSection,
    PackageEntry, OstreePackageOverride, ContainerSection, FlatpakApp,
)


def _make_ostree_snapshot() -> InspectionSnapshot:
    return InspectionSnapshot(
        system_type=SystemType.RPM_OSTREE,
        os_release=OsRelease(
            name="Fedora Linux", version_id="41", id="fedora",
            variant_id="silverblue", pretty_name="Fedora Linux 41 (Silverblue)",
        ),
        rpm=RpmSection(
            packages_added=[
                PackageEntry(name="httpd", version="2.4.59", release="1.fc41", arch="x86_64", epoch="0"),
                PackageEntry(name="vim-enhanced", version="9.1", release="1.fc41", arch="x86_64", epoch="0"),
            ],
            base_image="quay.io/fedora-ostree-desktops/silverblue:41",
            ostree_overrides=[
                OstreePackageOverride(name="kernel", from_nevra="kernel-6.7.9-200.fc41", to_nevra="kernel-6.8.1-100.fc41"),
            ],
            ostree_removals=["nano"],
        ),
        containers=ContainerSection(
            flatpak_apps=[
                FlatpakApp(app_id="org.mozilla.firefox", origin="flathub", branch="stable"),
                FlatpakApp(app_id="org.gnome.Calculator", origin="flathub", branch="stable"),
                FlatpakApp(app_id="org.fedoraproject.MediaWriter", origin="fedora", branch="stable"),
            ],
        ),
    )


def test_ostree_layered_packages_in_dnf_install(tmp_path):
    from yoinkc.renderers.containerfile._core import _render_containerfile_content
    snapshot = _make_ostree_snapshot()
    content = _render_containerfile_content(snapshot, tmp_path)
    assert "RUN dnf install -y" in content
    assert "httpd" in content
    assert "vim-enhanced" in content


def test_ostree_from_line_uses_ostree_base(tmp_path):
    from yoinkc.renderers.containerfile._core import _render_containerfile_content
    snapshot = _make_ostree_snapshot()
    content = _render_containerfile_content(snapshot, tmp_path)
    assert "FROM quay.io/fedora-ostree-desktops/silverblue:41" in content


def test_ostree_desktops_bootc_label_emitted(tmp_path):
    from yoinkc.renderers.containerfile._core import _render_containerfile_content
    snapshot = _make_ostree_snapshot()
    content = _render_containerfile_content(snapshot, tmp_path)
    assert 'LABEL containers.bootc 1' in content


def test_ostree_removed_packages_in_containerfile(tmp_path):
    from yoinkc.renderers.containerfile._core import _render_containerfile_content
    snapshot = _make_ostree_snapshot()
    content = _render_containerfile_content(snapshot, tmp_path)
    assert "RUN dnf remove" in content
    assert "nano" in content


def test_ostree_overridden_packages_in_containerfile(tmp_path):
    from yoinkc.renderers.containerfile._core import _render_containerfile_content
    snapshot = _make_ostree_snapshot()
    content = _render_containerfile_content(snapshot, tmp_path)
    assert "kernel" in content
    assert "Override" in content


def test_flatpaks_list_generated(tmp_path):
    from yoinkc.renderers.containerfile._core import render
    from jinja2 import Environment
    snapshot = _make_ostree_snapshot()
    render(snapshot, Environment(autoescape=True), tmp_path)
    flatpaks_file = tmp_path / "flatpaks.list"
    assert flatpaks_file.exists()
    content = flatpaks_file.read_text()
    assert "flathub org.mozilla.firefox" in content
    assert "flathub org.gnome.Calculator" in content
    assert "fedora org.fedoraproject.MediaWriter" in content


def test_flatpaks_list_not_generated_when_empty(tmp_path):
    from yoinkc.renderers.containerfile._core import render
    from jinja2 import Environment
    snapshot = _make_ostree_snapshot()
    snapshot.containers.flatpak_apps = []
    render(snapshot, Environment(autoescape=True), tmp_path)
    assert not (tmp_path / "flatpaks.list").exists()


def test_renderer_integration_from_ostree_snapshot(tmp_path):
    from yoinkc.renderers.containerfile._core import render
    from jinja2 import Environment
    snapshot = _make_ostree_snapshot()
    render(snapshot, Environment(autoescape=True), tmp_path)
    containerfile = tmp_path / "Containerfile"
    assert containerfile.exists()
    content = containerfile.read_text()
    assert "FROM quay.io/fedora-ostree-desktops/silverblue:41" in content
    assert "dnf install" in content
    assert (tmp_path / "flatpaks.list").exists()
