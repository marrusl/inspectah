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
├── scripts/          # Repo tooling run by the hook and CI (sensitive tarball gate); shell tests in scripts/tests/
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
| `crates/collect/` | System inspectors (RPM, config, units, etc.) | No |
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
- `units.rs`: systemd unit types for the snapshot field `units` (`UnitSection`: `state_changes`, `inventory`, `drop_ins`, `mount_findings`, `manual_links`, `undetermined_installs`, `default_target`), and the one unit-file line model (`logical_lines`, `holds_install_section`); `UnitName` proves a unit name once and gives its kind, instance and template. A state change's unit is a `CarriedUnit` (an ordinary unit with its `UnitFragment`, or a package-owned storage unit); an administrator's mount, automount or swap unit is an `AdvisoryUnit` in a `MountUnitFinding`, never a state change. `InstanceName::template` builds a `UnitName` without a re-parse, which is sound only because `InstanceName`'s fields are private; any type that feeds an unchecked constructor of a proved type has no public fields (`UnitName`, `UnitSubject`, `UnitStateChange`'s unit and state, and `PlannedUnit` and `Destination` in the pipeline), so a caller cannot swap in text the parser never checked
- `containers.rs` — Podman/Docker container types
- `network.rs`, `storage.rs`, `kernelboot.rs`, `selinux.rs`, `scheduled.rs`, `nonrpm.rs`, `users.rs`
- `sysctl/`: `KernelBootSection.sysctl`, the sysctl search path as collected (`SysctlSection`, `SysctlDir`, `SysctlKey`); records, decides nothing
  - `parse.rs`: the one `sysctl.d(5)` line parser and `sysctl_values_equal`
  - `resolve.rs`: precedence resolution the way systemd-sysctl applies it (`SysctlSection::effective()` gives `Resolved` or `Unknown`), vendor replacements and carriage read from the config entries
