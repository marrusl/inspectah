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
- Binaries section listing all 3 platforms
- Full changelog comparison link at the bottom

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

Requires `cargo-zigbuild` (`cargo install cargo-zigbuild` if missing).
Builds take 30-60 seconds each.

## Binary naming and staging

Copy binaries from build output to release names in the repo root:

| Build output path | Release name |
|---|---|
| `target/release/inspectah` | `inspectah-darwin-arm64` |
| `target/x86_64-unknown-linux-musl/release/inspectah` | `inspectah-linux-amd64` |
| `target/aarch64-unknown-linux-musl/release/inspectah` | `inspectah-linux-arm64-bin` |

The `-bin` suffix on ARM64 Linux distinguishes it from the macOS ARM64
binary (both are `aarch64` but different platforms).

## Pre-commit checks

Run before committing the release:

```bash
cargo clippy -- -W clippy::all   # zero warnings
cargo fmt --check                # formatting clean
```

## Commit and tag

Single commit with all release files:

```
chore(release): v0.8.6-beta.5
```

Tag format is v-prefixed: `git tag v0.8.6-beta.5`.

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

After the GitHub release exists, update
`homebrew-inspectah/Formula/inspectah.rb`:

- Version string
- Download URL (points to the new GH release asset)
- SHA256: `shasum -a 256 inspectah-darwin-arm64`

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
