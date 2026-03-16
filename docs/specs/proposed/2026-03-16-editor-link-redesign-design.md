# Editor Link Redesign

**Date:** 2026-03-16
**Status:** Proposed

## Problem

"View & edit in editor →" is a 6-word inline text link that repeats on every config file, drop-in, and quadlet row in refine mode. It's verbose, visually cluttered, and competes with the actual file path content in the table cells.

## Solution

Replace all "View & edit in editor →" text links with a pencil icon (✏️) in a dedicated rightmost table column.

## Icon Behavior

- **Default state:** greyscale via CSS `filter: grayscale(1)` — visually grey/muted, reduces noise
- **Hover state:** full color via `filter: grayscale(0)` — natural pencil emoji appearance draws attention
- **Transition:** smooth CSS transition (~150ms) between states
- **Cursor:** pointer on hover
- **Click:** calls existing `navigateToEditor(this)` — same JS function, no behavior change
- **Element:** `<button class="pf-v6-c-button pf-m-plain">` — keyboard focusable, semantic click target. Carries the same `data-section`, `data-list`, `data-index`, `data-path` attributes as the current `<a>` links
- **Tooltip:** HTML `title` attribute (not PF6 tooltip component — simplest approach, sufficient for an icon hint)

## Scope

Three templates currently render the text link:

| Template | Row types with icon |
|----------|-------------------|
| `_config.html.j2` | Single file rows, variant child rows |
| `_services.html.j2` | Drop-in override rows (single + variant children) |
| `_containers.html.j2` | Quadlet unit rows (single + variant children) |

Each template gets:
- A new `<th class="pf-m-fit-content" scope="col"></th>` as the rightmost column header (empty label)
- A corresponding `<td>` on each data row containing the pencil icon

## Refine Mode Guard

Icons only render when `refine_mode` is true — same conditional as the current text links. In read-only report mode, the icon column does not appear. The `<th>` should also be inside the refine_mode guard to avoid an empty column in read-only mode.

## Fleet Variant Handling

- **Single files (no variants):** icon with tooltip "Edit in editor"
- **Parent/group header row:** no icon — this row is a grouping header, not an editable file
- **Selected variant** (`include === true`): icon with tooltip "Edit in editor"
- **Non-selected variant** (`include === false`): icon with tooltip "View in editor (read-only)"

All icons use the same greyscale→color hover treatment regardless of selection state. The tooltip text differentiates the action.

## Variant Column Alignment Fix

The config table has a known issue where variant child rows have different `<td>` counts than non-variant rows, causing column misalignment. Adding the icon column to all row types (including variant rows with appropriate empty `<td>` cells) addresses part of this. The implementer should verify that all row types in each template have consistent column counts, adding empty `<td>` elements where needed.

## CSS

```css
.editor-icon {
  font-size: 14px;
  cursor: pointer;
  filter: grayscale(1);
  transition: filter 0.15s;
}
.editor-icon:hover {
  filter: grayscale(0);
}
```

This can go in `_css.html.j2` or inline in each template's existing style block.

## What Does Not Change

- `navigateToEditor()` JS function — unchanged
- Editor tab behavior — unchanged
- `data-section`, `data-list`, `data-index`, `data-path` attribute pattern — unchanged
- The `edit-in-editor-link` CSS class can be removed (no longer needed for text link styling)

## Out of Scope

- Editor tab UI (covered by separate fleet UI polish spec)
- Compare modal behavior
- Non-refine-mode report layout

## Testing

- Verify pencil icon appears on every editable row in refine mode across config, services, and containers tabs
- Verify icon is greyscale by default and full-color on hover
- Verify click navigates to editor tab and selects the correct file
- Verify tooltip shows "Edit in editor" for selected/single variants and "View in editor (read-only)" for non-selected variants
- Verify icon column does not appear in read-only (non-refine) mode
- Verify column alignment is consistent across single files, variant parent rows, and variant child rows
- Verify no "View & edit in editor →" text links remain in the codebase
