# Systemd units refine section: implementation plan

> ## Not started (2026-11-16)
>
> **Checkbox state below is not evidence** — the boxes were never ticked.
> Classified from the tree:
> - No implementation commit yet. The branch `feat/systemd-units-refine-section` does not exist.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task by task. Steps use checkbox (`- [ ]`) syntax for tracking. Every task is test-first: write the failing test the task names, run it and see it fail for the stated reason, then implement.

**Goal:** replace the flat "Services" list in single-host refine with the "Systemd units" section of option C: units grouped by kind, an outcome filter with counts, a follow-up flag per group, the reported rows in their own open group, rows that say their state, outcome and reason legibly, and five defect fixes around it.

**Architecture:** one pure module, `crates/web/ui/src/components/unitOutcome.ts`, owns every rule the section computes (row and occurrence keys, outcomes, counts, grouping and order, filter, search and pin visibility, label maps), tested on its own over recorded and planted fixtures. Two new components render it (`UnitGroup.tsx`, `UnitOutcomeFilter.tsx`); the section's state lives in one hook, `crates/web/ui/src/hooks/useUnitsSectionState.ts`; and `ServiceSection.tsx` keeps its name and test ids and renders through them. Navigation into the section resolves an item id to a row key and finds the row by `data-unit-key`. Shared components (`AppShell`, `GlobalSearch`, `ContainerfilePanel`) change in single-host mode only, through the existing `isAggregateMode` prop. The only Rust change is one `SectionMeta` label.

**Tech stack:** React 18, TypeScript, PatternFly 6 (`ToggleGroup`, `Badge`, `Label`, `EmptyState`, all already dependencies), Vitest with Testing Library, Playwright with `@axe-core/playwright`; Rust for the label, its test and the fixture tests (insta). No new dependency.

**Spec:** `process-docs/specs/proposed/2026-11-16-systemd-units-refine-section.md` at `a47faa1d`, as amended on 2026-11-16 after the mid-way gate of Task 4.5 (§ 2 items 9 to 13). The spec is the contract. Section numbers (§), string numbers (§ 7, "string N", 1 to 26) and test ids (U1 to U37, P1 to P8) are the spec's. The spec read source at `e6591669`; between that commit and `a47faa1d` no file under `crates/web` or `crates/pipeline/src/section_group.rs` changed, so its file references hold. The symbol name is the identifier; a line number is a convenience.

**Branch and worktree:** all work happens on `feat/systemd-units-refine-section`, cut from `main` by Task 1.1 in its own git worktree beside the main checkout (`../inspectah-systemd-units-refine-section`). This plan stays on `main`; the branch never edits it. The branch merges to `main` once, at the end, by the project owner (Task 9.3).

**Verification convention.** No `cargo` command runs through a pipe, so the exit code is cargo's. Never run cargo and Vitest at the same time (`build.rs` runs `npm ci`; `process-docs/skills/full-workspace-verification-gates.md`). The commands, from the worktree root:

| Gate | Command |
|---|---|
| Format | `cargo fmt --check` |
| Lint | `cargo clippy --workspace --all-targets -- -D clippy::all` |
| Rust tests | `cargo test --workspace` |
| Types | in `crates/web/ui`: `npx tsc --noEmit` |
| Vitest | in `crates/web/ui`: `npx vitest run` (`npm test`) |
| Playwright, new | in `crates/web/ui`: `npx playwright test e2e/systemd-units.spec.ts`: every P test passes and none is skipped |
| Playwright, whole | in `crates/web/ui`: `npx playwright test` (`npm run test:e2e`), compared test by test with the baseline of Task 1.1 |

The pre-commit hook gives a frontend-only commit no gate, so every frontend task runs Types and Vitest by hand before its commit. A Rust task runs the three cargo gates (the hook runs them too). Read Playwright's `skipped` line every time and report it apart from `passed` (`process-docs/skills/e2e-fixtures-are-hand-maintained.md`).

---

## Lanes

- **Frontend lane:** everything under `crates/web/ui/src` and `crates/web/ui/e2e/*.spec.ts`, and the hand-built Vitest fixtures. Tasks 2.1 to 8.3 except where marked.
- **Rust lane:** the `SectionMeta` label in `crates/pipeline/src/section_group.rs` and its test, re-recording the `*.groups.json` system-tuning recordings it forces, recording the kitchen-sink session (the Vitest fixture and the Playwright bodies, which needs a built `inspectah` and a running refine), and registering the new e2e bodies in `crates/web/ui/e2e/fixtures/manifest.json` and `crates/web/tests/fixture_structure_test.rs`. Tasks 1.2 and 1.3.

No Rust task depends on unfinished frontend work: Tasks 1.2 and 1.3 need only `main`, and can run in parallel with each other. The frontend lane needs Task 1.3's fixtures and its commit body from Task 1.4 on, and Task 1.2's re-recorded groups body from Task 8.1 on.

## Backlog items

Named by slug (spec Metadata, § 6.12, § 10.1). The items are updated outside this repository; the merge report says which this work closes.

- `inspectah-global-search-services-results-navigate-nowhere`: closed by Task 5.2.
- `inspectah-omitted-service-search-text-leaves-out-the-reason`: made moot for this UI by Task 5.2, stays open (§ 6.12).
- `inspectah-no-fixture-has-a-non-empty-services-drop-ins`: touched by Task 1.3, stays open (§ 10.1).
- `inspectah-refine-sections-reorg`: touched, not closed (§ 3.2).

## Global constraints

