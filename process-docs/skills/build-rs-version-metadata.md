# Build-time version metadata via build.rs

## Context
The CLI crate (`crates/cli/`) needs to display the git commit hash and build
date at runtime. Cargo does not expose these natively.

## Mechanism
A `build.rs` in the **binary crate** (`crates/cli/build.rs`) emits
`cargo:rustc-env` directives that set `INSPECTAH_COMMIT` and `INSPECTAH_DATE`
at compile time. Source code consumes these via `env!()` (compile-time
guaranteed) or `option_env!()` (optional fallback).

## Key details

- **build.rs lives in the binary crate**, not the workspace root. Only the
  crate that declares `[[bin]]` runs its build script for the final binary.
- **`env!()` in `concat!()`** works for building `&'static str` constants
  usable in clap's `#[command(version = ...)]` derive attribute. `format!()`
  returns `String`, which clap's derive macro rejects (needs `&'static str`).
- **Release builds only embed real values.** build.rs reads `PROFILE`
  (`release` for any profile that inherits from release). In release builds
  it runs `git` and `date` and emits `cargo:rerun-if-changed` for
  `../../.git/HEAD` and `../../.git/refs/` (relative to the crate's
  build.rs), so commits, checkouts and rebases trigger a rebuild.
- **Debug and test builds carry `dev` for both values** (the `DEV_METADATA`
  constant) and emit only `cargo:rerun-if-changed=build.rs`. Watching `.git`
  in dev builds rebuilt cli far too often: in a linked worktree `.git` is a
  file, so `../../.git/HEAD` never exists and cargo reran the script on
  every invocation; in the main checkout every commit moved `.git/refs` and
  rebuilt cli once per commit (about 17 to 20 s, measured 2026-11-19). So
  `--version` on a debug binary prints `(commit dev, built dev)`.
- **The date is `dev` too in debug builds.** A real date would need the
  script to rerun on every build, or go stale for the life of `target/`;
  either defeats the point.
- The `date` command uses `-u` for UTC output in `+%Y-%m-%d` format. This
  is portable across macOS and Linux.

## Gotcha
If you use `option_env!()` instead of `env!()`, the values default to `None`
when build.rs doesn't run (e.g., if cargo caching skips it). Using `env!()`
makes it a hard compile error if the env var is missing, which is the right
behavior for a build script that always runs.
