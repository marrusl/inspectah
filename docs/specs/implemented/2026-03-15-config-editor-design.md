# Config Editor Design

**Date:** 2026-03-15
**Status:** Implemented
**Branch:** `feature/config-editor-chunk1` (merged to main, 13 commits, 604 tests)
**Scope:** Transform the read-only file browser into an interactive config editor in inspectah-refine mode.

## Overview

The inspectah HTML report includes a file browser tab that displays config files, quadlet units, and systemd drop-ins. The file browser was read-only and largely redundant with the config/quadlet/drop-in tabs that showed the same content inline. This design transforms the file browser into an interactive editor that lets operators modify file content, create new files, and re-render the report—producing export-ready output with all edits baked into the tarball.

The editor is a refine-mode feature only. The static HTML report is unaffected.

## Goals

- Let operators edit config files, quadlet units, and systemd drop-ins directly in the browser.
- Let operators create new files of any of these three types.
- Edits are authoritative: they flow through re-render into the Containerfile and tarball output.
- Eliminate content redundancy between the file browser and other tabs.
- Preserve the static report as a self-contained, dependency-free HTML file.

## Non-Goals

- Editing the generated Containerfile (it is a rendered artifact, not a source).
- Line-level diffs of modifications in the audit report.
- Multi-user collaboration or server-side persistence of edits.

## Architecture

### Client-Side Snapshot Editing

All editing happens in the browser. The report page holds the full inspection snapshot in a `snapshot` JavaScript object (already embedded for the reset-to-original-inspection feature). Edits update this object directly.

- **Save** writes the CodeMirror buffer into `snapshot`—instant, no network round-trip.
- **Re-render** POSTs `{"snapshot": {...}, "original": {...}}` to the `POST /api/re-render` endpoint on inspectah-refine. The server runs inspectah with `--from-snapshot` and `--original-snapshot`, passing both snapshots so the original can be re-embedded and the audit diff can be computed.

This approach requires no new API endpoints for editing. The single source of truth is the in-memory `snapshot` object.

### Original Snapshot Storage

The report page embeds two copies of the snapshot: `snapshot` (mutable, receives edits) and `originalSnapshot` (immutable, never modified). `originalSnapshot` serves three purposes:

1. **Revert to snapshot** — restores a file's content to its original inspection value (uses path-based lookup, not index-based, to handle splice operations correctly).
2. **State detection** — determines whether a file has been modified (content differs from original) or added (not present in original).
3. **Audit diff** — sent alongside `snapshot` in the re-render request so the server can compute the modifications section.

### Preserving originalSnapshot Across Re-renders

The re-render flow does a full page replacement (`document.open(); document.write(html); document.close()`). To preserve the original across re-renders:

- The re-render request sends both `snapshot` and `originalSnapshot` as `{"snapshot": {...}, "original": {...}}`.
- inspectah-refine writes `original-snapshot.json` to the temp input dir and passes `--original-snapshot /input/original-snapshot.json` to inspectah.
- inspectah reads the original snapshot file and embeds it as `original_snapshot_json` in the template context.
- The template embeds them as two separate variables (no deep copy):

```javascript
var snapshot = {{ snapshot_json|safe }};
var originalSnapshot = {{ original_snapshot_json|safe }};
```

### Refine Mode Detection

The `--refine-mode` CLI flag threads through: `cli.py` → `__main__.py` (via `functools.partial`) → `run_all()` → `html_report.render()` → `_build_context()`. The template uses `{% if refine_mode %}` blocks to conditionally render editor UI.

inspectah-refine passes `--refine-mode` to the inspectah container command during re-render.

### inspectah-refine Changes

- **Re-render API:** Accepts wrapper format `{"snapshot": {...}, "original": {...}}` with backward compatibility for bare snapshot JSON. Mounts `original-snapshot.json` alongside the snapshot. Passes `--refine-mode` and `--original-snapshot` flags.
- **Re-render button:** Added to the toolbar alongside Reset to Original Inspection. Displays a count of files in `not rendered` state. Only saved files count.

### Data Flow

```
Operator edits file in CodeMirror
  → clicks Save → snapshot.config.files[i].content updated (instant)
  → clicks Re-render → POST /api/re-render with {snapshot, original}
  → inspectah-refine writes both to temp dir
  → inspectah runs --from-snapshot --original-snapshot --refine-mode
  → writes config/, quadlet/, drop-in files with modified content
  → generates Containerfile with COPY directives for all files
  → returns new report.html (with originalSnapshot re-embedded)
  → browser replaces page content
```

