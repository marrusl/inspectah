# Editor Section Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Redesign the Edit Files section with tabbed file organization, read-only-first viewing, explicit edit mode, three-baseline data model, and unsaved-changes protection.

**Architecture:** All changes are in one file: `cmd/inspectah/internal/renderer/static/report.html`. The editor section is rendered by JS functions (`renderEditorSection`, `renderEditorFileBrowser`, `openFileInEditor`, `initEditor`) that build DOM elements. CSS styles are in the `<style>` block (lines 21-715). The approach: replace existing editor CSS and JS functions incrementally, keeping the rest of the report unchanged.

**Tech Stack:** Vanilla JS, PatternFly v6 CSS (vendored), CodeMirror 6 (vendored via `go:embed`), Go `html/template`.

**Spec:** `docs/specs/proposed/2026-05-03-editor-redesign.md` (revision 4, approved)

---

### File Map

All changes are in one file:

| File | Action | Responsibility |
|------|--------|----------------|
| `cmd/inspectah/internal/renderer/static/report.html` | Modify | CSS (tab bar, read-only view, toolbar, modal, dot indicators) + JS (data model, state machine, tabs, modal, keyboard, focus, autosave integration) |

No new files. No Go code changes. The `html.go` renderer and its tests are unaffected — the editor section is entirely client-side JS.

**Testing approach:** Interactive behavior is verified via `dev-browser` scripts after each task. Golden-file tests are not affected because the editor section is dynamically rendered by JS, not by Go templates.

---

### Task 1: CSS — Tab Bar, Toolbar, Read-Only View, Modal

**Files:**
- Modify: `cmd/inspectah/internal/renderer/static/report.html` (CSS block, lines ~545-615)

- [ ] **Step 1: Add tab bar CSS**

Replace the existing editor CSS block (from `.editor-layout` through `.editor-empty`, approximately lines 545-615) with the full new editor CSS. Find the comment or the `.editor-layout` rule and replace through `.editor-empty`:

```css
    /* ── Editor section ── */
    .editor-layout {
      display: flex;
      gap: 0;
      min-height: 400px;
      border: 1px solid var(--pf-t--global--border--color--default, #444);
      border-radius: 4px;
      overflow: hidden;
    }

    .editor-file-panel {
      width: 240px;
      flex-shrink: 0;
      border-right: 1px solid var(--pf-t--global--border--color--default, #444);
      display: flex;
      flex-direction: column;
      background: var(--pf-t--global--background--color--secondary-default);
    }

    .editor-tabs {
      display: flex;
      border-bottom: 1px solid var(--pf-t--global--border--color--default, #444);
      flex-shrink: 0;
    }

    .editor-tab {
      padding: 0.5rem 0.625rem;
      font-size: 0.75rem;
      font-weight: 500;
      cursor: pointer;
      background: transparent;
      border: none;
      color: inherit;
      opacity: 0.6;
      white-space: nowrap;
      position: relative;
    }

    .editor-tab:hover { opacity: 0.8; }

    .editor-tab[aria-selected="true"] {
      opacity: 1;
      font-weight: 600;
      border-bottom: 2px solid var(--pf-t--global--color--status--info--default, #4493f8);
      color: var(--pf-t--global--color--status--info--default, #4493f8);
    }

    .editor-tab .tab-dot {
      display: inline-block;
      width: 6px;
      height: 6px;
      border-radius: 50%;
      background: var(--pf-t--global--color--status--info--default, #2b9af3);
      margin-left: 4px;
      vertical-align: middle;
    }

    .editor-file-list {
      flex: 1;
      overflow-y: auto;
      padding: 0.25rem 0;
    }

    .editor-file-item {
      padding: 0.375rem 0.75rem;
      font-size: 0.8rem;
      cursor: pointer;
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
      outline-offset: -2px;
      display: flex;
      align-items: center;
      gap: 4px;
    }

    .editor-file-item:hover,
    .editor-file-item:focus {
      background: var(--pf-t--global--background--color--hover, rgba(255,255,255,0.05));
    }

    .editor-file-item.selected {
      background: var(--pf-t--global--background--color--hover, rgba(255,255,255,0.1));
      font-weight: 600;
    }

    .editor-file-item .file-dot {
      width: 6px;
      height: 6px;
      border-radius: 50%;
      background: var(--pf-t--global--color--status--info--default, #2b9af3);
      flex-shrink: 0;
    }

    .editor-file-item .file-path { color: inherit; opacity: 0.6; }
    .editor-file-item .file-name { font-weight: 600; }

    .editor-content-pane {
      flex: 1;
      min-width: 0;
      display: flex;
      flex-direction: column;
      background: var(--pf-t--global--background--color--secondary-default);
    }

    .editor-toolbar {
      padding: 0.625rem 1rem;
      border-bottom: 1px solid var(--pf-t--global--border--color--default, #444);
      display: flex;
      justify-content: space-between;
      align-items: center;
      flex-shrink: 0;
    }

    .editor-toolbar .file-path-display {
      font-family: 'SF Mono', 'Fira Code', monospace;
      font-size: 0.85rem;
      display: flex;
      align-items: center;
      gap: 0.5rem;
    }

    .editor-toolbar .unsaved-badge {
      font-size: 0.7rem;
      padding: 1px 6px;
      border-radius: 3px;
      background: rgba(68, 147, 248, 0.15);
      color: var(--pf-t--global--color--status--info--default, #4493f8);
    }

    .editor-toolbar .toolbar-actions {
      display: flex;
      gap: 0.375rem;
    }

    .editor-toolbar .btn-edit,
    .editor-toolbar .btn-save {
      padding: 4px 12px;
      background: var(--pf-t--global--color--status--info--default, #4493f8);
      color: #fff;
      border: none;
      border-radius: 4px;
      font-size: 0.75rem;
      font-weight: 600;
      cursor: pointer;
    }

    .editor-toolbar .btn-edit:disabled {
      opacity: 0.4;
      cursor: default;
    }

    .editor-toolbar .btn-save:disabled {
      opacity: 0.4;
      cursor: default;
    }

    .editor-toolbar .btn-revert {
      padding: 4px 12px;
      background: transparent;
      color: inherit;
      border: 1px solid var(--pf-t--global--border--color--default, #666);
      border-radius: 4px;
      font-size: 0.75rem;
      cursor: pointer;
    }

    .editor-readonly-content {
      flex: 1;
      padding: 0.75rem 1rem;
      font-family: 'SF Mono', 'Fira Code', monospace;
      font-size: 0.8rem;
      line-height: 1.7;
      white-space: pre-wrap;
      word-break: break-all;
      overflow-y: auto;
    }

    .editor-empty {
      display: flex;
      align-items: center;
      justify-content: center;
      flex: 1;
      opacity: 0.5;
      font-style: italic;
    }

    .editor-content-pane .cm-editor {
      flex: 1;
      font-size: 0.8rem;
    }

    /* Unsaved changes modal */
    .editor-modal-backdrop {
      position: fixed;
      inset: 0;
      background: rgba(0,0,0,0.5);
      display: flex;
      align-items: center;
      justify-content: center;
      z-index: 1000;
    }

    .editor-modal {
      background: var(--pf-t--global--background--color--primary-default, #1b1d21);
      border: 1px solid var(--pf-t--global--border--color--default, #444);
      border-radius: 8px;
      padding: 1.5rem;
      max-width: 400px;
      width: 90%;
    }

    .editor-modal h3 { margin: 0 0 0.5rem 0; font-size: 1rem; }
    .editor-modal p { margin: 0 0 1.25rem 0; font-size: 0.85rem; opacity: 0.8; }

    .editor-modal-actions {
      display: flex;
      justify-content: flex-end;
      gap: 0.5rem;
    }

    .editor-modal-actions button {
      padding: 6px 14px;
      border-radius: 4px;
      font-size: 0.8rem;
      cursor: pointer;
    }

    .editor-modal-actions .btn-modal-cancel {
      background: transparent;
      color: inherit;
      border: 1px solid var(--pf-t--global--border--color--default, #666);
    }

    .editor-modal-actions .btn-modal-discard {
      background: rgba(248, 81, 73, 0.15);
      color: #f85149;
      border: 1px solid rgba(248, 81, 73, 0.3);
    }

    .editor-modal-actions .btn-modal-save {
      background: var(--pf-t--global--color--status--info--default, #4493f8);
      color: #fff;
      border: none;
      font-weight: 600;
    }

    .editor-tab-empty {
      display: flex;
      align-items: center;
      justify-content: center;
      flex: 1;
      opacity: 0.5;
      font-style: italic;
      font-size: 0.85rem;
      padding: 2rem;
      text-align: center;
    }
```

