# Refine Service Layer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the `inspectah-refine` crate (pure domain logic) and wire it through `inspectah-web` (axum HTTP API) and `inspectah-cli` (refine subcommand), enabling interactive refinement of scan output via a localhost web server.

**Architecture:** New `inspectah-refine` workspace crate owns all domain logic — operations, session state, attention model, undo/redo, view projection, tarball I/O. `inspectah-web` wraps it in an axum HTTP API (9 endpoints) behind `Arc<Mutex<RefineSession>>`. `inspectah-cli` adds a `refine` subcommand that loads a tarball, starts the server, and optionally opens a browser.

**Tech Stack:** Rust, serde/serde_json, thiserror, axum, tower-http (CORS), rust-embed (static assets), tar/flate2 (tarball I/O), tempfile, insta (snapshot tests), clap, tokio, open (browser launch)

**Spec:** `docs/specs/proposed/2026-05-15-refine-service-layer-design.md`

### Contract decisions

**Re-import:** Refine exports ARE re-openable in refine. Exported tarballs
preserve `redaction_state: FullyRedacted` and pass the import provenance
gate. Operator-authored excludes (`include: false`) survive the round-trip
because normalization preserves deserialized values, not blanket-rewrites
them.

**Export file set:** The approved export contract is an EXACT file set, not
a minimum. `render_refine_export()` materializes only the contracted
artifacts. Tests enforce both required-present and forbidden-absent. The
pipeline's `render_all()` is not used for refine export.

**Preview/export fidelity:** Both preview and export use the renderer's
materialized-root seam (`write_config_tree` → `config_copy_roots`). Preview
materializes to a tempdir to get the same roots as export. This guarantees
byte-identical output that is also truthful to the actual `config/` tree
contents, including repo files, GPG keys, and other non-`config.files`
sources the renderer writes.

**Scope:** This plan delivers the API/service substrate and CLI integration.
The browser UI is a placeholder — the full interactive UI is Phase 4 (Fern
designs, Kit builds), consuming this plan's API.

---

## File Structure

### New crate: `inspectah-refine/`

| File | Responsibility |
|------|---------------|
| `inspectah-refine/Cargo.toml` | Crate manifest — depends on inspectah-core, inspectah-pipeline |
| `inspectah-refine/src/lib.rs` | Re-exports: types, session, attention, tarball, normalize |
| `inspectah-refine/src/types.rs` | Data model: `PackageTarget`, `RefinementOp`, `AttentionLevel`, `AttentionReason`, `AttentionTag`, `RefinedPackage`, `RefinedConfig`, `RefineStats`, `RefinedView`, `ChangesSummary`, `AnnotatedOp`, `RefineError` |
| `inspectah-refine/src/session.rs` | `RefineSession` — state, apply/undo/redo, view projection, dirty tracking |
| `inspectah-refine/src/attention.rs` | Attention computation — `compute_package_attention`, `compute_config_attention`, sensitive paths |
| `inspectah-refine/src/normalize.rs` | Snapshot normalization for refine — include defaults, leaf package classification |
| `inspectah-refine/src/tarball.rs` | `from_tarball()` — archive safety, extraction, flattening, provenance gate |
| `inspectah-refine/tests/types_test.rs` | Serde roundtrip tests for all public types |
| `inspectah-refine/tests/session_test.rs` | Unit tests for apply/undo/redo, idempotency, cursor mechanics, view projection |
| `inspectah-refine/tests/attention_test.rs` | Attention heuristic tests |
| `inspectah-refine/tests/normalize_test.rs` | Normalization + dirty-state tests |
| `inspectah-refine/tests/tarball_test.rs` | Import provenance, archive safety, prefixed/flat loading, export contract |

### Modified crate: `inspectah-web/`

| File | Responsibility |
|------|---------------|
| `inspectah-web/Cargo.toml` | Add deps: inspectah-refine, axum, tower-http, rust-embed, tokio, serde_json |
| `inspectah-web/src/lib.rs` | Re-exports: `router()`, `AppState` |
| `inspectah-web/src/handlers.rs` | 9 axum handlers: health, view, op, undo, redo, ops, changes, tarball, serve_report |
| `inspectah-web/src/error.rs` | `RefineError` → axum response mapping (status codes, JSON error bodies) |
| `inspectah-web/src/assets.rs` | rust-embed static asset serving (placeholder index.html for v1) |
| `inspectah-web/tests/api_test.rs` | HTTP contract tests — all endpoints, CORS, generation tracking |

### Modified crate: `inspectah-cli/`

| File | Responsibility |
|------|---------------|
| `inspectah-cli/Cargo.toml` | Add deps: inspectah-web, inspectah-refine, tokio, open |
| `inspectah-cli/src/main.rs` | Add `Refine` variant to `Commands` enum |
| `inspectah-cli/src/commands/mod.rs` | Add `pub mod refine;` |
| `inspectah-cli/src/commands/refine.rs` | `RefineArgs`, `run_refine()` — tarball load, server start, shutdown |

### Modified: workspace root

| File | Responsibility |
|------|---------------|
| `Cargo.toml` | Add `inspectah-refine` to workspace members |

---

## Task 1: Create inspectah-refine crate scaffold

**Files:**
- Create: `inspectah-refine/Cargo.toml`
- Create: `inspectah-refine/src/lib.rs`
- Modify: `Cargo.toml` (workspace root, line ~3, members array)

- [ ] **Step 1: Create the crate directory**

```bash
mkdir -p inspectah-refine/src
```

- [ ] **Step 2: Write Cargo.toml**

```toml
[package]
name = "inspectah-refine"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
inspectah-core = { path = "../inspectah-core" }
inspectah-pipeline = { path = "../inspectah-pipeline" }
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
tempfile = "3"
tar = "0.4"
flate2 = "0.2"
walkdir = "2"

[dev-dependencies]
insta.workspace = true
tempfile = "3"
tar = "0.4"
flate2 = "0.2"
walkdir = "2"
```

- [ ] **Step 3: Write initial lib.rs**

```rust
pub mod types;
pub mod session;
pub mod attention;
pub mod normalize;
pub mod tarball;
```

- [ ] **Step 4: Create empty module files**

Create empty files so the workspace compiles:
- `inspectah-refine/src/types.rs` (empty)
- `inspectah-refine/src/session.rs` (empty)
- `inspectah-refine/src/attention.rs` (empty)
- `inspectah-refine/src/normalize.rs` (empty)
- `inspectah-refine/src/tarball.rs` (empty)

- [ ] **Step 5: Add to workspace members**

In the root `Cargo.toml`, add `"inspectah-refine"` to the `members` array:

```toml
[workspace]
members = [
    "inspectah-core",
    "inspectah-collect",
    "inspectah-pipeline",
    "inspectah-cli",
    "inspectah-web",
    "inspectah-refine",
]
```

- [ ] **Step 6: Verify workspace compiles**

Run: `cargo check --workspace`
Expected: success (empty modules compile fine)

- [ ] **Step 7: Commit**

```bash
git add inspectah-refine/ Cargo.toml
git commit -m "feat(refine): scaffold inspectah-refine crate

Add empty workspace member with dependency on inspectah-core and
inspectah-pipeline. Module stubs for types, session, attention,
normalize, tarball.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 2: Define data model types

**Files:**
- Create: `inspectah-refine/src/types.rs`
- Create: `inspectah-refine/tests/types_test.rs`

- [ ] **Step 1: Write failing test for PackageTarget serde roundtrip**

Create `inspectah-refine/tests/types_test.rs`:

```rust
use inspectah_refine::types::PackageTarget;

#[test]
fn package_target_serde_roundtrip() {
    let target = PackageTarget {
        name: "httpd".into(),
        arch: "x86_64".into(),
    };
    let json = serde_json::to_string(&target).unwrap();
    let parsed: PackageTarget = serde_json::from_str(&json).unwrap();
    assert_eq!(target, parsed);
}

#[test]
fn package_target_display() {
    let target = PackageTarget {
        name: "glibc".into(),
        arch: "i686".into(),
    };
    assert_eq!(format!("{target}"), "glibc.i686");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p inspectah-refine --test types_test -- --nocapture`
Expected: FAIL — `PackageTarget` not found

- [ ] **Step 3: Implement PackageTarget and RefinementOp**

Write `inspectah-refine/src/types.rs`:

```rust
use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use inspectah_core::types::config::ConfigFileEntry;
use inspectah_core::types::rpm::PackageEntry;

/// Composite key that uniquely identifies a package in a snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PackageTarget {
    pub name: String,
    pub arch: String,
}

impl PackageTarget {
    pub fn matches(&self, entry: &PackageEntry) -> bool {
        self.name == entry.name && self.arch == entry.arch
    }
}

impl std::fmt::Display for PackageTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.name, self.arch)
    }
}

