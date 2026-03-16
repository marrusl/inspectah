# PF6 Modal Migration Design

**Date:** 2026-03-16
**Status:** Proposed

## Problem

All four modals in the yoinkc HTML report use custom inline styles for
positioning, backdrop, centering, border-radius, shadows, and spacing.
PF6 ships a fully-featured `pf-v6-c-modal-box` component that handles
all of this. Migrating removes ~30 inline style declarations, improves
visual consistency with the rest of the PF6-based report, and makes
future modal work (if any) trivial.

## Scope

Four modals, all migrated to PF6 `pf-v6-c-modal-box`:

1. **Unsaved Changes** — `_editor.html.j2` lines 41-57
2. **Delete File** — `_editor.html.j2` lines 59-74
3. **New File** — `_new_file_modal.html.j2`
4. **Compare Variants** — dynamically created in `_js.html.j2` lines ~710-780

### Out of scope

- Fleet popovers (dynamic DOM, separate spec)
- Any new modals
- Changes to modal logic, callbacks, or event wiring

## Approach

Direct class swap (Approach A). Replace inline styles with PF6 classes
on the existing HTML structure. No shared macro, no web component, no
new abstractions. Four modals is not enough to justify DRY — the
repeated boilerplate is ~8 lines of predictable PF6 markup per modal.

## Target Structure

Each modal migrates to this PF6 skeleton:

```html
<div class="pf-v6-c-backdrop" style="display:none;" id="xxx-modal">
  <div class="pf-v6-c-modal-box pf-m-sm" role="dialog" aria-modal="true" aria-label="...">
    <div class="pf-v6-c-modal-box__close">
      <button class="pf-v6-c-button pf-m-plain" aria-label="Close">&times;</button>
    </div>
    <div class="pf-v6-c-modal-box__header">
      <h2 class="pf-v6-c-modal-box__title">Title</h2>
    </div>
    <div class="pf-v6-c-modal-box__body">
      <!-- content -->
    </div>
    <div class="pf-v6-c-modal-box__footer">
      <!-- buttons with existing pf-v6-c-button classes -->
    </div>
  </div>
</div>
```

Key points:

- `pf-v6-c-backdrop` replaces the custom `rgba(0,0,0,0.5)` overlay div
- `pf-v6-c-modal-box` replaces all centering, shadow, border-radius
  inline styles
- Size modifiers: `pf-m-sm` for unsaved/delete, `pf-m-md` for
  new-file/compare
- `pf-m-danger` modifier on the delete modal for red title accent
- `pf-v6-c-modal-box__title` replaces `pf-v6-c-title pf-m-xl`
- Only inline style retained: `display:none` for initial hidden state

## Per-Modal Changes

### 1. Unsaved Changes (`_editor.html.j2`)

- Replace 3-div inline-style structure with PF6 skeleton
- Size: `pf-m-sm`
- No close (×) button — actions are Save / Discard / Cancel
- No JS changes — `showUnsavedModal()` already toggles `style.display`
  on the container id

### 2. Delete File (`_editor.html.j2`)

- Same structural swap as unsaved changes
- Size: `pf-m-sm`
- Modifier: `pf-m-danger` on `pf-v6-c-modal-box`
- No close (×) button — actions are Cancel / Delete
- No JS changes

### 3. New File (`_new_file_modal.html.j2`)

- Same structural swap
- Size: `pf-m-md` (wider form content)
- Close (×) button moves into `pf-v6-c-modal-box__close`
- Form body content (radio buttons, inputs, path preview) unchanged
  inside `__body`
- Backdrop click-to-close: `onclick="closeNewFileModal()"` moves to the
  `pf-v6-c-backdrop` div
- No JS logic changes

### 4. Compare Variants (`_js.html.j2` → new `_compare_modal.html.j2`)

- New Jinja2 partial: `_compare_modal.html.j2` with PF6 skeleton,
  `pf-m-md`, empty body/footer
- Include the partial in the report skeleton
- JS `showCompareModal()`: gut DOM-creation code, replace with
  `getElementById` + `innerHTML` for body/footer population +
  `style.display = 'flex'`
- JS `closeCompareModal()`: replace `el.remove()` with
  `style.display = 'none'` + clear innerHTML
- Backdrop click-to-close on the `pf-v6-c-backdrop` div
- Diff styling classes (`diff-line-add/remove/same`) unchanged

## CSS

Minimal additions. PF6's `pf-v6-c-backdrop` and `pf-v6-c-modal-box`
handle positioning, centering, overlay color, border-radius, and shadows
out of the box.

- No new CSS classes needed
- Visibility toggled via `style.display` (same as today), not a CSS
  class toggle
- All removed inline styles: `position:fixed`, `position:absolute`,
  `transform:translate(-50%,-50%)`, `z-index:9999`,
  `background:rgba(0,0,0,0.5)`, `border-radius:8px`,
  `box-shadow:0 4px 24px rgba(0,0,0,0.3)`, `padding:*`,
  `border-top/bottom:*`, `gap:8px`, `justify-content:flex-end`

## JS Changes

- `showUnsavedModal()` / `editorDeleteFile()` / `showNewFileModal()` /
  `closeNewFileModal()`: no logic changes, same `style.display` toggle
- `showCompareModal()`: replace DOM creation with populate-and-show
- `closeCompareModal()`: replace `el.remove()` with hide-and-clear

## What Does Not Change

- Button classes (`pf-v6-c-button pf-m-primary/secondary/danger/link/plain`)
- Event handler wiring (clone-node cleanup pattern, callbacks)
- Jinja2 conditionals (`{% if refine_mode %}`)
- Diff styling classes
- Modal IDs (DOM ids stay the same for JS compatibility)

## Testing

- Visual verification: open report, trigger each modal, confirm
  appearance matches PF6 styling
- Functional verification: each modal's actions (save/discard/cancel,
  delete/cancel, create file, compare/switch variant) still work
- Existing Python tests unaffected (they test snapshot/rendering logic,
  not modal HTML structure)
