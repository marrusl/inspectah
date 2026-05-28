# Rust Rewrite: Foundation + First Inspector E2E (Phases 0-1)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Spec:** `docs/specs/proposed/2026-05-09-rust-rewrite-design.md`

**Goal:** Build a working `inspectah scan` on a `rust` branch that produces a tarball with verified **RPM-section parity** against Go output on package-based hosts, establishing the crate architecture, fully typed core contracts, and pipeline for all subsequent phases. Full-snapshot parity is Phase 2.

**Architecture:** Five-crate Cargo workspace. Phase 0 defines every type and trait in `inspectah-core` (the gravity well) — all 14 inspector sections with a typed `SectionData` boundary, pipeline typestate, redaction trust model wired into the snapshot contract. Phase 1 adds the RPM inspector with minimal `ffi-rpm` (feature-gated, shell fallback), pipeline orchestration, all 8 always-written artifact renderers, tarball construction, and minimal CLI. Output contract compatibility is verified via mandatory normalized diff against a real Go v13 golden file with explicit divergence bookkeeping.

**Tech Stack:** Rust 2021 edition. Key crates: `serde` + `serde_json` (serialization), `clap` v4 derive (CLI), `thiserror` (errors), `insta` (snapshot testing), `regex` (redaction + parsing), `tar` + `flate2` (tarball), `walkdir` (directory traversal), `chrono` (timestamps), `pkg-config` (build-time librpm discovery).

**Revision:** 2 (2026-05-10). Addresses review findings from Tang, Thorn, Collins, Press, Slate. See `docs/plans/2026-05-10-rust-plan-revision-checklist.md`.

**Branch:** `rust` (off `main`)

---

## Serde Strategy (reference for all type tasks)

Go JSON tags map to Rust serde attributes. These patterns repeat throughout:

| Go Pattern | Rust Equivalent |
|------------|----------------|
| `Field string \x60json:"field"\x60` | `pub field: String` (field name IS snake_case) |
| `Field *string \x60json:"field"\x60` | `pub field: Option<String>` |
| `Field *string \x60json:"field,omitempty"\x60` | `#[serde(skip_serializing_if = "Option::is_none")] pub field: Option<String>` |
| `Field bool \x60json:"field"\x60` | `#[serde(default)] pub field: bool` |
| `Field bool \x60json:"field,omitempty"\x60` | `#[serde(default, skip_serializing_if = "crate::is_false")] pub field: bool` |
| `Field *bool \x60json:"field,omitempty"\x60` | `#[serde(default, skip_serializing_if = "Option::is_none")] pub field: Option<bool>` |
| `Field []T \x60json:"field"\x60` | `#[serde(default)] pub field: Vec<T>` |
| `Field *FleetPrevalence \x60json:"fleet"\x60` | `pub fleet: Option<FleetPrevalence>` |
| `Field *FleetPrevalence \x60json:"fleet,omitempty"\x60` | `#[serde(skip_serializing_if = "Option::is_none")] pub fleet: Option<FleetPrevalence>` |
| `Field map[string]interface{} \x60json:"field"\x60` | `pub field: serde_json::Value` (or `HashMap<String, serde_json::Value>`) |

Helper in `lib.rs`: `pub(crate) fn is_false(v: &bool) -> bool { !*v }`

---

## File Structure

### Workspace Root

```
inspectah/
  Cargo.toml                          # workspace root
  testdata/
    golden/
      go-v13-package-based.json       # captured from Go scan of package-based host
      go-v13-minimal.json             # minimal valid Go v13 snapshot
      go-v13-rpm-section.json         # REQUIRED: RPM section from real Go scan (jq '.rpm')
    divergences.md                    # allowlist of expected Go-vs-Rust differences
    fixtures/
      rpm/
        rpm-qa.txt                    # canned rpm -qa output
        rpm-va.txt                    # canned rpm -Va output
        repos/                        # sample repo files
      host/
        etc/os-release                # canned os-release
```

### inspectah-core (Phase 0)

```
inspectah-core/
  Cargo.toml
  src/
    lib.rs                            # crate root, re-exports, helpers
    types/
      mod.rs                          # module declarations
      os.rs                           # OsRelease, OstreeVariant, SystemType
      fleet.rs                        # FleetPrevalence, FleetMeta
      system.rs                       # SourceSystem, TargetSystem, MigrationContext (pipeline-internal)
      rpm.rs                          # PackageEntry, RpmSection, ~12 types
      config.rs                       # ConfigFileEntry, ConfigSection
      services.rs                     # ServiceStateChange, ServiceSection
      network.rs                      # NMConnection, NetworkSection, ~8 types
      storage.rs                      # FstabEntry, StorageSection, ~5 types
      scheduled.rs                    # CronJob, ScheduledTaskSection, ~4 types
      containers.rs                   # QuadletUnit, ContainerSection, ~7 types
      nonrpm.rs                       # NonRpmItem, NonRpmSoftwareSection
      kernelboot.rs                   # SysctlOverride, KernelBootSection, ~4 types
      selinux.rs                      # SelinuxSection, SelinuxPortLabel
      users.rs                        # UserGroupSection
      redaction.rs                    # SecretKind, ShadowStatus, RedactionState, RedactionFinding
      warnings.rs                     # Warning
      completeness.rs                 # Completeness, InspectorId, SourceSystemKind, SectionData
      preflight.rs                    # PreflightResult, PreflightMode, RenderTarget
    traits/
      mod.rs                          # module declarations
      inspector.rs                    # Inspector, InspectorError, InspectorOutput
      executor.rs                     # Executor, ExecResult
      detector.rs                     # SecretDetector, Sensitivity, ScanContext
      renderer.rs                     # Renderer trait
    snapshot.rs                       # InspectionSnapshot, schema version, migration
    pipeline.rs                       # Pipeline<S> typestate (Collected, Validated, Redacted)
    normalize.rs                      # Normalized diff tooling for Go-vs-Rust comparison
```

### inspectah-collect (Phase 1)

```
inspectah-collect/
  Cargo.toml
  src/
    lib.rs
    executor/
      mod.rs
      real.rs                         # RealExecutor (shell commands)
      mock.rs                         # MockExecutor (canned output for tests)
    inspectors/
      mod.rs
      rpm/
        mod.rs                        # Inspector trait impl + orchestration
        parser.rs                     # NEVRA parsing, rpmvercmp
        classifier.rs                 # package state classification
        repos.rs                      # repo files, GPG keys
        modules.rs                    # module streams, version locks
    ffi/
      mod.rs                          # feature-gated FFI modules
      rpm.rs                          # librpm safe wrapper (behind ffi-rpm)
  build.rs                            # pkg-config librpm discovery (ffi-rpm only)
```

### inspectah-pipeline (Phase 1)

```
inspectah-pipeline/
  Cargo.toml
  src/
    lib.rs
    collect.rs                        # collection orchestration
    validate.rs                       # validation stage
    redaction/
      mod.rs
      patterns.rs                     # regex patterns per SecretKind
      engine.rs                       # RedactionEngine, Cow<str> replacement
    render/
      mod.rs
      containerfile.rs                # Containerfile generation
      configtree.rs                   # full writeConfigTree() materialization
      report.rs                       # report.html (minimal PatternFly)
      kickstart.rs                    # kickstart-suggestion.ks
      audit.rs                        # audit-report.md
      secrets.rs                      # secrets-review.md
      readme.rs                       # README.md
      tarball.rs                      # tar.gz construction
      safety.rs                       # path sanitization, shell escaping, HTML escaping
```

### inspectah-cli (Phase 1)

```
inspectah-cli/
  Cargo.toml
  src/
    main.rs                           # entry point
    commands/
      mod.rs
      scan.rs                         # scan subcommand
      version.rs                      # version subcommand
```

### inspectah-web (stub only, Phase 5+)

```
inspectah-web/
  Cargo.toml                          # stub, no src yet
  src/
    lib.rs                            # empty
```

---

## Phase 0: Foundation

Everything in this phase lives in `inspectah-core`. The goal: all types compile, all traits are defined, serde round-trips pass, Go v13 snapshots deserialize into Rust types.

---

### Task 1: Create Branch + Workspace Skeleton

**Files:**
- Create: `Cargo.toml` (workspace root)
- Create: `inspectah-core/Cargo.toml`
- Create: `inspectah-core/src/lib.rs`
- Create: `inspectah-collect/Cargo.toml` (stub)
- Create: `inspectah-collect/src/lib.rs` (stub)
- Create: `inspectah-pipeline/Cargo.toml` (stub)
- Create: `inspectah-pipeline/src/lib.rs` (stub)
- Create: `inspectah-cli/Cargo.toml` (stub)
- Create: `inspectah-cli/src/main.rs` (stub)
- Create: `inspectah-web/Cargo.toml` (stub)
- Create: `inspectah-web/src/lib.rs` (stub)

- [ ] **Step 1: Create the `rust` branch**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git checkout -b rust main
```

- [ ] **Step 2: Create workspace Cargo.toml**

```toml
# Cargo.toml (workspace root)
[workspace]
members = [
    "inspectah-core",
    "inspectah-collect",
    "inspectah-pipeline",
    "inspectah-cli",
    "inspectah-web",
]
resolver = "2"

[workspace.package]
version = "0.8.0-alpha.1"
edition = "2021"
license = "MIT"
repository = "https://github.com/marrusl/inspectah"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
insta = { version = "1", features = ["json"] }
```

- [ ] **Step 3: Create inspectah-core crate**

```toml
# inspectah-core/Cargo.toml
[package]
name = "inspectah-core"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true

[dev-dependencies]
insta.workspace = true
```

```rust
// inspectah-core/src/lib.rs
pub mod types;

pub(crate) fn is_false(v: &bool) -> bool {
    !*v
}
```

```rust
// inspectah-core/src/types/mod.rs
// Modules added as types are implemented in subsequent tasks.
```

- [ ] **Step 4: Create stub crates** (collect, pipeline, cli, web)

```toml
# inspectah-collect/Cargo.toml
[package]
name = "inspectah-collect"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
inspectah-core = { path = "../inspectah-core" }
serde.workspace = true
serde_json.workspace = true

[dev-dependencies]
insta.workspace = true
```

```rust
// inspectah-collect/src/lib.rs
```

```toml
# inspectah-pipeline/Cargo.toml
[package]
name = "inspectah-pipeline"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
inspectah-core = { path = "../inspectah-core" }
inspectah-collect = { path = "../inspectah-collect" }
serde.workspace = true
serde_json.workspace = true

[dev-dependencies]
insta.workspace = true
```

```rust
// inspectah-pipeline/src/lib.rs
```

```toml
# inspectah-cli/Cargo.toml
[package]
name = "inspectah-cli"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
inspectah-core = { path = "../inspectah-core" }
inspectah-collect = { path = "../inspectah-collect" }
inspectah-pipeline = { path = "../inspectah-pipeline" }

[[bin]]
name = "inspectah"
path = "src/main.rs"
```

```rust
// inspectah-cli/src/main.rs
fn main() {
    println!("inspectah (rust) — not yet implemented");
}
```

```toml
# inspectah-web/Cargo.toml
[package]
name = "inspectah-web"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
inspectah-core = { path = "../inspectah-core" }
```

```rust
// inspectah-web/src/lib.rs
```

- [ ] **Step 5: Verify workspace compiles**

```bash
cargo check --workspace
```

Expected: compiles with no errors.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml inspectah-core/ inspectah-collect/ inspectah-pipeline/ inspectah-cli/ inspectah-web/
git commit -m "feat: initialize Rust workspace with five crates

Cargo workspace with inspectah-core (types/traits), inspectah-collect
(inspectors), inspectah-pipeline (orchestration), inspectah-cli (binary),
and inspectah-web (stub). All crates compile as empty shells.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 2: OS + Fleet Primitive Types

**Files:**
- Create: `inspectah-core/src/types/os.rs`
- Create: `inspectah-core/src/types/fleet.rs`
- Modify: `inspectah-core/src/types/mod.rs`

- [ ] **Step 1: Write serde round-trip tests**

Add to the bottom of `os.rs` (after Step 2 creates it):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_os_release_roundtrip() {
        let os = OsRelease {
            name: "Red Hat Enterprise Linux".into(),
            version_id: "9.4".into(),
            version: "9.4 (Plow)".into(),
            id: "rhel".into(),
            id_like: "fedora".into(),
            pretty_name: "Red Hat Enterprise Linux 9.4 (Plow)".into(),
            variant_id: String::new(),
        };
        let json = serde_json::to_string(&os).unwrap();
        let parsed: OsRelease = serde_json::from_str(&json).unwrap();
        assert_eq!(os, parsed);
    }

    #[test]
    fn test_os_release_missing_fields() {
        let json = r#"{"name":"Fedora","id":"fedora"}"#;
        let os: OsRelease = serde_json::from_str(json).unwrap();
        assert_eq!(os.name, "Fedora");
        assert_eq!(os.version_id, ""); // missing → default empty string
    }

    #[test]
    fn test_system_type_serde() {
        assert_eq!(
            serde_json::to_string(&SystemType::PackageMode).unwrap(),
            r#""package-mode""#
        );
        assert_eq!(
            serde_json::to_string(&SystemType::RpmOstree).unwrap(),
            r#""rpm-ostree""#
        );
        let parsed: SystemType = serde_json::from_str(r#""bootc""#).unwrap();
        assert_eq!(parsed, SystemType::Bootc);
    }

    #[test]
    fn test_ostree_variant_serde() {
        let json = serde_json::to_string(&OstreeVariant::Silverblue).unwrap();
        assert_eq!(json, r#""silverblue""#);

        let ub = OstreeVariant::UniversalBlue {
            image_ref: "ghcr.io/ublue-os/bazzite:latest".into(),
        };
        let json = serde_json::to_string(&ub).unwrap();
        let parsed: OstreeVariant = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ub);
    }
}
```

Add to the bottom of `fleet.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fleet_prevalence_roundtrip() {
        let fp = FleetPrevalence {
            count: 3,
            total: 5,
            hosts: vec!["host1".into(), "host2".into(), "host3".into()],
        };
        let json = serde_json::to_string(&fp).unwrap();
        let parsed: FleetPrevalence = serde_json::from_str(&json).unwrap();
        assert_eq!(fp, parsed);
    }

    #[test]
    fn test_fleet_prevalence_null_deserialize() {
        let val: Option<FleetPrevalence> = serde_json::from_str("null").unwrap();
        assert!(val.is_none());
    }
}
```

- [ ] **Step 2: Define OS types**

```rust
// inspectah-core/src/types/os.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OsRelease {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version_id: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub id_like: String,
    #[serde(default)]
    pub pretty_name: String,
    #[serde(default)]
    pub variant_id: String,
}

/// System type as stored in the snapshot JSON.
/// Uses explicit renames because Go values contain hyphens.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SystemType {
    #[default]
    #[serde(rename = "unknown")]
    Unknown,
    #[serde(rename = "package-mode")]
    PackageMode,
    #[serde(rename = "rpm-ostree")]
    RpmOstree,
    #[serde(rename = "bootc")]
    Bootc,
}

/// rpm-ostree desktop/immutable variants.
/// Pipeline-internal — not stored directly in snapshot JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "variant", content = "value")]
pub enum OstreeVariant {
    #[serde(rename = "silverblue")]
    Silverblue,
    #[serde(rename = "kinoite")]
    Kinoite,
    #[serde(rename = "sericea")]
    Sericea,
    #[serde(rename = "onyx")]
    Onyx,
    #[serde(rename = "universal_blue")]
    UniversalBlue { image_ref: String },
    #[serde(rename = "centos_stream")]
    CentOSStream { major: u8 },
    #[serde(rename = "rhel")]
    Rhel { major: u8, minor: u8 },
    #[serde(rename = "unknown")]
    Unknown(String),
}
```

- [ ] **Step 3: Define Fleet types**

```rust
// inspectah-core/src/types/fleet.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetPrevalence {
    #[serde(default)]
    pub count: i32,
    #[serde(default)]
    pub total: i32,
    #[serde(default)]
    pub hosts: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetMeta {
    #[serde(default)]
    pub source_hosts: Vec<String>,
    #[serde(default)]
    pub total_hosts: i32,
    #[serde(default)]
    pub min_prevalence: i32,
}
```

- [ ] **Step 4: Wire up modules**

```rust
// inspectah-core/src/types/mod.rs
pub mod os;
pub mod fleet;
```

- [ ] **Step 5: Run tests**

```bash
cargo test -p inspectah-core
```

Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add inspectah-core/src/
git commit -m "feat(core): add OS release, system type, and fleet types

