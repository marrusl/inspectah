# Visual Consistency Pass

**Date:** 2026-03-22
**Status:** Implemented

## Problem

The report UI has grown organically across many feature specs. While each tab works correctly, the overall experience feels haphazard: table column spacing varies, some tabs have structural inconsistencies (missing fit-content, missing checkboxes), a few inline styles bypass the CSS system, and two tabs carry columns that don't earn their space. The result is a report that works but doesn't feel cohesive.

## Approach

A single sweep across all templates and CSS on a feature branch (`visual-consistency-pass`). Six focused changes that collectively make the report feel like one product instead of a patchwork.

## Part A: Global Table Spacing

> **Status:** Implemented in b428ac2.

Override PF6 table cell padding via CSS variables to achieve relaxed density across all tables. One rule in `_css.html.j2`:

```css
.pf-v6-c-table {
  --pf-v6-c-table--cell--PaddingBlockStart: var(--pf-t--global--spacer--sm);
  --pf-v6-c-table--cell--PaddingBlockEnd: var(--pf-t--global--spacer--sm);
  --pf-v6-c-table--cell--PaddingInlineStart: var(--pf-t--global--spacer--md);
  --pf-v6-c-table--cell--PaddingInlineEnd: var(--pf-t--global--spacer--md);
}
```

PF6 uses CSS logical properties (block/inline) rather than physical (top/left). These are the actual variable names from `patternfly.css`. This override affects headers and body cells uniformly. No per-table changes needed.

## Part B: Config Files Tab Cleanup

> **Status:** Implemented in 6f6c78a (B1–B5 all addressed: rpm -Va and diff columns removed, permissions badge added, pencil moved left, variant row colspan/filler cells updated).

### B1: Drop `rpm -Va` Column

