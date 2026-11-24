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

**Docs schema number.** When `SCHEMA_VERSION` changed since the last
tag, update `docs/reference/snapshot-schema.md`: the `Schema version`
line and the numbers in the "Loading and schema version" paragraph and
its sample error. That page is the only docs page that names the
current number. Then look for stale copies elsewhere and fix every hit
outside it, preferably by removing the number:

```sh
grep -rniE 'schema[ _]?(version)?[":= v]+[0-9]{1,2}\b' docs README.md | grep -v 'docs/reference/snapshot-schema.md'
```

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
push to `main` that changes one of the build's inputs (its `paths`
filter: crate sources and manifests, the `build.rs` files, the UI
sources, `package.json` and `package-lock.json`, the lockfile, the
toolchain file, and the workflow itself), so the release recipe is
exercised long before a tag.
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
  `inspectah --version` through `scripts/check-version-stamp.sh run`.
  A leg of a tag build requires exactly `inspectah <Cargo.toml version>`;
  any other leg (push to `main`, a rehearsal from `main`) requires
  `inspectah <version>+g<git rev-parse --short HEAD>`. On Linux it requires
  `file` to report a static
  binary (`statically linked` or `static-pie linked`; both are static).
  It requires the binary to contain `index.html` and every content-hashed
  `index-*` name in the freshly built `crates/web/ui/dist/assets`, which
  confirms the frontend is inside the artifact.
- **The publish job** checks the bytes of all three binaries (the Linux
  ones cannot run on its runner) with `scripts/check-version-stamp.sh
  bytes`: each must embed the line `inspectah <tag version>` ending in
  its newline, and no `inspectah <tag version>+...` form. A longer
  version, a `+` suffix and the old `(commit ..., built ...)` line all
  fail. It writes the three SHA-256 digests
  to the run summary.

Every check fails the run with an `::error::` line naming the binary and
what was wrong. Nothing is staged on a workstation any more, so the old
trap of a skipped copy leaving the previous release's binary under the
right name cannot happen.

**Test a change to these checks locally.** The workflows run only on a
push, but the script runs anywhere, from the checkout the binary was built
from. `scripts/tests/check-version-stamp-test.sh` covers it with stand-in
binaries, and CI runs that test. Against real binaries:

```bash
cargo build -p inspectah-cli --release
scripts/check-version-stamp.sh run target/release/inspectah untagged
```

A tagged build needs a tag `v<version>` on HEAD, and tags are shared by
every worktree of the clone, so do it on a detached scratch commit that
bumps the version to something no real tag uses (`0.0.0-scratch.1`), and
`git tag -d` the tag afterwards. Never push either.

To spot-check a draft by hand, download its assets and read the stamps
without running them:

```bash
gh release download v<version> -D /tmp/inspectah-v<version>
for f in inspectah-darwin-arm64 inspectah-linux-amd64 inspectah-linux-arm64-bin; do
  scripts/check-version-stamp.sh bytes "/tmp/inspectah-v<version>/$f" <version>
done
```

Always use `grep -o` or
`grep -q` on these binaries if you grep them yourself: they embed long
unbroken runs of text, and an unanchored `grep` dumps kilobytes per match.

### A bare version line means the tag commit

`crates/cli/build.rs` prints the bare version only when HEAD carries the
tag `v<version>` (see `build-rs-version-metadata.md`):

```
inspectah 0.9.0-beta.8
```

CI checks out the tag, so HEAD is the tag commit and all three binaries
print the bare line. A binary built from any other commit carries
`+g<short hash>`, and the publish job refuses it. A binary built from the
tag commit is the only kind that passes, so the check still proves each
binary came from the tagged tree, without comparing hashes.

Up to v0.9.0-beta.7 the line was `inspectah <version> (commit <hash>,
built <date>)`. Releases up to v0.9.0-beta.6 were built by hand from a HEAD
above the tag, after the release-notes commit, so their stamps name a later
commit than the tag (v0.9.0-beta.4: tag `802fdd11`, stamp `9f918710`).
That is expected for those binaries, not a defect to chase.

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

