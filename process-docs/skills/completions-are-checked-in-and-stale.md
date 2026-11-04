# Shell completions are checked-in generated output, and they are stale

`completions/inspectah.{bash,fish,zsh}` are `clap_complete` output committed
to the tree. Nothing regenerates them in CI, in the pre-commit hook, or at
build time. **A change to any `clap` argument therefore does not reach the
shipped completions unless someone edits them by hand.**

## The trap

The obvious fix is to regenerate:

```
cargo build -p inspectah-cli
./target/debug/inspectah completions bash > completions/inspectah.bash
```

That is correct output, but **the checked-in files have drifted far enough
that regenerating sweeps unrelated changes into your diff.** Measured on
2026-11-04, while removing one flag: regeneration produced a 49-line diff
across the three files. Only three of those lines were the flag. The rest
were flags added to the CLI months earlier that never reached the
completions — `-y/--yes` on every subcommand, and `--exclude-path`,
`--scan-home`, `--scan-path`, `--include-unmanaged` on `scan`.

So a one-flag change becomes a review where the reviewer cannot tell the
intended edit from the backlog of drift.

## The rule

- **Changing one flag:** edit the three files by hand. Each flag appears
  exactly once per file, and the three formats are mechanical:
  - `inspectah.bash` — a word in the subcommand's `opts="..."` string.
  - `inspectah.fish` — one `complete -c inspectah -n "...using_subcommand
    <cmd>" -l <flag> -d '<help>'` line.
  - `inspectah.zsh` — one `'--flag[help]' \` line in the subcommand's
    `_arguments` block.
- **Deliberately catching the files up:** regenerate all three, and commit
  that on its own, separate from whatever change exposed the drift.

Either way, grep the three files for the flag name afterwards. A flag left
in a completion is a user-visible surface that lies.

## See Also

- [codebase-layout](codebase-layout.md) — where `completions/` sits in the tree.
- [release-process](release-process.md) — the release checklist, which does
  not currently regenerate completions either.
