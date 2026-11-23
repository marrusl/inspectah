# Shell completions are checked-in generated output, and a test holds them to the CLI

`completions/inspectah.{bash,fish,zsh}` are `clap_complete` output committed
to the tree. Nothing regenerates them at build time, but
`crates/cli/tests/completions_sync_test.rs` runs the built binary's hidden
`inspectah completions <shell>` for each shell and fails unless the output
equals the tracked file byte for byte. **A change to any `clap` argument
therefore fails the test suite until the three files are regenerated.**

## Why it is a test

The files once drifted for months with nothing checking them: regenerating
on 2026-11-04 produced a 49-line diff, mostly flags added to the CLI long
before (`-y/--yes` on every subcommand, four `scan` flags), and the files
kept offering `--tui` after the flag was removed.

`completions/` is not on `.githooks/docs-only-paths`, so the pre-commit
hook runs the cargo gates on a commit that changes only completions. That
is the cost of the check: a hand edit to a completion file that does not
match the CLI is refused at commit time, the same as a flag change that
does not reach the files.

## The rule

- **Changing a flag:** regenerate all three files in the same commit, from
  the repository root:

  ```
  for s in bash zsh fish; do cargo run -q -p inspectah-cli -- completions $s > completions/inspectah.$s; done
  ```

  The test's failure message prints the same command.
- **Do not hand-edit the files.** Any edit that is not generator output
  fails the test.

The RPM never used these files: `packaging/inspectah.spec` generates its
own from the built binary at `%install`.

## See Also

- [codebase-layout](codebase-layout.md): where `completions/` sits in the tree.
- [full-workspace-verification-gates](full-workspace-verification-gates.md):
  the hook and the docs-only list.
