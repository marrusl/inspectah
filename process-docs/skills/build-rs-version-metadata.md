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
| Release profile, no git metadata (source tarball, SRPM, COPR) | `<version>` | `inspectah 0.9.0-beta.8` |
| Debug or test | `<version>+dev` | `inspectah 0.9.0-beta.8+dev` |

The suffix is semver build metadata. A bare version from a git checkout
means HEAD carried the release tag, and the release workflow relies on that
(see `release-process.md` § Binary verification). It does not mean more
than that, in two cases:

- **Uncommitted changes are ignored.** A dirty checkout at the tag prints
  the bare version, and a dirty untagged checkout prints HEAD's hash.
- **A build without git metadata prints the bare version**, whatever
  commit the source came from. See the Gotchas.

## Mechanism
- **build.rs lives in the binary crate**, not the workspace root. Only the
  crate that declares `[[bin]]` runs its build script for the final binary.
- **`VERSION_LINE` in `crates/cli/src/commands/version.rs`** is
  `concat!("inspectah ", env!("INSPECTAH_VERSION"), "\n")`; `inspectah
  version` prints it. The newline is in the constant on purpose: the binary
  then stores the line with a terminator, and `check-version-stamp.sh
  bytes` requires that byte. It searches with perl, not grep: grep's `$`
  also matches at end of input, so a file ending in the bare version with
  no newline would pass. Without the newline the version runs straight into the next
  string literal, so `inspectah 0.9.0-beta.4` would also match a
  `0.9.0-beta.40` binary. clap's `version = env!("INSPECTAH_VERSION")` in `main.rs`
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

`packed-refs` is watched even when it does not exist, so a tag that first
appears in a newly created `packed-refs` still reruns the script. The cost:
cargo treats a missing watched path as changed (`cargo build -v` says "the
file `packed-refs` is missing"), so until the file exists every release
build reruns the script and recompiles cli. A clone, `git gc` or `git
pack-refs` creates it, and a CI build runs cargo once, so in practice only
a fresh `git init` pays. `refs` is a directory and cannot cover this: it
does not contain `packed-refs`, and watching the whole git dir would rerun
on every index or object write.

`scripts/tests/cli-build-version-test.sh` copies `build.rs` into a
throwaway crate in a throwaway repository with no `packed-refs`, then
changes only `packed-refs` between release builds: created with the
matching tag, a different tag, the matching tag again, no tag. Each step
must flip the printed version. It runs in CI (`rust-ci.yml`), not in the
pre-commit hook.

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
- **A build with no git metadata prints the bare version.** When `git
  rev-parse HEAD` fails for any reason (no `.git`, no `git` binary), the
  script returns the bare version. The RPMs build from a `git archive`
  tarball with no `.git`. `package-release.yml` makes it from the tag
  checkout; COPR runs `.copr/Makefile`, which archives whatever HEAD COPR
  checked out, tagged or not; an SRPM rebuild uses the tarball inside it.
  All of them print the bare version. If the tarball is unpacked inside
  some other git checkout, git finds that repository instead, and the hash
  names its HEAD.
- **Use `env!()`, not `option_env!()`.** A missing variable is then a
  compile error rather than a silent empty version.
