# Editor Behavior Fixes Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix three editor UX issues: revert exits edit mode, file list sorted with path display, re-render button activates on save.

**Architecture:** All changes in `_editor_js.html.j2` (client-side JS). No Python, schema, or rendering changes.

**Tech Stack:** Vanilla JS, PF6 CSS tokens

**Spec:** `docs/specs/proposed/2026-03-16-editor-behavior-fixes-design.md`

---

## File Map

- **Modify:** `src/yoinkc/templates/report/_editor_js.html.j2` — all three fixes

No test file changes — these are client-side behavior fixes verified manually.

---

## Task 1: Revert exits edit mode

**Files:**
- Modify: `src/yoinkc/templates/report/_editor_js.html.j2` — `editorRevert()` function (lines ~248-268)

- [ ] **Step 1: Rewrite `editorRevert()`**

Find the current function:

```js
  function editorRevert() {
    var f = editorState.currentFile;
    if (!f) return;
    var origItems = (originalSnapshot[f.section] && originalSnapshot[f.section][f.list]) || [];
    var origItem = origItems.find(function(item) { return item.path === f.path; });
    var origContent = origItem ? (origItem.content || '') : '';

    snapshot[f.section][f.list][f.index].content = origContent;
    editorState.dirtyFiles.delete(f.path);
    editorState.savedFiles.add(f.path);

    if (window.CMEditor && editorState.cmView) {
      CMEditor.setContent(editorState.cmView, origContent);
    } else {
      var ta = document.getElementById('editor-textarea-fallback');
      if (ta) ta.value = origContent;
    }
    updateEditToolbar();
    updateStateLabels();
    updateChangedCount();
  }
```

Replace with:

```js
  function editorRevert() {
    var f = editorState.currentFile;
    if (!f) return;
    var origItems = (originalSnapshot[f.section] && originalSnapshot[f.section][f.list]) || [];
    var origItem = origItems.find(function(item) { return item.path === f.path; });
    var origContent = origItem ? (origItem.content || '') : '';

    // Restore snapshot content from original (undoes any prior save)
    snapshot[f.section][f.list][f.index].content = origContent;
    editorState.dirtyFiles.delete(f.path);
    editorState.savedFiles.delete(f.path);

    // Exit edit mode and re-render read-only view
    doSelectFile(f.section, f.list, f.index, f.path);
    updateChangedCount();
  }
```

Key changes:
- `savedFiles.add(f.path)` → `savedFiles.delete(f.path)` — content matches original, nothing to re-render
- No CM6/textarea manipulation needed — `doSelectFile()` handles teardown by setting `editMode = false`, hiding the CM container, and showing the read-only view
- `doSelectFile()` already calls `updateStateLabels()`
- `updateChangedCount()` called after to sync button state (for Fix 3)

- [ ] **Step 2: Verify behavior**

Manual test:
1. Open editor, click Edit on a file, make changes
2. Click Revert → should exit edit mode, show read-only view, no "not rendered" label
3. Click Edit again → should re-enter edit mode with original content
4. Edit → Save → Edit more → Revert → should undo the save too (file back to original, "not rendered" label removed)

- [ ] **Step 3: Commit**

```
git add src/yoinkc/templates/report/_editor_js.html.j2
git commit -m "fix: revert exits edit mode and undoes prior saves

Revert now calls doSelectFile() to return to read-only view instead
of staying in edit mode. Uses savedFiles.delete() instead of .add()
since content matches original — nothing to re-render."
```

---

## Task 2: File list ordering and path display

**Files:**
- Modify: `src/yoinkc/templates/report/_editor_js.html.j2` — `buildTree()` function (lines ~12-58)

- [ ] **Step 1: Change category order**

Find the sections array in `buildTree()`:

```js
    var sections = [
      {section: 'config', list: 'files', label: 'config'},
      {section: 'containers', list: 'quadlet_units', label: 'quadlet'},
      {section: 'services', list: 'drop_ins', label: 'drop-ins'},
    ];
```

Replace with:

```js
    var sections = [
      {section: 'config', list: 'files', label: 'config'},
      {section: 'services', list: 'drop_ins', label: 'drop-ins'},
      {section: 'containers', list: 'quadlet_units', label: 'quadlet'},
    ];
```

- [ ] **Step 2: Add sort within categories**

Find the line `items.forEach(function(item, idx) {` (currently line ~33).

Add a sort before it. Because we need the original indices for snapshot
access, create a sorted index array:

```js
      var sorted = items.map(function(item, idx) { return {item: item, idx: idx}; });
      sorted.sort(function(a, b) { return a.item.path.localeCompare(b.item.path); });

      sorted.forEach(function(entry) {
        var item = entry.item;
        var idx = entry.idx;
```

