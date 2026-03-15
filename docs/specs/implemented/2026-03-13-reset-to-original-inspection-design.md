# Reset to Original Inspection

**Date:** 2026-03-13
**Status:** Proposed

## Problem

During interactive refinement (`yoinkc-refine`), users toggle include/exclude
checkboxes and change provisioning strategies. There is no way to undo all
changes and return to the state the inspectors originally produced. Users
who want to start over must re-run the inspection or manually re-check
every item.

## Design

### Approach

**Deep-copy the snapshot at page load; reset restores from the copy.**

When the report renders, a second JS variable `originalSnapshot` is created
as a deep copy of `snapshot`. This copy is never modified — all runtime
mutations go to `snapshot`. The reset function walks `originalSnapshot` and
applies each `include` and `strategy` value back to `snapshot` and the DOM.

This was chosen over two alternatives:

- **Pipeline preserves original snapshot** — saves an
  `inspection-snapshot.original.json` on first run, embeds both snapshots.
  Enables true cross-session reset but requires pipeline changes, schema
  awareness of "original vs current," and lifecycle decisions. Deferred as
  a future enhancement if needed.
- **Hardcode inspector defaults** — set all `include` to `true` and
  recompute strategy defaults in JS. Fragile — strategy heuristics depend
  on inspection data and would need to be replicated and kept in sync.

### Data Layer

Immediately after `var snapshot = {{ snapshot_json|safe }};` in the
template's `<script>` block, add:

```js
var originalSnapshot = JSON.parse(JSON.stringify(snapshot));
```

The `resetToOriginal()` function:

1. Walks all DOM elements with `data-snap-section` / `data-snap-list` /
   `data-snap-index` attributes (both `<tr>` and `<div>` containers —
   compose files use `<div>`, not `<tr>`).
2. For each, reads the original `include` and `strategy` values from
   `originalSnapshot` by direct indexing:
   `originalSnapshot[section][list][idx]`. Do not use the existing
   `resolveSnapshotRef()` since it reads from `snapshot`, not
   `originalSnapshot`.
3. Applies values to both the live `snapshot` object and the DOM:
   - Checkbox `.checked` state
   - Checkbox `.disabled` state on `.leaf-cb` elements only (reset to
     `false` — repo cascade may have disabled these; the cascade
     re-run in step 4 will re-disable any that should be). Do not
     touch `.disabled` on repo checkboxes — default distribution
     repos are permanently disabled via the Jinja template and must
     stay that way.
   - Strategy select `.value`
   - Element `.excluded` class (add/remove)
4. Re-runs package cascade logic: `recomputeAutoDeps()`,
   `updatePkgBanner()`. Also re-runs repo cascade logic for any
   repo checkboxes that are unchecked in the original state (to
   correctly disable/enable associated leaf packages). The repo
   cascade body should be extracted into a callable function so
   both the event handler and `resetToOriginal()` can invoke it.
5. Calls `updateToolbar()` and `setDirty(false)`.
6. Shows toast: "Selections reset to initial state".

**Semantic note:** `originalSnapshot` captures page-load state. After a
re-render (which replaces the page via `document.write`), the new page
creates a fresh `originalSnapshot` from the re-rendered snapshot. "Reset"
therefore means "undo changes made in this session," not "undo all past
sessions." The button label "Reset" (not "Reset to original inspection")
and the confirmation dialog text reflect this: "Reset all selections to
their initial state? This cannot be undone." This is accurate regardless
of whether the page was loaded from a fresh inspection or a previously
refined snapshot.

### UI

A "Reset" button in the toolbar (`#exclude-toolbar`). Toolbar button
order: `[status text] [Reset] [Download Modified Snapshot] [Re-render]
[Download Tarball]`.

- PF6 classes: `pf-v6-c-button pf-m-link pf-m-danger` — visually
  lightweight but the danger modifier signals destructiveness.
- `id="btn-reset"`
- Starts `disabled` in the template HTML (Jinja-side, like
  `btn-rerender`). Enabled via JS when `!isSnapshotClean()`.
- `updateToolbar()` gains one line: set `btn-reset.disabled` based on
  `isSnapshotClean()`.
- On click: `confirm("Reset all selections to their initial state? This cannot be undone.")`. If confirmed, calls `resetToOriginal()`.
- The button is only interactive when yoinkc-refine is active (checkboxes
  are hidden in standalone mode, so no changes can be made and the button
  stays disabled). No special visibility handling needed beyond the
  existing toolbar visibility logic.

### Scope

**In scope:**
- `originalSnapshot` deep copy at page load
- Reset button in toolbar with disabled/enabled state
- `resetToOriginal()` function restoring includes, strategies, and
  cascaded state
- Confirmation dialog
- Toast feedback
- Python-side tests: button present in rendered HTML, `originalSnapshot`
  embedded in script

**Out of scope:**
- Cross-session reset (Approach B)
- Undo/redo
- New Python modules, schema changes, or pipeline changes

## Testing

**Python (pytest):**
- Rendered HTML contains `<button id="btn-reset"` with `disabled`
  attribute (set in the Jinja template, not by JS).
- Rendered HTML contains `var originalSnapshot =` in the script block.
- Toast element already exists (no new test needed for toast).

**Manual (browser):**
- Button disabled when no changes made.
- Button enables after unchecking a checkbox or changing a strategy.
- Click shows confirmation dialog.
- Confirming resets all checkboxes and strategy selects to original state.
- Package banner updates correctly after reset.
- Toolbar shows "No pending changes" after reset.
- Toast confirms the reset.
- Canceling the dialog makes no changes.
- Repo cascade: if a repo was unchecked (disabling its leaf packages),
  reset re-enables the repo and its packages.
