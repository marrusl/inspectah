# Unified Package/Repo Management

**Supersedes:** `2026-05-17-unified-repo-view-design.md`

## Summary

A single package management layout shared between single-machine and fleet
modes. Both modes render the same three-zone structure: repo control bar,
package list with sortable columns, and excluded zone. Fleet adds a
prevalence column; single-machine has a repo column. No dep tree in either
mode. No prevalence zone grouping. No attention level indicators.

The design principle: inspectah is a migration-motivated inspection tool,
not an exhaustive inspection tool. The UI helps users triage what packages
to carry forward from package-mode RHEL to image-mode RHEL.

## Motivation

The current codebase has two divergent package views:

- **Single-machine (report.html, ~7900 lines):** packages grouped by repo
  in accordions, each with include/exclude toggles, inline dep tree
  drill-down, third-party badges, and provenance tracking.
- **Fleet (architect.html, ~385 lines):** flat package list in a drawer,
  no repo grouping, no dep tree, prevalence zones as structural grouping.

Users lose coherent "what comes from where" context when switching to fleet.
The `source_repo` data exists on every `PackageEntry` in fleet mode but the
UI ignores it. Meanwhile, single-machine has features (dep tree, per-repo
accordion grouping) that add structural complexity without serving the core
migration use case: deciding what packages and repos to carry forward.

This spec unifies both views into a single layout, removing features that
don't serve migration triage and adding repo context to fleet.

## Design

### Layout Structure (Both Modes)

```
┌─────────────────────────────────────┐
│ Repo Control Bar                    │  ← always visible, outside scroll
│  baseos        4 pkgs  always incl. │
│  appstream     6 pkgs  always incl. │
│  crb           1 pkg   [toggle]     │
│  epel          3 pkgs  [toggle]     │
├─────────────────────────────────────┤
│ Packages ▲          Repo / Prev.    │  ← sortable column headers
├─────────────────────────────────────┤
│ ☑ bash                    baseos    │  ← package rows
│ ☑ coreutils               baseos   │
│ ☑ gcc                     crb      │
│ ☑ httpd                   appstrm  │
│ ☑ jq                      epel     │
│ ...                                 │
├─────────────────────────────────────┤
│ Excluded                            │  ← dimmed zone (when repos disabled)
│ ̶c̶u̶s̶t̶o̶m̶-̶t̶o̶o̶l̶          repo disabled │
└─────────────────────────────────────┘
```

### Repo Control Bar

Always visible above the scrollable package list, outside the scroll
region. Two-row layout that separates static context from interactive
controls:

**Row 1 — Distro repos (static context):** Plain inline text, not
clickable. Muted color, not reduced opacity (context, not disabled).
Example: `baseos 12 · appstream 28 · anaconda 3`

**Row 2 — Toggleable repos (interactive pills):** Colored pill shapes
with package counts and toggle indicators. Green pills for
official-optional repos, amber pills for third-party repos. Clickable
to enable/disable.

**Three repo tiers:**

| Tier | Examples | Repo bar treatment | Toggle | Color |
|------|----------|-------------------|--------|-------|
| Distro | baseos, appstream, fedora, updates, anaconda, extras | Plain text (row 1) | Not interactive | Muted gray |
| Official-optional | crb, rhel-extensions | Pill with count (row 2) | Toggleable | Green |
| Third-party | epel, COPRs, custom repos | Pill with count (row 2) | Toggleable | Amber |

The two-row split makes affordance self-evident from form factor: text is
static, pills are interactive. No convention to learn. Tab order skips
row 1 entirely — keyboard lands only on toggleable pills in row 2.

**No support status labels.** No "third-party" or "unsupported" text. The
color is the only signal. Users of this tool already know what EPEL and CRB
are.

**Toggle behavior:**
- Disabling a repo excludes all its packages from the containerfile output
  and moves them to the excluded zone.
