# Phase 5: Pipeline Rendering & Triage Quality

**Date:** 2026-05-16
**Status:** Proposed (revision 2 — addresses round 1 review)
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

Normalization materializes into the working snapshot at session construction time (same as Go). Both preview and export consume the same normalized state — there is no view-only layer that diverges from export truth.

This keeps each function focused, testable, and naturally accommodates fleet normalization later.

## Design

### 1. Core Type Changes (inspectah-core)

**1a. Add `BaselineMatch` variant to `ConfigFileKind`**

The Go schema has a `baseline_match` kind for config files whose content matches the base image. Add `BaselineMatch` to the enum with `#[serde(alias = "baseline_match")]` so existing scan data deserializes correctly. Without this, baseline-matching configs deserialize as `Unowned` and get incorrectly flagged.

**1b. New `AttentionReason` variants**

Add to the existing enum:
- `PackageBaselineMatch` — Tier 1 package found in baseline
- `PackageUserAdded` — Tier 2 package from a recognized repo, baseline verified
- `PackageVersionChanged` — Tier 2 package with version drift from baseline (Modified state)
- `PackageProvenanceUnavailable` — Tier 2 package from a recognized repo, but baseline data is missing (distinct from `PackageUserAdded` — signals reduced classification confidence)
- `PackageNoRepoSource` — Tier 3 package with no repository source
- `ConfigDefault` — Tier 1 config unchanged from RPM default
- `ConfigBaselineMatch` — Tier 1 config matching base image

These give the UI meaningful badge text. Critically, `PackageProvenanceUnavailable` prevents the UI from displaying calm "appstream" badges when the baseline check was never performed — the operator sees "baseline unavailable" instead of false confidence.

**1c. Repo identity model**

The canonical unit of repo identity is the **repo section ID** — the INI stanza header from `.repo` files (e.g., `[baseos]`, `[appstream]`, `[epel]`). This is what `PackageEntry.source_repo` contains and what the Go version uses for classification.

Key structural facts that the identity model must handle:
- **One repo file can define multiple repo section IDs.** `centos.repo` carries `baseos`, `appstream`, and `crb` as separate `[section]` stanzas. `ExcludeRepo` operates on a section ID, not a file — excluding `epel` does not exclude other sections in the same `.repo` file.
- **Multiple repo section IDs can share the same GPG key path.** `baseos` and `appstream` both reference `/etc/pki/rpm-gpg/RPM-GPG-KEY-centosofficial`. GPG key exclusion uses reference counting: a key's `include` flips to `false` only when ALL repo section IDs that reference it are excluded.
- **Repo files are the container, not the identity.** The `.repo` file path is a storage detail. The section ID is the semantic identity. `repo_files` entries map to section IDs via INI parsing of their `content` field.

**`RepoIndex`** — built at session construction time from snapshot data:

```
RepoIndex {
    // section_id → list of package names with this source_repo
    packages_by_repo: BTreeMap<String, Vec<String>>,
    // section_id → repo file path(s) containing this section
    repo_file_by_section: BTreeMap<String, Vec<String>>,
    // section_id → GPG key paths referenced by this section's gpgkey directive
    gpg_keys_by_section: BTreeMap<String, Vec<String>>,
    // GPG key path → set of section IDs that reference it (for ref counting)
    sections_by_gpg_key: BTreeMap<String, BTreeSet<String>>,
    // section_id → provenance state
    provenance: BTreeMap<String, RepoProvenance>,
}
```

**`RepoProvenance`** — computed during `RepoIndex` construction:

| State | Meaning | Bulk toggle available? |
|-------|---------|----------------------|
| `Verified` | Section ID found in `repo_files` content, GPG linkage resolved, packages mapped | Yes (if not a distro repo) |
| `Incomplete` | Section ID exists on packages but no matching repo file stanza, or GPG linkage unresolved | No — show label as informational text only, per-item review available |
| `Unknown` | No `source_repo` data at all (empty or missing field) | No — per-item review only |

When provenance is `Incomplete` or `Unknown`, the UI shows the repo label but removes the bulk toggle. The operator can still include/exclude individual packages. This is the fail-closed behavior: bulk actions only operate on proven scope.

**1d. New `RefinementOp` variants: `ExcludeRepo` / `IncludeRepo`**

