# Carried-file ownership: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task by task. Steps use checkbox (`- [ ]`) syntax for tracking. Every code task is test-first: write the failing test the task names, run it and see it fail for the stated reason, then implement.

**Goal:** carried `/etc` files, carried unmanaged files and directories, and the `/var` directories the export provisions keep the host's owner, group and mode in the image, never wider than the host, with every account they need created or proved, and every loss named before the first build. Ahead of it, local users move to one section-level delivery switch.

**Spec:** `process-docs/specs/proposed/2026-11-25-carried-file-ownership.md`, approved 2026-11-25. The spec is the contract. Section numbers (§), rule numbers (1 to 12, § 2), decision numbers (13.1 to 13.10), test ids (T1 to T38, T17a, T17b) and real-host check ids (R1 to R19) are the spec's. The symbol name is the identifier; a line number is a convenience and was read at `5157432c`.

**Architecture:** the scan records host facts in one new snapshot section, `ownership` (§ 3.2), keyed by image path, filled by one `OwnershipRecorder` and one `HostAccounts` index that every inspector shares through `InspectionContext`. Render builds one `OwnershipPlan` (`crates/pipeline/src/render/ownership.rs`) from the render's include set; every ownership line, account line, tmpfiles `d` line owner, notice and report row comes from it. Mode reaches the image through the staged tree; owner, group and special bits through one `RUN` after the users block; accounts through a block between `FROM` and the packages.

**Tech stack:** Rust (serde, insta), the existing mock executor, TypeScript and React for the users section control and row detail (Vitest, Playwright). No new crate dependency.

---

## Sequencing (spec § 14)

| Unit | Tasks | Branch | Lands |
|---|---|---|---|
| Landing 1, § 0 | 1 | `feat/local-user-delivery` | on its own, before anything else |
| Gate | 2 | `test/r11-fixed-id-fixture` (fixture and render test only) | fixture lands on its own; the real-host run decides whether Landing 2 starts |
| Landing 2, schema and scan | 3 to 10 | `feat/ownership-scan` | after Task 10 |
| Landing 3, render and both exports | 11 to 22 | `feat/ownership-render` | after Task 22 |

Each landing compiles, passes the workspace suite, and stands alone: nothing in it depends on a later one. After Landing 2 a render of the same host produces the same Containerfile, tarball and reports as before it, except the `/var` parents the dedup now keeps.

**If R11 fails** (Task 2), stop: Landing 2 does not start, and decision 13.5 goes back to the project owner.

## Worktrees and landing