- [ ] **Step 2: Also remove the old `.rebuild-bar` and editor-bottom-bar CSS if present**

Search for `.rebuild-bar` CSS rules in the editor section and remove them — the global toolbar handles rebuild. Leave the global rebuild bar CSS elsewhere untouched.

- [ ] **Step 3: Verify CSS loads**

Run: `go build -o /dev/null ./cmd/inspectah/` to verify the template still compiles.
Expected: Build succeeds with no errors.

- [ ] **Step 4: Commit**

```bash
git add cmd/inspectah/internal/renderer/static/report.html
git commit -m "refactor(renderer): replace editor CSS with tab bar, toolbar, modal styles"
```

---

### Task 2: Data Model — Three-Baseline File State

**Files:**
- Modify: `cmd/inspectah/internal/renderer/static/report.html` (JS block, near line 2274)

- [ ] **Step 1: Replace editor state variables and `collectEditorFiles`**

Find the existing editor state variables (around line 2274-2278):

```javascript
var editorInstance = null;
var currentEditorFile = null;
var autosaveTimer = null;
var autosaveHadFailure = false;
```

Replace with the new editor state block. Also find the existing `collectEditorFiles` function (around line 2279) and replace it:

```javascript
// ── Editor State ──
var editorInstance = null;
var currentEditorFile = null;
var autosaveTimer = null;
var autosaveHadFailure = false;

var editorState = {
  mode: 'empty',  // 'empty' | 'readonly' | 'editing-clean' | 'editing-dirty'
  activeTab: 'config',
  files: {config: [], 'drop-in': [], quadlet: []},
  tabState: {
    config:    {selectedPath: null, scrollTop: 0},
    'drop-in': {selectedPath: null, scrollTop: 0},
    quadlet:   {selectedPath: null, scrollTop: 0}
  },
  pendingNavAction: null
};

function collectEditorFiles() {
  var snap = App.snapshot;
  var files = {config: [], 'drop-in': [], quadlet: []};

  if (snap.config && snap.config.files) {
    for (var i = 0; i < snap.config.files.length; i++) {
      var f = snap.config.files[i];
      files.config.push({
        path: f.path || f.name,
        family: 'config',
        ref: f,
        originalContent: f.content || '',
        savedContent: f.content || '',
        bufferContent: f.content || ''
      });
    }
  }

  if (snap.services && snap.services.drop_ins) {
    for (var j = 0; j < snap.services.drop_ins.length; j++) {
      var d = snap.services.drop_ins[j];
      files['drop-in'].push({
        path: d.path || d.name,
        family: 'drop-in',
        ref: d,
        originalContent: d.content || '',
        savedContent: d.content || '',
        bufferContent: d.content || ''
      });
    }
  }

  if (snap.containers && snap.containers.quadlet_units) {
    for (var k = 0; k < snap.containers.quadlet_units.length; k++) {
      var q = snap.containers.quadlet_units[k];
      files.quadlet.push({
        path: q.path || q.name,
        family: 'quadlet',
        ref: q,
        originalContent: q.content || '',
        savedContent: q.content || '',
        bufferContent: q.content || ''
      });
    }
  }

  return files;
}

function findFileByPath(path) {
  var families = ['config', 'drop-in', 'quadlet'];
  for (var i = 0; i < families.length; i++) {
    var list = editorState.files[families[i]];
    for (var j = 0; j < list.length; j++) {
      if (list[j].path === path) return list[j];
    }
  }
  return null;
}

function isFileDirty(file) {
  return file.bufferContent !== file.savedContent;
}

function isFileModified(file) {
  return file.savedContent !== file.originalContent;
}

function isAnyFileModifiedInFamily(family) {
  var list = editorState.files[family];
  for (var i = 0; i < list.length; i++) {
    if (isFileModified(list[i])) return true;
  }
  return false;
}
```