**Whoever can push a `v*` tag chooses what is released.** The workflows
check that the tag, `Cargo.toml` and the spec agree on the version, not
who pushed the tag or whether its commit is on `main`, so a tag on an
off-`main` commit builds and drafts a release from that commit. The
draft still waits for a person to publish it, but what it holds is
whatever the tag built. That puts inspectah's tag permissions inside
the release's trust boundary. Its tag rulesets have not been read
(2026-11-21); whether to restrict `v*` tag creation is Mark's call.

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
Finish by hand what the job would have done, starting from the release's
current state, which the failed job may already have changed: creating
the release and uploading its assets are separate operations, so a
failure can leave a draft behind with some assets or none.

```bash
gh run download <run-id> -p 'inspectah-*' -D dist
gh release view v<version> --json isDraft --jq .isDraft
```

| `gh release view` says | Do |
|---|---|
| `release not found` | `gh release create v<version> dist/inspectah-*/inspectah-* --draft --verify-tag --title v<version> --notes "Release notes pending."` |
| `true` (a draft) | `gh release upload v<version> dist/inspectah-*/inspectah-* --clobber` |
| `false` (published) | Stop. Replacing assets users may already have downloaded is Mark's decision, not a recovery step |

`--clobber` deletes each existing asset before uploading its
replacement, so a replacement upload that fails leaves the draft
*without* that asset. Repeat the upload until it succeeds.

Before publishing, check the draft holds exactly the three assets and
that they are the files from the run:

```bash
gh release view v<version> --json assets --jq '.assets[].name'
gh release download v<version> -D /tmp/check-v<version>
shasum -a 256 /tmp/check-v<version>/inspectah-* dist/*/inspectah-*
```

Each name must appear once, and each digest twice.

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
by two jobs in `package-release.yml` when the release is published.
Nothing about it is done by hand.

```
homebrew-test   macos-latest, no tap token
  check out tap main (the base) -> skip if the tap is newer
  -> resolve the asset -> rewrite -> commit locally
  -> upload the formula file -> brew install, brew test, inspectah version
        |
        |  data only: the formula file, the base commit, the asset sha256
        v
homebrew-push   fresh ubuntu-latest runner, tap token in one step
  check out tap main -> verify base, version, digest, and that the
  received file is its own rewrite of the base -> commit -> push to main
```

`homebrew-test`, in order:

1. Checks out the tap's `main` without credentials and records that
   commit as the base.
2. Reads the formula's current `version` and compares it with the
   release's using Ruby's `Gem::Version`, which orders a prerelease
   below its release and compares numeric parts as numbers
   (`0.9.0-beta.9 < 0.9.0-beta.10 < 0.9.0`); `sort -V` gets the first of
   those wrong. If the tap is already newer, the release is superseded:
   the job logs a notice and stops green with nothing tested or pushed.
   This is what stops a re-run of an older release's `Package release`
   run from moving the tap backward. An equal version carries on to
   step 5.
3. Downloads `inspectah-darwin-arm64` from the published release
   (waiting up to 300 s), hashes it, and rejects a digest that is not 64
   characters or is the hash of an empty file.
4. Rewrites `version`, `url` and `sha256`, then confirms each
   substitution landed. `install`, `test` and everything else are
   untouched.
5. Commits locally. If the formula already matched the release (a
   re-run, or a tap someone bumped by hand), it logs a notice and stops
   green without testing or pushing anything.
6. Uploads the formula file as the `homebrew-formula` artifact, before
   anything from the release has run.
7. Taps the local checkout and runs `brew install` and `brew test` on
   exactly that commit, then `inspectah version`. The formula's `test`
   block asserts `version.to_s` appears in `inspectah version` output, so
   the formula's `version` must be the hyphenated `CARGO_PKG_VERSION`
   form (`0.9.0-beta.7`), not the RPM tilde form.

`homebrew-push` runs only when `homebrew-test` committed a change:

1. Checks out the tap's `main`, again without credentials, and
   downloads the formula artifact.
2. Requires `main` to still be at the base. If it moved, the job fails
   rather than re-evaluating: the tested file was written against the
   old base, and what moved `main` may be a hand bump or a newer
   release.
3. Requires the digest to be 64 lowercase hex characters, repeats the
   older-release check, and applies the same rewrite to its own
   checkout, taking `version` and `url` from the release payload rather
   than from `homebrew-test`. The result must be byte-identical to the
   received file.
