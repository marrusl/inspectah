# inspectah Roadmap

## Current Status (2026-05-31)

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
| Fleet Intersection Default | DONE (2026-05-26) |
| Fleet/UI Bug Fix Batch (10 fixes) | DONE (2026-05-26) |
| **v0.8.2-alpha.1** | **TAGGED (2026-05-26)** |
| Section Promotion & Triage Redesign | DONE (2026-05-26) |
| Naming Consistency (REVIEW/REFERENCE) | DONE (2026-05-26) |
| Containerfile Change Highlights | SHIPPED (2026-05-26) |
| Fleet Phase 2b: Refine UI | DONE (shipped incrementally across 2a/2b) |
| Playwright E2E Testing Expansion | SHIPPED (2026-05-27) |
| Single-Host / Fleet UI Convergence | DONE (2026-05-27) |
| Preserve Subscription + Build | SHIPPED (2026-05-29) |
| **v0.8.3-alpha.1** | **TAGGED (2026-05-29)** |
| RPM Baseline Filter Fix | DONE (2026-05-29) |
| RPM Dep Tree: rpm-based Primary | DONE (2026-05-30) |
| Build Output Streaming Fix | DONE (2026-05-30) |
| **v0.8.4-alpha.1** | **TAGGED (2026-05-30)** |
| Fleet Spec 2: Refine | DONE (2026-05-30) |
| Unified Package/Repo Management | DONE (2026-05-30) |
| Docs Overhaul | DONE (2026-05-30) |
| TUI Refine (inspectah refine --tui) | SHIPPED (2026-05-31) |
| Export Tarball Naming Fix | DONE (2026-05-31) |

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
✅ Fleet Intersection Default + Bug Fix Batch (10 fixes: intersection default all sections, fleet toggles, tuned stock suppression, UI polish — 2026-05-26)
    ↓
✅ v0.8.2-alpha.1 (tagged 2026-05-26, 282 commits since alpha.4, binaries for darwin-arm64/linux-arm64/linux-amd64)
    ↓
✅ Section Promotion & Triage Redesign (4 phases, 40 commits, spec+plan — 2026-05-26)
    ↓
✅ Containerfile Change Highlights (14 commits, 6 spec review rounds, 6 plan review rounds — 2026-05-26)
    ↓
✅ Fleet Phase 2b: Refine UI (zones, badges, drawers, variant view, diff drawer, fleet banner — shipped incrementally)
    ↓
✅ Playwright E2E Testing Expansion (66 mock tests + 3 API smoke tests — 2026-05-27)
    ↓
✅ Single-Host / Fleet UI Convergence (visual alignment, shared components — 2026-05-27)
    ↓
✅ Preserve Subscription + Build (2026-05-29)
    ↓
✅ v0.8.3-alpha.1 (tagged 2026-05-29)
    ↓
✅ RPM Baseline Filter Fix + Dep Tree rpm-based Primary + Build Output Streaming (2026-05-30)
    ↓
✅ v0.8.4-alpha.1 (tagged 2026-05-30)
    ↓
✅ Fleet Spec 2: Refine (2026-05-30)
    ↓
✅ Unified Package/Repo Management (2026-05-30)
    ↓
✅ Docs Overhaul (2026-05-30)
    ↓
✅ TUI Refine — terminal UI for refine workflow (32 commits, 60 tests, 2026-05-31)
    ↓
✅ Export Tarball Naming Fix — named subdirectory + input-derived filename (2026-05-31)
    ↓
Config Content Editor (inline editing of config files in refine view — needs spec)
    ↓
Fleet Spec 3: Architect (cross-role hierarchy, possibly multi-phase)
    ↓
CLI Cutover: Rust binary becomes primary `inspectah` command
    ↓
