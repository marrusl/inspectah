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
| Leaf Package Filter | SHIPPED (2026-05-17) |
| Post-Leaf Bug Fix Run | HIGH |

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
⏳ Post-Leaf Bug Fix Run ← in progress
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

### Post-Leaf Bug Fix Run (HIGH — in progress)

**Status:** Leaf Package Filter shipped 2026-05-17. Testing revealed 4 issues:

1. **Leaf classification quality (HIGH):** Base-install packages (kernel, dosfstools, efibootmgr, langpacks-en, lvm2, shim-aa64) show up as leaf packages on stock CentOS. `dnf repoquery --userinstalled` reports anaconda/kickstart packages as user-installed. Need additional filtering logic.

2. **Service classification noise (HIGH):** `systemctl enable/disable` lines too noisy on stock systems. Service diff isn't comparing against base image defaults properly.

3. **Leaf dep-tree UI (MEDIUM):** `leaf_dep_tree` data exists in snapshot but web UI doesn't surface it. Users can't see what dependencies a leaf package pulls in.

4. **Context tab (MEDIUM):** No way to view non-leaf packages, version changes, or full system picture. Need read-only "Context" tab showing all packages and version deltas. Requires Fern (UX) + Ember (product strategy) input before implementation.

Items 1-3 are bug fixes. Item 4 is a new UI surface requiring design input first.

**Note:** Leaf Package Filter shipped 2026-05-17. See `docs/plans/2026-05-17-leaf-package-filter.md` for implementation details.

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
