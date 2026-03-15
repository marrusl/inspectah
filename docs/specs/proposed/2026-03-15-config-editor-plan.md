# Config Editor Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Transform the read-only file browser into an interactive config editor in yoinkc-refine mode, allowing operators to edit/create config files, quadlet units, and systemd drop-ins with changes flowing through re-render into export-ready output.

**Architecture:** Client-side snapshot editing with CodeMirror 6. The browser holds `snapshot` (mutable) and `originalSnapshot` (immutable) JS objects. Save writes to `snapshot` instantly. Re-render POSTs both to `/api/re-render`, which runs yoinkc and returns new HTML. yoinkc-refine serves CodeMirror as static assets and injects a `refine_mode` template variable.

**Tech Stack:** Python 3.9+, Jinja2 templates, CodeMirror 6 (vendored), vanilla ES5 JavaScript, PatternFly 6 CSS, pytest.

**Spec:** `docs/specs/proposed/2026-03-15-config-editor-design.md`

---

## File Structure

### New Files

| File | Responsibility |
|------|----------------|
| `src/yoinkc/templates/report/_editor.html.j2` | Editor tab: two-pane layout (tree + content), read-only and edit modes |
| `src/yoinkc/templates/report/_editor_js.html.j2` | Editor JavaScript: CodeMirror init, save/revert/state tracking, new file modal |
| `src/yoinkc/templates/report/_new_file_modal.html.j2` | PF6 modal for creating new files (config/quadlet/drop-in) |
| `src/yoinkc/static/codemirror/codemirror.min.js` | Vendored CodeMirror 6 bundle (basic setup) |
| `src/yoinkc/static/codemirror/codemirror.min.css` | Vendored CodeMirror 6 styles |
| `tests/test_editor.py` | Editor-specific tests: refine mode rendering, editor tab, state labels |
| `tests/test_new_file.py` | New file creation tests: snapshot mutation, validation, path assembly |

### Modified Files

| File | Changes |
|------|---------|
| `yoinkc-refine` | Wrapper request format, static asset serving, `refine_mode` injection, `original_snapshot_json` passthrough |
| `src/yoinkc/renderers/html_report.py` | `_build_context()`: add `refine_mode`, `original_snapshot_json`. `_build_output_tree()`: add drop-ins folder |
| `src/yoinkc/templates/report/_js.html.j2` | Snapshot init from two variables (not deep copy), re-render sends wrapper, editor JS include |
| `src/yoinkc/templates/report/_file_browser.html.j2` | Conditional: show editor (refine) or read-only browser (static) |
| `src/yoinkc/templates/report/_config.html.j2` | Replace content pulldowns with "View & edit in editor →" links in refine mode |
| `src/yoinkc/templates/report/_containers.html.j2` | Same: "View & edit in editor →" links in refine mode |
| `src/yoinkc/templates/report/_services.html.j2` | Same: "View & edit in editor →" links for drop-ins in refine mode |
| `src/yoinkc/templates/report/_toolbar.html.j2` | Re-render button with changed-file count |
| `src/yoinkc/templates/report/_sidebar.html.j2` | Tab name: "Editor" (refine) vs "Files" (static) |
| `src/yoinkc/renderers/audit_report.py` | Modifications section: edited/added file lists |
| `tests/test_html_report_output.py` | Tests for refine_mode=False preserving existing behavior |
| `tests/test_audit_report_output.py` | Tests for modifications section |

---

## Chunk 1: Foundation — Server-Side & Rendering Changes

### Task 1: Re-render API Contract Change

The existing `/api/re-render` accepts a bare snapshot JSON body. Change it to accept a wrapper object `{"snapshot": {...}, "original": {...}}` and pass both through to yoinkc.

**Files:**
- Modify: `yoinkc-refine` (lines 341-367: do_POST handler, lines 163-231: _re_render function)
- Test: `tests/test_html_report_output.py`

- [ ] **Step 1: Update yoinkc-refine do_POST handler**

Note: yoinkc-refine is a standalone script and not easily unit-testable. This task is tested via the integration test in Task 14 and the JS-side wrapper format change.


In `yoinkc-refine`, modify the `/api/re-render` handler (around line 345):

```python
if path == "/api/re-render":
    body = self._read_body()
    payload = json.loads(body)
    # Support both wrapper format and legacy bare snapshot
    if "snapshot" in payload and "original" in payload:
        snapshot_data = payload["snapshot"]
        original_data = payload["original"]
    else:
        # Legacy: bare snapshot (backwards compat for existing callers)
        snapshot_data = payload
        original_data = None
    snapshot_bytes = json.dumps(snapshot_data).encode()
    original_bytes = json.dumps(original_data).encode() if original_data else None
    excluded = _count_excluded(snapshot_data)
    ok, result = _re_render(snapshot_bytes, output_dir, original_bytes)
```

- [ ] **Step 3: Update _re_render to accept optional original snapshot**

Modify `_re_render()` signature and implementation to pass original snapshot as a second mounted file:

```python
def _re_render(
    snapshot_data: bytes,
    output_dir: Path,
    original_data: Optional[bytes] = None,
) -> Tuple[bool, str]:
```

When `original_data` is provided, write it to the temp input dir as `original-snapshot.json` and mount it alongside the snapshot. yoinkc will detect this file and embed it as `original_snapshot_json` in the template context.

- [ ] **Step 4: Update _js.html.j2 re-render fetch to send wrapper**

In `src/yoinkc/templates/report/_js.html.j2` (around line 450-453), change the fetch body:

```javascript
body: JSON.stringify({snapshot: snapshot, original: originalSnapshot}),
```

- [ ] **Step 5: Run existing tests to verify nothing breaks**

Run: `cd yoinkc && python -m pytest tests/ -v`
Expected: All existing tests pass (static report path unchanged).

- [ ] **Step 6: Commit**

```bash
git add yoinkc-refine src/yoinkc/templates/report/_js.html.j2 tests/
git commit -m "refactor: re-render API accepts wrapper with snapshot and original"
```

---

### Task 2: Original Snapshot Embedding

Change the template from deep-copying `snapshot` to using a separately embedded `originalSnapshot`. This ensures the original survives re-render page replacements.

**Files:**
- Modify: `src/yoinkc/renderers/html_report.py` (lines 472-625: _build_context)
- Modify: `src/yoinkc/templates/report/_js.html.j2` (lines 4-5: snapshot init)
- Test: `tests/test_html_report_output.py`

- [ ] **Step 1: Write failing test**

```python
def test_original_snapshot_json_in_context(self):
    """Template context includes original_snapshot_json for editor."""
    # Render a report and check that both snapshot_json and
    # original_snapshot_json appear in the HTML output
    html = self._render_report()
    assert "var snapshot" in html
    assert "var originalSnapshot" in html
    # Should NOT use JSON.parse(JSON.stringify(snapshot)) anymore
    assert "JSON.parse(JSON.stringify(snapshot))" not in html
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd yoinkc && python -m pytest tests/test_html_report_output.py::TestHtmlReport::test_original_snapshot_json_in_context -v`
Expected: FAIL — `JSON.parse(JSON.stringify(snapshot))` still present.

- [ ] **Step 3: Add original_snapshot_json to _build_context()**

In `html_report.py`, in `_build_context()` around line 609 where `snapshot_json` is created:

```python
snapshot_json = snapshot.model_dump_json().replace("</", "<\\/")
original_snapshot_json = snapshot_json  # Same at initial render; re-render passes the real original
```

Add to the returned dict:
```python
"original_snapshot_json": original_snapshot_json,
```

- [ ] **Step 4: Update _js.html.j2 snapshot initialization**

Replace lines 4-5:
```javascript
var snapshot = {{ snapshot_json|safe }};
var originalSnapshot = JSON.parse(JSON.stringify(snapshot));
```

With:
```javascript
var snapshot = {{ snapshot_json|safe }};
{% if original_snapshot_json is defined %}
var originalSnapshot = {{ original_snapshot_json|safe }};
{% else %}
var originalSnapshot = JSON.parse(JSON.stringify(snapshot));
{% endif %}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cd yoinkc && python -m pytest tests/test_html_report_output.py::TestHtmlReport::test_original_snapshot_json_in_context -v`
Expected: PASS.

- [ ] **Step 6: Run full test suite**