- **No server change** except the `units` label in `SectionMeta` (§ 2 item 3, § 6.14). No new view response field, no snapshot or schema change, no Containerfile change; nothing in `crates/web/src` changes (§ 8 Untouched).
- **The fleet view is unchanged** (§ 2 items 2 and 8, § 3.2). No fleet product code is edited: `src/components/AggregateApp.tsx`, everything under `src/components/aggregate/` and `crates/web/src/aggregate_handlers.rs`. The one fleet file that changes is the test `src/components/aggregate/__tests__/AggregateApp.integration.test.tsx`, which U34 extends (Tasks 5.2 and 7.3). Every change to `AppShell`, `GlobalSearch` and `ContainerfilePanel` is conditioned on `isAggregateMode` being false (§ 6.15).
- **Strings are § 7's, verbatim.** No new served string. Reason and advisory sentences are shown as given, never composed, rephrased or parsed (§ 3.3).
- **Rows never move on a toggle.** Placement is by kind and name only; a filter hides and never copies (§ 3.3).
- **Two identities, one builder each.** Row keys and occurrence keys are built only by `unitOutcome.ts` (§ 6.11). An occurrence key never appears in a search entry, an item id, a URL, `data-unit-key` or a test id. No CSS selector is ever built from a key or an item id (`process-docs/skills/web-disposition-contract.md`).
- **Every existing test id stays** (§ 6.6 "Unchanged"), and `ServiceSection` keeps its file and component name.
- **Colours from PatternFly tokens only** (§ 6.13 Themes).
- **An existing test that asserts behaviour the spec changes** (the heading, `(preset: ...)`, "Service excluded", the shadow count, a kind chip on every row, a drop-in search entry's bare-path id, "Go to owner"'s target, "Controlled in Service Drop-ins") is updated in the task that changes the behaviour, and the commit body names each changed assertion. A test that feeds "Services" as input (a groups body, a fleet section's `display_name`) stays.
- **Named constants** for the group order, the label maps and the live-region templates, in `unitOutcome.ts`.
- **Commits:** `type(scope): description`, imperative; trailer `Assisted-by: <tool> (<model>)`; never a `Co-Authored-By:` trailer. Stage by explicit path. Text in commits, code comments and docs names no person, reviewer or tool beyond the trailer.
- **Voice** in user-facing text (docs, CHANGELOG): no em dashes, not "shape", not "immutable".

## Review focus

Inputs and conditions the spec implies and no U or P test names, most likely first. Each has a test in the owning task.

1. **A pin or tab stop whose row leaves the view.** An Undo removes the record that heads a pinned branch, or the record holding the tab stop: the pin is dropped (§ 6.4, last sentences of pinning) and the tab stop moves to the first shown row, with nothing thrown. Tests in Tasks 4.2 and 6.1.
2. **A toggle whose op fails.** `applyOp` rejects: the branch stays pinned and shown, focus stays on the row, counts are unchanged, and the failure shows as today. Test in Task 4.2.
3. **A unit name with an escape or `@` reaching the reveal.** `backup\x2djob.timer` as an `Also=` member and as a global search result: the reveal compares strings and focuses its row. Test in Task 4.3.
4. **A kind the UI has never seen.** A `unit_states` row with `kind` `swap`, `automount` or an unknown lowercase word lands in Other units with its kind chip; nothing is dropped. Test in Task 2.2.
5. **A long unit name and a long `declined.text` at 768 px.** No horizontal scroll; the name wraps. Test in Task 8.3 (P7's extra case).

---

## File structure

| File | Responsibility |
|---|---|
| `crates/web/ui/src/components/unitOutcome.ts` (new) | Pure logic, no React, every rule the section applies: keys, outcomes, counts, groups, reported ordering, label maps, filter, search and pin visibility, both search texts, the bar's drawn and disabled values and hint, the empty states, the live region text, the reveal's steps, the shown row order (§ 8) |
| `crates/web/ui/src/hooks/useUnitsSectionState.ts` (new) | The section's state: filter, live region text, pins, collapse, the reveal as applied, tab stop and focus after a toggle; calls `unitOutcome.ts` for every rule (§ 8, Task 6.2) |
| `crates/web/ui/src/components/UnitGroup.tsx` (new) | One collapsible group: header button (angle, title, count, flag), the labelled grid with its rowgroup (§ 6.3, § 6.13) |
| `crates/web/ui/src/components/UnitOutcomeFilter.tsx` (new) | The `ToggleGroup` bar, hint line and polite live region (§ 6.4) |
| `crates/web/ui/src/components/ServiceSection.tsx` | Renders through the groups; row changes; the empty states and region; its state comes from `useUnitsSectionState.ts` from Task 6.2 (held in the component until then) |
| `crates/web/ui/src/components/MainContent.tsx` | Heading and label; `SectionSearch` for `units`; passes text, reveal, navigation and config paths; clears the query for a reveal |
| `crates/web/ui/src/App.tsx` | Unit-row props to `AppShell`; the `data-unit-key` reveal lookup for `units` |
| `crates/web/ui/src/components/AppShell.tsx` | Passes `isAggregateMode` and the unit-row props down |
| `crates/web/ui/src/components/GlobalSearch.tsx` | The single-host units index, `searchText` on results |
| `crates/web/ui/src/components/shadowOwner.ts` | "Go to owner" sends the drop-in's row key; the owner badge reads string 26 |
| `crates/web/ui/src/components/Sidebar.tsx` | The Scheduled Tasks count (§ 9.4) |
| `crates/web/ui/src/components/ContainerfilePanel.tsx` | The single-host footer (§ 9.5) |
| `crates/web/ui/src/components/attentionUtils.ts` | The locked-reason label (§ 9.3) |
| `crates/pipeline/src/section_group.rs` | The `units` label and its assertion |
| `crates/web/ui/src/__tests__/fixtures/systemd-units/` (new) | `kitchen-sink.units.json`, `planted.units.json`, `stock10.units.json`, `stock9.units.json` |
| `crates/web/ui/e2e/fixtures/single-host/` and `sequences/units-exclude-tmp-mount/` | The recorded Playwright bodies (§ 10.1) |
| `crates/web/ui/e2e/systemd-units.spec.ts` (new) | P1 to P8 and their one setup function |

---

## Phase 1: branch, baseline, fixtures and the label (4 tasks)

Spec: § 10.1, § 6.14 (the served label), § 10.2's last two paragraphs. No visible change in the UI except the sidebar label.

### Task 1.1: Branch, worktree and the local Playwright baseline (either lane, S)

**Spec:** none directly; the gates of this plan need it. The fixtures skill records that a local Playwright run fails 49 to 51 of 82 tests on `main`.

- [ ] **Step 1:** from the main checkout, `git worktree add -b feat/systemd-units-refine-section ../inspectah-systemd-units-refine-section main`. In the worktree: `cd crates/web/ui && npm ci`, then `npx playwright install chromium` if the browser is missing.
- [ ] **Step 2:** in a clean checkout of the branch point (`main` at the commit the branch was cut from, no change made), run `npx playwright test --reporter=list` in `crates/web/ui`, with nothing listening on port 8642. Record the commit, the totals (passed, failed, skipped, flaky) and the name of every test that does not pass.
- [ ] **Step 3:** run it a second time. A test that passes once and fails once is listed as flaky, not as failing.
- [ ] **Step 4:** commit the record on the branch as an empty commit, the list and the branch-point commit in the body: `git commit --allow-empty -m "test(web): record the local Playwright baseline before the units section"`. The commit may land after other commits on the branch (Tasks 1.2 and 1.3 run beside this one); the runs it records are still the branch point's. Later gates read it with `git log --grep "Playwright baseline" -1`.

**Acceptance:** the branch exists in its own worktree; the baseline commit names the branch-point commit its runs were taken from and lists every non-passing e2e test there.

### Task 1.2: The sidebar label "Systemd units" (Rust lane, S)

**Spec:** § 6.14 first bullet; string 1 (sidebar half); § 10.2 the Rust assertion and the re-recorded `*.groups.json`.

**Files:**
- Modify: `crates/pipeline/src/section_group.rs` (`SectionMeta` with `id: "units"`; tests module beside `all_groups_have_labels`)
- Re-record: `crates/web/ui/src/__tests__/fixtures/system-tuning/{aggregate,files-no-tuned,glob-only,mask-only,outside-link-overlap,sysctl-a-fails}.groups.json`
- Test: `crates/pipeline/src/section_group.rs` tests, `crates/web/tests/system_tuning_fixtures_test.rs`

- [ ] **Step 1: Failing test first.** `fn units_section_is_labelled_systemd_units()`: find the `SectionMeta` whose `id` is `"units"` across `SectionGroup` sections and assert its `label == "Systemd units"`, and that its group is still the one with slug `services-scheduling`.
- [ ] **Step 2:** `cargo test -p inspectah-pipeline section_group` fails on `"Services"`.
- [ ] **Step 3:** change the label. Nothing else in the file changes (id, slug, `is_triage`).
- [ ] **Step 4:** `cargo test -p inspectah-web --test system_tuning_fixtures_test` fails on the six groups bodies; re-record with `UPDATE_SYSTEM_TUNING_FIXTURES=1 cargo test -p inspectah-web --test system_tuning_fixtures_test`, then run it again without the variable. `git diff --stat` shows the six `.groups.json` files and nothing else under `system-tuning/`; each diff is the one label.
- [ ] **Step 5:** confirm `build_web_sections_display_names` in `crates/web/src/adapter.rs` still asserts "Services" and passes (the reference projection keeps it, § 10.2).
- [ ] **Step 6: Gate, commit** `feat(pipeline): label the units section Systemd units in the sidebar`.

**Acceptance:** the new assertion passes; the six recordings differ from `main` by that one label each; `crates/web/src` is unchanged.

### Task 1.3: Record the kitchen-sink session (Rust lane, M)

**Spec:** § 10.1 bullets 1 (`kitchen-sink.units.json`), 3 (`stock10`, `stock9`) and the Playwright bodies paragraph in full.

**Needs (supplied by the project owner, not in the repository):** the driftify kitchen-sink scan tarball of RHEL 10.2 that the design pass used, `inspectah-rhel10-kitchen-sink-e6591669.tar.gz` (scanned by inspectah `e6591669`, schema 28). Refine at the branch commit serves it, so the recording is "served by refine at the implementing commit" as § 10.1 asks. If the owner supplies a fresh scan instead, it must be a driftify kitchen-sink profile on RHEL 10.2, and U2, U3 and U5's numbers are checked against it before anything else is built on it (Task 2.2 Step 1 states them).

**Files:**
- Create: `crates/web/ui/src/__tests__/fixtures/systemd-units/{kitchen-sink,stock10,stock9}.units.json`
- Create: `crates/web/ui/e2e/fixtures/single-host/{view-units-kitchen-sink,view-units-stock10,sections-units-kitchen-sink}.json`, `crates/web/ui/e2e/fixtures/sequences/units-exclude-tmp-mount/01-after-exclude.json`
- Modify: `crates/web/ui/e2e/fixtures/manifest.json`, `crates/web/tests/fixture_structure_test.rs`; new snapshots under `crates/web/tests/snapshots/`

- [ ] **Step 1: Failing tests first.** Add one `#[test]` per new e2e body to `fixture_structure_test.rs` (`fixture_single_host_view_units_kitchen_sink`, `fixture_single_host_view_units_stock10`, `fixture_single_host_sections_units_kitchen_sink`, `fixture_sequence_units_exclude_tmp_mount_01`), each calling `snapshot_fixture` on its path. `cargo test -p inspectah-web --test fixture_structure_test` fails: the files do not exist.
- [ ] **Step 2: Record.** `cargo build --bin inspectah`; serve the tarball with `target/debug/inspectah refine <tarball> --port 8642 --open false --fresh` and drive it with `curl` as `process-docs/skills/real-host-validation-runs.md` § "Driving refine without a browser" describes (no `Origin` header). Take `GET /api/view`, `GET /api/snapshot/sections`, then `POST /api/op` with `{"kind":"Op","op":"SetInclude","target":{"item_id":{"kind":"Unit","key":{"unit":"tmp.mount"}},"include":false}}` and keep its response body. Stop the server.
- [ ] **Step 3: Trim and write**, by a `python3 -I` script kept outside the repository:
  - `kitchen-sink.units.json`: the six unit fields of the view (`unit_states`, `unit_dropins`, `default_target`, `mount_findings`, `manual_links`, `undetermined_installs`), verbatim.
  - `view-units-kitchen-sink.json` and `01-after-exclude.json`: the view and the op response, every item list outside the six unit fields, `generated_timers` and `declined_generated` set to `[]`.
  - `sections-units-kitchen-sink.json`: the sections body with only the `units` and `scheduled_tasks` sections. Assert in the script that `scheduled_tasks` holds a timer row with `unit_row: true`; if not, stop and report (P5 cannot be set up, § 10.1).
  - `stock10.units.json` (`default_target` `multi-user.target`, included, `declined: null`, every list empty) and `stock9.units.json` (the same plus one `unit_states` row for `NetworkManager-wait-online.service`, owner `NetworkManager`, `current_state` `enabled`, `default_state` `disable`, carried), hand-built from the stock records in `process-docs/skills/real-host-validation-runs.md` (R4, 2026-11-16) with every always-present field of `ServiceDecisionDto` (`required_config: []`, `data_paths: []`). Values copied from the records and the types, never invented.
  - `view-units-stock10.json`: `view-units-kitchen-sink.json` with its six unit fields replaced by `stock10.units.json`'s.
- [ ] **Step 4: Check before staging.** Search every new file for the scan host's hostname (from `/api/health`), `PrivateKey`, `BEGIN `, `password`, `token` and `REDACTED`; each `REDACTED` hit is the scan's own marker and stays. Any other hit stops the task.
- [ ] **Step 5:** register the four e2e bodies in `manifest.json` (category `body`; schema `ViewResponse` for the views and the sequence body, `ContextSections` for the sections body). Run `cargo test -p inspectah-web --test fixture_structure_test`, review each new snapshot and the manifest snapshot diff, accept with `cargo insta accept` only after reading them.
- [ ] **Step 6: Gate, commit** `test(web): record the kitchen-sink units session for the units section tests`. The body records the inspectah commit, the tarball's name, the requests sent (including the op), the trim script's steps, and the counts the recording holds (19 unit rows, 4 drop-ins, 1 default target, 2 manual links, 1 undetermined install, 0 mount findings, if so). Task 1.4 copies this record into the header of `load.ts`, the file § 10.1's provenance header lives in (Readings 13).

**Acceptance:** `fixture_structure_test` passes with four new snapshots; the Vitest fixtures directory holds three files; nothing secret or host-specific remains.

### Task 1.4: The planted fixture (frontend lane, M)

**Spec:** § 10.1 bullet 2 (`planted.units.json`) in full.

**Interfaces (Produces):** `src/__tests__/fixtures/systemd-units/planted.units.json`: the six unit fields at top level, plus one more key, `u8_after_include`, holding the six fields of the view the mocked `applyOp` returns for U8 (b). A loader `src/__tests__/fixtures/systemd-units/load.ts`: `loadUnitFields(name: "kitchen-sink" | "planted" | "stock10" | "stock9"): UnitFields` (strips `u8_after_include`) and `loadPlantedAfterInclude(): UnitFields`. `UnitFields` is Task 2.1's type; until it exists the loader declares it locally and Task 2.1 moves it.

- [ ] **Step 1: Failing test first.** `src/__tests__/fixtures/systemd-units/planted.test.ts`, one `it` per planted case of § 10.1, each finding its record and asserting the fields that make it that case: a locked row whose `attention_reason` is `service-image-mode-incompatible`; an excluded runtime updater; a `kind: "path"` row; a `kind: "slice"` row; a mount finding; a drop-in with `withheld`; an orphan drop-in with no `kind`; a link with `relation: "alias"`; a link with cause `vendor_install_only`; a `default_target` with `declined`; two instance rows with a `required_config` path each (config rows are not unit fields, so U16 in Task 4.4 supplies a config row for the first path and none for the second); `getty@.service` (masked) and `getty@tty2.service` (`instance_of: "getty@.service"`); one unit name in `unit_states`, `mount_findings`, `undetermined_installs` and a manual link's `unit`; the U8 pair (a unit row declined because its `[Install]` drop-in is excluded, and in `u8_after_include` the drop-in included and `declined` absent); `undetermined-install-demo.service` as a unit row beside an undetermined install for `demo.service`; three manual links for one unit (`wants` under `multi-user.target`, `wants` under `graphical.target`, `requires` under `multi-user.target`); two identical `wants` links for `dup-link-demo.service` under `multi-user.target` and one for `dup-link-demo-other.service`; `dup-state-demo.service` twice in `unit_states`, carried, and `/etc/systemd/system/dup-state-demo.service.d/override.conf` twice in `unit_dropins`. It also asserts the kitchen-sink records are all present unchanged, and that no record outside the `dup-*` ones contains `dup-link-demo` or `dup-state-demo`.
- [ ] **Step 2:** `npx vitest run src/__tests__/fixtures/systemd-units` fails: no file.
- [ ] **Step 3:** build the file from `kitchen-sink.units.json` by a `python3 -I` script outside the repository. Every string (reasons, `declined.text`, advisory lines, `withheld.text`, the locked slug) is copied from source or from a kitchen-sink record of the same case, never invented: `declined.text` for the `[Install]` case from the pipeline's rule 5 constant in `crates/pipeline/src/render/unit_plan.rs`, the locked slug from `normalize_incompatible_services`. The planted default target replaces the kitchen-sink one (one target per view). Each record has every always-present field.
- [ ] **Step 4: Provenance header** (§ 10.1, `kitchen-sink.units.json` bullet). The header comment of `src/__tests__/fixtures/systemd-units/load.ts` records, from Task 1.3's commit body (`git log --grep "record the kitchen-sink units session" -1`): the inspectah commit refine was built from, the tarball's name, the requests sent to that refine, and how the six fields were cut from the view; that `stock10` and `stock9` are hand-built from the R4 records of `process-docs/skills/real-host-validation-runs.md`; and that `planted.units.json` is kitchen-sink plus the hand-made records `planted.test.ts` names, with where each string was copied from.
- [ ] **Step 5:** the test passes; `npx tsc --noEmit` is clean.
- [ ] **Step 6: Commit** `test(web): add the planted units fixture`.

**Acceptance:** every § 10.1 planted case is asserted present by name; `load.ts`'s header records how the kitchen-sink recording was taken and at which inspectah commit.

---

## Phase 2: the pure logic (3 tasks)

Spec: § 6.1, § 6.2, § 6.4 (counts, visibility, pins as an input), § 6.6 item 11, § 6.8 placement, § 6.11 keys, § 6.12 matching and search text, § 7 strings 2 to 5, 10, 15, 17 to 22. Tests: the logic halves of U1 to U5, U7, U20, U21, U24, U25, U35. All in `unitOutcome.ts`; no component changes.

### Task 2.1: Row keys, item ids and occurrence keys (frontend lane, M)

**Spec:** § 6.11 Row keys, Occurrence keys, "Which row an item id names"; § 6.12 True duplicates (key half).

**Files:** Create `src/components/unitOutcome.ts`; Test `src/components/__tests__/unitOutcome.keys.test.ts`.

**Interfaces (Produces):**

```ts
export type RowKind = "unit_state" | "dropin" | "default_target"
  | "manual_link" | "undetermined_install" | "mount_finding";
export type RowKey = string;        // JSON.stringify([kind, ...identity])
export type OccurrenceKey = string; // `${RowKey}#${ordinal}`
export interface UnitFields {
  unit_states: ServiceDecisionDto[]; unit_dropins: DropInDecisionDto[];
  default_target?: DefaultTargetView; mount_findings: MountFindingDto[];
  manual_links: ManualLinkView[]; undetermined_installs: UndeterminedInstallView[];
}
export type UnitRecord =
  | { kind: "unit_state"; data: ServiceDecisionDto }
  | { kind: "dropin"; data: DropInDecisionDto }
  | { kind: "default_target"; data: DefaultTargetView }
  | { kind: "manual_link"; data: ManualLinkView }
  | { kind: "undetermined_install"; data: UndeterminedInstallView }
  | { kind: "mount_finding"; data: MountFindingDto };
export type RecordOf<K extends RowKind> = Extract<UnitRecord, { kind: K }>;
export interface Keyed<R extends UnitRecord = UnitRecord> {
  record: R; rowKey: RowKey; occurrenceKey: OccurrenceKey;
}
export interface KeyedFields {
  unitStates: Keyed<RecordOf<"unit_state">>[]; dropins: Keyed<RecordOf<"dropin">>[];
  defaultTarget?: Keyed<RecordOf<"default_target">>;
  mountFindings: Keyed<RecordOf<"mount_finding">>[]; manualLinks: Keyed<RecordOf<"manual_link">>[];
  undeterminedInstalls: Keyed<RecordOf<"undetermined_install">>[];
}
export function rowKey(record: UnitRecord): RowKey;
export function rowKeyForItemId(itemId: string): RowKey | null;
export function keyFields(fields: UnitFields): KeyedFields;
```

- [ ] **Step 1: Failing tests first** (`describe("§ 6.11 keys")`), table-driven:
  - `rowKey` per kind gives exactly `["unit_state","cups.service"]`, `["dropin","/etc/systemd/system/httpd.service.d/override.conf"]`, `["default_target"]`, `["manual_link","wants","multi-user.target","driftify-static.service"]` (relation, parent, unit; `cause` ignored), `["undetermined_install","driftify-upheld.service"]`, `["mount_finding","tmp.mount"]`, compared as strings.
  - `rowKeyForItemId`: a bare name `cups.socket` gives `["unit_state","cups.socket"]`; `backup\x2djob.timer` gives `["unit_state","backup\\x2djob.timer"]` (JSON escaping of the backslash); a well-formed key of each kind comes back byte-identical; `[`, `["nope","x"]`, `["unit_state"]` (missing part), `["manual_link","wants","x"]` and `[1]` give `null`.
  - The prefixed-name case: `undetermined-install-demo.service` as a unit row and `demo.service` as an undetermined install have different keys, and `rowKeyForItemId("undetermined-install-demo.service")` names the unit row.
  - `keyFields(planted)`: the two `dup-link-demo.service` links get `#0` and `#1` on one row key; `dup-link-demo-other.service` `#0`; the two `dup-state-demo.service` rows `#0` and `#1`; their two drop-ins `#0` and `#1`; the default target `["default_target"]#0`; every occurrence key in the response is unique; the shared unit name of the four kinds gives four `#0` keys of four kinds. Calling `keyFields` twice on the same input gives equal keys.
- [ ] **Step 2:** `npx vitest run src/components/__tests__/unitOutcome.keys.test.ts` fails: module missing.
- [ ] **Step 3: Implement.** `JSON.stringify` of the array with no further argument. Ordinals counted per source list in response order, before any grouping. Parsing checks the kind and the part count for that kind and that every part is a string. Move `UnitFields` out of the Task 1.4 loader to import it from here.
- [ ] **Step 4:** tests pass; tsc clean; whole Vitest suite green. **Commit** `feat(web): add row and occurrence keys for the units section`.

**Acceptance:** the key tests pass; nothing else spells a key.

### Task 2.2: Outcomes, counts, groups and labels (frontend lane, M)

**Spec:** § 6.1; § 6.2 (order, sort, templates, drop-in nesting under the first occurrence, orphan placement); § 6.3 item 4's N (counted over every row of the group); § 6.4 Values and counts; § 6.6 items 2, 11, 12 (the maps); § 6.8; § 7 strings 2 to 5, 10, 15, 17, 18. Tests: U1; U2; U3, U4, U5, U20 and U21 on the model.

**Files:** Modify `src/components/unitOutcome.ts`; Test `src/components/__tests__/unitOutcome.test.ts`.

**Interfaces (Consumes):** Task 2.1. **(Produces):**

```ts
export type Outcome = "needs_follow_up" | "carried" | "excluded";
export function outcomeOf(record: UnitRecord): Outcome | null; // null for "dropin"
export interface OutcomeCounts { all: number; needs_follow_up: number; carried: number; excluded: number }
export function outcomeCounts(keyed: KeyedFields): OutcomeCounts;
export type GroupId = "default_target" | "services" | "sockets" | "timers" | "paths" | "other_units" | "reported";
export const GROUP_ORDER: readonly GroupId[];
export const GROUP_TITLES: Record<GroupId, string>;      // strings 2 to 5
export interface UnitBranch { head: Keyed<RecordOf<"unit_state">>; dropins: Keyed<RecordOf<"dropin">>[] }
export interface OrphanBlock { unit: string; dropins: Keyed<RecordOf<"dropin">>[] }
export type ReportedRow = Keyed<RecordOf<"mount_finding" | "manual_link" | "undetermined_install">>;
export interface ReportedBlock { unit: string; rows: ReportedRow[] }
export interface UnitGroupModel {
  id: GroupId; target?: Keyed<RecordOf<"default_target">>;
  branches: UnitBranch[]; orphans: OrphanBlock[]; reported: ReportedBlock[];
  followUpTotal: number; // § 6.3 item 4's N, kind groups only, else 0
}
export function groupUnits(keyed: KeyedFields): UnitGroupModel[];
export const OUTCOME_LABELS: Record<DeclinedView["renders"], "FIXME" | "Advisory" | "Omitted">;
export const RELATION_LABELS: Record<LinkRelation, string>;   // string 17
export function causeLabel(cause: ManualLinkCause): string;     // string 18
export const UNDETERMINED_CAUSE_LABEL: string;                  // "[Install] cannot be relied on"
export function followUpFlagText(n: number): string | null;     // string 10; null when n === 0
```

- [ ] **Step 1: Failing tests first,** over the fixtures:
  - U1, table: declined beats `include: false` beats carried, for a unit row and the default target; every reported row is `needs_follow_up`; a locked, excluded row is `excluded`; a drop-in is `null`.
  - U2: `outcomeCounts(kitchen-sink)` is `{ all: 23, needs_follow_up: 9, carried: 14, excluded: 0 }`; adding drop-ins to the input changes none of it. These numbers are also the check on Task 1.3's recording: if they differ, stop and report before Task 2.3.
  - U3 on the model: kitchen-sink group ids in order `default_target, services, sockets, timers, other_units, reported` with 11, 6, 1, 1 (`tmp.mount`) branches and 3 reported rows; no `paths`. Planted: a `paths` group exists, and the slice is in `other_units`.
  - U4 on the model: branch heads in each group are in code-point order of `unit`; `getty@.service` heads its own branch in `services` and `getty@tty2.service` is a separate branch beside it; `wg-quick@wg0.service` is among services by name; the two `dup-state-demo.service` heads are adjacent in response order, the first holding both drop-in records and the second none.
  - U5 on the model: `followUpTotal` is 5 for services and 1 for other units in kitchen-sink, 0 elsewhere; `followUpTotal` of default target and reported is 0. `followUpFlagText(1)` is `"1 needs follow-up"`, `followUpFlagText(5)` `"5 need follow-up"`, `followUpFlagText(0)` `null`.
  - U20 on the model: reported blocks sorted by unit; in `driftify-upheld.service`'s block the link row precedes the undetermined row; with the planted shared name the order is mount finding, links, undetermined install; `RELATION_LABELS` and `causeLabel` give every string of § 7 strings 17 and 18 for every `LinkRelation` and `ManualLinkCause` value, including `has alias` and `Only the package unit names it`, and `{undetermined:{reason:"upheld_by"}}` gives `[Install] cannot be relied on`. `OUTCOME_LABELS` gives string 15.
  - U21 on the model: `sshd.service`'s full shadow is an orphan block in `services`; the planted drop-in with no `kind` is an orphan block in `other_units`.
  - Review focus 4: a unit row with `kind` `swap`, one with `automount`, and one with `kind` `"widget"` all land in `other_units`.
- [ ] **Step 2:** run; fails (functions missing).
- [ ] **Step 3: Implement.** Sort with a plain `<` comparison on strings (code-point order), stable, so duplicates keep response order. Drop-ins nest under the first branch whose head has their `unit`; drop-ins with no such branch become orphans in the group of their `kind`, `other_units` without one; orphan blocks per unit in response order of first appearance (today's order).
- [ ] **Step 4:** pass; tsc; Vitest. **Commit** `feat(web): compute unit outcomes, counts and kind groups`.

**Acceptance:** the listed U assertions on the model pass.

### Task 2.3: Filter, search, pins and location (frontend lane, M)

**Spec:** § 6.4 "What a value does" and pinning (as a visibility rule); § 6.3 item 3 (counts under filter and search); § 6.11 the reveal's "hidden by" tests and "first row in section order"; § 6.12 matching and both search-text tables; § 6.9 (which empty state applies); § 7 strings 19 to 22. Tests: U7 and U24 matching on the model; U25 search text; U35 (b) counts on the model.

**Files:** Modify `src/components/unitOutcome.ts`; Test `src/components/__tests__/unitOutcome.visibility.test.ts`.

**Interfaces (Consumes):** Tasks 2.1, 2.2. **(Produces):**

```ts
export type FilterValue = "all" | Outcome;
export interface ShowState { filter: FilterValue; query: string; pins: ReadonlySet<OccurrenceKey> }
export interface ShownGroup {
  id: GroupId; target?: Keyed<RecordOf<"default_target">>;
  branches: UnitBranch[]; orphans: OrphanBlock[]; reported: ReportedBlock[];
  count: number;           // § 6.3 item 3; none for default_target (0)
  followUp: number | null; // § 6.3 item 4; null unless filter "all" and N > 0
}
export function showGroups(groups: UnitGroupModel[], state: ShowState): ShownGroup[]; // undrawn groups omitted
export function shownRowCount(shown: ShownGroup[]): number;   // resultCount, § 6.12
export function branchHeadOf(groups: UnitGroupModel[], toggled: OccurrenceKey): OccurrenceKey; // pin target
export function sectionSearchText(branchOrRow: UnitBranch | Keyed): string; // lowercased
export function globalSearchText(record: UnitRecord): string;              // lowercased, § 6.12 table
export interface RowLocation { groupId: GroupId; branchHead: OccurrenceKey; occurrenceKey: OccurrenceKey }
export function locateRow(groups: UnitGroupModel[], key: RowKey): RowLocation | null; // first in section order
export function hiddenByQuery(groups: UnitGroupModel[], loc: RowLocation, state: ShowState): boolean;
export function hiddenByFilter(groups: UnitGroupModel[], loc: RowLocation, state: ShowState): boolean;
export function liveRegionText(filter: FilterValue, n: number): string; // strings 19 to 22; n is shownRowCount after the change
```

The gate A fix rounds add, in this module, the helpers that decide the rest of the section's rules: `filterBarDrawn` and `filterBarPresentation` (the bar, its disabled values and the hint), `emptyStateFor` (§ 6.9), `revealPlan` (§ 6.11's three steps), `pinsInView`, and `shownRowOrder`, `firstShownRow`, `rovingTabStop` and `rowMoveTarget` (§ 6.13). Components and the state hook call them and decide none of these themselves.

A branch is a unit row with its drop-ins, the default target, one orphan drop-in, or (for visibility only) one reported row.

- [ ] **Step 1: Failing tests first.**
  - U7 on the model, per value over kitchen-sink: "needs_follow_up" leaves services (5 branches), other units (`tmp.mount`) and reported (3); "carried" leaves default target, services, sockets, timers, without reported or orphans; "excluded" leaves nothing; "all" everything. A shown branch keeps its drop-ins; orphans only under "all"; reported only under "all" and "needs_follow_up".
  - Pins: with `pins` holding a carried branch's head under "needs_follow_up", that branch is shown in its place with all its drop-ins and its group drawn; with a query that excludes it, still shown; a pin on an orphan's occurrence key shows that orphan under any filter; `branchHeadOf` maps a nested drop-in's occurrence key to its parent's head and leaves a head, the target and an orphan as themselves.
  - Matching (U24 on the model): `declined.text` words match their unit row; a drop-in path matches its parent and shows all its drop-ins; an advisory line matches its reported row; the query is lowercased; a unit row's text holds `unit`, `kind`, `owning_package` or the fragment word, `current_state`, `default_state`, `instance_of`, `declined.text`, `attention_reason` and drop-in paths; target `target`; orphan `path` and `unit`; a manual link `unit`, `parent`, `relation`, its relation phrase (string 17) and advisories; an undetermined install `unit`, its reason words (`_` as a space), `install_files` and advisories; a mount finding `unit`, `kind`, `mount_point` and advisories (§ 6.12, spec § 2 item 10). For each reported row, the section search text equals `globalSearchText` of its record; for a unit row, it equals `globalSearchText` plus its drop-ins' paths; both are built from one field list per row kind.
  - `count` per group under filter and query excludes drop-ins and orphans, counts each reported row once, is 0 for the default target; `followUp` counts every follow-up row in the group under "all" whether or not the query hides it, and is `null` under any other filter.
  - `globalSearchText` (U25 on the model): per § 6.12's table; the default target's text holds `default target`; with the planted shared name, a word only in the undetermined install's `reason` (with `_` as a space, as the row shows it) is in that record's text and not in the unit row's; `graphical` is in only one of the three planted links' text, `requires` in only one, and `required by` in only that same one.
  - U35 (b) on the model: query `dup-link-demo` gives 3 shown reported rows, `dup-link-demo.service` 2, and `shownRowCount` equals each.
  - `locateRow` for the dup link key gives the `#0` occurrence; for `["unit_state","dup-state-demo.service"]` the first head; `hiddenByQuery` and `hiddenByFilter` are true and false in the cases U26 uses (a member hidden by "needs_follow_up"; one failing the query; one pinned).
  - `liveRegionText`: `("all", 23)` "Showing all 23 rows.", `("all", 1)` "Showing all 1 row.", `("needs_follow_up", 9)` "Showing 9 rows that need follow-up.", `("needs_follow_up", 1)` "Showing 1 row that needs follow-up.", `("carried", 14)` "Showing 14 carried rows.", `("carried", 1)` "Showing 1 carried row.", `("excluded", 2)` "Showing 2 excluded rows.", `("excluded", 1)` "Showing 1 excluded row.".
- [ ] **Step 2:** run; fails.
- [ ] **Step 3: Implement.** No filter or query changes a key; the functions read occurrence keys assigned by `keyFields`.
- [ ] **Step 4:** pass; tsc; Vitest. **Commit** `feat(web): compute what the units filter, search and pins show`.

**Acceptance:** every rule the section applies is decided here and tested without React.

---

## Phase 3: the section renders by groups (3 tasks)

Spec: § 6.2, § 6.3, § 6.5, § 6.6 items 1 to 4, 6, 8 to 12, § 6.7, § 6.8, § 6.9 (rows without the filter), § 6.13 grid structure, § 6.14 heading, § 9.1. Tests: U3, U4, U5 (All), U10 to U14, U17 to U23 (without the filter), U31 heading half, U32 structure half, U35 (a) rows and Reported count (its filter counts are Task 4.1's), U37 (a). The filter bar does not exist yet; the section shows "All".

### Task 3.1: `UnitGroup` (frontend lane, S)

**Spec:** § 6.3; § 6.13 Group headers and grid structure; § 6.2 last bullet (collapse keeps rows in the DOM under `hidden`). Test: U32 structure half.

**Files:** Create `src/components/UnitGroup.tsx`; Test `src/components/__tests__/UnitGroup.test.tsx`.

**Interfaces (Produces):** `UnitGroup(props: { groupId: GroupId; title: string; count?: number; followUpText?: string | null; description?: string; expanded: boolean; onToggle: () => void; children: React.ReactNode }): JSX.Element`. It renders a `<button>` (the `AngleRightIcon`/`AngleDownIcon` pair, the title in an element with id `unit-group-title-<groupId>`, a read PatternFly `Badge` with the count when `count` is given, an orange outline compact `Label` with `followUpText` when given), `aria-expanded`, `aria-controls="unit-group-grid-<groupId>"`; then `description` when given; then `<div role="grid" id="unit-group-grid-<groupId>" aria-labelledby="unit-group-title-<groupId>" hidden={!expanded}>` with one `role="rowgroup"` holding `children`. Group ids are fixed words, so the DOM ids hold no host data.

- [ ] **Step 1: Failing tests first** (U32 structure): `aria-controls` equals the grid's `id`; the element `aria-labelledby` names has the title text alone (no count, no flag); a `role="row"` child sits in a `role="rowgroup"` inside the grid; Enter and Space on the header toggle `aria-expanded` and the grid's `hidden`; collapsed rows stay in the DOM; the accessible name of the button is "Services 11, 5 need follow-up" for title Services, count 11, flag "5 need follow-up" (§ 6.3, punctuation as the spec writes it); no count badge when `count` is undefined or 0, the name then being the title alone, or the title, a comma and the flag ("Services, 5 need follow-up"; § 6.3, spec § 2 item 9).
- [ ] **Step 2:** run; fails. **Step 3:** implement, following `TriageBucketGroup`'s button pattern without reusing it. **Step 4:** pass; tsc; Vitest. **Commit** `feat(web): add the collapsible units group`.

**Acceptance:** the structure assertions pass on the component.

### Task 3.2: `ServiceSection` renders through the groups (frontend lane, L, at the limit)

**Spec:** § 6.2; § 6.3 placement; § 6.5; § 6.7; § 6.8; § 6.9 rows 1 to 3 and 5's precedence (the filter and search rows come in Tasks 4.1 and 5.1); § 6.11 `data-unit-key` and occurrence-key React keys; § 6.12 a duplicated unit row's drop-ins under its first occurrence; § 6.14 heading and `MainContent` label; § 9.1; strings 1 (heading), 2 to 6, 11 to 13. Tests: U3, U4, U5 (under All), U10, U21, U23 (rows for `stock10`, empty view), U31 heading half, U35 (a) rows and Reported count, U37 (a).

**Files:** Modify `src/components/ServiceSection.tsx`, `src/components/MainContent.tsx` (units branch: heading, `SECTION_LABELS.units`, no shadow count); Test `src/components/__tests__/ServiceSection.groups.test.tsx`; update `ServiceSection.test.tsx`, `ServiceSection.orphan-dropins.test.tsx`, `SectionPlumbing.test.tsx` (`"Services (1 shadow override)"` becomes "Systemd units"), `src/__tests__/ScheduledTimer.unitLink.test.tsx` (heading "Services" becomes "Systemd units").

**Interfaces (Consumes):** `keyFields`, `groupUnits`, `showGroups` with `{ filter: "all", query: "", pins: new Set() }`, `GROUP_TITLES`, `followUpFlagText`, `UnitGroup`. **(Produces):** every row element carrying its row test id also carries `data-unit-key={rowKey}`; collapse state `Record<GroupId, boolean>` in the section, reset on unmount.

- [ ] **Step 1: Failing tests first** (render `ServiceSection` with the fixtures and a stub `pendingGuard`):
  - U3: group headers in order Default target, Services 11, Sockets 6, Timers 1, Other units 1, Reported, not carried 3; no Paths; planted draws Paths and the slice under Other units.
  - U4: in Services the row test ids follow code-point order; `service-item-getty@tty2.service` is not inside `service-group-getty@.service`, and its row's markup is equal with and without the template row in the input.
  - U5 under All: Services header text holds "5 need follow-up", Other units "1 needs follow-up", Sockets, Timers, Default target and Reported none.
  - U10 (rendering): Default target group holds `default-target` with string 11 for an included, an excluded and a declined target, no kind chip; the declined target also shows its `declined.text` (the existing `declined-default-target` assertions in `ServiceSection.links.test.tsx` stay and pass). Its outcome label is Task 3.3's (U12).
  - U21: `dropin-orphan-group-sshd.service` is inside the Services group, after its last unit row, with "Other drop-in overrides" and "These overrides have no unit-state decision row."; the planted no-`kind` orphan is inside Other units.
  - Reported group: string 6 under its header; every reported row's existing test id.
  - U23 rows: `stock10` shows the Default target group and string 12; an empty view (no target) shows string 13, and "No service state changes detected." appears nowhere; `stock9` shows Services 1.
  - U31 heading: `MainContent` for `units` renders an `h2` "Systemd units" with no shadow count, against kitchen-sink (which has a full shadow). The section's container (`service-section`) is a `role="region"` with the accessible name "Systemd units" (string 1, its last use; § 6.13), and no row carries `aria-rowindex`, since the per-group grids are now named by their titles.
  - U35 (a), counting shown `role="row"` elements: two rows with `data-unit-key` equal to the `dup-link-demo.service` link key, one for `dup-link-demo-other.service`; two unit rows with `["unit_state","dup-state-demo.service"]`; two drop-in rows with the drop-in's key, both inside the first unit row's `service-group-` wrapper and none in the second's; Reported header count equals the shown reported rows. U35 (a)'s filter-count clause needs the bar and is Task 4.1's.
  - Every rendered row has `data-unit-key` equal to its record's row key (one assertion over all rows of planted).
  - U37 (a): a view built from kitchen-sink's default target and `sshd.service`'s orphan full shadow alone draws Services with the orphan row, no count badge, and the button name "Services" (§ 6.3, spec § 2 item 9).
- [ ] **Step 2:** run; fails.
- [ ] **Step 3: Implement.** Every row's React key and the key of the element wrapping a unit row's branch is its occurrence key (§ 8, `ServiceSection` bullet); keep `linkIdentity` only if a test id still needs it, otherwise delete it. Keep every test id, the toggles, guard keys and Space or `x` handling as today. The orphan block's heading and note and the Reported description must not add `role="row"` elements (U35 counts rows). Update the existing tests listed under Files whose assertions this changes, naming each in the commit body.
- [ ] **Step 4:** pass; tsc; whole Vitest suite. **Commit** `feat(web): group the units section by kind`.

**Acceptance:** the listed assertions pass; the whole Vitest suite is green; no test id is gone (`git grep` the old test-id literals in the diff).

### Task 3.3: The row changes (frontend lane, M)

**Spec:** § 6.6 items 1 to 4, 6, 8, 9, 10, 11 (render), 12; § 6.5 last bullet (the target's outcome label); strings 15, 16, 17, 18, 23. Tests: U11, U12, U13, U14, U17, U18, U19, U20 (render), U22.

**Files:** Modify `src/components/ServiceSection.tsx`; Test `src/components/__tests__/ServiceSection.rows.test.tsx`; update `ServiceSection.test.tsx` (`"disabled (preset: enable)"`, `"Service excluded"`), `ServiceSection.kinds.test.tsx` (kind chip on every row), `ServiceSection.links.test.tsx` (relation and cause words) where they assert the old text.

- [ ] **Step 1: Failing tests first:**
  - U11: a row shows `enabled` then `preset: disable`, no parentheses; the preset element carries neither an italic style nor an opacity below 1, and uses the subtle-text token class or variable; `default_state: null` shows the state alone.
  - U12: `FIXME` on `tmp.mount`, `Advisory` on `cups.service`, `Omitted` on `wg-quick@wg0.service`, each a compact outline `Label`, and on planted's declined default target.
  - U13: no "Non-Default State" text anywhere in the section; "Unknown Origin" on `driftify-also-main.service`; "Drop-in Override" and "Shadow override" unchanged.
  - U14: `cups.service` shows its `declined.text` and not its `attention_reason`; an undeclined row with an `attention_reason` shows it.
  - U17: `wg-quick@wg0.service`'s first line holds `Instance of wg-quick@.service` after `(wireguard-tools)`.
  - U18: `(local)` on `driftify-also-main.service`; `(shadowed)` and `(absent)` on two planted rows the test edits to `owning_package: null` with that `fragment` (planted holds only `local` and `vendor`); `(httpd)` on `httpd.service`.
  - U19: a kind chip on `tmp.mount` in Other units and on the planted slice, none on any row in Services, Sockets, Timers or Paths.
  - U20 render: `driftify-upheld.service`'s link row and undetermined row are adjacent siblings; "wanted by" before `multi-user.target` on `driftify-static.service`'s row; the cause label in the badge cell; every relation and cause from planted renders its string 17 or 18; the undetermined row shows `[Install] cannot be relied on` and its reason as today (`upheld by`).
  - U22: with the parent excluded, its drop-in shows "Unit excluded" and "Service excluded" is nowhere.
- [ ] **Step 2:** run; fails. **Step 3:** implement with `OUTCOME_LABELS`, `RELATION_LABELS`, `causeLabel`. The outline `Label` colours: FIXME orange, Advisory blue, Omitted grey (§ 6.6 item 2). **Step 4:** pass; tsc; Vitest. **Commit** `feat(web): show state, outcome and one reason on each units row`.

**Acceptance:** the listed U tests pass; every served string is still rendered as given.

---

## Phase 4: filter, pinning and reveal (5 tasks, then the mid-way gate)

Spec: § 6.4, § 6.6 items 5 and 7, § 6.9 rows 1 and 4, § 6.11 (routes, reveal steps 2 and 3, lookup), § 6.13 Announcements. Tests: U6, U7, U8 (a), (b), (d), (e) filter-change and bar clauses, U9, U15, U16, U23 (filter rows), U26 (`Also=` link filter and collapse cases, global search case reveal half), U27, U33, U35 (a) filter counts, U25's "Go to owner" clause.

### Task 4.1: The outcome filter (frontend lane, M)

**Spec:** § 6.4 Values and counts, When it is drawn, disabled values, hint, What a value does, Persistence, Announcement; § 6.9 rows 1 and 4; § 6.13 The filter, Announcements; strings 7, 8, 9, 14, 19 to 22. Tests: U5 (no flag under a filter), U6, U7, U9, U23 (filter rows), U33, U35 (a) filter counts.

**Files:** Create `src/components/UnitOutcomeFilter.tsx`; Modify `src/components/ServiceSection.tsx`; Test `src/components/__tests__/UnitOutcomeFilter.test.tsx`, `src/components/__tests__/ServiceSection.filter.test.tsx`.

**Interfaces (Produces):** `UnitOutcomeFilter(props: { counts: OutcomeCounts; value: FilterValue; onChange: (v: FilterValue) => void; announcement: string }): JSX.Element`: label "Show", `ToggleGroup aria-label="Show"`, four items with count in their text, disabled at count 0 unless selected, hint (string 9) while `counts.needs_follow_up > 0`, all as `filterBarPresentation` in `unitOutcome.ts` decides them, and a `<div aria-live="polite">` always rendered holding `announcement`. `announcement` is `liveRegionText(value, shownRowCount(...))` over the rows shown after the change, orphan drop-ins included (§ 6.4 Announcement). `ServiceSection` owns `filter: FilterValue` (initial "all") until Task 6.2 moves it to the state hook.

- [ ] **Step 1: Failing tests first:**
  - U6: no bar on `stock10` or `stock9`; bar on kitchen-sink with "All 23", "Needs follow-up 9", "Carried 14", "Excluded 0"; "Excluded" disabled; hint present; with no follow-up row (a kitchen-sink copy with every `declined` removed and one row excluded) the bar shows and the hint does not.
  - U7 render: clicking each value shows the rows Task 2.3's U7 test lists, groups with none not drawn.
  - U5: after selecting "Needs follow-up", no group header shows a follow-up flag.
  - U9: unmount and remount gives "All"; a rerender with a new view keeps the selected value; a view in which follow-up and excluded both drop to 0 removes the bar and the filter reads "All" when the bar returns. Selecting a value leaves `window.location.href` unchanged and adds no `localStorage` key (§ 6.4 Persistence).
  - U35 (a) filter counts (`planted`): render `planted`, then a copy without the second `dup-link-demo.service` link record, then a copy without the second `dup-state-demo.service` `unit_states` record (both copies built in the test). Against `planted`'s bar, the first copy's All and Needs follow-up each read one lower and Carried and Excluded are unchanged; the second copy's All and Carried each read one lower and Needs follow-up is unchanged. So All and Needs follow-up count both link records, and All counts both unit records; a count keyed by row key fails.
  - U23 filter rows: `stock10` with `include: false` shows the bar with "All 1" and "Excluded 1" and string 12; `stock10` with planted's declined target shows "All 1", "Needs follow-up 1", the hint and string 12; a value that shows no row shows "No units match this filter." and a "Show all" link button that sets "All".
  - U33: before any change the live region exists and is empty; after selecting each value its text is the matching string 19 to 22 for the number of rows shown, orphan drop-ins included, singular and plural (use `stock10` excluded for the singulars). On kitchen-sink, "Needs follow-up" reads "Showing 9 rows that need follow-up." and returning to "All" reads "Showing all 24 rows." while the bar reads "All 23". U33's query and reveal clauses need section search and are Task 5.1's.
- [ ] **Step 2:** run; fails. **Step 3:** implement. The bar sits between the heading and the first group. **Step 4:** pass; tsc; Vitest. **Commit** `feat(web): add the units outcome filter`.

**Acceptance:** the listed tests pass.

### Task 4.2: Pinning and focus after a toggle (frontend lane, M)

**Spec:** § 6.4 pinning (every sentence); § 6.13 Focus after a toggle; § 12 third bullet (a `Set` of head occurrence keys). Tests: U8 (a), (b), (d), and (e) for the filter change and the bar ceasing to be drawn. U8 (c), and (e)'s query-change clause and both of its reveal clauses, are Task 5.1's.

**Files:** Modify `src/components/ServiceSection.tsx`; Test `src/components/__tests__/ServiceSection.pinning.test.tsx`.

**Interfaces (Produces):** `pins: Set<OccurrenceKey>` in `ServiceSection`, added through `branchHeadOf` on every toggle the operator makes (click, Space, `x`), cleared on a filter value change and on unmount, filtered on each view to keys still present; `clearPins()` exposed internally for Task 5.1, which clears pins when a reveal changes the filter or the query.

- [ ] **Step 1: Failing tests first** (mock `applyOp` as the existing ServiceSection tests do):
  - U8 (a): under "Needs follow-up", toggle `cups.service`; the mock returns kitchen-sink with `cups.service` `declined` cleared and `include: false`. The row stays in its place (same previous and next sibling), keeps focus, has no outcome label, and the counts read Needs follow-up 8 and Excluded 1. The Services header count equals the unit rows Services shows, the pinned `cups.service` among them; a further view in which another Services row's `declined` is cleared with no toggle on it (an Undo) lowers that count by one while `cups.service` stays shown (§ 6.4: a pinned row's group count updates from each view response).
  - U8 (b): planted under "Needs follow-up"; toggle the parent's excluded `[Install]` drop-in to included; the mock returns `loadPlantedAfterInclude()`. The parent's branch stays in place, the drop-in is included and focused, the parent shows no outcome label, the counts update.
  - U8 (d): rerender with a view in which a row's outcome changed and no toggle was made on its branch (an Undo): it follows the filter and leaves.
  - U8 (e), filter half: after (a), selecting "Carried" then "Needs follow-up" again drops the pin (`cups.service` no longer shown); a view in which the bar stops being drawn clears no pin.
  - Of two branches with one row key (planted `dup-state-demo.service`), toggling the second pins only the second.
  - Review focus 1: a view that no longer holds a pinned head's record drops that pin, and nothing throws.
  - Review focus 2: `applyOp` rejects; the branch stays shown, focus stays, counts unchanged, the failure message shows as today.
- [ ] **Step 2:** run; fails. **Step 3:** implement; focus is restored by occurrence key after the view update. **Step 4:** pass; tsc; Vitest. **Commit** `feat(web): keep a toggled units row in place under the filter`.

**Acceptance:** the listed tests pass.

### Task 4.3: Reveal into the section (frontend lane, M)

**Spec:** § 6.11 Routes (Scheduled Tasks, "Go to owner"), Which row an item id names, The reveal steps 2 and 3, Reveal lookup; § 6.2 collapse; § 8 `App.tsx`, `shadowOwner.ts`, `MainContent` (passing `revealItemId` and `onNavigateToItem`). Tests: U26's global search case, reveal half (a filter and a collapsed group, driven through `handleNavigateFromSearch`), U27, U25's "Go to owner" clause. Reveal steps 2 and 3 are also tested here by calling the navigation handler; U26's `Also=` cases go through the link, which exists from Task 4.4, and are Tasks 4.4's and 5.1's. U8 (e)'s "a reveal that changes the filter drops every pin" is Task 5.1's: under "All" with no query a pin changes nothing on screen, so the clause can be seen only with a query that survives the reveal.

**Files:** Modify `src/App.tsx`, `src/components/MainContent.tsx`, `src/components/ServiceSection.tsx`, `src/components/shadowOwner.ts`; Test `src/__tests__/UnitsReveal.test.tsx` (mounts `App`), extend `src/__tests__/ScheduledTimer.unitLink.test.tsx`; update `src/components/__tests__/shadowOwner.test.ts` (target `itemId` becomes `["dropin","<path>"]`).

**Interfaces (Consumes):** `rowKeyForItemId`, `locateRow`, `hiddenByFilter`. **(Produces):** `ServiceSection` props `revealItemId?: string` and `onNavigateToItem?: (sectionId: string, itemId: string) => void`; in `App.tsx`, `elementByUnitKey(key: RowKey): HTMLElement | null`, which walks `document.querySelectorAll("[data-unit-key]")` and compares `getAttribute("data-unit-key") === key`; the reveal effect uses it, and not `REVEAL_TESTID_PREFIXES`, when the navigation's section is `units`. `REVEAL_TESTID_PREFIXES` is unchanged.

- [ ] **Step 1: Failing tests first:**
  - Reveal step 2: `App` on units with "Needs follow-up" selected; navigate (`handleNavigateFromSearch("units", "cups.socket")`, the call an `Also=` link will make): the filter reads "All", `service-item-cups.socket` is focused.
  - Reveal step 3: collapse Sockets; navigate to `cups.socket`: Sockets is open, the row focused.
  - U26 global search case, reveal half: a filter selected and the destination's group collapsed; navigate with the row key `["unit_state","cockpit.socket"]`, as a units search result will: shown and focused. Selecting the real result from the search box is Task 5.2's.
  - Lookup: a bare name resolves to the `unit_states` row even when a reported row of the same name exists (planted shared name); `["undetermined_install","demo.service"]` focuses that row and not `undetermined-install-demo.service`'s unit row, and the bare name `undetermined-install-demo.service` focuses `service-item-undetermined-install-demo.service` (U25's bare-name clauses); an id starting with `[` that does not parse focuses nothing and throws nothing.
  - U27 (extend the existing file): select a filter and collapse Timers in units; go to Scheduled Tasks; follow the timer row's unit link; the units section shows "All", every group open, the unit row focused.
  - U25 "Go to owner": on a shadowed config row, "Go to owner" focuses its drop-in row; `resolveShadowOwner` returns `itemId` `["dropin","<path>"]`.
  - Review focus 3: a `unit_states` row `backup\x2djob.timer`; navigating with the bare name and with its row key both focus it.
- [ ] **Step 2:** run; fails. **Step 3: Implement.** `MainContent` passes `revealItemId` and `onNavigateToItem` to `ServiceSection`. `ServiceSection` resolves `revealItemId` with `rowKeyForItemId` and `locateRow`, then sets the filter to "All" if `hiddenByFilter` and opens the group if collapsed, before `App`'s reveal effect looks the row up (§ 12 leaves the sequencing; a layout effect keyed on the reveal is enough). `shadowOwner.ts` builds the key with `rowKey`.
- [ ] **Step 4:** pass; tsc; Vitest. **Commit** `feat(web): reveal units rows by row key through filter and collapse`.

**Acceptance:** the listed tests pass; other sections' reveal tests are unchanged and green.

### Task 4.4: `Also=` and configuration links (frontend lane, S)

**Spec:** § 6.6 items 5 and 7; § 6.11 `Also=` route. Tests: U15, U16, U26's `Also=` filter and collapse cases (through the link).

**Files:** Modify `src/components/ServiceSection.tsx`, `src/components/MainContent.tsx` (pass the `config_files` paths); Test `src/components/__tests__/ServiceSection.navlinks.test.tsx`, extend `src/__tests__/UnitsReveal.test.tsx`.

**Interfaces (Produces):** `ServiceSection` prop `configPaths?: ReadonlySet<string>`, optional, defaulting to an empty set (no path links).

- [ ] **Step 1: Failing tests first:**
  - U15: on `cups.service`'s `Also=` line, `cups.socket` is an `<a href="#units:cups.socket">` and `cups.path` is `<code>`; the `links` word beside each is unchanged; clicking `cups.socket` calls `onNavigateToItem("units", "cups.socket")` and does not follow the hash; through `App` (in `UnitsReveal.test.tsx`), the click focuses `service-item-cups.socket`.
  - U26 `Also=` cases, in `UnitsReveal.test.tsx` (mounts `App` on units); each first asserts `service-item-cups.socket` is hidden, then clicks the `cups.socket` link on `cups.service`'s `Also=` line with the user-event API. Filter case: with "Needs follow-up" selected, the click sets the filter to "All" and `service-item-cups.socket` is shown and focused. Collapse case: with Sockets collapsed, the click opens Sockets and the row is shown and focused.
  - U16: planted's row whose `required_config` path is in `configPaths` shows a link that calls `onNavigateToConfig(path)`; the other path is `<code>`; `data_paths` are `<code>`.
- [ ] **Step 2:** run; fails. **Step 3:** implement. **Step 4:** pass; tsc; Vitest. **Commit** `feat(web): link Also= members and needed configuration to their rows`.

### Task 4.5: Mid-way review gate (frontend lane, S)

- [ ] Merge `main` into the branch if it moved; run every gate of the Verification convention except the Playwright rows (no P test exists yet). Commit nothing unless the merge needs a commit.
- [ ] Request the implementation review below; fix rounds land as `fix(web): ...` commits on the branch.

**Review gate A (mid-way):** the section renders by group with filter, pinning and reveal working, so a wrong foundation is caught before search and accessibility build on it. **In scope:** Tasks 1.2 to 4.4, and only what they build. Spec § 6.1 to § 6.3; § 6.4 except what needs section search (pins cleared by a query change or by a reveal, counts unchanged by a query); § 6.5; § 6.6 items 1 to 12 and "Unchanged"; § 6.7; § 6.8; § 6.9 rows 1 to 4; § 6.11 (keys, occurrence keys, item ids, reveal steps 2 and 3, lookup, the Scheduled Tasks, `Also=` and "Go to owner" routes); § 6.14 (sidebar label and heading); § 9.1; § 10.1; the Rust assertion and re-recordings of § 10.2; strings 1 (sidebar, heading), 2 to 23. Tests U1 to U7, U8 (a, b, d, and e's filter-change and bar clauses), U9 to U23, U26 (`Also=` link filter and collapse cases, global search case reveal half), U27, U32 (structure half), U33, U35 (a), and U25's "Go to owner" clause. Not in scope here, and checked at gate B: § 6.6 item 13 (Task 7.1), § 6.9 row 5 and the search parts of § 6.4 (Task 5.1). **Checks:** `unitOutcome.ts` holds every rule and no component re-derives one; occurrence keys are the React keys and the pin and focus keys, and never reach an id, URL, `data-unit-key` or test id; no selector is built from host data; no fleet product code changed (`git diff main --stat -- crates/web/src crates/web/ui/src/components/AggregateApp.tsx crates/web/ui/src/components/aggregate ':(exclude)crates/web/ui/src/components/aggregate/__tests__/AggregateApp.integration.test.tsx'` prints nothing); every test id survives; the recorded fixtures hold nothing secret. **Gate commands:** `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D clippy::all`, `cargo test --workspace`, and in `crates/web/ui` `npx tsc --noEmit` and `npx vitest run`.

---

## Phase 5: search (2 tasks)

Spec: § 6.12, § 6.11 reveal step 1 and the global search route, § 6.15 (search half), § 9.2. Tests: U8 (c) and (e) query and reveal clauses, U24, U25, U26 (`Also=` link query cases, global search case from the box), U31 global half, U33 query and reveal clauses, U34 search half, U35 (b) and (c), U37 (b).

### Task 5.1: Section search in the units section (frontend lane, M)

**Spec:** § 6.12 Section search (the search text of every row kind, spec § 2 item 10); § 6.11 reveal step 1; § 6.4 pins cleared by a query change and by a reveal that changes the query or the filter, counts not affected by section search, and the announcement under a query; § 6.3 the badge of an orphan-only group under a query; § 6.9 row 5; § 9.2. Tests: U24, U8 (c), U8 (e) query-change and reveal clauses, U26 `Also=` link query cases, U33 query and reveal clauses, U35 (b) and (c), U37 (b).

**Files:** Modify `src/components/MainContent.tsx`, `src/components/ServiceSection.tsx`; Test `src/__tests__/UnitsSectionSearch.test.tsx` (mounts `App`), extend `ServiceSection.pinning.test.tsx`, `src/__tests__/UnitsReveal.test.tsx`, `ServiceSection.groups.test.tsx`.

**Interfaces (Produces):** `ServiceSection` props `filterText: string`, `onResultCount: (n: number) => void`, `onClearQuery: () => void` (supplied by `MainContent`, which owns the text), and an imperative `focusFirstRow()` reached the way `handleArrowDown` reaches other sections' first rows.

- [ ] **Step 1: Failing tests first:**
  - U24: `/` in units opens `SectionSearch`; a word of `cups.service`'s `declined.text` shows that row; `override.conf` shows `httpd.service` with both drop-ins; a word of a reported advisory line shows that row; the result count equals `shownRowCount`; a query with no match shows "No items match your search" with "Clear filter", and not string 14 even with a filter selected; with `planted`, `required by` shows the `requires` link and no `wants` link, `foreign name` shows `demo.service`'s undetermined-install row, and `/data/demo` shows the administrator's mount finding; ArrowDown from the box focuses the first shown row; query and filter combine (a query matching a carried row under "Needs follow-up" shows nothing but the empty state); with a query that hides rows, the bar's four counts are the same as with no query (§ 6.4, counts not affected by section search).
  - U8 (c): a query that matches `cups.service` only through its `declined.text`; toggle it as in U8 (a); it stays shown.
  - U8 (e): changing the query drops every pin; a reveal that clears the query drops every pin; a reveal that sets the filter to "All" and keeps the query drops every pin. For the last, the test builds its view from kitchen-sink so that, under "Needs follow-up" and a query, a pinned row stops matching the query after its toggle (as in U8 (c)) while the reveal's destination matches the query and is hidden by the filter; after the reveal the formerly pinned row is no longer shown.
  - U26 `Also=` query cases, through the link as in Task 4.4: with a query that shows `cups.service` and hides `cups.socket` (asserted hidden first), clicking the `cups.socket` link clears the query, the search box stays open, and the row is shown and focused; with a query both match, the click keeps the query and focuses the row.
  - U33 query and reveal clauses: with a section query, a filter change announces the rows shown under the query; a reveal that sets the filter to "All" announces string 19 for the rows then shown.
  - U37 (b): kitchen-sink under "All" with a query that only `sshd.service`'s orphan path matches (asserted first): Services shows the orphan alone with no count badge and the button name "Services, 5 need follow-up"; clearing the query brings the badge "11" back.
  - U35 (b) rendered: query `dup-link-demo` 3 link rows, then `dup-link-demo.service` 2, then `dup-link-demo` 3, then cleared the rows of U35 (a); at each query step the shown `role="row"` count, the Reported header count and the result count are equal. U35 (c): filter "Carried" shows both `dup-state-demo.service` unit rows and their two drop-in rows, Services header count equal to its shown unit rows; "All" restores U35 (a).
- [ ] **Step 2:** run; fails. **Step 3: Implement.** `ServiceSection` decides reveal step 1 (`hiddenByQuery`, which knows the pins) and calls `onClearQuery`; steps 1, 2 and 3 are evaluated in order, each against the state the step before leaves; a reveal whose step 1 or step 2 changes the query or the filter calls `clearPins()`. **Step 4:** pass; tsc; Vitest. **Commit** `feat(web): wire section search into the units section`.

### Task 5.2: Global search, single-host units index (frontend lane, L)

**Spec:** § 6.12 Global search, Search text travels on the entry, Duplicates, True duplicates (search half), the paragraphs on the reference section and aggregate mode, Backlog effect; § 6.14 second bullet (`GlobalSearch`'s label map keeps "Services"); § 6.15 bullets 1, 2, 4 (the `isAggregateMode` doc comment); § 8 `AppShell`, `GlobalSearch`, `App.tsx`. Tests: U25, U31 global half, U34 search half.

**Files:** Modify `src/components/GlobalSearch.tsx`, `src/components/AppShell.tsx`, `src/App.tsx` (`SingleHostApp` passes the unit fields); Test `src/__tests__/UnitsGlobalSearch.test.tsx` (mounts `App`), extend `src/components/aggregate/__tests__/AggregateApp.integration.test.tsx`; update `src/components/__tests__/GlobalSearch.dropins.test.tsx` (drop-in `itemId` becomes `["dropin","<path>"]` in single-host mode).

**Interfaces (Produces):** `GlobalSearchResult.searchText?: string`, read by the filter before the two `searchTextMap` lookups; `GlobalSearch` props `isAggregateMode?: boolean`, `unitStates?`, `defaultTarget?`, `mountFindings?`, `manualLinks?`, `undeterminedInstalls?`; `AppShell` passes them through. Units entries use `rowKey` as `itemId`, `globalSearchText` as `searchText` (the § 6.12 search text, which for a link holds its relation phrase too), and the label "Systemd units"; they put nothing in `searchTextMap`. The `units` reference section is skipped in single-host mode only.

- [ ] **Step 1: Failing tests first:**
  - U25: in single-host `App`, a `unit_states` row, a drop-in, the default target, a manual link, an undetermined install and a mount finding (planted) are each a result whose `itemId` is its row key, and selecting each focuses its row; no result comes from the `units` reference section (seed it with an entry whose title appears nowhere else); a word of `wg-quick@wg0.service`'s omission reason finds it. The planted shared name gives four results with four item ids, and a word only in the undetermined install's reason lists that result and not the unit row's; the prefixed-name pair gives two results with different ids and texts, each focusing its own row; the three links give three results, `graphical` lists only the `graphical.target` link, and `requires` and `required by` each list only the `requires` link, each focusing its own row; `dup-link-demo.service` gives two results with one item id, either focusing the first link row in section order, and the same for `dup-state-demo.service`'s unit rows.
  - U31 global half: single-host units results are labelled "Systemd units", and none "Services".
  - U26 global search case, completed: from within the units section, with "Needs follow-up" selected and Sockets collapsed, select the global search result for `cockpit.socket`: the row is shown and focused.
  - U34 search half (fleet `AggregateApp` with a `units` section): a search for one of its items lists a result labelled "Services"; selecting it navigates as today; no "Systemd units" text appears.
- [ ] **Step 2:** run; fails. **Step 3:** implement, conditioned on `isAggregateMode` false everywhere. Update the `isAggregateMode` doc comment in `AppShell.tsx` to name both uses. **Step 4:** pass; tsc; Vitest. **Commit** `feat(web): index units rows by row key in single-host global search`.

**Acceptance:** U25 and the search halves of U31 and U34 pass; the fleet view's search tests are unchanged and green.

---

## Phase 6: keyboard, accessibility and the state hook (2 tasks)

Spec: § 6.13 (one tab stop, row movement, toggle, focus after a toggle); § 8 the state hook. Tests: U32 (movement, roving), U36. U32's grid structure and header keys are Task 3.1's. Task 6.2 moves the section's state, the tab stop included, into its hook once Task 6.1 has added the last of it.

### Task 6.1: Roving tab stop and row movement (frontend lane, M)

**Files:** Modify `src/components/ServiceSection.tsx`; Test `src/components/__tests__/ServiceSection.keyboard.test.tsx`.

**Interfaces (Produces):** `tabStop: OccurrenceKey | null` in `ServiceSection`, recomputed after every change of shown rows.

- [ ] **Step 1: Failing tests first:**
  - U32 movement: `j`, `k`, ArrowDown and ArrowUp move across groups, skip a collapsed group, and visit unit rows, drop-ins, the default target and reported rows; `g` and `G` go to the first and last shown row; Space and `x` toggle a unit, drop-in and target row; on a reported row they do nothing.
  - U32 roving: exactly one row in the section has `tabIndex` 0 and it has no `[hidden]` ancestor; after collapsing its group, after a filter change and after a section query hides it, the `0` moves to the first shown row each time; with no row shown, no row has `tabIndex` 0. Filter buttons, headers and in-row links stay tab stops; checkboxes stay focusable.
  - U36 (planted): from `dup-link-demo-other.service`'s link row, `j` focuses the first `dup-link-demo.service` link row, `j` the second (a different element with the same `data-unit-key`), `k` the first; from the first `dup-state-demo.service` unit row, `j` goes through its two drop-in rows to the second unit row; after every move exactly one row has `tabIndex` 0, it is `document.activeElement`, and it is shown. `x` on the second `dup-state-demo.service` occurrence (mock returns planted unchanged) leaves focus and the tab stop on the second.
  - Review focus 1: a view that drops the record holding the tab stop moves it to the first shown row.
- [ ] **Step 2:** run; fails. **Step 3:** implement, matching `DecisionList`'s keys. **Step 4:** pass; tsc; Vitest. **Commit** `feat(web): add roving focus and row keys to the units section`.

### Task 6.2: The section's state in `useUnitsSectionState.ts` (frontend lane, S)

**Spec:** § 8 (`hooks/useUnitsSectionState.ts`, `ServiceSection`); § 2 item 12; § 12 second bullet. Tests: no new test id. The move changes no behaviour (§ 2 item 12), so the section's existing tests are its test.

**Files:** Create `src/hooks/useUnitsSectionState.ts`; Modify `src/components/ServiceSection.tsx`.

**Interfaces (Produces):** `useUnitsSectionState(...)`, taking what `ServiceSection` takes today for its state (the six unit fields, `filterText`, `revealItemId`, `onClearQuery`, `onResultCount`, the toggle handler) and returning what it renders and the handlers it calls: the shown groups, counts, filter value and its setter, the live region text, collapse state and its toggle, the tab stop, row refs and the toggle-with-pin handler. The exact signature is the builder's.

- [ ] **Step 1:** on the branch head, run tsc and the whole Vitest suite; record the number of tests passing.
- [ ] **Step 2:** move the filter value, the live region text, the pins, collapse state, the reveal's steps as applied, the roving tab stop and focus after a toggle, with the effects that keep them, from `ServiceSection` into the hook. The hook calls `unitOutcome.ts` for every rule and decides none; no rule leaves `unitOutcome.ts` and none is added. `ServiceSection` keeps its file and component name, props, test ids and markup, and holds rendering only.
- [ ] **Step 3:** tsc clean; the whole Vitest suite passes with the same count and no test file changed. A test that reached into the component's internals and must change is named in the commit body, with why.
- [ ] **Step 4: Commit** `refactor(web): move the units section state into a hook`.

**Acceptance:** `git diff` of the commit touches only the two files; `unitOutcome.ts` and every test are unchanged; `ServiceSection.tsx` keeps none of the state Step 2 lists.

---

## Phase 7: the remaining defect fixes and the owner badge (4 tasks)

Spec: § 9.3, § 9.4, § 9.5, § 6.6 item 13, § 6.15 bullet 3; § 6.14 the owner badge (string 26). Tests: U28, U29, U30, U34 footer half, U31's owner badge clause. Defect 9.1 is Task 3.2's and 9.2 Task 5.1's (Readings 1). Task 7.2 edits only `Sidebar.tsx` and its test, which no task of Phases 4 to 6 edits, so a second implementer may run it beside those phases. Tasks 7.1 and 7.3 may not: 7.1 edits `ServiceSection.tsx`, and 7.3 edits `AppShell.tsx` and `AggregateApp.integration.test.tsx`, which Phases 4 to 6 also edit; they run after Phase 6. Task 7.4 edits `shadowOwner.ts` and two tests that no task still to come in Phases 5 and 6 edits, so it may run beside them too. With one implementer all four run here.

### Task 7.1: The locked row's label (frontend lane, S)

**Spec:** § 9.3; § 6.6 item 13; string 24. Test: U28.

**Files:** Modify `src/components/attentionUtils.ts` (beside `formatTriageReason`), `src/components/ServiceSection.tsx`; Test `src/components/__tests__/ServiceSection.locked.test.tsx`, a case in the `attentionUtils` tests.

**Interfaces (Produces):** `lockedReasonLabel(reason: string | null | undefined): string`: `service-image-mode-incompatible` gives "Incompatible with image mode", `null` or `undefined` "LOCKED", anything else itself.

- [ ] **Step 1: Failing tests first** (U28): with planted, the locked row's badge reads "Incompatible with image mode", its checkbox label holds it, and `service-image-mode-incompatible` appears nowhere in `container.innerHTML`; a locked row with a null reason shows "LOCKED"; a locked drop-in's prose reason is unchanged.
- [ ] **Step 2-4:** fail, implement, pass; tsc; Vitest. **Commit** `fix(web): name the image-mode lock on a units row instead of its slug`.

### Task 7.2: The Scheduled Tasks badge (frontend lane, S)

**Spec:** § 9.4 Required (the `kernel_boot` and `selinux` note is out of scope). Test: U29.

**Files:** Modify `src/components/Sidebar.tsx` (`decisionCount`); Test extend `src/components/__tests__/Sidebar.test.tsx`.

- [ ] **Step 1: Failing tests first** (U29): with a `scheduled_tasks` reference section of three rows whose `generated_timer` names a `generated_timers` entry (one in a subsection) and one row without, the badge reads 3; making one entry `include: false` with `cron_job_excluded` set gives 2; before the sections or the view load it reads `...`; with no generated-timer row it reads 0.
- [ ] **Step 2-4:** fail, implement, pass; tsc; Vitest. **Commit** `fix(web): count generated-timer toggles in the Scheduled Tasks badge`.

### Task 7.3: The Containerfile panel footer (frontend lane, S)

**Spec:** § 9.5; § 6.15 bullets 1 and 3; § 2 item 8; string 25. Tests: U30, U34 footer half.

**Files:** Modify `src/components/ContainerfilePanel.tsx`, `src/components/AppShell.tsx`; Test extend `src/components/__tests__/ContainerfilePanel.test.tsx`, `src/components/aggregate/__tests__/AggregateApp.integration.test.tsx`.

- [ ] **Step 1: Failing tests first:** U30, the single-host panel footer reads "Preview reflects every include and exclude decision you make in refine."; U34 footer half, the fleet panel's footer reads today's text, "Preview reflects package and config decisions. Context sections are included as-is.".
- [ ] **Step 2-4:** fail, implement (`ContainerfilePanel` takes `isAggregateMode`), pass; tsc; Vitest. **Commit** `fix(web): say every decision reaches the single-host Containerfile preview`.

### Task 7.4: The owner badge names the Systemd units section (frontend lane, S)

**Spec:** § 6.14 the owner badge bullet; § 6.15 (single-host by where it is rendered, no mode boundary); § 8 `shadowOwner.ts`; § 2 item 13; string 26. Test: U31's owner badge clause.

**Files:** Modify `src/components/shadowOwner.ts` (`resolveShadowOwner`'s `label`); Test update `src/components/__tests__/shadowOwner.test.ts`, `src/components/__tests__/MainContent.shadowed-configs.test.tsx`. `DecisionItem.shadow-owner.test.tsx` feeds the label as an input and stays.

- [ ] **Step 1: Failing tests first** (U31 owner badge clause): `resolveShadowOwner` for a config entry with `shadowed_by: "units"` returns the label "Controlled in Systemd units"; in `MainContent` on the configs section, the shadowed config row's `shadow-owner-badge` reads it, and "Controlled in Service Drop-ins" appears nowhere.
- [ ] **Step 2-4:** fail, implement (the label only: the explanation sentence and "Go to owner" stay), pass; tsc; Vitest. **Commit** `fix(web): name the Systemd units section on a shadowed config row`.

---

## Phase 8: Playwright (3 tasks)

Spec: § 10.1 (setup), § 10.3, § 10.2's e2e label paragraph. Tests: P1 to P8. All in `crates/web/ui/e2e/systemd-units.spec.ts` (Readings 7).

### Task 8.1: Setup, rendering, filter, search and Scheduled Tasks (frontend lane, M)

**Spec:** § 10.1 "Every P test calls one setup function"; P1, P2, P4, P5. Also § 10.2: `e2e/sections.spec.ts` and `e2e/smoke.spec.ts` assert "Systemd units" for the single-host sidebar.

**Files:** Create `e2e/systemd-units.spec.ts`; Modify `e2e/sections.spec.ts`, `e2e/smoke.spec.ts`.

**Interfaces (Produces):** `async function setupUnits(page: Page, view: "kitchen-sink" | "stock10" = "kitchen-sink")`: `applyMockApi(page, "single-host", { "/api/view": <abs path of view-units-...>, "/api/snapshot/sections": <abs path of sections-units-kitchen-sink.json> })`, then a `page.route("**/api/groups", ...)` serving `src/__tests__/fixtures/system-tuning/files-no-tuned.groups.json`, as `e2e/system-tuning.spec.ts` does; `page.on("pageerror")` collected and asserted empty in `afterEach`.

- [ ] **Step 1: Failing tests first:** P1 (groups of U3 in order, bar "All 23", "Needs follow-up 9", "Carried 14", "Excluded 0", sidebar "Systemd units" with 24); P2 (click "Needs follow-up": 9 rows across Services, Other units and Reported; "All": 24 rows shown, the orphan drop-in included, beside the bar's "All 23"); P4 (global search `wg0`, select the Systemd units result, `service-item-wg-quick@wg0.service` focused; no units result labelled "Services"); P5 (sidebar Scheduled Tasks, then the timer row with `unit_row: true` reveals and focuses its unit row). Change the two "Services" sidebar assertions to "Systemd units".
- [ ] **Step 2:** run `npx playwright test e2e/systemd-units.spec.ts`. These are acceptance tests over behaviour Phases 3 to 7 built, so they may pass at once. A P test that fails for a product reason is a defect: write the Vitest test that reproduces it first, see it fail, then fix the component.
- [ ] **Step 3:** all four pass, none skipped. Run `npx playwright test e2e/sections.spec.ts e2e/smoke.spec.ts` and compare with the baseline (Readings 8).
- [ ] **Step 4: Commit** `test(web): cover the units section in the browser`.

### Task 8.2: Keyboard only, with the recorded exclude (frontend lane, S)

**Spec:** P3; § 10.1 the sequence body.

- [ ] **Step 1:** P3: `setupUnits`, then `mockSequence(page, "/api/view", [<view-units-kitchen-sink abs path>, <01-after-exclude abs path>], { triggerOn: "/api/op" })`. Tab to the bar, Space on "Needs follow-up", Tab into the rows, `j` to `tmp.mount`, `x`. After the op: the row is in the same place (same neighbours), focused, and the bar's counts are the post-exclude body's (computed in the test from that file with the Task 2.2 rules, not typed in).
- [ ] **Step 2:** run; pass; none skipped. **Commit** `test(web): toggle a units row from the keyboard in the browser`.

### Task 8.3: Axe, widths and dark theme (frontend lane, M)

**Spec:** P6, P7, P8; § 6.13 Narrow widths and Themes.

- [ ] **Step 1:** P6: `expectNoAxeViolations` (or `AxeBuilder` with the existing impact filter) on the units section with the bar under "All" and under "Needs follow-up", and on `setupUnits(page, "stock10")`: no critical or serious violations. P7: at 768 x 1024, `document.scrollingElement.scrollWidth <= clientWidth` and the bar's buttons span more than one line (their `top` values differ, or the bar's height exceeds one button's); at 1280 x 800 they share one line. Review focus 5: at 768 x 1024, with the view route overridden so one unit row has a 200-character name and a long `declined.text`, no horizontal scroll. P8: with the page in dark theme (as the existing dark-theme tests set it), axe `color-contrast` passes on the outcome labels, a follow-up flag and the preset text.
- [ ] **Step 2:** run; fix violations in the components, not by excluding rules. **Step 3:** all P tests pass, none skipped. **Commit** `test(web): check the units section for accessibility, width and dark theme`.

---

## Phase 9: changelog, docs, final gate and merge (3 tasks)

### Task 9.1: Changelog, docs and skills (frontend lane, S)

**Spec:** § 11 in full. The skill update is the repository's rule (`CLAUDE.md`, skill file maintenance), not the spec's.

- [ ] Merge `main` into the branch and run every gate.
- [ ] **CHANGELOG.md** under `## [Unreleased]` per `process-docs/changelog-policy.md` (a feature branch's entry is written at merge time, so here): `### Changed`, one entry for the Systemd units section (its name; groups by kind; the outcome filter and follow-up flag; the reported rows' group; the default target's explanation; the row changes: preset, outcome label, one reason, `Also=` and configuration links, no "Non-Default State" badge; the fleet view unchanged). `### Fixed`, one entry each: the empty-state text, section search in the units section, the locked row's badge, the Scheduled Tasks badge, the Containerfile panel footer in single-host refine.
- [ ] **Docs:** `docs/how-to/review-and-refine.md` (the "Services & Scheduling" table row names "Systemd units"; the "Other drop-in overrides" sentence says orphan drop-ins sit at the end of the group of their kind; the units-rows paragraph describes the groups, the filter, "Needs follow-up" and the reported group); `docs/reference/triage-classification.md` (the "Services" row renamed, and a note that the units section shows no badge for the non-default-state reason); `docs/reference/inspector-coverage.md` ("the units section" becomes "the Systemd units section"). `docs/tutorials/first-migration.md` and the getting-started output are not changed.
- [ ] **Skills:** `process-docs/skills/web-disposition-contract.md` § 1c "Row identity" and § 1b "Reveal matches test ids exactly": the units section's rows carry a row key in `data-unit-key`, its reveal finds rows by that attribute, and occurrence keys are the React keys; update its row in `process-docs/skills/index.md`. `process-docs/skills/e2e-fixtures-are-hand-maintained.md` § "The Recorded Alternative": name the recorded units bodies and how they were taken.
- [ ] Gate; **commit** `docs: describe the Systemd units section`.

### Task 9.2: Final review gate (frontend lane, S)

- [ ] Run every gate of the Verification convention, including both Playwright rows. Compare the whole e2e run with the baseline of Task 1.1: list each test that passes now and failed then, and each that fails now and passed then; the second list must be empty or explained by a named cause outside this branch.
- [ ] Request the implementation review below; fix rounds land as commits on the branch, and the gate commands are run again after each.

**Review gate B (final, whole branch):** **In scope:** every task, against the whole spec as amended on 2026-11-16: § 2 to § 11 (§ 2 items 9 to 13 included), strings 1 to 26, defects § 9.1 to § 9.5, U1 to U37 and P1 to P8, the CHANGELOG and docs of § 11, and the skill updates. That includes what gate A left out: § 6.4's search parts and its announcement as amended, § 6.3's badge rule, § 6.6 item 13 and DOM ids, § 6.9 row 5, § 6.12 with its amended search text, § 6.13, § 6.14's owner badge, § 6.15, § 8's state hook, and the clauses of U8, U24, U26, U31, U32, U33, U34, U35 and U37 owned by Tasks 3.2 to 7.4. **Checks:** everything gate A checks, again over the whole branch, and that `useUnitsSectionState.ts` decides no rule itself; the single-host scoping of § 6.15 (no fleet product code changed: `git diff main --stat -- crates/web/src crates/web/ui/src/components/AggregateApp.tsx crates/web/ui/src/components/aggregate ':(exclude)crates/web/ui/src/components/aggregate/__tests__/AggregateApp.integration.test.tsx'` prints nothing; that test file changes only by U34's additions; U34 green); the global search index skips the `units` reference section only in single-host mode; every units search result resolves; no new served string; every path `git diff main --name-only` lists is in one of these sets: the files of the File structure table (`src/hooks/useUnitsSectionState.ts` among them); tests and test data (`crates/web/tests/` and its snapshots, every `__tests__/` directory under `crates/web/ui/src` with its fixtures, `crates/web/ui/e2e/` specs, fixtures and `manifest.json`); and Task 9.1's `CHANGELOG.md`, `docs/how-to/review-and-refine.md`, `docs/reference/triage-classification.md`, `docs/reference/inspector-coverage.md`, `process-docs/skills/web-disposition-contract.md`, `process-docs/skills/e2e-fixtures-are-hand-maintained.md` and `process-docs/skills/index.md`; the new P tests pass on their own with none skipped; no e2e test fails now that passed in the baseline. **Gate commands:** `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D clippy::all`, `cargo test --workspace`, and in `crates/web/ui` `npx tsc --noEmit`, `npx vitest run`, `npx playwright test e2e/systemd-units.spec.ts`, `npx playwright test`.

### Task 9.3: Merge (project owner)

- [ ] With review gate B passed, the project owner merges `feat/systemd-units-refine-section` to `main` and removes the worktree.
- [ ] The merge's follow-up, on `main`: move the spec from `process-docs/specs/proposed/` to `process-docs/specs/implemented/` (updating its Metadata status and the links that name its path), replace this plan's status marker with the executed form and evidence, and update its row in `process-docs/plans/README.md`, per that file's rule. Update the backlog items named above, outside this repository.

---

## Sizes

| Phase | Tasks (Rust / frontend / either / owner) | Size |
|---|---|---|
| 1 branch, baseline, fixtures, label | 4 (2 / 1 / 1 / 0) | M |
| 2 pure logic | 3 (0 / 3 / 0 / 0) | M |
| 3 rendering | 3 (0 / 3 / 0 / 0) | L |
| 4 filter, pinning, reveal | 5 (0 / 5 / 0 / 0), gate A | L |
| 5 search | 2 (0 / 2 / 0 / 0) | M to L |
| 6 keyboard, state hook | 2 (0 / 2 / 0 / 0) | M |
| 7 defects, owner badge | 4 (0 / 4 / 0 / 0) | S |
| 8 Playwright | 3 (0 / 3 / 0 / 0) | M |
| 9 docs, gate B, merge | 3 (0 / 2 / 0 / 1) | S |

Task sizes: S is one short dispatch, M one full dispatch, L one dispatch at the limit of what a single review can check from the diff. The L tasks, 3.2 and 5.2, are the ones to watch for a split: 3.2 can split into "groups and headers" and "default target, reported and orphans, empty states"; 5.2 into "the index and `searchText`" and "the scoping and fleet regression".

## Readings and plan choices

Where the plan reads the spec one way of two, or orders work the spec does not. None changes the design.

1. **Defects 9.1 and 9.2 are not in the defect phase.** § 9.1's fix is § 6.9's empty states, built with the section's rendering (Task 3.2); § 9.2's fix is § 6.12's section search (Task 5.1), and § 12 says 9.2 needs § 6.12. Phase 7 holds the three fixes that stand alone (9.3, 9.4, 9.5). Otherwise the briefed order holds.
2. **U32's grid structure and header keys land with `UnitGroup`** (Task 3.1), the component that makes them; movement and roving are Task 6.1's. Building the grid wrong first and fixing it in Phase 6 would rework Task 3.2.
3. **Test ids that span phases are split by what they need.** U8 (c) and (e)'s query and reveal clauses, and U26's query cases, need section search, which comes after the mid-way gate, so they are Task 5.1's; their filter and collapse halves are Phase 4's. U26's `Also=` cases go through a real click on the link, so the filter and collapse cases are Task 4.4's, the first task with the link, and the query cases 5.1's; Task 4.3 tests the reveal steps by calling the navigation handler, which is not U26's route. U26's global search case is tested in 4.3 by sending the row key a units result will send, and completed in 5.2 by selecting the real result, which exists only from 5.2. U8 (e)'s two reveal clauses are 5.1's even though 4.3 builds step 2: under "All" with no query a pin changes nothing on screen, so dropping it can be seen only with a query that survives the reveal. U3, U4, U5, U20 and U21 are tested on the model in Phase 2 and rendered in Phase 3. U23's filter rows are Task 4.1's. U25's "Go to owner" clause is Task 4.3's (the `shadowOwner.ts` change); the rest is 5.2's. U31 splits heading (3.2), search label (5.2) and owner badge (7.4); U33's base clauses are 4.1's and its query and reveal clauses 5.1's; U34 search (5.2) and footer (7.3); U37 (a) needs no query and is 3.2's, (b) needs one and is 5.1's. U35 (a)'s rows and Reported count are 3.2's and its filter counts 4.1's, the first task with the bar; (b) and (c) are 5.1's (with (b) also on the model in 2.3). Each id is complete once every listed task is; the table under Coverage, "Split test ids", lists every clause and its task.
4. **Reveal step 1 is decided by the section and carried out by `MainContent`.** § 6.11 says `MainContent` clears the query when the destination's branch fails the match and is not pinned; the pins live in the section (§ 6.4, § 12). So the section decides (`revealPlan` in `unitOutcome.ts`, applied by the section's state) and calls `onClearQuery`, which `MainContent` supplies; `MainContent` still owns and clears the text.
5. **The reveal's steps are evaluated in order, each against the state the step before leaves.** Step 1 clears pins, so step 2 asks whether the filter hides the destination with no pins.
6. **`planted.units.json` holds U8's second view under one extra top-level key, `u8_after_include`.** § 10.1 says the fixture "has the pair" without a format; one file keeps the pair beside the rows it changes. The loader strips the key.
7. **All P tests live in `e2e/systemd-units.spec.ts`,** with one setup function (§ 12 leaves P6 to P8's place open).
8. **The Playwright baseline is an empty commit's body on the branch,** so every later gate and reviewer reads the same list without a file in the tree. The two e2e specs whose "Services" assertion changes (`sections.spec.ts`, `smoke.spec.ts`) serve no `/api/groups`, so the sidebar renders skeletons and those tests are expected to be in the failing baseline already; their edit is checked by reading the diff, and the gate's comparison shows whether they changed state.
9. **The kitchen-sink recording is a Rust-lane task** because it builds and runs refine and registers fixtures in `crates/web/tests`; the stock fixtures are hand-built in the same task because `view-units-stock10.json` is made from `stock10.units.json`.
10. **U35's result-count check applies where a query is active.** `resultCount` is shown only with a section query, and § 6.12 counts unit rows, the target, orphan drop-ins and reported rows, not nested drop-ins; at U35's query steps every shown row is a reported row, so the row count, the Reported header count and the result count are equal. At the unfiltered and "Carried" steps the plan checks the Reported and Services header counts against their shown rows, as U35 (a) and (c) name them.
11. **Changelog timing.** `process-docs/changelog-policy.md` writes a feature branch's entry at merge time, so the entries are written in Task 9.1, just before the final gate, not task by task.
12. **Collapse state is a `Record<GroupId, boolean>` held by the section** and the filter, pins and tab stop beside it: in `ServiceSection` as Tasks 3.2 to 6.1 build them, then in `useUnitsSectionState.ts` from Task 6.2 (§ 8). `ServiceSection` stays mounted through filter, search and `Also=` reveals (§ 6.11), and unmounts on a section change, which resets all of them as § 6.2 and § 6.4 require.
13. **§ 10.1's provenance header goes in `src/__tests__/fixtures/systemd-units/load.ts`.** § 10.1 asks "the test file's header" to record the inspectah commit and how `kitchen-sink.units.json` was taken; JSON holds no comment, and `load.ts` sits beside the fixtures and is the one file every test reads them through. Task 1.4 writes it from Task 1.3's commit body, which keeps the same record.

## Coverage

### Spec sections

| Spec section | Task(s) |
|---|---|
| § 2 items 1, 4, 5, 6 | 2.2, 3.2, 3.3, 4.1 |
| § 2 items 2, 3, 8 (scope) | Global constraints; 5.2, 7.3; gates A and B |
| § 2 item 7 (defects) | 3.2 (9.1), 5.1 (9.2), 7.1, 7.2, 7.3 |
| § 2 items 9 to 13 (amended 2026-11-16) | 3.1 and 3.2, 5.1 under a query (item 9, badge); 2.3, 5.1, 5.2 (item 10, search text); 4.1, 5.1 (item 11, announcement); 6.2 (item 12, state hook); 7.4 (item 13, owner badge) |
| § 3.1 goals | 2.2, 3.2, 3.3, 4.1, 4.2, 6.1, 8.3 |
| § 3.2 non-goals, known limits | Not built, by design: no `Also=` repair lines (3.3 shows `declined.text` only); a withheld drop-in found by the filter only through its parent (2.3); no change to `adapter.rs` (Global constraints); fleet footer kept (7.3) |
| § 3.3 anti-goals | 2.2 (placement by kind and name), 4.2 (no move on toggle), 2.3 (filter hides, never copies), Global constraints (strings) |
| § 4 today | Reference only |
| § 5 approaches | Reference only; option C is § 2 item 1 |
| § 6.1 outcome | 2.2 |
| § 6.2 groups, order, sort, templates, nesting, orphans, collapse | 2.2 (model), 3.1 (collapse), 3.2 (render) |
| § 6.3 group header | 3.1, 2.3 (count, flag), 3.2 (U37 (a)), 5.1 (badge under a query, U37 (b)) |
| § 6.4 filter: values, counts, drawn, disabled, hint, values' effect | 2.2, 2.3, 4.1; 5.1 (counts not affected by section search) |
| § 6.4 pinning | 2.3 (visibility), 4.2 (pin on toggle, filter change clears, group and filter counts update), 5.1 (query change clears, a reveal that changes the query or the filter clears) |
| § 6.4 persistence (not in `localStorage` or the URL), announcement | 4.1; 5.1 (announcement under a query and after a reveal) |
| § 6.5 default target | 3.2, 3.3 (outcome label) |
| § 6.6 items 1 to 4, 6, 8 to 12 | 3.3 (item 11 ordering also 2.2) |
| § 6.6 items 5, 7 | 4.4 |
| § 6.6 item 13 | 7.1 |
| § 6.6 "Unchanged", `data-unit-key` and DOM ids | 3.2, 3.3 (a true duplicate's own ids); gates A and B |
| § 6.7 reported group | 3.2 |
| § 6.8 orphan drop-ins | 2.2, 3.2 |
| § 6.9 empty states | 3.2 (rows 1 to 3), 4.1 (row 1 bar cases, row 4), 5.1 (row 5) |
| § 6.10 sketches | 3.2, 3.3, 4.1 (the rendered result is checked against them at gate A) |
| § 6.11 routes | 4.3 (Scheduled Tasks, "Go to owner"), 4.4 (`Also=`; its query cases 5.1), 5.2 (global search) |
| § 6.11 row keys, occurrence keys, item ids | 2.1 |
| § 6.11 reveal steps 1, 2, 3 and lookup | 5.1 (step 1), 4.3 (steps 2, 3, lookup) |
| § 6.12 section search | 2.3, 5.1 |
| § 6.12 global search, `searchText`, duplicates, reference section, aggregate | 2.3, 5.2 |
| § 6.12 true duplicates | 2.1, 2.2, 3.2, 5.1, 5.2, 6.1 |
| § 6.13 keyboard and accessibility | 3.1 (headers, grid), 3.2 (section region), 4.1 (filter, live region), 4.2 (focus after toggle), 6.1 (tab stop, movement, toggle keys), 8.3 (widths, themes) |
| § 6.14 sidebar label | 1.2 |
| § 6.14 frontend copies, heading, ids unchanged, sidebar count unchanged | 3.2 (heading, `MainContent` label), 5.2 (`GlobalSearch` keeps "Services"), 7.4 (owner badge); unchanged items checked at gates |
| § 6.15 single-host scoping | 5.2 (search), 7.3 (footer), 7.4 (owner badge, single-host by where it renders); `isAggregateMode` doc comment 5.2 |
| § 8 components new and changed | File structure; 2.1 to 7.4; the state hook 6.2 |
| § 8 untouched | Global constraints; gates A and B |
| § 9.1 to § 9.5 | 3.2, 5.1, 7.1, 7.2, 7.3 |
| § 10.1 fixtures | 1.3 (recorded, stock), 1.4 (planted; the provenance header in `load.ts`, Readings 13) |
| § 10.2 Rust assertion, groups re-record | 1.2 |
| § 10.2 e2e label specs | 8.1 |
| § 10.3 setup | 8.1 |
| § 11 CHANGELOG, docs | 9.1 |
| § 12 left to the builder | Readings 4, 5, 6, 7, 12; the rest stays the builder's |
| § 13 open questions | None |
| § 14 author's calls | Built as written, by the tasks of the sections each names |
| § 15 related | Reference; skills updated in 9.1 |

### Strings (§ 7)

| String | Task | String | Task |
|---|---|---|---|
| 1 sidebar | 1.2 | 14 | 4.1 |
| 1 heading | 3.2 | 15 | 2.2, 3.3 |
| 1 global search label | 5.2 | 16 | 3.3 |
| 1 accessible name | 3.2 (section `aria-label`) | 17 | 2.2, 3.3 |
| 2 to 5 | 2.2, 3.2 | 18 | 2.2, 3.3 |
| 6 | 3.2 | 19 to 22 | 2.3, 4.1 |
| 7, 8, 9 | 4.1 | 23 | 3.3 |
| 10 | 2.2, 3.1 | 24 | 7.1 |
| 11 | 3.2 | 25 | 7.3 |
| 12, 13 | 3.2 | 26 | 7.4 |

String 1's accessible-name use: the section's container is labelled "Systemd units" in Task 3.2 (today's `aria-label "Services"` on the flat grid moves to the section, since each group's grid is now labelled by its title).

### Tests

| Id | Task(s) | Id | Task(s) |
|---|---|---|---|
| U1 | 2.2 | U19 | 3.3 |
| U2 | 2.2 | U20 | 2.2, 3.3 |
| U3 | 2.2, 3.2 | U21 | 2.2, 3.2 |
| U4 | 2.2, 3.2 | U22 | 3.3 |
| U5 | 2.2, 3.2, 4.1 | U23 | 3.2, 4.1 |
| U6 | 4.1 | U24 | 2.3, 5.1 |
| U7 | 2.3, 4.1 | U25 | 2.1, 2.3, 4.3, 5.2 |
| U8 | 4.2, 5.1 | U26 | 4.3, 4.4, 5.1, 5.2 |
| U9 | 4.1 | U27 | 4.3 |
| U10 | 3.2, 3.3 | U28 | 7.1 |
| U11 | 3.3 | U29 | 7.2 |
| U12 | 3.3 | U30 | 7.3 |
| U13 | 3.3 | U31 | 3.2, 5.2, 7.4 |
| U14 | 3.3 | U32 | 3.1, 6.1 |
| U15 | 4.4 | U33 | 4.1, 5.1 |
| U16 | 4.4 | U34 | 5.2, 7.3 |
| U17 | 3.3 | U35 | 2.3, 3.2, 4.1, 5.1 |
| U18 | 3.3 | U36 | 6.1 |
| | | U37 | 3.2, 5.1 |
| P1, P2, P4, P5 | 8.1 | P3 | 8.2 |
| P6, P7, P8 | 8.3 | | |

### Split test ids

Every test id the table above gives more than one task, clause by clause as § 10.2 writes it. An id with one task owns all its clauses there.

| Id | Clause | Task |
|---|---|---|
| U3 | Groups, order, counts, no Paths; planted's Paths and slice, on the model | 2.2 |
| U3 | The same, rendered as headers | 3.2 |
| U4 | Sort by name; template and instance as siblings, on the model | 2.2 |
| U4 | Rendered: `getty@tty2.service` outside `service-group-getty@.service`, the same markup with and without the template | 3.2 |
| U5 | Flag numbers and text, on the model | 2.2 |
| U5 | Flags rendered under All | 3.2 |
| U5 | No flag under a filter other than All | 4.1 |
| U7 | What each value shows, on the model | 2.3 |
| U7 | Rendered per value; groups with none not drawn | 4.1 |
| U8 | (a) toggle under "Needs follow-up" keeps the row, focus and updates counts, the group count too | 4.2 |
| U8 | (b) planted `[Install]` drop-in pair | 4.2 |
| U8 | (c) query matching only through `declined.text` | 5.1 |
| U8 | (d) an Undo follows the filter | 4.2 |
| U8 | (e) a filter change drops pins; the bar ceasing to be drawn does not | 4.2 |
| U8 | (e) a query change drops pins; a reveal that changes the query, or the filter, drops pins | 5.1 |
| U10 | Explanation on included, excluded and declined targets; the declined target's `declined.text` | 3.2 |
| U10 | The declined target's outcome label | 3.3 |
| U20 | Order within a block; relation and cause labels for every value, on the model | 2.2 |
| U20 | Rendered: adjacency, relation phrase, cause label, undetermined row | 3.3 |
| U21 | Orphan placement, on the model | 2.2 |
| U21 | Rendered under "Other drop-in overrides" in Services and Other units | 3.2 |
| U23 | `stock10` string 12; empty view string 13, never the old sentence | 3.2 |
| U23 | `stock10` draws no bar (with U6); excluded and declined target bar cases; a filter left empty shows string 14 and "Show all" | 4.1 |
| U24 | Matching on `declined.text`, drop-in path and advisory text, and the search text of every row kind equal to global search's, on the model | 2.3 |
| U24 | `/` opens search; relation phrase, reason words and mount point found; result count; no-match `EmptyState`; ArrowDown; search and filter combine | 5.1 |
| U25 | Row keys and item ids, prefixed name on the model | 2.1 |
| U25 | Search text per record, shared name and three links on the model | 2.3 |
| U25 | "Go to owner" focuses the drop-in; bare-name navigation to the shared name and to `undetermined-install-demo.service` | 4.3 |
| U25 | Every result kind indexed by row key and reaching its row; reference section skipped; omitted unit found by its reason; shared name, prefixed name, three links and true duplicates as results | 5.2 |
| U26 | Global search case, reveal half, by the row key a result sends | 4.3 |
| U26 | `Also=` link: hidden by the filter; in a collapsed group | 4.4 |
| U26 | `Also=` link: failing the query (cleared, box open); a matching query kept | 5.1 |
| U26 | Global search case: the result selected from the box | 5.2 |
| U31 | Heading "Systemd units", no shadow count | 3.2 |
| U31 | Single-host results labelled "Systemd units" | 5.2 |
| U31 | Owner badge "Controlled in Systemd units" on a shadowed config row | 7.4 |
| U32 | Structure: `aria-controls`, `aria-labelledby`, rowgroup; headers toggle on Enter and Space | 3.1 |
| U32 | Movement, `g`/`G`, Space and `x`; roving tab stop after collapse, filter and query | 6.1 |
| U33 | Region present and empty; strings 19 to 22 per value for the rows shown, singular and plural; "Showing all 24 rows." beside "All 23" | 4.1 |
| U33 | The count under a section query; a reveal that sets "All" | 5.1 |
| U34 | Fleet search result labelled "Services", navigates, no "Systemd units" | 5.2 |
| U34 | Fleet footer keeps today's text | 7.3 |
| U35 | (b) on the model | 2.3 |
| U35 | (a) rows, nesting under the first occurrence, Reported header count | 3.2 |
| U35 | (a) All and Needs follow-up count both link records; All counts both unit records | 4.1 |
| U35 | (b) rendered, with Reported and result counts; (c) | 5.1 |
| U37 | (a) orphan-only view: no badge, name "Services" | 3.2 |
| U37 | (b) a query leaving only the orphan: no badge, name "Services, 5 need follow-up", badge back when cleared | 5.1 |

**Not mapped to a task:** nothing in the spec. § 4 and § 5 describe today and the options considered; § 13 is empty; § 9.4's note on `kernel_boot` and `selinux` is out of scope by the spec's own words.
