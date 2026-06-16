# Fleet → Aggregate Rename Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rename the `fleet` subcommand to `aggregate` across the entire inspectah codebase — source, tests, fixtures, templates, completions, and documentation. The current CLI top-level commands are `scan / refine / fleet / build / version` (plus hidden `completions`). After this rename: `scan / refine / aggregate / build / version`.

**Architecture:** Mechanical rename executed bottom-up: core types first (everything depends on them), then modules/CLI/pipeline, then frontend, then tests/fixtures/snapshots, then docs. The rename is `fleet` → `aggregate`, `Fleet` → `Aggregate`, `FLEET` → `AGGREGATE` throughout. Each task uses subtree-scoped audits (`grep -ri fleet <subtree>`) to catch surfaces beyond the enumerated file list.

**CLI structure change:** `fleet` was a parent subcommand with `aggregate` and `init` underneath it. Now `aggregate` IS the top-level command (peer of `scan` and `refine`), and `init` is a subcommand under `aggregate`:
- `inspectah aggregate host1.tar.gz host2.tar.gz` — combine host scans
- `inspectah aggregate init /path/to/tarballs/` — generate manifest

**Tech Stack:** Rust (crates/), TypeScript/React (crates/web/ui/), Tera templates, CSS, Markdown docs

**Owner split:**
- **Tang** — Tasks 1–18 (Rust source, frontend, tests, completions, fixtures, snapshots, templates, CSS)
- **Mango** — Tasks 19–22 (docs/, README.md, ROADMAP.md, CHANGELOG.md, process-docs/)

**Repo:** `/Users/mrussell/Work/bootc-migration/inspectah/`

---

## Naming Convention

All renames follow this mapping:

| Old | New |
|-----|-----|
| `fleet` | `aggregate` |
| `Fleet` | `Aggregate` |
| `FLEET` | `AGGREGATE` |
| `fleet_` | `aggregate_` |
| `fleet-` | `aggregate-` |
| `FleetArgs` | `AggregateArgs` |
| `FleetSubcommand` | `AggregateSubcommand` |
| `FleetAggregateArgs` | absorbed into `AggregateArgs` (fields merge into the top-level command args) |
| `FleetInitArgs` | `AggregateInitArgs` |
| `FleetManifest` | `AggregateManifest` |
| `FleetPrevalence` | `AggregatePrevalence` |
| `FleetMeta` | `AggregateMeta` |
| `FleetSnapshotMeta` | `AggregateSnapshotMeta` |
| `FleetData` | `AggregateData` |
| `fleet_meta` | `aggregate_meta` |
| `fleet_handlers` | `aggregate_handlers` |
| `is_fleet` | `is_aggregate` |
| `fleet-summary` | `aggregate-summary` |
| `fleet.css` | `aggregate.css` |

**Special case:** The current `FleetAggregateArgs` (args for the `fleet aggregate` subcommand) is absorbed into `AggregateArgs` directly — its fields become top-level args of the `aggregate` command. There is no separate `AggregateRunArgs` type.

User-visible strings (help text, headings, labels) change from "fleet" language to "aggregate" language. For example: "Fleet Overview" → "Aggregate Overview", "Fleet Label" → "Aggregate Label", "Aggregate host tarballs into a fleet tarball" → "Aggregate host tarballs into a combined snapshot".

**Serde compatibility note:** Any `#[serde(rename = "fleet")]` or JSON keys named `"fleet"` in the snapshot schema are part of the **output contract**. These MUST be renamed to `"aggregate"` — we are pre-1.0 and there is no backward compatibility requirement. Verify no `#[serde(alias = ...)]` shims are left behind.

---

## Phase 1: Core Types & Modules (Tang)

### Task 1: Rename core types module

**Files:**
- Rename: `crates/core/src/types/fleet.rs` → `crates/core/src/types/aggregate.rs`
- Modify: `crates/core/src/types/mod.rs` (update `mod fleet` → `mod aggregate`, update re-exports)

- [ ] **Step 1: Rename the file**
```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
mv crates/core/src/types/fleet.rs crates/core/src/types/aggregate.rs
```

- [ ] **Step 2: Rename all types inside `aggregate.rs`**

In `crates/core/src/types/aggregate.rs`, rename:
- `FleetPrevalence` → `AggregatePrevalence`
- `FleetMeta` → `AggregateMeta`
- `FleetSnapshotMeta` → `AggregateSnapshotMeta`
- `FleetData` → `AggregateData`
- `FleetLeafInfo` → `AggregateLeafInfo` (if present)
- All doc comments referencing "fleet" → "aggregate"
- Any `#[serde(rename = "...")]` attributes that reference "fleet"

- [ ] **Step 3: Update `crates/core/src/types/mod.rs`**

Change `mod fleet;` → `mod aggregate;` and update all `pub use` re-exports from `fleet::` → `aggregate::`.

- [ ] **Step 4: Update all `fleet` field names in sibling type files**

Each of these files has a `fleet: Option<...>` field on its main struct. Rename the field to `aggregate`:
- `crates/core/src/types/config.rs`
- `crates/core/src/types/containers.rs`
- `crates/core/src/types/kernelboot.rs`
- `crates/core/src/types/network.rs`
- `crates/core/src/types/nonrpm.rs`
- `crates/core/src/types/rpm.rs`
- `crates/core/src/types/scheduled.rs`
- `crates/core/src/types/selinux.rs`
- `crates/core/src/types/services.rs`
- `crates/core/src/types/storage.rs`

Also update any `use ...::types::fleet::` imports to `...::types::aggregate::`.

- [ ] **Step 5: Update `crates/core/src/snapshot.rs`**

Rename `fleet_meta` field → `aggregate_meta`. Update any `use` imports from `types::fleet` → `types::aggregate`. Update doc comments.

