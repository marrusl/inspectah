---
name: scan-root-check-precedes-all-validation
description: run_scan's geteuid() check is the first statement in the body and exits the process, so unprivileged tests on scan pass vacuously and a root-run suite performs a real scan.
---

# `scan`'s Root Check Precedes All Validation

`crates/cli/src/commands/scan.rs::run_scan` opens with the privilege check,
before `validate_sensitivity_flags` and before anything else:

```rust
pub fn run_scan(args: &ScanArgs, assume_yes: bool) -> Result<ScanOutcome> {
    // Require root: scanning reads system state that needs elevated privileges.
    // SAFETY: geteuid() is a simple syscall with no preconditions or invariants.
    let euid = unsafe { libc::geteuid() };
    if euid != 0 {
        eprintln!("Error: inspectah scan requires root privileges.");
        eprintln!("Run with: sudo inspectah scan [options]");
        std::process::exit(1);
    }

    validate_sensitivity_flags(args)?;
```

It calls `std::process::exit(1)`, not `return Err(..)`. That one ordering
produces two opposite test hazards, and which one you get depends on who runs
the suite.

## Hazard 1: unprivileged, the test passes vacuously

Any `assert_cmd` test that drives `inspectah scan` expecting a failure from
inside `run_scan` gets the root message instead and never reaches the code it
meant to exercise. The test is green and proves nothing.

Exit code does not separate the two paths: the root check exits **1**, and
`main.rs:73-76` prints `error: {e:#}` and also exits **1** for a returned
`Err`. So `.failure()` and `.code(1)` are both satisfied by the wrong path.

**Assert a positive and a negative on stderr, never just the positive:**

```rust
// the case under test, rejected before the root check
.failure()
.stderr(predicate::str::contains("INSPECTAH_HOSTNAME"))
.stderr(predicate::str::contains("requires root privileges").not());
```

The negative half is the whole test. Without it, a later change that moves the
validation after the root check turns the case green and silent.

The inverse assertion is also useful: to prove a *valid* input is accepted
without having root, assert that the process got as far as the root check and
failed there, with the validation message **absent**. Asserting the next
failure is how an unprivileged test shows the happy path is open.

## Hazard 2: as root, the test stops being a test and starts being a scan

Run the same suite as root and execution falls through the `euid != 0` branch
into the real body: source detection, a baseline container **image pull**
(`eprintln!("Pulling {}...")`, around `scan.rs:430`), every inspector, and a
tarball written into the test's working directory. A slow, network-dependent,
filesystem-mutating run triggered by nothing the test checks.

GitHub CI is safe as of 2026-11-05: `rust-ci.yml:91` runs
`cargo test --workspace` on `runs-on: ubuntu-latest` with no `container:` key,
so it runs as the unprivileged `runner` user. The exposure is local: a
devcontainer, a rootful container shell, or anyone who reaches for
`sudo cargo test` after hitting a permissions error.

**Guard any case that enters `run_scan`'s body.** `libc` is already a direct
dependency of `crates/cli`:

```rust
// This case asserts on the root-check rejection. Run as root it is both
// meaningless and destructive: it would perform a real scan.
// SAFETY: geteuid() is a simple syscall with no preconditions.
if unsafe { libc::geteuid() } == 0 {
    eprintln!("skipping: requires an unprivileged test runner");
    return;
}
```

A case that is rejected *before* the root check needs no guard, because it
behaves identically either way. That is the dividing line: guard the cases
whose expected outcome is the root message.

## The design lever

The ordering is not fixed. Putting argument validation **before** the
`geteuid()` check costs nothing (no side effect has happened yet: no executor,
no source detection, no image pull, no inspector) and buys a rejection path
that is reproducible unprivileged, which is the only thing that makes a
rejection test runnable in CI at all. When you add a new validated input to
`scan`, resolve and validate it at the top of `run_scan` above the root check,
and the test writes itself.

## One more thing that bites

`assert_cmd`'s `Command::cargo_bin(..)` passes **the caller's environment** to
the child. Anyone hand-validating an environment-driven feature will have that
variable exported in their shell, where it leaks into every case in the binary.
Call `.env_remove("<VAR>")` on any case that must observe the variable absent.

## Why the existing test does not show any of this

`crates/cli/tests/baseline_requirement_test.rs` drives `scan` unprivileged and
is fine, because clap rejects `--no-baseline` at exit code 2 before `run_scan`
is ever called. Its doc comment says so. That is a clap-level rejection and it
is not evidence that anything inside the function is reachable from a test. As
of 2026-11-05 nothing in the suite enters `run_scan`'s body, so the first test
that does is also the first to meet both hazards.

## See Also

- `crates/cli/src/commands/scan.rs` `run_scan`, the ordering itself
- `crates/cli/src/main.rs:73-76`, the `Err` path that shares exit code 1
- `full-workspace-verification-gates.md`, the same class of problem at the
  clippy and CI gate level: a check that cannot fail reports clean