`ExcludeRepo { section_id: String }` cascades:
1. Sets `include = false` on all packages where `source_repo == section_id`
2. If no other enabled section IDs reference the same `.repo` file path, sets `include = false` on the repo file
3. For each GPG key referenced by this section: decrements the reference count. If the count reaches zero (no other enabled sections reference this key), sets `include = false` on the key

`IncludeRepo { section_id: String }` reverses the cascade with the same logic (increment ref counts, re-enable artifacts).

**Override preservation:** Repo-level and package-level operations are both entries in the same undo/redo stack. `IncludeRepo` sets `include = true` on all packages in the section. If the operator previously made per-package overrides AFTER an `ExcludeRepo`, those overrides are part of the op stack history — undo/redo replays them in order. There is no special "preserve overrides" logic; the stack provides the correct semantics naturally.

**Guards:**
- `ExcludeRepo` rejects distro repo section IDs. The distro repo list is: `baseos`, `appstream`, `crb`, `fedora`, `updates`, `anaconda`. Defined once as `DISTRO_REPOS: &[&str]` in `inspectah-refine`.
- `ExcludeRepo` rejects section IDs with `RepoProvenance::Incomplete` or `Unknown`. Fail-closed: if we can't prove the cascade scope, we don't allow it.

**`ChangesSummary` integration:** Repo-level ops must show as dirty in `pending_changes()`. Add repo include/exclude tracking alongside existing package/config tracking.

**1e. Distro repo constant and browser exposure**

`DISTRO_REPOS` is defined in `inspectah-refine` and exposed to the browser via a new `policy` field in the existing `/api/health` response:

```json
{
  "status": "ok",
  "policy": {
    "distro_repos": ["baseos", "appstream", "crb", "fedora", "updates", "anaconda"]
  }
}
```

The `policy` object is narrow and versioned. It contains only classification constants needed by the UI. No filesystem paths, secrets, or host-local config details are exposed through this mechanism.

### 2. Attention Model — Classify (Pass 1)

Rewrite `compute_package_attention()` in `inspectah-refine/src/attention.rs`:

**Complete package classification matrix:**

This matrix is exhaustive over the four `PackageState` variants that appear in `packages_added`, crossed with baseline availability and `source_repo` availability. Every cell is a deliberate design choice — no fallthrough or implementer guesswork.

| PackageState | Baseline present, in baseline | Baseline present, not in baseline, repo known | Baseline present, not in baseline, repo empty | Baseline missing, repo known | Baseline missing, repo empty | 
|---|---|---|---|---|---|
| `Added` | Tier 1 Routine `PackageBaselineMatch` | Tier 2 Informational `PackageUserAdded` | Tier 3 NeedsReview `PackageNoRepoSource` | Tier 2 Informational `PackageProvenanceUnavailable` | Tier 3 NeedsReview `PackageNoRepoSource` |
| `Modified` | Tier 1 Routine `PackageBaselineMatch` | Tier 2 Informational `PackageVersionChanged` | Tier 3 NeedsReview `PackageNoRepoSource` | Tier 2 Informational `PackageProvenanceUnavailable` | Tier 3 NeedsReview `PackageNoRepoSource` |
| `LocalInstall` | Tier 3 NeedsReview `PackageLocalInstall` | Tier 3 NeedsReview `PackageLocalInstall` | Tier 3 NeedsReview `PackageLocalInstall` | Tier 3 NeedsReview `PackageLocalInstall` | Tier 3 NeedsReview `PackageLocalInstall` |
| `NoRepo` | Tier 3 NeedsReview `PackageNoRepoSource` | Tier 3 NeedsReview `PackageNoRepoSource` | Tier 3 NeedsReview `PackageNoRepoSource` | Tier 3 NeedsReview `PackageNoRepoSource` | Tier 3 NeedsReview `PackageNoRepoSource` |

