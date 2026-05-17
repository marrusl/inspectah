# inspectah Roadmap

## Current Status (2026-05-17)

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
| Leaf Package Filter | PLAN APPROVED |

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
⏳ Leaf Package Filter ← PLAN APPROVED
    ↓
⏳ User/Group Materialization ← brainstorm next
    ↓
⏳ v0.8.0-alpha.4 milestone
    ↓
Phase 3b: Fleet Refine
    ↓
CLI Cutover: Rust binary becomes primary `inspectah` command
    ↓
Post-cutover: Architect v2, TUI, build command
```

## Upcoming Work

### Leaf Package Filter (HIGH — plan approved)

**Problem:** Containerfile `dnf install` line has ~477 packages. Should be ~20-50.

**Root cause:** Rust scanner doesn't run `dnf repoquery --userinstalled` to identify user-intent (leaf) packages. The Go code does this and filters to leaf-only.

**Fix:** Two-part — (1) port `classifyLeafAuto()` to Rust RPM inspector, (2) filter view + Containerfile to leaf-only. Plan at `docs/plans/2026-05-17-leaf-package-filter.md`.

### User/Group Materialization (HIGH — brainstorm next)

sysusers classification model, multi-strategy output (sysusers.d, useradd, kickstart). Brainstorm after leaf filter ships.

### Package Group Detection (MEDIUM — future)

Neither Go nor Rust handles `dnf group install` / anaconda group selections. Individual packages from groups (e.g., GNOME desktop) show up as separate items instead of being grouped. Potential approach: query `dnf group list --installed` and `dnf history` to detect group-installed packages, then emit `dnf group install` lines in the Containerfile.

### v0.8.0-alpha.4 Milestone

Bundle: leaf filter + user/group materialization + accumulated bug fixes since alpha.3.

### Fleet Refine (Phase 3b)

Same refine crate, fleet aggregate session. Cross-host package prevalence analysis.

### CLI Cutover

Rust binary becomes primary `inspectah` command. Go binary deprecated.

### Post-Cutover

- Architect v2 (multi-artifact decomposition)
- TUI mode
- `inspectah build` command