- `aggregate.rs` — Aggregate-specific types (consensus items, variants)
- `repo.rs`: repository definitions, `parse_repo_sections`, the one `.repo` reader (`RepoSectionDef`, `GpgKeyRef`), `SectionId` (a header's exact spelling, the identity of a repo decision) beside the case-folding `RepoId`, `effective_definition` and `toggleable`
- `destination.rs`: `Destination`, a normalized image path, and `PathError`, moved from the pipeline's config tree so core's repo ownership pass and the stager compare paths one way
- `subscription.rs` — RHSM entitlement types
- `system.rs`, `os.rs` — System metadata
- `finding.rs` — `FindingKind` enum (Actionable/Advisory/Inventory), `ShadowType`, `AdvisoryType`
- `group_render.rs`, `verified_upload.rs`: group rendering and verified-upload types
- `symlink_allowlist.rs` — Cross-tree symlink allowlist for /usr → /etc paths
- `redaction.rs`, `completeness.rs`, `preflight.rs`, `warnings.rs`, `progress.rs`

**Core traits:** `crates/core/src/traits/` — Inspector, Executor, Renderer, Detector, Progress

**Core data containers:** `snapshot.rs`, `pipeline.rs`, `baseline.rs` at `crates/core/src/`. `baseline.rs` also holds the named unit lists: `TARGET_NEEDED_UNITS` and `TMP_MOUNT` (units the export never switches off, with `target_needs_unit_text`), `RUNTIME_UPDATERS`, `STOCK_STATE` and `INCOMPATIBLE_SERVICES`

**Machine-state class:** `crates/core/src/machine_state.rs` (the rule table, `classify_path`, `normal_path`, the content sniffs, the `ScreenedExecutor` read gate, `validate`) and `crates/core/src/types/machine_state.rs` (members, notices, `WithheldConfigEntry`, `WithheldUnmanagedEntry`); `crates/pipeline/src/render/machine_state.rs` builds the provisioning surfaces; `crates/collect/clippy.toml` refuses the direct routes around the gate (see `machine-state-class.md`)

**Repo ownership:** `crates/core/src/repo_ownership.rs` (`repo_ownership`, who owns each `.repo` file and key; `settle_repo_ownership`, run by the collector, `finish_scan` and the aggregate merge; `check_repo_ownership`, run at load); `crates/pipeline/src/render/repo_owned.rs` writes the Containerfile lines and export notices about them; refine's `RepoIndex` reads its owner maps from it

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
  - `rpm/dnf_vars.rs`: the host's dnf variables, used to expand `gpgkey=` paths in `rpm/repos.rs`
- `config/`: the config files the image carries, from four branches in `mod.rs`: modified packaged files (`rpm -Va`), `%ghost` files, the unowned `/etc` walk (`walk.rs`) and orphans of removed packages
  - `collector.rs`: `ConfigCollector::admit`, the only way a file enters the section: reads through the machine-state read gate and records a withheld row, a `MachineState` entry or an ordinary entry (see `machine-state-class.md`)
  - `admit.rs`: the checks before any read of a candidate: `locate` (a path under a directory link is admitted in both spellings, physical under `/etc` and both carried at their own path, and each branch's exclusions match both) and `readable` (never a FIFO, socket or device; a final link is read only to carried config under `/etc`). `capture_config` in `mod.rs` asks `readable` on every branch
  - `ghost.rs`: the `%ghost` branch: the file-flags query, `classify_ghost` (what is general config, a machine-state member, another section's file, generated, deferred) and the `authselect check`/`authselect test` parsing (see `ghost-config-files.md`)
  - `audit_rules.rs`: whether `/etc/audit/audit.rules` is written by `augenrules`: only when the audit loader units are exactly as packaged, decided by what is present in the unit search path, never by reading a unit
- `units/`: `UnitsInspector`, which writes the `units` section for every unit kind
  - `mod.rs`: the inspector: `systemctl list-unit-files`, the name gate, masks, drop-ins, owners, state changes, mount findings, instances found from links, the install comparison and the default target
  - `resolve.rs`: the unit file systemd would load for a name, from `/etc/systemd/system` and the vendor directory, and the search-path refusal (`UNREAD_SEARCH_PATH`)
  - `install.rs`: the effective `[Install]` section `enable` acts on, from the unit file and its own and template drop-ins (`read_install`, `read_vendor_install`)
  - `links.rs`: the links under both unit directories, the install state they give a unit, hand-made link findings and undetermined install records, and the comparison with the package's `[Install]` (see `unit-install-state.md`)
  - `also.rs`: the units `Also=` reaches from a unit and what the links say about each
  - `specifiers.rs`: `expand_unit_specifiers`, with one `SpecifierSet` each for the `[Install]` reader, the instance reader and a timer's `Unit=`, and `trigger_unit_ignored`, the `Unit=` values systemd ignores
  - `instance.rs`: what an enabled template instance needs (`instance_needs`): `required_config` and `data_paths` from the paths its effective unit names with `%i` or `%I`, and the fixed tables `INSTANCE_CONFIG_TABLE` and `INSTANCE_DATA_TABLE` (see `unit-install-state.md`)
- `containers.rs`, `network.rs`, `storage.rs`, `users.rs`, `selinux.rs`, `kernelboot.rs`, `nonrpm.rs`, `scheduled.rs`, `subscription.rs`
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
- `authselect.rs`: the collected authselect selection rendered as `RUN authselect select <profile> <features> --force` after the config `COPY`, or a FIXME naming the files a custom profile is missing
- `kickstart.rs` — Kickstart file for traditional installs
- `report.rs`, `report_data.rs`: HTML migration report; one template per section under `crates/pipeline/templates/report/` (`units.html` for the unit section)
- `audit.rs` — Audit trail of decisions
- `sysctl_report.rs`: the sysctl report model (`sysctl_report`) and its fixed copy; the audit, the HTML report and refine's sysctl panel all format it, so they cannot disagree
- `md_literal.rs`: host values as literal Markdown text (`literal_text`, code spans, table-cell pipe escapes); the HTML report shares `literal_text`
- `secrets.rs` — Redacted secrets summary
- `tarball.rs` — Snapshot archive packaging
- `users.rs` — User/group materialization (passwd/group entries)
- `not_carried.rs`: the not-carried class (`classify_auto_carriage`, `apply_scan_package_defaults`, `not_carried_packages`); the one decider of which auto packages the image leaves out, read by the Containerfile, the audit report and `unit_plan.rs`; `var_dirs.rs` asks `unit_plan::pulled_in_by_included` directly
- `unit_plan.rs`: `plan_units`, every `systemctl` line for a collected unit and the default target, checked against `staged_writes` (see `service-omission-model.md`); `declined_line`, the one source of a declined command's text; `mount_advisory_lines` for mount findings; `manual_link_lines`, `undetermined_install_lines` and `also_advisory_lines` for link findings and rule 7; `unit_state_lines` writes the `# === Systemd Unit State ===` section
- `language_packages.rs` — Tier 1 pip/npm/gem Containerfile rendering
- `unmanaged.rs`: Tier 2 unmanaged file COPY directives and symlink `ln -sf`; `bundle_unmanaged_files`, which copies the payloads into the scan tarball after the read gate checks each again
- `repoless.rs`: package routing (`route_for`, with the routes an export lost in `InstallRepos::lost_route`), repo-less install planning for the combined local and disabled-repo install, and the scan tarball's cached RPM writer
- `rpm_verify.rs`: `rpm_va_failed_notice`, the export warning for a snapshot whose `rpm -Va` failed, repeating the collector's `RPM_VA_FAILED_WARNING` on scan stderr and in the refine export
- `safety.rs` — Safety checks and destructive-action warnings
- `configtree.rs`: the staged file tree. `staged_writes` is the one decision of which offered files the export writes, keyed by `Destination` (a normalized image path), with every withheld and refused offer; `is_staged` asks it
- `baseline_fmt.rs` — Baseline comparison formatting
- `readme.rs` — Auto-generated README for output directory
- `advisory.rs`, `repo_enabling.rs`: advisory rendering and repository enablement
- `selinux.rs` — SELinux file context rule rendering and the build-time equivalence table (`selinux_equivalences`)
- `var_dirs.rs` — Effective `/var` directory backing at export time (`VarDirOutcome`, `EffectiveBacking`)
- `tmpfiles_accounts.rs`: carried `tmpfiles.d` lines naming a local account the image does not create, for the Containerfile's Configuration Files notice and the audit report; lines are parsed by `TmpfilesEntry` in `crates/core/src/types/storage.rs`, the one `tmpfiles.d` line parser, which the collector's `/var` backing index also uses

**Section grouping:** `crates/pipeline/src/section_group.rs` — groups findings into report sections

**Pipeline orchestration:** `crates/pipeline/src/orchestrate.rs`: `run_pipeline` chains collect, validate, redact, render and tarball. `finish_scan` is the scan's last snapshot change after redaction (repo-less cache annotation, package defaults, repo ownership), called by both `run_pipeline` and the scan command. Inspector execution itself is `crates/pipeline/src/collect.rs`.

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
  - `ServiceSection.tsx`, `UnitGroup.tsx`, `UnitOutcomeFilter.tsx`: the Systemd units section (single-host only) and its collapsible group and outcome filter bar; they render, and `unitOutcome.ts` decides row keys, outcomes, counts, groups, labels, what the filter and search show and the reveal plan, the group header's badge and name and which key presses a row ignores. The section's scoped styles in `App.css` hang off the `.inspectah-units-section` class, because the `.inspectah-decision-row` rules are shared with other sections and the fleet view: change the units section's layout there, never in the shared rules
  - `SystemTuningSection.tsx`: System Tuning section, the tuned decision rows and `SysctlPanel`
  - `SysctlPanel.tsx`, `sysctlCopy.ts`: the read-only sysctl panel and its copy, which mirrors `render/sysctl_report.rs` character for character
  - `RpmUploadModal.tsx` — Single-RPM upload modal with NEVRA validation
  - `RpmBatchUploadModal.tsx` — Multi-RPM batch upload with auto-matching and conflicts view
- `hooks/` — React hooks
  - `useRpmUpload.ts` — 5-state RPM upload row machine wired to POST /api/upload-rpm
  - `useKeyboard.ts`: keyboard shortcuts (number keys 1-8 jump to section groups from `/api/groups`; without groups, 1-9 jump to flat sections)
  - `useView.ts`, `useMutation.ts` — view data and mutation hooks
  - `useUnitsSectionState.ts`: the Systemd units section's state (filter, pins, collapse, reveal steps, tab stop, focus after a toggle); it calls `unitOutcome.ts` for every rule
- `api/types.ts` — TypeScript types (LanguagePackageEnv, UnmanagedFileItem, ProvenanceSignals, RpmUploadRowState)
- `utils/` — utility functions
- `test-utils/` — test fixtures

Playwright end-to-end tests live beside `src/`, in `crates/web/ui/e2e/`.

**Backend API:** `crates/web/src/` — axum handlers for REST endpoints
  - `upload.rs` — `POST /api/upload-rpm` for repo-less RPM uploads (500 MiB route-specific limit)
  - `sysctl_view.rs`: `project_sysctl`, the sysctl panel DTOs projected from `sysctl_report` for single-host and aggregate views

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
| `crates/*/tests/` | Per-crate integration tests, built as one binary per crate (cli keeps `units_scan_test` as a second binary): `tests/main.rs` declares each file as a module (`autotests = false`); run one file with `cargo test -p <package> --test integration <file stem>::`, and cli's `units_scan_test` with `cargo test -p inspectah-cli --test units_scan_test`. See [integration-test-layout](integration-test-layout.md) |
| `crates/cli/tests/units_scan_test.rs` | The units section through the scan path on collector-built hosts, then render, refine and the fleet merge; hosts come from the `UnitHost` builder in `crates/cli/tests/support/unit_hosts.rs` |
| `crates/cli/tests/staged_writes_parity_test.rs` | The scan and refine exports stage the same files from one snapshot |
| `crates/collect/tests/units_test.rs` | The units inspector over `MockExecutor` |
| `crates/pipeline/tests/unit_plan_test.rs` | `plan_units` and the unit state section on hand-built `UnitSection`s |
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

`crates/cli/tests/completions_sync_test.rs` fails unless each file equals what `inspectah completions <shell>` prints; see [completions-are-checked-in-and-sync-tested](completions-are-checked-in-and-sync-tested.md). The RPM does not install these files. `packaging/inspectah.spec` generates its own from the built binary with the hidden `inspectah completions <shell>` subcommand.

## Gotchas

- **Attribution style** — LLM-assisted commits include `Assisted-by: <tool> (<model>)`. No other identifiers.
- **Build script version:** `crates/cli/build.rs` stamps version metadata at compile time. Don't manually update version strings in code.
- **Schema versioning:** Snapshot JSON has a `schema_version` field that MUST be bumped when a change alters what the snapshot serializes, though a new `Option` field with `#[serde(default)]` or a new `SectionData` variant does not. See `snapshot-schema-versioning.md`.
- **Package identity is `name.arch`** — Never use bare package names. See `package-identity-is-name-dot-arch.md`.
- **Clippy clean is one command:** `cargo clippy --workspace --all-targets -- -D clippy::all`, run identically by the pre-commit hook and CI. See `full-workspace-verification-gates.md`.
- **Every gate is fast** — measured on an M3 Pro: a from-scratch `cargo clean` plus build plus the full 3010-test suite is two to two and a half minutes, and the pre-commit hook on a realistic change is about a minute to a minute and a half (measured 2026-11-08). There is no per-binary startup idle. A run that has not returned in a few minutes is a hang, not a slow build; check `ps` elapsed time instead of waiting. `target/` grows (3.8 GB clean, 36 GB when neglected), and a periodic `cargo clean` is cheap maintenance. See `full-workspace-verification-gates.md`.