Replace the existing `items.forEach(function(item, idx) {` with the
above. The rest of the forEach body stays the same. Update the closing
`});` to match.

**Important:** We sort the display order but preserve the original array
index (`idx`) so that `selectFile(sec.section, sec.list, idx, item.path)`
still points to the correct snapshot entry.

- [ ] **Step 3: Add path display with dimmed prefix**

Find the nameSpan creation inside the forEach:

```js
        var nameSpan = document.createElement('span');
        nameSpan.textContent = item.path.split('/').pop();
        nameSpan.title = item.path;
        entry.appendChild(nameSpan);
```

Replace with:

```js
        var nameSpan = document.createElement('span');
        nameSpan.style.cssText = 'overflow:hidden; text-overflow:ellipsis; white-space:nowrap; flex:1; min-width:0;';
        var lastSlash = item.path.lastIndexOf('/');
        var dir = item.path.substring(0, lastSlash + 1);
        var fname = item.path.substring(lastSlash + 1);
        nameSpan.innerHTML = '<span style="color:var(--pf-t--global--text--color--subtle);">'
          + escapeHtml(dir) + '</span><strong>' + escapeHtml(fname) + '</strong>';
        nameSpan.title = item.path;
        entry.appendChild(nameSpan);
```

This shows `/etc/ssh/` dimmed with `sshd_config` bold. The `flex:1;
min-width:0` allows text-overflow ellipsis to work within the flex
container. Long paths truncate with `...`.

- [ ] **Step 4: Verify behavior**

Manual test:
1. Open editor → file list shows config first, then drop-ins, then quadlet
2. Within each category, files are sorted alphabetically by full path
3. Each file shows dimmed directory prefix + bold filename
4. Clicking a file still opens the correct file content
5. Long paths show ellipsis instead of overflowing

- [ ] **Step 5: Commit**

```
git add src/yoinkc/templates/report/_editor_js.html.j2
git commit -m "feat: sort file list by path with dimmed directory prefix

Category order: config → drop-ins → quadlet. Files sorted
alphabetically by full path within each category. Display shows
dimmed directory prefix + bold filename with ellipsis for overflow."
```

---

## Task 3: Re-render button activation

**Files:**
- Modify: `src/yoinkc/templates/report/_editor_js.html.j2` — `updateChangedCount()` function (lines ~389-392)

- [ ] **Step 1: Extend `updateChangedCount()` to toggle button state**

Find the current function:

```js
  function updateChangedCount() {
    var countEl = document.getElementById('editor-changed-count');
    if (countEl) countEl.textContent = editorState.savedFiles.size;
  }
```

Replace with:

```js
  function updateChangedCount() {
    var countEl = document.getElementById('editor-changed-count');
    if (countEl) countEl.textContent = editorState.savedFiles.size;
    var reRenderBtn = document.getElementById('btn-re-render');
    if (reRenderBtn) reRenderBtn.disabled = (editorState.savedFiles.size === 0);
  }
```

- [ ] **Step 2: Clear savedFiles on successful re-render**

Find the re-render button click handler (lines ~512-536). Look for the
fetch success handler (the `.then` that processes the successful
response). After the page reload/replacement logic, add:

```js
    editorState.savedFiles.clear();
    updateChangedCount();
```

If the re-render replaces the page entirely (via `location.reload()` or
`document.write()`), this cleanup may not be needed since the page
resets. Check the actual implementation — if it does a full page
replacement, skip this step.

- [ ] **Step 3: Verify behavior**

Manual test:
1. Open editor, edit a file, save it → re-render button should enable
2. Save more files → count updates, button stays enabled
3. Click re-render → after success, button should disable (or page reloads)
4. Revert a file (without saving first) → button should stay disabled
5. Save a file, then revert it → button should disable (savedFiles now empty)

- [ ] **Step 4: Commit**

```
git add src/yoinkc/templates/report/_editor_js.html.j2
git commit -m "fix: enable re-render button when files are saved

updateChangedCount() now toggles btn-re-render disabled state based
on savedFiles.size. Button enables on save, disables when savedFiles
is empty (after revert or re-render)."
```

---

## Task 4: Final verification

- [ ] **Step 1: Run test suite**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
python -m pytest tests/ -x -q
```

Expected: all tests pass.

- [ ] **Step 2: End-to-end manual test**

Generate a report and verify all three fixes together:
1. File list is sorted with path display
2. Edit → revert exits cleanly
3. Edit → save → re-render button enables
4. Revert after save → re-render button disables
5. Edit → save → re-render → page reloads cleanly
