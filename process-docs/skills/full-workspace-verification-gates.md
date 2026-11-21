---
name: full-workspace-verification-gates
description: Running the full clippy/test/frontend gate suite correctly: the single clippy command the pre-commit hook and CI both run, the pinned toolchain in `rust-toolchain.toml` and how to bump it, what that hook does and does not gate (`tsc && vite build` for any commit that changes the UI, never vitest or Playwright), measured gate timings (2026-10-31 table, superseded figures noted) and why a slower run means something is wrong, why an agent cannot time its own run, and why periodic `cargo clean` is maintenance rather than risk.
---

# Running the Full Verification Gate Suite

Six things that have each already produced a wrong conclusion in this
repo's review history.

## 1. The pre-commit hook and CI run the same clippy command

`.githooks/pre-commit` (line 55) and `.github/workflows/rust-ci.yml`
(line 108) both run:

```bash
cargo clippy --workspace --all-targets -- -D clippy::all
```

CI has run this identical form since `79e46050`. `-D clippy::all`
denies the `clippy::all` lint group, so one lint in it fails the gate; rustc's own
warnings, such as an unused variable or a dead function, print and exit
0 (measured 2026-11-08). `--all-targets` lints test code as well as lib
and bin targets. A green hook or a green CI run means the workspace
has no lint from `clippy::all`; it says nothing about compiler warnings.

**Both run the same clippy version, too.** `rust-toolchain.toml` at the
repo root pins the toolchain to an exact release, and it is the only
place the version is written: rustup reads it for every cargo command
run inside the clone, and both `rust-ci.yml` and `build-binary.yml`
install from it with `rustup toolchain install`. CI used to follow
`@stable`, so on 2026-11-15 the 1.100.0 release widened
`needless_bool` and turned main red with no code change.

To bump, in one commit or PR: change `channel` in `rust-toolchain.toml`,
run `cargo clippy --workspace --all-targets -- -D clippy::all` (the hook
does this when the file is staged), and fix whatever the new lints
flag. Never bump the file alone and fix the lints later. Targets are
per toolchain; `build-binary.yml`, which builds the release binaries,
adds its target on every run, so a bump needs nothing more for releases
(see `release-process.md`).

**The hook type-checks TypeScript but runs no web test.** It gates
every commit that changes a path outside `.githooks/docs-only-paths`, a
frontend-only commit included, and runs only the three cargo gates, one
output line each. `crates/web/build.rs:35-40` runs `npm run build`
(`tsc && vite build`) inside the clippy gate whenever the UI source or
build files changed, and the hook unsets `INSPECTAH_SKIP_UI`, so setting
it does not skip that step. The unset only works because `build.rs`
prints `cargo:rerun-if-env-changed=INSPECTAH_SKIP_UI` before its early
return: without that line Cargo reuses a result cached by a skipped
build, and the hook passes a UI change it never type-checked.
`scripts/tests/web-build-skip-ui-test.sh` guards it with real cargo
builds of a throwaway crate that copies `build.rs`. It never runs vitest (`npm test`) or
Playwright (`npm run test:e2e`). A passing hook on a frontend change
says nothing about either.

**The hook tests the commit, not just the working tree.** Before fmt and
again after test it refuses while any path outside the docs-only list is
edited and unstaged or untracked, or is an ignored `crates/**/*.rs` or
`crates/**/Cargo.toml` (the unanchored `build/` rule in `.gitignore`
matches `crates/*/src/build/`, so an ignored module is a real case), and
after the gates it refuses if the index's tree changed. Every check reads
the index git hands the hook (`GIT_INDEX_FILE`), which under
`git commit --only` and `git commit -a` is a temporary index, not
`.git/index`. Other ignored files are not checked, so an ignored build
input of another kind can still be tested without being committed. With
`--head` the hook also refuses while the index differs from `HEAD`, and
classifies a merge by its changes against every parent. A docs-only
commit skips all of this and is never refused for other files. The git
and shell traps behind these checks are in `git-hook-shell-pitfalls.md`.

**Never run cargo and vitest at the same time.** `crates/web/build.rs`
runs `npm ci` in `crates/web/ui` whenever the build script reruns, and `npm ci`
deletes and reinstalls `node_modules`, so a vitest run started beside a
cargo command loses its dependencies partway through and fails for no
reason in the code. Run the frontend gate after cargo finishes.

## 2. `cargo clippy --workspace --all-targets -- -D clippy::all` is required and passes

This command is the clippy gate. It exits 0 with zero warnings as of
2026-11-04, so any warning it reports is a regression to fix, not a
baseline to compare against.