- Every code task runs in its own git worktree, created by the orchestration repo's `workflow/inspectah-worktree create <branch>`, one branch per landing as in the table above. The orchestrator lands it (`inspectah-worktree land <branch>`) after the review seat approves, then removes it (`cleanup`). Implementers commit, never push, never land. One code dispatch at a time.
- **`land` runs at four points:** after Task 1 (`feat/local-user-delivery`), after Task 2's fixture commit (`test/r11-fixed-id-fixture`), after Task 10 (`feat/ownership-scan`), and after Task 22 (`feat/ownership-render`). Nothing lands mid-landing.
- Before each Cargo command or `git commit`, unset `CARGO_TARGET_DIR` in the same shell. Builds use the worktree's own `target/`.
- **Verification convention.** Iterate with crate-scoped tests: `cargo test -p <crate> --lib <filter>` for unit tests, `cargo test -p <crate> --test integration <filter>` for integration tests (every crate's integration tests are modules of one `integration` binary declared in `tests/main.rs`; a new test file is declared there or `test_files_declared` fails). The pre-commit hook runs `cargo fmt --check`, clippy (`--workspace --all-targets -- -D clippy::all`) and the workspace suite on every non-docs commit, and refuses while a non-docs file is unstaged or untracked. Report the hook's per-gate lines as the gate result. Never `--no-verify`.
- **Frontend.** The hook gives a frontend-only change no gate, so a task that edits `crates/web/ui/src` runs `npx tsc --noEmit` and `npx vitest run` in `crates/web/ui` by hand before its commit, never at the same time as a cargo command (`process-docs/skills/full-workspace-verification-gates.md`). Playwright: `npx playwright test e2e/users.spec.ts` and the specs the task names; report `skipped` apart from `passed` (`process-docs/skills/e2e-fixtures-are-hand-maintained.md`).
- **Commits:** `type(scope): description`, imperative; body says why; trailer `Assisted-by: <tool> (<model>)`; never a `Co-Authored-By:` trailer. Stage by explicit path (`git add <paths>`, `git commit -F <file>`). Text in commits, code comments and docs names no person or reviewer.
- **Voice** in CHANGELOG and user docs: no em dashes, not "shape", not "immutable".
- **No compatibility code.** inspectah has no backwards-compatibility promise before 1.0. Each schema bump gets one `**Breaking:**` changelog entry that says what changed and to re-scan; no migration notes, no upgrade code, no load shims.

## Lanes

- **Rust lane** (default): core, collect, pipeline, refine, cli, and the web crate's Rust tests.
- **Kit lane** (web frontend, report templates, Axum handlers): Task 1 step group B (the users section switch and its warning, removing the card's checkbox and strategy radios, `setUserStrategy`, the handler and route swap, e2e fixtures); Task 10's two web adapters (`crates/web/src/adapter.rs`, `crates/web/src/aggregate_handlers.rs`, no API change); Task 17's HTML report rows; Task 18 (row detail and notice rows in the refine UI).

## Real-host tasks

| Task | Hosts | What runs |
|---|---|---|
| 2 (R11, gate) | rhel10, and rhel9 if both install routes are available there | local user `appuser` uid 1001 gid 1002, group `appgrp` gid 1050 on the source host; scan, export, build the image; install once through the image builder's blueprint and once through a kickstart; `getent` checks. Repeat with the switch at `Image` |
| 20 (builds) | rhel9 and rhel10 | driftify-prepared fixtures for each case, `inspectah scan`, `podman build` of the generated Containerfile on each release's `rhel-bootc` base, `stat` and `runuser` inside the built image: R1 to R4, R5 (build half), R6, R7, R9, R10, R12, R18 |
| 21 (booted) | rhel9 and rhel10 bootc hosts | install Task 20's images and check the merge, `systemd-sysusers` at boot and the composefs mount, across reboots and `bootc switch`/upgrade: R5 (booted half), R8, R13 to R17, R19 |

Every real-host task follows `process-docs/skills/real-host-validation-runs.md`: revert each VM to its clean snapshot before use, apply drift with driftify rather than by hand, and record every command and its output in the task's report. Q9 (tmpfiles before SSSD answers) is not checked on a host: the test VMs have no directory service (spec § 12).

## Backlog items

Named by slug (spec Metadata). Updated outside this repository; the Landing 3 report says which this work closes.

- `inspectah-carried-files-lose-owner-and-group`: closed by Landing 3.
- `inspectah-tmpfiles-line-owned-by-a-skipped-user-is-silently-skipped`: closed by Landing 3 (Tasks 12, 17).
- `inspectah-var-parent-dirs-lose-their-ownership`: closed by Tasks 9 and 14.
- `inspectah-storage-ownership-rows-report-var-directories-the-image-already-reproduces`: stays open (§ 5.6).

## Review focus

Inputs the spec implies that are easy to miss, most likely first. Each has a test in the owning task.

1. **Every commit compiles alone.** Landing 2 adds new records beside the old fields (Tasks 3 to 9) and removes the old fields only in Task 10, with every reader, in one commit.
2. **A host-byte write with no record** fails closed (`Unknown`, "not recorded"), never `0644 root:root`. Tasks 11 and 15.
3. **Owner and group resolve independently.** A test with each mixed pair (T28) and guards only for the numeric subject.
4. **No path is acted through a link.** `chown -h` only; the link refusal covers every `chmod` operand. Tasks 6 and 13.
5. **Host text reaches every report surface escaped:** `Unnamed.host_name`, `SysusersFile::Refused.host_name`, paths, ACL and capability lists (§ 5.8). Only counts reach the Containerfile. Task 17.

---

## File structure

| File | Responsibility |
|---|---|
| `crates/core/src/types/users.rs` | `LocalUserDelivery`; `UserGroupSection.local_user_delivery`; `UserContainerfileStrategy` and the per-user field removed (Task 1) |
| `crates/core/src/types/ownership.rs` (new) | `Ownership`, `AccountRef`, `Mode`, `OwnershipSection`, `PathRecord`, `ModeClamp`, `ClampReason`, `UserLookup`, `GroupLookup`, `HostUser`, `HostGroup`, `AccountSource`, `SysusersDeclaration`, `SysusersFile`, `SysusersFileName`, `SysusersId`, `Detection`, `DetectionGap` (Task 3) |
| `crates/core/src/types/account_name.rs` (new) | `AccountName` and `ExportToken`, moved from `crates/pipeline/src/render/safety.rs` with serde on `AccountName`; the pipeline re-exports both (Task 3) |
| `crates/core/src/types/destination.rs` | `Destination` string serde (Task 3) |
| `crates/core/src/snapshot.rs` | `ownership: Option<OwnershipSection>`; `SCHEMA_VERSION` and `MIN_SCHEMA` 31 to 32 (Task 1) and 32 to 33 (Task 3) |
| `crates/core/src/defaults.rs` (new) | The scan's ownership default, the `/etc` world-writable clamp (Task 6) |
| `crates/core/src/traits/executor.rs` | `FileMetadata.uid`, `.gid`; `Executor::symlink_metadata` (Task 4) |
| `crates/core/src/traits/inspector.rs` | `InspectionContext` gains `accounts` and `ownership` (Tasks 5, 6) |
| `crates/core/src/accounts.rs` (new) | `HostAccounts`: names, source, sysusers declarations, cached id lookups (Task 5) |
| `crates/core/src/ownership_recorder.rs` (new) | `OwnershipRecorder`: first recording stands, ancestor walk (Task 6) |
| `crates/collect/src/executor/{real,mock}.rs`, `crates/core/src/machine_state.rs` (`ScreenedExecutor`) | `symlink_metadata`, `uid`, `gid` (Task 4) |
| `crates/collect/src/accounts.rs` (new) | Builds `HostAccounts` from `getent` and the account files (Task 5) |
| `crates/collect/src/ownership_detect.rs` (new) | `getfacl` and `getcap` per carried root, with gaps (Task 8) |
| `crates/collect/src/inspectors/{config/collector.rs,rpm*,network*,units*,nonrpm.rs}` | Record each host path (Task 6) |
| `crates/collect/src/inspectors/storage.rs` | `/var` `Ownership` mapping and the dedup (Task 9) |
| `crates/pipeline/src/collect.rs` | Build the index and recorder before wave 1; ancestor walk, detection and the account pass after the waves (Tasks 5 to 8) |
| `crates/pipeline/src/render/ownership.rs` (new) | `OwnershipPlan`, the resolver, subjects, fallbacks, special bits, notice rows (Task 11) |
| `crates/pipeline/src/render/users.rs` | § 0 render; `ProvisioningDelivery` consumed by `render_kickstart` and `render_blueprint_toml` (Tasks 1, 11) |
| `crates/pipeline/src/render/containerfile.rs` | `from_lines`, accounts block, sysusers block, ownership block, `/var` lines; `sssd_mode_lines`, `mkdir_line` and the numeric cases of `resolve_tmpfiles_ownership` removed (Tasks 12 to 14) |
| `crates/pipeline/src/render/{tarball.rs,mod.rs,configtree.rs}`, `crates/pipeline/src/build/extract.rs` | Staged modes, `u+r` (Task 15) |
| `crates/refine/src/session.rs` (`render_refine_export`), `crates/refine/src/projection/{types.rs,reference.rs}` | § 0 op; export staging, projection detail, export warnings (Tasks 1, 16) |
| `crates/pipeline/src/render/{unmanaged_owners.rs,tmpfiles_accounts.rs,audit.rs,report.rs,report_data.rs}`, `crates/pipeline/src/export_warning.rs`, `crates/cli/src/commands/scan.rs` | The ownership notice on every surface (Task 17) |
| `crates/web/src/{handlers.rs,lib.rs,adapter.rs,aggregate_handlers.rs,web_types.rs}` | § 0 route; adapters fill unchanged API types (Tasks 1, 10, 18) |
| `crates/web/ui/src/components/{UsersGroupsSection.tsx,UserCard.tsx}`, `crates/web/ui/src/api/{client.ts,types.ts}` | § 0 control and warning (Task 1); row detail (Task 18) |
| `testdata/fixtures/users-block/r11-fixed-ids/` (new) | R11's fixture (Task 2) |
| `testdata/fixtures/ownership/` (new) | Collector-produced fixtures for T1 to T38 (Tasks 3 to 19) |

---

## Landing 1: § 0, one delivery switch for local users

### Task 1: `LocalUserDelivery` (Rust lane, then Kit lane; M)

**Spec:** § 0 whole; decision 13.4; § 9 "§ 0 change"; T36.

**Goal:** every local user goes into the kickstart and blueprint on a single host; one section switch, default `provisioning`, adds the users block to the Containerfile for every user that validates; the per-user strategy and single-host include are gone. Schema 32.

**Files (step group A, Rust lane):**
- Modify: `crates/core/src/types/users.rs` (add `LocalUserDelivery`; `UserGroupSection.local_user_delivery` with `#[serde(default)]`; remove `UserContainerfileStrategy` and `containerfile_strategy` on `UserGroupDecision`)
- Modify: `crates/core/src/snapshot.rs` (`SCHEMA_VERSION` and `MIN_SCHEMA` to 32)
- Modify: `crates/collect/src/inspectors/users.rs` (write `provisioning`; stop writing a per-user strategy)
- Modify: `crates/pipeline/src/render/users.rs` (`included_users` honours `include` only when `aggregate_meta` is set; `ContainerfileUserPlan::from_snapshot` reads the section switch, not the per-user branch; block comment "for testing, not recommended for production"; collision message "deliver users at provisioning"; the `preserve` hash notice under `Image`)
- Modify: `crates/pipeline/src/render/{containerfile.rs,audit.rs,report.rs,report_data.rs}` (users block only under `Image`; audit and report read the section value)
- Modify: `crates/refine/src/types.rs` (`RefinementOp::SetLocalUserDelivery { delivery }` in place of `UserStrategy`), `crates/refine/src/session.rs` (validation and apply), `crates/refine/src/projection/types.rs` (`DecisionProjection` carries the section's value)
- Modify: `crates/web/src/handlers.rs`, `crates/web/src/lib.rs` (handler and route for the new op replace `UserStrategyRequest` and its route)
- Tests: `crates/pipeline/tests/users_render_test.rs`, `crates/pipeline/tests/users_build_test.rs`, `crates/refine/tests/users_integration_test.rs`, `crates/refine/tests/export_contract_test.rs`, `crates/collect/tests/users_test.rs`, `crates/cli/tests/{users_password_export_test.rs,tmpfiles_owner_notice_test.rs,copy_source_exists_test.rs}`, `crates/pipeline/tests/{smoke_render_2b.rs,var_backing_export_test.rs}`

**Files (step group B, Kit lane):**
- Modify: `crates/web/ui/src/components/UsersGroupsSection.tsx` (one control, `Provisioning` / `Image`, the "not recommended for production" warning beside `Image`)
- Modify: `crates/web/ui/src/components/UserCard.tsx` (remove the header checkbox and the strategy fieldset)
- Modify: `crates/web/ui/src/api/client.ts` (`setLocalUserDelivery` replaces `setUserStrategy`), `crates/web/ui/src/api/types.ts`
- Modify: tests `crates/web/ui/src/components/__tests__/{PendingGuards.test.tsx,MainContent.user-failures.test.tsx}`, `crates/web/ui/e2e/users.spec.ts`
- Re-record: `crates/web/ui/e2e/fixtures/post-responses/user-strategy/` (renamed for the new op), the single-host and sequence bodies that hold `containerfile_strategy`; register in `crates/web/ui/e2e/fixtures/manifest.json` and `crates/web/tests/fixture_structure_test.rs`

Step group A:

- [ ] **Step 1: Failing test first (T36).** In `crates/pipeline/tests/users_render_test.rs`, a table-driven `t36_local_user_delivery` over the users-block fixture with two users, one carrying `include: false`, run at `Provisioning` and at `Image`: the kickstart and blueprint hold both users under both values; the Containerfile users block exists only under `Image`, its comment holds "not recommended for production"; with `Image` and one user's password choice `preserve`, the audit report holds the hash notice once. A second case with `aggregate_meta` set: `include: false` still drops the user from all three.
- [ ] **Step 2:** `cargo test -p inspectah-pipeline --test integration t36` fails to compile on `LocalUserDelivery`.
- [ ] **Step 3:** add the enum and field, bump the schema to 32 (raise `MIN_SCHEMA` with it, per `process-docs/skills/snapshot-schema-versioning.md`), remove the per-user strategy and its readers, render as the test states. Refine: replace the op, its validation, apply and undo; `DecisionProjection` carries `local_user_delivery`. Web: handler and route for the new op. Update the collision message string.
- [ ] **Step 4:** in `crates/refine/tests/users_integration_test.rs`, `set_local_user_delivery_round_trips`: apply `SetLocalUserDelivery { delivery: Image }`, export, see the users block; undo, see it gone.
- [ ] **Step 5:** fixture churn: every test that set `containerfile_strategy` sets the section field instead; each changed assertion is named in the commit body.
- [ ] **Step 6: Verify.** `cargo test -p inspectah-core`, `-p inspectah-collect --test integration users`, `-p inspectah-pipeline --test integration users`, `-p inspectah-refine --test integration users`, `-p inspectah-web`, `-p inspectah-cli --test integration users`.
- [ ] **Step 7: Changelog** under `## [Unreleased]`: `**Breaking:**` snapshot schema 32, re-scan earlier snapshots; Changed: local users are delivered by the kickstart and blueprint, every one; one switch adds them to the Containerfile for testing, not for production; the per-user Containerfile strategy and the per-user include on a single host are gone.
- [ ] **Step 8: Skills.** `process-docs/skills/bootc-base-groups-and-users.md` and `process-docs/skills/index.md` row: the users block runs only under `Image`. `process-docs/skills/snapshot-schema-versioning.md`: record the 31 to 32 bump and why it rejects 31.
- [ ] **Step 9: Commit** `feat(users): deliver local users through one section switch`.

Step group B:

- [ ] **Step 10: Failing test first.** Vitest in `UsersGroupsSection`: the switch renders with `Provisioning` selected, choosing `Image` calls `setLocalUserDelivery("image")` and shows the warning; `UserCard` has no checkbox labelled "Include <name>" and no strategy fieldset.
- [ ] **Step 11:** implement; remove `setUserStrategy`; re-record the e2e bodies and register them; update `e2e/users.spec.ts`.
- [ ] **Step 12: Verify.** In `crates/web/ui`: `npx tsc --noEmit`, `npx vitest run`, `npx playwright test e2e/users.spec.ts`. Then `cargo test -p inspectah-web --test integration fixture_structure`.
- [ ] **Step 13: Commit** `feat(web): show the local user delivery switch on the users section`.

**Land:** `inspectah-worktree land feat/local-user-delivery`.

---

## Gate: R11

### Task 2: R11 on its own fixed-id fixture (Rust lane for the fixture; real host; S + host time)

**Spec:** § 8 "R11 is a gate", § 10.2 R11, § 12 Q8, decision 13.5, § 14 Gate.

**Goal:** prove, before any ownership code, that the image builder's blueprint and Anaconda's kickstart create a local user at the fixed uid and its primary group at the fixed gid, and tolerate a user the image already holds.

**Files:**
- Create: `testdata/fixtures/users-block/r11-fixed-ids/passwd`, `testdata/fixtures/users-block/r11-fixed-ids/group` (`appuser` uid 1001 primary gid 1002; group `appuser` 1002; custom group `appgrp` 1050, with `appuser` a member)
- Modify: `crates/pipeline/tests/users_render_test.rs`

- [ ] **Step 1: Failing test first.** `r11_fixture_pins_uid_and_gid`: collect the fixture through the users inspector and the mock executor; the kickstart holds `group --name=appgrp --gid=1050` and a `user --name=appuser` line with `--uid=1001 --gid=1002` (no `group` line for `appuser`: the `user --gid` line is what R11 checks creates it, § 5.1); the blueprint holds the matching `[[customizations.group]]` entry for `appgrp` and a `[[customizations.user]]` entry with `uid = 1001`, `gid = 1002`; at `Image` the users block creates `appuser` at 1001:1002. It fails until the fixture files exist.
- [ ] **Step 2:** add the fixture. If the test still fails because a renderer does not write a fixed id, stop: that is the gate's finding, reported, not fixed here.
- [ ] **Step 3: Verify.** `cargo test -p inspectah-pipeline --test integration r11`.
- [ ] **Step 4: Commit** `test(users): add the fixed-id fixture the provisioning gate runs on`. **Land:** `inspectah-worktree land test/r11-fixed-id-fixture`.
- [ ] **Step 5: Real host, rhel10** (and rhel9 where both routes are available). Revert the VM. Create `appuser` (uid 1001, primary group `appuser` gid 1002) and `appgrp` (gid 1050). Run `inspectah scan` from the landed `main`, take the export, `podman build` its Containerfile on `rhel-bootc`. Install the image twice: once through the image builder with the export's blueprint as its config, once through a kickstart install with the export's kickstart (`ostreecontainer` pointing at the built image). On each installed host: `getent passwd appuser` shows uid 1001 gid 1002; `getent group appuser` shows 1002; `getent group appgrp` shows 1050.
- [ ] **Step 6:** set the switch to `Image` (refine, `SetLocalUserDelivery`), rebuild, install through both routes again: install completes, and `getent passwd appuser` and `getent group appuser` are unchanged from the image's.
- [ ] **Step 7: Report** each route, release and command with its output. **Pass:** every check above holds on at least one release through both routes. **Fail:** stop here; nothing from Landing 2 starts, and decision 13.5 goes back to the project owner.

---

## Landing 2: schema and scan

Tasks 3 to 9 add the new records beside the old fields; Task 10 removes the old fields and adapts every reader in one commit, so every commit compiles and passes alone. No render output changes in this landing except Task 9's kept `/var` parents.

### Task 3: Core types (Rust lane; M)

**Spec:** § 3.1, § 3.2 (types and the record table's `InspectionSnapshot` row), § 3.3, rule 2; T13 (load half), T35 (hand-built half).

**Files:**
- Create: `crates/core/src/types/ownership.rs`, `crates/core/src/types/account_name.rs`
- Modify: `crates/core/src/types/mod.rs`, `crates/core/src/types/destination.rs`, `crates/core/src/snapshot.rs` (field, schema 33)
- Modify: `crates/pipeline/src/render/safety.rs` (re-export `AccountName` and `ExportToken` from core; parse rules unchanged)
- Test: `crates/core/tests/ownership_serde_test.rs` (new, declared in `crates/core/tests/main.rs`)

- [ ] **Step 1: Failing tests first.** `t13_load_errors` (table): `Mode` `"10644"`, `"644"`, `"0o644"`, `""`; an `AccountName` with a space and one with a leading `-`; a `PathRecord` with an unknown `kind`; `SysusersFileName` `"x.conf;curl x|sh;#.conf"`, `".conf"`, `"-x.conf"`, `"named"`; each is a load error. `t35_round_trips` (hand-built values here; Task 19 repeats it over collector-produced snapshots): `Recorded` and `Unknown`; `Named`, `Unnamed` with `host_name` absent and present; `Mode` `0000` and `7777`; all three `PathRecord` kinds; `clamp`; `Found` and `NotFound`; both `SysusersFile` and both `SysusersId` kinds; `Detection` with and without gaps, with and without `host`; `Destination` and `AccountName` as map keys. Plus an insta snapshot of § 3.3's JSON example, parsed and re-serialized.
- [ ] **Step 2:** `cargo test -p inspectah-core --test integration ownership_serde` fails to compile.
- [ ] **Step 3:** implement the types exactly as § 3.2; `Mode` serializes as four octal digits and loads only `[0-7]{4}`; move `AccountName` and `ExportToken` with `try_from`/`into` on `AccountName` only; `Destination` gains the string pair, normal form absolute with no trailing `/`; `InspectionSnapshot.ownership: Option<OwnershipSection>`, `#[serde(default)]`. Raise `SCHEMA_VERSION` and `MIN_SCHEMA` to 33.
- [ ] **Step 4: Verify.** `cargo test -p inspectah-core`, `cargo test -p inspectah-pipeline --lib safety`.
- [ ] **Step 5: Changelog** `**Breaking:**` snapshot schema 33, a snapshot from an earlier version must be re-scanned. **Skills:** `snapshot-schema-versioning.md` records 32 to 33; `codebase-layout.md` lists `types/ownership.rs` and `types/account_name.rs`.
- [ ] **Step 6: Commit** `feat(core): add the ownership snapshot section`.

### Task 4: Executor metadata (Rust lane; S)

**Spec:** § 4.1 first paragraph.

**Files:** `crates/core/src/traits/executor.rs`, `crates/collect/src/executor/real.rs`, `crates/collect/src/executor/mock.rs`, `crates/core/src/machine_state.rs` (`ScreenedExecutor`), and the test executors that implement the trait: `crates/collect/tests/screen_test.rs` (`RootedHost`), `crates/cli/src/commands/scan.rs` (`ScanTestExecutor`).

- [ ] **Step 1: Failing tests first.** Real executor, in a temp dir: `symlink_metadata` on a link reports the link, `file_metadata` reports the target; both report `uid` and `gid` equal to the process's. Mock: a planted file with uid 985 gid 25 reports them through both calls; a planted link reports a link through `symlink_metadata`. Screened: `symlink_metadata` passes through the gate as `file_metadata` does.
- [ ] **Step 2:** `cargo test -p inspectah-collect --lib executor` fails to compile.
- [ ] **Step 3:** implement; `file_metadata` keeps following links for its existing callers.
- [ ] **Step 4: Verify.** `cargo test -p inspectah-collect --lib executor`, `cargo test -p inspectah-core --lib machine_state`.
- [ ] **Step 5: Commit** `feat(collect): read owner ids and link metadata through the executor`.

### Task 5: `HostAccounts` (Rust lane; M)

**Spec:** § 4.4 (Names, Source, Package declarations), § 3.1 last paragraph (scan-time classification), trust assumption on directory names (§ 2.1); T14 (scan half), T13 (scan half).

**Files:** create `crates/core/src/accounts.rs`, `crates/collect/src/accounts.rs`; modify `crates/core/src/traits/inspector.rs` (`InspectionContext.accounts: &'a HostAccounts`), `crates/pipeline/src/collect.rs` (build once before wave 1), every test that builds an `InspectionContext` (give it `HostAccounts::empty()`). Fixtures: `testdata/fixtures/ownership/accounts/`.

- [ ] **Step 1: Failing tests first** (table, mock executor): `getent passwd` and `getent group` enumerations; an id enumeration did not list answered by `getent passwd <id>` once and cached (the mock counts calls); an id with no answer is `Unnamed { host_name: None }`; `jdoe@ad.example.com` is `Unnamed { host_name: Some(..) }` (T14); two names on one id, the `getent <id>` answer stands; source `LocalFiles` from `/etc/passwd`, `/etc/group`, `/usr/lib/passwd`, `/usr/lib/group`, else `Directory`; sysusers: `u appsvc -` declares user `appsvc` `Unpinned` and implied group `appsvc` `Unpinned`; `u appwork -:appgrp` declares only the user; `g named 25` is `Pinned { id: 25 }`; `u x 985:appgrp` is `Pinned { id: 985 }` for the user only; a file named `x;y.conf` gives `SysusersFile::Refused` (T13 scan half).
- [ ] **Step 2:** `cargo test -p inspectah-collect --lib accounts` fails to compile.
- [ ] **Step 3:** implement. Every `getent` call passes an argument vector. `HostAccounts` keeps the executor for cached by-id and by-name lookups behind a `Mutex`.
- [ ] **Step 4: Verify.** `cargo test -p inspectah-collect --lib accounts`, `cargo test -p inspectah-pipeline --lib collect`.
- [ ] **Step 5: Skills** `codebase-layout.md`: the two `accounts.rs` files. **Commit** `feat(collect): index host accounts once per scan`.

### Task 6: `OwnershipRecorder` and the clamp default (Rust lane; L)

**Spec:** § 4.1 (which call per record), § 4.2 (collectors, read paths, ancestors), § 4.3, § 3.2 (directory recording rule, `WithheldConfigEntry` row), decision 13.2 (scan half), rules 1 and 7 (scan half); T4 (scan half), T8 (snapshot half), T20, T21, T22, T23.

**Files:** create `crates/core/src/ownership_recorder.rs`, `crates/core/src/defaults.rs`; modify `crates/core/src/traits/inspector.rs` (`InspectionContext.ownership: &'a OwnershipRecorder`), `crates/collect/src/inspectors/config/collector.rs` (every `ConfigFileEntry`, any disposition, read path `c.read`; withheld entries not recorded), the rpm inspector (`repo_files`, `gpg_keys`), the network inspector (`firewall_zones`), the units inspector (`drop_ins`), `crates/collect/src/inspectors/nonrpm.rs` (`UnmanagedFile` items and each directory `walk_for_unmanaged` enters, via `symlink_metadata`), `crates/pipeline/src/collect.rs` (ancestor walk after the waves; move the section into the snapshot). Fixtures: `testdata/fixtures/ownership/recorder/`.

- [ ] **Step 1: Failing tests first** (collector-produced, mock executor), in `crates/collect/tests/ownership_recorder_test.rs` (new, declared in `main.rs`):
  - `t4_stat_failure_is_unknown`: a file and a directory whose metadata call fails record `Unknown` with the error.
  - `t8_world_writable_clamp`: `/etc/foo.conf 0666`, `/etc/drop 0777` dir, `/etc/spool 1777` dir get `clamp` `0664`, `0775`, `1775` with `host.mode` unchanged; `/opt/foo 0666` has no clamp.
  - `t20_unmanaged_symlinks`: links owned by `appsvc` to `/etc/crontab`, `../lib/x` and a missing target record `PathRecord::Symlink` and nothing else.
  - `t21_directory_link_above_content`: a config file read through an `/etc` directory link whose target is `0750 root:named` records a `Directory` at the link's path with the target's ownership.
  - `t22_repo_file_without_twin`: an RPM-owned custom repository file `0640` with no config twin is recorded under its path.
  - `t23_shadowed_records`: a shadowed drop-in and a config entry shadowed by a repository, both `0640`, are recorded; a repo file and its config twin record once, first recording stands.
  - `ancestors_record_departures_only`: an ancestor `0755 root:root` is not recorded; `0750` is; carried roots are never recorded; a withheld entry is not recorded.
- [ ] **Step 2:** `cargo test -p inspectah-collect --test integration ownership_recorder` fails.
- [ ] **Step 3:** implement the recorder (`Mutex<BTreeMap<Destination, PathRecord>>`, first recording stands), its `record_content` (follows links) and `record_unmanaged` (`lstat`) entry points, the ancestor walk (`file_metadata` above a content write, `symlink_metadata` otherwise; the first rule wins where both apply), and the clamp default in `core::defaults`, called by the recorder. Generated content (cron-converted units, the flatpak service, the kargs file) is not recorded.
- [ ] **Step 4: Verify.** `cargo test -p inspectah-collect --test integration ownership_recorder`, `cargo test -p inspectah-pipeline --lib collect`.
- [ ] **Step 5: Skills** `codebase-layout.md`: recorder and defaults module. **Commit** `feat(collect): record host ownership for every carried path`.

### Task 7: Account dependency pass (Rust lane; M)

**Spec:** § 4.4 the `finalize_cross_inspector` pass (steps 1 to 3) and its last paragraph; § 5.7 (scan half); T24 (scan half), T25 (scan half).

**Files:** `crates/pipeline/src/collect.rs` (`finalize_cross_inspector` gains `accounts: &HostAccounts`; a new pass `fill_ownership_accounts`, its own function, called unconditionally, per the dispatcher's doc comment; update the seven test callers), `crates/collect/src/tmpfiles_fields.rs` (new, or the existing tmpfiles parser if one is in reach: the user and group fields of a fragment's lines).

- [ ] **Step 1: Failing tests first** in `crates/pipeline/src/collect.rs` tests: `t24_tmpfiles_content_names_accounts`: a root-owned fragment under `/etc/tmpfiles.d/` whose only reference to `appsvc:appgrp` is a line in its content puts both in `users` and `groups` as `Found`; the same under `/usr/local/lib/tmpfiles.d/` from an unmanaged file; a numeric field adds nothing; a field `AccountName` refuses is not recorded. `t25_lookup_gaps`: a system user whose primary gid has no name keeps `primary_group: Unnamed`; a tmpfiles name NSS does not know is `NotFound`. `owners_and_groups_of_records`: every `Named` owner and group of a recorded `File` or `Directory` is in the maps; each `Found` user's primary group is in `groups`.
- [ ] **Step 2:** `cargo test -p inspectah-pipeline --lib collect` fails.
- [ ] **Step 3:** implement.
- [ ] **Step 4: Verify** as Step 2. **Commit** `feat(collect): record the accounts carried paths and tmpfiles lines name`.

### Task 8: ACL and capability detection (Rust lane; S to M)

**Spec:** § 4.6, decision 13.7, § 6 item 4 (gap merge with `host`); T31.

**Files:** create `crates/collect/src/ownership_detect.rs`; modify `crates/pipeline/src/collect.rs` (run once per scan after the waves), `crates/core/src/aggregate/merge.rs` (union of `paths`, concatenation of `gaps` with each gap's `host` set from `aggregate_meta`).

- [ ] **Step 1: Failing tests first.** `t31_detection` (table, mock executor): `getfacl` absent gives three gaps (`/etc`, `/opt`, `/usr/local`) and no paths; `getcap` finding a path under `/etc` and failing on `/opt` keeps the `/etc` path with one `/opt` gap. Commands are `getfacl -R --skip-base --absolute-names <root>` and `getcap -r <root>`, one per root, paths only. Aggregate (in `crates/core/src/aggregate/merge.rs` tests): a host with ACL findings and a host where `getfacl` is absent keep the first host's paths and the second host's gaps with `host` set.
- [ ] **Step 2:** `cargo test -p inspectah-collect --lib ownership_detect` and `cargo test -p inspectah-core --lib aggregate` fail.
- [ ] **Step 3:** implement. The aggregate merge carries `acl` and `capabilities` only; `paths`, `users` and `groups` are the convergence spec's (§ 6).
- [ ] **Step 4: Verify** as Step 2. **Commit** `feat(collect): detect ACLs and file capabilities on carried roots`.

### Task 9: `/var` collector (Rust lane; S)

**Spec:** § 4.5, § 5.6 "Parents" (scan half), decision 13.9; T10 (scan half).

**Files:** `crates/core/src/types/storage.rs` (`VarDirectory.ownership: Ownership` added beside the old fields; Task 10 removes them), `crates/collect/src/inspectors/storage.rs` (`discover_var_directories` maps its `stat` into `Ownership`, `UNKNOWN` from `%U`/`%G` to `Unnamed`, a failed stat to `Unknown`; the dedup drops a proper prefix only when its ownership is `0755 root:root`).

- [ ] **Step 1: Failing test first.** Beside `discover_var_directories_captures_ownership_service_account`: `t10_parent_kept`: `/var/lib/pgsql 0700 postgres:postgres` with leaf `/var/lib/pgsql/data` keeps both; a `0755 root:root` parent is dropped as today; `UNKNOWN` names map to `Unnamed`; a failed stat maps to `Unknown`.
- [ ] **Step 2:** `cargo test -p inspectah-collect --lib storage` fails.
- [ ] **Step 3:** implement. Names found here enter Task 7's pass like any recorded path's.
- [ ] **Step 4: Verify** `cargo test -p inspectah-collect --lib storage`, `cargo test -p inspectah-pipeline --test integration var_backing`. Today's renderer writes a `d` line for the kept parent; update the affected render snapshots and name them in the commit body.
- [ ] **Step 5: Changelog** Fixed: `/var` parent directories keep their ownership. **Commit** `fix(collect): keep /var parent directories whose ownership departs`.

### Task 10: Reader adaptation and field removal (Rust lane, Kit lane for the web adapters; M)

**Spec:** § 3.2 record table (`UnmanagedFile.provenance` and `VarDirectory` rows), § 14 task 10, § 6 "Removed".

**Goal:** remove `UnmanagedFile.provenance.uid`, `.gid`, `.permissions` and `VarDirectory.mode`, `owner_name`, `group_name`, `owner_uid`, `owner_gid`, and make every reader read the new records with today's output. One commit.

**Files:** `crates/core/src/types/nonrpm.rs`, `crates/core/src/types/storage.rs`, `crates/collect/src/inspectors/nonrpm.rs` (`get_file_metadata` stops filling the removed fields), `crates/collect/src/inspectors/storage.rs`; readers: `crates/pipeline/src/render/unmanaged_owners.rs` (`ownership_losses` and its uid/gid filter read `ownership.paths`), `crates/pipeline/src/render/containerfile.rs` (`resolve_tmpfiles_ownership`, `mkdir_line` read `ownership`), `crates/pipeline/src/render/report.rs` (`/var` rows), `crates/refine/src/projection/reference.rs`, `crates/web/src/adapter.rs`, `crates/web/src/aggregate_handlers.rs` (Kit lane: fill the unchanged API types from `ownership`); fixtures that carry the removed keys.

- [ ] **Step 1: Failing test first.** `landing2_render_unchanged` in `crates/pipeline/tests/` (new file, declared): render a collector-produced snapshot with unmanaged files owned by non-root accounts and `/var` directories, and compare the Containerfile, tarball member list and modes, audit report and HTML report with insta snapshots taken on `main` before this landing (record them in this step from the pre-Task-3 tree, then switch back). It fails to compile once the fields are removed.
- [ ] **Step 2:** remove the fields; adapt each reader; the web API types do not change.
- [ ] **Step 3: Verify.** `cargo test -p inspectah-pipeline --test integration landing2_render_unchanged`, `cargo test -p inspectah-refine`, `cargo test -p inspectah-web`, `cargo test -p inspectah-collect --test integration nonrpm`. Existing tests pass with fixture changes only; the only snapshot difference is Task 9's kept `/var` parents.
- [ ] **Step 4: Skills.** `export-paths-stage-different-trees.md`: `unmanaged_owners.rs` reads `ownership.paths`. **Commit** `refactor(core): read unmanaged and /var ownership from the ownership records`.

**Land:** `inspectah-worktree land feat/ownership-scan`.

---

## Landing 3: render and both exports

Tasks 11 to 18 each change render output; each task's commit updates the render snapshots it moves and names them in the body.

### Task 11: The resolver (Rust lane; L)

**Spec:** § 5.1 whole (include set, `ProvisioningDelivery`, `Subject`, `NumericVia`, `FallbackCause`, `Uncreatable`, `SystemAccount`, `PlannedPath`, `OwnershipPlan`, resolution steps 1 to 7), § 5.4, § 5.5 (plan half), § 8, § 7 (one resolver for both exports), rules 1, 4, 5, 6, 8 (plan half), 10 (local-user half), decisions 13.1, 13.5, 13.8; T4 (fallback half), T5, T6 (plan half), T7, T8 (image half), T11 (plan half), T14 (fallback half), T15 (plan half), T25 (plan half), T26 (plan half), T28, T29, T32 (plan half), T37, T38 (plan half).

**Files:** create `crates/pipeline/src/render/ownership.rs`; modify `crates/pipeline/src/render/mod.rs`, `crates/pipeline/src/render/users.rs` (`ProvisioningDelivery`; `render_kickstart` and `render_blueprint_toml` render from it).

- [ ] **Step 1: Failing tests first**, unit tests in `render/ownership.rs` over hand-built snapshots (Task 19 repeats them collector-produced), table-driven:
  - `t4_unknown_falls_back`: `Unknown` and not-recorded writes are `root:root 0600` (file) and `0700` (dir), rows, never `0644`.
  - `t5_orphan_owner`: orphan uid 4242 with system group `appgrp`: owner `Fallback(OrphanId)`, group `ByName`, special bits cleared, row.
  - `t7_special_bits` (six cases as the spec's T7 row) and `t32_directory_bits`: `2775 root:appgrp` keeps setgid; `1777` under `/opt` keeps sticky; `2770` whose group falls back drops setgid with a row.
  - `t8_image_modes`: clamped modes reach the plan; `/opt/foo 0666` carries with a row.
  - `t11_tmpfiles_local_user` (plan half), `t15_local_owner` (plan half: `ByNumber { via: Provisioning }` under `Provisioning`, `ByName` under `Image`), `t26_directory_account`, `t28_mixed_subjects`, `t29_not_in_include_set`.
  - `t37_not_delivered`: refused name and home, aggregate-excluded user with its file included, no recorded uid: each `Fallback(NotDelivered)` with `Refused`, `Excluded`, `NoFixedId`; `ProvisioningDelivery` matches the `# FIXME:` and directive lines the kickstart and blueprint render.
  - `t38_primary_group_closure`: `appgrp` enters `create` before `useradd -g appgrp`; local and directory primary groups give `Fallback(Uncreatable)` with `PrimaryGroupLocal`, `PrimaryGroupDirectory`.
  - `t25_uncreatable_and_not_found`, `t14_invalid_name_fallback`, `t6_setuid_system_owner` (plan keeps `4750`).
- [ ] **Step 2:** `cargo test -p inspectah-pipeline --lib render::ownership` fails to compile.
- [ ] **Step 3:** implement the plan as § 5.1 states. `ProvisioningDelivery` is the one walk both provisioning renderers consume, so the kickstart and blueprint output is byte-identical to before for every existing fixture; assert that with the existing users render snapshots.
- [ ] **Step 4: Verify.** `cargo test -p inspectah-pipeline --lib render::ownership`, `cargo test -p inspectah-pipeline --test integration users`.
- [ ] **Step 5: Skills.** Create `process-docs/skills/carried-file-ownership.md` with the resolver order, the fallback table and the setuid rule; add its row to `process-docs/skills/index.md`; `codebase-layout.md` lists `render/ownership.rs`. **Commit** `feat(pipeline): resolve carried ownership in one plan`.

### Task 12: Containerfile sequence, accounts and sysusers blocks (Rust lane; L)

**Spec:** § 5.3 (sequence, `from_lines`, accounts block, sysusers block, package overrides, what the override is), rules 3, 4, 10, decisions 13.6, 13.8, 13.10, § 1 last render bullet; T1 (accounts half), T2 (accounts half), T13 (render half), T17, T17a, T17b, T24 (render half), T34, T38 (render half).

**Files:** `crates/pipeline/src/render/containerfile.rs` (`from_lines` split out of `packages_section_lines`; accounts block; sysusers block with the prune and per-file overrides).

- [ ] **Step 1: Failing tests first.**
  - `t1_t2_accounts_before_packages`: group `named` at the host gid, and `appsvc` at 985, appear between `FROM` and the repository `COPY`; each account is its own `if`; the mirror `echo` lines.
  - `t34_one_if_per_account`: the rendered block has one `if` per account and no bare `||`/`&&` chain across accounts.
  - `t17_override`, `t17a_one_file_many_accounts`, `t17b_prune_by_namespace`: render the override and prune `RUN` text, then execute the embedded `awk` against sample sysusers files in a temp dir under `sh` and compare the outputs line by line with the spec's expected lines. Skip with a printed reason only if `awk` or `sh` is absent.
  - `t13_refused_file_no_override`: a `SysusersFile::Refused` declaration writes no override and a row.
  - `t24_tmpfiles_account_created`, `t38_closure_rendered`.
- [ ] **Step 2:** `cargo test -p inspectah-pipeline --lib containerfile` fails.
- [ ] **Step 3:** implement. No account name appears in the prune `RUN`. Home and shell are `ExportPath`s, falling back to `/` and `/sbin/nologin`.
- [ ] **Step 4: Verify** `cargo test -p inspectah-pipeline --lib containerfile`, `cargo test -p inspectah-pipeline --test integration smoke_render`.
- [ ] **Step 5: Changelog** Changed: system accounts that own carried files are created at the source host's ids before packages and declared in `/usr/lib/sysusers.d/inspectah-accounts.conf` unless a package declares them; a package that declares one without a fixed id gets an override in `/etc/sysusers.d` that pins it. **Skills:** `bootc-base-groups-and-users.md`: account creation before packages with its primary-group closure, the mirror pruned by namespace, the override that pins the implied group and freezes on a host that edits it, `COPY --chown` failing on `/usr/lib/group` accounts; its `index.md` row. **Commit** `feat(pipeline): create carried accounts before packages and pin their ids`.

### Task 13: Ownership block (Rust lane; M)

**Spec:** § 5.3 guards and ownership block, § 5.2 SSSD bullet, § 5.5 (render half), rules 3, 5, 9 (comment), decision 13.1 (render half); T1 (chown half), T2 (chown half), T5, T6, T15 (render half), T20 (render half), T21 (render half), T26 (guard half), T27, T28, T30 (chmod half), T32 (render half), T33.

**Files:** `crates/pipeline/src/render/containerfile.rs` (ownership block after the users block; remove `sssd_mode_lines` and its constants).

- [ ] **Step 1: Failing tests first.** `t1_chown_named`, `t2_directory_chown_chmod`, `t5_orphan`, `t6_setuid_after_chown` (the `chmod 4750` line follows the `chown -h`), `t15_numeric_with_guards` (four guard checks under `Provisioning`; `chown -h appuser:appuser` and no guard under `Image`), `t20_no_link_operand`, `t21_link_refusal_names_directory`, `t26_directory_guard`, `t27_sssd_no_chmod_0600`, `t28_guards_only_numeric`, `t30_no_owner_read_bit` (`chmod 0040` after the `chown`), `t32_directory_bits`, `t33_name_check_rendered`. Every `chown` is `chown -h`; no path ends in `/`; a path the token set refuses becomes a `# FIXME:` through `sanitize_for_comment`.
- [ ] **Step 2:** `cargo test -p inspectah-pipeline --lib containerfile` fails.
- [ ] **Step 3:** implement; delete `sssd_mode_lines`.
- [ ] **Step 4: Verify** as Step 2 and `cargo test -p inspectah-pipeline --test integration`.
- [ ] **Step 5: Changelog** Fixed: carried `/etc` files keep their owner and group; carried unmanaged files and directories keep owner, group and special bits; the SSSD configuration keeps its host mode instead of a fixed `0600`. **Skills:** `carried-file-ownership.md` gains the `chown`-clears-setuid ordering, `chown -h` and the link refusal. **Commit** `feat(pipeline): carry owner, group and special bits after the users block`.

### Task 14: `/var` lines through the resolver (Rust lane; S to M)

**Spec:** § 5.6 (lines, not content; recorded and fallback lines; owner and group fields), rule 11, § 12 Q6 and Q9 (render half); T10 (render half), T18, T26 (`/var` half).

**Files:** `crates/pipeline/src/render/containerfile.rs` (remove `mkdir_line` and the numeric cases of `resolve_tmpfiles_ownership`), `crates/pipeline/src/render/mod.rs` (`write_tmpfiles_conf` takes `var_lines` from the plan), `crates/pipeline/src/render/var_dirs.rs` if it reads the removed helpers.

- [ ] **Step 1: Failing tests first.** `t10_parent_line_first`; `t18_lines` (`d /var/lib/app 0750 appsvc appsvc -`; `d <path> :0700 :root :root -` and a row for `Unknown`; no `RUN mkdir`); `t26_directory_account_numeric` (`ByNumber { via: Directory }` writes numbers; `ByNumber { via: Provisioning }` writes the name).
- [ ] **Step 2:** `cargo test -p inspectah-pipeline --lib containerfile` fails.
- [ ] **Step 3:** implement.
- [ ] **Step 4: Verify** as Step 2 and `cargo test -p inspectah-pipeline --test integration var_backing`.
- [ ] **Step 5: Changelog** Changed: `/var` directory lines name accounts (numbers for directory accounts), and a `/var` directory whose ownership the scan could not read gets a create-only tmpfiles.d line instead of `RUN mkdir`. **Skills:** `carried-file-ownership.md` gains the create-only `/var` fallback. **Commit** `feat(pipeline): write /var ownership as tmpfiles lines from the plan`.

### Task 15: Staging modes (Rust lane; M)

**Spec:** § 5.2 (tarball, both export paths, no-owner-read-bit, directories, redacted files), rules 1, 6, 7; T3, T9, T16 (CLI half), T22 (staged half), T23 (staged half), T27 (staged half), T30 (staged half).

**Files:** `crates/pipeline/src/render/tarball.rs` (each staged member with the plan's mode, uid 0; directories stay `0755`), `crates/pipeline/src/render/configtree.rs` (`write_config_tree` sets the staged file's mode from the plan; `u+r` added where the owner read bit is missing), `crates/pipeline/src/build/extract.rs` (keep every member's mode masked with `0o777`, not only unmanaged payloads').

- [ ] **Step 1: Failing tests first.** `t3_sshd_config_0600` (staged and tar member `0600`, no `chown`); `t9_redacted_keeps_mode` (`0640`); `t16_extract_keeps_0600`; `t22_t23_staged_modes`; `t27_sssd_modes` (`0400` and `0600`); `t30_staged_0440`; and the `export-paths-stage-different-trees` guard fixture gains a `0600` config file.
- [ ] **Step 2:** `cargo test -p inspectah-pipeline --lib tarball` and `--lib build::extract` fail.
- [ ] **Step 3:** implement. `PAYLOAD_MODE_BITS` is unchanged.
- [ ] **Step 4: Verify** as Step 2 and `cargo test -p inspectah-pipeline --test integration configtree`.
- [ ] **Step 5: Changelog** Fixed: carried `/etc` files, from every section that carries them, keep their mode. **Skills:** `export-paths-stage-different-trees.md`: every staged file's mode is now carried, on both paths; its `index.md` row. **Commit** `fix(pipeline): stage every carried file with its planned mode`.

### Task 16: Refine (Rust lane; M)

**Spec:** § 7 whole, § 5.2 (refine download), § 5.8 (export warnings surface); T16 (refine half), T12 (single-host import, first pass).

**Files:** `crates/refine/src/session.rs` (`render_refine_export` stages modes from the same plan; `x-export-warnings` carries the ownership notice), `crates/refine/src/projection/{types.rs,reference.rs}` (read-only `ownership` detail on config, repository, GPG key, firewall zone, drop-in and unmanaged rows: host owner, group, mode and clamp; never written by refine), `crates/web/src/web_types.rs` (the detail field on the row types).

- [ ] **Step 1: Failing tests first.** In `crates/refine/tests/export_parity_test.rs`: `t16_refine_download_keeps_0600`; `ownership_detail_projected` (each row kind carries its detail, looked up by path); `refine_never_writes_ownership` (apply an include edit and the delivery switch; the session's `ownership` section is byte-equal to the scan's).
- [ ] **Step 2:** `cargo test -p inspectah-refine --test integration export_parity` fails.
- [ ] **Step 3:** implement.
- [ ] **Step 4: Verify** `cargo test -p inspectah-refine`, `cargo test -p inspectah-web --lib`.
- [ ] **Step 5: Commit** `feat(refine): stage planned modes and project host ownership`.

### Task 17: Notices (Rust lane, Kit lane for the HTML report; M)

**Spec:** § 5.8 whole, § 5.7 (notice half), § 5.3 override rows, § 5.5 warning text, § 1 "Interim state"; T11 (notice half), T19.

**Files:** replace `crates/pipeline/src/render/unmanaged_owners.rs` with the plan's notice (delete the file, move its callers); trim `crates/pipeline/src/render/tmpfiles_accounts.rs` to uncreatable, `NotFound` and refused names; `crates/pipeline/src/render/audit.rs`, `crates/pipeline/src/render/report.rs`, `crates/pipeline/src/render/report_data.rs` (HTML report, Kit lane), `crates/pipeline/src/export_warning.rs`, `crates/cli/src/commands/scan.rs` (stderr count from the plan); the Containerfile's one counts comment line; `crates/cli/tests/tmpfiles_owner_notice_test.rs`.

- [ ] **Step 1: Failing tests first.** `t19_after_deployment_text`: the audit report, HTML report and export warnings each carry § 5.8's four after-deployment items once. `notice_rows_cover_every_item` (table over one snapshot holding each case: fallback, created account, provisioning account with uid and gid, directory account, not delivered with gap, uncreatable, override and refused declaration, setuid carried and dropped with the warning rows, clamped and carried world-writable, ACL and capability paths and gaps, `Image` users and `preserve` hashes). `host_text_escaped`: an `Unnamed.host_name`, a `SysusersFile::Refused.host_name`, a path and an ACL path holding `<`, `&` and a newline are escaped on the HTML report and the audit report, and the Containerfile holds only counts. `t11_no_tmpfiles_notice_for_delivered_user`.
- [ ] **Step 2:** `cargo test -p inspectah-pipeline --lib render::audit` and `--lib export_warning` fail.
- [ ] **Step 3:** implement.
- [ ] **Step 4: Verify** as Step 2, `cargo test -p inspectah-cli --test integration tmpfiles_owner_notice`, `cargo test -p inspectah-pipeline --test integration`.
- [ ] **Step 5: Changelog** Changed: the beta.8 ownership notice becomes the ownership notice; files owned by local users the kickstart and blueprint create are owned by number with a build guard, and files owned by a local user they do not create fall back to `root`, with a notice; tmpfiles.d lines whose account the image lacks get the account. **Skills:** `export-paths-stage-different-trees.md` drops the `unmanaged_owners.rs` sentence; `codebase-layout.md` drops the file. **Commit** `feat(pipeline): report ownership from the plan on every surface`.

### Task 18: Web row detail (Kit lane; S to M)

**Spec:** § 7 (row detail, export warnings in refine), § 5.8 last paragraph ("refine shows a carried row's host ownership and clamp as read-only detail"); § 14 task 18.

**Files:** the refine UI row components for config, repository, GPG key, firewall zone, drop-in and unmanaged rows under `crates/web/ui/src/components/`; `crates/web/ui/src/api/types.ts`; the export-warnings display if the new rows need layout; e2e bodies re-recorded for the detail field, registered in `manifest.json` and `crates/web/tests/fixture_structure_test.rs`.

- [ ] **Step 1: Failing test first.** Vitest: a row with `ownership` detail shows owner, group and mode read-only, and the clamp as "narrowed from 0666 to 0664" when present; a row with no detail shows "ownership not recorded"; no control edits it. Playwright: the export warnings panel lists ownership rows.
- [ ] **Step 2:** implement.
- [ ] **Step 3: Verify.** `npx tsc --noEmit`, `npx vitest run`, `npx playwright test e2e/export-warnings.spec.ts e2e/sections.spec.ts`, then `cargo test -p inspectah-web --test integration fixture_structure`.
- [ ] **Step 4: Commit** `feat(web): show host ownership on carried rows`.

### Task 19: Contract matrix (Rust lane; M)

**Spec:** § 10.1 (collector-produced matrix), § 7 parity; T12, T35 (collector-produced half), and the end-to-end form of T1 to T11, T13 to T38.

**Files:** create `crates/pipeline/tests/ownership_contract_test.rs` and `crates/refine/tests/ownership_parity_test.rs` (declared in each `main.rs`); fixtures under `testdata/fixtures/ownership/`.

- [ ] **Step 1:** one table-driven test over collector-produced snapshots (mock executor, real inspectors, `finalize_cross_inspector`, render) with a row per T case the spec lists, asserting the spec's Expected column on the rendered Containerfile, tarball members and reports. These run green against Tasks 11 to 18; a failing row is a bug in its owning task, fixed there before this commit.
- [ ] **Step 2: T12 parity.** Import the snapshot into a refine session, export untouched, compare with the CLI tarball: the Containerfile, staged modes and the `ownership` section are equal; § 0's value is unchanged.
- [ ] **Step 3: T35 collector-produced.** Serde round-trip every snapshot the matrix builds.
- [ ] **Step 4: Verify** `cargo test -p inspectah-pipeline --test integration ownership_contract`, `cargo test -p inspectah-refine --test integration ownership_parity`.
- [ ] **Step 5: Commit** `test(pipeline): run the ownership contract matrix from collected snapshots`.

### Task 20: Build checks (real host; rhel9 and rhel10)

**Spec:** § 10.2 builds; R1, R2, R3, R4, R5 (build half), R6, R7, R9, R10, R12, R18; § 12 Q3 (R9, R14 split), Q4 (R12), rule 3.

- [ ] **Step 1:** build the branch's binary for each VM's architecture. Revert each VM. Apply driftify fixtures for: `bind` with `/etc/named.conf 0640 root:named` (R1); `appsvc` system account at 985 with `/etc/appsvc 0750` and `secret.conf 0600` (R2); `sshd_config 0600` (R4); `/opt/app/bin/helper 4750 appsvc:appgrp` (R5); an unreadable file and an orphan-owned file (R6); a tmpfiles line naming local `appuser` 1001 (R7); a carried package that creates `appuser` at another id, and an account block group whose gid the base uses (R10); a package declaring `u appsvc -` (R12, T17); unmanaged symlinks (R18). Run `inspectah scan`, then `podman build` the export on that release's `rhel-bootc` base.
- [ ] **Step 2:** in each built image: `stat -c '%a %U:%G' <path>` per case; `runuser -u named -- cat /etc/named.conf` (R1); `getent passwd appsvc` (R2); R3: `stat /etc/sudoers.d` after a build with the directory `chmod` removed by hand from a copy of the Containerfile, to learn whether `COPY` resets it; R5: `chown` then `chmod` keeps `4750`, reversing the order clears it; R7 (switch at `Image`): `systemd-tmpfiles --create` exits 0 and the directory is owned by `appuser`; R9: a scratch Containerfile with `COPY --chown=root:cdrom` fails with "looking up UID/GID" and `RUN chown root:cdrom` succeeds; R10: each build stops with the inspectah message and no later account line ran; R12: `bootc container lint` shows no `sysusers` warning for `appsvc` and no `/var` tmpfiles warning from inspectah's lines; R18: each link target keeps its ownership and mode.
- [ ] **Step 3: Report** per release and check: command, output, pass or fail. A failure is fixed in the owning task's code on this branch, with a regression test, before Task 21.

### Task 21: Booted checks (real host; rhel9 and rhel10 bootc hosts)

**Spec:** § 10.2 booted; R5 (booted half), R8, R13, R14, R15, R16, R17, R19; § 12 Q1, Q2, Q5, Q6, Q7; rules 9 and 11; § 5.8 after-deployment text.

- [ ] **Step 1:** install Task 20's images on a rhel9 and a rhel10 bootc host.
- [ ] **Step 2:** R5: `stat /opt/app/bin/helper` shows `4750`, `findmnt -no OPTIONS /` has no `nosuid`. R8: across a reboot the recorded `d` lines create parent and leaf; `chown root:root` on the parent is reverted at the next boot; the create-only line leaves an existing directory alone. R13: `chown root:wheel /etc/chrony.conf`; `ostree admin config-diff | grep chrony` shows `M chrony.conf`; upgrade to an image changing it and a control file: the modified one keeps the host copy, the control follows; `cp -a /usr/etc/chrony.conf /etc/chrony.conf`, upgrade again, it follows. R14: `grep -c . /etc/passwd /usr/lib/passwd`, the `passwd` and `group` lines of `/etc/nsswitch.conf`, `getent passwd chrony`. R15: `ls -ld /opt /usr/local /srv /root /home`. R16: deploy `/etc/appsvc 0750 appsvc:appsvc`, edit `/etc/appsvc/app.conf` and `chmod 0755 /etc/appsvc`, switch to an image that changes nothing there: `0755` stays; then the reverse. R17: deploy an image without `appsvc`, `bootc switch` to T2's image, reboot: `getent passwd appsvc` shows 985 and `journalctl -b -u systemd-sysusers` has no conflict line; repeat with a package-declared account. R19: T17's image on a host whose `/etc/passwd` and `/etc/group` lack `appsvc`, with host uid and gid that differ: after reboot both show the pinned ids.
- [ ] **Step 3: Report** per release and check. A result that contradicts § 5.8's after-deployment text is fixed in Task 17's text on this branch before Task 22.

### Task 22: Changelog, skills and user docs (docs; S)

**Spec:** § 9 whole; § 14 task 22.

**Files:** `CHANGELOG.md`, `process-docs/skills/carried-file-ownership.md`, `process-docs/skills/index.md`, `process-docs/skills/codebase-layout.md`, `process-docs/skills/bootc-base-groups-and-users.md`, `process-docs/skills/export-paths-stage-different-trees.md`, `docs/reference/known-limitations.md`, `docs/reference/inspector-coverage.md` (the ownership recording and the `getfacl`/`getcap` commands), `docs/how-to/review-and-refine.md` (row detail, the users section switch), `docs/how-to/build-bootc-image.md` (accounts block, ownership block, `/var` lines).

- [ ] **Step 1:** read the `[Unreleased]` entries Tasks 3, 9 and 12 to 17 added against § 9's list; nothing in § 9 is missing and nothing is said twice. `cross-inspector-shadow-passes.md` is unaffected (§ 9).
- [ ] **Step 2:** complete `carried-file-ownership.md` (resolver order, `chown`-clears-setuid ordering, `chown -h` and the link refusal, the fallback table, the merge rules for files and directories, the create-only `/var` fallback) with what Tasks 20 and 21 measured, each claim marked measured or read.
- [ ] **Step 3:** remove the known-limitations entries beta.8 added for carried ownership. At plan time (`fd966549`) `docs/reference/known-limitations.md` holds none; check again and say which were removed, or that none existed.
- [ ] **Step 4: Commit** with `git commit --only <paths> -F <file>` (docs-only) `docs: describe carried-file ownership`.

**Land:** `inspectah-worktree land feat/ownership-render`. Move the spec to `process-docs/specs/implemented/` in a docs commit after the landing.

---

## Spec fidelity

### Sections

| Spec section | Task |
|---|---|
| Metadata, Release | Sequencing table; 1, 2, 3 to 10, 11 to 22 |
| § 0 (enum, field, every local user, `Image`, Goes, Comes, Unchanged, schema 32, Q8 note) | 1; Q8 in 2 |
| § 1 Problem (each evidence bullet is a requirement the tasks remove) | Scan bullets: 3 to 9; render bullets: 12 to 15; interim state: 17; why mode cannot go first: rule 6 row |
| § 2.1 rule 1 (never widen) | 6 (clamp), 11, 15 |
| rule 2 (names at scan) | 3, 5 |
| rule 3 (`chown` by name after the users block) | 13; R9 in 20 |
| rule 4 (create at host id before `chown`) | 11, 12 |
| rule 5 (setuid only when ownership carries) | 11, 13 |
| rule 6 (land together) | Landing 3 lands mode (15), owner and group (13) and fallback (11) in one landing |
| rule 7 (redacted keeps mode) | 15 |
| rule 8 (fleet never wider) | Convergence spec (§ 6); this plan carries the input fields (3) and the resolver's single-host half (11) |
| § 2.2 rule 9 (right the first time) | 13 (comment), 17 (notice), R13 and R16 in 21 |
| rule 10 (pin system accounts) | 12; R17 and R19 in 21 |
| rule 11 (`/var` as `d` lines) | 14; R8 in 21 |
| rule 12 (no SELinux labels) | Nothing carried; R5 and R15 in 20 and 21 confirm the layout |
| § 2.2 trust assumptions | 5 (directory names), 6 (stat as root), 11 (provisioning) |
| § 3.1 | 3 |
| § 3.2 types | 3 |
| § 3.2 record table | `InspectionSnapshot` 3; `UnmanagedFile.provenance` 10; `VarDirectory` 9, 10; `WithheldConfigEntry` 6 |
| § 3.3 | 3 |
| § 4.1 | 4 (executor), 6 (which call) |
| § 4.2 | 6 |
| § 4.3 | 6 |
| § 4.4 | 5 (index), 7 (pass) |
| § 4.5 | 9 |
| § 4.6 | 8 |
| § 5.1 | 11 |
| § 5.2 | 15 (staging), 13 (directory and no-read-bit `chmod`, SSSD), 16 (refine download) |
| § 5.3 sequence, accounts, sysusers, overrides | 12 |
| § 5.3 guards, ownership block | 13 |
| § 5.4 | 11 (plan), 13 (render) |
| § 5.5 | 11 (plan), 13 (render), 17 (audit listing and warning rows) |
| § 5.6 | 9 (dedup), 14 (lines); "not closed here" stays open |
| § 5.7 | 7 (scan), 11 (placement), 12 (accounts), 17 (notice) |
| § 5.8 | 17 (all surfaces), 16 (export warnings), 18 (refine display) |
| § 6 field list | 3 (fields exist), 8 (`acl`/`capabilities` merge); items 1 to 3, 5 and 6 merged by the convergence spec |
| § 7 | 16, 18; parity 19 |
| § 8 | 11 (`ProvisioningDelivery`), 13 (guards), 17 (required-accounts rows), 2 (R11) |
| § 9 | 1 (§ 0 change), 3, 9, 12 to 17 (entries and skills), 22 (docs and read-through) |
| § 10.1 | per-task tests below; 19 |
| § 10.2 | 2, 20, 21 |
| § 11 non-goals | Nothing built; ACL and capability notice in 8 and 17 |
| § 12 Q1 | R13, R16 (21) |
| Q2 | 12 (pinning); R17 (21) |
| Q3 | 12, 13; R9 (20), R14 (21) |
| Q4 | 14; R8 (21), R12 (20) |
| Q5 | R5 (20, 21), R15 (21) |
| Q6 | 14; R8 (21) |
| Q7 | 12; R19 (21) |
| Q8 | 2 |
| Q9 | 14 writes numbers for directory accounts either way; not checked on a host (no directory service) |
| § 13.1 | 11, 13, 17 |
| § 13.2 | 6, 11, 17 |
| § 13.3 | Convergence spec; 3 provides the fields |
| § 13.4 | 1 |
| § 13.5 | 2 (gate), 11, 13 |
| § 13.6 | 12, 13 |
| § 13.7 | 8, 17 |
| § 13.8 | 11, 12 |
| § 13.9 | 9, 14 |
| § 13.10 | 12, 17 |
| § 14 | This plan's sequencing; § 14 task *n* is Task *n* |

### Tests

| Test | Written first in | End-to-end in |
|---|---|---|
| T1 | 12, 13, 15 | 19 |
| T2 | 12, 13 | 19 |
| T3 | 15 | 19 |
| T4 | 6, 11 | 19 |
| T5 | 11, 13 | 19 |
| T6 | 11, 13 | 19 |
| T7 | 11 | 19 |
| T8 | 6, 11 | 19 |
| T9 | 15 | 19 |
| T10 | 9, 14 | 19 |
| T11 | 11, 17 | 19 |
| T12 | 19 | 19 |
| T13 | 3, 5, 12 | 19 |
| T14 | 5, 11 | 19 |
| T15 | 11, 13 | 19 |
| T16 | 15, 16 | 19 |
| T17, T17a, T17b | 12 | 19 |
| T18 | 14 | 19 |
| T19 | 17 | 19 |
| T20 | 6, 13 | 19 |
| T21 | 6, 13 | 19 |
| T22 | 6, 15 | 19 |
| T23 | 6, 15 | 19 |
| T24 | 7, 12 | 19 |
| T25 | 7, 11 | 19 |
| T26 | 11, 13, 14 | 19 |
| T27 | 13, 15 | 19 |
| T28 | 11, 13 | 19 |
| T29 | 11 | 19 |
| T30 | 13, 15 | 19 |
| T31 | 8 | 19 |
| T32 | 11, 13 | 19 |
| T33 | 13 | 19; R10 in 20 |
| T34 | 12 | 19; R10 in 20 |
| T35 | 3 | 19 |
| T36 | 1 | 1 |
| T37 | 11 | 19 |
| T38 | 11, 12 | 19 |
| R1, R2, R3, R4, R6, R7, R9, R10, R12, R18 | 20 | |
| R5 | 20 (build), 21 (booted) | |
| R8, R13, R14, R15, R16, R17, R19 | 21 | |
| R11 | 2 | |

### Not settled by the spec

- **Aggregate ownership between Landing 3 and the convergence spec.** § 6 hands the merge of `ownership.paths`, `users`, `groups` and `storage.var_directories[].ownership` to the convergence spec, and § 14 puts all of this spec before convergence code. Until then an aggregate snapshot has no merged `paths`, so § 5.1's "no record is `Unknown`" rule renders every carried file in a fleet export `0600 root:root`, with rows. That fails closed, but it is a visible change for fleet exports in 0.9.1-beta.1. Task 8 merges only `acl` and `capabilities` (T31). Needs a call from the project owner before Landing 3 lands: accept the interim fallback, or bring the convergence merge forward.
