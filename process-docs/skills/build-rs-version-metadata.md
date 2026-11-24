# Build-time version line via build.rs

## Context
`inspectah version` and `inspectah --version` print one line whose form
says what kind of build the binary is. Cargo knows only the package
version, so `crates/cli/build.rs` works out the rest from git and emits it
as `INSPECTAH_VERSION`.

| Build | `INSPECTAH_VERSION` | Printed line |
|---|---|---|
| Release profile, HEAD carries the tag `v<version>` | `<version>` | `inspectah 0.9.0-beta.8` |
| Release profile, any other commit | `<version>+g<short hash>` | `inspectah 0.9.0-beta.8+g5a4389ce` |
| Release profile, no git (source tarball, RPM) | `<version>` | `inspectah 0.9.0-beta.8` |
| Debug or test | `<version>+dev` | `inspectah 0.9.0-beta.8+dev` |

The suffix is semver build metadata. A bare version is the claim "built
from the release tag", and the release workflow relies on it (see
`release-process.md` § Binary verification).

## Mechanism
- **build.rs lives in the binary crate**, not the workspace root. Only the
  crate that declares `[[bin]]` runs its build script for the final binary.
- **`VERSION_LINE` in `crates/cli/src/commands/version.rs`** is
  `concat!("inspectah ", env!("INSPECTAH_VERSION"))`; `inspectah version`
  prints it. clap's `version = env!("INSPECTAH_VERSION")` in `main.rs`
  makes `--version` print the same line, since clap prefixes the command
  name. A test compares the two. `concat!()` and `env!()` give the
  `&'static str` clap's derive needs; `format!()` would not compile there.
- **"Tagged" means `git tag --points-at <HEAD>` lists `v<CARGO_PKG_VERSION>`.**
  No environment variable is involved, so a local build and a CI build of
  the same commit agree. HEAD is resolved once with `git rev-parse --verify
  HEAD`, and the tag check and the short hash both use that id, so they
  cannot name different commits.
- **Not `git describe`.** Between the version bump and the tag it names the
  previous tag while Cargo names the new version.

## Rerun triggers
A release build emits `rerun-if-changed` for three paths that
`git rev-parse --path-format=absolute --git-path <p>` resolves: `HEAD`,
`refs` and `packed-refs`. `--git-path` is what makes this work in a linked
worktree, where `.git` is a file: `HEAD` is in the worktree's own git dir,
`refs` and `packed-refs` in the common one. Cargo scans a watched directory
recursively, so creating or deleting a tag under `refs/tags`, or a commit
moving a branch, reruns the script. Measured 2026-11-23 in a linked
worktree: `git tag v<version>` on HEAD made the next release build rerun the
script and print the bare version, `git tag -d` brought the `+g` form
back, and a build with nothing changed stayed `Fresh`.

A watched path that does not exist makes cargo rerun on every build, so
`packed-refs` is watched only when present. A tag that lands only in
`packed-refs` when that file did not exist at the last build is missed;
`touch crates/cli/build.rs` forces the rerun.

## Debug builds read no git
Debug and test builds emit `<version>+dev` and watch only `build.rs`.
Watching `.git` in dev builds rebuilt cli far too often: in a linked
worktree the old `../../.git/HEAD` watch never existed, so cargo reran the
script on every invocation; in the main checkout every commit moved
`.git/refs` and rebuilt cli once per commit (about 17 to 20 s, measured
2026-11-19). A test pins the `+dev` value.

## Gotchas
- **No build date.** The line used to carry one. A real date in dev
  builds would need a rerun on every build or go stale, and in release
  builds it added nothing the tag or hash does not say.
- **A build with no git prints the bare version.** The RPMs build from a
  `git archive` tarball with no `.git`, from the tag, so bare is right for
  them; any other tarball build also prints bare. If the tarball is
  unpacked inside some other git checkout, git finds that repository
  instead, and the hash names its HEAD.
- **Use `env!()`, not `option_env!()`.** A missing variable is then a
  compile error rather than a silent empty version.