```bash
cargo clippy --workspace --all-targets -- -D clippy::all
```

`--all-targets` is part of the gate: without it clippy checks the lib and
bin targets only, and test code is where warnings accumulate unseen.

## 3. The gates are fast. A slow run is a symptom, not a budget

Every gate in this workspace finishes in **seconds to about a minute and a half**.
Measured 2026-10-31 on an Apple M3 Pro (12 cores, 36 GB), macOS 27.2,
`rustc 1.99.1 (58cf8f9c3 2026-10-13)`, workspace at
`phase-0-refine-ui-fixup`:

| command | starting state | wall clock |
|---|---|---|
| `cargo clean` | 9.4 GB target | 8s |
| `cargo test --workspace --no-run` | after `cargo clean` (269 crates) | **66s** |
| `cargo test --workspace` | binaries just linked, first execution | **29s**, 3010 tests |
| `cargo test --workspace` | fully warm, nothing to rebuild | 11s |
| `cargo fmt --check` | any | 1s |
| `cargo clippy --workspace -- -W clippy::all` | first clippy after a clean build | 25s |
| `cargo clippy --workspace -- -W clippy::all` | warm | 3s |
| `cargo clippy -p inspectah-core` | warm | 5s |
| all three hook gates in sequence | warm, nothing changed | 11s |
| all three hook gates in sequence | after touching `crates/core/src/lib.rs` — every downstream crate rebuilds and every test binary relinks | **47s** |

The clippy command measured in the table is the pre-`fd422ff6` form
(`-W`, no `--all-targets`). The current gate adds `--all-targets` and
`-D`, so expect somewhat longer.

An independent measurement the same day on the same machine got 51s for
the cold `--no-run`, 28-50s for a warm suite, 14s for workspace clippy
and 65s for the hook. Treat the two sets together as the range: **a
from-scratch rebuild plus the full suite is two to two and a half
minutes, and the hook on a realistic change is about a minute to a
minute and a half** (2026-11-08; the 2026-10-31 runs above came in
lower, under the old gate). The spread between the two runs is cache
state and machine load, nothing structural.
Re-measured 2026-11-08 on the same machine with the current
`-D clippy::all --all-targets` gate: 43 s for the cold build, 96 s for
the full suite, and 63 to 88 s for the hook on realistic changes.

Measured 2026-11-20 after the gate rework (hook at `1372ebaa`), same
machine, macOS 27.2, `rustc 1.100.0 (0ef1bc386 2026-11-09)`, in a fresh
linked worktree with its own `target/`. Each sample appends a comment
line to one file, stages it and runs `.githooks/pre-commit` directly;
the probe is then restored and never committed. Load is the one-minute
average before and after. The machine idles near 9 on its own, so these
are not quiet-machine numbers.

| edit | sample | fmt | clippy | test | hook wall | load before / after |
|---|---|---|---|---|---|---|
| `crates/cli/src/main.rs` (leaf) | 1 | 2s | 2s | 51s | 56s | 13.6 / 11.3 |
| `crates/cli/src/main.rs` (leaf) | 2 | 1s | 2s | 32s | 36s | 10.8 / 10.4 |
| `crates/cli/src/main.rs` (leaf) | 3 | 1s | 2s | 35s | 41s | 10.4 / 11.8 |
| `crates/core/src/lib.rs` (core) | 1 | 2s | 11s | 104s | 119s | 11.4 / 9.0 |
| `crates/core/src/lib.rs` (core) | 2 | 1s | 11s | 106s | 120s | 9.0 / 9.7 |
| `crates/core/src/lib.rs` (core) | 3 | 2s | 11s | 111s | 126s | 9.7 / 9.9 |

Medians: **41s for a leaf edit and 120s for a core edit**, against 70s
and 135s before the rework. The worktree's warm-up build (clippy plus
`cargo test --workspace --no-run` from an empty `target/`) took 161s
from creation to finish (55s clippy, 104s test build) at a load of
about 23. One `cargo test -p inspectah-cli --no-run` after it took 17s,
so the first hook run in a fresh worktree pays a short extra compile.
The first leaf sample's slower test gate is the first execution of
freshly linked binaries.

Measured 2026-11-20 again after integration tests moved to one binary
per crate (`575c159c`; cli keeps `units_scan_test` as a second binary,
see [integration-test-layout](integration-test-layout.md)). Same
machine, macOS 27.2, `rustc 1.100.0 (0ef1bc386 2026-11-09)`. A is the
same tip with the six layout commits reverted (86 integration test
binaries), B the tip (7). Same probe method as above, samples
interleaved A B B A A B A B B A, every one with all three gates ok.
Each leaf sample follows an untimed build of the gate's commands, so it
does not pay for the previous core probe. Load is the one-minute
average before and after; every sample started between 8.2 and 9.9.

