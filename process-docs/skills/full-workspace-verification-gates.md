---
name: full-workspace-verification-gates
description: Running the full clippy/test/frontend gate suite correctly — which clippy invocation the hook actually runs versus what a stricter "clean" acceptance needs, why `-D clippy::all --all-targets` cannot pass here, how long a clean full-workspace run legitimately takes, and how to tell a slow run from a real hang.
---

# Running the Full Verification Gate Suite

Four things that have each already produced a wrong conclusion in this
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

## 3. A clean full-workspace run takes 20-30 minutes, and a fresh test binary idles before its first test

`cargo test --workspace` compiles and links a separate test binary per
integration test file. Each freshly linked binary has a real startup
cost — 60 to 120 seconds idle before its first test result prints —
which is normal, not a hang. A session that reads "nothing printed for
90 seconds" and concludes the run is stuck has mistaken linker/startup
latency for a stall at least twice in this repo's history.

**The converse is just as real: an actual hang looks identical for
25 minutes.** Do not resolve the ambiguity by waiting longer or by
re-running the command. Check `ps` for the process and read its
**elapsed time**, not just whether it exists:

```bash
ps -eo pid,etime,command | grep -E "cargo|rustc"
```

A `rustc` child actively present with a plausible elapsed time next to
a `cargo test` parent is compiling, not stuck. If the elapsed time
climbs well past the 20-30 minute budget with no new child processes
appearing, that is the actual signal to investigate, not the silence
itself.

## 4. Never pipe a commit message

One `git commit` in this repo's history blocked on a pipe for 17 hours
and was only found by a human checking in, not by any automated check —
indistinguishable from the legitimate long run in point 3 above while it
was happening. Always pass the commit message with `-F <file>`, never
through a shell pipeline and never `-F -` (which reads stdin and blocks
identically if nothing is piped into it).

## See Also

- `process-docs/skills/e2e-fixtures-are-hand-maintained.md` — the
  frontend command this skill's point 1 names (`npm run test:e2e`) can
  itself report green while skipping real coverage; a different failure
  mode from anything above.
