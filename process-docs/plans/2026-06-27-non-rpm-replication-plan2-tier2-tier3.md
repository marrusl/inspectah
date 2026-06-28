# Non-RPM Replication Plan 2: Tier 2 (Unmanaged Files) + Tier 3 (Repo-less RPMs)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add opt-in unmanaged file cataloging/bundling (Tier 2) and automatic repo-less RPM bundling (Tier 3) to the scan and refine pipeline. Both tiers produce executable Containerfile output backed by collected artifacts in the export tarball.

**Architecture:** Five layers change: (1) CLI gains `--include-unmanaged`, `--exclude-path`, `-y`/`--yes` flags, (2) core types gain `ItemId::UnmanagedFile` and unmanaged file data model, (3) the non-RPM inspector catalogs unmanaged files and the RPM inspector scans the dnf cache for repo-less packages, (4) the pipeline renderer emits `COPY`/`RUN` directives for both tiers with appropriate warning blocks, (5) refine export materializes `unmanaged/` and `repoless-packages/` roots.

**Tech Stack:** Rust (2024 edition), clap (CLI args), serde, insta (snapshot testing), sha2/hex (path hashing), inspectah-core types, inspectah-refine, inspectah-pipeline, inspectah-collect, inspectah-cli.

**Spec:** `process-docs/specs/proposed/2026-06-27-non-rpm-replication.md` — read fresh before implementation. This plan covers Tier 2 and Tier 3. Plan 1 covers Tier 1 and shared contracts.

**Thorn Checkpoints:** After Tasks 3, 6, 9.

## Global Constraints

- Clippy clean: `cargo clippy -- -W clippy::all` with zero warnings.
- Format: `cargo fmt --check` must pass.
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
| `crates/cli/src/commands/scan.rs` | Add `--include-unmanaged` and `--exclude-path` flags to `ScanArgs`; pass config to pipeline; prompt before bundling; bundle files into render dir |
| `crates/core/src/types/nonrpm.rs` | Add `UnmanagedFile` struct, `UnmanagedFileSection` struct, `FileType` enum, `ProvenanceSignals` struct |
| `crates/core/src/snapshot.rs` | Add `unmanaged_files: Option<UnmanagedFileSection>` field to `InspectionSnapshot` |
| `crates/refine/src/types.rs` | Add `ItemId::UnmanagedFile` variant |
| `crates/collect/src/inspectors/nonrpm.rs` | Add `scan_unmanaged_files()` function, `scan_dnf_cache_for_repoless()` function |
| `crates/pipeline/src/render/containerfile.rs` | Add calls to new unmanaged and repoless renderers in `render_containerfile_inner()` |
| `crates/pipeline/src/render/mod.rs` | Add `pub mod unmanaged;` and `pub mod repoless;` declarations |
| `crates/refine/src/session.rs` | Add `unmanaged` and `repoless-packages` to export allowlist; add materialization functions |
| `crates/refine/src/classify.rs` | Add classification for repo-less RPMs (pre-excluded) and unmanaged files |
| `crates/refine/tests/export_contract_test.rs` | Add contract tests for `unmanaged/` and `repoless-packages/` roots |

### New files

| File | Responsibility |
|------|---------------|
| `crates/pipeline/src/render/unmanaged.rs` | Containerfile rendering for unmanaged file COPY directives |
| `crates/pipeline/src/render/repoless.rs` | Containerfile rendering for repo-less RPM `dnf localinstall` directives |
| `crates/collect/tests/unmanaged_scan_test.rs` | Integration tests for unmanaged file cataloging |
| `crates/collect/tests/repoless_rpm_test.rs` | Integration tests for dnf cache scanning |

---

### Task 1: Data Model — UnmanagedFile Types + ItemId

**Files:**
- Modify: `crates/core/src/types/nonrpm.rs`
- Modify: `crates/core/src/snapshot.rs`
- Modify: `crates/refine/src/types.rs`
- Test: existing roundtrip tests, new tests in `nonrpm.rs`

