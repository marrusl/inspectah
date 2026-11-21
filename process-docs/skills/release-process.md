# Skill: Release Process

How to cut an inspectah release. This captures the full checklist and
non-obvious gotchas discovered during actual releases.

## Version bump locations

Two files need version bumps:

1. **Root `Cargo.toml`** -- `[workspace.package]` section (line ~13).
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

Create `process-docs/release-notes/release-notes-<version>.md`. Follow the format
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
has already tagged.** The file is read when Mark publishes the draft
release (see "GitHub release"), not by the build, so the tagged commit
does not need it: write it as a standalone follow-up commit,
`docs(release): write v<version> release notes`, landing after the
release commit (and after any skill-fix commits on top of it). Do not
fold it into the release commit or block the tag on it.

## Build targets

Release binaries are built by CI, never by hand. Pushing a `v*` tag runs
`.github/workflows/release.yml`, which calls
`.github/workflows/build-binary.yml` to build three binaries from the
tag. `build-binary.yml` is the same workflow that smoke-builds every
push to `main`, so the release recipe is exercised long before a tag.
Each leg builds natively on a runner of its own architecture:

| Target | Runner | Release asset |
|---|---|---|
| `aarch64-apple-darwin` | `macos-latest` | `inspectah-darwin-arm64` |
| `x86_64-unknown-linux-musl` | `ubuntu-24.04` | `inspectah-linux-amd64` |
| `aarch64-unknown-linux-musl` | `ubuntu-24.04-arm` | `inspectah-linux-arm64-bin` |

Each leg runs `cargo build -p inspectah-cli --release --target <target>`,
with `musl-tools` installed on Linux, Rust from `rust-toolchain.toml` and
Node on the same major line as `rust-ci.yml`. The `-bin` suffix on ARM64
Linux distinguishes it from the macOS ARM64 binary (both are `aarch64`
but different platforms).

**The asset names are a published contract.** The Homebrew formula's
`install` is `bin.install "inspectah-darwin-arm64" => "inspectah"`, and
the `ASSETS` list in `release.yml` must match the matrix in
`build-binary.yml`. Rename nothing without changing all three.

**Never set `INSPECTAH_SKIP_UI=1` for these builds.** A release binary
must embed the built frontend, so `crates/web/build.rs` has to run
`npm ci` and `npm run build` for real. A binary built with the variable
set ships without a UI, and the failure is invisible until someone runs
`refine`. The workflow does not set it, and the verify step below would
fail if the UI were missing.

