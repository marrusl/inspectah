# Fleet Variant Comparison

**Date:** 2026-03-16
**Status:** Proposed
**Part of:** Fleet Refine (Spec 3 of 3)
**Depends on:** Spec 2 (Fleet Refine Lifecycle)

## Problem

Fleet snapshots can have multiple content variants per path — e.g., 3
different versions of `/etc/httpd/conf/httpd.conf` from different hosts. Spec
2 added radio-button variant selection, but users have no way to see *what's
different* between variants before choosing one. They must manually view each
variant's content and mentally compare, which is tedious and error-prone for
large config files.

This spec adds a Compare modal to section tabs that shows an inline diff
between the selected variant and any other variant, plus a one-click "Switch"
button.

## Context: Fleet Refine Decomposition

1. **Fleet Merge Completeness** (Spec 1, code complete) — merge for selinux
   and non_rpm_software, storage suppression
2. **Fleet Refine Lifecycle** (Spec 2, code complete) — prevalence slider,
   variant radio groups, triage recalculation, reset
3. **Fleet Variant Comparison** (this spec) — compare modal with inline diff

## Decisions

### Compare Button

Each non-selected variant row in a variant group gets a "Compare" button.
This appears on section tabs where variant groups are rendered: Config Files,
Containers (quadlets), and Services (drop-ins). The button is only rendered
when `fleet_meta` is present.

The selected (canonical) variant row shows "selected" text instead of a
Compare button.

**Content retrieval:** The Compare button's onclick handler uses the clicked
row's `data-snap-section`, `data-snap-list`, and `data-snap-index` attributes
to read `snapshot[section][list][index].content` for the comparison variant.
The selected variant is found by querying sibling rows in the same
`data-variant-group` for the one with a checked toggle, then reading its
content from the snapshot at the corresponding index.

### Compare Modal

Clicking Compare opens a modal following the existing custom modal pattern
used by unsaved-changes, delete-file, and new-file modals in the codebase
(position:fixed overlay with manual backdrop, not PF6 `pf-v6-c-modal-box`).
PF6 modal migration for all modals is a separate future enhancement.

**Header:** File path as the title, close button (×).

**Subtitle:** Host lists and prevalence for both variants — e.g.,
"Selected (3/5 hosts: web-01, web-02, web-03)" vs
"This variant (2/5 hosts: web-04, web-05)". Uses red/blue color coding
matching the diff lines.

**Body (scrollable):** Line-based inline diff. Red lines (`-`) exist only in
the selected variant. Blue lines (`+`) exist only in the comparison variant.
Gray lines are shared (context). Rendered as a styled `<pre>` with colored
line backgrounds.

**Footer:** Two buttons using PF6 button classes:
- **Close** (`pf-v6-c-button pf-m-secondary`) — dismisses without changes
- **Switch to this variant** (`pf-v6-c-button pf-m-primary`) — toggles the
  radio (same as Spec 2's radio behavior: auto-deselects the current variant,
  selects this one), updates snapshot `.include` fields, updates row styling,
  calls `updateToolbar()` and `setDirty()`, then closes the modal

After Switch, the variant group rows update: the previously-selected row
loses its "selected" label and gains a Compare button; the newly-selected
row gains the "selected" label and loses its Compare button. The modal
closes before this update, so no stale diff state is visible.

Close on backdrop click, consistent with existing modals.

### Diff Algorithm

Simple line-based diff in vanilla JavaScript. Split both file contents by
newline, compute longest common subsequence (LCS), emit additions, removals,
and context lines. Approximately 50-100 lines of code. No external library.

O(n*m) complexity is acceptable for config files, which are typically under
1000 lines. For defensive safety, files over 5000 lines show a "file too
large to diff" message instead of computing.

The diff always compares against the currently selected variant. The modal
subtitle makes this clear with labeled colors. Blue is used for comparison
variant lines (intentional — matches fleet prevalence color coding rather
than the conventional green, for visual consistency with the rest of the
fleet UI).

### Editor Tree (No Changes)

The comparison workflow lives entirely on section tabs. The editor tree does
not need variant grouping. Since Spec 2's radio behavior ensures only one
variant per path has `include: true`, the editor tree naturally shows just
the selected variant for each path. Users:

1. Pick variants on the section tab (Compare modal + radio toggles)
2. Switch to the editor tab to edit the selected variant's content
3. The editor shows only selected files — same as single-host refine

## Testing

### Diff Algorithm Unit Tests
- Identical files: no diff lines
- Single line change: one red, one blue
- Additions only: all blue lines
- Removals only: all red lines
- Empty file vs non-empty: all additions or all removals
- Large file: performance acceptable (no freeze on ~1000 lines)

### Compare Button Rendering
- Compare button appears on non-selected variant rows when fleet_meta is set
- Compare button absent on selected variant row (shows "selected" instead)
- Compare button absent when fleet_meta is not set

### Modal Behavior
- Modal displays correct host lists and prevalence for both variants
- Modal shows correct diff content matching actual file differences
- "Switch to this variant" toggles radio, updates snapshot, closes modal
- "Close" dismisses without changes

## Out of Scope

- Editor tree variant grouping (not needed)
- Side-by-side diff view (inline is sufficient)
- Comparing arbitrary variant pairs (always diffs against selected)
- Word-level diff highlighting
- Syntax highlighting in diff view
- Diff folding or collapsing unchanged sections
- Compose file variant comparison (compose variants not grouped by renderer)