Run: `cd yoinkc && python -m pytest tests/ -v`
Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/yoinkc/renderers/html_report.py src/yoinkc/templates/report/_js.html.j2 tests/
git commit -m "feat: embed originalSnapshot separately for editor support"
```

---

### Task 3: Refine Mode Template Variable

Add `refine_mode` to the template context. yoinkc-refine sets it to `True` when serving; static renders get `False`.

**Files:**
- Modify: `src/yoinkc/renderers/html_report.py` (_build_context)
- Modify: `yoinkc-refine` (pass refine_mode to yoinkc)
- Test: `tests/test_html_report_output.py`

- [ ] **Step 1: Write failing test**

```python
def test_refine_mode_defaults_to_false(self):
    """Static report has refine_mode=False in context."""
    html = self._render_report()
    # The template should have a data attribute or JS variable for refine mode
    assert 'var refineMode = false' in html
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd yoinkc && python -m pytest tests/test_html_report_output.py::TestHtmlReport::test_refine_mode_defaults_to_false -v`
Expected: FAIL.

- [ ] **Step 3: Add refine_mode to _build_context()**

In `html_report.py`, add a `refine_mode` parameter to `_build_context()`:

```python
def _build_context(
    snapshot: InspectionSnapshot,
    output_dir: Path,
    env: Environment,
    refine_mode: bool = False,
) -> dict:
```

Add to returned dict:
```python
"refine_mode": refine_mode,
```

- [ ] **Step 4: Add refine_mode JS variable to _js.html.j2**

After the snapshot initialization lines:
```javascript
var refineMode = {{ 'true' if refine_mode else 'false' }};
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cd yoinkc && python -m pytest tests/test_html_report_output.py::TestHtmlReport::test_refine_mode_defaults_to_false -v`
Expected: PASS.

- [ ] **Step 6: Update yoinkc-refine to pass refine_mode=True**

In `yoinkc-refine`, add `--refine-mode` to the yoinkc container command (in `_re_render()`). In yoinkc's CLI entry point (likely `__main__.py` or `cli.py`), add an `--refine-mode` flag:

```python
parser.add_argument("--refine-mode", action="store_true", help="Enable editor UI in rendered report")
```

Pass `refine_mode=args.refine_mode` through to `html_report.render()` → `_build_context()`.

In `yoinkc-refine`'s `_re_render()`, append `"--refine-mode"` to the container command args:

```python
cmd = [... existing args ..., "--refine-mode"]
```

- [ ] **Step 7: Commit**

```bash
git add src/yoinkc/renderers/html_report.py src/yoinkc/templates/report/_js.html.j2 yoinkc-refine tests/
git commit -m "feat: add refine_mode template variable for editor conditionals"
```

---

### Task 4: Drop-ins in Output Tree

The file browser tree currently walks `config/` and `quadlet/`. Add `drop-ins/` as a third root folder so systemd drop-in files appear in the editor tree.

**Files:**
- Modify: `src/yoinkc/renderers/html_report.py` (lines 41-80: _build_output_tree)
- Test: `tests/test_html_report_output.py`

- [ ] **Step 1: Write failing test**

```python
def test_output_tree_includes_dropins(self):
    """File browser tree includes drop-ins folder."""
    html = self._render_report_with_dropins()
    assert 'drop-ins' in html or 'drop_ins' in html
```

The test helper `_render_report_with_dropins()` should create a snapshot with at least one `SystemdDropIn` entry and render it. The drop-in files should be written to `output_dir/drop-ins/` during the render pipeline.

- [ ] **Step 2: Run test to verify it fails**

Expected: FAIL — drop-ins folder not walked by `_build_output_tree()`.

- [ ] **Step 3: Add drop-ins to _build_output_tree()**

In `html_report.py`, in `_build_output_tree()` (around line 50), add `"drop-ins"` to the folder list:

```python
for folder_name in ("config", "quadlet", "drop-ins"):
    folder = output_dir / folder_name
    if folder.is_dir():
        children = _walk_dir(folder, folder)
        if children:
            roots.append({"type": "dir", "name": folder_name, "children": children})
```

- [ ] **Step 4: Ensure drop-in files are written to output_dir/drop-ins/**

Check `src/yoinkc/renderers/containerfile/` for how config and quadlet files are written to `output_dir/config/` and `output_dir/quadlet/`. Add equivalent logic for drop-ins: iterate `snapshot.services.drop_ins`, create `output_dir/drop-ins/` directory, write each drop-in's `content` to a file at the relative path derived from `drop_in.path`. If drop-ins are already written to disk as part of config output, just verify the folder name and adjust `_build_output_tree()` accordingly.

- [ ] **Step 5: Run test to verify it passes**

Run: `cd yoinkc && python -m pytest tests/test_html_report_output.py::TestHtmlReport::test_output_tree_includes_dropins -v`
Expected: PASS.

- [ ] **Step 6: Run full test suite**

Run: `cd yoinkc && python -m pytest tests/ -v`

- [ ] **Step 7: Commit**

```bash
git add src/yoinkc/renderers/html_report.py tests/
git commit -m "feat: include drop-ins folder in file browser tree"
```

---

## Chunk 2: CodeMirror & Editor Tab

### Task 5: Vendor CodeMirror 6

Bundle CodeMirror 6 basic setup and configure yoinkc-refine to serve it as static assets.

**Files:**
- Create: `src/yoinkc/static/codemirror/codemirror.min.js`
- Create: `src/yoinkc/static/codemirror/codemirror.min.css`
- Modify: `yoinkc-refine` (static file serving)

- [ ] **Step 1: Create CodeMirror 6 bundle**

Use the CodeMirror 6 bundling approach. Create a minimal bundle with:
- `@codemirror/basic-setup` (line numbers, syntax highlighting, keybindings)
- `@codemirror/lang-javascript` is NOT needed — we want plain text with line numbers

Use a pre-built CM6 bundle and vendor it (air-gap friendly). Create a small npm project in a temp directory to build the bundle:

```bash
mkdir /tmp/cm6-build && cd /tmp/cm6-build
npm init -y
npm install @codemirror/basic-setup @codemirror/view @codemirror/state
```

Create `build.js`:
```javascript
import {basicSetup} from "@codemirror/basic-setup";
import {EditorView} from "@codemirror/view";
import {EditorState} from "@codemirror/state";
// ... bundle and export CMEditor API
```

Use esbuild to produce a single-file bundle:
```bash
npx esbuild build.js --bundle --outfile=codemirror.min.js --minify --format=iife --global-name=CMEditor
```

Copy `codemirror.min.js` and extract CSS to `src/yoinkc/static/codemirror/`. Delete the temp directory.

The bundle should export a single `createEditor(parentElement, content, options)` function that initializes a CM6 instance.

```javascript
// Exported API for yoinkc:
window.CMEditor = {
  create: function(parent, content) { /* returns EditorView */ },
  getContent: function(view) { /* returns string */ },
  setContent: function(view, content) { /* replaces content */ },
};
```

- [ ] **Step 2: Add static file serving to yoinkc-refine**

In `yoinkc-refine`, add a route for `/static/` that serves files from yoinkc's `src/yoinkc/static/` directory:

```python
elif path.startswith("/static/"):
    static_root = Path(__file__).parent / "src" / "yoinkc" / "static"
    file_path = static_root / path[len("/static/"):]
    if file_path.is_file() and static_root in file_path.resolve().parents:
        content_type = {
            ".js": "application/javascript",
            ".css": "text/css",
        }.get(file_path.suffix, "application/octet-stream")
        self._send(200, file_path.read_text(), content_type)
    else:
        self._send(404, "Not found")
