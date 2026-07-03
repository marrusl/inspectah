# inspectah Extended Findings Companion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add advisory finding type, section grouping, /usr walk detection, cross-tree symlink advisory, tmpfiles.d backing advisory, modernization advisory system, and systemd shadow detection to inspectah.

**Architecture:** Changes span 6 crates: core (types + enums), collect (detection), pipeline (HTML templates + audit report), refine (session + group_state), web (handlers + aggregate_handlers), tui (widgets + sections). The core type changes are the foundation — everything else depends on them. Section grouping is a **derived presentation concern** — the `SectionGroup` enum lives in the rendering layers (pipeline, web, tui), NOT in core.

**Tech Stack:** Rust (serde, axum, ratatui, tera templates), PatternFly 6 (HTML report), HTML/JS (refine view)

**Spec:** `../driftify/docs/specs/driftify-extended-findings-design.md` (approved R4 — EL8 target mapping + networking-as-inventory)

## Global Constraints

- `cargo clippy -- -W clippy::all` with zero warnings. `cargo fmt --check` passes.
- No `unsafe` outside FFI boundaries.
- Schema version bumps to next integer (currently 20 → 21).
- Exact-match schema gating preserved (MIN_SCHEMA == SCHEMA_VERSION).
- No backwards compatibility with pre-21 snapshots required.
- All advisory rendering must meet WCAG 2.2 AA: visible focus rings, ARIA labels on interactive elements, keyboard tab order matches visual order.
- The `build_rpm_owned_set()` helper is extracted to a shared module in the collect crate, reusable by both the /usr walk (Task 2) and /var backing detection (Task 3).
- Unbacked /var dirs get DUAL treatment: actionable Containerfile output (`RUN mkdir -p`) AND a single grouped advisory per scan listing all unbacked dirs. Advisories alone do NOT generate Containerfile output; the actionable entries do.
- EL8 scans target RHEL 9+ base images. Default: `registry.redhat.io/rhel9/rhel-bootc:latest`. CentOS 8 → `quay.io/centos-bootc/centos-bootc:stream9`. See spec §6.5 for full mapping.
- Networking config (ifcfg, keyfiles, NM profiles) is NOT included in the Containerfile. It is shown as informational inventory in the network section of the report. See spec §6.6.
- ifcfg is NOT a modernization advisory. It is network inventory with a contextual note about format deprecation on the target platform.
- Commit format: `feat(inspectah): <description>` with `Assisted-by: Claude Code (<model>)`.

---

### Task 1: FindingKind Enum + Schema Foundation (core crate)

**Files:**
- Create: `crates/core/src/types/finding.rs` — FindingKind, AdvisoryType enums
- Modify: `crates/core/src/types/mod.rs` — add `pub mod finding;`
- Modify: All 10 type files in `crates/core/src/types/` — replace `pub include: bool` with `pub disposition: FindingKind`
- Modify: `crates/core/src/types/services.rs` — add `pub shadow_type: Option<ShadowType>` (spec name, not `override_type`)
- Modify: `crates/core/src/snapshot.rs` — bump `SCHEMA_VERSION` to 21
- Modify: `crates/web/src/aggregate_handlers.rs` — update for FindingKind
- Modify: `crates/refine/src/aggregate/*.rs` — update aggregate consumers for FindingKind
- Test: `cargo test --workspace`

**Interfaces:**
- Produces: `FindingKind`, `AdvisoryType`, `ShadowType` enums used by all downstream tasks

- [ ] **Step 1: Create `finding.rs` with the core enums**

Create `crates/core/src/types/finding.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FindingKind {
    Actionable { include: bool },
    Advisory {
        advisory_type: AdvisoryType,
        rationale: String,
    },
}

impl FindingKind {
    pub fn included() -> Self {
        Self::Actionable { include: true }
    }

    pub fn excluded() -> Self {
        Self::Actionable { include: false }
    }

    pub fn advisory(advisory_type: AdvisoryType, rationale: impl Into<String>) -> Self {
        Self::Advisory {
            advisory_type,
            rationale: rationale.into(),
        }
    }

    pub fn is_included(&self) -> bool {
        matches!(self, Self::Actionable { include: true })
    }

    pub fn is_advisory(&self) -> bool {
        matches!(self, Self::Advisory { .. })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AdvisoryType {
    UnbackedVarDir,
    CrossTreeSymlink,
    Modernization,
}

/// Spec field name: `shadow_type` (not `override_type`)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ShadowType {
    DropIn,
    FullShadow,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_finding_kind_serde_roundtrip_actionable() {
        let kind = FindingKind::included();
        let json = serde_json::to_string(&kind).unwrap();
        let parsed: FindingKind = serde_json::from_str(&json).unwrap();
        assert_eq!(kind, parsed);
        assert!(parsed.is_included());
        assert!(!parsed.is_advisory());
    }

    #[test]
    fn test_finding_kind_serde_roundtrip_advisory() {
        let kind = FindingKind::advisory(
            AdvisoryType::UnbackedVarDir,
            "No tmpfiles.d backing",
        );
        let json = serde_json::to_string(&kind).unwrap();
        let parsed: FindingKind = serde_json::from_str(&json).unwrap();
        assert_eq!(kind, parsed);
        assert!(!parsed.is_included());
        assert!(parsed.is_advisory());
    }

    #[test]
    fn test_advisory_json_shape() {
        let kind = FindingKind::advisory(
            AdvisoryType::Modernization,
            "xinetd is deprecated",
        );
        let json = serde_json::to_string(&kind).unwrap();
        assert!(json.contains(r#""kind":"advisory"#));
        assert!(json.contains(r#""advisory_type":"modernization"#));
        assert!(json.contains(r#""rationale":"xinetd is deprecated"#));
    }
}
```

- [ ] **Step 2: Register module in `types/mod.rs`**

```rust
pub mod finding;
pub use finding::{AdvisoryType, FindingKind, ShadowType};
```

- [ ] **Step 3: Replace `include: bool` across all type files**

For each struct with `pub include: bool`, replace with `pub disposition: FindingKind`. ~26 fields across 10 files. For each file:

1. Add `use crate::types::finding::FindingKind;` to imports
2. Replace `pub include: bool` with `pub disposition: FindingKind`
3. Update `Default` impls to use `FindingKind::included()`
4. Update test fixtures that set `include: true/false`