- [ ] **Step 2: Verify build**

Run: `go build -o /dev/null ./cmd/inspectah/`
Expected: Build succeeds.

- [ ] **Step 3: Commit**

```bash
git add cmd/inspectah/internal/renderer/static/report.html
git commit -m "refactor(renderer): three-baseline editor data model"
```

---

### Task 3: Tab Bar and File List Rendering

**Files:**
- Modify: `cmd/inspectah/internal/renderer/static/report.html` (JS: `renderEditorFileBrowser`, `renderEditorSection`)

- [ ] **Step 1: Replace `renderEditorFileBrowser` with tab bar + per-tab file list**

Find the existing `renderEditorFileBrowser` function (around line 2320) and replace it, along with `renderEditorSection` (around line 2436):

```javascript
function renderEditorTabBar(container) {
  var tablist = document.createElement('div');
  tablist.className = 'editor-tabs';
  tablist.setAttribute('role', 'tablist');
  tablist.setAttribute('aria-label', 'File categories');

  var tabs = [
    {id: 'config', label: 'Config', count: editorState.files.config.length},
    {id: 'drop-in', label: 'Drop-ins', count: editorState.files['drop-in'].length},
    {id: 'quadlet', label: 'Quadlets', count: editorState.files.quadlet.length}
  ];

  tabs.forEach(function(tab, idx) {
    var btn = document.createElement('button');
    btn.className = 'editor-tab';
    btn.setAttribute('role', 'tab');
    btn.setAttribute('aria-selected', tab.id === editorState.activeTab ? 'true' : 'false');
    btn.setAttribute('aria-controls', 'editor-panel-' + tab.id);
    btn.setAttribute('tabindex', tab.id === editorState.activeTab ? '0' : '-1');
    btn.id = 'editor-tab-' + tab.id;
    btn.setAttribute('data-tab', tab.id);

    var labelText = tab.label + ' (' + tab.count + ')';
    btn.textContent = labelText;

    if (isAnyFileModifiedInFamily(tab.id)) {
      var dot = document.createElement('span');
      dot.className = 'tab-dot';
      dot.setAttribute('aria-hidden', 'true');
      btn.appendChild(dot);
    }

    btn.onclick = function() { switchTab(tab.id); };
    tablist.appendChild(btn);
  });

  // Keyboard: Arrow Left/Right, Home/End between tabs
  tablist.addEventListener('keydown', function(e) {
    var tabBtns = Array.prototype.slice.call(tablist.querySelectorAll('[role="tab"]'));
    var idx = tabBtns.indexOf(document.activeElement);
    if (idx === -1) return;

    var newIdx = idx;
    if (e.key === 'ArrowRight') newIdx = (idx + 1) % tabBtns.length;
    else if (e.key === 'ArrowLeft') newIdx = (idx - 1 + tabBtns.length) % tabBtns.length;
    else if (e.key === 'Home') newIdx = 0;
    else if (e.key === 'End') newIdx = tabBtns.length - 1;
    else return;

    e.preventDefault();
    tabBtns[idx].setAttribute('tabindex', '-1');
    tabBtns[newIdx].setAttribute('tabindex', '0');
    tabBtns[newIdx].focus();
  });

  container.appendChild(tablist);
}

function renderEditorFileList(family) {
  var panel = document.getElementById('editor-panel-' + family);
  if (!panel) return;
  panel.innerHTML = '';

  var files = editorState.files[family];
  if (files.length === 0) {
    var emptyMsg = document.createElement('div');
    emptyMsg.className = 'editor-tab-empty';
    var labels = {config: 'config files', 'drop-in': 'systemd drop-in files', quadlet: 'quadlet unit files'};
    emptyMsg.textContent = 'No ' + labels[family] + ' detected.';
    panel.appendChild(emptyMsg);
    return;
  }

  var list = document.createElement('div');
  list.className = 'editor-file-list';
  list.setAttribute('role', 'listbox');
  list.setAttribute('aria-label', family + ' files');

  var savedSelection = editorState.tabState[family].selectedPath;

  files.forEach(function(file, idx) {
    var item = document.createElement('div');
    item.className = 'editor-file-item';
    if (file.path === savedSelection) item.classList.add('selected');
    item.setAttribute('role', 'option');
    item.setAttribute('tabindex', idx === 0 ? '0' : '-1');
    item.setAttribute('aria-selected', file.path === savedSelection ? 'true' : 'false');
    item.setAttribute('data-file-path', file.path);

    if (isFileModified(file)) {
      item.setAttribute('aria-label', file.path.split('/').pop() + ', modified');
      var dot = document.createElement('span');
      dot.className = 'file-dot';
      dot.setAttribute('aria-hidden', 'true');
      item.appendChild(dot);
    }

    // Split path into dir + filename
    var lastSlash = file.path.lastIndexOf('/');
    if (lastSlash >= 0) {
      var dirSpan = document.createElement('span');
      dirSpan.className = 'file-path';
      dirSpan.textContent = file.path.substring(0, lastSlash + 1);
      item.appendChild(dirSpan);
      var nameSpan = document.createElement('span');
      nameSpan.className = 'file-name';
      nameSpan.textContent = file.path.substring(lastSlash + 1);
      item.appendChild(nameSpan);
    } else {
      var nameOnly = document.createElement('span');
      nameOnly.className = 'file-name';
      nameOnly.textContent = file.path;
      item.appendChild(nameOnly);
    }

    item.onclick = function() { handleFileClick(file); };
    list.appendChild(item);
  });

  // Roving tabindex keyboard navigation
  list.addEventListener('keydown', function(e) {
    var items = Array.prototype.slice.call(list.querySelectorAll('[role="option"]'));
    var idx = items.indexOf(document.activeElement);
    if (idx === -1) return;

    if (e.key === 'ArrowDown' && idx < items.length - 1) {
      e.preventDefault();
      items[idx].setAttribute('tabindex', '-1');
      items[idx + 1].setAttribute('tabindex', '0');
      items[idx + 1].focus();
    } else if (e.key === 'ArrowUp' && idx > 0) {
      e.preventDefault();
      items[idx].setAttribute('tabindex', '-1');
      items[idx - 1].setAttribute('tabindex', '0');
      items[idx - 1].focus();
    } else if (e.key === 'Enter') {
      e.preventDefault();
      items[idx].click();
    }
  });

  panel.appendChild(list);

  // Restore scroll position
  panel.scrollTop = editorState.tabState[family].scrollTop || 0;
}

function renderEditorSection() {
  var container = document.getElementById('section-editor');
  if (!container) return;
  container.innerHTML = '';

  var heading = document.createElement('h2');
  heading.className = 'section-heading';
  heading.id = 'heading-editor';
  heading.setAttribute('tabindex', '-1');
  heading.textContent = 'Edit Files';
  container.appendChild(heading);

  editorState.files = collectEditorFiles();

  // Check if any files exist at all
  var totalFiles = editorState.files.config.length +
                   editorState.files['drop-in'].length +
                   editorState.files.quadlet.length;
  if (totalFiles === 0) {
    var navLink = document.querySelector('[data-section="editor"]');
    if (navLink) navLink.parentElement.style.display = 'none';
    return;
  }

  var layout = document.createElement('div');
  layout.className = 'editor-layout';

  // File panel (tabs + file list)
  var filePanel = document.createElement('div');
  filePanel.className = 'editor-file-panel';

  renderEditorTabBar(filePanel);

  // Create tab panels for each family
  var families = ['config', 'drop-in', 'quadlet'];
  families.forEach(function(fam) {
    var panel = document.createElement('div');
    panel.id = 'editor-panel-' + fam;
    panel.setAttribute('role', 'tabpanel');
    panel.setAttribute('aria-labelledby', 'editor-tab-' + fam);
    panel.style.display = fam === editorState.activeTab ? '' : 'none';
    panel.style.flex = '1';
    panel.style.overflowY = 'auto';
    filePanel.appendChild(panel);
  });

  layout.appendChild(filePanel);

  // Content pane
  var contentPane = document.createElement('div');
  contentPane.className = 'editor-content-pane';
  contentPane.id = 'editor-content-pane';

  var emptyState = document.createElement('div');
  emptyState.className = 'editor-empty';
  emptyState.id = 'editor-empty';
  emptyState.textContent = 'Select a file to view';
  contentPane.appendChild(emptyState);

  layout.appendChild(contentPane);
  container.appendChild(layout);

  // ARIA live region for announcements
  var liveRegion = document.createElement('div');
  liveRegion.id = 'editor-live';
  liveRegion.setAttribute('aria-live', 'polite');
  liveRegion.setAttribute('class', 'sr-only');
  container.appendChild(liveRegion);

  // Render initial tab's file list
  renderEditorFileList(editorState.activeTab);
}
```