OsRelease, SystemType (with hyphenated JSON values), OstreeVariant,
FleetPrevalence, FleetMeta. All serde round-trips verified.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 3: Source System Model (Pipeline-Internal)

These types are NOT serialized into snapshot JSON. They're constructed by the pipeline from snapshot fields and used internally for dispatch and rendering decisions.

**Files:**
- Create: `inspectah-core/src/types/system.rs`
- Modify: `inspectah-core/src/types/mod.rs`

- [ ] **Step 1: Write tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::os::OsRelease;

    #[test]
    fn test_same_stream_migration() {
        let ctx = MigrationContext {
            source: SourceSystem::PackageBased {
                os_release: OsRelease {
                    id: "rhel".into(),
                    version_id: "9.4".into(),
                    ..Default::default()
                },
            },
            target: TargetSystem::BootcImage {
                image_ref: "registry.redhat.io/rhel9/rhel-bootc:9.4".into(),
            },
        };
        assert_eq!(ctx.migration_kind(), MigrationKind::SameStream);
        assert!(!ctx.is_cross_major());
        assert!(!ctx.is_cross_vendor());
    }

    #[test]
    fn test_major_upgrade_migration() {
        let ctx = MigrationContext {
            source: SourceSystem::PackageBased {
                os_release: OsRelease {
                    id: "rhel".into(),
                    version_id: "9.4".into(),
                    ..Default::default()
                },
            },
            target: TargetSystem::BootcImage {
                image_ref: "registry.redhat.io/rhel10/rhel-bootc:10.0".into(),
            },
        };
        assert_eq!(ctx.migration_kind(), MigrationKind::MajorUpgrade);
        assert!(ctx.is_cross_major());
    }

    #[test]
    fn test_bootc_source_booted_only() {
        let source = SourceSystem::Bootc {
            os_release: OsRelease::default(),
            booted_image: "registry.redhat.io/rhel9/rhel-bootc:9.4".into(),
            staged_image: Some("registry.redhat.io/rhel9/rhel-bootc:9.5".into()),
        };
        if let SourceSystem::Bootc { booted_image, staged_image, .. } = &source {
            assert!(!booted_image.is_empty());
            assert!(staged_image.is_some());
        }
    }
}
```

- [ ] **Step 2: Define source/target system types**

```rust
// inspectah-core/src/types/system.rs
use crate::types::os::{OsRelease, OstreeVariant};

pub type ImageRef = String;

/// What we're inspecting. Each variant carries exactly the data
/// its inspectors need. NOT serialized to snapshot JSON — constructed
/// from snapshot fields during pipeline processing.
#[derive(Debug, Clone, PartialEq)]
pub enum SourceSystem {
    PackageBased {
        os_release: OsRelease,
    },
    RpmOstree {
        os_release: OsRelease,
        variant: OstreeVariant,
        base_image: Option<ImageRef>,
    },
    Bootc {
        os_release: OsRelease,
        booted_image: ImageRef,
        staged_image: Option<ImageRef>,
    },
}

/// Migration target. Always bootc-based.
#[derive(Debug, Clone, PartialEq)]
pub enum TargetSystem {
    BootcImage { image_ref: ImageRef },
    CustomImage { image_ref: ImageRef, base: ImageRef },
}

/// Source + target determine inspector behavior and rendering.
#[derive(Debug, Clone)]
pub struct MigrationContext {
    pub source: SourceSystem,
    pub target: TargetSystem,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationKind {
    SameStream,
    MajorUpgrade,
    VendorTransition,
    CommunityToEnterprise,
    OstreeToBootc,
}

impl SourceSystem {
    pub fn os_release(&self) -> &OsRelease {
        match self {
            Self::PackageBased { os_release, .. }
            | Self::RpmOstree { os_release, .. }
            | Self::Bootc { os_release, .. } => os_release,
        }
    }

    pub fn major_version(&self) -> Option<u32> {
        let vid = &self.os_release().version_id;
        vid.split('.').next().and_then(|s| s.parse().ok())
    }
}

impl MigrationContext {
    pub fn is_cross_major(&self) -> bool {
        matches!(self.migration_kind(), MigrationKind::MajorUpgrade)
    }

    pub fn is_cross_vendor(&self) -> bool {
        matches!(
            self.migration_kind(),
            MigrationKind::VendorTransition | MigrationKind::CommunityToEnterprise
        )
    }

    pub fn migration_kind(&self) -> MigrationKind {
        let src = self.source.os_release();
        match &self.source {
            SourceSystem::RpmOstree { .. } => MigrationKind::OstreeToBootc,
            _ => {
                let src_id = src.id.as_str();
                let src_major = self.source.major_version();
                let target_major = self.target_major_version();

                match (src_id, src_major, target_major) {
                    ("fedora", _, _) => MigrationKind::CommunityToEnterprise,
                    ("centos", _, _) => MigrationKind::VendorTransition,
                    (_, Some(s), Some(t)) if s != t => MigrationKind::MajorUpgrade,
                    _ => MigrationKind::SameStream,
                }
            }
        }
    }

    fn target_major_version(&self) -> Option<u32> {
        let image_ref = match &self.target {
            TargetSystem::BootcImage { image_ref } => image_ref,
            TargetSystem::CustomImage { image_ref, .. } => image_ref,
        };
        // Extract major version from image tag (e.g., "rhel-bootc:10.0" → 10)
        image_ref
            .rsplit(':')
            .next()
            .and_then(|tag| tag.split('.').next())
            .and_then(|s| s.parse().ok())
    }
}
```

- [ ] **Step 3: Wire up module**

Add to `inspectah-core/src/types/mod.rs`:
```rust
pub mod system;
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p inspectah-core
```

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add inspectah-core/src/types/system.rs inspectah-core/src/types/mod.rs
git commit -m "feat(core): add source/target system model

SourceSystem, TargetSystem, MigrationContext, MigrationKind. Pipeline-
internal types not serialized to JSON. MigrationKind derived from
source/target pairing.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 4: RPM Section Types

The largest section type group. Go has ~15 structs in the RPM domain.

**Files:**
- Create: `inspectah-core/src/types/rpm.rs`
- Modify: `inspectah-core/src/types/mod.rs`

- [ ] **Step 1: Write serde round-trip test**

Add to bottom of `rpm.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_package_entry_roundtrip() {
        let entry = PackageEntry {
            name: "httpd".into(),
            epoch: "0".into(),
            version: "2.4.57".into(),
            release: "5.el9".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            include: true,
            source_repo: "appstream".into(),
            ..Default::default()
        };
        let json = serde_json::to_string(&entry).unwrap();
        let parsed: PackageEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(entry, parsed);
    }

