# Fleet UI Polish

**Date:** 2026-03-16
**Status:** Proposed
**Supersedes:** Editor file list section of `2026-03-16-editor-fleet-variants-design.md`

## Problem

Three visual/interaction issues in the fleet-mode report:

1. **Editor variant list** — fleet config variants are listed flat in a row with no grouping, no indication of which is selected, and no host provenance. Users can't quickly see which variant they're editing or where it came from.
2. **Table column widths** — columns containing short content (dropdowns, checkboxes, numeric values, labels) stretch to fill available space, pushing adjacent content columns off-screen. Most visible in Groups table (Strategy column consumes the entire row).
3. **Compare button** — silently fails when no variant is selected (no `include === true` in the group). The JS handler returns without feedback, leaving users confused.

## Part A: Editor Variant Accordion

### Design

Custom accordion in the editor sidebar using PF6 design tokens (colors, spacing, border-radius, font sizes) but not a PF6 component. The PF6 tree-view is designed for deep hierarchies; our structure is always exactly 2 levels (file → variants) with rich inline controls that don't fit tree-view nodes.

### Multi-variant files

Render as collapsible accordion groups:

**Header row:**
- Collapse/expand chevron (▼/►)
- File path with dimmed directory prefix, bold filename
- "N variants" badge (PF6 badge, `pf-m-read`)

**Variant rows** (indented children):
- Mini prevalence bar — blue (`#0066cc`) for majority, gold (`#cc8800`) for minority. Same color language as the config tab fleet bars.
- Host chips — individual hostname labels when they fit the available row width. Falls back to "N hosts" text when chips would overflow.
- **Selected variant** (`include === true`):
  - Highlighted row background with blue left border (3px solid `#0066cc`)
  - Green "selected" pill, right-aligned, same rounded shape as PF6 compact labels
- **Non-selected variants:**
  - Default row background (no highlight)
  - "use this variant" pill appears on hover — same shape/position as "selected" pill, blue (`#0066cc`), fades in (CSS transition ~150ms)
  - `stopPropagation` on pill click to prevent triggering row click

### Interaction model

| Action | Behavior |
|--------|----------|
| Click row (any variant) | Load variant content in editor pane. Selected variant opens editable; non-selected opens read-only. |
| Click "use this variant" pill | Switch selection: set `include=true` on this variant, `false` on siblings. Update pills. Cross-tab sync with config tab radio buttons. |
| Click accordion header | Toggle expand/collapse of variant children |

### Single-variant files

Render as flat entries — no chevron, no badge, no accordion. Identical to current behavior.

### Default state

All multi-variant groups start expanded.

### Host chip overflow

JS measures available row width minus prevalence bar and pill. If total chip width exceeds remaining space, replace chips with "N hosts" text. Evaluate on initial render (no dynamic resize needed — sidebar width is fixed).

### No regression on editing

The editor pane, CodeMirror instance, save/revert, dirty tracking, and the `doSelectFile()` / `selectFile()` logic are unchanged. Only the file list rendering (`buildTree()`) and variant navigation change.

## Part B: Table Column Width Pass

### Scope

Every `<table class="pf-v6-c-table">` across all report template partials.

### Change

Add `pf-m-fit-content` to `<th>` elements for columns containing fixed-width or short content. Leave name/path/description/content columns at default width so they stretch to fill available space.

### Column inventory

| Template | Columns getting `pf-m-fit-content` |
|----------|-------------------------------------|
| `_users_groups.html.j2` (Users) | checkbox, UID/GID, Type, Strategy |
| `_users_groups.html.j2` (Groups) | checkbox, GID, Strategy |
| `_config.html.j2` | checkbox, Kind, Category, rpm-Va flags |
| `_packages.html.j2` (dep tree) | checkbox, Version, Dependencies, Arch |
| `_packages.html.j2` (repos) | checkbox, Source |
| `_non_rpm.html.j2` (ELF) | checkbox, Language, Linking |
| `_non_rpm.html.j2` (pip/npm) | checkbox, Version |
| `_network.html.j2` (iptables) | checkbox, IPv, Table, Chain |
| `_network.html.j2` (firewall) | checkbox |
| `_containers.html.j2` | checkbox |
| `_selinux.html.j2` | Current, Default |
| `_storage.html.j2` | Type |

### Columns left at default width

Name, Path, Description, Details, Members, Diff content, Shared Libraries, Dependencies list, Rich Rules, Services, Ports, Args, Notes — anything that benefits from stretching.

## Part C: Compare Button Guard

### Problem

Compare buttons on fleet variant rows silently fail when no variant in the group has `include === true`. The JS handler in `_js.html.j2` finds no `selectedItem` and returns without any user feedback.

### Fix

Disable compare buttons when no variant is selected, both at render time and dynamically.

**Template (Jinja2):** Render compare buttons with `disabled` attribute and `pf-m-disabled` class when no sibling variant in the group has `include === true`.

**JS (variant switch):** When a variant is selected via radio button, "use this variant" pill, or editor pill:
- Enable all compare buttons in that variant group (remove `disabled` + `pf-m-disabled`)
- When a variant is deselected (e.g., reset to original), re-disable compare buttons if no variant remains selected

**JS (compare handler):** Keep the existing `if (!selectedItem) return;` as a defensive safety net, but the button should never be clickable in that state.

## Out of Scope

- "View & edit in editor →" link redesign (separate spec)
- Compare modal internals (diff rendering, switch behavior)
- Prevalence slider behavior
- Config tab layout
- Fleet popover content

## Testing

- **Part A:** Verify multi-variant files render as accordion groups. Verify host chips display or fall back to count. Verify row click opens correct variant (editable vs read-only). Verify pill click switches selection and syncs with config tab. Verify single-variant files render flat.
- **Part B:** Visual verification that Strategy, checkbox, and other short columns no longer stretch. Spot-check each template.
- **Part C:** Verify compare buttons are disabled when no variant selected. Verify they enable after selecting a variant. Verify reset re-disables them.