- [ ] **Step 6: Verify compilation**
```bash
cargo check -p inspectah-core 2>&1 | head -30
```
Expected: Compilation errors from downstream crates (they still reference `fleet`), but `inspectah-core` types should be internally consistent. If core itself has errors, fix them before proceeding.

- [ ] **Step 7: Commit**
```bash
git add -A crates/core/src/types/ crates/core/src/snapshot.rs
git commit -m "refactor(core): rename fleet types to aggregate

Rename FleetPrevalence → AggregatePrevalence, FleetMeta → AggregateMeta,
FleetSnapshotMeta → AggregateSnapshotMeta, FleetData → AggregateData.
Rename fleet field to aggregate on all section types.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 2: Rename core fleet module (merge/manifest/validate)

**Files:**
- Rename directory: `crates/core/src/fleet/` → `crates/core/src/aggregate/`
- Modify: `crates/core/src/lib.rs` (update `pub mod fleet` → `pub mod aggregate`)

- [ ] **Step 1: Rename the directory**
```bash
mv crates/core/src/fleet crates/core/src/aggregate
```

- [ ] **Step 2: Rename types and functions in all files under `crates/core/src/aggregate/`**

In `mod.rs`: rename all `Fleet`-prefixed types, functions, and doc comments. Update re-exports.

In `manifest.rs`: `FleetManifest` → `AggregateManifest`. Check for `FleetManifestEntry` (may not exist — verify before renaming). Update doc comments referencing "fleet manifest".

In `merge.rs`: The exported function is `merge_snapshots` (not `merge_fleet_snapshots`). Rename any `Fleet`-prefixed types it uses. Update doc comments.

In `validate.rs`: `FleetValidationError` → `AggregateValidationError`, `FleetWarning` → `AggregateWarning`. Update doc comments.

- [ ] **Step 3: Update `crates/core/src/lib.rs`**

Change `pub mod fleet;` → `pub mod aggregate;`.

- [ ] **Step 4: Verify compilation**
```bash
cargo check -p inspectah-core 2>&1 | head -30
```
Expected: `inspectah-core` compiles cleanly. Downstream crates will still error.

- [ ] **Step 5: Commit**
```bash
git add -A crates/core/src/aggregate/ crates/core/src/lib.rs
git rm -r --cached crates/core/src/fleet/ 2>/dev/null || true
git commit -m "refactor(core): rename fleet module to aggregate

Rename fleet/ directory to aggregate/. FleetManifest → AggregateManifest,
merge_fleet_snapshots → merge_aggregate_snapshots, etc.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 3: Restructure CLI — fleet → aggregate as top-level command

**Files:**
- Rename: `crates/cli/src/commands/fleet.rs` → `crates/cli/src/commands/aggregate.rs`
- Modify: `crates/cli/src/commands/mod.rs`
- Modify: `crates/cli/src/main.rs`

This is the only task with a structural change beyond rename. The old `fleet` was a parent subcommand containing `aggregate` and `init`. Now `aggregate` IS the top-level command (peer of `scan`, `refine`, `build`, and `version`). The old `fleet aggregate` args are absorbed into `aggregate` directly. `init` remains as the only subcommand under `aggregate`.

**Target CLI shape:**
```
inspectah scan          — scan a single host
inspectah refine        — open the refine UI
inspectah aggregate     — combine multiple host scans (was: inspectah fleet aggregate)
inspectah aggregate init — generate manifest (was: inspectah fleet init)
inspectah build         — generate migration artifacts
inspectah version       — print version information
```

- [ ] **Step 1: Rename the file**
```bash
mv crates/cli/src/commands/fleet.rs crates/cli/src/commands/aggregate.rs
```

- [ ] **Step 2: Restructure the command in `aggregate.rs`**

The old structure had:
- `FleetArgs` with a `FleetSubcommand` enum containing `Aggregate(FleetAggregateArgs)` and `Init(FleetInitArgs)`

The new structure needs two guarantees:
- `inspectah aggregate host1.tar.gz host2.tar.gz` parses as the normal aggregation run form
- `inspectah aggregate init /path/to/tarballs` parses as the `init` subcommand, not as a positional input named `init`

Do NOT leave the `clap` parser shape implicit. Use a parser model that produces an unambiguous runtime enum before dispatch. One acceptable end-state looks like:

```rust
pub enum AggregateMode {
    Run(AggregateArgs),
    Init(AggregateInitArgs),
}
```

The exact `clap` wrapper can vary, but it must reserve `init` as a real subcommand and must not rely on a naive `Vec<PathBuf>` + `Option<AggregateSubcommand>` shape unless a parser test proves that `init` cannot be swallowed as an input.

Read the current `fleet.rs` carefully to get the full arg list from `FleetAggregateArgs` — move those fields into the run-form `AggregateArgs`.

- Update all `use inspectah_core::fleet::` → `use inspectah_core::aggregate::`
- Update all doc comments and help strings: "fleet" → "aggregate"
- The old `FleetAggregateArgs` fields merge into `AggregateArgs`; `FleetInitArgs` becomes `AggregateInitArgs`

- [ ] **Step 3: Update dispatch logic**

Dispatch on the parsed mode, not by partially moving a `command` field and then trying to reuse the original args struct:
```rust
pub fn run(mode: AggregateMode) -> Result<()> {
    match mode {
        AggregateMode::Init(init_args) => run_init(init_args),
        AggregateMode::Run(run_args) => run_aggregate(run_args),
    }
}
```

- [ ] **Step 4: Update `crates/cli/src/commands/mod.rs`**

Change `mod fleet;` → `mod aggregate;`. Update the `Commands` enum variant from `Fleet(fleet::FleetArgs)` → `Aggregate(aggregate::AggregateArgs)`.

- [ ] **Step 5: Update `crates/cli/src/main.rs`**

Change `Commands::Fleet(args)` → `Commands::Aggregate(args)` in the match arm.

- [ ] **Step 6: Verify compilation**
```bash
cargo check -p inspectah-cli 2>&1 | head -30
```

