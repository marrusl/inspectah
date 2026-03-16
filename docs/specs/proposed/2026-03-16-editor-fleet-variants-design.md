# Editor Fleet Variants Design

**Date:** 2026-03-16
**Status:** Proposed

## Problem

In fleet-aggregated reports, config files with different content across
hosts produce multiple snapshot entries for the same path (one per
content variant). The editor's file list shows these as duplicate
entries with identical paths — confusing and unusable:

```
├─ /etc/httpd/conf.d/ssl.conf
├─ /etc/httpd/conf.d/ssl.conf
├─ /etc/httpd/conf.d/ssl.conf
```

No way to tell which variant you're editing, which hosts it applies to,
or which one is the selected variant.

## Scope

All changes in `_editor_js.html.j2` (client-side JS). No Python, schema,
or rendering pipeline changes.

**Files:**
- Modify: `src/yoinkc/templates/report/_editor_js.html.j2` — rewrite
  `buildTree()`, update `doSelectFile()` for variant-aware behavior

### Out of scope

- Config tab variant logic (already works — radio buttons, compare modal)
- Compare modal (stays on config tab)
- Non-fleet reports (no variants, tree renders as today)

### Dependencies

- File list sort + path display fix (this spec replaces the custom tree
  markup with PF6 tree-view but keeps the same sort and display logic)
- `setDirty()` fix from prevalence slider spec (for re-render button)

## Design

### PF6 Tree View for Variant Grouping

Replace the current custom DOM in `buildTree()` with PF6's
`pf-v6-c-tree-view` component.

**Structure:**

```html
<ul class="pf-v6-c-tree-view">
  <!-- Category: config -->
  <li class="pf-v6-c-tree-view__list-item pf-m-expanded">
    <div class="pf-v6-c-tree-view__content">
      <button class="pf-v6-c-tree-view__node">config/</button>
    </div>
    <ul class="pf-v6-c-tree-view__list">

      <!-- Single-variant file: leaf node -->
      <li class="pf-v6-c-tree-view__list-item">
        <div class="pf-v6-c-tree-view__content">
          <button class="pf-v6-c-tree-view__node">
            <span style="color:var(--pf-t--global--text--color--subtle);">/etc/chrony/</span>
            <strong>chrony.conf</strong>
          </button>
        </div>
      </li>

      <!-- Multi-variant file: expandable parent -->
      <li class="pf-v6-c-tree-view__list-item pf-m-expanded">
        <div class="pf-v6-c-tree-view__content">
          <button class="pf-v6-c-tree-view__node">
            <span style="color:var(--pf-t--global--text--color--subtle);">/etc/httpd/conf.d/</span>
            <strong>ssl.conf</strong>
            <span class="pf-v6-c-badge pf-m-read">3</span>
          </button>
        </div>
        <ul class="pf-v6-c-tree-view__list">

          <!-- Variant 1 (selected, currently viewed) -->
          <li class="pf-v6-c-tree-view__list-item">
            <div class="pf-v6-c-tree-view__content">
              <button class="pf-v6-c-tree-view__node pf-m-current">
                variant 1
                <span class="pf-v6-c-label pf-m-compact pf-m-success"><span class="pf-v6-c-label__content">selected</span></span>
                <span class="pf-v6-c-label pf-m-compact"><span class="pf-v6-c-label__content">2/3 hosts</span></span>
              </button>
            </div>
          </li>

          <!-- Variant 2 (not selected) -->
          <li class="pf-v6-c-tree-view__list-item">
            <div class="pf-v6-c-tree-view__content">
              <button class="pf-v6-c-tree-view__node">
                variant 2
                <span class="pf-v6-c-label pf-m-compact"><span class="pf-v6-c-label__content">1/3 hosts</span></span>
              </button>
            </div>
          </li>

        </ul>
      </li>

    </ul>
  </li>
</ul>
```

**Key elements:**
- `pf-v6-c-tree-view` container with proper nesting, hover states,
  indentation, and focus handling
