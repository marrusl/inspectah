# Prevalence Slider Fix Design

**Date:** 2026-03-16
**Status:** Proposed

## Problem

The prevalence slider has three issues:

1. **Slider resets after re-render** — user slides to 80%, clicks Apply,
   includes recalculate correctly, but after re-render the slider resets
   to the original aggregation default. Root cause: the re-render sends
   the modified snapshot to the server, but `fleet_meta.min_prevalence`
   is not updated in the snapshot, so the new HTML renders with the
   original value.
2. **No Cancel button** — after sliding, only Apply is available. No way
   to abandon the change and reset the slider to its previous value.
3. **No pending changes on Apply** — applying the slider recalculates
   includes client-side but does not register as a pending change in the
   toolbar, making it unclear that a re-render is needed.

## Scope

All changes in client-side JS and one line of toolbar HTML. No Python
or server-side changes.

**Files:**
- `src/yoinkc/templates/report/_toolbar.html.j2` — add Cancel button
- `src/yoinkc/templates/report/_js.html.j2` — slider handlers, re-render
  handler

## Fix 1: Slider survives re-render

**File:** `src/yoinkc/templates/report/_js.html.j2` — re-render click
handler (lines ~503-521)

Before the `fetch()` call that sends the snapshot to `/api/re-render`,
update the slider value in the snapshot:

```js
if (snapshot.meta && snapshot.meta.fleet) {
  snapshot.meta.fleet.min_prevalence = parseInt(slider.value, 10);
}
```

This persists the user's chosen threshold into the snapshot. When yoinkc
re-renders, the new HTML will have
`value="{{ fleet_meta.min_prevalence }}"` set to the correct value.
The slider renders at the user's chosen position after page reload.

This changes the original spec's "client-side only" stance — the slider
value is now persisted in the snapshot. This is the right call: the
snapshot should reflect the user's refinement choices.

## Fix 2: Cancel button

**File:** `src/yoinkc/templates/report/_toolbar.html.j2`

Add a Cancel button next to the existing Apply button, also hidden by
default:

```html
<button id="btn-cancel-prevalence" class="pf-v6-c-button pf-m-link fleet-slider-cancel"
        style="display: none;">Cancel</button>
```

**File:** `src/yoinkc/templates/report/_js.html.j2` — slider handlers

Behavior:
- On slider `input`: if value differs from `data-current-threshold`,
  show both Apply and Cancel. If same, hide both.
- **Apply** (existing): commits the threshold, updates
  `data-current-threshold`, hides both Apply and Cancel.
- **Cancel** (new): resets slider value to `data-current-threshold`,
  updates the prevalence display text, clears the preview span
  (`prevalence-preview`), hides both Apply and Cancel.

Both buttons disappear after either action.

## Fix 3: Apply registers as pending change

**File:** `src/yoinkc/templates/report/_js.html.j2` — Apply click
handler (lines ~1127-1180)

The Apply handler's Phase 3 already calls `setDirty(!isSnapshotClean())`
which compares current checkbox states against `includeBaseline`.
Since Apply modifies checkboxes, `isSnapshotClean()` should return
false, and `setDirty(true)` should fire.

However, `setDirty()` toggles `rerenderBtn` which is `btn-rerender`
(the non-refine placeholder). In refine mode, that button is now
hidden (per the duplicate re-render button fix). The refine-mode
button (`btn-re-render`) is toggled by `updateChangedCount()` in the
editor JS, which tracks `savedFiles` not include changes.

The fix: `setDirty()` should also toggle `btn-re-render` if it exists.
Add to `setDirty()`:

```js
var editorReRenderBtn = document.getElementById('btn-re-render');
if (editorReRenderBtn) editorReRenderBtn.disabled = !isDirty;
```

This ensures Apply (and any other include/strategy change) enables the
correct re-render button in refine mode.

After Apply, the toolbar shows pending changes, signaling that a
re-render will reflect the new prevalence threshold in the
Containerfile and report.

## Testing

- **Fix 1:** Slide to a non-default value → Apply → Re-render → verify
  slider retains the applied value after page reload
- **Fix 2:** Slide → Apply and Cancel appear. Click Cancel → slider
  resets, both buttons disappear. Slide again → Apply and Cancel appear.
  Click Apply → both buttons disappear.
- **Fix 3:** Apply a new threshold → toolbar shows pending changes.
  Verify the changed count reflects the include/exclude modifications.
- Existing Python tests unaffected.
