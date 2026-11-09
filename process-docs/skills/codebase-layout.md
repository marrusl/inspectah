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
├── release-artifacts/ # Local release binaries (gitignored, not in a clone)
├── testdata/         # Fixtures for integration tests
└── tests/            # Fixture tarballs only (tests/e2e/fixtures/), no test code
```

## Crates (workspace members)

| Crate | Purpose | Executable? |
|-------|---------|-------------|
| `crates/cli/` | Binary entry point, argument parsing, command dispatch | Yes |
| `crates/web/` | Embedded web server (axum), React UI host | No |
| `crates/pipeline/` | Orchestrates inspectors, rendering, redaction | No |
| `crates/refine/` | Interactive triage engine, session persistence | No |
| `crates/collect/` | System inspectors (RPM, config, services, etc.) | No |
| `crates/core/` | Types, traits, snapshot container, aggregate merge/validate logic | No |

**Dependency direction**, as the manifests declare it: `core` depends on
no other workspace crate; `collect` on `core`; `pipeline` on `core` and
`collect`; `refine` on `core` and `pipeline`; `web` on `core`,
`pipeline` and `refine`; `cli` on all five.

## CLI commands

**Location:** `crates/cli/src/commands/`

| File | Subcommand |
|------|-----------|
| `scan.rs` | `inspectah scan` — single-host scan |
| `aggregate.rs` | `inspectah aggregate` — cross-host aggregation |
| `refine.rs` | `inspectah refine` — launch the web refine UI |
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
- `finding.rs` — `FindingKind` enum (Actionable/Advisory/Inventory), `ShadowType`, `AdvisoryType`
- `group_render.rs`, `verified_upload.rs`: group rendering and verified-upload types
- `symlink_allowlist.rs` — Cross-tree symlink allowlist for /usr → /etc paths
- `redaction.rs`, `completeness.rs`, `preflight.rs`, `warnings.rs`, `progress.rs`

**Core traits:** `crates/core/src/traits/` — Inspector, Executor, Renderer, Detector, Progress

**Core data containers:** `snapshot.rs`, `pipeline.rs`, `baseline.rs` at `crates/core/src/`

**Core helper modules:** `crates/core/src/`
- `rpm_filename.rs` — RPM filename parsing (`RpmFilename::parse_untrusted`) and the staged-upload filename allowlist (`validate_staged_name`)

## Inspectors

**Location:** `crates/collect/src/inspectors/`

Each inspector implements the `Inspector` trait (`inspect()` → section data,
`crates/core/src/traits/inspector.rs:15`). The production inspector list is
built in `crates/cli/src/commands/scan.rs` (`SubscriptionInspector` is added
only when the subscription is preserved), and the pipeline runs it through
`collect()` in `crates/pipeline/src/collect.rs`:
- `rpm/` — RPM package inventory, modules, repos, classifier
  - `rpm/repoless.rs` — Repo-less RPM detection (empty/disabled repos) and dnf cache scanning
- `config/` — Modified config files via `rpm -Va`
- `services.rs`, `containers.rs`, `network.rs`, `storage.rs`, `users.rs`, `selinux.rs`, `kernelboot.rs`, `nonrpm.rs`, `scheduled.rs`, `subscription.rs`
- `modernization.rs`: not an `Inspector`, detection rules for modernization advisories (SysVinit, xinetd, anacrontab). Note: ifcfg is treated as network inventory, not a modernization advisory.
- `nonrpm.rs` also contains `scan_unmanaged_files()` for Tier 2 unmanaged file cataloging

**Helper modules:** `crates/collect/src/`
- `rpm_ownership.rs` — RPM file ownership queries for /var directory backing classification

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
- `not_carried.rs`: the not-carried class (`classify_auto_carriage`, `apply_scan_package_defaults`, `not_carried_packages`); the one decider of which auto packages the image leaves out, read by the Containerfile, the audit report and `service_intent.rs`; `var_dirs.rs` asks `service_intent::pulled_in_by_included` directly
- `service_intent.rs` — Quadlet/systemd service migration
- `language_packages.rs` — Tier 1 pip/npm/gem Containerfile rendering
- `unmanaged.rs` — Tier 2 unmanaged file COPY directives and symlink `ln -sf`
- `repoless.rs`: package routing (`route_for`), repo-less install planning for the combined local and disabled-repo install, and the scan tarball's cached RPM writer
- `safety.rs` — Safety checks and destructive-action warnings
- `configtree.rs` — Config file tree visualization
- `baseline_fmt.rs` — Baseline comparison formatting
- `readme.rs` — Auto-generated README for output directory
- `advisory.rs`, `repo_enabling.rs`: advisory rendering and repository enablement
- `selinux.rs` — SELinux file context rule rendering and the build-time equivalence table (`selinux_equivalences`)
- `var_dirs.rs` — Effective `/var` directory backing at export time (`VarDirOutcome`, `EffectiveBacking`)

**Section grouping:** `crates/pipeline/src/section_group.rs` — groups findings into report sections

**Pipeline orchestration:** `crates/pipeline/src/orchestrate.rs`: `run_pipeline` chains collect, validate, redact, render and tarball. Inspector execution itself is `crates/pipeline/src/collect.rs`.

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
  - `useKeyboard.ts`: keyboard shortcuts (number keys 1-8 jump to section groups from `/api/groups`; without groups, 1-9 jump to flat sections)
  - `useView.ts`, `useMutation.ts` — view data and mutation hooks
- `api/types.ts` — TypeScript types (LanguagePackageEnv, UnmanagedFileItem, ProvenanceSignals, RpmUploadRowState)
- `utils/` — utility functions
- `test-utils/` — test fixtures

Playwright end-to-end tests live beside `src/`, in `crates/web/ui/e2e/`.

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
- `baseline_summary.rs`, `group_state.rs`, `types.rs`: baseline summary, section-group state, shared refine types

## Tests

| Location | Type |
|----------|------|
| `crates/*/tests/` | Per-crate integration tests |
| `crates/web/ui/e2e/` | Playwright browser tests (React UI). Fixtures under `e2e/fixtures/` are hand-maintained, not generated — see `e2e-fixtures-are-hand-maintained.md`. |
| `crates/web/ui/src/**/__tests__/` | Vitest unit tests (React components and hooks) |
| `tests/e2e/fixtures/` | Fixture tarballs (`factor-topology/`); no test code at the top level |
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
  - `process-docs/release-notes/` — Per-release notes, `release-notes-<version>.md`
  - `process-docs/specs/` — Design specs
  - `process-docs/plans/` — Implementation plans

## Completions

**Location:** `completions/`

Checked-in shell completion scripts:
- `completions/inspectah.bash`
- `completions/inspectah.zsh`
- `completions/inspectah.fish`

The RPM does not install these files. `packaging/inspectah.spec` generates its own from the built binary with the hidden `inspectah completions <shell>` subcommand.

## Gotchas

- **Attribution style** — LLM-assisted commits include `Assisted-by: <tool> (<model>)`. No other identifiers.
- **Build script version:** `crates/cli/build.rs` stamps version metadata at compile time. Don't manually update version strings in code.
- **Schema versioning:** Snapshot JSON has a `schema_version` field that MUST be bumped when a change alters what the snapshot serializes, though a new `Option` field with `#[serde(default)]` or a new `SectionData` variant does not. See `snapshot-schema-versioning.md`.
- **Package identity is `name.arch`** — Never use bare package names. See `package-identity-is-name-dot-arch.md`.
- **Clippy clean is one command:** `cargo clippy --workspace --all-targets -- -D clippy::all`, run identically by the pre-commit hook and CI. See `full-workspace-verification-gates.md`.
- **Every gate is fast** — measured on an M3 Pro: a from-scratch `cargo clean` plus build plus the full 3010-test suite is two to two and a half minutes, and the pre-commit hook on a realistic change is about a minute to a minute and a half (measured 2026-11-08). There is no per-binary startup idle. A run that has not returned in a few minutes is a hang, not a slow build; check `ps` elapsed time instead of waiting. `target/` grows (3.8 GB clean, 36 GB when neglected), and a periodic `cargo clean` is cheap maintenance. See `full-workspace-verification-gates.md`.
