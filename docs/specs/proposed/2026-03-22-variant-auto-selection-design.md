# Variant Auto-Selection and Editor Drawer

**Date:** 2026-03-22
**Status:** Proposed

## Problem

In fleet mode, variant groups (config files, quadlets, drop-ins with differing content across hosts) start with no variant selected. This creates two issues:

1. **Compare is broken on first contact.** The editor tab is high in the sidebar nav, so users often land there before triaging variants in other tabs. Compare buttons are disabled when no variant is selected, and there's no explanation why. The user sees variants, sees Compare, clicks it, and nothing happens.

2. **Editor tree pane is too narrow.** The file list is fixed at 300px, which cramps variant rows that contain a prevalence bar, host count, Compare button, and "Use this variant" button. The cramping worsens with the new auto-selection badge.

## Design

### Part A: Auto-selection rule

On page load and after prevalence slider Apply, for each variant group:

- **Clear winner** (one variant has strictly higher `fleet.count` than all others): auto-select it (`include=true`).
- **Tie** (two or more variants share the highest `fleet.count`): select none. Display a gold "tied — compare & choose" badge on the variant group header.

The rule is documented in the fleet banner on the summary tab: *"The most prevalent variant of each config file is auto-selected. Tied variants require your choice."*

#### Tie detection

A tie exists when `variants[0].fleet.count === variants[1].fleet.count` after sorting descending by count. This covers 2-way ties (most common in small fleets) and N-way ties.

#### Where auto-selection happens

**Python renderer (initial page):** During fleet merge post-processing, after the threshold formula sets `include` based on prevalence, apply the tie-breaking rule per variant group (items sharing the same `path`). If the threshold included multiple variants and there's a strict winner by count, keep only the winner. If tied, set all to `include=False`. This requires grouping items by path in the merge module, which currently processes items individually.

**JS `applyPrevalenceThreshold()`:** After sorting variants by count, check for ties before selecting. Same rule: strict winner selects, tie selects none.

### Part B: Compare without a selection

**When a selection exists (status quo):** Compare diffs the clicked variant against the selected one. No change.

**When no selection exists — 2-variant groups:** Compare does a peer-to-peer diff. Clicking Compare on either variant diffs it against the other. No ambiguity.

**When no selection exists — 3+ variant groups:** Compare remains disabled. The "tied — compare & choose" badge guides the user to pick one via "Use this variant" first.

#### Compare modal changes (no-selection case)

In peer-to-peer mode, the clicked variant becomes `comparisonItem` (right/blue side of the diff) and the other variant becomes `selectedItem` (left/red side). This is cosmetic — neither is actually "selected" yet.

The modal footer shows "Use Variant A" / "Use Variant B" buttons (instead of the single "Switch to this variant" button). Clicking either sets `include=true` on that variant, `include=false` on the other, updates the DOM (radio-toggle behavior), and closes the modal.

#### Compare button enable/disable changes

`updateCompareButtons()` in `_js.html.j2` currently disables all Compare buttons when `hasSelected === false`. Update to also enable buttons when the group has exactly 2 variants, even with no selection. The click delegation handler's `if (!selectedItem) return;` guard must be relaxed for the 2-variant peer-to-peer case.

### Part C: PF6 resizable drawer for editor tree pane

Replace the fixed `width:300px` editor tree pane with a PF6 drawer component.

- Component: `pf-v6-c-drawer` with `pf-m-panel-left` and `pf-m-resizable`
- Default width: 340px
- Min/max: 240px–600px via PF6 CSS variables (`--pf-v6-c-drawer__panel--md--FlexBasis--min` / `--max`)
- The code editing pane becomes the drawer content (takes remaining space)
- PF6 provides the splitter markup and cursor styling; drag-to-resize requires custom JS (mousedown/mousemove/mouseup on the splitter handle)
- Persist width in `localStorage` (key: `inspectah-editor-drawer-width`) so it survives page navigation and re-renders

### Affected files

**Python:**
- Fleet merge or renderer post-processing: auto-selection logic with tie detection

**JS (`_js.html.j2`):**
- `applyPrevalenceThreshold()`: tie-aware auto-selection after sorting
- `updateCompareButtons()`: enable Compare for 2-variant groups even without a selection
- Click delegation for `.variant-compare-btn`: relax `if (!selectedItem) return;` guard for 2-variant peer-to-peer case
- `showCompareModal()`: "Use Variant A/B" footer buttons in no-selection case

**JS (`_editor_js.html.j2`):**
- `compareFromEditor()`: peer-to-peer compare when `selectedItem` is null and group has exactly 2 variants (relax `if (!selectedItem) return;` guard)
- Editor tree build: render gold "tied — compare & choose" badge on group headers when no variant is selected

**Templates (`_config.html.j2`, `_services.html.j2`, `_containers.html.j2`):**
- Compare button disabled state: render as enabled for 2-variant groups even when `group_has_selected` is false (or defer entirely to JS `updateCompareButtons` on page init)

**Template (`_editor.html.j2`):**
- Replace `div#editor-tree` + code pane layout with PF6 drawer markup
- Remove inline `width:300px` and `style` attributes on editor panes

**CSS (`_editor.html.j2` style block or `_css.html.j2`):**
- Gold badge style (reuse `#cc8800` palette from minority prevalence bars)
- Drawer width persistence via localStorage on resize end

**Template (`_summary.html.j2`):**
- Fleet banner: add auto-selection rule explanation

## Scope boundaries

**In scope:**
- Auto-select most prevalent variant (clear winner only)
- Tie detection and gold badge
- Peer-to-peer compare for 2-variant tied groups
- "Use Variant A/B" buttons in compare modal footer (no-selection case)
- Fleet banner text documenting the rule
- PF6 resizable drawer for editor tree pane
- localStorage persistence for drawer width

**Out of scope:**
- 3+ variant tied groups getting a multi-pick compare flow (disable Compare, show badge)
- Auto-selection for non-config variant types (packages have version variants — different problem, separate spec)
- Changes to main report tabs' variant rows (Compare already works when a selection exists)

## Testing

- Tie detection: 2-way tie, 3-way tie, clear winner, single variant (no tie possible)
- Auto-select on page load and after prevalence slider Apply
- Compare modal peer-to-peer mode (2-variant tied group)
- Compare disabled for 3+ variant tied group with no selection
- "Use Variant A/B" in compare modal sets selection and closes modal
- Drawer resize persists to localStorage
- Drawer respects min/max constraints