In `services.rs`, add to the appropriate service struct (use spec field name `shadow_type`):

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub shadow_type: Option<ShadowType>,
```

In `kernelboot.rs`, replace `pub tuned_include: bool` with `pub tuned_disposition: FindingKind`.

- [ ] **Step 4: Bump schema version**

In `crates/core/src/snapshot.rs`:

```rust
pub const SCHEMA_VERSION: u32 = 21;
```

Update rejection tests for old (20) and future (22) versions.

- [ ] **Step 5: Fix all compilation errors across ALL crates including aggregate consumers**

Run `cargo build 2>&1` and fix every reference to the old `include` field. Common patterns:

- `item.include = true` → `item.disposition = FindingKind::included()`
- `item.include = false` → `item.disposition = FindingKind::excluded()`
- `if item.include` → `if item.disposition.is_included()`

**Explicit consumer list (all must be updated):**
- `crates/collect/src/inspectors/*.rs` — all inspector modules
- `crates/refine/src/session.rs` — Containerfile generation
- `crates/refine/src/projection/*.rs` — projection logic
- `crates/refine/src/aggregate/*.rs` — fleet aggregate consumers
- `crates/web/src/handlers.rs` — single-host refine handlers
- `crates/web/src/adapter.rs` — web adapter
- `crates/web/src/aggregate_handlers.rs` — fleet aggregate handlers
- `crates/tui/src/sections.rs` — TUI section rendering
- `crates/tui/src/widget/triage_list.rs` — TUI toggle logic
- `crates/pipeline/src/*.rs` — HTML report + audit report rendering

- [ ] **Step 6: Run tests**

Run: `cargo test --workspace`
Run: `cargo clippy -- -W clippy::all`
Run: `cargo fmt --check`

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat(inspectah): replace include:bool with FindingKind enum (schema v21)"
```

---

### Task 2: Shared RPM-Owned Set Builder + /usr Walk (collect crate)

**Files:**
- Create: `crates/collect/src/rpm_ownership.rs` — shared `build_rpm_owned_set()` helper
- Modify: `crates/collect/src/lib.rs` — register module
- Modify: `crates/collect/src/inspectors/nonrpm.rs` — add /usr walk logic
- Modify: `crates/core/src/types/nonrpm.rs` — add collapsed directory entry type
- Test: unit tests for ancestor collapse, symlink-own-path rule, prune negatives

**Interfaces:**
- Consumes: `FindingKind::included()` from Task 1
- Produces: `build_rpm_owned_set()` helper (reused by Task 3), `UnmanagedUsrEntry` structs

- [ ] **Step 1: Create shared RPM ownership module**

Create `crates/collect/src/rpm_ownership.rs`:

```rust
use std::collections::HashSet;
use crate::executor::Executor;

pub fn build_rpm_owned_set(exec: &dyn Executor) -> HashSet<String> {
    let result = exec.run("rpm", &["--query", "--all", "--dump"]);
    let mut owned = HashSet::new();
    if result.exit_code != 0 {
        return owned;
    }
    for line in result.stdout.lines() {
        if let Some(path) = line.split_whitespace().next() {
            let normalized = normalize_path(path);
            if !normalized.is_empty() {
                owned.insert(normalized);
            }
        }
    }
    owned
}

fn normalize_path(path: &str) -> String {
    path.trim_end_matches('/').replace("//", "/")
}
```

Register in `crates/collect/src/lib.rs`: `pub mod rpm_ownership;`

- [ ] **Step 2: Add collapsed directory entry type with full output contract**

In `crates/core/src/types/nonrpm.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UnmanagedUsrEntry {
    pub path: String,
    pub file_count: u32,
    pub total_size_bytes: u64,
    pub file_type: FileType,
    pub disposition: FindingKind,
}
```

Add `pub usr_entries: Vec<UnmanagedUsrEntry>` to `UnmanagedFileSection`.

Note: no `children` field — the collapsed entry reports the directory path, count, and size. Individual file details are not persisted (noise reduction per spec §6.2).

- [ ] **Step 3: Define prune list with full spec coverage**

```rust
const USR_PRUNE_DIRS: &[&str] = &[
    "/usr/share/doc/",
    "/usr/share/man/",
    "/usr/share/locale/",
    "/usr/share/info/",
    "/usr/share/licenses/",
    "/usr/share/icons/",
    "/usr/share/pixmaps/",
    "/usr/share/fonts/",
    "/usr/share/mime/",
    "/usr/share/zoneinfo/",
    "/usr/lib/.build-id/",
];

const USR_PRUNE_FILE_PATTERNS: &[&str] = &[
    ".pyc",
    "__pycache__",
    ".cache",
    ".fontconfig",
    "fonts.cache",
    "ld.so.cache",
];

fn is_pruned_file(path: &str) -> bool {
    USR_PRUNE_FILE_PATTERNS.iter().any(|pat| path.ends_with(pat) || path.contains(pat))
}
```

- [ ] **Step 4: Implement /usr walk with ancestor collapse, size computation, and FileType**

**Traversal method:** Use `walkdir` crate (already a workspace dependency) for the `/usr` walk, not shell `find`. The spec (§3.3) names `walkdir` as the traversal mechanism. `walkdir` gives us native `DirEntry` metadata (file size, symlink detection) without per-file `stat` calls, and avoids shell argument-length limits.

```rust
fn walk_usr_for_unmanaged(
    exec: &dyn Executor,
    rpm_owned: &HashSet<String>,
) -> Vec<UnmanagedUsrEntry> {
    use walkdir::WalkDir;

    let mut unmanaged: Vec<(String, u64)> = Vec::new();

    for entry in WalkDir::new("/usr")
        .follow_links(false)  // check symlink's own path, not target
        .into_iter()
        .filter_entry(|e| {
            let path = e.path().to_string_lossy();
            // Prune directories from the walk itself
            !USR_PRUNE_DIRS.iter().any(|p| path.starts_with(p))
        })
    {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if entry.file_type().is_dir() { continue; }

        let path = entry.path().to_string_lossy();
        let normalized = path.trim_end_matches('/').replace("//", "/");

        if is_pruned_file(&normalized) { continue; }

        // Check ONLY the symlink's own path, not resolved target
        if !rpm_owned.contains(&normalized) {
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            unmanaged.push((normalized, size));
        }
    }

    collapse_to_ancestors(&unmanaged, rpm_owned)
}

fn collapse_to_ancestors(
    unmanaged_files: &[(String, u64)],
    rpm_owned: &HashSet<String>,
) -> Vec<UnmanagedUsrEntry> {
    use std::collections::BTreeMap;

    let mut dir_groups: BTreeMap<String, (Vec<String>, u64)> = BTreeMap::new();

    for (file_path, size) in unmanaged_files {
        let ancestor = find_shallowest_unowned_ancestor(file_path, rpm_owned);
        let entry = dir_groups.entry(ancestor).or_insert_with(|| (Vec::new(), 0));
        entry.0.push(file_path.clone());
        entry.1 += size;
    }

    dir_groups.into_iter().map(|(ancestor, (children, total_size))| {
        let file_type = if children.len() == 1 && children[0] == ancestor {
            // Single file directly under an RPM-owned parent
            classify_file_type(&children[0])
        } else {
            // Directory grouping
            FileType::Other
        };

        UnmanagedUsrEntry {
            path: ancestor,
            file_count: children.len() as u32,
            total_size_bytes: total_size,
            file_type,
            disposition: FindingKind::included(),
        }
    }).collect()
}

fn classify_file_type(path: &str) -> FileType {
    if path.ends_with(".so") || path.contains(".so.") {
        FileType::ElfBinary
    } else if path.ends_with(".jar") {
        FileType::Jar
    } else {
        FileType::Script // conservative default for /usr files
    }
}

fn find_shallowest_unowned_ancestor(
    file_path: &str,
    rpm_owned: &HashSet<String>,
) -> String {
    let parts: Vec<&str> = file_path.split('/').collect();
    for i in 2..parts.len() {
        let dir = parts[..i].join("/");
        if dir.is_empty() { continue; }
        if !rpm_owned.contains(&dir) {
            return dir;
        }
    }
    file_path.to_string()
}
```

- [ ] **Step 5: Write tests including symlink-own-path and prune negatives**

```rust
#[cfg(test)]
mod usr_walk_tests {
    use super::*;

    fn owned_set(paths: &[&str]) -> HashSet<String> {
        paths.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_collapse_direct_child_of_owned() {
        let rpm = owned_set(&["/usr/bin"]);
        let result = find_shallowest_unowned_ancestor("/usr/bin/custom-tool", &rpm);
        assert_eq!(result, "/usr/bin/custom-tool");
    }

    #[test]
    fn test_collapse_unowned_subdir() {
        let rpm = owned_set(&["/usr/lib64"]);
        let result = find_shallowest_unowned_ancestor("/usr/lib64/myapp/libfoo.so", &rpm);
        assert_eq!(result, "/usr/lib64/myapp");
    }

    #[test]
    fn test_collapse_owned_intermediate() {
        let rpm = owned_set(&["/usr/lib64", "/usr/lib64/myapp"]);
        let result = find_shallowest_unowned_ancestor("/usr/lib64/myapp/custom/x.so", &rpm);
        assert_eq!(result, "/usr/lib64/myapp/custom");
    }

    #[test]
    fn test_symlink_own_path_only() {
        // /usr/bin/custom-link -> /usr/bin/bash
        // bash is RPM-owned, but custom-link is NOT
        // Must report custom-link as unmanaged
        let rpm = owned_set(&["/usr/bin", "/usr/bin/bash"]);
        let normalized = "/usr/bin/custom-link";
        assert!(!rpm.contains(normalized)); // symlink's own path not owned
    }

    #[test]
    fn test_prune_pyc() {
        assert!(is_pruned_file("/usr/lib/python3.9/__pycache__/foo.cpython-39.pyc"));
    }

    #[test]
    fn test_prune_font_cache() {
        assert!(is_pruned_file("/usr/share/fonts/.fontconfig"));
    }

    #[test]
    fn test_prune_ldcache() {
        assert!(is_pruned_file("/etc/ld.so.cache"));
    }

    #[test]
    fn test_no_prune_real_file() {
        assert!(!is_pruned_file("/usr/bin/custom-tool"));
    }
}
```

- [ ] **Step 6: Integrate into nonrpm inspector, run tests + clippy**

Call `walk_usr_for_unmanaged()` after existing unmanaged file scan. Populate `usr_entries` on `UnmanagedFileSection`.

Run: `cargo test --workspace`
Run: `cargo clippy -- -W clippy::all`

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat(inspectah): add full /usr walk with rpm-dump diff and ancestor collapse"
```

---

### Task 3: tmpfiles.d Backing Detection + /var Dual Treatment (collect crate)

**Files:**
- Modify: `crates/collect/src/inspectors/storage.rs` — add backing detection
- Modify: `crates/core/src/types/storage.rs` — add backing status + grouped advisory fields
- Test: unit tests with mock executor

**Interfaces:**
- Consumes: `build_rpm_owned_set()` from Task 2's shared module, `FindingKind`, `AdvisoryType` from Task 1
- Produces: Per-dir backing status on actionable storage entries + ONE grouped advisory listing all unbacked dirs

**Critical contract (spec §3.2):** Unbacked /var dirs get DUAL treatment:
1. Each unbacked dir remains an **Actionable finding** with `include: true` → produces `RUN mkdir -p ... && chown ...` in the Containerfile
2. A **single grouped advisory** is added to the storage section listing ALL unbacked dirs with the rationale text

These are separate entries. The advisory does NOT replace the actionable findings.

- [ ] **Step 1: Add backing status to storage types**

In `crates/core/src/types/storage.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum VarDirBacking {
    Tmpfiles,
    StateDirectory,
    CacheDirectory,
    LogsDirectory,
    RpmOwned,
    Unbacked,
}
```

Add `pub backing: Option<VarDirBacking>` to the relevant storage item struct.

Add a field for the grouped advisory on `StorageSection`:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub unbacked_var_advisory: Option<UnbackedVarAdvisory>,
```

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UnbackedVarAdvisory {
    pub disposition: FindingKind, // Always Advisory
    pub paths: Vec<String>,
}
```

- [ ] **Step 2: Implement backing detection with all three directive families**

```rust
fn detect_var_dir_backing(
    exec: &dyn Executor,
    path: &str,
    rpm_owned: &HashSet<String>,
) -> VarDirBacking {
    // Check tmpfiles.d (both /etc and /usr/lib)
    let tmpfiles_result = exec.run("grep", &[
        "-r", "--include=*.conf", "-l", path,
        "/etc/tmpfiles.d/", "/usr/lib/tmpfiles.d/",
    ]);
    if tmpfiles_result.exit_code == 0 && !tmpfiles_result.stdout.trim().is_empty() {
        return VarDirBacking::Tmpfiles;
    }

    // Check all three systemd directory directives
    let dir_name = path.rsplit('/').next().unwrap_or("");
    for (directive, backing) in [
        ("StateDirectory", VarDirBacking::StateDirectory),
        ("CacheDirectory", VarDirBacking::CacheDirectory),
        ("LogsDirectory", VarDirBacking::LogsDirectory),
    ] {
        let grep = exec.run("grep", &[
            "-r", "--include=*.service", "--include=*.socket", "-l",
            &format!("{}={}", directive, dir_name),
            "/usr/lib/systemd/system/", "/etc/systemd/system/",
        ]);
        if grep.exit_code == 0 && !grep.stdout.trim().is_empty() {
            return backing;
        }
    }

    // Check RPM ownership
    if rpm_owned.contains(path) {
        return VarDirBacking::RpmOwned;
    }

    VarDirBacking::Unbacked
}
```

- [ ] **Step 3: Populate per-dir backing AND emit grouped advisory**

After detecting backing for all /var dirs:

```rust
// Collect all unbacked paths for the grouped advisory
let unbacked_paths: Vec<String> = storage_items.iter()
    .filter(|item| item.backing == Some(VarDirBacking::Unbacked))
    .map(|item| item.path.clone())
    .collect();

// Each unbacked dir stays Actionable (produces Containerfile mkdir/chown)
// Do NOT change their disposition to Advisory

// Add ONE grouped advisory if there are any unbacked dirs
if !unbacked_paths.is_empty() {
    section.unbacked_var_advisory = Some(UnbackedVarAdvisory {
        disposition: FindingKind::advisory(
            AdvisoryType::UnbackedVarDir,
            "These /var directories have no declarative backing (tmpfiles.d, \
             StateDirectory=, CacheDirectory=, LogsDirectory=). Consider adding \
             tmpfiles.d entries for a more reproducible, declarative approach.",
        ),
        paths: unbacked_paths,
    });
}
```

- [ ] **Step 4: Write tests**

```rust
#[test]
fn test_unbacked_dir_stays_actionable() {
    // Unbacked dirs must keep Actionable disposition (for Containerfile)
    let item = create_test_storage_item("/var/lib/pgsql/data", VarDirBacking::Unbacked);
    assert!(item.disposition.is_included()); // Still actionable
}

#[test]
fn test_grouped_advisory_lists_all_unbacked() {
    // The grouped advisory should list all unbacked dirs
    let section = run_storage_inspector_with_mixed_backing(&mock_exec);
    let advisory = section.unbacked_var_advisory.unwrap();
    assert!(advisory.disposition.is_advisory());
    assert!(advisory.paths.contains(&"/var/lib/pgsql/data".to_string()));
    assert!(!advisory.paths.contains(&"/var/lib/appone/cache".to_string())); // backed by tmpfiles.d
}

#[test]
fn test_backed_dir_no_advisory() {
    let backing = detect_var_dir_backing(&mock_exec_with_tmpfiles, "/var/lib/appone/cache", &rpm);
    assert_eq!(backing, VarDirBacking::Tmpfiles);
}

#[test]
fn test_state_directory_detection() {
    let backing = detect_var_dir_backing(&mock_exec_with_state_dir, "/var/lib/myservice", &rpm);
    assert_eq!(backing, VarDirBacking::StateDirectory);
}
```

- [ ] **Step 5: Run tests + clippy, commit**

```bash
git add -A
git commit -m "feat(inspectah): add tmpfiles.d backing detection with dual /var treatment"
```

---

### Task 4: Cross-tree Symlink Advisory (collect crate)

**Files:**
- Create: `crates/core/src/types/symlink_allowlist.rs` — allowlist const
- Modify: `crates/core/src/types/mod.rs` — register module
- Modify: `crates/collect/src/inspectors/config/mod.rs` — cross-tree symlink detection
- Test: unit tests for allowlist matching including negative cases

**Interfaces:**
- Consumes: `FindingKind::advisory()`, `AdvisoryType::CrossTreeSymlink` from Task 1
- Produces: Advisory findings on cross-tree symlinks in the config section

- [ ] **Step 1: Create allowlist module**

Create `crates/core/src/types/symlink_allowlist.rs`:

```rust
pub struct AllowlistEntry {
    pub source_prefix: &'static str,
    pub target_prefix: &'static str,
}

pub const CROSS_TREE_SYMLINK_ALLOWLIST: &[AllowlistEntry] = &[
    AllowlistEntry { source_prefix: "/etc/localtime", target_prefix: "/usr/share/zoneinfo/" },
    AllowlistEntry { source_prefix: "/etc/alternatives/", target_prefix: "/usr/" },
    AllowlistEntry { source_prefix: "/etc/ssl/certs/ca-bundle.crt", target_prefix: "/etc/pki/" },
    AllowlistEntry { source_prefix: "/etc/pki/tls/cert.pem", target_prefix: "/etc/pki/" },
    AllowlistEntry { source_prefix: "/etc/crypto-policies/back-ends/", target_prefix: "/usr/share/crypto-policies/" },
    AllowlistEntry { source_prefix: "/etc/resolv.conf", target_prefix: "/run/" },
];

pub fn is_allowlisted(source: &str, resolved_target: &str) -> bool {
    CROSS_TREE_SYMLINK_ALLOWLIST.iter().any(|entry| {
        source.starts_with(entry.source_prefix)
            && resolved_target.starts_with(entry.target_prefix)
    })
}

pub fn crosses_tree_boundary(source: &str, target: &str) -> Option<&'static str> {
    if source.starts_with("/etc/") && target.starts_with("/var/") {
        Some("Symlink crosses /etc → /var: config is stateful via /var persistence, not subject to /etc 3-way merge")
    } else if source.starts_with("/etc/") && target.starts_with("/usr/") {
        Some("Symlink crosses /etc → /usr: target is in the immutable /usr layer")
    } else if source.starts_with("/opt/") && target.starts_with("/usr/") {
        Some("Symlink crosses /opt → /usr: target is in the immutable /usr layer")
    } else {
        None
    }
}
```

- [ ] **Step 2: Integrate into config inspector**

When a symlink is encountered during file walk:
1. Resolve target via `readlink -f`
2. If resolution fails (broken symlink) → always emit advisory
3. Check `is_allowlisted(source, resolved_target)` → if yes, skip
4. Check `crosses_tree_boundary(source, resolved_target)` → if Some, emit advisory

- [ ] **Step 3: Write tests including negative cases**

```rust
#[test]
fn test_allowlisted_localtime_suppressed() {
    assert!(is_allowlisted("/etc/localtime", "/usr/share/zoneinfo/UTC"));
}

#[test]
fn test_alternatives_retargeted_to_var_not_suppressed() {
    // /etc/alternatives/foo -> /var/lib/custom/foo is NOT /usr, so NOT allowlisted
    assert!(!is_allowlisted("/etc/alternatives/foo", "/var/lib/custom/foo"));
}

#[test]
fn test_app_symlink_fires() {
    let rationale = crosses_tree_boundary("/etc/mydb/config", "/var/lib/mydb/config");
    assert!(rationale.is_some());
    assert!(rationale.unwrap().contains("/etc → /var"));
}

#[test]
fn test_broken_symlink_always_fires() {
    // Broken symlinks get advisory regardless of allowlist
    // (tested in integration with mock executor returning non-existent target)
}
```

- [ ] **Step 4: Run tests + clippy, commit**

```bash
git add -A
git commit -m "feat(inspectah): add cross-tree symlink advisory with allowlist"
```

---

### Task 5: Modernization Advisory System (collect crate)

**Files:**
- Create: `crates/collect/src/inspectors/modernization.rs` — pattern table + detection
- Modify: `crates/collect/src/inspectors/mod.rs` — register module
- Test: unit tests for all four patterns + negative cases

**Interfaces:**
- Consumes: `FindingKind::advisory()`, `AdvisoryType::Modernization`, OS metadata
- Produces: Modernization advisories on legacy patterns

- [ ] **Step 1: Define pattern table with correct detection rules**

```rust
pub struct ModernizationPattern {
    pub name: &'static str,
    pub detection: DetectionRule,
    pub replacement: &'static str,
    pub rationale: &'static str,
    pub min_os_major: Option<u32>,
}

pub enum DetectionRule {
    FileGlob(&'static str),
    FileGlobWithoutCounterpart {
        file_glob: &'static str,
        counterpart_pattern: fn(&str) -> String,
    },
    FileHasCustomEntries {
        path: &'static str,
        default_marker: &'static str,
    },
}

pub const MODERNIZATION_PATTERNS: &[ModernizationPattern] = &[
    ModernizationPattern {
        name: "sysvinit_script",
        detection: DetectionRule::FileGlobWithoutCounterpart {
            file_glob: "/etc/init.d/*",
            counterpart_pattern: sysvinit_to_systemd_unit,
        },
        replacement: "systemd unit",
        rationale: "SysVinit script with no systemd equivalent — create a .service unit for image mode",
        min_os_major: None,
    },
    // NOTE: ifcfg is NOT a modernization pattern. Networking config is treated as
    // informational inventory, not a modernization advisory. See spec §6.6.
    ModernizationPattern {
        name: "xinetd_config",
        detection: DetectionRule::FileGlob("/etc/xinetd.d/*"),
        replacement: "systemd socket activation",
        rationale: "xinetd is deprecated — convert to systemd socket activation",
        min_os_major: None,
    },
    ModernizationPattern {
        name: "anacrontab",
        detection: DetectionRule::FileHasCustomEntries {
            path: "/etc/anacrontab",
            default_marker: "cron.daily",
        },
        replacement: "systemd timer",
        rationale: "anacrontab has custom entries — consider systemd timers instead",
        min_os_major: None,
    },
];

fn sysvinit_to_systemd_unit(init_script: &str) -> String {
    let name = init_script.rsplit('/').next().unwrap_or(init_script);
    format!("/usr/lib/systemd/system/{}.service", name)
}
```

- [ ] **Step 2: Implement detection with full rule evaluation**

```rust
pub fn check_modernization_patterns(
    exec: &dyn Executor,
    os_major: u32,
) -> Vec<(String, FindingKind)> {
    let mut advisories = Vec::new();

    for pattern in MODERNIZATION_PATTERNS {
        if let Some(min) = pattern.min_os_major {
            if os_major < min { continue; }
        }

        match &pattern.detection {
            DetectionRule::FileGlob(glob) => {
                let r = exec.run("sh", &["-c", &format!("ls -1 {} 2>/dev/null", glob)]);
                for path in r.stdout.lines().map(|l| l.trim()).filter(|l| !l.is_empty()) {
                    advisories.push((
                        path.to_string(),
                        FindingKind::advisory(AdvisoryType::Modernization, pattern.rationale),
                    ));
                }
            }
            DetectionRule::FileGlobWithoutCounterpart { file_glob, counterpart_pattern } => {
                let r = exec.run("sh", &["-c", &format!("ls -1 {} 2>/dev/null", file_glob)]);
                for path in r.stdout.lines().map(|l| l.trim()).filter(|l| !l.is_empty()) {
                    let counterpart = counterpart_pattern(path);
                    let has_counterpart = exec.run("test", &["-f", &counterpart]).exit_code == 0;
                    if !has_counterpart {
                        advisories.push((
                            path.to_string(),
                            FindingKind::advisory(AdvisoryType::Modernization, pattern.rationale),
                        ));
                    }
                }
            }
            DetectionRule::FileHasCustomEntries { path, default_marker } => {
                let r = exec.run("test", &["-f", path]);
                if r.exit_code == 0 {
                    let content = exec.run("cat", &[path]);
                    let has_custom = content.stdout.lines()
                        .filter(|l| !l.trim().is_empty() && !l.trim().starts_with('#'))
                        .any(|l| !l.contains(default_marker));
                    if has_custom {
                        advisories.push((
                            path.to_string(),
                            FindingKind::advisory(AdvisoryType::Modernization, pattern.rationale),
                        ));
                    }
                }
            }
        }
    }

    advisories
}
```

- [ ] **Step 3: Write tests for all four patterns + negatives**

```rust
#[test]
fn test_sysvinit_with_matching_service_suppressed() {
    // /etc/init.d/httpd exists, /usr/lib/systemd/system/httpd.service also exists
    // → must NOT fire
    let mock = mock_exec_with_both_init_and_service("httpd");
    let advisories = check_modernization_patterns(&mock, 9);
    assert!(!advisories.iter().any(|(p, _)| p.contains("httpd")));
}

#[test]
fn test_sysvinit_without_service_fires() {
    // /etc/init.d/legacy-app exists, no matching .service
    let mock = mock_exec_with_init_only("legacy-app");
    let advisories = check_modernization_patterns(&mock, 9);
    assert!(advisories.iter().any(|(p, _)| p.contains("legacy-app")));
}

#[test]
fn test_ifcfg_not_in_modernization() {
    // ifcfg is network inventory, NOT a modernization advisory (spec §6.6)
    let advisories = check_modernization_patterns(&mock_exec_with_ifcfg, 9);
    assert!(!advisories.iter().any(|(p, _)| p.contains("ifcfg")));
}

#[test]
fn test_anacrontab_default_only_suppressed() {
    // /etc/anacrontab with only default cron.daily entries → must NOT fire
    let mock = mock_exec_with_default_anacrontab();
    let advisories = check_modernization_patterns(&mock, 9);
    assert!(!advisories.iter().any(|(p, _)| p.contains("anacrontab")));
}

#[test]
fn test_anacrontab_custom_entries_fires() {
    let mock = mock_exec_with_custom_anacrontab();
    let advisories = check_modernization_patterns(&mock, 9);
    assert!(advisories.iter().any(|(p, _)| p.contains("anacrontab")));
}

#[test]
fn test_xinetd_fires() {
    let mock = mock_exec_with_xinetd();
    let advisories = check_modernization_patterns(&mock, 9);
    assert!(advisories.iter().any(|(p, _)| p.contains("xinetd")));
}
```

- [ ] **Step 4: Run tests + clippy, commit**

```bash
git add -A
git commit -m "feat(inspectah): add modernization advisory system with full detection rules"
```

---

### Task 6: systemd Unit Shadow Detection (collect crate)

**Files:**
- Modify: `crates/collect/src/inspectors/services.rs` — detect full shadow vs drop-in
- Uses: `ShadowType` enum from Task 1 (field name: `shadow_type` per spec)
- Test: unit tests

**Interfaces:**
- Consumes: `ShadowType` enum from Task 1
- Produces: `shadow_type` field on service findings + rationale text for full shadows

- [ ] **Step 1: Implement shadow detection**

```rust
fn detect_shadow_type(exec: &dyn Executor, unit_name: &str) -> Option<ShadowType> {
    let etc_path = format!("/etc/systemd/system/{}", unit_name);
    let usr_path = format!("/usr/lib/systemd/system/{}", unit_name);
    let dropin_dir = format!("/etc/systemd/system/{}.d", unit_name);

    let etc_exists = exec.run("test", &["-f", &etc_path]).exit_code == 0;
    let usr_exists = exec.run("test", &["-f", &usr_path]).exit_code == 0;
    let dropin_exists = exec.run("test", &["-d", &dropin_dir]).exit_code == 0;

    if etc_exists && usr_exists {
        Some(ShadowType::FullShadow)
    } else if dropin_exists {
        Some(ShadowType::DropIn)
    } else {
        None
    }
}
```

- [ ] **Step 2: Populate shadow_type and set rationale text for full shadows**

During service inspection:

```rust
service.shadow_type = detect_shadow_type(exec, &service.name);

// Full shadows remain Actionable (produce COPY in Containerfile)
// but carry a rationale string for informational display
if service.shadow_type == Some(ShadowType::FullShadow) {
    service.shadow_rationale = Some(
        "Full unit shadow — base image updates to this unit will be silently ignored".to_string()
    );
}
```

Add to the service struct in `crates/core/src/types/services.rs`:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub shadow_rationale: Option<String>,
```

- [ ] **Step 3: Tests + commit**

```bash
git add -A
git commit -m "feat(inspectah): detect systemd full unit shadows vs drop-in overrides"
```

---

### Task 7: Section Grouping + HTML Report Rendering (pipeline + web crates)

**Files:**
- Create: `crates/pipeline/src/section_group.rs` — `SectionGroup` enum (rendering layer, NOT core)
- Modify: `crates/pipeline/src/lib.rs` — register module
- Modify: `crates/pipeline/templates/report/base.html` — group structure
- Modify: `crates/pipeline/templates/report/toc.html` — grouped TOC
- Modify: per-section templates — advisory row rendering + full-shadow rationale rendering
- Test: `insta` snapshot test for grouped HTML output, keyboard/ARIA verification steps

**Interfaces:**
- Consumes: `FindingKind` from Task 1, advisories from Tasks 3-5, `shadow_rationale` from Task 6
- Produces: Grouped HTML report with advisory rendering, section grouping, full-shadow rationale

**SectionGroup lives in the rendering layer**, not core. It is not serialized into snapshots. The pipeline, web, and tui crates each get their own reference to this enum (or it lives in pipeline and is re-exported).

- [ ] **Step 1: Create section_group.rs in pipeline crate**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SectionGroup {
    Packages,
    SystemConfig,
    Services,
    Identity,
    Network,
    Storage,
    Software,
    Secrets,
}

impl SectionGroup {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Packages => "Packages",
            Self::SystemConfig => "System Configuration",
            Self::Services => "Services & Scheduling",
            Self::Identity => "Users & Identity",
            Self::Network => "Network",
            Self::Storage => "Storage",
            Self::Software => "Software & Files",
            Self::Secrets => "Secrets & Subscription",
        }
    }

    pub fn for_section(section_name: &str) -> Self {
        match section_name {
            "rpm" => Self::Packages,
            "config" | "kernel_boot" | "selinux" => Self::SystemConfig,
            "services" | "scheduled_tasks" | "containers" => Self::Services,
            "users_groups" => Self::Identity,
            "network" => Self::Network,
            "storage" => Self::Storage,
            "non_rpm_software" | "unmanaged_files" => Self::Software,
            "secrets" | "subscription" => Self::Secrets,
            _ => Self::SystemConfig,
        }
    }

    pub fn all_in_order() -> &'static [SectionGroup] {
        &[Self::Packages, Self::SystemConfig, Self::Services, Self::Identity,
          Self::Network, Self::Storage, Self::Software, Self::Secrets]
    }
}
```

- [ ] **Step 2: Update HTML templates for grouped layout**

Group heading with disclosure and collapsed summary:

```html
<div class="pf-v6-c-expandable-section" id="group-{{ group_id }}">
  <button class="pf-v6-c-expandable-section__toggle"
          aria-expanded="true"
          aria-controls="group-{{ group_id }}-content"
          aria-label="{{ group_label }}">
    <span class="pf-v6-c-expandable-section__toggle-text">
      {{ group_label }}
    </span>
    <span class="pf-v6-c-badge pf-m-read">{{ actionable_count }}</span>
  </button>
  <!-- Collapsed summary (visible only when collapsed, toggled via JS) -->
  <span class="pf-v6-c-expandable-section__collapsed-summary" hidden>
    [{{ actionable_count }} actionable, {{ advisory_count }} advisories]
  </span>
  <div class="pf-v6-c-expandable-section__content"
       id="group-{{ group_id }}-content">
    <!-- sections rendered here -->
  </div>
</div>
```

- [ ] **Step 3: Add advisory row rendering + full-shadow rationale rendering**

Advisory rows (sorted below actionable findings):

```html
<div class="pf-v6-c-data-list__item pf-m-advisory" role="listitem"
     aria-label="Advisory: {{ advisory_type }} — {{ rationale }}"
     tabindex="0">
  <div class="pf-v6-c-data-list__item-row">
    <span class="pf-v6-c-label pf-m-blue pf-m-compact">
      <i class="pf-icon pf-icon-info"></i> Advisory
    </span>
    <span class="pf-v6-c-data-list__cell">{{ path }}</span>
  </div>
  <div class="pf-v6-c-data-list__item-row pf-m-rationale">
    <small>{{ rationale }}</small>
  </div>
</div>
```

Full-shadow rationale on actionable service findings (service remains actionable, rationale is informational context):

```html
{% if shadow_rationale %}
<div class="pf-v6-c-data-list__item-row pf-m-rationale">
  <small><i class="pf-icon pf-icon-warning-triangle"></i> {{ shadow_rationale }}</small>
</div>
{% endif %}
```

- [ ] **Step 4: Add CSS + JS for disclosure behavior and keyboard**

```javascript
// Group disclosure keyboard handler
document.querySelectorAll('.pf-v6-c-expandable-section__toggle').forEach(btn => {
  btn.addEventListener('click', () => {
    const expanded = btn.getAttribute('aria-expanded') === 'true';
    btn.setAttribute('aria-expanded', String(!expanded));
    const content = document.getElementById(btn.getAttribute('aria-controls'));
    content.hidden = expanded;
    // Show/hide collapsed summary
    const summary = btn.parentElement.querySelector('.pf-v6-c-expandable-section__collapsed-summary');
    if (summary) summary.hidden = !expanded;
    // Focus management
    if (!expanded) {
      const firstItem = content.querySelector('[tabindex="0"], button, [role="listitem"]');
      if (firstItem) firstItem.focus();
    }
  });
});
```

- [ ] **Step 5: Verify**

Run inspectah on a driftify-generated snapshot. Verify in browser:
- Groups render with disclosure controls
- Collapsed summary shows "[N actionable, M advisories]"
- Advisory rows show info icon, rationale, no toggle
- Full-shadow service findings show rationale line
- Keyboard: Enter/Space toggles disclosure, Tab reaches advisory rows, advisory rows are non-interactive on Enter/Space
- Screen reader: `aria-expanded`, `aria-controls`, `aria-label` on advisory rows

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat(inspectah): add section grouping and advisory rendering to HTML report"
```

---

### Task 8: Refine View Updates (web + refine crates)

**Files:**
- Modify: `crates/web/src/handlers.rs` — advisory items skip toggle logic
- Modify: `crates/web/src/adapter.rs` — group-level batch operations
- Modify: `crates/web/src/aggregate_handlers.rs` — aggregate view grouping
- Modify: `crates/refine/src/group_state.rs` — group collapse + batch toggle state
- Modify: `crates/refine/src/session.rs` — Containerfile: include actionable, skip advisory
- Modify: refine JavaScript assets — advisory row, batch toggle, collapse persistence
- Test: integration test for Containerfile generation (actionable included, advisory excluded)

**Interfaces:**
- Consumes: `FindingKind` from Task 1, `SectionGroup` from Task 7
- Produces: Refine view with advisory rendering, batch toggles, group collapse

**Critical Containerfile contract:** Containerfile generation includes `Actionable { include: true }` items and skips `Advisory` items. Unbacked /var dirs are actionable (they produce `RUN mkdir -p`); the grouped advisory is display-only.

- [ ] **Step 1: Update Containerfile generation**

```rust
fn should_include_in_containerfile(disposition: &FindingKind) -> bool {
    matches!(disposition, FindingKind::Actionable { include: true })
}
```

This correctly includes actionable findings (including unbacked /var dirs) and excludes advisories.

- [ ] **Step 2: Add batch toggle handler**

```rust
async fn batch_toggle_group(
    State(state): State<AppState>,
    Path(group_name): Path<String>,
    Json(payload): Json<BatchTogglePayload>,
) -> Result<Json<RefineResponse>, AppError> {
    // Toggle all Actionable items in the group; Advisory items unchanged
    // payload.include: bool
    // Count badge shows only actionable items
    // Mixed state: when group has both included and excluded actionable items
}
```

- [ ] **Step 3: Update refine JavaScript**

- Advisory rows: no toggle, info icon, rationale text, non-interactive on click
- Batch toggle: "Include all (N items)" / "Exclude all (N items)" where N = actionable only
- Mixed state indicator for partially-toggled groups
- Collapse state persists within session; search auto-expands collapsed groups
- **Full-shadow rationale on service findings:** When a service finding has `shadow_type: "full_shadow"` and a `shadow_rationale`, render the rationale as a helper-text line below the service toggle row (same pattern as HTML report §Task 7 Step 3). The finding remains actionable (toggle is present); the rationale is informational context. This matches the HTML and TUI surfaces.

- [ ] **Step 4: Verify Containerfile output and full-shadow rendering**

Run refine on a snapshot with unbacked /var dirs and a full-shadow service. Verify:
- `/var/lib/pgsql/data` → `RUN mkdir -p /var/lib/pgsql/data` in Containerfile (actionable)
- Unbacked /var advisory → NOT in Containerfile (display-only)
- Cross-tree symlink advisory → NOT in Containerfile
- Modernization advisory → NOT in Containerfile
- Full-shadow `sshd.service` → COPY line in Containerfile (actionable) AND rationale text visible below toggle in refine view

- [ ] **Step 5: Tests + commit**

```bash
git add -A
git commit -m "feat(inspectah): add advisory handling and batch toggles to refine view"
```

---

### Task 9: TUI Updates (tui crate)

**Files:**
- Modify: `crates/tui/src/sections.rs` — group-level tree structure using `SectionGroup`
- Modify: `crates/tui/src/widget/section_nav.rs` — group expand/collapse
- Modify: `crates/tui/src/widget/triage_list.rs` — advisory row rendering + full-shadow rationale
- Modify: `crates/tui/src/widget/detail_view.rs` — advisory rationale display
- Modify: `crates/tui/src/keys.rs` — Left/Right for group expand/collapse
- Test: unit tests for tree navigation, advisory non-selectability

**Interfaces:**
- Consumes: `FindingKind` from Task 1, `SectionGroup` from Task 7 (import from pipeline crate or duplicate in tui)
- Produces: TUI with group tree navigation, advisory rendering, full-shadow rationale

- [ ] **Step 1: Add group tree nodes**

Groups as parent nodes in section_nav. Left collapses, Right expands. Collapsed summary: "[N actionable, M advisories]".

- [ ] **Step 2: Advisory rendering in triage_list**

`ℹ` prefix, navigable but non-toggleable, dimmed styling. Full-shadow service findings show rationale line in list view.

- [ ] **Step 3: Detail pane for advisories**

Advisory rationale + type shown in detail pane when focused.

- [ ] **Step 4: Tests + commit**

```bash
git add -A
git commit -m "feat(inspectah): add advisory rendering and group navigation to TUI"
```

---

### Task 10: Audit Report Updates (pipeline)

**Files:**
- Modify: `crates/pipeline/src/` — audit report renderer
- Test: `insta` snapshot tests

**Interfaces:**
- Consumes: `SectionGroup` from Task 7, `FindingKind` from Task 1
- Produces: Grouped markdown audit report with advisory sections, full-shadow rationale

- [ ] **Step 1: Add group headings to markdown**

`## [Group Name]` → `### [Section Name]` → findings. Advisories under `### Advisories` within each group. Full-shadow service findings include rationale line.

Format: `- ℹ **[path/pattern]** — [rationale]`

- [ ] **Step 2: Snapshot tests**

Use `insta` to verify grouped audit report output with advisories, full-shadow rationale, and the grouped unbacked /var advisory.

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "feat(inspectah): add grouped information architecture to audit report"
```

---

### Task 11: EL8 Platform Compatibility + Acceptance Verification (collect crate)

**Files:**
- Modify: `crates/collect/src/inspectors/` — EL8-specific guards
- Modify: `crates/collect/src/inspectors/services.rs` — systemd 239 differences
- Test: EL8 mock executor tests + acceptance verification

**Interfaces:**
- Consumes: OS metadata from snapshot
- Produces: EL8-compatible inspection behavior

- [ ] **Step 1: Verify rpm-dump format compatibility**

Test `rpm -qa --dump` field layout on EL8 vs EL9+. Add version-specific parsing if needed.

- [ ] **Step 2: Guard EL8-specific differences**

- ifcfg is NOT a modernization advisory on any platform — it's network inventory (spec §6.6, handled in Task 12)
- systemd 239: verify timer inspection handles missing features
- tmpfiles.d: document which directives are EL8-safe

- [ ] **Step 3: Acceptance verification for custom tuned profile**

Verify that inspectah detects custom tuned profile directories (`/etc/tuned/*/tuned.conf`) and preserves them in the output contract. This is an explicit acceptance case from the spec (§3.4 + §7 acceptance matrix).

```rust
#[test]
fn test_custom_tuned_profile_detected() {
    let mock = mock_exec_with_custom_tuned_profile("/etc/tuned/myapp/tuned.conf");
    let snapshot = run_inspection(&mock);
    // Verify the tuned profile appears in kernel_boot section
    let kb = snapshot.kernel_boot.unwrap();
    assert!(kb.tuned_disposition.is_included());
}
```

- [ ] **Step 4: Tests + commit**

```bash
git add -A
git commit -m "feat(inspectah): add EL8 platform compatibility and tuned profile verification"
```

---

### Task 12: EL8 Target Image Mapping + Networking-as-Inventory (core + collect + pipeline + web + refine + tui crates)

**Files:**
- Modify: `crates/core/src/baseline.rs` — EL8→EL9 mapping in `resolve_from_os_release()` (line ~400)
- Modify: `crates/core/src/types/network.rs` — add `NetworkInventory` marker type
- Modify: `crates/collect/src/inspectors/network.rs` — mark network findings as inventory
- Modify: `crates/pipeline/src/render/containerfile.rs` — unconditionally skip network inventory
- Modify: `crates/pipeline/templates/report/network.html` — contextual note banner
- Modify: `crates/pipeline/src/render/audit.rs` — contextual note in markdown
- Modify: `crates/web/src/handlers.rs` — network items non-toggleable in refine
- Modify: `crates/refine/src/session.rs` — network inventory excluded from Containerfile unconditionally
- Modify: `crates/tui/src/widget/triage_list.rs` — network items non-toggleable in TUI
- Test: unit tests for all invariants

**Interfaces:**
- Consumes: OS metadata from snapshot, `FindingKind` from Task 1
- Produces: Correct `FROM` line for EL8 scans, non-toggleable network inventory across all surfaces

**Critical invariant (spec §6.6):** Network findings are **informational inventory, never Containerfile output**. This is not implemented via `FindingKind::excluded()` (which could be toggled back to `included` via refine/TUI). Instead, network findings use a dedicated inventory treatment that the Containerfile renderer unconditionally skips and interactive surfaces render as non-toggleable.

**Two options for the inventory-only contract:**

- **Option A (recommended):** Add a new `FindingKind::Inventory` variant alongside `Actionable` and `Advisory`. Inventory items render like findings (show the data) but have no toggle and no Containerfile output. The Containerfile renderer checks `is_actionable()` which returns false for both `Advisory` and `Inventory`.

- **Option B:** Keep network findings as `Actionable { include: false }` but add a `containerfile_eligible: bool` flag on the network section (or on each network item). The Containerfile renderer checks this flag unconditionally, and interactive surfaces check it to suppress the toggle. Less clean than Option A but avoids a schema change to `FindingKind`.

The implementer should choose based on what integrates most cleanly with Task 1's `FindingKind` enum. If `Inventory` is added, update `FindingKind` in `crates/core/src/types/finding.rs`:

```rust
pub enum FindingKind {
    Actionable { include: bool },
    Advisory { advisory_type: AdvisoryType, rationale: String },
    Inventory,
}
```

- [ ] **Step 1: Add EL8→EL9 mapping to existing `resolve_from_os_release()`**

The base image resolution logic is in `crates/core/src/baseline.rs` at `resolve_from_os_release()` (line ~400). For EL8, map up to version 9:

```rust
let effective_major = if major == 8 { 9 } else { major };
```

The existing `--base-image` CLI flag overrides this. EL8 hosts won't have a booted bootc image, so they always hit the os-release fallback.

- [ ] **Step 2: Implement network inventory treatment**

Mark all network section findings as inventory (not actionable, not advisory). Whichever option is chosen (A or B), the following invariants must hold:

1. **Containerfile renderer** (`crates/pipeline/src/render/containerfile.rs`): unconditionally skips network inventory. This is NOT gated on `include` — even if somehow toggled, network items never produce Containerfile output.
2. **HTML report** (`crates/pipeline/templates/report/`): network items display their data but have no toggle switch. Section shows an informational banner (see Step 3).
3. **Refine view** (`crates/web/src/handlers.rs`, refine JS): network items render without toggle controls. Batch toggle at the Network group level is suppressed or skips inventory items.
4. **TUI** (`crates/tui/src/widget/triage_list.rs`): network items are navigable but Enter/Space has no effect (same as advisory items).
5. **Audit report** (`crates/pipeline/src/render/audit.rs`): network items listed under an "### Inventory" subheading (not "### Advisories").

- [ ] **Step 3: Add contextual note across all surfaces**

When the source host uses ifcfg format and the target is RHEL 9+, render a section-level banner:

"Source host uses ifcfg network scripts. RHEL 9+ targets use NetworkManager keyfiles by default. ifcfg support is deprecated in RHEL 9 and removed in RHEL 10. Plan network configuration separately for the target environment."

**HTML report:** Informational banner at the top of the Network section (PatternFly alert component, `pf-m-info` variant).
**Refine view:** Same banner, non-interactive.
**TUI:** One-line note at the top of the network section in the detail pane.
**Audit report:** Block quote at the top of the `## Network` group.

