# Release Build Configuration

## Current State

The workspace `Cargo.toml` has **no `[profile.release]` section**. Release
builds use Rust defaults: no LTO, no stripping, default codegen-units.

## Release Builds

`.github/workflows/build-binary.yml` builds each target natively on a runner
of its own architecture. It does not cross-compile and does not use
`cargo-zigbuild` or Zig. Each leg runs `rustup target add <triple>`, installs
`musl-tools` on the Linux legs, then runs:

```
cargo build -p inspectah-cli --release --target <triple>
```

Linux targets are `x86_64-unknown-linux-musl` (`ubuntu-24.04`) and
`aarch64-unknown-linux-musl` (`ubuntu-24.04-arm`), both static. The Darwin
build is `aarch64-apple-darwin` on `macos-latest`.

## Binary Size

Without release profile tuning, binaries are 15-18 MB. Adding these
settings to the workspace `Cargo.toml` would reduce size 30-50%:

```toml
[profile.release]
lto = "thin"
strip = true
codegen-units = 1
```

This is tracked as a roadmap item (Release Binary Size Optimization).

## Version Metadata

Version is set in `[workspace.package]` in the root `Cargo.toml`:

```toml
[workspace.package]
version = "0.8.6-beta.3"
```

All crates inherit this via `version.workspace = true`. The `build.rs`
in `crates/cli/` adds `+g<short hash>` to the version line unless HEAD
carries the release tag.
See `process-docs/skills/build-rs-version-metadata.md` for details.

## Workspace Members

```
crates/cli/       — CLI commands (scan, build, refine, aggregate)
crates/collect/   — Host inspection (inspectors)
crates/core/      — Shared types, snapshot schema, aggregate logic
crates/pipeline/  — Rendering (Containerfile, README, audit, report, secrets)
crates/refine/    — Refine editor server
crates/web/       — Web server
```