```

Note: Use `resolve().parents` check to prevent path traversal.

- [ ] **Step 3: Test static serving manually**

Start yoinkc-refine and verify:
```bash
curl -s http://localhost:PORT/static/codemirror/codemirror.min.js | head -1
```
Expected: JavaScript content.

- [ ] **Step 4: Commit**

```bash
git add src/yoinkc/static/codemirror/ yoinkc-refine
git commit -m "feat: vendor CodeMirror 6 and serve as static assets"
```

---

### Task 6: Editor Tab — Read-Only Mode

Create the editor tab template with the two-pane layout. Initially read-only: tree on the left, file content on the right with an Edit button. Conditionally rendered in refine mode; static report keeps the existing file browser.

**Files:**
- Create: `src/yoinkc/templates/report/_editor.html.j2`
- Modify: `src/yoinkc/templates/report/_file_browser.html.j2` (conditional include)
- Modify: `src/yoinkc/templates/report/_sidebar.html.j2` (tab name)
- Test: `tests/test_editor.py`

- [ ] **Step 1: Write failing test**

Create `tests/test_editor.py`:

```python
class TestEditorTab:
    def test_refine_mode_shows_editor_tab(self):
        """In refine mode, the Editor tab replaces the File Browser tab."""
        html = self._render_report(refine_mode=True)
        assert 'id="editor-tab"' in html
        assert 'Edit' in html  # Edit button present

    def test_static_mode_shows_file_browser(self):
        """In static mode, the read-only file browser is shown."""
        html = self._render_report(refine_mode=False)
        assert 'id="file-browser"' in html or 'file-viewer-content' in html
        assert 'id="editor-tab"' not in html
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd yoinkc && python -m pytest tests/test_editor.py -v`
Expected: FAIL — editor tab doesn't exist yet.

- [ ] **Step 3: Create _editor.html.j2**

```html
{# Editor tab — refine mode only. Two-pane layout: file tree + content viewer/editor. #}
<div id="editor-tab" class="pf-v6-c-page__main-section">
  <div style="display:flex; height:calc(100vh - 200px); border:1px solid var(--pf-t--global--border--color--default);">

    {# --- Tree pane --- #}
    <div id="editor-tree" style="width:300px; border-right:1px solid var(--pf-t--global--border--color--default); background:var(--pf-t--global--background--color--secondary--default); display:flex; flex-direction:column; overflow:hidden;">
      <div style="padding:12px 16px; border-bottom:1px solid var(--pf-t--global--border--color--default); display:flex; justify-content:space-between; align-items:center;">
        <span class="pf-v6-c-title pf-m-lg">Files</span>
        <button class="pf-v6-c-button pf-m-primary pf-m-small" id="btn-new-file" type="button">+ New File</button>
      </div>
      <div id="editor-tree-list" style="padding:8px; overflow-y:auto; flex:1;">
        {# Tree built by JS from snapshot data #}
      </div>
    </div>

    {# --- Content pane --- #}
    <div id="editor-content" style="flex:1; display:flex; flex-direction:column;">
      {# Toolbar #}
      <div id="editor-toolbar" style="padding:8px 16px; border-bottom:1px solid var(--pf-t--global--border--color--default); display:flex; justify-content:space-between; align-items:center; min-height:48px;">
        <div id="editor-file-path" style="font-family:var(--pf-t--global--font--family--mono);"></div>
        <div id="editor-actions"></div>
      </div>
      {# Content area #}
      <div id="editor-view" style="flex:1; overflow:auto; padding:16px;">
        <pre id="editor-readonly-content" style="margin:0; white-space:pre-wrap; font-family:var(--pf-t--global--font--family--mono); font-size:14px;"></pre>
        <div id="editor-cm-container" style="display:none; height:100%;"></div>
      </div>
    </div>
  </div>
</div>
```

- [ ] **Step 4: Update _file_browser.html.j2 with conditional**

Wrap the existing content and add the editor include:

```html
{% if refine_mode %}
  {% include "report/_editor.html.j2" %}
{% else %}
  {# Existing read-only file browser content stays here unchanged #}
  <div id="file-browser" ...>
    ...existing content...
  </div>
{% endif %}
```

- [ ] **Step 5: Update _sidebar.html.j2 tab name**

Change the Files/File Browser tab label:
```html
<li>{{ "Editor" if refine_mode else "Files" }}</li>
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cd yoinkc && python -m pytest tests/test_editor.py -v`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/yoinkc/templates/report/_editor.html.j2 src/yoinkc/templates/report/_file_browser.html.j2 src/yoinkc/templates/report/_sidebar.html.j2 tests/test_editor.py
git commit -m "feat: add editor tab template with two-pane layout"
```

---

### Task 7: Editor JavaScript — Tree Building, Read-Only View, Edit Mode

Build the JS that populates the editor tree from the snapshot, handles file selection (read-only), and toggles to CodeMirror edit mode.

**Files:**
- Create: `src/yoinkc/templates/report/_editor_js.html.j2`
- Modify: `src/yoinkc/templates/report/_js.html.j2` (include editor JS)
- Test: `tests/test_editor.py`

- [ ] **Step 1: Create _editor_js.html.j2 — tree building**

```javascript
{% if refine_mode %}
<script>
(function() {
  // --- Editor state ---
  var editorState = {
    currentFile: null,      // {section, list, index, path}
    cmView: null,           // CodeMirror EditorView instance
    editMode: false,        // true when CodeMirror is active
    dirtyFiles: new Map(),  // path -> buffer content (unsaved edits)
    savedFiles: new Set(),  // paths saved but not yet re-rendered
  };

  // --- Build file tree from snapshot ---
  function buildTree() {
    var treeEl = document.getElementById('editor-tree-list');
    treeEl.innerHTML = '';

    var sections = [
      {section: 'config', list: 'files', label: 'config', pathPrefix: 'config/'},
      {section: 'containers', list: 'quadlet_units', label: 'quadlet', pathPrefix: 'quadlet/'},
      {section: 'services', list: 'drop_ins', label: 'drop-ins', pathPrefix: 'drop-ins/'},
    ];

    sections.forEach(function(sec) {
      var items = snapshot[sec.section] && snapshot[sec.section][sec.list];
      if (!items || items.length === 0) return;

      // Folder header
      var folder = document.createElement('div');
      folder.className = 'editor-tree-dir';
      folder.textContent = '▾ ' + sec.label + '/';
      folder.style.cssText = 'font-weight:600; padding:6px 0; cursor:pointer;';
      treeEl.appendChild(folder);

      // File entries
      items.forEach(function(item, idx) {
        var entry = document.createElement('div');
        entry.className = 'editor-tree-file';
        entry.style.cssText = 'padding:6px 8px 6px 24px; display:flex; align-items:center; justify-content:space-between; cursor:pointer; border-radius:4px;';
        entry.dataset.section = sec.section;
        entry.dataset.list = sec.list;
        entry.dataset.index = idx;
        entry.dataset.path = item.path;

        var nameSpan = document.createElement('span');
        nameSpan.textContent = item.path.split('/').pop();
        nameSpan.title = item.path;
        nameSpan.style.cssText = 'color:#004080;';
        entry.appendChild(nameSpan);

        // State label placeholder
        var labelSpan = document.createElement('span');
        labelSpan.className = 'editor-state-label';
        entry.appendChild(labelSpan);

        entry.addEventListener('click', function() {
          selectFile(sec.section, sec.list, idx, item.path);
        });

        treeEl.appendChild(entry);
      });
    });
  }

  // --- Select file (read-only mode) ---
  function selectFile(section, list, index, path) {
    // Check for unsaved edits on current file
    if (editorState.editMode && editorState.currentFile && editorState.dirtyFiles.has(editorState.currentFile.path)) {
      if (!confirmNavigation()) return;
    }

    editorState.currentFile = {section: section, list: list, index: index, path: path};
    editorState.editMode = false;

    // Highlight in tree
    document.querySelectorAll('.editor-tree-file').forEach(function(el) {
      el.style.background = '';
    });
    var activeEntry = document.querySelector('.editor-tree-file[data-path="' + CSS.escape(path) + '"]');
    if (activeEntry) activeEntry.style.background = '#e7f1fa';

    // Show content read-only
    var content = snapshot[section][list][index].content || '';
    document.getElementById('editor-readonly-content').textContent = content;
    document.getElementById('editor-readonly-content').style.display = '';
    document.getElementById('editor-cm-container').style.display = 'none';

    // Update toolbar
    var pathEl = document.getElementById('editor-file-path');
    pathEl.innerHTML = '<span style="color:#6a6e73;">' + escapeHtml(path.substring(0, path.lastIndexOf('/') + 1)) + '</span>' +
                       '<strong>' + escapeHtml(path.split('/').pop()) + '</strong>' +
                       ' <span class="pf-v6-c-label pf-m-compact" style="background:#f0f0f0; color:#6a6e73;">read-only</span>';

    var actionsEl = document.getElementById('editor-actions');
    actionsEl.innerHTML = '<button class="pf-v6-c-button pf-m-primary pf-m-small" onclick="editorEnterEditMode()">Edit</button>';

    updateStateLabels();
  }

  // --- Enter edit mode ---
  window.editorEnterEditMode = function() {
    if (!editorState.currentFile) return;
    editorState.editMode = true;

    var f = editorState.currentFile;
    var content = editorState.dirtyFiles.has(f.path)
      ? editorState.dirtyFiles.get(f.path)
      : (snapshot[f.section][f.list][f.index].content || '');

    // Hide read-only, show CodeMirror
    document.getElementById('editor-readonly-content').style.display = 'none';
    var cmContainer = document.getElementById('editor-cm-container');
    cmContainer.style.display = '';
    cmContainer.innerHTML = '';

    if (window.CMEditor) {
      editorState.cmView = CMEditor.create(cmContainer, content);
    } else {
      // Fallback: plain textarea if CodeMirror not loaded
      var ta = document.createElement('textarea');
      ta.value = content;
      ta.style.cssText = 'width:100%; height:100%; font-family:var(--pf-t--global--font--family--mono); font-size:14px; border:none; resize:none; padding:8px;';
      ta.id = 'editor-textarea-fallback';
      cmContainer.appendChild(ta);
    }

    // Update toolbar
    updateEditToolbar();
  };

  // --- Update toolbar for edit mode ---
  function updateEditToolbar() {
    var f = editorState.currentFile;
    var pathEl = document.getElementById('editor-file-path');
    var stateLabel = getStateLabel(f.path);
    pathEl.innerHTML = '<span style="color:#6a6e73;">' + escapeHtml(f.path.substring(0, f.path.lastIndexOf('/') + 1)) + '</span>' +
                       '<strong>' + escapeHtml(f.path.split('/').pop()) + '</strong>' +
                       (stateLabel ? ' ' + stateLabel : '');

    var isCreated = isOperatorCreated(f.path);
    var actionsEl = document.getElementById('editor-actions');
    actionsEl.innerHTML =
      (isCreated ? '<button class="pf-v6-c-button pf-m-danger pf-m-small" onclick="editorDeleteFile()" style="margin-right:8px;">Delete</button>' : '') +
      '<button class="pf-v6-c-button pf-m-secondary pf-m-small" onclick="editorRevert()">Revert to snapshot</button> ' +
      '<button class="pf-v6-c-button pf-m-secondary pf-m-small" onclick="editorSaveAll()">Save All</button> ' +
      '<button class="pf-v6-c-button pf-m-primary pf-m-small" onclick="editorSave()">Save</button>';
  }

  // --- Helpers ---
  function escapeHtml(s) {
    var d = document.createElement('div');
    d.textContent = s;
    return d.innerHTML;
  }

  function getEditorContent() {
    if (editorState.cmView && window.CMEditor) {
      return CMEditor.getContent(editorState.cmView);
    }
    var ta = document.getElementById('editor-textarea-fallback');
    return ta ? ta.value : '';
  }

  function isOperatorCreated(path) {
    // File exists in snapshot but not in originalSnapshot
    var sections = [
      {section: 'config', list: 'files'},
      {section: 'containers', list: 'quadlet_units'},
      {section: 'services', list: 'drop_ins'},
    ];
    for (var i = 0; i < sections.length; i++) {
      var s = sections[i];
      var origItems = originalSnapshot[s.section] && originalSnapshot[s.section][s.list] || [];
      var found = origItems.some(function(item) { return item.path === path; });
      if (found) return false;
    }
    return true;
  }

  function getStateLabel(path) {
    if (editorState.dirtyFiles.has(path)) {
      return '<span class="pf-v6-c-label pf-m-compact pf-m-red" style="background:#faeae8; color:#a30000; border:1px solid #c9190b;">unsaved</span>';
    }
    if (editorState.savedFiles.has(path)) {
      return '<span class="pf-v6-c-label pf-m-compact pf-m-blue" style="background:#e7f1fa; color:#004080; border:1px solid #06c;">not rendered</span>';
    }
    return '';
  }

  function updateStateLabels() {
    document.querySelectorAll('.editor-tree-file').forEach(function(el) {
      var path = el.dataset.path;
      var labelEl = el.querySelector('.editor-state-label');
      if (labelEl) labelEl.innerHTML = getStateLabel(path);
    });
  }

  function confirmNavigation() {
    // Deliberate simplification: uses confirm() for initial implementation.
    // A PF6 modal (save/discard/cancel) should replace this in a follow-up task.
    return confirm('You have unsaved changes. Discard them?');
  }

  // --- Initialize ---
  if (refineMode) {
    buildTree();
  }
})();
{% endif %}
```

- [ ] **Step 2: Include editor JS in _js.html.j2**

At the end of `_js.html.j2`, add:
```html
{% include "report/_editor_js.html.j2" %}
```

- [ ] **Step 3: Add CodeMirror script/style tags in refine mode**

In the report template (or `_js.html.j2`), conditionally load CodeMirror:
```html
{% if refine_mode %}
<link rel="stylesheet" href="/static/codemirror/codemirror.min.css">
<script src="/static/codemirror/codemirror.min.js"></script>
{% endif %}
```

- [ ] **Step 4: Write tests for tree building and file selection**

In `tests/test_editor.py`:

```python
def test_editor_tree_built_from_snapshot(self):
    """Editor tree JS builds file entries from snapshot sections."""
    html = self._render_report(refine_mode=True)
    assert 'buildTree' in html
    assert "section: 'config'" in html or 'section:"config"' in html

def test_editor_has_codemirror_assets(self):
    """Refine mode loads CodeMirror CSS and JS."""
    html = self._render_report(refine_mode=True)
    assert '/static/codemirror/codemirror.min.js' in html
    assert '/static/codemirror/codemirror.min.css' in html

def test_static_mode_no_codemirror(self):
    """Static mode does not load CodeMirror."""
    html = self._render_report(refine_mode=False)
    assert '/static/codemirror/' not in html
```

- [ ] **Step 5: Run tests**

Run: `cd yoinkc && python -m pytest tests/test_editor.py -v`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/yoinkc/templates/report/_editor_js.html.j2 src/yoinkc/templates/report/_js.html.j2 tests/test_editor.py
git commit -m "feat: editor JS with tree building, file selection, and edit mode"
```

---

## Chunk 3: Save/Revert, Cross-Tab Linking, New Files

### Task 8: Save, Save All, Revert Actions

Implement the snapshot mutation functions and Ctrl+S keyboard shortcut.

**Files:**
- Modify: `src/yoinkc/templates/report/_editor_js.html.j2`
- Test: `tests/test_editor.py`

- [ ] **Step 1: Add save/revert functions to _editor_js.html.j2**

Add inside the IIFE, after the helpers:

```javascript
// --- Save current file ---
window.editorSave = function() {
  if (!editorState.currentFile || !editorState.editMode) return;
  var f = editorState.currentFile;
  var content = getEditorContent();
  snapshot[f.section][f.list][f.index].content = content;
  editorState.dirtyFiles.delete(f.path);
  editorState.savedFiles.add(f.path);
  setDirty(true);  // Mark report as having unsaved changes (existing function)
  updateEditToolbar();
  updateStateLabels();
  updateReRenderCount();
};

// --- Save all unsaved files ---
window.editorSaveAll = function() {
  // Save current file first if in edit mode
  if (editorState.currentFile && editorState.editMode) {
    var f = editorState.currentFile;
    var content = getEditorContent();
    editorState.dirtyFiles.set(f.path, content);
  }

  editorState.dirtyFiles.forEach(function(content, path) {
    var loc = findFileInSnapshot(path);
    if (loc) {
      snapshot[loc.section][loc.list][loc.index].content = content;
      editorState.savedFiles.add(path);
    }
  });
  editorState.dirtyFiles.clear();
  setDirty(true);
  updateEditToolbar();
  updateStateLabels();
  updateReRenderCount();
};

// --- Revert to original snapshot ---
window.editorRevert = function() {
  if (!editorState.currentFile) return;
  var f = editorState.currentFile;
  var origItems = originalSnapshot[f.section] && originalSnapshot[f.section][f.list] || [];
  var origItem = origItems.find(function(item) { return item.path === f.path; });
  var origContent = origItem ? origItem.content : '';

  snapshot[f.section][f.list][f.index].content = origContent;
  editorState.dirtyFiles.delete(f.path);
  // Only mark as "not rendered" if snapshot content now differs from original
  // (i.e., there were prior edits that were re-rendered, and reverting changes things)
  // If content matches original AND no prior re-renders changed it, state is Clean.
  editorState.savedFiles.add(f.path);  // Conservative: mark as needing re-render
  setDirty(true);

  // Update editor content
  if (editorState.cmView && window.CMEditor) {
    CMEditor.setContent(editorState.cmView, origContent);
  } else {
    var ta = document.getElementById('editor-textarea-fallback');
    if (ta) ta.value = origContent;
  }

  updateEditToolbar();
  updateStateLabels();
  updateReRenderCount();
};

// --- Track dirty state on editor input ---
function setupDirtyTracking() {
  // Called after CodeMirror or textarea is created
  if (editorState.cmView && window.CMEditor) {
    // CM6 has updateListener extension — configure in CMEditor.create()
  } else {
    var ta = document.getElementById('editor-textarea-fallback');
    if (ta) {
      ta.addEventListener('input', function() {
        if (editorState.currentFile) {
          editorState.dirtyFiles.set(editorState.currentFile.path, ta.value);
          updateEditToolbar();
          updateStateLabels();
        }
      });
    }
  }
}

// --- Find file location in snapshot by path ---
function findFileInSnapshot(path) {
  var sections = [
    {section: 'config', list: 'files'},
    {section: 'containers', list: 'quadlet_units'},
    {section: 'services', list: 'drop_ins'},
  ];
  for (var i = 0; i < sections.length; i++) {
    var s = sections[i];
    var items = snapshot[s.section] && snapshot[s.section][s.list] || [];
    for (var j = 0; j < items.length; j++) {
      if (items[j].path === path) {
        return {section: s.section, list: s.list, index: j};
      }
    }
  }
  return null;
}

// --- Ctrl+S / Cmd+S shortcut ---
document.addEventListener('keydown', function(e) {
  if ((e.ctrlKey || e.metaKey) && e.key === 's' && refineMode && editorState.editMode) {
    e.preventDefault();
    editorSave();
  }
});

// --- Re-render count for toolbar button ---
function updateReRenderCount() {
  var count = editorState.savedFiles.size;
  var btn = document.getElementById('btn-re-render');
  if (btn) {
    btn.textContent = count > 0 ? 'Re-render (' + count + ' files changed)' : 'Re-render';
    btn.disabled = count === 0;
  }
}
```

- [ ] **Step 2: Wire up dirty tracking in editorEnterEditMode**

After creating the CM instance or textarea, call `setupDirtyTracking()`.

- [ ] **Step 3: Write tests**

```python
def test_editor_save_function_exists(self):
    """Editor JS includes save, saveAll, and revert functions."""
    html = self._render_report(refine_mode=True)
    assert 'editorSave' in html
    assert 'editorSaveAll' in html
    assert 'editorRevert' in html

def test_editor_keyboard_shortcut(self):
    """Editor JS registers Ctrl+S shortcut."""
    html = self._render_report(refine_mode=True)
    assert "e.key === 's'" in html or 'e.key==="s"' in html
```

- [ ] **Step 4: Run tests**

Run: `cd yoinkc && python -m pytest tests/test_editor.py -v`

- [ ] **Step 5: Commit**

```bash
git add src/yoinkc/templates/report/_editor_js.html.j2 tests/test_editor.py
git commit -m "feat: save, save all, revert, and Ctrl+S shortcut"
```

---

### Task 9: Delete Operator-Created Files

Add a Delete button for files created by the operator (not in originalSnapshot). Removes the file from the snapshot entirely.

**Files:**
- Modify: `src/yoinkc/templates/report/_editor_js.html.j2`
- Test: `tests/test_editor.py`

- [ ] **Step 1: Add editorDeleteFile function**

```javascript
window.editorDeleteFile = function() {
  if (!editorState.currentFile) return;
  var f = editorState.currentFile;
  if (!isOperatorCreated(f.path)) return;  // Safety: only delete operator-created

  if (!confirm('Delete ' + f.path + '? This cannot be undone.')) return;

  // Remove from snapshot
  var items = snapshot[f.section][f.list];
  items.splice(f.index, 1);

  // Clean up state
  editorState.dirtyFiles.delete(f.path);
  editorState.savedFiles.add(f.path);  // Track that snapshot changed, needs re-render
  editorState.currentFile = null;
  editorState.editMode = false;

  // Clear content pane
  document.getElementById('editor-readonly-content').textContent = '';
  document.getElementById('editor-cm-container').style.display = 'none';
  document.getElementById('editor-readonly-content').style.display = '';
  document.getElementById('editor-file-path').innerHTML = '';
  document.getElementById('editor-actions').innerHTML = '';

  setDirty(true);
  buildTree();
  updateReRenderCount();
};
```

- [ ] **Step 2: Write test**

```python
def test_editor_delete_function_exists(self):
    """Editor JS includes delete function for operator-created files."""
    html = self._render_report(refine_mode=True)
    assert 'editorDeleteFile' in html
    assert 'isOperatorCreated' in html
```

- [ ] **Step 3: Run tests**

Run: `cd yoinkc && python -m pytest tests/test_editor.py -v`

- [ ] **Step 4: Commit**

```bash
git add src/yoinkc/templates/report/_editor_js.html.j2 tests/test_editor.py
git commit -m "feat: delete operator-created files from snapshot"
```

---

### Task 10: Cross-Tab "Edit in Editor" Linking

Replace content pulldowns with "View & edit in editor →" links in refine mode for config, quadlet, and drop-in tabs.

**Files:**
- Modify: `src/yoinkc/templates/report/_config.html.j2`
- Modify: `src/yoinkc/templates/report/_containers.html.j2`
- Modify: `src/yoinkc/templates/report/_services.html.j2`
- Test: `tests/test_editor.py`

- [ ] **Step 1: Write failing test**

```python
def test_refine_mode_replaces_content_with_edit_link(self):
    """Config tab shows 'View & edit in editor' link instead of content pulldown in refine mode."""
    html = self._render_report(refine_mode=True)
    assert 'View &amp; edit in editor' in html or 'View & edit in editor' in html

def test_static_mode_keeps_content_pulldown(self):
    """Config tab keeps content pulldown in static mode."""
    html = self._render_report(refine_mode=False)
    # Content should be inline, not behind a link
    assert 'View &amp; edit in editor' not in html
```

- [ ] **Step 2: Run tests to verify they fail**

- [ ] **Step 3: Update _config.html.j2**

Find the expandable content section for each file entry — this is the `<div>` or PF6 expandable-section that displays `{{ file.content }}` inline. Wrap it in a conditional:

```html
{% if refine_mode %}
  <div style="display:flex; justify-content:space-between; align-items:center; padding:8px 0;">
    <span style="color:#6a6e73;">File content</span>
    <a href="#" class="edit-in-editor-link" data-path="{{ file.path }}" data-section="config" data-list="files" data-index="{{ loop.index0 }}" onclick="navigateToEditor(this); return false;" style="color:#06c; text-decoration:none; font-weight:500;">View &amp; edit in editor →</a>
  </div>
{% else %}
  {# Keep the existing expandable content block (the one rendering file.content) unchanged #}
  ... (keep existing markup verbatim) ...
{% endif %}
```

- [ ] **Step 4: Apply same pattern to _containers.html.j2 and _services.html.j2**

For `_containers.html.j2` (quadlets): use `data-section="containers"` `data-list="quadlet_units"` `data-path="{{ unit.path }}"`.

For `_services.html.j2` (drop-ins): use `data-section="services"` `data-list="drop_ins"` `data-path="{{ dropin.path }}"`.

- [ ] **Step 5: Add navigateToEditor JS function**

In `_editor_js.html.j2`:

```javascript
window.navigateToEditor = function(linkEl) {
  var section = linkEl.dataset.section;
  var list = linkEl.dataset.list;
  var index = parseInt(linkEl.dataset.index, 10);
  var path = linkEl.dataset.path;

  // Switch to Editor tab
  // Find the tab button for the editor and click it
  var editorTabBtn = document.querySelector('[data-tab="editor-tab"]') ||
                     document.querySelector('button[aria-controls="editor-tab"]');
  if (editorTabBtn) editorTabBtn.click();

  // Select the file
  selectFile(section, list, index, path);
};
```

- [ ] **Step 6: Add state labels on card headers**

In each tab's card header, add a conditional state label:

```html
{% if refine_mode %}
<span id="state-label-{{ file.path | replace('/', '-') }}" class="editor-card-state-label"></span>
{% endif %}
```

The JS `updateStateLabels()` function should also update these card-level labels.

- [ ] **Step 7: Run tests**

Run: `cd yoinkc && python -m pytest tests/test_editor.py -v`

- [ ] **Step 8: Commit**

```bash
git add src/yoinkc/templates/report/_config.html.j2 src/yoinkc/templates/report/_containers.html.j2 src/yoinkc/templates/report/_services.html.j2 src/yoinkc/templates/report/_editor_js.html.j2 tests/test_editor.py
git commit -m "feat: replace content pulldowns with editor links in refine mode"
```

---

### Task 11: New File Creation Modal

Create the PF6 modal for adding new config files, quadlet units, and systemd drop-ins with smart path pre-population.

**Files:**
- Create: `src/yoinkc/templates/report/_new_file_modal.html.j2`
- Modify: `src/yoinkc/templates/report/_editor_js.html.j2` (modal logic)
- Test: `tests/test_new_file.py`

- [ ] **Step 1: Create _new_file_modal.html.j2**

```html
{# New file creation modal — refine mode only #}
{% if refine_mode %}
<div id="new-file-modal" class="pf-v6-c-modal-box" role="dialog" aria-modal="true" aria-label="Create new file" style="display:none; position:fixed; top:0; left:0; right:0; bottom:0; z-index:9999;">
  <div style="position:absolute; top:0; left:0; right:0; bottom:0; background:rgba(0,0,0,0.5);" onclick="closeNewFileModal()"></div>
  <div style="position:absolute; top:50%; left:50%; transform:translate(-50%,-50%); background:#fff; border-radius:8px; width:560px; box-shadow:0 4px 24px rgba(0,0,0,0.2);">

    <div style="padding:20px 24px; border-bottom:1px solid #d2d2d2; display:flex; justify-content:space-between; align-items:center;">
      <h2 class="pf-v6-c-title pf-m-xl" style="margin:0;">Create new file</h2>
      <button class="pf-v6-c-button pf-m-plain" onclick="closeNewFileModal()" aria-label="Close">✕</button>
    </div>

    <div style="padding:24px;">
      {# File type radio group #}
      <div style="margin-bottom:20px;">
        <label class="pf-v6-c-form__label" style="display:block; margin-bottom:6px;"><strong>File type</strong></label>
        <div style="display:flex; gap:24px;">
          <label style="display:flex; align-items:center; gap:8px; cursor:pointer;">
            <input type="radio" name="new-file-type" value="config" checked onchange="updateNewFileForm()"> Config file
          </label>
          <label style="display:flex; align-items:center; gap:8px; cursor:pointer;">
            <input type="radio" name="new-file-type" value="quadlet" onchange="updateNewFileForm()"> Quadlet unit
          </label>
          <label style="display:flex; align-items:center; gap:8px; cursor:pointer;">
            <input type="radio" name="new-file-type" value="dropin" onchange="updateNewFileForm()"> Systemd drop-in
          </label>
        </div>
      </div>

      {# Config file fields #}
      <div id="nf-config-fields">
        <div style="margin-bottom:20px;">
          <label class="pf-v6-c-form__label" style="display:block; margin-bottom:6px;"><strong>File path</strong></label>
          <div style="font-size:13px; color:#6a6e73; margin-bottom:6px;">Absolute path on the target system</div>
          <input id="nf-config-path" class="pf-v6-c-form-control" type="text" placeholder="/etc/myapp/custom.conf" oninput="validateNewFileForm()" style="width:100%; font-family:var(--pf-t--global--font--family--mono);">
          <div id="nf-config-error" style="color:#c9190b; font-size:13px; margin-top:4px; display:none;"></div>
        </div>
      </div>

      {# Quadlet fields #}
      <div id="nf-quadlet-fields" style="display:none;">
        <div style="margin-bottom:20px;">
          <label class="pf-v6-c-form__label" style="display:block; margin-bottom:6px;"><strong>Scope</strong></label>
          <select id="nf-quadlet-scope" class="pf-v6-c-form-control" onchange="updateNewFilePath()" style="width:100%;">
            <option value="system">System (root)</option>
            {# snapshot is available as a Python object in the template context #}
            {% for user in snapshot.users_groups.users if user.get('home') %}
            <option value="{{ user.name }}">User — {{ user.name }}</option>
            {% endfor %}
          </select>
        </div>
        <div style="margin-bottom:20px;">
          <label class="pf-v6-c-form__label" style="display:block; margin-bottom:6px;"><strong>Unit name</strong></label>
          <div style="font-size:13px; color:#6a6e73; margin-bottom:6px;">e.g. myapp.container</div>
          <input id="nf-quadlet-name" class="pf-v6-c-form-control" type="text" placeholder="myapp.container" oninput="updateNewFilePath(); validateNewFileForm();" style="width:100%; font-family:var(--pf-t--global--font--family--mono);">
          <div id="nf-quadlet-error" style="color:#c9190b; font-size:13px; margin-top:4px; display:none;"></div>
        </div>
        <div style="margin-bottom:20px;">
          <label class="pf-v6-c-form__label" style="display:block; margin-bottom:6px;"><strong>Full path</strong></label>
          <div id="nf-quadlet-path-preview" style="padding:10px 14px; background:#fafafa; border:1px solid #d2d2d2; border-radius:4px; font-family:var(--pf-t--global--font--family--mono); font-size:13px; color:#6a6e73;"></div>
        </div>
      </div>

      {# Drop-in fields #}
      <div id="nf-dropin-fields" style="display:none;">
        <div style="margin-bottom:20px;">
          <label class="pf-v6-c-form__label" style="display:block; margin-bottom:6px;"><strong>Service</strong></label>
          <div style="font-size:13px; color:#6a6e73; margin-bottom:6px;">Select from services discovered on this host</div>
          <select id="nf-dropin-service" class="pf-v6-c-form-control" onchange="updateNewFilePath()" style="width:100%;">
            {% for svc in snapshot.services.services %}
            <option value="{{ svc.name }}">{{ svc.name }}</option>
            {% endfor %}
          </select>
        </div>
        <div style="margin-bottom:20px;">
          <label class="pf-v6-c-form__label" style="display:block; margin-bottom:6px;"><strong>Override filename</strong></label>
          <input id="nf-dropin-filename" class="pf-v6-c-form-control" type="text" value="override.conf" oninput="updateNewFilePath(); validateNewFileForm();" style="width:100%; font-family:var(--pf-t--global--font--family--mono);">
          <div id="nf-dropin-error" style="color:#c9190b; font-size:13px; margin-top:4px; display:none;"></div>
        </div>
        <div style="margin-bottom:20px;">
          <label class="pf-v6-c-form__label" style="display:block; margin-bottom:6px;"><strong>Full path</strong></label>
          <div id="nf-dropin-path-preview" style="padding:10px 14px; background:#fafafa; border:1px solid #d2d2d2; border-radius:4px; font-family:var(--pf-t--global--font--family--mono); font-size:13px; color:#6a6e73;"></div>
        </div>
      </div>
    </div>

    <div style="padding:16px 24px; border-top:1px solid #d2d2d2; display:flex; justify-content:flex-end; gap:8px;">
      <button class="pf-v6-c-button pf-m-secondary" onclick="closeNewFileModal()">Cancel</button>
      <button class="pf-v6-c-button pf-m-primary" id="btn-create-file" onclick="createNewFile()" disabled>Create &amp; edit</button>
    </div>
  </div>
</div>
{% endif %}
```

- [ ] **Step 2: Add modal JS logic to _editor_js.html.j2**

```javascript
// --- New File Modal ---
document.getElementById('btn-new-file').addEventListener('click', function() {
  document.getElementById('new-file-modal').style.display = '';
  updateNewFileForm();
});

window.closeNewFileModal = function() {
  document.getElementById('new-file-modal').style.display = 'none';
};

window.updateNewFileForm = function() {
  var type = document.querySelector('input[name="new-file-type"]:checked').value;
  document.getElementById('nf-config-fields').style.display = type === 'config' ? '' : 'none';
  document.getElementById('nf-quadlet-fields').style.display = type === 'quadlet' ? '' : 'none';
  document.getElementById('nf-dropin-fields').style.display = type === 'dropin' ? '' : 'none';
  updateNewFilePath();
  validateNewFileForm();
};

window.updateNewFilePath = function() {
  var type = document.querySelector('input[name="new-file-type"]:checked').value;
  if (type === 'quadlet') {
    var scope = document.getElementById('nf-quadlet-scope').value;
    var name = document.getElementById('nf-quadlet-name').value || 'unit.container';
    var base = scope === 'system' ? '/etc/containers/systemd/' : '/home/' + scope + '/.config/containers/systemd/';
    document.getElementById('nf-quadlet-path-preview').innerHTML = base + '<strong>' + escapeHtml(name) + '</strong>';
  } else if (type === 'dropin') {
    var svc = document.getElementById('nf-dropin-service').value;
    var fname = document.getElementById('nf-dropin-filename').value || 'override.conf';
    document.getElementById('nf-dropin-path-preview').innerHTML = '/etc/systemd/system/<strong>' + escapeHtml(svc) + '</strong>.d/<strong>' + escapeHtml(fname) + '</strong>';
  }
};

window.validateNewFileForm = function() {
  var type = document.querySelector('input[name="new-file-type"]:checked').value;
  var valid = true;
  var validExts = {
    quadlet: ['.container', '.volume', '.network', '.kube', '.image'],
    dropin: ['.conf'],
  };

  if (type === 'config') {
    var path = document.getElementById('nf-config-path').value.trim();
    var errEl = document.getElementById('nf-config-error');
    if (!path) { valid = false; errEl.style.display = 'none'; }
    else if (!path.startsWith('/')) { valid = false; errEl.textContent = 'Path must be absolute (start with /).'; errEl.style.display = ''; }
    else if (fileExistsInSnapshot(path)) { valid = false; errEl.textContent = 'File already exists — edit it in the Editor tab instead.'; errEl.style.display = ''; }
    else { errEl.style.display = 'none'; }
  } else if (type === 'quadlet') {
    var name = document.getElementById('nf-quadlet-name').value.trim();
    var errEl = document.getElementById('nf-quadlet-error');
    if (!name) { valid = false; errEl.style.display = 'none'; }
    else if (!validExts.quadlet.some(function(ext) { return name.endsWith(ext); })) {
      valid = false; errEl.textContent = 'Must end with ' + validExts.quadlet.join(', '); errEl.style.display = '';
    } else { errEl.style.display = 'none'; }
  } else if (type === 'dropin') {
    var fname = document.getElementById('nf-dropin-filename').value.trim();
    var errEl = document.getElementById('nf-dropin-error');
    if (!fname) { valid = false; errEl.style.display = 'none'; }
    else if (!fname.endsWith('.conf')) { valid = false; errEl.textContent = 'Must end with .conf'; errEl.style.display = ''; }
    else { errEl.style.display = 'none'; }
  }

  document.getElementById('btn-create-file').disabled = !valid;
  return valid;
};

function fileExistsInSnapshot(path) {
  return findFileInSnapshot(path) !== null;
}

window.createNewFile = function() {
  if (!validateNewFileForm()) return;
  var type = document.querySelector('input[name="new-file-type"]:checked').value;
  var entry, section, list, path;

  if (type === 'config') {
    path = document.getElementById('nf-config-path').value.trim();
    entry = {path: path, kind: 'UNOWNED', content: '', include: true, fleet: null, rpm_va_flags: null, package: null, diff_against_rpm: null};
    section = 'config'; list = 'files';
  } else if (type === 'quadlet') {
    var scope = document.getElementById('nf-quadlet-scope').value;
    var name = document.getElementById('nf-quadlet-name').value.trim();
    var base = scope === 'system' ? '/etc/containers/systemd/' : '/home/' + scope + '/.config/containers/systemd/';
    path = base + name;
    entry = {path: path, name: name, content: '', image: '', include: true, fleet: null};
    section = 'containers'; list = 'quadlet_units';
  } else if (type === 'dropin') {
    var svc = document.getElementById('nf-dropin-service').value;
    var fname = document.getElementById('nf-dropin-filename').value.trim();
    path = '/etc/systemd/system/' + svc + '.d/' + fname;
    entry = {path: path, unit: svc, content: '', include: true, fleet: null};
    section = 'services'; list = 'drop_ins';
  }

  // Add to snapshot
  if (!snapshot[section][list]) snapshot[section][list] = [];
  snapshot[section][list].push(entry);

  // Close modal and open in editor
  closeNewFileModal();
  setDirty(true);
  buildTree();

  var index = snapshot[section][list].length - 1;
  selectFile(section, list, index, path);
  editorEnterEditMode();
};
```

- [ ] **Step 3: Include modal template**

In the main report template (or `_file_browser.html.j2` after the editor include):
```html
{% include "report/_new_file_modal.html.j2" %}
```

- [ ] **Step 4: Write tests**

Create `tests/test_new_file.py`:

```python
class TestNewFileCreation:
    def test_new_file_modal_exists_in_refine_mode(self):
        html = self._render_report(refine_mode=True)
        assert 'new-file-modal' in html
        assert 'Create new file' in html

    def test_new_file_modal_absent_in_static_mode(self):
        html = self._render_report(refine_mode=False)
        assert 'new-file-modal' not in html

    def test_new_file_validation_functions_exist(self):
        html = self._render_report(refine_mode=True)
        assert 'validateNewFileForm' in html
        assert 'createNewFile' in html
        assert 'updateNewFilePath' in html

    def test_service_dropdown_populated(self):
        """Drop-in form populates service dropdown from snapshot."""
        html = self._render_report(refine_mode=True)
        # Should contain service names from the test snapshot
        assert 'nf-dropin-service' in html

    def test_user_dropdown_populated(self):
        """Quadlet form populates scope dropdown from snapshot users."""
        html = self._render_report(refine_mode=True)
        assert 'nf-quadlet-scope' in html
```

- [ ] **Step 5: Run tests**

Run: `cd yoinkc && python -m pytest tests/test_new_file.py -v`

- [ ] **Step 6: Commit**

```bash
git add src/yoinkc/templates/report/_new_file_modal.html.j2 src/yoinkc/templates/report/_editor_js.html.j2 tests/test_new_file.py
git commit -m "feat: new file creation modal with validation and path assembly"
```

---

## Chunk 4: Re-render Button, Audit, & Integration

### Task 12: Re-render Button with Changed-File Count

Add a "Re-render" button to the yoinkc-refine toolbar showing the count of files in `not rendered` state.

**Files:**
- Modify: `src/yoinkc/templates/report/_toolbar.html.j2`
- Modify: `src/yoinkc/templates/report/_editor_js.html.j2` (updateReRenderCount already added in Task 8)
- Test: `tests/test_editor.py`

- [ ] **Step 1: Write failing test**

```python
def test_rerender_button_in_refine_mode(self):
    html = self._render_report(refine_mode=True)
    assert 'btn-re-render' in html
    assert 'Re-render' in html

def test_no_rerender_button_in_static_mode(self):
    html = self._render_report(refine_mode=False)
    assert 'btn-re-render' not in html
```

- [ ] **Step 2: Run tests to verify they fail**

- [ ] **Step 3: Add Re-render button to _toolbar.html.j2**

In the toolbar template, alongside the existing Reset button:

```html
{% if refine_mode %}
<button class="pf-v6-c-button pf-m-primary" id="btn-re-render" onclick="triggerReRender()" disabled>Re-render</button>
{% endif %}
```

- [ ] **Step 4: Add triggerReRender function**

In `_editor_js.html.j2`:

```javascript
window.triggerReRender = function() {
  // Save all unsaved files first
  editorSaveAll();

  var btn = document.getElementById('btn-re-render');
  btn.textContent = 'Re-rendering...';
  btn.disabled = true;

  fetch(window.location.origin + '/api/re-render', {
    method: 'POST',
    headers: {'Content-Type': 'application/json'},
    body: JSON.stringify({snapshot: snapshot, original: originalSnapshot}),
  }).then(function(r) {
    if (!r.ok) throw new Error('Re-render failed: HTTP ' + r.status);
    return r.text();
  }).then(function(html) {
    setDirty(false);
    document.open();
    document.write(html);
    document.close();
  }).catch(function(err) {
    // Show error banner
    btn.textContent = 'Re-render (failed)';
    btn.disabled = false;
    alert('Re-render failed: ' + err.message);
  });
};
```

- [ ] **Step 5: Update existing re-render fetch in _js.html.j2**

The existing re-render code in `_js.html.j2` (lines ~450-469) also needs to send the wrapper format. Update it to match:

```javascript
body: JSON.stringify({snapshot: snapshot, original: originalSnapshot}),
```

This was partially done in Task 1, Step 5 — verify it's applied consistently.

- [ ] **Step 6: Run tests**

Run: `cd yoinkc && python -m pytest tests/test_editor.py -v`

- [ ] **Step 7: Commit**

```bash
git add src/yoinkc/templates/report/_toolbar.html.j2 src/yoinkc/templates/report/_editor_js.html.j2 src/yoinkc/templates/report/_js.html.j2 tests/test_editor.py
git commit -m "feat: re-render button with changed-file count"
```

---

### Task 13: Audit Report — Modifications Section

Add a "Modifications" section at the end of the audit report listing edited and added files. Computed server-side during re-render.

**Files:**
- Modify: `src/yoinkc/renderers/audit_report.py`
- Test: `tests/test_audit_report_output.py`

- [ ] **Step 1: Write failing test**

In `tests/test_audit_report_output.py`:

```python
def test_modifications_section_with_edits(self):
    """Audit report includes Modifications section when files are edited."""
    original = self._make_snapshot()
    modified = self._make_snapshot()
    modified.config.files[0].content = "modified content"
    # render() writes audit-report.md to output_dir; read it back
    render(modified, env=self.env, output_dir=self.output_dir, original_snapshot=original)
    audit_md = (self.output_dir / "audit-report.md").read_text()
    assert '## Modifications' in audit_md
    assert 'Edited' in audit_md

def test_modifications_section_with_added_files(self):
    """Audit report lists added files in Modifications section."""
    original = self._make_snapshot()
    modified = self._make_snapshot()
    modified.config.files.append(ConfigFileEntry(path="/etc/new.conf", kind="UNOWNED", content="new"))
    render(modified, env=self.env, output_dir=self.output_dir, original_snapshot=original)
    audit_md = (self.output_dir / "audit-report.md").read_text()
    assert '## Modifications' in audit_md
    assert '/etc/new.conf' in audit_md
    assert 'Added' in audit_md

def test_no_modifications_section_when_unchanged(self):
    """Audit report omits Modifications section when no edits made."""
    snapshot = self._make_snapshot()
    render(snapshot, env=self.env, output_dir=self.output_dir, original_snapshot=snapshot)
    audit_md = (self.output_dir / "audit-report.md").read_text()
    assert '## Modifications' not in audit_md
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd yoinkc && python -m pytest tests/test_audit_report_output.py -v -k modifications`

- [ ] **Step 3: Add original_snapshot parameter to audit render()**

Update `audit_report.py` `render()` to accept an optional `original_snapshot`:

```python
def render(
    snapshot: InspectionSnapshot,
    env: Environment,
    output_dir: Path,
    original_snapshot: Optional[InspectionSnapshot] = None,
) -> None:
```

- [ ] **Step 4: Implement modifications diff logic**

At the end of the audit report generation, before writing the file:

```python
def _compute_modifications(
    snapshot: InspectionSnapshot,
    original: InspectionSnapshot,
) -> Tuple[List[str], List[str]]:
    """Compare snapshot against original to find edited and added files.

    Returns (edited_paths, added_paths).
    """
    edited = []
    added = []

    sections = [
        (snapshot.config.files, original.config.files, "path", "content"),
        (snapshot.containers.quadlet_units, original.containers.quadlet_units, "path", "content"),
        (snapshot.services.drop_ins, original.services.drop_ins, "path", "content"),
    ]

    for current_items, orig_items, path_field, content_field in sections:
        orig_by_path = {getattr(item, path_field): getattr(item, content_field) for item in orig_items}
        for item in current_items:
            p = getattr(item, path_field)
            c = getattr(item, content_field)
            if p not in orig_by_path:
                added.append(p)
            elif c != orig_by_path[p]:
                edited.append(p)

    return edited, added
```

- [ ] **Step 5: Generate the Modifications markdown section**

```python
if original_snapshot:
    edited, added = _compute_modifications(snapshot, original_snapshot)
    if edited or added:
        lines.append("\n## Modifications\n")
        lines.append("Changes made by the operator during refinement.\n")
        if edited:
            lines.append("\n### Edited\n")
            for p in sorted(edited):
                lines.append(f"- `{p}`\n")
        if added:
            lines.append("\n### Added\n")
            for p in sorted(added):
                lines.append(f"- `{p}`\n")
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cd yoinkc && python -m pytest tests/test_audit_report_output.py -v -k modifications`

- [ ] **Step 7: Wire up original_snapshot passthrough from yoinkc-refine**

In Task 1, yoinkc-refine writes `original-snapshot.json` alongside the snapshot in the temp input dir. In yoinkc's CLI entry point, detect this file:

```python
original_path = input_dir / "original-snapshot.json"
original_snapshot = None
if original_path.exists():
    original_snapshot = InspectionSnapshot.model_validate_json(original_path.read_text())
```

Pass `original_snapshot` through the render pipeline:
- `cli.py` / `__main__.py` → `render()` → `html_report.render(snapshot, ..., original_snapshot=original_snapshot)`
- `html_report.render()` → `_build_context(snapshot, ..., original_snapshot=original_snapshot)` → embeds as `original_snapshot_json`
- `html_report.render()` → `audit_report.render(snapshot, ..., original_snapshot=original_snapshot)`

- [ ] **Step 8: Run full test suite**

Run: `cd yoinkc && python -m pytest tests/ -v`

- [ ] **Step 9: Commit**

```bash
git add src/yoinkc/renderers/audit_report.py tests/test_audit_report_output.py
git commit -m "feat: audit report modifications section for edited/added files"
```

---

### Task 14: Integration Testing & Final Verification

End-to-end verification that the full editor workflow functions correctly.

**Files:**
- Test: `tests/test_editor.py` (add integration tests)
- Test: `tests/test_html_report_output.py` (verify existing tests still pass)

- [ ] **Step 1: Write integration test — static report unchanged**

```python
def test_static_report_unchanged(self):
    """Static report (refine_mode=False) has no editor artifacts."""
    html = self._render_report(refine_mode=False)
    assert 'id="editor-tab"' not in html
    assert 'new-file-modal' not in html
    assert '/static/codemirror/' not in html
    assert 'editorSave' not in html
    assert 'btn-re-render' not in html
    # Existing file browser present
    assert 'file-viewer-content' in html or 'file-browser' in html
```

- [ ] **Step 2: Write integration test — refine mode has all editor components**

```python
def test_refine_mode_complete(self):
    """Refine mode report has all editor components."""
    html = self._render_report(refine_mode=True)
    # Editor tab
    assert 'id="editor-tab"' in html
    # CodeMirror assets
    assert '/static/codemirror/' in html
    # JS functions
    assert 'editorSave' in html
    assert 'editorRevert' in html
    assert 'createNewFile' in html
    # New file modal
    assert 'new-file-modal' in html
    # Re-render button
    assert 'btn-re-render' in html
    # Edit in editor links
    assert 'View &amp; edit in editor' in html
    # Original snapshot embedded separately
    assert 'var originalSnapshot' in html
    assert 'JSON.parse(JSON.stringify(snapshot))' not in html
```

- [ ] **Step 3: Run full test suite**

Run: `cd yoinkc && python -m pytest tests/ -v`
Expected: All tests pass. Zero regressions.

- [ ] **Step 4: Manual smoke test with yoinkc-refine**

1. Run yoinkc on a test system or use driftify to generate a snapshot.
2. Start yoinkc-refine and open the report.
3. Verify: Editor tab appears, tree shows files, clicking a file shows read-only content.
4. Click Edit — CodeMirror loads, make an edit, click Save.
5. Check state label changes: `unsaved` → `not rendered` after save.
6. Click + New File — create a config file, verify it appears in tree.
7. Click Re-render — verify page updates with changes.
8. Check audit report — Modifications section lists the edited/added files.

- [ ] **Step 5: Final commit**

```bash
git add tests/
git commit -m "test: integration tests for editor feature completeness"
```

---

## Notes for Implementer

1. **`_editor_js.html.j2` will be large** (~300+ lines). This is acceptable for initial implementation since it's all closely related editor logic. If it becomes unwieldy during development, consider splitting into `_editor_tree.html.j2` and `_editor_actions.html.j2`.

2. **`confirmNavigation()` uses `confirm()`** as a deliberate simplification. Replace with a PF6 modal (save/discard/cancel) in a follow-up task.

3. **`triggerReRender()` uses `alert()`** for error display. The spec calls for a PF6 error banner. Implement the banner if straightforward; otherwise use `alert()` as a placeholder.

4. **Template context**: The `snapshot` Python object is available in Jinja2 templates for iteration (e.g., service lists, user lists in the new file modal). The `snapshot_json` string is what gets embedded in `<script>` tags for JavaScript. Don't confuse the two.

5. **Cross-chunk dependency**: Task 1 changes the re-render fetch body format. Task 12 also touches the re-render fetch. Verify consistency after implementing both.
