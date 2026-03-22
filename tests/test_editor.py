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
        """Refine mode shows pencil icon editor buttons for content sections."""
        html = _render(refine_mode=True, with_content=True)
        assert 'class="pf-v6-c-button pf-m-plain editor-icon"' in html
        assert 'navigateToEditor' in html
        assert 'View &amp; edit in editor' not in html

    def test_no_edit_links_static_mode(self):
        """Static mode keeps content pulldowns, no editor links or pencil icons."""
        html = _render(refine_mode=False, with_content=True)
        assert 'class="pf-v6-c-button pf-m-plain editor-icon"' not in html
        assert 'navigateToEditor' not in html
        assert 'View &amp; edit in editor' not in html

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

    def test_rerender_button_in_refine_mode(self):
        html = _render(refine_mode=True)
        assert 'id="btn-re-render"' in html
        assert 'editor-changed-count' in html

    def test_no_editor_rerender_button_in_static_mode(self):
        html = _render(refine_mode=False)
        assert 'id="btn-re-render"' not in html

    def test_unsaved_changes_modal_in_refine_mode(self):
        html = _render(refine_mode=True)
        assert 'unsaved-changes-modal' in html
        assert 'showUnsavedModal' in html
        assert 'unsaved-btn-save' in html
        assert 'unsaved-btn-discard' in html

    def test_no_unsaved_modal_in_static_mode(self):
        html = _render(refine_mode=False)
        assert 'unsaved-changes-modal' not in html

    def test_error_banner_in_refine_mode(self):
        html = _render(refine_mode=True)
        assert 'editor-error-banner' in html
        assert 'showEditorError' in html
        assert 'dismissEditorError' in html

    def test_no_error_banner_in_static_mode(self):
        html = _render(refine_mode=False)
        assert 'editor-error-banner' not in html

    def test_no_alert_in_rerender(self):
        """Re-render error path uses PF6 banner, not alert()."""
        html = _render(refine_mode=True)
        assert 'showEditorError' in html
        # The editor JS itself should not call alert(); the CM6 bundle may
        # contain alert() from third-party extensions (vim), so we check
        # only the editor_js template output.
        editor_js_start = html.find('function buildTree')
        editor_js_end = html.find('buildTree();\n  }', editor_js_start)
        if editor_js_start >= 0 and editor_js_end >= 0:
            editor_js = html[editor_js_start:editor_js_end]
            assert 'alert(' not in editor_js


class TestEditorDrawer:
    """PF6 resizable drawer for the editor tree pane.

    These tests verify rendered markup structure only. The following behaviors
    require a browser to verify manually:

    - Drag splitter to resize — panel width updates smoothly
    - Drag past 240px minimum — clamps to 240px
    - Drag past 600px maximum — clamps to 600px
    - Resize via ArrowLeft/ArrowRight — panel shrinks/grows by 20px per keystroke
    - Resize, then refresh page — width restores from localStorage
    - Resize, then re-render via button — width restores from localStorage
    """

    def test_drawer_uses_pf6_classes(self):
        html = _render(refine_mode=True)
        assert 'pf-v6-c-drawer' in html
        assert 'pf-m-panel-left' in html
        assert 'pf-m-resizable' in html
        assert 'pf-m-expanded' in html

    def test_drawer_default_width_340px(self):
        html = _render(refine_mode=True)
        assert '--pf-v6-c-drawer__panel--md--FlexBasis' in html
        assert '340px' in html

    def test_drawer_min_max_constraints(self):
        html = _render(refine_mode=True)
        assert '--pf-v6-c-drawer__panel--md--FlexBasis--min' in html
        assert '240px' in html
        assert '600px' in html

    def test_drawer_splitter_markup(self):
        html = _render(refine_mode=True)
        assert 'pf-v6-c-drawer__splitter' in html

    def test_drawer_has_panel_and_content_regions(self):
        html = _render(refine_mode=True)
        assert 'pf-v6-c-drawer__panel' in html
        assert 'pf-v6-c-drawer__content' in html

    def test_drawer_splitter_aria_attributes(self):
        """Splitter must carry static ARIA attributes for screen readers."""
        html = _render(refine_mode=True)
        assert 'aria-label="Resize file list"' in html
        assert 'aria-valuemin="240"' in html
        assert 'aria-valuemax="600"' in html
        assert 'aria-valuenow=' in html

    def test_drawer_no_fixed_300px_width(self):
        """Old fixed-width inline style must be removed."""
        html = _render(refine_mode=True)
        assert 'width:300px' not in html

    def test_drawer_static_mode_unaffected(self):
        """Static report must not reference drawer resize JS."""
        html = _render(refine_mode=False)
        assert 'yoinkc-editor-drawer-width' not in html


class TestEditorIntegration:
    """End-to-end tests verifying the complete editor feature set."""

    def test_static_report_has_no_editor_artifacts(self):
        """Static report (refine_mode=False) must have zero editor artifacts."""
        html = _render(refine_mode=False, with_content=True)
        assert 'id="editor-tab"' not in html
        assert 'new-file-modal' not in html
        assert 'unsaved-changes-modal' not in html
        assert 'editor-error-banner' not in html
        assert 'CMEditor' not in html
        assert 'editorSave' not in html
        assert 'id="btn-re-render"' not in html
        assert 'class="pf-v6-c-button pf-m-plain editor-icon"' not in html
        assert 'navigateToEditor' not in html
        # Existing file browser is intact
        assert 'id="file-viewer-content"' in html
        assert 'File browser</a>' in html

    def test_refine_mode_has_all_editor_components(self):
        """Refine mode report has every editor component."""
        html = _render(refine_mode=True, with_content=True)
        # Editor tab
        assert 'id="editor-tab"' in html
        assert 'id="btn-new-file"' in html
        # CodeMirror
        assert 'CMEditor' in html
        # Core editor functions
        assert 'editorSave' in html
        assert 'editorSaveAll' in html
        assert 'editorRevert' in html
        assert 'editorDeleteFile' in html
        assert 'buildTree' in html
        assert 'selectFile' in html
        assert 'setupDirtyTracking' in html
        assert 'findFileInSnapshot' in html
        # New file modal
        assert 'new-file-modal' in html
        assert 'createNewFile' in html
        assert 'validateNewFileForm' in html
        # Unsaved changes modal
        assert 'unsaved-changes-modal' in html
        assert 'showUnsavedModal' in html
        # Error banner
        assert 'editor-error-banner' in html
        assert 'showEditorError' in html
        # Re-render button
        assert 'id="btn-re-render"' in html
        assert 'editor-changed-count' in html
        # Cross-tab editor buttons (pencil icon)
        assert 'class="pf-v6-c-button pf-m-plain editor-icon"' in html
        assert 'navigateToEditor' in html
        # Keyboard shortcut
        assert "e.key === 's'" in html
        # Original snapshot embedded separately
        assert 'var originalSnapshot' in html
        assert 'JSON.parse(JSON.stringify(snapshot))' not in html
        # Sidebar says Editor
        assert 'Editor</a>' in html
