---
title: Developer Guide
parent: Contributing
nav_order: 1
---

# Developer Guide

How to set up a development environment, build from source, run the
gates the way the pre-commit hook and CI run them, and get a change
merged. When each gate is required, when a real host or a browser pass
is required, and what a green result does not prove are on their own
page: [Testing](testing.md).

## Read first

Three files in the repository, in this order:

1. [`CLAUDE.md`](https://github.com/marrusl/inspectah/blob/main/CLAUDE.md)
   at the repo root: the project's conventions on one screen, including
   the clippy command, the commit format, schema versioning, and which
   directories hold specs rather than user docs.
2. [`process-docs/skills/codebase-layout.md`](https://github.com/marrusl/inspectah/blob/main/process-docs/skills/codebase-layout.md):
   a map of the workspace, crate by crate, with where commands, types,
   inspectors, renderers, tests and docs live.
3. [`process-docs/skills/index.md`](https://github.com/marrusl/inspectah/blob/main/process-docs/skills/index.md):
   the index of skill files, each recording a non-obvious pattern or
   correctness requirement that cost someone time to discover.

This project is developed with AI assistance, and the core team keeps
its harness setup in a private team repository.

## Prerequisites

| Tool | Why | Check |
|---|---|---|
| [rustup](https://rustup.rs/) and a C linker | `rust-toolchain.toml` pins the exact Rust release, with `clippy` and `rustfmt`, and rustup installs it on the first cargo command in the clone. The hook and CI run `cargo fmt`, `cargo clippy` and `cargo test` on that same release, so a new stable never fails CI on unchanged code. | `cargo --version`, `cargo clippy --version` |
| Node.js 26 and npm | The line CI pins; the web UI's vitest suite fails on Node 20. `crates/web/build.rs` runs `npm ci` and `npm run build` (`tsc && vite build`) whenever the web crate's build script reruns: on the first build, and on any `cargo build`, `cargo clippy` or `cargo test` after a change under `crates/web/ui/src/` or to the UI build files (`package.json`, `package-lock.json`, `vite.config.ts`, `tsconfig.json`, `index.html`), or after `INSPECTAH_SKIP_UI` changes. Setting or unsetting it reruns the build script, but the UI build runs only when it is unset. The script panics when `npm` is missing unless `crates/web/ui/dist/` already exists. Setting `INSPECTAH_SKIP_UI` skips the UI build for Rust-only work, but only once an earlier build has produced `dist/`, because the embedded asset folder must exist to compile. | `node --version` |
| `cargo-insta` | Reviewing snapshot changes. Install with `cargo install cargo-insta`. | `cargo insta --version` |
| Playwright's Chromium | The e2e suite only. Install from `crates/web/ui` with `npx playwright install chromium`. | Run the install command again: it exits 0 and prints nothing when Chromium is present. |
| A throwaway Linux VM you can revert | Real-host runs. See [Testing § Real-host runs](testing.md#real-host-runs). | None |
| `cargo-zigbuild` and `zig` | Release cross-compiles only. See § Cross-compilation. | `cargo-zigbuild --version` |

## Clone and build

```bash
git clone https://github.com/marrusl/inspectah.git
cd inspectah
git config core.hooksPath .githooks
cargo build
```

`git config core.hooksPath .githooks` is once per clone. Git only runs
hooks from `.git/hooks`, which is not tracked, so the committed
`.githooks/` directory does nothing until this setting points at it.
Skip it and nothing warns you; your commits go unchecked until a
reviewer notices.

The first `cargo build` compiles the six crates in dependency order
and, through `crates/web/build.rs`, runs `npm ci` and `npm run build`
for the web UI. The debug binary is `target/debug/inspectah`.

For a release build, use the command from
[Getting Started § From source](../getting-started.md#from-source):

```bash
cargo build --release -p inspectah-cli
```

The binary is `target/release/inspectah`.

## Workspace structure

inspectah is a Cargo workspace with six crates, all located under
`crates/` with the `inspectah-` prefix dropped from directory names (e.g.,
`crates/core/`, `crates/cli/`). The Cargo package names remain unchanged.

| Crate | Directory | Purpose |
|---|---|---|
| `inspectah-core` | `crates/core/` | Shared traits (`Inspector`, `Executor`, `Renderer`), type definitions, snapshot structures, and aggregate logic |
| `inspectah-collect` | `crates/collect/` | Inspector implementations: each inspector gathers data from one system domain |
| `inspectah-pipeline` | `crates/pipeline/` | Orchestrates collection, rendering, output generation, and build planning |
| `inspectah-refine` | `crates/refine/` | Refinement engine: triage classification, decision projection, aggregate consensus |
| `inspectah-web` | `crates/web/` | Web UI (refine interface) for interactive triage |
| `inspectah-cli` | `crates/cli/` | Command-line interface and progress display |

Dependencies flow downward: `inspectah-cli` depends on all other crates.
`inspectah-web` depends on `inspectah-refine`, `inspectah-pipeline`, and
`inspectah-core`. `inspectah-refine` depends on `inspectah-pipeline` and
`inspectah-core`.

<div id="diagram-software-architecture-contrib">
  <iframe
    src="../diagrams/software-architecture.html"
    width="100%"
    height="500"
    style="border: 1px solid #ddd; border-radius: 4px;"
    title="Software architecture diagram">
  </iframe>
</div>

*Software architecture diagram showing crate relationships and data flow.*

## Run the gates

The pre-commit hook runs these three, in this order, from the repo
root. CI runs the same three on every pull request.

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D clippy::all
cargo test --workspace
```

The clippy command is the one that rejects a commit; `CLAUDE.md` and CI
state the same one. Both flags matter. `--all-targets` makes clippy
check test targets as well as lib and bin, and every warning this tree
has carried lived in a test target. `-D clippy::all` denies the
`clippy::all` lint group, so the gate fails on any lint in it. Compiler
warnings such as an unused variable or a dead function still print and
do not block; fix them anyway.

Then the web UI toolchain, from `crates/web/ui`:

```bash
cd crates/web/ui
npm ci
npm run test
npm run build
npm run test:e2e
```

`npm run test` is the vitest unit suite. `npm run build` is
`tsc && vite build`, so it is the type check. `npm run test:e2e` is
Playwright: it starts Vite itself, and when no refine server answers on
port 8642 it skips the real-server spec, which still counts as green.
Read the `skipped` count, not just the exit code. The full suite also
fails locally today for a known reason that is not your change; both
facts are in
[Testing § What a green result does not prove](testing.md#what-a-green-result-does-not-prove).

### The insta flow

Several crates snapshot their output with [insta](https://insta.rs/). A
changed snapshot fails `cargo test`. Review it with:

```bash
cargo insta review
```

Accept only a change you intended, and commit the updated `.snap` file
beside its test. The snapshot directories are
`crates/cli/src/progress/snapshots/`, `crates/web/tests/snapshots/`,
`crates/collect/tests/snapshots/` and
`crates/pipeline/src/render/snapshots/`. On a clean tree the command
reports:

```
done: no snapshots to review
```

`cargo insta review` needs a terminal. Run it without one while a
snapshot is pending and it exits 1 with
`error: Interactive review requires a terminal` and lists the
non-interactive commands instead: `cargo insta pending-snapshots`,
`cargo insta accept` and `cargo insta reject`.

When each gate is required, and what a green result does and does not
prove, is on [Testing](testing.md).

## Where changes go

| Kind of change | Where it goes |
|---|---|
| Design spec | `process-docs/specs/proposed/`, moved to `process-docs/specs/implemented/` once shipped |
| Implementation plan | `process-docs/plans/` |
| Skill file: a non-obvious pattern or correctness requirement | `process-docs/skills/<topic>.md`, with a row in `process-docs/skills/index.md`. When a crate, command, or major directory changes, update `process-docs/skills/codebase-layout.md` too |
| Changelog entry | `CHANGELOG.md` under `## [Unreleased]`, per [`process-docs/changelog-policy.md`](https://github.com/marrusl/inspectah/blob/main/process-docs/changelog-policy.md): every user-visible change gets one; test-only, CI, docs, and internal refactors do not |
| User documentation | `docs/` (GitHub Pages). Never a spec or a plan |
| Rust tests | Beside the code: `#[cfg(test)]` modules in the source file, integration tests in `crates/*/tests/`, built as one binary per crate (cli keeps `units_scan_test` as a second binary): `tests/main.rs` declares each file as a module (`autotests = false`); run one file with `cargo test -p <package> --test integration <file stem>::`, and cli's `units_scan_test` with `cargo test -p inspectah-cli --test units_scan_test` |
| Web UI unit tests | `crates/web/ui/src/**/*.test.ts` and `*.test.tsx` (vitest) |
| Web UI end-to-end tests | `crates/web/ui/e2e/` (Playwright) |

Two rules that cost people time:

- **A change that alters what the snapshot serializes bumps the
  schema.** Snapshot JSON carries `schema_version`, and loading is an
  exact match, not a window. Removing, renaming, or retyping a field, or
  adding a variant to a serialized enum such as `InspectorId`, bumps it;
  a new `Option` field with `#[serde(default)]`, or a new `SectionData`
  variant, does not.
  [`snapshot-schema-versioning.md`](https://github.com/marrusl/inspectah/blob/main/process-docs/skills/snapshot-schema-versioning.md)
  in the repository says where the version literals live.
- **A flag change regenerates all three files under `completions/`.**
  `completions/inspectah.bash`, `.fish` and `.zsh` are checked-in
  `clap_complete` output, and a test fails unless each one equals what
  `inspectah completions <shell>` prints. Regenerate them in the same
  commit, from the repository root:
  `for s in bash zsh fish; do cargo run -q -p inspectah-cli -- completions $s > completions/inspectah.$s; done`.
  [`completions-are-checked-in-and-sync-tested.md`](https://github.com/marrusl/inspectah/blob/main/process-docs/skills/completions-are-checked-in-and-sync-tested.md)
  has the background.

## What rejects a commit

Two hooks run once `core.hooksPath` points at `.githooks/`.

**`pre-commit`** first checks every staged tarball for preserved
sensitive content (§ Fixture tarballs are local-only below), on every
commit. It then decides whether the commit is docs-only: every path the
commit adds, changes, deletes or renames matches the list in
`.githooks/docs-only-paths`, one anchored extended regular expression
per line:

```text
^docs/
^process-docs/
^\.github/
^packaging/
^\.copr/
^[A-Za-z_]+\.md$
^LICENSE$
```

The hook reads the list from the index being committed, so an edit to
the list takes effect only once it is staged. A docs-only commit, or an
empty one, prints `pre-commit: docs-only, cargo gates skipped` and
passes. Every other commit is gated, including one that only deletes a
file, renames one, or changes a `.snap` file, a fixture under
`testdata/`, or the web UI. For a gated commit the hook:

1. Refuses while any path outside the list is edited and not staged,
   untracked and not ignored, or an ignored `.rs` or `Cargo.toml` file
   under `crates/`, which the gates would compile but the commit would
   leave out. It names every such path: commit it, delete it, or ignore
   it (an ignored Rust source can only be committed or deleted).
2. Runs three gates, in order: `cargo fmt --check`,
   `cargo clippy --workspace --all-targets -- -D clippy::all`, and
   `cargo test --workspace`. Each prints one line, such as
   `pre-commit: clippy ok (41 s)`. The first to fail prints its output
   and blocks the commit.
3. Repeats the refusal in step 1, and also refuses if the index changed
   while the gates ran.

Four things follow.

- **The gates test your commit, except for ignored files outside
  `crates/**/*.rs` and `crates/**/Cargo.toml`.** They run on the files
  on disk, and steps 1 and 3 refuse whenever those could differ from
  what you are committing. That holds for `git commit -a` and
  `git commit --only <paths>` too: git gives the hook the index it
  builds for that commit, and the hook compares against it. The hook
  does not look for other ignored files, so an ignored build input of
  another kind, such as a template or a UI asset, can still be tested
  without being committed.
- **A docs-only commit is never refused for other files.** Unstaged or
  untracked code beside it does not block it, so a docs fix can land
  while a code change is in progress in the same clone.
- **`INSPECTAH_SKIP_UI` has no effect on the hook.** The hook unsets it,
  so the gates never embed a stale `crates/web/ui/dist/`. A commit that
  changes the web UI runs `tsc && vite build` through
  `crates/web/build.rs` inside the gates. Vitest and Playwright run only
  in CI.
- **The hook exists only if you installed it**, and
  `git commit --no-verify` bypasses it. Use `--no-verify` as a last
  resort only.

To gate every commit of a rebase, run
`git rebase --exec '.githooks/pre-commit --head'`: in that mode the hook
checks the paths `HEAD` changed (for a merge, its changes against every
parent), and refuses while the index differs from `HEAD` outside the
docs-only list, so the tree it tests is `HEAD`. The hook's own tests are
`scripts/tests/pre-commit-hook-test.sh`, and
`scripts/tests/docs-only-paths-test.sh` checks that no Rust source under
`crates/` reads a docs-only path.

Staged sets and the hook's verdict:

| Staged | Hook |
|---|---|
| `docs/contributing/testing.md` only | skips the gates |
| a `docs/` edit, with `crates/core/src/lib.rs` edited and not staged | skips the gates and passes |
| `crates/web/ui/src/App.tsx` only | runs all three gates |
| `Cargo.lock` only | runs all three gates |
| `crates/core/src/lib.rs` edited, with a clippy fix left unstaged | refuses before the gates, naming `crates/core/src/lib.rs` |
| `crates/core/src/lib.rs`, with an untracked `crates/core/src/new.rs` | refuses before the gates, naming `crates/core/src/new.rs` |
| a deleted `.rs` file only | runs all three gates |
| `crates/web/tests/snapshots/integration__contract_snapshots__contract_view.snap` only | runs all three gates |
| `testdata/e2e-fixture.tar.gz` only, produced with `--preserve` | refuses, before the docs-only check |

### Fixture tarballs are local-only

A scan run with `--preserve`, or with `--no-redaction`, keeps entitlement
certificates and keys, password hashes or SSH keys in its output. Such
tarballs, including the rich fleet tarballs the companion fixture tool
produces by default, are for local use only. A preserve-mode tarball was
once copied onto a tracked fixture path on purpose and committed;
`.gitignore` cannot stop a deliberate copy, so the control is a content
check, `scripts/check-sensitive-tarballs.sh`. The pre-commit hook runs it
over staged blobs and CI runs it over every tracked file. Both check files
named `*.tar.gz`, `*.tgz` or `*.tar`, and any other file whose content
starts with the gzip magic, since refine loads a gzip archive whatever it
is called.

It refuses a tarball when a member path contains `subscription/entitlement/`,
or when its `inspection-snapshot.json` has `redaction_state.state` set to
`sensitive_retained` or `sensitive_snapshot` set to `true`. It also
refuses a tarball it cannot read, or whose snapshot it cannot parse, and
a gzip file that is not a tar archive. A tarball with neither a snapshot
nor an entitlement member is not inspectah output and passes.

The fix is to re-produce the fixture without `--preserve` and without
`--no-redaction`. To commit one to a private repository on purpose,
bypass the hook with `git commit --no-verify`. Never bypass it in a clone
that pushes to a public remote. CI runs the same check, but only after a
push: on `main` the content is already public by then, and a red run
reports the leak without undoing it. On a pull request CI can stop the
merge, not the upload. The hook is the only check that runs before
anything leaves your clone. Run the gate's tests with
`scripts/tests/check-sensitive-tarballs-test.sh`.

**`commit-msg`** refuses any message carrying a `Co-Authored-By:`
trailer. AI assistance is recorded as one trailer naming both the tool
and the model, `Assisted-by: <tool> (<model>)`, for example
`Assisted-by: Claude Code (Opus 5)`. Several AI coding tools add the
refused trailer on their own; remove the line and commit again.

**Commit format** is conventional: `type(scope): description` in
imperative mood, with a body that explains why, not what. Common types:
`feat`, `fix`, `refactor`, `test`, `docs`, `chore`. As a template:

```
type(scope): description in imperative mood

Body explaining why, not what.

Assisted-by: <tool> (<model>)
```

Pass the message with `git commit -F <file>`, never through a pipe and
never as `-F -`: a commit that reads its message from stdin with nothing
piped in blocks silently, and one in this repo's history sat for 17
hours that way
([`full-workspace-verification-gates.md`](https://github.com/marrusl/inspectah/blob/main/process-docs/skills/full-workspace-verification-gates.md)
§ 5).

## PR process

1. Create a feature branch from `main` (or from an existing feature
   branch for large efforts).
2. Make focused commits: one logical change per commit when practical.
3. Run the gates in § Run the gates before pushing, including the web UI
   ones when you touched `crates/web/ui/` or a web DTO or handler under
   `crates/web/src/`.
4. Open a pull request against `main`. CI runs the same gates on every
   pull request.
5. Address review feedback in follow-up commits (don't force-push during
   review).
6. Add the changelog entry under `## [Unreleased]` when the change is
   user-visible (§ Where changes go).

## Cross-compilation

Release binaries are static musl builds made with `cargo-zigbuild`. Add
each target once with `rustup target add <triple>`. For Linux ARM64:

```bash
cargo zigbuild --target aarch64-unknown-linux-musl --release
```

For Linux x86_64:

```bash
cargo zigbuild --target x86_64-unknown-linux-musl --release
```

The release checklist, including binary naming and publishing, is
[`process-docs/skills/release-process.md`](https://github.com/marrusl/inspectah/blob/main/process-docs/skills/release-process.md)
in the repository.
