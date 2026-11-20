# Merge integration tests into one binary per crate: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build each crate's integration tests as one test binary instead of one binary per file, so a core edit links about 80 fewer test binaries, without losing, renaming or changing any test or snapshot.

**Architecture:** Test files stay where they are. Each crate sets `autotests = false`, declares one `[[test]] name = "integration", path = "tests/main.rs"`, and `tests/main.rs` declares every former test file as a `mod`. A shared guard test fails when a `tests/*.rs` file is not declared. Insta snapshots are renamed (content untouched) to the new module path. A pilot crate is measured before the rest proceed.

**Tech Stack:** Cargo targets, libtest, insta 1.47.2 (cargo-insta 1.49.0 for the unreferenced check), the pre-commit hook in `.githooks/pre-commit`.

**Spec:** the approved commit-gate recommendation of 2026-11-19, § 5 ("Later, on clean numbers: one integration-test binary per crate") and § 8 step 7, with the estimate from the commit-gate timing report of the same day, § 4. Both are private working notes; every number this plan relies on is restated under Background, so the plan stands alone.

## Global Constraints

- One crate per commit. Every commit goes through `.githooks/pre-commit` (fmt, `cargo clippy --workspace --all-targets -- -D clippy::all`, `cargo test --workspace`). Never `--no-verify`.
- Iterate with crate-scoped commands (`cargo test -p <crate>`); let the hook run the workspace gates once at commit time.
- Stage new and changed files by explicit path before running `scripts/tests/docs-only-paths-test.sh`: it lists sources with `git ls-files`, so an untracked `tests/main.rs` or guard file is invisible to it.
- No test is deleted, skipped, `#[ignore]`d or renamed, other than gaining its file stem as a module prefix.
- Every `.snap` file keeps byte-identical content: a pure rename, shown as `R100` by git.
- No production source under `crates/*/src/` changes.
- No CHANGELOG entry: test layout only, no user-visible change.
- Work happens in a worktree created by the orchestration repo's `workflow/inspectah-worktree create <branch>`. The orchestrator lands it (`land`) and removes it (`cleanup`). Implementers commit, never push, never land.
- Commit trailer: `Assisted-by: <tool> (<model>)`. No other attribution trailer.
- Times are taken with `/usr/bin/time -p` inside the same command, never from the agent's own sense of elapsed time (`process-docs/skills/full-workspace-verification-gates.md` § 4). Record `uptime` load before and after every sample. Do not poll processes with `pgrep -f` loops.

## Background: the numbers this plan argues from

- Core-edit hook today: median 120 s, of which the test step is 104 to 111 s; leaf (`crates/cli/src/main.rs`) edit: median 41 s (measured 2026-11-20, `full-workspace-verification-gates.md` § 3, load about 9 to 13).
- Core-edit test step split (2026-11-20, `cargo test` runner): about 75 s of build and relink, 27 s of run.
- The 2026-11-19 estimate for this step: "most of the ~62 s core test build (about 80 fewer links) and ~10 s of binary start-up". The start-up half is now known to be smaller: every new process on this Mac costs 25 to 40 ms (measured 2026-11-20), so 86 binaries down to 6 or 7 saves about 2 to 3.5 s of spawn, not 10 s. Any further run-time gain comes from tests of different files running in parallel inside one process instead of one binary after another.
- A cold build's `--timings` report (2026-11-19, load about 30, one sample) shows the test targets of web and cli at the tail of the build, starting at 76 to 93 s of a 100 s build, while refine's 28 test targets and pipeline's 20 finish earlier, overlapping the web and cli library builds. So merging off-tail crates (refine, pipeline, collect, core) cuts CPU contention, and merging tail crates (web, cli) can lengthen the critical path if one merged crate compiles slower than the parallel set it replaces. The plan measures both cases rather than assuming either.
- `[build] incremental = false` (rcgu leak workaround) stays. Without incremental, editing any one test file recompiles the whole merged crate. This is the main cost of the change and the pilot measures it.

## Current layout (read from the tree at `da9a746c`)

| crate | package | test files | `#[test]` fns | lines | helper modules declared by test files |
|---|---|---|---|---|---|
| cli | `inspectah-cli` | 10 | 298 | 22,587 | 5: `#[path = "support/unit_hosts.rs"]` in 3 files, `#[path = "support/sysctl_hosts.rs"]` in 2 |
| collect | `inspectah-collect` | 16 | 221 | 13,942 | 0 |
| core | `inspectah-core` | 7 | 212 | 7,000 | 0 |
| pipeline | `inspectah-pipeline` | 19 | 353 | 21,122 | 2: `mod selinux_support;` in `smoke_render.rs` and `selinux_block_shell_test.rs` |
| refine | `inspectah-refine` | 27 | 351 | 16,383 | 1: `mod helpers;` in `session_test.rs` |
| web | `inspectah-web` | 7 | 194 | 10,042 | 2: cross-crate `#[path = "../../cli/tests/support/…"]`, one each in `api_test.rs` and `system_tuning_fixtures_test.rs` |
| **total** | | **86** | **1,629** | | **10** |