- [ ] **Step 2: Add `switchTab` function**

Add this after `renderEditorSection`:

```javascript
function switchTab(family) {
  if (family === editorState.activeTab) return;

  // Save scroll position of current tab
  var currentPanel = document.getElementById('editor-panel-' + editorState.activeTab);
  if (currentPanel) {
    editorState.tabState[editorState.activeTab].scrollTop = currentPanel.scrollTop;
  }

  // If dirty, this will be called after modal resolves
  function doSwitch() {
    editorState.activeTab = family;

    // Update tab aria-selected and tabindex
    var tabs = document.querySelectorAll('.editor-tab');
    tabs.forEach(function(t) {
      var isActive = t.getAttribute('data-tab') === family;
      t.setAttribute('aria-selected', isActive ? 'true' : 'false');
      t.setAttribute('tabindex', isActive ? '0' : '-1');
    });

    // Show/hide panels
    var families = ['config', 'drop-in', 'quadlet'];
    families.forEach(function(f) {
      var p = document.getElementById('editor-panel-' + f);
      if (p) p.style.display = f === family ? '' : 'none';
    });

    renderEditorFileList(family);

    // Restore selection or show empty
    var savedPath = editorState.tabState[family].selectedPath;
    if (savedPath) {
      var file = findFileByPath(savedPath);
      if (file) {
        showFileReadOnly(file);
        return;
      }
    }

    // No prior selection — show empty state or focus first file
    var fileItems = document.querySelectorAll('#editor-panel-' + family + ' [role="option"]');
    showEmptyState();

    if (fileItems.length > 0) {
      fileItems[0].focus();
    } else {
      // Empty tab — focus the tab button itself
      var tabBtn = document.getElementById('editor-tab-' + family);
      if (tabBtn) tabBtn.focus();
    }

    announceEditor('Showing ' + family + ' files, ' + editorState.files[family].length + ' files');
  }

  // Check dirty state
  if (editorState.mode === 'editing-dirty') {
    editorState.pendingNavAction = doSwitch;
    showUnsavedModal();
  } else {
    if (editorState.mode === 'editing-clean') exitEditMode();
    doSwitch();
  }
}
```

