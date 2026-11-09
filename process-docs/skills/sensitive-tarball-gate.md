# Sensitive Tarball Gate

`scripts/check-sensitive-tarballs.sh` refuses inspectah output that carries
preserved sensitive content. The pre-commit hook runs it with `--staged`,
CI runs it with `--tracked` in its own `sensitive-tarballs` job. Tests:
`scripts/tests/check-sensitive-tarballs-test.sh`, which also drives the real
hook in a throwaway repository.

## Where the markers really are

- **The snapshot member is `inspection-snapshot.json`, not `snapshot.json`.**
  The scan tarball puts it under a top-level directory (the `prefix` in
  `crates/pipeline/src/render/tarball.rs`); the checked-in fixtures have it
  at the root. Match the basename at any depth, as
  `crates/cli/src/commands/aggregate.rs` does, and never a `._` AppleDouble
  copy, which macOS `tar` adds to tarballs built from a Mac filesystem.
- **`redaction_state.state == "sensitive_retained"` is not enough on its
  own.** The aggregate merge sets `redaction_state` to `None`
  (`crates/core/src/aggregate/mod.rs`), so a merged fleet snapshot built
  from preserved hosts carries no state at all. `sensitive_snapshot` is the
  umbrella flag: `scan.rs` sets it for any `--preserve` or `--no-redaction`,
  and the merge ORs it across hosts. Every writer of `sensitive_retained`
  found on 2026-11-09 goes with the flag: the redaction engine writes the
  state only when the flag is set, and refine's upgrade in
  `crates/refine/src/session.rs` sets both. The state check is redundant
  today. The gate keeps both so a future writer that sets one
  and forgets the other is still caught.
- **`subscription/entitlement/`** is where `stage_subscription_files` in
  `crates/pipeline/src/render/configtree.rs` writes entitlement certs and
  keys. A snapshot-only tarball has no such member, yet still carries the
  key material base64-encoded inside the snapshot's `subscription` section,
  so the member check alone does not cover it.

## Ordering in the hook

The check runs before the hook's Rust fast-path, which exits 0 on any
commit with no staged `.rs` or Cargo file. A fixture-only commit is exactly
that kind of commit, so a check placed after the fast-path never runs when
it matters.

## Fail-closed rules

Unreadable input is refused: a tarball `tar` cannot list, a snapshot `jq`
cannot parse, an empty snapshot, or one that is not a JSON object. A
readable tarball with neither an `inspection-snapshot.json` nor an
entitlement member is not inspectah output and passes. Exit codes: 1
sensitive, 2 unreadable, 64 usage.

The hook reads staged blobs (`git cat-file blob :path`), not working-tree
files, so a clean copy left on disk over a staged sensitive one does not
pass.
