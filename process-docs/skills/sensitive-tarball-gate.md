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

## Which files are checked

`--staged` and `--tracked` check every file named `*.tar.gz`, `*.tgz` or
`*.tar`, and every other regular file whose first two bytes are the gzip
magic `1f 8b`. refine loads a gzip archive whatever it is called, so a
name filter alone let a renamed preserve-mode tarball through. `tar` runs
without `-z` so it detects gzip itself and reads a bare `.tar` as well.

The magic is read with a builtin `read` in the C locale, not a process
per file, because `--tracked` sniffs the whole tree. `--staged` sniffs the
working-tree copy of each staged file and runs `git cat-file` only for
candidates and for paths whose working-tree copy differs from the staged
blob (`git diff --name-only`). Running git once per staged file cost tens
of milliseconds each on macOS. Symlinks and gitlinks are skipped: their
blob is a target path or a commit id, never an archive.

Known limits, by design of a basename-based check, not fixed:

- A tarball nested inside another tarball is not opened.
- A snapshot member renamed away from `inspection-snapshot.json`, in an
  archive with no `subscription/entitlement/` member, passes.
- A renamed uncompressed tar (no tar extension, no gzip magic) is not a
  candidate. refine does not load one.

The gate catches an accidental or careless copy of real scan output. It is
not a defence against someone concealing an archive on purpose.

## Two shell traps

- **Put `--` before every member name passed to `tar`.** Member names come
  from the archive. Without `--`, a member named
  `--checkpoint-action=exec=<command>/inspection-snapshot.json` passes the
  basename match and becomes a GNU tar option, and GNU tar runs the
  command while it reads, with no `--checkpoint` needed. bsdtar on macOS
  rejects the option instead, so the test's marker file only proves
  anything under GNU tar (CI, or a Fedora container); on macOS the exit
  status half of that test still fails without `--`.
- **Never pipe into `grep -q` or `head` under `pipefail`.** When grep
  exits on an early match and the producer is still writing, the producer
  dies on SIGPIPE, the pipeline fails, and an `if` reads the match as no
  match. A tarball listing past the 64 KiB pipe buffer, with the
  entitlement member first, passed the gate this way. Match with `case`
  or a here-string (`grep -q ... <<<"$var"`). The hook's Rust fast path had
  the same bug, which skipped every cargo gate on a long staged list. A
  test for this must fix the member order; directory order differs between
  file systems and put the match last on some.

## Ordering in the hook

The check runs before the hook's Rust fast-path, which exits 0 on any
commit with no staged `.rs` or Cargo file. A fixture-only commit is exactly
that kind of commit, so a check placed after the fast-path never runs when
it matters.

## Fail-closed rules

Unreadable input is refused: a tarball `tar` cannot list, a gzip file
that is not a tar archive, a snapshot `jq` cannot parse, an empty
snapshot, or one that is not a JSON object. A readable tarball with
neither an `inspection-snapshot.json` nor an entitlement member is not
inspectah output and passes. A missing `jq` is a usage error, so it is not
reported as an unparseable snapshot. Exit codes: 1 sensitive, 2
unreadable, 64 usage.

## The bypass

`git commit --no-verify` skips the hook. CI runs the same check, but only
after a push: on `main` the content is already public, and on a pull
request CI can stop the merge, not the upload. CI is a backstop, not a
protection. Message text that says otherwise is wrong.

The hook reads staged blobs (`git cat-file blob :path`), not working-tree
files, so a clean copy left on disk over a staged sensitive one does not
pass.