- [ ] **Step 3: Verify build**

Run: `go build -o /dev/null ./cmd/inspectah/`
Expected: Build succeeds.

- [ ] **Step 4: Commit**

```bash
git add cmd/inspectah/internal/renderer/static/report.html
git commit -m "feat(renderer): editor tab bar and per-tab file lists"
```

---

### Task 4: Content Pane — Read-Only View, Toolbar, State Machine

**Files:**
- Modify: `cmd/inspectah/internal/renderer/static/report.html` (JS: replace `openFileInEditor`)

- [ ] **Step 1: Replace `openFileInEditor` with state-machine functions**

Find the existing `openFileInEditor` function and replace it with these functions:

```javascript
function showEmptyState() {
  editorState.mode = 'empty';
  currentEditorFile = null;

  var pane = document.getElementById('editor-content-pane');
  if (!pane) return;
  pane.innerHTML = '';

  var empty = document.createElement('div');
  empty.className = 'editor-empty';
  empty.textContent = 'Select a file to view';
  pane.appendChild(empty);
}

function showFileReadOnly(file) {
  // Sync buffer back if we were editing a different file
  syncEditorBuffer();

  editorState.mode = 'readonly';
  currentEditorFile = file;
  editorState.tabState[file.family].selectedPath = file.path;

  // Update selection in file list
  updateFileListSelection(file.path);

  var pane = document.getElementById('editor-content-pane');
  if (!pane) return;
  pane.innerHTML = '';

  // Toolbar
  var toolbar = document.createElement('div');
  toolbar.className = 'editor-toolbar';

  var pathDisplay = document.createElement('div');
  pathDisplay.className = 'file-path-display';
  var lastSlash = file.path.lastIndexOf('/');
  if (lastSlash >= 0) {
    var dirPart = document.createElement('span');
    dirPart.style.opacity = '0.6';
    dirPart.textContent = file.path.substring(0, lastSlash + 1);
    pathDisplay.appendChild(dirPart);
    var namePart = document.createElement('span');
    namePart.style.fontWeight = '600';
    namePart.textContent = file.path.substring(lastSlash + 1);
    pathDisplay.appendChild(namePart);
  } else {
    pathDisplay.textContent = file.path;
  }
  toolbar.appendChild(pathDisplay);

  var actions = document.createElement('div');
  actions.className = 'toolbar-actions';
  var editBtn = document.createElement('button');
  editBtn.className = 'btn-edit';
  editBtn.textContent = 'Edit';
  editBtn.id = 'editor-edit-btn';

  if (App.mode === 'static') {
    editBtn.disabled = true;
    editBtn.setAttribute('aria-disabled', 'true');
    editBtn.title = 'Editing requires refine mode. Run: inspectah refine <tarball>';
  } else {
    editBtn.onclick = function() { enterEditMode(); };
  }

  actions.appendChild(editBtn);
  toolbar.appendChild(actions);
  pane.appendChild(toolbar);

  // Read-only content
  var content = document.createElement('pre');
  content.className = 'editor-readonly-content';
  content.textContent = file.savedContent;
  pane.appendChild(content);

  // Focus the Edit button
  editBtn.focus();

  announceEditor(file.path.split('/').pop() + ' selected');
}

function enterEditMode() {
  if (!currentEditorFile || App.mode === 'static') return;

  editorState.mode = 'editing-clean';
  currentEditorFile.bufferContent = currentEditorFile.savedContent;

  var pane = document.getElementById('editor-content-pane');
  if (!pane) return;
  pane.innerHTML = '';

  // Toolbar with Revert + Save
  renderEditToolbar(pane);

  // CodeMirror editor
  var editorArea = document.createElement('div');
  editorArea.style.flex = '1';
  editorArea.style.overflow = 'auto';
  pane.appendChild(editorArea);

  editorInstance = CM.createEditor(editorArea, currentEditorFile.savedContent, {
    onChange: function(newContent) {
      if (!currentEditorFile) return;
      currentEditorFile.bufferContent = newContent;

      var wasDirty = editorState.mode === 'editing-dirty';
      var isDirty = isFileDirty(currentEditorFile);

      if (isDirty && !wasDirty) {
        editorState.mode = 'editing-dirty';
        updateEditToolbarState();
      } else if (!isDirty && wasDirty) {
        editorState.mode = 'editing-clean';
        updateEditToolbarState();
      }
    }
  });

  // Register Escape at lower precedence (Task 6 adds this)
  announceEditor('Editing ' + currentEditorFile.path.split('/').pop());
}

function renderEditToolbar(pane) {
  var toolbar = document.createElement('div');
  toolbar.className = 'editor-toolbar';
  toolbar.id = 'editor-edit-toolbar';

  var pathDisplay = document.createElement('div');
  pathDisplay.className = 'file-path-display';
  var file = currentEditorFile;
  var lastSlash = file.path.lastIndexOf('/');
  if (lastSlash >= 0) {
    var dirPart = document.createElement('span');
    dirPart.style.opacity = '0.6';
    dirPart.textContent = file.path.substring(0, lastSlash + 1);
    pathDisplay.appendChild(dirPart);
    var namePart = document.createElement('span');
    namePart.style.fontWeight = '600';
    namePart.textContent = file.path.substring(lastSlash + 1);
    pathDisplay.appendChild(namePart);
  } else {
    pathDisplay.textContent = file.path;
  }

  // Unsaved badge (hidden initially)
  var badge = document.createElement('span');
  badge.className = 'unsaved-badge';
  badge.id = 'editor-unsaved-badge';
  badge.textContent = 'unsaved';
  badge.style.display = 'none';
  pathDisplay.appendChild(badge);
  toolbar.appendChild(pathDisplay);

  var actions = document.createElement('div');
  actions.className = 'toolbar-actions';

  var revertBtn = document.createElement('button');
  revertBtn.className = 'btn-revert';
  revertBtn.textContent = 'Revert';
  revertBtn.id = 'editor-revert-btn';
  revertBtn.onclick = function() { revertFile(); };
  actions.appendChild(revertBtn);

  var saveBtn = document.createElement('button');
  saveBtn.className = 'btn-save';
  saveBtn.textContent = 'Save';
  saveBtn.id = 'editor-save-btn';
  saveBtn.disabled = true;
  saveBtn.onclick = function() { saveFileAndExit(); };
  actions.appendChild(saveBtn);

  toolbar.appendChild(actions);
  pane.insertBefore(toolbar, pane.firstChild);
}

function updateEditToolbarState() {
  var badge = document.getElementById('editor-unsaved-badge');
  var saveBtn = document.getElementById('editor-save-btn');
  var isDirty = editorState.mode === 'editing-dirty';

  if (badge) badge.style.display = isDirty ? '' : 'none';
  if (saveBtn) saveBtn.disabled = !isDirty;
}

function syncEditorBuffer() {
  if (editorInstance && currentEditorFile) {
    currentEditorFile.bufferContent = editorInstance.state.doc.toString();
  }
}

function exitEditMode() {
  if (editorInstance) {
    syncEditorBuffer();
    editorInstance = null;
  }
  cancelPendingAutosave();
}

function saveFile() {
  if (!currentEditorFile) return;
  syncEditorBuffer();
  currentEditorFile.savedContent = currentEditorFile.bufferContent;
  currentEditorFile.ref.content = currentEditorFile.savedContent;
  editorState.mode = 'editing-clean';
  updateEditToolbarState();
  updateFileModifiedDots();
  announceEditor(currentEditorFile.path.split('/').pop() + ' saved');
  scheduleAutosave();
}

function saveFileAndExit() {
  if (!currentEditorFile) return;
  syncEditorBuffer();
  currentEditorFile.savedContent = currentEditorFile.bufferContent;
  currentEditorFile.ref.content = currentEditorFile.savedContent;
  exitEditMode();
  showFileReadOnly(currentEditorFile);
  updateFileModifiedDots();
  announceEditor(currentEditorFile.path.split('/').pop() + ' saved');
}

function revertFile() {
  if (!currentEditorFile) return;
  currentEditorFile.savedContent = currentEditorFile.originalContent;
  currentEditorFile.bufferContent = currentEditorFile.originalContent;
  currentEditorFile.ref.content = currentEditorFile.originalContent;
  exitEditMode();
  showFileReadOnly(currentEditorFile);
  updateFileModifiedDots();
  announceEditor(currentEditorFile.path.split('/').pop() + ' reverted to original');
}

function handleFileClick(file) {
  if (file === currentEditorFile && editorState.mode !== 'empty') return;

  function doFileSwitch() {
    if (editorState.mode.startsWith('editing')) exitEditMode();
    showFileReadOnly(file);
  }

  if (editorState.mode === 'editing-dirty') {
    editorState.pendingNavAction = doFileSwitch;
    showUnsavedModal();
  } else {
    doFileSwitch();
  }
}

function updateFileListSelection(path) {
  var items = document.querySelectorAll('#editor-panel-' + editorState.activeTab + ' [role="option"]');
  items.forEach(function(item) {
    var isSelected = item.getAttribute('data-file-path') === path;
    item.classList.toggle('selected', isSelected);
    item.setAttribute('aria-selected', isSelected ? 'true' : 'false');
  });
}

function updateFileModifiedDots() {
  // Update tab dots
  var tabs = document.querySelectorAll('.editor-tab');
  tabs.forEach(function(tab) {
    var family = tab.getAttribute('data-tab');
    var existingDot = tab.querySelector('.tab-dot');
    var needsDot = isAnyFileModifiedInFamily(family);
    if (needsDot && !existingDot) {
      var dot = document.createElement('span');
      dot.className = 'tab-dot';
      dot.setAttribute('aria-hidden', 'true');
      tab.appendChild(dot);
    } else if (!needsDot && existingDot) {
      existingDot.remove();
    }
  });

  // Re-render current tab's file list to update file dots
  renderEditorFileList(editorState.activeTab);
}

function announceEditor(message) {
  var live = document.getElementById('editor-live');
  if (live) live.textContent = message;
}
```

