# Non-RPM Replication Plan 2: Tier 2 (Unmanaged Files) + Tier 3 (Repo-less RPMs)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add opt-in unmanaged file cataloging/bundling (Tier 2) and automatic repo-less RPM bundling (Tier 3) to the scan and refine pipeline. Both tiers produce executable Containerfile output backed by collected artifacts in the export tarball.

**Architecture:** Five layers change: (1) CLI gains `--include-unmanaged`, `--exclude-path`, `-y`/`--yes` flags, (2) core types gain `ItemId::UnmanagedFile` and unmanaged file data model with derived provenance signals, (3) the non-RPM inspector catalogs unmanaged files (excluding RPM-owned paths and Tier 1 language environments) and the RPM inspector scans the dnf cache for repo-less packages (including packages whose repo is no longer configured), (4) the pipeline renderer emits `COPY`/`RUN` directives for both tiers with appropriate warning blocks, (5) refine export extracts payload files from the source tarball and materializes `unmanaged/` and `repoless-packages/` roots, with an upload endpoint for user-provided RPMs.

**Scope boundary:** This plan covers backend plumbing only — collector, CLI flags, renderer, export contract, refine classification, RPM upload API endpoint. The refine UI decision surface (Unmanaged Files section, Repo-less RPM toggles, upload modal, grouped interaction, size rollup) is Plan 3. Aggregate-mode reviewability (aggregate identity, prevalence, variant handling) is Plan 4. Both plans consume the `ItemId::UnmanagedFile` and export contracts established here.

**Tech Stack:** Rust (2024 edition), clap (CLI args), serde, insta (snapshot testing), sha2/hex (path hashing), axum (upload endpoint), inspectah-core types, inspectah-refine, inspectah-pipeline, inspectah-collect, inspectah-cli, inspectah-refine-web.

**Spec:** `process-docs/specs/proposed/2026-06-27-non-rpm-replication.md` — read fresh before implementation. This plan covers Tier 2 and Tier 3 backend. Plan 1 covers Tier 1 and shared contracts. Plan 3 covers Tier 2/3 UI.

**Thorn Checkpoints:** After Tasks 3, 7, 12.

## Global Constraints

- Clippy clean: `cargo clippy -- -W clippy::all` with zero warnings.
- Format: `cargo fmt --check` must pass.
- **Verification commands:** Every task's "Run tests" step must also run `cargo clippy -- -W clippy::all` and `cargo fmt --check` in addition to `cargo test`. These are hard gates, not aspirational.
- No team member names in code or commits.
- Commit format: `type(scope): description`. Attribution: `Assisted-by: Claude Code (Opus 4.6)`.
- All new `#[serde]` fields use `#[serde(default)]` for backward-compatible deserialization.
- Schema version is bumped in Plan 1 (19 → 20). No additional bump in this plan — Plan 1's bump covers all new fields across Plans 1-4.
- Existing tests must keep passing throughout. Run `cargo test` after each task.
- This plan consumes shared contracts from Plan 1 exactly: `ItemId` variants, export allowlist pattern, method strings, confidence rendering gate.

## File Map

### Modified files

| File | Change |
|------|--------|
| `crates/cli/src/main.rs` | Add `-y`/`--yes` global flag to `Cli` struct |
| `crates/cli/src/commands/scan.rs` | Add `--include-unmanaged` and `--exclude-path` flags to `ScanArgs`; pass config to pipeline; prompt before bundling; bundle files into scan tarball |
| `crates/core/src/types/nonrpm.rs` | Add `UnmanagedFile` struct, `UnmanagedFileSection` struct, `FileType` enum, `ProvenanceSignals` struct (with derived signals) |
| `crates/core/src/types/rpm.rs` | Add `repoless_annotation`, `repoless_cached`, `cache_path` fields to `PackageEntry` |
| `crates/core/src/snapshot.rs` | Add `unmanaged_files: Option<UnmanagedFileSection>` field to `InspectionSnapshot` |
| `crates/refine/src/types.rs` | Add `ItemId::UnmanagedFile` variant |
| `crates/collect/src/inspectors/nonrpm.rs` | Add `scan_unmanaged_files()` function with RPM-ownership exclusion and derived provenance signals |
| `crates/pipeline/src/render/containerfile.rs` | Add calls to new unmanaged and repoless renderers in `render_containerfile_inner()` |
| `crates/pipeline/src/render/mod.rs` | Add `pub mod unmanaged;` and `pub mod repoless;` declarations |
| `crates/refine/src/session.rs` | Add `unmanaged`, `repoless-packages` to export allowlist; add source-tarball extraction for payload files; add `upload_dir` to `RefineSession` |
| `crates/refine/src/classify.rs` | Add classification for repo-less RPMs (pre-excluded) and unmanaged files |
| `crates/refine-web/src/lib.rs` | Add `/api/upload-rpm` route |
| `docs/reference/output-artifacts.md` | Document `unmanaged/` and `repoless-packages/` roots |

### New files

| File | Responsibility |
|------|---------------|
| `crates/pipeline/src/render/unmanaged.rs` | Containerfile rendering for unmanaged file COPY directives |
| `crates/pipeline/src/render/repoless.rs` | Containerfile rendering for repo-less RPM `dnf localinstall` directives |
| `crates/collect/tests/unmanaged_scan_test.rs` | Integration tests for unmanaged file cataloging |
| `crates/collect/tests/repoless_rpm_test.rs` | Integration tests for dnf cache scanning and repo-disabled detection |
| `crates/refine-web/src/upload.rs` | RPM upload endpoint handler |
| `crates/refine/tests/export_parity_test.rs` | Preview/export parity tests for unmanaged and repoless COPY paths |

---

### Task 1: Data Model — UnmanagedFile Types + ItemId

**Files:**
- Modify: `crates/core/src/types/nonrpm.rs`
- Modify: `crates/core/src/snapshot.rs`
- Modify: `crates/refine/src/types.rs`
- Test: existing roundtrip tests, new tests in `nonrpm.rs`

**Interfaces:**
- Produces: `UnmanagedFile`, `UnmanagedFileSection`, `FileType`, `ProvenanceSignals`, `ItemId::UnmanagedFile`
- Consumed by: Tasks 2-12

- [ ] **Step 1: Add FileType enum**

In `crates/core/src/types/nonrpm.rs`, add after the existing structs:

```rust
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileType {
    #[default]
    Other,
    ElfBinary,
    Jar,
    Script,
    DataFile,
    Config,
    Symlink,
}
```

- [ ] **Step 2: Add ProvenanceSignals struct**

This struct carries both raw metadata and the three derived signals
the spec requires (mutability, writable mount, service working directory).