Tarball output includes the modified files automatically since inspectah writes snapshot content to disk.

**Re-render failure:** The browser shows an error via `alert()` (placeholder — PF6 error banner planned). All `not rendered` labels remain — no state is cleared on failure.

**After successful re-render:** All `not rendered` labels clear. Any files still in `unsaved` state remain `unsaved`.

## Editor Tab UI

### Two-Pane Layout

The existing file browser layout is preserved: left tree pane, right content pane.

**Tree pane:**
- Displays config files, quadlet units, and systemd drop-ins in a collapsible tree. `_build_output_tree()` walks `config/`, `quadlet/`, and `drop-ins/` directories. Drop-ins are dual-written to both `config/` and `drop-ins/` by the Containerfile renderer.
- A **+ New File** button in the tree toolbar opens the new file creation modal.
- PF6 compact labels on tree entries indicate file state.
- No zebra striping—follows PF6 tree view convention (hover highlights, selected state).

**Content pane — read-only mode (default):**
- Clicking a file in the tree opens it read-only as plain text.
- Toolbar shows the file path, a `read-only` badge, and an **Edit** button.

**Content pane — edit mode:**
- Clicking Edit loads CodeMirror 6 with line numbers and basic keybindings.
- Toolbar shows the file path, a state label, and buttons: **Revert to snapshot**, **Save All**, **Save** (plus **Delete** for operator-created files).
- Clicking another file while the current file has unsaved edits shows a `confirm()` dialog (placeholder — PF6 modal planned).
- Discarding edits clears the dirty buffer so changes don't silently reappear.

### File State Labels

PF6 compact label pills in the tree and editor toolbar:

| State | Label | Color | Meaning |
|-------|-------|-------|---------|
| Clean | (none) | — | Content matches original inspection, or saved + re-rendered |
| Not saved | `unsaved` | Red (PF6 danger) | Edits in CodeMirror buffer, not yet saved to snapshot |
| Not rendered | `not rendered` | Blue (PF6 info) | Saved to snapshot, report not yet re-rendered |

**State transitions:**

```
Clean → (operator types in CodeMirror) → Unsaved
Unsaved → (clicks Save / Save All) → Not Rendered
Unsaved → (clicks Revert to snapshot) → Clean
Not Rendered → (successful Re-render) → Clean
Not Rendered → (operator types again) → Unsaved
Not Rendered → (clicks Revert to snapshot) → Not Rendered (content reverted to original, but report still shows old rendered content)
Not Rendered → (failed Re-render) → Not Rendered (unchanged)
```

### Toolbar Actions

- **Save** — writes current file's CodeMirror buffer into `snapshot` (instant, no network).
- **Save All** — saves all files with unsaved edits into `snapshot` at once.
- **Revert to snapshot** — restores the current file to its original inspection content (path-based lookup into `originalSnapshot`), writes through to `snapshot`, and marks as needing re-render.
- **Edit** — switches from read-only mode to CodeMirror edit mode (read-only mode only).
- **Delete** — removes operator-created files from the snapshot entirely (only shown for files not in `originalSnapshot`).

## Other Tabs — "Edit in Editor" Linking

In refine mode, the config, quadlet, and drop-in tabs replace their inline content `<details>` elements with a **"View & edit in editor →"** link. Clicking the link navigates to the Editor tab with that file selected in read-only mode. The operator clicks Edit if they want to modify it.

State labels (`unsaved`, `not rendered`) also appear on the card headers in these tabs.

In the static report, the content pulldowns remain unchanged.

## New File Creation

A PF6 modal dialog triggered by the **+ New File** button. The form adapts based on selected file type.

### Config File

- **File type:** radio group — Config file / Quadlet unit / Systemd drop-in.
- **File path:** text input for the absolute path on the target system.
- Creates a `ConfigFileEntry` with `kind: UNOWNED`, empty content, `include: True`.

### Quadlet Unit

- **Scope:** dropdown — "System (root)" or "User — \<username\>" populated from `UserGroupSection.users` in the snapshot.
- **Unit name:** text input (e.g., `myapp.container`).
- **Full path (read-only preview):** assembled from scope and name.
- Creates a `QuadletUnit` with `name` derived from filename, empty `content` and `image`, `include: True`.

### Systemd Drop-in