4. Commits and pushes directly to the tap's `main`. There is no branch
   and no pull request: the tap update needs no review (Mark,
   2026-08-16), and `homebrew-test` is the gate instead.

The tap has no rulesets and no branch protection, and auto-merge is off
(checked 2026-11-21), so nothing on the tap side stands between the
token and `main`.

**`HOMEBREW_TAP_TOKEN` exists only in the push step of
`homebrew-push`**, on a runner where nothing from the release has run.
Keeping the token out of the other steps' environment would not be
enough on a shared runner: the brew steps run code from the release
(the binary, and the formula, which is Ruby), and that code could leave
something behind, a git `pre-push` hook for one, that a later step
holding the token would run. So the push job receives three values and
one file, never `.git`, hooks or runner state, and executes nothing it
receives. Every value it takes from `homebrew-test` is set before
anything from the release runs, and the only one it cannot recompute is
the digest. A wrong digest makes `brew install` fail for users rather
than install something else, because the `url` comes from the release
payload. This protects the token, not the asset: a compromised binary
still reaches users through a correct formula. The token needs Contents
write on `marrusl/homebrew-inspectah`; it no longer needs pull-request
access.

**If `homebrew-push` fails**, nothing landed on the tap: every check
runs before the token step, and the push is a single non-force update
to `main`. The likely causes:

- `main` moved after `homebrew-test` read it (a hand bump, or another
  release's run). Re-run `homebrew-test`, which re-runs `homebrew-push`
  after it. Re-running `homebrew-push` alone ("Re-run failed jobs"
  does that) reuses the old base and fails the same way.
- The token cannot write to `main`. Fix the secret and re-run
  `homebrew-push`.

If the tap is already at the release by then, the re-run stops at
`homebrew-test` step 5; if it is newer, at step 2.

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

- **The tap follows publishing, not tagging.** The tap jobs run only
  when a release is published, so a tag that is never published leaves
  the tap on the previous release, which is correct. A tap bumped by
  hand ahead of the jobs turns them into a green no-op notice, and so
  does a re-run of an older release's run once a newer release has
  reached the tap. A release published while the jobs are broken leaves
  the tap behind without anyone noticing unless the `Package release`
  run is checked:
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
  runner: the `attach-packages` job and both tap jobs are guarded to
  `github.event_name == 'release'`, so a test run cannot overwrite a
  published release's assets or push to the tap. The built packages come
  back as workflow artifacts (`srpm`, `rpm-fedora`, `rpm-el9`,
  `rpm-el10`), so a green run leaves something to inspect.

  The guards are not the only protection. The workflow grants no
  permissions by default, the build jobs get read access only, and the
  SRPM checkout keeps no credential, so code that runs during a build
  (`npm ci`, the UI build, `cargo`, `rpmbuild`) cannot write to the
  repository or a release on either trigger. Write access lives only in
  `attach-packages`, which checks nothing out and only reads the
  packages it receives as artifacts. It runs after a failed RPM leg, so
  the SRPM and the legs that built still reach the release.

  **A green manual run is not a green release run.** The release-upload
  paths and the tap job are guarded off on a manual run, so a manual
  green proves the build and packaging steps, not the release path. The
  RPM upload-to-release step first succeeded on a real release with
  v0.9.0-beta.6 (run `44862766376`), as a step in the RPM build job.
  `attach-packages`, which replaced it, the two tap jobs and the
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

- **Third-party actions are pinned to commit SHAs** in `release.yml`,
  `build-binary.yml` and `package-release.yml`, with the release the
  SHA came from in a trailing comment (`# v4.4.0`). A major tag such as
  `v4` can be moved to new code by the action's owner, or by whoever
  compromises it, without any change here, and these workflows hold the
  release's write access and the tap token. To move one, resolve the
  new tag read-only with `gh api repos/<owner>/<repo>/git/ref/tags/<tag>`;
  if the object it returns is a `tag`, not a `commit`, follow it with
  `gh api repos/<owner>/<repo>/git/tags/<sha>` to the commit. Update the
  SHA and the comment together. `rust-ci.yml` still uses tags.

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