- `pf-v6-c-badge pf-m-read` for variant count on parent nodes
- `pf-v6-c-label pf-m-compact pf-m-success` for "selected" indicator
- `pf-v6-c-label pf-m-compact` for host count (N/M hosts)
- `pf-m-current` on the `pf-v6-c-tree-view__node` button (not the
  `<li>`) for the actively-viewed variant
- `pf-v6-c-label__content` inner span on all labels (PF6 convention)
- Non-selected variant text uses subtle color
  (`--pf-t--global--text--color--subtle`)

**Grouping logic:**
1. Within each category, collect items and group by path
2. Sort groups alphabetically by path
3. Single-item groups: render as leaf nodes (no variant nesting)
4. Multi-item groups: render as expandable parent with variant children
5. Variant children preserve original snapshot array indices for
   `selectFile()` calls

**Category order:** config → drop-ins → quadlet (from earlier fix)

**Path display:** dimmed directory prefix + bold filename on parent/leaf
nodes (from earlier fix). Variant children show "variant N" only.

### Variant Interaction Behavior

**Toolbar button decision:**
`doSelectFile()` checks `snapshot[section][list][index].include` to
determine which button to show. If `include === true` (or if no fleet
data), show Edit. If `include === false`, show "Switch to this variant."

**DOM selector updates:**
The current `doSelectFile()` highlights the active entry using a
selector on `.editor-tree-file[data-section=...][data-index=...]`.
`updateStateLabels()` queries `.editor-state-label` elements. Both
must be updated to work with the new PF6 tree-view DOM structure.
Use `data-section`, `data-list`, and `data-index` attributes on the
variant `<button>` elements to preserve the selector pattern.

**Variant numbering:**
Variants are sorted by `fleet.count` descending (most prevalent first)
and numbered sequentially. This matches the config tab's display order.

**New files:**
Files created via the new-file modal have no `fleet` field and render
as leaf nodes (no grouping). The grouping logic naturally handles this
since they produce single-item groups.

**All variants excluded:**
If all variants of a path have `include === false`, the parent node
stays visible with no "selected" label on any child. The file is
omitted from the Containerfile on re-render (existing behavior).

**Click a selected variant:**
- Opens in read-only view (same as today)
- Edit button appears — click to enter edit mode
- Full editing flow (save, revert) works unchanged

**Click a non-selected variant:**
- Opens in read-only view
- Edit button is replaced with "Switch to this variant" button
  (`pf-v6-c-button pf-m-primary pf-m-small`)
- Content is viewable but not editable

**"Switch to this variant" action:**
- Sets this variant's `include = true` in the snapshot
- Sets sibling variants' `include = false` (same path, different content)
- Updates the tree labels: new selection gets "selected" label, old
  selection loses it
- Calls `setDirty(!isSnapshotClean())` to register as pending change
- After switch, the variant is now selected and the Edit button appears

**Sync with config tab:**
- Both the config tab's variant radio buttons and the editor tree
  read/write the same `snapshot` data
- No explicit cross-tab event needed — switching on either side is
  reflected when the other tab is viewed, since both read `include`
  from the snapshot array

### Non-Fleet Reports

When the snapshot has no fleet metadata, the tree renders exactly as
today: flat list of files per category, no variant grouping, no badges.
The grouping logic produces only single-item groups, so it falls through
to the leaf-node rendering path.

## Testing

- **Variant grouping:** Open a fleet report with multi-variant config
  files. Verify parent nodes show path + badge, children show variant
  labels with selected/host-count indicators.
- **Single-variant files:** Verify they render as flat leaf nodes (no
  nesting, no badge).
- **View variant:** Click a variant child. Verify content loads in
  read-only view.
- **Edit selected variant:** Click selected variant → Edit → make
  changes → Save/Revert. Verify normal editing flow.
- **Non-selected is view-only:** Click non-selected variant. Verify
  "Switch to this variant" button appears instead of Edit.
- **Switch variant:** Click "Switch to this variant". Verify include
  flags update, tree labels swap, toolbar shows pending changes.
- **Cross-tab sync:** Switch variant in editor → go to config tab.
  Verify the radio button reflects the new selection.
- **Non-fleet report:** Open a single-host report. Verify tree renders
  as a flat list with no variant grouping.
- **Existing Python tests:** Unaffected (no rendering/snapshot changes).
