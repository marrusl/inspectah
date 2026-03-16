# Fleet UI Polish

**Date:** 2026-03-16
**Status:** Proposed
**Supersedes:** Editor file list section of `2026-03-16-editor-fleet-variants-design.md`
**Implementation note:** Parts A, B, and C are independently implementable. B and C are small, mechanical changes that can ship without waiting for A.

## Problem

Three visual/interaction issues in the fleet-mode report:

1. **Editor variant list** — fleet config variants are listed flat in a row with no grouping, no indication of which is selected, and no host provenance. Users can't quickly see which variant they're editing or where it came from.
2. **Table column widths** — columns containing short content (dropdowns, include toggles, numeric values, labels) stretch to fill available space, pushing adjacent content columns off-screen. Most visible in Groups table (Strategy column consumes the entire row).
3. **Compare button** — silently fails when no variant is selected (no `include === true` in the group). The JS handler returns without feedback, leaving users confused.

## Part A: Editor Variant Accordion

### Why not the PF6 tree-view

The editor file list was recently built using PF6 `pf-v6-c-tree-view` (see `2026-03-16-editor-fleet-variants-design.md` in `implemented/`). In practice, the tree-view renders variants as flat sibling nodes without visual grouping — there's no indication which variant is selected, which hosts each came from, or how to switch between them. The tree-view component is designed for deep navigational hierarchies (file browsers, org charts), not for a shallow 2-level structure with rich inline controls (prevalence bars, host chips, action pills). Cramming these into tree-view nodes fights the component's layout model. A custom accordion using PF6 design tokens gives us the right visual density and interaction surface without fighting the framework.

### Design

Custom accordion in the editor sidebar using PF6 design tokens (colors, spacing, border-radius, font sizes) but not a PF6 component.

### Multi-variant files

Render as collapsible accordion groups:

**Header row:**
- Collapse/expand chevron (▼/►)
- File path with dimmed directory prefix, bold filename
- "N variants" badge (PF6 badge, `pf-m-read`)

**Variant rows** (indented children):
- Mini prevalence bar — blue (`#0066cc`) for majority, gold (`#cc8800`) for minority. Same color language as the config tab fleet bars.
- Host chips — individual hostname labels when they fit the available row width. Falls back to "N hosts" text when chips would overflow. Individual chips get `max-width: 80px` with `text-overflow: ellipsis` as a defensive measure against very long hostnames.
- **Selected variant** (`include === true`):
  - Highlighted row background with blue left border (3px solid `#0066cc`)
  - Green "selected" **badge** (`pf-v6-c-badge pf-m-read` or compact label), right-aligned — this is a status indicator, not interactive
- **Non-selected variants:**
  - Default row background (no highlight)
  - "Use this variant" **button** (`pf-v6-c-button pf-m-small pf-m-secondary`) appears on hover, right-aligned in the same position as the selected badge. Fades in (CSS transition ~150ms).
  - "Compare" **button** (`pf-v6-c-button pf-m-small pf-m-link`) — always visible on non-selected rows. Clicking navigates to the config tab and scrolls to the variant group, opening the compare modal for this variant against the selected one. Disabled when no variant is selected (same guard as Part C).
  - `stopPropagation` on both buttons to prevent triggering row click
  - **Design rationale:** badge vs button distinguishes status (selected) from actions (switch, compare). The button shape makes the action affordance immediately clear without needing to discover hover behavior.

### Interaction model

| Action | Behavior |
|--------|----------|
| Click row (any variant) | Load variant content in editor pane. Selected variant opens editable; non-selected opens read-only. |
| Click "Use this variant" button | Switch selection: set `include=true` on this variant, `false` on siblings. Update badge/buttons. Cross-tab sync with config tab radio buttons. |
| Click "Compare" button | Navigate to config tab, scroll to the variant group, open compare modal for this variant vs the selected variant. |
| Click accordion header | Toggle expand/collapse of variant children |

### Cross-tab sync

Inherits the lazy sync model from the superseded spec: both the editor and config tab read/write `include` directly on the shared `snapshot` object. The pill click mutates `snapshot[section][list][index].include` — when the user switches to the config tab, the radio buttons reflect the current state because they read from the same object. No explicit event dispatch or DOM manipulation across tabs is needed.

