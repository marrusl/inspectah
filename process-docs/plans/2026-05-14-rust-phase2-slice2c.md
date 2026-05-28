# Phase 2 Slice 2c: RPM-Dependent Inspectors Implementation Plan

**Date:** 2026-05-14
**Branch:** `rust`
**Baseline commit:** `426af1c` (Slice 2b complete, 478 tests)

## Prerequisites

1. Slice 2b merged to `rust` branch — all 478 tests passing
2. CentOS Stream 9 test host accessible for host validation
3. Go `inspectah` available on host for golden file capture
4. CentOS toolchain: `dnf install rust cargo gcc` (system Rust, NOT Darwin rustup path)

## Three Proof Lanes

This plan maintains three separate proof lanes, matching the pattern established in Slices 2a and 2b:

| Lane | Location | What it proves |
|------|----------|----------------|
| **Serde/golden roundtrip** | `inspectah-core/tests/parity_gate.rs` | Rust types can deserialize Go-captured golden JSON and re-serialize without loss |
| **Inspector-on-fixture** | `inspectah-collect/tests/{scheduled,config,selinux,nonrpm}_test.rs` | Rust inspectors produce correct output from fixture data via MockExecutor |
| **Live-host closure** | `testdata/evidence/`, `scripts/host-validation.sh` | Real Go+Rust scans match on a live system |

Each new section (scheduled_tasks, config, selinux, non_rpm_software) MUST have entries in all three lanes. The lanes are NOT interchangeable — passing one does not excuse skipping another.

### Normalizer note

`normalize.rs` strips Rust-only fields (`redaction_state`, `completeness`) before parity comparison. The serde/golden lane tests deserialization of Go output into Rust types; it does NOT run inspectors. The inspector-on-fixture lane tests Rust inspector logic against synthetic fixtures; it does NOT compare against Go golden files. These are separate proof obligations.

## Success Criteria

1. Four new inspectors implemented: `ScheduledTasksInspector`, `ConfigInspector`, `SelinuxInspector`, `NonRpmInspector`
2. All four implement the unified `Inspector` trait (same as Wave 1 inspectors). They access `rpm_state` via `ctx.rpm_state` in `InspectionContext`, NOT via a separate trait.
3. Each inspector has unit tests covering happy path, degraded, empty, and edge cases
4. Sensitive input handling verified:
   - Config file `content` field: redaction scans for embedded passwords, tokens, keys
   - `.env` file content from nonrpm: redaction scans for secret-like key=value pairs
   - Cron job `command`, at job `command`, timer `ExecStart`: redaction scans for credential arguments
   - Audit rule files: redaction scans for embedded credentials
   - PAM config content: redaction scans for password-related module arguments
   - Git remote URLs: redaction scans for embedded credentials in URL
   - No `rpm -Va` diff content in metadata or remediation fields without redaction scan
5. RpmState expanded with `packages`, `verification_results`, `module_streams` fields + capability methods
6. Wave 2 dispatch logic implemented: ID-based classifier using `matches!()` macro
7. Parity gate covers all 11 sections cumulative (RPM + services + storage + kernelboot + network + containers + users_groups + scheduled_tasks + config + selinux + non_rpm_software)
8. Renderer smoke tests passing for all 4 new sections against all 7 consumers (containerfile, configtree, env-files, kickstart, readme, audit, report). Basic audit.rs and report.rs rendering for these sections is implemented and tested in this slice. `.env` files materialize under `env-files/` (not `config/`); Containerfile emits commented-out COPY with FIXME.
9. Failure policy tested: degraded for parse errors, PermissionDenied; silent skip for NotFound
10. Redaction engine extended for new persisted surfaces with planted-secret proofs
11. Host validation on same CentOS Stream 9 box using the real Rust CLI (`inspectah-cli scan`), closing all Slice 2a, 2b, AND 2c evidence in one pass across all 11 sections
12. Test count target: ~478 (Slice 2b baseline) + ~170 new = ~648+ (includes +14 tests from round-3 revision: 2 RpmState failure policy, 3 redaction detection proofs, 9 audit/report renderer smoke tests; +6 tests from round-4 revision: 2 rendered-output absence proofs for audit/report, 4 env-files output contract tests)
13. All commits follow conventional commit format
14. Clippy clean, `cargo fmt` clean

## Design Decisions

### 1. RpmState ownership contract

`RpmState` is the single source of truth for RPM package ownership data consumed by Wave 2 inspectors. This section documents the full data flow from shell command to capability method.

**Current state (Slice 2b):** `RpmState` has two fields — `installed_packages: HashSet<String>` and `owned_paths: HashSet<String>`. The `handle_result()` function in `collect.rs` populates `installed_packages` from `rpm.packages_added` when the RPM inspector completes. `owned_paths` has a placeholder comment ("populated in later slices").

**Slice 2c expansion:** `RpmState` gains three new fields and corresponding capability methods. Types already exist in `inspectah-core/src/types/rpm.rs`:

```
packages: Vec<PackageEntry>        → installed_packages() -> &[PackageEntry]
verification_results: Vec<RpmVaEntry> → verification_results() -> &[RpmVaEntry]
module_streams: Vec<EnabledModuleStream> → module_streams() -> &[EnabledModuleStream]
```

Plus two derived capability methods built from `packages`:

```
owned_paths() -> &HashSet<PathBuf>    // built once, cached
is_rpm_owned(path: &Path) -> bool     // O(1) lookup into owned_paths
package_for_path(path: &Path) -> Option<&PackageEntry>  // reverse lookup
```

**Data flow — single query, two data structures:**

1. **Shell command:** `rpm -qa --queryformat '%{NAME}\t[%{FILENAMES}\n]'` — produces `package_name\tfilepath` per line. This is ONE query that provides BOTH the owned-paths set and the reverse path-to-package map. This matches Go's `BuildRpmOwnedPaths` function (line 344 of `orchestrator.go`).
2. **RPM inspector output:** The RPM inspector already runs this query and stores the results in its `RpmSection` output. The `file_ownership` field contains `(package_name, filepath)` pairs. The full listing is available in `rpm_output.section` after the RPM inspector joins.
3. **`handle_result()` in `collect.rs`:** Currently extracts `installed_packages` from RPM output. Slice 2c extends this to also extract both `owned_paths` and `path_to_package` from the same `file_ownership` data, plus `verification_results` and `module_streams`:
   ```rust
   if inspector.id() == InspectorId::Rpm {
       if let SectionData::Rpm(ref rpm) = output.section {
           rpm_state.installed_packages =
               rpm.packages_added.iter().map(|p| p.name.clone()).collect();
           // NEW in Slice 2c: populate BOTH owned_paths AND path_to_package
           // from the SAME file_ownership query output
           for (pkg_name, filepath) in &rpm.file_ownership {
               if filepath.starts_with("/etc") {
                   let path = PathBuf::from(filepath);
                   rpm_state.owned_paths.insert(path.clone());
                   // Find the package index for the reverse lookup
                   if let Some(idx) = rpm.packages_added.iter()
                       .position(|p| &p.name == pkg_name) {
                       rpm_state.path_to_package.insert(path, idx);
                   }
               }
           }
           rpm_state.packages = rpm.packages_added.clone();
           rpm_state.verification_results = rpm.rpm_va.clone();
           rpm_state.module_streams = rpm.enabled_module_streams.clone();
       }
   }
   ```
4. **`RpmState::owned_paths()` / `is_rpm_owned()`:** Pure O(1) lookups into the pre-built HashSet. No lazy initialization, no interior mutability.
5. **`package_for_path()`:** Uses `path_to_package: HashMap<PathBuf, usize>` — an index mapping each owned `/etc` path to its position in the `packages` vec. Built in the same loop as `owned_paths` during `handle_result()` (see step 3). This means `package_for_path()` is a two-step lookup: HashMap get → index into `packages` slice. Both `owned_paths` and `path_to_package` are derived from the same `file_ownership` data — there is no separate query or separate data source.

**Design constraint:** `RpmState` is immutable after construction in `handle_result()`. No interior mutability, no write access. Wave 2 inspectors receive `&RpmState` (shared reference).

**RPM failure propagation through `collect()`:**

```
collect() runs RPM inspector in Wave 1.
- If RPM inspector returns Ok: extract RpmState from output, pass Some(&rpm_state) to Wave 2
- If RPM inspector returns Err(Failed): rpm_state stays None, pass None to Wave 2 context
- If RPM inspector returns Err(Degraded): extract partial RpmState from degraded output, pass Some(&partial_state)

Wave 2 inspectors check ctx.rpm_state:
- None → return Err(InspectorError::Failed { reason: "RPM prerequisite unavailable" })
- Some(state) → proceed (even if state has empty owned_paths)
```

The key distinction: `ctx.rpm_state` is `None` (not `Some(Default::default())`). The `InspectionContext` field is already `Option<&'a RpmState>`:
- `ctx.rpm_state: None` → RPM inspector failed entirely. Wave 2 inspectors MUST return `Err(InspectorError::Failed { reason: "RPM prerequisite unavailable" })`. They do NOT produce partial or Degraded output — RPM failure is fatal to all dependents.
- `ctx.rpm_state: Some(state)` where `state.owned_paths.is_empty()` → RPM inspector succeeded but found no ownership data (unusual but valid — e.g., minimal container with no `/etc` files in RPM manifests). Wave 2 inspectors proceed normally with empty lookups, producing valid (but sparse) output.

This distinction matters: `None` means "we have no data and cannot trust any ownership classification," while `Some(empty)` means "we confirmed there are no RPM-owned paths." Wave 2 inspectors check `ctx.rpm_state` with a match or `if let` — NOT `unwrap()`.

### 2. Wave 2 partition — unified `Inspector` trait with ID-based classifier

**Single orchestration model:** All 11 inspectors implement the existing `Inspector` trait. There is NO separate `RpmDependentInspector` trait. Wave 2 inspectors access `rpm_state` through `InspectionContext.rpm_state` (which is already `Option<&'a RpmState>` in the current codebase).

**Classifier in `collect.rs`:**

```rust
fn is_wave2(id: InspectorId) -> bool {
    matches!(id,
        InspectorId::ScheduledTasks
        | InspectorId::Config
        | InspectorId::Selinux
        | InspectorId::NonRpmSoftware
    )
}
```

**How it integrates with existing code:**

- **CLI construction (`scan.rs`):** All 11 inspectors go into a single `Vec<Box<dyn Inspector>>`. No separate collection. The CLI does not know about waves.
- **`collect()` inputs:** Unchanged — takes `&[Box<dyn Inspector>]` as today.
- **Partition in `collect()`:** The existing partition loop (currently putting everything in `wave1`) gains the `is_wave2()` gate:
  ```rust
  for insp in &applicable {
      if is_wave2(insp.id()) {
          wave2.push(insp);
      } else {
          wave1.push(insp);
      }
  }
  ```
- **Wave 2 context:** Conditional on RPM outcome. If RPM succeeded: `enriched_ctx` with `rpm_state: Some(&rpm_state)`. If RPM failed: `enriched_ctx` with `rpm_state: None`. Wave 2 inspectors match on `ctx.rpm_state`: `None` → return `Failed("RPM prerequisite unavailable")`, `Some(state)` → proceed with `state.is_rpm_owned()` etc.
- **Result handling:** Wave 2 results go through the same `handle_result()` path. No separate routing.
- **Completeness routing:** Same `failed`/`degraded` vectors for both waves.

Wave 1 spawns RPM + 6 independent inspectors. After RPM joins and `handle_result()` populates `RpmState`, Wave 2 spawns the 4 RPM-dependent inspectors with `&RpmState` via the enriched context. Wave 2 joins via the existing `std::thread::scope` pattern.

### 3. Config inspector decomposition

The config inspector is ~800 lines in Go — the largest single inspector. It decomposes into 4 submodules:

```
inspectah-collect/src/inspectors/config/
├── mod.rs       — ConfigInspector struct, Inspector trait impl, orchestration
├── classify.rs  — ClassifyConfigPath, categoryRules table, 13 ConfigCategory variants
├── walk.rs      — walkEtcRecursive, isExcludedUnowned, excludedUnownedPaths/Globs, IsDevArtifact
└── rpmva.rs     — parseRpmVaLine, RpmVaFlags parsing (S/M/5/D/L/U/G/T/P/c/d/g/l/r)
```

`dnf download + rpm2cpio` diff enrichment is **deferred** to Phase 3 — Phase 2 config inspector does not generate diffs.

### 4. NonRpm RPM dependency and .env file handling

NonRpm is Wave 2 because the Go orchestrator runs it after RPM. However, Go's `RunNonRpmSoftware` does NOT use `rpmOwnedPaths` — it uses `file` and `readelf` for classification, pip/npm/gem metadata for language packages, and `.env` file scanning. Match Go: place in Wave 2 for ordering, but the inspector does not call `rpm_state.is_rpm_owned()`.

**IMPORTANT — .env file output contract (DECIDED):** `.env` files materialize under a SEPARATE `env-files/` output path, NOT under `config/`. This is a deliberate divergence from Go parity — Go puts .env files into the config tree, but .env files are high-probability secret carriers that require operator review before inclusion in a container image.

The NonRpm inspector MUST:
- Emit `RedactionHint` entries for every `.env` file detected (these are high-probability secret carriers)
- Persist `.env` file paths and content in `env_files[]` (matching Go schema)
- The redaction engine scans all persisted `env_files[].content` for secret patterns