**Reading the matrix:**
- **`Added` + baseline match** → Tier 1. Standard OS package, auto-include.
- **`Added` + baseline present + known repo** → Tier 2 with `PackageUserAdded`. Verified classification.
- **`Added` + baseline present + repo empty** → Tier 3. Without repo provenance, we can't distinguish user-added from problematic. Fail-closed.
- **`Added` + baseline missing + known repo** → Tier 2 with `PackageProvenanceUnavailable`. Distinct from `PackageUserAdded` — signals reduced confidence.
- **`Added` + baseline missing + repo empty** → Tier 3. No baseline AND no repo = genuinely unknown.
- **`Modified`** → same classification as `Added` for the corresponding provenance state, but with `PackageVersionChanged` reason when Tier 2 with verified baseline. `Modified` means the host has a different version than the baseline — the package is known and from a repo, just version-drifted.
- **`LocalInstall`** → always Tier 3 regardless of baseline or repo. Locally installed without a repository source — always needs operator input.
- **`NoRepo`** → always Tier 3 regardless of baseline or repo. No repository source means inspectah cannot reconstruct install steps.

**Bug fix:** `NoRepo` currently maps to `Informational` — change to `NeedsReview`. This was a severity inversion.

**Provenance-aware fallback when `baseline_package_names` is `None`:** `Added` and `Modified` packages from known repos classify as Tier 2 `Informational` but with `PackageProvenanceUnavailable` reason, not `PackageUserAdded`. The operator sees "baseline unavailable" badge text and a section-level completeness warning, not calm repo badges that imply verified classification. Packages with empty `source_repo` classify as Tier 3 regardless — no baseline AND no repo = no basis for calm classification.

**Config classification (rewrite `compute_config_attention()`):**

| Condition | Tier | AttentionLevel | AttentionReason |
|-----------|------|---------------|-----------------|
| `ConfigFileKind::RpmOwnedDefault` | 1 | Routine | ConfigDefault (new variant) |
| `ConfigFileKind::BaselineMatch` | 1 | Routine | ConfigBaselineMatch |
| `ConfigFileKind::Unowned` | 2 | Informational | ConfigUnowned |
| `ConfigFileKind::RpmOwnedModified` | 3 | NeedsReview | ConfigModified |
| `ConfigFileKind::Orphaned` | — | Informational | ConfigOrphaned |

**Intentional divergence from Go:** The Go version treats `RpmOwnedModified` as tier 2 (included by default, reviewable). This spec promotes it to Tier 3 (NeedsReview). Rationale: a config file that the operator explicitly modified is a real decision point — the operator should confirm it belongs in the target image. This is a deliberate product choice, not a parity gap.

**Sensitive path overlay:** Additive NeedsReview tag for sensitive paths (`/etc/shadow`, `/etc/ssh/`, etc.), but behavior depends on provenance:
- Tier 1 with verified baseline provenance → NOT promoted. The base image ships this file; it doesn't need review.
- Tier 1 without baseline provenance (classified via repo metadata fallback) → promoted to Tier 3. Without baseline verification, we cannot confirm the sensitive file is an expected default.
- Tier 2 → promoted to Tier 3 as before.

### 3. Normalize Defaults (Pass 2)

**State authority:** Normalization happens at session construction time, immediately after classification. It materializes into the working snapshot's `include` flags before the operation stack begins. This means:
- The "original" snapshot state (used as the baseline for undo/redo and dirty tracking) already reflects normalized defaults
- Both live preview and export render from the same projected state — there is no divergence between what the UI shows and what the tarball contains
- This matches Go's model where normalization happens before the immutable sidecar is created

**Lifecycle:** `import tarball → deserialize snapshot → build RepoIndex → classify (compute attention) → normalize (materialize include defaults) → op stack begins empty → operator interacts`

**`normalize_package_defaults(snapshot: &mut InspectionSnapshot, packages: &[RefinedPackage])`**

- **Leaf filtering:** When `rpm.leaf_packages` is `Some`, non-leaf Tier 2 packages are hidden from triage. They're still included in the Containerfile (dnf resolves them), but they don't appear as individual triage items. Dependency info from `leaf_dep_tree` is available on expand but is not a decision point.
- **Include defaults by tier:**
  - Tier 1 → `include = true` (auto-included)
  - Tier 2 leaf → `include = true` (operator confirms)
  - Tier 3 → `include = false` (operator must explicitly opt in)
- **Fallback when `leaf_packages` is `None`:** All Tier 2 packages remain visible as triage items with `include = true`. Noisier (150-200 items) but still dramatically better than 734 NeedsReview.

**`normalize_config_defaults(snapshot: &mut InspectionSnapshot, configs: &[RefinedConfig])`**

