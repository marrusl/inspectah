# Unified systemd unit collection: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task by task. Steps use checkbox (`- [ ]`) syntax for tracking. Every task is test-first: write the failing test the task names, run it and see it fail for the stated reason, then implement.

**Goal:** one model of systemd unit state for every unit type that exists as a static unit file, replacing the service-only collection, so that a socket, path, timer, target, slice or package-owned storage unit the administrator switched against its preset is carried into the image or omitted with a stated reason.

**Architecture:** proved types in `inspectah-core` (`types::units`) that make an administrator's mount unit unable to reach a `systemctl` line; a `UnitsInspector` in `inspectah-collect` that reads enablement from the links under `/etc/systemd/system` and the effective `[Install]` section; one render-time check in `inspectah-pipeline` (`staged_writes` and `plan_units`) that decides every `systemctl` line against the assembled export; refine and the fleet merge migrated to the new types, with units shown in the existing flat list.

**Tech stack:** Rust, serde, insta, proptest, axum, React and TypeScript with Vitest. One new dev-dependency: `proptest` in `inspectah-core`, for T2's property test (Task 2.1), which the spec requires.

**Spec:** `process-docs/specs/proposed/2026-11-13-unified-systemd-units.md` at `205181a6`. The spec is the contract. Section numbers (§) and test ids (T1 to T29) are the spec's; run ids R1 to R5 are § 13's. Source line numbers below were read at `205181a6`, where no source file differs from `70aa8996`, the commit the spec cites. The symbol name is the identifier; the line is a convenience.

**Branch:** `feat/unified-systemd-units`, cut from `main` by Task 1.1. Phases 1 to 5 land on it and it merges to `main` once, after phase 5 (§ 15).