Post-cutover: build command
```

## Shipped Work

### TUI Refine (SHIPPED — 2026-05-31)

**Status:** Merged to `rust`. Terminal interface for the refine workflow, invoked via `inspectah refine --tui <tarball>`.

New crate `inspectah-tui`: 6,150 lines across 38 files, 60 tests. Keyboard-driven triage at 80×24 over SSH. Features: 14-section sidebar, grouped triage list with [+]/[-] indicators, info bar and fullscreen detail with diff highlighting, cross-section search, command mode (:export, :fresh, :section, :stats, :undo/:redo), containerfile preview toggle, user strategy cycling, reviewed progress tracking, SIGTSTP/SIGCONT suspend/resume, session resume.

Also fixed export tarballs: now extract into a named subdirectory (`-refined` suffix) instead of spilling into cwd. Applies to both web and TUI export paths.

### Export Tarball Naming Fix (DONE — 2026-05-31)

Refine export tarballs now use input-derived naming (`foo-refined.tar.gz`) and extract into a named subdirectory. Previously used flat extraction and generic filename. Affects both web and TUI export paths.

### Playwright E2E Testing Expansion (SHIPPED — 2026-05-27)

**Status:** Implemented. Spec at `docs/specs/proposed/2026-05-27-playwright-testing-expansion.md`. Plan at `docs/plans/2026-05-27-playwright-testing-expansion.md`.

Expanded the Playwright e2e suite from 6 spec files (many skipped) to 11 with full surface area coverage. Hybrid fixture strategy: 80% mock API via `page.route()`, 20% real-server smoke tests with checked-in tarballs. Four sub-phases: mock infrastructure POC, remaining fixtures + mutation proof, fixture-structure validation (insta snapshots), real-server smoke tests + curated tarballs. Then Phase 2 (single-host core) and Phase 3 (recent features + fleet gaps).

### Single-Host / Fleet UI Convergence (SHIPPED — 2026-05-27)

**Status:** Visual alignment completed across all promoted sections. Remaining nits tracked in nit-list.md, addressed incrementally.

Single-host and fleet modes achieved visual convergence. Shared component patterns applied: RepoBar, DecisionItem, ContextItem, nav labels, padding, repo sort, os_name deduplication. Bugs fixed in shared components benefit both modes.

### Fleet Spec 2: Refine (DONE — 2026-05-30)

**Status:** Fleet sessions in the refine crate. Resolves the provenance gate, adds fleet-aware refine UX with prevalence columns, threshold controls, variant comparison and selection, baseline confirmation workflow, and section-level host indicators. Builds on Spec 1's merge engine, FleetSnapshotMeta, and per-section host counts.

### Unified Package/Repo Management (DONE — 2026-05-30)

**Status:** Unified package and repository management across single-host and fleet modes. Centralized package resolution and repository handling with consistent UI patterns.

### Docs Overhaul (DONE — 2026-05-30)

**Status:** Comprehensive documentation update following Diataxis framework with diagrams and GitHub Pages deployment.

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

### ~~Fleet Default: Intersection Not Union~~ (DONE — 2026-05-26)

Applied strict intersection default to all 14 section types. Items below full prevalence start excluded but remain visible in the UI. Stock tuned profiles (14 recognized) suppressed by default.

### Anaconda Gap Classifier (IN PROGRESS)

Four-tier classification for packages Anaconda installs beyond the base container image: platform plumbing (locked exclude), promoted (user-intent via service+config signals), installer noise (soft exclude), ambiguous (Investigate, included by default). Group data collected via `dnf group list --installed`. Spec: `process-docs/specs/proposed/2026-06-11-anaconda-gap-classifier.md`. Plan: `process-docs/plans/2026-06-11-anaconda-gap-classifier.md`. Implementation on `feature/anaconda-gap-classifier` branch.

### Group-Aware Rendering (MEDIUM — after anaconda classifier)

Render group-installed packages as `dnf group install` in the Containerfile instead of individual `dnf install` lines. Refine UI shows groups as collapsible rows with ungroup action. Pre-spec: `process-docs/specs/proposed/2026-06-11-group-rendering-pre-spec.md`. Depends on anaconda gap classifier (provides `installed_groups` snapshot data).

### Classification Logic Developer Docs (MEDIUM — after anaconda classifier)

Developer-facing explanation doc covering the full classification pipeline: baseline subtraction, anaconda gap tiers, leaf/auto classification, service/config classification, aggregate consensus. Audience: developers working on inspectah. Location: `docs/explanation/classification-logic.md`.

### Driftify E2E Fixture Coverage Audit (MEDIUM — after testing expansion)

Review what driftify's kitchen-sink mode generates and verify it covers all inspectah sections: packages across repos/states, modified configs, services, users/groups, containers, sysctls, tuned profiles, SELinux, network, storage, scheduled tasks, kernel modules, non-RPM software. Gap analysis: which sections get empty fixtures because driftify doesn't touch them? Expand driftify's mutations to fill gaps so `single-host-e2e.tar.gz` exercises every triage path.

### Playwright E2E: CI Automation (MEDIUM — after testing expansion)

Add `webServer` config to `playwright.config.ts` to auto-start the refine server with checked-in tarballs. GitHub Actions integration. Makes `npx playwright test` run everything including real-server tests without manual server startup.

### Playwright E2E: Visual Regression (MEDIUM — after CI automation)

Playwright screenshot comparison for key views (single-host refine, aggregate zones, containerfile panel, responsive breakpoints). Catches CSS regressions and theme rendering bugs that functional tests miss.

### Playwright E2E: Multi-Browser (MEDIUM — after CI automation)

Add Firefox project to `playwright.config.ts`. Firefox's Gecko engine handles CSS grid/flexbox and keyboard events differently from Chromium, especially relevant for PatternFly 6.

### Aggregate Spec 3: Architect

Takes refined aggregate tarballs, discovers cross-role hierarchy, exports decomposed tarball set. May be multi-phase. Spec to be written after Spec 2 ships.

### ~~CLI UX: Scan Progress Reporting~~ (DONE)

Shipped in `scan-progress` branch (2026-05-24). Full checklist with nested sub-checklists for RPM/Config/Non-RPM, three rendering modes (rich/plain/flat), typed ProgressSink trait, exit codes, SIGINT cancellation. Spec: `docs/specs/proposed/2026-05-24-cli-scan-progress-design.md`.

### ~~CLI UX: Baseline Pull Viewport Height~~ (DONE)

Shipped with scan progress (2026-05-24). Dynamic viewport height: 30% of terminal rows, floor 8, cap 16.

### ~~Test Hygiene: Rename phase6_integration_test~~ (DONE — 2026-05-25)

Renamed to `cross_crate_integration_test.rs` — "Phase 6" was too narrow; the tests cover cross-crate data flow (core, pipeline, refine), not just baseline. Schema version assertions updated to use `SCHEMA_VERSION` constant.

### ~~Section Promotion & Triage Redesign~~ (DONE — 2026-05-26)

Replaced the attention-level triage system (NeedsReview/Informational/Routine) with action-oriented triage buckets (Site/Divergent/Partial). Promoted services, quadlets, flatpak provisioning, sysctls, and tuned profiles from read-only Reference to toggleable Review sections. Ownership/pruning contract between promoted sections and config carry-forward path. Compose deferred until a render contract exists. 40 commits across 4 phases. Spec: `docs/specs/proposed/2026-05-25-section-promotion-triage-redesign.md`. Plan: `docs/plans/2026-05-25-section-promotion-triage-redesign.md`.

### Sysctl Source File Preservation (MEDIUM — needs spec)

The current pipeline collapses all included sysctl overrides into a single synthesized `99-inspectah-migrated.conf`, losing the original source file structure. Instead: preserve the original filenames (e.g., `/etc/sysctl.d/99-kubernetes.conf`, `/etc/sysctl.d/99-tuning.conf`). Group sysctls by source file in the UI — each source file becomes a collapsible group containing its keys. Toggling a key removes it from its source file's rendered output. If all keys from a source file are excluded, the entire file drops from the render. Undo restores the file with the appropriate keys. Touches pipeline rendering (per-file output instead of merged), UI grouping (by-source layout), and the undo model.

### Config Content Viewer (MEDIUM — needs spec)

Config files truncate at 500 chars in a 200px inline box — insufficient for real triage review. Need a modal or drawer that shows the full file content with monospace formatting, the RPM diff when available, and file metadata (path, kind, package owner). Interaction design decision: modal vs drawer vs detail pane. Impacts the row-click behavior (should clicking a config row open the viewer instead of inline expand?). Fern specs, Kit builds.

### ~~Triage Label Vocabulary~~ (DONE — 2026-05-26)

Absorbed by Section Promotion & Triage Redesign. Triage bucket system (Site/Divergent/Partial) replaced attention-level labels. `AttentionGroup` → `TriageBucketGroup`, `attention.rs` → `classify.rs`. Some legacy `AttentionLevel` naming persists in `DecisionList.tsx` and `attentionUtils.ts` as internal plumbing — cosmetic, not user-facing.


### Fleet Divergence Review UX (MEDIUM — needs spec)

The "0/11 confirmed" counter on every fleet Review section is opaque — confirmed *what*? The underlying feature (variant acknowledgment via `useVariantAck`) tracks whether the user has reviewed each divergent item across hosts, but nothing in the UI explains the workflow, why it matters, or what "confirmed" means. Needs a spec covering: what triggers the need for confirmation, how the user signals they've reviewed an item, what visual state changes on confirmation, whether non-divergent items should show a counter at all, and how this integrates with the include/exclude toggles. Fern specs the interaction model, Kit builds.

### Clean Export Mode (MEDIUM — needs spec)

A "clean export" option in the export modal that strips working-state files from the tarball, leaving only buildable output. The current export already respects toggles — excluded configs and packages are omitted from the Containerfile and config tree. But it also includes `snapshot.json` (full inspection data), `secrets-review.md`, `session.json` (autosave), and variant metadata. A clean export drops those, producing a tarball you can hand directly to a build pipeline. Wording TBD (clean / final / production). Could be a toggle in the existing modal or a second button.

### ~~Heuristic Redaction Enhancements~~ (DONE — 2026-06-02)

Rust redaction pipeline now has parity with Go's heuristic detection layer. Shipped in v0.8.5-alpha.1: PasswordHash pattern matching, PEM block detection, false-positive filtering for non-secret patterns, and comment-line filtering to avoid redacting documentation.

### NIC Naming Risk Detection (HIGH — needs spec)

Detect `eth*` kernel-assigned NIC names on multi-NIC systems. After `bootc switch`, predictable naming kicks in and NIC assignment order may change, silently breaking networking. inspectah should detect multi-NIC hosts using kernel-assigned names and emit a HIGH severity warning with remediation guidance (configure persistent naming before migration). The `network.rs` inspector exists but has no `eth*` risk check today.

### PAM Module Parsing (HIGH — needs spec)

inspectah catches modified `pam.d` files but doesn't parse the module load list. Non-base-image PAM modules (`pam_radius`, `pam_duo`, `pam_ldap`, `pam_centrify`) will break authentication silently post-switch. Parse each pam.d file's module references, diff against the base image's module set, and flag missing modules as HIGH severity. This is the difference between "your PAM config changed" and "your authentication will break."

### Scan Output Rethink (MEDIUM — pre-spec ready)

Rethink the `inspectah scan` CLI progress output for the inspector section. The current output was designed for 12-minute scans; the Rust rewrite reduced this to ~10 seconds. Per-inspector spinners, sub-steps, and timers are noise at current speeds, and the in-place ANSI redraw causes rendering artifacts. Direction: streaming receipt (append-only, one line per inspector, sub-steps behind `--verbose`, slow-inspector safety valve). Pre-spec at `process-docs/specs/proposed/2026-06-10-scan-output-rethink.md`.

### Autosave UX Improvements (MEDIUM — needs spec)

Rethink the autosave resume experience. Currently the resume prompt (`[r] Resume [f] Fresh [q] Quit`) is functional but doesn't communicate enough. At minimum: inform the user that they're resuming an autosaved session and show the autosave path. Possibly add a "reset to original tarball" button in the web UI so users can start fresh mid-session without restarting the CLI. Scope TBD — may be a small UX pass or a larger rethink of session lifecycle.

### sshd_config Structured Parse (MEDIUM — needs spec)

inspectah catches modified `sshd_config` but doesn't parse individual directives. Custom ports, AllowUsers, key-only auth settings are operator intent that must carry forward. Additionally, some directives are removed or deprecated across RHEL versions — these are silent footguns when the user doesn't know to check. Parse key directives, flag deprecated/removed ones against the target RHEL version, and surface custom settings as actionable triage items rather than a raw file diff.

### Tier 2 Section Promotion (MEDIUM — after Tier 1)

Promote scheduled tasks (cron + systemd timers), SELinux booleans, and boot parameters (kargs) from Reference to Review. Follows the pattern established by Tier 1 but with additional complexity: SELinux booleans use JSON value dedup instead of prevalence-based merge, kargs need cmdline decomposition for per-argument prevalence, and scheduled tasks need RPM-owned vs user-created filtering. Separate spec after Tier 1 ships.

### Internationalization (i18n) (MEDIUM — taking requests)

Locale-aware output for HTML audit reports and CLI. Translate user-facing strings (headings, labels, recommendations, summary text) at the template/render boundary — internal data identifiers stay English. Initial language support driven by user demand.

### Release Binary Size Optimization (LOW — before 1.0)

Add `[profile.release]` settings to the workspace `Cargo.toml`: `lto = "thin"`, `strip = true`, `codegen-units = 1`. Expected 30-50% binary size reduction (current: 15-18 MB across platforms). Trade-off: slower release builds and stripped stack traces. Dev builds unaffected.

### Pre-1.0 Compat Sweep (LOW — before 1.0)

Audit and remove defensive backward-compatibility code added during the Rust rewrite. Before 1.0, old tarballs are not sacred — users re-scan. Remove: legacy snapshot field sniffing, dual-carrier fallbacks, serde(default) shims for fields that only existed in transitional schemas, and any "if old format, try X" branching. The goal is a clean codebase where every code path serves the current schema, not historical ones.

### ~~Go Compat Removal from Rust Codebase~~ (DONE — 2026-05-24)

Completed as part of Go Retirement. Go source tree, CI workflow, schema compat code, parity tests all removed. Schema floor raised to current version.

### CLI Cutover

Rust binary becomes primary `inspectah` command. Go binary deprecated.

### Post-Cutover

- Architect v2 (multi-artifact decomposition)
- ~~Remove Go source tree~~ (DONE — 2026-05-24)
- ~~Documentation overhaul~~ (DONE — 2026-05-30)
- ~~TUI mode~~ (SHIPPED — 2026-05-31)
- `inspectah build` command