- **Service:** dropdown populated from `snapshot.services.state_changes` (each entry has a `.unit` field).
- **Override filename:** text input (default: `override.conf`).
- **Full path (read-only preview):** assembled from service and filename.
- Creates a `SystemdDropIn` with `unit` from selected service, empty `content`, `include: True`.

All types: after clicking **Create & edit**, the file opens immediately in the editor in edit mode with `unsaved` state.

### Validation

- **Path required:** the Create button is disabled until a non-empty path is provided.
- **Absolute path:** config file paths must start with `/`. Inline error if relative.
- **Duplicate detection:** inline error if path already exists in snapshot.
- **Quadlet/drop-in filename:** must end with a valid extension.

### Deleting Operator-Created Files

Operator-created files can be removed from the snapshot entirely via a **Delete** button in the editor toolbar (only shown for files not in `originalSnapshot`). Original inspection files can only be excluded via include/exclude toggles, not deleted.

## Audit Report — Modifications Section

A new "Modifications" section at the end of the audit report (both markdown and HTML tab) listing operator changes at the path level:

- **Edited** — files whose content differs from the original inspection.
- **Added** — files created by the operator that did not exist in the original snapshot.
- **Removed** — already tracked via include/exclude.

The section only appears if there are modifications. Computed server-side during re-render by `_compute_modifications()` in `audit_report.py`. The original snapshot is read once in `run_all()` and passed to both `html_report.render()` and `audit_report.render()`.

## CodeMirror 6 Integration

- **Bundle:** CodeMirror 6 via the `codemirror` meta-package (~373KB minified IIFE). Built by `scripts/build-codemirror.sh` for reproducibility.
- **Delivery:** Vendored at `src/inspectah/static/codemirror/codemirror.min.js`. Inlined in the report HTML via `<script>{{ codemirror_js }}</script>` — same pattern as the PatternFly CSS. No separate CSS file (CM6 themes are JS-based). No `/static/` route needed on inspectah-refine.
- **Loading:** Conditionally inlined when `refine_mode` is true. Static reports never include it.
- **Instance management:** One CodeMirror instance, content swapped per file. Undo history resets on file switch.
- **Keyboard shortcuts:** Ctrl+S / Cmd+S triggers Save.
- **API:** Bundle exports `window.CMEditor` with `create(parent, content)`, `getContent(view)`, `setContent(view, content)`.
- **Dirty tracking:** Uses keyup polling on the CM6 container (the bundle API doesn't expose an onChange callback; improving this is planned).
- **Package data:** `pyproject.toml` includes `static/**/*` so the bundle ships with the installed package.

## Static Report

The static HTML report is completely unaffected by this feature:

- Keeps the read-only file browser tab.
- Keeps content pulldowns in config/quadlet/drop-in tabs.
- No CodeMirror dependency.
- No editor UI, state labels, or "Edit in editor" links.
- Self-contained single HTML file, no external assets.

## Testing

604 tests passing. Test files:
- `tests/test_editor.py` — editor tab rendering, CM6 embedding, dirty tracking, cross-tab links, new file modal, re-render button, integration.
- `tests/test_html_report_output.py` — original snapshot embedding, refine_mode defaults, drop-ins tree.
- `tests/test_audit_report_output.py` — modifications section (edited/added/unchanged).
- `tests/test_inspectah_refine.py` — wrapper format re-render.

## Future Considerations

- **Checkboxes → toggle switches:** Convert include/exclude checkboxes to PF6 toggle switches.
- **Persistence across page refresh:** `POST /api/snapshot` endpoint to persist in-memory snapshot to disk.
- **Syntax-aware editing:** CodeMirror language modes for INI, TOML, systemd unit files.
- **Consolidate companion tools:** Fold inspectah-refine and inspectah-fleet into inspectah as subcommands (`inspectah refine`, `inspectah fleet`). Both are pure Python — refine becomes simpler (re-render is a function call, not container-in-container). Leave build separate (needs podman/buildah on host).
- **`confirm()` → PF6 modal:** Replace browser `confirm()` for unsaved-changes navigation guard with a proper PF6 modal (save/discard/cancel).
- **`alert()` → PF6 error banner:** Replace browser `alert()` for re-render errors with an inline PF6 alert banner.
- **CM6 dirty tracking:** Replace keyup polling with CM6's `updateListener` extension by extending `build.mjs` to accept an onChange callback.
- **Vim keybindings:** Optional `@replit/codemirror-vim` extension with a toggle in the editor toolbar.