- [ ] **Step 7: Smoke test**
```bash
cargo run -p inspectah-cli -- aggregate --help
cargo run -p inspectah-cli -- aggregate init --help
cargo run -p inspectah-cli -- --help
```
Verify: `aggregate` appears as a top-level peer of `scan` and `refine`. `aggregate --help` shows positional args for tarballs and the `init` subcommand. `aggregate init --help` resolves to the `init` subcommand rather than treating `init` as an input tarball. `fleet` does not appear anywhere.

- [ ] **Step 8: Check runtime-generated strings**

The CLI also generates `fleet.toml` manifest filenames, `fleet-{label}` tarball names, default `"fleet"` labels, and `"Fleet ..."` output/header text. Grep `crates/cli/src/commands/aggregate.rs` for any remaining `fleet` string literals and rename them.

- [ ] **Step 9: Commit**
```bash
git add -A crates/cli/src/commands/ crates/cli/src/main.rs
git commit -m "refactor(cli): replace fleet with aggregate as top-level command

inspectah fleet aggregate → inspectah aggregate. The old fleet parent
subcommand is removed; aggregate is now a peer of scan and refine.
init remains as a subcommand under aggregate.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Phase 2: Downstream Crates (Tang)

### Task 4: Rename collect crate fleet references

**Files:**
- Modify: `crates/collect/src/inspectors/kernelboot.rs`
- Modify: `crates/collect/src/inspectors/rpm/modules.rs`
- Modify: `crates/collect/src/inspectors/rpm/repos.rs`
- Modify: `crates/collect/src/inspectors/selinux.rs`
- Modify: `crates/collect/src/inspectors/services.rs`
- Modify: `crates/collect/src/inspectors/storage.rs`
- Modify: `crates/collect/src/inspectors/subscription.rs`

- [ ] **Step 1: Rename field initializers**

Each file has `fleet: None` initializers. Change to `aggregate: None`. Also update any `use` imports from `types::fleet` → `types::aggregate`.

- [ ] **Step 2: Verify compilation**
```bash
cargo check -p inspectah-collect 2>&1 | head -30
```

- [ ] **Step 3: Commit**
```bash
git add -A crates/collect/
git commit -m "refactor(collect): rename fleet field to aggregate in inspectors

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 5: Rename pipeline crate fleet references

**Files:**
- Modify: `crates/pipeline/src/render/audit.rs`
- Modify: `crates/pipeline/src/render/configtree.rs`
- Modify: `crates/pipeline/src/render/containerfile.rs`
- Modify: `crates/pipeline/src/render/report.rs`
- Modify: `crates/pipeline/src/render/service_intent.rs`
- Rename: `crates/pipeline/templates/report/fleet-summary.html` → `aggregate-summary.html`
- Modify: `crates/pipeline/templates/report/base.html`
- Modify: `crates/pipeline/assets/report.css`

- [ ] **Step 1: Rename template file**
```bash
mv crates/pipeline/templates/report/fleet-summary.html crates/pipeline/templates/report/aggregate-summary.html
```

- [ ] **Step 2: Update template content**

In `aggregate-summary.html`: rename CSS classes (`fleet-summary` → `aggregate-summary`, `fleet-summary-meta` → `aggregate-summary-meta`), headings ("Fleet Overview" → "Aggregate Overview", "Fleet Label" → "Aggregate Label"), and template variables (`fleet_label` → `aggregate_label`, `fleet_host_count` → `aggregate_host_count`, `fleet_baseline_provisional` → `aggregate_baseline_provisional`, `fleet_leaf_partial` → `aggregate_leaf_partial`, `fleet_leaf_authority_hosts` → `aggregate_leaf_authority_hosts`).

In `base.html`: `is_fleet` → `is_aggregate`, include path `report/fleet-summary.html` → `report/aggregate-summary.html`.

- [ ] **Step 3: Update render source files**

In `report.rs`: rename all template variable names (`fleet_label`, `fleet_host_count`, etc. → `aggregate_*`), `is_fleet` → `is_aggregate`, include reference.

In `audit.rs`, `configtree.rs`, `containerfile.rs`, `service_intent.rs`: rename `fleet_meta` → `aggregate_meta`, update imports from `types::fleet` → `types::aggregate`, rename any `Fleet`-prefixed types.

- [ ] **Step 4: Update CSS**

In `report.css`: `.fleet-summary` → `.aggregate-summary`, `.source-info-fleet` → `.source-info-aggregate`.

Also check `crates/pipeline/templates/report/source-info.html` for `source-info-fleet` class references.

- [ ] **Step 5: Verify compilation**
```bash
cargo check -p inspectah-pipeline 2>&1 | head -30
```

- [ ] **Step 6: Commit**
```bash
git add -A crates/pipeline/
git commit -m "refactor(pipeline): rename fleet to aggregate in render and templates

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 6: Rename refine crate fleet module

**Files:**
- Rename directory: `crates/refine/src/fleet/` → `crates/refine/src/aggregate/`
- Modify: `crates/refine/src/lib.rs`
- Modify: `crates/refine/src/classify.rs`
- Modify: `crates/refine/src/normalize.rs`
- Modify: `crates/refine/src/projection/decisions.rs`
- Modify: `crates/refine/src/projection/reference.rs`
- Modify: `crates/refine/src/session.rs`
- Modify: `crates/refine/src/tarball.rs`
- Modify: `crates/refine/src/types.rs`

- [ ] **Step 1: Rename the directory**
```bash
mv crates/refine/src/fleet crates/refine/src/aggregate
```

- [ ] **Step 2: Rename types and functions in all files under `crates/refine/src/aggregate/`**

In `classify.rs`, `diff.rs`, `mod.rs`, `variant_ops.rs`: rename all `Fleet`-prefixed types, function names, doc comments. Update imports from `core::types::fleet` → `core::types::aggregate` and `core::fleet` → `core::aggregate`.

- [ ] **Step 3: Update `crates/refine/src/lib.rs`**

Change `pub mod fleet;` → `pub mod aggregate;`.

- [ ] **Step 4: Update remaining refine source files**

In `classify.rs`, `normalize.rs`, `projection/decisions.rs`, `projection/reference.rs`, `session.rs`, `tarball.rs`, `types.rs`: update all `fleet` references — imports, field accesses, function calls, doc comments.

- [ ] **Step 5: Verify compilation**
```bash
cargo check -p inspectah-refine 2>&1 | head -30
```

- [ ] **Step 6: Commit**
```bash
git add -A crates/refine/src/
git commit -m "refactor(refine): rename fleet module to aggregate

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 7: Rename web crate fleet handlers + API wire contract