**Interfaces:**
- Produces: `UnmanagedFile`, `UnmanagedFileSection`, `FileType`, `ProvenanceSignals`, `ItemId::UnmanagedFile`
- Consumed by: Tasks 2-9

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
    /// Whether the path is under a writable mount
    #[serde(default)]
    pub writable_mount: bool,
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
    /// Provenance signals for review
    #[serde(default)]
    pub provenance: ProvenanceSignals,
    /// Include in export (default true — user toggles in refine)
    #[serde(default = "crate::default_true")]
    pub include: bool,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub acknowledged: bool,
    /// True if path is under /var (needs bootc persistence warning)
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

- [ ] **Step 7: Write roundtrip test**

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
            writable_mount: false,
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
}
```

- [ ] **Step 8: Run tests**

Run: `cargo test -p inspectah-core -p inspectah-refine`
Expected: all pass, zero clippy warnings.

- [ ] **Step 9: Commit**

```
feat(core): add unmanaged file data model and ItemId variant

Add UnmanagedFile, UnmanagedFileSection, FileType, ProvenanceSignals
types. Add unmanaged_files field to InspectionSnapshot. Add
ItemId::UnmanagedFile variant for refine toggle operations. All new
fields use serde(default) for backward compatibility.
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
#[derive(Parser)]
#[command(name = "inspectah", version = LONG_VERSION, about)]
struct Cli {
    /// Print full CLI reference in markdown format
    #[arg(long, hide = true)]
    markdown_help: bool,

    /// Assume yes to all interactive prompts (for CI/automation)
    #[arg(short = 'y', long = "yes", global = true)]
    pub yes: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}
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

In the scan command's `run()` function (or wherever ScanArgs is consumed),
the `yes` flag comes from the parent `Cli` struct. Ensure the scan
function receives both `args: ScanArgs` and `yes: bool` (or accesses
`cli.yes` from the call site in `main.rs`).

In `crates/cli/src/main.rs`, at the `Commands::Scan(args)` match arm,
pass `cli.yes` to the scan runner:

```rust
Commands::Scan(args) => commands::scan::run(args, cli.yes),
```

Update the `run` function signature in `scan.rs`:

```rust
pub fn run(args: ScanArgs, assume_yes: bool) -> Result<()> {
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p inspectah-cli`
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
- Consumes: `Executor.list_dir()`, `Executor.file_metadata()`, `Executor.read_file()` for file classification; `NonRpmSoftwareSection.items` to exclude Tier 1 language environments
- Produces: `UnmanagedFileSection` with cataloged files, provenance signals, and Tier 1 exclusion

- [ ] **Step 1: Write failing test for unmanaged file scan**

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
fn scan_unmanaged_detects_var_paths() {
    // MockExecutor with /var/lib/myapp/data.db.
    // Assert: UnmanagedFile.under_var == true.
}