No crate has `autotests` or a `[[test]]` entry. `crates/cli/tests/units_scan_test.rs` alone is 15,105 lines. The 5 `compile_fail` doc tests (3 in `crates/core/src/types/units.rs`, 2 in `crates/pipeline/src/render/unit_plan.rs`) live in library source and are not touched.

## Layout decision

**Chosen: files stay put, `autotests = false`, one `[[test]]` per crate.**

```toml
# crates/<crate>/Cargo.toml
[package]
autotests = false

[[test]]
name = "integration"
path = "tests/main.rs"
```

`tests/main.rs` holds a module doc comment, one `mod <stem>;` per former file in alphabetical order, the shared helper modules, and the guard module. Because each file keeps its path, every `include_str!`/`include_bytes!` relative path, every `#[path]` attribute, every doc and skill link to `crates/*/tests/<file>.rs`, and every docs-only allowlist entry stays valid. Insta still looks in `tests/snapshots/`; only the file name prefix changes.

Why this resolves: a module declared from a non-root file looks for its children under a directory named after that file (`tests/session_test/helpers.rs`), but `#[path]` on such a module resolves from the directory the file sits in, and `include_str!` always resolves from the file. Neither moves, so neither changes.

**Rejected: `tests/<name>/main.rs` with the files moved into that directory** (Cargo auto-discovers a directory test target, no manifest change). It needs all 86 files moved, every one of the 81 `include_str!`/`include_bytes!` paths deepened by one `../`, both snapshot directories moved, 4 docs-only allowlist entries rewritten, and about 30 skill and contributor doc links updated. It does keep one advantage: a new top-level `tests/foo.rs` would still compile, as its own binary. The guard test below removes that difference.

**Rejected: several binaries for the heavy crates by default.** One binary per crate is the default. Tasks 4 and 5 measure a split only for the two crates on the build's tail (web, cli), where a merge can lengthen the critical path.

**`units_scan_test`:** it goes into cli's merged binary by default, where its slowest tests (up to 11.6 s at the 11-19 measurement, about 6 s since deps moved to opt-level 2) overlap with other files' tests instead of running alone. Task 5 measures the alternative, cli as two `[[test]]` targets (`units_scan_test` alone, keeping its current target name, and `integration` for the other 9 files), and keeps the split only if it wins on the leaf-edit gate by at least 3 s.

**Retained targets.** If Task 4 or Task 5 keeps a split, that crate has one extra `[[test]]` target besides `integration` (web `api_test`, cli `units_scan_test`). The extra target is its own crate root: it declares its own helper modules, it does not include the guard (the guard counts it as declared through its manifest `path` entry), and its tests list without a `<stem>::` prefix. The inventory and order procedures below loop over every target of the crate, and the docs name the extra target's run command (`cargo test -p <package> --test <target>`) as the exception to `--test integration <stem>::`.

**Binary name `integration` in every crate.** On the pinned toolchain (cargo 1.100) each test executable sits in its own hashed directory, `target/debug/build/<package>/<hash>/out/integration-<hash>`, so equal names across packages do not collide. `cargo test --workspace --test integration` runs every crate's integration tests at once.

## Hazards of turning a file into a module, counted

Counts are from reading every `crates/*/tests/**/*.rs` file at `da9a746c`.

