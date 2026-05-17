# Unified Repo View

## Summary

Replace the current attention-tier triage view with a unified repo-grouped view
where packages are organized by source repository. Attention-requiring packages
bubble to the top within each repo group. Third-party repos have enable/disable
toggles for bulk exclusion from the Containerfile output.

This is a **frontend-only change**. No new API endpoints and no backend
behavioral changes. The existing `/api/view` response already provides all
needed data (`packages` with `source_repo` and attention data, `repo_groups`
with provenance and enabled state). The refine session's existing
`ExcludeRepo`/`IncludeRepo` operations, toggle eligibility rules, and
`RefineStats` computation are used as-is.

## Motivation

The current triage view groups packages by attention level (Needs Review >
Informational > Routine) across all repos. This answers "what needs my
attention?" but not "where did things come from?" or "can I drop this entire
repo?" A PM evaluating what to carry from package-mode RHEL into an image-mode
Containerfile thinks in repositories and sources — the triage view forces them
to work at the package level for decisions that are naturally repo-level.

The existing backend already supports `ExcludeRepo`/`IncludeRepo` operations,
`RepoIndex` classification, and `RepoGroupInfo` metadata. The informational
attention tier already sub-groups by repo. This spec promotes repo grouping from
a sub-feature of one tier to the primary organizational axis.

## Design

### Package Organization

Packages are grouped by source repository (`source_repo` field). Within each
repo group, packages are sorted by attention level — NeedsReview first, then
Informational, then Routine.

Packages with blank or missing `source_repo` are grouped under an "Unknown
repository" catch-all section, rendered last in the repo list.

### Repo Group Display Rules

**Expansion defaults (in order of specificity):**

- **Repos containing `needs_review` packages:** Expanded. Individual
  attention-requiring packages shown with their attention reason. Routine
  packages in the same repo collapse to a summary line ("+ N routine").
- **Repos containing only `informational` packages (no `needs_review`):**
  Collapsed. Expandable on click. The collapsed header shows the repo name,
  count, and "N informational" to indicate non-routine content exists.
- **All-routine repos:** Collapsed to a single line showing repo name, package
  count, and "No action needed." Expandable on click.
- **Disabled repos:** Collapsed, dimmed. See Disabled Repo State below.

**Within an expanded repo group:**

- NeedsReview packages appear first, each shown individually with attention
  reason and include/exclude toggle.
- Informational packages appear next, each shown individually.
- Routine packages collapse to a summary line ("+ N routine") that is
  expandable to reveal individual packages.

**Search/filter auto-expansion:** When the section search filter is active,
repo groups containing matching packages auto-expand regardless of their
default expansion state. Collapsed routine summaries within a repo also
auto-expand if they contain matches. This preserves the current filter-driven
expansion contract in the existing UI.

### Attention Summary Counter

A summary line at the top of the Packages section provides cross-repo attention
signal without scanning each group:

```
3 packages need review across 2 repos
```

When needs_review count is zero but informational items exist:

```
No packages flagged for review · 12 informational across 3 repos
```

When all packages are routine:

```
All actionable items reviewed
```

This matches the existing `StatsBar` completion language rather than
overclaiming that everything is "routine."

### Repo Headers

Each repo group has a header row containing:

- **Chevron** (left edge) — disclosure control for expand/collapse. Click target
  for expansion.
- **Repo name** (e.g., `appstream`, `epel`, `copr:inspectah`)
- **Package count** — number of included packages in this repo. This is the
  count of packages with `include: true`, matching how the existing stats bar
  counts visible packages.
- **"Third-party" label** (non-distro repos only) — simple text label, no
  trust/verification claims. See Source Classification below.
- **Enable/disable toggle** (toggleable repos only) — a switch control, not a
  badge. Only rendered for repos that meet the backend's toggle preconditions.
  See Repo Toggle Eligibility below.

**Control separation:** The chevron controls expand/collapse. The switch
controls enable/disable. These are separate controls with separate click
targets. The repo name and count are display-only (not clickable).

### Source Classification

Repos are classified into two categories based on the existing
`DISTRO_REPOS` constant in `repo_index.rs`. This is a policy heuristic based
on section-ID matching, not a cryptographic trust assertion.

- **Distro-origin repos** (baseos, appstream, crb, fedora, updates, anaconda):
  No label. These are the platform repos from the base distribution. They are
  always included (non-toggleable).