#[test]
fn scan_unmanaged_classifies_scripts() {
    // MockExecutor with /opt/app/run.sh containing "#!/bin/bash".
    // Assert: file_type == FileType::Script.
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p inspectah-collect --test unmanaged_scan_test`
Expected: FAIL — function does not exist.

- [ ] **Step 3: Implement scan_unmanaged_files()**

In `crates/collect/src/inspectors/nonrpm.rs`, add:

```rust
use inspectah_core::types::nonrpm::{
    FileType, ProvenanceSignals, UnmanagedFile, UnmanagedFileSection,
};
use std::os::unix::fs::MetadataExt;

/// Directories to scan for unmanaged files.
const UNMANAGED_SCAN_ROOTS: &[&str] = &["/opt", "/srv", "/usr/local", "/var"];

/// Scan for unmanaged files not claimed by RPM or Tier 1 language packages.
pub fn scan_unmanaged_files(
    exec: &dyn Executor,
    language_env_paths: &[String],
    exclude_paths: &[String],
) -> UnmanagedFileSection {
    let mut items = Vec::new();
    let mut total_size: u64 = 0;

    for root in UNMANAGED_SCAN_ROOTS {
        walk_for_unmanaged(
            exec,
            root,
            language_env_paths,
            exclude_paths,
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
    language_env_paths: &[String],
    exclude_paths: &[String],
    items: &mut Vec<UnmanagedFile>,
    total_size: &mut u64,
) {
    // Use find command to list all regular files under root
    let args = vec![root.to_string(), "-type".into(), "f".into()];
    let result = match exec.execute("find", &args.iter().map(|s| s.as_str()).collect::<Vec<_>>()) {
        Ok(r) if r.exit_code == 0 => r,
        _ => return,
    };

    for line in result.stdout.lines() {
        let path = line.trim();
        if path.is_empty() {
            continue;
        }

        // Apply exclude-path filters
        if exclude_paths.iter().any(|ep| path.starts_with(ep)) {
            continue;
        }

        // Exclude Tier 1 language environment paths (no double-counting)
        if language_env_paths.iter().any(|lp| path.starts_with(lp)) {
            continue;
        }

        // Get file metadata via stat
        let (size, last_modified, uid, gid, permissions) =
            get_file_metadata(exec, path);

        let file_type = classify_file_type(exec, path);
        let under_var = path.starts_with("/var/");

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
                writable_mount: false, // Conservative default
            },
            include: true,
            under_var,
            ..Default::default()
        });
    }
}