- Re-enabling restores all packages from that repo to `include=true`
  (the engine's default). Any per-package unchecks the user made before
  disabling are **not preserved** — the user re-unchecks the few packages
  they don't want. This matches the engine's `ExcludeRepo`/`IncludeRepo`
  semantics, which operate all-or-nothing on the repo's package set.
- No confirmation dialog — the action is reversible with one click.
- Uses the existing `RefinementOp::ExcludeRepo` / `IncludeRepo` operations
  without modification.
- **Fleet toggle semantics:** The toggle operates on the merged fleet
  output, not per-host. If only 2 of 50 hosts have EPEL, disabling EPEL
  excludes those 2 hosts' EPEL packages from the containerfile. The other
  48 hosts never had EPEL packages — nothing changes for them.

**Protected repos (distro tier):** baseos, BaseOS, appstream, AppStream,
anaconda, fedora, updates, updates-testing, extras. CRB is explicitly
excluded from this list — it is toggleable (see below). This aligns with
the existing `RepoIndex::is_distro_repo()` logic minus CRB.

**CRB classification change:** CRB moves from distro (locked) to
official-optional (toggleable). `is_distro_repo()` must be updated to
return false for CRB. CRB gets the green color treatment, not amber.

### Package List

A flat list with two sortable columns. No accordions, no grouping headers,
no dep tree.

**Row layout per mode:**

| Mode | Left side | Right side |
|------|-----------|------------|
| Single-machine | ☑ package-name | repo-name (colored text) |
| Fleet | ☑ package-name  repo-name (colored text) | N/M hosts |

**Repo text is always visible** on every row as colored text:
- Distro repos: muted gray (low visual weight)
- Official-optional (crb, rhel-extensions): green
- Third-party (epel, COPRs): amber

**Position differs by mode:** In single-machine, repo is the right column
(it's a primary data axis). In fleet, repo sits inline next to the package
name on the left because the right column is prevalence. This is acceptable
because the shift occurs between deliberate mode switches, not within a
single mode. The recognition cue (colored text, same font treatment) is
spatially stable within each mode.

**Fleet repo provenance:** The merged fleet snapshot assigns one
`source_repo` per `name.arch`. When the same package comes from different
repos across hosts (e.g., `nginx` from `epel` on 3 hosts and `appstream`
on 2), this is treated as a **repo variant** — the same pattern used for
conflicting config file content. The package row shows a variant indicator
alongside the repo text, and the user can inspect which hosts sourced the
package from which repo. The majority repo is shown as the default; the
user's repo selection determines which repo file is included in the
containerfile. Packages with a single consistent repo across all hosts
(the common case) show no variant indicator.

**Prevalence display (fleet only):** Right-aligned N/M count, color-coded:
- Green: consensus (all hosts)
- Amber: near-consensus (most hosts)
- Red: divergent (minority of hosts)

Zone thresholds match the existing `PrevalenceZone` enum in
`inspectah-core/src/types/fleet.rs`.

### Sortable Column Headers

Each mode has two independently sortable columns.

**Single-machine:**
- **Packages** — alphabetical ascending (default) / descending
- **Repo** — tier-first sort: distro repos first (alpha within), then
  official-optional (alpha within), then third-party (alpha within).
  Ascending = distro→third-party. Descending = third-party→distro.

**Fleet:**
- **Packages** — alphabetical ascending (default) / descending
- **Prevalence** — ascending (lowest prevalence first, divergent at top) /
  descending (highest prevalence first, consensus at top)

**Sort interaction:**
- Click a column header to make it the active sort. Click again to toggle
  direction. Three-state cycle: ascending → descending → ascending.
- Only one column is active at a time. Clicking one deactivates the other.
- Active column: header text in accent blue + chevron (▲ or ▼).
  Inactive column: header text in muted gray, no chevron.
- Secondary sort is always the other column (e.g., prevalence sort breaks
  ties alphabetically).
- **Default on load:** alphabetical ascending in single-machine.
  **Prevalence ascending (rarest first) in fleet** — divergent items
  surface at the top by default, giving the fleet view an immediate
  triage-first posture without structural zone grouping.
- **Sort resets on mode switch.** If the user changes sort in fleet,
  switches to single-machine, then returns to fleet, sort resets to
  the mode's default. Sort is a transient exploration action, not a
  preference.

**No sort in the excluded zone.** Excluded packages are always listed
alphabetically within the zone.

### Excluded Zone

A dimmed section at the bottom of the package list. Appears when any
non-distro repo is toggled off.

**Visual treatment:**
- Strikethrough package names
- "repo disabled" label on each row
- Reduced opacity (~40%)
- Separated from the active list by a subtle border

**Visibility:**
- **Hidden** until at least one repo has been toggled off. No "No excluded
  packages" noise in the default state.
- **Visible** once any repo is disabled. Remains visible if the user
  re-enables all repos (showing empty state) so the feature stays
  discoverable after first use.

**States (when visible):**
- **Empty (all repos re-enabled):** "No excluded packages" text
- **Non-empty:** Full list of excluded packages with their repo names
- **Large (50+ packages):** Collapsed by default with "Show N excluded
  packages" expander button

**Re-enabling a repo** removes its packages from the excluded zone and
returns them to the active list at their appropriate sorted position,
all set to `include=true` (engine default).

### What Was Removed

**Dep tree (both modes).** The expandable dependency tree is removed from
both single-machine and fleet views. Leaf/auto classification continues to
operate in the engine — it drives which packages appear in the
containerfile `dnf install` line. But the UI does not surface the tree.

Rationale: inspectah is migration-motivated, not exhaustive. Users don't
exclude git because perl has too many transitive deps. They need git, the
deps come along. The dep tree created analysis paralysis in the exact
population (sysadmins migrating fleets) that needs to move fast.

**Prevalence zone grouping (fleet).** Divergent / near-consensus /
consensus zone headers are removed as a structural grouping mechanism.
Prevalence is per-row metadata, visible as the N/M count and color. Users
sort by the prevalence column to surface divergent items — the sort replaces
the structural grouping.

**Attention level indicators.** Removed from both modes.

**Repo accordion grouping (single-machine).** The accordion-per-repo
pattern from the current report.html is replaced by the flat list with
repo column. The repo bar handles repo-level actions (enable/disable).

## Accessibility

### Repo Toggle Switches
- `role="switch"`, `aria-checked="true|false"`
- Label includes repo name: "EPEL repository: enabled"
- Distro repos are plain text (row 1), not buttons — they are not in
  the tab order at all, not `aria-disabled`
- On disable, trigger `aria-live="polite"` announcement:
  "N packages excluded from [repo name]"
- On re-enable: "EPEL enabled. N packages restored"

### Sort Headers
- Focusable buttons with `aria-sort="ascending|descending|none"`
- Enter or Space toggles sort direction
- Left/Right arrow keys move between the two column headers (wrapping
  at boundary)
- Screen reader announcement: "Packages, sorted ascending" /
  "Prevalence, sortable"

### Excluded Zone
- Count updated via `aria-live="polite"` when packages move in/out
- Expander button (when 50+ packages): `aria-expanded="true|false"`,
  `aria-controls` pointing to the excluded list region
- When collapsed, screen reader announces the count
- When expanded, focus stays on the expander button

### Focus Ring
- All interactive elements (toggles, sort headers, checkboxes, expander)
  must show a visible focus ring: `focus-visible: 3-4px` ring in accent
  blue. Never remove `outline` without a replacement.

### Color-Only Indicator Mitigation
- Repo tier colors (gray/green/amber) must NOT be the sole differentiator.
  The repo bar provides non-color context (locked label vs toggle), and
  tier-first sort clusters repos by tier regardless of color perception.
- Prevalence colors (green/amber/red) are reinforced by the numeric N/M
  count on every row — the number is the primary signal, color is secondary.
- All colored text must meet 4.5:1 contrast ratio against the dark
  background. Muted distro text must use at minimum #888 on #1b1d21
  (~4.6:1). #555 on #1b1d21 is ~2.1:1 — far below threshold.

### General
- Focus stays on the control that was activated (toggle, sort header) —
  don't chase moving packages
- Checkbox state preserved across sort operations. Space toggles
  individual checkboxes. Shift+Click range selection is not supported
  in v1.
- Tab order: repo bar toggle pills (row 2) → column headers → package
  checkboxes → excluded zone expander. Distro text (row 1) is skipped.
- `@media (prefers-reduced-motion: reduce)` — disable any transitions
  on sort reorder or excluded zone movement

## Implementation Notes

### Backend Changes

**`inspectah-web/src/handlers.rs` (single-machine):**
- `ViewResponse` no longer needs `leaf_dep_tree` field (remove or always
  return empty)
- `build_repo_groups()` continues to produce `RepoGroupInfo` for the repo
  bar — no changes needed
- `RepoGroupInfo` gains a `tier` field: `Distro | OfficialOptional |
  ThirdParty` to drive toggle/color behavior

**`inspectah-web/src/fleet_handlers.rs` (fleet):**
- `FleetViewResponse` gains `repo_groups: Vec<RepoGroupInfo>` (reuse the
  single-machine type)
- `FleetItem` already has prevalence data — no changes needed
- `source_repo` on `PackageEntry` is already populated — just needs to be
  included in the fleet JSON response if not already

**`inspectah-core/src/types/rpm.rs`:**
- No changes to `PackageEntry`

**`inspectah-refine`:**
- `ExcludeRepo` / `IncludeRepo` ops already exist — no changes needed
- CRB must be reclassified: update `is_distro_repo()` to exclude CRB

### Frontend Changes

**Both modes share the same component structure:**
- Repo bar component (renders from `RepoGroupInfo[]`)
- Package list component (renders rows with checkbox + name + context column)
- Sort header component (two-column sortable, mode-aware)
- Excluded zone component

**Mode-specific rendering:**
- Single-machine: right column = repo name text
- Fleet: left column = name + repo text, right column = prevalence count

**HTML template:**
- New unified template replaces both the packages section of `report.html`
  and the drawer rendering in `architect.html`
- Sorting, toggling, and excluded zone management are client-side JS
  operating on the view data — no server round-trips for sort/filter

**Performance:**
- No list virtualization in v1. Render all package rows in the DOM.
  If performance degrades with large package lists (200+), revisit with
  a virtual/windowed list in a follow-up. Premature optimization risk
  outweighs the speculative performance concern.

### Migration Path

This is a breaking redesign of the package view. The implementation
replaces the existing accordion-based package section in report.html and
the flat drawer in architect.html. No backwards-compatibility shims.

## Out of Scope

- **Dep tree UI** — removed, not deferred. Re-add only if real users request it.
- **Prevalence zone headers** — removed. Prevalence is metadata, not structure.
- **Attention levels** — removed from this view.
- **Per-package repo badge** — replaced by always-visible repo text column.
- **Sort persistence across sessions** — sort resets on page load.
- **Repo search/filter** — if the repo bar grows beyond ~8 repos, consider
  a filter. Not needed for v1.
- **Mixed-arch fleet warnings** — deferred. Show a confidence note if
  fleet contains mixed architectures, but design is out of scope here.
