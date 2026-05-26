# inspectah Roadmap

## Current Status (2026-05-25)

| Phase | Status |
|-------|--------|
| Phase 0-1: Schema + RPM Inspector | COMPLETE |
| Phase 2: Inspector Parity (all inspectors) | COMPLETE |
| Phase 3: Refine Service Layer | COMPLETE |
| Phase 4: Web UI | COMPLETE |
| Phase 5: Pipeline Rendering & Triage Quality | COMPLETE |
| Phase 6: Base Image Selection & Cross-Distro | COMPLETE |
| Alpha.3 Bug Fix Pass | COMPLETE |
| Unified Repo View | SHIPPED (2026-05-17) |
| Leaf Package Filter | SHIPPED (2026-05-17) |
| Post-Leaf Bug Fix Run | SHIPPED (2026-05-18) |
| Baseline Visibility | SHIPPED (2026-05-18) |
| User/Group Materialization | SHIPPED (2026-05-19) |
| Service Intent Inference | SHIPPED (2026-05-19) |
| **v0.8.0-alpha.4** | **TAGGED (2026-05-19)** |
| Fleet Spec 1: Aggregate | SHIPPED (2026-05-20) |
| Fleet Phase 2a: Refine Engine | SHIPPED (2026-05-21) |
| CLI Scan Progress | SHIPPED (2026-05-24) |
| Go Retirement + Legacy Removal | DONE (2026-05-24/25) |
| Nit List Sweep (5 items) | DONE (2026-05-25) |
| Nit List Batch A+B (6 fixes) | DONE (2026-05-25) |

## Roadmap to CLI Cutover

```
✅ Phase 2: Inspector Parity
    ↓
✅ Phase 3: Refine Service Layer
    ↓
✅ Phase 4: Web UI for Refine (7 review rounds)
    ↓
✅ Phase 5: Pipeline Rendering (29 commits, 935+ tests)
    ↓
✅ Phase 6: Base Image Selection (14 commits, edition 2024)
    ↓
✅ Alpha.3 Bug Fix Pass (11 commits)
    ↓
✅ Unified Repo View (11 commits, 300 tests, 2026-05-17)
    ↓
✅ Leaf Package Filter (2026-05-17)
    ↓
✅ Post-Leaf Bug Fix Run (16 commits, 2026-05-18)
    ↓
✅ Baseline Visibility (2026-05-18)
    ↓
✅ User/Group Materialization (2026-05-19)
    ↓
✅ Service Intent Inference (13 commits, 2026-05-19)
    ↓
✅ v0.8.0-alpha.4 (tagged 2026-05-19, 181 commits since alpha.3)
    ↓
✅ Fleet Spec 1: Aggregate (29 commits, 3 review rounds, 2026-05-20)
    ↓
✅ Fleet Phase 2a: Refine Engine (21 commits, 4 review rounds, 2026-05-21)
    ↓
✅ Nit List Batch A+B (6 fixes: @commandline handling, fleet prevalence, conflict count, baseline summary — 2026-05-25)
    ↓
Section Promotion + Fleet Users/Groups (spec in progress — containers, kernel/boot, SELinux → actionable)
    ↓
Fleet Phase 2b: Refine UI (badges, drawers, zone headers — built against 2a's API)
    ↓
Fleet Spec 3: Architect (cross-role hierarchy, possibly multi-phase)
    ↓
CLI Cutover: Rust binary becomes primary `inspectah` command
    ↓
Post-cutover: TUI, build command
```

## Shipped Work

### Fleet Phase 2a: Refine Engine (SHIPPED — 2026-05-21)

**Status:** 21 implementation commits, 4 review rounds (Tang, Collins, Thorn, Lens). Spec at `docs/specs/proposed/2026-05-20-fleet-refine-engine-spec.md` (7 review rounds). Plan at `docs/plans/2026-05-20-fleet-refine-engine.md` (3 revisions).

Fleet-aware refinement engine extending the single-host refine crate. Key components:

1. **PrevalenceZone** enum + `classify_zone()` — item-level zone classification using sum-prevalence across variants
2. **ContentHash** newtype + **ItemId** enum (20 variants) — type-safe variant identity across all snapshot sections
3. **FleetContext** + **RefineMode** — auto-detected at session init, zones_active flag for fleet-of-2 suppression
4. **FleetAttention** with custom Ord — zone/attention/prevalence tri-sort, `Option<PrevalenceZone>` for unclassified-sort-last, wired into production `recompute_view()`
5. **Diff engine** — `similar` crate LCS diffs with batch API, binary/size guards
6. **Variant ops via projection** — SelectVariant, EditVariant, DiscardVariant flowing through `snapshot_projected()`. Config/DropIn/Quadlet full ops, Compose select-only. Undo works via cursor replay.
7. **Auto-save persistence** — atomic write-then-rename, stale tarball detection, redo-safe direct restore (not replay-through-apply)
8. **CLI resume flow** — interactive `[r] Resume [f] Fresh [q] Quit` prompt, `--fresh` with destructive confirmation
9. **Variant-aware export** — `fleet/variants/` with hierarchical path structure in export tarball
10. **28 files changed, +8,649 / -12 lines**, 229+ tests in inspectah-refine