These signatures take the snapshot (not just the view objects) because normalization materializes into authoritative snapshot state at construction time, not into presentation-only clones.

- Tier 1 (RpmOwnedDefault, BaselineMatch) → `include = false`. These files are managed by the package manager or already present in the base image. Copying them would freeze source system defaults and potentially override newer configs from the target image's packages. The collapsed Tier 1 summary is informational: "N configs managed by packages (not copied)."
- Tier 2 (Unowned) → `include = true`. User-created files that need to be explicitly copied to the target.
- Tier 3 (RpmOwnedModified) → `include = true`. User-customized configs that must be preserved — these are the files the operator intentionally changed.
- Orphaned → `include = false`. The owning package was removed — config is likely stale.

**Count/reporting semantics:** Tier 1 configs with `include = false` are NOT counted as "excluded" in `RefineStats` or export summaries. They are a separate category: "package-managed" (not copied because the package manager handles them). This prevents the UI/export from describing RPM defaults as if the operator explicitly excluded them. `RefineStats` should distinguish: included configs (Tier 2 + Tier 3 with `include = true`), package-managed configs (Tier 1 with `include = false` from normalization), operator-excluded configs (`include = false` from explicit operator action), and orphaned configs (`include = false` from normalization).

**Expected triage surface for a typical CentOS Stream 9 system:**
- Packages: ~734 → ~50-80 visible (Tier 2 leaf + Tier 3)
- Configs: ~257 → ~20-40 visible

### 4. Repo Identity and `source_repo` Data Fix

**4a. `source_repo` population**

All packages currently display "Unknown" for repository. The `source_repo` field exists in `PackageEntry` and the UI reads it (`entry.source_repo || "Unknown"`), but scan data isn't populating it.

Investigation scope:
1. Does the Go scanner populate `source_repo` in the snapshot JSON? (The Go `populateSourceRepos(...)` function suggests yes, but verify the field name in serialized output.)
2. Does the Rust RPM inspector populate it during `inspectah scan`?
3. Is there a serialization mismatch (field naming, casing)?

**Required outcome:** `source_repo` should be populated with the repo section ID (e.g., "baseos", "appstream", "epel") whenever the scanner can determine the source repository. However, empty/missing `source_repo` is a valid degraded state — some packages genuinely lack repo provenance (locally installed RPMs, packages from removed repos). The classification matrix in Section 2 explicitly handles empty `source_repo` as a separate column: these packages classify based on their `PackageState` and baseline availability, falling to Tier 3 when provenance is insufficient. The `RepoIndex` assigns `RepoProvenance::Unknown` for empty `source_repo` (Section 4b), disabling bulk repo actions for those packages.

**4b. `RepoIndex` construction**

At session construction time (during import), build the `RepoIndex` by:

1. **Parse repo files:** For each entry in `rpm.repo_files`, parse the `content` field as INI to extract section IDs. Map each section ID to its repo file path. Extract `gpgkey` directives to map section IDs to GPG key paths.
2. **Map packages:** Group `packages_added` by `source_repo` to build `packages_by_repo`.
3. **Link GPG keys:** Build `sections_by_gpg_key` reverse index for reference counting.
4. **Compute provenance:** For each unique `source_repo` value found on packages:
   - If a matching section ID exists in a parsed repo file AND GPG linkage resolves → `Verified`
   - If `source_repo` is non-empty but no matching repo file stanza found, or GPG linkage is partial → `Incomplete`
   - If `source_repo` is empty → `Unknown`

**4c. Incomplete linkage behavior**

When provenance is `Incomplete` or `Unknown`:
- Packages still classify normally (tiers work on `source_repo` + baseline, not on repo file linkage)
- Repo grouping in the UI still shows the `source_repo` label — it's useful context even without a repo file
- Bulk toggle is DISABLED — label is informational text only, no `ExcludeRepo` / `IncludeRepo` available
- Per-package include/exclude still works normally
- A completeness warning appears on the section header: "N packages from repos with unverified provenance"

### 5. Containerfile Renderer Fixes

Changes to `inspectah-pipeline/src/render/containerfile.rs`:

**5a. GPG key batching**

When all included GPG keys share a common standard directory (`/etc/pki/rpm-gpg/`), emit a single directory COPY with no explicit `rpm --import` (keys in the standard path are picked up automatically). For keys in non-standard locations, keep the per-key `COPY` + `rpm --import` pattern.