Remove the `rpm -Va` flags column from `_config.html.j2`. The raw verification output (`S.5.....`) is an implementation detail — how yoinkc detected the change, not what the user needs to act on. The information it provides is either redundant (file content changed → it's in the list) or better surfaced differently (see B3).

### B2: Drop Diff Preview Column

Remove the "Diff" column from `_config.html.j2`. This column currently renders a full colored unified diff preview (via `_render_diff_html()` in `html_report.py` — a `<pre class="diff-view">` with color-coded add/delete/hunk spans, truncated at 80 lines). This is a real UX tradeoff: users lose the ability to quick-scan diffs inline in the config table.

**Rationale:** The editor tab is now the canonical surface for viewing, arbitrating, and editing file differences — it shows the complete diff with full context and allows editing. The inline preview duplicates this in a less useful form (truncated, read-only, takes significant horizontal space). Removing it tightens the config table and reinforces the editor as the single diff surface.

**Backend cleanup:** The `_render_diff_html()` function in `html_report.py` and the `diff_html` field it populates can be removed, along with the associated CSS classes (`.diff-view`, `.diff-hdr`, `.diff-hunk`, `.diff-add`, `.diff-del`) in `_css.html.j2`.

### B3: Add "Permissions Changed" Badge

When rpm -Va flags include `M` (mode), `U` (user), or `G` (group), render a PF6 compact label on the row — e.g., a small badge reading `permissions` in the kind/category area. This surfaces the one actionable signal from rpm -Va without a dedicated column.

**Implementation note:** The template receives this field as `flags` (set by `html_report.py`). The permissions check can be done directly in Jinja2 (e.g., `{% if 'M' in f.flags or 'U' in f.flags or 'G' in f.flags %}`) — no renderer or model changes needed. The badge must appear on both normal config rows and fleet variant child rows (anywhere kind/category badges are rendered).

### B4: Move Pencil Icon Left

Move the editor pencil column from rightmost position to between the checkbox and path columns. This groups the two interactive elements (toggle + edit) on the left side of the row, creating a clear flow: *interact* → *identify* → *classify* → *fleet context*.

**Applies to all editor-enabled tabs.** Exact column orders after reorder:

**`_config.html.j2`:** Checkbox | Pencil | Path | Kind | Category | Fleet

**`_services.html.j2` (Drop-in Overrides table only — State Changes table has no pencil):** Checkbox | Pencil | Parent unit | Drop-in path | Content | Fleet

**`_containers.html.j2` (Quadlets table only — Compose Services and Running Containers tables have no pencil):** Checkbox | Pencil | Unit | Image | Path | Content | Fleet

### Resulting Config Columns

| Position | Column | Notes |
|----------|--------|-------|
| 1 | Checkbox | `pf-m-fit-content` |
| 2 | Pencil | `pf-m-fit-content`, refine-mode only |
| 3 | Path | Flex (primary content) |
| 4 | Kind | `pf-m-fit-content`, badge. "Permissions changed" badge appended here when applicable |
| 5 | Category | `pf-m-fit-content`, badge |
| 6 | Fleet | Flex, fleet-mode only |

Down from 8 columns to 6 (or 4 without refine + fleet).

### B5: Update Variant Row Structure

The config table currently has a hardcoded `colspan="10"` on fleet variant group rows and 4 empty filler `<td></td>` cells to pad parent rows to the old column count. These must be updated to match the new column count:

- **colspan:** Recalculate based on new column count (base 4 + conditional fleet + conditional pencil)
- **Filler cells:** Reduce from 4 to 2 (pad for Kind and Category; rpm -Va and Diff fillers removed)
- **Pencil column in variant rows:** Move from last position to position 2, matching the header

The **inner/nested variant tables** (compact `pf-m-compact` tables inside fleet variant group rows) must also be updated: remove the rpm -Va and diff columns from nested variant rows, and move the pencil column to position 2. These inner tables have their own `<th>`/`<td>` structure that must match the new outer-table column order.

The same pencil-column reorder applies to `_services.html.j2` and `_containers.html.j2` — check each for filler cells, colspan values, and nested variant tables that reference the old column order.

## Part C: Packages Tab Restructure

> **Status:** Implemented in f530c15.

Collapse the separate "Repositories" card into the dependency tree. The repo name becomes the expandable section header within the dependency tree card, with the include/exclude toggle on that header row.

### Before

```
[Card: Repositories]
  ☑ baseos  ☑ appstream  ☐ epel

[Card: Dependency Tree]
  baseos
    ├── kernel-core
    ...
```

### After

```
[Card: Package Dependency Tree]
  ▼ baseos (23 packages)              ☑
    ├── kernel-core  5.14.0-427
    ├── systemd      252-32
    ...
  ▼ appstream (41 packages)           ☑
    ├── httpd  2.4.57-8
    ...
  ▶ epel (3 packages)                 ☐
```

The toggle on the repo header cascades: it includes/excludes both the repo file entry (`data-snap-section="rpm" data-snap-list="repo_files"`) and all dep-tree leaf rows (`data-leaf`) — both the grouped `repo_groups` table and the `#pkg-dep-tree` fallback table. This matches the current `applyRepoCascade()` behavior in `_js.html.j2`, which walks rows with `data-leaf`. It does **not** touch unclassified packages or the simple non-leaf `packages_added[:100]` list, and this spec does not expand that scope.

The implementation work is UI restructuring (merging the two cards, promoting repo names to expandable group headers) and migrating the `#pkg-repo-table .repo-cb` selectors to match the new DOM structure — not inventing new cascade logic.

Individual packages still have their own toggles for fine-grained control (e.g., include the epel repo but exclude a specific package). Toggling an individual package does not affect the repo-level toggle state.

### Expand/Collapse Behavior

Repo group headers currently use `<th colspan>` rows (non-interactive). In the new design, repo group headers become expandable rows using a custom row-level toggle (chevron icon that shows/hides the package rows within that group via JS class toggle). **Default state: expanded.** Collapsing a repo hides its package rows but does not change their include/exclude state.

### Default Distribution Repos

Repos with `is_default_repo` (baseos, appstream) currently have disabled toggles in the Repositories card (`disabled title="Default distribution repository — cannot be excluded"`). In the new merged view, the repo group header for default repos must also render a disabled toggle with the same title. The cascade logic should skip these repos (they cannot be toggled off).

### Unmatchable Repos

If a repo file's name does not match any key in `repo_groups`, the cascade will find no matching `packages_added` rows. This is acceptable — the toggle still controls the repo file entry itself, and the no-op on packages is correct (there are no packages to cascade to).

### Mixed-State Behavior

When the user excludes individual packages within a repo, the repo-level toggle remains checked (no indeterminate/tri-state). The repo toggle is a bulk action:

- **Repo toggled OFF:** Forces all dep-tree leaf rows in that repo to excluded, overriding any individual selections.
- **Repo toggled ON:** Forces all dep-tree leaf rows in that repo to included, overriding any individual exclusions.
- **Individual package toggled:** Only affects that package. Repo toggle stays in its current state.

This "bulk override" model matches the reset button's behavior — it's a coarse control that overrides fine-grained edits. Users who need per-package control use the package-level toggles and leave the repo toggle alone.

**JS implementation:** Migrate `applyRepoCascade()` to work with the new DOM structure (repo headers are now inside the dep tree card, not `#pkg-repo-table`). The existing cascade logic walks `data-leaf` rows within the same repo group. **Required new work:** add a `recalcTriageCounts()` call to the repo-toggle handler after cascade completes — the generic checkbox handler recalculates counts for the toggled row, but repo cascade changes additional rows after that flow, leaving sidebar counts stale unless explicitly recalculated.

## Part D: Column Consistency Across Tabs

> **Status:** Implemented in b428ac2 (fit-content on scheduled jobs Schedule column, network Method/Type/Deployment columns).

### D1: Scheduled Jobs — Timers Table

Current columns: `Timer` | `Schedule` | `ExecStart` (no checkboxes, no fit-content).

Add `pf-m-fit-content` to `Schedule` (short cron-like expressions). Do **not** add checkboxes — `SystemdTimer` in `schema.py` has no `include` field, the triage JS in `_js.html.j2` does not wire timers, and the Containerfile renderers treat timers as always-present informational items. Adding a checkbox that doesn't affect output would be worse than no checkbox. If timers need to become triageable, that requires a separate spec covering schema, JS triage, and Containerfile renderer changes.

### D2: Scheduled Jobs — At Jobs Table

Current columns: `File` | `User` (fit-content) | `Command` (no checkboxes).

Do **not** add checkboxes — `AtJob` in `schema.py` has no `include` field, and the triage/render pipeline does not support at-job toggling. Same rationale as D1: non-functional switches are worse than none. If at jobs need to become triageable, that's a separate spec.

### D3: Network — Connections Table

Current columns: `Name` | `Method` | `Type` | `Deployment` (no checkboxes, no fit-content).

Add `pf-m-fit-content` to `Method`, `Type`, and `Deployment` (all short-value columns). Do **not** add checkboxes — network connections are a point-in-time snapshot, not triageable items. The user can't meaningfully include/exclude an active connection from a migration.

### What's Already Consistent

SELinux, Kernel & Boot, Users & Groups, and Non-RPM Software tables already have appropriate fit-content and checkbox usage. They benefit from Part A's global spacing but need no structural changes.

## Part E: Inline Style Cleanup

> **Status:** Implemented in b428ac2 (fleet-variant-toggle, fleet-variant-table, and variant-index classes added to _css.html.j2).

Move three remaining `style="..."` attributes to CSS classes in `_css.html.j2`:

| Current inline style | New CSS class | Location |
|---------------------|---------------|----------|
| `style="margin-left: 8px; cursor: pointer;"` | `.fleet-variant-toggle` | Fleet variant label in multiple tabs |
| `style="margin: 0;"` | `.fleet-variant-table` or scoped under `.fleet-prevalence` | Compact nested variant tables |
| `style="color: var(--pf-v6-global--Color--200);"` | `.variant-index` | Config variant index text "(variant 1)" |

No visual change — just moves styling to the CSS block for maintainability.

## Feature Branch

All work on a `visual-consistency-pass` branch off `main`. Review the full report visually across multiple tabs before merging.

## Out of Scope

- **Fleet awareness for new tabs** (SELinux, Kernel & Boot, Users & Groups, Non-RPM, Secrets) — requires Python backend merge logic per inspector, separate spec
- **Non-RPM content reform** — content/usefulness question, separate spec
- **Visual improvements batch** (animations, keyboard nav, triage progress indicator) — already specced at `docs/specs/implemented/2026-03-17-visual-improvements-design.md`
- **Editor tab changes** — off limits
- **Cross-stream targeting** — separate spec
- **Static DOM fleet popovers** — separate spec

## Testing

### Part A (Spacing)
- Verify relaxed padding applies to all PF6 tables across all tabs
- Verify no table reverts to cramped default spacing
- Verify nested tables (fleet variant tables) inherit spacing appropriately

### Part B (Config Cleanup)
- Verify `rpm -Va` column is gone from config tab
- Verify diff preview column is gone from config tab
- Verify `_render_diff_html()` function and `diff_html` field are removed from `html_report.py`
- Verify `.diff-view`, `.diff-hdr`, `.diff-hunk`, `.diff-add`, `.diff-del` CSS classes are removed
- Verify "permissions changed" badge appears when rpm -Va flags contain M, U, or G
- Verify badge does not appear when flags contain only S, 5, T (content-only changes)
- Verify pencil icon is between checkbox and path in config, services, and containers tabs
- Verify pencil column does not appear in read-only mode
- Verify colspan on variant group rows matches new column count
- Verify filler `<td>` cells on parent rows match new column count
- Verify column alignment is consistent across single files, variant parent rows, and variant child rows

### Part C (Packages)
- Verify separate Repositories card is removed
- Verify repo names appear as expandable section headers in the dependency tree
- Verify repo groups start expanded by default
- Verify collapsing a repo group hides its package rows without changing include/exclude state
- Verify clicking a repo toggle includes/excludes the repo file entry AND dep-tree leaf rows (grouped table and `#pkg-dep-tree` fallback), but not unclassified packages or the simple non-leaf list
- Verify toggling an individual package does not affect the repo-level toggle
- Verify default distribution repos (baseos, appstream) have disabled toggles with "Default distribution repository" title
- Verify cascade no-ops gracefully when a repo file has no matching packages
- Verify `recalcTriageCounts()` fires after repo toggle cascade

### Part D (Column Consistency)
- Verify scheduled timers table has `pf-m-fit-content` on `Schedule` column (no checkbox added)
- Verify at jobs table is unchanged (no checkbox added)
- Verify network connections table has `pf-m-fit-content` on `Method`, `Type`, and `Deployment` columns (no checkbox added)
- Verify no other tables have changed structurally

### Part E (Inline Styles)
- Verify fleet variant toggle labels render identically after class migration
- Verify compact variant tables render identically after class migration
- Verify variant index text color is unchanged after class migration
- Verify the three targeted inline styles are replaced with their CSS class equivalents
