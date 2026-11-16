# Unified systemd unit collection: implementation plan

> ## Executed (phases 1 to 5) (2026-11-16)
>
> **Checkbox state below is not evidence** — most boxes were never ticked.
> This plan is complete; phase 6 (the dedicated refine section, spec § 11.2)
> is not in it, has not started, and is separate work.
> Classified from the tree:
> - Merged at `aacb5e17 Merge branch 'refs/heads/feat/unified-systemd-units'`.
> - `crates/core/src/types/units.rs` and the units inspector in
>   `crates/collect/src/inspectors/units/` are in the tree.
> - The spec is in `process-docs/specs/implemented/`.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task by task. Steps use checkbox (`- [ ]`) syntax for tracking. Every task is test-first: write the failing test the task names, run it and see it fail for the stated reason, then implement.

**Goal:** one model of systemd unit state for every unit type that exists as a static unit file, replacing the service-only collection, so that a socket, path, timer, target, slice or package-owned storage unit the administrator switched against its preset is carried into the image or omitted with a stated reason.

**Architecture:** proved types in `inspectah-core` (`types::units`) that make an administrator's mount unit unable to reach a `systemctl` line; a `UnitsInspector` in `inspectah-collect` that reads enablement from the links under `/etc/systemd/system` and the effective `[Install]` section; one render-time check in `inspectah-pipeline` (`staged_writes` and `plan_units`) that decides every `systemctl` line against the assembled export; refine and the fleet merge migrated to the new types, with units shown in the existing flat list.

**Tech stack:** Rust, serde, insta, proptest, axum, React and TypeScript with Vitest. One new dev-dependency: `proptest` in `inspectah-core`, for T2's property test (Task 2.1), which the spec requires.

**Spec:** `process-docs/specs/implemented/2026-11-13-unified-systemd-units.md` at `6a045e69`. The spec is the contract. Section numbers (§) and test ids (T1 to T44) are the spec's; run ids R1 to R5 are § 13's. Source line numbers below were read at `205181a6`, where no source file differs from `70aa8996`, the commit the spec cites. The symbol name is the identifier; the line is a convenience.

**Branch:** `feat/unified-systemd-units`, cut from `main` by Task 1.1. Phases 1 to 5 land on it and it merges to `main` once, after phase 5 (§ 15).

**Verification convention:** every `cargo` command runs without a pipe, so the exit code is cargo's. The gate for every commit is the pre-commit hook's: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D clippy::all`, `cargo test --workspace`. Frontend tasks add `npx tsc --noEmit` and `npx vitest run` in `crates/web/ui`. A phase is done when its last commit passes all five.

---

## Phase 6 is not in this plan

Phase 6, the Systemd units section of § 11.2, waits on the frontend's own design pass for grouping and presentation. It starts once that pass is done and lands on `main` after this branch merges, as its own change with its own plan. It reads the view response that phases 1 to 5 complete (§ 11.1) and changes no serialized snapshot type, so it needs no schema version. Until it lands, refine shows every unit kind, and the mount, link and default-target rows, in the existing flat list.

## Lanes

- **Rust lane:** the workspace crates `core`, `collect`, `pipeline`, `refine`, `cli`.
- **Frontend lane:** `crates/web/src` (Axum handlers, `web_types.rs`, `adapter.rs`, `aggregate_handlers.rs`) and `crates/web/ui` (TypeScript types, components, hooks, Vitest suites). A different implementer takes these tasks.

A Rust-lane task whose type change stops `crates/web` from compiling makes the compile-forced edits there in the same commit: renames, deletions, and mechanical conversions at the boundary between a new core type and an existing DTO or operation, such as `UnitName::as_str().to_owned()` where a DTO holds a `String` (`crates/web/src/adapter.rs:135`), passing the unit's name where an operation takes it (`crates/web/src/handlers.rs:891`), or a match arm for a new `ItemId` variant that forwards it unchanged. No new DTO field, endpoint, route or user-visible behavior: every addition in `crates/web` is a frontend-lane task's. The Rust tasks expected to make such edits are 1.2 (`UnitName` in the adapter and the bulk operation), 1.3 (inventory consumers), 1.4 (the two shadow fields leave the state DTO), 1.5 (`ItemId::Unit`), 2.2 (name and owner read out of `CarriedUnit`), 3.1 (the reduced `SystemdTimer`), 3.2 (the removed timer operation) and 4.10 (`ItemId::DefaultTarget`); any other task that needs one says so in its commit body. Each frontend-lane task says whether it can run in parallel with the Rust task beside it.

## Backlog items

Named by slug, in the phase that closes each, per § 14. The phase report says which it closes; the items themselves are updated outside this repository.

- `inspectah-socket-units-are-never-collected`: closed by phase 2 (Tasks 2.3, 2.7, 2.9) and confirmed by R1 (Task 2.16).
- `inspectah-masked-units-are-recorded-as-full-shadows`: closed by phase 2 (Tasks 2.4, 2.8), both mask forms.
- `inspectah-timer-enable-lines-unquoted-and-trust-empty-units`: closed by phases 2 (quoting, Task 2.9; the file check, Task 2.7) and 3 (one writer, Task 3.1).
- `inspectah-local-timer-has-two-controls-and-neither-works`: closed by phase 3 (Tasks 3.1 to 3.3).
- `inspectah-write-the-collector-overlap-contract`: partly covered by phase 4 (Task 4.12, with the exclusions of Tasks 2.4 and 4.5). The general rules document and the non-systemd claimants remain.
- `inspectah-full-shadow-wants-link-points-at-the-vendor-path`: partly covered: the order change (Task 2.9), `add-wants` in link advisories (Task 4.5), and the booted-host check in R2 (Task 5.6).
- `inspectah-srv-mount-units-must-target-var-srv`: partly covered by the mount advisory (Task 2.10). No automatic rewrite; fstab lines and the audit report are untouched.
- `inspectah-etc-unit-files-accessor`: out of scope; its blocker moves after phase 4, when the `/etc/systemd/system` claimants are config and units.
- `inspectah-var-unit-grep-matches-comments-and-longer-names`, `inspectah-etc-systemd-user-has-no-symlink-rule`, `inspectah-redaction-pseudo-paths-miss-systemd-comment-rule`, `inspectah-unredacted-export-carries-no-marker`, `inspectah-redaction-token-eats-the-key-it-replaces`: out of scope (§ 14).

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

Spec: § 6.1, § 6.4, § 9, § 9.1, § 11.3, § 12 (inventory). Tests T1, T3, T4. Scan behavior is unchanged except the snapshot's unit section, and the scan export's Containerfile is unchanged; a refine export no longer writes the `systemctl` line for a unit with a full shadow and no state change of its own. The collector still asks for services only and a state change keeps `owning_package: Option<String>`.

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

**Acceptance:** T4's session assertions pass. The frontend shadow count is Task 1.6's. The `systemctl` line the synthetic row produced in a refine export is the one thing § 11.3 says is not kept: where the shadow leaves `[Install]` alone the image already has the links, and where it changes `[Install]`, Tasks 4.5a and 4.5b carry or advise it.

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

**Phase 1 ends:** a scan writes `units` at schema 28 and refuses 27; the scan export's Containerfile is unchanged, and a refine export no longer writes the `systemctl` line for a unit with a full shadow and no state change of its own; refine shows a full shadow on its drop-in row only.

**Review gate 1:** implementation review of the phase before Task 2.1 starts. Focus: § 9.1 rows each migrated, no string consumer left reading an empty fallback, the T1 payload states why it is refused.

---

## Phase 2: every kind (size L, upper end and above; 17 tasks)

Spec: § 5, § 6.2, § 6.3 (without `also`, `install_drop_ins`, `vendor_drop_ins` and the instance fields), § 6.5, § 6.7, § 7.1 sources 1 and 2, § 7.2 steps 2 and 3, § 7.4, § 7.7, § 7.8, § 10.1, § 10.2 (staging, rules 1 to 4, the fragment half of rule 5), § 10.3, § 10.4, § 10.6, § 10.7, § 11.1 (`declined`, `withheld`, `mount_findings`), § 12. Tests T2, T5 to T13, T15 to T18. Local timers keep today's deferral to the scheduled section (§ 15).

Before Task 2.1: merge `main` into the branch and run the gate.

### Task 2.1: Carried and advisory types (Rust lane, M)

**Spec:** § 6.2 (all types and `classify`), § 6.5 (`MountUnitFinding`), § 7.6 (`TargetUnit` only); T2 cases on the types themselves. The `UnitStateChange` constructor and T2's `state_changes` cases are Task 2.2's, where the field changes type.

**Files:** Modify `crates/core/src/types/units.rs`. Test: `#[cfg(test)]` there and `crates/core/tests/snapshot_test.rs`.

**Interfaces (Produces):** `OrdinaryUnitName`, `StorageUnitName`, `TargetUnitName`, `UnitFragment`, `UnitSubject`, `CarriedUnit`, `TargetUnit`, `AdminStorageFragment`, `AdvisoryUnit`, `Classified`, `UnitSubject::classify`, `MountUnitFinding`, `UnitSection.mount_findings`. Added beside the phase 1 fields; `UnitStateChange.unit` switches to `CarriedUnit` in Task 2.2.

- [ ] **Step 1: Failing tests first,** T2 on the types: for each storage kind, a `CarriedUnit` wire value with `local`, `shadowed` or `absent`, or `vendor` with a null owner, fails `try_from` naming `classify`; `vendor` with an owner deserializes as `CarriedUnit`, and the same value fails as the `unit` of a `MountUnitFinding` in `mount_findings`; `TargetUnit` refuses `cockpit.socket` and a packaged `tmp.mount` and round-trips a `.target`; a `UnitSubject` whose `file` is neither the unit nor its template fails. A proptest: `classify` is total over generated names and fragments and stable through serialize then deserialize.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(core): add carried and advisory unit types`.

**Acceptance:** the type-level T2 cases pass. T2's `state_changes` cases are Task 2.2's and its signature test Task 2.7's.

### Task 2.2: Fragment resolution and owner lookup (Rust lane, M)

**Spec:** § 7.2 steps 2 and 3 (two directories, instance falls back to template, form test, size-zero mask), § 7.8; § 6.3 (`unit: CarriedUnit` replaces `owning_package`, the checked constructor and its two refusals); T2 `state_changes` cases; T5 second sentence, collector half (the render half is Task 2.7's).

**Files:** `git mv crates/collect/src/inspectors/services.rs crates/collect/src/inspectors/units/mod.rs` and `git mv crates/collect/tests/services_test.rs crates/collect/tests/units_test.rs` (the inspector type keeps its name until Task 2.3); modify `populate_owning_packages` (`:113`), `crates/core/src/types/units.rs` (switch the field, add the checked constructor), `crates/pipeline/src/render/service_intent.rs` (`classify_service_presence`, `:359`, reads the owner from the fragment; mechanical), consumers the compiler names, and the compile-forced conversions in `crates/web`. Create `crates/collect/src/inspectors/units/resolve.rs` (the `Resolved` enum with `File`, `Masked`, `AliasOf`, `NotFound`; `Unsupported` arrives in Task 4.2). Test: `crates/collect/tests/units_test.rs`, `crates/core/tests/snapshot_test.rs`.

- [ ] **Step 1: Failing tests first,** T2 `state_changes` cases: for each storage kind, a `state_changes` entry with `local`, `shadowed` or `absent`, or `vendor` with a null owner, fails to load naming `classify`; `vendor` with an owner loads; a template loads `masked` and fails `enabled`, likewise an `absent` fragment; a `file` that is neither the unit nor its template fails. Collector: a local unit and a vendor unit whose owner lookup fails get `Local` and `Vendor { owning_package: None }` (T5); a `/dev/null` link and a zero-length file over a vendor unit are `Masked { hidden: Vendor }`; a comment-only file is a regular fragment; a two-hop link ending at `/dev/null` is a mask and a link to a file outside both directories is `linked` with today's warning (Review focus 2); a storage unit's owner is looked up before classification.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(collect): resolve each unit's fragment and owner`.

**Acceptance:** the listed cases pass; render output for existing service fixtures is unchanged (insta snapshots unmodified).

### Task 2.3: `UnitsInspector` over every kind (Rust lane, M)

**Spec:** § 5 table, § 7.1 sources 1 and 2, § 7.4 (all of it except the instance preset and stock-state list, which are Task 4.4); § 4 "The `dbus-` filter"; T5 first sentence, T7.

**Files:** `crates/collect/src/inspectors/units/mod.rs`: `ServicesInspector` becomes `UnitsInspector`; `list-unit-files` loses `--type=service` (`:226`); the `@` skip at `:303` and `is_real_service` (`:25-36`) become the name gate; `crates/collect/src/executor/mock.rs` keys follow `process-docs/skills/mock-executor-key-format.md`. Test: `crates/collect/tests/units_test.rs`.

