# Phase 5: Pipeline Rendering & Triage Quality

**Date:** 2026-05-16
**Status:** Proposed
**Scope:** inspectah-core, inspectah-refine, inspectah-pipeline, inspectah-web

## Problem

First real-user testing of `inspectah refine` against a CentOS Stream 9 scan revealed that the Rust pipeline produces a poor operator experience. The web UI faithfully renders what the pipeline gives it, but the pipeline's attention model and Containerfile renderer have fundamental gaps compared to the Go version.

**Root cause:** The Rust pipeline was built for schema/inspector parity, not triage/rendering parity. The Go version has a three-tier classification system with baseline awareness, leaf package filtering, and repo-based grouping. The Rust version collapses all added packages into `NeedsReview` and all unowned configs into `NeedsReview`, producing a "732 of 734 to review" experience that is unusable.

**Observed issues:**
1. All added packages flagged as `NeedsReview` regardless of whether they're baseline OS packages
2. `source_repo` shows "Unknown" for all packages — repo data not surfacing
3. No leaf package filtering — transitive dependencies shown as triage items
4. Config files: all unowned configs flagged as `NeedsReview` including standard system defaults
5. `NoRepo` packages incorrectly classified as `Informational` (should be `NeedsReview` — severity inverted)
6. GPG keys: per-key `rpm --import` lines produce verbose Containerfile output
7. Service enablement: single joined line wraps chaotically in the panel
8. Layout issues: not full-width, hostname buried at bottom of sidebar

## Approach

Two-pass classify-then-normalize architecture, matching the Go version's proven `classifyPackage()` + `NormalizeLeafDefaults()` pattern:

1. **Classify** — rewrite attention model for three-tier baseline-aware classification
2. **Normalize** — separate function applies leaf filtering and include-defaults based on tier

This keeps each function focused, testable, and naturally accommodates fleet normalization later.

## Design

### 1. Core Type Changes (inspectah-core)

**1a. Add `BaselineMatch` variant to `ConfigFileKind`**

The Go schema has a `baseline_match` kind for config files whose content matches the base image. Add `BaselineMatch` to the enum with `#[serde(alias = "baseline_match")]` so existing scan data deserializes correctly. Without this, baseline-matching configs deserialize as `Unowned` and get incorrectly flagged.

**1b. New `AttentionReason` variants**

Add to the existing enum:
- `PackageBaselineMatch` — Tier 1 package found in baseline
- `PackageUserAdded` — Tier 2 package from a recognized repo, not in baseline
- `PackageNoRepoSource` — Tier 3 package with no repository source
- `ConfigBaselineMatch` — Tier 1 config matching base image

These give the UI meaningful badge text instead of generic "Package Not In Baseline" on everything.

**1c. New `RefinementOp` variant: `ExcludeRepo` / `IncludeRepo`**

A repo-level bulk action in the refine session. `ExcludeRepo { repo_id: String }` cascades: sets `include = false` on all packages with matching `source_repo`, excludes the corresponding entry from `repo_files`, and excludes GPG key imports associated with that repo. `IncludeRepo` re-enables them. These are discrete operations through the existing undo/redo stack.

Only third-party repos can be disabled. Distro repos (baseos, appstream, crb, fedora, updates, anaconda) are always included — the `ExcludeRepo` operation should reject attempts to disable them. The distro repo list is defined once as a constant in `inspectah-refine` and used by both the `ExcludeRepo` guard and the UI's repo group rendering.

**1d. Repo grouping metadata**

The attention model output carries grouping information via the existing `entry.source_repo` field on `RefinedPackage`. The UI derives `is_distro_repo` from the shared constant list (exposed via the `/api/health` or a new `/api/config` endpoint) to determine which repos get a disable toggle and which get a "Distro" vs "Third-party" label.

No changes needed to `PackageState`, `RpmSection`, or the `baseline_package_names` / `leaf_packages` / `source_repo` fields — they already exist in the types.

### 2. Attention Model — Classify (Pass 1)

Rewrite `compute_package_attention()` in `inspectah-refine/src/attention.rs`:

**Package classification:**

| Condition | Tier | AttentionLevel | AttentionReason |
|-----------|------|---------------|-----------------|
| Name in `baseline_package_names` | 1 | Routine | PackageBaselineMatch |
| `source_repo` is a known repo, not in baseline | 2 | Informational | PackageUserAdded |
| `PackageState::LocalInstall` | 3 | NeedsReview | PackageLocalInstall |
| `PackageState::NoRepo` | 3 | NeedsReview | PackageNoRepoSource |

**Bug fix:** `NoRepo` currently maps to `Informational` — change to `NeedsReview`.

