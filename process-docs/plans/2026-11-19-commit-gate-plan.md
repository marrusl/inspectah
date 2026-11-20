# Commit gate under concurrent agents: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task by task. Steps use checkbox (`- [ ]`) syntax for tracking. Every code task is test-first: write the failing test the task names, run it and see it fail for the stated reason, then implement.

**Goal:** make every commit on `main` tested on exactly its own content, cheaply enough to run per commit, and give each code-writing agent its own git worktree with a warmed build.

**Architecture:** three cargo-side fixes first (Tasks 1 and 2), then a reworked `.githooks/pre-commit` (Task 3) that gates on an inverted docs-only list held in one tracked file, refuses when anything outside that list differs from the index being committed, and prints one line per gate unless a gate fails. A worktree script in the private orchestration repo (Task 4) creates, lands and cleans up one worktree per code dispatch, classifying paths with the docs-only file read from `main`. The orchestrator's instructions change to route code dispatches through it (Task 5). Task 6 measures the result.

**Tech stack:** Rust build script and Cargo profiles; bash 3.2 (macOS `/bin/bash`) for the hook, the script and every test, with `perl` for source scanning; git 2.x worktrees. No new dependency.

**Spec:** the approved recommendation "inspectah commit gate under concurrent agents" (2026-11-19), its § 8 implementation outline (approved as written by Mark, 2026-11-19), with § 4 (hook), § 5 (cargo) and § 6 (worktree lifecycle) as the detail, and its two input reports (gate timing; gate design). The recommendation is held privately; the orchestrator attaches it to every dispatch of this plan. Section numbers (§) below are the recommendation's.

**Where each task runs.** Tasks 1 to 3 run in the shared inspectah checkout under today's rules: one code writer, commit with `git commit --only <paths> -F <file>` (after `git add` on any new file: `--only` refuses a path git does not know), no cargo through a pipe, never `--no-verify`. Task 4 runs in the private orchestration repo, at the absolute path the orchestrator gives. Task 5 is applied by the orchestrator, not an implementer. Task 6 runs in a worktree made by Task 4's script. Task 3's commit is gated by the new hook itself (the hook runs from the working tree), so the shared checkout must have no non-docs edits but Task 3's when it commits.

**`$SCRATCH`.** Verification commands write logs to `$SCRATCH`: a directory outside the repo, either the scratch directory the dispatch brief names or one made with `mktemp -d`. Shell variables do not persist between tool calls, so set it at the start of the same command (`SCRATCH=<dir>; ...`) or write the path literally.

## Global Constraints

