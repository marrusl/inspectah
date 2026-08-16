# Skill: Release Process

How to cut an inspectah release. This captures the full checklist and
non-obvious gotchas discovered during actual releases.

## Version bump locations

Two files need version bumps:

1. **Root `Cargo.toml`** -- `[workspace.package]` section (line ~14).
   All crates inherit via `version.workspace = true`, so only the root
   needs changing. Run `cargo check` afterward to regenerate `Cargo.lock`.

2. **RPM spec** -- `packaging/inspectah.spec`, the `Version:` field.
   RPM uses tilde for pre-release: `0.8.6~beta.5`, not `0.8.6-beta.5`.
   The tilde sorts *before* the release version in RPM, so
   `0.8.6~beta.5 < 0.8.6`. Using a hyphen would break RPM ordering.

**`ROADMAP.md` is *not* part of this list.** It has a "Current version"
line near the top that looks like a sync-rule candidate, but history
shows it's bumped in a separate `docs(roadmap):` commit, not the release
commit -- leave it alone when preparing a release commit.

That separate commit does not reliably happen on its own: it was
skipped entirely for beta.1, leaving the line two releases stale by the
time beta.2 shipped. There's no trigger that reminds anyone to do it,
so check it every release rather than assuming a prior cut caught it.
The line also carries a schema number in parentheses (e.g.
`v0.9.0-beta.2 (pure Rust, schema 22)`) -- update that against
`SCHEMA_VERSION` in `crates/core/src/snapshot.rs` too, don't just swap
the version string. It has drifted from the actual value before.

## CHANGELOG.md

Move all entries from `## [Unreleased]` into a new dated section:
`## [0.8.6-beta.5] - YYYY-MM-DD`. Leave an empty `## [Unreleased]`
at the top. Update the comparison links at the bottom of the file --
add a new link for the release version and point `[Unreleased]` at the
new tag.

## Release notes

Create `process-docs/release-notes-<version>.md`. Follow the format
from the most recent release notes file. Key sections:

- Thematic groupings of changes (not just a flat list)
- "Also included" section if a prior tag was never released on GitHub
- Schema version section -- state the current value even when unchanged
  (readers of the previous release's notes will look for it). The
  source of truth is `SCHEMA_VERSION` in `crates/core/src/snapshot.rs`,
  not the CHANGELOG or ROADMAP, both of which have carried stale or
  inconsistent schema numbers.
- Binaries section listing all 3 platforms
- Full changelog comparison link at the bottom, of the form
  `.../compare/<previous-tag>...<this-tag>` -- read it off the
  `CHANGELOG.md` comparison links at the bottom of that file rather
  than reconstructing it, since skipped/rolled versions (see Gotchas)
  make the previous tag not always "the last released version."

**This is a separate commit from the release commit, written after Mark
has already tagged.** `gh release create` needs the file at tag time,
but the version-bump commit that gets tagged does not include it --
both the beta.1 and beta.2 cuts wrote it as a standalone follow-up
commit, `docs(release): write v<version> release notes`, landing after
the release commit (and after any skill-fix commits on top of it).
Do not fold it into the release commit or block the tag on it.

## Build targets

Three binaries, all built from the `inspectah-cli` crate:

```bash
# macOS ARM64 (native)
cargo build --release -p inspectah-cli

# Linux x86_64 (static musl via zigbuild)
cargo zigbuild --target x86_64-unknown-linux-musl --release -p inspectah-cli

# Linux ARM64 (static musl via zigbuild)
cargo zigbuild --target aarch64-unknown-linux-musl --release -p inspectah-cli
```

Requires `cargo-zigbuild` (`cargo install cargo-zigbuild` if missing),
`zig` on `PATH`, and both musl targets added via `rustup target add
x86_64-unknown-linux-musl aarch64-unknown-linux-musl`. Verify all four
before starting rather than discovering a gap mid-build.

**Never set `INSPECTAH_SKIP_UI=1` for these builds.** That escape hatch
exists only for the release *commit*, whose pre-commit hook touches no
frontend source. A release binary must embed the built frontend, so
`crates/web/build.rs` has to run `npm ci` and `npm run build` for real.
A binary built with the variable set ships without a UI and the failure
is invisible until someone runs `refine`.

**`npm ci` needs network and will hang under the default command
sandbox.** Run the cargo builds with the sandbox disabled. A build that
appears to hang with no output is this, not a slow compile -- do not
wait it out. Note `npm ci` deletes and reinstalls `node_modules`, and
it runs once per target triple, so a three-target release pays that
cost three times.

Timings: a cold build (after `cargo clean`) measured 73s / 67s / 69s
per target, 209s total, on an M-series Mac. The earlier "30-60 seconds
each" figure was a warm-build number; budget roughly double for cold.

## Binary naming and staging

Copy binaries from build output to release names in the repo root:

| Build output path | Release name |
|---|---|
| `target/release/inspectah` | `inspectah-darwin-arm64` |
| `target/x86_64-unknown-linux-musl/release/inspectah` | `inspectah-linux-amd64` |
| `target/aarch64-unknown-linux-musl/release/inspectah` | `inspectah-linux-arm64-bin` |

The `-bin` suffix on ARM64 Linux distinguishes it from the macOS ARM64
binary (both are `aarch64` but different platforms).

Verify the staged files rather than trusting build exit codes -- the
release names persist between releases, so a copy that silently didn't
happen leaves the *previous* release's binaries sitting under the right
names, ready to be uploaded under the new tag:

```bash
file inspectah-darwin-arm64 inspectah-linux-amd64 inspectah-linux-arm64-bin
./inspectah-darwin-arm64 version
shasum -a 256 inspectah-* | tee /dev/stderr
```

Both Linux binaries must report `statically linked`. Confirm the version
string is the version being released, not the previous one.

### The embedded commit hash is never the tag commit

`crates/cli/build.rs` stamps the binary with `git rev-parse --short HEAD`
at compile time, and `inspectah version` prints it:

```
inspectah 0.9.0-beta.2 (commit c52b02bd, built 2026-08-16)
```

Because release notes are written as a *separate commit after* tagging
(see "Release notes" above), HEAD is always ahead of the tag by the time
the binaries get built. This is structural, not a mistake, and it has
happened on every recent release:

| Release | Tag commit | Binary stamp | Delta |
|---|---|---|---|
| v0.9.0-beta.1 | `ed39bbd6` | `d4857478` | 2 docs commits above tag |
| v0.9.0-beta.2 | `dfc32f4f` | `c52b02bd` | 4 docs commits above tag |

Acceptable only while those intervening commits are docs-only. **Confirm
that before building** -- if this returns anything, the binaries would
ship code that is not in the tag:

```bash
git diff --stat <tag>..HEAD -- crates/ Cargo.toml Cargo.lock
```

**Also confirm all three binaries carry the same stamp.** Agents commit
docs concurrently, so HEAD can move *between* targets and give the three
binaries different hashes. Compare the `version` line across all three,
not just the native one.

## Pre-commit checks

Run these yourself and read the output before committing the release.
Do not trust exit codes here: `-W` warns without failing, and the
pre-commit hook inherits that, so its clippy step cannot fail.

```bash
cargo build --workspace
cargo fmt --check
cargo clippy --workspace --all-targets -- -W clippy::all
cargo test --workspace
```

**`--all-targets` is required.** Every clippy warning in this tree lives
in a test target, so plain `cargo clippy` (lib and bin only) reports
zero and sees nothing. At v0.9.0-beta.2 the baseline is **nine**
pre-existing warnings -- 4 in `inspectah-web` (test `contract_snapshots`),
1 in `inspectah-web` (lib test), 2 in `inspectah-cli` (bin test), 2 in
`inspectah-pipeline` (lib test). Compare against that count; a release
commit should introduce none. Note this baseline is in tension with the
`-D clippy::all` standard in `CLAUDE.md`, which those nine would fail.

Build and test are part of the gate even though a release commit changes
no source: the version bump rewrites `Cargo.lock` and every crate
version, so confirm the workspace still builds and the suite still
passes at the bumped version. v0.9.0-beta.2 was 2784 passed, 0 failed,
6 ignored; v0.9.0-beta.3 was 2785 passed, 0 failed, 6 ignored.

Two sandbox traps make a clean tree look broken. Neither is a real
failure, and both cost time on more than one release:

- **`npm ci` hangs.** The `inspectah-web` `build.rs` runs it and it needs
  network. Set `INSPECTAH_SKIP_UI=1` on `git commit`, and on the gate
  commands above, when the release commit touches no frontend source.
  Do not reach for `--no-verify`. (Never set it for release *binaries*
  -- see "Build targets".)
- **`refine_e2e_test::refine_server_lifecycle` fails with
  `PermissionDenied`.** The test binds a TCP listener, which the sandbox
  refuses:
  `Os { code: 1, kind: PermissionDenied, message: "Operation not permitted" }`.
  `cargo test` aborts at that point, so the run also reports a *partial*
  pass count (191, not 2785) and looks like a mass failure. It is
  pre-existing and unrelated to any release change. Run the gate and the
  commit with the sandbox disabled, which also lets the pre-commit hook's
  real test gate run and pass.

## Commit and tag

The release commit is exactly four files. "All release files" is
narrower than it sounds -- the release notes and the ROADMAP line are
deliberately *not* in it (see their sections above), so a release cut
is three commits, not one:

| Commit | Files |
|---|---|
| `chore(release): v<version>` | `Cargo.toml`, `Cargo.lock`, `packaging/inspectah.spec`, `CHANGELOG.md` |
| `docs(release): write v<version> release notes` | `process-docs/release-notes-<version>.md` |
| `docs(roadmap): bump current version to v<version>` | `ROADMAP.md` |

**Stage the release commit by explicit path.** Other agents commit to
this repo concurrently, and a plans or skills file being edited in
another session will be sitting modified in the working tree while you
cut the release. `git commit -a` or `git add -A` would sweep it into
the tagged commit. This is not hypothetical: it happened during the
beta.3 cut, where a `docs(plans):` commit from another session landed
*between* the release commit and the release-notes commit.

Tag format is v-prefixed: `git tag v0.8.6-beta.5`.

Before tagging, confirm nothing above the release commit touches code,
since the tag point is the release commit and not HEAD:

```bash
git diff --stat <release-commit>..HEAD -- crates/ Cargo.toml Cargo.lock
```

**Do not push.** Mark reviews and pushes commit + tag.

## GitHub release

After Mark pushes the tag:

```bash
gh release create v0.8.6-beta.5 \
  inspectah-darwin-arm64 \
  inspectah-linux-amd64 \
  inspectah-linux-arm64-bin \
  --title "v0.8.6-beta.5" \
  --prerelease \
  --notes-file process-docs/release-notes-0.8.6-beta.5.md
```

Use `--prerelease` for any beta/alpha/rc tag. Omit for stable releases.

## Homebrew formula

Update `homebrew-inspectah/Formula/inspectah.rb`:

- Version string
- Download URL (points to the new GH release asset)
- SHA256: `shasum -a 256 inspectah-darwin-arm64`

The formula can be *committed* before the GitHub release exists, but
**must not be pushed until after it does** -- the `url` points at an
asset that 404s until `gh release create` runs, and a pushed tap in that
state is broken for every user who taps it.

The tap lives outside the agent sandbox's write allowlist (unlike
inspectah/driftify/osfragment-assemble), so `git` operations there fail
with `Unable to create '.git/index.lock': Operation not permitted`.
Run them with the sandbox disabled.

The formula's `test` block asserts `version.to_s` appears in
`inspectah version` output. That holds as long as the version string in
the formula exactly matches `CARGO_PKG_VERSION` -- note the formula uses
the hyphenated form (`0.9.0-beta.2`), not the RPM tilde form.

## Gotchas

- **Tag must be on remote before `gh release create`.** The command
  fails if the tag only exists locally. Mark must push first.

- **RPM spec uses tilde, not hyphen.** `0.8.6~beta.5` sorts correctly
  in RPM; `0.8.6-beta.5` would sort *after* `0.8.6` (wrong).

- **Skipped releases happen.** beta.4 was tagged but never released on
  GitHub. When this happens, roll the unreleased changes into the next
  version. The CHANGELOG keeps the skipped version's section as-is;
  the release notes for the new version mention "also included from
  beta.N" for visibility.

- **Cargo.lock regeneration.** After bumping `Cargo.toml`, run
  `cargo check` to update `Cargo.lock`. Don't forget to stage it.

- **Binary names are in .gitignore.** The staged binaries in the repo
  root are not tracked by git. They exist only for the `gh release
  create` upload step.