The pill click must also call the existing `setDirty()` function and update compare button state (see Part C interaction below).

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

**Note on include toggle column:** The toggle switches spec (2026-03-15) renames the checkbox column to a PF6 toggle switch. The `pf-m-fit-content` treatment applies regardless of whether the column contains a checkbox or toggle. The inventory below uses "include toggle" to be generic.

### Column inventory

| Template | Columns getting `pf-m-fit-content` |
|----------|-------------------------------------|
| `_users_groups.html.j2` (Users) | include toggle, UID/GID, Type, Strategy |
| `_users_groups.html.j2` (Groups) | include toggle, GID, Strategy |
| `_config.html.j2` | include toggle, Kind, Category, rpm-Va flags |
| `_packages.html.j2` (dep tree) | include toggle, Version, Dep count, Arch |
| `_packages.html.j2` (repos) | include toggle, Source |
| `_non_rpm.html.j2` (ELF) | include toggle, Language, Linking |
| `_non_rpm.html.j2` (pip/npm) | include toggle, Version |
| `_network.html.j2` (iptables) | include toggle, IPv, Table, Chain |
| `_network.html.j2` (firewall) | include toggle |
| `_containers.html.j2` | include toggle |
| `_services.html.j2` (state changes) | include toggle, Current, Default, Action |
| `_services.html.j2` (drop-ins) | include toggle |
| `_scheduled_jobs.html.j2` (cron-converted) | include toggle |
| `_scheduled_jobs.html.j2` (cron jobs) | include toggle, Source, Action |
| `_scheduled_jobs.html.j2` (at jobs) | User |
| `_kernel_boot.html.j2` (sysctl) | include toggle, Runtime, Default |
| `_selinux.html.j2` (booleans) | Current, Default |
| `_storage.html.j2` | Type |

**Templates reviewed with no fit-content candidates:**
- `_secrets.html.j2` — no tables (renders as card list)
- `_audit_report.html.j2` — read-only summary tables; columns are all variable-width content (Name, Version, Package, etc.). Direction column in version changes table could qualify but is a compact label already sized by its content.
- `_scheduled_jobs.html.j2` (systemd timers) — Timer, Schedule, ExecStart are all variable-width
- `_kernel_boot.html.j2` (module/dracut config) — Path, Content are variable-width

**Note on SELinux:** If the Fleet Merge Completeness spec adds include toggles to SELinux tables, those columns would also need `pf-m-fit-content`. The implementer should check the current state of `_selinux.html.j2` at implementation time.

### Columns left at default width

Name, Path, Description, Details, Members, Diff content, Shared Libraries, Dependencies list, Rich Rules, Services, Ports, Args, Notes, Content, Schedule, Command, Key, Cron Expression — anything that benefits from stretching.

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

### Part A / Part C interaction

Part A's "use this variant" pill is a new selection trigger. The pill click mutates `snapshot[section][list][index].include` on the shared snapshot object. The compare button enable/disable logic should key off the snapshot state (check if any variant in the group has `include === true`), not off a specific UI event. This means:
- The pill click in Part A calls a shared `updateCompareButtons(group)` function after mutating the snapshot
- The existing radio button change handler in the config tab calls the same function
- Reset calls the same function

This avoids coupling Part C's logic to Part A's specific DOM elements.

## Out of Scope

- "View & edit in editor →" link redesign (separate spec)
- Compare modal internals (diff rendering, switch behavior)
- Prevalence slider behavior
- Config tab layout
- Fleet popover content

## Testing

- **Part A:** Verify multi-variant files render as accordion groups. Verify host chips display or fall back to count. Verify long hostnames truncate with ellipsis. Verify row click opens correct variant (editable vs read-only). Verify pill click switches selection and syncs with config tab. Verify single-variant files render flat.
- **Part B:** Visual verification that Strategy, include toggle, and other short columns no longer stretch. Spot-check each template listed in the inventory.
- **Part C:** Verify compare buttons are disabled when no variant selected. Verify they enable after selecting a variant via radio button OR editor pill. Verify reset re-disables them.