- Every commit on `main` is green on its own, not only the pushed tip (Mark, 2026-11-19).
- One code writer at a time; docs writers work alongside in the shared checkout (Mark, 2026-11-19).
- No stash in the hook (Mark, 2026-11-19).
- Option C: one git worktree per code-writing dispatch, its own `target/`, a background warm-up build at creation (Mark, 2026-11-19).
- Worktrees live at `inspectah/.worktrees/<branch>`; the script lives in the private orchestration repo's `workflow/` (Mark, 2026-11-19).
- Never share a `CARGO_TARGET_DIR` between checkouts: on cargo 1.100 a second checkout ran the first checkout's test code (measured 2026-11-19).
- Worktree cleanup uses `git worktree remove` without `--force`, then `git branch -d`; never `rm`.
- Keep `incremental = false` in the `.cargo/config.toml` above the repo; a worktree under `inspectah/.worktrees/` picks it up from there.
- The clippy gate is exactly `cargo clippy --workspace --all-targets -- -D clippy::all`.
- The docs-only list, verbatim from § 4, one pattern per line: `^docs/`, `^process-docs/`, `^\.github/`, `^packaging/`, `^\.copr/`, `^completions/`, `^[A-Za-z_]+\.md$`, `^LICENSE$`.
- One commit per task at most, each through the existing hook. Step 1 lands before the worktree script is used.
- Shell code is bash 3.2: no `mapfile`, no associative arrays, no `${var^^}`. Path lists are read with `$(...)` assignment under `set -e`, never from a process substitution (which hides the producer's failure), and matched with a here-string, never a pipe into `grep -q`.
- Public text in this repo names no AI team member and no private repo. Voice: no em dashes.

## Review Focus

1. **`git rebase --exec .githooks/pre-commit` gates nothing as § 6 wrote it.** Inside `rebase --exec` the index equals `HEAD`, so the staged list is empty and the fast path exits 0, in today's hook and in an inverted one alike. The plan adds a `--head` mode that gates the paths `HEAD` changed (Task 3), and the land step uses it (Task 4). Tests: `head mode gates a code commit`, `land re-gates every commit after a code commit on main`, which checks the gated hashes and directory.
2. **`git commit --only` with code staged in the real index.** The hook must see the temporary index git builds, or a staged code path left out of the commit reads as clean. Test: `--only refuses a code path staged in the real index but left out of the commit`, which must assert the refused path by name (Task 3).
3. **Deletions and renames.** Today's fast path filters `ACMR`, so a commit that only deletes `.rs` files skips every gate; a rename from a code path to a docs path lists only the docs side. Tests: `a deletion-only code commit is gated`, `a rename out of a code path is gated` (Task 3).
4. **A stray scratch file in the worktree.** cargo auto-discovers `tests/*.rs` and `refine`'s `testdata_tarballs_load` reads every `testdata/*.tar.gz`, so an untracked file can make a gate pass on content the commit leaves out. It must be refused by name, before fmt and again after test. Tests: `an untracked code path is refused before fmt`, `an untracked file created during the test gate is refused after it`, `a tracked code file edited during the test gate is refused after it` (Task 3).
5. **The docs-only read check's coverage.** It reads every string literal in `crates/**/*.rs`, so its literal extraction (comments, raw strings, escapes) decides what it sees; the allowlist matches file and literal pairs, not line numbers, and a stale entry fails. What it does not cover is written in the test's header (Task 3 Step 3).
6. **Cleanup while the warm-up still builds, or after a dispatch died.** `git worktree remove` racing a cargo process writing `target/` can fail half-way. Tests: `cleanup refuses while the warm-up runs`, `cleanup refuses an unlanded branch` (Task 4).

---

### Task 1: Embed the git commit in release builds only

**Files:**
- Modify: `crates/cli/build.rs`
- Modify: `crates/cli/src/commands/version.rs` (tests module)
- Modify: `process-docs/skills/build-rs-version-metadata.md`

**Interfaces:**
- Produces: in any profile that does not inherit from `release`, `INSPECTAH_COMMIT` and `INSPECTAH_DATE` are both the literal `dev`, and the build script emits only `cargo:rerun-if-changed=build.rs`. In release builds both values and both `.git` rerun lines are unchanged.

What consumes the values, checked 2026-11-19, and what changes for each:

| Consumer | Change |
|---|---|
| `crates/cli/src/main.rs` `LONG_VERSION` (`--version`) | debug builds print `0.9.0-beta.5 (commit dev, built dev)`; release unchanged |
| `crates/cli/src/commands/version.rs` `print_version` (`inspectah version`) | same |
| `version.rs` test `compile_time_vars_are_set` | still passes: it asserts non-empty and not `unknown` |
| Snapshot and integration tests | none read either value (grep of `crates/` for both names and for `(commit `) |
| `build-binary.yml`, `packaging/inspectah.spec`, `cargo install` | all build `--release` (or inherit from release), so `PROFILE=release` and the hash is embedded as today |
| `rust-ci.yml` e2e step | builds a debug binary and runs `refine`; never reads the version |
| Homebrew formula test | asserts the package version in `inspectah version` output; unaffected |
| `docs/getting-started.md` "Verify the install" | documents a `--release` build; unchanged |

Pre-existing, not changed here: the RPM is built from a `git archive` tarball with no `.git`, so RPM binaries already report `commit unknown`.

**Date choice.** The date becomes `dev` too in non-release builds. A real date would need the build script to rerun on every build, or go stale for the life of the `target/`, and both defeat the fix.

- [ ] **Step 1: Write the failing test** in the `version.rs` tests module:

```rust
#[cfg(debug_assertions)]
#[test]
fn dev_builds_carry_fixed_version_metadata() {
    assert_eq!(env!("INSPECTAH_COMMIT"), "dev");
    assert_eq!(env!("INSPECTAH_DATE"), "dev");
}
```

`debug_assertions` stands in for "not a release profile"; no profile in this workspace sets them otherwise.

- [ ] **Step 2: Run it and see it fail**

Run: `cargo test -p inspectah-cli --bin inspectah dev_builds_carry_fixed_version_metadata`
Expected: FAIL, left is the short hash, right is `"dev"`.

- [ ] **Step 3: Implement in `crates/cli/build.rs`**: read `PROFILE` with `std::env::var("PROFILE")`; when it is `release`, keep today's body as it is; otherwise emit both env lines with `dev` and `cargo:rerun-if-changed=build.rs`, and run neither `git` nor `date`. Name the fixed value as a constant (`DEV_METADATA`). Comment why: in a linked worktree `.git` is a file, so `../../.git/HEAD` never exists and cli rebuilt on every cargo call; in the shared checkout every commit moved `.git/refs` and rebuilt cli once per commit (~17 to 20 s, measured 2026-11-19).

- [ ] **Step 4: Run it and see it pass**, plus the old test

Run: `cargo test -p inspectah-cli --bin inspectah version`
Expected: PASS, 2 tests.

- [ ] **Step 5: Update `process-docs/skills/build-rs-version-metadata.md`**: the rerun lines apply to release builds only; debug builds carry `dev` for both values and why; the date is `dev` too.

- [ ] **Step 6: Commit** `crates/cli/build.rs crates/cli/src/commands/version.rs process-docs/skills/build-rs-version-metadata.md` with `--only`, as `perf(cli): embed the git commit in release builds only`. No CHANGELOG entry: a debug build's version text is not user-visible behavior under `process-docs/changelog-policy.md`.

- [ ] **Step 7: Verify cli no longer rebuilds after a commit.** Step 6's commit moved `HEAD`, which used to dirty cli.

Run: `SCRATCH=<dir>; cargo test -p inspectah-cli --no-run -v > "$SCRATCH/t1.log" 2>&1; grep -E '(Dirty|Fresh) inspectah-cli' "$SCRATCH/t1.log"`
Expected: `Fresh inspectah-cli`. A `Dirty inspectah-cli` line can also come from another session's uncommitted cli edits in the shared checkout; read its reason. It must not name a `.git` path. If it names a source file, the check passed and the report says which file.

- [ ] **Step 8: Verify the release path**

Run: `nice -n 10 cargo build --release -p inspectah-cli`, then `target/release/inspectah version`
Expected: `(commit <short hash of HEAD>, built <today, UTC>)`.

---

### Task 2: Optimise third-party dependencies in the dev profile

**Files:**
- Modify: `Cargo.toml` (workspace root)

**Interfaces:**
- Produces: `[profile.dev.package."*"] opt-level = 2`. Applies to every non-workspace package in `dev` and in `test` (which inherits from `dev`). Workspace crates stay at `opt-level = 0`. `debug-assertions` and `overflow-checks` are not overridden and stay on for dependencies. A `package."*"` override takes precedence over `build-override` (Cargo profile reference, § Overrides: first match wins, `package.<name>`, then `package."*"`, then `build-override`, then the profile), so non-workspace build scripts, proc macros and their dependencies also build at `opt-level = 2`. The workspace's own build scripts (`crates/cli`, `crates/web`) are workspace members, so `package."*"` skips them and they follow the default `build-override`, `opt-level = 0`.

**CI.** Nothing to change. `rust-ci.yml` runs clippy, `cargo test --workspace` and `cargo build -p inspectah-cli` in the default profiles, with no `--profile`, `--release` or `CARGO_PROFILE_*` override (checked 2026-11-20), so it applies the setting from `Cargo.toml`. `Swatinem/rust-cache@v2` builds its key from the member packages' manifests and `Cargo.lock`, not from the virtual root manifest, so this change does not change the cache key; cargo rebuilds the dependencies a restored target holds at the old profile. `build-binary.yml` and the RPM build `--release` and are unaffected.

- [ ] **Step 1: Add the section** with a comment: what it buys (reported test time 37 s to 19 s; `units_scan_test` 17 s to 6 s; measured 2026-11-19 at load 42 to 71, so treat it as an upper bound), that it costs one dependency rebuild, and that workspace crates stay unoptimised for debugging.

- [ ] **Step 2: Verify the profile applies to dependencies only.** Run this directly after Step 1, before any other cargo command in the shared checkout: once something has rebuilt the dependencies under the new profile, they print `Fresh` and carry no flags.

Run: `SCRATCH=<dir>; cargo build -v -p inspectah-core > "$SCRATCH/t2.log" 2>&1; grep -c 'opt-level=2' "$SCRATCH/t2.log"; grep -E 'Running .*--crate-name inspectah_core ' "$SCRATCH/t2.log"`
Expected: a non-zero count; the `inspectah_core` line carries no `opt-level=2`. If the count is 0 and the log shows the dependencies `Fresh`, another cargo run got there first: run the same command again with `--target-dir "$SCRATCH/t2-target"` added, which builds from cold, and read that log. Leave the scratch target where it is.

- [ ] **Step 3: Commit** `Cargo.toml` with `--only` as `perf(build): optimise dependencies in dev and test builds`. The hook runs the full suite on the new profile, which is the check that no test depends on unoptimised dependencies. After Mark pushes, report the first two `tier1` runs: both green, with their durations. If both compile the dependencies in full, report it as a follow-up for Mark: the cache key does not change with the profile, so a restored cache may keep holding the old build.

---

### Task 3: Rework the pre-commit hook

**Files:**
- Create: `.githooks/docs-only-paths`
- Modify: `.githooks/pre-commit`
- Create: `scripts/tests/pre-commit-hook-test.sh`
- Create: `scripts/tests/docs-only-paths-test.sh`
- Modify: `scripts/tests/check-sensitive-tarballs-test.sh` (its hook section)
- Modify: `.github/workflows/rust-ci.yml` (two steps in the `sensitive-tarballs` job)
- Modify, prose only: `CLAUDE.md` (Pre-commit hook line), `README.md` (§ Contributing, hook list), `docs/contributing/developer-guide.md` (§ What rejects a commit), `docs/contributing/testing.md` (§ The hook's trigger), `process-docs/skills/full-workspace-verification-gates.md` (§ 1), `process-docs/skills/sensitive-tarball-gate.md` (§ Ordering in the hook), `process-docs/skills/e2e-fixtures-are-hand-maintained.md` (the fixture-edit paragraph), `process-docs/skills/release-process.md` (both `INSPECTAH_SKIP_UI` passages)

**Interfaces:**
- Produces `.githooks/docs-only-paths`: one anchored ERE per line, the eight patterns from Global Constraints in that order. Lines that are blank or start with `#` are ignored. Every consumer loads it the same way: drop ignored lines, join the rest with `|`, match with `grep -E`. The hook reads it from the index being committed (`git show :.githooks/docs-only-paths`), and in `--head` mode from `HEAD` (`git show HEAD:.githooks/docs-only-paths`; under `rebase --exec` the index equals `HEAD` anyway), so the list being committed is the list applied. A missing or unreadable file refuses the commit. Task 4 reads it from `main:.githooks/docs-only-paths`; the docs-only read check reads the checkout's file.
- Produces `.githooks/pre-commit [--head]`. Git calls it with no argument. `--head` gates the paths `HEAD` changed (`git diff-tree -r --no-commit-id --name-only --no-renames --root HEAD`) instead of the staged paths, and runs gate 0 as `--tracked`; it exists for `git rebase --exec '.githooks/pre-commit --head'`. Any other argument prints `pre-commit: usage: pre-commit [--head]`. Exit 0 pass or skipped, 1 for everything else: a refusal, a failed gate, a usage error, and every non-zero status of gate 0's helper (which itself exits 1, 2 for unreadable input or 64 for usage or a missing `jq`). The hook maps those to 1 after the helper's own output.
- Produces these output lines, which tests and Task 6 match on: `pre-commit: <gate> ok (<n> s)` and `pre-commit: <gate> FAILED (<n> s)` for gates `fmt`, `clippy`, `test`; `pre-commit: docs-only, cargo gates skipped`; refusals start `pre-commit: commit refused:`.

**Hook order**, all under `set -euo pipefail`:

1. Parse the argument. `unset INSPECTAH_SKIP_UI`, so `rust-embed` never embeds a stale `crates/web/ui/dist/`.
2. Gate 0: `scripts/check-sensitive-tarballs.sh --staged` (`--tracked` in `--head` mode), run inside an `if`, so its status is caught rather than ending the hook through `set -e`. On a non-zero status `<n>`, print `pre-commit: commit refused: sensitive-tarball check failed (exit <n>)` and exit 1. It stays above the fast path.
3. Load the docs-only list. On failure, print `pre-commit: commit refused: cannot read .githooks/docs-only-paths from the index` (`from HEAD` in `--head` mode) and exit 1.
4. Path list: `git diff --cached --name-only --no-renames` (no `--diff-filter`: deletions count), or the `--head` list.
5. Fast path: if every path matches the list, print the docs-only line and exit 0. An empty list counts as docs-only (an `--allow-empty` or message-only amend changes no content). The fast path comes before the refuse check on purpose: a docs commit in the shared checkout is never blocked by a code writer's or another session's unstaged and untracked files.
6. Refuse check, first pass: every path in `git diff --name-only` and in `git ls-files --others --exclude-standard` that fails the list. If any, refuse, naming every such path and saying "commit it, delete it, or ignore it; docs-only paths are exempt". Then record `git write-tree`. Both commands read `GIT_INDEX_FILE` when git sets it, so under `git commit --only` and `-a` they compare against the index being committed. The hook never unsets or overrides it. The check runs in `--head` mode too, where it proves the tree being tested is `HEAD`.
7. Gates `fmt` (`cargo fmt --check`), `clippy` (the Global Constraints command), `test` (`cargo test --workspace`). Each one's output goes to its own file in a `mktemp -d` directory removed by a `trap`; print one line per gate with `SECONDS`-based elapsed time; on failure print the file, then today's banner for that gate, and exit 1.
8. Refuse check, second pass, same rule; then refuse if `git write-tree` differs from step 6's, with "the index changed while the gate ran".
9. Print `pre-commit: all checks passed.`

Rewrite the opening comment to describe this order, the docs-only file, `--head`, the exit statuses, and that the gate tests the index being committed by refusing when the tree differs from it.

**Frontend gate: out of scope.** § 8 step 3 does not list a vitest or `tsc --noEmit` branch. With the inverted list a UI commit now runs the cargo gates, and `crates/web/build.rs` runs `tsc && vite build` inside them, so type errors block; vitest stays CI-only.

**`INSPECTAH_SKIP_UI` and the release commit.** `release-process.md` tells the release session to set `INSPECTAH_SKIP_UI=1` on `git commit` so the sandbox's missing network does not hang `npm ci`. The hook now ignores it. The same passage already says to run the gate and commit with the sandbox disabled for the TCP-listener test, so the rewrite drops the `INSPECTAH_SKIP_UI` advice for the commit and keeps "sandbox disabled". The "Never set it for release binaries" rule stays.

**Docs-only read check: scope.** § 4.1 asks that no `include_str!` or runtime path resolve under a docs-only prefix. A static check cannot see every runtime path, so `docs-only-paths-test.sh` checks this, and only this, over every tracked `crates/**/*.rs` file:

- every `include_str!` and `include_bytes!` argument resolves, from the source file's directory, to a path outside the list, and any argument that is not a plain string literal fails as unresolved;
- no string literal, normalized lexically with its leading `./` and `../` segments stripped, matches the list or names a docs-only directory on its own (`docs`, `process-docs`, `.github`, `packaging`, `.copr`, `completions`, with or without a trailing `/`), unless its file and literal pair is on the allowlist.

The second rule catches `repo_root().join("docs/input.json")`, `"../../README.md"` and `.join("..").join("docs")`. It does not catch a path assembled from fragments none of which names a docs-only path or directory on its own (`format!("{}s/x.md", "doc")`, or a directory walk from the repo root), a path read from the environment or from a file, or a read by a non-Rust build input (`crates/web/ui` sources and config, `crates/pipeline/templates`). Today none of those reaches a docs-only path: the one helper that resolves the repo root (`refine`'s `testdata_tarballs_load`) reads `testdata/`, and a grep of the non-Rust files under `crates/` for a `../` path into a docs-only location found none (2026-11-20). **This is narrower than § 4.1's "no runtime path"; accepted by Mark on 2026-11-20.**

The allowlist starts with seven pairs, all output file names or fixture data, none read from the repo (counted 2026-11-20: 15 literals on 13 lines in 7 files, 14 of them `README.md` and one `notes.md`; no literal matched any other pattern or a bare directory name):

| File | Literal | Why it is not a repo read |
|---|---|---|
| `crates/cli/tests/copy_source_exists_test.rs` | `README.md` | expected member of a rendered output tarball |
| `crates/cli/tests/units_scan_test.rs` | `README.md` | read from a `tempfile::TempDir` the test renders into |
| `crates/cli/tests/warning_markdown_test.rs` | `README.md` | name of the rendered output file under test |
| `crates/collect/src/inspectors/subscription.rs` | `notes.md` | fixture file name under a fake `/etc/pki/entitlement` |
| `crates/pipeline/src/render/mod.rs` | `README.md` | the renderer writes `README.md` into the output directory |
| `crates/pipeline/src/render/tarball.rs` | `README.md` | test writes it into a temporary source tree |
| `crates/pipeline/tests/e2e.rs` | `README.md` | expected member list of the rendered output |

- [ ] **Step 1: Create `.githooks/docs-only-paths`** with the eight patterns and a header comment naming its consumers. Today's hook does not read it, so nothing changes yet; the tests below seed their throwaway repos from it.

- [ ] **Step 2: Write `scripts/tests/pre-commit-hook-test.sh`** after the pattern of `check-sensitive-tarballs-test.sh`: bash 3.2, `set -euo pipefail`, a `mktemp -d` work area removed by `trap`, `ok`/`fail` counters, final `N passed, M failed` and a non-zero exit on any failure. Each case starts from a fresh throwaway repo (`git init`, local `user.name`, `user.email`, `commit.gpgsign false`, `core.hooksPath .githooks`) seeded with the repo's `.githooks/pre-commit`, `.githooks/docs-only-paths` and `scripts/check-sensitive-tarballs.sh`, plus `crates/a/src/lib.rs`, `crates/a/tests/t.rs`, `docs/guide.md` and `README.md`, committed `--no-verify`. A stub `cargo` first on `PATH` appends `"$* INSPECTAH_SKIP_UI=${INSPECTAH_SKIP_UI-unset}"` to `$STUB_LOG` and obeys: `STUB_FAIL=<subcommand>` exits 1 for that subcommand (`$1`) after printing `stub <subcommand> failed`; `STUB_NOISE=1` prints 500 lines; `STUB_DURING_TEST=<mode>:<path>` acts during `test`, where `untracked` creates the file, `add` creates it and runs `git add` on it, and `edit` appends a line to a tracked file without staging it. A refusal must carry `pre-commit: commit refused:`, so a crash never counts as a refusal. Where a case names a refused path, it asserts that path appears in the output.

| Case (test name) | Setup and command | Expected |
|---|---|---|
| docs-only commit skips the cargo gates | edit `docs/guide.md`, `git add`, `git commit` | accepted; stub log empty; output has the docs-only line |
| a docs-only commit is not blocked by dirty code | stage a docs edit; leave `crates/a/src/lib.rs` edited unstaged and `crates/a/src/new.rs` untracked; commit | accepted; docs-only line; stub log empty |
| an empty commit is docs-only | `git commit --allow-empty` | accepted; docs-only line; stub log empty |
| a code commit runs fmt, clippy and test in order | edit `crates/a/src/lib.rs`, add, commit | accepted; the stub log is exactly three lines, starting `fmt --check `, `clippy --workspace --all-targets -- -D clippy::all `, `test --workspace ` |
| a passing commit prints one line per gate | as above with `STUB_NOISE=1` | accepted; output has the three `ok` lines and no stub noise; under 10 lines |
| a failing gate prints its output | code commit with `STUB_FAIL=clippy` | refused, exit 1; output has `clippy FAILED` and `stub clippy failed`; stub log has no `test` line |
| the hook unsets INSPECTAH_SKIP_UI | code commit with `INSPECTAH_SKIP_UI=1` in the environment | every stub log line ends `INSPECTAH_SKIP_UI=unset` |
| a snapshot-only commit is gated | add `crates/a/tests/snapshots/x.snap` | stub log has `fmt` |
| a UI-only commit is gated | add `crates/web/ui/src/App.tsx` | stub log has `fmt` |
| a deletion-only code commit is gated | `git rm crates/a/tests/t.rs`, commit | stub log has `fmt` |
| a rename out of a code path is gated | `git mv crates/a/src/lib.rs docs/lib.rs`, commit | stub log has `fmt` |
| an unstaged code edit is refused before fmt | stage a docs edit and a code edit, then edit the code file again | refused naming `crates/a/src/lib.rs`; stub log empty |
| an untracked code path is refused before fmt | code commit with untracked `crates/a/tests/new.rs` | refused naming `crates/a/tests/new.rs`; stub log empty |
| an unstaged docs edit and an untracked docs file do not block | code commit with `README.md` edited unstaged and `docs/new.md` untracked | accepted |
| an untracked file created during the test gate is refused after it | code commit with `STUB_DURING_TEST=untracked:crates/a/late.rs` | refused naming `crates/a/late.rs`; stub log has `test` |
| a tracked code file edited during the test gate is refused after it | code commit of `crates/a/src/lib.rs` with `STUB_DURING_TEST=edit:crates/a/tests/t.rs` | refused naming `crates/a/tests/t.rs`; stub log has `test` |
| the index changing during the gate is refused | code commit with `STUB_DURING_TEST=add:crates/a/late.rs` | refused with `the index changed while the gate ran` |
| --only refuses a code path staged in the real index but left out of the commit | (a) `git add` an edit to `crates/a/src/lib.rs`, edit `docs/guide.md`, `git commit --only docs/guide.md`; (b) in a fresh repo, `git add` an edit to `crates/a/src/lib.rs`, edit `crates/a/tests/t.rs`, `git commit --only crates/a/tests/t.rs` | (a) accepted; docs-only line; stub log empty; `crates/a/src/lib.rs` is still staged afterwards. (b) refused naming `crates/a/src/lib.rs` and not naming `crates/a/tests/t.rs`; stub log empty |
| -a refuses an untracked code path | untracked `crates/a/src/new.rs`, tracked code edit, `git commit -a` | refused naming `crates/a/src/new.rs` |
| a missing docs-only list refuses | `git rm --cached .githooks/docs-only-paths`, code commit | refused with `cannot read .githooks/docs-only-paths from the index`; stub log empty |
| an unstaged edit to the docs-only list is not applied | append `^crates/` to the working-tree copy only, then a code commit | no docs-only line; refused naming `.githooks/docs-only-paths`; stub log empty |
| an unreadable staged tarball refuses with exit 1 | `git add -f testdata/bad.tar.gz` holding text that is not an archive, then run `.githooks/pre-commit` directly | exit 1 (the helper exits 2); output has `cannot be read` and `commit refused: sensitive-tarball check failed (exit 2)` |
| head mode gates a code commit | code commit made `--no-verify`, then run `.githooks/pre-commit --head` | exit 0; stub log has `fmt` |
| head mode skips a docs commit | docs commit made `--no-verify`, then `--head` | exit 0; docs-only line; stub log empty |

The `--only` pair is the case that proves `GIT_INDEX_FILE` is honoured. A hook that reads the real index runs the gates in (a), and in (b) refuses naming `crates/a/tests/t.rs` instead of `crates/a/src/lib.rs`, so both assertions are needed. If either fails for a reason other than the hook, stop and report; do not work around it.

- [ ] **Step 3: Write `scripts/tests/docs-only-paths-test.sh`** to the scope above, with `check_tree` stubbed (`return 0`) for now. `check_tree <root> <allowlist>` loads `<root>/.githooks/docs-only-paths` and reads `<root>`'s tracked `crates/**/*.rs` files (`git -C <root> ls-files`). It extracts with `perl -0777`: `include_str!` and `include_bytes!` arguments, including ones split over lines; and every string literal, plain (`"..."`, `b"..."`, with escapes) or raw (`r"..."`, `r#"..."#`), skipping line comments, block comments and char literals. Each failure prints one line: the file, the line, the literal, and the resolved repo-relative path, or `unresolved include`. The allowlist is a newline-separated list of `<file> <literal>` pairs; the repository's lives in the script's header, one pair per line with a comment giving the reason, as in the table above. An allowlist pair that no longer occurs in its file fails as `stale allowlist entry`, so entries do not outlive their code. The header also states what the check does not cover, in the words of the scope above.

  Each planted case builds a throwaway tree with `git init`, the repo's `.githooks/docs-only-paths` and one `crates/x/src/lib.rs`, committed, so `git ls-files` sees it, and asserts the reported file and resolved path, not only a non-zero exit:

  | Case | Planted in `crates/x/src/lib.rs` | Expected |
  |---|---|---|
  | a planted include under docs/ fails | `include_str!("../../../docs/x.md")` | fails naming `crates/x/src/lib.rs` and `docs/x.md` |
  | a planted literal joined to the repo root fails | `repo_root().join("docs/input.json")` | fails naming `docs/input.json` |
  | a planted directory-name literal fails | `.join("..").join("..").join("process-docs")` | fails naming `process-docs` |
  | a planted runtime path to a top-level markdown file fails | `"../../README.md"` | fails naming `README.md` |
  | a literal in a comment is ignored | `// see "docs/x.md"` | passes |
  | an allowlisted literal passes | `"README.md"`, with allowlist `crates/x/src/lib.rs README.md` | passes |
  | a stale allowlist entry fails | no literal, allowlist `crates/x/src/lib.rs README.md` | fails with `stale allowlist entry` |
  | a non-literal include argument fails as unresolved | `include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/x"))` | fails with `unresolved include` naming the file |
  | the repository passes | `check_tree "$REPO_ROOT" "$REPO_ALLOWLIST"` | passes |

- [ ] **Step 4: Run both and see them fail**

Run: `scripts/tests/pre-commit-hook-test.sh` then `scripts/tests/docs-only-paths-test.sh`
Expected: the hook test fails every case except `a code commit runs fmt, clippy and test in order` and `an unstaged docs edit and an untracked docs file do not block`, which today's hook already satisfies. The docs-only test fails every case that expects a failure and passes the three that expect a pass, since the stub reports nothing.

- [ ] **Step 5: Implement `check_tree`** and run `scripts/tests/docs-only-paths-test.sh`. Expected: `N passed, 0 failed`.

- [ ] **Step 6: Rewrite `.githooks/pre-commit`** to the order above. Helper names: `load_docs_only`, `non_docs_paths <newline-separated list>` (prints the paths that fail the list; `grep -vE` on a here-string, treating its exit 1 for "nothing printed" as success), `refuse <message>` (prints the refusal line and exits 1), `refuse_if_dirty <first|second>`, `run_gate <name> <command...>`.

- [ ] **Step 7: Update `check-sensitive-tarballs-test.sh`'s hook section.** It now also copies `.githooks/docs-only-paths` into its throwaway repo, puts a stub `cargo` that exits 0 on `PATH` for its accepted cases (those stage `testdata/` files, which are now gated), and moves the untracked `testdata/sensitive.*` files out of the repo before `a text file and a symlink are accepted` and back before the `--tracked` section (the refuse check would otherwise name them). Its long-staged-list case keeps its exit-1 stub and must still find `cargo stub ran` in the output, now printed from the failed gate's file.

- [ ] **Step 8: Run all three tests and see them pass**

Run: `scripts/tests/pre-commit-hook-test.sh && scripts/tests/docs-only-paths-test.sh && scripts/tests/check-sensitive-tarballs-test.sh`
Expected: each ends `N passed, 0 failed`, exit 0.

- [ ] **Step 9: Add both new tests to `rust-ci.yml`**, as steps in the `sensitive-tarballs` job after `Test the gate`, each with `if: ${{ !cancelled() }}`: `Test the pre-commit hook` and `Check that no build input reads a docs-only path`. The job's runner already has git, jq and perl, and the workflow's path triggers already cover `.githooks/**` and `scripts/**`.

- [ ] **Step 10: Rewrite the prose that describes the hook** in the files listed above. Each states the current behavior, not its history: the docs-only list and where it lives; that a deletion, rename, snapshot, fixture or UI commit is gated; that the hook refuses while a non-docs path differs from the index being committed, before fmt and after test; that a docs-only commit is never refused for other files; that `INSPECTAH_SKIP_UI` has no effect on the hook; one line per gate. `developer-guide.md`'s staged-set table is re-derived from the new rule. `docs/` is published, so voice rules apply there.

- [ ] **Step 11: Commit.** `git add` the three new files first (`.githooks/docs-only-paths` and both test scripts), then `git commit --only` naming every file in this task, as `fix(hooks): gate the commit's own content on an inverted docs-only list`. No CHANGELOG entry: hooks, CI and contributor docs are excluded by the changelog policy. The commit is gated by the new hook; report its per-gate lines.

---

### Task 4: Worktree script (private orchestration repo)

This task lands in the private orchestration repo, under `workflow/`. Read that repo's instructions first; its pre-commit refuses certain words, so the script and its test name no private repo.

**Files** (paths relative to the orchestration repo root):
- Create: `workflow/inspectah-worktree`
- Create: `workflow/test-inspectah-worktree` (run by hand, like that repo's hook tests)

**Interfaces:**
- Consumes, from Task 3: the classifier `.githooks/docs-only-paths`, read from `main` (`git show main:.githooks/docs-only-paths`); and the executable `.githooks/pre-commit --head`, run by `git rebase --exec` inside the worktree, so each rebased commit runs its own version of the hook, not `main`'s and not the shared checkout's.
- Produces this CLI:

```
inspectah-worktree [--repo <inspectah checkout>] create <branch>
inspectah-worktree [--repo <inspectah checkout>] land <branch>
inspectah-worktree [--repo <inspectah checkout>] cleanup <branch>
```

Every subcommand exits 0 on success and 1 otherwise. A refused precondition prints `inspectah-worktree: refused: <reason naming the path or branch>`; a step that started and then stopped prints `inspectah-worktree: stopped: <reason>`. Tests match on both prefixes, so a crash never reads as a refusal.

`--repo` defaults to the `inspectah` checkout beside the orchestration repo. The worktree path is `<repo>/.worktrees/<branch>` with each `/` in the branch name replaced by `-`, so cleanup leaves no empty parent directories.

**`create <branch>`**
1. Refuse unless `git check-ref-format --branch <branch>` passes, the branch does not exist, and the worktree path does not exist.
2. Refuse unless `git -C <repo> check-ignore -q .worktrees/<dir>` succeeds. inspectah's `.gitignore` already lists `.worktrees/`, so the `.git/info/exclude` line from § 6 is not needed; this check replaces it and fails loudly if the `.gitignore` line is ever dropped.
3. `git -C <repo> worktree add -b <branch> <path> main`.
4. Start the warm-up in the background, detached so it outlives the script (`nohup`, stdin from `/dev/null`): in `<path>`, with `INSPECTAH_SKIP_UI` unset, `nice -n 10 cargo clippy --workspace --all-targets -- -D clippy::all && nice -n 10 cargo test --workspace --no-run`. Output to `<path>/target/warmup.log` (create `target/` first; it is ignored, so the refuse check never sees it, and `git worktree remove` takes it with the worktree). The PID of the shell running both commands goes to `<path>/target/warmup.pid`; it lives until both have ended.
5. Print the worktree path, the branch, the log path and the PID, one per line, `key: value`.

**`land <branch>`**
1. Refuse unless the shared checkout is on `main` (`git symbolic-ref --short HEAD`); no rebase is in progress in the worktree (neither `rebase-merge` nor `rebase-apply` exists under `git -C <path> rev-parse --path-format=absolute --git-path <name>`; the refusal names `git -C <path> rebase --abort`); and the worktree is clean (`git -C <path> status --porcelain` prints nothing).
2. `base=$(git merge-base main <branch>)`.
3. If `base` is `main`'s tip: go to step 6.
4. If every path in `git diff --name-only --no-renames <base> main` matches the docs-only list read from `main`: `git -C <path> rebase main`. The range diff, not each commit, is the right test: what the rebased commits sit on differs from what they were gated on only by those paths.
5. Otherwise: `git -C <path> rebase --exec '.githooks/pre-commit --head' main`.
6. `git -C <repo> merge --ff-only <branch>`.

If a rebase stops (conflict or red gate), leave it in progress, print the worktree path and `git -C <path> rebase --abort` as the way back, and stop. If the fast-forward fails, print git's own error and stop. Say "run land again" only when `main` has moved past the base the branch now sits on (`git merge-base main <branch>` is no longer `main`'s tip); otherwise git's message names the shared-checkout files in the way, and running again will not help.

**`cleanup <branch>`**
1. Refuse unless `git -C <repo> merge-base --is-ancestor <branch> main`.
2. Refuse while the warm-up PID in `target/warmup.pid` is alive (`kill -0`).
3. `git -C <repo> worktree remove <path>` (no `--force`: it refuses on uncommitted or untracked files, and removes ignored `target/` and `node_modules/`), `git -C <repo> branch -d <branch>`, `git -C <repo> worktree prune`.

An unlanded branch from a dispatch that died is Mark's call by hand; the script never forces.

**Docs-only list: one copy.** The script reads `main:.githooks/docs-only-paths` with `git show`, so it applies the list `main` carries and never a working-tree edit. Rejected: copying the patterns into the script (a second definition that drifts, the failure the inverted list exists to end), and calling the hook as a classifier (couples the script to hook internals and runs whatever hook sits in the shared working tree).

**Hooks in a linked worktree.** `core.hooksPath` is `.githooks`, set in `.git/config`, which linked worktrees share (`extensions.worktreeConfig` is unset; both checked 2026-11-19). Git runs a hook from the root of the worktree it fires in, and resolves a relative `core.hooksPath` from there, so each worktree runs its own branch's `.githooks/pre-commit` and needs no install. A throwaway-repo experiment confirmed this (2026-11-20); the test below keeps it proved.

- [ ] **Step 1: Write `workflow/test-inspectah-worktree`**: bash 3.2, a throwaway "inspectah" repo per case, with `main`, a `.gitignore` holding `.worktrees/` and `/target/`, a `.githooks/docs-only-paths` with `^docs/` and a stub `.githooks/pre-commit` that appends `gate <args> $(git rev-parse --short HEAD) $PWD` to `$GATE_LOG` and exits 0, `core.hooksPath .githooks`. A stub `cargo` on `PATH` logs its full arguments (`$*`) and whether stdin is at EOF (`if read -r _; then echo stdin-open; else echo stdin-null; fi`), and sleeps for `$STUB_SLEEP` seconds if set. Every `create` in the test runs with a live stdin that holds a line (`<<<x`), so a `create` that lets the warm-up inherit stdin logs `stdin-open`. Every refusal case asserts the `refused:` prefix and the named path or branch, not only the exit status.

| Case | Expected |
|---|---|
| create makes the worktree on a new branch from main | path `.worktrees/feat-x` for branch `feat/x`; `git -C <path> rev-parse HEAD` equals `main`; output has `path:`, `branch:`, `log:`, `pid:` |
| create starts the warm-up with stdin closed | `target/warmup.log` in the worktree shows `clippy --workspace --all-targets -- -D clippy::all` then `test --workspace --no-run`, both `stdin-null` |
| create refuses when .worktrees is not ignored | `.gitignore` without the line: refused naming `.worktrees`; nothing created |
| create refuses an existing branch | refused naming the branch |
| a commit in the worktree runs the worktree's own hook | change the stub hook on the branch to log `branch-hook`, commit in the worktree: `$GATE_LOG` has `branch-hook` and the worktree path |
| land fast-forwards when main has not moved | `main` equals the branch tip; `$GATE_LOG` has no `--head` line |
| land rebases without re-gating after docs-only commits | the branch carries two code commits; a `docs/` commit on `main` first: the branch lands on top of it; no `--head` line |
| land re-gates every commit after a code commit on main | a non-docs commit on `main` first, two commits on the branch: exactly two `--head` lines, whose hashes equal `git rev-list --reverse <main before land>..main` after landing, in that order, and whose directory is the worktree path |
| land stops on a red re-gate | stub hook exits 1 under `--head`: exit 1, `stopped:`, rebase in progress, `main` unmoved, output names `rebase --abort` |
| land refuses while a rebase is in progress | after the red re-gate case, `land` again: refused naming `rebase --abort`; `main` unmoved |
| land stops when a shared-checkout edit blocks the fast-forward | the branch changes `docs/a.md`; the shared checkout has an uncommitted edit to it: exit 1, `stopped:`, git's "would be overwritten" message, no "run land again" |
| land refuses a dirty worktree | untracked file in the worktree: refused naming it |
| land refuses when the shared checkout is not on main | refused naming the branch it is on |
| cleanup refuses an unlanded branch | refused naming the branch; worktree and branch still there |
| cleanup refuses while the warm-up runs | `STUB_SLEEP=3` at create, cleanup at once: refused naming the PID. Then poll `kill -0 "$(cat <path>/target/warmup.pid)"` once a second, failing the case after 30 s, until it fails; kill nothing. Cleanup then succeeds |
| cleanup removes a landed worktree with ignored build output | files under `target/` and `crates/web/ui/node_modules/`: worktree directory gone, branch gone, `git worktree list` shows one entry |

- [ ] **Step 2: Run it and see it fail**: `workflow/test-inspectah-worktree`. Expected: every case fails, the script does not exist.

- [ ] **Step 3: Write `workflow/inspectah-worktree`** to the behavior above: bash 3.2, `set -euo pipefail`, a `usage` function, `refuse` and `stop` functions that print the two prefixes and exit 1, one function per subcommand, named constants for the worktree directory (`.worktrees`), the log and PID file names, and the `nice` level.

- [ ] **Step 4: Run it and see it pass.** Expected: `N passed, 0 failed`.

- [ ] **Step 5: Check the warm-up survives the caller's shell.** Run `create` once against a scratch clone of inspectah through the same tool the orchestrator uses, then from a new command `kill -0 "$(cat <path>/target/warmup.pid)"`. Expected: the process is alive. If it is not, the tool reaps background children: record that in the script's header and say so in the report, so the orchestrator runs `create` in the tool's background mode. Remove the scratch worktree with `cleanup` once the warm-up ends.

- [ ] **Step 6: Commit.** `git add` both new files, then commit them in the orchestration repo as `feat(workflow): add the inspectah worktree script`, through that repo's hooks.

---

### Task 5: Dispatch procedure (orchestrator applies)

The orchestrator's instruction files are guarded: the orchestrator edits them, not an implementer. This task is the exact text and where it goes. Write "the orchestrator" as the orchestrator's own name in those files, and the script path as each file needs it (relative `workflow/inspectah-worktree` in the orchestration repo's copy, the absolute path in the other).

- [ ] **Step 1: Add to both instruction files**, in the Delegation Protocol list directly after the item on inspectah and driftify implementation reviews, renumbering what follows:

```markdown
4. **inspectah code dispatches run in their own worktree.** A dispatch that may edit any path outside inspectah's `.githooks/docs-only-paths` is a code dispatch. Before dispatching it, the orchestrator runs `<script> create <branch>`, which starts a warm-up build in the background; dispatch at once, without waiting for it. The brief names the worktree path `create` printed as the repo path. After the review seat approves, the orchestrator runs `<script> land <branch>`, then `<script> cleanup <branch>` once the warm-up has ended. If `land` stops on a conflict, a red gate or a blocked fast-forward, stop and bring it to Mark. One code dispatch at a time. Docs dispatches stay in the shared checkout, touch only docs-only paths, and commit with `git commit --only <paths>`.
```

- [ ] **Step 2: Replace the brief text for inspectah implementation dispatches** (today: iterate on crate tests, commit with `--only`, the hook is the full gate) with:

```markdown
Code dispatch: work only in <worktree path>, your own git worktree on branch <branch>. Iterate with crate-scoped tests (`cargo test -p <crate>`, or one test by name). Do not run fmt, clippy or the workspace suite by hand: the pre-commit hook runs all three on every commit that is not docs-only, and refuses while any non-docs file is unstaged or untracked, so commit or delete scratch files first. Commit with `git add <paths>` and `git commit -F <file>`. Report the hook's per-gate lines as the gate result. Never `--no-verify`. A warm-up build may hold the cargo lock for the first minutes; wait for it.

Docs dispatch: work in the shared checkout, touch only paths in `.githooks/docs-only-paths`, `git add` any new file, then commit with `git commit --only <paths> -F <file>`.
```

- [ ] **Step 3: Run the drift check** between the two instruction files and resolve anything it reports beyond the deliberate differences.

- [ ] **Step 4: Update the orchestrator's memory notes**: the note that inspectah briefs iterate on crate tests and commit with `--only` (now: code writers commit normally in their worktree, docs writers use `--only`, and "docs-only" means the list file, not "no Rust file"); the note recording the approved gate redesign (now implemented, with the script's path); the note on parallel agents' scratch directories (code dispatches no longer share a working tree). Update the two private backlog items about the hook skipping frontend commits: the inverted list closes their "no gate at all" gap; vitest in the hook and hook installation for fresh clones remain.

- [ ] **Step 5: Commit** each instruction file in its own repo, `docs(process): route inspectah code dispatches through a worktree`.

---

### Task 6: Measure the per-commit gate

Runs after Tasks 1 to 3 have landed and Task 4's script exists. Commits nothing but the results in Step 5; the probe edits are never committed.

- [ ] **Step 1: Create the worktree**: `<script> create measure-gate`. Wait for the warm-up to end by polling its PID (`kill -0 "$(cat <path>/target/warmup.pid)"`), never by `pgrep` on a command line, which matches the polling shell itself.

- [ ] **Step 2: Leaf edit, three samples.** In the worktree: append a comment line to `crates/cli/src/main.rs`, `git add` it, record `uptime`, run `.githooks/pre-commit` directly (no commit), record `uptime` and the three per-gate lines, then `git restore --staged --worktree crates/cli/src/main.rs`. Change the comment text each sample so each one recompiles.

- [ ] **Step 3: Core edit, three samples**, the same with `crates/core/src/lib.rs`.

- [ ] **Step 4: Confirm nothing is left**: `git -C <path> status --porcelain` prints nothing, then `<script> cleanup measure-gate`.

- [ ] **Step 5: Record and compare.** Add a dated row set to the timing table in `process-docs/skills/full-workspace-verification-gates.md` § 3: machine, toolchain, the command, per-gate seconds per sample, load average before and after. Compare with the estimates (leaf ~35 s, core ~95 s, down from ~70 s and ~135 s) and say in the report whether each holds, naming the load. Commit that file alone from the shared checkout with `--only`, as `docs(skills): record per-commit gate timings after the gate rework`.

---

## Out of scope

§ 8 steps 6 to 8: a `cargo nextest` trial in hook and CI; merging integration tests into one binary per crate; a driftify pre-commit gate (blocked on its macOS-red suite). Also out: a vitest or `tsc --noEmit` branch in the hook, a setup script that installs the hooks in a fresh clone, real `.git` rerun paths for release builds in linked worktrees, the RPM's `commit unknown`, and runtime reads the docs-only read check cannot see (Task 3, § Docs-only read check: scope).
