# inspectah Roadmap

## Current Status (2026-05-19)

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
⏳ Phase 3b: Fleet Refine
    ↓
CLI Cutover: Rust binary becomes primary `inspectah` command
    ↓
Post-cutover: Architect v2, TUI, build command
```

## Shipped Work

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

### Package Group Detection (MEDIUM — future)

Neither Go nor Rust handles `dnf group install` / anaconda group selections. Individual packages from groups (e.g., GNOME desktop) show up as separate items instead of being grouped. Potential approach: query `dnf group list --installed` and `dnf history` to detect group-installed packages, then emit `dnf group install` lines in the Containerfile.

### Fleet Refine (Phase 3b)

Same refine crate, fleet aggregate session. Cross-host package prevalence analysis.

### Pre-1.0 Compat Sweep (LOW — before 1.0)

Audit and remove defensive backward-compatibility code added during the Rust rewrite. Before 1.0, old tarballs are not sacred — users re-scan. Remove: legacy snapshot field sniffing, dual-carrier fallbacks, serde(default) shims for fields that only existed in transitional schemas, and any "if old format, try X" branching. The goal is a clean codebase where every code path serves the current schema, not historical ones.

### CLI Cutover

Rust binary becomes primary `inspectah` command. Go binary deprecated.

### Post-Cutover

- Architect v2 (multi-artifact decomposition)
- TUI mode
- `inspectah build` command