**Files:**
- Rename: `crates/web/src/fleet_handlers.rs` → `crates/web/src/aggregate_handlers.rs`
- Modify: `crates/web/src/handlers.rs`
- Modify: `crates/web/src/lib.rs`

This task covers both the module rename AND the API wire contract. The web crate exposes fleet-related API routes and health payload fields that are part of the contract with the frontend.

- [ ] **Step 1: Rename the file**
```bash
mv crates/web/src/fleet_handlers.rs crates/web/src/aggregate_handlers.rs
```

- [ ] **Step 2: Rename all types and functions in `aggregate_handlers.rs`**

Rename all `fleet`-prefixed function names, imports from `core::fleet` → `core::aggregate` and `core::types::fleet` → `core::types::aggregate`, `refine::fleet` → `refine::aggregate`. Update doc comments.

- [ ] **Step 3: Rename API route paths**

The web crate serves routes like `/api/fleet/view`, `/api/fleet/diff`, etc. These must change to `/api/aggregate/view`, `/api/aggregate/diff`. Check:
- `crates/web/src/lib.rs` — route registration (this is where routes are registered, not `handlers.rs`)
- `crates/web/src/handlers.rs` — the `/api/health` endpoint emits a `fleet` field in its JSON payload; rename to `aggregate`

Run `grep -rn 'fleet' crates/web/src/` to find all wire-contract surfaces.

- [ ] **Step 4: Update `crates/web/src/lib.rs`**

Change `mod fleet_handlers;` → `mod aggregate_handlers;`. Update route registration to use `aggregate_handlers::` and `/api/aggregate/*` paths.

- [ ] **Step 5: Verify full workspace compilation**
```bash
cargo check --workspace 2>&1 | head -50
```
Expected: All Rust crates compile. This is the first full-workspace check — fix any cross-crate reference misses.

- [ ] **Step 6: Subtree audit**
```bash
grep -ri 'fleet' crates/web/src/ --include='*.rs'
```
Expected: No matches. Fix any stragglers.

- [ ] **Step 7: Commit**
```bash
git add -A crates/web/src/
git commit -m "refactor(web): rename fleet handlers and API routes to aggregate

Rename fleet_handlers → aggregate_handlers. API routes /api/fleet/*
→ /api/aggregate/*. Health payload fleet field → aggregate.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Phase 3: Rust Tests (Tang)

### Task 8: Rename core crate test files

**Files:**
- Rename: `crates/core/tests/fleet_e2e_test.rs` → `aggregate_e2e_test.rs`
- Rename: `crates/core/tests/fleet_merge_test.rs` → `aggregate_merge_test.rs`
- Rename: `crates/core/tests/fleet_orchestrator_test.rs` → `aggregate_orchestrator_test.rs`
- Rename: `crates/core/tests/fleet_validate_test.rs` → `aggregate_validate_test.rs`
- Rename: `crates/core/tests/fleet_zone_test.rs` → `aggregate_zone_test.rs`
- Modify: `crates/core/tests/subscription_integration_test.rs`

- [ ] **Step 1: Rename test files**
```bash
cd crates/core/tests
for f in fleet_*.rs; do mv "$f" "aggregate_${f#fleet_}"; done
cd /Users/mrussell/Work/bootc-migration/inspectah
```

- [ ] **Step 2: Update content in all renamed files**

In each renamed file: update imports (`use inspectah_core::fleet::` → `use inspectah_core::aggregate::`, `use inspectah_core::types::fleet::` → `use inspectah_core::types::aggregate::`), type names (`Fleet*` → `Aggregate*`), field names (`fleet_meta` → `aggregate_meta`, `fleet:` → `aggregate:`), test function names (`test_fleet_*` → `test_aggregate_*`), string literals referencing "fleet", and doc comments.

- [ ] **Step 3: Update `subscription_integration_test.rs`**

Rename `fleet:` field initializers → `aggregate:`, update imports.

- [ ] **Step 4: Run core tests**
```bash
cargo test -p inspectah-core 2>&1 | tail -20
```
Expected: All tests pass.

- [ ] **Step 5: Commit**
```bash
git add -A crates/core/tests/
git commit -m "test(core): rename fleet tests to aggregate

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 9: Rename refine crate test files

**Files:**
- Rename: `crates/refine/tests/fleet_diff_test.rs` → `aggregate_diff_test.rs`
- Rename: `crates/refine/tests/fleet_e2e_test.rs` → `aggregate_e2e_test.rs`
- Rename: `crates/refine/tests/fleet_export_test.rs` → `aggregate_export_test.rs`
- Rename: `crates/refine/tests/fleet_session_test.rs` → `aggregate_session_test.rs`
- Rename: `crates/refine/tests/fleet_types_test.rs` → `aggregate_types_test.rs`
- Rename: `crates/refine/tests/fleet_variant_ops_test.rs` → `aggregate_variant_ops_test.rs`
- Modify: `crates/refine/tests/attention_test.rs`
- Modify: `crates/refine/tests/autosave_test.rs`
- Modify: `crates/refine/tests/cross_crate_integration_test.rs`
- Modify: `crates/refine/tests/session_test.rs`
- Modify: `crates/refine/tests/types_test.rs`

