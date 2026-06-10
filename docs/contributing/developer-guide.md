---
title: Developer Guide
parent: Contributing
nav_order: 1
---

# Developer Guide

How to set up a development environment, build from source, run the test
suite, and submit changes.

## Prerequisites

- **Rust toolchain** (stable, 2024 edition) -- install via [rustup](https://rustup.rs/)
- **Git**
- A Fedora, CentOS Stream, or RHEL system (or VM/container) for integration testing

## Clone and build

```bash
git clone https://github.com/marrusl/inspectah.git
cd inspectah
cargo build
```

The workspace compiles seven crates in dependency order. A clean build
typically takes 1-2 minutes.

For a release build with optimizations:

```bash
cargo build --release
```

The resulting binary is at `target/release/inspectah`.

## Workspace structure

inspectah is a Cargo workspace with seven member crates, all located under
`crates/` with the `inspectah-` prefix dropped from directory names (e.g.,
`crates/core/`, `crates/cli/`). The Cargo package names remain unchanged.

| Crate | Directory | Purpose |
|---|---|---|
| `inspectah-core` | `crates/core/` | Shared traits (`Inspector`, `Executor`, `Renderer`), type definitions, snapshot structures, and fleet logic |
| `inspectah-collect` | `crates/collect/` | Inspector implementations -- each inspector gathers data from one system domain |
| `inspectah-pipeline` | `crates/pipeline/` | Orchestrates collection, rendering, output generation, and build planning |
| `inspectah-refine` | `crates/refine/` | Refinement engine -- triage classification, decision projection, fleet consensus |
| `inspectah-web` | `crates/web/` | Web UI (refine interface) for interactive triage |
| `inspectah-tui` | `crates/tui/` | Terminal UI for interactive triage (ratatui-based alternative to the web UI) |
| `inspectah-cli` | `crates/cli/` | Command-line interface and progress display |

Dependencies flow downward: `inspectah-cli` depends on all other crates.
`inspectah-web` depends on `inspectah-refine`, `inspectah-pipeline`, and
`inspectah-core`. `inspectah-tui` depends on `inspectah-refine` and
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

## Running tests

Run the full test suite across all workspace crates:

```bash
cargo test --workspace
```

Run tests for a specific crate:

```bash
cargo test -p inspectah-collect
cargo test -p inspectah-pipeline
cargo test -p inspectah-core
```

Run a single test by name:

```bash
cargo test -p inspectah-collect test_selinux_mode_enforcing
```

### Snapshot tests

Several crates use [insta](https://insta.rs/) for snapshot testing. When a
snapshot changes, review it with:

```bash
cargo insta review
```

Accept snapshots only after verifying the output change is intentional.

### Clippy

All code must pass clippy with denied warnings:

```bash
cargo clippy --workspace -- -D warnings
```

Fix any warnings before submitting a PR.

## Code style

- Follow standard Rust formatting (`cargo fmt`)
- Use the 2024 edition idioms
- Keep inspector implementations self-contained -- each inspector handles
  one system domain
- Write doc comments on public types and functions
- Match existing patterns in the codebase rather than introducing new ones

## PR process

1. Create a feature branch from `main` (or from an existing feature branch
   for large efforts)
2. Make focused commits -- one logical change per commit when practical
3. Run `cargo test --workspace` and `cargo clippy --workspace -- -D warnings`
   before pushing
4. Open a pull request against `main`
5. Address review feedback in follow-up commits (don't force-push during review)

### Commit messages

Use conventional commit format:

```
type(scope): description in imperative mood

Optional body explaining *why*, not *what*.
```

Common types: `feat`, `fix`, `refactor`, `test`, `docs`, `chore`.

## Cross-compilation

For Linux ARM64 static binaries, use `cargo-zigbuild` with musl:

```bash
cargo zigbuild --target aarch64-unknown-linux-musl --release
```

For Linux x86_64:

```bash
cargo zigbuild --target x86_64-unknown-linux-musl --release
```
