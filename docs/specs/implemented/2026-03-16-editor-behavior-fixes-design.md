# Editor Behavior Fixes Design

**Date:** 2026-03-16
**Status:** Proposed

## Problem

Three editor UX issues found during manual testing:

1. **Revert traps in edit mode** — reverting a file restores original
   content but stays in edit mode with no way to exit. Saving the
   unchanged file marks it "not rendered" in the file list even though
   nothing changed.
2. **File list has no structure** — files within each category appear in
   snapshot array order (insertion order), not alphabetically. Only the
   filename is shown, with no path context.
3. **Re-render button never activates** — the re-render button starts
   disabled and nothing ever enables it, even after saving edited files.

## Scope

All changes are in `_editor_js.html.j2` (JS behavior) with no changes
to Python code, rendering pipeline, or snapshot schema.

### Out of scope

- Duplicate re-render button issue (toolbar spec)
- Fleet variant display in editor (larger spec)
- Back navigation (toolbar spec)

## Fix 1: Revert exits edit mode

**File:** `src/yoinkc/templates/report/_editor_js.html.j2`
**Function:** `editorRevert()` (lines ~248-268)

Current behavior:
- Restores original content via CM6 `setContent()` or textarea
- Adds file to `savedFiles` (marks "not rendered")
- Stays in edit mode (`editorState.editMode` remains `true`)

New behavior:
- Restore original content in both the editor buffer AND the snapshot
  array (`snapshot[section][list][index].content`) from
  `originalSnapshot` — this undoes any prior save, not just unsaved
  buffer changes
- Do NOT add to `savedFiles` — content matches original, nothing to
  re-render. Remove from `savedFiles` if previously saved.
- Remove from `dirtyFiles` if present
- Set `editorState.editMode = false`
- Tear down CodeMirror / textarea (same cleanup path as `doSelectFile`)
- Re-render the file in read-only view mode
- Update toolbar, state labels, and changed count

Revert = "undo ALL my edits (saved or not) and go back to viewing." If
the user wants to edit again from clean, they click Edit.

## Fix 2: File list ordering and path display

**File:** `src/yoinkc/templates/report/_editor_js.html.j2`
**Function:** `buildTree()` (lines ~12-58)

### Category order

Change from: config → quadlet → drop-ins
Change to: **config → drop-ins → quadlet**

Quadlets are less common; drop-ins should come second.

### Sort within categories

Add `items.sort(function(a, b) { return a.path.localeCompare(b.path); })`
before the forEach loop that renders file entries.

### Path display

Currently shows filename only (e.g., `sshd_config`). Change to show
dimmed directory prefix + bold filename:

```
/etc/ssh/sshd_config
```

Where `/etc/ssh/` is rendered in PF6 subtle text color and `sshd_config`
is bold.

Implementation:
- Split path on last `/` to separate directory from filename
- Directory prefix: `color:var(--pf-t--global--text--color--subtle)`
- Filename: `<strong>filename</strong>`
- Sort by full path (which naturally groups files in the same directory)
- No truncation — long paths handled by CSS `overflow:hidden;
  text-overflow:ellipsis; white-space:nowrap` on the file entry element

## Fix 3: Re-render button activation

**File:** `src/yoinkc/templates/report/_editor_js.html.j2`
**Function:** `updateChangedCount()` (lines ~389-392)

Current behavior:
- Updates the `editor-changed-count` span text with saved file count
- Never touches the re-render button's `disabled` attribute

New behavior:
- After updating count text, toggle `btn-re-render.disabled`:
  - `savedFiles.size > 0` → `disabled = false`
  - `savedFiles.size === 0` → `disabled = true`
- Ensure `updateChangedCount()` is called from:
  - `editorSave()` (already calls it)
  - `editorRevert()` (add call after removing from savedFiles)
  - `editorSaveAll()` (already calls it)
- On successful re-render response, clear `savedFiles` and call
  `updateChangedCount()` to re-disable the button

This spec only addresses the existing `btn-re-render` button. The
duplicate button issue is deferred to the toolbar/navigation spec.

## Testing

- **Fix 1:** Edit a file → revert → verify exits edit mode, file shows
  no label (not "not rendered"), can click Edit to re-enter
- **Fix 2:** Verify files sorted alphabetically by path within each
  category. Verify path prefix is dimmed, filename is bold. Verify
  category order: config → drop-ins → quadlet.
- **Fix 3:** Save an edited file → verify re-render button enables.
  Re-render → verify button disables again. Revert without saving →
  verify button stays disabled (or disables if it was enabled).
- Existing Python tests unaffected (no rendering/snapshot logic changes).