- [ ] **Step 1: Rename test files**
```bash
cd crates/refine/tests
for f in fleet_*.rs; do mv "$f" "aggregate_${f#fleet_}"; done
cd /Users/mrussell/Work/bootc-migration/inspectah
```

- [ ] **Step 2: Update content in all renamed and modified files**

Same pattern as Task 8: imports, type names, field names, test function names, string literals, doc comments.

- [ ] **Step 3: Run refine tests**
```bash
cargo test -p inspectah-refine 2>&1 | tail -20
```

- [ ] **Step 4: Commit**
```bash
git add -A crates/refine/tests/
git commit -m "test(refine): rename fleet tests to aggregate

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 10: Rename web crate test files and pipeline tests

**Files:**
- Rename: `crates/web/tests/fleet_api_test.rs` → `aggregate_api_test.rs`
- Modify: `crates/web/tests/api_test.rs`
- Modify: `crates/web/tests/contract_snapshots.rs`
- Modify: `crates/web/tests/fixture_structure_test.rs`
- Modify: `crates/pipeline/tests/failure_policy.rs`
- Modify: `crates/pipeline/tests/service_intent_test.rs`
- Modify: `crates/pipeline/tests/smoke_render.rs`

- [ ] **Step 1: Rename test file**
```bash
mv crates/web/tests/fleet_api_test.rs crates/web/tests/aggregate_api_test.rs
```

- [ ] **Step 2: Update content in all files**

Same rename pattern. Pay attention to `service_intent_test.rs` which has a test named `test_fleet_snapshot_skips_service_omission_and_advisories` — rename to `test_aggregate_snapshot_skips_service_omission_and_advisories`.

- [ ] **Step 3: Run tests**
```bash
cargo test -p inspectah-web -p inspectah-pipeline 2>&1 | tail -20
```

- [ ] **Step 4: Commit**
```bash
git add -A crates/web/tests/ crates/pipeline/tests/
git commit -m "test(web,pipeline): rename fleet tests to aggregate

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 11: Delete and re-record snapshot files

**Files:**
- Delete and regenerate: `crates/web/tests/snapshots/fixture_structure_test__fleet_*.snap` (4 files)
- Update content: ~12 additional `.snap` files with `"fleet"` in content

- [ ] **Step 1: Delete fleet-named snapshot files**
```bash
rm crates/web/tests/snapshots/fixture_structure_test__fleet_*.snap
```

- [ ] **Step 2: Update remaining snapshot files**

For snap files that contain `"fleet": null` or fleet-related data in their content but don't have "fleet" in the filename: update the content to use `"aggregate"` instead. These are typically in `crates/web/tests/snapshots/` and `crates/core/tests/` (if any).

Alternatively, delete ALL snapshot files and re-record:
```bash
cargo insta test --workspace --accept 2>&1 | tail -20
```

- [ ] **Step 3: Verify snapshots are clean**
```bash
cargo insta test --workspace 2>&1 | tail -20
```
Expected: All snapshot tests pass with no pending reviews.

- [ ] **Step 4: Commit**
```bash
git add -A crates/web/tests/snapshots/ crates/core/tests/snapshots/ 2>/dev/null
git commit -m "test: re-record snapshots for fleet→aggregate rename

Assisted-by: Claude Code (Opus 4.6)"
```

---

### ~~Task 12: REMOVED~~

This task was removed during review — `testdata/fleet-e2e.tar.gz` does not exist in the repo. No testdata rename needed.

---

## Phase 4: Frontend (Tang)

### Task 13: Rename frontend component directory and files

**Files:**
- Rename directory: `crates/web/ui/src/components/fleet/` → `crates/web/ui/src/components/aggregate/`
- Rename: `crates/web/ui/src/components/FleetApp.tsx` → `AggregateApp.tsx`

- [ ] **Step 1: Rename directory and top-level component**
```bash
mv crates/web/ui/src/components/fleet crates/web/ui/src/components/aggregate
mv crates/web/ui/src/components/FleetApp.tsx crates/web/ui/src/components/AggregateApp.tsx
```

- [ ] **Step 2: Rename component files inside the directory**

Inside `crates/web/ui/src/components/aggregate/`:
```bash
cd crates/web/ui/src/components/aggregate
mv FleetBanner.tsx AggregateBanner.tsx
mv FleetItemRow.tsx AggregateItemRow.tsx
mv FleetSection.tsx AggregateSection.tsx
mv FleetSidebar.tsx AggregateSidebar.tsx
cd /Users/mrussell/Work/bootc-migration/inspectah
```

Files that don't have "Fleet" in name (`DiffDrawer.tsx`, `ItemDetailPane.tsx`, `VariantView.tsx`, `ZoneGroup.tsx`, `RepoConflictPopover.tsx`) stay as-is but need content updates.

- [ ] **Step 3: Rename test files**
```bash
cd crates/web/ui/src/components/aggregate/__tests__
mv FleetApp.test.tsx AggregateApp.test.tsx
mv FleetApp.integration.test.tsx AggregateApp.integration.test.tsx
mv FleetBanner.test.tsx AggregateBanner.test.tsx
mv FleetContextReadonly.test.tsx AggregateContextReadonly.test.tsx
mv FleetDivergentTracking.test.tsx AggregateDivergentTracking.test.tsx
mv FleetItemRow.test.tsx AggregateItemRow.test.tsx
mv FleetKeyboard.test.tsx AggregateKeyboard.test.tsx
mv FleetPortalIdempotency.test.tsx AggregatePortalIdempotency.test.tsx
mv FleetSection.test.tsx AggregateSection.test.tsx
cd /Users/mrussell/Work/bootc-migration/inspectah
```

- [ ] **Step 4: Update all imports and component names in every file in the directory**

