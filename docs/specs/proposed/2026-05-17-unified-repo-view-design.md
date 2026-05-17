# Unified Repo View

## Summary

Replace the current attention-tier triage view with a unified repo-grouped view
where packages are organized by source repository. Attention-requiring packages
bubble to the top within each repo group. Repo-level enable/disable toggles
allow bulk exclusion of entire repositories from the Containerfile output.

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

**Repo group display rules:**

- **Repos with attention items:** Expanded by default. Attention-requiring
  packages shown individually with their attention reason (e.g., "Version
  Downgraded"). Routine packages in the same repo collapse to a summary line
  ("+ 128 routine").
- **All-routine repos:** Collapsed to a single line showing repo name, package
  count, and "All routine — no action needed." Expandable on click.
- **Disabled repos:** Dimmed, struck-through name, collapsed to "N packages
  excluded from Containerfile." Expandable to see what's excluded.

### Attention Summary Counter

A summary line at the top of the Packages section provides cross-repo attention
signal without scanning each group:

```
3 packages need review across 2 repos
```

When the count is zero: "All packages routine — no review needed."

This replaces the need for a separate triage view. The user gets the global
attention signal at the top and the per-repo breakdown below.

### Repo Headers

Each repo group has a header showing:

- **Repo name** (e.g., `appstream`, `epel`, `copr:inspectah`)
- **Package count** (e.g., "130 packages")
- **Enable/disable toggle** — green "enabled" / red "disabled" badge, clickable
- **Provenance indicator** (third-party repos only):
  - "verified" — GPG keys match known signing keys
  - "incomplete" — partial GPG verification (some keys matched)
  - "unknown" — no GPG verification or unknown keys

**Distro repos** (baseos, appstream, crb, fedora, updates, anaconda) get no
provenance badge — they are trusted by definition. Detection uses the existing
`DISTRO_REPOS` constant in `repo_index.rs`, which already covers both RHEL and
Fedora.

**Third-party repos** (EPEL, COPRs, vendor repos) show the provenance badge on
the repo header. Individual packages within third-party repos do NOT get
per-package "third party" badges — the repo grouping makes this redundant.

### Repo Header Ordering

Repos are sorted in the following order:

1. Distro repos first (sorted alphabetically)
2. Third-party verified repos (sorted alphabetically)
3. Third-party unverified/unknown repos (sorted alphabetically)
4. Disabled repos last (within their original category)

### Repo Enable/Disable Toggle

Clicking the enable/disable badge on a repo header toggles the entire repo:

- **Disable:** Emits `ExcludeRepo { section_id }`. All packages from that repo
  are excluded from the Containerfile and triage counts. Any per-package
  include/exclude decisions within the repo are discarded.
- **Re-enable:** Emits `IncludeRepo { section_id }`. All packages from that
  repo return to their default include state. Prior per-package decisions are
  NOT restored — re-enabling is a reset to defaults.

This matches the existing `ExcludeRepo`/`IncludeRepo` operation semantics in
the refine session.

### Per-Package Actions

Within an enabled repo, individual packages can still be included/excluded using
the existing per-package toggles. This is unchanged from the current
implementation.

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
- **Per-package include/exclude** — unchanged.
- **Config Files section** — unchanged. Config files are not repo-grouped.
- **Context sections** — unchanged (Services, Network, Storage, etc.).
- **Containerfile preview** — reflects repo enable/disable decisions.
- **Stats bar** — package/config counts, triage remaining.

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

No new API endpoints needed. The existing `/api/view` response already contains
both `packages` (with `source_repo` and attention data) and `repo_groups` (with
provenance and enabled state). The change is entirely in how the React UI
organizes and renders this data.

## Components Affected

### Modified

- **`DecisionList.tsx`** — Primary change. Currently renders packages in
  `AttentionGroup` components (grouped by attention level). Needs to render in
  repo groups instead, with attention bubbling within each group. The existing
  informational-tier repo sub-grouping logic (lines ~478-514) provides a
  starting pattern.
- **`AttentionGroup.tsx`** — May be repurposed or replaced. The collapsible
  group pattern is reusable but the "attention level as primary axis" framing
  changes.
- **`RepoGroupHeader.tsx`** — Already exists. Needs provenance badge and
  enable/disable toggle.
- **`StatsBar.tsx`** or `MainContent.tsx` — Add the attention summary counter.

### Unchanged

- **`ContainerfilePanel.tsx`** — Reflects decisions, doesn't need to know about
  grouping.
- **`Sidebar.tsx`** — Section navigation unchanged.
- **`attentionUtils.ts`** — Attention reason formatting unchanged.
- **Backend (Rust)** — No changes needed. All data already served.

## Testing

- Repo groups render with correct packages.
- Attention items appear first within each repo group.
- All-routine repos collapse to summary line.
- Repo enable/disable toggle emits correct operation and updates UI.
- Re-enabling a repo resets per-package decisions.
- Disabled repos appear dimmed at bottom of list.
- Attention summary counter updates on repo toggle.
- Per-package include/exclude still works within enabled repos.
- Distro repos show no provenance badge; third-party repos do.
- Fedora repos (`fedora`, `updates`) recognized as distro repos.
