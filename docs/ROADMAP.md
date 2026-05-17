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

Produce actionable output for migrating system and human accounts to image mode.

**Three output buckets:**
- **sysusers_ready** — passes all criteria, has a corresponding RPM with upstream sysusers.d snippet. Just ensure the package is in the bootc image.
- **sysusers_candidate** — passes criteria but no upstream snippet. inspectah generates a proposed snippet.
- **needs_review** — fails one or more criteria. Human user or customized service account needing migration planning.

**Composite sysusers-eligible predicate (all five must pass):**
1. UID < SYS_UID_MAX (usually 999, check /etc/login.defs)
2. Shell in {nologin, false, sync, halt, shutdown}
3. Home NOT in /home/*
4. Password locked/empty in shadow (!! or *)
5. Not in {root, nobody}

**Strategy overrides in refine UI:** Each account can be switched between sysusers (default for system accounts), kickstart/blueprint TOML, or useradd (with warning about secrets in image layers). Dual output: produce BOTH kickstart AND blueprint TOML for selected accounts.

**Open design question:** useradd strategy needs password hashes from shadow, but the redaction engine strips them at scan time. Three options: collect-time opt-in flag, re-read at export time, or accept the gap with a warning.

**Edge cases:** group-only entries (supplementary memberships via `m` lines), UID/GID stability for persistent volume owners, non-RPM-sourced accounts (Docker, Ansible, manual useradd), reserved-but-unused accounts (already in setup's basic.conf).

**Classification explainability:** Each account includes a plain-language explanation of WHY it was classified that way, not just the bucket.

**Brainstorm team:** Fern (interaction design for override toggles), Collins (which strategy fits which account types in image mode). Full pre-spec details in PKA project memory.

### Package Group Detection (MEDIUM — future)

Neither Go nor Rust handles `dnf group install` / anaconda group selections. Individual packages from groups (e.g., GNOME desktop) show up as separate items instead of being grouped. Potential approach: query `dnf group list --installed` and `dnf history` to detect group-installed packages, then emit `dnf group install` lines in the Containerfile.

### v0.8.0-alpha.4 Milestone

Bundle: leaf filter + user/group materialization + accumulated bug fixes since alpha.3.

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