- **Third-party repos** (EPEL, COPRs, vendor repos, everything not in
  `DISTRO_REPOS`): Labeled "Third-party" on the repo header. The label
  communicates source classification, not verification status.

The backend `RepoProvenance` field (`verified`, `incomplete`, `unknown`) is
retained in the data model for edge-case detection (e.g., packages referencing
a repo with no matching repo-file stanza), but is **not surfaced as a UI
badge**. On a functioning system with working `dnf update`, repo provenance
issues are already caught at the package-management layer.

### Repo Header Ordering

1. Distro-origin repos (sorted alphabetically)
2. Enabled third-party repos (sorted alphabetically)
3. Disabled third-party repos (sorted alphabetically)
4. Unknown repository (catch-all, always last)

### Repo Toggle Eligibility

Not all repos are toggleable. The UI renders the enable/disable switch **only**
for repos that meet the backend's existing preconditions:

- `is_distro == false` — distro-origin repos are non-toggleable
- `provenance == "verified"` — the repo section ID was found in a parsed repo
  file on the source system

Repos that fail either condition (distro repos, repos with `incomplete` or
`unknown` provenance) show no toggle control. This matches the existing
behavior in `RepoGroupHeader.tsx` and the validation in
`RefineSession::apply_op()`.

### Repo Enable/Disable Behavior

**Disable** (`ExcludeRepo { section_id }`):

- All packages from that repo have `include` set to `false`.
- Per-package include/exclude decisions within the repo are discarded.
- The repo header moves to the disabled section of the list.
- The repo group collapses and dims. See Disabled Repo State below.
- `RefineStats` counts update — excluded packages remain in the view data
  but with `include: false`. The stats bar's "triage remaining" and
  "packages included" counts reflect this.

**Re-enable** (`IncludeRepo { section_id }`):

- All packages from that repo return to their default include state.
- Prior per-package decisions are NOT restored — re-enabling is a reset.
- The repo group moves back to its position in the enabled third-party
  section and expands to its default state.
- Focus moves to the re-enabled repo header.

### Disabled Repo State

Disabled repos are visually distinct:

- **Header:** Dimmed text, struck-through repo name, package count shows
  "N packages excluded." The enable/disable switch shows "disabled" state.
- **Collapsed by default.** Expandable for inspection.
- **When expanded:** Packages are shown in a read-only list. Per-package
  include/exclude toggles are **hidden** (not disabled/grayed — hidden
  entirely). The list is informational: "these are the packages you excluded
  by disabling this repo."
- **No per-package actions** are available while the repo is disabled.
  Re-enabling the repo restores the default toggles.

### Per-Package Actions

Within an enabled repo, individual packages can still be included/excluded
using the existing per-package toggles. This is unchanged from the current
implementation.

### Keyboard and Accessibility

Repo headers participate in the existing roving-tabindex keyboard model:

- **Arrow keys** move focus between repo headers and package rows within the
  packages section.
- **Enter** on a repo header toggles expand/collapse.
- **Space** on the enable/disable switch toggles the repo (where available).
  On repo headers without a switch, Space is a no-op.
- **Tab** moves focus from the repo header to the enable/disable switch (when
  present), then to the first package row within the group.

ARIA attributes:

- Repo header chevron: `aria-expanded`, `aria-controls` pointing to the
  repo group content region.
- Enable/disable switch: `role="switch"`, `aria-checked`, `aria-label`
  including the repo name (e.g., "Disable epel repository").
- Repo group content: `role="group"`, `aria-label` with repo name.

### What This Replaces

- **Attention-tier triage view:** Replaced by repo-grouped view with attention
  bubbling. The attention summary counter at the top provides the cross-repo
  signal that triage view used to give.
- **Decisions/Full toggle:** Already removed in alpha.3. Not reintroduced.
- **Informational tier repo sub-grouping:** The informational tier's existing
  repo sub-grouping in `DecisionList.tsx` is superseded — all tiers now use
  repo grouping as the primary axis.

### What This Keeps

- **Attention levels** (NeedsReview, Informational, Routine) — still computed
  by the attention model, still determine sort order within repo groups, still
  shown as visual indicators on individual packages.
