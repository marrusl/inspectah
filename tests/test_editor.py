"""Editor tab tests: refine mode rendering, static mode preservation."""

import re
import tempfile
from pathlib import Path

from yoinkc.renderers import run_all as run_all_renderers
from yoinkc.schema import (
    ConfigFileEntry,
    ConfigFileKind,
    ConfigSection,
    ContainerSection,
    FleetPrevalence,
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


def _render_fleet_variants(tmp_path, variant_count=2, selected_index=None):
    hosts = [f"web-0{i + 1}" for i in range(variant_count)]
    snapshot = InspectionSnapshot(
        meta={
            "host_root": "/host",
            "fleet": {
                "source_hosts": hosts,
                "total_hosts": variant_count,
                "min_prevalence": 100,
            },
        },
        os_release=OsRelease(name="RHEL", version_id="9.6", pretty_name="RHEL 9.6"),
        config=ConfigSection(files=[
            ConfigFileEntry(
                path="/etc/myapp/app.conf",
                kind=ConfigFileKind.UNOWNED,
                content=f"variant={i}",
                include=(selected_index == i),
                fleet=FleetPrevalence(count=1, total=variant_count, hosts=[hosts[i]]),
            )
            for i in range(variant_count)
        ]),
    )
    run_all_renderers(snapshot, tmp_path, refine_mode=True)
    return (tmp_path / "report.html").read_text()


def _extract_js_function(html: str, name: str) -> str:
    marker = f"function {name}"
    start = html.find(marker)
    assert start >= 0, f"could not find JS function {name}"
    brace_start = html.find("{", start)
    assert brace_start >= 0, f"could not find opening brace for {name}"
    depth = 0
    for idx in range(brace_start, len(html)):
        ch = html[idx]
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
            if depth == 0:
                return html[start:idx + 1]
    raise AssertionError(f"could not find closing brace for {name}")


def _extract_js_block(html: str, start_marker: str, end_marker: str) -> str:
    start = html.find(start_marker)
    assert start >= 0, f"could not find start marker {start_marker!r}"
    end = html.find(end_marker, start)
    assert end >= 0, f"could not find end marker {end_marker!r}"
    return html[start:end]


def _extract_opening_tag_attrs(html: str, selector: str) -> str:
    if selector == "editor-tab":
        pattern = r'<div id="editor-tab"\s+class="[^"]+"\s+style="([^"]+)"'
    elif selector == "drawer-panel":
        pattern = r'<div class="([^"]*\bpf-v6-c-drawer__panel\b[^"]*)"(?:\s+[^>]*)?>'
    else:
        raise AssertionError(f"unsupported selector {selector!r}")
    match = re.search(pattern, html)
    assert match, f"could not find opening tag for {selector}"
    return match.group(1)


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
    """PF6 fixed-width drawer for the editor tree pane.

    These tests verify rendered markup structure only. The following behaviors
    require a browser to verify manually:

    - File tree renders at a fixed 480px width
    - No drag handle is visible between the tree and editor panes
    - Panel border cleanly separates the tree from the editor content
    """

    def test_drawer_uses_pf6_classes(self):
        html = _render(refine_mode=True)
        match = re.search(r'<div id="editor-tab"\s+class="([^"]+)"', html)
        assert match
        classes = set(match.group(1).split())
        assert "pf-v6-c-drawer" in classes
        assert "pf-m-expanded" in classes
        assert "pf-m-panel-left" in classes
        assert "pf-m-inline" in classes
        assert "pf-m-resizable" not in classes

    def test_drawer_panel_is_not_resizable(self):
        html = _render(refine_mode=True)
        panel_classes = set(_extract_opening_tag_attrs(html, "drawer-panel").split())
        assert "pf-v6-c-drawer__panel" in panel_classes
        assert "pf-m-resizable" not in panel_classes

    def test_drawer_default_width_480px(self):
        html = _render(refine_mode=True)
        style = _extract_opening_tag_attrs(html, "editor-tab")
        assert "--pf-v6-c-drawer__panel--md--FlexBasis: 480px;" in style

    def test_drawer_fixed_width_constraints(self):
        html = _render(refine_mode=True)
        style = _extract_opening_tag_attrs(html, "editor-tab")
        assert "--pf-v6-c-drawer__panel--md--FlexBasis--min: 480px;" in style
        assert "--pf-v6-c-drawer__panel--md--FlexBasis--max: 480px;" in style
        assert style.count("480px") == 3

    def test_drawer_omits_splitter_markup(self):
        html = _render(refine_mode=True)
        assert 'class="pf-v6-c-drawer__splitter' not in html

    def test_drawer_has_panel_and_content_regions(self):
        html = _render(refine_mode=True)
        assert 'pf-v6-c-drawer__panel' in html
        assert 'pf-v6-c-drawer__content' in html

    def test_drawer_uses_default_pf_visual_separator(self):
        html = _render(refine_mode=True)
        panel_classes = set(_extract_opening_tag_attrs(html, "drawer-panel").split())
        assert "pf-m-no-border" not in panel_classes
        assert '<div class="pf-v6-c-drawer__panel" style=' not in html

    def test_drawer_no_fixed_300px_width(self):
        """Old fixed-width inline style must be removed."""
        html = _render(refine_mode=True)
        assert 'width:300px' not in html

    def test_drawer_refine_mode_omits_resize_js_state(self):
        html = _render(refine_mode=True)
        assert 'yoinkc-editor-drawer-width' not in html
        assert 'DRAWER_LS_KEY' not in html
        assert 'applyDrawerWidth' not in html

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


class TestVariantCompare:

    def test_update_compare_buttons_shows_compare_for_two_way_tie(self, tmp_path):
        html = _render_fleet_variants(tmp_path, variant_count=2)
        fn = _extract_js_function(html, "updateCompareButtons")
        assert "variant-display-btn" in fn
        assert "variant-compare-btn" in fn
        # 2-way ties use Compare (peer mode), not Display
        assert "rows.length === 2" in fn

    def test_update_compare_buttons_shows_display_for_three_plus_way_tie(self, tmp_path):
        html = _render_fleet_variants(tmp_path, variant_count=3)
        fn = _extract_js_function(html, "updateCompareButtons")
        # 3+ way ties use Display for individual inspection
        assert "displayBtn.textContent = 'Display'" in fn
        # When a variant IS selected, buttons should become Compare
        assert "compareBtn.textContent = 'Compare'" in fn

    def test_compare_click_on_two_variant_group_without_selection_uses_peer_mode(self, tmp_path):
        html = _render_fleet_variants(tmp_path, variant_count=2)
        block = _extract_js_block(
            html,
            "/* Fleet: Compare button click delegation */",
            "/* Fleet: keyboard activation for fleet bars */",
        )
        assert "if (!selectedItem) {" in block
        assert "if (rows.length !== 2) return;" in block
        assert "showCompareModal(group, otherItem, comparisonItem, true);" in block

    def test_compare_modal_peer_mode_shows_use_buttons_instead_of_switch_button(self, tmp_path):
        html = _render_fleet_variants(tmp_path, variant_count=2)
        fn = _extract_js_function(html, "showCompareModal")
        assert "function showCompareModal(path, selectedItem, comparisonItem, peerMode)" in fn
        assert 'data-action="use-a"' in fn
        assert 'data-action="use-b"' in fn
        assert "Use Variant A" in fn
        assert "Use Variant B" in fn

    def test_use_variant_a_button_selects_that_variant_and_excludes_the_other(self, tmp_path):
        html = _render_fleet_variants(tmp_path, variant_count=2)
        modal_fn = _extract_js_function(html, "showCompareModal")
        select_fn = _extract_js_function(html, "applyVariantSelection")
        assert "applyVariantSelection(path, selectedItem);" in modal_fn
        assert "applyVariantSelection(path, comparisonItem);" in modal_fn
        assert "arr[idx].include = isTarget;" in select_fn
        assert "if (cb) cb.checked = isTarget;" in select_fn
        assert "row.classList.toggle('excluded', !isTarget);" in select_fn

    def test_compare_buttons_are_synced_on_initial_page_load(self, tmp_path):
        html = _render_fleet_variants(tmp_path, variant_count=2)
        assert "var initialCompareGroups = {};" in html
        assert "Object.keys(initialCompareGroups).forEach(function(g) { updateCompareButtons(g); });" in html

    def test_editor_tree_shows_compare_label_for_two_way_ties(self, tmp_path):
        html = _render_fleet_variants(tmp_path, variant_count=2)
        build_tree_fn = _extract_js_function(html, "buildTree")
        # 2-way ties show Compare (peer mode A-vs-B)
        assert "entries.length === 2" in build_tree_fn
        assert "compareBtn.textContent = 'Compare'" in build_tree_fn

    def test_editor_tree_shows_display_label_for_three_plus_ties(self, tmp_path):
        html = _render_fleet_variants(tmp_path, variant_count=3)
        build_tree_fn = _extract_js_function(html, "buildTree")
        # 3+ ties show Display for individual inspection
        assert "compareBtn.textContent = 'Display'" in build_tree_fn
        assert "compareBtn.textContent = 'Compare'" in build_tree_fn

    def test_compare_from_editor_shows_display_modal_for_ties_with_many_variants(self, tmp_path):
        html = _render_fleet_variants(tmp_path, variant_count=2)
        fn = _extract_js_function(html, "compareFromEditor")
        assert "var siblings = findSiblingVariants(section, list, path);" in fn
        assert "if (!selectedItem) {" in fn
        # 2-variant ties still use peer compare
        assert "if (siblings.length === 2) {" in fn
        assert "showCompareModal(path, selectedItem, comparisonItem, true);" in fn
        # 3+ variant ties use Display modal
        assert "showDisplayModal(path, comparisonItem);" in fn
