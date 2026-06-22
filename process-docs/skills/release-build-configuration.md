# Release Build Configuration

## Current State

The workspace `Cargo.toml` has **no `[profile.release]` section**. Release
builds use Rust defaults: no LTO, no stripping, default codegen-units.

## Cross-Compilation

Linux static binaries (for ARM64 and x86_64) are built via:

```
cargo zigbuild --target aarch64-unknown-linux-musl --release
cargo zigbuild --target x86_64-unknown-linux-musl --release
```

`cargo zigbuild` uses Zig as a cross-linker. The `musl` target produces
fully static binaries with no glibc dependency.

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
in `crates/cli/` injects git commit hash and build date at compile time.
See `process-docs/skills/build-rs-version-metadata.md` for details.

## Workspace Members

```
crates/cli/       — CLI commands (scan, build, refine, aggregate)
crates/collect/   — Host inspection (inspectors)
crates/core/      — Shared types, snapshot schema, aggregate logic
crates/pipeline/  — Rendering (Containerfile, README, audit, report, secrets)
crates/refine/    — Refine editor server
crates/tui/       — TUI interface
crates/web/       — Web server
```