The `.env` content IS persisted in the snapshot (matching Go behavior), and `include: true` entries are visible in the refine UI. However, materialization goes to `env-files/` (not `config/`), and the Containerfile emits a commented-out `# COPY env-files/ /` with a FIXME noting operator review is required.

**configtree.rs change:** The existing `.env` materialization code (lines 426-441 of configtree.rs) is REMOVED from `write_config_tree()`. A new `write_env_files()` function writes `.env` files under `env-files/` in the output directory instead. This function applies the same `include` filter and path validation as the old code.

### 5. ffi-selinux deferred

Pure command-based collection for Phase 2. No `libselinux` FFI bindings. All SELinux data comes from:
- `getenforce` / reading `/sys/fs/selinux/enforce`
- `semanage boolean -l`, `semanage fcontext -l -C`, `semanage port -l`
- Filesystem reads for custom modules, audit rules, PAM configs, FIPS mode

FFI bindings deferred to Phase 3.

### 6. Renderers already handle all 4 sections

Existing renderer functions verified in the Rust tree (line numbers from `git show rust:`):

**containerfile.rs:**
- `scheduled_tasks_section_lines()` (line 589) — timer enable, cron-to-timer FIXME
- `config_section_lines()` (line 694) — config COPY comments, crypto policy
- `config_copy_roots_from_snapshot()` (line 761) — config directory listing
- `non_rpm_section_lines()` (line 862) — ELF binary FIXME, pip/npm/gem provisioning
- `selinux_section_lines()` (line 1109) — custom modules, boolean overrides, fcontext, ports

**configtree.rs:**
- `write_config_tree()` (line 138) — materializes config files under `config/`
- Scheduled tasks: generated/local timer units → `config/etc/systemd/system/` (line 282)
- Non-RPM `.env` files: NO LONGER materialized by configtree.rs (moved to `write_env_files()` under `env-files/`)

**kickstart.rs and readme.rs:** Consume scheduled/config/selinux/nonrpm data for kickstart suggestions and findings summaries.

**Audit/report renderers:** `audit.rs` currently renders: rpm, config, services, storage, kernel_boot, plus completeness/warnings/redactions. It does NOT render scheduled_tasks, selinux, or non_rpm_software. `report.rs` renders summary cards (package count, config count, service count, storage count, kernel/boot count, warnings) via HTML — it does NOT have per-section detail rendering for any Slice 2c sections.

**Slice 2c scope (DECIDED):** This plan adds basic audit.rs and report.rs rendering for the 4 new sections — minimal viable output, not full-featured. The implementation is lightweight (~30-50 lines per section in audit.rs, summary card updates in report.rs) and is budgeted in Task 11 alongside the smoke tests. See Task 11 for the specific renderer additions and their smoke tests.

Smoke tests in Task 11 verify ALL 7 output consumers: containerfile, configtree, env-files, kickstart, readme (existing behavior), PLUS audit and report (new additions in this slice).

### 7. Sensitive input handling — authoritative secret surface inventory

Every field that persists free-form text content is a potential secret carrier. This inventory is exhaustive for all 4 Slice 2c inspectors.

**Config inspector secret surfaces:**

| Field | Persisted? | Secret risk | Redaction action |
|-------|-----------|-------------|-----------------|
| `ConfigFileEntry.content` | YES | HIGH — RPM-owned modified files contain real config (httpd.conf, sshd_config) that may embed passwords, API keys, database credentials | Redaction engine scans every persisted `content` field |
| `ConfigFileEntry.diff_against_rpm` | NO (Phase 3) | HIGH when populated | Will require redaction scan when implemented |
| `ConfigFileEntry.rpm_va_flags` | YES | NONE — flag characters like `S.5....T.` describe attribute changes, not content | No scan needed |
| `ConfigFileEntry.path` | YES | NONE — filesystem path | No scan needed |
| `ConfigFileEntry.package` | YES | NONE — RPM package name | No scan needed |

**Scheduled tasks inspector secret surfaces:**

| Field | Persisted? | Secret risk | Redaction action |
|-------|-----------|-------------|-----------------|
| Cron job `command` | YES | MEDIUM — command arguments may contain `--password=X`, `--token=X` | Redaction engine scans |
| At job `command` | YES | MEDIUM — shell commands may contain embedded credentials | Redaction engine scans |
| Timer unit `ExecStart` | YES | LOW — usually references script paths, but may have credential arguments | Redaction engine scans |
| Timer `OnCalendar`/schedule fields | YES | NONE — temporal expressions | No scan needed |

**SELinux inspector secret surfaces:**

| Field | Persisted? | Secret risk | Redaction action |
|-------|-----------|-------------|-----------------|
| Audit rule file content | YES | LOW — audit directives, may reference sensitive paths | Redaction engine scans |
| PAM config content | YES | LOW — PAM module config, may have password-related module arguments | Redaction engine scans |
| Boolean names/values | YES | NONE — SELinux boolean metadata | No scan needed |
| Fcontext rules | YES | NONE — SELinux file context policy | No scan needed |
| Port labels | YES | NONE — type/protocol/port tuples | No scan needed |
| Module names | YES | NONE — SELinux module identifiers | No scan needed |

**NonRpm inspector secret surfaces:**

| Field | Persisted? | Secret risk | Redaction action |
|-------|-----------|-------------|-----------------|
| `env_files[].content` | YES | CRITICAL — `.env` files are primary secret carriers (`DATABASE_URL`, `API_KEY`, etc.) | Redaction engine scans + RedactionHint emitted |
| Git remote URLs | YES | MEDIUM — may contain embedded credentials (`https://user:token@github.com/...`) | Redaction engine scans |
| `strings` 4KB head output | NO — classification-only, not persisted | N/A | The 4KB `strings` head is used ONLY for version extraction (e.g., "1.2.3"). The raw `strings` output is never written to the snapshot. Only the extracted version string is persisted in `NonRpmItem.version`, and version strings are safe metadata (not secret-bearing). No redaction scan needed. |
| Binary paths/names | YES | NONE — filesystem paths | No scan needed |
| Language detection results | YES | NONE — Go/Rust/C classification | No scan needed |

**Planted-secret-absent proof obligation (Task 9):**
For every persisted secret-bearing field listed above, the redaction test suite must include a planted-secret test proving that a raw secret substring (e.g., `password=secret123`, `AKIA...`) does NOT survive undetected into:
- Snapshot JSON output
- `config/` tree artifacts (materialized files under `config/`)
- `env-files/` artifacts (materialized .env files)
- Rendered outputs (Containerfile, kickstart, readme, audit report, HTML report)

**Golden promotion safety gate (Task 13):**
Before promoting host-captured JSON to committed goldens:
- Host validation uses a private temp directory (`/tmp/inspectah-host-validation-*`) with cleanup
- Config file content in goldens must be reviewed for real secrets from the validation host
- If the validation host has real credentials in `/etc` configs, the golden files must be sanitized or the specific fields redacted before committing
- Alternative: use a clean validation host with no real secrets in `/etc`

## Artifact-Consumer Matrix

This matrix maps which Slice 2c inspector sections drive which renderers. Audit (`audit.rs`) and report (`report.rs`) get basic rendering additions in Task 11 of this slice.

| Section | containerfile.rs | configtree.rs | env-files/ (NEW) | kickstart.rs | readme.rs | audit.rs (NEW) | report.rs (NEW) |
|---------|-----------------|---------------|------------------|--------------|-----------|
| **scheduled_tasks** | `scheduled_tasks_section_lines()`: timer unit enables for included timers, cron-to-timer FIXME comments for convertible entries, `@reboot` flagged as non-convertible | Generated/local timer units materialized under `config/etc/systemd/system/` as `.timer` + `.service` files | — | Cron-to-timer conversion suggestions | Timer/cron count in findings summary | NEW: `## Scheduled Tasks` — cron job count, timer count, at job count, `@reboot` warnings | NEW: scheduled task count in summary card |
| **config** | `config_section_lines()`: COPY comments for included config files organized by config directory roots, crypto policy detection note | `write_config_tree()`: materializes all included `ConfigFileEntry` files under `config/` tree. DHCP connections excluded via `dhcp_connection_paths()`. Path validation prevents traversal. | — | Config file references for kickstart `%post` scripts | Config file counts by kind (modified/unowned/orphaned) in findings summary | EXISTING: `## Configuration Files` — already renders modified/unowned file lists | EXISTING: config file count in summary card |
| **selinux** | `selinux_section_lines()`: custom module COPY+semodule comments, `setsebool -P` for non-default booleans, `semanage fcontext` for custom rules, `semanage port` for port labels | Audit rule files and PAM config files materialized under `config/etc/audit/rules.d/` and `config/etc/pam.d/` as declarative carry-forward content. `configtree.rs` handles materialization. These are intentional admin customizations — no FIXME wrapping. | — | SELinux boolean/fcontext suggestions | SELinux customization counts in findings summary | NEW: `## SELinux` — mode, custom module count, non-default boolean count, custom fcontext count, FIPS status | NEW: SELinux mode in summary card |
| **non_rpm_software** | `non_rpm_section_lines()`: ELF binary FIXME comments, pip `requirements.txt` COPY+install, npm `package-lock.json` COPY+install, gem `Gemfile` COPY+install, venv provisioning. `.env` files: commented-out `# COPY env-files/ /` + FIXME for operator review | `.env` files materialized under `env-files/` (NOT under `config/`) via `write_env_files()`. configtree.rs does NOT handle .env files. | Not directly consumed | Non-RPM item counts by type/language in findings summary | NEW: `## Non-RPM Software` — item count by type (ELF/pip/npm/gem), `.env` file count with warning | NEW: non-RPM item count in summary card |

### Inspector ownership boundaries (no double-ownership)

These boundaries prevent two inspectors from claiming the same filesystem paths:

| Path pattern | Owning inspector | Other inspectors |
|-------------|-----------------|-----------------|
| `/etc/audit/rules.d/*` | **SELinux** — collected as audit rule content, materialized to `config/etc/audit/rules.d/` by `configtree.rs` as declarative carry-forward (no FIXME wrapping) | **Config** MUST skip these paths during `/etc` walk (add to `unownedExcludeExact` or prefix filter) |
| `/etc/pam.d/*` | **SELinux** — collected as PAM config content, materialized to `config/etc/pam.d/` by `configtree.rs` as declarative carry-forward (no FIXME wrapping) | **Config** MUST skip these paths during `/etc` walk (add to prefix filter) |
| Vendor/system timer units in `/usr/lib/systemd/system/` | **Scheduled** — informational scan only | **Config** does not touch `/usr/lib` (only walks `/etc`) |
| Cron spool files in `/var/spool/cron/`, `/var/spool/at/` | **Scheduled** — advisory scan, `/var` is not declarative | These files are NOT copied to `config/` tree (cron spool from `/var` is advisory, not declarative) |

### Negative contract coverage (Task 11 must verify)

These are things that MUST NOT happen:

- Vendor timer units from `/usr/lib/systemd/system/` MUST NOT be copied to `config/` tree (only generated/local timers and explicitly captured user timers)
- Cron spool from `/var` is advisory — no materialization into `config/`
- Audit rule files owned by SELinux inspector MUST NOT appear as unowned config files
- PAM configs owned by SELinux inspector MUST NOT appear as unowned config files

## File Map

### New Files

| File | Purpose |
|------|---------|
| `inspectah-collect/src/inspectors/scheduled.rs` | Scheduled tasks inspector |
| `inspectah-collect/src/inspectors/config/mod.rs` | Config inspector orchestration |
| `inspectah-collect/src/inspectors/config/classify.rs` | Config path classification (13 categories) |
| `inspectah-collect/src/inspectors/config/walk.rs` | Recursive `/etc` walk + exclusion logic |
| `inspectah-collect/src/inspectors/config/rpmva.rs` | `rpm -Va` output parsing |
| `inspectah-collect/src/inspectors/selinux.rs` | SELinux inspector |
| `inspectah-collect/src/inspectors/nonrpm.rs` | Non-RPM software inspector |
| `testdata/fixtures/scheduled/*` | Cron, timer, at job fixtures |
| `testdata/fixtures/config/*` | Config file, rpm -Va output fixtures |
| `testdata/fixtures/selinux/*` | SELinux command output fixtures |
| `testdata/fixtures/nonrpm/*` | ELF readelf output, pip/npm/gem fixtures |
| `testdata/golden/go-v13-scheduled-tasks-section.json` | Go golden for scheduled_tasks (provisional) |
| `testdata/golden/go-v13-config-section.json` | Go golden for config (provisional) |
| `testdata/golden/go-v13-selinux-section.json` | Go golden for selinux (provisional) |
| `testdata/golden/go-v13-non-rpm-software-section.json` | Go golden for non_rpm_software (provisional) |
| `inspectah-collect/tests/scheduled_test.rs` | Inspector-on-fixture tests for scheduled |
| `inspectah-collect/tests/config_test.rs` | Inspector-on-fixture tests for config |
| `inspectah-collect/tests/selinux_test.rs` | Inspector-on-fixture tests for selinux |
| `inspectah-collect/tests/nonrpm_test.rs` | Inspector-on-fixture tests for nonrpm |
| `inspectah-pipeline/tests/smoke_render_2c.rs` | Renderer smoke tests for 2c sections |
| `inspectah-pipeline/tests/redaction_2c_surfaces_test.rs` | Redaction planted-secret proofs |
| `testdata/evidence/slice-2c-host-validation.md` | Host validation evidence |