**Fallback when `baseline_package_names` is `None`:** All `PackageState::Added` packages default to Tier 2 (Informational) rather than NeedsReview. The known-standard-repo list provides partial tiering even without baseline data.

**Config classification (rewrite `compute_config_attention()`):**

| Condition | Tier | AttentionLevel | AttentionReason |
|-----------|------|---------------|-----------------|
| `ConfigFileKind::RpmOwnedDefault` | 1 | Routine | ConfigModified |
| `ConfigFileKind::BaselineMatch` | 1 | Routine | ConfigBaselineMatch |
| `ConfigFileKind::Unowned` | 2 | Informational | ConfigUnowned |
| `ConfigFileKind::RpmOwnedModified` | 3 | NeedsReview | ConfigModified |
| `ConfigFileKind::Orphaned` | — | Informational | ConfigOrphaned |

**Sensitive path overlay:** Additive NeedsReview tag for sensitive paths (`/etc/shadow`, `/etc/ssh/`, etc.), but only promotes Tier 2 items to Tier 3. Tier 1 items (baseline match) are NOT promoted — if the base image ships these files, they don't need review.

### 3. Normalize Defaults (Pass 2)

**`normalize_package_defaults(packages: &mut Vec<RefinedPackage>, rpm: &RpmSection)`**

- **Leaf filtering:** When `rpm.leaf_packages` is `Some`, non-leaf Tier 2 packages are hidden from triage. They're still included in the Containerfile (dnf resolves them), but they don't appear as individual triage items. Dependency info from `leaf_dep_tree` is available on expand but is not a decision point.
- **Include defaults by tier:**
  - Tier 1 → `include = true` (auto-included)
  - Tier 2 leaf → `include = true` (operator confirms)
  - Tier 3 → `include = false` (operator must explicitly opt in)
- **Fallback when `leaf_packages` is `None`:** All Tier 2 packages remain visible as triage items with `include = true`. Noisier (150-200 items) but still dramatically better than 734 NeedsReview.

**`normalize_config_defaults(configs: &mut Vec<RefinedConfig>)`**

- Tier 1 → `include = true`, collapsed in UI
- Tier 2 (Unowned) → `include = true`, shown as reviewable cards
- Tier 3 (RpmOwnedModified) → `include = true`, shown with attention badge
- Orphaned → `include = false`

**Expected triage surface for a typical CentOS Stream 9 system:**
- Packages: ~734 → ~50-80 visible (Tier 2 leaf + Tier 3)
- Configs: ~257 → ~20-40 visible

### 4. `source_repo` Data Fix

All packages currently display "Unknown" for repository. The `source_repo` field exists in `PackageEntry` and the UI reads it (`entry.source_repo || "Unknown"`), but scan data isn't populating it.

**Investigation scope:**
1. Does the Go scanner populate `source_repo` in the snapshot JSON?
2. Does the Rust RPM inspector populate it during `inspectah scan`?
3. Is there a serialization mismatch (field naming, casing)?

**Required outcome:** `source_repo` is populated with the actual repo name (e.g., "baseos", "appstream", "epel") for every package in `packages_added`.

**Repo-to-artifact linkage:** Verify that `source_repo` on packages maps consistently to `repo_id` on `repo_files` entries and that GPG keys can be traced to their originating repo. This linkage is what makes `ExcludeRepo` cascading work. If the mapping is inconsistent, the cascade logic needs a fallback (e.g., matching by repo file path patterns).

### 5. Containerfile Renderer Fixes

Changes to `inspectah-pipeline/src/render/containerfile.rs`:

**5a. GPG key batching**

When all GPG keys share a common standard directory (`/etc/pki/rpm-gpg/`), emit a single directory COPY with no explicit `rpm --import` (keys in the standard path are picked up automatically). For keys in non-standard locations, keep the per-key `COPY` + `rpm --import` pattern.

When a repo is excluded via `ExcludeRepo`, its GPG keys are excluded from the render — key filtering happens upstream via `include` flags.

**5b. Service enablement formatting**

When service count exceeds 3, use backslash-continuation format:
```dockerfile
RUN systemctl enable \
    httpd.service \
    sshd.service \
    chronyd.service \
    firewalld.service
```
At 3 or fewer, keep the single-line format. Same treatment for `systemctl disable`.

**5c. Repo-aware rendering**

When a repo is excluded, all its artifacts disappear from the Containerfile — packages, repo file COPY, GPG imports. The Containerfile re-renders via the existing live-preview mechanism. No new renderer logic beyond respecting `include` flags.

### 6. Web UI Changes

**6a. Layout fixes (independent — no pipeline dependency)**