/// A single refinement operation. Serde-tagged for JSON transport.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", content = "target")]
pub enum RefinementOp {
    ExcludePackage(PackageTarget),
    IncludePackage(PackageTarget),
    ExcludeConfig { path: PathBuf },
    IncludeConfig { path: PathBuf },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionLevel {
    NeedsReview,
    Informational,
    Routine,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionReason {
    ConfigModified,
    ConfigUnowned,
    ConfigOrphaned,
    SensitivePath,
    PackageNotInBaseline,
    PackageLocalInstall,
    PackageStateChanged,
    PackageNoRepo,
    Custom(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttentionTag {
    pub level: AttentionLevel,
    pub reason: AttentionReason,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefinedPackage {
    pub entry: PackageEntry,
    pub attention: Vec<AttentionTag>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefinedConfig {
    pub entry: ConfigFileEntry,
    pub attention: Vec<AttentionTag>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefineStats {
    pub total_packages: usize,
    pub included_packages: usize,
    pub excluded_packages: usize,
    pub total_configs: usize,
    pub included_configs: usize,
    pub excluded_configs: usize,
    pub needs_review_count: usize,
    pub ops_applied: usize,
    pub can_undo: bool,
    pub can_redo: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefinedView {
    pub packages: Vec<RefinedPackage>,
    pub config_files: Vec<RefinedConfig>,
    pub containerfile_preview: String,
    pub stats: RefineStats,
    pub generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangesSummary {
    pub packages_included: Vec<PackageTarget>,
    pub packages_excluded: Vec<PackageTarget>,
    pub configs_included: Vec<String>,
    pub configs_excluded: Vec<String>,
    pub is_dirty: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotatedOp {
    #[serde(flatten)]
    pub op: RefinementOp,
    pub active: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum RefineError {
    #[error("unknown target: {0}")]
    UnknownTarget(String),

    #[error("nothing to undo")]
    NothingToUndo,

    #[error("nothing to redo")]
    NothingToRedo,

    #[error("stale generation: expected {expected}, got {actual}")]
    StaleGeneration { expected: u64, actual: u64 },

    #[error("render failed: {0}")]
    RenderFailed(String),

    #[error("tarball error: {0}")]
    TarballError(String),

    #[error("snapshot load error: {0}")]
    SnapshotLoad(String),

    #[error("untrusted snapshot: {0}")]
    UntrustedSnapshot(String),

    #[error("archive safety violation: {0}")]
    ArchiveSafety(String),

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p inspectah-refine --test types_test -- --nocapture`
Expected: PASS

- [ ] **Step 5: Add more serde roundtrip tests**

Append to `inspectah-refine/tests/types_test.rs`:

```rust
use inspectah_refine::types::{RefinementOp, AnnotatedOp, AttentionLevel, AttentionReason, AttentionTag};
use std::path::PathBuf;

#[test]
fn refinement_op_exclude_package_json() {
    let op = RefinementOp::ExcludePackage(PackageTarget {
        name: "httpd".into(),
        arch: "x86_64".into(),
    });
    let json = serde_json::to_string(&op).unwrap();
    assert!(json.contains(r#""op":"ExcludePackage""#));
    assert!(json.contains(r#""name":"httpd""#));
    assert!(json.contains(r#""arch":"x86_64""#));
    let parsed: RefinementOp = serde_json::from_str(&json).unwrap();
    assert_eq!(op, parsed);
}

#[test]
fn refinement_op_exclude_config_json() {
    let op = RefinementOp::ExcludeConfig {
        path: PathBuf::from("/etc/httpd/conf/httpd.conf"),
    };
    let json = serde_json::to_string(&op).unwrap();
    let parsed: RefinementOp = serde_json::from_str(&json).unwrap();
    assert_eq!(op, parsed);
}

#[test]
fn annotated_op_json_flattens() {
    let aop = AnnotatedOp {
        op: RefinementOp::ExcludePackage(PackageTarget {
            name: "vim".into(),
            arch: "x86_64".into(),
        }),
        active: true,
    };
    let json = serde_json::to_string(&aop).unwrap();
    // serde(flatten) merges op fields into the top level
    assert!(json.contains(r#""op":"ExcludePackage""#));
    assert!(json.contains(r#""active":true"#));
}

#[test]
fn attention_tag_serde() {
    let tag = AttentionTag {
        level: AttentionLevel::NeedsReview,
        reason: AttentionReason::ConfigModified,
        detail: Some("RPM-owned config was modified".into()),
    };
    let json = serde_json::to_string(&tag).unwrap();
    let parsed: AttentionTag = serde_json::from_str(&json).unwrap();
    assert_eq!(tag, parsed);
}

#[test]
fn attention_reason_custom_variant() {
    let reason = AttentionReason::Custom("fleet-uncommon".into());
    let json = serde_json::to_string(&reason).unwrap();
    let parsed: AttentionReason = serde_json::from_str(&json).unwrap();
    assert_eq!(reason, parsed);
}
```

- [ ] **Step 6: Run all type tests**

Run: `cargo test -p inspectah-refine --test types_test -- --nocapture`
Expected: PASS (all 7 tests)

- [ ] **Step 7: Commit**

```bash
git add inspectah-refine/src/types.rs inspectah-refine/tests/types_test.rs
git commit -m "feat(refine): define data model types with serde roundtrip tests

PackageTarget (name+arch composite key), RefinementOp (4 variants,
serde-tagged), attention model (AttentionLevel/Reason/Tag),
RefinedView, ChangesSummary, AnnotatedOp, RefineError.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 3: Implement snapshot normalization for refine

**Files:**
- Create: `inspectah-refine/src/normalize.rs`
- Create: `inspectah-refine/tests/normalize_test.rs`

### Import/re-import contract and the `include` field ambiguity

The approved spec requires:
- Missing `include` fields normalize to `true` (default-include convention)
- Explicit `include: false` is preserved (authored exclusion)
- Re-imported refine exports preserve operator-set excludes

The Rust model uses `#[serde(default)] pub include: bool`, which collapses
"field absent" and "field explicitly false" into the same `false` value at
deserialize time. Standard typed deserialization cannot distinguish them.

**Resolution: raw-JSON presence-aware defaulting.** Before typed
deserialization, the normalization path reads the raw JSON as
`serde_json::Value`, walks the package/config arrays, and patches any
entry that *lacks* the `include` key by inserting `"include": true`.
Entries that already have `"include": false` are untouched. The patched
JSON is then deserialized into `InspectionSnapshot` where the typed
`bool` field correctly reflects the intent:

| JSON state | After patching | After deser |
|-----------|---------------|-------------|
| `include` absent | `"include": true` added | `true` |
| `"include": true` | untouched | `true` |
| `"include": false` | untouched | `false` |

This approach keeps the core type unchanged (`bool`, not `Option<bool>`)
while resolving the approved contract faithfully.

- [ ] **Step 1: Write failing test for presence-aware include defaulting**

Create `inspectah-refine/tests/normalize_test.rs`:

```rust
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind, ConfigSection};
use inspectah_refine::types::RefineError;

#[test]
fn omitted_include_defaults_to_true() {
    // JSON with no "include" field on the package — old schema
    let json = r#"{
        "schema_version": 14,
        "rpm": {
            "packages_added": [
                {"name": "httpd", "arch": "x86_64", "state": "added"}
            ]
        }
    }"#;

    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    assert!(
        snap.rpm.as_ref().unwrap().packages_added[0].include,
        "omitted include must default to true"
    );
}

#[test]
fn explicit_false_preserved() {
    // JSON with explicit "include": false — re-imported refine export
    let json = r#"{
        "schema_version": 14,
        "rpm": {
            "packages_added": [
                {"name": "httpd", "arch": "x86_64", "state": "added", "include": false}
            ]
        }
    }"#;

    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    assert!(
        !snap.rpm.as_ref().unwrap().packages_added[0].include,
        "explicit include: false must be preserved"
    );
}

#[test]
fn explicit_true_preserved() {
    let json = r#"{
        "schema_version": 14,
        "rpm": {
            "packages_added": [
                {"name": "httpd", "arch": "x86_64", "state": "added", "include": true}
            ]
        }
    }"#;

    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    assert!(snap.rpm.as_ref().unwrap().packages_added[0].include);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p inspectah-refine --test normalize_test -- --nocapture`
Expected: FAIL — `load_for_refine` not found

- [ ] **Step 3: Implement load_for_refine with raw-JSON patching**

Write `inspectah-refine/src/normalize.rs`:

```rust
use inspectah_core::snapshot::{migrate, InspectionSnapshot};
use crate::types::RefineError;
use serde_json::Value;

/// Load, normalize, and migrate a snapshot from raw JSON for refine use.
///
/// This is the sole entry point for snapshot import in the refine crate.
/// It handles the `include` field ambiguity: the core types use
/// `#[serde(default)] bool`, which collapses absent fields and explicit
/// `false` into the same value. This function patches the raw JSON
/// before typed deserialization to distinguish them:
///
/// - Absent `include` → patched to `true` (default-include convention)
/// - Explicit `include: false` → untouched (authored exclusion preserved)
/// - Explicit `include: true` → untouched
///
/// After patching, the JSON is deserialized and schema-migrated.
pub fn load_for_refine(raw_json: &str) -> Result<InspectionSnapshot, RefineError> {
    let mut value: Value = serde_json::from_str(raw_json)
        .map_err(|e| RefineError::SnapshotLoad(e.to_string()))?;

    patch_missing_includes(&mut value);

    let mut snap: InspectionSnapshot = serde_json::from_value(value)
        .map_err(|e| RefineError::SnapshotLoad(e.to_string()))?;

    migrate(&mut snap);

    Ok(snap)
}

/// Walk the raw JSON and add `"include": true` to any package or config
/// entry that lacks the `include` key. Entries with an existing `include`
/// key (whether true or false) are untouched.
fn patch_missing_includes(value: &mut Value) {
    // Patch rpm.packages_added
    if let Some(rpm) = value.get_mut("rpm") {
        patch_array_includes(rpm, "packages_added");
        patch_array_includes(rpm, "base_image_only");
    }

    // Patch config.files
    if let Some(config) = value.get_mut("config") {
        patch_array_includes(config, "files");
    }
}

fn patch_array_includes(parent: &mut Value, array_key: &str) {
    if let Some(Value::Array(entries)) = parent.get_mut(array_key) {
        for entry in entries {
            if let Value::Object(map) = entry {
                if !map.contains_key("include") {
                    map.insert("include".into(), Value::Bool(true));
                }
            }
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p inspectah-refine --test normalize_test -- --nocapture`
Expected: PASS

- [ ] **Step 5: Add comprehensive fixture tests**

Append to `inspectah-refine/tests/normalize_test.rs`:

```rust
#[test]
fn omitted_config_include_defaults_to_true() {
    let json = r#"{
        "schema_version": 14,
        "config": {
            "files": [
                {"path": "/etc/httpd/conf/httpd.conf", "kind": "rpm_owned_modified"}
            ]
        }
    }"#;

    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    assert!(
        snap.config.as_ref().unwrap().files[0].include,
        "omitted config include must default to true"
    );
}

#[test]
fn explicit_config_false_preserved() {
    let json = r#"{
        "schema_version": 14,
        "config": {
            "files": [
                {"path": "/etc/httpd/conf/httpd.conf", "kind": "rpm_owned_modified", "include": false}
            ]
        }
    }"#;

    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    assert!(
        !snap.config.as_ref().unwrap().files[0].include,
        "explicit config include: false must be preserved"
    );
}

#[test]
fn base_image_only_include_false_preserved() {
    let json = r#"{
        "schema_version": 14,
        "rpm": {
            "packages_added": [],
            "base_image_only": [
                {"name": "kernel", "arch": "x86_64", "state": "base_image_only", "include": false}
            ]
        }
    }"#;

    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    assert!(!snap.rpm.as_ref().unwrap().base_image_only[0].include);
}

#[test]
fn base_image_only_omitted_include_defaults_true() {
    let json = r#"{
        "schema_version": 14,
        "rpm": {
            "packages_added": [],
            "base_image_only": [
                {"name": "kernel", "arch": "x86_64", "state": "base_image_only"}
            ]
        }
    }"#;

    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    assert!(
        snap.rpm.as_ref().unwrap().base_image_only[0].include,
        "omitted base_image_only include must default to true"
    );
}

#[test]
fn mixed_present_and_absent_includes() {
    // Snapshot with both present and absent include fields in the same array
    let json = r#"{
        "schema_version": 14,
        "rpm": {
            "packages_added": [
                {"name": "httpd", "arch": "x86_64", "state": "added", "include": false},
                {"name": "vim", "arch": "x86_64", "state": "added", "include": true},
                {"name": "curl", "arch": "x86_64", "state": "added"}
            ]
        }
    }"#;

    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    let pkgs = &snap.rpm.as_ref().unwrap().packages_added;
    assert!(!pkgs[0].include, "httpd: explicit false preserved");
    assert!(pkgs[1].include, "vim: explicit true preserved");
    assert!(pkgs[2].include, "curl: omitted defaulted to true");
}

#[test]
fn empty_snapshot_loads() {
    let json = r#"{"schema_version": 14}"#;
    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    assert!(snap.rpm.is_none());
    assert!(snap.config.is_none());
}

#[test]
fn go_emitted_snapshot_roundtrip() {
    // Full Go-style snapshot: every field present, all include: true
    let json = r#"{
        "schema_version": 14,
        "rpm": {
            "packages_added": [
                {"name": "httpd", "arch": "x86_64", "state": "added", "include": true},
                {"name": "vim", "arch": "x86_64", "state": "added", "include": true}
            ],
            "base_image_only": [
                {"name": "kernel", "arch": "x86_64", "state": "base_image_only", "include": false}
            ]
        },
        "config": {
            "files": [
                {"path": "/etc/httpd/conf/httpd.conf", "kind": "rpm_owned_modified", "include": true}
            ]
        }
    }"#;

    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    let rpm = snap.rpm.as_ref().unwrap();
    assert!(rpm.packages_added[0].include);
    assert!(rpm.packages_added[1].include);
    assert!(!rpm.base_image_only[0].include);
    assert!(snap.config.as_ref().unwrap().files[0].include);
}
```

- [ ] **Step 6: Run all normalize tests**

Run: `cargo test -p inspectah-refine --test normalize_test -- --nocapture`
Expected: PASS (10 tests)

- [ ] **Step 7: Commit**

```bash
git add inspectah-refine/src/normalize.rs inspectah-refine/tests/normalize_test.rs
git commit -m "feat(refine): presence-aware include defaulting via raw-JSON patching

load_for_refine() patches raw JSON before typed deserialization:
absent include → true (default-include convention), explicit false →
preserved. Resolves the serde(default) bool ambiguity without changing
core types. Tests cover omitted, explicit true, explicit false, mixed
arrays, base_image_only, configs, and Go-emitted snapshots.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 4: Implement attention model

**Files:**
- Create: `inspectah-refine/src/attention.rs`
- Create: `inspectah-refine/tests/attention_test.rs`

- [ ] **Step 1: Write failing tests for package attention**

Create `inspectah-refine/tests/attention_test.rs`:

```rust
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind, ConfigSection};
use inspectah_refine::types::{AttentionLevel, AttentionReason};

#[test]
fn package_added_gets_needs_review() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "httpd".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            include: true,
            ..Default::default()
        }],
        ..Default::default()
    });

    let packages = inspectah_refine::attention::compute_package_attention(&snap);
    assert_eq!(packages.len(), 1);
    assert_eq!(packages[0].attention[0].level, AttentionLevel::NeedsReview);
    assert_eq!(packages[0].attention[0].reason, AttentionReason::PackageNotInBaseline);
}

#[test]
fn package_local_install_gets_needs_review() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "custom-tool".into(),
            arch: "x86_64".into(),
            state: PackageState::LocalInstall,
            include: true,
            ..Default::default()
        }],
        ..Default::default()
    });

    let packages = inspectah_refine::attention::compute_package_attention(&snap);
    assert_eq!(packages[0].attention[0].level, AttentionLevel::NeedsReview);
    assert_eq!(packages[0].attention[0].reason, AttentionReason::PackageLocalInstall);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-refine --test attention_test -- --nocapture`
Expected: FAIL — module `attention` not found or functions missing

- [ ] **Step 3: Implement attention computation**

Write `inspectah-refine/src/attention.rs`:

```rust
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::config::ConfigFileKind;
use inspectah_core::types::rpm::PackageState;
use crate::types::{AttentionLevel, AttentionReason, AttentionTag, RefinedConfig, RefinedPackage};

const SENSITIVE_PATHS: &[&str] = &[
    "/etc/shadow",
    "/etc/gshadow",
    "/etc/ssh/",
    "/etc/pki/",
    "/etc/ssl/",
    "/etc/security/",
];

fn is_sensitive_path(path: &str) -> bool {
    SENSITIVE_PATHS.iter().any(|s| path.starts_with(s))
}

pub fn compute_package_attention(snap: &InspectionSnapshot) -> Vec<RefinedPackage> {
    let rpm = match &snap.rpm {
        Some(r) => r,
        None => return Vec::new(),
    };

    rpm.packages_added
        .iter()
        .map(|entry| {
            let mut tags = Vec::new();

            match entry.state {
                PackageState::Added => {
                    tags.push(AttentionTag {
                        level: AttentionLevel::NeedsReview,
                        reason: AttentionReason::PackageNotInBaseline,
                        detail: None,
                    });
                }
                PackageState::LocalInstall => {
                    tags.push(AttentionTag {
                        level: AttentionLevel::NeedsReview,
                        reason: AttentionReason::PackageLocalInstall,
                        detail: None,
                    });
                }
                PackageState::Modified => {
                    tags.push(AttentionTag {
                        level: AttentionLevel::Informational,
                        reason: AttentionReason::PackageStateChanged,
                        detail: None,
                    });
                }
                PackageState::NoRepo => {
                    tags.push(AttentionTag {
                        level: AttentionLevel::Informational,
                        reason: AttentionReason::PackageNoRepo,
                        detail: None,
                    });
                }
                _ => {
                    tags.push(AttentionTag {
                        level: AttentionLevel::Routine,
                        reason: AttentionReason::PackageStateChanged,
                        detail: None,
                    });
                }
            }

            RefinedPackage {
                entry: entry.clone(),
                attention: tags,
            }
        })
        .collect()
}

pub fn compute_config_attention(snap: &InspectionSnapshot) -> Vec<RefinedConfig> {
    let config = match &snap.config {
        Some(c) => c,
        None => return Vec::new(),
    };

    config
        .files
        .iter()
        .map(|entry| {
            let mut tags = Vec::new();

            // Kind-based attention
            match entry.kind {
                ConfigFileKind::RpmOwnedModified => {
                    tags.push(AttentionTag {
                        level: AttentionLevel::NeedsReview,
                        reason: AttentionReason::ConfigModified,
                        detail: None,
                    });
                }
                ConfigFileKind::Unowned => {
                    tags.push(AttentionTag {
                        level: AttentionLevel::NeedsReview,
                        reason: AttentionReason::ConfigUnowned,
                        detail: None,
                    });
                }
                ConfigFileKind::Orphaned => {
                    tags.push(AttentionTag {
                        level: AttentionLevel::Informational,
                        reason: AttentionReason::ConfigOrphaned,
                        detail: None,
                    });
                }
                ConfigFileKind::RpmOwnedDefault => {
                    tags.push(AttentionTag {
                        level: AttentionLevel::Routine,
                        reason: AttentionReason::ConfigModified,
                        detail: None,
                    });
                }
            }

            // Sensitive path check (additional tag)
            if is_sensitive_path(&entry.path) {
                tags.push(AttentionTag {
                    level: AttentionLevel::NeedsReview,
                    reason: AttentionReason::SensitivePath,
                    detail: Some(entry.path.clone()),
                });
            }

            RefinedConfig {
                entry: entry.clone(),
                attention: tags,
            }
        })
        .collect()
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p inspectah-refine --test attention_test -- --nocapture`
Expected: PASS

- [ ] **Step 5: Add config attention and sensitive path tests**

Append to `inspectah-refine/tests/attention_test.rs`:

```rust
#[test]
fn config_modified_gets_needs_review() {
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/httpd/conf/httpd.conf".into(),
            kind: ConfigFileKind::RpmOwnedModified,
            include: true,
            ..Default::default()
        }],
    });

    let configs = inspectah_refine::attention::compute_config_attention(&snap);
    assert_eq!(configs[0].attention[0].level, AttentionLevel::NeedsReview);
    assert_eq!(configs[0].attention[0].reason, AttentionReason::ConfigModified);
}

#[test]
fn config_rpm_default_gets_routine() {
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/logrotate.conf".into(),
            kind: ConfigFileKind::RpmOwnedDefault,
            include: true,
            ..Default::default()
        }],
    });

    let configs = inspectah_refine::attention::compute_config_attention(&snap);
    assert_eq!(configs[0].attention[0].level, AttentionLevel::Routine);
}

#[test]
fn sensitive_path_adds_extra_tag() {
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/ssh/sshd_config".into(),
            kind: ConfigFileKind::RpmOwnedModified,
            include: true,
            ..Default::default()
        }],
    });

    let configs = inspectah_refine::attention::compute_config_attention(&snap);
    assert_eq!(configs[0].attention.len(), 2);
    assert!(configs[0].attention.iter().any(|t| t.reason == AttentionReason::SensitivePath));
}

#[test]
fn empty_snapshot_returns_empty_attention() {
    let snap = InspectionSnapshot::new();
    assert!(inspectah_refine::attention::compute_package_attention(&snap).is_empty());
    assert!(inspectah_refine::attention::compute_config_attention(&snap).is_empty());
}
```

- [ ] **Step 6: Run all attention tests**

Run: `cargo test -p inspectah-refine --test attention_test -- --nocapture`
Expected: PASS (6 tests)

- [ ] **Step 7: Commit**

```bash
git add inspectah-refine/src/attention.rs inspectah-refine/tests/attention_test.rs
git commit -m "feat(refine): attention model with heuristic triage tags

Compute AttentionLevel (NeedsReview/Informational/Routine) for
packages and configs based on PackageState, ConfigFileKind, and
sensitive path matching.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 5: Implement RefineSession core — apply, undo, redo, view

**Files:**
- Create: `inspectah-refine/src/session.rs`
- Create: `inspectah-refine/tests/session_test.rs`

This is the largest task. The session owns the original snapshot, the
operation stack, the cursor, the generation counter, and the cached view.

- [ ] **Step 1: Write failing tests for session creation and view**

Create `inspectah-refine/tests/session_test.rs`:

```rust
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind, ConfigSection};
use inspectah_refine::types::{PackageTarget, RefinementOp, RefineError};
use inspectah_refine::session::RefineSession;

fn test_snapshot() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                include: true,
                ..Default::default()
            },
            PackageEntry {
                name: "glibc".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                include: true,
                ..Default::default()
            },
            PackageEntry {
                name: "glibc".into(),
                arch: "i686".into(),
                state: PackageState::Added,
                include: true,
                ..Default::default()
            },
        ],
        ..Default::default()
    });
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/httpd/conf/httpd.conf".into(),
            kind: ConfigFileKind::RpmOwnedModified,
            include: true,
            ..Default::default()
        }],
    });
    snap
}

#[test]
fn new_session_has_generation_zero() {
    let session = RefineSession::new(test_snapshot());
    assert_eq!(session.view().generation, 0);
}

#[test]
fn new_session_is_not_dirty() {
    let session = RefineSession::new(test_snapshot());
    assert!(!session.is_dirty());
}

#[test]
fn new_session_has_correct_stats() {
    let session = RefineSession::new(test_snapshot());
    let view = session.view();
    assert_eq!(view.stats.total_packages, 3);
    assert_eq!(view.stats.included_packages, 3);
    assert_eq!(view.stats.excluded_packages, 0);
    assert_eq!(view.stats.total_configs, 1);
    assert_eq!(view.stats.included_configs, 1);
    assert_eq!(view.stats.excluded_configs, 0);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-refine --test session_test -- --nocapture`
Expected: FAIL — `RefineSession` not found

- [ ] **Step 3: Implement RefineSession struct and new/view**

Write `inspectah-refine/src/session.rs`:

```rust
use std::path::Path;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::renderer::RenderContext;
use inspectah_pipeline::render::containerfile::render_containerfile;
use crate::attention::{compute_config_attention, compute_package_attention};
use crate::types::*;

pub struct RefineSession {
    original: InspectionSnapshot,
    ops: Vec<RefinementOp>,
    cursor: usize,
    cached_view: Option<RefinedView>,
    generation: u64,
}

impl RefineSession {
    pub fn new(snapshot: InspectionSnapshot) -> Self {
        let mut session = Self {
            original: snapshot,
            ops: Vec::new(),
            cursor: 0,
            cached_view: None,
            generation: 0,
        };
        // Eagerly compute initial view
        session.recompute_view();
        session
    }

    pub fn view(&self) -> &RefinedView {
        self.cached_view
            .as_ref()
            .expect("view is always computed after new() or mutation")
    }

    pub fn apply(&mut self, op: RefinementOp) -> Result<(), RefineError> {
        // Validate target exists
        self.validate_target(&op)?;

        // Check idempotency
        if self.is_op_noop(&op) {
            return Ok(());
        }

        // Truncate redo history at cursor
        self.ops.truncate(self.cursor);
        self.ops.push(op);
        self.cursor += 1;
        self.generation += 1;
        self.cached_view = None;
        self.recompute_view();
        Ok(())
    }

    pub fn undo(&mut self) -> Result<(), RefineError> {
        if self.cursor == 0 {
            return Err(RefineError::NothingToUndo);
        }
        self.cursor -= 1;
        self.generation += 1;
        self.cached_view = None;
        self.recompute_view();
        Ok(())
    }

    pub fn redo(&mut self) -> Result<(), RefineError> {
        if self.cursor >= self.ops.len() {
            return Err(RefineError::NothingToRedo);
        }
        self.cursor += 1;
        self.generation += 1;
        self.cached_view = None;
        self.recompute_view();
        Ok(())
    }

    pub fn ops_history(&self) -> Vec<AnnotatedOp> {
        self.ops
            .iter()
            .enumerate()
            .map(|(i, op)| AnnotatedOp {
                op: op.clone(),
                active: i < self.cursor,
            })
            .collect()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn can_undo(&self) -> bool {
        self.cursor > 0
    }

    pub fn can_redo(&self) -> bool {
        self.cursor < self.ops.len()
    }

    pub fn pending_changes(&self) -> ChangesSummary {
        let projected = self.project_snapshot();
        let mut packages_included = Vec::new();
        let mut packages_excluded = Vec::new();
        let mut configs_included = Vec::new();
        let mut configs_excluded = Vec::new();

        if let (Some(orig_rpm), Some(proj_rpm)) = (&self.original.rpm, &projected.rpm) {
            for (orig, proj) in orig_rpm.packages_added.iter().zip(&proj_rpm.packages_added) {
                let target = PackageTarget {
                    name: orig.name.clone(),
                    arch: orig.arch.clone(),
                };
                if orig.include != proj.include {
                    if proj.include {
                        packages_included.push(target);
                    } else {
                        packages_excluded.push(target);
                    }
                }
            }
        }

        if let (Some(orig_cfg), Some(proj_cfg)) = (&self.original.config, &projected.config) {
            for (orig, proj) in orig_cfg.files.iter().zip(&proj_cfg.files) {
                if orig.include != proj.include {
                    if proj.include {
                        configs_included.push(orig.path.clone());
                    } else {
                        configs_excluded.push(orig.path.clone());
                    }
                }
            }
        }

        let is_dirty = !packages_included.is_empty()
            || !packages_excluded.is_empty()
            || !configs_included.is_empty()
            || !configs_excluded.is_empty();

        ChangesSummary {
            packages_included,
            packages_excluded,
            configs_included,
            configs_excluded,
            is_dirty,
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.pending_changes().is_dirty
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Snapshot the current projected state. Returns an owned clone.
    /// Used by the HTTP layer to snapshot under the lock and then
    /// release the lock before doing expensive export work.
    pub fn snapshot_projected(&self) -> InspectionSnapshot {
        self.project_snapshot()
    }

    pub fn export_tarball(
        &self,
        path: &Path,
        expected_generation: u64,
    ) -> Result<(), RefineError> {
        if expected_generation != self.generation {
            return Err(RefineError::StaleGeneration {
                expected: expected_generation,
                actual: self.generation,
            });
        }

        let projected = self.project_snapshot();
        render_refine_export(&projected, path)
    }

    // --- Private helpers ---

    fn validate_target(&self, op: &RefinementOp) -> Result<(), RefineError> {
        match op {
            RefinementOp::ExcludePackage(target) | RefinementOp::IncludePackage(target) => {
                let found = self
                    .original
                    .rpm
                    .as_ref()
                    .map(|r| r.packages_added.iter().any(|e| target.matches(e)))
                    .unwrap_or(false);
                if !found {
                    return Err(RefineError::UnknownTarget(target.to_string()));
                }
            }
            RefinementOp::ExcludeConfig { path } | RefinementOp::IncludeConfig { path } => {
                let found = self
                    .original
                    .config
                    .as_ref()
                    .map(|c| c.files.iter().any(|e| e.path == path.to_string_lossy()))
                    .unwrap_or(false);
                if !found {
                    return Err(RefineError::UnknownTarget(
                        path.to_string_lossy().to_string(),
                    ));
                }
            }
        }
        Ok(())
    }

    fn is_op_noop(&self, op: &RefinementOp) -> bool {
        let projected = self.project_snapshot();
        match op {
            RefinementOp::ExcludePackage(target) => {
                projected
                    .rpm
                    .as_ref()
                    .and_then(|r| r.packages_added.iter().find(|e| target.matches(e)))
                    .map(|e| !e.include) // already excluded = noop
                    .unwrap_or(false)
            }
            RefinementOp::IncludePackage(target) => {
                projected
                    .rpm
                    .as_ref()
                    .and_then(|r| r.packages_added.iter().find(|e| target.matches(e)))
                    .map(|e| e.include) // already included = noop
                    .unwrap_or(false)
            }
            RefinementOp::ExcludeConfig { path } => {
                projected
                    .config
                    .as_ref()
                    .and_then(|c| c.files.iter().find(|e| e.path == path.to_string_lossy()))
                    .map(|e| !e.include)
                    .unwrap_or(false)
            }
            RefinementOp::IncludeConfig { path } => {
                projected
                    .config
                    .as_ref()
                    .and_then(|c| c.files.iter().find(|e| e.path == path.to_string_lossy()))
                    .map(|e| e.include)
                    .unwrap_or(false)
            }
        }
    }

    fn project_snapshot(&self) -> InspectionSnapshot {
        let mut snap = self.original.clone();

        for op in &self.ops[..self.cursor] {
            match op {
                RefinementOp::ExcludePackage(target) => {
                    if let Some(ref mut rpm) = snap.rpm {
                        if let Some(pkg) = rpm.packages_added.iter_mut().find(|e| target.matches(e)) {
                            pkg.include = false;
                        }
                    }
                }
                RefinementOp::IncludePackage(target) => {
                    if let Some(ref mut rpm) = snap.rpm {
                        if let Some(pkg) = rpm.packages_added.iter_mut().find(|e| target.matches(e)) {
                            pkg.include = true;
                        }
                    }
                }
                RefinementOp::ExcludeConfig { path } => {
                    if let Some(ref mut config) = snap.config {
                        if let Some(entry) = config.files.iter_mut().find(|e| e.path == path.to_string_lossy()) {
                            entry.include = false;
                        }
                    }
                }
                RefinementOp::IncludeConfig { path } => {
                    if let Some(ref mut config) = snap.config {
                        if let Some(entry) = config.files.iter_mut().find(|e| e.path == path.to_string_lossy()) {
                            entry.include = true;
                        }
                    }
                }
            }
        }

        snap
    }

    fn recompute_view(&mut self) {
        let projected = self.project_snapshot();
        let packages = compute_package_attention(&projected);
        let config_files = compute_config_attention(&projected);

        // Preview must use the SAME root derivation as export to guarantee
        // byte-identical Containerfiles. The renderer's single source of
        // truth for COPY roots is the materialized config tree (which
        // includes repo files, GPG keys, firewall zones, etc. beyond
        // config.files). We materialize to a tempdir, read the roots,
        // render the Containerfile, then drop the tempdir.
        let preview_dir = tempfile::tempdir().expect("tempdir for preview");
        let materialized_roots =
            inspectah_pipeline::render::configtree::write_config_tree(
                &projected, preview_dir.path(),
            )
            .unwrap_or_default();
        let containerfile_preview =
            render_containerfile(&projected, Some(&materialized_roots));
        drop(preview_dir);

        let stats = RefineStats {
            total_packages: packages.len(),
            included_packages: packages.iter().filter(|p| p.entry.include).count(),
            excluded_packages: packages.iter().filter(|p| !p.entry.include).count(),
            total_configs: config_files.len(),
            included_configs: config_files.iter().filter(|c| c.entry.include).count(),
            excluded_configs: config_files.iter().filter(|c| !c.entry.include).count(),
            needs_review_count: packages
                .iter()
                .filter(|p| {
                    p.attention
                        .iter()
                        .any(|t| t.level == AttentionLevel::NeedsReview)
                })
                .count()
                + config_files
                    .iter()
                    .filter(|c| {
                        c.attention
                            .iter()
                            .any(|t| t.level == AttentionLevel::NeedsReview)
                    })
                    .count(),
            ops_applied: self.cursor,
            can_undo: self.can_undo(),
            can_redo: self.can_redo(),
        };

        self.cached_view = Some(RefinedView {
            packages,
            config_files,
            containerfile_preview,
            stats,
            generation: self.generation,
        });
    }
}

/// Render exactly the approved refine export file set to a tarball.
///
/// This is NOT `render_all()`. The pipeline's `render_all()` writes 8
/// artifacts (including report.html, README.md, secrets-review.md,
/// kickstart-suggestion.ks) that are outside the approved refine export
/// contract. This function materializes only the contracted set:
///
/// Required: inspection-snapshot.json, Containerfile, audit-report.md,
///           schema/snapshot.schema.json
/// Conditional: config/ (when snapshot has included config files),
///              env-files/ (when snapshot has env-file data)
/// Excluded: report.html, README.md, secrets-review.md,
///           kickstart-suggestion.ks, original-inspection-snapshot.json
///
/// Preview/export fidelity: both paths use `render_containerfile(snap,
/// Some(&materialized_roots))` with the same materialized root set, so
/// the exported Containerfile is byte-identical to what the preview shows.
fn render_refine_export(
    snap: &InspectionSnapshot,
    tarball_path: &Path,
) -> Result<(), RefineError> {
    let tempdir = tempfile::tempdir()
        .map_err(|e| RefineError::TarballError(e.to_string()))?;
    let out = tempdir.path();

    // 1. Materialize config tree FIRST — gives us materialized_roots,
    //    the renderer's single source of truth for COPY lines.
    let materialized_roots =
        inspectah_pipeline::render::configtree::write_config_tree(snap, out)
            .map_err(|e| RefineError::RenderFailed(e.to_string()))?;

    // 2. Materialize env-files (conditional)
    inspectah_pipeline::render::configtree::write_env_files(snap, out)
        .map_err(|e| RefineError::RenderFailed(e.to_string()))?;

    // 3. Containerfile — uses materialized_roots from the SAME config
    //    tree write that populated the tarball's config/ directory.
    //    Preview also materializes to a tempdir for the same roots,
    //    guaranteeing byte-identical output.
    let containerfile = render_containerfile(snap, Some(&materialized_roots));
    std::fs::write(out.join("Containerfile"), containerfile)?;

    // 4. audit-report.md
    let audit = inspectah_pipeline::render::audit::render_audit(snap);
    std::fs::write(out.join("audit-report.md"), audit)?;

    // 5. inspection-snapshot.json (projected)
    let snap_json = serde_json::to_string_pretty(snap)
        .map_err(|e| RefineError::TarballError(e.to_string()))?;
    std::fs::write(out.join("inspection-snapshot.json"), snap_json)?;

    // 6. schema/snapshot.schema.json (placeholder — same as scan.rs)
    let schema_dir = out.join("schema");
    std::fs::create_dir_all(&schema_dir)?;
    std::fs::write(
        schema_dir.join("snapshot.schema.json"),
        r#"{"$schema":"http://json-schema.org/draft-07/schema#","title":"InspectionSnapshot","description":"Phase 7 placeholder","type":"object"}"#,
    )?;

    // 7. Create flat tarball (no prefix subdirectory)
    create_flat_tarball(out, tarball_path)?;

    Ok(())
}

/// Create a flat tarball (no prefix directory) from a source directory.
fn create_flat_tarball(source_dir: &Path, tarball_path: &Path) -> Result<(), RefineError> {
    let f = std::fs::File::create(tarball_path)?;
    let gz = flate2::write::GzEncoder::new(f, flate2::Compression::default());
    let mut tar = tar::Builder::new(gz);

    let mut paths: Vec<_> = walkdir::WalkDir::new(source_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path() != source_dir)
        .map(|e| e.into_path())
        .collect();
    paths.sort();

    for path in &paths {
        let rel = path.strip_prefix(source_dir)
            .map_err(|e| RefineError::TarballError(e.to_string()))?;
        if path.is_dir() {
            tar.append_dir(rel, path)
                .map_err(|e| RefineError::TarballError(e.to_string()))?;
        } else {
            tar.append_path_with_name(path, rel)
                .map_err(|e| RefineError::TarballError(e.to_string()))?;
        }
    }

    tar.finish().map_err(|e| RefineError::TarballError(e.to_string()))?;
    Ok(())
}
```

Note: add `walkdir = "2"`, `tar = "0.4"`, and `flate2 = "0.2"` to
`inspectah-refine/Cargo.toml` `[dependencies]` (not just dev-dependencies,
since `export_tarball` uses them at runtime).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p inspectah-refine --test session_test -- --nocapture`
Expected: PASS (3 tests)

- [ ] **Step 5: Add apply, undo, redo, and edge case tests**

Append to `inspectah-refine/tests/session_test.rs`:

```rust
use std::path::PathBuf;

#[test]
fn apply_exclude_package() {
    let mut session = RefineSession::new(test_snapshot());
    session
        .apply(RefinementOp::ExcludePackage(PackageTarget {
            name: "httpd".into(),
            arch: "x86_64".into(),
        }))
        .unwrap();

    assert_eq!(session.view().generation, 1);
    assert_eq!(session.view().stats.excluded_packages, 1);
    assert_eq!(session.view().stats.included_packages, 2);
    assert!(session.is_dirty());
}

#[test]
fn apply_unknown_target_returns_error() {
    let mut session = RefineSession::new(test_snapshot());
    let result = session.apply(RefinementOp::ExcludePackage(PackageTarget {
        name: "nonexistent".into(),
        arch: "x86_64".into(),
    }));
    assert!(matches!(result, Err(RefineError::UnknownTarget(_))));
}

#[test]
fn apply_wrong_arch_returns_error() {
    let mut session = RefineSession::new(test_snapshot());
    let result = session.apply(RefinementOp::ExcludePackage(PackageTarget {
        name: "glibc".into(),
        arch: "s390x".into(),
    }));
    assert!(matches!(result, Err(RefineError::UnknownTarget(_))));
}

#[test]
fn idempotent_exclude_is_noop() {
    let mut session = RefineSession::new(test_snapshot());
    session
        .apply(RefinementOp::ExcludePackage(PackageTarget {
            name: "httpd".into(),
            arch: "x86_64".into(),
        }))
        .unwrap();
    let gen_after_first = session.view().generation;

    // Second exclude of the same target should be a no-op
    session
        .apply(RefinementOp::ExcludePackage(PackageTarget {
            name: "httpd".into(),
            arch: "x86_64".into(),
        }))
        .unwrap();

    assert_eq!(session.view().generation, gen_after_first);
    assert_eq!(session.ops_history().len(), 1);
}

#[test]
fn undo_reverts_to_original() {
    let mut session = RefineSession::new(test_snapshot());
    session
        .apply(RefinementOp::ExcludePackage(PackageTarget {
            name: "httpd".into(),
            arch: "x86_64".into(),
        }))
        .unwrap();
    session.undo().unwrap();

    assert!(!session.is_dirty());
    assert_eq!(session.view().stats.excluded_packages, 0);
    assert_eq!(session.view().generation, 2); // apply=1, undo=2
}

#[test]
fn undo_on_empty_returns_error() {
    let mut session = RefineSession::new(test_snapshot());
    assert!(matches!(session.undo(), Err(RefineError::NothingToUndo)));
}

#[test]
fn redo_after_undo() {
    let mut session = RefineSession::new(test_snapshot());
    session
        .apply(RefinementOp::ExcludePackage(PackageTarget {
            name: "httpd".into(),
            arch: "x86_64".into(),
        }))
        .unwrap();
    session.undo().unwrap();
    session.redo().unwrap();

    assert!(session.is_dirty());
    assert_eq!(session.view().stats.excluded_packages, 1);
    assert_eq!(session.view().generation, 3);
}

#[test]
fn redo_with_nothing_undone_returns_error() {
    let mut session = RefineSession::new(test_snapshot());
    assert!(matches!(session.redo(), Err(RefineError::NothingToRedo)));
}

#[test]
fn apply_after_undo_truncates_redo_history() {
    let mut session = RefineSession::new(test_snapshot());
    session
        .apply(RefinementOp::ExcludePackage(PackageTarget {
            name: "httpd".into(),
            arch: "x86_64".into(),
        }))
        .unwrap();
    session.undo().unwrap();

    // Apply a different op — should truncate the undone op
    session
        .apply(RefinementOp::ExcludeConfig {
            path: PathBuf::from("/etc/httpd/conf/httpd.conf"),
        })
        .unwrap();

    assert!(matches!(session.redo(), Err(RefineError::NothingToRedo)));
    assert_eq!(session.ops_history().len(), 1);
}

#[test]
fn multiarch_targeting() {
    let mut session = RefineSession::new(test_snapshot());
    session
        .apply(RefinementOp::ExcludePackage(PackageTarget {
            name: "glibc".into(),
            arch: "i686".into(),
        }))
        .unwrap();

    let view = session.view();
    let glibc_x86 = view
        .packages
        .iter()
        .find(|p| p.entry.name == "glibc" && p.entry.arch == "x86_64")
        .unwrap();
    let glibc_i686 = view
        .packages
        .iter()
        .find(|p| p.entry.name == "glibc" && p.entry.arch == "i686")
        .unwrap();

    assert!(glibc_x86.entry.include);
    assert!(!glibc_i686.entry.include);
}

#[test]
fn exclude_config_file() {
    let mut session = RefineSession::new(test_snapshot());
    session
        .apply(RefinementOp::ExcludeConfig {
            path: PathBuf::from("/etc/httpd/conf/httpd.conf"),
        })
        .unwrap();

    let view = session.view();
    assert_eq!(view.stats.excluded_configs, 1);
    assert!(session.is_dirty());
}

#[test]
fn pending_changes_tracks_excludes() {
    let mut session = RefineSession::new(test_snapshot());
    session
        .apply(RefinementOp::ExcludePackage(PackageTarget {
            name: "httpd".into(),
            arch: "x86_64".into(),
        }))
        .unwrap();

    let changes = session.pending_changes();
    assert_eq!(changes.packages_excluded.len(), 1);
    assert_eq!(changes.packages_excluded[0].name, "httpd");
    assert!(changes.is_dirty);
}

#[test]
fn exclude_then_include_returns_to_clean() {
    let mut session = RefineSession::new(test_snapshot());
    session
        .apply(RefinementOp::ExcludePackage(PackageTarget {
            name: "httpd".into(),
            arch: "x86_64".into(),
        }))
        .unwrap();
    session
        .apply(RefinementOp::IncludePackage(PackageTarget {
            name: "httpd".into(),
            arch: "x86_64".into(),
        }))
        .unwrap();

    // State-based dirty: not dirty because state matches original
    assert!(!session.is_dirty());
}

#[test]
fn undo_all_then_redo_all() {
    let mut session = RefineSession::new(test_snapshot());
    session
        .apply(RefinementOp::ExcludePackage(PackageTarget {
            name: "httpd".into(),
            arch: "x86_64".into(),
        }))
        .unwrap();
    session
        .apply(RefinementOp::ExcludeConfig {
            path: PathBuf::from("/etc/httpd/conf/httpd.conf"),
        })
        .unwrap();

    let view_after_ops = session.view().clone();

    session.undo().unwrap();
    session.undo().unwrap();
    assert!(!session.is_dirty());

    session.redo().unwrap();
    session.redo().unwrap();

    // Stats should match the fully-applied state
    assert_eq!(
        session.view().stats.excluded_packages,
        view_after_ops.stats.excluded_packages
    );
    assert_eq!(
        session.view().stats.excluded_configs,
        view_after_ops.stats.excluded_configs
    );
}

#[test]
fn stale_generation_export_rejected() {
    let session = RefineSession::new(test_snapshot());
    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("output.tar.gz");

    let result = session.export_tarball(&tarball_path, 999);
    assert!(matches!(
        result,
        Err(RefineError::StaleGeneration {
            expected: 999,
            actual: 0
        })
    ));
}
```

- [ ] **Step 6: Run all session tests**

Run: `cargo test -p inspectah-refine --test session_test -- --nocapture`
Expected: PASS (16 tests)

- [ ] **Step 7: Commit**

```bash
git add inspectah-refine/src/session.rs inspectah-refine/tests/session_test.rs
git commit -m "feat(refine): RefineSession with apply/undo/redo and view projection

Session owns original snapshot and ops stack. View projection replays
ops against a snapshot clone. State-based dirty tracking. Generation
counter for stale-state detection. Export requires matching generation.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 6: Implement tarball loading with archive safety and provenance gate

**Files:**
- Create: `inspectah-refine/src/tarball.rs`
- Create: `inspectah-refine/tests/tarball_test.rs`

- [ ] **Step 1: Write failing test for flat tarball loading**

Create `inspectah-refine/tests/tarball_test.rs`:

```rust
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::redaction::RedactionState;
use inspectah_refine::types::RefineError;
use std::io::Write;
use tempfile::tempdir;

fn make_test_snapshot(redaction: Option<RedactionState>) -> String {
    let mut snap = InspectionSnapshot::new();
    snap.redaction_state = redaction;
    serde_json::to_string_pretty(&snap).unwrap()
}

fn write_flat_tarball(dir: &std::path::Path, snap_json: &str) -> std::path::PathBuf {
    let snap_path = dir.join("inspection-snapshot.json");
    std::fs::write(&snap_path, snap_json).unwrap();

    let tarball_path = dir.join("test.tar.gz");
    let f = std::fs::File::create(&tarball_path).unwrap();
    let gz = flate2::write::GzEncoder::new(f, flate2::Compression::default());
    let mut tar = tar::Builder::new(gz);
    tar.append_path_with_name(&snap_path, "inspection-snapshot.json")
        .unwrap();
    tar.finish().unwrap();
    tarball_path
}

fn write_prefixed_tarball(dir: &std::path::Path, snap_json: &str) -> std::path::PathBuf {
    let snap_path = dir.join("inspection-snapshot.json");
    std::fs::write(&snap_path, snap_json).unwrap();

    let tarball_path = dir.join("test.tar.gz");
    let f = std::fs::File::create(&tarball_path).unwrap();
    let gz = flate2::write::GzEncoder::new(f, flate2::Compression::default());
    let mut tar = tar::Builder::new(gz);
    tar.append_path_with_name(
        &snap_path,
        "hostname-20260515-1430/inspection-snapshot.json",
    )
    .unwrap();
    tar.finish().unwrap();
    tarball_path
}

#[test]
fn load_flat_tarball() {
    let dir = tempdir().unwrap();
    let snap_json = make_test_snapshot(Some(RedactionState::FullyRedacted {
        redacted_by: "inspectah 0.8.0".into(),
        config_hash: "abc123".into(),
    }));
    let tarball = write_flat_tarball(dir.path(), &snap_json);

    let session = inspectah_refine::tarball::from_tarball(&tarball).unwrap();
    assert_eq!(session.view().generation, 0);
}

#[test]
fn load_prefixed_tarball() {
    let dir = tempdir().unwrap();
    let snap_json = make_test_snapshot(Some(RedactionState::FullyRedacted {
        redacted_by: "inspectah 0.8.0".into(),
        config_hash: "abc123".into(),
    }));
    let tarball = write_prefixed_tarball(dir.path(), &snap_json);

    let session = inspectah_refine::tarball::from_tarball(&tarball).unwrap();
    assert_eq!(session.view().generation, 0);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-refine --test tarball_test -- --nocapture`
Expected: FAIL — `from_tarball` not found

- [ ] **Step 3: Add tar and flate2 dependencies**

Add to `inspectah-refine/Cargo.toml` under `[dependencies]`:

```toml
tar = "0.4"
flate2 = "0.2"
```

And under `[dev-dependencies]`:

```toml
tar = "0.4"
flate2 = "0.2"
```

Check workspace — if tar/flate2 are already workspace deps, use `.workspace = true`.
Otherwise add them as direct deps.

- [ ] **Step 4: Implement from_tarball**

Write `inspectah-refine/src/tarball.rs`:

```rust
use std::path::Path;
use inspectah_core::types::redaction::RedactionState;
use crate::normalize::load_for_refine;
use crate::session::RefineSession;
use crate::types::RefineError;

const MAX_UNPACKED_SIZE: u64 = 512 * 1024 * 1024; // 512 MiB
const MAX_FILE_COUNT: usize = 10_000;
const MAX_SINGLE_FILE: u64 = 256 * 1024 * 1024; // 256 MiB

pub fn from_tarball(path: &Path) -> Result<RefineSession, RefineError> {
    let tempdir = tempfile::tempdir()
        .map_err(|e| RefineError::TarballError(e.to_string()))?;

    // Extract with safety checks
    extract_safe(path, tempdir.path())?;

    // Flatten prefixed archives
    let root = flatten_if_needed(tempdir.path())?;

    // Load snapshot
    let snap_path = root.join("inspection-snapshot.json");
    if !snap_path.exists() {
        return Err(RefineError::SnapshotLoad(
            "missing inspection-snapshot.json".into(),
        ));
    }

    let snap_json = std::fs::read_to_string(&snap_path)?;

    // load_for_refine handles the full pipeline:
    // raw-JSON include patching → deserialize → schema migration
    let snapshot = load_for_refine(&snap_json)?;

    // Check provenance — FullyRedacted only
    validate_provenance(&snapshot)?;

    Ok(RefineSession::new(snapshot))
}

fn extract_safe(tarball_path: &Path, dest: &Path) -> Result<(), RefineError> {
    let file = std::fs::File::open(tarball_path)?;
    let gz = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);

    let mut total_size: u64 = 0;
    let mut file_count: usize = 0;

    for entry_result in archive.entries()
        .map_err(|e| RefineError::TarballError(e.to_string()))? {

        let mut entry = entry_result
            .map_err(|e| RefineError::TarballError(e.to_string()))?;

        file_count += 1;
        if file_count > MAX_FILE_COUNT {
            return Err(RefineError::ArchiveSafety(
                format!("archive exceeds {MAX_FILE_COUNT} entry limit"),
            ));
        }

        let path = entry
            .path()
            .map_err(|e| RefineError::TarballError(e.to_string()))?
            .to_path_buf();

        // Path traversal check
        for component in path.components() {
            if let std::path::Component::ParentDir = component {
                return Err(RefineError::ArchiveSafety(
                    format!("path traversal: {}", path.display()),
                ));
            }
        }
        if path.is_absolute() {
            return Err(RefineError::ArchiveSafety(
                format!("path traversal: absolute path {}", path.display()),
            ));
        }

        // Entry type check
        let entry_type = entry.header().entry_type();
        if !matches!(entry_type, tar::EntryType::Regular | tar::EntryType::Directory) {
            return Err(RefineError::ArchiveSafety(
                format!("unsupported entry type: {:?} for {}", entry_type, path.display()),
            ));
        }

        // Single file size check
        let size = entry.header().size()
            .map_err(|e| RefineError::TarballError(e.to_string()))?;
        if size > MAX_SINGLE_FILE {
            return Err(RefineError::ArchiveSafety(
                format!("single file exceeds {} MiB", MAX_SINGLE_FILE / (1024 * 1024)),
            ));
        }

        total_size += size;
        if total_size > MAX_UNPACKED_SIZE {
            return Err(RefineError::ArchiveSafety(
                format!("archive exceeds {} MiB unpacked limit", MAX_UNPACKED_SIZE / (1024 * 1024)),
            ));
        }

        entry
            .unpack_in(dest)
            .map_err(|e| RefineError::TarballError(e.to_string()))?;
    }

    Ok(())
}

fn flatten_if_needed(dir: &Path) -> Result<std::path::PathBuf, RefineError> {
    // Check if extraction produced a single subdirectory
    let entries: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .collect();

    if entries.len() == 1 && entries[0].file_type().map(|t| t.is_dir()).unwrap_or(false) {
        let subdir = entries[0].path();
        // Check if the snapshot is inside the subdirectory
        if subdir.join("inspection-snapshot.json").exists() {
            return Ok(subdir);
        }
    }

    Ok(dir.to_path_buf())
}

fn validate_provenance(snap: &InspectionSnapshot) -> Result<(), RefineError> {
    match &snap.redaction_state {
        Some(RedactionState::FullyRedacted { .. }) => Ok(()),
        _ => Err(RefineError::UntrustedSnapshot(
            "Snapshot has not been fully redacted. Run inspectah scan to produce a redacted snapshot.".into(),
        )),
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p inspectah-refine --test tarball_test -- --nocapture`
Expected: PASS

- [ ] **Step 5: Add provenance rejection and archive safety tests**

Append to `inspectah-refine/tests/tarball_test.rs`:

```rust
#[test]
fn reject_raw_redaction_state() {
    let dir = tempdir().unwrap();
    let snap_json = make_test_snapshot(Some(RedactionState::Raw));
    let tarball = write_flat_tarball(dir.path(), &snap_json);

    let result = inspectah_refine::tarball::from_tarball(&tarball);
    assert!(matches!(result, Err(RefineError::UntrustedSnapshot(_))));
}

#[test]
fn reject_partially_redacted() {
    let dir = tempdir().unwrap();
    let snap_json = make_test_snapshot(Some(RedactionState::PartiallyRedacted {
        redacted_by: "inspectah 0.8.0".into(),
        config_hash: "abc123".into(),
        unresolved_count: 2,
        unresolved_hints: Vec::new(),
    }));
    let tarball = write_flat_tarball(dir.path(), &snap_json);

    let result = inspectah_refine::tarball::from_tarball(&tarball);
    assert!(matches!(result, Err(RefineError::UntrustedSnapshot(_))));
}

#[test]
fn reject_unknown_redaction_state() {
    let dir = tempdir().unwrap();
    let snap_json = make_test_snapshot(Some(RedactionState::Unknown));
    let tarball = write_flat_tarball(dir.path(), &snap_json);

    let result = inspectah_refine::tarball::from_tarball(&tarball);
    assert!(matches!(result, Err(RefineError::UntrustedSnapshot(_))));
}

#[test]
fn reject_absent_redaction_state() {
    let dir = tempdir().unwrap();
    let snap_json = make_test_snapshot(None);
    let tarball = write_flat_tarball(dir.path(), &snap_json);

    let result = inspectah_refine::tarball::from_tarball(&tarball);
    assert!(matches!(result, Err(RefineError::UntrustedSnapshot(_))));
}

#[test]
fn reject_path_traversal() {
    let dir = tempdir().unwrap();
    let tarball_path = dir.path().join("evil.tar.gz");
    let f = std::fs::File::create(&tarball_path).unwrap();
    let gz = flate2::write::GzEncoder::new(f, flate2::Compression::default());
    let mut tar = tar::Builder::new(gz);

    // Add a file with path traversal
    let content = b"evil content";
    let mut header = tar::Header::new_gnu();
    header.set_path("../escape.txt").unwrap();
    header.set_size(content.len() as u64);
    header.set_cksum();
    tar.append(&header, &content[..]).unwrap();
    tar.finish().unwrap();

    let result = inspectah_refine::tarball::from_tarball(&tarball_path);
    assert!(matches!(result, Err(RefineError::ArchiveSafety(_))));
}

#[test]
fn reject_missing_snapshot_json() {
    let dir = tempdir().unwrap();
    let tarball_path = dir.path().join("empty.tar.gz");
    let f = std::fs::File::create(&tarball_path).unwrap();
    let gz = flate2::write::GzEncoder::new(f, flate2::Compression::default());
    let mut tar = tar::Builder::new(gz);

    let content = b"not a snapshot";
    let mut header = tar::Header::new_gnu();
    header.set_path("other-file.txt").unwrap();
    header.set_size(content.len() as u64);
    header.set_cksum();
    tar.append(&header, &content[..]).unwrap();
    tar.finish().unwrap();

    let result = inspectah_refine::tarball::from_tarball(&tarball_path);
    assert!(matches!(result, Err(RefineError::SnapshotLoad(_))));
}
```

- [ ] **Step 6: Add old-schema migration fixture test**

Append to `inspectah-refine/tests/tarball_test.rs`:

```rust
#[test]
fn load_v12_schema_tarball_migrates() {
    // v12 snapshot — the minimum schema version load() accepts.
    // If migrate() were removed, this snapshot would still load but
    // would not have current-version fields populated correctly.
    let dir = tempdir().unwrap();
    let snap_json = r#"{
        "schema_version": 12,
        "redaction_state": {
            "state": "fully_redacted",
            "redacted_by": "inspectah 0.7.0",
            "config_hash": "old123"
        },
        "rpm": {
            "packages_added": [
                {"name": "httpd", "arch": "x86_64", "state": "added", "include": true}
            ]
        }
    }"#;
    let tarball = write_flat_tarball(dir.path(), snap_json);

    let session = inspectah_refine::tarball::from_tarball(&tarball).unwrap();
    // Session loads successfully after migration
    assert_eq!(session.view().generation, 0);
    assert_eq!(session.view().stats.total_packages, 1);
}
```

- [ ] **Step 7: Run all tarball tests**

Run: `cargo test -p inspectah-refine --test tarball_test -- --nocapture`
Expected: PASS (9 tests)

- [ ] **Step 8: Commit**

```bash
git add inspectah-refine/src/tarball.rs inspectah-refine/tests/tarball_test.rs inspectah-refine/Cargo.toml
git commit -m "feat(refine): tarball loading with archive safety, provenance, and migration

from_tarball() extracts, flattens prefixed archives, loads via
load_for_refine (presence-aware include defaulting + schema migration),
validates FullyRedacted provenance, creates RefineSession.
Archive safety: path traversal, symlink rejection, size limits.
Includes v12 old-schema migration fixture.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 7: Wire up inspectah-web with axum HTTP API

**Files:**
- Modify: `inspectah-web/Cargo.toml`
- Create: `inspectah-web/src/lib.rs` (replace empty file)
- Create: `inspectah-web/src/handlers.rs`
- Create: `inspectah-web/src/error.rs`
- Create: `inspectah-web/src/assets.rs`
- Create: `inspectah-web/tests/api_test.rs`

- [ ] **Step 1: Update inspectah-web Cargo.toml**

Replace `inspectah-web/Cargo.toml`:

```toml
[package]
name = "inspectah-web"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
inspectah-core = { path = "../inspectah-core" }
inspectah-refine = { path = "../inspectah-refine" }
axum = "0.8"
tower-http = { version = "0.6", features = ["cors"] }
rust-embed = "8"
serde.workspace = true
serde_json.workspace = true
tokio = { version = "1", features = ["full"] }
mime_guess = "2"

[dev-dependencies]
tempfile.workspace = true
tower = { version = "0.5", features = ["util"] }
http-body-util = "0.1"
hyper = "1"
```

- [ ] **Step 2: Write the error mapper**

Create `inspectah-web/src/error.rs`:

```rust
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use inspectah_refine::types::RefineError;
use serde_json::json;

pub struct AppError(pub RefineError);

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self.0 {
            RefineError::UnknownTarget(t) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                format!("unknown target: {t}"),
            ),
            RefineError::NothingToUndo => (
                StatusCode::CONFLICT,
                "nothing to undo".into(),
            ),
            RefineError::NothingToRedo => (
                StatusCode::CONFLICT,
                "nothing to redo".into(),
            ),
            RefineError::StaleGeneration { expected, actual } => (
                StatusCode::CONFLICT,
                format!("stale generation: expected {expected}, got {actual}"),
            ),
            RefineError::UntrustedSnapshot(msg) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                msg.clone(),
            ),
            RefineError::ArchiveSafety(msg) => (
                StatusCode::BAD_REQUEST,
                msg.clone(),
            ),
            RefineError::BadRequest(msg) => (
                StatusCode::BAD_REQUEST,
                msg.clone(),
            ),
            // Internal errors — do not leak details
            _ => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal error".into(),
            ),
        };

        (status, Json(json!({ "error": message }))).into_response()
    }
}
```

- [ ] **Step 3: Write the static asset server**

Create `inspectah-web/src/assets.rs`:

```rust
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "static/"]
pub struct StaticAssets;

pub async fn serve_report() -> Response {
    match StaticAssets::get("index.html") {
        Some(content) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            content.data.to_vec(),
        )
            .into_response(),
        None => (StatusCode::OK, "inspectah refine server running").into_response(),
    }
}

pub async fn serve_static(
    axum::extract::Path(path): axum::extract::Path<String>,
) -> Response {
    match StaticAssets::get(&path) {
        Some(content) => {
            let mime = mime_guess::from_path(&path)
                .first_or_octet_stream()
                .to_string();
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, mime)],
                content.data.to_vec(),
            )
                .into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
```

- [ ] **Step 4: Create placeholder static assets directory**

```bash
mkdir -p inspectah-web/static
echo '<!DOCTYPE html><html><head><title>inspectah refine</title></head><body><h1>inspectah refine</h1><p>Web UI coming soon.</p></body></html>' > inspectah-web/static/index.html
```

- [ ] **Step 5: Write the HTTP handlers**

Create `inspectah-web/src/handlers.rs`:

```rust
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::RefinementOp;
use serde::Deserialize;
use serde_json::json;
use std::sync::{Arc, Mutex};

use crate::error::AppError;

pub type AppState = Arc<Mutex<RefineSession>>;

pub async fn health() -> Json<serde_json::Value> {
    Json(json!({"status": "ok"}))
}

pub async fn get_view(State(state): State<AppState>) -> impl IntoResponse {
    let session = state.lock().unwrap();
    Json(serde_json::to_value(session.view()).unwrap())
}

pub async fn apply_op(
    State(state): State<AppState>,
    Json(op): Json<RefinementOp>,
) -> Result<impl IntoResponse, AppError> {
    let mut session = state.lock().unwrap();
    session.apply(op).map_err(AppError)?;
    Ok(Json(serde_json::to_value(session.view()).unwrap()))
}

pub async fn undo(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let mut session = state.lock().unwrap();
    session.undo().map_err(AppError)?;
    Ok(Json(serde_json::to_value(session.view()).unwrap()))
}

pub async fn redo(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let mut session = state.lock().unwrap();
    session.redo().map_err(AppError)?;
    Ok(Json(serde_json::to_value(session.view()).unwrap()))
}

pub async fn get_ops(State(state): State<AppState>) -> impl IntoResponse {
    let session = state.lock().unwrap();
    Json(serde_json::to_value(session.ops_history()).unwrap())
}

pub async fn get_changes(State(state): State<AppState>) -> impl IntoResponse {
    let session = state.lock().unwrap();
    Json(serde_json::to_value(session.pending_changes()).unwrap())
}

#[derive(Deserialize)]
pub struct TarballRequest {
    pub generation: u64,
}

pub async fn export_tarball(
    State(state): State<AppState>,
    body: axum::body::Bytes,
) -> Result<impl IntoResponse, AppError> {
    // Parse generation from request body — malformed JSON → 400
    let req: TarballRequest = serde_json::from_slice(&body)
        .map_err(|_| AppError(inspectah_refine::types::RefineError::BadRequest(
            "request body must be JSON with 'generation' field".into()
        )))?;

    // Snapshot state under the lock, then release before expensive work.
    // This prevents export from monopolizing the session mutex.
    let (projected, generation) = {
        let session = state.lock().unwrap();
        if req.generation != session.generation() {
            return Err(AppError(inspectah_refine::types::RefineError::StaleGeneration {
                expected: req.generation,
                actual: session.generation(),
            }));
        }
        (session.snapshot_projected(), session.generation())
    };
    // Lock is released here.

    // Expensive render + tar work happens outside the lock via spawn_blocking.
    let bytes = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, inspectah_refine::types::RefineError> {
        let tempdir = tempfile::tempdir()?;
        let tarball_path = tempdir.path().join("inspectah-refine-output.tar.gz");
        // render_refine_export is a free function, not a session method
        inspectah_refine::session::render_refine_export(&projected, &tarball_path)?;
        Ok(std::fs::read(&tarball_path)?)
    })
    .await
    .map_err(|e| AppError(inspectah_refine::types::RefineError::TarballError(e.to_string())))?
    .map_err(AppError)?;

    Ok((
        StatusCode::OK,
        [
            (axum::http::header::CONTENT_TYPE, "application/gzip"),
            (
                axum::http::header::CONTENT_DISPOSITION,
                "attachment; filename=\"inspectah-refine-output.tar.gz\"",
            ),
        ],
        bytes,
    ))
}
```

- [ ] **Step 6: Write lib.rs with router function**

Replace `inspectah-web/src/lib.rs`:

```rust
pub mod assets;
pub mod error;
pub mod handlers;

use axum::routing::{get, post};
use axum::Router;
use handlers::AppState;
use tower_http::cors::{AllowOrigin, CorsLayer};
use axum::http::{HeaderValue, Method};

pub fn router(state: AppState, served_origin: &str) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::exact(
            HeaderValue::from_str(served_origin).unwrap(),
        ))
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([axum::http::header::CONTENT_TYPE]);

    Router::new()
        .route("/", get(assets::serve_report))
        .route("/assets/{*path}", get(assets::serve_static))
        .route("/api/health", get(handlers::health))
        .route("/api/view", get(handlers::get_view))
        .route("/api/op", post(handlers::apply_op))
        .route("/api/undo", post(handlers::undo))
        .route("/api/redo", post(handlers::redo))
        .route("/api/ops", get(handlers::get_ops))
        .route("/api/changes", get(handlers::get_changes))
        .route("/api/tarball", post(handlers::export_tarball))
        .layer(cors)
        .layer(axum::extract::DefaultBodyLimit::max(1024 * 1024)) // 1 MiB
        .with_state(state)
}
```

- [ ] **Step 7: Verify it compiles**

Run: `cargo check -p inspectah-web`
Expected: success

- [ ] **Step 8: Commit**

```bash
git add inspectah-web/
git commit -m "feat(web): axum HTTP API for refine service layer

9 endpoints: health, view, op, undo, redo, ops, changes, tarball,
and static asset serving. CORS restricted to served origin. 1 MiB
body limit. RefineError mapped to appropriate HTTP status codes.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 8: Write HTTP API integration tests

**Files:**
- Create: `inspectah-web/tests/api_test.rs`

- [ ] **Step 1: Write core endpoint tests**

Create `inspectah-web/tests/api_test.rs`:

```rust
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind, ConfigSection};
use inspectah_refine::session::RefineSession;
use inspectah_web::handlers::AppState;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

fn test_state() -> AppState {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                include: true,
                ..Default::default()
            },
        ],
        ..Default::default()
    });
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/httpd/conf/httpd.conf".into(),
            kind: ConfigFileKind::RpmOwnedModified,
            include: true,
            ..Default::default()
        }],
    });
    Arc::new(Mutex::new(RefineSession::new(snap)))
}

fn app(state: AppState) -> axum::Router {
    inspectah_web::router(state, "http://localhost:8642")
}

async fn get_json(app: &axum::Router, path: &str) -> (StatusCode, serde_json::Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    (status, json)
}

async fn post_json(
    app: &axum::Router,
    path: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    (status, json)
}

#[tokio::test]
async fn health_returns_ok() {
    let app = app(test_state());
    let (status, json) = get_json(&app, "/api/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["status"], "ok");
}

#[tokio::test]
async fn view_returns_refined_view() {
    let app = app(test_state());
    let (status, json) = get_json(&app, "/api/view").await;
    assert_eq!(status, StatusCode::OK);
    assert!(json.get("packages").is_some());
    assert!(json.get("stats").is_some());
    assert!(json.get("generation").is_some());
    assert_eq!(json["generation"], 0);
}

#[tokio::test]
async fn apply_valid_op() {
    let app = app(test_state());
    let (status, json) = post_json(
        &app,
        "/api/op",
        serde_json::json!({
            "op": "ExcludePackage",
            "target": {"name": "httpd", "arch": "x86_64"}
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["generation"], 1);
    assert_eq!(json["stats"]["excluded_packages"], 1);
}

#[tokio::test]
async fn apply_unknown_target_returns_422() {
    let app = app(test_state());
    let (status, json) = post_json(
        &app,
        "/api/op",
        serde_json::json!({
            "op": "ExcludePackage",
            "target": {"name": "nonexistent", "arch": "x86_64"}
        }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(json.get("error").is_some());
}

#[tokio::test]
async fn undo_on_fresh_session_returns_409() {
    let app = app(test_state());
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/undo")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn redo_on_fresh_session_returns_409() {
    let app = app(test_state());
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/redo")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn ops_returns_array() {
    let app = app(test_state());
    let (status, json) = get_json(&app, "/api/ops").await;
    assert_eq!(status, StatusCode::OK);
    assert!(json.is_array());
    assert_eq!(json.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn changes_returns_summary() {
    let app = app(test_state());
    let (status, json) = get_json(&app, "/api/changes").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["is_dirty"], false);
}

#[tokio::test]
async fn tarball_with_stale_generation_returns_409() {
    let app = app(test_state());
    let (status, json) = post_json(
        &app,
        "/api/tarball",
        serde_json::json!({"generation": 999}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(json["error"].as_str().unwrap().contains("stale generation"));
}

#[tokio::test]
async fn tarball_with_malformed_body_returns_400() {
    let app = app(test_state());
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/tarball")
                .header("content-type", "application/json")
                .body(Body::from("not valid json"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn tarball_with_empty_body_returns_400() {
    let app = app(test_state());
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/tarball")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
```

- [ ] **Step 2: Run API tests**

Run: `cargo test -p inspectah-web --test api_test -- --nocapture`
Expected: PASS (11 tests)

- [ ] **Step 3: Commit**

```bash
git add inspectah-web/tests/api_test.rs
git commit -m "test(web): HTTP contract tests for all refine API endpoints

Tests health, view, op, undo, redo, ops, changes, tarball endpoints.
Covers error codes (409, 422) for invalid operations and stale
generation.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 9: Add refine subcommand to CLI

**Files:**
- Modify: `inspectah-cli/Cargo.toml`
- Modify: `inspectah-cli/src/main.rs`
- Modify: `inspectah-cli/src/commands/mod.rs`
- Create: `inspectah-cli/src/commands/refine.rs`

- [ ] **Step 1: Update CLI Cargo.toml**

Add these dependencies to `inspectah-cli/Cargo.toml`:

```toml
inspectah-web = { path = "../inspectah-web" }
inspectah-refine = { path = "../inspectah-refine" }
tokio = { version = "1", features = ["full"] }
open = "5"
```

- [ ] **Step 2: Write the refine command module**

Create `inspectah-cli/src/commands/refine.rs`:

```rust
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(clap::Args)]
pub struct RefineArgs {
    /// Path to scan output tarball (.tar.gz)
    pub tarball: PathBuf,

    /// Port to bind (default: 8642, use 0 for ephemeral)
    #[arg(long, default_value = "8642")]
    pub port: u16,

    /// Open browser automatically
    #[arg(long, default_value = "true")]
    pub open: bool,
}

pub fn run_refine(args: &RefineArgs) -> anyhow::Result<()> {
    eprintln!("Loading snapshot...");

    let session = inspectah_refine::tarball::from_tarball(&args.tarball)?;
    let is_dirty_on_exit = {
        let state: inspectah_web::handlers::AppState = Arc::new(Mutex::new(session));

        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(async {
            let addr = std::net::SocketAddr::from(([127, 0, 0, 1], args.port));
            let listener = tokio::net::TcpListener::bind(addr).await?;
            let actual_addr = listener.local_addr()?;
            let origin = format!("http://{actual_addr}");

            eprintln!("Starting refine server on {origin}");
            eprintln!("Press Ctrl-C to stop.");

            if args.open {
                let url = origin.clone();
                tokio::task::spawn_blocking(move || {
                    let _ = open::that(&url);
                });
            }

            let app = inspectah_web::router(state.clone(), &origin);

            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown_signal())
                .await?;

            let session = state.lock().unwrap();
            Ok::<bool, anyhow::Error>(session.is_dirty())
        })?
    };

    if is_dirty_on_exit {
        eprintln!(
            "Warning: unsaved changes. Use POST /api/tarball to export before stopping."
        );
    }

    Ok(())
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install CTRL+C signal handler");
    eprintln!("\nShutting down...");
}
```

- [ ] **Step 3: Add Refine to Commands enum**

In `inspectah-cli/src/main.rs`, add the `Refine` variant:

```rust
#[derive(Subcommand)]
enum Commands {
    /// Scan the current system and produce a migration snapshot
    Scan(commands::scan::ScanArgs),
    /// Interactively refine scan output and re-render
    Refine(commands::refine::RefineArgs),
    Version,
}
```

And in the match block, add:

```rust
Commands::Refine(args) => commands::refine::run_refine(&args),
```

- [ ] **Step 4: Add module declaration**

In `inspectah-cli/src/commands/mod.rs`, add:

```rust
pub mod refine;
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo check -p inspectah-cli`
Expected: success

- [ ] **Step 6: Verify help text**

Run: `cargo run -p inspectah-cli -- refine --help`
Expected: help text showing `tarball`, `--port`, `--open` arguments

- [ ] **Step 7: Commit**

```bash
git add inspectah-cli/
git commit -m "feat(cli): add refine subcommand

inspectah refine <tarball> [--port 8642] [--open true]
Loads scan tarball, starts axum server, opens browser.
Graceful shutdown on SIGINT with dirty-state warning.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 10: Exact export contract proof

**Files:**
- Create: `inspectah-refine/tests/export_contract_test.rs`

This is the primary contract enforcement test. It proves the exact file
set, absence of extras, flat layout, snapshot fidelity, preview/export
Containerfile identity, and re-import round-trip.

- [ ] **Step 1: Write the exact file set test**

Create `inspectah-refine/tests/export_contract_test.rs`:

```rust
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind, ConfigSection};
use inspectah_core::types::redaction::RedactionState;
use inspectah_refine::types::{PackageTarget, RefinementOp, RefineError};
use inspectah_refine::session::RefineSession;
use std::collections::BTreeSet;

fn test_snapshot() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                include: true,
                ..Default::default()
            },
            PackageEntry {
                name: "vim".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                include: true,
                ..Default::default()
            },
        ],
        ..Default::default()
    });
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/httpd/conf/httpd.conf".into(),
            kind: ConfigFileKind::RpmOwnedModified,
            include: true,
            ..Default::default()
        }],
    });
    snap.redaction_state = Some(RedactionState::FullyRedacted {
        redacted_by: "inspectah 0.8.0".into(),
        config_hash: "abc123".into(),
    });
    snap
}

/// Collect all file entries from a tarball as a sorted set of paths.
/// Directories are excluded — only regular file paths.
fn tarball_file_set(tarball_path: &std::path::Path) -> BTreeSet<String> {
    let file = std::fs::File::open(tarball_path).unwrap();
    let gz = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);
    let mut files = BTreeSet::new();
    for entry in archive.entries().unwrap() {
        let entry = entry.unwrap();
        if entry.header().entry_type() == tar::EntryType::Regular {
            let path = entry.path().unwrap().to_string_lossy().to_string();
            files.insert(path);
        }
    }
    files
}

#[test]
fn export_exact_file_set() {
    let mut session = RefineSession::new(test_snapshot());
    session
        .apply(RefinementOp::ExcludePackage(PackageTarget {
            name: "httpd".into(),
            arch: "x86_64".into(),
        }))
        .unwrap();

    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("output.tar.gz");
    session.export_tarball(&tarball_path, session.generation()).unwrap();

    let actual = tarball_file_set(&tarball_path);

    // Build the EXACT expected set for this fixture.
    // The test snapshot has one included config file at
    // /etc/httpd/conf/httpd.conf, so config/ tree is populated.
    let expected: BTreeSet<String> = [
        "inspection-snapshot.json",
        "Containerfile",
        "audit-report.md",
        "schema/snapshot.schema.json",
        // config tree materialized from the included config file:
        "config/etc/httpd/conf/httpd.conf",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();

    // Assert FULL equality — not subset, not superset.
    // Any missing file, any extra file, any wrong path = failure.
    let missing: BTreeSet<_> = expected.difference(&actual).collect();
    let extra: BTreeSet<_> = actual.difference(&expected).collect();

    assert!(
        missing.is_empty() && extra.is_empty(),
        "export contract violated!\n  missing: {missing:?}\n  extra: {extra:?}\n  expected: {expected:?}\n  actual: {actual:?}"
    );
}

#[test]
fn export_snapshot_reflects_refinements() {
    let mut session = RefineSession::new(test_snapshot());
    session
        .apply(RefinementOp::ExcludePackage(PackageTarget {
            name: "httpd".into(),
            arch: "x86_64".into(),
        }))
        .unwrap();

    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("output.tar.gz");
    session.export_tarball(&tarball_path, session.generation()).unwrap();

    // Extract and verify
    let extract_dir = tempdir.path().join("extract");
    std::fs::create_dir(&extract_dir).unwrap();
    let file = std::fs::File::open(&tarball_path).unwrap();
    let gz = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);
    archive.unpack(&extract_dir).unwrap();

    let snap_path = walkdir::WalkDir::new(&extract_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .find(|e| e.file_name() == "inspection-snapshot.json")
        .expect("snapshot file must exist");

    let snap_json = std::fs::read_to_string(snap_path.path()).unwrap();
    let snap: InspectionSnapshot = serde_json::from_str(&snap_json).unwrap();

    let httpd = snap.rpm.as_ref().unwrap().packages_added
        .iter().find(|p| p.name == "httpd").unwrap();
    assert!(!httpd.include, "httpd must be excluded in exported snapshot");

    let vim = snap.rpm.as_ref().unwrap().packages_added
        .iter().find(|p| p.name == "vim").unwrap();
    assert!(vim.include, "vim must remain included");
}

#[test]
fn preview_export_containerfile_fidelity() {
    let mut session = RefineSession::new(test_snapshot());
    session
        .apply(RefinementOp::ExcludePackage(PackageTarget {
            name: "httpd".into(),
            arch: "x86_64".into(),
        }))
        .unwrap();

    // Capture the preview Containerfile
    let preview = session.view().containerfile_preview.clone();

    // Export and extract the Containerfile
    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("output.tar.gz");
    session.export_tarball(&tarball_path, session.generation()).unwrap();

    let extract_dir = tempdir.path().join("extract");
    std::fs::create_dir(&extract_dir).unwrap();
    let file = std::fs::File::open(&tarball_path).unwrap();
    let gz = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);
    archive.unpack(&extract_dir).unwrap();

    let cf_path = walkdir::WalkDir::new(&extract_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .find(|e| e.file_name() == "Containerfile")
        .expect("Containerfile must exist in export");

    let exported = std::fs::read_to_string(cf_path.path()).unwrap();

    assert_eq!(
        preview, exported,
        "preview and exported Containerfile must be byte-identical"
    );
}

#[test]
fn reimport_preserves_excludes() {
    // First session: exclude httpd, export
    let mut session1 = RefineSession::new(test_snapshot());
    session1
        .apply(RefinementOp::ExcludePackage(PackageTarget {
            name: "httpd".into(),
            arch: "x86_64".into(),
        }))
        .unwrap();

    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("export1.tar.gz");
    session1.export_tarball(&tarball_path, session1.generation()).unwrap();

    // Second session: re-import the exported tarball
    let session2 = inspectah_refine::tarball::from_tarball(&tarball_path).unwrap();

    // httpd must still be excluded in the re-imported session
    let httpd = session2.view().packages
        .iter().find(|p| p.entry.name == "httpd").unwrap();
    assert!(!httpd.entry.include, "httpd must remain excluded after re-import");

    // The re-imported session should NOT be dirty — the exclude is
    // part of the normalized original, not a new mutation
    assert!(!session2.is_dirty(), "re-imported session must not be dirty");
}
```

- [ ] **Step 2: Run the export contract tests**

Run: `cargo test -p inspectah-refine --test export_contract_test -- --nocapture`
Expected: PASS (4 tests)

- [ ] **Step 3: Commit**

```bash
git add inspectah-refine/tests/export_contract_test.rs
git commit -m "test(refine): exact export contract proof with re-import round-trip

Verifies exact file set (required present, forbidden absent, no prefix),
snapshot reflects refinements, preview/export Containerfile byte-identity,
and re-import preserves operator excludes without dirty-state regression.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 11: HTTP lifecycle integration test

**Files:**
- Create: `inspectah-cli/tests/refine_e2e_test.rs`

This exercises the real user path end-to-end: load a tarball, start
the server, interact via HTTP, export, and verify shutdown behavior.

- [ ] **Step 1: Write the E2E test**

Create `inspectah-cli/tests/refine_e2e_test.rs`:

```rust
use std::io::Write;
use std::time::Duration;

/// Create a minimal test tarball with a FullyRedacted snapshot.
fn create_test_tarball(dir: &std::path::Path) -> std::path::PathBuf {
    let snap = serde_json::json!({
        "schema_version": 14,
        "rpm": {
            "packages_added": [
                {
                    "name": "httpd",
                    "arch": "x86_64",
                    "state": "added",
                    "include": true
                }
            ]
        },
        "config": {
            "files": [
                {
                    "path": "/etc/httpd/conf/httpd.conf",
                    "kind": "rpm_owned_modified",
                    "include": true
                }
            ]
        },
        "redaction_state": {
            "state": "fully_redacted",
            "redacted_by": "inspectah 0.8.0",
            "config_hash": "abc123"
        }
    });

    let snap_path = dir.join("inspection-snapshot.json");
    std::fs::write(&snap_path, serde_json::to_string_pretty(&snap).unwrap()).unwrap();

    let tarball_path = dir.join("test-scan.tar.gz");
    let f = std::fs::File::create(&tarball_path).unwrap();
    let gz = flate2::write::GzEncoder::new(f, flate2::Compression::default());
    let mut tar = tar::Builder::new(gz);
    tar.append_path_with_name(&snap_path, "inspection-snapshot.json").unwrap();
    tar.finish().unwrap();
    tarball_path
}

#[tokio::test]
async fn refine_server_lifecycle() {
    let tempdir = tempfile::tempdir().unwrap();
    let tarball = create_test_tarball(tempdir.path());

    // Load tarball and create session
    let session = inspectah_refine::tarball::from_tarball(&tarball).unwrap();
    let state: inspectah_web::handlers::AppState =
        std::sync::Arc::new(std::sync::Mutex::new(session));

    // Bind to ephemeral port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let origin = format!("http://{addr}");

    let app = inspectah_web::router(state.clone(), &origin);

    // Spawn the server
    let server_handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();
    let base = format!("http://{addr}");

    // 1. Health check
    let resp = client.get(format!("{base}/api/health")).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");

    // 2. Initial view
    let resp = client.get(format!("{base}/api/view")).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let view: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(view["generation"], 0);
    assert_eq!(view["stats"]["total_packages"], 1);

    // 3. Apply an operation
    let resp = client
        .post(format!("{base}/api/op"))
        .json(&serde_json::json!({
            "op": "ExcludePackage",
            "target": {"name": "httpd", "arch": "x86_64"}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let view: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(view["generation"], 1);
    assert_eq!(view["stats"]["excluded_packages"], 1);

    // 4. Undo
    let resp = client.post(format!("{base}/api/undo")).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let view: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(view["generation"], 2);
    assert_eq!(view["stats"]["excluded_packages"], 0);

    // 5. Redo
    let resp = client.post(format!("{base}/api/redo")).send().await.unwrap();
    assert_eq!(resp.status(), 200);

    // 6. Changes
    let resp = client.get(format!("{base}/api/changes")).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let changes: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(changes["is_dirty"], true);

    // 7. Ops history
    let resp = client.get(format!("{base}/api/ops")).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let ops: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(ops.len(), 1);

    // 8. Export with matching generation
    let current_gen = {
        let s = state.lock().unwrap();
        s.generation()
    };
    let resp = client
        .post(format!("{base}/api/tarball"))
        .json(&serde_json::json!({"generation": current_gen}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers().get("content-type").unwrap(),
        "application/gzip"
    );
    let tarball_bytes = resp.bytes().await.unwrap();
    assert!(!tarball_bytes.is_empty());

    // 9. Export with stale generation → 409
    let resp = client
        .post(format!("{base}/api/tarball"))
        .json(&serde_json::json!({"generation": 999}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 409);

    // 10. Invalid op → 422
    let resp = client
        .post(format!("{base}/api/op"))
        .json(&serde_json::json!({
            "op": "ExcludePackage",
            "target": {"name": "nonexistent", "arch": "x86_64"}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 422);

    // Clean shutdown
    server_handle.abort();
}
```

Note: add `reqwest = { version = "0.12", features = ["json"] }` to
`inspectah-cli/Cargo.toml` `[dev-dependencies]`, along with
`tokio = { version = "1", features = ["full"] }`,
`inspectah-refine = { path = "../inspectah-refine" }`,
`inspectah-web = { path = "../inspectah-web" }`,
`serde_json = "1"`, `tempfile = "3"`, `tar = "0.4"`, `flate2 = "0.2"`.

- [ ] **Step 2: Run the E2E test**

Run: `cargo test -p inspectah-cli --test refine_e2e_test -- --nocapture`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add inspectah-cli/tests/refine_e2e_test.rs inspectah-cli/Cargo.toml
git commit -m "test(cli): end-to-end refine server lifecycle test

Exercises the full user path: tarball load, ephemeral port bind,
health/view/op/undo/redo/ops/changes/tarball endpoints, stale
generation rejection, unknown target rejection.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 12: Full workspace verification

- [ ] **Step 1: Run all workspace tests**

Run: `cargo test --workspace`
Expected: all tests pass, zero failures

- [ ] **Step 2: Run clippy**

Run: `cargo clippy --workspace -- -D warnings`
Expected: zero warnings

- [ ] **Step 3: Verify the binary runs**

Run: `cargo build -p inspectah-cli && ./target/debug/inspectah --help`
Expected: `refine` subcommand appears in help output

- [ ] **Step 4: Commit any fix-ups if needed**

Only if clippy or test failures required changes:

```bash
git add -A
git commit -m "fix(refine): address clippy warnings and test fixes

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Summary

| Task | What it builds | Tests |
|------|---------------|-------|
| 1 | Crate scaffold | compile check |
| 2 | Data model types | 7 serde roundtrip tests |
| 3 | Snapshot normalization (raw-JSON presence-aware) | 10 tests (omitted vs explicit, fixtures, mixed) |
| 4 | Attention model | 6 tests |
| 5 | RefineSession core (materialized-root preview) | 16 tests |
| 6 | Tarball I/O (safety + provenance + migrate) | 9 tests (incl. v12 schema fixture) |
| 7 | HTTP API (axum, snapshot-release-lock, BadRequest) | compile check |
| 8 | HTTP API tests | 11 tests (incl. malformed body → 400) |
| 9 | CLI subcommand | compile + help |
| 10 | Exact export contract proof | 4 tests (BTreeSet equality, fidelity, re-import) |
| 11 | HTTP lifecycle integration | 1 test (10-step lifecycle) |
| 12 | Workspace verification | full suite |

**Total:** 12 tasks, ~63 new tests, 3 new/modified crates.

## Revision History

### Round 3 (2026-05-15)

Revised to address two remaining must-fix blockers from 2-lane re-review
(Tang, Thorn — both returned request-changes on R2).

1. **Normalization: presence-aware include defaulting.** The R2 fix (accept
   deserialized values) still collapsed omitted `include` and explicit
   `include: false` because `#[serde(default)] bool` loses field presence.
   R3 adds raw-JSON patching via `serde_json::Value`: walk package/config
   arrays before typed deserialization, inject `"include": true` on entries
   that lack the key, leave explicit false untouched. New `load_for_refine()`
   is the single import entry point. 10 tests cover all three states (omitted,
   explicit true, explicit false) from both Rust structs and JSON fixtures.
   (Task 3 rewritten, Task 6 tarball import updated.)

2. **Export: materialized-root fidelity.** The R2 fix (both paths use `None`)
   was self-consistent but not truthful to the renderer seam — `write_config_tree`
   writes files from sources beyond `config.files` (repo files, GPG keys, etc.)
   and `config_copy_roots_from_snapshot()` misses those roots. R3 fix: both
   preview (`recompute_view`) and export (`render_refine_export`) materialize
   the config tree to a tempdir via `write_config_tree` to get the real
   `materialized_roots`, then render Containerfile with `Some(&materialized_roots)`.
   Preview tempdirs are dropped immediately. (Task 5, export function revised.)

3. **Exact file set proof.** Task 10 now uses full `BTreeSet` equality instead
   of subset/superset checking. The expected set is constructed from the fixture,
   including materialized `config/` entries. Missing or extra files both fail.
   (Task 10 rewritten.)

4. **Non-blocking polish.** Added `BadRequest` error variant + 400 mapping for
   malformed `POST /api/tarball` bodies (was routing through catch-all 500).
   Added malformed/empty body tests to Task 8. Added v12 old-schema migration
   fixture to Task 6. Retitled Task 11 as "HTTP lifecycle" to match actual scope.

Backlog items closed by this revision:
- `inspectah-rust-rewrite-refine-service-layer-plan-normalization-and-reimport-truthfulness.md`
- `inspectah-rust-rewrite-refine-service-layer-plan-export-contract-and-preview-fidelity.md`
- `inspectah-rust-rewrite-refine-service-layer-plan-http-and-cli-proof-polish.md`

### Round 2 (2026-05-15)

Revised to address three must-fix blockers from 4-lane review (Tang, Kit,
Thorn, Slate — all returned request-changes).

1. **Normalization truthfulness.** Removed blanket `include: false → true`
   rewrite. Normalization preserved deserialized values as the baseline.
   (Superseded by R3 — presence-aware patching.)

2. **Export contract and preview fidelity.** Replaced `render_all()` with
   dedicated `render_refine_export()`. Added `schema/snapshot.schema.json`.
   (Strengthened further in R3 — materialized-root fidelity.)

3. **CLI/web E2E proof.** Added Task 11 lifecycle test. Fixed web export
   handler for snapshot-release-lock. Added `migrate()` to import pipeline.