**Rehearse before tagging.** `gh workflow run release.yml --ref main`
builds and verifies all three binaries from `main` and stops there: the
publish job is guarded off on a manual run (see "The release workflows
can be run without shipping" under Gotchas). To see the release build go
green before shipping, push the release commit first, run the rehearsal,
then push the tag. It is optional: the push-to-main smoke build already
runs the same recipe on every code change.

## Binary verification

The workflow checks what shipped, not what was intended:

- **Preflight**, before anything is built: the tag, the `Cargo.toml`
  version and the RPM spec version (tilde form) agree. A tag on the
  wrong commit fails in seconds.
- **Each build leg** runs its own binary's `inspectah version` and
  requires `inspectah <Cargo.toml version> (commit <git rev-parse --short
  HEAD>, built ...)`. On Linux it requires `file` to report a static
  binary (`statically linked` or `static-pie linked`; both are static).
  It requires the binary to contain `index.html` and every content-hashed
  `index-*` name in the freshly built `crates/web/ui/dist/assets`, which
  confirms the frontend is inside the artifact.
- **The publish job** reads the commit stamp from the bytes of all three
  binaries (the Linux ones cannot run on its runner), requires exactly
  one stamp per binary, requires it to be the tag commit with the tag's
  version, and writes the three SHA-256 digests to the run summary.

Every check fails the run with an `::error::` line naming the binary and
what was wrong. Nothing is staged on a workstation any more, so the old
trap of a skipped copy leaving the previous release's binary under the
right name cannot happen.

To spot-check a draft by hand, download its assets and read the stamps
without running them. Expect one distinct line per binary (its count
may be above 1), identical across all three:

```bash
gh release download v<version> -D /tmp/inspectah-v<version>
cd /tmp/inspectah-v<version>
for f in inspectah-darwin-arm64 inspectah-linux-amd64 inspectah-linux-arm64-bin; do
  echo "== $f"
  LC_ALL=C grep -aoE '[0-9][^ ]* \(commit [0-9a-f]+, built [0-9-]+\)' "$f" | sort | uniq -c
done
```

Always use `grep -o` or `grep -c` on these binaries: they embed long
unbroken runs of text, and an unanchored `grep` dumps kilobytes per match.

### The embedded commit hash is the tag commit

`crates/cli/build.rs` stamps release builds with `git rev-parse --short
HEAD`, and `inspectah version` prints it:

```
inspectah 0.9.0-beta.2 (commit c52b02bd, built 2026-08-16)
```

CI checks out the tag, so HEAD is the tag commit and all three binaries
carry the same stamp. The publish job refuses a binary stamped with
anything else. The build date is not compared, since legs can straddle
midnight UTC.

Releases up to v0.9.0-beta.6 were built by hand from a HEAD above the
tag, after the release-notes commit, so their stamps name a later commit
than the tag (v0.9.0-beta.4: tag `802fdd11`, stamp `9f918710`). That is
expected for those binaries, not a defect to chase.

## Pre-commit checks

Run these yourself and read the output before committing the release.

```bash
cargo build --workspace
cargo fmt --check
cargo clippy --workspace --all-targets -- -D clippy::all
cargo test --workspace
```

**The expected clippy result is zero warnings, and the command now fails
if there are any.** This line matches `CLAUDE.md` and the pre-commit
hook exactly; if you change one, change all three.

Two flags carry that guarantee. `-D` makes warnings errors: the earlier
`-W` form exited 0 however many it printed, which is why the hook's
clippy gate could never block and why the baseline below was allowed to
stand. `--all-targets` checks test targets too; without it clippy sees
lib and bin only, and every warning this tree has ever carried lived in
a test target.

Until v0.9.0-beta.4 this section documented a standing baseline of nine
pre-existing warnings to compare against. **That baseline is retired --
the nine were fixed and the gate now enforces zero.** If you are reading
an older release's notes and expecting nine, that is why.

Build and test are part of the gate even though a release commit changes
no source: the version bump rewrites `Cargo.lock` and every crate
version, so confirm the workspace still builds and the suite still
passes at the bumped version. v0.9.0-beta.2 was 2784 passed, 0 failed,
6 ignored; v0.9.0-beta.3 was 2785 passed, 0 failed, 6 ignored; v0.9.0-beta.4
was 2969 passed, 0 failed, 6 ignored, across 83 suites; v0.9.0-beta.5 was
2990 passed, 0 failed, **0 ignored**, across 84 binaries. The jump at
beta.4 is expected (it carries the whole accumulated `[Unreleased]`
body plus the TUI removal) -- the pass count legitimately grows release to
release, so don't treat a gap against this baseline as a problem on its
own. **0 failed and 0 ignored are the numbers that signal trouble**, not
the pass count.

**The ignored count flipped at beta.5 and the older figures above are
not the target.** The quarantine was emptied on 2026-11-04: the one
quarantined test that passed was un-ignored and the parked one deleted
(`763d0d58`), and four obsolete HTML report tests went with them
(`93549d39`). Six ignored was the healthy number up to beta.4; zero is
the healthy number now, and a nonzero ignored count means something was
quarantined since.

Two sandbox traps make a clean tree look broken. Neither is a real
failure, and both cost time on more than one release:

- **`npm ci` hangs.** The `inspectah-web` `build.rs` runs it and it needs
  network. Set `INSPECTAH_SKIP_UI=1` on the gate commands above when the
  release commit touches no frontend source. It has no effect on
  `git commit`, because the pre-commit hook unsets it: run the commit
  with the sandbox disabled, as the next item says. Do not reach for
  `--no-verify`. (Never set it for release *binaries*
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
| `docs(release): write v<version> release notes` | `process-docs/release-notes/release-notes-<version>.md` |
| `docs(roadmap): bump current version to v<version>` | `ROADMAP.md` |

**Stage the release commit by explicit path.** Other agents commit to
this repo concurrently, and a plans or skills file being edited in
another session will be sitting modified in the working tree while you
cut the release. `git commit -a` or `git add -A` would sweep it into
the tagged commit. This is not hypothetical: it happened during the
beta.3 cut, where a `docs(plans):` commit from another session landed
*between* the release commit and the release-notes commit.

Tag format is v-prefixed: `git tag v0.8.6-beta.5`. The tag goes on the
release commit; the binaries are built from it, so commits above it do
not ship.

**Do not push.** Mark reviews and pushes the commits and the tag.
Pushing the tag is what starts the release: `release.yml` builds from
it at once. A pushed tag that has to move means a new build and a
deleted draft.

## GitHub release

`release.yml` creates the release as a **draft** named `v<version>` with
the three binaries attached, and never publishes it. The run summary
shows the SHA-256 of each asset and the command to publish. Mark
publishes from a checkout that has the release-notes commit:

```bash
gh release edit v<version> --draft=false \
  --notes-file process-docs/release-notes/release-notes-<version>.md
```

Publishing fires `package-release.yml`, which attaches the SRPM and RPMs
and updates the Homebrew tap (see "Homebrew formula"). Mark's whole part
of a release is: tag, push, check the `Release` run went green, publish
the draft, check the `Package release` run went green.

Why a draft rather than a release the workflow publishes itself:

- **A release the workflow creates cannot trigger other workflows.**
  GitHub raises no workflow events for actions taken with
  `GITHUB_TOKEN`, so a release published by `release.yml` would never fire
  `release: published`. The RPMs and the tap update would silently not
  happen. Mark publishing with his own credentials is what fires them.
- **The release notes are written after the tag**, so they are not in
  the tree the workflow builds.
- **Mark sees the draft before users can download it.**

What the publish job does when the release already exists:

| Release state | Result |
|---|---|
| Does not exist | Created as a draft with the three assets |
| Draft (an earlier attempt) | The three assets are replaced; safe to re-run |
| Published | The job fails and touches nothing; replacing assets users may already have downloaded is a decision for a person |

The tag must be on the remote: the job runs `gh release create
--verify-tag`.

**If the publish job fails after the builds passed**, the binaries are
still in the run as artifacts for 30 days, under their release names.
Create the draft by hand from them:

```bash
gh run download <run-id> -p 'inspectah-*' -D dist
gh release create v<version> dist/inspectah-*/inspectah-* \
  --draft --verify-tag --title v<version> --notes "Release notes pending."
```

Re-running a failed tag run replays the workflow as it was at the tag,
so a fix to `release.yml` on `main` does not reach that run.

**Do not mark the release pre-release, even on a beta/alpha/rc tag,
until 1.0 ships.** A pre-release does not show up on the repo home page,
and there is no stable release yet to hold that spot, so every beta is
currently marked as a full release on purpose (Mark, 2026-11-04). The
workflow creates the draft without the pre-release flag and `gh release
edit` above leaves it off. Confirmed against `gh release list`:
`v0.9.0-beta.4`, `beta.3`, `beta.2`, `beta.1`, and `v0.8.7-beta.1` are
all `isPrerelease: false`, five releases of consistent practice.
**Expiry condition: once 1.0 ships, betas go back to pre-release**
(`--prerelease` on the `gh release edit`), because a stable release then
exists to hold the home page and a beta no longer needs to.

## Homebrew formula

The tap (`marrusl/homebrew-inspectah`, `Formula/inspectah.rb`) is updated
by the `update-homebrew` job in `package-release.yml` when the release
is published. Nothing about it is done by hand. The job runs on
`macos-latest` and, in order:

1. Downloads `inspectah-darwin-arm64` from the published release
   (waiting up to 300 s), hashes it, and rejects a digest that is not 64
   characters or is the hash of an empty file.
2. Checks out the tap's `main` without credentials and rewrites
   `version`, `url` and `sha256`, then confirms each substitution landed.
   `install`, `test` and everything else are untouched.
3. Commits locally. If the formula already matched the release (a
   re-run, or a tap someone bumped by hand), it logs a notice and stops
   green without testing or pushing anything.
4. Taps the local checkout and runs `brew install` and `brew test` on
   exactly that commit. The formula's `test` block asserts `version.to_s`
   appears in `inspectah version` output, so the formula's `version` must
   be the hyphenated `CARGO_PKG_VERSION` form (`0.9.0-beta.7`), not the RPM
   tilde form.
5. Pushes that commit directly to the tap's `main`. There is no branch
   and no pull request: the tap update needs no review (Mark,
   2026-08-16), and step 4 is the gate instead.

The tap has no rulesets and no branch protection, and auto-merge is off
(checked 2026-11-21), so nothing on the tap side stands between the
token and `main`.

**`HOMEBREW_TAP_TOKEN` is visible to the push step only.** The checkout
and the brew steps, which download and run the release binary, never
see it, and the checkout leaves no credential in `.git/config`. The
token needs Contents write on `marrusl/homebrew-inspectah`; it no longer
needs pull-request access.

**If the push is refused**, the step fails with git's error in the log
and nothing lands on the tap: the push is a single update to `main`.
The two likely causes are a token without write access to `main`, and
`main` having moved since the checkout (a hand bump). Fix the cause and
re-run the job; if the tap is already at the release, the re-run stops
at step 3.

## Gotchas

- **RPM spec uses tilde, not hyphen.** `0.8.6~beta.5` sorts correctly
  in RPM; `0.8.6-beta.5` would sort *after* `0.8.6` (wrong).

- **Skipped releases happen.** `v0.8.6-beta.4` was tagged but never
  released on GitHub. (Note this is specifically `v0.8.6-beta.4` -- a later,
  unrelated `v0.9.0-beta.4` exists and was released normally; don't read
  "beta.4" here as a pattern that recurs at every `.4`.) When a version is
  skipped like this, roll the unreleased changes into the next version.
  The CHANGELOG keeps the skipped version's section as-is; the release
  notes for the new version mention "also included from beta.N" for
  visibility. A pushed tag that is skipped leaves a draft release behind;
  delete it (`gh release delete v<version>`, which keeps the tag) so it
  cannot be published by mistake.

- **The tap follows publishing, not tagging.** `update-homebrew` runs
  only when a release is published, so a tag that is never published
  leaves the tap on the previous release, which is correct. A tap bumped
  by hand ahead of the job turns the job into a green no-op notice. A
  release published while the job is broken leaves the tap behind
  without anyone noticing unless the `Package release` run is checked:
  that happened at beta.4, when the tap was still bumped by hand and the
  bump was skipped.

- **Cargo.lock regeneration.** After bumping `Cargo.toml`, run
  `cargo check` to update `Cargo.lock`. Don't forget to stage it.

- **Release-named binaries in a clone's root are leftovers.**
  `.gitignore` still covers `/inspectah-*`, and clones used for hand-cut
  releases up to v0.9.0-beta.6 have old binaries sitting there under the
  release names. Nothing reads or uploads them; delete them if they
  confuse.

- **The release workflows can be run without shipping.**
  `release.yml` takes a `workflow_dispatch` with no inputs and builds and
  verifies from whichever ref it is started on. Its `publish` job is
  guarded by a job-level
  `if: github.event_name == 'push' && startsWith(github.ref, 'refs/tags/v')`,
  so a manual run, even one started on a tag ref, never creates or
  changes a release.

  `package-release.yml` takes a `workflow_dispatch` with a required `tag`
  input, so an existing tag can be rebuilt on demand:
  `gh workflow run package-release.yml --ref main -f tag=v<version>`. The
  workflow definition comes from `--ref`; the source tree comes from the
  tag. A manual run builds and verifies but touches nothing outside the
  runner: both release-asset upload steps and the whole `update-homebrew`
  job are guarded to `github.event_name == 'release'`, so a test run
  cannot overwrite a published release's assets or push to the tap. The built packages come back as workflow artifacts (`srpm`,
  `rpm-fedora`, `rpm-el9`, `rpm-el10`), so a green run leaves something to
  inspect.

  **A green manual run is not a green release run.** The release-upload
  paths and the tap job are guarded off on a manual run, so a manual
  green proves the build and packaging steps, not the release path. The
  RPM upload-to-release step first succeeded on a real release with
  v0.9.0-beta.6 (run `44862766376`); the direct-push tap job and the
  draft-creating `release.yml` publish job first run for real at
  v0.9.0-beta.7.

  **Use it.** Before this trigger existed, the only way to fire the
  pipeline was to publish a release, and a failed release run's re-run
  replays that same run's workflow definition, so re-running a broken
  release replays the break. That is the structural reason the RPM legs
  stayed red across five releases: nobody could test the release pipeline
  without shipping a release.

  Run `41922788939` (2026-11-04) is the first CI-built RPM in the
  project's current history, across all three targets (Fedora, EL9,
  EL10). Two earlier successful runs exist in the workflow's history,
  from 2026-04-28, but they predate the web UI: don't read those as "it
  used to work" without that qualifier, since the pipeline now has to
  build something considerably heavier than it did then.

- **A CI failure in `Build SRPM` that reads as a gzip error is an
  ownership problem.** `actions/checkout` writes the workspace as a
  different UID than the build step runs as, so git refuses the
  repository as dubiously owned. `git archive HEAD` then emits an empty
  stream, and the first thing that complains is `gzip: stdin: unexpected
  end of file`, well downstream of the actual cause. The real line is
  `fatal: detected dubious ownership`, further up in the log. The
  workflow runs `git config --global --add safe.directory
  "$GITHUB_WORKSPACE"` before `make` to prevent it. Keep that fix in the
  workflow, not in `.copr/Makefile`: COPR owns the tree it builds and has
  no such UID mismatch.