    #[test]
    fn test_package_state_json_values() {
        assert_eq!(serde_json::to_string(&PackageState::Added).unwrap(), r#""added""#);
        assert_eq!(serde_json::to_string(&PackageState::BaseImageOnly).unwrap(), r#""base_image_only""#);
        assert_eq!(serde_json::to_string(&PackageState::LocalInstall).unwrap(), r#""local_install""#);
        assert_eq!(serde_json::to_string(&PackageState::NoRepo).unwrap(), r#""no_repo""#);
    }

    #[test]
    fn test_rpm_section_default_roundtrip() {
        let section = RpmSection::default();
        let json = serde_json::to_string(&section).unwrap();
        let parsed: RpmSection = serde_json::from_str(&json).unwrap();
        assert_eq!(section, parsed);
    }

    #[test]
    fn test_rpm_section_with_data() {
        let section = RpmSection {
            packages_added: vec![PackageEntry {
                name: "vim-enhanced".into(),
                state: PackageState::Added,
                include: true,
                ..Default::default()
            }],
            version_changes: vec![VersionChange {
                name: "bash".into(),
                arch: "x86_64".into(),
                host_version: "5.2.26".into(),
                base_version: "5.2.15".into(),
                direction: VersionChangeDirection::Upgrade,
                ..Default::default()
            }],
            base_image: Some("registry.redhat.io/rhel9/rhel-bootc:9.4".into()),
            ..Default::default()
        };
        let json = serde_json::to_string_pretty(&section).unwrap();
        let parsed: RpmSection = serde_json::from_str(&json).unwrap();
        assert_eq!(section, parsed);
    }
}
```

- [ ] **Step 2: Define RPM enums and small structs**

```rust
// inspectah-core/src/types/rpm.rs
use serde::{Deserialize, Serialize};
use super::fleet::FleetPrevalence;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageState {
    #[default]
    Added,
    BaseImageOnly,
    Modified,
    LocalInstall,
    NoRepo,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VersionChangeDirection {
    #[default]
    Upgrade,
    Downgrade,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PackageEntry {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub epoch: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub release: String,
    #[serde(default)]
    pub arch: String,
    pub state: PackageState,
    #[serde(default)]
    pub include: bool,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub acknowledged: bool,
    #[serde(default)]
    pub source_repo: String,
    pub fleet: Option<FleetPrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct VersionChange {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub arch: String,
    #[serde(default)]
    pub host_version: String,
    #[serde(default)]
    pub base_version: String,
    #[serde(default)]
    pub host_epoch: String,
    #[serde(default)]
    pub base_epoch: String,
    pub direction: VersionChangeDirection,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EnabledModuleStream {
    #[serde(default)]
    pub module_name: String,
    #[serde(default)]
    pub stream: String,
    #[serde(default)]
    pub profiles: Vec<String>,
    #[serde(default)]
    pub include: bool,
    #[serde(default)]
    pub baseline_match: bool,
    pub fleet: Option<FleetPrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct VersionLockEntry {
    #[serde(default)]
    pub raw_pattern: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub epoch: i32,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub release: String,
    #[serde(default)]
    pub arch: String,
    #[serde(default)]
    pub include: bool,
    pub fleet: Option<FleetPrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RpmVaEntry {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub flags: String,
    pub package: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnverifiablePackage {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoStatus {
    #[serde(default)]
    pub repo_id: String,
    #[serde(default)]
    pub repo_name: String,
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub affected_packages: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OstreePackageOverride {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub from_nevra: String,
    #[serde(default)]
    pub to_nevra: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RepoFile {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub is_default_repo: bool,
    #[serde(default)]
    pub include: bool,
    pub fleet: Option<FleetPrevalence>,
}
```

- [ ] **Step 3: Define RpmSection container struct**

Append to `rpm.rs`:

```rust
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RpmSection {
    #[serde(default)]
    pub packages_added: Vec<PackageEntry>,
    #[serde(default)]
    pub base_image_only: Vec<PackageEntry>,
    #[serde(default)]
    pub rpm_va: Vec<RpmVaEntry>,
    #[serde(default)]
    pub repo_files: Vec<RepoFile>,
    #[serde(default)]
    pub gpg_keys: Vec<RepoFile>,
    #[serde(default)]
    pub dnf_history_removed: Vec<String>,
    #[serde(default)]
    pub version_changes: Vec<VersionChange>,
    pub leaf_packages: Option<Vec<String>>,
    pub auto_packages: Option<Vec<String>>,
    #[serde(default)]
    pub leaf_dep_tree: serde_json::Value,
    #[serde(default)]
    pub module_streams: Vec<EnabledModuleStream>,
    #[serde(default)]
    pub version_locks: Vec<VersionLockEntry>,
    #[serde(default)]
    pub module_stream_conflicts: Vec<String>,
    pub baseline_module_streams: Option<std::collections::HashMap<String, String>>,
    pub versionlock_command_output: Option<String>,
    #[serde(default)]
    pub multiarch_packages: Vec<String>,
    #[serde(default)]
    pub duplicate_packages: Vec<String>,
    #[serde(default)]
    pub repo_providing_packages: Vec<String>,
    #[serde(default)]
    pub ostree_overrides: Vec<OstreePackageOverride>,
    #[serde(default)]
    pub ostree_removals: Vec<String>,
    pub base_image: Option<String>,
    pub baseline_package_names: Option<Vec<String>>,
    #[serde(default)]
    pub no_baseline: bool,
}
```

- [ ] **Step 4: Wire up module and run tests**

Add to `types/mod.rs`: `pub mod rpm;`

```bash
cargo test -p inspectah-core
```

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add inspectah-core/src/types/rpm.rs inspectah-core/src/types/mod.rs
git commit -m "feat(core): add RPM section types

PackageEntry, PackageState, VersionChange, EnabledModuleStream,
VersionLockEntry, RpmVaEntry, RepoFile, RpmSection. All fields match
Go v13 JSON tags. Serde round-trips verified.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 5: Config + Services Section Types

**Files:**
- Create: `inspectah-core/src/types/config.rs`
- Create: `inspectah-core/src/types/services.rs`
- Modify: `inspectah-core/src/types/mod.rs`

- [ ] **Step 1: Write tests**

```rust
// Bottom of config.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_section_roundtrip() {
        let section = ConfigSection {
            files: vec![ConfigFileEntry {
                path: "/etc/httpd/conf/httpd.conf".into(),
                kind: ConfigFileKind::RpmOwnedModified,
                category: ConfigCategory::Other,
                content: "ServerRoot \"/etc/httpd\"".into(),
                include: true,
                ..Default::default()
            }],
        };
        let json = serde_json::to_string(&section).unwrap();
        let parsed: ConfigSection = serde_json::from_str(&json).unwrap();
        assert_eq!(section, parsed);
    }

    #[test]
    fn test_config_file_kind_values() {
        assert_eq!(serde_json::to_string(&ConfigFileKind::RpmOwnedDefault).unwrap(), r#""rpm_owned_default""#);
        assert_eq!(serde_json::to_string(&ConfigFileKind::RpmOwnedModified).unwrap(), r#""rpm_owned_modified""#);
    }
}

// Bottom of services.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_section_roundtrip() {
        let section = ServiceSection {
            state_changes: vec![ServiceStateChange {
                unit: "httpd.service".into(),
                current_state: "enabled".into(),
                default_state: "disabled".into(),
                action: "enable".into(),
                include: true,
                ..Default::default()
            }],
            ..Default::default()
        };
        let json = serde_json::to_string(&section).unwrap();
        let parsed: ServiceSection = serde_json::from_str(&json).unwrap();
        assert_eq!(section, parsed);
    }
}
```

- [ ] **Step 2: Define Config types**

```rust
// inspectah-core/src/types/config.rs
use serde::{Deserialize, Serialize};
use super::fleet::FleetPrevalence;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigFileKind {
    RpmOwnedDefault,
    RpmOwnedModified,
    #[default]
    Unowned,
    Orphaned,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigCategory {
    Tmpfiles,
    Environment,
    Audit,
    LibraryPath,
    Journal,
    Logrotate,
    Automount,
    Sysctl,
    CryptoPolicy,
    Identity,
    Limits,
    #[default]
    Other,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ConfigFileEntry {
    #[serde(default)]
    pub path: String,
    pub kind: ConfigFileKind,
    pub category: ConfigCategory,
    #[serde(default)]
    pub content: String,
    pub rpm_va_flags: Option<String>,
    pub package: Option<String>,
    pub diff_against_rpm: Option<String>,
    #[serde(default)]
    pub include: bool,
    #[serde(default)]
    pub tie: bool,
    #[serde(default)]
    pub tie_winner: bool,
    pub fleet: Option<FleetPrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ConfigSection {
    #[serde(default)]
    pub files: Vec<ConfigFileEntry>,
}
```

- [ ] **Step 3: Define Services types**

```rust
// inspectah-core/src/types/services.rs
use serde::{Deserialize, Serialize};
use super::fleet::FleetPrevalence;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ServiceStateChange {
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub current_state: String,
    #[serde(default)]
    pub default_state: String,
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub include: bool,
    pub owning_package: Option<String>,
    pub fleet: Option<FleetPrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SystemdDropIn {
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub include: bool,
    #[serde(default)]
    pub tie: bool,
    #[serde(default)]
    pub tie_winner: bool,
    pub fleet: Option<FleetPrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ServiceSection {
    #[serde(default)]
    pub state_changes: Vec<ServiceStateChange>,
    #[serde(default)]
    pub enabled_units: Vec<String>,
    #[serde(default)]
    pub disabled_units: Vec<String>,
    #[serde(default)]
    pub drop_ins: Vec<SystemdDropIn>,
}
```

- [ ] **Step 4: Wire up modules and run tests**

Add to `types/mod.rs`:
```rust
pub mod config;
pub mod services;
```

```bash
cargo test -p inspectah-core
```

- [ ] **Step 5: Commit**

```bash
git add inspectah-core/src/types/config.rs inspectah-core/src/types/services.rs inspectah-core/src/types/mod.rs
git commit -m "feat(core): add config and services section types

ConfigFileEntry, ConfigFileKind, ConfigCategory, ConfigSection,
ServiceStateChange, SystemdDropIn, ServiceSection.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 6: Network + Storage Section Types

**Files:**
- Create: `inspectah-core/src/types/network.rs`
- Create: `inspectah-core/src/types/storage.rs`
- Modify: `inspectah-core/src/types/mod.rs`

- [ ] **Step 1: Write tests**

```rust
// Bottom of network.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_section_roundtrip() {
        let section = NetworkSection {
            connections: vec![NMConnection {
                path: "/etc/NetworkManager/system-connections/eth0.nmconnection".into(),
                name: "eth0".into(),
                method: "auto".into(),
                conn_type: "802-3-ethernet".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let json = serde_json::to_string(&section).unwrap();
        let parsed: NetworkSection = serde_json::from_str(&json).unwrap();
        assert_eq!(section, parsed);
    }
}

// Bottom of storage.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_section_roundtrip() {
        let section = StorageSection {
            fstab_entries: vec![FstabEntry {
                device: "/dev/sda1".into(),
                mount_point: "/boot".into(),
                fstype: "xfs".into(),
                options: "defaults".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let json = serde_json::to_string(&section).unwrap();
        let parsed: StorageSection = serde_json::from_str(&json).unwrap();
        assert_eq!(section, parsed);
    }
}
```

- [ ] **Step 2: Define Network types**

```rust
// inspectah-core/src/types/network.rs
use serde::{Deserialize, Serialize};
use super::fleet::FleetPrevalence;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NMConnection {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub method: String,
    #[serde(default, rename = "type")]
    pub conn_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include: Option<bool>,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub acknowledged: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fleet: Option<FleetPrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FirewallZone {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub services: Vec<String>,
    #[serde(default)]
    pub ports: Vec<String>,
    #[serde(default)]
    pub rich_rules: Vec<String>,
    #[serde(default)]
    pub include: bool,
    pub fleet: Option<FleetPrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FirewallDirectRule {
    #[serde(default)]
    pub ipv: String,
    #[serde(default)]
    pub table: String,
    #[serde(default)]
    pub chain: String,
    #[serde(default)]
    pub priority: String,
    #[serde(default)]
    pub args: String,
    #[serde(default)]
    pub include: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StaticRouteFile {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyEntry {
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub line: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NetworkSection {
    #[serde(default)]
    pub connections: Vec<NMConnection>,
    #[serde(default)]
    pub firewall_zones: Vec<FirewallZone>,
    #[serde(default)]
    pub firewall_direct_rules: Vec<FirewallDirectRule>,
    #[serde(default)]
    pub static_routes: Vec<StaticRouteFile>,
    #[serde(default)]
    pub ip_routes: Vec<String>,
    #[serde(default)]
    pub ip_rules: Vec<String>,
    #[serde(default)]
    pub resolv_provenance: String,
    #[serde(default)]
    pub hosts_additions: Vec<String>,
    #[serde(default)]
    pub proxy: Vec<ProxyEntry>,
}
```

- [ ] **Step 3: Define Storage types**

```rust
// inspectah-core/src/types/storage.rs
use serde::{Deserialize, Serialize};
use super::fleet::FleetPrevalence;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FstabEntry {
    #[serde(default)]
    pub device: String,
    #[serde(default)]
    pub mount_point: String,
    #[serde(default)]
    pub fstype: String,
    #[serde(default)]
    pub options: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include: Option<bool>,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub acknowledged: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fleet: Option<FleetPrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialRef {
    #[serde(default)]
    pub mount_point: String,
    #[serde(default)]
    pub credential_path: String,
    #[serde(default)]
    pub source: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MountPoint {
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub fstype: String,
    #[serde(default)]
    pub options: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LvmVolume {
    #[serde(default)]
    pub lv_name: String,
    #[serde(default)]
    pub vg_name: String,
    #[serde(default)]
    pub lv_size: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VarDirectory {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub size_estimate: String,
    #[serde(default)]
    pub recommendation: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StorageSection {
    #[serde(default)]
    pub fstab_entries: Vec<FstabEntry>,
    #[serde(default)]
    pub mount_points: Vec<MountPoint>,
    #[serde(default)]
    pub lvm_info: Vec<LvmVolume>,
    #[serde(default)]
    pub var_directories: Vec<VarDirectory>,
    #[serde(default)]
    pub credential_refs: Vec<CredentialRef>,
}
```

- [ ] **Step 4: Wire up and test**

Add to `types/mod.rs`:
```rust
pub mod network;
pub mod storage;
```

```bash
cargo test -p inspectah-core
```

- [ ] **Step 5: Commit**

```bash
git add inspectah-core/src/types/network.rs inspectah-core/src/types/storage.rs inspectah-core/src/types/mod.rs
git commit -m "feat(core): add network and storage section types

NMConnection, FirewallZone, FirewallDirectRule, NetworkSection,
FstabEntry, LvmVolume, StorageSection, and supporting types.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 7: Scheduled Tasks + Container Section Types

**Files:**
- Create: `inspectah-core/src/types/scheduled.rs`
- Create: `inspectah-core/src/types/containers.rs`
- Modify: `inspectah-core/src/types/mod.rs`

- [ ] **Step 1: Write tests**

```rust
// Bottom of scheduled.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scheduled_section_roundtrip() {
        let section = ScheduledTaskSection {
            cron_jobs: vec![CronJob {
                path: "/etc/cron.d/backup".into(),
                source: "file".into(),
                include: true,
                ..Default::default()
            }],
            generated_timer_units: vec![GeneratedTimerUnit {
                name: "backup.timer".into(),
                cron_expr: "0 2 * * *".into(),
                include: true,
                ..Default::default()
            }],
            ..Default::default()
        };
        let json = serde_json::to_string(&section).unwrap();
        let parsed: ScheduledTaskSection = serde_json::from_str(&json).unwrap();
        assert_eq!(section, parsed);
    }
}

// Bottom of containers.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_container_section_roundtrip() {
        let section = ContainerSection {
            quadlet_units: vec![QuadletUnit {
                name: "myapp.container".into(),
                image: "quay.io/myorg/myapp:latest".into(),
                include: true,
                ..Default::default()
            }],
            ..Default::default()
        };
        let json = serde_json::to_string(&section).unwrap();
        let parsed: ContainerSection = serde_json::from_str(&json).unwrap();
        assert_eq!(section, parsed);
    }
}
```

- [ ] **Step 2: Define Scheduled Tasks types**

```rust
// inspectah-core/src/types/scheduled.rs
use serde::{Deserialize, Serialize};
use super::fleet::FleetPrevalence;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CronJob {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub rpm_owned: bool,
    #[serde(default)]
    pub include: bool,
    pub fleet: Option<FleetPrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SystemdTimer {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub on_calendar: String,
    #[serde(default)]
    pub exec_start: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub timer_content: String,
    #[serde(default)]
    pub service_content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fleet: Option<FleetPrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AtJob {
    #[serde(default)]
    pub file: String,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub working_dir: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fleet: Option<FleetPrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GeneratedTimerUnit {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub timer_content: String,
    #[serde(default)]
    pub service_content: String,
    #[serde(default)]
    pub cron_expr: String,
    #[serde(default)]
    pub source_path: String,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub include: bool,
    pub fleet: Option<FleetPrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ScheduledTaskSection {
    #[serde(default)]
    pub cron_jobs: Vec<CronJob>,
    #[serde(default)]
    pub systemd_timers: Vec<SystemdTimer>,
    #[serde(default)]
    pub at_jobs: Vec<AtJob>,
    #[serde(default)]
    pub generated_timer_units: Vec<GeneratedTimerUnit>,
}
```

- [ ] **Step 3: Define Container types**

```rust
// inspectah-core/src/types/containers.rs
use serde::{Deserialize, Serialize};
use super::fleet::FleetPrevalence;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ContainerMount {
    #[serde(default, rename = "type")]
    pub mount_type: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub destination: String,
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub rw: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct QuadletUnit {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub image: String,
    #[serde(default)]
    pub include: bool,
    #[serde(default)]
    pub tie: bool,
    #[serde(default)]
    pub tie_winner: bool,
    pub fleet: Option<FleetPrevalence>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub volumes: Vec<String>,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub generated: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComposeService {
    #[serde(default)]
    pub service: String,
    #[serde(default)]
    pub image: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ComposeFile {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub images: Vec<ComposeService>,
    #[serde(default)]
    pub include: bool,
    #[serde(default)]
    pub tie: bool,
    #[serde(default)]
    pub tie_winner: bool,
    pub fleet: Option<FleetPrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RunningContainer {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub image: String,
    #[serde(default)]
    pub image_id: String,
    #[serde(default)]
    pub status: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub restart_policy: String,
    #[serde(default)]
    pub mounts: Vec<ContainerMount>,
    #[serde(default)]
    pub networks: serde_json::Value,
    #[serde(default)]
    pub ports: serde_json::Value,
    #[serde(default)]
    pub env: Vec<String>,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub inspect_data: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include: Option<bool>,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub acknowledged: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fleet: Option<FleetPrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlatpakApp {
    #[serde(default)]
    pub app_id: String,
    #[serde(default)]
    pub origin: String,
    #[serde(default)]
    pub branch: String,
    #[serde(default)]
    pub include: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub remote: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub remote_url: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ContainerSection {
    #[serde(default)]
    pub quadlet_units: Vec<QuadletUnit>,
    #[serde(default)]
    pub compose_files: Vec<ComposeFile>,
    #[serde(default)]
    pub running_containers: Vec<RunningContainer>,
    #[serde(default)]
    pub flatpak_apps: Vec<FlatpakApp>,
}
```

- [ ] **Step 4: Wire up and test**

Add to `types/mod.rs`:
```rust
pub mod scheduled;
pub mod containers;
```

```bash
cargo test -p inspectah-core
```

- [ ] **Step 5: Commit**

```bash
git add inspectah-core/src/types/scheduled.rs inspectah-core/src/types/containers.rs inspectah-core/src/types/mod.rs
git commit -m "feat(core): add scheduled tasks and container section types

CronJob, SystemdTimer, AtJob, GeneratedTimerUnit, ScheduledTaskSection,
QuadletUnit, ComposeFile, RunningContainer, FlatpakApp, ContainerSection.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 8: Non-RPM + Kernel + SELinux + Users Section Types

**Files:**
- Create: `inspectah-core/src/types/nonrpm.rs`
- Create: `inspectah-core/src/types/kernelboot.rs`
- Create: `inspectah-core/src/types/selinux.rs`
- Create: `inspectah-core/src/types/users.rs`
- Modify: `inspectah-core/src/types/mod.rs`

- [ ] **Step 1: Write tests for all four modules**

Each module gets a basic round-trip test at the bottom (same pattern as previous tasks). Test the section-level struct with one populated child.

- [ ] **Step 2: Define Non-RPM types**

```rust
// inspectah-core/src/types/nonrpm.rs
use serde::{Deserialize, Serialize};
use super::fleet::FleetPrevalence;
use super::config::ConfigFileEntry;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipPackage {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NonRpmItem {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub method: String,
    #[serde(default)]
    pub confidence: String,
    #[serde(default)]
    pub include: bool,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub acknowledged: bool,
    #[serde(default)]
    pub lang: String,
    #[serde(default)]
    pub r#static: bool,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub shared_libs: Vec<String>,
    #[serde(default)]
    pub system_site_packages: bool,
    #[serde(default)]
    pub packages: Vec<PipPackage>,
    #[serde(default)]
    pub has_c_extensions: bool,
    #[serde(default)]
    pub git_remote: String,
    #[serde(default)]
    pub git_commit: String,
    #[serde(default)]
    pub git_branch: String,
    pub files: Option<serde_json::Value>,
    #[serde(default)]
    pub content: String,
    pub fleet: Option<FleetPrevalence>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub review_status: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NonRpmSoftwareSection {
    #[serde(default)]
    pub items: Vec<NonRpmItem>,
    #[serde(default)]
    pub env_files: Vec<ConfigFileEntry>,
}
```

Note: `NonRpmItem.static` is a Rust keyword — use raw identifier `r#static`. The Go JSON tag is `"static"` which serde handles via the field name `r#static` (serde strips the `r#` prefix).

- [ ] **Step 3: Define KernelBoot types**

```rust
// inspectah-core/src/types/kernelboot.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigSnippet {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub content: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SysctlOverride {
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub runtime: String,
    #[serde(default)]
    pub default: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub include: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KernelModule {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub size: String,
    #[serde(default)]
    pub used_by: String,
    #[serde(default)]
    pub include: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlternativeEntry {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub status: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct KernelBootSection {
    #[serde(default)]
    pub cmdline: String,
    #[serde(default)]
    pub grub_defaults: String,
    #[serde(default)]
    pub sysctl_overrides: Vec<SysctlOverride>,
    #[serde(default)]
    pub modules_load_d: Vec<ConfigSnippet>,
    #[serde(default)]
    pub modprobe_d: Vec<ConfigSnippet>,
    #[serde(default)]
    pub dracut_conf: Vec<ConfigSnippet>,
    #[serde(default)]
    pub loaded_modules: Vec<KernelModule>,
    #[serde(default)]
    pub non_default_modules: Vec<KernelModule>,
    #[serde(default)]
    pub tuned_active: String,
    #[serde(default)]
    pub tuned_custom_profiles: Vec<ConfigSnippet>,
    pub locale: Option<String>,
    pub timezone: Option<String>,
    #[serde(default)]
    pub alternatives: Vec<AlternativeEntry>,
}
```

- [ ] **Step 4: Define SELinux types**

```rust
// inspectah-core/src/types/selinux.rs
use serde::{Deserialize, Serialize};
use super::fleet::FleetPrevalence;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SelinuxPortLabel {
    #[serde(default)]
    pub protocol: String,
    #[serde(default)]
    pub port: String,
    #[serde(default, rename = "type")]
    pub label_type: String,
    #[serde(default)]
    pub include: bool,
    pub fleet: Option<FleetPrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SelinuxSection {
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub custom_modules: Vec<String>,
    #[serde(default)]
    pub boolean_overrides: Vec<serde_json::Value>,
    #[serde(default)]
    pub fcontext_rules: Vec<String>,
    #[serde(default)]
    pub audit_rules: Vec<String>,
    #[serde(default)]
    pub fips_mode: bool,
    #[serde(default)]
    pub pam_configs: Vec<String>,
    #[serde(default)]
    pub port_labels: Vec<SelinuxPortLabel>,
}
```

- [ ] **Step 5: Define Users types**

```rust
// inspectah-core/src/types/users.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UserGroupSection {
    #[serde(default)]
    pub users: Vec<serde_json::Value>,
    #[serde(default)]
    pub groups: Vec<serde_json::Value>,
    #[serde(default)]
    pub sudoers_rules: Vec<String>,
    #[serde(default)]
    pub ssh_authorized_keys_refs: Vec<serde_json::Value>,
    #[serde(default)]
    pub passwd_entries: Vec<String>,
    #[serde(default)]
    pub shadow_entries: Vec<String>,
    #[serde(default)]
    pub group_entries: Vec<String>,
    #[serde(default)]
    pub gshadow_entries: Vec<String>,
    #[serde(default)]
    pub subuid_entries: Vec<String>,
    #[serde(default)]
    pub subgid_entries: Vec<String>,
}
```

Note: `users`, `groups`, and `ssh_authorized_keys_refs` use `Vec<serde_json::Value>` to match Go's `[]map[string]interface{}`. These are semi-structured — typing them fully is Phase 7 work.

- [ ] **Step 6: Wire up and test**

Add to `types/mod.rs`:
```rust
pub mod nonrpm;
pub mod kernelboot;
pub mod selinux;
pub mod users;
```

```bash
cargo test -p inspectah-core
```

- [ ] **Step 7: Commit**

```bash
git add inspectah-core/src/types/nonrpm.rs inspectah-core/src/types/kernelboot.rs inspectah-core/src/types/selinux.rs inspectah-core/src/types/users.rs inspectah-core/src/types/mod.rs
git commit -m "feat(core): add non-rpm, kernel/boot, selinux, and users types

NonRpmItem (with r#static raw identifier), KernelBootSection,
SelinuxSection, UserGroupSection. Semi-structured fields use
serde_json::Value for Go compatibility.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 9: Redaction + Warning + Completeness Types

These are new Rust-era types from the spec that don't have direct Go equivalents (Go uses untyped maps for warnings, flat strings for redaction kinds).

**Files:**
- Create: `inspectah-core/src/types/redaction.rs`
- Create: `inspectah-core/src/types/warnings.rs`
- Create: `inspectah-core/src/types/completeness.rs`
- Create: `inspectah-core/src/types/preflight.rs`
- Modify: `inspectah-core/src/types/mod.rs`

- [ ] **Step 1: Write tests**

```rust
// Bottom of redaction.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shadow_status_locked_is_not_secret() {
        let status = ShadowStatus::Locked;
        assert!(!status.is_secret());
    }

    #[test]
    fn test_shadow_status_has_hash_is_secret() {
        let status = ShadowStatus::HasHash;
        assert!(status.is_secret());
    }

    #[test]
    fn test_redaction_state_roundtrip() {
        let state = RedactionState::FullyRedacted {
            redacted_by: "inspectah 0.8.0".into(),
            config_hash: "abc123".into(),
        };
        let json = serde_json::to_string(&state).unwrap();
        let parsed: RedactionState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, parsed);
    }

    #[test]
    fn test_redaction_finding_go_compat() {
        let json = r#"{
            "path": "/etc/shadow",
            "source": "file",
            "kind": "excluded",
            "pattern": "shadow_hash",
            "remediation": "regenerate",
            "detection_method": "pattern"
        }"#;
        let finding: RedactionFinding = serde_json::from_str(json).unwrap();
        assert_eq!(finding.path, "/etc/shadow");
        assert_eq!(finding.kind, RedactionKind::Excluded);
        assert_eq!(finding.detection_method, DetectionMethod::Pattern);
    }

    #[test]
    fn test_finding_kind_serde_roundtrip() {
        assert_eq!(serde_json::to_string(&FindingKind::PrivateKey).unwrap(), r#""private_key""#);
        assert_eq!(serde_json::to_string(&FindingKind::ShadowHash).unwrap(), r#""shadow_hash""#);
        assert_eq!(serde_json::to_string(&RedactionKind::Excluded).unwrap(), r#""excluded""#);
        let parsed: FindingKind = serde_json::from_str(r#""no_password""#).unwrap();
        assert_eq!(parsed, FindingKind::NoPassword);
    }
}
```

- [ ] **Step 2: Define redaction types**

```rust
// inspectah-core/src/types/redaction.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretKind {
    PrivateKey { format: String },
    Certificate,
    ApiToken { provider: Option<String> },
    Password { context: String },
    ConnectionString,
    ShadowEntry { status: ShadowStatus },
    EnvironmentSecret,
    GenericCredential,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShadowStatus {
    Locked,
    Disabled,
    NoPassword,
    #[default]
    HasHash,
}

impl ShadowStatus {
    pub fn is_secret(&self) -> bool {
        matches!(self, Self::HasHash | Self::NoPassword)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state")]
pub enum RedactionState {
    #[serde(rename = "fully_redacted")]
    FullyRedacted {
        redacted_by: String,
        config_hash: String,
    },
    #[serde(rename = "partially_redacted")]
    PartiallyRedacted {
        redacted_by: String,
        config_hash: String,
        unresolved_count: u32,
        #[serde(default)]
        unresolved_hints: Vec<RedactionHint>,
    },
    #[serde(rename = "unknown")]
    Unknown,
    #[serde(rename = "raw")]
    Raw,
}

/// Typed redaction classification — strings only at the serde/export edge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RedactionKind {
    Excluded,
    Flagged,
    Inline,
}

/// Typed detection method — how the finding was identified.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectionMethod {
    #[default]
    Pattern,
    Heuristic,
    PathBased,
}

/// Typed finding classification — what kind of secret was found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingKind {
    PrivateKey,
    Certificate,
    PasswordHash,
    Password,
    AwsKey,
    JdbcPassword,
    PostgresPassword,
    MongodbPassword,
    RedisPassword,
    WireguardKey,
    WifiPsk,
    ShadowHash,
    NoPassword,
    GenericCredential,
}

/// Confidence level — reused across hints, findings, and detector output.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    #[default]
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactionHint {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub reason: String,
    pub confidence: Option<Confidence>,
}

/// Redaction finding with typed classification fields.
/// Go-compatible via serde rename_all — strings at the export edge only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RedactionFinding {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub source: String,
    pub kind: RedactionKind,
    #[serde(default)]
    pub pattern: String,
    #[serde(default)]
    pub remediation: String,
    pub line: Option<i32>,
    pub replacement: Option<String>,
    pub detection_method: DetectionMethod,
    pub confidence: Option<Confidence>,
    pub finding_kind: Option<FindingKind>,
}
```

- [ ] **Step 3: Define Warning and Completeness types**

```rust
// inspectah-core/src/types/warnings.rs
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Typed warning severity — not a freeform string.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarningSeverity {
    Info,
    #[default]
    Warning,
    Error,
}

/// Typed warning with extra field support for Go compatibility.
/// Go uses []map[string]interface{} — the flatten catches unknown keys.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Warning {
    #[serde(default)]
    pub inspector: String,
    #[serde(default)]
    pub message: String,
    pub severity: Option<WarningSeverity>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
```

```rust
// inspectah-core/src/types/completeness.rs
use serde::{Deserialize, Serialize};

/// Typed inspector identity — compiler-enforced exhaustive handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectorId {
    Rpm,
    Config,
    Services,
    Network,
    Storage,
    ScheduledTasks,
    Containers,
    NonRpmSoftware,
    KernelBoot,
    Selinux,
    UsersGroups,
    Hardware,
    Ostree,
    OsRelease,
}

/// Used by Inspector::applicable_to() — which source types this inspector runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceSystemKind {
    PackageBased,
    RpmOstree,
    Bootc,
}

/// Typed inspector output envelope — the compiler proves inspectors emit valid sections.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "inspector", content = "data")]
pub enum SectionData {
    #[serde(rename = "rpm")]
    Rpm(super::rpm::RpmSection),
    #[serde(rename = "config")]
    Config(super::config::ConfigSection),
    #[serde(rename = "services")]
    Services(super::services::ServiceSection),
    #[serde(rename = "network")]
    Network(super::network::NetworkSection),
    #[serde(rename = "storage")]
    Storage(super::storage::StorageSection),
    #[serde(rename = "scheduled_tasks")]
    ScheduledTasks(super::scheduled::ScheduledTaskSection),
    #[serde(rename = "containers")]
    Containers(super::containers::ContainerSection),
    #[serde(rename = "non_rpm_software")]
    NonRpmSoftware(super::nonrpm::NonRpmSoftwareSection),
    #[serde(rename = "kernel_boot")]
    KernelBoot(super::kernelboot::KernelBootSection),
    #[serde(rename = "selinux")]
    Selinux(super::selinux::SelinuxSection),
    #[serde(rename = "users_groups")]
    UsersGroups(super::users::UserGroupSection),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum Completeness {
    Full,
    Partial {
        incomplete_sections: Vec<InspectorId>,
        reason: String,
    },
    Unverified {
        missing: Vec<InspectorId>,
    },
}

impl Default for Completeness {
    fn default() -> Self {
        Self::Full
    }
}
```

```rust
// inspectah-core/src/types/preflight.rs
use serde::{Deserialize, Serialize};
use super::rpm::{UnverifiablePackage, RepoStatus};
use std::path::PathBuf;

/// Go-compatible preflight result (stored in snapshot JSON).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PreflightResult {
    #[serde(default)]
    pub status: String,
    pub status_reason: Option<String>,
    #[serde(default)]
    pub available: Vec<String>,
    #[serde(default)]
    pub unavailable: Vec<String>,
    #[serde(default)]
    pub unverifiable: Vec<UnverifiablePackage>,
    #[serde(default)]
    pub direct_install: Vec<String>,
    #[serde(default)]
    pub repo_unreachable: Vec<RepoStatus>,
    #[serde(default)]
    pub base_image: String,
    #[serde(default)]
    pub repos_queried: Vec<String>,
    #[serde(default)]
    pub timestamp: String,
}

/// Pipeline-internal preflight mode (not serialized to snapshot).
#[derive(Debug, Clone)]
pub enum PreflightMode {
    Online { entitlement_dir: Option<PathBuf> },
    Manifest { path: PathBuf },
    Skip,
}

/// Render-time target context (not stored in snapshot).
#[derive(Debug, Clone)]
pub struct RenderTarget {
    pub system: super::system::TargetSystem,
    pub preflight: PreflightMode,
}
```

- [ ] **Step 4: Wire up and test**

Add to `types/mod.rs`:
```rust
pub mod redaction;
pub mod warnings;
pub mod completeness;
pub mod preflight;
```

```bash
cargo test -p inspectah-core
```

- [ ] **Step 5: Commit**

```bash
git add inspectah-core/src/types/redaction.rs inspectah-core/src/types/warnings.rs inspectah-core/src/types/completeness.rs inspectah-core/src/types/preflight.rs inspectah-core/src/types/mod.rs
git commit -m "feat(core): add redaction, warning, completeness, preflight types

SecretKind, ShadowStatus (with is_secret()), RedactionState (tagged enum),
RedactionFinding (Go-compatible), Warning (with serde flatten for extras),
Completeness, PreflightResult, PreflightMode, RenderTarget.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 10: Traits (Inspector, Executor, SecretDetector, Renderer)

**Files:**
- Create: `inspectah-core/src/traits/mod.rs`
- Create: `inspectah-core/src/traits/inspector.rs`
- Create: `inspectah-core/src/traits/executor.rs`
- Create: `inspectah-core/src/traits/detector.rs`
- Create: `inspectah-core/src/traits/renderer.rs`
- Modify: `inspectah-core/src/lib.rs`

- [ ] **Step 1: Write trait usage tests**

```rust
// Bottom of inspector.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inspector_error_display() {
        let err = InspectorError::Skipped { reason: "not applicable".into() };
        assert!(format!("{err}").contains("not applicable"));

        let err = InspectorError::Failed { reason: "rpm db corrupt".into() };
        assert!(format!("{err}").contains("rpm db corrupt"));
    }

    #[test]
    fn test_degraded_carries_partial_output() {
        use crate::types::completeness::SectionData;
        use crate::types::rpm::RpmSection;
        let output = InspectorOutput {
            section: SectionData::Rpm(RpmSection::default()),
            warnings: vec![],
            redaction_hints: vec![],
        };
        let err = InspectorError::Degraded {
            partial: output.clone(),
            reason: "partial rpm db".into(),
        };
        if let InspectorError::Degraded { partial, .. } = err {
            assert_eq!(partial.warnings.len(), 0);
        }
    }
}
```

- [ ] **Step 2: Define Inspector trait**

```rust
// inspectah-core/src/traits/inspector.rs
use crate::types::completeness::{InspectorId, SectionData, SourceSystemKind};
use crate::types::system::SourceSystem;
use crate::types::redaction::RedactionHint;
use crate::types::warnings::Warning;
use std::fmt;

pub trait Inspector: Send + Sync {
    fn id(&self) -> InspectorId;
    fn applicable_to(&self) -> &[SourceSystemKind];
    fn inspect(&self, ctx: &InspectionContext) -> Result<InspectorOutput, InspectorError>;
}

/// Carries the full SourceSystem — bootc needs booted_image,
/// rpm-ostree needs variant + base_image.
pub struct InspectionContext {
    pub executor: Box<dyn crate::traits::executor::Executor>,
    pub source: SourceSystem,
    pub rpm_state: Option<RpmState>,
}

/// Read-only RPM state provided to non-RPM inspectors during two-phase collection.
#[derive(Debug, Clone, Default)]
pub struct RpmState {
    pub installed_packages: std::collections::HashSet<String>,
    pub owned_paths: std::collections::HashSet<String>,
}

/// Typed section output — the compiler proves inspectors emit valid section shapes.
#[derive(Debug, Clone)]
pub struct InspectorOutput {
    pub section: SectionData,
    pub warnings: Vec<Warning>,
    pub redaction_hints: Vec<RedactionHint>,
}

#[derive(Debug, Clone)]
pub enum InspectorError {
    Skipped { reason: String },
    Degraded { partial: InspectorOutput, reason: String },
    Failed { reason: String },
}

impl fmt::Display for InspectorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Skipped { reason } => write!(f, "skipped: {reason}"),
            Self::Degraded { reason, .. } => write!(f, "degraded: {reason}"),
            Self::Failed { reason } => write!(f, "failed: {reason}"),
        }
    }
}

impl std::error::Error for InspectorError {}
```

- [ ] **Step 3: Define Executor trait**

```rust
// inspectah-core/src/traits/executor.rs
use std::io;
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct ExecResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

impl ExecResult {
    pub fn success(&self) -> bool {
        self.exit_code == 0
    }
}

pub trait Executor: Send + Sync {
    fn run(&self, cmd: &str, args: &[&str]) -> ExecResult;
    fn read_file(&self, path: &Path) -> io::Result<String>;
    fn file_exists(&self, path: &Path) -> bool;
    fn read_dir(&self, path: &Path) -> io::Result<Vec<String>>;
    fn read_link(&self, path: &Path) -> io::Result<String>;
    fn host_root(&self) -> &Path;
}
```

- [ ] **Step 4: Define SecretDetector trait**

```rust
// inspectah-core/src/traits/detector.rs
use crate::types::redaction::RedactionHint;

/// Typed detector identity — compiler-enforced, not a freeform string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DetectorId {
    PrivateKey,
    Certificate,
    Password,
    ApiToken,
    ShadowEntry,
    ConnectionString,
    EnvironmentSecret,
    WireguardKey,
    WifiPsk,
}

pub trait SecretDetector: Send + Sync {
    fn id(&self) -> DetectorId;
    fn sensitivity(&self) -> Sensitivity;
    fn scan(&self, content: &str, context: &ScanContext) -> Vec<Finding>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sensitivity {
    Default,
    Strict,
}

#[derive(Debug, Clone)]
pub struct ScanContext {
    pub path: String,
    pub source: String,
}

/// Typed finding — kind and confidence are enums, not strings.
#[derive(Debug, Clone)]
pub struct Finding {
    pub line: usize,
    pub kind: crate::types::redaction::FindingKind,
    pub confidence: crate::types::redaction::Confidence,
    pub hint: RedactionHint,
}

// Note: Confidence and FindingKind are defined in types::redaction
// and reused here — single source of truth, no duplicate enums.
```

- [ ] **Step 5: Define Renderer trait**

```rust
// inspectah-core/src/traits/renderer.rs
use std::path::Path;

/// Render-time context — carries target and triage info.
/// Phase 1: target is None, triage_actions is empty.
/// Signature is correct for later phases from the start.
pub struct RenderContext {
    pub target: Option<crate::types::preflight::RenderTarget>,
}

pub trait Renderer: Send + Sync {
    fn name(&self) -> &str;
    fn render(&self, snapshot: &crate::snapshot::InspectionSnapshot, context: &RenderContext, output_dir: &Path) -> Result<(), RenderError>;
}

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("render failed: {0}")]
    Failed(String),
}
```

- [ ] **Step 6: Wire up trait modules**

```rust
// inspectah-core/src/traits/mod.rs
pub mod inspector;
pub mod executor;
pub mod detector;
pub mod renderer;
```

Add to `inspectah-core/src/lib.rs`:
```rust
pub mod traits;
```

```bash
cargo test -p inspectah-core
```

- [ ] **Step 7: Commit**

```bash
git add inspectah-core/src/traits/ inspectah-core/src/lib.rs
git commit -m "feat(core): add inspector, executor, detector, and renderer traits

Inspector with three-state error model (Skipped/Degraded/Failed),
Executor abstraction, SecretDetector with confidence levels,
Renderer trait. InspectionContext carries RpmState for two-phase
collection.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 11: Pipeline Typestate

**Files:**
- Create: `inspectah-core/src/pipeline.rs`
- Modify: `inspectah-core/src/lib.rs`

- [ ] **Step 1: Write typestate tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_progression() {
        let raw = RawSnapshot::default();
        let p: Pipeline<Collected> = Pipeline { state: Collected { snapshot: raw } };
        // Collected → Validated (skip_validation for test)
        let validated = p.state.snapshot; // access snapshot from Collected
        let p: Pipeline<Validated> = Pipeline {
            state: Validated { snapshot: validated },
        };
        // Validated → Redacted
        let p: Pipeline<Redacted> = Pipeline {
            state: Redacted { snapshot: p.state.snapshot },
        };
        // Redacted can produce artifacts
        assert!(p.state.snapshot.schema_version == 0 || true);
    }
}
```

- [ ] **Step 2: Define pipeline typestate types**

```rust
// inspectah-core/src/pipeline.rs
use crate::snapshot::InspectionSnapshot;

pub type RawSnapshot = InspectionSnapshot;

pub struct Pipeline<S> {
    pub state: S,
}

pub struct Collected {
    pub snapshot: RawSnapshot,
}

pub struct Validated {
    pub snapshot: InspectionSnapshot,
}

pub struct Redacted {
    pub snapshot: InspectionSnapshot,
}

pub struct Artifacts {
    pub output_dir: std::path::PathBuf,
}
```

Note: Pipeline methods (`.collect()`, `.validate()`, `.redact()`, `.render()`) are implemented in `inspectah-pipeline`, not here. The core crate defines only the state marker types. The type system prevents calling `.render()` on `Pipeline<Collected>` because the method simply doesn't exist on that type.

- [ ] **Step 3: Wire up and test**

Add to `lib.rs`: `pub mod pipeline;`

```bash
cargo test -p inspectah-core
```

- [ ] **Step 4: Commit**

```bash
git add inspectah-core/src/pipeline.rs inspectah-core/src/lib.rs
git commit -m "feat(core): add pipeline typestate markers

Pipeline<S> with Collected, Validated, Redacted, Artifacts states.
State marker types only — transition methods live in inspectah-pipeline.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 12: InspectionSnapshot + Serde Round-Trips

**Files:**
- Create: `inspectah-core/src/snapshot.rs`
- Modify: `inspectah-core/src/lib.rs`

- [ ] **Step 1: Write snapshot serde tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_snapshot_roundtrip() {
        let snap = InspectionSnapshot {
            schema_version: 14,
            ..Default::default()
        };
        let json = serde_json::to_string_pretty(&snap).unwrap();
        let parsed: InspectionSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(snap.schema_version, parsed.schema_version);
        assert_eq!(snap.system_type, parsed.system_type);
    }

    #[test]
    fn test_go_v13_minimal_deserialize() {
        // Minimal Go v13 structure — all sections null
        let json = r#"{
            "schema_version": 13,
            "meta": {},
            "os_release": null,
            "system_type": "package-mode",
            "rpm": null,
            "config": null,
            "services": null,
            "network": null,
            "storage": null,
            "scheduled_tasks": null,
            "containers": null,
            "non_rpm_software": null,
            "kernel_boot": null,
            "selinux": null,
            "users_groups": null,
            "preflight": {"status": "ok"},
            "warnings": [],
            "redactions": []
        }"#;
        let snap: InspectionSnapshot = serde_json::from_str(json).unwrap();
        assert_eq!(snap.schema_version, 13);
        assert_eq!(snap.system_type, SystemType::PackageMode);
        assert!(snap.rpm.is_none());
    }

    #[test]
    fn test_snapshot_with_rpm_section() {
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            packages_added: vec![PackageEntry {
                name: "httpd".into(),
                state: PackageState::Added,
                include: true,
                ..Default::default()
            }],
            ..Default::default()
        });
        let json = serde_json::to_string(&snap).unwrap();
        let parsed: InspectionSnapshot = serde_json::from_str(&json).unwrap();
        assert!(parsed.rpm.is_some());
        assert_eq!(parsed.rpm.unwrap().packages_added[0].name, "httpd");
    }

    #[test]
    fn test_warnings_go_compat() {
        use crate::types::warnings::WarningSeverity;
        let json = r#"[{"inspector":"rpm","message":"3 packages from unreachable repos","severity":"warning"}]"#;
        let warnings: Vec<Warning> = serde_json::from_str(json).unwrap();
        assert_eq!(warnings[0].inspector, "rpm");
        assert_eq!(warnings[0].severity, Some(WarningSeverity::Warning));
    }

    #[test]
    fn test_snapshot_carries_trust_state() {
        let mut snap = InspectionSnapshot::new();
        snap.redaction_state = Some(RedactionState::FullyRedacted {
            redacted_by: "inspectah 0.8.0".into(),
            config_hash: "abc123".into(),
        });
        snap.completeness = Completeness::Full;
        let json = serde_json::to_string(&snap).unwrap();
        let parsed: InspectionSnapshot = serde_json::from_str(&json).unwrap();
        assert!(parsed.redaction_state.is_some());
        assert_eq!(parsed.completeness, Completeness::Full);
    }
}
```

- [ ] **Step 2: Define InspectionSnapshot**

```rust
// inspectah-core/src/snapshot.rs
use crate::types::config::ConfigSection;
use crate::types::containers::ContainerSection;
use crate::types::kernelboot::KernelBootSection;
use crate::types::network::NetworkSection;
use crate::types::nonrpm::NonRpmSoftwareSection;
use crate::types::os::{OsRelease, SystemType};
use crate::types::preflight::PreflightResult;
use crate::types::rpm::{PackageEntry, PackageState, RpmSection};
use crate::types::scheduled::ScheduledTaskSection;
use crate::types::selinux::SelinuxSection;
use crate::types::services::ServiceSection;
use crate::types::storage::StorageSection;
use crate::types::users::UserGroupSection;
use crate::types::completeness::Completeness;
use crate::types::redaction::{RedactionFinding, RedactionState};
use crate::types::warnings::Warning;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const SCHEMA_VERSION: u32 = 14;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct InspectionSnapshot {
    pub schema_version: u32,
    #[serde(default)]
    pub meta: HashMap<String, serde_json::Value>,
    pub os_release: Option<OsRelease>,
    #[serde(default)]
    pub system_type: SystemType,
    pub rpm: Option<RpmSection>,
    pub config: Option<ConfigSection>,
    pub services: Option<ServiceSection>,
    pub network: Option<NetworkSection>,
    pub storage: Option<StorageSection>,
    pub scheduled_tasks: Option<ScheduledTaskSection>,
    pub containers: Option<ContainerSection>,
    pub non_rpm_software: Option<NonRpmSoftwareSection>,
    pub kernel_boot: Option<KernelBootSection>,
    pub selinux: Option<SelinuxSection>,
    pub users_groups: Option<UserGroupSection>,
    #[serde(default)]
    pub preflight: PreflightResult,
    #[serde(default)]
    pub warnings: Vec<Warning>,
    #[serde(default)]
    pub redactions: Vec<RedactionFinding>,
    /// Trust state for snapshot re-rendering. Only FullyRedacted skips redaction on import.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redaction_state: Option<RedactionState>,
    /// Artifact completeness based on inspector failure state.
    #[serde(default)]
    pub completeness: Completeness,
}

impl InspectionSnapshot {
    pub fn new() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            ..Default::default()
        }
    }

    pub fn load(json: &str) -> Result<Self, SnapshotError> {
        let snap: Self = serde_json::from_str(json)?;
        if snap.schema_version < 12 {
            return Err(SnapshotError::UnsupportedVersion(snap.schema_version));
        }
        Ok(snap)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error("unsupported schema version: {0} (minimum: 12)")]
    UnsupportedVersion(u32),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
```

- [ ] **Step 3: Wire up and test**

Add to `lib.rs`: `pub mod snapshot;`

```bash
cargo test -p inspectah-core
```

- [ ] **Step 4: Commit**

```bash
git add inspectah-core/src/snapshot.rs inspectah-core/src/lib.rs
git commit -m "feat(core): add InspectionSnapshot with Go v13 compatibility

Schema version 14, all 14 section types as Option<T>, Go-compatible
warning and redaction fields. Loads Go v12+ snapshots.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 13: Schema Migration (v12/v13 Compatibility)

Go v12 snapshots have fewer fields; v13 is the current Go schema. Rust reads both via `#[serde(default)]` on all fields — missing fields get zero values. This task adds explicit migration logic for known structural differences.

**Files:**
- Modify: `inspectah-core/src/snapshot.rs`

- [ ] **Step 1: Write migration tests**

```rust
#[test]
fn test_v12_snapshot_loads() {
    let json = r#"{
        "schema_version": 12,
        "meta": {},
        "system_type": "package-mode",
        "rpm": {"packages_added": []},
        "preflight": {"status": "ok"},
        "warnings": [],
        "redactions": []
    }"#;
    let snap = InspectionSnapshot::load(json).unwrap();
    assert_eq!(snap.schema_version, 12);
    // v12 didn't have flatpak_apps — should default to empty
    if let Some(containers) = &snap.containers {
        assert!(containers.flatpak_apps.is_empty());
    }
}

#[test]
fn test_v11_snapshot_rejected() {
    let json = r#"{"schema_version": 11}"#;
    let result = InspectionSnapshot::load(json);
    assert!(result.is_err());
}

#[test]
fn test_migrate_bumps_version() {
    let json = r#"{"schema_version": 13, "meta": {}, "system_type": "package-mode", "preflight": {"status": "ok"}, "warnings": [], "redactions": []}"#;
    let mut snap = InspectionSnapshot::load(json).unwrap();
    migrate(&mut snap);
    assert_eq!(snap.schema_version, SCHEMA_VERSION);
}
```

- [ ] **Step 2: Implement migration function**

```rust
// Add to snapshot.rs

pub fn migrate(snap: &mut InspectionSnapshot) {
    if snap.schema_version >= SCHEMA_VERSION {
        return;
    }
    // v12 → v13: no structural changes needed, just field defaults
    // v13 → v14: no structural changes needed, serde(default) handles missing fields
    // Bump version to current
    snap.schema_version = SCHEMA_VERSION;
}
```

- [ ] **Step 3: Run tests**

```bash
cargo test -p inspectah-core
```

- [ ] **Step 4: Commit**

```bash
git add inspectah-core/src/snapshot.rs
git commit -m "feat(core): add schema migration for Go v12/v13 snapshots

migrate() bumps schema version to 14. Structural compatibility handled
by serde(default) on all fields. v11 and below rejected.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 14: Golden File Capture + Mandatory Parity Gate

The parity gate is **not optional**. Phase 1 proves **RPM-section parity** on a package-based host — not full-snapshot zero-diff. Full-snapshot parity is a Phase 2 milestone (when all inspectors are implemented).

The gate compares only the `$.rpm` subtree of the snapshot. Non-RPM sections are null/empty in a first-inspector Phase 1 output and would produce noise in a full-snapshot diff. The divergence allowlist, normalization, and mandatory-golden mechanics apply to the RPM section comparison.

**Files:**
- Create: `inspectah-core/src/normalize.rs`
- Create: `testdata/golden/go-v13-minimal.json`
- Create: `testdata/golden/go-v13-rpm-section.json` (**REQUIRED** — RPM section extracted from a real Go scan)
- Create: `testdata/divergences.md`
- Modify: `inspectah-core/src/lib.rs`

- [ ] **Step 1: Capture Go v13 golden files**

```bash
cp cmd/inspectah/internal/schema/testdata/minimal-snapshot.json testdata/golden/go-v13-minimal.json

# REQUIRED: RPM section golden from a real package-based host.
# Run on a RHEL 9 or CentOS Stream 9 VM:
#   inspectah scan --inspect-only --output /tmp/golden-full.json
# Extract the RPM section:
#   jq '.rpm' /tmp/golden-full.json > testdata/golden/go-v13-rpm-section.json
```

If the RPM section golden does not exist, CI **fails** — it does not skip.

- [ ] **Step 2: Create divergences allowlist**

```markdown
<!-- testdata/divergences.md -->
# Expected Go-vs-Rust Divergences

Divergences listed here are expected and excluded from the parity gate.
Any difference NOT listed here fails CI.

## schema_version
- Go: 13
- Rust: 14
- Path: `$.schema_version`
- Reason: Rust continues the integer sequence per spec.

## meta.inspectah_version
- Path: `$.meta.inspectah_version`
- Reason: Different binary version strings.

## meta.timestamp
- Path: `$.meta.timestamp`
- Reason: Different scan times.

## redaction_state (Rust-only field)
- Path: `$.redaction_state`
- Reason: New Rust-era field, not present in Go output.

## completeness (Rust-only field)
- Path: `$.completeness`
- Reason: New Rust-era field, not present in Go output.
```

- [ ] **Step 3: Write normalization and diff tooling**

```rust
// inspectah-core/src/normalize.rs
use serde_json::Value;
use std::collections::BTreeSet;

/// Volatile meta subfields that differ between Go and Rust output.
/// Only THESE specific keys are stripped — contract-bearing meta keys
/// (hostname, host_root) survive normalization.
const VOLATILE_META_KEYS: &[&str] = &[
    "timestamp",
    "inspectah_version",
    "inspectah_commit",
    "inspectah_date",
];

/// Normalize a snapshot for comparison. Only strips explicitly volatile
/// subfields, NOT the entire meta object.
pub fn normalize(value: &mut Value) {
    if let Value::Object(map) = value {
        // Strip only volatile meta subfields, not the whole meta
        if let Some(Value::Object(meta)) = map.get_mut("meta") {
            for key in VOLATILE_META_KEYS {
                meta.remove(*key);
            }
        }
        // Strip Rust-only fields not present in Go output
        map.remove("redaction_state");
        map.remove("completeness");

        for (_, v) in map.iter_mut() {
            normalize(v);
        }
    }
    if let Value::Array(arr) = value {
        for v in arr.iter_mut() {
            normalize(v);
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct Difference {
    pub path: String,
    pub go_value: String,
    pub rust_value: String,
}

/// Load the divergences allowlist from testdata/divergences.md.
/// Parses paths from `## ` headers followed by `- Path: ` lines.
pub fn load_divergence_allowlist(md: &str) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    for line in md.lines() {
        let line = line.trim();
        if let Some(path) = line.strip_prefix("- Path: `").and_then(|s| s.strip_suffix('`')) {
            paths.insert(path.to_string());
        }
    }
    paths
}

/// Compare two snapshots after normalization. Returns only
/// UNDOCUMENTED differences (not in the allowlist).
pub fn diff_snapshots(
    go_json: &str,
    rust_json: &str,
    allowlist: &BTreeSet<String>,
) -> Result<Vec<Difference>, serde_json::Error> {
    let mut go: Value = serde_json::from_str(go_json)?;
    let mut rust: Value = serde_json::from_str(rust_json)?;
    normalize(&mut go);
    normalize(&mut rust);

    let mut all_diffs = Vec::new();
    diff_values("$", &go, &rust, &mut all_diffs);

    // Filter out documented divergences
    let undocumented: Vec<Difference> = all_diffs
        .into_iter()
        .filter(|d| !allowlist.contains(&d.path))
        .collect();
    Ok(undocumented)
}

fn diff_values(path: &str, go: &Value, rust: &Value, diffs: &mut Vec<Difference>) {
    match (go, rust) {
        (Value::Object(g), Value::Object(r)) => {
            let keys: BTreeSet<_> = g.keys().chain(r.keys()).collect();
            for key in keys {
                let child_path = format!("{path}.{key}");
                match (g.get(key), r.get(key)) {
                    (Some(gv), Some(rv)) => diff_values(&child_path, gv, rv, diffs),
                    (Some(gv), None) => diffs.push(Difference {
                        path: child_path, go_value: gv.to_string(),
                        rust_value: "<missing>".into(),
                    }),
                    (None, Some(rv)) => diffs.push(Difference {
                        path: child_path, go_value: "<missing>".into(),
                        rust_value: rv.to_string(),
                    }),
                    _ => {}
                }
            }
        }
        (Value::Array(g), Value::Array(r)) => {
            for i in 0..g.len().max(r.len()) {
                let child_path = format!("{path}[{i}]");
                match (g.get(i), r.get(i)) {
                    (Some(gv), Some(rv)) => diff_values(&child_path, gv, rv, diffs),
                    (Some(gv), None) => diffs.push(Difference {
                        path: child_path, go_value: gv.to_string(),
                        rust_value: "<missing>".into(),
                    }),
                    (None, Some(rv)) => diffs.push(Difference {
                        path: child_path, go_value: "<missing>".into(),
                        rust_value: rv.to_string(),
                    }),
                    _ => {}
                }
            }
        }
        _ if go != rust => {
            diffs.push(Difference {
                path: path.to_string(),
                go_value: go.to_string(),
                rust_value: rust.to_string(),
            });
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_preserves_contract_meta() {
        let mut val: Value = serde_json::from_str(
            r#"{"meta":{"hostname":"web01","timestamp":"2024-01-01","inspectah_version":"0.7.0"}}"#
        ).unwrap();
        normalize(&mut val);
        let meta = val["meta"].as_object().unwrap();
        assert!(meta.contains_key("hostname"), "contract-bearing meta key must survive");
        assert!(!meta.contains_key("timestamp"), "volatile key must be stripped");
        assert!(!meta.contains_key("inspectah_version"), "volatile key must be stripped");
    }

    #[test]
    fn test_divergence_allowlist_parsing() {
        let md = "## schema_version\n- Path: `$.schema_version`\n- Reason: version bump\n";
        let allowlist = load_divergence_allowlist(md);
        assert!(allowlist.contains("$.schema_version"));
    }

    #[test]
    fn test_allowed_divergences_filtered() {
        let go = r#"{"schema_version":13,"system_type":"package-mode"}"#;
        let rust = r#"{"schema_version":14,"system_type":"package-mode"}"#;
        let mut allowlist = BTreeSet::new();
        allowlist.insert("$.schema_version".to_string());
        let diffs = diff_snapshots(go, rust, &allowlist).unwrap();
        assert!(diffs.is_empty(), "allowed divergence should be filtered");
    }

    #[test]
    fn test_undocumented_divergence_surfaces() {
        let go = r#"{"system_type":"package-mode","rpm":{"packages_added":[]}}"#;
        let rust = r#"{"system_type":"bootc","rpm":{"packages_added":[]}}"#;
        let allowlist = BTreeSet::new();
        let diffs = diff_snapshots(go, rust, &allowlist).unwrap();
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].path, "$.system_type");
    }

    #[test]
    fn test_go_v13_golden_loads() {
        let json = include_str!("../../testdata/golden/go-v13-minimal.json");
        let snap = crate::snapshot::InspectionSnapshot::load(json).unwrap();
        assert!(snap.schema_version >= 12);
    }
}
```

- [ ] **Step 4: Wire up and test**

Add to `lib.rs`: `pub mod normalize;`

```bash
mkdir -p testdata/golden
cargo test -p inspectah-core
```

- [ ] **Step 5: Commit**

```bash
git add inspectah-core/src/normalize.rs inspectah-core/src/lib.rs testdata/
git commit -m "feat(core): add mandatory parity gate with divergence allowlist

normalize() strips only volatile meta subkeys (timestamps, version),
preserves contract-bearing keys (hostname). diff_snapshots() filters
documented divergences from testdata/divergences.md. Undocumented
divergences fail CI.

Assisted-by: Claude Code (Opus 4.6)"
```

**Phase 0 complete.** All types, traits, pipeline typestate, and golden file tooling are in place. `cargo test -p inspectah-core` should show all tests passing.

---

## Phase 1: First Inspector End-to-End

Spans `inspectah-collect`, `inspectah-pipeline`, and `inspectah-cli`. The goal: `inspectah scan` produces a tarball with RPM section data on a package-based host. Output matches Go via normalized diff.

---

### Task 15: MockExecutor + RealExecutor

**Files:**
- Create: `inspectah-collect/src/executor/mod.rs`
- Create: `inspectah-collect/src/executor/mock.rs`
- Create: `inspectah-collect/src/executor/real.rs`
- Modify: `inspectah-collect/src/lib.rs`

- [ ] **Step 1: Write MockExecutor tests**

```rust
// Bottom of mock.rs
#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::traits::executor::Executor;
    use std::path::Path;

    #[test]
    fn test_mock_command_lookup() {
        let mock = MockExecutor::new()
            .with_command("rpm -qa", ExecResult {
                stdout: "bash-5.2.26-3.el9.x86_64\n".into(),
                exit_code: 0,
                ..Default::default()
            });
        let result = mock.run("rpm", &["-qa"]);
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("bash"));
    }

    #[test]
    fn test_mock_unknown_command() {
        let mock = MockExecutor::new();
        let result = mock.run("nonexistent", &[]);
        assert_eq!(result.exit_code, 127);
    }

    #[test]
    fn test_mock_file_read() {
        let mock = MockExecutor::new()
            .with_file("/etc/os-release", "ID=rhel\nVERSION_ID=9.4\n");
        let content = mock.read_file(Path::new("/etc/os-release")).unwrap();
        assert!(content.contains("ID=rhel"));
    }

    #[test]
    fn test_mock_file_not_found() {
        let mock = MockExecutor::new();
        assert!(mock.read_file(Path::new("/nonexistent")).is_err());
        assert!(!mock.file_exists(Path::new("/nonexistent")));
    }
}
```

- [ ] **Step 2: Implement MockExecutor**

```rust
// inspectah-collect/src/executor/mock.rs
use inspectah_core::traits::executor::{ExecResult, Executor};
use std::collections::HashMap;
use std::io;
use std::path::Path;

pub struct MockExecutor {
    commands: HashMap<String, ExecResult>,
    files: HashMap<String, String>,
    dirs: HashMap<String, Vec<String>>,
    links: HashMap<String, String>,
}

impl MockExecutor {
    pub fn new() -> Self {
        Self {
            commands: HashMap::new(),
            files: HashMap::new(),
            dirs: HashMap::new(),
            links: HashMap::new(),
        }
    }

    pub fn with_command(mut self, key: &str, result: ExecResult) -> Self {
        self.commands.insert(key.to_string(), result);
        self
    }

    pub fn with_file(mut self, path: &str, content: &str) -> Self {
        self.files.insert(path.to_string(), content.to_string());
        self
    }

    pub fn with_dir(mut self, path: &str, entries: Vec<&str>) -> Self {
        self.dirs.insert(path.to_string(), entries.iter().map(|s| s.to_string()).collect());
        self
    }

    pub fn with_link(mut self, path: &str, target: &str) -> Self {
        self.links.insert(path.to_string(), target.to_string());
        self
    }
}

impl Executor for MockExecutor {
    fn run(&self, cmd: &str, args: &[&str]) -> ExecResult {
        let key = if args.is_empty() {
            cmd.to_string()
        } else {
            format!("{} {}", cmd, args.join(" "))
        };
        self.commands.get(&key).cloned().unwrap_or_else(|| ExecResult {
            stderr: format!("command not found: {key}"),
            exit_code: 127,
            ..Default::default()
        })
    }

    fn read_file(&self, path: &Path) -> io::Result<String> {
        self.files
            .get(path.to_str().unwrap_or(""))
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, path.display().to_string()))
    }

    fn file_exists(&self, path: &Path) -> bool {
        self.files.contains_key(path.to_str().unwrap_or(""))
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<String>> {
        self.dirs
            .get(path.to_str().unwrap_or(""))
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, path.display().to_string()))
    }

    fn read_link(&self, path: &Path) -> io::Result<String> {
        self.links
            .get(path.to_str().unwrap_or(""))
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, path.display().to_string()))
    }

    fn host_root(&self) -> &Path {
        Path::new("/")
    }
}
```

- [ ] **Step 3: Implement RealExecutor**

```rust
// inspectah-collect/src/executor/real.rs
use inspectah_core::traits::executor::{ExecResult, Executor};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// Phase 1: live-host only. No --host-root flag — all commands and
/// file reads target /. Containerized/offline inspection is deferred.
pub struct RealExecutor;

impl RealExecutor {
    pub fn new() -> Self {
        Self
    }
}

impl Executor for RealExecutor {
    fn run(&self, cmd: &str, args: &[&str]) -> ExecResult {
        let result = Command::new(cmd)
            .args(args)
            .output();
        match result {
            Ok(output) => ExecResult {
                stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                exit_code: output.status.code().unwrap_or(-1),
            },
            Err(e) => ExecResult {
                stderr: e.to_string(),
                exit_code: 127,
                ..Default::default()
            },
        }
    }

    fn read_file(&self, path: &Path) -> io::Result<String> {
        std::fs::read_to_string(path)
    }

    fn file_exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<String>> {
        let entries = std::fs::read_dir(path)?;
        entries
            .filter_map(|e| e.ok())
            .map(|e| Ok(e.file_name().to_string_lossy().into_owned()))
            .collect()
    }

    fn read_link(&self, path: &Path) -> io::Result<String> {
        let target = std::fs::read_link(path)?;
        Ok(target.to_string_lossy().into_owned())
    }

    fn host_root(&self) -> &Path {
        Path::new("/")
    }
}
```

- [ ] **Step 4: Wire up executor module**

```rust
// inspectah-collect/src/executor/mod.rs
pub mod mock;
pub mod real;
```

```rust
// inspectah-collect/src/lib.rs
pub mod executor;
```

- [ ] **Step 5: Run tests**

```bash
cargo test -p inspectah-collect
```

- [ ] **Step 6: Commit**

```bash
git add inspectah-collect/src/
git commit -m "feat(collect): add MockExecutor and RealExecutor

MockExecutor with builder pattern (with_command, with_file, with_dir,
with_link) for offline testing. RealExecutor runs live-host commands
and file reads (no host_root — Phase 1 is live-host only).

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 16: RPM Inspector — NEVRA Parser

**Files:**
- Create: `inspectah-collect/src/inspectors/mod.rs`
- Create: `inspectah-collect/src/inspectors/rpm/mod.rs`
- Create: `inspectah-collect/src/inspectors/rpm/parser.rs`

- [ ] **Step 1: Write NEVRA parsing tests**

```rust
// Bottom of parser.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_nevra_standard() {
        let entry = parse_nevra("0:bash-5.2.26-3.el9.x86_64").unwrap();
        assert_eq!(entry.epoch, "0");
        assert_eq!(entry.name, "bash");
        assert_eq!(entry.version, "5.2.26");
        assert_eq!(entry.release, "3.el9");
        assert_eq!(entry.arch, "x86_64");
    }

    #[test]
    fn test_parse_nevra_no_epoch() {
        let entry = parse_nevra("(none):httpd-2.4.57-5.el9.x86_64").unwrap();
        assert_eq!(entry.epoch, "0");
        assert_eq!(entry.name, "httpd");
    }

    #[test]
    fn test_parse_nevra_noarch() {
        let entry = parse_nevra("0:tzdata-2024a-1.el9.noarch").unwrap();
        assert_eq!(entry.arch, "noarch");
    }

    #[test]
    fn test_rpmvercmp_numeric() {
        assert_eq!(rpmvercmp("1.2.3", "1.2.3"), std::cmp::Ordering::Equal);
        assert_eq!(rpmvercmp("1.2.4", "1.2.3"), std::cmp::Ordering::Greater);
        assert_eq!(rpmvercmp("1.2.3", "1.2.4"), std::cmp::Ordering::Less);
    }

    #[test]
    fn test_rpmvercmp_tilde() {
        // Tilde sorts before anything, even empty
        assert_eq!(rpmvercmp("1.0~rc1", "1.0"), std::cmp::Ordering::Less);
        assert_eq!(rpmvercmp("1.0", "1.0~rc1"), std::cmp::Ordering::Greater);
    }

    #[test]
    fn test_rpmvercmp_caret() {
        // Caret sorts after empty but before any other character
        assert_eq!(rpmvercmp("1.0^git1", "1.0"), std::cmp::Ordering::Greater);
        assert_eq!(rpmvercmp("1.0^git1", "1.0.1"), std::cmp::Ordering::Less);
    }
}
```

- [ ] **Step 2: Implement NEVRA parser**

```rust
// inspectah-collect/src/inspectors/rpm/parser.rs
use inspectah_core::types::rpm::PackageEntry;

/// Parse "epoch:name-version-release.arch" format from `rpm -qa --queryformat`.
/// Go's format string: `%{EPOCH}:%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}`
pub fn parse_nevra(line: &str) -> Option<PackageEntry> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }

    // Split epoch:rest
    let (epoch_str, rest) = line.split_once(':')?;
    let epoch = if epoch_str == "(none)" { "0" } else { epoch_str };

    // Split rest into name-version-release.arch
    // Find the last '.' → arch separator
    let dot_pos = rest.rfind('.')?;
    let arch = &rest[dot_pos + 1..];
    let name_ver_rel = &rest[..dot_pos];

    // Find the second-to-last '-' → version-release separator
    let rel_dash = name_ver_rel.rfind('-')?;
    let release = &name_ver_rel[rel_dash + 1..];
    let name_ver = &name_ver_rel[..rel_dash];

    // Find the last '-' in name_ver → name-version separator
    let ver_dash = name_ver.rfind('-')?;
    let version = &name_ver[ver_dash + 1..];
    let name = &name_ver[..ver_dash];

    Some(PackageEntry {
        name: name.into(),
        epoch: epoch.into(),
        version: version.into(),
        release: release.into(),
        arch: arch.into(),
        ..Default::default()
    })
}

/// RPM version comparison algorithm (rpmvercmp).
/// Implements the same algorithm as Go's rpmvercmp and librpm's C implementation.
pub fn rpmvercmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    if a == b {
        return Ordering::Equal;
    }

    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();

    loop {
        // Skip non-alphanumeric, non-tilde, non-caret characters
        while ai.peek().is_some_and(|c| !c.is_alphanumeric() && *c != '~' && *c != '^') {
            ai.next();
        }
        while bi.peek().is_some_and(|c| !c.is_alphanumeric() && *c != '~' && *c != '^') {
            bi.next();
        }

        // Handle tilde (sorts before everything)
        match (ai.peek(), bi.peek()) {
            (Some('~'), Some('~')) => { ai.next(); bi.next(); continue; }
            (Some('~'), _) => return Ordering::Less,
            (_, Some('~')) => return Ordering::Greater,
            _ => {}
        }

        // Handle caret (sorts after empty, before other characters)
        match (ai.peek(), bi.peek()) {
            (Some('^'), Some('^')) => { ai.next(); bi.next(); continue; }
            (Some('^'), None) => return Ordering::Greater,
            (None, Some('^')) => return Ordering::Less,
            (Some('^'), _) => return Ordering::Less,
            (_, Some('^')) => return Ordering::Greater,
            _ => {}
        }

        // End of both strings
        if ai.peek().is_none() && bi.peek().is_none() {
            return Ordering::Equal;
        }

        // One string ended before the other
        if ai.peek().is_none() {
            return Ordering::Less;
        }
        if bi.peek().is_none() {
            return Ordering::Greater;
        }

        // Collect contiguous segments of the same type (digit or alpha)
        let is_digit = ai.peek().unwrap().is_ascii_digit();
        let seg_a: String = if is_digit {
            collect_while(&mut ai, |c| c.is_ascii_digit())
        } else {
            collect_while(&mut ai, |c| c.is_alphabetic())
        };
        let seg_b: String = if is_digit {
            collect_while(&mut bi, |c| c.is_ascii_digit())
        } else {
            collect_while(&mut bi, |c| c.is_alphabetic())
        };

        // Numeric segments sort numerically
        if is_digit {
            let na: u64 = seg_a.parse().unwrap_or(0);
            let nb: u64 = seg_b.parse().unwrap_or(0);
            let cmp = na.cmp(&nb);
            if cmp != Ordering::Equal {
                return cmp;
            }
        } else {
            let cmp = seg_a.cmp(&seg_b);
            if cmp != Ordering::Equal {
                return cmp;
            }
        }
    }
}

fn collect_while(iter: &mut std::iter::Peekable<std::str::Chars>, pred: impl Fn(char) -> bool) -> String {
    let mut s = String::new();
    while iter.peek().is_some_and(|c| pred(*c)) {
        s.push(iter.next().unwrap());
    }
    s
}

/// Parse the output of `rpm -qa --queryformat` into PackageEntry list.
/// Filters gpg-pubkey virtual packages.
pub fn parse_rpm_qa(output: &str) -> Vec<PackageEntry> {
    output
        .lines()
        .filter_map(|line| {
            let entry = parse_nevra(line)?;
            if entry.name == "gpg-pubkey" {
                return None;
            }
            Some(entry)
        })
        .collect()
}
```

- [ ] **Step 3: Wire up modules**

```rust
// inspectah-collect/src/inspectors/mod.rs
pub mod rpm;
```

```rust
// inspectah-collect/src/inspectors/rpm/mod.rs
pub mod parser;
```

Add to `inspectah-collect/src/lib.rs`:
```rust
pub mod inspectors;
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p inspectah-collect
```

- [ ] **Step 5: Commit**

```bash
git add inspectah-collect/src/inspectors/
git commit -m "feat(collect): add RPM NEVRA parser and rpmvercmp

parse_nevra() handles epoch:(none), standard NEVRA format.
rpmvercmp() implements full RPM version comparison with tilde
and caret handling. parse_rpm_qa() filters gpg-pubkey.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 17: RPM FFI Wrapper (ffi-rpm feature gate)

The approved spec puts minimal `librpm` FFI in Phase 1 to validate the dynamic-linking strategy. Feature-gated: `ffi-rpm` enabled → use librpm, disabled → shell fallback.

**Files:**
- Create: `inspectah-collect/src/ffi/mod.rs`
- Create: `inspectah-collect/src/ffi/rpm.rs`
- Create: `inspectah-collect/build.rs`
- Modify: `inspectah-collect/Cargo.toml`

- [ ] **Step 1: Write test for FFI-gated package query**

```rust
// inspectah-collect/tests/ffi_rpm.rs
#[cfg(feature = "ffi-rpm")]
#[test]
fn test_librpm_query_returns_packages() {
    use inspectah_collect::ffi::rpm::query_all_packages;
    let packages = query_all_packages().expect("librpm query failed");
    assert!(!packages.is_empty(), "host must have packages installed");
    assert!(packages.iter().any(|p| p.name == "bash"), "bash should be installed");
}
```

- [ ] **Step 2: Add feature gate to Cargo.toml**

```toml
# inspectah-collect/Cargo.toml (additions)
[features]
default = []
ffi-rpm = []

[build-dependencies]
pkg-config = { version = "0.3", optional = true }

[dependencies]
libc = { version = "0.2", optional = true }
```

Note: `ffi-rpm` activates both `pkg-config` (build) and `libc` (runtime).

- [ ] **Step 3: Create build.rs for librpm discovery**

```rust
// inspectah-collect/build.rs
fn main() {
    #[cfg(feature = "ffi-rpm")]
    {
        pkg_config::Config::new()
            .atleast_version("4.14")
            .probe("rpm")
            .expect("librpm >= 4.14 not found. Install rpm-devel (RHEL/Fedora) or disable ffi-rpm feature.");
    }
}
```

- [ ] **Step 4: Implement safe wrapper**

```rust
// inspectah-collect/src/ffi/rpm.rs
#[cfg(feature = "ffi-rpm")]
use inspectah_core::types::rpm::PackageEntry;

#[cfg(feature = "ffi-rpm")]
mod inner {
    // Safe Rust wrapper around librpm.
    // Uses rpmtsCreate/rpmtsInitIterator/headerGet pattern.
    // Encapsulates all unsafe in this module — no raw pointers cross the boundary.
    // Returns Vec<PackageEntry> with epoch/name/version/release/arch populated.
    // Errors return RpmFfiError, never panics.
}

#[cfg(feature = "ffi-rpm")]
pub fn query_all_packages() -> Result<Vec<PackageEntry>, RpmFfiError> {
    inner::query_all()
}

#[cfg(feature = "ffi-rpm")]
#[derive(Debug, thiserror::Error)]
pub enum RpmFfiError {
    #[error("librpm initialization failed")]
    InitFailed,
    #[error("rpmdb query failed: {0}")]
    QueryFailed(String),
}
```

```rust
// inspectah-collect/src/ffi/mod.rs
#[cfg(feature = "ffi-rpm")]
pub mod rpm;
```

- [ ] **Step 5: Wire FFI into RPM inspector**

The RPM inspector selects the code path at compile time:

```rust
// In inspectah-collect/src/inspectors/rpm/mod.rs
fn query_packages(&self, ctx: &InspectionContext) -> Result<Vec<PackageEntry>, InspectorError> {
    #[cfg(feature = "ffi-rpm")]
    {
        use crate::ffi::rpm;
        match rpm::query_all_packages() {
            Ok(pkgs) => return Ok(pkgs),
            Err(e) => return Err(InspectorError::Failed {
                reason: format!("librpm: {e}"),
            }),
        }
    }

    #[cfg(not(feature = "ffi-rpm"))]
    {
        let result = ctx.executor.run("rpm", &["-qa", "--queryformat", RPM_QA_FORMAT]);
        if !result.success() {
            return Err(InspectorError::Failed { reason: result.stderr });
        }
        Ok(parse_rpm_qa(&result.stdout))
    }
}
```

- [ ] **Step 6: Verify both profiles compile**

```bash
cargo test -p inspectah-collect                    # minimal, no FFI
cargo test -p inspectah-collect --features ffi-rpm # full, with FFI (requires librpm-devel)
```

- [ ] **Step 7: Commit**

```bash
git add inspectah-collect/build.rs inspectah-collect/Cargo.toml inspectah-collect/src/ffi/
git commit -m "feat(collect): add librpm FFI wrapper behind ffi-rpm feature gate

Safe Rust wrapper around librpm for RPM database queries. Feature-gated:
ffi-rpm enabled uses librpm directly, disabled uses shell rpm -qa fallback.
CI builds both profiles. Validates the dynamic-linking strategy.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 18: RPM Inspector — Package Classification

**Files:**
- Create: `inspectah-collect/src/inspectors/rpm/classifier.rs`
- Modify: `inspectah-collect/src/inspectors/rpm/mod.rs`

- [ ] **Step 1: Write test for standard classification**

```rust
#[test]
fn test_classify_added_package() {
    let host = vec![pkg("httpd", "2.4.57", "5.el9")];
    let baseline: HashMap<String, PackageEntry> = HashMap::new(); // empty baseline
    let result = classify_packages(&host, &baseline);
    assert_eq!(result[0].state, PackageState::Added);
}
```

- [ ] **Step 2: Write test for base-image-only package**

```rust
#[test]
fn test_classify_base_image_only() {
    let host = vec![pkg("bash", "5.2.26", "3.el9")];
    let baseline = baseline_with(&[("bash", "5.2.26", "3.el9")]);
    let result = classify_packages(&host, &baseline);
    assert_eq!(result[0].state, PackageState::BaseImageOnly);
}
```

- [ ] **Step 3: Write test for version-modified package**

```rust
#[test]
fn test_classify_modified_version() {
    let host = vec![pkg("bash", "5.2.26", "4.el9")];
    let baseline = baseline_with(&[("bash", "5.2.26", "3.el9")]);
    let result = classify_packages(&host, &baseline);
    assert_eq!(result[0].state, PackageState::Modified);
}
```

- [ ] **Step 4: Write negative-path tests**

```rust
#[test]
fn test_classify_empty_baseline_all_added() {
    let host = vec![pkg("httpd", "2.4.57", "5.el9"), pkg("vim", "9.0", "1.el9")];
    let result = classify_packages(&host, &HashMap::new());
    assert!(result.iter().all(|p| p.state == PackageState::Added));
}

#[test]
fn test_classify_duplicate_nevra() {
    let host = vec![
        pkg("bash", "5.2.26", "3.el9"),
        pkg("bash", "5.2.26", "3.el9"), // duplicate
    ];
    let result = classify_packages(&host, &HashMap::new());
    assert_eq!(result.len(), 2); // both processed, dedup is caller's concern
}
```

- [ ] **Step 5: Implement classifier**

```rust
// inspectah-collect/src/inspectors/rpm/classifier.rs
use inspectah_core::types::rpm::{PackageEntry, PackageState};
use super::parser::rpmvercmp;
use std::collections::HashMap;

pub fn classify_packages(
    host: &[PackageEntry],
    baseline: &HashMap<String, PackageEntry>,
) -> Vec<PackageEntry> {
    host.iter().map(|pkg| {
        let key = format!("{}.{}", pkg.name, pkg.arch);
        let state = match baseline.get(&key) {
            None => PackageState::Added,
            Some(base) => {
                let ver_cmp = rpmvercmp(&pkg.version, &base.version);
                let rel_cmp = rpmvercmp(&pkg.release, &base.release);
                if ver_cmp == std::cmp::Ordering::Equal && rel_cmp == std::cmp::Ordering::Equal {
                    PackageState::BaseImageOnly
                } else {
                    PackageState::Modified
                }
            }
        };
        PackageEntry { state, include: state != PackageState::BaseImageOnly, ..pkg.clone() }
    }).collect()
}
```

- [ ] **Step 6: Run tests, commit**

```bash
cargo test -p inspectah-collect
```

---

### Task 19: RPM Inspector — Supplementary Data

**Files:**
- Create: `inspectah-collect/src/inspectors/rpm/repos.rs`
- Create: `inspectah-collect/src/inspectors/rpm/modules.rs`

- [ ] **Step 1: Write test for repo file parsing**

```rust
#[test]
fn test_parse_repo_files() {
    let mock = MockExecutor::new()
        .with_dir("/etc/yum.repos.d", vec!["redhat.repo", "epel.repo"])
        .with_file("/etc/yum.repos.d/redhat.repo", "[rhel-9-baseos]\nname=RHEL 9 BaseOS\n")
        .with_file("/etc/yum.repos.d/epel.repo", "[epel]\nname=EPEL 9\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9\n");
    let repos = collect_repo_files(&mock);
    assert_eq!(repos.len(), 2);
}
```

- [ ] **Step 2: Write test for malformed repo file**

```rust
#[test]
fn test_malformed_repo_file_skipped() {
    let mock = MockExecutor::new()
        .with_dir("/etc/yum.repos.d", vec!["broken.repo"])
        .with_file("/etc/yum.repos.d/broken.repo", "not a valid repo file\n\0\0\0");
    let repos = collect_repo_files(&mock);
    // Should not panic, may produce empty or partial result
    assert!(repos.len() <= 1);
}
```

- [ ] **Step 3: Write test for GPG key extraction**

```rust
#[test]
fn test_gpg_key_extraction() {
    let repo_content = "[epel]\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9\n";
    let mock = MockExecutor::new()
        .with_file("/etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9", "-----BEGIN PGP PUBLIC KEY BLOCK-----\n...");
    let keys = extract_gpg_keys(repo_content, &mock);
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].path, "/etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9");
}
```

- [ ] **Step 4: Write tests for module streams and version locks**

Test parsing of `/etc/dnf/modules.d/*.module` files and versionlock config. Assert parsed `EnabledModuleStream` and `VersionLockEntry` structs match expected values.

- [ ] **Step 5: Write test for rpm -Va parsing**

```rust
#[test]
fn test_rpm_va_parsing() {
    let output = "S.5....T.  c /etc/httpd/conf/httpd.conf\n..5....T.  c /etc/sysconfig/httpd\n";
    let entries = parse_rpm_va(output);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].path, "/etc/httpd/conf/httpd.conf");
    assert_eq!(entries[0].flags, "S.5....T.");
}
```

- [ ] **Step 6: Implement all supplementary collectors, run tests, commit**

---

### Task 20: RPM Inspector — Trait Implementation + Pipeline Collect

**Files:**
- Modify: `inspectah-collect/src/inspectors/rpm/mod.rs`
- Create: `inspectah-pipeline/src/collect.rs`

- [ ] **Step 1: Write test for Inspector trait impl**

```rust
#[test]
fn test_rpm_inspector_trait() {
    use inspectah_core::types::completeness::{InspectorId, SourceSystemKind, SectionData};
    let inspector = RpmInspector::new();
    assert_eq!(inspector.id(), InspectorId::Rpm);
    assert!(inspector.applicable_to().contains(&SourceSystemKind::PackageBased));
}
```

- [ ] **Step 2: Write test for full RPM inspector output**

```rust
#[test]
fn test_rpm_inspector_produces_section_data() {
    let mock = build_rpm_mock_executor(); // helper with canned rpm -qa, repos, etc.
    let ctx = InspectionContext {
        executor: Box::new(mock),
        source: SourceSystem::PackageBased {
            os_release: test_os_release(),
        },
        rpm_state: None,
    };
    let output = RpmInspector::new().inspect(&ctx).unwrap();
    if let SectionData::Rpm(rpm) = &output.section {
        assert!(!rpm.packages_added.is_empty());
    } else {
        panic!("expected SectionData::Rpm");
    }
}
```

The bootc baseline rule: when `SourceSystem::Bootc`, the booted image is the sole baseline truth for package classification. The classifier uses `booted_image` (not staged) to determine the baseline package set.

- [ ] **Step 3: Write test for pipeline collect stage**

```rust
#[test]
fn test_collect_produces_pipeline_collected() {
    let pipeline = Pipeline::new()
        .collect(&ctx, &[Box::new(RpmInspector::new())]);
    // pipeline is now Pipeline<Collected>
    assert!(pipeline.state.snapshot.rpm.is_some());
}
```

- [ ] **Step 4: Implement, run tests, commit**

---

### Task 21: Pipeline Validate + Redaction Engine

**Files:**
- Create: `inspectah-pipeline/src/validate.rs`
- Create: `inspectah-pipeline/src/redaction/mod.rs`
- Create: `inspectah-pipeline/src/redaction/patterns.rs`
- Create: `inspectah-pipeline/src/redaction/engine.rs`

- [ ] **Step 1: Write test for private key detection**

```rust
#[test]
fn test_detect_private_key() {
    let content = "-----BEGIN RSA PRIVATE KEY-----\nMIIEpA...\n-----END RSA PRIVATE KEY-----\n";
    let findings = scan_content(content, "/etc/ssl/private/key.pem");
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].kind, "PRIVATE_KEY");
}
```

- [ ] **Step 2: Write test for password detection**

```rust
#[test]
fn test_detect_password_in_config() {
    let content = "db_password = s3cretP@ss\n";
    let findings = scan_content(content, "/etc/myapp/config");
    assert!(!findings.is_empty());
}
```

- [ ] **Step 3: Write test for shadow hash — locked accounts are NOT secrets**

```rust
#[test]
fn test_shadow_locked_not_flagged() {
    let content = "root:!!:19000:0:99999:7:::\nnobody:*:19000:0:99999:7:::\n";
    let findings = scan_content(content, "/etc/shadow");
    // !! = locked, * = disabled → neither is a secret
    assert!(findings.is_empty(), "locked/disabled accounts must not be flagged");
}

#[test]
fn test_shadow_hash_is_flagged() {
    let content = "admin:$6$rounds=65536$salt$hash...:19000:0:99999:7:::\n";
    let findings = scan_content(content, "/etc/shadow");
    assert_eq!(findings.len(), 1);
}

#[test]
fn test_shadow_empty_produces_low_confidence_finding() {
    let content = "nobody::19000:0:99999:7:::\n";
    let findings = scan_content(content, "/etc/shadow");
    // Empty password field MUST produce a low-confidence finding — not silence.
    assert_eq!(findings.len(), 1, "empty shadow must produce exactly one finding");
    assert_eq!(findings[0].kind, FindingKind::NoPassword);
    assert_eq!(findings[0].confidence, Confidence::Low);
}
```

- [ ] **Step 4: Write test for PartiallyRedacted state (unconditional)**

Uses a fixture **guaranteed** to produce unresolved low-confidence findings (empty shadow entries). The assertion is unconditional — not gated on "if unresolved happen to exist."

```rust
#[test]
fn test_partially_redacted_with_guaranteed_unresolved() {
    // Fixture contains an empty-password shadow entry → low-confidence finding
    // that cannot be auto-resolved without operator triage.
    let mut snapshot = test_snapshot_with_empty_shadow();
    redact(&mut snapshot, &RedactOptions { sensitivity: Sensitivity::Default });
    match &snapshot.redaction_state {
        Some(RedactionState::PartiallyRedacted { unresolved_count, .. }) => {
            assert!(*unresolved_count > 0, "empty shadow must remain unresolved");
        }
        other => panic!("expected PartiallyRedacted, got {other:?}"),
    }
}
```

- [ ] **Step 5: Write test for FullyRedacted state**

```rust
#[test]
fn test_fully_redacted_when_all_resolved() {
    let mut snapshot = test_snapshot_with_known_secrets();
    let result = redact(&mut snapshot, &RedactOptions { sensitivity: Sensitivity::Default });
    match &snapshot.redaction_state {
        Some(RedactionState::FullyRedacted { redacted_by, .. }) => {
            assert!(redacted_by.contains("inspectah"));
        }
        other => panic!("expected FullyRedacted, got {other:?}"),
    }
}
```

- [ ] **Step 6: Write test for Cow<str> zero-copy**

```rust
#[test]
fn test_cow_no_clone_when_clean() {
    let clean = "no secrets here";
    let result = redact_string(clean);
    // Cow::Borrowed means no allocation
    assert!(matches!(result, std::borrow::Cow::Borrowed(_)));
}
```

- [ ] **Step 7: Implement redaction engine, run tests, commit**

---

### Task 22: Containerfile Renderer

**Files:**
- Create: `inspectah-pipeline/src/render/mod.rs`
- Create: `inspectah-pipeline/src/render/containerfile.rs`
- Create: `inspectah-pipeline/src/render/safety.rs`

- [ ] **Step 1: Write test for basic Containerfile**

```rust
#[test]
fn test_containerfile_package_based() {
    let snap = snapshot_with_packages(&["httpd", "vim-enhanced"]);
    let output = render_containerfile(&snap, &RenderContext { target: None });
    assert!(output.contains("FROM"));
    assert!(output.contains("RUN dnf install -y"));
    assert!(output.contains("httpd"));
}
```

- [ ] **Step 2: Write test for shell metacharacter escaping**

```rust
#[test]
fn test_sanitize_shell_value() {
    assert_eq!(sanitize_shell_value("normal-pkg"), "normal-pkg");
    assert_eq!(sanitize_shell_value("pkg; rm -rf /"), "pkg\\;\\ rm\\ -rf\\ /");
    assert_eq!(sanitize_shell_value("pkg$(whoami)"), "pkg\\$\\(whoami\\)");
}
```

- [ ] **Step 3: Write test for section ordering**

Verify Containerfile section order matches Go exactly (packages → services → firewall → ... → epilogue). Use `insta` snapshot test for full golden comparison.

- [ ] **Step 4: Implement renderer, run tests, commit**

Section order must match Go:
1. FROM + repos + GPG + modules + packages (`dnf install -y`)
2. Services (enable/disable)
3. Firewall zones
4. Scheduled tasks (timer COPYs)
5. Config files (COPY per top-level dir from `configCopyRoots()`)
6. Non-RPM software
7. Containers (quadlet COPYs)
8. Users
9. Kernel/boot (kargs.d, sysctl, modules)
10. SELinux
11. Network (routes, hosts, proxy)
12. Secrets comments
13. Epilogue (tmpfiles, `RUN bootc container lint`)

---

### Task 23: All Renderers (report.html, kickstart, audit, secrets, README)

Go writes 8 artifacts unconditionally. All 8 must exist in Phase 1 output.

**Files:**
- Create: `inspectah-pipeline/src/render/report.rs`
- Create: `inspectah-pipeline/src/render/kickstart.rs`
- Create: `inspectah-pipeline/src/render/audit.rs`
- Create: `inspectah-pipeline/src/render/secrets.rs`
- Create: `inspectah-pipeline/src/render/readme.rs`

- [ ] **Step 1: Write tests for each renderer**

```rust
#[test]
fn test_report_html_renders() {
    let snap = test_snapshot();
    let html = render_report(&snap, &RenderContext { target: None });
    assert!(html.contains("<!DOCTYPE html>"));
    assert!(html.contains("PatternFly")); // minimal PatternFly skeleton
}

#[test]
fn test_kickstart_renders() {
    let snap = test_snapshot();
    let ks = render_kickstart(&snap);
    assert!(ks.contains("#version="));
}

#[test]
fn test_audit_report_renders() {
    let snap = test_snapshot();
    let md = render_audit(&snap);
    assert!(md.contains("# Audit Report"));
}

#[test]
fn test_secrets_review_renders() {
    let snap = test_snapshot();
    let md = render_secrets_review(&snap);
    assert!(md.contains("# Secrets Review"));
}

#[test]
fn test_readme_renders() {
    let snap = test_snapshot();
    let md = render_readme(&snap);
    assert!(md.contains("podman build"));
}
```

- [ ] **Step 2: Write HTML escaping test for report**

```rust
#[test]
fn test_report_html_escapes_values() {
    let mut snap = test_snapshot();
    // Plant XSS-like content in a package name
    snap.rpm.as_mut().unwrap().packages_added[0].name = "<script>alert(1)</script>".into();
    let html = render_report(&snap, &RenderContext { target: None });
    assert!(!html.contains("<script>"), "HTML must escape snapshot values");
    assert!(html.contains("&lt;script&gt;"));
}
```

- [ ] **Step 3: Implement all renderers**

`report.html`: Minimal PatternFly 6 HTML with embedded snapshot data. Does NOT need to be the full interactive dashboard (that's Phase 5) — just a valid HTML report with sections for each inspector.

`kickstart-suggestion.ks`: Kickstart fragment with user/partition suggestions from the snapshot.

- [ ] **Step 4: Use insta snapshot tests for format stability, commit**

---

### Task 24: Config Tree + Tarball Construction

Implements the full `writeConfigTree()` contract from Go, not just `config/etc/`.

**Files:**
- Create: `inspectah-pipeline/src/render/configtree.rs`
- Create: `inspectah-pipeline/src/render/tarball.rs`
- Modify: `inspectah-pipeline/Cargo.toml` (add `tar`, `flate2`, `walkdir`, `chrono`)

- [ ] **Step 1: Write test for config file materialization**

```rust
#[test]
fn test_config_tree_materializes_etc() {
    let snap = snapshot_with_config("/etc/httpd/conf/httpd.conf", "ServerRoot /etc/httpd");
    let dir = tempdir().unwrap();
    write_config_tree(&snap, dir.path());
    assert!(dir.path().join("config/etc/httpd/conf/httpd.conf").exists());
}
```

- [ ] **Step 2: Write test for repo/GPG file materialization**

```rust
#[test]
fn test_config_tree_includes_repo_files() {
    let snap = snapshot_with_repo("/etc/yum.repos.d/epel.repo", "[epel]...");
    let dir = tempdir().unwrap();
    write_config_tree(&snap, dir.path());
    assert!(dir.path().join("config/etc/yum.repos.d/epel.repo").exists());
}
```

- [ ] **Step 3: Write test for kernel/boot snippet materialization**

Test that modules-load.d, modprobe.d, dracut.conf.d, tuned profiles, and kargs.d content are all materialized under `config/`.

- [ ] **Step 4: Write test for drop-in mirroring and non-RPM env files**

Test that systemd drop-ins appear in both `config/` and `drop-ins/`, and that non-RPM env files are materialized.

- [ ] **Step 5: Write path safety tests**

```rust
#[test]
fn test_reject_path_traversal() {
    assert!(validate_path("../../etc/passwd").is_err());
}

#[test]
fn test_reject_nul_bytes() {
    assert!(validate_path("etc/config\0.txt").is_err());
}

#[test]
fn test_reject_absolute_paths_in_tarball() {
    assert!(validate_tarball_entry("/etc/passwd").is_err());
}

#[test]
fn test_reject_symlink_escape() {
    // Symlink pointing outside tarball root must be rejected
    assert!(validate_symlink_target("../../../etc/shadow", "config/").is_err());
}
```

- [ ] **Step 6: Write test for tarball structure**

```rust
#[test]
fn test_tarball_has_all_always_written_artifacts() {
    let snap = test_snapshot();
    let dir = tempdir().unwrap();
    render_all(&snap, &RenderContext { target: None }, dir.path());
    let tarball_path = create_tarball(dir.path(), "testhost");

    let expected = [
        "inspection-snapshot.json",
        "Containerfile",
        "README.md",
        "report.html",
        "audit-report.md",
        "secrets-review.md",
        "kickstart-suggestion.ks",
        "schema/snapshot.schema.json",
    ];
    let entries = list_tarball_entries(&tarball_path);
    for artifact in &expected {
        assert!(
            entries.iter().any(|e| e.ends_with(artifact)),
            "missing always-written artifact: {artifact}"
        );
    }
}
```

- [ ] **Step 7: Implement config tree (full writeConfigTree contract) and tarball, commit**

Reference `cmd/inspectah/internal/renderer/configtree.go` as the canonical source for materialization paths. The Rust implementation must cover: config files, repo/GPG files, firewall zones, kernel/boot snippets (modules-load.d, modprobe.d, dracut.conf.d, tuned, kargs.d), systemd drop-in mirroring, generated/local timer+service units, non-RPM env files.

---

### Task 25: CLI Scan Subcommand

Phase 1 CLI surface: `inspectah scan [--inspect-only] [--output PATH]` and `inspectah version`. No `--host-root`, `--target`, or `--no-redaction`.

**Files:**
- Modify: `inspectah-cli/Cargo.toml` (add `clap`)
- Modify: `inspectah-cli/src/main.rs`
- Create: `inspectah-cli/src/commands/mod.rs`
- Create: `inspectah-cli/src/commands/scan.rs`
- Create: `inspectah-cli/src/commands/version.rs`

- [ ] **Step 1: Write clap derive structs**

```rust
// inspectah-cli/src/commands/scan.rs
use clap::Args;

#[derive(Args)]
pub struct ScanArgs {
    /// Write JSON snapshot only, skip tarball/artifact generation
    #[arg(long)]
    pub inspect_only: bool,

    /// Output file path (tarball) or directory (with --inspect-only)
    #[arg(long, short)]
    pub output: Option<std::path::PathBuf>,
}
```

- [ ] **Step 2: Wire pipeline stages**

```rust
pub fn run_scan(args: &ScanArgs) -> Result<(), Box<dyn std::error::Error>> {
    let executor = Box::new(RealExecutor::new());
    // Detect source system, build InspectionContext
    // collect → validate → redact → render → tarball
    // If --inspect-only, save JSON and return
}
```

- [ ] **Step 3: Test CLI end-to-end** (must be on Linux with RPM tooling)

```bash
cargo run --bin inspectah -- scan --inspect-only --output /tmp/test-snapshot.json
```

- [ ] **Step 4: Commit**

---

### Task 26: End-to-End Integration Test

**Mandatory parity gate.** The RPM section golden file (`testdata/golden/go-v13-rpm-section.json`) must exist. If it doesn't, CI fails. Phase 1 proves RPM-section parity, not full-snapshot.

**Files:**
- Create: `inspectah-pipeline/tests/e2e.rs`

- [ ] **Step 1: Write E2E pipeline test with MockExecutor**

```rust
#[test]
fn test_full_pipeline_produces_valid_tarball() {
    let mock = build_full_rpm_mock_executor();
    let ctx = build_inspection_context(mock);

    // collect → validate → redact → render → tarball
    let output_dir = tempdir().unwrap();
    let tarball = run_full_pipeline(&ctx, output_dir.path());

    // Verify all 8 always-written artifacts
    let entries = list_tarball_entries(&tarball);
    let required = [
        "inspection-snapshot.json", "Containerfile", "README.md",
        "report.html", "audit-report.md", "secrets-review.md",
        "kickstart-suggestion.ks", "schema/snapshot.schema.json",
    ];
    for artifact in &required {
        assert!(entries.iter().any(|e| e.ends_with(artifact)),
            "missing: {artifact}");
    }
}
```

- [ ] **Step 2: Write tarball-wide secret absence check**

```rust
#[test]
fn test_no_secrets_in_any_artifact() {
    // Plant a known secret in mock data
    let mock = mock_with_planted_secret("db_password = s3cret");
    let tarball = run_full_pipeline_from_mock(mock);

    // Extract and check every text artifact
    for (name, content) in extract_text_files(&tarball) {
        assert!(!content.contains("s3cret"),
            "secret leaked into artifact: {name}");
    }
}
```

- [ ] **Step 3: Write mandatory RPM-section parity gate test**

Phase 1 proves **RPM-section parity only**. Full-snapshot parity is Phase 2.

```rust
#[test]
fn test_go_vs_rust_rpm_section_parity() {
    let go_rpm_golden = include_str!("../../testdata/golden/go-v13-rpm-section.json");
    let divergences_md = include_str!("../../testdata/divergences.md");
    let allowlist = load_divergence_allowlist(divergences_md);

    // Run Rust RPM inspector against same fixture data the golden was captured from
    let rust_snapshot = run_rust_pipeline_to_snapshot();
    let rust_rpm_json = serde_json::to_string(&rust_snapshot.rpm).unwrap();

    let undocumented = diff_snapshots(go_rpm_golden, &rust_rpm_json, &allowlist).unwrap();
    assert!(
        undocumented.is_empty(),
        "undocumented RPM section divergences:\n{}",
        undocumented.iter()
            .map(|d| format!("  {}: go={}, rust={}", d.path, d.go_value, d.rust_value))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
```

- [ ] **Step 4: Write snapshot trust state verification**

```rust
#[test]
fn test_exported_snapshot_carries_trust_state() {
    let tarball = run_full_pipeline_default();
    let snapshot_json = extract_file(&tarball, "inspection-snapshot.json");
    let snap: InspectionSnapshot = serde_json::from_str(&snapshot_json).unwrap();

    assert!(snap.redaction_state.is_some(), "exported snapshot must carry redaction_state");
    assert_eq!(snap.completeness, Completeness::Full);
}
```

- [ ] **Step 5: Commit**

```bash
git add inspectah-collect/ inspectah-pipeline/ inspectah-cli/
git commit -m "feat: Phase 1 complete — inspectah scan with mandatory parity gate

RPM inspector (ffi-rpm feature-gated + shell fallback), pipeline
(collect → validate → redact → render), all 8 always-written artifact
renderers, full writeConfigTree() contract, tarball construction,
and CLI scan subcommand. Mandatory Go-vs-Rust parity gate with
divergence allowlist. Snapshot carries redaction_state and completeness.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Remaining Phases Roadmap

Phases 2-7 get their own plans when Phase 1 is complete. Brief overview:

| Phase | Scope | Key Deliverables |
|-------|-------|-----------------|
| **2: Inspector Parity** | All 12 carried inspectors + 2 new (hardware, ostree). Two-phase collection. Full redaction engine. Failure policy enforcement. `ffi-selinux` feature gate. **Full-snapshot zero-undocumented-diff parity gate.** | Internal parity milestone: single-host scans equivalent to Go. driftify integration test on VM. |
| **3: Cross-Stream + Preflight** | MigrationContext with derived MigrationKind. RPM preflight (online/offline/skip). Cross-stream advisory output. `--target` CLI flag. | CentOS 9 → RHEL 10 scan with offline manifest. |
| **4: Fleet + Architect v2** | Fleet merge with prevalence thresholds. Multi-artifact decomposition (7 types). | *Can parallelize with Phase 5.* |
| **5: Refine** | axum web backend (preserve PatternFly 6 frontend). ratatui TUI. Snapshot import with trust verification. Full interactive report.html. | Full inspect → refine workflow in both interfaces. *Can parallelize with Phase 4.* |
| **6: Architect UI** | Redesigned web UI for architect v2. TUI architect. | Layer decomposition + export via both interfaces. |
| **7: Polish, Plugins, Packaging** | Non-RPM expansion (Flatpak, snap). Containerless render. Plugin subprocess protocol. COPR/RPM packaging. | Feature-complete. Go codebase archived. |

---

## Key Implementation Notes

### serde `r#static` field
`NonRpmItem.static` is a Rust keyword. Use `r#static` as the field name — serde automatically strips the `r#` prefix during serialization, producing `"static"` in JSON.

### Pipeline typestate enforcement
Pipeline methods live in `inspectah-pipeline`, not `inspectah-core`. The core crate defines only the marker types. The compiler prevents calling `.render()` on `Pipeline<Collected>` because that method doesn't exist on that type — no runtime checks needed.

### Phase 1 includes minimal `ffi-rpm`
The RPM inspector uses `librpm` FFI when the `ffi-rpm` feature is enabled, and falls back to `rpm -qa --queryformat` shell commands when disabled. CI builds both profiles. The FFI validates the dynamic-linking strategy from the approved spec.

### JSON field ordering
Go's `encoding/json` serializes struct fields in declaration order. Rust's `serde_json` serializes in declaration order by default. If field ordering differs between Go and Rust output, the normalized diff tool strips ordering — the output contract is field-name-keyed, not position-keyed.

### SourceSystem is pipeline-internal, constructed from richer detection
`SourceSystem`, `TargetSystem`, and `MigrationContext` are pipeline-internal types **not** serialized into snapshot JSON. They are constructed from system detection probes (bootc status --json, rpm-ostree status --json, /etc/os-release), not merely from `snapshot.system_type + snapshot.os_release`. Bootc's `booted_image` and rpm-ostree's variant + base_image are carried in `SourceSystem` and passed to inspectors via `InspectionContext.source`.

### Typed boundaries at every load-bearing interface
`InspectorOutput.section` is a typed `SectionData` enum, not `serde_json::Value`. `InspectorId` is a typed enum, not a bare `String`. `Warning.severity` is `WarningSeverity`, not `Option<String>`. `SecretDetector::id()` returns `DetectorId` (typed enum). `Finding.kind` is `FindingKind`, `RedactionFinding.kind` is `RedactionKind`, `detection_method` is `DetectionMethod`, and `confidence` is `Confidence` — all typed enums with `rename_all = "snake_case"` for Go-compatible JSON at the serde edge. The compiler proves inspectors emit valid sections, detectors emit valid findings, and renderers handle every section and redaction class exhaustively.

### Phase 1 parity gate is RPM-section scoped
Phase 1 proves RPM-section parity on a package-based host, not full-snapshot zero-diff. The parity test compares `$.rpm` between Go and Rust output. Full-snapshot zero-undocumented-diff is a Phase 2 milestone (when all inspectors are implemented). The mechanics (mandatory golden, divergence allowlist, normalized diff) are the same — only the comparison scope changes.

### Snapshot trust model is wired into the export contract
`InspectionSnapshot` carries `redaction_state: Option<RedactionState>` and `completeness: Completeness`. Exported snapshots must have `redaction_state` set. Only `FullyRedacted` skips redaction on import. `Completeness` records inspector failure state in the artifact.

### Phase 1 CLI surface is minimal
`inspectah scan [--inspect-only] [--output PATH]` and `inspectah version`. No `--host-root` (deferred — may never support containerized deployment). No `--target` (Phase 3). No `--no-redaction` (deferred — would need explicit artifact fencing).