- [ ] **Step 4: Write tests**

```rust
#[test]
fn test_el8_rhel_resolves_to_rhel9() {
    let os = OsRelease { id: "rhel".into(), version_id: "8.10".into(), .. };
    let result = resolve_from_os_release(&os).unwrap();
    assert!(result.image_ref.contains("rhel9/rhel-bootc"));
}

#[test]
fn test_el8_centos_resolves_to_stream9() {
    let os = OsRelease { id: "centos".into(), version_id: "8".into(), .. };
    let result = resolve_from_os_release(&os).unwrap();
    assert!(result.image_ref.contains("centos-bootc:stream9"));
}

#[test]
fn test_el9_rhel_resolves_to_rhel9() {
    let os = OsRelease { id: "rhel".into(), version_id: "9.6".into(), .. };
    let result = resolve_from_os_release(&os).unwrap();
    assert!(result.image_ref.contains("rhel9/rhel-bootc"));
}

#[test]
fn test_network_inventory_excluded_from_containerfile_unconditionally() {
    // Even if someone manually set include=true on a network item,
    // the Containerfile renderer must still skip it
    let mut snapshot = scan_host_with_ifcfg(&mock_exec);
    // Attempt to force-include a network item (simulating a toggle)
    force_include_network_items(&mut snapshot);
    let containerfile = render_containerfile(&snapshot);
    assert!(!containerfile.contains("ifcfg"));
    assert!(!containerfile.contains("network-scripts"));
}

#[test]
fn test_network_items_non_toggleable_in_refine() {
    // Network inventory items should not respond to toggle requests
    let session = create_refine_session_with_network(&snapshot);
    let result = session.toggle_item("network", "ifcfg-eth1");
    assert!(result.is_err() || result.unwrap().is_unchanged());
}
```

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(inspectah): add EL8 target image mapping and networking-as-inventory"
```