In every `.tsx` file under `components/aggregate/` and `components/aggregate/__tests__/`: rename component names (`FleetBanner` → `AggregateBanner`, etc.), update import paths (`../fleet/` → `../aggregate/`, `./FleetBanner` → `./AggregateBanner`), update CSS class references, update any string literals referencing "fleet".

In `AggregateApp.tsx` (formerly `FleetApp.tsx`): rename the component function, all internal references.

- [ ] **Step 5: Commit**
```bash
git add -A crates/web/ui/src/components/
git commit -m "refactor(ui): rename fleet components to aggregate

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 14: Rename frontend hooks, API client, and CSS

**Files:**
- Rename: `crates/web/ui/src/hooks/useFleetDiff.ts` → `useAggregateDiff.ts`
- Rename: `crates/web/ui/src/hooks/useFleetFocusRecovery.ts` → `useAggregateFocusRecovery.ts`
- Rename: `crates/web/ui/src/hooks/useFleetMutation.ts` → `useAggregateMutation.ts`
- Rename: `crates/web/ui/src/api/fleet-client.ts` → `aggregate-client.ts`
- Rename: `crates/web/ui/src/fleet.css` → `aggregate.css`
- Rename test files in `hooks/__tests__/`: `useFleetDiff.test.ts` → `useAggregateDiff.test.ts`, etc.

- [ ] **Step 1: Rename files**
```bash
mv crates/web/ui/src/hooks/useFleetDiff.ts crates/web/ui/src/hooks/useAggregateDiff.ts
mv crates/web/ui/src/hooks/useFleetFocusRecovery.ts crates/web/ui/src/hooks/useAggregateFocusRecovery.ts
mv crates/web/ui/src/hooks/useFleetMutation.ts crates/web/ui/src/hooks/useAggregateMutation.ts
mv crates/web/ui/src/api/fleet-client.ts crates/web/ui/src/api/aggregate-client.ts
mv crates/web/ui/src/fleet.css crates/web/ui/src/aggregate.css
```

Rename hook test files similarly.

- [ ] **Step 2: Update content in renamed files**

In each hook: rename the exported function (`useFleetDiff` → `useAggregateDiff`, etc.), update imports, doc comments.

In `aggregate-client.ts`: rename all exported functions/classes, update API endpoint paths if they contain `fleet`.

In `aggregate.css`: rename all CSS classes from `.fleet-*` → `.aggregate-*`.

- [ ] **Step 3: Update imports across the frontend**

Files that import from renamed modules need updated import paths:
- `App.tsx`, `App.css` — update CSS import and routing references
- `AppShell.tsx` — fleet routing
- `StatsBar.tsx` — fleet stats and fleet-* test IDs/classes
- `PackageDetail.tsx`, `PackageList.tsx`, `ConfigDetail.tsx` — fleet-aware rendering
- `attentionUtils.ts` — fleet attention logic
- `api/types.ts` — fleet type definitions
- `api/__tests__/fleet-client.test.ts` — rename to `aggregate-client.test.ts` + update content
- `hooks/__tests__/useFleetFocusRecovery.test.tsx` — rename + update content
- `__tests__/App.routing.test.tsx` — update fleet route references

- [ ] **Step 4: Subtree audit**
```bash
grep -ri 'fleet' crates/web/ui/src/ --include='*.ts' --include='*.tsx' --include='*.css' -l
```
Expected: No matches. Fix any stragglers before committing.

- [ ] **Step 5: Commit**
```bash
git add -A crates/web/ui/src/
git commit -m "refactor(ui): rename fleet hooks, API client, and CSS to aggregate

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 15: Rename frontend test fixtures

**Files:**
- Rename directories:
  - `crates/web/ui/e2e/fixtures/fleet/` → `aggregate/`
  - `crates/web/ui/e2e/fixtures/post-responses/fleet-diff/` → `aggregate-diff/`
  - `crates/web/ui/e2e/fixtures/sequences/fleet-toggle-undo/` → `aggregate-toggle-undo/`
- Rename: `e2e/fleet.spec.ts` → `aggregate.spec.ts`
- Modify: JSON fixture content and mock-api references

- [ ] **Step 1: Rename fixture directories and spec file**
```bash
mv crates/web/ui/e2e/fixtures/fleet crates/web/ui/e2e/fixtures/aggregate
mv crates/web/ui/e2e/fixtures/post-responses/fleet-diff crates/web/ui/e2e/fixtures/post-responses/aggregate-diff
mv crates/web/ui/e2e/fixtures/sequences/fleet-toggle-undo crates/web/ui/e2e/fixtures/sequences/aggregate-toggle-undo
mv crates/web/ui/e2e/fleet.spec.ts crates/web/ui/e2e/aggregate.spec.ts
```

- [ ] **Step 2: Rename fleet-view.json inside fixtures**
```bash
mv crates/web/ui/e2e/fixtures/aggregate/fleet-view.json crates/web/ui/e2e/fixtures/aggregate/aggregate-view.json
```

- [ ] **Step 3: Update JSON fixture content**

In all fixture JSON files: rename `"fleet"` keys to `"aggregate"`, update any values that reference "fleet".

- [ ] **Step 4: Update e2e spec and mock-api**

In `aggregate.spec.ts` and `e2e/helpers/mock-api.ts`: update fixture paths, route references (`/api/fleet/*` → `/api/aggregate/*`), test names.

In `e2e/a11y.spec.ts`, `e2e/README.md`, and any other e2e specs: update fleet references.

Also check `crates/web/ui/e2e/fixtures/manifest.json` for fleet references.

- [ ] **Step 5: Run Vitest unit tests**
```bash
cd crates/web/ui && npm test 2>&1 | tail -30
cd /Users/mrussell/Work/bootc-migration/inspectah
```

- [ ] **Step 6: Run Playwright e2e smoke test**
```bash
cd crates/web/ui && npm run test:e2e 2>&1 | tail -30
cd /Users/mrussell/Work/bootc-migration/inspectah
```
Expected: The renamed e2e specs, fixtures, and `/api/aggregate/*` mock routes execute successfully.