**Verification convention:** every `cargo` command runs without a pipe, so the exit code is cargo's. The gate for every commit is the pre-commit hook's: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D clippy::all`, `cargo test --workspace`. Frontend tasks add `npx tsc --noEmit` and `npx vitest run` in `crates/web/ui`. A phase is done when its last commit passes all five.

---

## Phase 6 is not in this plan

Phase 6, the Systemd units section of § 11.2, waits on the frontend's own design pass for grouping and presentation. It starts once that pass is done and lands on `main` after this branch merges, as its own change with its own plan. It reads the view response that phases 1 to 5 complete (§ 11.1) and changes no serialized snapshot type, so it needs no schema version. Until it lands, refine shows every unit kind, and the mount, link and default-target rows, in the existing flat list.

## Lanes

- **Rust lane:** the workspace crates `core`, `collect`, `pipeline`, `refine`, `cli`.
- **Frontend lane:** `crates/web/src` (Axum handlers, `web_types.rs`, `adapter.rs`, `aggregate_handlers.rs`) and `crates/web/ui` (TypeScript types, components, hooks, Vitest suites). A different implementer takes these tasks.

A Rust-lane task whose type change stops `crates/web` from compiling makes only the compile-forced edits there: renames and deletions, no new behavior. Every addition in `crates/web` is a frontend-lane task's. Each frontend-lane task says whether it can run in parallel with the Rust task beside it.

## Global constraints

- `SCHEMA_VERSION` and `MIN_SCHEMA` (`crates/core/src/snapshot.rs:21`, `:116`) move from 27 to 28 together, once, in Task 1.2. Later tasks change serialized types again with no further bump (§ 9); each task that changes a serialized type regenerates every fixture and inline literal it breaks, in the same commit.
- No compatibility shim and no migration for a version 27 snapshot (§ 9).
- Every `systemctl` line in the Containerfile comes from `plan_units`; from Task 3.5 on, no other code writes one (§ 10.2).
- Every unit operand is a proved `UnitName`, single-quoted with `single_quote` (`crates/pipeline/src/render/safety.rs:162`), after `--` (§ 10.1).
- Advisory, FIXME and `# Omitted:` text is the spec's text, held in named constants or one `describe` function in the pipeline crate, the one source for the Containerfile and the view response (§ 11.1).
- Named constants for every fixed list the spec names: protected units, `tmp.mount`, runtime updaters, `INCOMPATIBLE_SERVICES`, stock state, search-path directories, host-device templates, instance configuration and data tables, runtime `/var` prefixes, base-image layout lists.
- Tests are built from collector-produced fields wherever the spec says "through the real inspectors" or "through the scan": the real `UnitsInspector` (and `ConfigInspector` where carriage is asserted) over a `MockExecutor`, then `validate`, `redact`, render. A hand-built `UnitSection` is allowed for type and render-rule unit tests.
- The three test rules of § 13 hold for every test: a refusal names what refused; an absence test proves presence first; an omission test has a positive control.
- User-facing text (docs, CHANGELOG, advisory copy beyond the spec's) follows the voice rules: no em dashes, not "shape", not "immutable".
- Commits: `type(scope): description`, imperative; trailer `Assisted-by: <tool> (<model>)`; never a `Co-Authored-By:` trailer. Stage by explicit path.

## Review focus

Inputs the spec implies and no T-test names, most likely first. Each has a test in the owning task.

1. **Escaped and odd unit names reaching every surface.** `foo\x2dbar.service`, `-.slice`, a 255-byte name, and a name with a byte outside the § 6.1 set in a snapshot: the first three render quoted after `--` and round-trip through the view response; the last fails to load naming `UnitName`. Tests in Tasks 1.1 and 2.7.
2. **A unit file that is a symlink chain.** `/etc/systemd/system/a.service -> b.service -> /dev/null`, and a link to a file outside both unit directories: the first is a mask, the second is `linked` and excluded with today's warning (§ 7.2 step 3). Test in Task 2.2.
3. **CRLF and trailing whitespace in unit files and drop-ins.** `[Install]\r\n`, `WantedBy=multi-user.target \r\n` and a `Where=/srv \r\n` read as the plain forms; "holds an `[Install]` section" is true for `[Install]\r\n` and for `  [Install]  `. Tests in Tasks 2.5, 4.1 and 4.3.
4. **The same unit through the scan export and the refine export.** For each render rule task, the Containerfile from `inspectah scan` and from a refine export of the same snapshot with no toggle changed are equal in their unit lines. Tests in Tasks 2.7 and 4.8.
5. **A drop-in directory for a name that does not parse** (`foo.service.d` beside a `foo.servic.d` typo, `*.device.d`): the typo is collected as a drop-in row with its `unit` string and never reaches a `systemctl` line; the `.device.d` row has no state change. Test in Task 2.4.

---

## File structure

| File | Responsibility |
|---|---|
| `crates/core/src/types/units.rs` (new, replaces `types/services.rs`) | `UnitName`, kinds, fragments, `CarriedUnit`, `AdvisoryUnit`, `UnitStateChange`, `UnitSection`, `MountUnitFinding`, `ManualLink`, `AlsoMember`, `DefaultTarget` and their checked constructors |
| `crates/core/src/baseline.rs` | `INCOMPATIBLE_SERVICES` (`:127`) narrowed; new constants for runtime updaters and stock state, and `apply_runtime_updater_default` |
| `crates/collect/src/inspectors/units/mod.rs` (new, replaces `inspectors/services.rs`) | `UnitsInspector`: sources, install state, state changes, mount findings, manual links, default target |
| `crates/collect/src/inspectors/units/resolve.rs` (new) | `Resolved` and the effective-unit resolution, search-path constant (§ 7.1, § 7.2) |
| `crates/collect/src/inspectors/units/install.rs` (new) | The pure `[Install]` reader (§ 7.2 step 4) |
| `crates/collect/src/inspectors/units/instance.rs` (new, phase 5) | `required_config` and `data_paths` derivation and fixed tables (§ 7.5) |
| `crates/collect/src/inspectors/config/walk.rs` | Zero-length unit files (`is_systemd_unit_symlink`, `:241`) and `*.upholds/*` (`:154-155`) excluded |
| `crates/pipeline/src/render/configtree.rs` | `Destination`, `StagedWrites`, `staged_writes` replacing `staged_sources` (`:257`); the stager writes only what it lists |
| `crates/pipeline/src/render/unit_plan.rs` (renamed from `service_intent.rs`) | `plan_units`, `PlannedUnit`, `UnitPlan`, `OmissionReason`, `systemctl_lines`, the unit state section and its advisories |
| `crates/pipeline/src/render/containerfile.rs` | Section order (`:177-198`), scheduled and flatpak requests, tuned line removed |
| `crates/refine/src/projection/` | `unit_states`, `unit_dropins`, `mount_findings`, `manual_links`, `default_target`, `declined` and `withheld` |
| `crates/cli/tests/support/unit_hosts.rs` (new) | Collector-driven host builder over `MockExecutor`, shared with other test crates by `#[path]`, as `sysctl_hosts.rs` is |
| `crates/cli/tests/units_scan_test.rs` (new, absorbs `scan_service_shadow_test.rs`) | End-to-end cases through the real inspectors, one function per spec case and surface |
| `crates/pipeline/tests/unit_plan_test.rs` (renamed from `service_intent_test.rs`) | Render-rule tables over hand-built snapshots |

**Why `service_intent.rs` is renamed rather than replaced beside.** It already holds `systemctl_lines`, `classify_service_presence` and `OmissionReason`, which § 10.2 keeps and extends. A new module beside it would leave two homes for omission reasons. The rename is a `git mv` in Task 2.7 so history follows the file.

---

## Phase 1: the rename (size M; 7 tasks)

Spec: § 6.1, § 6.4, § 9, § 9.1, § 11.3, § 12 (inventory). Tests T1, T3, T4. Scan behavior is unchanged except the section's name; the collector still asks for services only and a state change keeps `owning_package: Option<String>`.

### Task 1.1: Branch, `UnitName` and the kind enums (Rust lane, S)

**Spec:** § 6.1; the `UnitKind`, `OrdinaryKind`, `StorageKind` enums of § 6.2 (the enums only).

**Files:**
- Create: `crates/core/src/types/units.rs`; Modify: `crates/core/src/types/mod.rs`
- Test: `#[cfg(test)]` in `units.rs`

**Interfaces (Produces):** `UnitName` (serde `try_from = "String"`), `UnitName::{kind, as_str, instance, is_template}`, `InstanceName<'a>::template`, `UnitKind`, `OrdinaryKind`, `StorageKind`, `UnitNameError` (thiserror) naming the failed rule.

- [ ] **Step 0:** `git switch -c feat/unified-systemd-units main`.
- [ ] **Step 1: Failing tests first,** one table test: every suffix of § 5 that has a variant parses to its kind; `.device` and `.scope` fail; 0 bytes and 256 bytes fail, 255 pass; a quote, space, newline, `/` and `*` each fail naming the character rule; `foo\x2dbar.service` and `-.slice` pass; `getty@tty1.service` gives instance prefix `getty`, instance `tty1`, template `getty@.service`; `getty@.service` is a template and not an instance. A serde test: a JSON string outside the set fails to deserialize with the `UnitNameError` text. (Review focus 1.)
- [ ] **Step 2: Run** `cargo test -p inspectah-core units::` and see it fail to compile.
- [ ] **Step 3: Implement** the type. Nothing outside the module uses it yet.
- [ ] **Step 4: Gate, commit** `feat(core): add a proved systemd unit name type`.

**Acceptance:** the table test passes; no other crate changes.

### Task 1.2: Rename the section and set schema 28 (Rust lane, L, wide and mechanical)

**Spec:** § 9 (all bullets), § 9.1 rows "Snapshot key", "Per-section host counts", "Section routing", "Config ownership discriminator", "Redaction pass and its guard", "Reports"; T1; T3 core half.

**Files:**
- Modify: `crates/core/src/snapshot.rs` (`:21`, `:33`, `:116`), `crates/core/src/types/services.rs` (types move to `units.rs`; the file is deleted), `crates/core/src/types/config.rs:58` (`ConfigShadow::Units`), `crates/core/src/aggregate/mod.rs:269`, `crates/core/src/aggregate/merge.rs`, `crates/pipeline/src/section_group.rs:60`, `crates/pipeline/src/redaction/engine.rs:978`, `crates/pipeline/src/render/{audit,report,readme,mod}.rs`, every `InspectorId::Services` and `SectionData::Services` site, and every file the compiler names (73 files under `crates/` name the section or a field today). Compile-forced renames only in `crates/web`.
- Create: `testdata/fixtures/snapshot/schema-27-services-section.json`
- Modify (stale-safe literals, per `process-docs/skills/snapshot-schema-versioning.md`): `crates/cli/src/commands/aggregate.rs`, `crates/cli/tests/refine_e2e_test.rs`, `crates/refine/tests/normalize_test.rs`, `crates/core/tests/snapshot_test.rs`, `testdata/e2e-fixture.tar.gz`, `scripts/tests/check-sensitive-tarballs-test.sh`, `docs/reference/snapshot-schema.md`
- Test: `crates/core/tests/snapshot_test.rs`, `crates/refine/tests/tarball_test.rs`, unit tests in `crates/cli/src/commands/aggregate.rs`, `crates/pipeline/src/section_group.rs` tests, `crates/core/tests/aggregate_merge_test.rs`

**Interfaces (Produces):** `InspectionSnapshot.units: Option<UnitSection>`, `UnitSection`, `UnitStateChange` (today's fields, `unit: UnitName`, `shadow_type` and `shadow_rationale` still present until Task 1.4), `UnitState`, `UnitAction`, `InspectorId::Units`, `SectionData::Units`, `ConfigShadow::Units` (serialized `"units"`).

- [ ] **Step 1: Failing tests first.** T1: the version 27 payload (a snapshot with a `services` section holding one service state change and no `units`; the test's doc comment says why it is refused) is refused through `InspectionSnapshot::load`, refine's `from_tarball` (`crates/refine/src/tarball.rs:46`) and `load_snapshot_from_tarball` (`crates/cli/src/commands/aggregate.rs:340`), each asserting the version refusal; a version 28 payload with a `units` section loads through the same three. T3 core half: a config entry a drop-in owns serializes `shadowed_by: "units"`; `compute_section_host_counts` keys `"units"`; `SectionGroup::for_section("units")` routes as `"services"` did and `"services"` no longer routes.
- [ ] **Step 2: Run** the four test targets; expect compile failure, then version and key failures.
- [ ] **Step 3: Implement** the rename and the bump. Add the 27 to 28 bullet to `snapshot-schema-versioning.md` (unreadable and semantically stale: sockets and timers absent with nothing to say so).
- [ ] **Step 4: Gate, commit** `refactor(core)!: rename the services section to units and set schema 28`.

**Acceptance:** T1 and the T3 core assertions pass; `grep -rn '"services"' crates --include='*.rs'` hits only test names and comments, each listed in the commit body.

### Task 1.3: One inventory list (Rust lane, M)

**Spec:** § 6.4 (`UnitInventoryEntry`, `inventory`); § 4 "Lifting the filter alone floods the inventories"; § 12 bullet "`inventory` merges by unit name, keeping the most prevalent state".

**Files:**
- Modify: `crates/core/src/types/units.rs`, `crates/collect/src/inspectors/services.rs:320-324` (push sites), `crates/pipeline/src/render/readme.rs:89-98`, `crates/core/src/aggregate/merge.rs`, consumers the compiler names
- Test: `crates/collect/tests/services_test.rs`, `crates/core/tests/aggregate_merge_test.rs`, `crates/pipeline/src/render/readme.rs` tests

**Interfaces (Produces):** `UnitInventoryEntry { unit: UnitName, state: UnitState, preset: Option<PresetDefault> }`; `UnitSection.inventory` replaces `enabled_units`, `disabled_units`, `preset_matched_units`.

- [ ] **Step 1: Failing tests first.** Collector: a mocked host with one enabled, one disabled and one preset-matched service gives three inventory entries with those states and presets, and no unit appears twice. README: counts read by `unit.kind()` and `state` (today: services only, so the numbers equal today's). Merge: three hosts with a unit `Enabled`, `Enabled`, `Disabled` merge to one entry `Enabled`; a unit on one host only is kept.
- [ ] **Step 2-4:** run (fail), implement, gate.
- [ ] **Step 5: Commit** `refactor(core): replace the three unit name lists with one inventory`.

**Acceptance:** the three tests pass; the three string-list fields no longer exist.

### Task 1.4: Retire orphan-shadow synthesis (Rust lane, S)

**Spec:** § 11.3; § 6.3 paragraph "The fragment inside `unit` replaces ... the `shadow_type` and `shadow_rationale` pair"; T4 (session half).

**Files:**
- Modify: `crates/refine/src/classify.rs:547` (delete `synthesize_orphan_shadows`), `crates/refine/src/session.rs:517-521` (delete the call), `crates/core/src/types/units.rs` (drop the two fields from `UnitStateChange`; they stay on `SystemdDropIn`)
- Test: `crates/refine/tests/session_test.rs`

- [ ] **Step 1: Failing test first,** T4 through `RefineSession::new`: an enabled, preset-matched service with a full shadow has no state change row; its drop-in carries `shadow_type: full_shadow`; `SetInclude(false)` on `ItemId::DropIn { path }` removes the file from the staged tree and `true` restores it.
- [ ] **Step 2-4:** run (fails: a synthetic row exists), implement, gate.
- [ ] **Step 5: Commit** `refactor(refine): stop inventing state changes for shadowed units`.

**Acceptance:** T4's session assertions pass. The frontend shadow count is Task 1.6's.

### Task 1.5: Refine projection and operation rename (Rust lane, M)

**Spec:** § 9.1 rows "View response fields" (the `crates/refine/src/projection/types.rs` half), "Refine operation id", "Refine normalization and synthesis"; § 11.1 first sentence of "Operations".

**Files:**
- Modify: `crates/refine/src/projection/{types,decisions}.rs`, `crates/refine/src/types.rs:58` (`ItemId::Service` becomes `ItemId::Unit`), `crates/refine/src/session.rs:2807-2821` (arm, cascade and membership check kept), `crates/refine/src/normalize.rs:242` (migrated to the new types, behavior unchanged); compile-forced renames in `crates/web`
- Test: `crates/refine/tests/{session_test,normalize_test,serde_test}.rs`

**Interfaces (Produces):** projection fields `unit_states` and `unit_dropins`; `ItemId::Unit { unit: UnitName }` serialized with kind `"Unit"`.

- [ ] **Step 1: Failing tests first:** the projected view serializes `unit_states` and `unit_dropins` and neither old key; `ItemId::Unit` round-trips and `{"kind":"Service",...}` is rejected; toggling `ItemId::Unit` cascades to every drop-in of that name, full shadows included, as `Service` did; the normalizer still locks the four `INCOMPATIBLE_SERVICES` units (narrowed in Task 2.11).
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `refactor(refine): rename the service view fields and operation to units`.

**Acceptance:** the four tests pass.

### Task 1.6: Frontend and handlers follow the rename (frontend lane, M)

**Spec:** § 9.1 rows "Ownership link in the frontend", "View response fields" (`web_types.rs`, `types.ts:704`), "Frontend reads with an empty fallback"; § 11.3 "The count"; T3 handler and frontend halves; T4 count.

**Parallel:** can start beside Task 1.5 against the names in Task 1.5's Interfaces block; it merges after Task 1.5 and its handler test runs against the real projection.

**Files:**
- Modify: `crates/web/src/{web_types,adapter,handlers}.rs`, `crates/web/ui/src/api/types.ts:704` (and the `ItemId` mirror), `crates/web/ui/src/components/{MainContent.tsx:710-714,Sidebar.tsx,GlobalSearch.tsx,shadowOwner.ts:45,ServiceSection.tsx}`, `crates/web/ui/src/hooks/useKeyboard.ts`, `crates/web/ui/src/App.tsx`, every Vitest suite that mocks the view response (26 files under `crates/web/ui/src` name `service_states`)
- Test: `crates/web/tests/api_test.rs`, `crates/web/tests/contract_snapshots.rs`, `crates/web/ui/src/components/__tests__/` (`DecisionItem.shadow-owner.test.tsx`, `GlobalSearch.dropins.test.tsx`, `SectionPlumbing.test.tsx`)

- [ ] **Step 1: Failing tests first.** Through the production handlers: the view response holds `unit_states` and `unit_dropins` and neither old key (T3). Vitest: the section renders a non-empty list from `unit_states` with section id `"units"`; `resolveShadowOwner` resolves `"units"`; the shadow count reads `unit_dropins` entries whose `shadow_type` is `full_shadow` (T4).
- [ ] **Step 2-4:** run, implement, run `npx tsc --noEmit`, `npx vitest run`, cargo gate.
- [ ] **Step 5: Commit** `refactor(web): read the units view fields`.

**Acceptance:** T3 and T4 frontend assertions pass; `grep -rn "service_states\|service_dropins" crates/web` is empty.

### Task 1.7: Phase 1 close (Rust lane, S)

**Spec:** § 9.1 closing paragraph (the string search); repo skill rule.

- [ ] Search the tree for the quoted strings `"services"`, `service_states`, `service_dropins`; rename each remaining hit or list it, with the reason it stays, in the phase report.
- [ ] Update `process-docs/skills/codebase-layout.md` (`types/units.rs`, snapshot field `units`), `process-docs/skills/service-omission-model.md` (names), `docs/reference/inspector-coverage.md:103` (snapshot field `units`).
- [ ] **CHANGELOG.md**, under `## [Unreleased]`, **Changed**: the snapshot section is `units`, schema 28, version 27 snapshots must be re-scanned; refine no longer shows a state row for a preset-matched unit whose only change is a full shadow.
- [ ] Gate; commit `docs: record the units rename in the layout and coverage docs`.

**Phase 1 ends:** a scan writes `units` at schema 28 and refuses 27; the Containerfile is unchanged; refine shows a full shadow on its drop-in row only.

**Review gate 1:** implementation review of the phase before Task 2.1 starts. Focus: § 9.1 rows each migrated, no string consumer left reading an empty fallback, the T1 payload states why it is refused.

---

## Phase 2: every kind (size L, upper end and above; 17 tasks)

Spec: § 5, § 6.2, § 6.3 (without `also`, `install_drop_ins`, `vendor_drop_ins` and the instance fields), § 6.5, § 6.7, § 7.1 sources 1 and 2, § 7.2 steps 2 and 3, § 7.4, § 7.7, § 7.8, § 10.1, § 10.2 (staging, rules 1 to 4, the fragment half of rule 5), § 10.3, § 10.4, § 10.6, § 10.7, § 11.1 (`declined`, `withheld`, `mount_findings`), § 12. Tests T2, T5 to T13, T15 to T18. Local timers keep today's deferral to the scheduled section (§ 15).

Before Task 2.1: merge `main` into the branch and run the gate.

### Task 2.1: Carried and advisory types (Rust lane, M)

**Spec:** § 6.2 (all types and `classify`), § 6.3 (`UnitStateChange` checked constructor, the two refusals), § 6.5 (`MountUnitFinding`), § 7.6 (`TargetUnit` only); T2 type cases.

**Files:** Modify `crates/core/src/types/units.rs`. Test: `#[cfg(test)]` there and `crates/core/tests/snapshot_test.rs`.

**Interfaces (Produces):** `OrdinaryUnitName`, `StorageUnitName`, `TargetUnitName`, `UnitFragment`, `UnitSubject`, `CarriedUnit`, `TargetUnit`, `AdminStorageFragment`, `AdvisoryUnit`, `Classified`, `UnitSubject::classify`, `MountUnitFinding`, `UnitSection.mount_findings`. Added beside the phase 1 fields; `UnitStateChange.unit` switches to `CarriedUnit` in Task 2.2.

- [ ] **Step 1: Failing tests first,** T2 type cases: for each storage kind, a `state_changes` entry with `local`, `shadowed` or `absent`, or `vendor` with a null owner, fails to load naming `classify`; `vendor` with an owner loads, and fails in `mount_findings`; `TargetUnit` refuses `cockpit.socket` and a packaged `tmp.mount` and round-trips a `.target`; a template loads `masked` and fails `enabled`, likewise an `absent` fragment; a `file` that is neither the unit nor its template fails. A proptest: `classify` is total over generated names and fragments and stable through serialize then deserialize.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(core): add carried and advisory unit types`.

**Acceptance:** T2 type cases pass. (T2's signature test is Task 2.7's.)

### Task 2.2: Fragment resolution and owner lookup (Rust lane, M)

**Spec:** § 7.2 steps 2 and 3 (two directories, instance falls back to template, form test, size-zero mask), § 7.8; § 6.3 (`unit: CarriedUnit` replaces `owning_package`); T5 second sentence; T11 collector half for services.

**Files:** Modify `crates/collect/src/inspectors/services.rs` (`populate_owning_packages`, `:113`), `crates/core/src/types/units.rs` (switch the field), `crates/pipeline/src/render/service_intent.rs` (`classify_service_presence`, `:359`, reads the owner from the fragment; mechanical), consumers the compiler names. Create `crates/collect/src/inspectors/units/resolve.rs` (the `Resolved` enum with `File`, `Masked`, `AliasOf`, `NotFound`; `Unsupported` arrives in Task 4.2). Test: `crates/collect/tests/services_test.rs`.

- [ ] **Step 1: Failing tests first:** a local unit and a vendor unit whose owner lookup fails get `Local` and `Vendor { owning_package: None }` (T5); a `/dev/null` link and a zero-length file over a vendor unit are `Masked { hidden: Vendor }`; a comment-only file is a regular fragment; a two-hop link ending at `/dev/null` is a mask and a link to a file outside both directories is `linked` with today's warning (Review focus 2); a storage unit's owner is looked up before classification.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(collect): resolve each unit's fragment and owner`.

**Acceptance:** the listed cases pass; render output for existing service fixtures is unchanged (insta snapshots unmodified).

### Task 2.3: `UnitsInspector` over every kind (Rust lane, M)

**Spec:** § 5 table, § 7.1 sources 1 and 2, § 7.4 (all of it except the instance preset and stock-state list, which are Task 4.4); § 4 "The `dbus-` filter"; T5 first sentence, T7.

**Files:** `git mv crates/collect/src/inspectors/services.rs crates/collect/src/inspectors/units/mod.rs`; `ServicesInspector` becomes `UnitsInspector`; `list-unit-files` loses `--type=service` (`:226`); the `@` skip at `:303` and `is_real_service` (`:25-36`) become the name gate; `crates/collect/src/executor/mock.rs` keys follow `process-docs/skills/mock-executor-key-format.md`. `git mv crates/collect/tests/services_test.rs crates/collect/tests/units_test.rs`.

- [ ] **Step 1: Failing tests first:** T5 (`cockpit.socket` enabled against a `disable` preset: one state change of kind `Socket`, `Vendor` fragment, owner from the mocked answers); T7 (`dbus-broker.service` kept, a `dbus-org.` alias dropped; a local enabled unit with no preset rule is a state change; a disabled local unit is not; `generated` and each runtime state give none, with today's warning); a masked template is a `Masked` state change on the template name; an unmasked template is skipped; a storage entry that classifies advisory goes to `mount_findings`, never `state_changes`.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(collect): collect unit state for every unit kind`.

**Acceptance:** T5 and T7 pass.

### Task 2.4: Masks, drop-ins and shadows, one owner per path (Rust lane, M)

**Spec:** § 7.7 (table and bullets except the `*.upholds/*` exclusion, which is Task 4.5); § 4 "A masked unit is recorded twice"; T11 collector half.

**Files:** Modify `crates/collect/src/inspectors/units/mod.rs` (shadow scan, today `:456-485`; drop-in scan, today `:763`), `crates/collect/src/inspectors/config/walk.rs:241` (`is_systemd_unit_symlink` also excludes a zero-length regular file with a unit suffix), `crates/pipeline/src/collect.rs:274` (`shadow_service_owned_paths`, renamed for units). Test: `crates/collect/tests/units_test.rs`, `crates/collect/tests/config_test.rs`.

- [ ] **Step 1: Failing tests first,** T11 collector cases: each mask form over a vendor unit gives one `Masked` state change, no drop-in entry and no config entry; a mask with no vendor file is `Absent` and carried; a masked template under its own name; for each storage kind and both mask forms, a package-owned vendor file gives a carried mask and an unowned one or none gives a mount finding with state `Masked`. A regular non-empty storage file is a config entry and a mount finding, never a full shadow. Drop-ins are collected for every parsed name, `*.device.d`, `*.scope.d` and generated parents; a `.d` directory whose name does not parse (`foo.servic.d`) is a drop-in row with its `unit` string and no state change (Review focus 5).
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `fix(collect): give masks, shadows and drop-ins of every kind one owner`.

**Acceptance:** T11 collector cases pass; a zero-length unit file appears in neither `config.files` nor `drop_ins`.

### Task 2.5: Mount findings, extracted fields and the redaction guard (Rust lane, M)

**Spec:** § 6.5 (which units produce a finding, `mount_point`, `nofail`, no `What=`), § 6.7 (table, guard additions for the types that exist now: `UnitStateChange`, `UnitInventoryEntry`, `MountUnitFinding`); T10.

**Files:** Modify `crates/collect/src/inspectors/units/mod.rs`, `crates/pipeline/src/redaction/engine.rs:978`, `crates/pipeline/src/render/mod.rs:305` (`every_field_of_a_content_bearing_type_is_classified`). Test: `crates/pipeline/tests/redaction_new_surfaces_test.rs`, `crates/cli/tests/units_scan_test.rs` (new).

- [ ] **Step 1: Failing tests first:** T10 (a local mount unit whose `What=` holds a token the generic pattern matches: the token is absent from the whole serialized snapshot after scan and after a refine export, and from the view response); `Where=srv` (relative) gives `mount_point: None`; `Where=/srv \r\n` gives `/srv` and `Options=defaults,nofail\r\n` gives `nofail: true` (Review focus 3); an unowned vendor mount unit gives a finding only when enabled or masked. The field guard destructures the three types with no `..`.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(collect): record the administrator's mount units as findings`.

**Acceptance:** T10 passes on both export routes.

### Task 2.6: `staged_writes` and `Destination` (Rust lane, M)

**Spec:** § 10.2 `Destination`, `StagedWrites`, `WithheldWhy` (`ZeroLengthUnitFile`, `Conflict`), "`staged_writes` is the stager's decision" bullets, rule 1; § 4 "The stager skips writes it cannot make"; T15 conflict and failed-write cases; T12 staging cases.

**Files:** Modify `crates/pipeline/src/render/configtree.rs` (`validate_path` `:35`, `safe_write_file` `:158`, `staged_sources` `:257`, the drop and refuse tests at `:375-388`; `unstaged_files` carries withheld files to the FIXME); `crates/pipeline/src/render/containerfile.rs` callers. Test: `crates/pipeline/tests/configtree_single_source_test.rs`.

**Interfaces (Produces):** `Destination::new(&str) -> Result<Destination, PathError>`; `pub(crate) fn staged_writes(&InspectionSnapshot) -> StagedWrites<'_>`; the stager writes `writes` and nothing else and returns an I/O error on any failed write.

- [ ] **Step 1: Failing tests first:** config entries for `/etc/systemd/system/worker.service/extra.conf` and `/etc/systemd/system/worker.service`, in either order: neither is in `writes`, both are withheld as `Conflict` and named in the FIXME; without the first, the second is written (T15). Two entries at one destination: both withheld. `/etc/systemd/./system//bootc-x.timer` and the plain spelling are one destination; `..` is refused. A zero-length file at `/etc/systemd/system/<name>.service` offered as a config entry and as a drop-in, including with a `unit` field naming another unit: withheld as `ZeroLengthUnitFile` (T12 staging half). A write into a read-only directory fails the export.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `fix(pipeline): decide every staged file once, and fail on a write that does not land`.

**Acceptance:** the listed cases pass; `safe_write_file` no longer returns silently.

### Task 2.7: `plan_units`, quoting and the fragment half of rule 5 (Rust lane, L)

**Spec:** § 10.1 (one function, one `RUN` per verb, kind then name order, single quotes and `--`, `systemctl_lines(&[PlannedUnit])`), § 10.2 `PlannedUnit`, `UnitPlan`, `plan_units`, rule 5 fragment table (`Local`, `Shadowed`, `Vendor`/`PackagedStorage`), the aggregate content-variant sentence for staged unit files, `OmissionReason::{UnitFileNotStaged, InstallEvidenceChanged}`, "The no-RPM path runs every rule"; § 4 "Unit names reach `RUN` lines bare" for the unit section; T2 signature test, T6, T13 quoting, T15 (except the conflict case), T18 (d).

**Files:** `git mv crates/pipeline/src/render/service_intent.rs crates/pipeline/src/render/unit_plan.rs` (`render_service_intent` `:503`, `systemctl_lines` `:292`, `render_without_rpm` `:660` and its early return at `:519-522` removed); `git mv crates/pipeline/tests/service_intent_test.rs crates/pipeline/tests/unit_plan_test.rs`. Test also `crates/cli/tests/units_scan_test.rs`, `crates/pipeline/tests/instruction_injection_test.rs`.

**Interfaces (Produces):** `pub fn plan_units(&InspectionSnapshot, &StagedWrites<'_>) -> UnitPlan`; `PlannedUnit` with private fields and no public constructor; `UnitPlan.declined: Vec<(UnitName, UnitAction, OmissionReason)>`; `OmissionReason::describe(&self) -> String`, the one text source.

- [ ] **Step 1: Failing tests first:** T2 signature (a compile-fail doctest that `PlannedUnit` cannot be built outside the module, and `systemctl_lines` accepts `&[PlannedUnit]` only); T6 (the socket snapshot yields the enable; the owner excluded yields `# Omitted: cockpit.socket (...)`); T13 quoting (`probe@a\x2db.service` and `-.slice` render as quoted operands after `--`, and so does a 255-byte name, each surviving a refine export unchanged: Review focus 1); T15: a local enabled socket whose config entry is excluded gives no staged file, the `UnitFileNotStaged` line and no enable, each of the two toggles driven through `RefineSession` alone and shown to change the staged tree or the Containerfile (as `process-docs/skills/cross-inspector-shadow-passes.md` requires), and the same with no package section; a `Shadowed` row whose shadow is excluded gives no `enable` and no `disable` with `InstallEvidenceChanged`; a `Vendor` row with a unit file staged at its name by another row gives the same; each returns when the export matches again. T18 (d): two hosts with a full shadow of one unit and different content give `InstallEvidenceChanged` whichever variant is selected. Review focus 4: scan and refine export give equal unit lines for the T6 snapshot.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(pipeline): plan every unit command against the staged export`.

**Acceptance:** the listed tests pass. The refine row half of T15 ("shows the reason and the path") is Task 2.13's.

### Task 2.8: Rules 2, 3 and 4, and the units the target needs (Rust lane, M)

**Spec:** § 10.2 rules 2, 3, 4 and `OmissionReason::{StorageFileStaged, TargetNeedsUnit, MaskPathOccupied}`; § 10.3 (protected names constant, `tmp.mount` constant, FIXME text, collector `attention_reason`, every route); T8 render cases, T12 render cases, T16.

**Files:** Modify `crates/pipeline/src/render/unit_plan.rs`, `crates/core/src/baseline.rs` (both constants), `crates/collect/src/inspectors/units/mod.rs` (attention reason). Test: `crates/pipeline/tests/unit_plan_test.rs`, `crates/cli/tests/units_scan_test.rs`.

- [ ] **Step 1: Failing tests first:** T12 table over the protected names for `disable` and `mask`: the FIXME and no command, `enable` carried; a masked `tmp.mount` gives its FIXME while its `enable` and `disable` are carried and a masked `dev-hugepages.mount` is carried; a zero-length file for a protected unit from a scan and imported as a drop-in and as a config entry (with a `unit` field naming another unit, an interior `.`, a doubled separator) gives no `mask` and no staged file. T8: a package-owned mount unit enabled against preset is in `RUN systemctl enable`, masked in `mask`; with an administrator's file staged at its name, `StorageFileStaged`. T16: a mask and a staged unit file of one name give `MaskPathOccupied`; excluding the file brings the mask back.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(pipeline): refuse commands the image cannot take`.

**Acceptance:** T12 (except its view-response sentence, Task 2.13), T8 render cases and T16 pass.

### Task 2.9: Section order, every `systemctl` site quoted, README sentences (Rust lane, S)

**Spec:** § 10.7 (unit state section directly after the Configuration Files section; generated units keep their own sections); § 15 phase 2 "quoting and `--` at every `systemctl` site"; § 10.1 "the exported README states each in a sentence" (first boot keeps it; `disable` removes the administrator's links and nothing more); T13 order.

**Files:** `crates/pipeline/src/render/containerfile.rs:177-198` (order), `:1400` (timer enable), `:1652` (flatpak), `:1885` (tuned); `crates/pipeline/src/render/readme.rs`. Test: `crates/pipeline/tests/smoke_render.rs`, `crates/cli/tests/units_scan_test.rs`.

- [ ] **Step 1: Failing tests first:** T13 order (a local service with no paired timer: the config `COPY` and the enable both exist, and the enable follows); the timer, flatpak and tuned lines are quoted after `--`; the README holds both sentences when the export has a unit state section.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `fix(pipeline): run unit commands after the files they name`.

**Acceptance:** T13 passes in full.

### Task 2.10: Mount advisories (Rust lane, M)

**Spec:** § 10.6 (table, layout constants beside `VAR_SYMLINKED_TOPLEVELS` at `crates/pipeline/src/render/selinux.rs:29`, the unit file stays a config entry); T8 advisory cases, T9.

**Files:** Modify `crates/pipeline/src/render/unit_plan.rs`, `crates/pipeline/src/render/selinux.rs`. Test: `crates/pipeline/tests/unit_plan_test.rs`, `crates/cli/tests/units_scan_test.rs`.

- [ ] **Step 1: Failing tests first:** T9 (a local `srv.mount`: the `/var` rename line; `Where=/opt/vendor`: the `/opt` line and no rename line; under `/usr`: the `/usr` line; the unit-file line follows the config row's toggle; no instruction names any of them); T8 advisory cases (a local unit of each storage kind enabled, and a vendor one whose owner lookup fails: advisory and no instruction; a `tmp.mount` full copy in `/etc`: the advisory with the `tmp.mount` line, and its config row's toggle removes and restores the staged file); a mount without `nofail` gets the `nofail` line and a swap with it does not.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(pipeline): explain the administrator's mount units instead of carrying them`.

**Acceptance:** T8 and T9 pass in full.

### Task 2.11: Runtime package updaters (Rust lane, M)

**Spec:** § 10.4 (constant, `apply_runtime_updater_default`, collector call, the `# Omitted:` text, `packagekit-offline-update.service` removed from `INCOMPATIBLE_SERVICES`, the disjointness test); § 12 "The exception"; T17, T18 (b).

**Files:** Modify `crates/core/src/baseline.rs:127-143`, `crates/collect/src/inspectors/units/mod.rs`, `crates/core/src/aggregate/merge.rs` (call after `merge_items`, near `:557`), `crates/refine/src/normalize.rs:242`. Test: `crates/core/src/baseline.rs` tests (`:608` expects 3), `crates/refine/tests/normalize_test.rs`, `crates/core/tests/aggregate_merge_test.rs`, `crates/cli/tests/units_scan_test.rs`.

- [ ] **Step 1: Failing tests first:** T17 (each name: a state change with `include: false` and the reason, the `# Omitted:` comment, no enable; through the production refine loader the row is not locked, `SetInclude` yields the enable, the include survives export and reload; the two constants share no entry); T18 (b) (two hosts each with `dnf-automatic.timer` excluded merge to an excluded row; an explicit include yields the enable).
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(collect): exclude in-place package updaters by default`.

**Acceptance:** T17 and T18 (b) pass.

### Task 2.12: Fleet merge of the new types (Rust lane, M)

**Spec:** § 12 bullets for `UnitStateChange` identity, `mount_findings` by unit name, "A merged export is validated at render"; T18 (a), (c).

**Files:** Modify `crates/core/src/aggregate/merge.rs` (`:147-150` identity unchanged; `:1533-1542` lists). Test: `crates/refine/tests/aggregate_session_test.rs`, `crates/core/tests/aggregate_merge_test.rs`.

- [ ] **Step 1: Failing tests first:** T18 (a) (one host with a package-owned enabled mount unit and one with an administrator's full copy, merged and taken through `RefineSession`: while the file's config row is included, no enable and the advisory present whatever the unit row's toggle and cascade do; excluding the file yields the enable); T18 (c) (a mask on one host and a local unit file of that name on another: `MaskPathOccupied`). Mount findings from two hosts merge to one per name with `aggregate` set.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(core): merge unit fragments and mount findings across hosts`.

**Acceptance:** T18 passes in full (with Tasks 2.7 and 2.11).

### Task 2.13: Refine rows carry the plan's reasons (Rust lane, M)

**Spec:** § 11.1 `unit_states[]` fields that exist now (`unit`, `kind`, `instance_of`, `current_state`, `default_state`, `fragment`, `unit_file_path`, `owning_package`, `include`, `locked`, `triage`, `attention_reason`, `declined`), `unit_dropins[]` `kind`, `mount_findings[]` (all fields, `advisories[]` from the § 10.6 strings), `withheld` on the offering config or drop-in row, the paragraph on results caused by another row; T12 last sentence, T15 last clause.

**Files:** Modify `crates/refine/src/projection/{types,decisions}.rs` (calls `staged_writes` and `plan_units` on the session snapshot). Test: `crates/refine/tests/session_test.rs`, `crates/refine/tests/export_contract_test.rs`.

**Interfaces (Produces):** projection types `UnitStateView`, `DeclinedView { renders: Omitted|Fixme|Advisory, reason: <variant name>, text: String }`, `WithheldView`, `MountFindingView`.

- [ ] **Step 1: Failing tests first:** each T12 refusal appears as a structured `declined` on its row for a row whose `attention_reason` is null; the T15 `Shadowed` case shows `InstallEvidenceChanged` with the shadow's path on the unit row while declined; a withheld conflict file shows `withheld: Conflict` on its config row; a mount finding row has no `include`.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(refine): show why the export declines a unit command`.

**Acceptance:** the listed assertions pass.

### Task 2.14: Flat list shows every kind and its reasons (frontend lane, M)

**Spec:** § 11.1 (the same fields, as read by the frontend); § 15 phase 2 "Refine shows every kind in the existing flat list".

**Parallel:** can run beside Task 2.13 against its Interfaces block; merges after it.

**Files:** Modify `crates/web/src/{web_types,adapter}.rs`, `crates/web/ui/src/api/types.ts`, `crates/web/ui/src/components/{ServiceSection.tsx,MainContent.tsx,ConfigDetail.tsx}`. Test: `crates/web/tests/api_test.rs`, Vitest suites beside each component.

- [ ] **Step 1: Failing tests first:** a socket row renders with its kind; a declined row renders the `text` with its `renders` class; a config row with `withheld` renders the reason; mount findings render as rows with no toggle and their advisories; a row whose `attention_reason` is null still shows `declined`.
- [ ] **Step 2-4:** run, implement, `npx tsc --noEmit`, `npx vitest run`, cargo gate. **Step 5: Commit** `feat(web): show every unit kind in the units list`.

**Acceptance:** the assertions pass; no new section or grouping (that is phase 6).

### Task 2.15: Phase 2 close (Rust lane, S)

**Spec:** § 13 "Real-host evidence" (the facts measured the same day and not yet recorded; "Phase 2 adds them"); repo skill rule; changelog policy.

- [ ] Add to `process-docs/skills/real-host-validation-runs.md` § Measured, 2026-11-13, the facts § 13 lists as not yet recorded, copied as the spec states them: the stock and drifted survey (what differed from preset, the one enabled instance), the rows a package install makes through `Also=`, that packaged units with `Also=` have nothing under `/etc` but links, the `cups` enable then disable pair, the contents of the four template units of § 7.5, the upheld-unit restart loop, `/tmp` on a package-mode host with `tmp.mount` masked.
- [ ] Update `process-docs/skills/service-omission-model.md` (rules 1 to 5, kinds), `containerfile-value-quoting.md` (unit operands are quoted after `--`, no refusal), `cross-inspector-shadow-passes.md` (masks are states; storage files belong to config), `codebase-layout.md` (`inspectors/units/`, `render/unit_plan.rs`), `docs/reference/inspector-coverage.md` (every kind, mount findings, the protected list).
- [ ] **CHANGELOG.md** under `## [Unreleased]`: **Added** unit state for sockets, paths, timers, targets, slices and package-owned mount, automount and swap units; mount advisories. **Changed** unit commands are quoted and run after the configuration `COPY`; in-place package updaters are excluded by default. **Fixed** a masked unit is no longer also recorded as an empty full shadow; a local service's enable no longer runs before its file is copied; `dbus-broker.service` is no longer dropped; a staged file that cannot be written fails the export.
- [ ] Gate; commit `docs: record unit state for every kind in skills and coverage`.

**Phase 2 ends:** a scan carries enable, disable and mask for every ordinary kind and package-owned storage unit, after the configuration `COPY`, quoted; reports the administrator's mount units; and declines, with a stated reason, any command the export would not reproduce.

**Review gate 2:** implementation review before Task 2.16. Focus: **rule 5's fragment half and rules 1, 2 and 4 against `staged_writes`, and against the fleet merge** (T15, T16, T18): a toggle on one row that changes another row's command, a merged snapshot whose rows are valid alone and not together, and a path spelled two ways.

### Task 2.16: Run R1 after phase 2 (real host, M)

**Spec:** § 13 R1; § 15 real-host table, "After phase 2".

**Needs:** the branch build at the end of phase 2; the `rhel9` and `rhel10` VMs, each reverted to its clean snapshot; the driftify **standard** profile, which sets `cockpit.socket` against its preset (enabled on RHEL 9, disabled on RHEL 10); the project's VM operating notes, supplied at dispatch.

- [ ] On each VM: record `systemctl is-enabled cockpit.socket` in the unmodified base image; run driftify standard; scan; confirm the Containerfile holds the socket's line in the unit state section after the configuration `COPY`; build; confirm the image reads the host's state. Record under a new `### Measured, <date>: cockpit.socket end to end` in `process-docs/skills/real-host-validation-runs.md`. Revert both VMs.
- [ ] Commit `docs(skills): record the cockpit.socket run on the units branch`.

**Gate:** phase 3 does not start until the image reads the host's state on both releases. A failure is a phase 2 defect, fixed and reviewed before phase 3. See Spec questions 1 on the run's stated direction.

### Task 2.17: Run R3 after phase 2 (real host, L)

**Spec:** § 13 R3 (a) to (f); § 15 "R3 (a) to (c) can change how 'off' has to be rendered"; § 17 "Evidence that would change the design".

**Needs:** reverted `rhel9` and `rhel10` VMs; the phase 2 branch build for (a); probe images built from `rhel9/rhel-bootc:9.8` and `rhel10/rhel-bootc:10.2` for (b) to (f), with no inspectah build; the VM operating notes. (a) uses the driftify **standard** profile on `rhel9`, whose cases should render an `enable` (`cockpit.socket`), a `disable` (`kdump`, planted at minimal) and a `mask` (`bluetooth`); if the rendered Containerfile lacks one of the three verbs, plant one by hand and record it. (b) to (f) need no driftify profile.

- [ ] Run each sub-case as § 13 states it, booting with `bootc install to-existing-root` and reading results through a oneshot unit and the console, as the notes describe. Record each under a dated Measured subsection. Revert both VMs.
- [ ] Commit `docs(skills): record the units boot checks`.

**Gate:** may run beside phase 3. A result on § 17's list (presets applied at first boot on any route; `/tmp` writable on RHEL 9 with `tmp.mount` masked) stops the branch before phase 4 for a spec change by the project owner. Other results are recorded and change nothing.

---

## Phase 3: one control per fact (size M; 6 tasks)

Spec: § 8, § 10.2 "What the plan takes" item 3 and the tuned decision, § 11.1 `scheduled_tasks` row, § 9.1 "Aggregate timer control". Tests T19 to T21.

Before Task 3.1: merge `main` into the branch and run the gate.

### Task 3.1: Timers become a description; the second writer goes (Rust lane, M)

**Spec:** § 8 (table, single-control rule, `SystemdTimer` reduced to `name`, `on_calendar`, `exec_start`, `description`, `source`, `path`, `aggregate`; the local-timer loop and enable removed; `config_tree_units` keeps its generated half); § 6.7 rows for `SystemdTimer`; T19.

**Files:** Modify `crates/core/src/types/scheduled.rs`, `crates/collect/src/inspectors/scheduled.rs:772-826`, `crates/pipeline/src/render/configtree.rs:440-449` (removed), `crates/pipeline/src/render/containerfile.rs:1328-1332`, `:1382-1386` (removed), `crates/pipeline/src/render/unit_plan.rs` (`config_tree_units`, today `service_intent.rs:308-329`), the redaction scheduled pass and field guard. Test: `crates/cli/tests/units_scan_test.rs`, `crates/collect/tests/scheduled_test.rs`.

- [ ] **Step 1: Failing test first,** T19 through the real inspectors: a local `.timer` and paired `.service`, timer `disabled`: no enable names the timer; excluding the timer's config row removes the timer file and leaves the service file; excluding the service row too leaves neither; no other writer restores either. Positive control: the timer `enabled` gives its enable from the unit state section.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `fix(scheduled): give a local timer one control for its file and one for its state`.

**Acceptance:** T19 passes; `SystemdTimer` has no `disposition`, `locked`, `timer_content` or `service_content`.

### Task 3.2: Remove the timer operation; link scheduled rows to their unit (Rust lane, S)

**Spec:** § 8 last bullet (refine half), § 11.1 `scheduled_tasks` timers (`unit`, `unit_row`), § 11.1 Operations "`ItemId::SystemdTimer` is removed"; T20 refine half.

**Files:** Modify `crates/refine/src/types.rs:117`, `crates/refine/src/session.rs:2190-2193`, `crates/refine/src/projection/`; compile-forced deletions in `crates/web/src/handlers.rs:949-958` and `crates/web/src/aggregate_handlers.rs:1158-1166`. Test: `crates/refine/tests/{serde_test,session_test}.rs`.

- [ ] **Step 1: Failing tests first:** a request naming `SystemdTimer` is rejected at deserialization; a scheduled timer row has `unit` and `unit_row` and no include flag; the timer's `unit_states` row keeps its flag.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `refactor(refine): remove the timer toggle that applied nothing`.

### Task 3.3: Timer control gone from the UI and aggregate view (frontend lane, S)

**Spec:** § 8 last bullet (aggregate handler and `AggregateItemRow.tsx`), § 9.1 "Aggregate timer control"; T20 handler and frontend half.

**Parallel:** can run beside Task 3.2 against its Interfaces; merges after it.

**Files:** `crates/web/src/{handlers,aggregate_handlers}.rs`, `crates/web/ui/src/api/types.ts:864`, `:1083`, `crates/web/ui/src/components/aggregate/AggregateItemRow.tsx`, the scheduled tasks component. Test: `crates/web/tests/{api_test,aggregate_api_test}.rs`, Vitest.

- [ ] **Step 1: Failing tests first:** neither the view nor the aggregate response offers an include flag on a scheduled timer row; the bulk operation emits no `SystemdTimer`; a scheduled timer row links to its unit row when `unit_row` is true.
- [ ] **Step 2-4:** run, implement, frontend checks, gate. **Step 5: Commit** `refactor(web): drop the scheduled timer toggle and link to the unit row`.

### Task 3.4: Generated units and flatpak are plan requests (Rust lane, M)

**Spec:** § 10.2 "What the plan takes" items 1 to 3, `WithheldWhy::GeneratedNameCollision`, `OmissionReason::GeneratedNameCollision`, the flatpak rescan case; § 10.7 second paragraph; § 11.1 "A declined generated unit is shown on its scheduled or flatpak row"; T21 generated and flatpak cases.

**Files:** `crates/pipeline/src/render/configtree.rs` (generated timer and service files and `flatpak-provision.service` at `:586` enter `staged_writes`), `crates/pipeline/src/render/containerfile.rs` (`scheduled_tasks_section_lines`, the container workloads section at `:1652`), `crates/pipeline/src/render/unit_plan.rs`, `crates/refine/src/projection/`. Test: `crates/cli/tests/units_scan_test.rs`, `crates/refine/tests/export_parity_test.rs`.

- [ ] **Step 1: Failing tests first:** the flatpak enable is quoted and follows its own `COPY`; a generated cron timer and service named for a collected, enabled vendor timer, through the scan export and the refine export: the request is declined with a FIXME, neither generated file is in the staged tree, and the vendor timer's enable is carried against the package's unit; the declined request shows on the scheduled row.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(pipeline): plan generated and flatpak units like collected ones`.

### Task 3.5: The tuned enable goes; every line is planned (Rust lane, S)

**Spec:** § 10.2 tuned decision; § 8 single-control rule; T21 tuned case and the full-scan assertion.

**Files:** `crates/pipeline/src/render/containerfile.rs:1885`. Test: `crates/cli/tests/units_scan_test.rs`.

- [ ] **Step 1: Failing test first,** T21: a snapshot with flatpak applications, tuned activation, a generated timer and collected state changes: every `RUN systemctl` instruction in the rendered Containerfile matches a `PlannedUnit`, found by scanning the output; the kernel section writes the tuned profile and no `systemctl` line; `tuned.service` enabled against a `disable` preset is enabled from the unit state section, and omitted when the `tuned` package is excluded.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `fix(pipeline): leave tuned.service to its unit row`.

**Acceptance:** T21 passes in full.

### Task 3.6: Phase 3 close (Rust lane, S)

- [ ] Update `process-docs/skills/service-omission-model.md` (generated units and flatpak in the plan), `docs/reference/inspector-coverage.md` (scheduled tasks describe; units control), `codebase-layout.md` if a module moved.
- [ ] **CHANGELOG.md**: **Fixed** a disabled local timer is no longer enabled in the image, and a vendor timer enabled against its preset is carried. **Removed** the scheduled timer toggle, which applied nothing, and the unconditional `tuned.service` enable (the unit's own row decides it).
- [ ] Gate; commit `docs: record one control per timer fact`.

**Phase 3 ends:** every `systemctl` line in the Containerfile is a planned one; a timer's state has one control and its file another.

**Review gate 3:** implementation review before Task 4.0. Focus: no second writer remains for any unit file; generated-name collisions through both export routes.

---

## Phase 4: the `/etc/systemd/system` scan (size L, above § 15's estimate; 13 tasks)

Spec: § 6.3 (`also`, `install_drop_ins`, `vendor_drop_ins`), § 6.6, § 7.1 sources 3 to 5 and the search path, § 7.2 (all), § 7.3, § 7.4 (instance preset, stock state), § 7.5 (first three paragraphs), § 7.6, § 7.7 (`*.upholds/*`), § 10.2 (install files, rule 5 drop-in half, rule 7), § 10.5, § 10.9, § 11.1 (`manual_links`, `default_target`, `also`, `install_drop_ins`), § 12 (list fields, `manual_links`, `default_target`). Tests T22 to T27, T25 (a) to (l).

### Task 4.0: Run R5 before phase 4 (real host, M)

**Spec:** § 13 R5; § 15 real-host table, "Before phase 4 is planned" (see Spec questions 3).

**Needs:** reverted `rhel9` and `rhel10` VMs, no inspectah build, no driftify profile: every case is a unit planted by hand. The `systemd-cryptsetup@` and `systemd-fsck@` case needs a host with an encrypted volume or a separate filesystem; if the VMs have neither, record that and do not fabricate one.

- [ ] Run each R5 question as § 13 states it, on both releases. Record under a dated Measured subsection. Revert both VMs. Commit `docs(skills): record the small systemd facts phase 4 relies on`.

**Gate:** phase 4 code does not start until each answer is checked against Tasks 4.1 to 4.8. An answer that matches the spec's assumption lets its guard stand. An answer on § 17's R5 line (a drop-in reset behaving as documented, a different search-path order, `disable` failing on an absent `Also=` member) goes to the project owner for a spec change, and this plan's affected task is rewritten before it starts.

### Task 4.1: The `[Install]` reader (Rust lane, M)

**Spec:** § 7.2 `InstallSection`, `InstallRead`, `InstallUnknown`, step 4 (fragment plus drop-ins in systemd's order, `%i`, `%I`, `%p`, `%N` expanded, own, template, type-wide and dash-prefix directories under both roots), "A drop-in that assigns an empty value", "The reader is a pure function over text"; § 7.3 `UpheldBy=` is `Undetermined(UpheldBy)`.

**Files:** Create `crates/collect/src/inspectors/units/install.rs`. Test: `#[cfg(test)]` there.

**Interfaces (Produces):** `pub(crate) fn read_install(name: &UnitName, fragment: &str, drop_ins: &[(DropInSource, &str)]) -> InstallRead`; `pub(crate) fn drop_in_dirs(name: &UnitName) -> Vec<String>` (the ordered `.d` directory names under each root).

- [ ] **Step 1: Failing tests first,** one table: `WantedBy=` from the fragment, from an `/etc` drop-in, from a `/usr/lib` drop-in; a later drop-in adds; `WantedBy=` empty is `Undetermined(DropInReset)`; `%H` is `Undetermined(Specifier)`; `%i` expands for an instance; a `.device` value is `ForeignName`; `UpheldBy=` is `Undetermined(UpheldBy)`; no section is `Absent`; `[Install]\r\n` and a value with trailing `\r` read as plain (Review focus 3); `drop_in_dirs("a-b-c.service")` lists `a-.service.d`, `a-b-.service.d`, `service.d`, the template's for an instance, in systemd's documented order.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(collect): read a unit's effective [Install] section`.

### Task 4.2: The effective unit and the search-path test (Rust lane, M)

**Spec:** § 7.1 "The search path" (the constant, both refusal conditions), § 7.2 `Resolved::Unsupported` and step 1; T24 search-path and concrete-instance cases.

**Files:** Modify `crates/collect/src/inspectors/units/resolve.rs`, `crates/collect/src/inspectors/units/mod.rs`. Test: `crates/collect/tests/units_test.rs`.

- [ ] **Step 1: Failing tests first:** a concrete `worker@site.service` under `/etc` beside a vendor template is `Local` with `file` the instance; an instance with no file has the template as `file`; a concrete instance file under `/run/systemd/system`, a copy under `/usr/local/lib/systemd/system`, and a drop-in with `[Install]` under `/run/systemd/system` each give a warning naming the path and no state change, and the same unit without the extra file gives its row.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(collect): refuse units the systemd search path would change`.

### Task 4.3: Install drop-ins and the drop-in half of rule 5 (Rust lane, M)

**Spec:** § 6.3 `install_drop_ins`, `vendor_drop_ins`; § 6.7 rows for both (names only, `.conf`, one component); § 7.2 last two paragraphs; § 10.2 "Install files", the decision on same-named drop-ins, the lexical `[Install]` test, rule 5 "for every fragment" paragraph (exact set, `Destination` parse, aggregate variant, instance own-name file); T24 drop-in cases, T25 (k).

**Files:** `crates/core/src/types/units.rs`, `crates/collect/src/inspectors/units/mod.rs`, `crates/pipeline/src/render/unit_plan.rs`. Test: `crates/pipeline/tests/unit_plan_test.rs`, `crates/cli/tests/units_scan_test.rs`.

- [ ] **Step 1: Failing tests first,** T24: a `WantedBy=` only from a drop-in is carried with the drop-in in `install_drop_ins`; that drop-in excluded, or a second `[Install]` drop-in staged that the row does not list, gives `InstallEvidenceChanged` and no enable; an `install_drop_ins` path holding `..` gives the same; an `/etc` drop-in with no `[Install]` and a vendor drop-in's file name is in `install_drop_ins`, and one with its own file name is not; a drop-in that assigns an empty `WantedBy=` is advised. T25 (k) first half: the vendor `bundle.service.d/10-members.conf` case with the `/etc` file included carries the enable and lists the file, excluded gives `InstallEvidenceChanged`. A `vendor_drop_ins` value with `/` or without `.conf` fails to load. The lexical test is true for `  [Install]  `, `[Install]\r`, and a backslash-continued assignment (Review focus 3).
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(pipeline): carry an enable only with the [Install] files it was recorded from`.

### Task 4.4: Install state from the links (Rust lane, L)

**Spec:** § 7.1 sources 3 and 4, § 7.3 (sets, table, three points except `.upholds` advising, which is Task 4.5), § 7.4 instance preset and stock-state constant, § 6.4 "a unit that systemd calls `enabled` on the strength of a hand-made link is `Disabled`"; T22.

**Files:** `crates/collect/src/inspectors/units/mod.rs`, `crates/core/src/baseline.rs` (stock-state constant, one entry: `remote-cryptsetup.target`, `Disabled`). Test: `crates/collect/tests/units_test.rs`.

- [ ] **Step 1: Failing tests first,** T22 in full, the first four cases repeated for an instance.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `fix(collect): read enablement from the links, not systemctl's word`.

**Acceptance:** T22 passes; the inventory state is the derived one.

### Task 4.5: Hand-made links and their advisories (Rust lane, M)

**Spec:** § 6.6, § 7.3 rows producing `ManualLink`, alias handling, `.upholds` advising, unparseable link names warned; § 7.5 "Instances tied to one host's devices" (constant); § 7.7 `*.upholds/*` walk exclusion (`walk.rs:154-155`); § 10.9; § 6.7 guard gains `ManualLink`; § 11.1 `manual_links[]` projection; T23.

**Files:** `crates/core/src/types/units.rs`, `crates/collect/src/inspectors/units/mod.rs`, `crates/collect/src/inspectors/config/walk.rs`, `crates/pipeline/src/render/unit_plan.rs`, `crates/pipeline/src/render/mod.rs`, `crates/refine/src/projection/`. Test: `crates/cli/tests/units_scan_test.rs`.

- [ ] **Step 1: Failing tests first,** T23: the `StaticUnit` link and its refine row exist; then the advisory holds the `add-wants` line, no `RUN` names the unit, the row has no include; table cases for `.requires`, an unexpanded specifier, `systemd-fsck@` (no command line) and a hand-made alias; `UpheldBy=multi-user.target` with its `.upholds` link through the units and config inspectors: no state change, no enable, the restart-loop text, the link's path absent from `config.files` and the staged tree.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(collect): report hand-made unit links with the command that recreates them`.

### Task 4.6: Enabled instances, omitted for now (Rust lane, S)

**Spec:** § 7.5 first two paragraphs; § 15 phase 4 placeholder (`template instances are not carried yet`); T27.

**Files:** `crates/collect/src/inspectors/units/mod.rs`, `crates/pipeline/src/render/unit_plan.rs` (a placeholder `OmissionReason` variant, removed in Task 5.2). Test: `crates/cli/tests/units_scan_test.rs`.

- [ ] **Step 1: Failing test first,** T27: an enabled instance found through the directory scan has a state change and a refine row, an `# Omitted:` line with the placeholder reason, and no enable; an instance linked only under `/run` is not found; an instance linked from two parents is one state change.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(collect): collect enabled template instances`.

### Task 4.7: `Also=` members recorded (Rust lane, M)

**Spec:** § 6.3 `MemberLinks`, `AlsoMember`; § 7.3 "`Also=` members" (closure walk, `Unlinked`/`Inert` count every link, template `DefaultInstance=`, no walk through an unreadable section); § 6.7 guard gains `AlsoMember`; § 11.1 `also[]` and `install_drop_ins[]` on `unit_states`; T25 (l).

**Files:** `crates/core/src/types/units.rs`, `crates/collect/src/inspectors/units/mod.rs`, `crates/pipeline/src/render/mod.rs`, `crates/refine/src/projection/`. Test: `crates/collect/tests/units_test.rs`.

- [ ] **Step 1: Failing tests first:** a chain `a`, `b`, `c` records `b` and `c` on `a`; each `MemberLinks` value from a planted link set; a member with no `[Install]` and a hand-made link is `Other`; T25 (l) (`Also=` naming a template is its `DefaultInstance=`, and `Other` under the template name when there is none); a member's `vendor_drop_ins` names are recorded.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(collect): record the units Also= reaches`.

### Task 4.8: Rule 7 on a single host (Rust lane, L)

**Spec:** § 10.2 rule 7 decisions, conditions 1 to 3, `AlsoMember`/`AlsoWhy` (`LinksDiffer`, `RowNotCarried`, `NeedsOwnRow`, `InstallFile`), the advisory text and its line rules, "Three things rule 7 leaves alone"; § 10.3 "Another unit's `Also=`"; T25 (a) to (i) and (k) second half.

**Files:** `crates/pipeline/src/render/unit_plan.rs`, `crates/refine/src/projection/`. Test: `crates/pipeline/tests/unit_plan_test.rs`, `crates/cli/tests/units_scan_test.rs`.

- [ ] **Step 1: Failing tests first,** T25 (a) to (i) and the (k) member case, each asserting the block or the carried line and the structured reason on the refine row; (c) expects `RowNotCarried` while instances are omitted (Task 5.2 changes it). Review focus 4: scan and refine export give equal unit lines for case (a).
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(pipeline): carry an enable or disable with Also= only when every member agrees`.

### Task 4.9: Fleet merge of the phase 4 fields, and rule 7 on an aggregate (Rust lane, M)

**Spec:** § 12 bullets on list-field union, member `links` disagreement becoming `Other`, `vendor_drop_ins` union, `manual_links` by relation, parent and unit, "always an advisory in an aggregate", the rows-without-`Also=` limit (no product text); § 10.2 `AlsoWhy::FleetAggregate` and its advisory text; T25 (j).

**Files:** `crates/core/src/aggregate/merge.rs`, `crates/pipeline/src/render/unit_plan.rs`. Test: `crates/core/tests/aggregate_merge_test.rs`, `crates/refine/tests/aggregate_export_test.rs`.

- [ ] **Step 1: Failing tests first,** T25 (j) through the real collector and merge (host A alone carries the enable; merged gives `FleetAggregate` on the refine row with the fleet text); two hosts recording one member `Installed` and `Unlinked` merge to `Other`; manual links from two hosts merge by the triple.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(core): merge Also= and link findings across hosts`.

### Task 4.10: The default target (Rust lane, M)

**Spec:** § 7.6, § 10.5, § 6.7 guard gains `DefaultTarget`, § 11.1 `default_target` and `ItemId::DefaultTarget`, § 12 `default_target` bullet; T26.

**Files:** `crates/core/src/types/units.rs`, `crates/collect/src/inspectors/units/mod.rs`, `crates/pipeline/src/render/unit_plan.rs`, `crates/pipeline/src/render/mod.rs`, `crates/refine/src/{types,session}.rs`, `crates/refine/src/projection/`, `crates/core/src/aggregate/merge.rs`. Test: `crates/cli/tests/units_scan_test.rs`, `crates/core/tests/aggregate_merge_test.rs`.

- [ ] **Step 1: Failing tests first,** T26 in full; three hosts with two agreeing merge to the most prevalent target, excluded.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat: carry the source host's default target`.

### Task 4.11: Links, default target and members in the flat list (frontend lane, M)

**Spec:** § 11.1 `manual_links[]`, `default_target`, `unit_states[].also[]` and `install_drop_ins[]`, `ItemId::DefaultTarget`. See Spec questions 4.

**Parallel:** can start beside Tasks 4.5, 4.7 and 4.10 against their projection Interfaces; merges after the last of them.

**Files:** `crates/web/src/{web_types,adapter,handlers}.rs`, `crates/web/ui/src/api/types.ts`, the units list component. Test: `crates/web/tests/api_test.rs`, Vitest.

- [ ] **Step 1: Failing tests first:** a manual link renders as a row with its advisories and no toggle; the default target renders with a toggle that sends `ItemId::DefaultTarget` and changes the Containerfile; a unit row lists its `also` members with their links.
- [ ] **Step 2-4:** run, implement, frontend checks, gate. **Step 5: Commit** `feat(web): show unit links, Also= members and the default target`.

### Task 4.12: Phase 4 close (Rust lane, S)

**Spec:** § 14 row for `inspectah-write-the-collector-overlap-contract` ("Phase 4 writes this part into `process-docs/` and fixes `docs/reference/inspector-coverage.md`").

- [ ] Write the `/etc/systemd/system` ownership contract (§ 7.7 table, § 8 table, the three dependency-directory exclusions) as `process-docs/skills/etc-systemd-system-ownership.md`, add it to `process-docs/skills/index.md`; fix `docs/reference/inspector-coverage.md:110-133` (sources, what becomes a state change, links, the default target); update `service-omission-model.md` (rules 5 and 7).
- [ ] **CHANGELOG.md**: **Added** hand-made unit link advisories with the `add-wants` or `add-requires` line; the default target is carried. **Changed** enablement is read from the links, so a unit enabled only by a hand-made link is reported, not enabled; a command whose `Also=` members differ on the source is an advisory with the lines to add by hand.
- [ ] Gate; commit `docs: write the /etc/systemd/system ownership contract`.

**Phase 4 ends:** a scan reads enablement from the links, reports hand-made links, carries the default target, records `Also=` members and carries a command only when they agree, and names every enabled instance in an `# Omitted:` line.

**Review gate 4:** implementation review before Task 5.1. Focus: **the interaction of rule 5's drop-in half and rule 7 with `staged_writes` and the fleet merge** (T24, T25 (h), (j), (k)): an install file staged, excluded, or present on another host only; a same-named `/etc` drop-in hiding a vendor one; a member's toggle changing its owner's command.

---

## Phase 5: carried instances (size S to M; 7 tasks)

Spec: § 7.5 (lists, tables, `/var/run` and `/var/lock` constant), § 10.2 rule 6, § 10.8, § 11.1 `required_config[]`, `data_paths[]`. Tests T28, T29.

Before Task 5.1: merge `main` into the branch and run the gate.

### Task 5.1: What an instance needs (Rust lane, M)

**Spec:** § 7.5 "What an instance needs" table, the fixed tables (decided), "Every `required_config` path is kept", "`data_paths` holds persistent data only"; § 6.7 row for both lists; T28 collector cases.

**Files:** Create `crates/collect/src/inspectors/units/instance.rs`; modify `crates/core/src/types/units.rs`. Test: `crates/collect/tests/units_test.rs`.

- [ ] **Step 1: Failing tests first:** `wg-quick@wg0` gives `required_config = ["/etc/wireguard/wg0.conf"]` from the table; a template with `EnvironmentFile=/etc/x/%i.env` and `EnvironmentFile=-/etc/y/%i.env` keeps only the first; `dirsrv@example` gives `/var/lib/dirsrv/slapd-example` when it exists and nothing when it does not; `PIDFile=/var/run/worker/%i.pid` gives an empty `data_paths`; `StateDirectory=worker/%i` gives `/var/lib/worker/<i>`; a value that expands outside `/etc` or `/var` is dropped.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(collect): record what an enabled instance reads and keeps`.

### Task 5.2: Rule 6 and carried instances (Rust lane, M)

**Spec:** § 10.2 rule 6; § 10.8 table, `InstanceConfigNotStaged`, `ConfigGap`, "Only a file the export carries passes", "Redacted", the `--no-redaction` and `Flagged` attention notes, the unknown-template note (decided); the phase 4 placeholder removed; T28 carry, T29.

**Files:** `crates/pipeline/src/render/unit_plan.rs`, `crates/collect/src/inspectors/units/mod.rs` (attention reasons). Test: `crates/cli/tests/units_scan_test.rs`, `crates/pipeline/tests/unit_plan_test.rs`.

- [ ] **Step 1: Failing tests first,** T28 (the `openvpn-client@site` case through the real inspectors: one state change with the template's owner, the instance in `RUN systemctl enable`); T29 in full, positive controls included, the WireGuard case through the real redaction pass asserting the staged file holds no key; update T25 (c) to the phase 5 result (the member row carried, so the owner's command carried when the member passes, `RowNotCarried` when the member's configuration is excluded), matching T29's last sentence. An instance of a template on no list with both lists empty is carried with its attention note.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(pipeline): carry an instance's enable when its configuration is in the export`.

### Task 5.3: The instance data advisory (Rust lane, S)

**Spec:** § 10.8 "Data under `/var`" (one comment per instance before the `RUN` lines, same sentence on `attention_reason`); T28 `dirsrv` case.

**Files:** `crates/pipeline/src/render/unit_plan.rs`, `crates/collect/src/inspectors/units/mod.rs`. Test: `crates/cli/tests/units_scan_test.rs`.

- [ ] **Step 1: Failing test first:** `dirsrv@example.service` with an existing data directory has the advisory beside a carried enable and the same sentence on its row; with the enable omitted there is no data advisory.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(pipeline): warn that a carried instance's data stays behind`.

### Task 5.4: Instance fields in the flat list (frontend lane, S)

**Spec:** § 11.1 `instance_of`, `required_config[]`, `data_paths[]`.

**Parallel:** can run beside Task 5.2; merges after Task 5.3.

- [ ] **Step 1: Failing test first:** an instance row shows its template, its required configuration paths and its data paths. **Step 2-4:** implement, frontend checks, gate. **Step 5: Commit** `feat(web): show what an instance needs on its row`.

### Task 5.5: Merge preparation (Rust lane, M)

**Spec:** § 9 last bullet ("re-run against the final types before the merge"); § 15 "`main` is merged into the branch between phases"; changelog policy ("For feature branches, the entry is written at merge time").

- [ ] Merge `main` into the branch. Re-run T1 against the final types through all three loaders; regenerate `testdata/e2e-fixture.tar.gz` and every snapshot fixture from the final types (not by editing the version number alone); confirm `docs/reference/snapshot-schema.md` describes the final `units` section.
- [ ] Update `process-docs/skills/service-omission-model.md` (rule 6) and `docs/reference/inspector-coverage.md` (instances).
- [ ] **CHANGELOG.md**: **Added** enabled template instances are carried when their configuration is in the export, and omitted with the reason otherwise. Then rewrite the branch's `[Unreleased]` entries so they describe the net change against `main`, not the order phases landed in.
- [ ] Gate; commit `docs: prepare the units branch for merge`.

**Phase 5 ends:** an enabled instance is carried when every path it needs is in the export as the host had it, and omitted, with the path and the reason, when not.

**Review gate 5:** implementation review of phase 5 and a whole-branch review before Task 5.6. Focus: rule 6 against redaction findings and `staged_writes`; T25 (c) after instances became carried; the CHANGELOG and skills checked per the repo's policy.

### Task 5.6: Runs R1, R2 and R4 at the branch tip (real host, L)

**Spec:** § 13 R1, R2, R4; § 15 real-host table, "At the branch tip, after phase 5"; § 17 "Run R4 finds stock rows beyond the known ones".

**Needs:** the branch build at the end of Task 5.5 (and, for R2's ordering case, a `main` build from before the merge); reverted `rhel9` and `rhel10` VMs, reverted before each run; the VM operating notes.

- [ ] **R1:** as Task 2.16, driftify **standard**.
- [ ] **R2:** driftify **kitchen-sink** (it plants `wg-quick@wg0` with a key and the `cups`, `iscsid` and `virtnetworkd` service-on, socket-off pairs), plus by hand: a local service with no paired timer (built with the `main` build, then the branch build), a full shadow built and booted, an instance with clean configuration checked active after boot, and the `service.d` package case. If no package in the enabled repositories owns an `[Install]`-bearing `/etc/systemd/system/service.d/*.conf`, record that and what was searched.
- [ ] **R4:** scan each VM stock (no driftify) and then after driftify **kitchen-sink** (cumulative, so every unit-state case of all three levels). Classify every row the new kinds add, every link advisory, every instance and every `Also=` advisory, and keep the classified list with its reasons. Known before the run: `remote-cryptsetup.target` on RHEL 9 and the rows a package install makes through `Also=` (§ 7.4).
- [ ] Record each under a dated Measured subsection. Revert both VMs. Commit `docs(skills): record the units branch merge-gate runs`.

**Gate (the merge gate):** the branch merges only when R1 shows the image reading the host's socket state on both releases; R2's builds succeed with the enable after its file, the WireGuard enable omitted and the clean instance active; and R4 finds no unexplained stock row. A stock row beyond the known ones either joins the stock-state constant (a reviewed change on the branch) or, if the list is shown to be the wrong tool, goes to the project owner before the merge.

### Task 5.7: Merge (project owner)

- [ ] With review gate 5 passed and Task 5.6's gate met, the project owner merges `feat/unified-systemd-units` to `main`. The spec moves from `proposed/` to `implemented/` in the merge's follow-up, with the plan's status marker and a row in `process-docs/plans/README.md`, per that file's rule.

---

## Sizes

| Phase | Tasks (Rust / frontend / real host) | § 15 size | This plan's size |
|---|---|---|---|
| 1 the rename | 7 (6 / 1 / 0) | M | M |
| 2 every kind | 17 (13 / 1 / 2 runs; 1 close) | L, upper end | XL: about one and a half L |
| 3 one control per fact | 6 (5 / 1 / 0) | M | M |
| 4 the `/etc/systemd/system` scan | 13 (11 / 1 / 1 run) | L | L+: about a third over L |
| 5 carried instances | 7 (4 / 1 / 1 run; 1 merge) | S to M | M |

Task sizes: S is one short dispatch, M one full dispatch, L one dispatch at the limit of what a single review can check from the diff. The L tasks (1.2, 2.7, 4.4, 4.8, 2.17, 5.6) are the ones to watch for a split.

**The whole is larger than § 15 estimates, by roughly a third.** Phases 2 and 4 carry most of it: phase 2 holds staging, the plan, three rule tasks, mount findings, updaters, fleet and refine rows, and phase 4 holds a reader, a resolver, link state, links, members, rule 7 and the default target. § 15's sizes also do not count the five real-host tasks or the frontend lane's four flat-list tasks.

## Spec questions

Found while planning. None is resolved here; each says what the plan does meanwhile.

1. **R1's direction is inverted against the measured presets.** § 13 R1 says "disable on RHEL 9, enable on RHEL 10". The recorded package states (`cockpit.socket` preset `disabled` on RHEL 9, `enabled` on RHEL 10) and the driftify standard profile make "against its preset" an `enable` on RHEL 9 and a `disable` on RHEL 10. T5's "enabled against a `disable` preset" agrees with the measured direction. The plan runs R1 against the preset as driftify sets it.
2. **There is no T14.** § 13 numbers T13 then T15. The plan keeps the spec's ids and assumes no test was lost; if one was, it belongs in phase 2.
3. **R5 is scheduled "before phase 4 is planned", and this plan plans phase 4 now.** The plan runs R5 as Task 4.0, before any phase 4 code, and rewrites an affected phase 4 task before it starts if an answer departs from the spec.
4. **The flat list and rows with no toggle.** § 15 says units are shown in the existing flat list at the merge, and § 11.2 leaves presentation to phase 6. The spec does not say whether mount findings, hand-made links and the default target appear in refine before phase 6. The default target has a toggle (§ 10.5) that is unreachable without a row. The plan adds a minimal row for each in the flat list in the phase that adds its data (Tasks 2.14, 4.11), with no grouping.
5. **Changelog timing.** `process-docs/changelog-policy.md` writes a feature branch's entry at merge time; this plan writes each phase's lines in its close task and rewrites the block at Task 5.5. Not a spec question, a plan choice, listed so the review sees it.

## Coverage

Each spec section that states a requirement, and the task that implements it.

| Spec section | Task(s) |
|---|---|
| § 2 decisions | 2.3 (every kind), 2.5 and 2.10 (storage advisory), 2.8 (`tmp.mount` mask), 4.8 (`Also=` verbs), 4.6 and 5.2 (instances), 4.5 (hand-made links), 1.2 (schema 28), 5.7 (merge after phase 5) |
| § 3 non-goals | Non-goal. Generated and runtime states keep today's warning: 2.3. User units and quadlets untouched. |
| § 5 per-kind table | 2.3, 2.4, 2.5, 4.5, 4.6, 5.2 |
| § 6.1 `UnitName` | 1.1 |
| § 6.2 carried and advisory types, `classify` | 2.1, 2.2 |
| § 6.3 state change, refusals | 2.1, 2.2; `also` 4.7; drop-in lists 4.3; instance lists 5.1 |
| § 6.4 section, inventory | 1.3; derived state 4.4 |
| § 6.5 mount findings | 2.1, 2.5 |
| § 6.6 hand-made links | 4.5 |
| § 6.7 extracted fields and guard | 2.5; `AlsoMember` 4.7; `ManualLink` 4.5; `DefaultTarget` 4.10; `vendor_drop_ins` 4.3; instance lists 5.1; timers 3.1 |
| § 7.1 sources 1, 2 | 2.3 |
| § 7.1 sources 3 to 5, search path | 4.2, 4.4 |
| § 7.2 steps 2, 3 | 2.2 |
| § 7.2 step 1, step 4, install files recorded | 4.1, 4.2, 4.3 |
| § 7.3 install state, table | 4.4 |
| § 7.3 `ManualLink`, aliases, `.upholds` | 4.5 |
| § 7.3 `Also=` members | 4.7 |
| § 7.4 state change rules, name gate | 2.3 |
| § 7.4 instance preset, stock-state list | 4.4 |
| § 7.4 updaters, protected units at collect | 2.11, 2.8 |
| § 7.5 instances collected, host-device list | 4.6, 4.5 |
| § 7.5 what an instance needs, fixed tables | 5.1 |
| § 7.6 default target | 4.10 |
| § 7.7 one owner per path, masks, drop-ins | 2.4 |
| § 7.7 `*.upholds/*` exclusion | 4.5 |
| § 7.8 owning package | 2.2 |
| § 8 scheduled tasks | 3.1, 3.2, 3.3; generated units 3.4 |
| § 9 schema, one bump, refusal test | 1.2; re-run 5.5 |
| § 9.1 string consumers | 1.2, 1.5, 1.6, 1.7; aggregate timer control 3.3 |
| § 10.1 lines, quoting, order within section | 2.7, 2.9 |
| § 10.1 README sentences | 2.9 |
| § 10.2 `staged_writes`, `Destination`, rule 1 | 2.6 |
| § 10.2 `plan_units`, `PlannedUnit`, rule 5 fragment half, no-RPM path | 2.7 |
| § 10.2 rules 2, 3, 4 | 2.8 |
| § 10.2 what the plan takes, item 3; tuned | 3.4, 3.5 |
| § 10.2 install files, rule 5 drop-in half | 4.3 |
| § 10.2 rule 6 | 5.2 |
| § 10.2 rule 7, single host | 4.8 |
| § 10.2 rule 7, aggregate | 4.9 |
| § 10.3 units the target needs | 2.8; via `Also=` 4.8 |
| § 10.4 runtime updaters, normalizer | 2.11 |
| § 10.5 default target render | 4.10 |
| § 10.6 mount advisories | 2.10 |
| § 10.7 order | 2.9; generated units 3.4 |
| § 10.8 instance configuration, data advisory | 5.2, 5.3 |
| § 10.9 link advisories | 4.5 |
| § 11.1 rename of fields and operation | 1.5, 1.6 |
| § 11.1 `unit_states`, `declined`, `withheld`, `mount_findings` | 2.13, 2.14 |
| § 11.1 scheduled rows, timer operation removed | 3.2, 3.3 |
| § 11.1 `manual_links`, `default_target`, `also` | 4.5, 4.7, 4.10, 4.11 |
| § 11.1 instance fields | 5.1, 5.4 |
| § 11.2 section design | Phase 6 |
| § 11.3 synthesis retired | 1.4, 1.6 |
| § 12 inventory merge | 1.3 |
| § 12 identity, mount findings, updater exception, render-time validation | 2.11, 2.12 |
| § 12 list fields, `manual_links`, `default_target`, aggregate `Also=` | 4.9, 4.10 |
| § 13 T1, T3, T4 | 1.2, 1.4, 1.5, 1.6 |
| § 13 T2, T5 to T13, T15 to T18 | 2.1 to 2.13 |
| § 13 T19 to T21 | 3.1 to 3.5 |
| § 13 T22 to T27 | 4.2 to 4.10 |
| § 13 T28, T29 | 5.1 to 5.3 |
| § 13 R1 | 2.16, 5.6 |
| § 13 R2, R4 | 5.6 |
| § 13 R3 | 2.17 |
| § 13 R5 | 4.0 |
| § 13 unrecorded measurements | 2.15 |
| § 14 overlap contract text | 4.12 |
| § 15 branch, phases, merge after phase 5 | Whole plan; 5.7 |
| § 17 limit: fleet rows without `Also=` | Non-goal (stated limit, no product text) |
| § 17 limit: flatpak rescan | 3.4 (the FIXME is its text) |
| § 17 limit: fleet fragment disagreement | Non-goal (stated limit, no product text) |
| § 17 limit: `disable` with an absent `Also=` member | 4.0 (R5 measures it); no product text |
| § 17 limit: an edited `also` list is believed | Non-goal (stated limit, no product text) |
| § 17 limit: fleet evidence may omit install metadata | 4.9 (the `FleetAggregate` advisory text) |
