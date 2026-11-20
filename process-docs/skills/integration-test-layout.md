# Integration tests: one binary per crate

## Context
Cargo builds every top-level `tests/*.rs` file as its own test binary, and
each binary is linked separately. With about 86 such files across the
workspace, a core edit relinks every binary that depends on core, and
those links are a large part of the commit gate's test step. The crates are moving to one binary
each: refine, pipeline, core and collect so far, the other crates one binary per file until their
turn comes.

## Layout
A converted crate's manifest turns off auto-discovery and names one target:

```toml
[package]
autotests = false

[[test]]
name = "integration"
path = "tests/main.rs"
```

`tests/main.rs` declares the shared helper modules, the guard module, and
one `mod <stem>;` per test file, in alphabetical order. The files keep their
paths, so every `include_str!`/`include_bytes!` path, every `#[path]`
attribute and every link to `crates/*/tests/<file>.rs` stays valid.

Run one file's tests with
`cargo test -p <package> --test integration <file stem>::`. Test names in
`--list` output and in failures gain the file stem as a module prefix.

## Rules

- **A new test file goes in `tests/main.rs`, or it never compiles.** With
  `autotests = false` Cargo ignores any undeclared file in `tests/`. The
  guard, `crates/core/tests/support/test_files_declared.rs`, is included by
  path from each crate's `main.rs`; its `every_test_file_is_compiled` test
  lists the top-level `tests/*.rs` files and fails, naming each one, when a
  file has neither a top-level `mod <stem>;` declaration in `main.rs` nor a
  `path = "tests/<stem>.rs"` entry in a `[[test]]` table of the manifest.
  It reads past comments, string literals and macro input, so a
  commented-out `mod` line does not count. It lives in a subdirectory of core's `tests/`, so
  it is not a test target of its own; core's `main.rs` includes it as
  `#[path = "support/test_files_declared.rs"]`.
- **Helper modules are declared once, in `main.rs`, and imported with
  `use crate::<mod>::...`.** A `mod helpers;` inside a test file no longer
  works: a module declared from a non-root file looks for
  `tests/<stem>/helpers.rs`. The `#[path]` form still compiles, but loading
  one file as a module twice in one crate trips `clippy::duplicate_mod`,
  which the gate's `-D clippy::all` denies.
- **insta file snapshots are named `integration__<stem>__<name>.snap`**,
  because insta prefixes the snapshot name with the module path. Converting
  a crate renames each `.snap` with byte-identical content (git shows
  `R100`). A renamed snapshot's header needs no edit: its `source:` line
  names the test's source file, which does not move.
- **A crate that keeps a second `[[test]]` target** runs it with
  `cargo test -p <package> --test <target>`. That target is its own crate
  root: it declares its own helper modules, does not include the guard, and
  is listed in the manifest with its `path`, which is how the guard counts
  it as declared. Its tests list without a stem prefix.

## Why `autotests = false` and not `tests/<name>/main.rs`
Cargo auto-discovers a directory target with no manifest change, but that
layout needs every test file moved into the directory, every relative
`include_str!`/`include_bytes!` path deepened by one `../`, the snapshot
directories moved, the docs-only allowlist entries and about 30 doc and
skill links rewritten. Its one advantage, that a stray top-level
`tests/foo.rs` still compiles as its own binary, is covered by the guard.
