---
name: full-workspace-verification-gates
description: Running the full clippy/test/frontend gate suite correctly — which clippy invocation the hook actually runs versus what a stricter "clean" acceptance needs, why `-D clippy::all --all-targets` cannot pass here, how to count the clippy baseline without overcounting, measured timings for every gate (all under a minute) and why a slower run means something is wrong, why an agent cannot time its own run, and why periodic `cargo clean` is maintenance rather than risk.
---

# Running the Full Verification Gate Suite

Six things that have each already produced a wrong conclusion in this
repo's review history.

## 1. The pre-commit hook and CI do not gate what "clippy clean" means here

`.githooks/pre-commit` and `.github/workflows/rust-ci.yml` both run:

```bash
cargo clippy --workspace -- -W clippy::all
```

No `--all-targets`, so neither lints a test target at all, and `-W`
warns rather than denies, so the command exits 0 even with warnings. A
green hook or a green CI run is evidence the hook ran, not evidence the
workspace has zero clippy lint.

**The hook also does not gate TypeScript at all.** It fast-paths out
entirely when no `.rs`/`Cargo.*`/`build.rs` file is staged, so a
frontend-only commit gets no `tsc`, no `npm test`, and no `npm run
test:e2e` from the hook. A passing hook on a frontend change says
nothing about any of the three.

## 2. `cargo clippy --workspace --all-targets -- -D clippy::all` does not pass, and that's expected

As of this writing there is a 9-warning baseline, entirely
`clippy::all`-style lint in test code (`bool_assert_comparison` x5,
`field_reassign_with_default` x2, `needless_range_loop`, `op_ref`).
`-D` promotes every one of those nine to a hard error, so the command
above **exits 101** on a clean tree. A task description that says
"`--all-targets -- -D clippy::all` reports nothing beyond the N-warning
baseline" is describing an impossible combination of flag and
expectation; the `-D` form cannot report a baseline, only fail on one.

Use this instead, and compare the count, not the diff:

```bash
cargo clippy --workspace --all-targets 2>&1 | grep -cE "^warning: [a-z]"
```

**Two counting traps sit right next to that number:**

- A bare `grep -c '^warning: '` overcounts. It also matches each
  per-crate "generated N warnings" summary line, so a 9-warning
  baseline reads as 13.
- Counting `-->` lines (one per reported span) overcounts differently.
  A single warning can carry more than one span —
  `field_reassign_with_default` carries two — so 9 warnings produce 11
  `-->` lines. **The span count is not the warning count.** This has
  already produced a wrong baseline number once in this repo's own
  planning documents (an 11 was proposed to replace a correct 9).

When the baseline changes, update it in one place
(`process-docs/plans/*` Global Constraints or wherever the current plan
states it) and re-derive the new number with the `^warning: [a-z]`
form, not by eyeballing `-->` counts.

## 3. The gates are fast. A slow run is a symptom, not a budget

Every gate in this workspace finishes in **seconds to about a minute**.
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

An independent measurement the same day on the same machine got 51s for
the cold `--no-run`, 28-50s for a warm suite, 14s for workspace clippy
and 65s for the hook. Treat the two sets together as the range: **a
from-scratch rebuild plus the full suite is one to two minutes, and the
hook on a realistic change is under a minute.** The spread between the
two runs is cache state and machine load, nothing structural.

There is no per-binary idle. The row that matters is the third one: the
very first execution of all **77** freshly linked test binaries, none of
them ever run before, is 29 seconds for all 3010 tests together — well
under half a second of binary, on average, for the thing that was
reported as 60-120 seconds of idle each.

**So a run that has not returned in a couple of minutes is not slow, it
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