```rust
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProvenanceSignals {
    #[serde(default)]
    pub file_type: FileType,
    /// Last-modified timestamp (seconds since epoch)
    #[serde(default)]
    pub last_modified: u64,
    /// Filesystem UID
    #[serde(default)]
    pub uid: u32,
    /// Filesystem GID
    #[serde(default)]
    pub gid: u32,
    /// Octal file permissions (e.g., "0755")
    #[serde(default)]
    pub permissions: String,

    // --- Derived signals (spec-required) ---

    /// True when the file's mtime is newer than the system install date.
    /// System install date is derived from `/etc/machine-id` ctime or
    /// the install time of a baseos RPM (e.g., `filesystem` package).
    /// Newer files are likely runtime-generated data, not deployed payload.
    #[serde(default)]
    pub mutable: bool,
    /// True when the file lives on a read-write mount point.
    /// Determined by parsing `/proc/mounts` and matching the file's
    /// path to the longest-prefix mount that has the `rw` option.
    #[serde(default)]
    pub writable_mount: bool,
    /// True when the file path is under any systemd service's
    /// `WorkingDirectory=` (parsed from `/etc/systemd/system/*.service`
    /// and `/usr/lib/systemd/system/*.service` unit files).
    #[serde(default)]
    pub service_working_dir: bool,
}
```

- [ ] **Step 3: Add UnmanagedFile struct**

```rust
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UnmanagedFile {
    /// Absolute path on the source host
    #[serde(default)]
    pub path: String,
    /// File size in bytes
    #[serde(default)]
    pub size: u64,
    /// Detected file type
    #[serde(default)]
    pub file_type: FileType,
    /// Provenance signals for review (raw metadata + derived signals)
    #[serde(default)]
    pub provenance: ProvenanceSignals,
    /// Include in export (default true — user toggles in refine)
    #[serde(default = "crate::default_true")]
    pub include: bool,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub acknowledged: bool,
    /// True if path is under /var (needs bootc persistence warning).
    /// Note: /var is NOT a scan root — this flag is only set when an
    /// unmanaged file under /opt, /srv, or /usr/local has a symlink
    /// target or runtime-generated path that resolves under /var.
    /// The /var WARNING in the spec is advisory guidance for the refine
    /// UI, not a scan-scope directive.
    #[serde(default)]
    pub under_var: bool,
    /// Aggregate prevalence (populated in aggregate mode)
    pub aggregate: Option<AggregatePrevalence>,
}
```

- [ ] **Step 4: Add UnmanagedFileSection struct**

```rust
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UnmanagedFileSection {
    #[serde(default)]
    pub items: Vec<UnmanagedFile>,
    /// Total size of all cataloged files in bytes
    #[serde(default)]
    pub total_size: u64,
    /// Total number of cataloged files
    #[serde(default)]
    pub total_count: usize,
}
```

- [ ] **Step 5: Add unmanaged_files field to InspectionSnapshot**

In `crates/core/src/snapshot.rs`, add after the `non_rpm_software` field:

```rust
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unmanaged_files: Option<UnmanagedFileSection>,
```

Add the import at the top of `snapshot.rs`:

```rust
use crate::types::nonrpm::UnmanagedFileSection;
```

- [ ] **Step 6: Add ItemId::UnmanagedFile variant**

In `crates/refine/src/types.rs`, add to the `ItemId` enum:

```rust
    // Unmanaged files section
    UnmanagedFile {
        path: String,
    },
```

- [ ] **Step 7: Write roundtrip tests**

In the `#[cfg(test)]` module of `crates/core/src/types/nonrpm.rs`, add:

```rust
#[test]
fn test_unmanaged_file_roundtrip() {
    let file = UnmanagedFile {
        path: "/opt/splunk/bin/splunkd".into(),
        size: 52428800,
        file_type: FileType::ElfBinary,
        provenance: ProvenanceSignals {
            file_type: FileType::ElfBinary,
            last_modified: 1700000000,
            uid: 0,
            gid: 0,
            permissions: "0755".into(),
            mutable: false,
            writable_mount: false,
            service_working_dir: false,
        },
        include: true,
        under_var: false,
        ..Default::default()
    };
    let json = serde_json::to_string(&file).unwrap();
    let deser: UnmanagedFile = serde_json::from_str(&json).unwrap();
    assert_eq!(file, deser);
}

#[test]
fn test_provenance_signals_derived_fields_roundtrip() {
    let signals = ProvenanceSignals {
        file_type: FileType::DataFile,
        last_modified: 1700000000,
        uid: 1000,
        gid: 1000,
        permissions: "0644".into(),
        mutable: true,
        writable_mount: true,
        service_working_dir: true,
    };
    let json = serde_json::to_string(&signals).unwrap();
    let deser: ProvenanceSignals = serde_json::from_str(&json).unwrap();
    assert_eq!(signals, deser);
}

#[test]
fn test_unmanaged_file_section_roundtrip() {
    let section = UnmanagedFileSection {
        items: vec![UnmanagedFile {
            path: "/opt/app/server".into(),
            size: 1024,
            file_type: FileType::ElfBinary,
            ..Default::default()
        }],
        total_size: 1024,
        total_count: 1,
    };
    let json = serde_json::to_string(&section).unwrap();
    let deser: UnmanagedFileSection = serde_json::from_str(&json).unwrap();
    assert_eq!(section, deser);
}

#[test]
fn test_unmanaged_file_defaults_from_empty_json() {
    let deser: UnmanagedFile = serde_json::from_str("{}").unwrap();
    assert_eq!(deser.path, "");
    assert_eq!(deser.size, 0);
    assert_eq!(deser.file_type, FileType::Other);
    assert!(deser.include); // default_true
    assert!(!deser.under_var);
    assert!(!deser.provenance.mutable);
    assert!(!deser.provenance.writable_mount);
    assert!(!deser.provenance.service_working_dir);
}
```

- [ ] **Step 8: Run tests + lint**

Run:
```
cargo test -p inspectah-core -p inspectah-refine
cargo clippy -- -W clippy::all
cargo fmt --check
```
Expected: all pass, zero clippy warnings, format clean.

- [ ] **Step 9: Commit**

```
feat(core): add unmanaged file data model with derived provenance signals

Add UnmanagedFile, UnmanagedFileSection, FileType, ProvenanceSignals
types with spec-required derived signals (mutable, writable_mount,
service_working_dir). Add unmanaged_files field to InspectionSnapshot.
Add ItemId::UnmanagedFile variant for refine toggle operations.
All new fields use serde(default) for backward compatibility.
```

---

### Task 2: CLI Flags — --include-unmanaged, --exclude-path, -y/--yes

**Files:**
- Modify: `crates/cli/src/main.rs` (`Cli` struct)
- Modify: `crates/cli/src/commands/scan.rs` (`ScanArgs` struct)

**Interfaces:**
- Produces: `Cli.yes`, `ScanArgs.include_unmanaged`, `ScanArgs.exclude_path`
- Consumed by: Tasks 3, 4

- [ ] **Step 1: Add -y/--yes global flag**

In `crates/cli/src/main.rs`, add to the `Cli` struct:

```rust
    /// Assume yes to all interactive prompts (for CI/automation)
    #[arg(short = 'y', long = "yes", global = true)]
    pub yes: bool,
```

- [ ] **Step 2: Add --include-unmanaged and --exclude-path to ScanArgs**

In `crates/cli/src/commands/scan.rs`, add to the `ScanArgs` struct:

```rust
    /// Catalog and bundle unmanaged files from /opt, /srv, /usr/local.
    /// Prompts with total size before bundling (suppressed by -y/--yes).
    #[arg(long)]
    pub include_unmanaged: bool,

    /// Exclude specific paths from unmanaged file collection (repeatable)
    #[arg(long = "exclude-path", value_name = "PATH")]
    pub exclude_path: Vec<String>,
```

- [ ] **Step 3: Thread yes flag to scan command**

In `crates/cli/src/main.rs`, at the `Commands::Scan(args)` match arm,
pass `cli.yes` to the scan runner:

```rust
Commands::Scan(args) => commands::scan::run(args, cli.yes),
```

Update the `run` function signature in `scan.rs`:

```rust
pub fn run(args: ScanArgs, assume_yes: bool) -> Result<()> {
```

- [ ] **Step 4: Run tests + lint**

Run:
```
cargo test -p inspectah-cli
cargo clippy -- -W clippy::all
cargo fmt --check
```
Expected: all pass. Verify `inspectah scan --help` shows the new flags.

- [ ] **Step 5: Commit**

```
feat(cli): add --include-unmanaged, --exclude-path, and -y/--yes flags

--include-unmanaged catalogs and bundles unmanaged files from /opt,
/srv, /usr/local at scan time. --exclude-path allows repeatable path
exclusions. -y/--yes is a global flag that suppresses interactive
prompts for CI/automation use.
```

---

### Task 3: Unmanaged File Cataloging in Non-RPM Inspector

**Files:**
- Modify: `crates/collect/src/inspectors/nonrpm.rs`
- Create: `crates/collect/tests/unmanaged_scan_test.rs`

**Interfaces:**
- Consumes: `Executor.execute()` for find/stat/file commands; `RpmState.owned_paths` for RPM-ownership exclusion; `NonRpmSoftwareSection.items` to exclude Tier 1 language environments
- Produces: `UnmanagedFileSection` with cataloged files, provenance signals (raw + derived), RPM and Tier 1 exclusion applied

**Data flow:** The collector runs on the source host. It has direct
access to the filesystem, `/proc/mounts`, systemd unit files, and
`/etc/machine-id`. All provenance signal computation happens here at
scan time — the refine session only consumes the pre-computed booleans.

- [ ] **Step 1: Write failing tests for unmanaged file scan**

Create `crates/collect/tests/unmanaged_scan_test.rs`:

```rust
use inspectah_core::types::nonrpm::{
    FileType, UnmanagedFile, UnmanagedFileSection,
};

#[test]
fn scan_unmanaged_catalogs_elf_binaries() {
    // MockExecutor with:
    //   /opt/splunk/bin/splunkd (ELF binary, 50 MB)
    //   /opt/splunk/etc/config.ini (config file, 2 KB)
    // Assert: both files cataloged with correct file_type.
    // Assert: total_size = sum of file sizes.
    // Assert: total_count = 2.
}

#[test]
fn scan_unmanaged_excludes_rpm_owned_paths() {
    // MockExecutor with:
    //   /opt/rh/httpd24/root/usr/sbin/httpd (RPM-owned)
    //   /opt/myapp/server (not RPM-owned)
    // RpmState.owned_paths includes /opt/rh/httpd24/root/usr/sbin/httpd.
    // Assert: only /opt/myapp/server appears in unmanaged results.
}

#[test]
fn scan_unmanaged_excludes_tier1_language_paths() {
    // MockExecutor with:
    //   /opt/myapp/venv/bin/python (venv — claimed by Tier 1)
    //   /opt/myapp/server (ELF binary — unclaimed)
    // Provide NonRpmSoftwareSection with a pip entry for /opt/myapp/venv.
    // Assert: only /opt/myapp/server appears in unmanaged results.
}

#[test]
fn scan_unmanaged_applies_exclude_paths() {
    // MockExecutor with /opt/splunk/ and /opt/datadog/.
    // exclude_paths = ["/opt/datadog"].
    // Assert: only /opt/splunk/ files appear.
}

#[test]
fn scan_unmanaged_does_not_scan_var() {
    // MockExecutor with /var/lib/myapp/data.db.
    // Assert: file is NOT cataloged — /var is not a scan root.
    // The spec's /var guidance is advisory for the UI, not a scan directive.
}

#[test]
fn scan_unmanaged_classifies_scripts() {
    // MockExecutor with /opt/app/run.sh containing "#!/bin/bash".
    // Assert: file_type == FileType::Script.
}

#[test]
fn scan_unmanaged_computes_mutability_signal() {
    // MockExecutor returns:
    //   file mtime = 1700000000 (recent)
    //   /etc/machine-id ctime = 1600000000 (older install date)
    // Assert: provenance.mutable == true (file is newer than install).
}

#[test]
fn scan_unmanaged_computes_writable_mount_signal() {
    // MockExecutor returns /proc/mounts with /opt mounted rw.
    // File at /opt/app/server.
    // Assert: provenance.writable_mount == true.
}

#[test]
fn scan_unmanaged_computes_service_working_dir_signal() {
    // MockExecutor returns systemd unit file with WorkingDirectory=/opt/myapp.
    // File at /opt/myapp/data.log.
    // Assert: provenance.service_working_dir == true.
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p inspectah-collect --test unmanaged_scan_test`
Expected: FAIL — function does not exist.

- [ ] **Step 3: Implement system-install-date detection**

In `crates/collect/src/inspectors/nonrpm.rs`, add:

```rust
/// Determine the system install date (seconds since epoch).
///
/// Strategy: use the ctime of `/etc/machine-id` (created at OS install).
/// Fallback: query RPM install time of the `filesystem` package
/// (a baseos package present on every RHEL system).
fn detect_system_install_date(exec: &dyn Executor) -> u64 {
    // Try /etc/machine-id ctime first
    let result = exec.execute("stat", &["-c", "%Z", "/etc/machine-id"]);
    if let Ok(r) = result {
        if r.exit_code == 0 {
            if let Ok(ts) = r.stdout.trim().parse::<u64>() {
                return ts;
            }
        }
    }
    // Fallback: RPM install time of `filesystem` package
    let result = exec.execute("rpm", &["-q", "--qf", "%{INSTALLTIME}", "filesystem"]);
    if let Ok(r) = result {
        if r.exit_code == 0 {
            if let Ok(ts) = r.stdout.trim().parse::<u64>() {
                return ts;
            }
        }
    }
    0 // Unknown — all files will be marked as not mutable
}
```

- [ ] **Step 4: Implement mount-point read-write detection**

```rust
use std::collections::HashMap;

/// Parse /proc/mounts and return a map of mount_point -> is_rw.
fn parse_mount_rw_flags(exec: &dyn Executor) -> HashMap<String, bool> {
    let mut mounts = HashMap::new();
    let result = exec.execute("cat", &["/proc/mounts"]);
    let output = match result {
        Ok(r) if r.exit_code == 0 => r.stdout,
        _ => return mounts,
    };
    for line in output.lines() {
        // Format: device mount_point fs_type options ...
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 4 {
            let mount_point = parts[1].to_string();
            let options = parts[3];
            let is_rw = options.split(',').any(|opt| opt == "rw");
            mounts.insert(mount_point, is_rw);
        }
    }
    mounts
}

/// Check if a file path is on a writable mount by finding the
/// longest-prefix mount point match.
fn is_on_writable_mount(path: &str, mounts: &HashMap<String, bool>) -> bool {
    let mut best_match = "";
    let mut best_rw = false;
    for (mount_point, is_rw) in mounts {
        if path.starts_with(mount_point.as_str()) && mount_point.len() > best_match.len() {
            best_match = mount_point;
            best_rw = *is_rw;
        }
    }
    best_rw
}
```

- [ ] **Step 5: Implement service working directory detection**

```rust
/// Collect all WorkingDirectory= values from systemd unit files.
fn collect_service_working_dirs(exec: &dyn Executor) -> Vec<String> {
    let mut dirs = Vec::new();
    for unit_dir in &["/etc/systemd/system", "/usr/lib/systemd/system"] {
        let result = exec.execute(
            "grep",
            &["-rh", "WorkingDirectory=", unit_dir],
        );
        if let Ok(r) = result {
            if r.exit_code == 0 {
                for line in r.stdout.lines() {
                    let value = line
                        .trim()
                        .strip_prefix("WorkingDirectory=")
                        .unwrap_or("")
                        .trim();
                    if !value.is_empty() && value.starts_with('/') {
                        dirs.push(value.to_string());
                    }
                }
            }
        }
    }
    dirs
}

/// Check if a file path is under any service's WorkingDirectory.
fn is_under_service_workdir(path: &str, workdirs: &[String]) -> bool {
    workdirs.iter().any(|wd| path.starts_with(wd.as_str()))
}
```

- [ ] **Step 6: Implement scan_unmanaged_files()**

```rust
use inspectah_core::types::nonrpm::{
    FileType, ProvenanceSignals, UnmanagedFile, UnmanagedFileSection,
};
use inspectah_core::traits::inspector::RpmState;

/// Scan roots for unmanaged files per spec: /opt, /srv, /usr/local ONLY.
/// /var is NOT a scan root — the spec's /var guidance is advisory for
/// the refine UI, not a scan-scope directive.
const UNMANAGED_SCAN_ROOTS: &[&str] = &["/opt", "/srv", "/usr/local"];

/// Scan for unmanaged files not claimed by RPM or Tier 1 language packages.
///
/// Exclusion layers (applied in order):
/// 1. `--exclude-path` user-specified filters
/// 2. RPM-owned paths via `RpmState.owned_paths` (same mechanism Plan 1
///    uses for pip filtering)
/// 3. Tier 1 language environment paths (no double-counting with Plan 1)
pub fn scan_unmanaged_files(
    exec: &dyn Executor,
    rpm_state: Option<&RpmState>,
    language_env_paths: &[String],
    exclude_paths: &[String],
) -> UnmanagedFileSection {
    let mut items = Vec::new();
    let mut total_size: u64 = 0;

    // Pre-compute derived signal inputs once
    let install_date = detect_system_install_date(exec);
    let mounts = parse_mount_rw_flags(exec);
    let service_workdirs = collect_service_working_dirs(exec);

    for root in UNMANAGED_SCAN_ROOTS {
        walk_for_unmanaged(
            exec,
            root,
            rpm_state,
            language_env_paths,
            exclude_paths,
            install_date,
            &mounts,
            &service_workdirs,
            &mut items,
            &mut total_size,
        );
    }

    let total_count = items.len();
    UnmanagedFileSection {
        items,
        total_size,
        total_count,
    }
}

fn walk_for_unmanaged(
    exec: &dyn Executor,
    root: &str,
    rpm_state: Option<&RpmState>,
    language_env_paths: &[String],
    exclude_paths: &[String],
    install_date: u64,
    mounts: &HashMap<String, bool>,
    service_workdirs: &[String],
    items: &mut Vec<UnmanagedFile>,
    total_size: &mut u64,
) {
    // Use find to list all regular files under root, respecting PRUNE_DIRS
    let args = vec![root.to_string(), "-type".into(), "f".into()];
    let result = match exec.execute(
        "find",
        &args.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
    ) {
        Ok(r) if r.exit_code == 0 => r,
        _ => return,
    };

    for line in result.stdout.lines() {
        let path = line.trim();
        if path.is_empty() {
            continue;
        }

        // Layer 1: User-specified exclusions
        if exclude_paths.iter().any(|ep| path.starts_with(ep)) {
            continue;
        }

        // Layer 2: RPM-owned path exclusion
        if let Some(state) = rpm_state {
            if state.is_owned(std::path::Path::new(path)) {
                continue;
            }
        }

        // Layer 3: Tier 1 language environment exclusion
        if language_env_paths.iter().any(|lp| path.starts_with(lp)) {
            continue;
        }

        // Get file metadata via stat
        let (size, last_modified, uid, gid, permissions) =
            get_file_metadata(exec, path);

        let file_type = classify_file_type(exec, path);

        // Compute derived provenance signals
        let mutable = install_date > 0 && last_modified > install_date;
        let writable_mount = is_on_writable_mount(path, mounts);
        let service_working_dir = is_under_service_workdir(path, service_workdirs);

        *total_size += size;
        items.push(UnmanagedFile {
            path: path.to_string(),
            size,
            file_type: file_type.clone(),
            provenance: ProvenanceSignals {
                file_type,
                last_modified,
                uid,
                gid,
                permissions,
                mutable,
                writable_mount,
                service_working_dir,
            },
            include: true,
            under_var: false, // Not possible — /var is not a scan root
            ..Default::default()
        });
    }
}
```

- [ ] **Step 7: Implement classify_file_type() and get_file_metadata()**

```rust
/// Classify a file's type by reading its magic bytes / shebang.
fn classify_file_type(exec: &dyn Executor, path: &str) -> FileType {
    let result = exec.execute("file", &["-b", path]);
    match result {
        Ok(r) if r.exit_code == 0 => {
            let output = r.stdout.to_lowercase();
            if output.contains("elf") {
                FileType::ElfBinary
            } else if output.contains("java archive") || path.ends_with(".jar") {
                FileType::Jar
            } else if output.contains("script") || output.contains("text executable") {
                FileType::Script
            } else if output.contains("symbolic link") {
                FileType::Symlink
            } else if path.ends_with(".conf")
                || path.ends_with(".cfg")
                || path.ends_with(".ini")
                || path.ends_with(".yaml")
                || path.ends_with(".yml")
                || path.ends_with(".toml")
                || path.ends_with(".json")
            {
                FileType::Config
            } else {
                FileType::DataFile
            }
        }
        _ => FileType::Other,
    }
}

/// Get metadata for a file via stat command.
fn get_file_metadata(exec: &dyn Executor, path: &str) -> (u64, u64, u32, u32, String) {
    // stat -c '%s %Y %u %g %a' <path>
    let result = exec.execute("stat", &["-c", "%s %Y %u %g %a", path]);
    match result {
        Ok(r) if r.exit_code == 0 => {
            let parts: Vec<&str> = r.stdout.trim().split_whitespace().collect();
            if parts.len() >= 5 {
                let size = parts[0].parse().unwrap_or(0);
                let mtime = parts[1].parse().unwrap_or(0);
                let uid = parts[2].parse().unwrap_or(0);
                let gid = parts[3].parse().unwrap_or(0);
                let perms = format!("0{}", parts[4]);
                (size, mtime, uid, gid, perms)
            } else {
                (0, 0, 0, 0, String::new())
            }
        }
        _ => (0, 0, 0, 0, String::new()),
    }
}
```

- [ ] **Step 8: Run tests + lint**

Run:
```
cargo test -p inspectah-collect
cargo clippy -- -W clippy::all
cargo fmt --check
```
Expected: all pass including new unmanaged scan tests.

- [ ] **Step 9: Commit**

```
feat(collect): catalog unmanaged files from /opt, /srv, /usr/local

Scan for files not owned by RPM (via RpmState.owned_paths) or Tier 1
language packages. Classify file types (ELF, JAR, script, config,
data, symlink), collect raw provenance metadata (size, mtime, uid,
gid, permissions) and three derived signals: mutability relative to
system install date, writable-mount detection via /proc/mounts, and
service WorkingDirectory= detection from systemd units. Respects
--exclude-path filters. /var is not a scan root per spec.
```

**Thorn checkpoint: review Tasks 1-3 before proceeding.**

---

### Task 4: Size Prompt + Bundling at Scan Time

**Files:**
- Modify: `crates/cli/src/commands/scan.rs`

**Interfaces:**
- Consumes: `UnmanagedFileSection` (from Task 3), `ScanArgs.include_unmanaged`, `assume_yes: bool`
- Produces: Unmanaged files copied into the render directory under `unmanaged/` (bundled into the scan tarball)

**Data flow:** Files are bundled at scan time because the tarball may be
transferred to a different machine for refine — the original files
won't be available later. The scan tarball is the single vehicle that
carries payload files from collector to refine.

- [ ] **Step 1: Add size prompt after unmanaged scan**

In the scan command, after the pipeline runs and produces the snapshot
(which now includes `unmanaged_files` when `--include-unmanaged` was used),
prompt the user with the total count and size:

```rust
if args.include_unmanaged {
    if let Some(ref unmanaged) = snapshot.unmanaged_files {
        if !unmanaged.items.is_empty() {
            let size_display = format_size(unmanaged.total_size);
            let roots = describe_scan_roots(&unmanaged.items);
            if !assume_yes {
                eprintln!(
                    "Found {} unmanaged files in {} ({} total)",
                    unmanaged.total_count, roots, size_display,
                );
                eprint!("Include in tarball? [Y/n] ");
                let mut input = String::new();
                std::io::stdin().read_line(&mut input).ok();
                let input = input.trim().to_lowercase();
                if input == "n" || input == "no" {
                    // Clear unmanaged files from snapshot
                    snapshot.unmanaged_files = None;
                }
            }
        }
    }
}
```

- [ ] **Step 2: Implement format_size helper**

```rust
fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} bytes", bytes)
    }
}

fn describe_scan_roots(items: &[UnmanagedFile]) -> String {
    let mut roots: Vec<&str> = Vec::new();
    for item in items {
        for root in &["/opt", "/srv", "/usr/local"] {
            if item.path.starts_with(root) && !roots.contains(root) {
                roots.push(root);
            }
        }
    }
    roots.join(", ")
}
```

- [ ] **Step 3: Bundle unmanaged files into render directory**

After the prompt (or if `--yes` skips it), copy unmanaged files into
the render directory before tarball creation:

```rust
if let Some(ref unmanaged) = snapshot.unmanaged_files {
    bundle_unmanaged_files(&unmanaged.items, render_dir.path())?;
}
```

Implement:

```rust
fn bundle_unmanaged_files(
    items: &[UnmanagedFile],
    render_dir: &Path,
) -> Result<()> {
    for item in items {
        if !item.include {
            continue;
        }
        // Strip leading / to create relative path under unmanaged/
        let rel_path = item.path.trim_start_matches('/');
        let dest = render_dir.join("unmanaged").join(rel_path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .context(format!("failed to create dir for {}", dest.display()))?;
        }
        std::fs::copy(&item.path, &dest)
            .context(format!("failed to copy {} to tarball", item.path))?;
    }
    Ok(())
}
```

- [ ] **Step 4: Run tests + lint**

Run:
```
cargo test -p inspectah-cli
cargo clippy -- -W clippy::all
cargo fmt --check
```
Expected: all pass.

- [ ] **Step 5: Commit**

```
feat(cli): prompt and bundle unmanaged files at scan time

Display file count and total size, prompt for confirmation before
bundling. -y/--yes suppresses the prompt. Files copied into render
directory under unmanaged/ preserving directory structure for
tarball inclusion. Scan roots limited to /opt, /srv, /usr/local.
```

---

### Task 5: Repo-less RPM Detection + dnf Cache Scanning

**Files:**
- Modify: `crates/collect/src/inspectors/nonrpm.rs` (or new function in rpm inspector area)
- Modify: `crates/core/src/types/rpm.rs`
- Create: `crates/collect/tests/repoless_rpm_test.rs`

**Interfaces:**
- Consumes: `RpmSection.packages`, `Executor.execute()` for dnf cache listing and `dnf repolist --enabled`
- Produces: RPM entries annotated with `repoless_cached: bool`, `repoless_annotation: String`, `cache_path: Option<String>` on `PackageEntry`; cached RPM files bundled into render dir under `repoless-packages/`

**Detection contract:** A package is repo-less when EITHER:
1. `source_repo` is empty (no repo recorded), OR
2. `source_repo` points to a repo name not in the output of `dnf repolist --enabled`

Both cases trigger the same scan/bundle flow.

- [ ] **Step 1: Add fields to PackageEntry**

In `crates/core/src/types/rpm.rs`, add to `PackageEntry`:

```rust
    /// Triage annotation for repo-less packages
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub repoless_annotation: String,

    /// True if cached RPM was found in /var/cache/dnf/
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub repoless_cached: bool,

    /// Path to the cached RPM file in /var/cache/dnf/.
    /// Survives serialization into the snapshot so the CLI bundler
    /// can read it when creating the tarball. Consumed by the scan
    /// command's bundle step (Task 4) to copy the file into
    /// repoless-packages/ in the render directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_path: Option<String>,
```

- [ ] **Step 2: Write failing tests for repo-less detection**

Create `crates/collect/tests/repoless_rpm_test.rs`:

```rust
#[test]
fn repoless_rpm_found_in_cache() {
    // MockExecutor with:
    //   PackageEntry { name: "custom-tool", source_repo: "", arch: "x86_64",
    //                  version: "1.2.3", release: "1.el9" }
    //   dnf cache listing includes custom-tool-1.2.3-1.el9.x86_64.rpm
    // Assert: repoless_cached == true.
    // Assert: cache_path == Some("/var/cache/dnf/.../custom-tool-1.2.3-1.el9.x86_64.rpm").
}

#[test]
fn repoless_rpm_not_in_cache() {
    // Same PackageEntry but dnf cache listing is empty.
    // Assert: repoless_cached == false.
    // Assert: repoless_annotation contains "manual resolution needed".
    // Assert: cache_path == None.
}

#[test]
fn rpm_with_source_repo_not_flagged() {
    // PackageEntry with source_repo = "appstream".
    // dnf repolist --enabled includes "appstream".
    // Assert: not treated as repo-less (no annotation, no cache_path).
}

#[test]
fn rpm_with_disabled_repo_detected_as_repoless() {
    // PackageEntry with source_repo = "internal-tools" (non-empty).
    // dnf repolist --enabled does NOT include "internal-tools".
    // Assert: treated as repo-less.
    // Assert: repoless_annotation mentions disabled/removed repo.
}

#[test]
fn cache_path_survives_json_roundtrip() {
    // PackageEntry with cache_path = Some("/var/cache/dnf/.../foo.rpm").
    // Serialize to JSON, deserialize back.
    // Assert: cache_path is preserved.
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p inspectah-collect --test repoless_rpm_test`
Expected: FAIL

- [ ] **Step 4: Implement get_enabled_repos()**

```rust
/// Fetch the list of enabled repo IDs from dnf.
fn get_enabled_repos(exec: &dyn Executor) -> Vec<String> {
    let result = exec.execute("dnf", &["repolist", "--enabled", "-q"]);
    match result {
        Ok(r) if r.exit_code == 0 => {
            r.stdout
                .lines()
                .skip(1) // Skip header line
                .filter_map(|line| {
                    let id = line.split_whitespace().next()?;
                    if id.is_empty() { None } else { Some(id.to_string()) }
                })
                .collect()
        }
        _ => Vec::new(),
    }
}
```

- [ ] **Step 5: Implement scan_dnf_cache_for_repoless()**

```rust
use inspectah_core::types::rpm::PackageEntry;

/// Identify repo-less packages and scan /var/cache/dnf/ for cached RPMs.
///
/// A package is repo-less when:
/// 1. source_repo is empty, OR
/// 2. source_repo names a repo not in `dnf repolist --enabled`
pub fn scan_dnf_cache_for_repoless(
    exec: &dyn Executor,
    packages: &mut [PackageEntry],
) {
    let enabled_repos = get_enabled_repos(exec);

    // Identify which packages are repo-less
    let repoless_indices: Vec<usize> = packages
        .iter()
        .enumerate()
        .filter(|(_, p)| {
            p.source_repo.is_empty()
                || (!p.source_repo.is_empty()
                    && !enabled_repos.iter().any(|r| r == &p.source_repo))
        })
        .map(|(i, _)| i)
        .collect();

    if repoless_indices.is_empty() {
        return;
    }

    // List all .rpm files in the dnf cache
    let cache_result = exec.execute(
        "find",
        &["/var/cache/dnf", "-name", "*.rpm", "-type", "f"],
    );
    let cache_files: Vec<String> = match cache_result {
        Ok(r) if r.exit_code == 0 => {
            r.stdout
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        }
        _ => Vec::new(),
    };

    for idx in repoless_indices {
        let pkg = &mut packages[idx];
        let nevra = format!(
            "{}-{}-{}.{}",
            pkg.name, pkg.version, pkg.release, pkg.arch
        );
        let expected_filename = format!("{nevra}.rpm");

        let cache_match = cache_files.iter().find(|f| {
            f.ends_with(&expected_filename)
                || f.contains(&format!("/{expected_filename}"))
        });

        let is_disabled_repo = !pkg.source_repo.is_empty();
        let reason = if is_disabled_repo {
            format!(
                "No repo source — repo '{}' not in enabled repos",
                pkg.source_repo
            )
        } else {
            "No repo source".to_string()
        };

        if let Some(path) = cache_match {
            pkg.repoless_cached = true;
            pkg.cache_path = Some(path.clone());
            pkg.repoless_annotation = format!(
                "{reason} — cached RPM bundled (pre-excluded, no GPG verification)"
            );
        } else {
            pkg.repoless_cached = false;
            pkg.cache_path = None;
            pkg.repoless_annotation = format!(
                "{reason} — manual resolution needed"
            );
        }
    }
}
```

- [ ] **Step 6: Bundle cached RPMs into render directory**

In the scan command (same area as Task 4's bundling), after the
pipeline runs:

```rust
fn bundle_repoless_rpms(
    packages: &[PackageEntry],
    render_dir: &Path,
) -> Result<()> {
    let dest_dir = render_dir.join("repoless-packages");
    for pkg in packages {
        if let Some(ref cache_path) = pkg.cache_path {
            std::fs::create_dir_all(&dest_dir)
                .context("failed to create repoless-packages dir")?;
            let nevra = format!(
                "{}-{}-{}.{}",
                pkg.name, pkg.version, pkg.release, pkg.arch
            );
            let filename = format!("{nevra}.rpm");
            let dest = dest_dir.join(&filename);
            std::fs::copy(cache_path, &dest)
                .context(format!("failed to copy cached RPM {cache_path}"))?;
        }
    }
    Ok(())
}
```

Note the data-flow boundary: `cache_path` is set by the collector on
`PackageEntry`, serialized into the snapshot JSON, and consumed by the
scan command's bundle step. After bundling, the RPM files live in
`repoless-packages/` inside the tarball. At refine export time, the
export function extracts them from the source tarball (see Task 9).

- [ ] **Step 7: Run tests + lint**

Run:
```
cargo test -p inspectah-collect -p inspectah-core
cargo clippy -- -W clippy::all
cargo fmt --check
```
Expected: all pass.

- [ ] **Step 8: Commit**

```
feat(collect): scan dnf cache for repo-less RPM packages

Detect packages with empty source_repo or source_repo pointing to
a disabled/removed repo (not in `dnf repolist --enabled`). For
each, check /var/cache/dnf/ for cached .rpm files. Found RPMs are
bundled under repoless-packages/. Missing RPMs get "manual
resolution needed" annotation. cache_path field on PackageEntry
survives serialization for the bundler to consume.
```

---

### Task 6: Containerfile Renderer — Unmanaged Files

**Files:**
- Create: `crates/pipeline/src/render/unmanaged.rs`
- Modify: `crates/pipeline/src/render/mod.rs`
- Modify: `crates/pipeline/src/render/containerfile.rs`

**Interfaces:**
- Consumes: `InspectionSnapshot.unmanaged_files`
- Produces: `Vec<String>` of Containerfile lines with COPY directives and warning block

- [ ] **Step 1: Create unmanaged.rs renderer module**

Create `crates/pipeline/src/render/unmanaged.rs`:

```rust
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::nonrpm::UnmanagedFile;
use std::collections::BTreeMap;

/// Render Containerfile lines for unmanaged files.
///
/// Groups files by parent directory for readability.
/// Includes warning block per spec.
pub fn unmanaged_file_lines(snap: &InspectionSnapshot) -> Vec<String> {
    let section = match &snap.unmanaged_files {
        Some(s) if !s.items.is_empty() => s,
        _ => return Vec::new(),
    };

    let included: Vec<&UnmanagedFile> = section
        .items
        .iter()
        .filter(|f| f.include)
        .collect();

    if included.is_empty() {
        return Vec::new();
    }

    let mut lines = Vec::new();
    lines.push(String::new());
    lines.push("# === Unmanaged files (no package manager provenance) ===".into());
    lines.push(
        "# These files were copied directly from the source host. They have".into(),
    );
    lines.push(
        "# no upstream update path and must be manually maintained.".into(),
    );

    // Group by parent directory
    let mut groups: BTreeMap<String, Vec<&UnmanagedFile>> = BTreeMap::new();
    for file in &included {
        let parent = std::path::Path::new(&file.path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "/".to_string());
        groups.entry(parent).or_default().push(file);
    }

    for (dir, files) in &groups {
        let rel_dir = dir.trim_start_matches('/');
        if files.len() > 1 {
            // Directory-level COPY
            lines.push(format!(
                "COPY unmanaged/{rel_dir}/ /{rel_dir}/"
            ));
        } else {
            // Single file COPY
            for file in files {
                let rel_path = file.path.trim_start_matches('/');
                lines.push(format!(
                    "COPY unmanaged/{rel_path} {}",
                    file.path
                ));
            }
        }
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::types::nonrpm::{
        FileType, UnmanagedFile, UnmanagedFileSection,
    };

    fn test_snapshot_with_unmanaged(items: Vec<UnmanagedFile>) -> InspectionSnapshot {
        let total_size = items.iter().map(|f| f.size).sum();
        let total_count = items.len();
        let mut snap = InspectionSnapshot::default();
        snap.unmanaged_files = Some(UnmanagedFileSection {
            items,
            total_size,
            total_count,
        });
        snap
    }

    #[test]
    fn renders_copy_with_warning_block() {
        let snap = test_snapshot_with_unmanaged(vec![
            UnmanagedFile {
                path: "/opt/splunk/bin/splunkd".into(),
                size: 1024,
                file_type: FileType::ElfBinary,
                include: true,
                ..Default::default()
            },
        ]);
        let lines = unmanaged_file_lines(&snap);
        assert!(lines.iter().any(|l| l.contains("=== Unmanaged files")));
        assert!(lines.iter().any(|l| l.contains("COPY unmanaged/")));
        assert!(lines.iter().any(|l| l.contains("manually maintained")));
    }

    #[test]
    fn excluded_files_not_rendered() {
        let snap = test_snapshot_with_unmanaged(vec![
            UnmanagedFile {
                path: "/opt/app/server".into(),
                include: false,
                ..Default::default()
            },
        ]);
        let lines = unmanaged_file_lines(&snap);
        assert!(lines.is_empty());
    }

    #[test]
    fn groups_by_directory() {
        let snap = test_snapshot_with_unmanaged(vec![
            UnmanagedFile {
                path: "/opt/splunk/bin/splunkd".into(),
                include: true,
                ..Default::default()
            },
            UnmanagedFile {
                path: "/opt/splunk/bin/btool".into(),
                include: true,
                ..Default::default()
            },
        ]);
        let lines = unmanaged_file_lines(&snap);
        assert!(lines.iter().any(|l| l.contains("COPY unmanaged/opt/splunk/bin/ /opt/splunk/bin/")));
    }
}
```

- [ ] **Step 2: Add module to mod.rs**

In `crates/pipeline/src/render/mod.rs`, add:

```rust
pub mod unmanaged;
```

- [ ] **Step 3: Wire into containerfile.rs**

In `crates/pipeline/src/render/containerfile.rs`, in
`render_containerfile_inner()`, add a call to the unmanaged renderer
after `non_rpm_section_lines()`:

```rust
// Unmanaged files section (opt-in via --include-unmanaged)
lines.extend(unmanaged::unmanaged_file_lines(snap));
```

- [ ] **Step 4: Run tests + lint**

Run:
```
cargo test -p inspectah-pipeline
cargo clippy -- -W clippy::all
cargo fmt --check
```
Expected: all pass.

- [ ] **Step 5: Commit**

```
feat(pipeline): add unmanaged file Containerfile rendering

Render COPY directives for unmanaged files with warning block.
Files grouped by parent directory. Excluded files are not rendered.
```

---

### Task 7: Containerfile Renderer — Repo-less RPMs

**Files:**
- Create: `crates/pipeline/src/render/repoless.rs`
- Modify: `crates/pipeline/src/render/mod.rs`
- Modify: `crates/pipeline/src/render/containerfile.rs`

**Interfaces:**
- Consumes: `InspectionSnapshot.rpm` with `repoless_cached` and `repoless_annotation` fields
- Produces: `Vec<String>` of Containerfile lines with `dnf localinstall` directives

- [ ] **Step 1: Create repoless.rs renderer module**

Create `crates/pipeline/src/render/repoless.rs`:

```rust
use inspectah_core::snapshot::InspectionSnapshot;

/// Render Containerfile lines for repo-less RPM packages.
///
/// Cached RPMs: COPY + dnf localinstall (commented out by default —
/// pre-excluded in refine, user must explicitly include).
/// Missing RPMs: MANUAL comment block.
pub fn repoless_rpm_lines(snap: &InspectionSnapshot) -> Vec<String> {
    let rpm_section = match &snap.rpm {
        Some(r) => r,
        None => return Vec::new(),
    };

    let repoless: Vec<_> = rpm_section
        .packages
        .iter()
        .filter(|p| !p.repoless_annotation.is_empty())
        .collect();

    if repoless.is_empty() {
        return Vec::new();
    }

    let mut lines = Vec::new();
    lines.push(String::new());
    lines.push("# === Repo-less RPM packages ===".into());

    for pkg in &repoless {
        let nevra = format!(
            "{}-{}-{}.{}",
            pkg.name, pkg.version, pkg.release, pkg.arch
        );
        let rpm_filename = format!("{nevra}.rpm");

        if pkg.repoless_cached {
            if pkg.include {
                // User explicitly included — render active
                lines.push(format!(
                    "# Repo-less package: {} (cached RPM, no repository provenance)",
                    pkg.name
                ));
                lines.push(
                    "# WARNING: This package has no upstream repo and no GPG verification.".into(),
                );
                lines.push(
                    "# It was found in the local dnf cache. Updates must be managed manually."
                        .into(),
                );
                lines.push(format!(
                    "COPY repoless-packages/{rpm_filename} /tmp/"
                ));
                lines.push(format!(
                    "RUN dnf localinstall -y /tmp/{rpm_filename} \\"
                ));
                lines.push(format!(
                    "    && rm /tmp/{rpm_filename}"
                ));
            } else {
                // Pre-excluded — render commented out
                lines.push(format!(
                    "# Repo-less package: {} (cached RPM, no repository provenance)",
                    pkg.name
                ));
                lines.push(
                    "# WARNING: This package has no upstream repo and no GPG verification.".into(),
                );
                lines.push(
                    "# Pre-excluded — uncomment after verifying provenance:".into(),
                );
                lines.push(format!(
                    "# COPY repoless-packages/{rpm_filename} /tmp/"
                ));
                lines.push(format!(
                    "# RUN dnf localinstall -y /tmp/{rpm_filename} \\"
                ));
                lines.push(format!(
                    "#     && rm /tmp/{rpm_filename}"
                ));
            }
        } else {
            // No cached RPM — manual resolution
            lines.push(format!(
                "# MANUAL: {} (no repo source, RPM not in cache)",
                pkg.name
            ));
            lines.push(
                "# Provide the RPM via the refine UI upload, add a repo, or uncomment:".into(),
            );
            lines.push(format!(
                "# RUN dnf install {}",
                pkg.name
            ));
        }
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::types::rpm::{PackageEntry, RpmSection};

    fn test_snapshot_with_repoless(packages: Vec<PackageEntry>) -> InspectionSnapshot {
        let mut snap = InspectionSnapshot::default();
        snap.rpm = Some(RpmSection {
            packages,
            ..Default::default()
        });
        snap
    }

    #[test]
    fn cached_rpm_pre_excluded_renders_commented() {
        let snap = test_snapshot_with_repoless(vec![PackageEntry {
            name: "custom-tool".into(),
            version: "1.2.3".into(),
            release: "1.el9".into(),
            arch: "x86_64".into(),
            source_repo: String::new(),
            include: false,
            repoless_cached: true,
            repoless_annotation: "No repo source — cached RPM bundled".into(),
            ..Default::default()
        }]);
        let lines = repoless_rpm_lines(&snap);
        assert!(lines.iter().any(|l| l.starts_with("# COPY repoless-packages/")));
        assert!(lines.iter().any(|l| l.starts_with("# RUN dnf localinstall")));
    }

    #[test]
    fn cached_rpm_user_included_renders_active() {
        let snap = test_snapshot_with_repoless(vec![PackageEntry {
            name: "custom-tool".into(),
            version: "1.2.3".into(),
            release: "1.el9".into(),
            arch: "x86_64".into(),
            source_repo: String::new(),
            include: true,
            repoless_cached: true,
            repoless_annotation: "No repo source — cached RPM bundled".into(),
            ..Default::default()
        }]);
        let lines = repoless_rpm_lines(&snap);
        assert!(lines.iter().any(|l| l.starts_with("COPY repoless-packages/")));
        assert!(lines.iter().any(|l| l.starts_with("RUN dnf localinstall")));
    }

    #[test]
    fn missing_rpm_renders_manual_block() {
        let snap = test_snapshot_with_repoless(vec![PackageEntry {
            name: "custom-tool".into(),
            version: "1.2.3".into(),
            release: "1.el9".into(),
            arch: "x86_64".into(),
            source_repo: String::new(),
            include: false,
            repoless_cached: false,
            repoless_annotation: "No repo source — manual resolution needed".into(),
            ..Default::default()
        }]);
        let lines = repoless_rpm_lines(&snap);
        assert!(lines.iter().any(|l| l.contains("MANUAL: custom-tool")));
        assert!(lines.iter().any(|l| l.contains("dnf install custom-tool")));
    }

    #[test]
    fn packages_with_repo_not_rendered() {
        let snap = test_snapshot_with_repoless(vec![PackageEntry {
            name: "httpd".into(),
            source_repo: "appstream".into(),
            ..Default::default()
        }]);
        let lines = repoless_rpm_lines(&snap);
        assert!(lines.is_empty());
    }

    #[test]
    fn disabled_repo_package_renders_as_repoless() {
        let snap = test_snapshot_with_repoless(vec![PackageEntry {
            name: "internal-tool".into(),
            version: "2.0".into(),
            release: "1.el9".into(),
            arch: "x86_64".into(),
            source_repo: "internal-tools".into(), // non-empty but disabled
            include: false,
            repoless_cached: true,
            repoless_annotation:
                "No repo source — repo 'internal-tools' not in enabled repos — cached RPM bundled"
                    .into(),
            ..Default::default()
        }]);
        let lines = repoless_rpm_lines(&snap);
        assert!(lines.iter().any(|l| l.contains("Repo-less package")));
    }
}
```

- [ ] **Step 2: Add module to mod.rs**

In `crates/pipeline/src/render/mod.rs`, add:

```rust
pub mod repoless;
```

- [ ] **Step 3: Wire into containerfile.rs**

In `render_containerfile_inner()`, add after the unmanaged section:

```rust
// Repo-less RPM packages
lines.extend(repoless::repoless_rpm_lines(snap));
```

- [ ] **Step 4: Run tests + lint**

Run:
```
cargo test -p inspectah-pipeline
cargo clippy -- -W clippy::all
cargo fmt --check
```
Expected: all pass.

- [ ] **Step 5: Commit**

```
feat(pipeline): add repo-less RPM Containerfile rendering

Render dnf localinstall directives for cached repo-less RPMs.
Pre-excluded packages render as commented-out with provenance
warnings. Missing RPMs get MANUAL comment blocks with fallback
dnf install suggestion. Handles both empty source_repo and
disabled-repo cases.
```

**Thorn checkpoint: review Tasks 4-7 before proceeding.**

---

### Task 8: Refine Classification — Pre-exclude Repo-less RPMs

**Files:**
- Modify: `crates/refine/src/classify.rs`

**Interfaces:**
- Consumes: `PackageEntry.repoless_annotation`
- Produces: `include: false` on repo-less RPMs (provenance trust gate)

- [ ] **Step 1: Write failing test**

In the classify test module (or a new test in
`crates/refine/tests/`), add:

```rust
#[test]
fn repoless_rpms_pre_excluded_by_classifier() {
    // Snapshot with:
    //   PackageEntry { name: "httpd", source_repo: "appstream", include: true }
    //   PackageEntry { name: "custom-tool", source_repo: "", repoless_cached: true,
    //                  repoless_annotation: "...", include: true }
    //   PackageEntry { name: "internal-tool", source_repo: "internal-tools",
    //                  repoless_annotation: "...", include: true }
    // Run classify_packages.
    // Assert: httpd.include == true (normal package).
    // Assert: custom-tool.include == false (pre-excluded, empty repo).
    // Assert: internal-tool.include == false (pre-excluded, disabled repo).
}
```

- [ ] **Step 2: Run test to verify it fails**

Expected: FAIL — classifier doesn't know about repo-less.

- [ ] **Step 3: Implement repo-less pre-exclusion**

In `crates/refine/src/classify.rs`, in `classify_packages()` (or
the relevant classification function), add a rule:

```rust
// Repo-less RPMs are pre-excluded — user must explicitly include.
// This is the provenance trust gate per spec. Applies to both
// empty source_repo and packages from disabled/removed repos.
for pkg in &mut packages {
    if !pkg.repoless_annotation.is_empty() {
        pkg.include = false;
    }
}
```

- [ ] **Step 4: Run tests + lint**

Run:
```
cargo test -p inspectah-refine
cargo clippy -- -W clippy::all
cargo fmt --check
```
Expected: all pass.

- [ ] **Step 5: Commit**

```
feat(refine): pre-exclude repo-less RPMs by default

Repo-less packages (those with a repoless_annotation) are set to
include: false during classification. This is the provenance trust
gate — users must explicitly include packages with no upstream repo
verification. Covers both empty source_repo and disabled-repo cases.
```

---

### Task 9: Export Contract — Source Tarball Extraction + Allowlist

**Files:**
- Modify: `crates/refine/src/session.rs` (export allowlist + source tarball extraction)

**Interfaces:**
- Consumes: `InspectionSnapshot.unmanaged_files`, `PackageEntry.repoless_cached`, source tarball at `self.tarball_path`
- Produces: `unmanaged/` and `repoless-packages/` directories in export tarball

**Data flow — this is the critical path the original plan left underspecified:**

```
Collector (scan time)
  → bundles payload files into scan tarball
    → unmanaged/opt/splunk/bin/splunkd
    → repoless-packages/custom-tool-1.2.3.x86_64.rpm

RefineSession (refine time)
  → self.tarball_path points to the scan tarball on disk
  → snapshot JSON has metadata (paths, provenance, include flags)
  → user toggles include/exclude in the refine UI

Export (render_refine_export)
  → creates a fresh tempdir
  → materializes config tree, Containerfile, etc. (existing flow)
  → NEW: extracts payload files from source tarball into tempdir
    → reads unmanaged/ and repoless-packages/ entries from source tarball
    → only extracts files where include == true (after user toggling)
  → allowlist permits unmanaged/ and repoless-packages/ to survive cleanup
  → creates the export tarball from the tempdir
```

The source tarball is the single vehicle. Payload files do NOT appear
magically in the export tempdir — they must be explicitly extracted
from the source tarball. `RefineSession.tarball_path` provides the
path to read from.

- [ ] **Step 1: Add roots to export allowlist**

In `crates/refine/src/session.rs`, find the `allowed_top_level` HashSet.
Add:

```rust
"unmanaged",
"repoless-packages",
```

Note: Plan 1 adds `"language-packages"` — these entries are additive.

- [ ] **Step 2: Add source tarball payload extraction to render_refine_export**

Extend `render_refine_export()` to accept the source tarball path and
extract payload directories. The function signature gains an optional
parameter:

```rust
pub fn render_refine_export(
    snap: &InspectionSnapshot,
    tarball_path: &Path,
    original_includes: Option<&std::collections::HashMap<String, bool>>,
    render_ctx: Option<&RenderContext>,
    source_tarball: Option<&Path>,  // NEW — path to the scan tarball
    upload_dir: Option<&Path>,      // NEW — path to uploaded RPMs dir
) -> Result<(), RefineError> {
```

After materializing config tree and before the allowlist cleanup, add:

```rust
// Extract payload files from source tarball for Tier 2 and Tier 3.
// These files were bundled at scan time and must be re-extracted
// into the export tempdir for the export tarball to include them.
if let Some(source) = source_tarball {
    extract_payload_dirs_from_tarball(source, snap, out)?;
}

// Copy uploaded RPMs (from refine UI upload endpoint) alongside
// cached RPMs from the source tarball.
if let Some(uploads) = upload_dir {
    copy_uploaded_rpms(uploads, out)?;
}
```

Implement:

```rust
/// Extract unmanaged/ and repoless-packages/ from the source tarball
/// into the export directory, filtering by include flags.
fn extract_payload_dirs_from_tarball(
    source_tarball: &Path,
    snap: &InspectionSnapshot,
    out: &Path,
) -> Result<(), RefineError> {
    let file = std::fs::File::open(source_tarball)
        .map_err(|e| RefineError::TarballError(format!("open source tarball: {e}")))?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);

    // Build set of included unmanaged paths for filtering
    let included_unmanaged: std::collections::HashSet<String> = snap
        .unmanaged_files
        .as_ref()
        .map(|s| {
            s.items
                .iter()
                .filter(|f| f.include)
                .map(|f| f.path.trim_start_matches('/').to_string())
                .collect()
        })
        .unwrap_or_default();

    // Build set of included repoless RPM NEVRAs
    let included_repoless: std::collections::HashSet<String> = snap
        .rpm
        .as_ref()
        .map(|r| {
            r.packages
                .iter()
                .filter(|p| p.include && p.repoless_cached)
                .map(|p| format!("{}-{}-{}.{}.rpm", p.name, p.version, p.release, p.arch))
                .collect()
        })
        .unwrap_or_default();

    for entry in archive.entries().map_err(|e| RefineError::TarballError(e.to_string()))? {
        let mut entry = entry.map_err(|e| RefineError::TarballError(e.to_string()))?;
        let path = entry.path().map_err(|e| RefineError::TarballError(e.to_string()))?;
        let path_str = path.to_string_lossy();

        // Strip the top-level directory prefix from the tarball entry
        let rel = path_str
            .find('/')
            .map(|i| &path_str[i + 1..])
            .unwrap_or(&path_str);

        if rel.starts_with("unmanaged/") {
            let file_rel = rel.strip_prefix("unmanaged/").unwrap_or("");
            if included_unmanaged.contains(file_rel) {
                let dest = out.join(rel);
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| RefineError::TarballError(e.to_string()))?;
                }
                let mut outfile = std::fs::File::create(&dest)
                    .map_err(|e| RefineError::TarballError(e.to_string()))?;
                std::io::copy(&mut entry, &mut outfile)
                    .map_err(|e| RefineError::TarballError(e.to_string()))?;
            }
        } else if rel.starts_with("repoless-packages/") {
            let filename = rel.strip_prefix("repoless-packages/").unwrap_or("");
            if included_repoless.contains(filename) {
                let dest = out.join(rel);
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| RefineError::TarballError(e.to_string()))?;
                }
                let mut outfile = std::fs::File::create(&dest)
                    .map_err(|e| RefineError::TarballError(e.to_string()))?;
                std::io::copy(&mut entry, &mut outfile)
                    .map_err(|e| RefineError::TarballError(e.to_string()))?;
            }
        }
    }
    Ok(())
}

/// Copy uploaded RPMs from the upload staging directory into
/// repoless-packages/ in the export directory.
fn copy_uploaded_rpms(upload_dir: &Path, out: &Path) -> Result<(), RefineError> {
    if !upload_dir.exists() {
        return Ok(());
    }
    let dest_dir = out.join("repoless-packages");
    for entry in std::fs::read_dir(upload_dir)
        .map_err(|e| RefineError::TarballError(e.to_string()))?
    {
        let entry = entry.map_err(|e| RefineError::TarballError(e.to_string()))?;
        let name = entry.file_name();
        if name.to_string_lossy().ends_with(".rpm") {
            std::fs::create_dir_all(&dest_dir)
                .map_err(|e| RefineError::TarballError(e.to_string()))?;
            std::fs::copy(entry.path(), dest_dir.join(&name))
                .map_err(|e| RefineError::TarballError(e.to_string()))?;
        }
    }
    Ok(())
}
```

- [ ] **Step 3: Update export_tarball() to pass source tarball path**

In `RefineSession::export_tarball()`, pass `self.tarball_path` and
`self.upload_dir` (new field, see Task 11) to `render_refine_export()`:

```rust
pub fn export_tarball(&self, path: &Path, expected_generation: u64) -> Result<(), RefineError> {
    // ... existing generation check ...
    render_refine_export(
        &projected,
        path,
        Some(&orig_inc),
        self.cached_render_context.as_ref(),
        self.tarball_path.as_deref(),  // source tarball for payload extraction
        self.upload_dir.as_deref(),    // uploaded RPMs directory
    )
}
```

- [ ] **Step 4: Run tests + lint**

Run:
```
cargo test -p inspectah-refine
cargo clippy -- -W clippy::all
cargo fmt --check
```
Expected: all pass.

- [ ] **Step 5: Commit**

```
feat(refine): extract payload files from source tarball during export

Export now extracts unmanaged/ and repoless-packages/ entries from
the source scan tarball, filtering by include flags. Only included
files appear in the export tarball. Uploaded RPMs from the refine
UI are merged into repoless-packages/ alongside cached RPMs.
Allowlist extended with both new roots.
```

---

### Task 10: Preview/Export Parity Tests

**Files:**
- Create: `crates/refine/tests/export_parity_test.rs`

**Interfaces:**
- Consumes: Containerfile rendering output, export tarball contents
- Produces: Tests that verify every `COPY unmanaged/...` and `COPY repoless-packages/...` path in the generated Containerfile has a corresponding file in the export tarball

- [ ] **Step 1: Write parity test for unmanaged files**

```rust
#[test]
fn containerfile_unmanaged_copy_paths_match_export_layout() {
    // Build a snapshot with unmanaged files (include: true).
    // Create a source tarball with those files under unmanaged/.
    // Render Containerfile lines.
    // Run render_refine_export with the source tarball.
    // Extract all COPY source paths from the Containerfile that
    // start with "unmanaged/".
    // Assert: every COPY source path exists in the export tarball.
}
```

- [ ] **Step 2: Write parity test for repoless RPMs**

```rust
#[test]
fn containerfile_repoless_copy_paths_match_export_layout() {
    // Build a snapshot with a repoless RPM (include: true, cached: true).
    // Create a source tarball with the RPM under repoless-packages/.
    // Render Containerfile lines.
    // Run render_refine_export with the source tarball.
    // Extract all COPY source paths that start with "repoless-packages/".
    // Assert: every COPY source path exists in the export tarball.
}
```

- [ ] **Step 3: Write parity test for excluded items**

```rust
#[test]
fn excluded_items_absent_from_both_containerfile_and_export() {
    // Build a snapshot with an unmanaged file (include: false) and
    // a repoless RPM (include: false).
    // Render Containerfile lines.
    // Run export.
    // Assert: no COPY lines reference these items.
    // Assert: items are not present in the export directory.
}
```

- [ ] **Step 4: Run tests + lint**

Run:
```
cargo test -p inspectah-refine --test export_parity_test
cargo clippy -- -W clippy::all
cargo fmt --check
```
Expected: all pass.

- [ ] **Step 5: Commit**

```
test(refine): add preview/export parity tests for Tier 2 and Tier 3

Verify that Containerfile COPY source paths for unmanaged/ and
repoless-packages/ match actual files in the export tarball.
Verify excluded items appear in neither the Containerfile nor
the export.
```

---

### Task 11: RPM Upload API Endpoint

**Files:**
- Create: `crates/refine-web/src/upload.rs`
- Modify: `crates/refine-web/src/lib.rs` (add route)
- Modify: `crates/refine/src/session.rs` (add `upload_dir` field)

**Interfaces:**
- Consumes: multipart/form-data RPM file upload from the refine UI
- Produces: RPM files staged in a session-specific temp directory; export reads from this directory alongside the source tarball

**Data flow:**

```
Refine UI (Plan 3)
  → POST /api/upload-rpm with .rpm file
  → Backend stores file in session-specific upload_dir (tempdir)
  → RefineSession.upload_dir tracks the staging directory
  → Export reads from both:
    1. Source tarball (cached RPMs from scan time)
    2. upload_dir (user-uploaded RPMs from refine UI)
  → Both sources merge into repoless-packages/ in the export tarball
```

- [ ] **Step 1: Add upload_dir field to RefineSession**

In `crates/refine/src/session.rs`, add to `RefineSession`:

```rust
    /// Directory for user-uploaded RPM files (from refine UI).
    /// Created on first upload. Export reads from here alongside
    /// the source tarball's repoless-packages/.
    upload_dir: Option<PathBuf>,
```

Add a method:

```rust
/// Ensure the upload directory exists and return its path.
pub fn ensure_upload_dir(&mut self) -> Result<&Path, RefineError> {
    if self.upload_dir.is_none() {
        let dir = tempfile::tempdir()
            .map_err(|e| RefineError::TarballError(format!("create upload dir: {e}")))?;
        self.upload_dir = Some(dir.into_path());
    }
    Ok(self.upload_dir.as_deref().unwrap())
}

pub fn upload_dir(&self) -> Option<&Path> {
    self.upload_dir.as_deref()
}
```

- [ ] **Step 2: Create upload handler**

Create `crates/refine-web/src/upload.rs`:

```rust
use axum::extract::{Multipart, State};
use axum::response::Json;
use crate::AppState;
use crate::AppError;

/// Accept an uploaded RPM file and stage it for export.
///
/// The uploaded file is stored in the session's upload directory.
/// Export merges these files into repoless-packages/ alongside
/// cached RPMs from the source tarball.
pub async fn upload_rpm(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, AppError> {
    let mut uploaded_count = 0u32;

    while let Some(field) = multipart.next_field().await
        .map_err(|e| AppError(inspectah_refine::types::RefineError::BadRequest(
            format!("multipart error: {e}")
        )))?
    {
        let filename = field
            .file_name()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "unknown.rpm".to_string());

        if !filename.ends_with(".rpm") {
            return Err(AppError(inspectah_refine::types::RefineError::BadRequest(
                format!("only .rpm files accepted, got: {filename}")
            )));
        }

        let data = field.bytes().await
            .map_err(|e| AppError(inspectah_refine::types::RefineError::BadRequest(
                format!("failed to read upload: {e}")
            )))?;

        let mut session = state.session.lock().unwrap();
        let upload_dir = session.ensure_upload_dir()
            .map_err(AppError)?;
        let dest = upload_dir.join(&filename);
        std::fs::write(&dest, &data)
            .map_err(|e| AppError(inspectah_refine::types::RefineError::TarballError(
                format!("write uploaded RPM: {e}")
            )))?;

        uploaded_count += 1;
    }

    Ok(Json(serde_json::json!({
        "uploaded": uploaded_count,
        "status": "staged"
    })))
}
```

- [ ] **Step 3: Add route to router**

In `crates/refine-web/src/lib.rs`, add the route:

```rust
.route("/api/upload-rpm", post(upload::upload_rpm))
```

Add `pub mod upload;` to the module declarations.

- [ ] **Step 4: Run tests + lint**

Run:
```
cargo test -p inspectah-refine-web
cargo clippy -- -W clippy::all
cargo fmt --check
```
Expected: all pass.

- [ ] **Step 5: Commit**

```
feat(refine-web): add RPM upload endpoint for repo-less packages

POST /api/upload-rpm accepts multipart .rpm file uploads and
stages them in a session-specific temp directory. Export reads
from both the source tarball (cached RPMs) and the upload
directory (user-provided RPMs), merging both into
repoless-packages/ in the export tarball.
```

---

### Task 12: Docs Update + Export Contract Tests

**Files:**
- Modify: `docs/reference/output-artifacts.md`
- Modify or create: `crates/refine/tests/export_contract_test.rs`

**Interfaces:**
- This is the final task — no downstream dependencies within this plan.

- [ ] **Step 1: Update output artifacts docs**

In `docs/reference/output-artifacts.md`, add `unmanaged/` and
`repoless-packages/` to the artifact root table:

| Root | Purpose | Gate |
|------|---------|------|
| `unmanaged/` | Copied unmanaged files, directory structure preserved | `--include-unmanaged` |
| `repoless-packages/` | Cached and uploaded RPM files for repo-less packages | Automatic |

- [ ] **Step 2: Write export contract tests**

In `crates/refine/tests/export_contract_test.rs`, add:

```rust
#[test]
fn export_allowlist_includes_unmanaged_root() {
    // Build a snapshot with unmanaged files.
    // Create a source tarball with files under unmanaged/.
    // Run render_refine_export with source tarball.
    // Assert: unmanaged/ directory present in output.
}

#[test]
fn export_prunes_excluded_unmanaged_files() {
    // Build a snapshot with two unmanaged files, one include:false.
    // Create source tarball with both.
    // Run export.
    // Assert: included file present, excluded file absent.
}

#[test]
fn export_allowlist_includes_repoless_packages_root() {
    // Build a snapshot with repo-less RPM data (include: true).
    // Create source tarball with RPM under repoless-packages/.
    // Run export.
    // Assert: repoless-packages/ present in output.
}

#[test]
fn export_includes_uploaded_rpms() {
    // Create upload_dir with an uploaded RPM.
    // Run export with upload_dir.
    // Assert: uploaded RPM appears in repoless-packages/ output.
}

#[test]
fn export_merges_cached_and_uploaded_rpms() {
    // Source tarball has cached-tool.rpm under repoless-packages/.
    // Upload dir has uploaded-tool.rpm.
    // Run export.
    // Assert: both RPMs present in repoless-packages/ output.
}
```

- [ ] **Step 3: Run full test suite + lint**

Run:
```
cargo test
cargo clippy -- -W clippy::all
cargo fmt --check
```
Expected: all tests pass. Some snapshot tests may need updating due to
new Containerfile sections — update insta snapshots with
`cargo insta review`.

- [ ] **Step 4: Commit**

```
docs(reference): document unmanaged/ and repoless-packages/ export roots

Add new artifact roots to output-artifacts.md. Add export contract
tests verifying payload extraction from source tarball, exclusion
pruning, uploaded RPM handling, and cached+uploaded RPM merging.
```

**Thorn checkpoint: review Tasks 8-12 before proceeding to Plan 3.**

---

## Dependency Map

```
Task 1 (types) ─────┬──→ Task 3 (unmanaged scan) ──→ Task 4 (prompt + bundle)
                     │                                       │
Task 2 (CLI flags) ──┘                                       ├──→ Task 6 (unmanaged render)
                                                             │
                     ┌──→ Task 5 (repoless scan) ────────────┤
                     │                                       ├──→ Task 7 (repoless render)
                     │                                       │
                     │                                Task 8 (classify)
                     │                                       │
                     └──→ Task 9 (export contract) ──────────┤
                                                             │
                                                     Task 10 (parity tests)
                                                             │
                                                     Task 11 (upload endpoint)
                                                             │
                                                     Task 12 (docs + final tests)
```

Tasks 1-2 can run in parallel. Tasks 3-5 depend on Task 1. Tasks 6-7 depend on Tasks 3-5. Task 8 depends on Task 5. Task 9 depends on Tasks 6-7. Tasks 10-12 depend on Task 9.

## Shared Contracts Consumed from Plan 1

This plan consumes the following contracts established by Plan 1:

### ItemId Variants (Plan 1, Task 1)

| Variant | Used in This Plan |
|---------|------------------|
| `ItemId::UnmanagedFile { path }` | **Added by this plan** (Task 1, Step 6) |
| `ItemId::Package { name, arch }` | Used for repo-less RPMs (existing variant, no change) |
| `ItemId::LanguageEnv { ecosystem, path }` | Not used in this plan (Tier 1/Plan 3) |

### Export Allowlist Pattern (Plan 1, Task 7)

This plan adds two new roots to the same `allowed_top_level` HashSet:

| Root | Gate |
|------|------|
| `unmanaged` | `--include-unmanaged` flag used at scan time |
| `repoless-packages` | Automatic when repo-less RPMs detected |

### Method Strings (Plan 1, Shared Contracts)

| Method | Source | Used By |
|--------|--------|---------|
| `"binary"` | ELF binary scan | This plan (unmanaged file classification) |

### Confidence Rendering Gate (Plan 1, Shared Contracts)

Repo-less RPMs use the confidence gate indirectly:
- Pre-excluded by default (provenance trust gate) — equivalent to medium confidence behavior
- User must explicitly include — active rendering only when toggled on

Unmanaged files do not use the confidence gate — they are always included by default (user toggles off in refine).