### Fleet Spec 1: Aggregate (SHIPPED — 2026-05-20)

**Status:** 29 implementation commits, 3 review rounds (Tang, Collins, Thorn, Mango). Spec at `docs/specs/proposed/2026-05-19-fleet-aggregate-spec.md` (8 review rounds). Plan at `docs/plans/2026-05-20-fleet-aggregate.md` (4 revisions).

Implements `inspectah fleet aggregate` and `inspectah fleet init` commands. Aggregates N single-host tarballs into a fleet tarball with prevalence metadata. Key components:

1. **VariantSelection enum** replaces tie/tie_winner bools — schema-breaking change with load-time migration pre-patch
2. **FleetMergeable trait** with 16 implementations, generic `merge_items` function with prevalence and variant handling
3. **11 section adapters** — RPM, Config, Services, Containers, Network, Storage, Scheduled, SELinux, KernelBoot, NonRpm, UsersGroups
4. **merge_snapshots() orchestrator** — validation, canonical host ordering, section merge, target image/baseline selection, completeness merge, FleetSnapshotMeta
5. **Fleet CLI** — tarball loading, input resolution (manifest/directory/explicit), render+package pipeline, --json-only output matrix, --strict warning promotion
6. **Fleet init** — directory scan, TOML manifest generation with relative paths, baseline conflict detection
7. **Variant file staging** under fleet/variants/ with content hash naming, Containerfile draft header
8. **Fleet-aware audit report** with host counts, section coverage, variant conflicts by unique path
9. **62 files changed, +9,347 / -759 lines**, 315+ tests

### Service Intent Inference (SHIPPED — 2026-05-19)

**Status:** 13 implementation commits. Spec at `docs/specs/proposed/2026-05-19-service-intent-inference-design.md` (9 revisions, 7 review rounds). Plan at `docs/plans/2026-05-19-service-intent-inference.md` (2 revisions).

Replaced stringly-typed service filtering with typed intent inference. Only services where the operator made a deliberate choice (enable, disable, mask, or drop-in override) appear in the Containerfile. Stock-default services matching systemd presets are suppressed. Non-actionable systemd states (`alias`, `indirect`, `enabled-runtime`, etc.) dropped at collection time. Tiered omission/advisory model with structured renderer authority.

### User/Group Materialization (SHIPPED — 2026-05-19)

**Status:** Implemented. Spec at `docs/specs/proposed/2026-05-18-user-group-materialization-design.md`. Plan at `docs/plans/2026-05-18-user-group-materialization.md`.

Collects user and group data from the source host. Custom users surfaced in refine UI with per-account strategy control (skip or useradd). Password handling: omit, preserve, or new. Renders kickstart fragments, blueprint TOML, and Containerfile lines. Custom groups, supplementary memberships, sudoers rules, and SSH key references captured.

### Baseline Visibility (SHIPPED — 2026-05-18)

**Status:** Implemented. Spec at `docs/specs/proposed/2026-05-18-baseline-visibility-design.md`. Plan at `docs/plans/2026-05-18-baseline-visibility.md`.

Shared `baseline_fmt` presentation helpers render baseline comparison sections across audit, readme, and Containerfile outputs. CLI shows pull progress with live viewport during base image extraction.

### Post-Leaf Bug Fix Run (COMPLETE — 2026-05-18)

**Status:** Implemented in 16 commits. Spec at `docs/specs/implemented/2026-05-17-post-leaf-fixes.md`. Plan at `docs/plans/2026-05-18-post-leaf-fixes.md` (8 revision rounds).

**Context-only drift model:** Baseline-present packages are suppressed from the decision surface and `RUN dnf install`, regardless of Added or Modified state. Version drift is informational context only.

1. **Leaf classification quality:** `baseline_suppressed` field threads through `classify_leaf_auto` → `LeafClassification` → `RpmSection` → `recompute_view()`. Epoch normalization (`""` → `"0"`) prevents spurious drift.

2. **Service classification noise:** Three-way contract via `preset_matched_units` collector carrier. Stock-default services suppressed (~110 → divergences + unknowns only). Drop-in overrides preserved.

3. **Leaf dep-tree modal:** Per-package dependency modal on leaf cards. Flat sorted list, fleet-gated, full a11y (distinct ARIA labels, focus trap, keyboard, scroll).

4. **Version Changes context section:** New sidebar section at key `4`. Paired epoch-aware `format_evr_pair` rendering. Typed `VersionChangeEntry` in ViewResponse. Three-state empty reason. Audit table renders when populated.

## Upcoming Work

### Fleet Default: Intersection Not Union (HIGH — next)

Fleet merge currently includes ALL items from ALL hosts by default. A package appearing on only 1 of 3 hosts starts as "included" — the user has to manually exclude it. This produces wrong defaults: 54 packages at 1/3 prevalence all start checked on.

