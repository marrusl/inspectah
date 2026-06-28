# Codebase Layout

Directory structure and module organization for inspectah. Helps new agents navigate the Rust workspace.

## Workspace structure

```
/
├── crates/           # All Rust crates
├── docs/             # GitHub Pages documentation
├── process-docs/     # Specs, plans, and skill files
├── completions/      # Shell completion scripts
├── packaging/        # RPM packaging (COPR)
├── release-artifacts/ # Tagged release binaries
├── testdata/         # Fixtures for integration tests
└── tests/            # Top-level integration tests
```

## Crates (workspace members)

| Crate | Purpose | Executable? |
|-------|---------|-------------|
| `crates/cli/` | Binary entry point, argument parsing, command dispatch | Yes |
| `crates/web/` | Embedded web server (axum), React UI host | No |
| `crates/tui/` | Terminal UI (ratatui) for refine | No |
| `crates/pipeline/` | Orchestrates inspectors, rendering, redaction | No |
| `crates/refine/` | Interactive triage engine, session persistence | No |
| `crates/collect/` | System inspectors (RPM, config, services, etc.) | No |
| `crates/core/` | Types, traits, aggregate data structures (no logic) | No |

**Dependency direction:** CLI/web/tui → pipeline → refine → collect → core.
Core has zero dependencies on other crates.

## CLI commands

**Location:** `crates/cli/src/commands/`

| File | Subcommand |
|------|-----------|
| `scan.rs` | `inspectah scan` — single-host scan |
| `aggregate.rs` | `inspectah aggregate` — cross-host aggregation |
| `refine.rs` | `inspectah refine` — launch web or TUI |
| `build.rs` | `inspectah build` — extract tarball, mount certs, run podman build |
| `version.rs` | `inspectah --version` or `inspectah version` |
| `pull_progress.rs` | Progress reporting during snapshot tarball pulls |
| `pull_failure.rs` | Error handling for snapshot pull failures |

## Core types

**Location:** `crates/core/src/types/`

Each module defines the data model for a snapshot section:
- `rpm.rs` — RPM package types
- `config.rs` — Modified config file types
- `services.rs` — systemd service types
- `containers.rs` — Podman/Docker container types
- `network.rs`, `storage.rs`, `kernelboot.rs`, `selinux.rs`, `scheduled.rs`, `nonrpm.rs`, `users.rs`
- `aggregate.rs` — Aggregate-specific types (consensus items, variants)
- `repo.rs` — Repository definitions
- `subscription.rs` — RHSM entitlement types
- `system.rs`, `os.rs` — System metadata
- `redaction.rs`, `completeness.rs`, `preflight.rs`, `warnings.rs`, `progress.rs`

**Core traits:** `crates/core/src/traits/` — Inspector, Executor, Renderer, Detector, Progress

**Core data containers:** `snapshot.rs`, `pipeline.rs`, `baseline.rs` at `crates/core/src/`

## Inspectors

**Location:** `crates/collect/src/inspectors/`

Each inspector implements the `Inspector` trait (`collect()` → section data):
- `rpm/` — RPM package inventory, modules, repos, classifier
  - `rpm/repoless.rs` — Repo-less RPM detection (empty/disabled repos) and dnf cache scanning
- `config/` — Modified config files via `rpm -Va`
- `services.rs`, `containers.rs`, `network.rs`, `storage.rs`, `users.rs`, `selinux.rs`, `kernelboot.rs`, `nonrpm.rs`, `scheduled.rs`, `subscription.rs`
- `nonrpm.rs` also contains `scan_unmanaged_files()` for Tier 2 unmanaged file cataloging

**Executor abstraction:** `crates/collect/src/executor/` — `real.rs` (live system), `mock.rs` (test doubles)

**Baseline comparison:** `crates/collect/src/baseline.rs`

## Renderers

**Location:** `crates/pipeline/src/render/`

Artifact generators invoked after triage decisions finalized:
- `containerfile.rs` — Containerfile generation
- `kickstart.rs` — Kickstart file for traditional installs
- `report.rs`, `report_data.rs` — HTML migration report
- `audit.rs` — Audit trail of decisions
- `secrets.rs` — Redacted secrets summary
- `tarball.rs` — Snapshot archive packaging
- `users.rs` — User/group materialization (passwd/group entries)
- `service_intent.rs` — Quadlet/systemd service migration
- `language_packages.rs` — Tier 1 pip/npm/gem Containerfile rendering
- `unmanaged.rs` — Tier 2 unmanaged file COPY directives and symlink `ln -sf`
- `repoless.rs` — Tier 3 repo-less RPM `dnf localinstall` directives
- `safety.rs` — Safety checks and destructive-action warnings
- `configtree.rs` — Config file tree visualization
- `baseline_fmt.rs` — Baseline comparison formatting
- `readme.rs` — Auto-generated README for output directory