GPG key exclusion respects reference counting from the `RepoIndex`: a key's `include` is `false` only when all referencing repo sections are excluded. The renderer just checks `include` flags — the ref counting logic lives in the session's `ExcludeRepo` handler.

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

When a repo is excluded, all its artifacts disappear from the Containerfile — packages, repo file COPY, GPG imports (subject to ref counting). The Containerfile re-renders via the existing live-preview mechanism. No new renderer logic beyond respecting `include` flags.

### 6. Web UI Changes

**6a. Layout fixes (independent — no pipeline dependency)**

- **Full-width layout:** Strip PatternFly Page padding. CSS-only in `App.css`.
- **Nav spacing:** Remove `flex: 1` from sidebar nav. Top-align items with natural spacing. CSS-only.
- **Hostname to top of sidebar:** Move hostname/OS block above nav groups. Bold hostname, OS name + version below. First thing the operator sees.
- **Panel collapse direction:** Fix icon to point in the direction the panel will move (right-pointing when collapsed, left-pointing when open). Component change in `ContainerfilePanel.tsx`.

**6b. Tier-aware card treatment (depends on pipeline fix)**

- **Tier 1 (Routine):** Collapsed summary — "N baseline packages (auto-included)" with expand toggle. Default: collapsed. When expanded, compact list (name only, muted text). No checkbox or action buttons.
- **Tier 2 (Informational):** Full card layout with info-level styling (blue left border). Badge shows repo source ("appstream", "epel") when provenance is `Verified`, or "baseline unavailable" when `PackageProvenanceUnavailable`.
- **Tier 3 (NeedsReview):** Current card layout with attention badge.

**Provenance completeness warning:** When `baseline_package_names` is `None`, the Packages section header shows a banner: "Baseline data unavailable — classification confidence reduced. All packages shown for review." This ties into existing completeness signaling.

**6c. Repo grouping and bulk actions (depends on pipeline + source_repo fix)**

- Group Tier 2 packages by `source_repo`. Each group has a header row showing: repo label, distro/third-party badge, package count, and (for third-party repos with `Verified` provenance) an enable/disable toggle.
- **Distro repos** (from `policy.distro_repos`): labeled "Distro". No toggle. Always included.
- **Third-party repos with `Verified` provenance:** labeled "Third-party". Toggle fires `ExcludeRepo` / `IncludeRepo`.
- **Repos with `Incomplete` provenance:** labeled "Unverified". No toggle — label is informational only. Packages are individually actionable.
- **Repos with `Unknown` provenance (empty `source_repo`):** labeled "Unknown". No toggle. Per-item review only.
- Tier 3 items appear in their own "Needs Review" section, not grouped by repo.

**Expand/collapse behavior:**
- Tier 1 groups: collapsed by default. Expand/collapse state is session-local (not persisted across browser sessions).
- Tier 2 repo groups: expanded by default. Each group is independently collapsible.
- Config kind groups: expanded by default.

**Search auto-reveal:** When global search selects an item that is inside a collapsed group (Tier 1 or collapsed repo group):
1. Auto-expand the containing group
2. Scroll the item into view
3. Apply a flash highlight (2-second fade) on the item
4. Focus lands on the item's primary action control (toggle or expand button)

**Keyboard traversal:**
- Repo group headers are first-class keyboard stops in the existing nav model
- `Tab` / `Shift-Tab` moves between group headers. Within a group header, `Tab` advances focus from the header label to the repo toggle (if present), then to the first item in the group. This gives the toggle a natural focus position without requiring a separate keyboard mode.
- `Arrow` / `j` / `k` moves between items within the currently focused group
- `Enter` or `Space` on a group header toggles expand/collapse
- `Enter` or `Space` on a repo toggle fires `ExcludeRepo` / `IncludeRepo`
- All shortcuts suppressed when focus is inside a search field, dialog, or text input

**Repo toggle feedback:**
- Optimistic UI: toggle state flips immediately on click/keypress
- On success: undo toast announced via `role="status"` live region (non-focus-stealing). Text: "Excluded epel — N packages, 1 repo file, 2 GPG keys removed. Undo". Toast does not steal focus from the toggle.
- On failure: revert toggle, show error banner via `role="alert"` live region with reason
- Containerfile preview updates on next render cycle (existing mechanism)

