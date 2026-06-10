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
- **`cargo:rerun-if-changed`** points to `../../.git/HEAD` and
  `../../.git/refs/` (relative to the crate's build.rs) to trigger rebuilds
  on commits, checkouts, and rebases.
- The `date` command uses `-u` for UTC output in `+%Y-%m-%d` format. This
  is portable across macOS and Linux.

## Gotcha
If you use `option_env!()` instead of `env!()`, the values default to `None`
when build.rs doesn't run (e.g., if cargo caching skips it). Using `env!()`
makes it a hard compile error if the env var is missing, which is the right
behavior for a build script that always runs.