**Pipeline orchestration:** `crates/pipeline/src/orchestrate.rs` — runs inspectors in order

**Build execution:** `crates/pipeline/src/build/` — tarball extraction, RHEL pass-through detection, podman command construction

**Redaction engine:** `crates/pipeline/src/redaction/` — secret scrubbing patterns

## Refine web UI

**Location:** `crates/web/ui/src/`

React/TypeScript UI embedded into the Rust binary via `rust-embed`:
- `App.tsx` — top-level app shell
- `main.tsx` — entry point
- `components/` — React components
  - `aggregate/` — aggregate-specific views (variants, consensus, diff drawer)
  - Single-host components at top level (DecisionList, PackageList, ContainerfilePanel, etc.)
  - `LanguagePackageList.tsx` — Language Packages decision section (pip/npm/gem environments)
  - `UnmanagedFileList.tsx` — Unmanaged Files decision section (directory grouping, provenance signals)
  - `RpmUploadModal.tsx` — Single-RPM upload modal with NEVRA validation
  - `RpmBatchUploadModal.tsx` — Multi-RPM batch upload with auto-matching and conflicts view
- `hooks/` — React hooks
  - `useRpmUpload.ts` — 5-state RPM upload row machine wired to POST /api/upload-rpm
  - `useKeyboard.ts` — keyboard shortcuts (keys 6-7 = Language Packages, Unmanaged Files)
  - `useView.ts`, `useMutation.ts` — view data and mutation hooks
- `api/types.ts` — TypeScript types (LanguagePackageEnv, UnmanagedFileItem, ProvenanceSignals, RpmUploadRowState)
- `utils/` — utility functions
- `test-utils/` — test fixtures
- `e2e/` — Playwright end-to-end tests

**Backend API:** `crates/web/src/` — axum handlers for REST endpoints
  - `upload.rs` — `POST /api/upload-rpm` for repo-less RPM uploads (500 MiB route-specific limit)

## Refine logic

**Location:** `crates/refine/src/`

- `classify.rs` — Automatic triage classification (include/exclude/review)
- `session.rs` — Session state persistence and loading
- `aggregate/` — Aggregate-specific logic (classify, diff, variant operations)
- `normalize.rs` — Data normalization for comparison
- `autosave.rs` — Periodic auto-save during interactive triage
- `projection/` — Decision projection for renderer consumption
- `tarball.rs`, `repo_index.rs` — Snapshot archive I/O

## Tests

| Location | Type |
|----------|------|
| `crates/*/tests/` | Per-crate integration tests |
| `crates/web/ui/e2e/` | Playwright browser tests (React UI) |
| `crates/web/ui/src/**/__tests__/` | Jest unit tests (React components) |
| `tests/` | Top-level end-to-end tests |
| `testdata/` | Fixtures for integration tests |

**Mock executor:** Test doubles with deterministic output keyed by `cmd + " " + args.join(" ")`.
See `mock-executor-key-format.md`.

## Documentation

**Location:** `docs/`

Diataxis structure for GitHub Pages:
- `docs/explanation/` — Explanatory deep-dives
- `docs/how-to/` — Task-oriented guides
- `docs/tutorials/` — Learning-oriented walkthroughs
- `docs/reference/` — API/command reference
- `docs/diagrams/` — D3 interactive diagrams
- `docs/contributing/` — Contributor guides

**Process docs:** `process-docs/` — specs, plans, skills (internal, not published)
  - `process-docs/skills/` — Non-obvious patterns and gotchas (this file)
  - `process-docs/specs/` — Design specs
  - `process-docs/plans/` — Implementation plans

## Completions

**Location:** `completions/`

Shell completion scripts generated by the CLI build:
- `bash/inspectah.bash`
- `zsh/_inspectah`
- `fish/inspectah.fish`

Bundled into RPM packages.

## Gotchas

- **Attribution style** — LLM-assisted commits include `Assisted-by: <tool> (<model>)`. No other identifiers.
- **Build script version:** `crates/cli/build.rs` stamps version metadata at compile time. Don't manually update version strings in code.
- **Schema versioning:** Snapshot JSON has a `schema_version` field that MUST be bumped when types change. See `snapshot-schema-versioning.md`.
- **Package identity is `name.arch`** — Never use bare package names. See `package-identity-is-name-dot-arch.md`.