| hazard | count | effect under the chosen layout | action |
|---|---|---|---|
| `include_str!`/`include_bytes!` with a relative path | 81 in 14 files (collect 11 files, core 1, pipeline 1, refine 1), all `../` relative | none: the file does not move | none |
| `env!("CARGO_MANIFEST_DIR")` | 3 (refine `testdata_tarballs_load.rs`, web `fixture_structure_test.rs`, web `system_tuning_fixtures_test.rs`) | none: same package | none |
| `CARGO_BIN_EXE_inspectah` | 1 (cli `not_carried_parity_test.rs`) | none: Cargo sets it for every `[[test]]` target of the package | none |
| insta file snapshots | 37 `.snap` files (collect 3, web 34) from 5 files, 10 macro sites | the name prefix `<stem>__` becomes `integration__<stem>__` (insta 1.47.2 `get_snapshot_filename` writes `module_path` with `::` as `__`; the binary's crate name is the first segment) | `git mv` each to the new name; content untouched |
| insta inline snapshots | 1 (`contract_snapshots.rs`) plus string `@` data in 3 other files | none: inline snapshots have no file | none |
| `std::env::set_var`/`remove_var`, `set_current_dir` | 0 | none | none |
| `static`, `OnceLock`, `LazyLock`, `OnceCell`, `thread_local!`, `lazy_static!` in test files | 0 | none | none |
| mutable process state in library code reachable from integration tests | 0 (library statics are `LazyLock<Regex>` caches, read-only; the only `set_var` is in `crates/cli/src/progress/mod.rs` unit tests, which stay in the lib test binary) | none | none |
| panic hooks, global tracing or logger init, umask, rlimit, signals | 0 (3 text matches for "signal" in `unmanaged_scan_test.rs` are test names) | none | none |
| fixed ports | 0 bound. 24 `host:port` literals in 6 files are `Origin`/`Host` header strings; the one listener (`cli/tests/refine_e2e_test.rs`) binds `127.0.0.1:0` | none | none |
| shared temp or tree paths | 25 `"/tmp/..."` literals in 4 files are string data, never opened. One test writes into the tree, only with `UPDATE_SYSTEM_TUNING_FIXTURES` set, into `crates/web/ui/src/__tests__/fixtures/system-tuning/`, which no other web test reads (`fixture_structure_test.rs` reads `ui/e2e/fixtures/`) | none | none |
| child-process environment | 1 (`selinux_block_shell_test.rs` sets `HOME` on a spawned `Command` only) | none | none |
| helper `mod` declarations inside test files | 10 in 10 files (table above) | refine 1 and pipeline 2: compile error, a non-root file looks for `tests/<stem>/<mod>.rs`. cli 5: the `#[path]` form still compiles, but loading one file as a module twice in one crate trips `clippy::duplicate_mod`, which `-D clippy::all` denies. web 2: one use each per crate, still valid | move each helper to one declaration in `main.rs` and import it with `use crate::<mod>` (refine, pipeline, cli); leave web's two as they are |
| `crate::` or `super::` paths in test files | 0 | none | none |
| inner attributes `#![...]` in test files | 0 (3 helper files carry `#![allow(dead_code)]`, valid at module level) | none | none |
| `#[cfg]`-gated test files or items | 0 | none | none |
| `#[ignore]` | 0 | none | none |
| `fn main`, `harness = false`, `macro_rules!`, `extern crate`, `#[macro_use]` | 0 | none | none |
| function names defined in more than one file of a crate | 54 (cli 13, collect 5, core 2, pipeline 15, refine 13, web 6), for example `snapshot`, `package`, `set_include`, `with_loopback_host` | none: each file is its own module namespace; test names gain the `<stem>::` prefix and stay unique | none |
| a new `tests/*.rs` file nobody declares | n/a today | with `autotests = false` it compiles into nothing and its tests silently never run | guard test (Task 1) |
| `cargo test --test <stem>` in docs and comments | 5 lines in 3 files (below) | the target no longer exists | rewrite as `--test integration <stem>::` |

**Docs-only read check.** `scripts/tests/docs-only-paths-test.sh` keys its allowlist by file path. Four entries name integration test files: `crates/cli/tests/copy_source_exists_test.rs`, `crates/cli/tests/units_scan_test.rs`, `crates/cli/tests/warning_markdown_test.rs`, `crates/pipeline/tests/e2e.rs`. Under the chosen layout none of them moves, so the allowlist does not change. Every task still runs the script before committing, because the new `tests/main.rs` files and the guard module are new scanned sources and a stale entry would fail it. (Under the rejected directory layout all four entries would have to move in the same commit as their files.)

## Review Focus

1. **A test file added after the merge and never declared.** A contributor following habit drops `tests/new_test.rs` into a crate; it must fail the gate, not vanish. Pinned by the guard test and its control case in Task 1.
2. **A snapshot renamed to the wrong prefix or left behind.** With `INSTA_UPDATE=no` a wrong name fails as a missing snapshot; an old name left beside the new one would pass silently, so `--unreferenced=reject` and the `R100` count are both required (Tasks 2 to 4).
3. **Cross-file order dependence that one binary per file was hiding.** No shared process state was found by reading, but reading is not running: three shuffled runs and one single-threaded run per merged binary (every task).
4. **Editing one test file now recompiles the whole crate's tests.** A contributor iterating on one test pays for all of them; the pilot measures it and the bar caps it (Task 1).
5. **A tail crate's merged binary lengthening the core-edit gate.** web and cli each land only if the selected variant's core-edit gate is no slower, within the tolerance, than the layout immediately before it; otherwise that crate stays one binary per file (Tasks 4 and 5).

---

## Measurement procedure (used by Tasks 1, 4, 5 and 6)

Two warm worktrees: **A**, a baseline at the layout immediately before the change under test (created with `inspectah-worktree create measure/step7-baseline-<n>`, never committed to except for Task 6's reverts, cleaned up after), and **B**, the branch under test. A is main for Task 1, and the branch tip before the crate's own edits for Tasks 4 and 5, so one crate's saving cannot hide another's regression. Wait for each worktree's warm-up to finish before sampling.

- **Gateable state.** The hook refuses, in about a second and before any gate runs, when a non-docs path differs from the index (an unstaged edit or an untracked file). Before sampling B with uncommitted layout changes, `git add` every changed, new and renamed path of the variant under test, and check `git status --porcelain` shows no ` M`, `MM` or `??` line. A committed B needs nothing.
- **Gate sample (edit E in worktree W):** `uptime; printf '// probe\n' >> E && git add E && /usr/bin/time -p .githooks/pre-commit; echo "exit $?"; git restore --staged --worktree E; uptime`. `E` is `crates/core/src/lib.rs` (core edit) or `crates/cli/src/main.rs` (leaf edit). Accept the sample only when the hook exited 0 and printed all three of `pre-commit: fmt ok`, `pre-commit: clippy ok` and `pre-commit: test ok`; discard and retake any other (a refusal or a failed gate is not a fast sample). Record per-gate lines, wall, and both loads.
- **Interleave:** A B B A A B A B B A for the go/no-go (5 per side); A B B A A B for the per-crate checks (3 per side). A sample that starts above load 15 is discarded and retaken.
- **Noise floor and tolerance.** Every comparison has a same-commit set taken first, with A and B at the same commit (Task 1 Step 1, Task 4 Step 1, Task 5 Step 1). Its noise floor N for a metric is the gap between A's and B's medians on that set. The tolerance for that metric is T = max(2 s, N). "No slower" means median(B) minus median(A) is at most T; "slower" means it is more than T; "faster by X" means median(A) minus median(B) is at least X.
- **Crate-scoped build (crate C):** append a probe comment to `crates/C/src/lib.rs`, `/usr/bin/time -p cargo test -p inspectah-C --no-run`, restore. Same with the probe in one test file (`crates/C/tests/<largest file>.rs`) for the test-edit cost.
- **Crate-scoped run:** warm, `/usr/bin/time -p cargo test -p inspectah-C --tests`.
- Record every sample in the commit body's measurement table (crate tasks) or in `full-workspace-verification-gates.md` § 3 (Task 6). Keep raw logs in the dispatch's own scratch subdirectory, not in the repo.

## Test inventory procedure (used by every crate task)

Run in the worktree, outputs to the dispatch's scratch subdirectory. `<targets>` is the crate's target list written out literally (a variable would not word-split under zsh): `integration` everywhere, `integration api_test` for a split web, `integration units_scan_test` for a split cli (Layout decision, Retained targets).

- **Before (on the branch's base, before any edit):**
  `for f in crates/C/tests/*.rs; do s=$(basename "$f" .rs); cargo test -q -p inspectah-C --test "$s" -- --list 2>/dev/null | sed -n "s/: test\$//p" | sed "s/^/$s::/"; done | sort > C-before.txt`
- **After:** `for t in <targets>; do cargo test -q -p inspectah-C --test "$t" -- --list 2>/dev/null | sed -n 's/: test$//p' | if [ "$t" = integration ]; then grep -v '^test_files_declared::'; else sed "s/^/$t::/"; fi; done | sort > C-after.txt`
- **Pass:** `diff C-before.txt C-after.txt` prints nothing, and `wc -l` matches. Record the count in the commit body.
- **Order independence, for each target t in `<targets>`:** take the executable path from cargo's JSON output rather than guessing a directory: `bin=$(cargo test -p inspectah-C --test "$t" --no-run --message-format=json 2>/dev/null | jq -r --arg t "$t" 'select(.executable != null and .target.name == $t) | .executable')`. Check `bin` is non-empty and that `"$bin" --list | grep -c ': test$'` matches that target's line count in `C-after.txt` (plus 2 for `integration`, the guard tests). Then from `crates/C/` run it three times as `RUSTC_BOOTSTRAP=1 "$bin" -Z unstable-options --shuffle` and once as `"$bin" --test-threads 1`. All four green; record the shuffle seeds. (Running the binary directly keeps `RUSTC_BOOTSTRAP` out of cargo's fingerprint, so nothing rebuilds.)
- **Doc tests unchanged:** `cargo test --workspace --doc` reports the same 5 passed before and after.

---

### Task 1: Pilot, refine (27 files), plus the guard test

**Why refine first:** it has the most files (27 of 86, about a third of the links removed), its test targets sit off the build's tail so the pilot cannot lengthen the critical path, it is rebuilt by every core edit, and it is the crate where cross-file state sharing was thought most likely, which the reading above did not confirm. It has one helper module and no file snapshots, so the pilot isolates build time and test correctness from insta mechanics. If even this best case misses the bar, the rest stops.

**Files:**
- Create: `crates/core/tests/support/test_files_declared.rs` (shared guard, included by path from each crate; not a test target of its own because it is in a subdirectory)
- Create: `crates/refine/tests/main.rs`
- Modify: `crates/refine/Cargo.toml` (`autotests = false`, one `[[test]]`)
- Modify: `crates/refine/tests/session_test.rs:2,16` (drop `mod helpers;`; `use helpers::*;` becomes `use crate::helpers::*;`)
- Modify: `process-docs/skills/codebase-layout.md:195` and `docs/contributing/developer-guide.md:180` (the layout sentence, below)
- Create: `process-docs/skills/integration-test-layout.md`; Modify: `process-docs/skills/index.md` (one row)

**Interfaces:**
- Produces: `fn undeclared_test_files(files: &[&str], main_rs: &str, manifest: &str) -> Vec<String>` in `test_files_declared.rs`, returning the stems of `tests/*.rs` files (excluding `main`) that have neither a `mod <stem>;` line in `main_rs` nor a `path = "tests/<stem>.rs"` entry in `manifest`, sorted. Plus `#[test] fn every_test_file_is_compiled()`, which reads `env!("CARGO_MANIFEST_DIR")/tests/*.rs` (top level only), `tests/main.rs` and `Cargo.toml` at run time, asserts the file list is non-empty, and asserts `undeclared_test_files` is empty, naming the files and the fix ("add `mod <stem>;` to tests/main.rs") in the message.
- Produces: each crate's `tests/main.rs` includes it as `#[path = "../../core/tests/support/test_files_declared.rs"] mod test_files_declared;`.

- [ ] **Step 1: Take the before inventory and baseline numbers.** Inventory procedure, "before", for refine (expect 351 tests). In worktrees A and B (both at main), the go/no-go set: 5 core-edit gate samples per side interleaved, plus 3 crate-scoped build samples per side for the lib probe and for the test-file probe in `crates/refine/tests/session_test.rs`, plus 3 crate-scoped runs. A and B at the same commit give the noise floor N and tolerance T for each metric (Measurement procedure); record them. If the core-edit N is 4 s or more, retake the core-edit set once; if it is still 4 s or more, report the table to the orchestrator and stop, because the 4 to 8 s band below cannot be read on that noise.

- [ ] **Step 2: Write the guard's table test first**, in `test_files_declared.rs`:

```rust
#[test]
fn undeclared_test_files_finds_only_the_stray() {
    let main_rs = "mod a_test;\n#[path = \"x.rs\"] mod helper;\n";
    let manifest = "[[test]]\nname = \"b_test\"\npath = \"tests/b_test.rs\"\n";
    let cases: &[(&[&str], &[&str])] = &[
        (&["a_test", "main"], &[]),
        (&["a_test", "b_test", "main"], &[]),
        (&["a_test", "stray_test", "main"], &["stray_test"]),
        (&["a_test_two", "main"], &["a_test_two"]),
    ];
    for (files, want) in cases {
        assert_eq!(undeclared_test_files(files, main_rs, manifest), *want, "{files:?}");
    }
}
```

The fourth case pins whole-word matching: `mod a_test;` must not declare `a_test_two`.

- [ ] **Step 3: Write `crates/refine/tests/main.rs` and the manifest change; run it to see it fail.** `main.rs`: a `//!` doc comment saying this is the crate's only integration test binary and every file in `tests/` must be declared here; `mod helpers;`; the guard module; then the 27 `mod <stem>;` lines. Leave `session_test.rs` unedited for now. Run `cargo test -p inspectah-refine --test integration`. Expected: compile error `file not found for module helpers` from `session_test.rs`.

- [ ] **Step 4: Fix `session_test.rs`** (two lines, as listed under Files). Run `cargo test -p inspectah-refine --test integration`. Expected: all pass, including both guard tests.

- [ ] **Step 5: Prove the guard bites.** `touch crates/refine/tests/zz_stray_test.rs`, run `cargo test -p inspectah-refine --test integration test_files_declared`, expect `every_test_file_is_compiled` to FAIL naming `zz_stray_test`; delete the file and re-run to PASS.

- [ ] **Step 6: Inventory "after", diff, shuffle runs, doc tests.** All as in the inventory procedure. Expected: empty diff, 351 tests, four green runs, 5 doc tests.

- [ ] **Step 7: Docs.** `codebase-layout.md:195` row becomes: `crates/*/tests/` | Per-crate integration tests, built as one binary per crate: `tests/main.rs` declares each file as a module (`autotests = false`); run one file with `cargo test -p <package> --test integration <file stem>::`. (Until Task 5 lands, say "refine so far; other crates one binary per file".) Same sentence in `developer-guide.md:180`. New skill `integration-test-layout.md`: a new test file goes in `tests/main.rs` or it never compiles (the guard catches it); helper modules are declared once in `main.rs` and imported with `use crate::<mod>`, because a second declaration trips `clippy::duplicate_mod`; insta file snapshots are named `integration__<stem>__<name>.snap`, and a renamed snapshot's header needs no edit, because its `source:` line names the test's source file, which does not move; a crate that keeps a second `[[test]]` target runs it with `--test <target>`, declares that target's helpers in the target file itself, and lists it in the manifest so the guard accepts it; why `autotests = false` was chosen over a `tests/<name>/main.rs` directory. Add its row to `process-docs/skills/index.md`.

- [ ] **Step 8: Stage, docs-only check, commit.** Stage every new and changed file by explicit path first (the script reads `git ls-files`, so the new `tests/main.rs` and guard file must be in the index). Then `scripts/tests/docs-only-paths-test.sh` passes. Commit through the hook: `test(refine): build the integration tests as one binary`. The body says why (the gate's link count) and carries the inventory count.

- [ ] **Step 9: After numbers and the go/no-go.** Same sample set as Step 1, A at main, B at the pilot commit, interleaved. Report to the orchestrator with the table, then stop.

**Go/no-go bar (all on medians, A vs B, with "faster", "no slower" and T as defined in the Measurement procedure, read only after Step 1 found the core-edit N under 4 s):**
- **GO** when every one holds: the core-edit hook is faster by at least 8 s; the refine lib-probe `--no-run` is at least 40% faster; the refine test-file-probe `--no-run` is no more than 10 s slower than today's single-file rebuild; the crate-scoped run is no slower (within its T); inventory and shuffle runs clean.
- **STOP** when any one holds: the core-edit hook is faster by under 4 s; the test-file probe is more than 10 s slower; any test fails for a reason other than a module path. The pilot branch is then not landed; the worktree is left for Mark to discard.
- **Any other outcome** (the core-edit gain between 4 and 8 s, a lib-probe gain under 40%, a run slower beyond its T, or any mix that is neither GO nor STOP): Mark decides from the table.

The 8 s line is the pilot's share of the estimate: 27 of about 86 links, against a test build of about 75 s, is at most about 23 s if link count were the whole cost; half of that, after the contention-only argument under Background, is about 11 s. 4 s is about the noise seen between identical 11-20 samples (119 to 126 s), which is why the bar is read only when this run's own same-commit gap is under it.

### Task 2: pipeline (19 files) and core (7 files)

Off-tail crates with no file snapshots, one dispatch, two commits, pipeline first.

**Files:**
- Modify: `crates/pipeline/Cargo.toml`, `crates/core/Cargo.toml` (as in Layout decision)
- Create: `crates/pipeline/tests/main.rs` (declares `mod selinux_support;` once, the guard, 19 `mod` lines), `crates/core/tests/main.rs` (guard, 7 `mod` lines)
- Modify: `crates/pipeline/tests/smoke_render.rs:531,534` and `crates/pipeline/tests/selinux_block_shell_test.rs:44,48`: drop `mod selinux_support;`; `use selinux_support as sel;` becomes `use crate::selinux_support as sel;`
- Modify: the layout sentence in `codebase-layout.md:195` and `developer-guide.md:180` (crate list)

**Interfaces:** Consumes `test_files_declared.rs` from Task 1.

- [ ] **Step 1:** Before inventory for pipeline (353) and core (212); `cargo test --workspace --doc` (5).
- [ ] **Step 2:** pipeline: manifest, `main.rs`, run `cargo test -p inspectah-pipeline --test integration`; expect the `selinux_support` file-not-found error; apply the two-file fix; run again, expect PASS.
- [ ] **Step 3:** Inventory after, diff empty, three shuffled runs and one single-threaded run green; stage by explicit path, then the docs-only check passes. Commit `test(pipeline): build the integration tests as one binary`.
- [ ] **Step 4:** core: manifest and `main.rs`; `cargo test -p inspectah-core --test integration` PASS (no helper edits expected); inventory, shuffle; stage, then the docs-only check. Commit `test(core): build the integration tests as one binary`. Doc tests still 5.

### Task 3: collect (16 files, 3 snapshots)

**Files:**
- Modify: `crates/collect/Cargo.toml`; Create: `crates/collect/tests/main.rs` (guard, 16 `mod` lines)
- Rename: `crates/collect/tests/snapshots/{kernelboot_test__kernelboot_snapshot,storage_test__storage_snapshot,units_test__services_snapshot}.snap` to the same names prefixed `integration__`
- Modify: the layout sentence (crate list)

- [ ] **Step 1:** Before inventory (221).
- [ ] **Step 2:** Manifest and `main.rs` only. Run `INSTA_UPDATE=no cargo test -p inspectah-collect --test integration`. Expected: exactly 3 failures, each a missing snapshot at `integration__<old name>`; this proves the new names before any rename.
- [ ] **Step 3:** `for f in crates/collect/tests/snapshots/*.snap; do git mv "$f" "crates/collect/tests/snapshots/integration__$(basename "$f")"; done`. Edit no snapshot.
- [ ] **Step 4:** `cargo insta test --check --test-runner cargo-test -p inspectah-collect --unreferenced=reject` (`--check` is required: without it cargo-insta 1.49.0 sets `INSTA_UPDATE=new` and `INSTA_FORCE_PASS=1` on the test process, overriding any `INSTA_UPDATE=no` in the environment). Expected: PASS, no unreferenced snapshot. `git status --porcelain crates/collect/tests/snapshots` shows only the three renames, no `.snap.new`.
- [ ] **Step 5:** `git diff --cached -M100% --name-status -- 'crates/collect/tests/snapshots/*.snap'` prints exactly 3 lines, every one `R100`. Paste it into the commit body.
- [ ] **Step 6:** Inventory after, shuffle runs; stage, then the docs-only check. Commit `test(collect): build the integration tests as one binary`.

### Task 4: web (7 files, 34 snapshots), a tail crate

**Files:**
- Modify: `crates/web/Cargo.toml`; Create: `crates/web/tests/main.rs` (guard, 7 `mod` lines; the two cross-crate `#[path]` modules stay inside `api_test.rs` and `system_tuning_fixtures_test.rs`, each used once in this crate)
- Rename: the 34 files in `crates/web/tests/snapshots/` to `integration__<name>`
- Modify: `crates/web/tests/fixture_structure_test.rs:10` and `crates/web/tests/system_tuning_fixtures_test.rs:11` (run commands in comments), `process-docs/skills/e2e-fixtures-are-hand-maintained.md:57,69,97` (`--test fixture_structure_test` and `--test system_tuning_fixtures_test` become `--test integration fixture_structure_test::` and `--test integration system_tuning_fixtures_test::`), `docs/contributing/developer-guide.md:287` (the example snapshot path becomes `crates/web/tests/snapshots/integration__contract_snapshots__contract_view.snap`), the layout sentence

- [ ] **Step 1:** Before inventory (194). Core-edit gate, 3 samples per side, with A and B both at the branch tip before this task's edits (the Task 3 commit): this is the same-commit set that gives web's N and T.
- [ ] **Step 2:** Manifest and `main.rs`; `INSTA_UPDATE=no cargo test -p inspectah-web --test integration` fails with exactly 34 missing snapshots.
- [ ] **Step 3:** Rename loop as Task 3 Step 3. `cargo insta test --check --test-runner cargo-test -p inspectah-web --unreferenced=reject` PASS. `git diff --cached -M100% --name-status` over the snapshots: 34 lines, all `R100`.
- [ ] **Step 4:** `UPDATE_SYSTEM_TUNING_FIXTURES=1 cargo test -p inspectah-web --test integration system_tuning_fixtures_test::` then `git status --porcelain crates/web/ui/src/__tests__/fixtures` is empty (the regeneration path still finds its files and writes identical bodies).
- [ ] **Step 5:** Inventory after, shuffle runs, docs. Stage every changed, new and renamed path by explicit path, then the docs-only check.
- [ ] **Step 6:** With everything staged (Measurement procedure, Gateable state), core-edit gate, 3 samples per side, A at the Task 3 commit. If the one-binary layout is no slower than A, it is selected. If it is slower, build the two-binary variant (`[[test]] name = "api_test", path = "tests/api_test.rs"` plus `integration` without it; `main.rs` drops `mod api_test;`; `api_test.rs` compiles alone unchanged, since its cross-crate helper is declared inside it), stage it, and take the same samples; select it if it is no slower than A. Run the inventory and order procedures with targets `integration api_test` for that variant. If neither variant is no slower than A, stop: web stays one binary per file, unstage and restore its changes, record both tables in the report, and continue with Task 5 on the unchanged tip.
- [ ] **Step 7:** Commit the selected variant as `test(web): build the integration tests as one binary` (or `as two binaries`), with the gate tables in the body and the variant not taken named with its numbers. If the split was taken, the layout sentence and `integration-test-layout.md` name `cargo test -p inspectah-web --test api_test` as web's exception.

### Task 5: cli (10 files), the tail crate and the leaf gate

**Files:**
- Modify: `crates/cli/Cargo.toml`; Create: `crates/cli/tests/main.rs` (declares `#[path = "support/unit_hosts.rs"] mod unit_hosts;` and `#[path = "support/sysctl_hosts.rs"] mod sysctl_hosts;` once each, the guard, 10 `mod` lines)
- Modify: `crates/cli/tests/{fleet_repo_precedence_test,staged_writes_parity_test,units_scan_test}.rs` (drop the `#[path]` and `mod unit_hosts;` lines; `use unit_hosts::…` becomes `use crate::unit_hosts::…`; add `use crate::unit_hosts;` where bare `unit_hosts::` paths remain) and `crates/cli/tests/{aggregate_host_labels_test,sysctl_scan_test}.rs` (same for `sysctl_hosts`)
- Modify: the layout sentence (now "every crate")

- [ ] **Step 1:** Before inventory (298). Leaf-edit and core-edit gates, 3 samples per side, with A and B both at the branch tip before this task's edits (the Task 4 commit, or the Task 3 commit if web did not land): the same-commit set that gives cli's N and T for both gates. A stays at that commit for Step 4.
- [ ] **Step 2:** Manifest and `main.rs` without the helper edits; run `cargo clippy -p inspectah-cli --all-targets -- -D clippy::all`. Expected: `duplicate_mod` errors for `unit_hosts` and `sysctl_hosts` (this proves the hazard). Apply the five helper edits; clippy clean; `cargo test -p inspectah-cli --test integration` PASS. The web crate's `#[path]` to these files still compiles: `cargo test -p inspectah-web --tests` PASS.
- [ ] **Step 3:** Inventory after, shuffle runs (watch `units_scan_test::` and `refine_e2e_test::`, the heaviest and the one with a live listener). Stage every changed and new path by explicit path, then the docs-only check.
- [ ] **Step 4:** With the one-binary layout staged, leaf-edit and core-edit gates, 3 samples per side against A. Then build the two-binary variant: `[[test]] name = "units_scan_test", path = "tests/units_scan_test.rs"` plus `integration` without it; `main.rs` drops `mod units_scan_test;`; `units_scan_test.rs` gets back `#[path = "support/unit_hosts.rs"] mod unit_hosts;` at its top, because as its own crate root it has no `crate::unit_hosts` otherwise (its `use crate::unit_hosts::…` lines then resolve against its own root, and the second load is in a different crate, so no `duplicate_mod`); the guard accepts it through the manifest entry. Stage it, check clippy, run the inventory and order procedures with targets `integration units_scan_test`, and take the same samples. Selection: the split is preferred only if its leaf-edit median is at least 3 s lower than the one-binary layout's and its core-edit median is no higher than the one-binary layout's plus T; otherwise the one-binary layout is preferred. The preferred variant lands only if it is no slower than A on both the leaf-edit and the core-edit gate; if it fails and the other variant passes both, the other lands. If neither passes, stop: cli stays one binary per file, unstage and restore its changes, and record both tables in the report.
- [ ] **Step 5:** Commit the landed variant as `test(cli): build the integration tests as one binary` (or `as two binaries`), gate tables in the body, the variant not taken named with its numbers. Update the layout sentence (now "every crate", or naming any crate that stayed one binary per file); if the split was taken, it and `integration-test-layout.md` name `cargo test -p inspectah-cli --test units_scan_test` as cli's exception.

### Task 6: Final measurement and the gate skill

**Files:**
- Modify: `process-docs/skills/full-workspace-verification-gates.md` § 3 (new dated table after the 2026-11-20 one)

- [ ] **Step 1:** B at the tip after Task 5. A is a baseline worktree at the same tip with the step-7 commits reverted (`git revert --no-edit` on each, newest first), so commits that landed on main between the pilot and now sit in both sides. A's reverts are never landed; the worktree is cleaned up after. Core-edit and leaf-edit gates, 5 samples per side, interleaved, loads recorded. Also `cargo test --workspace` warm run wall, and the total integration-test count across crates (1,629 static `#[test]` functions at `da9a746c`, plus the 2 guard tests in each merged `integration` binary) and test binaries before and after.
- [ ] **Step 2:** Add the table to § 3 with the date, toolchain, load, and one sentence comparing the core-edit saving to the 2026-11-19 estimate ("most of the ~62 s test build", that is at least 31 s). If the saving is under half that, say so plainly and say which half of the estimate failed (build or start-up).
- [ ] **Step 3:** Docs-only commit `docs(skills): record the one-binary-per-crate gate timings`.

## Execution under the worktree procedure

- **Dispatch 1 (Task 1):** the pilot, in its own worktree (`test/step7-refine`) plus a baseline worktree. It ends at the go/no-go report. The orchestrator lands it only on GO, or on Mark's call in the middle band.
- **Dispatch 2 (Tasks 2 and 3):** pipeline, core, collect in one worktree, three commits, one hook run each. Batched because they are off the tail, the recipe is proven by then, and none needs a gate measurement to decide landing.
- **Dispatch 3 (Tasks 4, 5 and 6):** web, cli and the final measurement, one worktree plus a baseline worktree that moves forward: at the Task 3 commit for Task 4, at the branch tip before Task 5's edits for Task 5, and at the Task 6 revert baseline last. Batched because both tail crates use the same procedure and land condition, and Task 6 needs both decided in the branch.
- The orchestrator lands each dispatch with `inspectah-worktree land`, then `cleanup`, and cleans up each baseline worktree. Each dispatch gets its own scratch subdirectory for inventories and logs. A review seat per the usual review routing reviews each dispatch's commits before landing; the reviewer checks the `R100` lists, the inventory diffs, and the measurement tables.

## Docs and skills that change (no CHANGELOG)

| file | change | task |
|---|---|---|
| `process-docs/skills/codebase-layout.md:195` | layout sentence and how to run one file | 1, then crate list in 2 to 5 |
| `docs/contributing/developer-guide.md:180` | same sentence | 1 to 5 |
| `docs/contributing/developer-guide.md:287` | renamed example snapshot | 4 |
| `process-docs/skills/e2e-fixtures-are-hand-maintained.md:57,69,97` | `--test <stem>` commands | 4 |
| `crates/web/tests/fixture_structure_test.rs:10`, `crates/web/tests/system_tuning_fixtures_test.rs:11` | run commands in comments | 4 |
| `process-docs/skills/integration-test-layout.md` (new) and `process-docs/skills/index.md` | the layout rules | 1 |
| `process-docs/skills/full-workspace-verification-gates.md` § 3 | final timings | 6 |

Unchanged on purpose: the 30-odd skill and doc references to `crates/*/tests/<file>.rs` paths (the files do not move), the docs-only allowlist, `docs/contributing/testing.md` (its commands are workspace-wide), CI (`cargo test --workspace` is unchanged), and the hook.

## Strongest case against

- **The saving may be mostly CPU contention, not critical path.** The 11-19 cold-build timings show the web and cli test targets at the tail and the others overlapping library builds. If the core-edit gate's wall is set by that tail, merging off-tail crates only frees cores, and the gain could be well under the estimate. The pilot measures exactly this on the crate with the most to gain.
- **Every test-file edit gets dearer.** With incremental builds off, touching one refine test recompiles all 16,383 lines of refine's tests instead of one file. For test-heavy development that cost lands on every iteration, not only on commits. The pilot bar caps it at 10 s, but it is a real trade.
- **A guard replaces a convention.** `autotests = false` turns "drop a file in `tests/`" into "drop a file and declare it". The guard test makes a missed declaration fail the gate, but a contributor will meet that failure once before learning the rule.

**Evidence that would change this plan:** a pilot core-edit saving under 4 s (stop step 7 and record it); a test-file probe more than 10 s slower (stop, or merge only the crates whose tests are rarely edited); a fixed incremental toolchain that lets `incremental = false` go (re-measure first, since incremental changes the per-edit compile cost the plan trades against); or a `--timings` run of the current core-edit build showing the tail is not the web and cli tests (then the tail-crate land conditions in Tasks 4 and 5 can relax).