- **Full-width layout:** Strip PatternFly Page padding. CSS-only in `App.css`.
- **Nav spacing:** Remove `flex: 1` from sidebar nav. Top-align items with natural spacing. CSS-only.
- **Hostname to top of sidebar:** Move hostname/OS block above nav groups. Bold hostname, OS name + version below. First thing the operator sees.
- **Panel collapse direction:** Fix icon to point in the direction the panel will move. Component change in `ContainerfilePanel.tsx`.

**6b. Tier-aware card treatment (depends on pipeline fix)**

- **Tier 1 (Routine):** Collapsed summary — "N baseline packages (auto-included)" with expand toggle. When expanded, compact list (name only, muted text). No checkbox or action buttons.
- **Tier 2 (Informational):** Full card layout with info-level styling (blue left border). Badge shows repo source ("appstream", "epel") instead of "Package Not In Baseline."
- **Tier 3 (NeedsReview):** Current card layout with attention badge.

**6c. Repo grouping and bulk actions (depends on pipeline + source_repo fix)**

- Group Tier 2 packages by `source_repo`.
- **Distro repos** (baseos, appstream, crb, fedora, updates, anaconda): labeled "Distro". Cannot be disabled — no toggle. Always included.
- **Third-party repos** (epel, custom repos, anything not in the distro list): labeled "Third-party". Enable/disable toggle fires `ExcludeRepo` / `IncludeRepo` cascade.
- Unknown repos treated as third-party by default.
- Tier 3 items appear in their own "Needs Review" section, not grouped by repo.

**6d. Config grouping (depends on pipeline fix)**

- Tier 1 collapsed: "N configs match base image (auto-included)"
- Tier 2 (Unowned) shown as reviewable cards
- Tier 3 (RpmOwnedModified) shown with diff indicator when available
- Grouped by kind, not a flat list

## Testing & Success Criteria

**Success metrics (against CentOS Stream 9 scan):**
- Package triage: ~734 → ~50-80 items (Tier 2 leaf + Tier 3)
- Config triage: ~257 → ~20-40 items
- `source_repo` shows actual repo names, not "Unknown"
- Containerfile GPG: 1-2 lines for standard keys, not N repeated imports
- Service enablement: readable multi-line format when >3 services
- Repo grouping visible with distro/third-party labels
- ExcludeRepo on a third-party repo removes packages, repo file, and GPG keys from Containerfile

**Testing approach:**
- Unit tests for classification (given package state + baseline data → expect tier)
- Unit tests for normalization (given tiers + leaf data → expect include defaults)
- Unit tests for `ExcludeRepo` / `IncludeRepo` cascade (packages + repo files + GPG keys toggle together)
- Containerfile renderer tests for GPG batching and service formatting
- E2E test with actual CentOS Stream 9 tarball for end-to-end triage counts
- Regression: existing golden-file tests updated to match new output

## Deferred / Future Work

These items build on the fixed triage foundation and should be tracked for future phases:

1. **Image-mode incompatible service flagging** — Flag services like `dnf-makecache.service`, `dnf-makecache.timer`, `packagekit.service` as incompatible with image mode. New detection logic. Should be its own spec.
2. **Migration summary framing** — Human-readable summary alongside the Containerfile ("Install 23 packages from 3 repos, copy 12 config files, enable 4 services"). Presentation layer enhancement.
3. **Decision/Full view toggle** — Progressive disclosure toggle between "Decisions only" (Tier 2+3) and "Full view" (Tier 1 expanded). Depends on tiering being stable.
4. **Diff view** — Side-by-side "source system" vs "target Containerfile" for a migration overview.
5. **Fleet normalization** — `normalize_package_defaults` supports single-host; fleet aggregate sessions need cross-host consensus logic.

## Files Changed

**inspectah-core:**
- `src/types/config.rs` — add `BaselineMatch` variant
- `src/types/rpm.rs` — verify `source_repo` serialization

**inspectah-refine:**
- `src/attention.rs` — rewrite `compute_package_attention()` and `compute_config_attention()`
- `src/normalize.rs` (new) — `normalize_package_defaults()`, `normalize_config_defaults()`
- `src/session.rs` — add `ExcludeRepo` / `IncludeRepo` operation handling with cascade
- `src/types.rs` — new `AttentionReason` variants, `RefinementOp` variants

**inspectah-pipeline:**
- `src/render/containerfile.rs` — GPG batching, service formatting

**inspectah-web:**
- `ui/src/App.css` — full-width, nav spacing
- `ui/src/components/Sidebar.tsx` — hostname to top
- `ui/src/components/ContainerfilePanel.tsx` — collapse icon direction
- `ui/src/components/DecisionSections.tsx` — tier-aware card treatment, repo grouping
- `ui/src/components/PackageDetail.tsx` — repo badge, distro/third-party label
