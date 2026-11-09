---
name: full-workspace-verification-gates
description: Running the full clippy/test/frontend gate suite correctly: the single clippy command the pre-commit hook and CI both run, what that hook does and does not gate (nothing for a frontend-only commit, `tsc && vite build` but never vitest or Playwright for a mixed commit), measured gate timings (2026-10-31 table, superseded figures noted) and why a slower run means something is wrong, why an agent cannot time its own run, and why periodic `cargo clean` is maintenance rather than risk.
---

# Running the Full Verification Gate Suite

Six things that have each already produced a wrong conclusion in this
repo's review history.

## 1. The pre-commit hook and CI run the same clippy command

`.githooks/pre-commit` (line 53) and `.github/workflows/rust-ci.yml`
(line 77) both run:

```bash
cargo clippy --workspace --all-targets -- -D clippy::all
```

CI has run this identical form since `79e46050`. `-D clippy::all`
denies the `clippy::all` lint group, so one lint in it fails the gate; rustc's own
warnings, such as an unused variable or a dead function, print and exit
0 (measured 2026-11-08). `--all-targets` lints test code as well as lib
and bin targets. A green hook or a green CI run means the workspace
has no lint from `clippy::all`; it says nothing about compiler warnings.

**The hook does not gate TypeScript.** A frontend-only commit gets
nothing from it: the fast path at `.githooks/pre-commit:18-27` exits
before any gate when no `.rs`, `Cargo.*` or `build.rs` file is staged.
A mixed Rust-plus-UI commit does get `tsc && vite build`, because
`crates/web/build.rs:30-35` runs `npm run build` inside the clippy gate,
but it never runs vitest (`npm test`) or Playwright (`npm run
test:e2e`). A passing hook on a frontend change says nothing about
either.

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