- [ ] **Step 2: Update `initEditor` to use new functions**

Replace the existing `initEditor` function:

```javascript
function initEditor() {
  renderEditorFileList(editorState.activeTab);
}
```

- [ ] **Step 3: Verify build**

Run: `go build -o /dev/null ./cmd/inspectah/`
Expected: Build succeeds.

- [ ] **Step 4: Commit**

```bash
git add cmd/inspectah/internal/renderer/static/report.html
git commit -m "feat(renderer): editor state machine with read-only and edit modes"
```

---

### Task 5: Unsaved Changes Modal

**Files:**
- Modify: `cmd/inspectah/internal/renderer/static/report.html` (JS)

- [ ] **Step 1: Add modal functions**

Add after the `announceEditor` function:

```javascript
function showUnsavedModal() {
  var file = currentEditorFile;
  if (!file) return;

  var backdrop = document.createElement('div');
  backdrop.className = 'editor-modal-backdrop';
  backdrop.id = 'editor-modal';
  backdrop.setAttribute('role', 'dialog');
  backdrop.setAttribute('aria-modal', 'true');
  backdrop.setAttribute('aria-labelledby', 'editor-modal-title');

  var modal = document.createElement('div');
  modal.className = 'editor-modal';

  var title = document.createElement('h3');
  title.id = 'editor-modal-title';
  title.textContent = 'Unsaved changes';
  modal.appendChild(title);

  var body = document.createElement('p');
  body.textContent = 'You have unsaved changes to ' + file.path.split('/').pop() + '. What would you like to do?';
  modal.appendChild(body);

  var actions = document.createElement('div');
  actions.className = 'editor-modal-actions';

  var cancelBtn = document.createElement('button');
  cancelBtn.className = 'btn-modal-cancel';
  cancelBtn.textContent = 'Cancel';
  cancelBtn.onclick = function() { dismissModal(false); };

  var discardBtn = document.createElement('button');
  discardBtn.className = 'btn-modal-discard';
  discardBtn.textContent = 'Discard';
  discardBtn.onclick = function() {
    // Drop buffer to savedContent (preserve checkpoint saves)
    currentEditorFile.bufferContent = currentEditorFile.savedContent;
    currentEditorFile.ref.content = currentEditorFile.savedContent;
    announceEditor('Changes discarded for ' + currentEditorFile.path.split('/').pop());
    dismissModal(true);
  };

  var saveBtn = document.createElement('button');
  saveBtn.className = 'btn-modal-save';
  saveBtn.textContent = 'Save';
  saveBtn.onclick = function() {
    syncEditorBuffer();
    currentEditorFile.savedContent = currentEditorFile.bufferContent;
    currentEditorFile.ref.content = currentEditorFile.savedContent;
    updateFileModifiedDots();
    announceEditor(currentEditorFile.path.split('/').pop() + ' saved');
    dismissModal(true);
  };

  actions.appendChild(cancelBtn);
  actions.appendChild(discardBtn);
  actions.appendChild(saveBtn);
  modal.appendChild(actions);
  backdrop.appendChild(modal);

  document.body.appendChild(backdrop);

  // Focus trap
  cancelBtn.focus();

  // Tab cycling within modal
  var focusables = [cancelBtn, discardBtn, saveBtn];
  backdrop.addEventListener('keydown', function(e) {
    if (e.key === 'Escape') {
      e.preventDefault();
      dismissModal(false);
    } else if (e.key === 'Tab') {
      e.preventDefault();
      var idx = focusables.indexOf(document.activeElement);
      var next = e.shiftKey ? (idx - 1 + focusables.length) % focusables.length : (idx + 1) % focusables.length;
      focusables[next].focus();
    }
  });

  // Click outside to cancel
  backdrop.addEventListener('click', function(e) {
    if (e.target === backdrop) dismissModal(false);
  });
}

function dismissModal(proceed) {
  var modal = document.getElementById('editor-modal');
  if (modal) modal.remove();

  if (proceed && editorState.pendingNavAction) {
    exitEditMode();
    var action = editorState.pendingNavAction;
    editorState.pendingNavAction = null;
    action();
  } else {
    editorState.pendingNavAction = null;
    // Return focus to editor
    if (editorInstance) {
      editorInstance.focus();
    }
  }
}
```