**6d. Config grouping (depends on pipeline fix)**

- Tier 1 collapsed: "N configs managed by packages (not copied)" — collapsed by default, expand to see compact list. These files are handled by the package manager in the target image; copying them would freeze source defaults.
- Tier 2 (Unowned) shown as reviewable cards, grouped by parent directory for visual organization
- Tier 3 (RpmOwnedModified) shown with attention badge. When `diff_against_rpm` data is available on the config entry, show a "View diff" link that opens an inline diff below the card. When no diff data is present, no indicator shown.
- Kind groups are expanded by default

**6e. Responsive behavior**

At widths where the sidebar hides (<1024px) and the Containerfile panel collapses:
- Repo group headers: label + count on first line, toggle (if available) on second line. Truncate long repo names with ellipsis.
- Distro/third-party badges: abbreviate to "D" / "3P" at <768px. Abbreviated badges retain `aria-label="Distro"` / `aria-label="Third-party"` / `aria-label="Unverified"` / `aria-label="Unknown"` so screen readers announce the full meaning regardless of visual abbreviation.
- Tier summary counts remain inline

## Testing & Success Criteria

**Success metrics (against CentOS Stream 9 scan):**
- Package triage: ~734 → ~50-80 items (Tier 2 leaf + Tier 3)
- Config triage: ~257 → ~20-40 items
- `source_repo` shows actual repo names for packages with known provenance; packages with genuinely unknown provenance (LocalInstall, removed repos) display "Unknown" with appropriate degraded styling
- Containerfile GPG: 1-2 lines for standard keys, not N repeated imports
- Service enablement: readable multi-line format when >3 services
- Repo grouping visible with distro/third-party labels and provenance states
- ExcludeRepo on a verified third-party repo removes packages, repo file, and GPG keys from Containerfile
- ExcludeRepo rejected for distro repos and repos with incomplete provenance

**Contract-level tests:**
- Serde round-trip for new enum variants (`BaselineMatch`, all new `AttentionReason` variants) and `RefinementOp` variants
- Repo identity canonicalization: INI parsing of multi-section repo files, section ID extraction, GPG key mapping
- `RepoIndex` construction with verified/incomplete/unknown provenance states
- `RepoProvenance` guard: `ExcludeRepo` accepted for `Verified` third-party, rejected for distro and `Incomplete`/`Unknown`
- Preview/export parity: after normalization, the Containerfile rendered for preview matches the Containerfile in the exported tarball (same `include` flags, same projection path)
- Fallback behavior when `baseline_package_names` is `None`: verify `PackageProvenanceUnavailable` reason, distinct badge text, completeness warning
- Fallback behavior when `leaf_packages` is `None`: all Tier 2 visible, no filtering applied
- Fallback behavior when `source_repo` is empty: `Added`/`Modified` with empty repo + baseline present → Tier 3 `PackageNoRepoSource`; `Added`/`Modified` with empty repo + baseline missing → Tier 3 `PackageNoRepoSource`; `LocalInstall` → Tier 3 always; `NoRepo` → Tier 3 always. Provenance `Unknown`, no bulk toggle.
- Undo/redo for `ExcludeRepo` → per-package override → `IncludeRepo`: op stack replays correctly
- GPG key reference counting: shared key stays `include = true` until all referencing sections excluded
- **Shared repo file retention:** excluding one section from a multi-section `.repo` file (e.g., excluding `crb` from `centos.repo` which also carries `baseos` and `appstream`) must leave the repo file `include = true` until the last enabled section using that file is excluded
- **Repo-only dirty tracking:** a repo-level `ExcludeRepo` / `IncludeRepo` operation must appear in `pending_changes()` and cause `is_dirty()` to return `true`, even if the operation does not change any individual package or config `include` flags (e.g., an empty repo with no matching packages)
- **`Modified` + verified baseline + known repo → `PackageVersionChanged`:** explicit proof that `Modified` packages with full provenance classify as Tier 2 Informational with the `PackageVersionChanged` reason, not as `PackageUserAdded` or `NeedsReview`
- **Tier 1 config kinds stay out of copied config tree:** `RpmOwnedDefault` and `BaselineMatch` configs must default to `include = false` and must NOT appear in the materialized config COPY roots. Verify that the Containerfile renderer does not emit COPY directives for these files.
- Config tier regression: verify each `ConfigFileKind` maps to the expected tier, with explicit test for `RpmOwnedModified` → Tier 3 (intentional divergence from Go)