**Required behavior:** Default to the intersection. Only items present on ALL hosts start included. Items below full prevalence start excluded. Users opt in from there. Applies to all section types: packages, configs, users/groups, containers, services, everything.

**Where to fix:** The default `include` state is set during fleet snapshot merging in `inspectah-refine`. The merge logic needs to set `include = false` for any item where `prevalence.count < prevalence.total`.

### Package Group Detection (MEDIUM — future)

Neither Go nor Rust handles `dnf group install` / anaconda group selections. Individual packages from groups (e.g., GNOME desktop) show up as separate items instead of being grouped. Potential approach: query `dnf group list --installed` and `dnf history` to detect group-installed packages, then emit `dnf group install` lines in the Containerfile.

### Fleet Spec 2: Refine

Fleet sessions in the refine crate. Resolves the provenance gate (`redaction_state: None` for fleet tarballs), adds fleet-aware refine UX: prevalence columns, threshold controls, variant comparison and selection, baseline confirmation workflow, section-level host indicators. Builds on Spec 1's merge engine, FleetSnapshotMeta, and per-section host counts. Brainstorm inputs at `docs/specs/proposed/2026-05-07-fleet-refine-product-brainstorm.md` and `2026-05-07-fleet-refine-ux-brainstorm.md`.

**Design decision for Spec 2:** Variant filenames currently use 8-char SHA-256 prefix (human-browsable). If refine needs to machine-correlate variant files back to merge-time identity, extend to full hash or 16+ chars. Decide during brainstorm whether variant files need to be machine-addressable.

### Fleet Spec 3: Architect

Takes refined fleet tarballs, discovers cross-role hierarchy, exports decomposed tarball set. May be multi-phase. Spec to be written after Spec 2 ships.

### ~~CLI UX: Scan Progress Reporting~~ (DONE)

Shipped in `scan-progress` branch (2026-05-24). Full checklist with nested sub-checklists for RPM/Config/Non-RPM, three rendering modes (rich/plain/flat), typed ProgressSink trait, exit codes, SIGINT cancellation. Spec: `docs/specs/proposed/2026-05-24-cli-scan-progress-design.md`.

### ~~CLI UX: Baseline Pull Viewport Height~~ (DONE)

Shipped with scan progress (2026-05-24). Dynamic viewport height: 30% of terminal rows, floor 8, cap 16.

### ~~Test Hygiene: Rename phase6_integration_test~~ (DONE — 2026-05-25)

Renamed to `cross_crate_integration_test.rs` — "Phase 6" was too narrow; the tests cover cross-crate data flow (core, pipeline, refine), not just baseline. Schema version assertions updated to use `SCHEMA_VERSION` constant.

### Section Promotion & Triage Redesign (HIGH — next)

Replace the attention-level triage system (NeedsReview/Informational/Routine) with action-oriented buckets. Promote services, quadlets, flatpak provisioning, sysctls, and tuned profiles from read-only Reference to toggleable Review sections. Includes ownership/pruning contract between promoted sections and the generic config carry-forward path. Compose deferred until a render contract exists. Spec: `docs/specs/proposed/2026-05-25-section-promotion-triage-redesign.md`. Plan: `docs/plans/2026-05-25-section-promotion-triage-redesign.md`.

### Heuristic Redaction Enhancements (MEDIUM — parity with Go)

Rust redaction pipeline needs parity with Go's heuristic redaction capabilities. Go has pattern-based redaction for passwords, API keys, tokens, and connection strings embedded in config files. Rust currently handles explicit redaction but lacks the heuristic detection layer. Needs spec.

### Tier 2 Section Promotion (MEDIUM — after Tier 1)

Promote scheduled tasks (cron + systemd timers), SELinux booleans, and boot parameters (kargs) from Reference to Review. Follows the pattern established by Tier 1 but with additional complexity: SELinux booleans use JSON value dedup instead of prevalence-based merge, kargs need cmdline decomposition for per-argument prevalence, and scheduled tasks need RPM-owned vs user-created filtering. Separate spec after Tier 1 ships.

### Pre-1.0 Compat Sweep (LOW — before 1.0)

Audit and remove defensive backward-compatibility code added during the Rust rewrite. Before 1.0, old tarballs are not sacred — users re-scan. Remove: legacy snapshot field sniffing, dual-carrier fallbacks, serde(default) shims for fields that only existed in transitional schemas, and any "if old format, try X" branching. The goal is a clean codebase where every code path serves the current schema, not historical ones.

### ~~Go Compat Removal from Rust Codebase~~ (DONE — 2026-05-24)

Completed as part of Go Retirement. Go source tree, CI workflow, schema compat code, parity tests all removed. Schema floor raised to current version.

### CLI Cutover

Rust binary becomes primary `inspectah` command. Go binary deprecated.

### Post-Cutover

- Architect v2 (multi-artifact decomposition)
- ~~Remove Go source tree~~ (DONE — 2026-05-24)
- Documentation overhaul
- TUI mode
- `inspectah build` command