### Modified Files

| File | Change |
|------|--------|
| `inspectah-core/src/traits/inspector.rs` | Expand `RpmState` with new fields + capability methods |
| `inspectah-pipeline/src/collect.rs` | Wave 2 dispatch: `is_wave2()` classifier, partition loop gate, `handle_result()` RpmState expansion |
| `inspectah-collect/src/inspectors/mod.rs` | Register `scheduled`, `config`, `selinux`, `nonrpm` modules |
| `inspectah-cli/src/commands/scan.rs` | Add 4 new inspectors to the `inspectors` vec in `run_scan()` |
| `inspectah-pipeline/src/redaction/engine.rs` | Extend to scan new persisted surfaces (config content, .env content, cron/at commands, timer ExecStart, audit rules, PAM configs, Git remote URLs) |
| `inspectah-pipeline/src/render/configtree.rs` | Remove `.env` file materialization from `write_config_tree()`; add `write_env_files()` function that writes `.env` files under `env-files/` output path |
| `inspectah-pipeline/src/render/containerfile.rs` | Add commented-out `# COPY env-files/ /` + FIXME for .env files in `non_rpm_section_lines()` |
| `inspectah-pipeline/src/render/audit.rs` | Add scheduled_tasks, selinux, non_rpm_software section rendering (~80-100 lines) |
| `inspectah-pipeline/src/render/report.rs` | Add scheduled tasks, SELinux mode, non-RPM item count summary cards (~15-20 lines) |
| `inspectah-core/tests/parity_gate.rs` | Expand serde/golden roundtrip tests to scheduled_tasks, config, selinux, non_rpm_software sections |
| `inspectah-collect/tests/parity_test.rs` | Expand inspector-on-fixture tests to 4 new sections |
| `scripts/host-validation.sh` | Extend to cover all 11 sections |
| `testdata/divergences.md` | Add any new divergence entries |

## Task Dependency Graph

```
Task 1 (RpmState + Wave 2) ── Task 2 (fixtures) ──┬── Task 3 (scheduled) ───┐
                                                   ├── Task 4 (config) ──────┤
                                                   ├── Task 5 (selinux) ─────┼── Task 7 (CLI) ── Task 8 (integration) ── Task 9 (redaction) ── Task 10 (parity gate) ── Task 11 (renderer smoke) ── Task 12 (failure policy) ── Task 13 (host validation) ── Task 14 (final)
                                                   └── Task 6 (nonrpm) ──────┘
```

Tasks 3, 4, 5, 6 can run in parallel after Task 2 completes.
Tasks 7–14 are sequential.

---

## Task 1: RpmState Expansion + Wave 2 Dispatch Logic

**Files:**
- Modify: `inspectah-core/src/traits/inspector.rs`
- Modify: `inspectah-pipeline/src/collect.rs`

This task expands `RpmState` from a placeholder to a full capability surface and wires up Wave 2 dispatch in the collection pipeline. No new traits are introduced — the existing `Inspector` trait is used for all inspectors.

- [ ] **Step 1: Expand RpmState struct**

Expand the existing `RpmState` in `inspectah-core/src/traits/inspector.rs` (currently has `installed_packages: HashSet<String>` and `owned_paths: HashSet<String>`):

```rust
pub struct RpmState {
    pub installed_packages: HashSet<String>,     // existing
    pub owned_paths: HashSet<PathBuf>,           // existing (change from String to PathBuf)
    pub packages: Vec<PackageEntry>,             // NEW
    pub verification_results: Vec<RpmVaEntry>,   // NEW
    pub module_streams: Vec<EnabledModuleStream>,// NEW
    pub path_to_package: HashMap<PathBuf, usize>,  // NEW — index into packages vec
}
```

Implement capability methods:

| Method | Signature | Notes |
|--------|-----------|-------|
| `installed_packages` | `&self -> &HashSet<String>` | Existing field access |
| `packages` | `&self -> &[PackageEntry]` | Direct slice access |
| `owned_paths` | `&self -> &HashSet<PathBuf>` | Pre-built set |
| `is_rpm_owned` | `&self, path: &Path -> bool` | O(1) HashSet lookup |
| `package_for_path` | `&self, path: &Path -> Option<&PackageEntry>` | Index lookup via `path_to_package` |
| `verification_results` | `&self -> &[RpmVaEntry]` | Direct slice access |
| `module_streams` | `&self -> &[EnabledModuleStream]` | Direct slice access |

No builder constructor needed — `handle_result()` populates the fields directly (see Step 3).

- [ ] **Step 2: Add Wave 2 classifier in collect.rs**

```rust
fn is_wave2(id: InspectorId) -> bool {
    matches!(id,
        InspectorId::ScheduledTasks
        | InspectorId::Config
        | InspectorId::Selinux
        | InspectorId::NonRpmSoftware
    )
}
```

- [ ] **Step 3: Expand `handle_result()` RpmState extraction**

The existing `handle_result()` already extracts `installed_packages` from RPM output. Extend it to also populate `owned_paths`, `path_to_package`, and new fields — all from the same `file_ownership` query output:

```rust
if inspector.id() == InspectorId::Rpm {
    if let SectionData::Rpm(ref rpm) = output.section {
        rpm_state.installed_packages =
            rpm.packages_added.iter().map(|p| p.name.clone()).collect();
        // NEW: populate BOTH owned_paths AND path_to_package
        // from the SAME file_ownership data (single query output)
        for (pkg_name, filepath) in &rpm.file_ownership {
            if filepath.starts_with("/etc") {
                let path = PathBuf::from(filepath);
                rpm_state.owned_paths.insert(path.clone());
                if let Some(idx) = rpm.packages_added.iter()
                    .position(|p| &p.name == pkg_name) {
                    rpm_state.path_to_package.insert(path, idx);
                }
            }
        }
        rpm_state.packages = rpm.packages_added.clone();
        rpm_state.verification_results = rpm.rpm_va.clone();
        rpm_state.module_streams = rpm.enabled_module_streams.clone();
    }
}
```

Note: `rpm.file_ownership` contains the output of `rpm -qa --queryformat '%{NAME}\t[%{FILENAMES}\n]'` — `(package_name, filepath)` pairs. This is the same data Go's `BuildRpmOwnedPaths` uses. Both `owned_paths` and `path_to_package` are derived from this single source in one pass.

**RPM failure path:** If the RPM inspector returns `Err(Failed)`, `handle_result()` records the failure and sets a `rpm_populated = false` flag. The `rpm_state` variable is never populated from output. In Step 4, the Wave 2 context checks this flag: `rpm_state: if rpm_populated { Some(&rpm_state) } else { None }`. Wave 2 inspectors that receive `None` return `Err(InspectorError::Failed { reason: "RPM prerequisite unavailable" })`. See Step 4 for the conditional logic.

- [ ] **Step 4: Wire Wave 2 partition and conditional rpm_state in collect.rs**

Modify the existing partition loop (which currently puts everything in `wave1`):

```rust
for insp in &applicable {
    if is_wave2(insp.id()) {
        wave2.push(insp);
    } else {
        wave1.push(insp);
    }
}
```

Modify the existing Wave 2 dispatch block to pass `None` when RPM failed and `Some(&rpm_state)` when it succeeded (including degraded with partial data):

```rust
if !wave2.is_empty() {
    // Track whether RPM inspector succeeded (Ok or Degraded)
    // vs failed entirely. rpm_populated is set to true in
    // handle_result() when RPM output was successfully extracted.
    let wave2_rpm_state: Option<&RpmState> = if rpm_populated {
        Some(&rpm_state)
    } else {
        None  // RPM failed → Wave 2 inspectors get None → return Failed
    };

    let enriched_ctx = InspectionContext {
        source_system: source,
        executor,
        rpm_state: wave2_rpm_state,
    };

    // ... spawn and join as before
}
```

The `rpm_populated` flag is set to `true` inside `handle_result()` when the RPM inspector's output is successfully extracted (both Ok and Degraded paths). If the RPM inspector returns `Err(Failed)`, the flag stays `false` and Wave 2 inspectors receive `None`.

- [ ] **Step 5: Write tests**

Tests in `inspectah-core/src/traits/inspector.rs` (unit tests) and `inspectah-pipeline/src/collect.rs` (integration):

1. `test_rpm_state_owned_paths_filters_etc` — only `/etc` paths in owned_paths set (not `/usr`, `/var`)
2. `test_rpm_state_is_rpm_owned_true` — known owned path returns true
3. `test_rpm_state_is_rpm_owned_false` — unknown path returns false
4. `test_rpm_state_package_for_path` — returns correct PackageEntry
5. `test_rpm_state_package_for_path_unknown` — returns None for unknown
6. `test_rpm_state_empty` — empty packages → empty owned_paths, all lookups return false/None
7. `test_is_wave2_classifier` — Wave 2 IDs return true, others false
8. `test_wave2_receives_rpm_state` — integration test confirming Wave 2 inspectors get `ctx.rpm_state == Some(...)` with populated data
9. `test_rpm_state_none_vs_empty` — `None` (RPM failed) is distinguishable from `Some(Default::default())` (RPM succeeded, no data)

- [ ] **Step 6: Verify**

```bash
cargo test --workspace
cargo clippy --workspace -- -W clippy::all
```

- [ ] **Step 7: Commit**

```bash
git add inspectah-core/src/traits/inspector.rs inspectah-pipeline/src/collect.rs
git commit -m "feat(collect): expand RpmState with capability methods and wire Wave 2 dispatch"
```

---

## Task 2: Test Fixtures for All Four Inspectors

**Files:**
- Create: all `testdata/fixtures/scheduled/*`, `testdata/fixtures/config/*`, `testdata/fixtures/selinux/*`, `testdata/fixtures/nonrpm/*` files listed below

Fixtures are adapted from Go's `cmd/inspectah/internal/inspector/testdata/` with values aligned to the test expectations we will write.

- [ ] **Step 1: Create scheduled task fixtures**

Create `testdata/fixtures/scheduled/` directory with the following files:

- `cron-d-logrotate`: `/etc/cron.d/` style file with `0 3 * * * root /usr/sbin/logrotate /etc/logrotate.conf` (RPM-owned)
- `cron-d-custom-backup`: `/etc/cron.d/` style file with `30 2 * * * root /opt/backup.sh` (user-created)
- `user-crontab`: user crontab with `*/15 * * * * /home/app/check-health.sh` and `@reboot /home/app/startup.sh`
- `crontab-system`: `/etc/crontab` with SHELL, PATH, MAILTO headers and a sample entry
- `cleanup-timer`: systemd timer unit content with `OnCalendar=daily` and `Persistent=true`
- `cleanup-service`: matching service unit with `ExecStart=/usr/local/bin/cleanup.sh`
- `at-job-sample`: at job content with preamble lines (#!/bin/sh, umask, cd) and command body

- [ ] **Step 2: Create config fixtures**

Create `testdata/fixtures/config/` directory:

- `rpm-va-output.txt`: multi-line `rpm -Va` output with entries like `S.5....T.  c /etc/httpd/conf/httpd.conf`, `..5....T.  c /etc/ssh/sshd_config`, `missing    c /etc/deleted.conf`
- `rpm-qa-file-ownership.txt`: `rpm -qa --queryformat '%{NAME}\t[%{FILENAMES}\n]'` output with `package_name\tfilepath` pairs (same format used to build both `owned_paths` and `path_to_package` in RpmState)
- `httpd.conf`: sample config file content for RPM-owned modified file
- `sshd_config`: sample sshd config content
- `custom-app.conf`: unowned config file in `/etc/custom-app/`
- `crypto-policy-current.txt`: `/etc/crypto-policies/state/current` content (e.g., `DEFAULT`)
- `dnf-history-removed.txt`: `dnf history userinstalled` style output for orphan detection

- [ ] **Step 3: Create SELinux fixtures**

Create `testdata/fixtures/selinux/` directory:

- `sestatus-enforcing.txt`: full `sestatus` output with mode=enforcing, policy=targeted
- `sestatus-permissive.txt`: full `sestatus` output with mode=permissive
- `selinux-config-enforcing.txt`: `/etc/selinux/config` with `SELINUX=enforcing`, `SELINUXTYPE=targeted`
- `semanage-boolean.txt`: `semanage boolean -l` output with columns: name, current, default, description. Include entries where current differs from default.
- `semanage-fcontext.txt`: `semanage fcontext -l -C` output with custom file context rules
- `semanage-port.txt`: `semanage port -l` output with custom port labels
- `audit-rule-custom.rules`: custom audit rule file content (not RPM-owned)
- `pam-custom-sshd`: custom PAM config for sshd (not RPM-owned)
- `fips-enabled.txt`: content `1` (FIPS enabled)
- `fips-disabled.txt`: content `0` (FIPS disabled)
- `fcontext-local.txt`: `/etc/selinux/targeted/contexts/files/file_contexts.local` content

- [ ] **Step 4: Create non-RPM software fixtures**

Create `testdata/fixtures/nonrpm/` directory:

- `readelf-sections-go.txt`: `readelf -S` output for a Go binary (contains `.note.go.buildid`, `.gopclntab`)
- `readelf-sections-rust.txt`: `readelf -S` output for a Rust binary (contains `.rustc`)
- `readelf-sections-c.txt`: `readelf -S` output for a C binary (no Go/Rust markers)
- `readelf-dynamic-linked.txt`: `readelf -d` output with NEEDED entries (dynamically linked)
- `readelf-dynamic-static.txt`: `readelf -d` output with no NEEDED entries (statically linked)
- `file-elf-output.txt`: `file` command output for an ELF binary
- `strings-version.txt`: `strings` output containing version patterns (`version=1.2.3`)
- `pyvenv.cfg`: Python venv config with `home = /usr/bin`, `version = 3.9.18`
- `pip-list-output.txt`: `pip list --format=json` output
- `requirements.txt`: pip requirements file
- `package-lock.json`: minimal npm lockfile with a few dependencies
- `gemfile.lock`: minimal Gemfile.lock
- `git-config`: `.git/config` file with remote origin
- `env-file.txt`: `.env` file with `DATABASE_URL=postgres://user:pass@host/db` (for redaction testing)

- [ ] **Step 5: Commit**

```bash
git add testdata/fixtures/scheduled/ testdata/fixtures/config/ testdata/fixtures/selinux/ testdata/fixtures/nonrpm/
git commit -m "test(fixtures): add test data for scheduled, config, selinux, and nonrpm inspectors"
```

---

## Task 3: Scheduled Tasks Inspector

**Files:**
- Create: `inspectah-collect/src/inspectors/scheduled.rs`
- Modify: `inspectah-collect/src/inspectors/mod.rs` (register module)

### Go function parity map

| Go function | Rust equivalent | Notes |
|-------------|-----------------|-------|
| `RunScheduledTasks` | `ScheduledTasksInspector::inspect()` | Orchestrates all scanning (accesses `ctx.rpm_state`) |
| `scanCronDir` | `scan_cron_dir()` | Scan `/etc/cron.d` entries |
| `scanCronFile` | `scan_cron_file()` | Parse single cron file |
| `parseCronEntries` | `parse_cron_entries()` | Extract cron jobs from file content |
| `scanCronPeriodDir` | `scan_cron_period_dir()` | Scan `cron.{hourly,daily,weekly,monthly}` |
| `CronToOnCalendar` | `cron_to_on_calendar()` | 5-field cron → systemd OnCalendar conversion |
| `normaliseCronToken` | `normalise_cron_token()` | Expand day/month names to numbers |
| `cronFieldToCalendar` | `cron_field_to_calendar()` | Convert cron field to systemd format |
| `makeTimerService` | `make_timer_service()` | Generate .timer + .service unit content |
| `scanSystemdTimers` | `scan_systemd_timers()` | Scan systemd timer units from dirs |
| `parseUnitField` | `parse_unit_field()` | Extract field from unit file content |
| `parseAtJob` | `parse_at_job()` | Parse at job file into AtJob struct |
| `isPreambleLine` | `is_preamble_line()` | Filter at job preamble (#!/bin/sh, umask, cd, etc.) |
| `scanAtJobs` | `scan_at_jobs()` | Scan `/var/spool/at` directory |
| `mustAtoi` | Standard `str::parse::<i32>()` | No separate function needed |

### Key behaviors

- Scans 6 cron locations: `/etc/crontab`, `/etc/cron.d/*`, `/var/spool/cron/*`, and 4 period dirs (`cron.{hourly,daily,weekly,monthly}`)
- Systemd timers from `/etc/systemd/system` and `/usr/lib/systemd/system`
- At jobs from `/var/spool/at`
- `rpm_state.is_rpm_owned(path)` marks cron files as `rpm_owned: true/false`
- Cron-to-systemd timer generation: `CronToOnCalendar` parses 5-field expressions, handles `@shortcuts` (`@daily`, `@hourly`, etc.), flags `@reboot` as non-convertible
- Generated timer units include both `.timer` and `.service` file content

### Degraded handling

- `PermissionDenied` on `/var/spool/cron/` or `/var/spool/at/` → Degraded
- `NotFound` on any cron dir → silent skip (cron not installed/configured)
- `NotFound` on systemd timer dirs → silent skip
- Cron parse failure on individual file → skip that file, warning, continue
- At job parse failure → skip that job, warning, continue

### Redaction surfaces

- Cron command strings: may contain embedded credentials in command arguments. Redaction engine scans `command` field.
- At job command body: may contain embedded credentials. Redaction engine scans `command` field.
- Timer unit `ExecStart`: may reference scripts with credential arguments. Redaction engine scans.

### Tests

1. `test_scan_cron_d_entries` — parse `/etc/cron.d/` file with multiple entries
2. `test_scan_cron_file_system_crontab` — parse `/etc/crontab` format
3. `test_scan_user_crontab` — parse user crontab with comments and blank lines
4. `test_scan_cron_period_dir` — scripts in `cron.daily` etc., mark rpm_owned
5. `test_cron_to_on_calendar_basic` — `0 3 * * *` → `*-*-* 03:00:00`
6. `test_cron_to_on_calendar_complex` — `*/15 1-5 * * 1-5` → correct calendar spec
7. `test_cron_to_on_calendar_shortcuts` — `@daily`, `@hourly`, `@weekly` → correct OnCalendar
8. `test_cron_to_on_calendar_reboot` — `@reboot` → returns not-convertible
9. `test_make_timer_service` — generates valid .timer + .service content
10. `test_scan_systemd_timers` — discovers timer+service pairs from dirs
11. `test_parse_at_job` — at job file parsed, preamble lines stripped
12. `test_scan_at_jobs_empty` — empty `/var/spool/at` → empty atJobs
13. `test_rpm_owned_classification` — cron file owned by RPM → `rpm_owned: true`
14. `test_scheduled_empty_system` — no cron dirs, no timers, no at → empty section
15. `test_scheduled_degraded_permission_denied` — PermissionDenied → Degraded output

- [ ] **Steps 1-7: TDD cycle**

1. Register `scheduled` module in `inspectah-collect/src/inspectors/mod.rs`
2. Create `ScheduledTasksInspector` struct implementing the unified `Inspector` trait
3. Write tests first, then implement to pass
4. In `inspect()`, match on `ctx.rpm_state`: `None` → return `Err(InspectorError::Failed { reason: "RPM prerequisite unavailable" })`. `Some(state)` → use `state.is_rpm_owned(path)` for cron file ownership classification.
5. Verify: `cargo test --workspace`
6. Clippy: `cargo clippy --workspace -- -W clippy::all`
7. Commit

Commit message: `feat(collect): implement scheduled tasks inspector with rpm ownership classification`

---

## Task 4: Config Inspector (4 Submodules)

**Files:**
- Create: `inspectah-collect/src/inspectors/config/mod.rs`
- Create: `inspectah-collect/src/inspectors/config/classify.rs`
- Create: `inspectah-collect/src/inspectors/config/walk.rs`
- Create: `inspectah-collect/src/inspectors/config/rpmva.rs`
- Modify: `inspectah-collect/src/inspectors/mod.rs` (register module)

This is the most complex inspector. Plan for ~400-500 lines across the 4 submodules.

### Go function parity map

| Go function | Rust equivalent | Location | Notes |
|-------------|-----------------|----------|-------|
| `RunConfig` | `ConfigInspector::inspect()` | `mod.rs` | Orchestrates three-category walk (accesses `ctx.rpm_state`) |
| `ClassifyConfigPath` | `classify_config_path()` | `classify.rs` | 13 categories via prefix rules |
| `categoryRules` (table) | `CATEGORY_RULES` (static) | `classify.rs` | 11 explicit rules + Other default |
| `isExcludedUnowned` | `is_excluded_unowned()` | `walk.rs` | Exact + glob exclusion |
| `matchUnownedGlob` | `match_unowned_glob()` | `walk.rs` | Segment-by-segment glob matching |
| `matchParts` | `match_parts()` | `walk.rs` | Helper for glob segment matching |
| `IsDevArtifact` | `is_dev_artifact()` | `walk.rs` | Filter `.git`, `node_modules`, etc. |
| `walkEtcRecursive` | `walk_etc_recursive()` | `walk.rs` | Recursive `/etc` traversal with VCS pruning |
| `BuildRpmOwnedPaths` | Handled by `RpmState` | N/A | Already computed in Task 1 |
| `getOwningPackage` | `rpm_state.package_for_path()` | N/A | Uses RpmState capability |
| `detectCryptoPolicy` | `detect_crypto_policy()` | `mod.rs` | Read `/etc/crypto-policies/state/current` |
| `runOstreeConfig` | `run_ostree_config()` | `mod.rs` | Branch for ostree/bootc systems |
| `unifiedDiff` | Deferred to Phase 3 | N/A | `diff_against_rpm` is Phase 3 |
| `extractOriginalFromRpm` | Deferred to Phase 3 | N/A | `dnf download + rpm2cpio` is Phase 3 |
| `downloadAndExtract` | Deferred to Phase 3 | N/A | RPM download deferred |
| `strPtr` | Standard `Some(s.to_string())` | N/A | No separate function needed |

### Three-category classification

1. **RpmOwnedModified:** Files that appear in `rpm -Va` output with modification flags. Source: `RpmState::verification_results()`, filtered to `/etc` paths. Each gets `kind: RpmOwnedModified`, `rpm_va_flags`, `package` from `rpm_state.package_for_path()`.

2. **Unowned:** Files found in `/etc` tree walk that are NOT in `rpm_state.owned_paths()` and NOT in exclusion lists (`unownedExcludeExact`, `excludedUnownedGlobs`). Each gets `kind: Unowned`.

3. **Orphaned:** Files owned by packages that have been removed. Source: `dnf history` removed packages cross-referenced with `/etc` files. Each gets `kind: Orphaned`.

### Config categories (classify.rs)

The 11 explicit category rules from Go, translated to Rust:

| Category | Prefixes |
|----------|----------|
| `Tmpfiles` | `/etc/tmpfiles.d/` |
| `Environment` | `/etc/environment`, `/etc/profile.d/` |
| `Audit` | `/etc/audit/rules.d/` |
| `LibraryPath` | `/etc/ld.so.conf.d/` |
| `Journal` | `/etc/systemd/journald.conf.d/` |
| `Logrotate` | `/etc/logrotate.d/` |
| `Automount` | `/etc/auto.master`, `/etc/auto.` |
| `Sysctl` | `/etc/sysctl.d/`, `/etc/sysctl.conf` |
| `CryptoPolicy` | `/etc/crypto-policies/` |
| `Identity` | `/etc/nsswitch.conf`, `/etc/sssd/`, `/etc/krb5.conf`, `/etc/krb5.conf.d/`, `/etc/ipa/` |
| `Limits` | `/etc/security/limits.` |

Matching rule: exact match first, then prefix match (for entries ending in `/` or `.`). Default: `Other`.

### Excluded unowned paths (walk.rs)

Translate Go's `unownedExcludeExact` map and `excludedUnownedGlobs` slice. These are system-generated files that should not appear as "unowned" config:

- Exact: `/etc/machine-id`, `/etc/hostname`, `/etc/localtime`, `/etc/adjtime`, user backup files (`/etc/passwd-`, `/etc/shadow-`, etc.), SELinux policy store files, tuned state, udisks2 configs, PAM base configs, etc.
- Glob: `/etc/selinux/*/contexts/*`, `/etc/NetworkManager/system-connections/*`, `/etc/lvm/devices/*`, etc.

**Cross-inspector ownership exclusions (no double-ownership with SELinux inspector):**

The config inspector MUST skip these paths during the `/etc` walk to avoid double-ownership with the SELinux inspector:

- `/etc/audit/rules.d/*` — owned by SELinux inspector (audit rules)
- `/etc/pam.d/*` — owned by SELinux inspector (PAM configs)

These are added as prefix exclusions in `walk.rs` alongside the existing exclusion lists. The SELinux inspector is the sole collector for audit rule content and PAM config content.

### rpm -Va flag parsing (rpmva.rs)

Parse `rpm -Va` output lines. Each line is 9 flag characters + attribute type + path:

```
S.5....T.  c /etc/httpd/conf/httpd.conf
```

Flags: S=size, M=mode, 5=md5/sha256, D=device, L=link, U=user, G=group, T=mtime, P=caps.
Attribute: c=config, d=doc, g=ghost, l=license, r=readme.

Build `RpmVaEntry` with parsed flags and path.

### Degraded handling

- `PermissionDenied` on `/etc` subdirectory during walk → Degraded, skip that subtree
- `rpm -Va` command failure → Degraded, RPM-owned modified detection unavailable
- `dnf history` failure → warning, orphan detection unavailable (not fatal)
- Individual file read failure during content capture → skip file, warning
- `NotFound` on `/etc` → empty section (extremely unlikely but handle gracefully)

### Redaction surfaces

- `ConfigFileEntry.content`: config file content may contain embedded secrets. **Every persisted content field must be scanned by the redaction engine.** This is the primary secret surface for this inspector.
- `ConfigFileEntry.diff_against_rpm`: Phase 3 only, but if populated, contains file content — must be scanned.
- `ConfigFileEntry.rpm_va_flags`: safe metadata (flag characters like `S.5....T.`), no redaction needed.

### Tests

1. `test_classify_tmpfiles` — `/etc/tmpfiles.d/foo.conf` → Tmpfiles
2. `test_classify_sysctl` — `/etc/sysctl.d/99-custom.conf` → Sysctl
3. `test_classify_identity` — `/etc/sssd/sssd.conf` → Identity
4. `test_classify_other` — `/etc/httpd/conf/httpd.conf` → Other
5. `test_classify_exact_match` — `/etc/sysctl.conf` → Sysctl (exact, not prefix)
6. `test_classify_environment` — `/etc/environment` → Environment (exact), `/etc/profile.d/foo.sh` → Environment (prefix)
7. `test_parse_rpm_va_line` — parse `S.5....T.  c /etc/httpd/conf/httpd.conf` → flags + path
8. `test_parse_rpm_va_missing` — parse `missing    c /etc/deleted.conf` → missing file
9. `test_parse_rpm_va_all_flags` — parse `SM5DLUGTP c /path` → all flags set
10. `test_is_excluded_unowned_exact` — `/etc/machine-id` → excluded
11. `test_is_excluded_unowned_glob` — `/etc/selinux/targeted/contexts/foo` → excluded
12. `test_is_excluded_unowned_not_excluded` — `/etc/httpd/conf/httpd.conf` → not excluded
13. `test_walk_etc_skips_vcs` — `.git`, `node_modules` skipped during walk
14. `test_is_dev_artifact` — `.git`, `.svn`, `node_modules`, `__pycache__` → true
15. `test_config_rpm_owned_modified` — file in rpm -Va → RpmOwnedModified with flags
16. `test_config_unowned` — file in /etc not in owned_paths → Unowned
17. `test_config_orphaned` — file from removed package → Orphaned
18. `test_config_ostree_branch` — ostree system type → ostree-specific config walk
19. `test_config_crypto_policy_detection` — crypto policy file read and warning
20. `test_config_empty_etc` — no `/etc` → empty section
21. `test_config_degraded_permission_denied` — PermissionDenied on /etc subdir → Degraded
22. `test_config_content_with_embedded_secret` — content containing `password=secret123` → redaction engine catches it
23. `test_config_deterministic_output` — files sorted by path for deterministic output
24. `test_config_rpm_va_filters_etc_only` — only `/etc` paths from rpm -Va, not `/usr` paths
25. `test_config_excluded_paths_comprehensive` — spot-check 5+ exclusion patterns from both exact and glob lists

### Additional config tests for cross-inspector boundaries

26. `test_config_skips_audit_rules_dir` — `/etc/audit/rules.d/custom.rules` excluded from config walk (owned by SELinux)
27. `test_config_skips_pam_dir` — `/etc/pam.d/custom-sshd` excluded from config walk (owned by SELinux)

- [ ] **Steps 1-8: TDD cycle**

1. Register `config` module (as directory module) in `inspectah-collect/src/inspectors/mod.rs`
2. Create the 4 submodule files
3. Start with `classify.rs` — pure logic, easy to test in isolation
4. Then `rpmva.rs` — pure parsing, no I/O
5. Then `walk.rs` — MockExecutor-based filesystem traversal. Include the audit/PAM prefix exclusions.
6. Then `mod.rs` — orchestration tying all submodules together. Implement unified `Inspector` trait; match on `ctx.rpm_state`: `None` → return `Failed`, `Some(state)` → proceed.
7. Verify: `cargo test --workspace`
8. Clippy: `cargo clippy --workspace -- -W clippy::all`

Commit message: `feat(collect): implement config inspector with classify/walk/rpmva submodules`

---

## Task 5: SELinux Inspector

**Files:**
- Create: `inspectah-collect/src/inspectors/selinux.rs`
- Modify: `inspectah-collect/src/inspectors/mod.rs` (register module)

### Go function parity map

| Go function | Rust equivalent | Notes |
|-------------|-----------------|-------|
| `RunSelinux` | `SelinuxInspector::inspect()` | Orchestrates all collectors (accesses `ctx.rpm_state`) |
| `collectSELinuxMode` | `collect_selinux_mode()` | `getenforce` or `/sys/fs/selinux/enforce` |
| `readPolicyType` | `read_policy_type()` | Parse `/etc/selinux/config` for SELINUXTYPE |
| `collectCustomModules` | `collect_custom_modules()` | Scan `/etc/selinux/{type}/active/modules/400/` |
| `parseSemanageBooleans` | `parse_semanage_booleans()` | Parse `semanage boolean -l` output |
| `readBoolsFromFS` | `read_bools_from_fs()` | Fallback: read `/sys/fs/selinux/booleans/` |
| `collectBooleanOverrides` | `collect_boolean_overrides()` | semanage first, sysfs fallback |
| `collectFcontextRules` | `collect_fcontext_rules()` | `semanage fcontext -l -C` |
| `parseSemanagePorts` | `parse_semanage_ports()` | Parse `semanage port -l` output |
| `collectPortLabels` | `collect_port_labels()` | `semanage port -l` |
| `collectAuditRules` | `collect_audit_rules()` | Scan `/etc/audit/rules.d/*`, filter by rpm_owned |
| `collectFIPSMode` | `collect_fips_mode()` | Read `/proc/sys/crypto/fips_enabled` |
| `collectPAMConfigs` | `collect_pam_configs()` | Scan `/etc/pam.d/*`, filter by rpm_owned |

### Key behaviors

- Mode detection: try `getenforce` command first, fall back to `/sys/fs/selinux/enforce` file
- Policy type: parse `/etc/selinux/config` for `SELINUXTYPE=targeted` line
- Custom modules: list directories under `/etc/selinux/{policytype}/active/modules/400/`
- Boolean overrides: parse `semanage boolean -l` with regex for name/current/default/description columns. Only include booleans where current differs from default. Fallback to sysfs `/sys/fs/selinux/booleans/` when semanage unavailable.
- Fcontext rules: `semanage fcontext -l -C` for customizations only (not full policy)
- Port labels: `semanage port -l` parsed into type/protocol/port entries
- Audit rules: scan `/etc/audit/rules.d/*.rules`, filter out RPM-owned files via `rpm_state.is_rpm_owned(path)`
- PAM configs: scan `/etc/pam.d/*`, filter out RPM-owned files via `rpm_state.is_rpm_owned(path)`
- FIPS mode: read `/proc/sys/crypto/fips_enabled`, `1` = enabled

### Degraded handling

- `semanage` not installed → Degraded for booleans/fcontext/ports, try sysfs fallback for booleans
- `getenforce` failure AND `/sys/fs/selinux/enforce` not readable → Degraded, mode unknown
- `/etc/selinux/config` not found → warning, policy type defaults to "targeted"
- `PermissionDenied` on audit rules dir → Degraded
- `PermissionDenied` on PAM dir → Degraded
- Individual file read failure → skip that file, warning

### Redaction surfaces

- Audit rule content: rules may reference sensitive paths or contain embedded directives. Redaction engine scans.
- PAM config content: PAM modules may have credential-related arguments. Redaction engine scans.
- Boolean/fcontext/port data: safe metadata (names, types, labels), no redaction needed.
- Module names: safe metadata, no redaction needed.

### Tests

1. `test_selinux_mode_enforcing` — `getenforce` returns "Enforcing" → mode set
2. `test_selinux_mode_permissive` — `getenforce` returns "Permissive" → mode set
3. `test_selinux_mode_disabled` — `getenforce` returns "Disabled" → mode set
4. `test_selinux_mode_fallback_sysfs` — `getenforce` fails, `/sys/fs/selinux/enforce` = "1" → Enforcing
5. `test_policy_type_targeted` — parse `/etc/selinux/config` → "targeted"
6. `test_custom_modules_found` — dirs under modules/400/ → module names collected
7. `test_custom_modules_empty` — no modules/400/ dir → empty list
8. `test_parse_semanage_booleans` — parse multi-line output, filter non-default only
9. `test_boolean_fallback_sysfs` — semanage fails, sysfs booleans read instead
10. `test_fcontext_rules_parsed` — `semanage fcontext -l -C` output parsed
11. `test_parse_semanage_ports` — port output parsed into type/protocol/port
12. `test_audit_rules_rpm_owned_filtered` — RPM-owned .rules files skipped
13. `test_audit_rules_custom_included` — non-RPM .rules files included
14. `test_pam_configs_rpm_owned_filtered` — RPM-owned PAM configs skipped
15. `test_pam_configs_custom_included` — non-RPM PAM configs included
16. `test_fips_mode_enabled` — `/proc/sys/crypto/fips_enabled` = "1" → fips_mode = true
17. `test_fips_mode_disabled` — `/proc/sys/crypto/fips_enabled` = "0" → fips_mode = false
18. `test_selinux_empty_system` — no SELinux → minimal section with empty fields

- [ ] **Steps 1-7: TDD cycle**

1. Register `selinux` module in `inspectah-collect/src/inspectors/mod.rs`
2. Create `SelinuxInspector` struct implementing the unified `Inspector` trait
3. Write tests first, then implement to pass
4. In `inspect()`, match on `ctx.rpm_state`: `None` → return `Err(InspectorError::Failed { reason: "RPM prerequisite unavailable" })`. `Some(state)` → use `state.is_rpm_owned(path)` for audit rules and PAM filtering — only non-RPM-owned files are collected.
5. Verify: `cargo test --workspace`
6. Clippy: `cargo clippy --workspace -- -W clippy::all`
7. Commit

Commit message: `feat(collect): implement selinux inspector with boolean fallback and rpm ownership filtering`

---

## Task 6: Non-RPM Software Inspector

**Files:**
- Create: `inspectah-collect/src/inspectors/nonrpm.rs`
- Modify: `inspectah-collect/src/inspectors/mod.rs` (register module)

### Go function parity map

| Go function | Rust equivalent | Notes |
|-------------|-----------------|-------|
| `RunNonRpmSoftware` | `NonRpmInspector::inspect()` | Orchestrates all scanning (receives but ignores `ctx.rpm_state`) |
| `probeCommand` | `probe_command()` | Check if readelf/file available |
| `classifyBinary` | `classify_binary()` | readelf -S/-d based ELF classification |
| `isBinary` | `is_binary()` | file command check for ELF |
| `stringsVersion` | `strings_version()` | 4KB head scan for version patterns |
| `classifyFile` | `classify_file()` | Classify a single file (binary or script) |
| `scanFhsDirFiles` | `scan_fhs_dir_files()` | Scan files in /opt, /srv, /usr/local subdirs |
| `scanDirs` | `scan_dirs()` | Top-level directory scanner |
| `classifyFirstBinary` | `classify_first_binary()` | Language detection from first ELF in dir |
| `walkDir` | `walk_dir()` | Recursive directory walk with visitor |
| `scanGitRepo` | `scan_git_repo()` | Detect git repos, read remote origin |
| `scanVenvPackages` | `scan_venv_packages()` | Python venv detection and package listing |
| `findVenvs` | `find_venvs()` | Locate Python venvs by pyvenv.cfg |
| `scanDistInfo` | `scan_dist_info()` | Parse .dist-info directories |
| `parseDistInfoName` | `parse_dist_info_name()` | Extract name-version from dist-info dir name |
| `tryPipList` | `try_pip_list()` | Try `pip list --format=json` in venv |
| `findSitePackagesPath` | `find_site_packages_path()` | Locate site-packages dir |
| `parsePipList` | `parse_pip_list()` | Parse pip list JSON output |
| `scanPip` | `scan_pip()` | Top-level pip/venv scanner |
| `findFiles` | `find_files()` | Find files by name recursively |
| `readLockfileDir` | `read_lockfile_dir()` | Read package-lock.json / Gemfile.lock |
| `scanNpm` | `scan_npm()` | npm/yarn lockfile detection |
| `scanGem` | `scan_gem()` | Ruby Gemfile.lock detection |
| `scanEnvFiles` | `scan_env_files()` | `.env` file detection in /opt, /srv, /usr/local |
| `findFilesMatching` | `find_files_matching()` | Find files by predicate |
| `isDevArtifactRel` | `is_dev_artifact_rel()` | Filter dev artifacts (node_modules, .git, etc.) |
| `filterOstreeVarPaths` | `filter_ostree_var_paths()` | Remove /var items on ostree systems |
| `deduplicateItems` | `deduplicate_items()` | Remove duplicate NonRpmItem entries |
| `mergeMaps` | Not needed | Rust HashMap::extend or itertools |
| `dirHasContent` | `dir_has_content()` | Check if directory has files |
| `itoa` | Standard `format!()` | No separate function needed |

### Key behaviors

- Scan directories: `/opt`, `/srv`, `/usr/local` for non-RPM software. `/home` is intentionally excluded.
- ELF binary classification via `readelf -S`:
  - Go: `.note.go.buildid` or `.gopclntab` sections
  - Rust: `.rustc` section
  - C/C++: shared library NEEDED entries (dynamically linked) vs static
- Version extraction via `strings` with 4KB head scan limit (deep scan optional)
- Version regex patterns: `version=X.Y.Z`, `vX.Y.Z`, `X.Y.Z` at word boundaries
- Language package managers: pip (requirements.txt, dist-info, venv), npm (package-lock.json), gem (Gemfile.lock)
- `.env` file detection for secrets review
- Git repo detection (`.git/config` → remote origin URL)
- Dev artifact filtering: skip `.git`, `node_modules`, `__pycache__`, `.venv`, etc.
- Ostree/bootc filtering: remove `/var` paths on ostree systems
- Deduplication: remove items with same path

### RPM dependency note

**Deliberate divergence from Wave 2 pattern:** NonRpm is Wave 2 for ordering (it runs after RPM) but does NOT call `rpm_state.is_rpm_owned()` or any other `RpmState` capability. This matches Go where `RunNonRpmSoftware` receives `NonRpmOptions` with no `RpmOwnedPaths` field. The inspector implements the unified `Inspector` trait and is partitioned to Wave 2 by `is_wave2()`, but its `inspect()` implementation ignores `ctx.rpm_state`.

### Degraded handling

- `readelf` not available → Degraded, binary classification unavailable (falls back to `file`)
- `file` not available → Degraded, binary detection unavailable
- `strings` not available → warning, version extraction skipped
- `PermissionDenied` on `/opt`, `/srv`, or `/usr/local` → Degraded
- `NotFound` on scan directories → silent skip (dir doesn't exist)
- Individual file read failure → skip that file, warning

### .env file output contract

`.env` files are persisted in the snapshot under `env_files[]` with `include: true` (visible in refine UI). They do NOT materialize under `config/` — instead, a separate `write_env_files()` function writes them under `env-files/` in the output directory. The Containerfile emits a commented-out `# COPY env-files/ /` with a FIXME noting operator review is required. This separation ensures .env files (high-probability secret carriers) are never silently included in a container image build.

### Redaction surfaces

- `.env` file content in `env_files[].content`: primary secret surface. Redaction engine scans for key=value patterns with secret-like names (DATABASE_URL, API_KEY, SECRET, PASSWORD, etc.).
- Git remote URLs: may contain embedded credentials (`https://user:token@github.com/...`). Redaction engine scans.
- `strings` 4KB head: classification-only, never persisted. Only the extracted version string is persisted, and version strings are safe metadata. No redaction needed.
- Binary paths/names: safe metadata, no redaction needed.

### Tests

1. `test_classify_binary_go` — readelf output with `.note.go.buildid` → Go
2. `test_classify_binary_rust` — readelf output with `.rustc` → Rust
3. `test_classify_binary_c_dynamic` — readelf with NEEDED entries → C/C++ dynamic
4. `test_classify_binary_c_static` — readelf with no NEEDED → C/C++ static
5. `test_classify_binary_readelf_unavailable` — no readelf → returns None
6. `test_strings_version_extraction` — `version=1.2.3` pattern matched
7. `test_strings_version_go_pattern` — `go1.21.5` pattern matched
8. `test_strings_version_no_match` — no version found → empty string
9. `test_scan_pip_venv` — pyvenv.cfg found → venv packages listed
10. `test_scan_pip_dist_info` — dist-info directories parsed
11. `test_parse_pip_list` — pip list JSON output parsed
12. `test_scan_npm_lockfile` — package-lock.json detected
13. `test_scan_gem_lockfile` — Gemfile.lock detected
14. `test_scan_git_repo` — .git/config with remote origin parsed
15. `test_scan_env_files` — .env file detected with content
16. `test_is_dev_artifact` — node_modules, .git, __pycache__ filtered
17. `test_nonrpm_empty_system` — no /opt, /srv, /usr/local → empty section
18. `test_nonrpm_degraded_no_readelf` — readelf unavailable → Degraded
19. `test_nonrpm_ostree_var_filtering` — ostree system filters /var paths
20. `test_nonrpm_deduplication` — duplicate paths removed

- [ ] **Steps 1-7: TDD cycle**

1. Register `nonrpm` module in `inspectah-collect/src/inspectors/mod.rs`
2. Create `NonRpmInspector` struct implementing the unified `Inspector` trait
3. Write tests first, then implement to pass
4. `inspect()` receives `ctx` with `ctx.rpm_state` available. Match on it: `None` → return `Err(InspectorError::Failed { reason: "RPM prerequisite unavailable" })` (consistent with other Wave 2 inspectors — if RPM failed, the ordering contract is broken). `Some(_)` → proceed, but do NOT call any `rpm_state` capability methods (match Go behavior where `RunNonRpmSoftware` does not consume `rpmOwnedPaths`).
5. Verify: `cargo test --workspace`
6. Clippy: `cargo clippy --workspace -- -W clippy::all`
7. Commit

Commit message: `feat(collect): implement nonrpm inspector with ELF classification and language package scanning`

---

## Task 7: CLI Registration

**Files:**
- Modify: `inspectah-cli/src/commands/scan.rs`

Register the 4 new inspectors in the CLI's inspector list, matching the existing pattern for Slice 2a/2b inspectors. All inspectors go into a single `Vec<Box<dyn Inspector>>` — the CLI does not know about waves (wave partition happens in `collect()`).

- [ ] **Step 1: Register inspectors**

Add `ScheduledTasksInspector`, `ConfigInspector`, `SelinuxInspector`, `NonRpmInspector` to the existing `inspectors` vec in `run_scan()`. They implement the same `Inspector` trait as all other inspectors. The `collect()` function handles wave partitioning via `is_wave2()`.

```rust
let inspectors: Vec<Box<dyn Inspector>> = vec![
    Box::new(RpmInspector::new()),
    Box::new(ServicesInspector::new()),
    Box::new(StorageInspector::new()),
    Box::new(KernelbootInspector::new()),
    Box::new(NetworkInspector::new()),
    Box::new(ContainersInspector::new()),
    Box::new(UsersGroupsInspector::new()),
    // Wave 2 (partitioned automatically by collect())
    Box::new(ScheduledTasksInspector::new()),
    Box::new(ConfigInspector::new()),
    Box::new(SelinuxInspector::new()),
    Box::new(NonRpmInspector::new()),
];
```

- [ ] **Step 2: Write tests**

1. `test_cli_creates_all_inspectors` — verify all 11 inspectors are created in the single list
2. `test_cli_wave2_ids_present` — verify the 4 Wave 2 inspector IDs are present (ScheduledTasks, Config, Selinux, NonRpmSoftware)

- [ ] **Step 3: Verify**

```bash
cargo test --workspace
```

- [ ] **Step 4: Commit**

```bash
git add inspectah-cli/src/commands/scan.rs
git commit -m "feat(cli): register scheduled, config, selinux, and nonrpm inspectors for Wave 2 dispatch"
```

---

## Task 8: Integration Tests (Inspector-on-Fixture Proof Lane)

**Files:**
- Create: `inspectah-collect/tests/scheduled_test.rs`
- Create: `inspectah-collect/tests/config_test.rs`
- Create: `inspectah-collect/tests/selinux_test.rs`
- Create: `inspectah-collect/tests/nonrpm_test.rs`

These are the inspector-on-fixture proof lane tests, matching the pattern in `inspectah-collect/tests/parity_test.rs` for Slice 2a inspectors. They run the actual Rust inspectors on fixture data via MockExecutor and verify output is structurally correct. Slice 2c tests build an `InspectionContext` with `rpm_state: Some(&mock_rpm_state)` since these are Wave 2 inspectors.

- [ ] **Step 1: Create `inspectah-collect/tests/scheduled_test.rs`**

Following the `parity_test.rs` pattern:
- Load scheduled fixtures via `include_str!`
- Build MockExecutor with cron/timer/at command and file mocks
- Build mock `RpmState` with owned paths for RPM-owned cron files
- Build `InspectionContext` with `rpm_state: Some(&mock_rpm_state)`
- Run `ScheduledTasksInspector.inspect(&ctx)` — the inspector accesses `ctx.rpm_state` internally
- Verify structural correctness (cron jobs, timers, at jobs, generated timer units)

Tests:
1. `test_scheduled_inspector_happy_path` — all sub-collectors produce data
2. `test_scheduled_inspector_cron_not_found` — no cron dirs → still succeeds with cron-only empty
3. `test_scheduled_inspector_degraded_permissions` — PermissionDenied → Degraded output
4. `test_scheduled_inspector_json_roundtrip` — output round-trips through ScheduledTaskSection type

- [ ] **Step 2: Create `inspectah-collect/tests/config_test.rs`**

1. `test_config_inspector_happy_path` — rpm -Va + /etc walk + orphan detection all produce data
2. `test_config_inspector_empty_etc` — no /etc → empty section
3. `test_config_inspector_degraded_rpm_va_failure` — rpm -Va fails → Degraded, only unowned files
4. `test_config_inspector_json_roundtrip` — output round-trips through ConfigSection type

- [ ] **Step 3: Create `inspectah-collect/tests/selinux_test.rs`**

1. `test_selinux_inspector_happy_path` — all collectors produce data
2. `test_selinux_inspector_no_selinux` — SELinux disabled/absent → minimal section
3. `test_selinux_inspector_degraded_semanage` — semanage unavailable → Degraded with sysfs fallback
4. `test_selinux_inspector_json_roundtrip` — output round-trips through SelinuxSection type

- [ ] **Step 4: Create `inspectah-collect/tests/nonrpm_test.rs`**

1. `test_nonrpm_inspector_happy_path` — ELF binaries + pip + npm + gem + env files all produce data
2. `test_nonrpm_inspector_empty_system` — no /opt, /srv, /usr/local → empty section
3. `test_nonrpm_inspector_degraded_no_readelf` — readelf unavailable → Degraded
4. `test_nonrpm_inspector_json_roundtrip` — output round-trips through NonRpmSoftwareSection type

- [ ] **Step 5: Verify**

```bash
cargo test --workspace
```

- [ ] **Step 6: Commit**

```bash
git add inspectah-collect/tests/
git commit -m "test(collect): add inspector-on-fixture integration tests for scheduled, config, selinux, nonrpm"
```

---

## Task 9: Redaction Engine Extension

**Files:**
- Modify: `inspectah-pipeline/src/redaction/engine.rs`
- Create: `inspectah-pipeline/tests/redaction_2c_surfaces_test.rs`

Extend the redaction engine to scan the new persisted surfaces introduced by the Slice 2c inspectors.

- [ ] **Step 1: Identify new persisted surfaces**

Surfaces added by Slice 2c that contain free-form text (potential secret carriers):

| Surface | Type | Source Inspector |
|---------|------|-----------------|
| `ConfigFileEntry.content` | Config file content | Config |
| `NonRpmSoftwareSection.env_files[].content` | .env file content | NonRpm |
| Cron job `command` field | Cron command string | Scheduled |
| At job `command` field | At job command body | Scheduled |
| Timer unit `ExecStart` field | Service ExecStart content | Scheduled |
| Audit rule file content | Audit rule directives | SELinux |
| PAM config content | PAM module config | SELinux |
| Git remote URL | Repository URL with potential embedded credentials | NonRpm |

Surfaces that are safe metadata (no scanning needed):
- `rpm_va_flags` (flag characters like `S.5....T.`)
- Boolean names/values
- Fcontext rules (SELinux policy, not credentials)
- Port labels (type/protocol/port tuples)
- Module names
- ELF binary paths/names
- Language detection results (Go/Rust/C)
- Version strings extracted by `strings` (the raw 4KB `strings` head is classification-only, never persisted)
- Timer `OnCalendar`/schedule fields (temporal expressions)

- [ ] **Step 2: Extend engine.rs**

Add scanning for new persisted surfaces. The existing `PATTERNS` in `patterns.rs` already cover the common secret patterns (password=, api_key=, PEM private keys, AWS keys, etc.). The engine needs to route the new surfaces through these patterns.

- [ ] **Step 3: Write planted-secret proof tests**

Create `inspectah-pipeline/tests/redaction_2c_surfaces_test.rs`:

**Detection proofs (redaction engine catches planted secrets):**
1. `test_redaction_config_content_password` — config file content with `password=secret123` → finding detected
2. `test_redaction_config_content_api_key` — config with `api_key=AKIA...` → finding detected
3. `test_redaction_env_file_database_url` — .env with `DATABASE_URL=postgres://user:pass@host/db` → finding detected
4. `test_redaction_cron_command_credential` — cron command with `--password=foo` → finding detected
5. `test_redaction_timer_execstart_credential` — timer ExecStart with `--token=secret_token_xyz` → finding detected
6. `test_redaction_at_job_credential` — at job command with `DB_PASS=secret_dbpass` → finding detected
7. `test_redaction_git_remote_credential` — Git remote URL with `https://user:token123@github.com/org/repo.git` → finding detected (embedded credential in URL)
8. `test_redaction_audit_rule_clean` — audit rule without secrets → no findings
9. `test_redaction_pam_config_clean` — PAM config without embedded passwords → no findings

**Absence proofs (raw secret substrings do NOT survive into outputs):**

Each planted-secret absence proof covers ALL persisted secret-bearing surfaces from Design Decision 7. The snapshot under test is seeded with a unique recognizable secret in every persisted field that the redaction engine scans:

| Surface | Planted secret seed | Source inspector |
|---------|-------------------|-----------------|
| Config file `content` | `password=cfg_secret_42` | Config |
| `.env` file `content` | `API_KEY=env_secret_99` | NonRpm |
| Timer `ExecStart` | `--token=timer_secret_77` | Scheduled |
| Cron job `command` | `--password=cron_secret_88` | Scheduled |
| At job `command` | `DB_PASS=atjob_secret_55` | Scheduled |
| Audit rule content | `key=audit_secret_33` (unusual but defensive) | SELinux |
| PAM config content | `password=pam_secret_11` (module arg) | SELinux |
| Git remote URL | `https://user:git_secret_66@github.com/org/repo.git` | NonRpm |

7. `test_planted_secret_absent_from_snapshot_json` — build snapshot with planted secrets in ALL surfaces above, run redaction, serialize to JSON, assert NONE of the raw secret substrings (`cfg_secret_42`, `env_secret_99`, `timer_secret_77`, `cron_secret_88`, `atjob_secret_55`, `audit_secret_33`, `pam_secret_11`, `git_secret_66`) appear in the JSON string
8. `test_planted_secret_absent_from_containerfile` — build snapshot with planted secrets in ALL surfaces above, run redaction, render Containerfile, assert NONE of the raw secret substrings appear in rendered output
9. `test_planted_secret_absent_from_config_tree` — build snapshot with planted secrets in config content AND .env content AND generated timer units AND audit rule files AND PAM config files, run redaction, render config tree to tempdir, read ALL materialized files (under `config/` and `env-files/`), assert NONE of the raw secret substrings appear in any materialized file content. Surfaces that materialize to disk: config file content → `config/`, .env file content → `env-files/`, generated timer units (ExecStart) → `config/etc/systemd/system/`, audit rule files → `config/etc/audit/rules.d/`, PAM config files → `config/etc/pam.d/`.
10. `test_planted_secret_absent_from_audit_report` — build snapshot with planted secrets in ALL surfaces above, run redaction, render audit report (audit.rs), assert NONE of the raw secret substrings appear in the rendered markdown
11. `test_planted_secret_absent_from_report_html` — build snapshot with planted secrets in ALL surfaces above, run redaction, render HTML report (report.rs), assert NONE of the raw secret substrings appear in the rendered HTML

- [ ] **Step 4: Verify**

```bash
cargo test --workspace
```

- [ ] **Step 5: Commit**

```bash
git add inspectah-pipeline/src/redaction/engine.rs inspectah-pipeline/tests/redaction_2c_surfaces_test.rs
git commit -m "feat(redaction): extend engine to scan config content, env files, cron commands, and audit rules"
```

---

## Task 10: Parity Gate Expansion (Serde/Golden Proof Lane)

**Files:**
- Create: `testdata/golden/go-v13-scheduled-tasks-section.json` (provisional)
- Create: `testdata/golden/go-v13-config-section.json` (provisional)
- Create: `testdata/golden/go-v13-selinux-section.json` (provisional)
- Create: `testdata/golden/go-v13-non-rpm-software-section.json` (provisional)
- Modify: `inspectah-core/tests/parity_gate.rs`
- Modify: `testdata/divergences.md`

This is the serde/golden proof lane ONLY. It tests that Rust types can deserialize Go-captured JSON and re-serialize without field loss. It does NOT run inspectors — that is the inspector-on-fixture lane (Task 8).

- [ ] **Step 1: Create provisional golden files**

Hand-craft minimal but representative Go-format JSON for each section, based on the Go schema types. These goldens are **not replaced** by host validation — they are the durable CI goldens covering:
- All top-level fields populated
- At least 2 entries in each array field
- All enum variants exercised where practical
- Edge cases (empty strings, null optionals) included
- Array shapes and optional field patterns that a single host may not exhibit

Host-captured sections from Task 13 are **separate evidence** — they validate real-world behavior but do NOT replace these representative goldens. The provisional goldens ensure the parity gate covers structural completeness even on hosts without specific configurations (e.g., no SELinux custom modules, no at jobs, no npm packages).

- [ ] **Step 2: Expand parity_gate.rs**

Add 4 new serde roundtrip tests following the existing pattern:

1. `test_serde_roundtrip_scheduled_tasks` — deserialize Go golden → ScheduledTaskSection → re-serialize → compare
2. `test_serde_roundtrip_config` — deserialize Go golden → ConfigSection → re-serialize → compare
3. `test_serde_roundtrip_selinux` — deserialize Go golden → SelinuxSection → re-serialize → compare
4. `test_serde_roundtrip_non_rpm_software` — deserialize Go golden → NonRpmSoftwareSection → re-serialize → compare

Each test:
- Loads the provisional golden JSON via `include_str!`
- Deserializes into the Rust type
- Re-serializes back to JSON
- Normalizes both (sort keys, strip whitespace)
- Compares, applying any divergence allowlist entries from `testdata/divergences.md`

- [ ] **Step 3: Add full-snapshot parity test**

5. `test_full_snapshot_serde_11_sections` — deserialize a complete Go snapshot JSON with all 11 sections populated, verify all sections deserialize without loss. This is the cumulative parity gate.

- [ ] **Step 4: Document divergences**

Update `testdata/divergences.md` with any new divergence entries. Expected divergences:
- NonRpm receives `rpm_state` via `ctx.rpm_state` but ignores it — no behavioral divergence, just Wave 2 scheduling
- Config `diff_against_rpm` always null in Phase 2 (deferred to Phase 3) — document as intentional Phase 2 scope boundary
- `.env` files materialize under `env-files/` (Rust) vs `config/` (Go) — deliberate divergence, .env files are high-probability secret carriers requiring separate operator review. Snapshot schema is unchanged (`env_files[]` in non_rpm_software section).

6. `test_full_snapshot_serde_all_sections_present` — verify all 11 section keys are present in roundtripped JSON

**Behavioral divergence proofs belong in the collector/pipeline test lanes (Task 8, Task 12), NOT in parity_gate.rs.** The parity gate is strictly serde/golden roundtrip. Specifically:
- "NonRpm ignores rpm_state" → tested in `inspectah-collect/tests/nonrpm_test.rs` (two runs with different RpmState, same output)
- "Config diff_against_rpm is always None" → tested in `inspectah-collect/tests/config_test.rs` (verify field is None in inspector output)

- [ ] **Step 5: Verify**

```bash
cargo test --workspace
```

- [ ] **Step 6: Commit**

```bash
git add testdata/golden/ inspectah-core/tests/parity_gate.rs testdata/divergences.md
git commit -m "test(parity): expand serde/golden roundtrip gate to scheduled, config, selinux, nonrpm sections"
```

---

## Task 11: Renderer Smoke Tests

**Files:**
- Create: `inspectah-pipeline/tests/smoke_render_2c.rs`

Verify that the existing renderers produce correct output for the 4 new sections. These are integration tests that build a snapshot with populated section data and verify the rendered output contains expected content.

**IMPORTANT:** Every assertion below has been verified against the actual renderer function names and behavior in the Rust tree. The artifact-consumer matrix above maps which renderers consume which sections.

### Containerfile renderer tests

1. `containerfile_scheduled_timer_enable` — scheduled section with generated timer units → Containerfile contains `systemctl enable` for timer
2. `containerfile_scheduled_cron_to_timer_fixme` — scheduled section with cron jobs → Containerfile contains FIXME comment about cron-to-timer conversion
3. `containerfile_scheduled_reboot_non_convertible` — cron job with @reboot → FIXME noting non-convertible
4. `containerfile_config_copy_roots` — config section with included files → Containerfile contains `COPY config/` with correct directory roots
5. `containerfile_config_crypto_policy` — config with crypto policy detected → relevant comment
6. `containerfile_selinux_custom_modules` — selinux with custom modules → COPY + semodule comment
7. `containerfile_selinux_boolean_overrides` — selinux with non-default booleans → `setsebool -P` comments
8. `containerfile_selinux_fcontext_rules` — selinux with custom fcontext → `semanage fcontext` comments
9. `containerfile_nonrpm_pip_install` — nonrpm with pip packages → requirements.txt COPY + pip install
10. `containerfile_nonrpm_npm_install` — nonrpm with npm lockfile → package-lock.json COPY + npm ci

### ConfigTree renderer tests

11. `configtree_generated_timer_units` — scheduled section with generated timers → `config/etc/systemd/system/*.timer` and `*.service` files materialized
12. `configtree_local_timer_units` — scheduled section with local timers (source="local") → timer/service files materialized
13. `configtree_config_files_materialized` — config section with included files → files materialized under `config/` tree
14. `envfiles_nonrpm_env_files` — nonrpm with env_files → `.env` files materialized under `env-files/` (NOT under `config/`), via `write_env_files()` function

### Readme renderer tests

15. `readme_scheduled_task_summary` — scheduled section with cron jobs and timers → findings summary includes task counts
16. `readme_config_file_summary` — config section with files → findings summary includes file counts by kind
17. `readme_nonrpm_binary_summary` — nonrpm with items → findings summary includes item counts

### Audit renderer tests (NEW — requires implementation in audit.rs)

**Implementation budget:** `audit.rs` currently renders rpm, config, services, storage, kernel_boot sections. It does NOT render scheduled_tasks, selinux, or non_rpm_software. This task adds ~30-50 lines per section to `render_audit()`, following the existing pattern (check `if let Some(section) = &snap.field`, emit markdown headings and lists).

**What needs to be added to `audit.rs`:**

```rust
// Scheduled tasks (after services section, ~line 210)
if let Some(sched) = &snap.scheduled_tasks {
    // ## Scheduled Tasks heading
    // Cron job count, timer count, at job count
    // @reboot entries flagged as warnings
}

// SELinux (after kernel_boot section, ~line 350)
if let Some(sel) = &snap.selinux {
    // ## SELinux heading
    // Mode, custom module count, non-default boolean list
    // Custom fcontext count, FIPS status
}

// Non-RPM Software (after SELinux)
if let Some(nrs) = &snap.non_rpm_software {
    // ## Non-RPM Software heading
    // Item count by type, .env file count with warning
}
```

18. `audit_scheduled_section` — audit.rs renders `## Scheduled Tasks` with cron job count, timer count, at job count
19. `audit_config_section` — audit.rs renders `## Configuration Files` with modified/unowned counts (EXISTING — verify)
20. `audit_selinux_section` — audit.rs renders `## SELinux` with mode, custom module count, non-default boolean count, FIPS status
21. `audit_nonrpm_section` — audit.rs renders `## Non-RPM Software` with item count, `.env` file warning

### Report renderer tests (NEW — requires implementation in report.rs)

**Implementation budget:** `report.rs` renders summary cards for packages, config, services, storage, kernel/boot, warnings. This task adds summary cards for scheduled tasks, SELinux, and non-RPM software (~15 lines total in the summary grid, plus counting logic).

**What needs to be added to `report.rs`:**

```rust
// Add count variables (~line 67)
let scheduled_count = snap.scheduled_tasks.as_ref()
    .map(|s| s.cron_jobs.len() + s.timers.len() + s.at_jobs.len())
    .unwrap_or(0);
let selinux_mode = snap.selinux.as_ref()
    .map(|s| s.mode.clone())
    .unwrap_or_default();
let nonrpm_count = snap.non_rpm_software.as_ref()
    .map(|n| n.items.len())
    .unwrap_or(0);

// Add summary cards in the HTML template
// <div class="summary-card"><h3>Scheduled Tasks</h3><div class="value">{scheduled_count}</div></div>
// <div class="summary-card"><h3>SELinux</h3><div class="value">{selinux_mode}</div></div>
// <div class="summary-card"><h3>Non-RPM Items</h3><div class="value">{nonrpm_count}</div></div>
```

22. `report_scheduled_section` — report.rs includes `Scheduled Tasks` summary card with count
23. `report_config_section` — report.rs includes `Config Files` summary card (EXISTING — verify)
24. `report_selinux_section` — report.rs includes `SELinux` summary card with mode
25. `report_nonrpm_section` — report.rs includes `Non-RPM Items` summary card with count

### .env file output contract tests

26. `envfiles_written_to_separate_dir` — nonrpm with `env_files` (include: true) → files appear under `env-files/` output path, NOT under `config/`
27. `envfiles_not_in_config_tree` — nonrpm with `env_files` → `write_config_tree()` does NOT produce any `.env` files under `config/`
28. `containerfile_env_files_commented_copy` — nonrpm with env_files → Containerfile contains `# COPY env-files/ /` (commented out) + FIXME noting operator review needed
29. `containerfile_env_files_no_active_copy` — nonrpm with env_files → Containerfile does NOT contain an uncommented `COPY env-files/` line

### Cross-cutting tests

30. `containerfile_empty_2c_sections` — empty scheduled/config/selinux/nonrpm → no crash, no section output
31. `containerfile_degraded_2c_sections` — degraded completeness → FIXME comments in Containerfile (verified: lines 98, 105, 112, 143 of containerfile.rs emit FIXME for degraded sections)
32. `audit_empty_2c_sections` — empty scheduled/selinux/nonrpm → no crash, no `## Scheduled Tasks` / `## SELinux` / `## Non-RPM Software` headings

### Negative contract tests (things that MUST NOT happen)

33. `configtree_vendor_timers_not_copied` — vendor timer units from `/usr/lib/systemd/system/` MUST NOT appear in config tree output
34. `configtree_cron_spool_not_materialized` — cron spool from `/var/spool/cron/` MUST NOT be materialized into config tree (advisory, not declarative)
35. `configtree_audit_rules_not_in_config` — audit rule files are owned by SELinux inspector, not config — verify config section does not include `/etc/audit/rules.d/` files
36. `configtree_pam_not_in_config` — PAM config files are owned by SELinux inspector, not config — verify config section does not include `/etc/pam.d/` files

Each test:
- Builds an `InspectionSnapshot` with the target section populated
- Calls the renderer
- Asserts output contains expected strings matching the ACTUAL renderer contract (per the artifact-consumer matrix above and verified function names from `git show rust:`)
- Asserts no panics on empty/None sections
- Negative tests assert specific strings/paths are ABSENT from output

- [ ] **Step 2: Implement renderer changes**

**2a: configtree.rs — move .env files to `env-files/`:**
- Remove the `.env` file materialization block from `write_config_tree()` (lines 425-441 that iterate `nrs.env_files` and write under `config/`)
- Add a new `write_env_files()` public function that writes `.env` files under `env-files/` in the output directory. Same `include` filter and `validate_path()` safety checks as the old code.
- Update the existing `test_config_tree_nonrpm_env_files` test to verify `.env` files appear under `env-files/` and NOT under `config/`

**2b: containerfile.rs — commented .env COPY:**
- In `non_rpm_section_lines()`, when `.env` files are present, emit:
  ```
  # FIXME: .env files detected — review before including in container image
  # These files likely contain secrets that should use container secrets management instead
  # COPY env-files/ /
  ```
- Do NOT emit an uncommented `COPY env-files/` line

**2c: audit.rs and report.rs additions:**
Add the scheduled_tasks, selinux, and non_rpm_software sections to `audit.rs` and the summary cards to `report.rs`. Follow the existing patterns in each file. Budget: ~80-100 lines in audit.rs, ~15-20 lines in report.rs.

**Files modified:**
- `inspectah-pipeline/src/render/configtree.rs` — remove .env from `write_config_tree()`, add `write_env_files()`
- `inspectah-pipeline/src/render/containerfile.rs` — add commented .env COPY in `non_rpm_section_lines()`
- `inspectah-pipeline/src/render/audit.rs` — add 3 new section blocks after existing sections
- `inspectah-pipeline/src/render/report.rs` — add 3 summary card variables + HTML template entries

- [ ] **Step 3: Verify**

```bash
cargo test --workspace
```

- [ ] **Step 4: Commit**

```bash
git add inspectah-pipeline/src/render/audit.rs inspectah-pipeline/src/render/report.rs inspectah-pipeline/src/render/configtree.rs inspectah-pipeline/src/render/containerfile.rs inspectah-pipeline/tests/smoke_render_2c.rs
git commit -m "feat(render): move env-files to separate output path, add audit/report rendering for 2c sections"
```

---

## Task 12: Failure Policy Tests

**Files:**
- Modify: `inspectah-pipeline/tests/failure_policy.rs`

Extend the failure policy test suite to cover the 4 new inspectors' degraded behavior.

- [ ] **Step 1: Add failure policy tests**

1. `test_scheduled_permission_denied_degraded` — PermissionDenied on cron spool → completeness.status = Degraded
2. `test_scheduled_not_found_silent` — cron dirs NotFound → no error, empty section
3. `test_config_rpm_va_failure_degraded` — rpm -Va fails → Degraded
4. `test_config_etc_permission_denied_degraded` — PermissionDenied on /etc subdir → Degraded
5. `test_selinux_semanage_unavailable_degraded` — semanage not found → Degraded with sysfs fallback
6. `test_selinux_audit_permission_denied_degraded` — PermissionDenied on audit rules → Degraded
7. `test_nonrpm_readelf_unavailable_degraded` — readelf not found → Degraded
8. `test_nonrpm_scan_dir_not_found_silent` — /opt not found → no error, no items
9. `test_wave2_rpm_unavailable_fails_all_dependents` — `ctx.rpm_state = None` (RPM inspector failed) → all 4 Wave 2 inspectors (Scheduled, Config, Selinux, NonRpm) return `Err(InspectorError::Failed { reason: "RPM prerequisite unavailable" })`. This proves the `None` vs `Some(empty)` distinction: RPM failure is fatal, not degraded.

- [ ] **Step 2: Verify**

```bash
cargo test --workspace
```

- [ ] **Step 3: Commit**

```bash
git add inspectah-pipeline/tests/failure_policy.rs
git commit -m "test(policy): extend failure policy tests for scheduled, config, selinux, nonrpm degraded behaviors"
```

---

## Task 13: Host Validation & Golden File Finalization

**Files:**
- Create: `testdata/evidence/slice-2c-host-validation.md`
- Modify: `scripts/host-validation.sh`

This task produces the durable evidence artifact. It is NOT a template — it is filled with real data from a real host.

**Source of truth for the evidence flow:** `scripts/host-validation.sh` — this script handles building, scanning, diffing, and evidence collection. The steps below describe what the script does and how to extend it, not a separate manual process.

- [ ] **Step 1: Build and run on CentOS Stream 9 host**

On the CentOS Stream 9 target host (NOT on Darwin/macOS):

```bash
# Install Rust toolchain on CentOS (system packages, NOT Darwin rustup)
sudo dnf install -y rust cargo gcc jq

# Build Rust binary from source (or scp a pre-built binary)
cd /path/to/inspectah
git checkout rust
cargo build --release -p inspectah-cli
# Binary at: target/release/inspectah-cli

# Run the host validation script (builds from source if no binary provided)
sudo ./scripts/host-validation.sh ./target/release/inspectah-cli inspectah
```

The script handles:
1. Running Go scan (`inspectah scan --output ...`)
2. Running Rust scan (`./inspectah-cli scan --output ...`)
3. Extracting per-section JSON from both outputs
4. Section-level normalized diff
5. Collecting host info for evidence
6. Creating evidence tarball
7. Copying evidence to `testdata/evidence/`

**Alternative: scp pre-built binary to host**
```bash
# On build machine (cross-compile or native build)
cargo build --release -p inspectah-cli --target x86_64-unknown-linux-gnu
scp target/x86_64-unknown-linux-gnu/release/inspectah-cli host:/tmp/inspectah-rust

# On CentOS host
sudo /path/to/host-validation.sh /tmp/inspectah-rust inspectah
```

- [ ] **Step 2: Compare host-captured sections (SEPARATE from CI goldens)**

The host validation script produces per-section diffs. Review each diff for:
- Expected divergences (documented in `testdata/divergences.md`)
- Unexpected divergences (require investigation and resolution or allowlist entry)

Host-captured section JSON files are evidence artifacts — they do NOT replace the hand-crafted provisional goldens in `testdata/golden/`. The provisionals cover structural completeness (all field shapes, all enum variants). Host evidence validates real-world behavior.

If a host-captured section reveals a schema gap in the provisional golden (missing field, wrong type), fix the provisional golden to cover it — but keep the provisional as the CI golden.

- [ ] **Step 3: Golden promotion safety gate**

Before committing any host-captured JSON as supplementary evidence:
1. Review for real secrets from the validation host (passwords in `/etc` configs, API keys in `.env` files)
2. If real secrets are present: sanitize the specific fields or use a clean validation host
3. Host validation uses a private temp directory (`/tmp/inspectah-host-validation-*`) — the script cleans up, but verify no secrets persist in the repo tree

- [ ] **Step 4: Update host-validation.sh**

Extend `scripts/host-validation.sh` to cover all 11 sections (currently covers 7 from Slices 2a+2b). Add the 4 new sections: `scheduled_tasks`, `config`, `selinux`, `non_rpm_software` to the section extraction and diff loops.

- [ ] **Step 5: Run parity gate with real goldens**

```bash
cargo test --workspace
```

All parity gate tests should pass with the provisional CI goldens. Host evidence is supplementary validation.

- [ ] **Step 6: Write evidence document**

Create `testdata/evidence/slice-2c-host-validation.md`:

```markdown
# Slice 2c Host Validation Evidence

**Date:** [actual date]
**Scope:** Closes Slice 2a, 2b, AND 2c host validation evidence

## Host Details
- **OS:** [actual, e.g., CentOS Stream 9]
- **Kernel:** [actual]
- **Architecture:** [actual, e.g., x86_64]
- **Go inspectah version:** [actual]
- **Rust inspectah version:** [actual from Cargo.toml]
- **Rust toolchain:** system Rust via `dnf install rust cargo gcc`

## Sections Validated (all 11)

### Slice 2a sections
- [x] rpm — [match / divergences noted]
- [x] services — [match / divergences noted]
- [x] storage — [match / divergences noted]
- [x] kernel_boot — [match / divergences noted]

### Slice 2b sections
- [x] network — [match / divergences noted]
- [x] containers — [match / divergences noted]
- [x] users_groups — [match / divergences noted]

### Slice 2c sections
- [x] scheduled_tasks — [match / divergences noted]
- [x] config — [match / divergences noted]
- [x] selinux — [match / divergences noted]
- [x] non_rpm_software — [match / divergences noted]

## Trust-Bearing Fields (Rust-only)
- **redaction_state:** [populated / structure described]
- **completeness:** [populated / structure described]
- Note: These fields are stripped by normalize.rs for parity comparison
  but must be correct in the raw Rust output.

## Golden File Status
- CI goldens: provisional (hand-crafted, structurally complete)
- Host evidence: supplementary (real-world validation, not CI replacements)
- Any live-host divergence that is NOT in divergences.md fails the slice

## Test Results
- Total tests: [actual count]
- Parity gate (serde/golden): [pass/fail]
- Inspector-on-fixture: [pass/fail]
- Clippy: [clean/warnings]

## Secret Safety
- [x] Host evidence files reviewed for real secrets
- [x] No raw credentials in committed evidence
- [x] Temp directories cleaned up

## Divergences
[List any divergences found, with references to testdata/divergences.md entries]
```

- [ ] **Step 7: Commit**

```bash
git add testdata/evidence/slice-2c-host-validation.md scripts/host-validation.sh
git commit -m "evidence(slice-2c): host validation on CentOS Stream 9 covering all 11 sections"
```

---

## Task 14: Final Verification

- [ ] **Step 1: Full test suite**

```bash
cargo test --workspace 2>&1 | grep 'test result'
```

Record total test count. Target: Slice 2b baseline (~478) + Slice 2c additions (~170) = ~648+.

- [ ] **Step 2: Clippy clean**

```bash
cargo clippy --workspace -- -W clippy::all
```

Expected: Zero warnings.

- [ ] **Step 3: Format check**

```bash
cargo fmt --all -- --check
```

Expected: No issues.

- [ ] **Step 4: Verify slice checklist**

- [ ] Scheduled, config, selinux, nonrpm inspectors implemented with unified `Inspector` trait (NO separate `RpmDependentInspector` trait)
- [ ] All inspectors declare `applicable_to() -> &[PackageBased]`
- [ ] All four access `rpm_state` via `ctx.rpm_state` in Wave 2 context
- [ ] RpmState expanded with packages, verification_results, module_streams, owned_paths + capability methods
- [ ] RpmState population traceable: `rpm -qa --queryformat` → RPM inspector output → `handle_result()` extraction → `owned_paths` HashSet + `path_to_package` reverse index
- [ ] RPM failure policy: `ctx.rpm_state = None` → all Wave 2 inspectors return `Failed` (not Degraded). `Some(empty)` → proceed with empty lookups.
- [ ] Wave 2 classifier (`is_wave2`) correctly identifies all 4 RPM-dependent inspectors
- [ ] Config inspector decomposed into 4 submodules (mod/classify/walk/rpmva)
- [ ] Config inspector skips `/etc/audit/rules.d/` and `/etc/pam.d/` (owned by SELinux inspector, no double-ownership)
- [ ] NonRpm placed in Wave 2 but does not use RpmState capabilities (matches Go)
- [ ] Section parity gate passing for all 11 sections with representative CI goldens
- [ ] Inspector-on-fixture tests passing for all 4 new inspectors
- [ ] Renderer smoke tests passing for all 4 sections against all 7 artifact consumers (containerfile, configtree, env-files, kickstart, readme, audit, report)
- [ ] Basic audit.rs rendering added for scheduled_tasks, selinux, non_rpm_software sections
- [ ] Basic report.rs summary cards added for scheduled tasks, SELinux mode, non-RPM item count
- [ ] `.env` files materialize under `env-files/` (NOT `config/`), configtree.rs does not write .env files
- [ ] Containerfile emits commented-out `# COPY env-files/ /` with FIXME for .env files (no active COPY)
- [ ] Negative contract tests passing (no vendor timers in config tree, no cron spool materialization, no audit/PAM double-ownership, no .env files in config tree)
- [ ] Failure policy tested (Degraded for permissions/missing tools, silent skip for NotFound)
- [ ] Redaction engine scanning all persisted content surfaces (config content, env files, cron commands, at job commands, timer ExecStart, audit rules, PAM configs, Git remote URLs)
- [ ] Planted-secret-absent proofs passing — every persisted secret-bearing surface seeded with a unique planted secret, all raw secret substrings absent from snapshot JSON, Containerfile, config tree, env-files, audit report, and HTML report
- [ ] Host validation evidence committed with real data (supplementary to CI goldens, not replacement)
- [ ] CI golden files cover all field shapes, enum variants, and edge cases (representative, not single-host)
- [ ] Golden promotion reviewed for real secrets — no raw credentials in committed evidence
- [ ] All divergence allowlist entries have review-approval annotations
- [ ] All commits follow conventional commit format

- [ ] **Step 5: Review commit history**

```bash
git log --oneline
```

Verify focused, well-described commits following conventional format. Expected ~12-14 commits for this slice.

---

## Phase 2 Exit Gate

Slice 2c delivers **full inspector parity**. After this slice:

- All 11 Go inspectors have Rust equivalents
- Three-wave parallel execution working end-to-end
- Parity gate covers all 11 sections
- 648+ tests providing comprehensive coverage
- All 7 output consumers (containerfile, configtree, env-files, kickstart, readme, audit, report) consuming all 11 sections
- Redaction engine scanning all persisted surfaces

**Phase 2 is complete.** Phase 3 (CLI expansion, fleet comparison, diff enrichment) is a separate planning cycle.
