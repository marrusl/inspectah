"""Editor tab tests: refine mode rendering, static mode preservation."""

import tempfile
from pathlib import Path

from yoinkc.renderers import run_all as run_all_renderers
from yoinkc.schema import InspectionSnapshot, OsRelease


def _render(refine_mode=False):
    snapshot = InspectionSnapshot(
        meta={"host_root": "/host"},
        os_release=OsRelease(name="RHEL", version_id="9.6", pretty_name="RHEL 9.6"),
    )
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