| edit | side | hook wall, 5 samples | median | test gate | load after |
|---|---|---|---|---|---|
| `crates/cli/src/main.rs` (leaf) | A, per-file | 58.71, 34.15, 33.11, 36.05, 36.79 s | **36.05s** | 29-54s | 8.5-11.8 |
| `crates/cli/src/main.rs` (leaf) | B, per-crate | 30.93, 31.38, 29.83, 30.51, 30.15 s | **30.51s** | 25-27s | 10.2-19.0 |
| `crates/core/src/lib.rs` (core) | A, per-file | 113.18, 111.35, 111.62, 116.13, 114.51 s | **113.18s** | 98-102s | 8.1-19.1 |
| `crates/core/src/lib.rs` (core) | B, per-crate | 63.40, 57.34, 58.82, 61.10, 59.14 s | **59.14s** | 47-51s | 9.1-14.0 |

A warm `cargo test --workspace` with nothing to rebuild took 27.16 s on
A (97 test result lines) and 22.98 s on B (18).
Both run the same 1,629 integration tests; B adds the 3 guard tests in
each of its six `integration` binaries (3,940 tests passed on A, 3,958
on B).

The core-edit saving is 54.04 s (113.18 to 59.14), more than the
2026-11-19 estimate of "most of the ~62 s test build" (at least 31 s):
the test gate itself fell from about 100 s to about 49 s. The leaf-edit
gate saved 5.54 s.

There is no per-binary idle. The row that matters is the third one: the
very first execution of all **77** freshly linked test binaries, none of
them ever run before, is 29 seconds for all 3010 tests together — well
under half a second of binary, on average, for the thing that was
reported as 60-120 seconds of idle each.

**So a run that has not returned in a few minutes is not slow, it
is wrong.** Do not wait it out and do not re-run it. Check `ps` and read
the **elapsed time**:

```bash
ps -eo pid,etime,command | grep -E "cargo|rustc"
```

A `rustc` child with a plausible elapsed time under a `cargo test`
parent is compiling. No child process and a parent at 5 minutes is a
hang — most likely a command blocked on stdin (see point 5). Diagnose
it; it will not finish.

## 4. An agent is a poor witness to its own wall clock

Every long-duration claim in this phase came from an agent reporting on
its own run, and **every one was wrong by roughly twenty times when
somebody measured it**: 20-30 minutes for the suite, 15-17 minutes for
the hook, 60-120 seconds of per-binary idle. Those figures then shaped
days of work — they padded every wait, they got `cargo clean` banned
for a whole plan, and they led two separate seats to diagnose entirely
normal behaviour as a hang.

The reason is structural, not carelessness. An agent's sense of "how
long that took" is the length of its own turn, which includes model
latency, tool round trips and its own thinking — none of which it can
see or subtract. It has no clock, only an impression.

**A duration therefore belongs in a skill file, a plan or a review only
with the command that produced it and the machine it ran on.** If you
cannot name the command you timed, you do not have a number, and the
honest sentence is "I did not measure it."

## 5. Never pipe a commit message

One `git commit` in this repo's history blocked on a pipe for **17
hours** and was found by a human looking at a UI, not by any automated
check. Nothing in the session noticed, because the expectation at the
time was that long runs were normal — this is exactly the cost of the
wrong numbers in point 3. Against the real numbers it would have been
obvious inside two minutes.

Always pass the commit message with `-F <file>`, never through a shell
pipeline and never `-F -` (which reads stdin and blocks identically if
nothing is piped into it).

## 6. The target directory grows, and `cargo clean` is cheap maintenance

A clean rebuild leaves `target/` at **3.8 GB**. Over this phase it
reached **36 GB** — stale artifacts from every intermediate build,
nothing that speeds anything up. A bloated target makes incremental
builds look expensive and tempts the next seat into believing the
inflated durations in point 3.

`cargo clean` costs 8 seconds, and the full rebuild it forces costs
about a minute (66s to `--no-run`). **That is maintenance, not a risk.**
Any guidance that bans `cargo clean` to protect a cache is trading a
one-minute rebuild against tens of gigabytes and a misleading
performance picture; run it periodically.

## See Also

- `process-docs/skills/e2e-fixtures-are-hand-maintained.md` — the
  frontend command this skill's point 1 names (`npm run test:e2e`) can
  itself report green while skipping real coverage; a different failure
  mode from anything above.