**Smoke tests:**
- E2E test with actual CentOS Stream 9 tarball for end-to-end triage counts
- Regression: existing golden-file tests updated to match new output

## Intentional Divergences from Go

| Behavior | Go | Rust (this spec) | Rationale |
|----------|----|----|-----------|
| `RpmOwnedModified` config tier | Tier 2 (included, reviewable) | Tier 3 (NeedsReview) | Operator explicitly changed this config — it's a real decision point |
| Missing baseline fallback | Not applicable (baseline always present in Go fleet path) | Tier 2 with `ProvenanceUnavailable` reason + completeness warning | Honest about reduced confidence |
| Sensitive path on Tier 1 without baseline | Not applicable | Promotes to Tier 3 | Without baseline verification, can't confirm sensitive file is expected |
| Tier 1 config `include` default | `include = true` (copied to target) | `include = false` (not copied — package manager handles) | Source defaults should not freeze into target image; target packages install correct versions |

## Deferred / Future Work

These items build on the fixed triage foundation and should be tracked for future phases:

1. **Baseline-aware service filtering** — Services enabled by default in the base image should not be explicitly enabled in the Containerfile. Only render `systemctl enable` for services the user enabled beyond the baseline, and `systemctl disable` for services the user disabled from the baseline. Requires a `baseline_enabled_services` field in the scan schema and a tiering model for services analogous to packages. Pairs with item 2 below.
2. **Image-mode incompatible service flagging** — Flag services like `dnf-makecache.service`, `dnf-makecache.timer`, `packagekit.service` as incompatible with image mode. New detection logic. Should be its own spec.
2. **Migration summary framing** — Human-readable summary alongside the Containerfile ("Install 23 packages from 3 repos, copy 12 config files, enable 4 services"). Presentation layer enhancement.
4. **Decision/Full view toggle** — Progressive disclosure toggle between "Decisions only" (Tier 2+3) and "Full view" (Tier 1 expanded). Depends on tiering being stable.
5. **Diff view** — Side-by-side "source system" vs "target Containerfile" for a migration overview.
6. **Fleet normalization** — `normalize_package_defaults` supports single-host; fleet aggregate sessions need cross-host consensus logic.
7. **Automount/static-route config exceptions** — Specialized config-file handling for automount entries and static network routes. Deferred from Phase 5 scope.

## Files Changed

**inspectah-core:**
- `src/types/config.rs` — add `BaselineMatch` variant to `ConfigFileKind`
- `src/types/rpm.rs` — verify `source_repo` serialization

**inspectah-refine:**
- `src/attention.rs` — rewrite `compute_package_attention()` and `compute_config_attention()` with provenance-aware classification
- `src/normalize.rs` (new) — `normalize_package_defaults()`, `normalize_config_defaults()`
- `src/repo_index.rs` (new) — `RepoIndex` construction, INI parsing, provenance computation, reference counting
- `src/session.rs` — add `ExcludeRepo` / `IncludeRepo` operation handling with cascade, `RepoIndex` integration, `ChangesSummary` extension
- `src/types.rs` — new `AttentionReason` variants, `RefinementOp` variants, `RepoProvenance` enum

**inspectah-pipeline:**
- `src/render/containerfile.rs` — GPG batching, service formatting

**inspectah-web (Rust):**
- `src/handlers.rs` — add `policy` field to `/api/health` response

**inspectah-web (UI):**
- `ui/src/App.css` — full-width, nav spacing
- `ui/src/components/Sidebar.tsx` — hostname to top
- `ui/src/components/ContainerfilePanel.tsx` — collapse icon direction
- `ui/src/components/DecisionSections.tsx` — tier-aware card treatment, repo grouping, expand/collapse, keyboard stops
- `ui/src/components/PackageDetail.tsx` — repo badge with provenance-aware text, distro/third-party label
- `ui/src/components/RepoGroupHeader.tsx` (new) — group header with label, badge, count, conditional toggle
- `ui/src/components/ConfigGroup.tsx` (new or refactored) — kind-based grouping with collapse
