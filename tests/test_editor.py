"""Editor tab tests: refine mode rendering, static mode preservation."""

import tempfile
from pathlib import Path

from yoinkc.renderers import run_all as run_all_renderers
from yoinkc.schema import (
    ConfigFileEntry,
    ConfigFileKind,
    ConfigSection,
    ContainerSection,
    InspectionSnapshot,
    OsRelease,
    QuadletUnit,
    ServiceSection,
    SystemdDropIn,
)


def _render(refine_mode=False, with_content=False):
    kwargs = dict(
        meta={"host_root": "/host"},
        os_release=OsRelease(name="RHEL", version_id="9.6", pretty_name="RHEL 9.6"),
    )
    if with_content:
        kwargs["config"] = ConfigSection(files=[
            ConfigFileEntry(path="/etc/myapp/app.conf", kind=ConfigFileKind.RPM_OWNED_MODIFIED, content="key=val"),
        ])
        kwargs["containers"] = ContainerSection(quadlet_units=[
            QuadletUnit(path="/etc/containers/systemd/myapp.container", name="myapp.container", content="[Container]\nImage=ghcr.io/myapp:latest"),
        ])
        kwargs["services"] = ServiceSection(drop_ins=[
            SystemdDropIn(unit="postgresql.service", path="etc/systemd/system/postgresql.service.d/override.conf", content="[Service]\nLimitNOFILE=65536"),
        ])
    snapshot = InspectionSnapshot(**kwargs)
    with tempfile.TemporaryDirectory() as tmp:
        run_all_renderers(snapshot, Path(tmp), refine_mode=refine_mode)
        return (Path(tmp) / "report.html").read_text()


class TestEditorTab:

    def test_refine_mode_shows_editor_tab(self):
        """In refine mode, the editor tab replaces the file browser."""
        html = _render(refine_mode=True)
        assert 'id="editor-tab"' in html
        assert 'id="btn-new-file"' in html

    def test_refine_mode_sidebar_says_editor(self):
        html = _render(refine_mode=True)
        assert 'Editor</a>' in html

    def test_static_mode_shows_file_browser(self):
        """In static mode, the read-only file browser is shown."""
        html = _render(refine_mode=False)
        assert 'id="file-viewer-content"' in html
        assert 'id="editor-tab"' not in html

    def test_static_mode_sidebar_says_file_browser(self):
        html = _render(refine_mode=False)
        assert 'File browser</a>' in html

    def test_editor_tree_built_from_snapshot(self):
        """Editor tree JS builds file entries from snapshot sections."""
        html = _render(refine_mode=True)
        assert 'buildTree' in html
        assert "section: 'config'" in html or 'section: "config"' in html

    def test_editor_has_codemirror(self):
        """Refine mode embeds the CM6 bundle inline."""
        html = _render(refine_mode=True)
        assert 'CMEditor' in html

    def test_static_mode_no_codemirror(self):
        """Static mode does not include the CM6 bundle."""
        html = _render(refine_mode=False)
        assert 'CMEditor' not in html

    def test_editor_save_all_exists(self):
        html = _render(refine_mode=True)
        assert 'editorSaveAll' in html
        assert 'findFileInSnapshot' in html

    def test_editor_ctrl_s_shortcut(self):
        html = _render(refine_mode=True)
        assert "e.key === 's'" in html

    def test_editor_dirty_tracking(self):
        html = _render(refine_mode=True)
        assert 'setupDirtyTracking' in html

    def test_edit_in_editor_links_refine_mode(self):
        """Refine mode shows 'View & edit in editor' links for content sections."""
        html = _render(refine_mode=True, with_content=True)
        assert 'View &amp; edit in editor' in html
        assert 'navigateToEditor' in html

    def test_no_edit_links_static_mode(self):
        """Static mode keeps content pulldowns, no editor links."""
        html = _render(refine_mode=False, with_content=True)
        assert 'View &amp; edit in editor' not in html
        assert 'navigateToEditor' not in html

    def test_new_file_modal_in_refine_mode(self):
        html = _render(refine_mode=True)
        assert 'new-file-modal' in html
        assert 'Create new file' in html
        assert 'createNewFile' in html
        assert 'validateNewFileForm' in html

    def test_no_new_file_modal_in_static_mode(self):
        html = _render(refine_mode=False)
        assert 'new-file-modal' not in html
        assert 'createNewFile' not in html

    def test_new_file_modal_service_dropdown(self):
        """Drop-in service dropdown populated from snapshot."""
        html = _render(refine_mode=True, with_content=True)
        assert 'nf-dropin-service' in html