- **Per-package include/exclude** — unchanged within enabled repos.
- **Config Files section** — unchanged. Config files are not repo-grouped.
- **Context sections** — unchanged (Services, Network, Storage, etc.).
- **Containerfile preview** — reflects repo enable/disable decisions. Note:
  `ExcludeRepo` only drops a repo file or GPG key from the Containerfile when
  all sections sharing that artifact are excluded (existing backend behavior).
- **Stats bar** — package/config counts, triage remaining. Counts reflect
  `include` state as computed by the existing `RefineStats`.

## Data Flow

```
Snapshot → RefineSession → RepoIndex (packages_by_repo)
                         → AttentionModel (per-package levels)
                         → ViewResponse { packages, repo_groups }
                              ↓
                         React UI groups by source_repo
                         sorts within group by attention level
                         renders with repo headers + toggles
```

No new API endpoints. The existing `/api/view` response already contains both
`packages` (with `source_repo` and attention data per package) and `repo_groups`
(with `is_distro`, `provenance`, `package_count`, and `enabled` per repo). The
change is entirely in how the React UI organizes and renders this data.

## Components Affected

### Modified

- **`DecisionList.tsx`** — Primary change. Currently renders packages in
  `AttentionGroup` components (grouped by attention level). Refactored to
  render in repo groups with attention bubbling within each group. The existing
  informational-tier repo sub-grouping logic (lines ~478-514) provides a
  starting pattern. Mutation plumbing, viewed-state tracking, and filter-driven
  expansion are preserved but re-wired to the repo-first structure.
- **`AttentionGroup.tsx`** — Replaced by a generic `RepoGroup` collapsible
  component. The attention-tier-specific colors and labels move to per-package
  indicators rather than group-level styling.
- **`RepoGroupHeader.tsx`** — Already exists. Updated to use "Third-party"
  label instead of provenance badges. Toggle control unchanged (already
  correctly scoped to eligible repos).
- **`StatsBar.tsx`** or `MainContent.tsx` — Add the attention summary counter
  line. Match existing stats bar completion language.

### Unchanged

- **`ContainerfilePanel.tsx`** — Reflects decisions, doesn't need to know
  about grouping.
- **`Sidebar.tsx`** — Section navigation unchanged.
- **`attentionUtils.ts`** — Attention reason formatting unchanged.
- **Backend (Rust)** — No changes. Session validation, `ExcludeRepo`/
  `IncludeRepo` semantics, `RefineStats` computation, `RepoIndex`,
  `RepoProvenance`, and Containerfile rendering all used as-is.

## Testing

### Repo Group Rendering
- Packages grouped by `source_repo`, sorted by attention within groups.
- Packages with blank `source_repo` appear in "Unknown repository" group.
- Distro repos appear first, then enabled third-party, then disabled, then
  unknown.

### Expansion Defaults
- Repos with `needs_review` items start expanded.
- Repos with only `informational` items start collapsed.
- All-routine repos start collapsed with summary line.
- Disabled repos start collapsed and dimmed.
- Routine summary within an expanded repo is collapsed by default.

### Repo Toggle
- Toggle switch only rendered for non-distro repos with `verified` provenance.
- Distro repos and `incomplete`/`unknown` provenance repos show no switch.
- Disable emits `ExcludeRepo`, sets packages to `include: false`, collapses
  and dims the group.
- Re-enable emits `IncludeRepo`, resets to defaults, moves repo back to
  enabled section, focus lands on repo header.
- Stats bar counts update correctly after toggle.

### Disabled Repo Behavior
- Expanded disabled repo shows read-only package list.
- Per-package toggles are hidden (not disabled) in disabled repos.
- Re-enabling restores default toggles.

### Attention Summary
- Counter shows needs_review count and repo spread when > 0.
- Shows informational count when needs_review is zero.
- Shows "All actionable items reviewed" when both are zero.

### Search and Filter
- Section search auto-expands repo groups containing matches.
- Collapsed routine summaries auto-expand when containing matches.
- Clearing filter restores default expansion state.

### Keyboard and Accessibility
- Arrow keys navigate between repo headers and package rows.
- Enter on repo header toggles expand/collapse.
- Space on toggle switch activates enable/disable.
- Tab moves from header to switch to first package row.
- ARIA attributes present on chevron, switch, and group container.

### Existing Behavior Preserved
- Per-package include/exclude in enabled repos works unchanged.
- Containerfile preview reflects repo and package decisions.
- Config Files section rendering unchanged.
- Context sections rendering unchanged.