- [ ] **Step 2: Add `beforeunload` handler**

Add after the modal functions:

```javascript
window.addEventListener('beforeunload', function(e) {
  if (editorState.mode === 'editing-dirty') {
    e.preventDefault();
    e.returnValue = '';
  }
});
```

- [ ] **Step 3: Wire sidebar navigation interception**

Find the existing `show(sectionId)` function (or equivalent sidebar navigation handler). Wrap it with a dirty-state guard. The guard should check `editorState.mode === 'editing-dirty'` and show the modal before allowing navigation. Find where sidebar links call their click handler and add:

```javascript
// Add this guard before existing show() logic:
function guardedShow(sectionId) {
  if (editorState.mode === 'editing-dirty') {
    editorState.pendingNavAction = function() {
      exitEditMode();
      showSection(sectionId);
    };
    showUnsavedModal();
    return;
  }
  if (editorState.mode.startsWith('editing')) exitEditMode();
  showSection(sectionId);
}
```

Replace the existing `show()` call in sidebar link click handlers with `guardedShow()`. Keep the original navigation function as `showSection()`.

- [ ] **Step 4: Verify build**

Run: `go build -o /dev/null ./cmd/inspectah/`
Expected: Build succeeds.

- [ ] **Step 5: Commit**

```bash
git add cmd/inspectah/internal/renderer/static/report.html
git commit -m "feat(renderer): unsaved changes modal and navigation guards"
```

---

### Task 6: Keyboard Shortcuts and Escape Precedence

**Files:**
- Modify: `cmd/inspectah/internal/renderer/static/report.html` (JS)

- [ ] **Step 1: Add global keyboard handler for `E` and `Ctrl+S`**

Add after the `beforeunload` handler:

```javascript
document.addEventListener('keydown', function(e) {
  // E to enter edit mode from read-only (only when not in a text input)
  if (e.key === 'e' && !e.ctrlKey && !e.metaKey && !e.altKey &&
      editorState.mode === 'readonly' &&
      document.activeElement.tagName !== 'INPUT' &&
      document.activeElement.tagName !== 'TEXTAREA' &&
      !document.activeElement.classList.contains('cm-content')) {
    e.preventDefault();
    enterEditMode();
    return;
  }

  // Ctrl+S / Cmd+S to save in edit mode
  if ((e.ctrlKey || e.metaKey) && e.key === 's' && editorState.mode.startsWith('editing')) {
    e.preventDefault();
    if (editorState.mode === 'editing-dirty') {
      saveFile();
    }
    return;
  }
});
```

- [ ] **Step 2: Register Escape at lower precedence in CodeMirror**

In `enterEditMode()`, after `CM.createEditor(...)`, add a CodeMirror Escape handler at low precedence. The exact approach depends on how `CM.createEditor` works in the vendored build. Add this after the editor is created:

```javascript
// Escape at lower precedence — CM overlays (search, autocomplete) win
editorInstance.dom.addEventListener('keydown', function(e) {
  if (e.key !== 'Escape') return;
  // Check if CodeMirror handled it (search panel, autocomplete, etc.)
  // If a CM overlay consumed the event, it will be handled already.
  // We use setTimeout(0) to run after CM's own handlers.
  setTimeout(function() {
    // Only proceed if we're still in edit mode (CM didn't consume it)
    if (editorState.mode.startsWith('editing')) {
      if (editorState.mode === 'editing-dirty') {
        editorState.pendingNavAction = function() {
          exitEditMode();
          showFileReadOnly(currentEditorFile);
        };
        showUnsavedModal();
      } else {
        exitEditMode();
        showFileReadOnly(currentEditorFile);
      }
    }
  }, 0);
});
```

Note: The `setTimeout(0)` approach defers our handler until after CodeMirror's internal keymap processing. If CodeMirror's search panel or autocomplete consumed the Escape, those overlays will have closed, and the editor state will have changed accordingly. Test this by opening CodeMirror search (Ctrl+F) and pressing Escape — it should close search, not exit edit mode.

- [ ] **Step 3: Verify build**

Run: `go build -o /dev/null ./cmd/inspectah/`
Expected: Build succeeds.

- [ ] **Step 4: Commit**

```bash
git add cmd/inspectah/internal/renderer/static/report.html
git commit -m "feat(renderer): keyboard shortcuts and CodeMirror Escape precedence"
```

---

### Task 7: Static Mode and Editor Visibility

**Files:**
- Modify: `cmd/inspectah/internal/renderer/static/report.html` (JS)

- [ ] **Step 1: Remove the old static-mode editor hiding logic**

Find the block that hides the editor section in static mode (around line 940-946, containing something like `if (App.mode === 'static') editorNav.style.display = 'none'`). Remove or replace it so the editor remains visible in static mode. The `showFileReadOnly` function already handles static mode by disabling the Edit button.

- [ ] **Step 2: Verify the static-mode behavior**

The Edit button's disabled state is already handled in `showFileReadOnly()` — it checks `App.mode === 'static'` and sets `disabled = true` with tooltip text. Verify this is correct by searching for the static-mode check in the function.

- [ ] **Step 3: Commit**

```bash
git add cmd/inspectah/internal/renderer/static/report.html
git commit -m "fix(renderer): keep editor visible in static mode with disabled Edit"
```

---

### Task 8: Browser Verification

**Files:**
- No file changes — verification only

- [ ] **Step 1: Build and start the refine server**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
go build -o cmd/inspectah/inspectah-darwin-arm64 ./cmd/inspectah/
```

Then start refine on a tarball:
```bash
./cmd/inspectah/inspectah-darwin-arm64 refine <path-to-tarball> --port 8642
```

- [ ] **Step 2: Verify dark mode — tab bar, read-only view, file list**

```bash
dev-browser <<'SCRIPT'
const page = await browser.getPage("editor-test");
await page.goto("http://localhost:8642/#section-editor");
await new Promise(r => setTimeout(r, 2000));
const buf = await page.screenshot({ fullPage: false });
await saveScreenshot(buf, "editor-tabs-readonly.png");
console.log("Screenshot saved");
SCRIPT
```

Expected: Three tabs visible (Config, Drop-ins, Quadlets), file list showing, content pane showing "Select a file to view" or a read-only file view.

- [ ] **Step 3: Verify file selection → read-only → edit → save flow**

Click a file, verify Edit button appears. Click Edit, verify CodeMirror loads. Type some text, verify "unsaved" badge appears and Save enables. Click Save, verify returns to read-only.

- [ ] **Step 4: Verify unsaved changes modal**

In edit mode, type changes, then click a different file. Verify modal appears with Cancel/Discard/Save. Test each button.

- [ ] **Step 5: Verify keyboard shortcuts**

Test `E` to enter edit mode, `Escape` to exit, `Ctrl+S` to checkpoint save. Test Escape while CodeMirror search is open (Ctrl+F → Escape should close search, not exit edit).

- [ ] **Step 6: Verify tab switching with modified dots**

Edit and save a file. Verify blue dot appears on the file and tab. Switch tabs, verify the dot persists. Switch back, verify selection is restored.

- [ ] **Step 7: Verify light mode**

Toggle theme, verify all editor components look correct in light mode.

- [ ] **Step 8: Commit verification notes**

If any fixes were needed, commit them. Then:

```bash
git add -A
git commit -m "test(renderer): verify editor redesign in browser"
```
