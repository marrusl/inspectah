---
name: git-hook-shell-pitfalls
description: Git and bash behaviors that made the pre-commit hook and the docs-only read check fail open, each with the fix in place: set -e is off inside a function called from `||`, git quotes non-ASCII path names unless core.quotePath is off, diff-tree prints nothing for a merge without -m, and --exclude-standard hides ignored files that are still build inputs.
---

# Git and Shell Pitfalls in the Hook Scripts

`.githooks/pre-commit` and `scripts/tests/docs-only-paths-test.sh` each
decide whether a commit is tested. Every item below once let one of them
pass something it should have refused. Check a change to either script
against this list.

## 1. `set -e` does nothing inside a function called from `||`

`check_tree ... || rc=$?` runs `check_tree` in a context where `set -e` is
suppressed, for the whole call, nested functions and all. A failed
`files=$(git ls-files ...)` inside it no longer ends the function: the
assignment fails, the next line runs with an empty `$files`, and an
"empty means nothing to check" branch returns 0. The same applies to
`if f; then`, `f && ...` and `! f`.

A function meant to be called that way checks every status itself:
`if ! files=$(git ...); then echo ...; return 1; fi`, and
`raw=$(cat "$1") || return 1`. Keep "the producer failed" and "the
producer printed nothing" separate, and make the empty case fail too when
an empty result means the check looked at nothing.

## 2. git quotes non-ASCII path names by default

With the default `core.quotePath=true`, `git diff --name-only`,
`ls-files` and `diff-tree` print `docs/café.md` as
`"docs/caf\303\251.md"`, quotes included, so `^docs/` does not match it.
The hook lists paths through `git_paths` (`git -c core.quotePath=false`),
which prints UTF-8 names as they are. git still quotes a name holding a
double quote, a backslash or a control character; such a name keeps its
leading `"`, fails every pattern in the docs-only list, and is gated or
refused. That is the safe direction: never strip the quotes to "fix" it.

## 3. `git diff-tree` prints nothing for a merge commit

`git diff-tree -r --root HEAD` on a merge prints no paths, because without
`-m` (or `-c`/`--cc`) diff-tree does not diff a commit with more than one
parent. An empty path list reads as docs-only, so `--head` would skip
every merge, including one whose resolution changed code. The hook uses
`diff-tree -m`, which lists the changes against each parent (a path may
appear twice); for an ordinary or root commit the output is unchanged.

## 4. `--exclude-standard` hides ignored build inputs

`git ls-files --others --exclude-standard` lists untracked files that are
not ignored. An ignored file the compiler still reads is invisible to it.
`.gitignore` has an unanchored `build/` rule (needed for other build
output), so `crates/<crate>/src/build/mod.rs` is ignored unless re-included,
as `!crates/pipeline/src/build/` does for the one module that exists. The
hook also runs
`git ls-files --others --ignored --exclude-standard -- ':(glob)crates/**/*.rs' ':(glob)crates/**/Cargo.toml'`
and refuses what it lists. Ignored files outside that set are not checked.

## Related

- `full-workspace-verification-gates.md` § 1: what the hook gates and refuses.
- `sensitive-tarball-gate.md`: gate 0, and the SIGPIPE trap with `producer | grep -q`.