/// Classify a file's type by reading its magic bytes / shebang.
fn classify_file_type(exec: &dyn Executor, path: &str) -> FileType {
    // Use `file -b` for classification
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

- [ ] **Step 4: Wire scan_unmanaged_files into the inspector**

In the `NonRpmInspector::inspect()` method, after scanning language
packages, conditionally scan for unmanaged files. The inspector needs
a signal for whether `--include-unmanaged` was requested. Add a field
to the inspector context or pass it via the snapshot's `meta` map.

Option: store the flag in `InspectionSnapshot.meta` under key
`"include_unmanaged"` (set by the CLI before calling the pipeline).
In the inspector:

```rust
let include_unmanaged = ctx
    .snapshot_meta
    .as_ref()
    .and_then(|m| m.get("include_unmanaged"))
    .and_then(|v| v.as_bool())
    .unwrap_or(false);

if include_unmanaged {
    let language_paths: Vec<String> = section
        .items
        .iter()
        .filter(|i| is_language_env(i))
        .map(|i| format!("/{}", i.path))
        .collect();
    let exclude_paths: Vec<String> = ctx
        .snapshot_meta
        .as_ref()
        .and_then(|m| m.get("exclude_paths"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    // Return unmanaged section separately — caller stores on snapshot
}
```

The exact wiring depends on how the inspector returns data. If the
inspector only returns `NonRpmSoftwareSection`, add the unmanaged
section to the snapshot in the pipeline orchestrator after the
inspector runs.

- [ ] **Step 5: Run tests**

Run: `cargo test -p inspectah-collect`
Expected: all pass including new unmanaged scan tests.

- [ ] **Step 6: Commit**

```
feat(collect): catalog unmanaged files from /opt, /srv, /usr/local, /var

Scan for files not owned by RPM or Tier 1 language packages.
Classify file types (ELF, JAR, script, config, data, symlink),
collect provenance signals (size, mtime, uid, gid, permissions),
flag /var paths for bootc persistence warning. Respects
--exclude-path filters and Tier 1 exclusion to avoid double-counting.
```

**Thorn checkpoint: review Tasks 1-3 before proceeding.**

---

### Task 4: Size Prompt + Bundling at Scan Time

**Files:**
- Modify: `crates/cli/src/commands/scan.rs`

**Interfaces:**
- Consumes: `UnmanagedFileSection` (from Task 3), `ScanArgs.include_unmanaged`, `assume_yes: bool`
- Produces: Unmanaged files copied into the render directory under `unmanaged/`

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
        for root in &["/opt", "/srv", "/usr/local", "/var"] {
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

- [ ] **Step 4: Run tests**

Run: `cargo test -p inspectah-cli`
Expected: all pass. Manual test: verify `--include-unmanaged` prompts
and `--yes` suppresses.

- [ ] **Step 5: Commit**

```
feat(cli): prompt and bundle unmanaged files at scan time

Display file count and total size, prompt for confirmation before
bundling. -y/--yes suppresses the prompt. Files copied into render
directory under unmanaged/ preserving directory structure for
tarball inclusion.
```

---

### Task 5: Repo-less RPM Detection + dnf Cache Scanning

**Files:**
- Modify: `crates/collect/src/inspectors/nonrpm.rs` (or new function in rpm inspector area)
- Create: `crates/collect/tests/repoless_rpm_test.rs`

**Interfaces:**
- Consumes: `RpmSection.packages` with `source_repo == ""`, `Executor.execute()` for dnf cache listing
- Produces: RPM entries annotated with `repoless_cached: bool`, cached RPM files bundled into render dir under `repoless-packages/`

- [ ] **Step 1: Write failing test for dnf cache scan**

Create `crates/collect/tests/repoless_rpm_test.rs`:

```rust
#[test]
fn repoless_rpm_found_in_cache() {
    // MockExecutor with:
    //   PackageEntry { name: "custom-tool", source_repo: "", arch: "x86_64",
    //                  version: "1.2.3", release: "1.el9" }
    //   dnf cache listing includes custom-tool-1.2.3-1.el9.x86_64.rpm
    // Assert: RPM is flagged as repoless_cached = true.
}

#[test]
fn repoless_rpm_not_in_cache() {
    // Same PackageEntry but dnf cache listing is empty.
    // Assert: RPM is flagged as repoless_cached = false.
    // Assert: method annotation says "manual resolution needed".
}

#[test]
fn rpm_with_source_repo_not_flagged() {
    // PackageEntry with source_repo = "appstream".
    // Assert: not treated as repo-less.
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p inspectah-collect --test repoless_rpm_test`
Expected: FAIL

- [ ] **Step 3: Implement scan_dnf_cache_for_repoless()**

In `crates/collect/src/inspectors/nonrpm.rs` (or a new module — the
function operates on RPM data but is part of the non-RPM replication
feature), add:

```rust
use inspectah_core::types::rpm::PackageEntry;

/// Metadata for a repo-less RPM package.
pub struct RepolessRpm {
    /// NEVRA of the package
    pub nevra: String,
    /// Package name
    pub name: String,
    /// Package architecture
    pub arch: String,
    /// True if the RPM file was found in /var/cache/dnf/
    pub cached: bool,
    /// Filename of the cached RPM (if found)
    pub cache_filename: Option<String>,
    /// Full path to the cached RPM (if found)
    pub cache_path: Option<String>,
}

/// Scan /var/cache/dnf/ for cached RPMs matching repo-less packages.
pub fn scan_dnf_cache_for_repoless(
    exec: &dyn Executor,
    packages: &[PackageEntry],
) -> Vec<RepolessRpm> {
    let repoless: Vec<&PackageEntry> = packages
        .iter()
        .filter(|p| p.source_repo.is_empty())
        .collect();

    if repoless.is_empty() {
        return Vec::new();
    }

    // List all .rpm files in the dnf cache
    let cache_result = exec.execute(
        "find",
        &["/var/cache/dnf", "-name", "*.rpm", "-type", "f"],
    );
    let cache_files: Vec<String> = match cache_result {
        Ok(r) if r.exit_code == 0 => {
            r.stdout.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()
        }
        _ => Vec::new(),
    };

    repoless
        .iter()
        .map(|pkg| {
            let nevra = format!(
                "{}-{}-{}.{}",
                pkg.name, pkg.version, pkg.release, pkg.arch
            );
            let expected_filename = format!("{nevra}.rpm");

            // Check if cached RPM matches
            let cache_match = cache_files.iter().find(|f| {
                f.ends_with(&expected_filename)
                    || f.contains(&format!("/{expected_filename}"))
            });

            RepolessRpm {
                nevra: nevra.clone(),
                name: pkg.name.clone(),
                arch: pkg.arch.clone(),
                cached: cache_match.is_some(),
                cache_filename: cache_match.map(|f| {
                    std::path::Path::new(f)
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string()
                }),
                cache_path: cache_match.cloned(),
            }
        })
        .collect()
}
```

- [ ] **Step 4: Bundle cached RPMs into render directory**

In the scan command (same area as Task 4's bundling), after the pipeline
runs:

```rust
fn bundle_repoless_rpms(
    repoless: &[RepolessRpm],
    render_dir: &Path,
) -> Result<()> {
    let dest_dir = render_dir.join("repoless-packages");
    for rpm in repoless {
        if let Some(ref cache_path) = rpm.cache_path {
            std::fs::create_dir_all(&dest_dir)
                .context("failed to create repoless-packages dir")?;
            let filename = rpm.cache_filename.as_deref().unwrap_or(&rpm.nevra);
            let dest = dest_dir.join(filename);
            std::fs::copy(cache_path, &dest)
                .context(format!("failed to copy cached RPM {cache_path}"))?;
        }
    }
    Ok(())
}
```

- [ ] **Step 5: Store repoless metadata on snapshot**

Add triage annotation metadata to the `PackageEntry` or snapshot `meta`
so the refine UI can distinguish repo-less RPMs:

In `crates/core/src/types/rpm.rs`, add to `PackageEntry`:

```rust
    /// Triage annotation for repo-less packages
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub repoless_annotation: String,

    /// True if cached RPM was found in /var/cache/dnf/
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub repoless_cached: bool,
```

Set these during the scan:

```rust
for rpm in &repoless_results {
    if let Some(pkg) = packages.iter_mut().find(|p| {
        p.name == rpm.name && p.arch == rpm.arch && p.source_repo.is_empty()
    }) {
        pkg.repoless_cached = rpm.cached;
        pkg.repoless_annotation = if rpm.cached {
            "No repo source — cached RPM bundled (pre-excluded, no GPG verification)".into()
        } else {
            "No repo source — manual resolution needed".into()
        };
    }
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test -p inspectah-collect -p inspectah-core`
Expected: all pass.

- [ ] **Step 7: Commit**

```
feat(collect): scan dnf cache for repo-less RPM packages

For packages with no source_repo, check /var/cache/dnf/ for cached
.rpm files. Found RPMs are bundled under repoless-packages/ in the
tarball. Missing RPMs get "manual resolution needed" annotation.
Triage annotation and repoless_cached fields added to PackageEntry.
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
/// Includes warning block per spec. /var paths get extra persistence note.
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
        // Check if all files in this group share the same parent —
        // if so, use a single directory COPY
        let rel_dir = dir.trim_start_matches('/');
        if files.len() > 1 {
            // Directory-level COPY
            let has_var = dir.starts_with("/var");
            if has_var {
                lines.push(format!(
                    "# NOTE: /var is persistent — files under this path can drift from the image after boot."
                ));
            }
            lines.push(format!(
                "COPY unmanaged/{rel_dir}/ /{rel_dir}/"
            ));
        } else {
            // Single file COPY
            for file in files {
                let rel_path = file.path.trim_start_matches('/');
                if file.under_var {
                    lines.push(format!(
                        "# NOTE: /var is persistent — this file can drift from the image after boot."
                    ));
                }
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
        FileType, ProvenanceSignals, UnmanagedFile, UnmanagedFileSection,
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
    fn var_path_gets_persistence_note() {
        let snap = test_snapshot_with_unmanaged(vec![
            UnmanagedFile {
                path: "/var/lib/myapp/data.db".into(),
                size: 512,
                file_type: FileType::DataFile,
                include: true,
                under_var: true,
                ..Default::default()
            },
        ]);
        let lines = unmanaged_file_lines(&snap);
        assert!(lines.iter().any(|l| l.contains("/var is persistent")));
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
        // Should use directory-level COPY for /opt/splunk/bin/
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

- [ ] **Step 4: Run tests**

Run: `cargo test -p inspectah-pipeline`
Expected: all pass.

- [ ] **Step 5: Commit**

```
feat(pipeline): add unmanaged file Containerfile rendering

Render COPY directives for unmanaged files with warning block.
Files grouped by parent directory. /var paths get persistence
warning per bootc guidance. Excluded files are not rendered.
```

**Thorn checkpoint: review Tasks 4-6 before proceeding.**

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
        .filter(|p| p.source_repo.is_empty() && !p.repoless_annotation.is_empty())
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

- [ ] **Step 4: Run tests**

Run: `cargo test -p inspectah-pipeline`
Expected: all pass.

- [ ] **Step 5: Commit**

```
feat(pipeline): add repo-less RPM Containerfile rendering

Render dnf localinstall directives for cached repo-less RPMs.
Pre-excluded packages render as commented-out with provenance
warnings. Missing RPMs get MANUAL comment blocks with fallback
dnf install suggestion. Uses dnf localinstall (not rpm -i) to
preserve dependency resolution.
```

---

### Task 8: Export Contract — unmanaged/ and repoless-packages/ Roots

**Files:**
- Modify: `crates/refine/src/session.rs` (export allowlist + materialization)
- Modify: `crates/refine/tests/export_contract_test.rs`

**Interfaces:**
- Consumes: `InspectionSnapshot.unmanaged_files`, `RepolessRpm` data
- Produces: `unmanaged/` and `repoless-packages/` directories in export tarball

- [ ] **Step 1: Add roots to export allowlist**

In `crates/refine/src/session.rs`, find the `allowed_top_level` HashSet
(currently contains `"config"`, `"drop-ins"`, `"flatpak"`, etc.). Add:

```rust
"unmanaged",
"repoless-packages",
```

Note: Plan 1 adds `"language-packages"` — these entries are additive.

- [ ] **Step 2: Add unmanaged file materialization to export**

In the export function (near the language-packages materialization from
Plan 1), add:

```rust
write_unmanaged_files(snap, out)?;
```

Implement:

```rust
fn write_unmanaged_files(
    snap: &InspectionSnapshot,
    out: &Path,
) -> Result<(), RefineError> {
    let section = match &snap.unmanaged_files {
        Some(s) => s,
        None => return Ok(()),
    };

    for file in &section.items {
        if !file.include {
            continue;
        }
        let rel_path = file.path.trim_start_matches('/');
        let dest = out.join("unmanaged").join(rel_path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| RefineError::ExportFailed(
                    format!("mkdir {}: {e}", parent.display()),
                ))?;
        }
        // The actual file content was bundled into the tarball at scan time.
        // During export from refine, we copy from the loaded tarball's
        // unmanaged/ directory. The file should already exist in the
        // session's working directory if the tarball was extracted.
        let source = out.join("unmanaged").join(rel_path);
        if !source.exists() {
            // File was in the original tarball but may need re-extraction.
            // Log a warning rather than failing — the file is only available
            // if the original tarball's unmanaged/ was preserved.
            continue;
        }
    }
    Ok(())
}
```

Note: The unmanaged files are bundled at scan time into the tarball. At
refine export time, the tarball has already been extracted, so the files
exist under `unmanaged/` in the working directory. The allowlist ensures
they survive the cleanup pass. Files with `include: false` are removed
by the cleanup pass (they're not in the allowlist and we don't
re-materialize them).

The actual implementation may need to selectively remove excluded files:

```rust
fn prune_excluded_unmanaged(
    snap: &InspectionSnapshot,
    out: &Path,
) -> Result<(), RefineError> {
    let section = match &snap.unmanaged_files {
        Some(s) => s,
        None => return Ok(()),
    };

    let unmanaged_dir = out.join("unmanaged");
    if !unmanaged_dir.exists() {
        return Ok(());
    }

    for file in &section.items {
        if !file.include {
            let rel_path = file.path.trim_start_matches('/');
            let file_path = unmanaged_dir.join(rel_path);
            if file_path.exists() {
                std::fs::remove_file(&file_path).ok();
            }
        }
    }

    // Clean up empty directories
    remove_empty_dirs(&unmanaged_dir).ok();
    Ok(())
}
```

- [ ] **Step 3: Write export contract tests**

In `crates/refine/tests/export_contract_test.rs`, add:

```rust
#[test]
fn export_allowlist_includes_unmanaged_root() {
    // Build a snapshot with unmanaged files.
    // Create unmanaged/opt/app/server in the export dir.
    // Run the export cleanup pass.
    // Assert: unmanaged/ directory survives cleanup.
}

#[test]
fn export_prunes_excluded_unmanaged_files() {
    // Build a snapshot with two unmanaged files, one include:false.
    // Create both in unmanaged/ dir.
    // Run export.
    // Assert: included file present, excluded file removed.
}

#[test]
fn export_allowlist_includes_repoless_packages_root() {
    // Build a snapshot with repo-less RPM data.
    // Create repoless-packages/custom-tool-1.2.3.x86_64.rpm in export dir.
    // Run export cleanup.
    // Assert: repoless-packages/ survives cleanup.
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p inspectah-refine`
Expected: all pass.

- [ ] **Step 5: Commit**

```
feat(refine): add unmanaged/ and repoless-packages/ export roots

Extend export allowlist with unmanaged and repoless-packages
directories. Excluded unmanaged files are pruned from the export.
Export contract tests verify presence and exclusion rules for
both roots.
```

---

### Task 9: Refine Classification — Pre-exclude Repo-less RPMs

**Files:**
- Modify: `crates/refine/src/classify.rs`

**Interfaces:**
- Consumes: `PackageEntry.source_repo`, `PackageEntry.repoless_cached`
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
    //                  include: true }
    // Run classify_packages.
    // Assert: httpd.include == true (normal package).
    // Assert: custom-tool.include == false (pre-excluded by provenance gate).
}
```

- [ ] **Step 2: Run test to verify it fails**

Expected: FAIL — classifier doesn't know about repo-less.

- [ ] **Step 3: Implement repo-less pre-exclusion**

In `crates/refine/src/classify.rs`, in `classify_packages()` (or
the relevant classification function), add a rule:

```rust
// Repo-less RPMs are pre-excluded — user must explicitly include.
// This is the provenance trust gate per spec.
for pkg in &mut packages {
    if pkg.source_repo.is_empty() && !pkg.repoless_annotation.is_empty() {
        pkg.include = false;
    }
}
```

This runs after normal classification so it overrides the default
`include: true` on these packages.

- [ ] **Step 4: Run tests**

Run: `cargo test -p inspectah-refine`
Expected: all pass.

- [ ] **Step 5: Commit**

```
feat(refine): pre-exclude repo-less RPMs by default

Repo-less packages (empty source_repo with repoless annotation)
are set to include: false during classification. This is the
provenance trust gate — users must explicitly include packages
with no upstream repo verification.
```

**Thorn checkpoint: review Tasks 7-9 before proceeding to Plan 3.**

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
                     └──→ Task 8 (export contract) ──────────┘
                                                             │
                                                     Task 9 (classify)
```

Tasks 1-2 can run in parallel. Tasks 3-5 depend on Task 1. Tasks 6-7 depend on Tasks 3-5. Task 8 depends on Tasks 6-7. Task 9 depends on Task 5.

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