- [ ] **Step 7: Subtree audit**
```bash
grep -ri 'fleet' crates/web/ui/e2e/ --include='*.ts' --include='*.json' --include='*.md'
```
Expected: No matches.

- [ ] **Step 8: Commit**
```bash
git add -A crates/web/ui/e2e/ crates/web/ui/src/
git commit -m "test(ui): rename fleet fixtures and e2e specs to aggregate

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Phase 5: Completions & Full Verification (Tang)

### Task 16: Regenerate shell completions

**Files:**
- Regenerate: `completions/inspectah.bash`, `completions/inspectah.fish`, `completions/inspectah.zsh`

- [ ] **Step 1: Regenerate completions**

The completions are clap-generated. After the CLI rename, regenerate them:
```bash
cargo run -p inspectah-cli -- completions bash > completions/inspectah.bash
cargo run -p inspectah-cli -- completions fish > completions/inspectah.fish
cargo run -p inspectah-cli -- completions zsh > completions/inspectah.zsh
```

If the completions subcommand uses a different invocation, check `cargo run -p inspectah-cli -- --help` first.

- [ ] **Step 2: Verify no fleet references remain**
```bash
grep -i fleet completions/*
```
Expected: No output.

- [ ] **Step 3: Commit**
```bash
git add completions/
git commit -m "chore: regenerate shell completions for aggregate subcommand

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 17: Full workspace test and fleet audit

- [ ] **Step 1: Run full Rust test suite**
```bash
cargo test --workspace 2>&1 | tail -30
```
Expected: All tests pass.

- [ ] **Step 2: Run clippy**
```bash
cargo clippy --workspace -- -D warnings 2>&1 | tail -20
```
Expected: Zero warnings.

- [ ] **Step 3: Run frontend Vitest**
```bash
cd crates/web/ui && npm test 2>&1 | tail -30
cd /Users/mrussell/Work/bootc-migration/inspectah
```

- [ ] **Step 4: Final fleet audit — source code**
```bash
grep -ri 'fleet' crates/ --include='*.rs' --include='*.ts' --include='*.tsx' --include='*.css' --include='*.html' -l
```
Expected: No files found. If any remain, fix them.

- [ ] **Step 5: Final fleet audit — JSON fixtures and snapshots**
```bash
grep -ri 'fleet' crates/web/ui/e2e/fixtures/ --include='*.json' -l
grep -ri 'fleet' crates/web/tests/snapshots/ --include='*.snap' -l
grep -ri 'fleet' crates/core/tests/snapshots/ --include='*.snap' -l 2>/dev/null
```
Expected: No files found.

- [ ] **Step 6: Final fleet audit — completions and templates**
```bash
grep -ri 'fleet' completions/ crates/pipeline/templates/ --include='*.bash' --include='*.fish' --include='*.zsh' --include='*.html' -l
```
Expected: No files found.

- [ ] **Step 7: Build the binary and smoke test**
```bash
cargo build --release 2>&1 | tail -5
./target/release/inspectah --help 2>&1 | grep -i aggregate
./target/release/inspectah aggregate --help
```
Expected: `aggregate` appears in the subcommand list; `fleet` does not. `aggregate --help` shows positional tarball args and the `init` subcommand.

- [ ] **Step 8: Commit any stragglers**

If any audit found remaining `fleet` references, fix and commit.

---

### Task 18: Rebuild frontend and verify

`crates/web/ui/dist/` is gitignored, so this is a local verification step only.

- [ ] **Step 1: Rebuild frontend**
```bash
cd crates/web/ui && npm run build 2>&1 | tail -10
cd /Users/mrussell/Work/bootc-migration/inspectah
```
Expected: Build succeeds with no errors. The built output will use the renamed modules/CSS/routes.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Phase 6: Documentation (Mango)

### Task 19: Rename and rewrite docs/ fleet files

**Files:**
- Rename: `docs/explanation/fleet-consensus.md` → `aggregate-consensus.md`
- Rename: `docs/how-to/fleet-aggregation.md` → `aggregation.md`
- Rename: `docs/reference/fleet-manifest.md` → `aggregate-manifest.md`
- Rename: `docs/diagrams/fleet-topology.html` → `aggregate-topology.html`
- Modify (content only, 19 files):
  - `docs/explanation/architecture.md`
  - `docs/explanation/migration-model.md`
  - `docs/explanation/triage-philosophy.md`
  - `docs/getting-started.md`
  - `docs/index.md`
  - `docs/reference/cli.md`
  - `docs/reference/configuration.md`
  - `docs/reference/output-artifacts.md`
  - `docs/reference/snapshot-schema.md`
  - `docs/reference/triage-classification.md`
  - `docs/tutorials/first-migration.md`
  - `docs/contributing/adding-an-inspector.md`
  - `docs/contributing/developer-guide.md`
  - `docs/diagrams/data-flow.html`
  - `docs/diagrams/software-architecture.html`
  - `docs/diagrams/triage-decision-tree.html`
  - `docs/diagrams/user-flow.html`
  - `docs/how-to/baseline-subtraction.md`
  - `docs/how-to/review-and-refine.md`

- [ ] **Step 1: Rename files**
```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
mv docs/explanation/fleet-consensus.md docs/explanation/aggregate-consensus.md
mv docs/how-to/fleet-aggregation.md docs/how-to/aggregation.md
mv docs/reference/fleet-manifest.md docs/reference/aggregate-manifest.md
mv docs/diagrams/fleet-topology.html docs/diagrams/aggregate-topology.html
```

- [ ] **Step 2: Rewrite content in renamed files**

These are semantic rewrites, not just find-replace. "Fleet" as a concept becomes "aggregate" — update headings, body text, examples, cross-references. Ensure internal links to other docs are updated if they pointed to `fleet-*.md` filenames.

- [ ] **Step 3: Update content in the 19 files with fleet references**

For each file: replace "fleet" with "aggregate" in context-appropriate ways. Pay attention to:
- CLI examples: `inspectah fleet` → `inspectah aggregate`
- Module path references: `fleet/` → `aggregate/`
- Cross-references: links to `fleet-consensus.md` → `aggregate-consensus.md`, etc.
- Conceptual language: "fleet analysis" → "aggregate analysis", "fleet aggregation" → "aggregation"
- D3 diagram HTML files: update labels, node names, variable names

- [ ] **Step 4: Verify no broken links**
```bash
grep -r 'fleet' docs/ --include='*.md' --include='*.html' -l
```
Expected: No files found.

- [ ] **Step 5: Commit**
```bash
git add -A docs/
git commit -m "docs: rename fleet to aggregate across all documentation

Assisted-by: Claude Code (Sonnet 4.6)"
```

---

### Task 20: Update README.md

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Update README content**

Rename:
- Subcommand table: `fleet` → `aggregate`
- "Fleet Aggregation" section heading → "Aggregation" (not "Aggregate Aggregation")
- CLI examples: `inspectah fleet init` → `inspectah aggregate init`, `inspectah fleet aggregate` → `inspectah aggregate`
- Workflow diagram references
- Any other "fleet" mentions

- [ ] **Step 2: Verify**
```bash
grep -i fleet README.md
```
Expected: No output.

- [ ] **Step 3: Commit**
```bash
git add README.md
git commit -m "docs: update README for fleet→aggregate rename

Assisted-by: Claude Code (Sonnet 4.6)"
```

---

### Task 21: Update ROADMAP.md and CHANGELOG.md

**Files:**
- Modify: `ROADMAP.md`
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Update ROADMAP.md**

Rename references in **current/future** milestones only. Historical milestone names (completed work) should be left as-is to preserve history. Only rename "fleet" in sections describing ongoing or future work.

- [ ] **Step 2: Update CHANGELOG.md**

Add a new entry under the current unreleased version:

```markdown
### Changed
- Renamed `fleet` subcommand to `aggregate` — all CLI commands, types, modules, and documentation updated
```

**Do NOT rename "fleet" in historical changelog entries.** Those describe what happened at the time and should stay accurate.

- [ ] **Step 3: Verify current surfaces only**
```bash
grep -i fleet ROADMAP.md
```
Expected: Only historical/completed milestones. All current/future references should say "aggregate".

- [ ] **Step 4: Commit**
```bash
git add ROADMAP.md CHANGELOG.md
git commit -m "docs: update ROADMAP and CHANGELOG for fleet→aggregate rename

Assisted-by: Claude Code (Sonnet 4.6)"
```

---

### Task 22: Update active process-docs only

**Scope:** Only rename `fleet` in **active** process-docs (skill files, current designs, nit lists that reference current behavior). Leave historical specs, plans, and release notes untouched — they describe what happened at the time.

**Files to rename:**
- `process-docs/skills/fleet-vs-single-host-behavioral-split.md` → `aggregate-vs-single-host-behavioral-split.md`
- SVG mockup files in `process-docs/designs/mockups/` containing "fleet" in name

**Files to update content (active docs only):**
- `process-docs/designs/fleet-hostname-display.md` — rename file + update content
- `process-docs/nit-list.md` — update references to current behavior
- `process-docs/future-visual-improvements.md` — update references to current behavior

**Leave untouched (historical):**
- `process-docs/plans/2026-06-09-fleet-leaf-intersection.md` — historical plan
- `process-docs/specs/proposed/2026-06-08-fleet-leaf-intersection.md` — historical spec
- `process-docs/nits-2026-03-16.md` — historical nit snapshot
- `process-docs/RELEASE-*.md` — historical release notes

- [ ] **Step 1: Rename active files**
```bash
cd /Users/mrussell/Work/bootc-migration/inspectah/process-docs
mv skills/fleet-vs-single-host-behavioral-split.md skills/aggregate-vs-single-host-behavioral-split.md
mv designs/fleet-hostname-display.md designs/aggregate-hostname-display.md
```

Rename SVG mockups:
```bash
cd designs/mockups
for f in *fleet*; do mv "$f" "${f//fleet/aggregate}"; done
cd /Users/mrussell/Work/bootc-migration/inspectah
```

- [ ] **Step 2: Update content in renamed and active files**

Semantic rewrites in renamed files and active nit/future-improvements docs. Update headings, body text, references where they describe current or future behavior.

- [ ] **Step 3: Verify active surfaces**
```bash
grep -ri fleet process-docs/skills/ process-docs/designs/ process-docs/nit-list.md process-docs/future-visual-improvements.md --include='*.md' --include='*.svg' -l
```
Expected: No files found in active docs. Historical files under `plans/`, `specs/`, `RELEASE-*` may still contain "fleet" — that's intentional.

- [ ] **Step 4: Commit**
```bash
git add -A process-docs/
git commit -m "docs: rename fleet to aggregate in active process-docs

Historical specs, plans, and release notes left as-is.

Assisted-by: Claude Code (Sonnet 4.6)"
```

---

## Final Verification

After all tasks complete, run a repo-wide audit of **runtime and current user-facing surfaces**:

```bash
grep -ri 'fleet' --include='*.rs' --include='*.ts' --include='*.tsx' --include='*.css' --include='*.html' --include='*.json' --include='*.toml' --include='*.bash' --include='*.fish' --include='*.zsh' -l | grep -v node_modules | grep -v dist | grep -v .git-backup | grep -v .superpowers | grep -v target/
```

Expected: No files found in source code, tests, fixtures, templates, or completions.

For docs:
```bash
grep -ri 'fleet' docs/ README.md --include='*.md' --include='*.html' -l
```

Expected: No files found. Historical process-docs and changelogs may still reference "fleet" — that's intentional and correct.