- [ ] **Step 1: Failing tests first:** T5 (`cockpit.socket` enabled against a `disable` preset: one state change of kind `Socket`, `Vendor` fragment, owner from the mocked answers); T7 (`dbus-broker.service` kept, a `dbus-org.` alias dropped; a local enabled unit with no preset rule is a state change; a disabled local unit is not; `generated` and each runtime state give none, with today's warning); a masked template is a `Masked` state change on the template name; an unmasked template is skipped; a storage entry that classifies advisory goes to `mount_findings`, never `state_changes`.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(collect): collect unit state for every unit kind`.

**Acceptance:** T5's collector assertions and T7 pass.

### Task 2.4: Masks, drop-ins and shadows, one owner per path (Rust lane, M)

**Spec:** § 7.7 (table and bullets except the `*.upholds/*` exclusion, which is Task 4.5); § 4 "A masked unit is recorded twice"; T11 collector half (the staged-file and command half is Task 2.8's).

**Files:** Modify `crates/collect/src/inspectors/units/mod.rs` (shadow scan, today `:456-485`; drop-in scan, today `:763`), `crates/collect/src/inspectors/config/walk.rs:241` (`is_systemd_unit_symlink` also excludes a zero-length regular file with a unit suffix), `crates/pipeline/src/collect.rs:274` (`shadow_service_owned_paths`, renamed for units). Test: `crates/collect/tests/units_test.rs`, `crates/collect/tests/config_test.rs`.

- [ ] **Step 1: Failing tests first,** T11 collector cases: each mask form over a vendor unit gives one `Masked` state change, no drop-in entry and no config entry; a mask with no vendor file is `Absent` and carried; a masked template under its own name; for each storage kind and both mask forms, a package-owned vendor file gives a carried mask and an unowned one or none gives a mount finding with state `Masked`. A comment-only file over a vendor unit is a full shadow in `drop_ins`, not a mask. A regular non-empty storage file is a config entry and a mount finding, never a full shadow. Drop-ins are collected for every parsed name, `*.device.d`, `*.scope.d` and generated parents; a `.d` directory whose name does not parse (`foo.servic.d`) is a drop-in row with its `unit` string and no state change (Review focus 5).
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `fix(collect): give masks, shadows and drop-ins of every kind one owner`.

**Acceptance:** T11 collector cases pass; a zero-length unit file appears in neither `config.files` nor `drop_ins`.

### Task 2.5: Mount findings, extracted fields and the redaction guard (Rust lane, M)

**Spec:** § 6.5 (which units produce a finding, `mount_point`, `nofail`, no `What=`), § 6.7 (table, guard additions for the types that exist now: `UnitStateChange`, `UnitInventoryEntry`, `MountUnitFinding`); T10.

**Files:** Modify `crates/collect/src/inspectors/units/mod.rs`, `crates/pipeline/src/redaction/engine.rs:978`, `crates/pipeline/src/render/mod.rs:305` (`every_field_of_a_content_bearing_type_is_classified`). Test: `crates/pipeline/tests/redaction_new_surfaces_test.rs`, `crates/cli/tests/units_scan_test.rs` (new).

- [ ] **Step 1: Failing tests first:** T10 (a local mount unit whose `What=` holds a token the generic pattern matches: the token is absent from the whole serialized snapshot after scan and after a refine export; the view-response half is Task 2.13's, once the finding is in the view); `Where=srv` (relative) gives `mount_point: None`; `Where=/srv \r\n` gives `/srv` and `Options=defaults,nofail\r\n` gives `nofail: true` (Review focus 3); an unowned vendor mount unit gives a finding only when enabled or masked. The field guard destructures the three types with no `..`.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(collect): record the administrator's mount units as findings`.

**Acceptance:** T10's snapshot and export assertions pass on both routes.

### Task 2.6: `staged_writes` and `Destination` (Rust lane, M)

**Spec:** § 10.2 `Destination`, `StagedWrites`, `WithheldWhy` (`ZeroLengthUnitFile`, `Conflict`), "`staged_writes` is the stager's decision" bullets, rule 1; § 4 "The stager skips writes it cannot make"; T15 conflict and failed-write cases; T12 staging cases.

**Files:** Modify `crates/pipeline/src/render/configtree.rs` (`validate_path` `:35`, `safe_write_file` `:158`, `staged_sources` `:257`, the drop and refuse tests at `:375-388`; `unstaged_files` carries withheld files to the FIXME); `crates/pipeline/src/render/containerfile.rs` callers. Test: `crates/pipeline/tests/configtree_single_source_test.rs`.

**Interfaces (Produces):** `Destination::new(&str) -> Result<Destination, PathError>`; `pub fn staged_writes(&InspectionSnapshot) -> StagedWrites<'_>` (public, as § 10.2 declares it, because refine calls it in Task 2.13); the stager writes `writes` and nothing else and returns an I/O error on any failed write.

- [ ] **Step 1: Failing tests first:** config entries for `/etc/systemd/system/worker.service/extra.conf` and `/etc/systemd/system/worker.service`, in either order: neither is in `writes`, both are withheld as `Conflict` and named in the FIXME; without the first, the second is written (T15). Two entries at one destination: both withheld. `/etc/systemd/./system//bootc-x.timer` and the plain spelling are one destination; `..` is refused. A zero-length file at `/etc/systemd/system/<name>.service` offered as a config entry and as a drop-in, including with a `unit` field naming another unit: withheld as `ZeroLengthUnitFile` (T12 staging half). A write into a read-only directory fails the export.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `fix(pipeline): decide every staged file once, and fail on a write that does not land`.

**Acceptance:** the listed cases pass; `safe_write_file` no longer returns silently.

**Note, added after Task 2.6 was done:** the spec now also has `staged_writes` withhold any write to `/etc/machine-id` (§ 10.2 "Never the machine-id", `WithheldWhy::MachineId`). Task 4.0a adds it.

### Task 2.7: `plan_units`, quoting and the fragment half of rule 5 (Rust lane, L)

**Spec:** § 10.1 (one function, one `RUN` per verb, kind then name order, single quotes and `--`, `systemctl_lines(&[PlannedUnit])`), § 10.2 `PlannedUnit`, `UnitPlan`, `plan_units`, rule 5 fragment table (`Local`, `Shadowed`, `Vendor`/`PackagedStorage`), the aggregate content-variant sentence for staged unit files, `OmissionReason::{UnitFileNotStaged, InstallEvidenceChanged}`, "The no-RPM path runs every rule"; § 4 "Unit names reach `RUN` lines bare" for the unit section; T2 signature test, T6, T13 quoting, T15 (except the conflict case), T18 (d).

**Files:** `git mv crates/pipeline/src/render/service_intent.rs crates/pipeline/src/render/unit_plan.rs` (`render_service_intent` `:503`, `systemctl_lines` `:292`, `render_without_rpm` `:660` and its early return at `:519-522` removed); `git mv crates/pipeline/tests/service_intent_test.rs crates/pipeline/tests/unit_plan_test.rs`. Test also `crates/cli/tests/units_scan_test.rs`, `crates/pipeline/tests/instruction_injection_test.rs`.

**Interfaces (Produces):** `pub fn plan_units(&InspectionSnapshot, &StagedWrites<'_>) -> UnitPlan`; `PlannedUnit` with private fields and no public constructor; `UnitPlan.declined: Vec<(UnitName, UnitAction, OmissionReason)>`; `OmissionReason::describe(&self) -> String`, the one text source.

- [ ] **Step 1: Failing tests first:** T2 signature (a compile-fail doctest that `PlannedUnit` cannot be built outside the module, and `systemctl_lines` accepts `&[PlannedUnit]` only); T6 (the socket snapshot yields the enable; the owner excluded yields `# Omitted: cockpit.socket (...)`); T13 quoting (`probe@a\x2db.service` and `-.slice` render as quoted operands after `--`, and so does a 255-byte name, each surviving a refine export unchanged: Review focus 1); T15: a local enabled socket whose config entry is excluded gives no staged file, the `UnitFileNotStaged` line and no enable, each of the two toggles driven through `RefineSession` alone and shown to change the staged tree or the Containerfile (as `process-docs/skills/cross-inspector-shadow-passes.md` requires), and the same with no package section; the Task 2.6 conflict case (`worker.service/extra.conf` and `worker.service`, either order) omits the enable, and without the conflicting child the enable is carried; T5 render half: a local unit and a vendor unit whose owner lookup fails, from the same scan, render differently (the local unit is omitted with `UnitFileNotStaged` when its file is excluded and carried when it is staged; the unknown-owner vendor unit is emitted by Tier 1 and omitted with `InstallEvidenceChanged` when a unit file is staged at its name); a `Shadowed` row whose shadow is excluded gives no `enable` and no `disable` with `InstallEvidenceChanged`; a `Vendor` row with a unit file staged at its name by another row gives the same; each returns when the export matches again. T18 (d): two hosts with a full shadow of one unit and different content give `InstallEvidenceChanged` whichever variant is selected. Review focus 4: scan and refine export give equal unit lines for the T6 snapshot.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(pipeline): plan every unit command against the staged export`.

**Acceptance:** the listed tests pass; T5 and T15 pass in full apart from the refine row half of T15 ("shows the reason and the path"), which is Task 2.13's.

### Task 2.8: Rules 2, 3 and 4, and the units the target needs (Rust lane, M)

**Spec:** § 10.2 rules 2, 3, 4 and `OmissionReason::{StorageFileStaged, TargetNeedsUnit, MaskPathOccupied}`; § 10.3 (protected names constant, `tmp.mount` constant, FIXME text, collector `attention_reason`, every route); T8 render cases, T12 render cases, T16.

**Files:** Modify `crates/pipeline/src/render/unit_plan.rs`, `crates/core/src/baseline.rs` (both constants), `crates/collect/src/inspectors/units/mod.rs` (attention reason). Test: `crates/pipeline/tests/unit_plan_test.rs`, `crates/cli/tests/units_scan_test.rs`.

- [ ] **Step 1: Failing tests first:** T12 table over the protected names for `disable` and `mask`: the FIXME and no command, `enable` carried; a masked `tmp.mount` gives its FIXME while its `enable` and `disable` are carried and a masked `dev-hugepages.mount` is carried; a zero-length file for a protected unit from a scan and imported as a drop-in and as a config entry (with a `unit` field naming another unit, an interior `.`, a doubled separator) gives no `mask` and no staged file. T8: a package-owned mount unit enabled against preset is in `RUN systemctl enable`, masked in `mask`; with an administrator's file staged at its name, `StorageFileStaged`. T16: a mask and a staged unit file of one name give `MaskPathOccupied`; excluding the file brings the mask back. T11 staged-file and command half, through the scan: each mask form over a vendor unit gives one `RUN systemctl mask` line and no staged file at the unit's path; a mask with no vendor file (`Absent`) and a masked template are in the `mask` line under their own names; for each storage kind and both mask forms, a package-owned vendor file gives a `mask` line and an unowned one or none gives a mount finding with state `Masked` and no instruction (its rendered advisory is Task 2.10's); the comment-only full shadow is staged by the drop-in writer and no `mask` names its unit.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(pipeline): refuse commands the image cannot take`.

**Acceptance:** T12 (except its view-response sentence, Task 2.13), T8 render cases, T11 (except the rendered advisory for unowned storage masks, Task 2.10) and T16 pass.

**Note, added after this task was done:** the spec now also applies rule 5's package check to `mask` (§ 10.2 "A mask follows the package check"). This task carries a mask whatever its package's state. Task 4.0b adds the check.

### Task 2.9: Section order, every `systemctl` site quoted, README sentences (Rust lane, S)

**Spec:** § 10.7 (unit state section directly after the Configuration Files section; generated units keep their own sections); § 15 phase 2 "quoting and `--` at every `systemctl` site"; § 10.1 "the exported README states each in a sentence" (first boot keeps it; `disable` removes the administrator's links and nothing more); T13 order.

**Files:** `crates/pipeline/src/render/containerfile.rs:177-198` (order), `:1400` (timer enable), `:1652` (flatpak), `:1885` (tuned); `crates/pipeline/src/render/readme.rs`. Test: `crates/pipeline/tests/smoke_render.rs`, `crates/cli/tests/units_scan_test.rs`.

- [ ] **Step 1: Failing tests first:** T13 order (a local service with no paired timer: the config `COPY` and the enable both exist, and the enable follows); the timer, flatpak and tuned lines are quoted after `--`; the README holds both sentences when the export has a unit state section.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `fix(pipeline): run unit commands after the files they name`.

**Acceptance:** T13 passes in full.

### Task 2.10: Mount advisories (Rust lane, M)

**Spec:** § 10.6 (table, layout constants beside `VAR_SYMLINKED_TOPLEVELS` at `crates/pipeline/src/render/selinux.rs:29`, the unit file stays a config entry); T8 advisory cases, T9.

**Files:** Modify `crates/pipeline/src/render/unit_plan.rs`, `crates/pipeline/src/render/selinux.rs`. Test: `crates/pipeline/tests/unit_plan_test.rs`, `crates/cli/tests/units_scan_test.rs`.

- [ ] **Step 1: Failing tests first:** T9 (a local `srv.mount`: the `/var` rename line; `Where=/opt/vendor`: the `/opt` line and no rename line; under `/usr`: the `/usr` line; the unit-file line follows the config row's toggle; no instruction names any of them); T8 advisory cases (a local unit of each storage kind enabled, and a vendor one whose owner lookup fails: advisory and no instruction; a `tmp.mount` full copy in `/etc`: the advisory with the `tmp.mount` line, and its config row's toggle removes and restores the staged file); a mount without `nofail` gets the `nofail` line and a swap with it does not. T11's unowned storage masks, on Task 2.8's fixtures: for each storage kind and both mask forms, an unowned vendor file or none gives the advisory with state `masked` and, for a name other than `tmp.mount`, the `To carry it` line, and no instruction.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(pipeline): explain the administrator's mount units instead of carrying them`.

**Acceptance:** T8 and T9 pass in full, and with Task 2.8, T11.

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

**Spec:** § 11.1 `unit_states[]` fields that exist now (`unit`, `kind`, `instance_of`, `current_state`, `default_state`, `fragment`, `unit_file_path`, `owning_package`, `include`, `locked`, `triage`, `attention_reason`, `declined`), `unit_dropins[]` `kind`, `mount_findings[]` (all fields, `advisories[]` from the § 10.6 strings), `withheld` on the offering config or drop-in row, the paragraph on results caused by another row; T10 view half, T12 last sentence, T15 last clause.

**Files:** Modify `crates/refine/src/projection/{types,decisions}.rs` (calls `staged_writes` and `plan_units` on the session snapshot). Test: `crates/refine/tests/session_test.rs`, `crates/refine/tests/export_contract_test.rs`.

**Interfaces (Produces):** projection types `UnitStateView`, `DeclinedView { renders: Omitted|Fixme|Advisory, reason: <variant name>, text: String }`, `WithheldView`, `MountFindingView`.

- [ ] **Step 1: Failing tests first:** each T12 refusal appears as a structured `declined` on its row for a row whose `attention_reason` is null; the T15 `Shadowed` case and the T15 `Vendor` row with another row's unit file staged at its name each show `InstallEvidenceChanged` with that file's path on the unit row while declined, and neither shows it once the export matches again; T10 view half: the mount finding row for the planted unit exists, and the token is absent from the whole view response; a withheld conflict file shows `withheld: Conflict` on its config row; a mount finding row has no `include`.
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
- [ ] The phase report names `inspectah-masked-units-are-recorded-as-full-shadows` as closed, `inspectah-socket-units-are-never-collected` as closed pending R1 (Task 2.16), and `inspectah-srv-mount-units-must-target-var-srv` as partly covered.
- [ ] Gate; commit `docs: record unit state for every kind in skills and coverage`.

**Phase 2 ends:** a scan carries enable, disable and mask for every ordinary kind and package-owned storage unit, after the configuration `COPY`, quoted; reports the administrator's mount units; and declines, with a stated reason, any command the export would not reproduce.

**Review gate 2:** implementation review before Task 2.16. Focus: **rule 5's fragment half and rules 1, 2 and 4 against `staged_writes`, and against the fleet merge** (T15, T16, T18): a toggle on one row that changes another row's command, a merged snapshot whose rows are valid alone and not together, and a path spelled two ways.

### Task 2.16: Run R1 after phase 2 (real host, M)

**Spec:** § 13 R1; § 15 real-host table, "After phase 2".

**Needs:** the branch build at the end of phase 2; the `rhel9` and `rhel10` VMs, each reverted to its clean snapshot; the driftify **standard** profile, which reads `cockpit.socket`'s preset at run time and sets the socket against it; the project's VM operating notes, supplied at dispatch.

- [ ] On each VM, first record `cockpit.socket`'s preset on the host (the preset column of `systemctl list-unit-files cockpit.socket`), then `systemctl is-enabled cockpit.socket` in the unmodified base image; run driftify standard; scan; confirm the Containerfile holds the socket's line in the unit state section after the configuration `COPY`; build; confirm the image reads the host's state. Record under a new `### Measured, <date>: cockpit.socket end to end` in `process-docs/skills/real-host-validation-runs.md`. Revert both VMs.
- [ ] Commit `docs(skills): record the cockpit.socket run on the units branch`.

**Gate:** phase 3 does not start until the image reads the host's state on both releases. A failure is a phase 2 defect, fixed and reviewed before phase 3.

### Task 2.17: Run R3 after phase 2 (real host, L)

**Spec:** § 13 R3 (a) to (f); § 15 "R3 (a) to (c) can change how 'off' has to be rendered"; § 17 "Evidence that would change the design".

**Needs:** reverted `rhel9` and `rhel10` VMs; the phase 2 branch build for (a); probe images built from `rhel9/rhel-bootc:9.8` and `rhel10/rhel-bootc:10.2` for (b) to (f), with no inspectah build; the VM operating notes. (a) uses the driftify **standard** profile on `rhel9`, whose cases should render an `enable` (`cockpit.socket`), a `disable` (`kdump`, planted at minimal) and a `mask` (`bluetooth`); if the rendered Containerfile lacks one of the three verbs, plant one by hand and record it. (b) to (f) need no driftify profile.

- [ ] Run each sub-case as § 13 states it, reading results through a oneshot unit and the console, as the notes describe. (a), (b), (d), (e) and (f) install with `bootc install to-existing-root`. (c) is about the other routes and does not use it: install once with `bootc install to-disk`, once through Anaconda, and once from an image whose `/etc/machine-id` reads `uninitialized`, and for each record whether first boot applies presets. A route the test guests cannot run is recorded as not run, with the reason. Record each under a dated Measured subsection. Revert both VMs.
- [ ] Commit `docs(skills): record the units boot checks`.

**Gate:** may run beside phase 3. A result on § 17's list (presets applied at first boot on any route; `/tmp` writable on RHEL 9 with `tmp.mount` masked) stops the branch before phase 4 for a spec change by the project owner. Other results are recorded and change nothing.

**Note, added after this run:** it was run, and § 13 R3 now lists what it settled. An image whose machine-id reads `uninitialized` does get a preset pass, and the spec was changed for it (§ 10.1 condition, § 10.2 guard: Task 4.0a). `to-disk` was measured to deploy a zero-length machine-id and was not booted, and the installer route was not run. The routes still open are Task 5.6's.

---

## Phase 3: one control per fact (size M; 7 tasks)

Spec: § 8, § 10.2 "What the plan takes" item 3 and the tuned decision, § 11.1 `scheduled_tasks` row, § 9.1 "Aggregate timer control". Tests T19 to T21. T35 to T38 are built by the four phase 3 fixes listed after review gate 3.

Before Task 3.1: merge `main` into the branch and run the gate.

### Task 3.1: Timers become a description; the second writer goes (Rust lane, M)

**Spec:** § 8 (table, single-control rule, `SystemdTimer` reduced to `name`, `on_calendar`, `exec_start`, `description`, `source`, `path`, `aggregate`; the local-timer loop and enable removed; `config_tree_units` removed, its generated units staged with the other unit files); § 6.7 rows for `SystemdTimer`; T19.

**Files:** Modify `crates/core/src/types/scheduled.rs`, `crates/collect/src/inspectors/scheduled.rs:772-826`, `crates/pipeline/src/render/configtree.rs:440-449` (removed), `crates/pipeline/src/render/containerfile.rs:1328-1332`, `:1382-1386` (removed), `crates/pipeline/src/render/unit_plan.rs` (`config_tree_units`, today `service_intent.rs:308-329`, removed), the redaction scheduled pass and field guard. Test: `crates/cli/tests/units_scan_test.rs`, `crates/collect/tests/scheduled_test.rs`.

- [ ] **Step 1: Failing test first,** T19 through the real inspectors: a local `.timer` and paired `.service`, timer `disabled`: no enable names the timer; excluding the timer's config row removes the timer file and leaves the service file; excluding the service row too leaves neither; no other writer restores either. Positive control: the timer `enabled` gives its enable from the unit state section.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `fix(scheduled): give a local timer one control for its file and one for its state`.

**Acceptance:** T19 passes; `SystemdTimer` has no `disposition`, `locked`, `timer_content` or `service_content`.

**Note, added after this task was done:** removing the second writer lets an operator exclude a local timer's service and keep the timer. The spec now records each timer's trigger (`SystemdTimer.trigger`, § 8) and omits the timer's `enable` when that trigger is local and not staged (§ 10.2 rule 5). Phase 3 fix 3 adds both.

### Task 3.2: Remove the timer operation; link scheduled rows to their unit (Rust lane, S)

**Spec:** § 8 last bullet (refine half), § 11.1 `scheduled_tasks` timers (`unit`, `unit_row`), § 11.1 Operations "`ItemId::SystemdTimer` is removed"; T20 refine half.

**Files:** Modify `crates/refine/src/types.rs:117`, `crates/refine/src/session.rs:2190-2193`, `crates/refine/src/projection/`; compile-forced deletions in `crates/web/src/handlers.rs:949-958` and `crates/web/src/aggregate_handlers.rs:1158-1166`. Test: `crates/refine/tests/{serde_test,session_test}.rs`.

- [ ] **Step 1: Failing tests first:** a request naming `SystemdTimer` is rejected at deserialization; a scheduled timer row has `unit` and `unit_row` and no include flag; the timer's `unit_states` row keeps its flag.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `refactor(refine): remove the timer toggle that applied nothing`.

**Note, added after this task was done:** the same validation arm still accepts `SetInclude` for `GeneratedTimer`, `CronJob` and `AtJob` and applies nothing. The spec now requires each to apply or be refused by name (§ 8). Phase 3 fix 1 does it.

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

**Note, added after this task was done:** this task's collision set holds the units that have a row. The spec now counts inventory entries as well (§ 10.2 "What the plan takes" item 3). Phase 3 fix 2 widens the set and adds the rename advice.

### Task 3.5: The tuned enable goes; every line is planned (Rust lane, S)

**Spec:** § 10.2 tuned decision; § 8 single-control rule; T21 tuned case and the full-scan assertion.

**Files:** `crates/pipeline/src/render/containerfile.rs:1885`. Test: `crates/cli/tests/units_scan_test.rs`.

- [ ] **Step 1: Failing test first,** T21: a snapshot with flatpak applications, tuned activation, a generated timer and collected state changes: every `RUN systemctl` instruction in the rendered Containerfile matches a `PlannedUnit`, found by scanning the output; the kernel section writes the tuned profile and no `systemctl` line; `tuned.service` enabled against a `disable` preset is enabled from the unit state section, and omitted when the `tuned` package is excluded.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `fix(pipeline): leave tuned.service to its unit row`.

**Acceptance:** T21 passes in full.

### Task 3.6: Declined generated units on their rows (frontend lane, S)

**Spec:** § 11.1 "A declined generated unit is shown on its scheduled or flatpak row"; T21 refine half.

**Parallel:** can run beside Task 3.5 against Task 3.4's projection Interfaces; merges after Task 3.4.

**Files:** `crates/web/src/{web_types,adapter}.rs`, `crates/web/ui/src/api/types.ts`, the scheduled tasks component and `crates/web/ui/src/components/ContainerSection.tsx` (where flatpak applications are listed). Test: `crates/web/tests/api_test.rs`, Vitest.

- [ ] **Step 1: Failing tests first:** through the production handlers, a generated cron timer declined for a name collision carries its `declined` reason and text on its scheduled row; the flatpak provisioning service declined the same way shows it on the flatpak row; Vitest renders both with the reason text.
- [ ] **Step 2-4:** run, implement, frontend checks, gate. **Step 5: Commit** `feat(web): show a declined generated unit on its row`.

### Task 3.7: Phase 3 close (Rust lane, S)

- [ ] Update `process-docs/skills/service-omission-model.md` (generated units and flatpak in the plan), `docs/reference/inspector-coverage.md` (scheduled tasks describe; units control), `codebase-layout.md` if a module moved.
- [ ] The phase report names `inspectah-local-timer-has-two-controls-and-neither-works` and `inspectah-timer-enable-lines-unquoted-and-trust-empty-units` as closed.
- [ ] **CHANGELOG.md**: **Fixed** a disabled local timer is no longer enabled in the image, and a vendor timer enabled against its preset is carried. **Removed** the scheduled timer toggle, which applied nothing, and the unconditional `tuned.service` enable (the unit's own row decides it).
- [ ] Gate; commit `docs: record one control per timer fact`.

**Phase 3 ends:** every `systemctl` line in the Containerfile is a planned one; a timer's state has one control and its file another.

**Review gate 3:** implementation review before Task 4.0. Focus: no second writer remains for any unit file; generated-name collisions through both export routes.

### Phase 3 fixes, after review gate 3 (Rust lane; fixes 1 and 4 have a frontend piece)

Four rules the spec gained once the phase's tasks were built. Each is one test-first fix on the branch before phase 3 closes, with the usual gate, and is not a numbered task. Build them in this order; fix 3 is last, since its check reads the final `staged_writes`, which fixes 2 and 4 change.

- **Fix 1: the scheduled controls apply (§ 8, § 11.1; T35).** Files: `crates/refine/src/session.rs` (validation and apply arms for `GeneratedTimer`, `CronJob` with its cascade to the units it generates, and `AtJob`, refused), `crates/refine/src/projection/` (the generated rows' include state, and the units of an excluded cron job excluded in every projection), `crates/core/src/types/scheduled.rs` (`GeneratedTimerUnit::generated_from`), `crates/web/src/handlers.rs` (the bulk toggle sends no at-job operation and no cron-job operation for a cron job that generates nothing). Test: T35 in `crates/cli/tests/units_scan_test.rs`, built by the real inspectors and asserted through the refine session and its export: the generated-timer row removes and restores its unit; a cron-job row drops and restores the units it generates, and including a generated unit under an excluded cron job is `BadRequest` with the export unchanged; a cron job that generates nothing is `NoopTarget` and an at job is `BadRequest`, the Containerfile unchanged. The bulk toggle in `crates/web/tests/api_test.rs` (`the_scheduling_group_toggle_carries_and_drops_generated_units`): the exact operation list and its effect on the staged tree and the plan. **Frontend piece (§ 11.1):** `crates/refine/src/projection/types.rs` and `reference.rs` (`GenericRefItem.scheduled`, `ScheduledSource`), `crates/refine/src/projection/decisions.rs` (a name's `include` is true while any record of it is included), `crates/web/src/web_types.rs` and `adapter.rs` (`generated_timers[]` with `cron_job_excluded`, computed with the session's refusal test; scheduled row ids by kind and source; `ContextItem.generated_timer`), `crates/web/ui/src/api/types.ts`, `MainContent.tsx` and `ContextItem.tsx` (the generated row's toggle, the cron-job text in its place, a failed toggle kept on the row). Tests in `crates/web/tests/api_test.rs`: `a_generated_timer_row_is_decided_on_its_row`, `a_generated_name_with_one_included_source_shows_included`, `each_scheduled_row_has_its_own_id`, `a_scheduled_timer_without_a_path_is_identified_by_name`. Vitest: `GeneratedTimerControl.test.tsx` (the toggle, the cron-job text, the same-name case, a kept failure and its retry, and its end on Undo and Redo), `ScheduledRowIds.test.tsx`, `MainContent.declined-generated.test.tsx`, the generated-timer row of `PendingGuards.test.tsx`.
- **Fix 2: an inventory name blocks a generated pair (§ 10.2 item 3; T36).** Files: `crates/pipeline/src/render/configtree.rs` (the collision set in `staged_writes`, now with inventory names and mount findings, `Reservation`, `WithheldWhy::GeneratedNameCollision { unit, reserved_by }`), `crates/pipeline/src/render/unit_plan.rs` (`OmissionReason::GeneratedNameCollision { name, reserved_by }` and the two FIXME texts, each naming the collided unit). The flatpak case is unchanged. Test: T36 in `crates/cli/tests/units_scan_test.rs`, through the scan export, the refine export and the refine projection, with the service-only row collision beside it (`a_generated_timer_declined_for_its_service_names_the_service`); `crates/pipeline/tests/unit_plan_test.rs` and `crates/web/tests/api_test.rs` follow the type.
- **Fix 4: an unmanaged file under `/etc/systemd/system` is refused (§ 10.2 "No other writer", § 11.1; T38).** Files: `crates/pipeline/src/render/configtree.rs` (the one shared decision, `unmanaged_unit_path`, by `Destination`, covering nested paths; `WithheldWhy::UnmanagedUnitPath` and the `StagedFileKind::UnmanagedFile` source kind on the withheld record), `crates/pipeline/src/render/unmanaged.rs` (the renderer asks the decision: no `COPY`, no `ln`), and both payload routes, which ask the same decision: `crates/cli/src/commands/scan.rs` (`bundle_unmanaged_files`) and `crates/refine/src/session.rs` (`extract_payload_dirs_with_budget`, where the export extracts payload; `crates/refine/src/tarball.rs` only unpacks a scan to load the session), then `crates/refine/src/projection/` (`unmanaged_withheld`). Test: T38 in `crates/cli/tests/units_scan_test.rs`, an imported snapshot and tarball through the refine export and the scan export, asserting the extracted tree, the `COPY` and `ln` lines, the FIXME in the Configuration Files section and `unmanaged_withheld`. Scan bundling has its own test, `bundling_writes_no_payload_under_the_unit_directory` in `crates/cli/src/commands/scan.rs`: files and symlinks under the unit directory, spelled plainly and with `.` and doubled separators, write nothing there, and an ordinary file and symlink are bundled. **Frontend piece:** `crates/web/src/web_types.rs` and `adapter.rs` (`UnmanagedFileItemDto.withheld`, which does not exist today), `crates/web/ui/src/api/types.ts` and the unmanaged files component show `withheld` on the row, tested in `crates/web/tests/api_test.rs` (`an_unmanaged_file_at_a_unit_path_is_withheld_on_its_row`, with the interior `.` spelling) and in `UnmanagedFileList.test.tsx`; it follows the Rust commit, in the frontend lane.
- **Fix 3: a timer needs its trigger (§ 8, § 10.2 rule 5, § 6.7 rows; T37). Built last.** Files: `crates/core/src/types/scheduled.rs` (`TimerTrigger`, defaulting to `Unresolved`, `SystemdTimer.trigger`, `SystemdTimer.trigger_drop_ins`), `crates/collect/src/inspectors/scheduled.rs` (record `Unit=` from the timer file and `<name>.timer.d` under `/etc/systemd/system` and `/usr/lib/systemd/system`, else the same-named service; expand `%i`, `%I`, `%p`, `%N`; anything else is `Unresolved`; list the `/etc` drop-ins that assign `Unit=`), `crates/pipeline/src/render/unit_plan.rs` (`TimerTriggerEvidenceChanged`, `TimerTriggerUnresolved`, `TimerTriggerNotStaged`, in that order, after the timer's own file and package), `crates/pipeline/src/render/mod.rs` (the field guard). Test: T37 in `crates/cli/tests/units_scan_test.rs`, built by the real inspectors through both exports and the refine row: the excluded trigger, with the default service and with `Unit=other.service`; the excluded drop-in that sets `Unit=`, together with the default service, giving `TimerTriggerEvidenceChanged` though the recorded trigger is staged; an unexpanded specifier giving `TimerTriggerUnresolved`; and the triggers that pass. The reader's cases, `%i` expansion for an instance timer among them, are collector unit tests in `crates/collect/src/inspectors/scheduled.rs`, and `crates/collect/tests/scheduled_test.rs` gains the `trigger` key. The systemd behavior the rule rests on is read from source; Task 4.0 measures it.

The changelog lines are Task 3.7's, or a follow-up to it if it is already done.

---

## Phase 4: the `/etc/systemd/system` scan (size L, above § 15's estimate; 17 tasks)

Spec: § 6.3 (`also`, `install_drop_ins`, `vendor_drop_ins`), § 6.6, § 7.1 sources 3 to 5 and the search path, § 7.2 (all), § 7.3, § 7.4 (instance preset, stock state, a unit at its preset whose `/etc` files change `[Install]`), § 7.5 (first three paragraphs), § 7.6, § 7.7 (`*.upholds/*`), § 10.2 (install files, rule 5 drop-in half, rule 7), § 10.5, § 10.9, § 11.1 (`manual_links`, `default_target`, `also`, `install_drop_ins`), § 12 (list fields, `manual_links`, `default_target`). Tests T22 to T27, T25 (a) to (m), T30 to T34, T39 to T42. Tasks 4.0a and 4.0b are follow-on work for rules decided after phases 1 and 2 were done; they do not depend on the scan and come first.

### Task 4.0: Run R5 before phase 4 (real host, M)

**Spec:** § 13 R5; § 15 real-host table, "Before phase 4's scan-dependent code starts".

- [x] Run on 2026-11-14 on RHEL 9.8 and RHEL 10.2, every case a unit planted by hand, both VMs reverted after. Recorded in `process-docs/skills/real-host-validation-runs.md`, section "Measured, 2026-11-14: the small systemd facts phase 4 relies on" (`docs(skills): record the small systemd facts phase 4 relies on`). The spec carries each answer where it relies on it.

Each answer, checked against the tasks:

| Answer | Tasks |
|---|---|
| An empty `WantedBy=` or `Alias=` in a drop-in resets the list; an empty `Also=` does not; `RequiredBy=` was not tried. The reader models the reset (§ 7.2) and declines an empty `RequiredBy=` after a value. | 4.1 (the rule), 4.3 and 4.5 (T24 reset cases), 4.5a and 4.5b (T30 (k) to (n), T31, T32) |
| `enable` reads `[Install]` only from the unit's own `.d` and the template's `.d`; same-name hiding holds in those two and not from the type-wide directory. | 4.1 (`drop_in_dirs`), 4.2 (the search-path drop-in test), 4.3 (install files, `vendor_drop_ins`), 4.8 (T25 (h)) |
| The search path is the documented order, `generator.late` last. A concrete instance file loads over its template wherever each sits. | 4.2: the constant stands; the instance test refuses an instance file on the other directories whatever their rank |
| `disable` with an absent `Also=` member exits 0 on both releases. | 4.8: rule 7's text states it; no product change |
| `systemctl preset` on a template enables its `DefaultInstance=` under a bare rule and the listed instances under the list form, and ignores rules that name an instance; on an instance name it applies a rule naming that instance or listing it, and with only the bare rule it links nothing. | 4.4: the lookup by what the package ships, an instance's own name when it has a unit file and the template's first rule otherwise (decided after the run); 4.6: T39's state changes for collected instances; 5.2: disabled template-backed instances derived from the presets |
| The `systemd-fsck@` instance for `/boot/efi` has no link and reads `static`. | 4.5: T23's `systemd-fsck@` case stays a hand-made link |
| The additive sequence gives one new link and leaves the existing one untouched, for a drop-in and a full shadow. | 4.5a and 4.5b stand |
| A timer whose service is not in the image is refused at start, with no failed unit. | `TimerTriggerNotStaged` (phase 3 fix 3) stands |
| Both bases enable `tuned.service` by preset after `dnf install tuned`. | The removal of the kernel section's enable (Task 3.5) stands |

Still open, needing no inspectah build and gating no phase 4 code; they run with the next real-host run, at the latest in Task 5.6: `systemd-cryptsetup@` on a host with an encrypted volume (neither guest had one); an empty `RequiredBy=` after a value, and an empty `UpheldBy=` on RHEL 10, in an `/etc` drop-in; the search of both bases' stock preset files for a list-form rule (Task 4.4's last step); the additive sequence for a unit with an existing alias link; hiding between an instance's own `.d` and its template's `.d`, in both placements; for an instance with its own unit file, whether a rule that does not count for it lets later rules decide; and the names the stock scriptlets of template-owning packages pass to `systemctl preset` (§ 13 R5 gives each case).

**Gate:** met. Tasks 4.1 to 4.8, 4.5a and 4.5b are written to the answers and may start.

Before Task 4.0a: merge `main` into the branch and run the gate.

### Task 4.0a: Follow-on: the machine-id is never staged (Rust lane, S)

**Spec:** § 10.1 "First boot keeps it with the base's empty `/etc/machine-id`" (the decision and the condition); § 10.2 "Never the machine-id" and `WithheldWhy::MachineId`; § 11.1 `withheld` on the owning row; T33.

**Files:** `crates/pipeline/src/render/unit_plan.rs` or wherever Task 2.6 put `staged_writes` (one more withhold test, by `Destination`), `crates/pipeline/src/render/readme.rs` (the two first-boot sentences, worded as § 10.2 gives them: no "only while"), `crates/refine/src/projection/` (the new `WithheldWhy` variant, text only). Test: `crates/pipeline/tests/unit_plan_test.rs`, `crates/cli/tests/units_scan_test.rs`.

- [ ] **Step 1: Failing tests first,** T33, through the scan export and through the refine export: a snapshot offering `/etc/machine-id` through a config entry, and separately one offering it through a drop-in row's path, each with the path spelled relative (`etc/machine-id`), with a doubled separator, with an interior `.` and with a trailing separator, stages nothing there, the Containerfile names it in a FIXME, and each offering row shows `withheld` with `MachineId`, two rows that offer the one destination included (the guard runs before offers are deduplicated); `/etc/machine-info` offered in each of the same ways is staged; a host scan records no entry for the path (`walk.rs:45` already excludes it: assert it, do not change it); the exported README holds the two first-boot sentences. One more assertion, not in T33: no rendered file or instruction writes `uninitialized`.
- [ ] **Step 2-4:** run (fails: the imported entry is staged), implement, gate. **Step 5: Commit** `fix(pipeline): never stage a machine-id, and say when first boot keeps unit state`.

**Acceptance:** T33 passes.

**As built (`f3021695`), and how the spec now states it:** the guard runs as soon as an offer's `Destination` is built, before `owned_elsewhere` drops a config entry at a drop-in's path and before deduplication; an unmanaged file at the path is withheld as `MachineId` through the shared `unmanaged_withheld` check that scan bundling, refine extraction, the unmanaged-files render and `staged_writes` ask; the FIXME text is the `MACHINE_ID_KEPT` string, quoted in § 10.2; and the README sentences are in the scan export only, since a refine export writes no README.

### Task 4.0b: Follow-on: a mask follows its package (Rust lane, S)

**Spec:** § 10.2 rule 5 "A mask follows the package check" (the three outcomes, the order against rules 3 and 4, the zero-length shadow, the fleet owner) and the rules table's row 5; T34.

**Files:** `crates/pipeline/src/render/unit_plan.rs` (the presence tiers, unchanged, now asked for `mask`). Test: `crates/pipeline/tests/unit_plan_test.rs`, `crates/cli/tests/units_scan_test.rs`.

- [ ] **Step 1: Failing tests first,** T34: a masked vendor unit whose owning package is excluded and not in the baseline gives no `mask` and an `# Omitted:` line with the package reason; the same mask with the package included is carried; excluded and in the baseline is carried; no known owner, and an `Absent` fragment, are carried; a masked protected unit whose package is excluded gives rule 3's FIXME; a mask whose path a staged file occupies, package excluded, gives `MaskPathOccupied`. Two existing tests in `crates/pipeline/tests/unit_plan_test.rs` assert the old exemption and are changed here: in `units_of_a_not_carried_package_are_omitted_as_not_carried_on_the_scan_path`, the masked case becomes an omission with the not-carried reason; and `a_mask_is_carried_when_its_package_is_excluded_and_an_enable_is_not` becomes an omission with the excluded-package reason for the mask as well as the enable, renamed to say so. Replace both assertions and their doc comments; do not delete the cases. The carried side is T34's included-package and in-baseline controls. T11, T12 and T16 stay unchanged and must still pass.
- [ ] **Step 2-4:** run (fails: the mask is carried with its package excluded), implement, gate. **Step 5: Commit** `fix(pipeline): omit a mask whose unit's package is not in the image`.

**Acceptance:** T34 passes. The refine row for an omitted mask shows the package reason through the existing `declined` projection.

### Task 4.1: The `[Install]` reader (Rust lane, M)

**Spec:** § 7.2 `InstallSection`, `InstallRead`, `InstallUnknown` (`UpheldBy` for any assignment, an empty one included; the derives, since the reason is stored on findings), step 4 (the unit's own `.d` and, for an instance, the template's `.d`, under both roots; type-wide and dash-prefix directories never read; `%i`, `%I`, `%p`, `%N` expanded) and its load order (the main file first, then one drop-in per file name in file-name order across every directory that applies, not root by root; `/etc` over `/run` over `/usr/lib` for one directory name; the own and template `.d` treated as one set of names, the higher-priority root winning and the own `.d` winning within a root, assumed and not measured), the reset rule (decided) and the two keys it does not cover, "The reader is a pure function over text"; § 7.3 `UpheldBy=` is `Undetermined(UpheldBy)`.

**Files:** Create `crates/collect/src/inspectors/units/install.rs`. Test: `#[cfg(test)]` there.

**Interfaces (Produces):** `pub(crate) fn read_install(name: &UnitName, fragment: &str, drop_ins: &[(DropInSource, &str)]) -> InstallRead`; `pub(crate) fn drop_in_dirs(name: &UnitName) -> Vec<String>` (the `.d` directory names `enable` reads `[Install]` from: `<name>.d` and, for an instance, `<template>.d`; the caller looks for each under both roots).

- [ ] **Step 1: Failing tests first,** one table: `WantedBy=` from the fragment, from an `/etc` drop-in, from a `/usr/lib` drop-in, and from the template's `.d` for an instance; a later drop-in adds; drop-ins apply in file-name order across the unit's own and the template's `.d` and across both roots, not root by root (`10-a.conf` under `/usr/lib` applies before `20-b.conf` under `/etc`); an `/etc` file replaces a vendor file of its name in the same directory; an `/etc` file in `t@x.service.d` replaces a vendor file of its name in `t@.service.d`, and of two vendor files of one name the one in `t@x.service.d` is read (the assumed rule). Resets, each over `WantedBy=multi-user.target` in the fragment: a drop-in holding only `WantedBy=` is `Known` with nothing expected; `WantedBy=` then `WantedBy=graphical.target` in one drop-in expects `graphical.target` only; `WantedBy=` in `10-a.conf` and `WantedBy=extra.target` in `20-b.conf` expects `extra.target` only, and `WantedBy=extra.target` in `10-a.conf` with `WantedBy=` in `20-b.conf` expects nothing; an empty `Alias=` clears the fragment's alias; an empty `Also=` keeps the fragment's member; an empty `RequiredBy=` after `RequiredBy=a.target` clears it, as an empty `WantedBy=` does. `UpheldBy=`, and an empty `UpheldBy=`, are `Undetermined(UpheldBy)`; `%H` is `Undetermined(Specifier)`; `%i` expands for an instance; a `.device` value is `ForeignName`; no section is `Absent`; `[Install]\r\n` and a value with trailing `\r` read as plain (Review focus 3). `drop_in_dirs("a-b-c.service")` is `["a-b-c.service.d"]` and `drop_in_dirs("t@x.service")` lists `t@x.service.d` and `t@.service.d`; neither lists `service.d` or a dash-prefix directory.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(collect): read a unit's effective [Install] section`.

### Task 4.2: The effective unit and the search-path test (Rust lane, M)

**Spec:** § 7.1 "The search path" (the constant, `generator.late` outside it, both refusal conditions: a unit file systemd loads in place of the chosen fragment, a concrete instance file on the list whatever its rank when the template was chosen, and a drop-in in the name's own or the template's `.d` only), § 7.2 `Resolved::Unsupported`, step 1 and step 2's reverse placement; T24 search-path and concrete-instance cases.

**Files:** Modify `crates/collect/src/inspectors/units/resolve.rs`, `crates/collect/src/inspectors/units/mod.rs`. Test: `crates/collect/tests/units_test.rs`.

- [ ] **Step 1: Failing tests first:** a concrete `worker@site.service` under `/etc` beside a vendor template is `Local` with `file` the instance; a concrete instance under `/usr/lib` beside its template under `/etc` has the instance file as its fragment; an instance with no file has the template as `file`; a concrete instance file under `/run/systemd/system`, a copy under `/usr/local/lib/systemd/system`, and a drop-in with `[Install]` in the unit's own `.d` under `/run/systemd/system` each give a warning naming the path and no state change, and the same unit without the extra file gives its row; a concrete instance under `/usr/local/lib/systemd/system` with its template under `/etc` gives the warning; a template under `/run/systemd/system` beside a concrete instance under `/usr/lib` gives no warning; a drop-in with `[Install]` in `/run/systemd/system/service.d` gives no warning. T24's render assertions for the concrete instance (carried with the template's package excluded and its own file staged; `UnitFileNotStaged` with its file excluded) are deferred to Task 5.2: until then every instance is omitted by Task 4.6's placeholder, which would decide first.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(collect): refuse units the systemd search path would change`.

### Task 4.3: Install drop-ins and the drop-in half of rule 5 (Rust lane, M)

**Spec:** § 6.3 `install_drop_ins`, `vendor_drop_ins`; § 6.7 rows for both (names only, `.conf`, one component); § 7.2 the two paragraphs after the reset (install evidence, `vendor_drop_ins` from the unit's own and the template's `.d`); § 10.2 "Install files" (the unit's own and the template's `.d` only, nothing else), the decision on same-named drop-ins, the lexical `[Install]` test, rule 5 "for every fragment" paragraph (exact set, `Destination` parse, aggregate variant, instance own-name file); T24 drop-in and reset cases, T25 (k).

**Files:** `crates/core/src/types/units.rs`, `crates/collect/src/inspectors/units/mod.rs`, `crates/pipeline/src/render/unit_plan.rs`. Test: `crates/pipeline/tests/unit_plan_test.rs`, `crates/cli/tests/units_scan_test.rs`.

- [ ] **Step 1: Failing tests first,** T24: a `WantedBy=` only from a drop-in is carried with the drop-in in `install_drop_ins`; that drop-in excluded, or a second `[Install]` drop-in in the unit's own `.d` staged that the row does not list, gives `InstallEvidenceChanged` and no enable; an `install_drop_ins` path holding `..` gives the same; an `/etc` drop-in with no `[Install]` and the file name of a vendor drop-in in the unit's own or the template's `.d` is in `install_drop_ins`, and one with its own file name is not. Type-wide and dash-prefix directories: a vendor drop-in holding `WantedBy=multi-user.target` in `service.d` or in `a-.service.d` for `a-b.service` is not in `vendor_drop_ins` and makes no expected link; an `/etc` drop-in with `[Install]` and `WantedBy=multi-user.target` there is not in `install_drop_ins`, and staging it beside a carried enable leaves the enable carried; an `/etc` file in `service.d` named like a vendor drop-in in the unit's own `.d` is not an install file and does not hide it. Resets, through the scan: a drop-in holding `WantedBy=` then `WantedBy=extra.target` over the vendor's section is in `install_drop_ins` like any other install drop-in; an empty `RequiredBy=` after a value clears it, so the section names no link, with no state change and no enable (its advisory is Task 4.5's). T25 (k) first half: the vendor `bundle.service.d/10-members.conf` case with the `/etc` file included carries the enable and lists the file, excluded gives `InstallEvidenceChanged`. A `vendor_drop_ins` value with `/` or without `.conf` fails to load. The lexical test is true for `  [Install]  `, `[Install]\r`, and a backslash-continued assignment (Review focus 3).
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(pipeline): carry an enable only with the [Install] files it was recorded from`.

### Task 4.4: Install state from the links (Rust lane, L)

**Spec:** § 7.1 sources 2 to 4 (source 2 keeps the instance list), § 7.3 (sets, the table with a section whose lists were reset to empty in the nothing-expected row and the three `Undetermined` rows: no link and no `Also=` is `Disabled`, no link with `Also=` is an `UndeterminedInstall` (recorded by Task 4.5), some link is no state; three points except `.upholds` advising, which is Task 4.5), § 7.4 the preset for an instance (decided: by what the package ships; first case, an instance with its own unit file, by its own name, with rules naming it, globs matching it and list-form rules listing it, other template rules letting later rules decide, assumed; second case, a template-backed instance, by the template's first matching rule through the second table, instance-name rules not read; a matching `Unknown` line stops with no preset and one structured warning per line naming the line, its file and the units it decided, for every unit; the `PresetRule` type with `Unknown`'s file and line) and the stock-state constant, § 6.4 "a unit that systemd calls `enabled` on the strength of a hand-made link is `Disabled`"; T22 (its getty fixture with `disable *` after the bare rule); T39's lookup, parser and warning half.

**Files:** `crates/collect/src/inspectors/units/mod.rs` (today's `read_preset_rules`, `PresetRule` and `resolve_preset_default` there: the parser keeps the list form as `PresetRule::EnableInstances` and an unread verb as `PresetRule::Unknown` with its file and line; one matcher serves every unit, with the instance rows of § 7.4 for an instance), `crates/core/src/baseline.rs` (stock-state constant, one entry: `remote-cryptsetup.target`, `Disabled`). Test: `crates/collect/tests/units_test.rs`, and the parser cases in `#[cfg(test)]` in `units/mod.rs`.

**Interfaces (Produces):** `fn preset_for(unit: &UnitName, own_file: bool, default_instance: Option<&str>, rules: &[PresetRule]) -> PresetLookup`, internal to the collector, where `PresetLookup` is `Rule(PresetDefault)`, `Unknown { file, line }` or `NoRule`: for a unit that is not an instance, or an instance with its own unit file, the first rule in file order that matches its name (§ 7.4 first table for an instance); for a template-backed instance, the template's first matching rule read through § 7.4's second table. `Unknown` feeds the warning and gives the inventory entry no preset. Task 5.2 reuses the second-table half to derive candidates.

- [ ] **Step 1: Failing tests first,** T39's lookup, parser and warning half: for each T39 rule set, what the lookup gives each instance. With its own unit file `t@two.service`: `enable t@two.service` then `disable *`, `enable`; `enable t@*` then `disable *`, `enable`; `enable t@.service two three` then `disable *`, `enable`; a bare `enable t@.service` then `disable *`, `disable`. Template-backed: `enable t@.service` then `disable *`, `t@one` `enable` and `t@two` `disable`, and with no `DefaultInstance=` `t@one` `disable`; the list form then `disable *`, `t@two` `enable` and `t@one` `disable`; `enable t@two.service` then `disable *`, `t@two` `disable`; `disable t@.service`, `disable`; `frobnicate t@.service` before `enable t@.service`, `Unknown` with its file and line and one warning naming the line, its file and `t@one`; no matching rule, `NoRule`. RHEL 9.8's and RHEL 10.2's getty rules: `getty@tty1` `enable` from `90-systemd.preset` on both, `getty@tty2` `disable` on both. The parser's variants for each T39 line. The `Undetermined` rows of § 7.3: a vendor unit with `UpheldBy=` and no link is `Disabled`; with a link, no install state. T22's state assertions:
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `fix(collect): read enablement from the links, not systemctl's word`.
- [ ] **Step 6, with the next real-host run, at the latest Task 5.6; it does not block this task's code or commit.** Search both bootc bases' stock preset files (`/usr/lib/systemd/system-preset/*.preset`) for an `enable` line with more than two words. Record the files searched and every match, or that there is none, in `process-docs/skills/real-host-validation-runs.md`, so § 7.4 can say whether a stock host meets the list form. A match goes to the project owner and becomes a T39 fixture (§ 17).

**Acceptance:** T22's state assertions, T24's reset install states, T39's lookup, parser and warning half and the `Undetermined` install states pass; the inventory state is the derived one.

### Task 4.5: Hand-made links and their advisories (Rust lane, M)

**Spec:** § 6.6, § 7.3 rows producing `ManualLink`, alias handling, `.upholds` advising, unparseable link names warned; § 7.5 "Instances tied to one host's devices" (constant); § 7.7 `*.upholds/*` walk exclusion (`walk.rs:154-155`); § 6.6 `ManualLinkCause::Undetermined { reason }` and `UndeterminedInstall`, § 6.4 `undetermined_installs`; § 7.3 the `Undetermined` row (one `UndeterminedInstall` when the unit has an `/etc` install file, whether or not it has a link); § 10.9 (the `StaticUnit` text that does not say "made by hand", the `Undetermined` reasons in words, the unit-level block for an `UndeterminedInstall`); § 6.7 guard gains `ManualLink` and `UndeterminedInstall`; § 11.1 `manual_links[]` (`cause` with its `reason`) and `undetermined_installs[]` projections; T23; T22's link findings and advisories; T24's reset links; T40 (the omission line when its file is excluded is Task 4.5b's).

**Files:** `crates/core/src/types/units.rs`, `crates/collect/src/inspectors/units/mod.rs`, `crates/collect/src/inspectors/config/walk.rs`, `crates/pipeline/src/render/unit_plan.rs`, `crates/pipeline/src/render/mod.rs`, `crates/refine/src/projection/`. Test: `crates/cli/tests/units_scan_test.rs`.

- [ ] **Step 1: Failing tests first,** T23: the `StaticUnit` link and its refine row exist; then the advisory holds the `add-wants` line, no `RUN` names the unit, the row has no include; table cases for `.requires`, an unexpanded specifier, `systemd-fsck@` (no command line) and a hand-made alias; `UpheldBy=multi-user.target` with its `.upholds` link through the units and config inspectors: no state change, no enable, the restart-loop text, the link's path absent from `config.files` and the staged tree. T22 link half, on Task 4.4's fixtures: one `NotInInstall` link for the `rescue.target.wants/` case, an advisory for the extra link when both links exist, `PartialInstall` for two `WantedBy=` and one link, and no advisory for the link shipped under `/usr/lib`. T24 resets: a local unit with `WantedBy=multi-user.target` and an `/etc` drop-in holding only `WantedBy=`, with the `multi-user.target.wants/` link still on the host, gives a `StaticUnit` link, its advisory in the new wording and its `add-wants` line; an empty `RequiredBy=` after `RequiredBy=a.target`, with the `a.target.requires/` link still on the host, gives a `StaticUnit` link with its `add-requires` line and no unit-level block. T40 except its excluded-file case: the `RequiredBy=`-only unit with an `UpheldBy=` drop-in and no link has its `UndeterminedInstall`, the § 10.9 block naming the unit, the reason and the drop-in, the refine row, and no `systemctl` line; the unit with no `/etc` file has no record. T40's `UpheldBy=` vendor unit with no `/etc` file: no link, a `Disabled` state change rendering `disable` and no record; one link, the per-link advisories; with `Also=helper.service` and no link, no state change and one `UndeterminedInstall` with no files, its block without the file clause.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(collect): report hand-made unit links with the command that recreates them`.

**Acceptance:** T23 and T40 (without its excluded-file case) pass, and with Task 4.4, T22.

### Task 4.5a: The vendor-side read and the install comparison (Rust lane, M)

**Spec:** § 7.2 "The vendor-side read"; § 7.4 "A unit at its preset whose `/etc` files change `[Install]`" (the decision, which units the test applies to, `Enabled` or no install state because the effective section names no link, the two pairs of sets, the four-row table, "A unit whose `/etc` files leave it no link", including that a link the package's section names and the host keeps produces nothing, and that a reset unit still `Disabled` gets only its row; the other-preset paragraph); § 6.6 `LinkRelation::Also`, `ManualLinkCause::HostInstallOnly` and `VendorInstallOnly`; T30.

**Needs:** Task 4.1 (`read_install`), Task 4.3 (`install_drop_ins`, `vendor_drop_ins`), Task 4.4 (install state), Task 4.5 (`ManualLink`).

**Files:** `crates/core/src/types/units.rs` (the relation and the two causes), `crates/collect/src/inspectors/units/install.rs` (a second call of `read_install` over the vendor fragment and vendor drop-ins only), `crates/collect/src/inspectors/units/mod.rs` (the comparison, after the install state is known). Test: `crates/collect/tests/units_test.rs`.

**Interfaces (Produces):** `pub(crate) fn compare_install(effective: Option<&InstallSection>, vendor: &InstallRead) -> InstallDifference`, where `effective` is the unit's effective section (`None` when it has none; the caller runs the comparison only for a `Known` or `Absent` effective section) and `InstallDifference` is `Same`, `Additive`, `NotAdditive { host_only, vendor_only, also_host_only, also_vendor_only }` or `VendorUndetermined { reason }`. The caller maps a template in `Also=` to its `DefaultInstance=` on both sides first. It is internal to the collector and is not stored.

- [ ] **Step 1: Failing tests first,** T30 (a) to (n) through the real inspectors ((a) asserts first that the shadow is in `drop_ins` and the unit has an `Enabled` inventory entry, then that no row and no advisory exist; (i) is the `Undetermined` vendor-side section; (j) is a vendor link that is on the host by hand, which is not `VendorInstallOnly`; (k) is the reset-only drop-in and the shadow with no `[Install]`, a unit with no install state, asserting first that the drop-in or shadow is collected, then the `VendorInstallOnly` link, and with the package's link kept on the host no `ManualLink` of any cause for it; (l) is reset-then-add, compared like any other drop-in; (m) the `Alias=` reset; (n) the `Also=` reset that changes nothing, asserting the drop-in and the `Enabled` inventory entry before the absence; (o) the `[Service]`-only control on a unit with no `[Install]`; (p) the reset-then-add drop-in on a host without the new link, a `Disabled` row with no install-difference link), asserting the state change or its absence, the fragment, `install_drop_ins`, the attention note, and each `ManualLink` with its relation, cause and `install_files`. A unit test on `compare_install`: reordered values are `Same`; an added `Alias=` is `Additive`; equal links with a differing `Also=` name are `NotAdditive`; an effective section that names no link against a vendor section with one is `NotAdditive` with that link in `vendor_only`; an `Undetermined` vendor section is `VendorUndetermined`.
- [ ] **Step 2-4:** run (fails: no row and no link finding for any divergent unit, and none for a unit with no install state), implement, gate. **Step 5: Commit** `feat(collect): compare a unit's /etc [Install] with its package's`.

**Acceptance:** T30 passes. A shadow that leaves `[Install]` alone produces no row and no finding.

### Task 4.5b: The added links carried, the changed ones advised (Rust lane, M)

**Spec:** § 7.4 "An additive row is planned like any other" (rule 5 on the shadow or the drop-ins, rule 7 if the unit has members); § 7.4 "An install-difference advisory holds only while the files that cause it are in the export, and an exclusion that breaks it is said" (`ManualLink.install_files` and `UndeterminedInstall.install_files`, read through `Destination`; the repair lines suppressed and one `# Omitted:` line per unit naming every excluded file); § 10.9 "An excluded install file is said once per unit"; § 10.9 the `HostInstallOnly` and `VendorInstallOnly` rows, the `add-wants` line for the first, the quoted `rm --` line for the second, the alias and `Also` wording; § 11.1 `manual_links[]` with the new relation and causes, and `not_applied`; § 6.7 row for `install_files`; § 11.3 "The command, which is the exception"; T31, T32.

**Needs:** Task 4.5a; Task 4.3 (rule 5's drop-in half); Task 4.5 (the link advisory renderer).

**Files:** `crates/pipeline/src/render/unit_plan.rs`, `crates/refine/src/projection/`. Test: `crates/cli/tests/units_scan_test.rs`, `crates/pipeline/tests/unit_plan_test.rs`.

- [ ] **Step 1: Failing tests first,** T31: the units of T30 (b), (c), (e) and the first case of (l) are in `RUN systemctl enable` after the configuration `COPY`; with the drop-in of (b) or the shadow of (c) excluded, `InstallEvidenceChanged` and no enable; the unit of (a) is in no `systemctl` line. T32: for (d), the `HostInstallOnly` block with its `add-wants` line, the `VendorInstallOnly` block with `RUN rm -- '<path>'`, a unit named with `\x2d` keeping its backslashes inside the quotes, no `systemctl` instruction naming the unit, and two refine rows with no include field; for (f) and the first case of (k), the removal line and no `systemctl` instruction naming the unit; for (g), the `Also` text and no command line; a `VendorInstallOnly` alias names `'/etc/systemd/system/<alias>'`. With the replacing shadow of (d) excluded: no `rm` line, no `add-wants` line, no command for the unit, one `# Omitted:` line naming the unit and the shadow, and the two refine rows marked `not_applied` with the shadow's path and that line in `advisories[]`; included again, both blocks return and the line goes. T41 in full. T40's excluded-file case: the unit's `# Omitted:` line naming the drop-in, and the `undetermined_installs[]` row `not_applied` with its path.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(pipeline): carry added [Install] links and advise changed ones`.

**Acceptance:** T31, T32, T41 and T40's excluded-file case pass. No `systemctl` verb other than `enable`, `disable`, `mask` and `set-default` is rendered.

### Task 4.6: Enabled instances, omitted for now (Rust lane, S)

**Spec:** § 7.5 first two paragraphs; § 7.4 the preset for an instance (from Task 4.4); § 15 phase 4 placeholder (`template instances are not carried yet`); T27; T39's state changes.

**Files:** `crates/collect/src/inspectors/units/mod.rs`, `crates/pipeline/src/render/unit_plan.rs` (a placeholder `OmissionReason` variant, removed in Task 5.2). Test: `crates/cli/tests/units_scan_test.rs`.

- [ ] **Step 1: Failing test first,** T27: an enabled instance found through the directory scan has a state change and a refine row, an `# Omitted:` line with the placeholder reason, and no enable; an instance linked only under `/run` is not found; an instance linked from two parents is one state change. T22's instance repeats, state-change half: the instance versions of the first four T22 cases give the state changes T22 states. T39's state changes for instances the scan collects, on Task 4.4's lookup: each T39 case with an enabled instance or a unit file of its own, after its collection and inventory assertions, gives the state change or its absence T39 states, the concrete instance's disabled regression and the template-only `enable t@two.service` regression included; their commands are the placeholder's `# Omitted:` lines here, and the carried commands are Task 5.2's. The derived cases (a disabled template-backed instance found from the presets, the disabled getty cases) are Task 5.2's: this phase does not find them and says nothing about them, which the phase report states.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(collect): collect enabled template instances`.

### Task 4.7: `Also=` members recorded (Rust lane, M)

**Spec:** § 6.3 `MemberLinks`, `AlsoMember`; § 7.3 "`Also=` members" (closure walk, `Unlinked`/`Inert` count every link, template `DefaultInstance=`, no walk through an unreadable section); § 7.4 "An owner at its preset whose member is off" (decided: every member; the walk run from each `Vendor` or `Shadowed` unit `Enabled` under an `enable` preset; a member `Unlinked` or `Other` makes the owner an `Enabled` state change with its `also` list and attention note; `Installed` and `Inert` do not); § 6.7 guard gains `AlsoMember`; § 11.1 `also[]` and `install_drop_ins[]` on `unit_states`; T25 (l); T39's owner cases, collector half.

**Files:** `crates/core/src/types/units.rs`, `crates/collect/src/inspectors/units/mod.rs`, `crates/pipeline/src/render/mod.rs`, `crates/refine/src/projection/`. Test: `crates/collect/tests/units_test.rs`.

- [ ] **Step 1: Failing tests first:** a chain `a`, `b`, `c` records `b` and `c` on `a`; each `MemberLinks` value from a planted link set; a member with no `[Install]` and a hand-made link is `Other`; T24's `Also=` reset (an empty `Also=` in an `/etc` drop-in leaves the fragment's member in `also`); T25 (l) (`Also=` naming a template is its `DefaultInstance=`, and `Other` under the template name when there is none); a member's `vendor_drop_ins` names are recorded. T39's owner cases, collector half: the non-instance `m.service` with `m.socket` unlinked is an `Enabled` row with `m.socket` `Unlinked` and the attention note, and `m.socket` has no row; with `m.socket` linked, no owner row; the `Inert` member, no owner row; the instance owner `p.service` with `t@two` unlinked is an `Enabled` row with `t@two` `Unlinked`, resolved through `t@.service`, and with `t@two` linked, no owner row. Each asserts the owner's inventory entry before the absence.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(collect): record the units Also= reaches`.

### Task 4.8: Rule 7 on a single host (Rust lane, L)

**Spec:** § 10.2 rule 7 decisions, conditions 1 to 3 (the first that fails gives the `AlsoWhy`), `AlsoMember`/`AlsoWhy` (`LinksDiffer`, `RowNotCarried`, `NeedsOwnRow`, `InstallFile`), the advisory text and its line rules, "Three things rule 7 leaves alone"; § 10.3 "Another unit's `Also=`"; T25 (a) to (i) and (k) second half; T39's owner cases, render half.

**Files:** `crates/pipeline/src/render/unit_plan.rs`, `crates/refine/src/projection/`. Test: `crates/pipeline/tests/unit_plan_test.rs`, `crates/cli/tests/units_scan_test.rs`.

- [ ] **Step 1: Failing tests first,** T25 (a) to (i) and the (k) member case, each asserting the block or the carried line and the structured reason on the refine row; (c) expects `RowNotCarried` while instances are omitted (Task 5.2 changes it). T25 (h)'s instance case (an instance recorded from a vendor template, with a `name@.service` that sets `Also=` staged from another row, omitted by rule 5) is deferred to Task 5.2, since the phase 4 placeholder decides first. Review focus 4: scan and refine export give equal unit lines for case (a). T39's owner cases, render half, on Task 4.7's rows: for `m.service` and for `p.service`, the plan declines the `enable` with `AlsoWhy::LinksDiffer`, the Containerfile holds rule 7's advisory with the owner's `enable` and the member's `disable` as lines to add, and no `systemctl` instruction names owner or member; the linked and `Inert` controls emit nothing from this check, and `m.socket`'s own `enable` row against its `disable` preset is carried as before.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(pipeline): carry an enable or disable with Also= only when every member agrees`.

### Task 4.9: Fleet merge of the phase 4 fields, and rule 7 on an aggregate (Rust lane, M)

**Spec:** § 12 bullets on list-field union for the lists that exist in phase 4 (`also`, `install_drop_ins`, `vendor_drop_ins`; `required_config` and `data_paths` are Task 5.1's), member `links` disagreement becoming `Other`, `vendor_drop_ins` union, `manual_links` by relation, parent and unit, the install-difference causes merging the same way with `install_files` the union and the first host's cause kept when two hosts record opposing causes, "always an advisory in an aggregate", the rows-without-`Also=` limit (no product text); § 10.2 `AlsoWhy::FleetAggregate` and its advisory text; T25 (j). § 12 `undetermined_installs` merge.

**Files:** `crates/core/src/aggregate/merge.rs`, `crates/pipeline/src/render/unit_plan.rs`. Test: `crates/core/tests/aggregate_merge_test.rs`, `crates/refine/tests/aggregate_export_test.rs`.

- [ ] **Step 1: Failing tests first,** T25 (j) through the real collector and merge (host A alone carries the enable; merged gives `FleetAggregate` in the plan's `declined` list and the fleet advisory text in the Containerfile, and the fleet refine row carries no reason, as § 12 says); two hosts recording one member `Installed` and `Unlinked` merge to `Other`; an owner row made by § 7.4's owner check on one host gives `FleetAggregate` in the aggregate; two hosts with different `install_drop_ins` and `vendor_drop_ins` for one unit merge to the union of each, and a member's `vendor_drop_ins` to the union of names; manual links from two hosts merge by the triple; two hosts with the same `VendorInstallOnly` link and different `install_files` merge to one row with the union and the prevalence of both; two hosts with opposing causes for one triple merge to the first host's cause in hostname order, with the prevalence of both; two hosts with an `UndeterminedInstall` for one unit, different reasons and different files, merge to one record with the first host's reason and the union of the files.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(core): merge Also= and link findings across hosts`.

### Task 4.10: The default target (Rust lane, M)

**Spec:** § 7.6, § 10.5, § 6.7 guard gains `DefaultTarget`, § 11.1 `default_target` and `ItemId::DefaultTarget`, § 12 `default_target` bullet; T26.

**Files:** `crates/core/src/types/units.rs`, `crates/collect/src/inspectors/units/mod.rs`, `crates/pipeline/src/render/unit_plan.rs`, `crates/pipeline/src/render/mod.rs`, `crates/refine/src/{types,session}.rs`, `crates/refine/src/projection/`, `crates/core/src/aggregate/merge.rs`. Test: `crates/cli/tests/units_scan_test.rs`, `crates/core/tests/aggregate_merge_test.rs`.

- [ ] **Step 1: Failing tests first,** T26 in full; three hosts with two agreeing merge to the most prevalent target, excluded.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat: carry the source host's default target`.

### Task 4.11: Links, default target and members in the flat list (frontend lane, M)

**Spec:** § 11.1 `manual_links[]` (with `not_applied` and the omission line), `undetermined_installs[]`, `default_target`, `unit_states[].also[]` and `install_drop_ins[]`, `ItemId::DefaultTarget`. See Readings and plan choices, item 1.

**Parallel:** can start beside Tasks 4.5, 4.5b, 4.7 and 4.10 against their projection Interfaces; merges after the last of them.

**Files:** `crates/web/src/{web_types,adapter,handlers}.rs`, `crates/web/ui/src/api/types.ts`, the units list component. Test: `crates/web/tests/api_test.rs`, Vitest.

- [ ] **Step 1: Failing tests first:** a manual link renders as a row with its advisories and no toggle, the `also` relation and the two install-difference causes of Task 4.5b included, and a `not_applied` row shows its excluded paths and the omission line; an `undetermined_installs[]` row renders with its reason, files and block, and no toggle; the default target renders with a toggle that sends `ItemId::DefaultTarget` and changes the Containerfile; a unit row lists its `also` members with their links.
- [ ] **Step 2-4:** run, implement, frontend checks, gate. **Step 5: Commit** `feat(web): show unit links, Also= members and the default target`.

### Task 4.12: Phase 4 close (Rust lane, S)

**Spec:** § 14 row for `inspectah-write-the-collector-overlap-contract` ("Phase 4 writes this part into `process-docs/` and fixes `docs/reference/inspector-coverage.md`").

- [ ] Write the `/etc/systemd/system` ownership contract (§ 7.7 table, § 8 table, the three dependency-directory exclusions) as `process-docs/skills/etc-systemd-system-ownership.md`, add it to `process-docs/skills/index.md`; fix `docs/reference/inspector-coverage.md:110-133` (sources, what becomes a state change, links, the default target); update `service-omission-model.md` (rules 5 and 7).
- [ ] **CHANGELOG.md**: **Added** hand-made unit link advisories with the `add-wants` or `add-requires` line; the default target is carried. **Changed** enablement is read from the links, so a unit enabled only by a hand-made link is reported, not enabled; a mask on a unit whose package is not in the image is omitted with a comment; the README states that rendered unit state holds at first boot only while the image keeps the base's empty `/etc/machine-id`; a command whose `Also=` members differ on the source is an advisory with the lines to add by hand; a unit at its preset whose `/etc` files add `[Install]` links is enabled after the `COPY`, and one whose files drop or replace a link is advised with the line to add or remove.
- [ ] The phase report names `inspectah-write-the-collector-overlap-contract` as partly covered (what remains, per § 14) and notes that the blocker of `inspectah-etc-unit-files-accessor` has moved.
- [ ] Gate; commit `docs: write the /etc/systemd/system ownership contract`.

**Phase 4 ends:** a scan reads enablement from the links, reports hand-made links, carries the default target, records `Also=` members and carries a command only when they agree, carries or advises a unit at its preset whose `/etc` files change `[Install]`, and names every enabled instance in an `# Omitted:` line.

**Review gate 4:** implementation review before Task 5.1. Focus: **the interaction of rule 5's drop-in half and rule 7 with `staged_writes` and the fleet merge** (T24, T25 (h), (j), (k)): an install file staged, excluded, or present on another host only; a same-named `/etc` drop-in hiding a vendor one; a member's toggle changing its owner's command. And **the install comparison** (T30 to T32): that an unchanged shadow is a no-op, and that the additive test compares link sets and not text.

**As built (`82827f5e`):** phase 4 is done. Tasks 4.0 to 4.12 landed on `71c334ab..0ad072cb`; review gate 4 and its four fix rounds landed on `d4095eeb..82827f5e`, and the gate closed at `82827f5e` with fmt and clippy clean, `cargo test --workspace` 3743 passed and Vitest 1302 of 1302. Schema stays 28. The fix rounds changed the rules the tasks above were written to, and the spec at `300c3a79` states each: `%I` in `[Install]` is undetermined (§ 7.2 step 4); a drop-in of one file name is read from the instance's directories before the template's, root priority within each, with the four directories above `/etc` hiding same-named lower drop-ins, and a hiding drop-in is install evidence for rule 5 (§ 7.2, § 10.2); a unit file or drop-in that is not text makes the section `Undetermined(Unreadable)` (§ 7.2, § 7.3); an alias is the unit its whole chain ends at (§ 7.3); a partial unit's unnamed links are `NotInInstall` (§ 7.3); a vendor-side `Undetermined` section gets the `UndeterminedInstall` record (§ 7.4); an undetermined storage unit gets no mount finding (§ 6.5); hidden drop-in names are never install files (§ 10.2); rule 7's recipe withholds a protected member's line and drops the header and footer it cannot keep (§ 10.2); the removal line tests for a link first (§ 10.9); and the alias, `.upholds` and README first-boot texts (§ 10.9, § 10.2). T42 covers the unreadable file; T22 to T25, T30 and T32 gained cases.

---

## Phase 5: carried instances (size S to M; 8 tasks)

Spec: § 7.1 source 6, § 7.5 (instances derived from the presets and from enabled units' `Also=`; a masked instance carried as `mask`; lists, tables, `/var/run` and `/var/lock` constant), § 8 (an instance timer's description and its template's `<prefix>@.timer.d`), § 10.2 rule 6, § 10.8, § 11.1 `required_config[]`, `data_paths[]`. Tests T28, T29, T37's scan-built instance case, and T39's derived instances and carried commands.

Before Task 5.1: merge `main` into the branch and run the gate. `main` at `a9567931` is already an ancestor of `82827f5e`, so no merge is needed there, and the gate at `82827f5e` serves.

### Task 5.1: What an instance needs (Rust lane, M)

**Spec:** § 7.5 "What an instance needs" table, the fixed tables (decided), "Every `required_config` path is kept", "`data_paths` holds persistent data only"; § 6.7 row for both lists; § 12 list-field union for `required_config` and `data_paths`; T28 collector cases.

**Files:** Create `crates/collect/src/inspectors/units/instance.rs`; modify `crates/core/src/types/units.rs`, `crates/core/src/aggregate/merge.rs` (the merge keeps a representative's fields today, so the two new lists need their own union). Test: `crates/collect/tests/units_test.rs`, `crates/core/tests/aggregate_merge_test.rs`.

- [ ] **Step 1: Failing tests first:** `wg-quick@wg0` gives `required_config = ["/etc/wireguard/wg0.conf"]` from the table; a template with `EnvironmentFile=/etc/x/%i.env` and `EnvironmentFile=-/etc/y/%i.env` keeps only the first; `dirsrv@example` gives `/var/lib/dirsrv/slapd-example` when it exists and nothing when it does not; `PIDFile=/var/run/worker/%i.pid` gives an empty `data_paths`; `StateDirectory=worker/%i` gives `/var/lib/worker/<i>`; a value that expands outside `/etc` or `/var` is dropped. Merge: two hosts with one instance whose `required_config` and `data_paths` differ merge to the union of each list.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(collect): record what an enabled instance reads and keeps`.

### Task 5.1a: An instance timer's description and trigger (Rust crates, S)

**Spec:** § 8 "Where `Unit=` is read" (the template's `<prefix>@.timer.d` for an instance timer, an instance timer's description recorded from its template's file) and "Specifiers"; § 10.2 rule 5, the timer trigger's three steps; T37's scan-built instance case.

**Why before Task 5.2:** an instance timer has no timer file of its own, so today the scheduled inspector records no description for it, and `timer_trigger_check` reads a timer with no description as `TimerTriggerUnresolved`. Once Task 5.2 carries instance enables, every enabled instance timer would be omitted for that reason. A derived instance timer is `Disabled`, and `disable` needs no trigger, so only enables are affected.

**Files:** `crates/collect/src/inspectors/scheduled.rs` (for each instance timer linked under `/etc/systemd/system`, a description from its template's timer file, with `Unit=` read from the template file, then `<name>.timer.d` and `<prefix>@.timer.d` under `/etc/systemd/system` and `/usr/lib/systemd/system` in § 8's order, `%i`, `%I`, `%p`, `%N` and `%%` expanded for the instance, and the `/etc` drop-ins that assign `Unit=` in `trigger_drop_ins`). Test: `crates/collect/src/inspectors/scheduled.rs` unit tests, `crates/collect/tests/scheduled_test.rs`, `crates/cli/tests/units_scan_test.rs`.

- [ ] **Step 1: Failing tests first:** an enabled `worker@a.timer` backed by `worker@.timer` with `Unit=worker@%i.service` records a description whose trigger is `worker@a.service`; the same `Unit=` set only in `/etc/systemd/system/worker@.timer.d/10-unit.conf` is read, and that drop-in is in `trigger_drop_ins`; an unexpanded specifier is `Unresolved`. T37's scan-built instance case through the real inspectors: with the trigger's file excluded, the instance timer's `enable` is omitted as `TimerTriggerNotStaged` once Task 5.2 carries it (assert the recorded trigger here, the omission in Task 5.2); with the template-directory drop-in excluded, `TimerTriggerEvidenceChanged`.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(collect): record an instance timer's trigger from its template`.

**Acceptance:** every instance timer linked under `/etc/systemd/system` has a description and a resolved trigger when its `Unit=` resolves, so Task 5.2's carried enables are judged on the trigger and not omitted as `TimerTriggerUnresolved`.

### Task 5.2: Rule 6 and carried instances (Rust lane, M)

**Spec:** § 7.5, the masked instance carried as `mask` (decided); § 7.1 source 6 and § 7.5 "A disabled instance is found from the presets" (candidates from the template's first matching rule: the `DefaultInstance=` under a bare `enable`, each listed instance under the list form; § 7.3 on each candidate's links; a candidate with its own unit file not derived twice; nothing new read from the host) and "Instances named by an enabled unit's `Also=` are candidates too" (decided: owners `Enabled` with a `Known` section, followed transitively; an `Undetermined` owner derives none; the owner check that reports them is Tasks 4.7 and 4.8); § 7.3 the `Undetermined` row with links that records an `UndeterminedInstall` when the section assigns `Also=`; § 10.2 rule 7, the first failed condition giving the `AlsoWhy`; § 10.2 rule 6; § 10.8 table, `InstanceConfigNotStaged`, `ConfigGap`, "Only a file the export carries passes", "Redacted", the `--no-redaction` and `Flagged` attention notes, the unknown-template note (decided); the phase 4 placeholder removed; T28 carry, T29, T39's derived instances and carried commands.

**Files:** `crates/pipeline/src/render/unit_plan.rs`, `crates/collect/src/inspectors/units/mod.rs` (attention reasons; the candidate instances, using Task 4.4's second-table lookup and Task 4.7's `Also=` walk). Test: `crates/cli/tests/units_scan_test.rs`, `crates/pipeline/tests/unit_plan_test.rs`, `crates/collect/tests/units_test.rs`.

- [ ] **Step 1: Failing tests first,** T39's derived instances, through the real inspectors: under a bare `enable t@.service` with no link for `t@one`, `t@one` is collected as a `Disabled` state change against `enable` and `RUN systemctl disable` names it; a list-form rule derives each listed instance the same way; under `disable t@.service` nothing is derived; a candidate with its own unit file is one row, not two; the disabled `getty@tty1` on RHEL 9.8's and RHEL 10.2's rules is a `disable` row on each. T39's owner case, candidate half: `t@two`, reached through `p.service`'s `Also=` with no link, is collected with a `Disabled` inventory entry and no row of its own, beside the owner row Task 4.7 made. T40's `Undetermined` owner with `Also=` and one link: the per-link advisories and its `UndeterminedInstall`. T39's carried commands for the cases Task 4.6 collected: the concrete instance's `disable` and the template-only `enable t@two.service` regression's `enable`. T28 (the `openvpn-client@site` case through the real inspectors: one state change with the template's owner, the instance in `RUN systemctl enable`); T29 in full, positive controls included, the WireGuard case through the real redaction pass asserting the staged file holds no key; update T25 (c) to the phase 5 result (the member row carried, so the owner's command carried when the member passes, `RowNotCarried` when the member's configuration is excluded), matching T29's last sentence. An instance of a template on no list with both lists empty is carried with its attention note. Deferred from phase 4: T24's concrete `worker@site.service` under `/etc` is carried with the template's package excluded and its own file staged, and omitted with `UnitFileNotStaged` when its file is excluded; T25 (h)'s instance recorded from a vendor template, with a `name@.service` that sets `Also=` staged from another row, is omitted by rule 5. T39: the concrete-instance regression and the disabled getty cases carry their `disable`. T27: the placeholder reason and its no-enable assertion are obsolete once the placeholder goes; replace them with the phase 5 result for the same instance (omitted under rule 6 when it has a `required_config` path missing from the export, carried otherwise), and keep T27's directory-discovery and `/run` exclusion checks unchanged.
- [ ] **Step 1, carried from phase 4:** a masked instance, template-backed and with its own unit file, is carried as `RUN systemctl mask` through both exports, under rules 3 and 4 and the package half of rule 5 like any mask (§ 7.5), and the placeholder no longer omits it. T24's concrete-instance and instance search-path cases, asserted on `resolve` alone in Task 4.2, through the scan; the scan-level instance cases of Task 4.3 (an install drop-in in the template's `.d`, a unit file at the instance's own name). Restore the four instance cases of rule 5 moved out of `rule_5_needs_exactly_the_recorded_install_drop_ins` (`crates/pipeline/tests/unit_plan_test.rs`), and T13's instance operand (`'probe@a\x2db.service'`) in a rendered line. Before the placeholder goes, check again that a link-found instance never re-admits a name the gate refused. T39's instance-owner control tightened to rule 7's text in § 10.2; T25 (h)'s instance case and (c)'s phase 5 result, as below. § 7.3's units that a link under `/etc` names and the listing does not print: today the gate admits only listed `static` and `indirect` units, a masked unit's `/etc` links and links to an `Unsupported`, `NotFound` or `AliasOf` unit produce no `ManualLink`, and a dangling link to a name the listing does not print produces nothing. Assert each case's result through the scan and record it in the task report; a result § 7.3 does not settle goes to the project owner.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(pipeline): carry an instance's enable when its configuration is in the export`.

### Task 5.3: The instance data advisory, and the instance lists in the view (Rust lane, S)

**Spec:** § 10.8 "Data under `/var`" (one comment per instance before the `RUN` lines, same sentence on `attention_reason`); § 11.1 `unit_states[]` `required_config[]` and `data_paths[]`; T28 `dirsrv` case.

**Files:** `crates/pipeline/src/render/unit_plan.rs`, `crates/collect/src/inspectors/units/mod.rs`, `crates/refine/src/projection/`. Test: `crates/cli/tests/units_scan_test.rs`, `crates/refine/tests/session_test.rs`.

**Interfaces (Produces):** `UnitStateView.required_config: Vec<String>` and `UnitStateView.data_paths: Vec<String>`.

- [ ] **Step 1: Failing test first:** `dirsrv@example.service` with an existing data directory has the advisory beside a carried enable and the same sentence on its row; with the enable omitted there is no data advisory. The projected `unit_states` row for `wg-quick@wg0.service` holds its `required_config`, and the `dirsrv` row its `data_paths`.
- [ ] **Step 2-4:** run, implement, gate. **Step 5: Commit** `feat(pipeline): warn that a carried instance's data stays behind`.

### Task 5.4: Instance fields in the flat list (frontend lane, S)

**Spec:** § 11.1 `instance_of`, `required_config[]`, `data_paths[]`.

**Parallel:** can run beside Task 5.2 against Task 5.3's Interfaces; merges after Task 5.3.

**Files:** `crates/web/src/{web_types,adapter}.rs`, `crates/web/ui/src/api/types.ts`, the units list component. Test: `crates/web/tests/api_test.rs`, Vitest.

- [ ] **Step 1: Failing test first:** through the production handlers the view response carries `required_config` and `data_paths`; an instance row shows its template, its required configuration paths and its data paths. **Step 2-4:** implement, frontend checks, gate. **Step 5: Commit** `feat(web): show what an instance needs on its row`.

### Task 5.5: Merge preparation (Rust lane, M)

**Spec:** § 9 last bullet ("re-run against the final types before the merge"); § 15 "`main` is merged into the branch between phases"; changelog policy ("For feature branches, the entry is written at merge time").

- [ ] Merge `main` into the branch. Re-run T1 against the final types through all three loaders; regenerate `testdata/e2e-fixture.tar.gz` and every snapshot fixture from the final types (not by editing the version number alone); confirm `docs/reference/snapshot-schema.md` describes the final `units` section.
- [ ] Update `process-docs/skills/service-omission-model.md` (rule 6) and `docs/reference/inspector-coverage.md` (instances).
- [ ] **CHANGELOG.md**: **Added** enabled template instances are carried when their configuration is in the export, and omitted with the reason otherwise. Then rewrite the branch's `[Unreleased]` entries so they describe the net change against `main`, not the order phases landed in.
- [ ] Gate; commit `docs: prepare the units branch for merge`.

**Phase 5 ends:** an enabled instance is carried when every path it needs is in the export as the host had it, and omitted, with the path and the reason, when not.

**Review gate 5:** implementation review of phase 5 and a whole-branch review before Task 5.6. Focus: rule 6 against redaction findings and `staged_writes`; T25 (c) after instances became carried; the CHANGELOG and skills checked per the repo's policy.

### Task 5.6: Runs R1, R2, R4 and the open routes of R3 at the branch tip (real host, L)

**Spec:** § 13 R1, R2, R4, and R3's open routes; § 15 real-host table, "At the branch tip, after phase 5"; § 17 "Run R4 finds stock rows beyond the known ones".

**Needs:** the branch build at the end of Task 5.5 (and, for R2's ordering case, a `main` build from before the merge); reverted `rhel9` and `rhel10` VMs, reverted before each run; the VM operating notes.

- [x] Run on 2026-11-16 on RHEL 9.8 (systemd 252) and RHEL 10.2 (systemd 257), both aarch64, in two parts: part A with the branch at `f9ddc7de` (and `main` at `f6fe8333` for R2's ordering case and R4's comparison), recorded at `a7a878d1`; part B, after the two fixes part A found, with the branch at `49027f6e`, recorded at `85800626`. Both VMs reverted before and after each run. Record: the eight "Measured, 2026-11-16" subsections of `process-docs/skills/real-host-validation-runs.md`. The run report names `inspectah-full-shadow-wants-link-points-at-the-vendor-path` as partly covered by observation 2.
- [x] **R1:** met on both releases. RHEL 9: `cockpit.socket` enabled against its `disable` preset, the image `enabled`. RHEL 10: `cockpit.socket` disabled against its `enable` preset, the image `disabled`, and `podman.socket` planted enabled against its `disable` preset, the image `enabled`.
- [x] **R2:** met on both releases, driftify **kitchen-sink** plus planted cases.
  1. The branch build succeeds with the unit state section after the configuration `COPY`; the `main` build fails at `driftify-also-main.service`'s enable.
  2. The full shadow of `sshd.service`, booted through `to-existing-root`, loads from `/etc` with its `ExecStart` and is active.
  3. `config.files` holds `/etc/wireguard/wg0.conf`, `redactions` an `inline` `wireguard_key` finding; the enable is omitted with the redacted wording, and the booted image reads `disabled`.
  4. `ci@site.service`, with clean configuration, is carried and is `enabled` and active after boot.
  5. The libvirt, `cups` and `iscsid` advisories, applied by hand, reproduce every unit's state and every `.wants` link of the host.
  6. No package in BaseOS, AppStream or EPEL for either release owns a file under `/etc/systemd/system/service.d/` (`dnf repoquery --file`), so the case was not met.
- [x] **R4:** met. Stock rows: `NetworkManager-wait-online.service`'s `enable` on RHEL 9 (the known `Also=` class) and `set-default -- 'multi-user.target'` on both (the installer's link; the bases default to `graphical.target`). `remote-cryptsetup.target` is held by the stock-state constant. No owner advisory, link warning or manual link on either stock host, and the degraded reasons and warnings match `main`'s. Every kitchen-sink row is classified in the record.
- [x] **R5's open items:** all run, every answer in the spec. They agree with it except two, which went to the project owner and are now built: an empty `RequiredBy=` resets on both releases, and a timer `Unit=` holding `%I` is ignored by systemd, which then triggers the same-named service. `%I` in `[Install]` makes `enable` exit 1; the drop-in order across the instance's and the template's `.d`, hiding from all four directories above `/etc`, `;` preset comments, hidden drop-in names, an instance with its own file under non-counting rules, the stock scriptlets' preset names, the additive sequence with an existing alias link, and `systemd-cryptsetup@` (generator links under `/run` only) agree. No stock preset file holds a list-form rule.
- [x] **Carried from phase 5:** the shipped `wg-quick@`, `openvpn-*@` and `dirsrv@` templates' paths, a quoted configuration argument, type-wide and dash-prefix `EnvironmentFile=`, an `EnvironmentFile=` reset and a `[Unit]` one, an overlong optional `WorkingDirectory=` replacement, a hidden timer drop-in, two instances of one timer template (their row ids read through the API, not in a browser), the degraded-reason list before and after the widened `Also=` walk, and template links under each kind of parent. Each agrees with systemd except `[Unit] EnvironmentFile=<path>`, which systemd ignores and the scan requires (spec § 17). Part A found two build defects, fixed and re-measured in part B: a link to a template name under a plain parent starts `<prefix>@<parent prefix>.<type>` (`ba19a6c7`), and `openvpn-server@` was never carried until the manager's directory specifiers expanded (`49027f6e`).
- [x] **R3, the routes read from source:** an image-builder disk image (`bootc-image-builder --type raw`) was built and read before boot on both releases: a zero-length `/etc/machine-id` and the rendered links in the deployment's `/etc`. It was not booted, and the installer ISO and plain `ostreecontainer` kickstart routes were not run: the VMs have no nested virtualization and one disk each, and the test network has no boot media or registry. A layer that reinstalls `systemd` writes a 33-byte machine-id on both releases.

**Gate (the merge gate):** met for R1, R2 and R4. No measured route deploys a machine-id other than the image's zero-length one. **The merge does not wait on the installer ISO and plain `ostreecontainer` kickstart routes; they stay listed as unmeasured. (Mark Russell, 2026-11-16)**

**Open after the merge,** with the next real-host run, gating nothing (spec § 13, "Still unmeasured"): the installer ISO and plain `ostreecontainer` kickstart routes; a boot of the image-builder disk; the first-accepted timer `Unit=` reading, with `Unit=a.service` in the timer's file and `Unit=b.service` in an `/etc` drop-in, then `systemctl show -p Triggers`, and the same with an empty `Unit=` or the timer's own name first; a `dirsrv@` instance made by its tools, with its NSS databases and `pin.txt`; and the preset rows § 7.4 still marks assumed.

**As built (`b8e8fa46`):** Tasks 5.1 to 5.6 are done; Task 5.7 is open. `main` was merged into the branch at `70aa3e98`. Tasks 5.1 to 5.5 landed on `ea1b537f..7b290afb`; review gate 5, of phase 5 and of the whole branch, closed after three fix rounds on `cdc3303d..5781d87c`. The project owner's wording rulings of 2026-11-15 landed on `dddccb1f..eda412bb`, the warnings for links the scan does not read and the removal of decision records from code comments on `aff54f91..f9ddc7de`, Task 5.6's records and its two fixes from `a7a878d1` to `85800626`, and the fixes after the runs, reviewed over three fix rounds, on `85800626..b8e8fa46`. At `b8e8fa46`, fmt and clippy are clean and `cargo test --workspace` passes 3836 with 0 failed; tsc is clean and Vitest passes 1307 of 1307 at `1e894d52`, the last web change. Schema stays 28. These changed the rules the tasks above were written to, and the spec at `6a045e69` states each: what an instance reads (the runtime drop-in set, each setting's syntax, resets where systemd applies them, the manager's directory specifiers), `needs_unknown` and its `InstanceNeedsUnknown` omission, and drop-ins that change an instance's needs as evidence with `InstanceNeedsEvidenceChanged` (§ 7.5, § 10.2, § 10.8); rule 6's `..` and fleet-variant declines, its notes for `Raw` scans and `Flagged` findings, and the data advisory's "may start" (§ 10.8); a preset read failure dropping every state change (§ 7.1); the default target's three omission reasons (§ 10.5); the alias, undetermined and owner-note wordings (§ 10.9, § 7.4); the link warnings (§ 7.3); the empty `RequiredBy=` reset (§ 7.2); which timer `Unit=` counts, its own specifier set, another timer's name and the trigger evidence (§ 8); an instance trigger judged at its template's file (§ 10.2); and the `timer-instance:` row id (§ 11.1). T43 and T44 are new; T24, T26 to T29, T35, T37, T39 and T40 gained or changed cases.

### Task 5.7: Merge (project owner)

- [x] With review gate 5 passed and Task 5.6's gate met, the project owner merges `feat/unified-systemd-units` to `main`. The spec moves from `proposed/` to `implemented/` in the merge's follow-up, with the plan's status marker and a row in `process-docs/plans/README.md`, per that file's rule.

---

## Sizes

| Phase | Tasks (Rust / frontend / real host) | § 15 size | This plan's size |
|---|---|---|---|
| 1 the rename | 7 (6 / 1 / 0) | M | M |
| 2 every kind | 17 (13 / 1 / 2 runs; 1 close) | L, upper end | XL: about one and a half L |
| 3 one control per fact | 7 (5 / 2 / 0), plus four fixes after its review gate, one with a frontend piece | M | M, plus the fixes |
| 4 the `/etc/systemd/system` scan | 17 (15 / 1 / 1 run) | L, upper end | L+: about a half over L, plus two small follow-on tasks |
| 5 carried instances | 8 (5 / 1 / 1 run; 1 merge) | S to M | M |

Task sizes: S is one short dispatch, M one full dispatch, L one dispatch at the limit of what a single review can check from the diff. The L tasks (1.2, 2.7, 4.4, 4.8, 2.17, 5.6) are the ones to watch for a split.

**The whole is larger than § 15 estimates, by roughly a third.** Phases 2 and 4 carry most of it: phase 2 holds staging, the plan, three rule tasks, mount findings, updaters, fleet and refine rows, and phase 4 holds a reader, a resolver, link state, links, the install comparison, members, rule 7 and the default target. § 15's sizes also do not count the five real-host tasks or the frontend lane's five tasks after phase 1 (2.14, 3.3, 3.6, 4.11, 5.4).

## Readings and plan choices

Where the plan reads the spec or chooses between valid approaches. None changes the design.

1. **Minimal flat-list rows before phase 6.** Mount findings, hand-made links and the default target get a minimal row each in the existing flat list in the phase that adds their data (Tasks 2.14, 4.11), with no grouping. § 11.1 defines these rows, and the default target's toggle (§ 10.5) has no other way to be reached in the merged product. Grouping and presentation stay with phase 6.
2. **Changelog timing.** `process-docs/changelog-policy.md` writes a feature branch's entry at merge time. Each phase's close task writes that phase's lines, and Task 5.5 rewrites the block into the net change against `main` before the merge, so what reaches `main` is one merge-time entry.
3. **Compile-forced edits in `crates/web`.** Rust tasks make renames, deletions and mechanical boundary conversions there, with no new DTO field or behavior (Lanes). The alternative, landing a frontend-lane change with each type change, couples two implementers on one commit.
4. **`staged_writes` is public.** Refine calls it to compute `declined` and `withheld` (Task 2.13), as the spec's § 10.2 signature now declares.

## Coverage

Each spec section that states a requirement, and the task that implements it.

| Spec section | Task(s) |
|---|---|
| § 2 decisions | 2.3 (every kind), 2.5 and 2.10 (storage advisory), 2.8 (`tmp.mount` mask), 4.8 (`Also=` verbs), 4.6 and 5.2 (instances), 4.5 (hand-made links), 1.2 (schema 28), 5.7 (merge after phase 5) |
| § 3 non-goals | Non-goal. Generated and runtime states keep today's warning: 2.3. User units and quadlets untouched. |
| § 5 per-kind table | 2.3, 2.4, 2.5, 4.5, 4.6, 5.2 |
| § 6.1 `UnitName` | 1.1 |
| § 6.2 carried and advisory types, `classify` | 2.1, 2.2 |
| § 6.3 state change, refusals | 2.2; `also` 4.7; drop-in lists 4.3; instance lists 5.1 |
| § 6.4 section, inventory | 1.3; derived state 4.4 |
| § 6.5 mount findings | 2.1, 2.5 |
| § 6.6 hand-made links | 4.5; the `Also` relation and the two install-difference causes 4.5a |
| § 6.7 extracted fields and guard | 2.5; `AlsoMember` 4.7; `ManualLink` 4.5; `DefaultTarget` 4.10; `vendor_drop_ins` 4.3; instance lists 5.1; timers 3.1 |
| § 7.1 sources 1, 2 | 2.3 |
| § 7.1 sources 3 to 5, search path | 4.2, 4.4 |
| § 7.2 steps 2, 3 | 2.2 |
| § 7.2 step 1, step 4, install files recorded | 4.1, 4.2, 4.3 |
| § 7.2 the vendor-side read | 4.5a |
| § 7.3 install state, table | 4.4 |
| § 7.3 `ManualLink`, aliases, `.upholds` | 4.5 |
| § 7.3 `Also=` members | 4.7 |
| § 7.4 state change rules, name gate | 2.3 |
| § 7.4 the preset for an instance, the `PresetRule` list form, stock-state list | 4.4; instance state changes 4.6; the stock preset search 4.4 step 6 |
| § 7.1 source 6, § 7.5 disabled instances found from the presets and from enabled units' `Also=` | 5.2 |
| § 7.4 an owner at its preset whose member is off, every member | 4.7 (row), 4.8 (advisory), 4.9 (aggregate) |
| § 7.4 a unit at its preset whose `/etc` files change `[Install]` | 4.5a (comparison, row, findings); 4.5b (render) |
| § 7.4 updaters, protected units at collect | 2.11, 2.8 |
| § 7.5 instances collected, host-device list | 4.6, 4.5 |
| § 7.5 a masked instance carried as `mask` | 5.2 |
| § 8 an instance timer's description and its template's `<prefix>@.timer.d` | 5.1a |
| § 7.5 what an instance needs, fixed tables | 5.1; the runtime drop-in set, syntax, resets and drop-ins as evidence, review gate 5 fix rounds; the manager's directory specifiers, `49027f6e`; `needs_unknown`, post-5.6 fixes |
| § 7.3 warnings for links the scan does not read | Phase 5 owner items, after review gate 5; `template` and `template_alias`, `ba19a6c7` and post-5.6 fixes |
| § 7.6 default target | 4.10; the three omission reasons of § 10.5, the 2026-11-15 wording rulings |
| § 7.7 one owner per path, masks, drop-ins | 2.4 |
| § 7.7 `*.upholds/*` exclusion | 4.5 |
| § 7.8 owning package | 2.2 |
| § 8 scheduled tasks | 3.1, 3.2, 3.3; generated units 3.4, 3.6 |
| § 8 the scheduled controls apply, one name from two cron sources; § 11.1 operations on scheduled rows | Phase 3 fix 1 |
| § 11.1 `generated_timers[]`, `cron_job_excluded`, scheduled row ids, `GenericRefItem.scheduled` | Phase 3 fix 1, its frontend piece |
| § 8 a carried timer needs its trigger, declined on changed evidence; § 10.2 rule 5 timer trigger (three steps); § 6.7 `SystemdTimer.trigger` and `trigger_drop_ins` | Phase 3 fix 3 |
| § 10.2 item 3, a collected unit includes inventory names | Phase 3 fix 2 |
| § 10.2 no other writer under `/etc/systemd/system`; § 11.1 `withheld` on the unmanaged row | Phase 3 fix 4, with its frontend piece |
| § 9 schema, one bump, refusal test | 1.2; re-run 5.5 |
| § 9.1 string consumers | 1.2, 1.5, 1.6, 1.7; aggregate timer control 3.3 |
| § 10.1 lines, quoting, order within section | 2.7, 2.9 |
| § 10.1 README sentences | 2.9; the first-boot condition 4.0a |
| § 10.1 first-boot decision; § 10.2 never the machine-id | 4.0a |
| § 10.2 rule 5, a mask follows the package check; § 12 a mask follows its package on an aggregate | 4.0b |
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
| § 10.6 mount advisories | 2.10 (T8, T9, and T11's unowned storage masks) |
| § 10.7 order | 2.9; generated units 3.4 |
| § 10.8 instance configuration, data advisory | 5.2, 5.3; `FleetVariants`, `..`, the notes and "may start", review gate 5; `InstanceNeedsUnknown`, post-5.6 fixes |
| § 10.9 link advisories | 4.5; install differences 4.5b |
| § 6.6 `UndeterminedInstall`, `Undetermined { reason }`; § 10.9 its unit-level block | 4.5 |
| § 7.4 and § 10.9 an excluded install file said once per unit | 4.5b |
| § 11.1 rename of fields and operation | 1.5, 1.6 |
| § 11.1 `unit_states`, `declined`, `withheld`, `mount_findings` | 2.13, 2.14 |
| § 11.1 scheduled rows, timer operation removed | 3.2, 3.3 |
| § 11.1 `manual_links`, `undetermined_installs`, `default_target`, `also` | 4.5, 4.5b, 4.7, 4.10, 4.11 |
| § 11.1 generated units declined on their rows | 3.4, 3.6 |
| § 11.1 instance fields | 5.1, 5.3, 5.4 |
| § 11.2 section design | Phase 6 |
| § 11.3 synthesis retired | 1.4, 1.6; the command it produced 4.5a, 4.5b |
| § 12 inventory merge | 1.3 |
| § 12 identity, mount findings, updater exception, render-time validation | 2.11, 2.12 |
| § 12 list fields, `manual_links`, `undetermined_installs`, `default_target`, aggregate `Also=` | 4.9, 4.10; install-difference causes and `install_files` 4.9; `required_config` and `data_paths` 5.1 |
| § 13 tests | Per test in the table below |
| § 13 R1 | 2.16, 5.6 |
| § 13 R2 (six observations), R4 | 5.6 |
| § 13 R3 settled results | 2.17 |
| § 13 R3 open routes (image-builder disk image, installer ISO, `ostreecontainer` kickstart) | 5.6 |
| § 13 R5, the additive sequence, the timer without its service and the `tuned.service` preset included | 4.0 (run); its open items 4.4 step 6 and 5.6 |
| § 13 unrecorded measurements | 2.15 |
| § 14 backlog items | Backlog items section; closures named in 2.15, 3.7, 4.12, 5.6 |
| § 14 overlap contract text | 4.12 |
| § 15 branch, phases, merge after phase 5 | Whole plan; 5.7 |
| § 17 limit: fleet rows without `Also=` | Non-goal (stated limit, no product text) |
| § 17 limit: flatpak rescan | 3.4 (the FIXME is its text) |
| § 17 limit: fleet fragment disagreement | Non-goal (stated limit, no product text) |
| § 17 limit: an edited `also` list is believed | Non-goal (stated limit, no product text) |
| § 17 limit: fleet evidence may omit install metadata | 4.9 (the `FleetAggregate` advisory text) |
| § 11.1 single-host scope of `declined` and `withheld`; § 12 what a fleet refine row shows; § 17 limit: the fleet view does not say why a command was not written | No product code: the fleet view is unchanged. 2.13 and 2.14 implement the reasons for the single-host view only. The changelog line saying the fleet view does not yet show them is Task 2.15's. |
| § 17 cost: a changed `[Install]` is carried only when it adds | 4.5b (the two advisory texts) |
| § 17 cost: two systemd behaviors are modeled from two releases | 4.1 (the reset), 4.4 (the preset lookup); stated costs, no product text |
| § 17 limit: rendered unit state depends on the image's machine-id | 4.0a (the README sentence); what a later layer does is outside the product |
| § 17 cost: a mask is dropped with its package | 4.0b (the `# Omitted:` line is its text) |
| § 17 cost: four rules that come with the single writer, with what the trigger check cannot see, the drop-in directories the trigger reader does not read, the declined unrecorded trigger, the inventory FIXME's wording and the cron source left in place | Phase 3 fixes 1 to 4 (each rule's FIXME, error or `# Omitted:` line is its text); the unread directories, the wording and the cron source are stated limits with no product text; the template drop-in directory, Task 5.1a |

### Tests

Each test, and each lettered case, in the task that carries it. Every task listed comes after the tasks that create the types and rules its assertions need.

| Test | Task(s) |
|---|---|
| T1 | 1.2; re-run 5.5 |
| T2 | types 2.1; `state_changes` cases 2.2; signature 2.7 |
| T3 | core half 1.2; projection 1.5; handlers and frontend 1.6 |
| T4 | session 1.4; count 1.6 |
| T5 | collector 2.2, 2.3; render 2.7 |
| T6 | 2.7 |
| T7 | 2.3 |
| T8 | render 2.8; advisories 2.10 |
| T9 | 2.10 |
| T10 | snapshot and exports 2.5; view response 2.13 |
| T11 | collector 2.4; staged file and commands 2.8; rendered advisory for unowned storage masks 2.10 |
| T12 | staging 2.6; commands 2.8; view response 2.13 |
| T13 | quoting 2.7; order 2.9 |
| T14 | None: the spec's numbering has no T14 |
| T15 | conflict staging 2.6; commands and toggles 2.7; refine row 2.13 |
| T16 | 2.8 |
| T17 | 2.11 |
| T18 | (a) 2.12; (b) 2.11; (c) 2.12; (d) 2.7 |
| T19 | 3.1 |
| T20 | refine 3.2; handlers and frontend 3.3 |
| T21 | generated and flatpak 3.4; tuned and every line planned 3.5; rows 3.6 |
| T22 | state 4.4; instance state changes 4.6; links and advisories 4.5 |
| T23 | 4.5 |
| T24 | search path and concrete-instance fragment 4.2; drop-ins, type-wide and dash-prefix directories, resets 4.3; reset install states 4.4; reset links 4.5; the `RequiredBy=` reset, post-5.6 fixes; `Also=` reset 4.7; concrete-instance render 5.2 |
| T25 | (a) to (g), (i), (k) member case 4.8; (h) 4.8, its instance case 5.2; (j) 4.9; (k) first half 4.3; (l) 4.7; (m) review gate 4 fix round 1; (c) revised 5.2 |
| T26 | 4.10; the default target reasons, the 2026-11-15 wording rulings |
| T27 | 4.6; placeholder assertions replaced 5.2 |
| T28 | collector 5.1; carry 5.2; `dirsrv` advisory 5.3 |
| T29 | 5.2 |
| T30 | 4.5a |
| T31 | 4.5b |
| T32 | render and the excluded-file case 4.5b; rows 4.11 |
| T33 | 4.0a |
| T34 | 4.0b |
| T35 | Phase 3 fix 1; handler and UI half, its frontend piece |
| T36 | Phase 3 fix 2 |
| T37 | Phase 3 fix 3; the scan-built instance timer case 5.1a, its omission 5.2; which `Unit=` counts and another timer's name, post-5.6 fixes |
| T38 | Phase 3 fix 4; the row's `withheld`, its frontend piece |
| T39 | lookup, parser and warning 4.4; state changes of collected instances 4.6; owner cases, collector 4.7 and render 4.8; derived instances, the owner case's candidate and every carried instance command 5.2 |
| T40 | record, advisory and row, and the `UpheldBy=` vendor unit's `Disabled` row and its no-link `Also=` variant 4.5 (the `Disabled` install state 4.4); the linked `Also=` variant 5.2; excluded-file case 4.5b; row in the list 4.11 |
| T41 | 4.5b; rows in the list 4.11 |
| T42 | Review gate 4, fix rounds 1 and 2 (built at `82827f5e`) |
| T43 | Phase 5 owner items (built at `f9ddc7de`); the `template` cause `ba19a6c7`; `template_alias` post-5.6 fixes |
| T44 | Review gate 5 fix rounds; `49027f6e`; `needs_unknown`, post-5.6 fixes (built at `b8e8fa46`) |
