# Subscription Preservation & Build Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `--preserve-subscription` to collect RHEL entitlement certs during scan, and `inspectah build` to run `podman build` with automatic cert mounting on non-RHEL hosts.

**Architecture:** Two features implemented across five crates. `SubscriptionInspector` collects cert files and org metadata during scan, stores them in a `SubscriptionSection` on the snapshot. Build logic lives in `inspectah-pipeline` (not CLI) — `inspectah-pipeline/src/build/` handles extraction, archive validation, RHEL detection, mount planning, and podman command construction. CLI stays thin: clap args + terminal output. Fleet merge deduplicates by latest cert expiry using typed UTC timestamps, with serial-number-matched cert/key pairing.

**Tech Stack:** Rust (inspectah workspace), `x509-parser` for cert parsing, `base64` for encoding, `time` for typed UTC timestamps (preferred over `chrono` — already in the dep tree via other crates; if not, use `chrono`), `dirs` for platform cache paths, `clap` derive for CLI, `insta` for snapshot testing.

**Spec:** `process-docs/specs/proposed/preserve-subscription-and-build.md`

**Proof discipline:** Every task ends with `cargo check -p <crate>` (or `cargo test -p <crate>`) to confirm compilation. No task is complete until the touched crate compiles cleanly.

---

## File Structure

### New files
- `inspectah-core/src/types/subscription.rs` — `SubscriptionFile`, `SubscriptionSection`, `EntitlementPair` types
- `inspectah-collect/src/inspectors/subscription.rs` — `SubscriptionInspector`
- `inspectah-pipeline/src/build/mod.rs` — build orchestration: `BuildPlan`, `BuildOutcome`
- `inspectah-pipeline/src/build/extract.rs` — `TarballExtractor` / `ArchiveValidator` with full safety contract
- `inspectah-pipeline/src/build/rhel.rs` — RHEL pass-through detection and ambient subscription validation
- `inspectah-cli/src/commands/build.rs` — thin CLI wrapper for `inspectah build`

### Modified files
- `inspectah-core/src/types/mod.rs` — add `pub mod subscription;`
- `inspectah-core/src/snapshot.rs` — add subscription fields, bump schema version to 18
- `inspectah-core/src/types/completeness.rs` — add `InspectorId::Subscription`, `SectionData::Subscription`
- `inspectah-core/src/fleet/mod.rs` — fleet merge for subscription (typed timestamp comparison, serial-matched pairing, source hostname)
- `inspectah-core/Cargo.toml` — add `time` (or `chrono`) dep
- `inspectah-collect/src/inspectors/mod.rs` — add `pub mod subscription;`
- `inspectah-collect/Cargo.toml` — add `x509-parser`, `base64` deps
- `inspectah-pipeline/src/lib.rs` — add `pub mod build;`
- `inspectah-pipeline/src/render/configtree.rs` — stage `subscription/` in tarball
- `inspectah-pipeline/src/render/containerfile.rs` — subscription mount comment block
- `inspectah-pipeline/src/render/secrets.rs` — subscription entry in secrets-review (including subscription-only case)
- `inspectah-pipeline/src/render/readme.rs` — build instructions with `-v` mounts and `inspectah build` reference
- `inspectah-pipeline/Cargo.toml` — add `tar`, `flate2`, `dirs`, `base64`, `time` deps
- `inspectah-cli/src/main.rs` — add `Build(commands::build::BuildArgs)` variant to `Commands` enum
- `inspectah-cli/src/commands/mod.rs` — add `pub mod build;`
- `inspectah-cli/src/commands/scan.rs` — add `--preserve-subscription`, rename `--ack-sensitive`, wire inspector, dynamic error message
- `inspectah-cli/src/commands/fleet.rs` — add `--ack-sensitive` flag to fleet aggregate, refusal-to-export gate
- `inspectah-web/src/handlers.rs` — accept both header names
- `inspectah-web/src/lib.rs` — CORS for both header names

---

### Task 1: Core types — SubscriptionFile, SubscriptionSection, EntitlementPair

**Files:**
- Create: `inspectah-core/src/types/subscription.rs`
- Modify: `inspectah-core/src/types/mod.rs`
- Modify: `inspectah-core/Cargo.toml`

**R2 changes:** Added `EntitlementPair` for serial-number-matched cert/key pairing (finding #4). Changed `cert_expiry` from `Option<String>` to `Option<time::OffsetDateTime>` with serde as RFC 3339 (finding #3). Added `source_hostname` field for fleet provenance (finding #14).

**Contract decision:** Source hostname is stored per-section in `SubscriptionSection.source_hostname`, not in fleet-level `FleetSnapshotMeta`. This keeps provenance with the data it describes and avoids a schema change to `FleetSnapshotMeta` (which already has `hostnames: Vec<String>` for a different purpose).

- [ ] **Step 1: Add dependencies to inspectah-core**

In `inspectah-core/Cargo.toml`, add under `[dependencies]`:
```toml
base64 = "0.22"
time = { version = "0.3", features = ["serde", "serde-well-known", "parsing", "formatting"] }
```

- [ ] **Step 2: Create the types module with typed timestamps**

Create `inspectah-core/src/types/subscription.rs`:
```rust
use serde::{Deserialize, Serialize};

/// A single file collected from the subscription tree.
/// `cert_expiry` is a typed UTC timestamp — serialized as RFC 3339 at the serde boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubscriptionFile {
    pub path: String,
    pub content: String,
    pub size_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(with = "time::serde::rfc3339::option")]
    pub cert_expiry: Option<time::OffsetDateTime>,
}

/// A cert/key pair matched by serial number.
/// Completeness requires BOTH cert and key for a given serial.
#[derive(Debug, Clone, PartialEq)]
pub struct EntitlementPair {
    pub serial: String,
    pub cert_path: Option<String>,
    pub key_path: Option<String>,
}

impl EntitlementPair {
    pub fn is_complete(&self) -> bool {
        self.cert_path.is_some() && self.key_path.is_some()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SubscriptionSection {
    pub entitlement_certs: Vec<SubscriptionFile>,
    pub ca_certs: Vec<SubscriptionFile>,
    pub config_files: Vec<SubscriptionFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(with = "time::serde::rfc3339::option")]
    pub earliest_expiry: Option<time::OffsetDateTime>,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub incomplete: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_uuid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rhsm_server: Option<String>,
    /// Hostname of the source system — used for fleet provenance tracking.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_hostname: Option<String>,
}

/// Extract serial number from an entitlement cert/key filename.
/// Convention: `<serial>.pem` for certs, `<serial>-key.pem` for keys.
pub fn parse_serial(path: &str) -> Option<(String, bool)> {
    let filename = std::path::Path::new(path)
        .file_name()?
        .to_str()?;
    if let Some(serial) = filename.strip_suffix("-key.pem") {
        Some((serial.to_string(), true))
    } else if let Some(serial) = filename.strip_suffix(".pem") {
        Some((serial.to_string(), false))
    } else {
        None
    }
}

/// Group entitlement files into serial-matched pairs.
/// Returns pairs and a list of orphaned files (cert without key or vice versa).
pub fn match_entitlement_pairs(files: &[SubscriptionFile]) -> (Vec<EntitlementPair>, Vec<String>) {
    use std::collections::BTreeMap;

    let mut pairs: BTreeMap<String, EntitlementPair> = BTreeMap::new();

    for f in files {
        if let Some((serial, is_key)) = parse_serial(&f.path) {
            let pair = pairs.entry(serial.clone()).or_insert_with(|| EntitlementPair {
                serial,
                cert_path: None,
                key_path: None,
            });
            if is_key {
                pair.key_path = Some(f.path.clone());
            } else {
                pair.cert_path = Some(f.path.clone());
            }
        }
    }

    let mut orphans = Vec::new();
    let mut complete = Vec::new();
    for (_, pair) in pairs {
        if pair.is_complete() {
            complete.push(pair);
        } else {
            let path = pair.cert_path.as_ref().or(pair.key_path.as_ref()).unwrap().clone();
            orphans.push(path);
        }
    }

    (complete, orphans)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subscription_section_roundtrip() {
        let expiry = time::OffsetDateTime::from_unix_timestamp(1_723_680_000).unwrap();
        let section = SubscriptionSection {
            entitlement_certs: vec![SubscriptionFile {
                path: "/etc/pki/entitlement/123.pem".into(),
                content: "base64data".into(),
                size_bytes: 1024,
                cert_expiry: Some(expiry),
            }],
            ca_certs: vec![],
            config_files: vec![],
            earliest_expiry: Some(expiry),
            incomplete: false,
            org_id: Some("12345".into()),
            system_uuid: Some("abc-def-ghi".into()),
            rhsm_server: Some("subscription.rhsm.redhat.com".into()),
            source_hostname: Some("host-a.example.com".into()),
        };
        let json = serde_json::to_string(&section).unwrap();
        let parsed: SubscriptionSection = serde_json::from_str(&json).unwrap();
        assert_eq!(section, parsed);
        // Verify ISO 8601 / RFC 3339 format in output
        assert!(json.contains("2024-08-15T"));
        assert!(!json.contains("Mon,"));  // not RFC 2822
    }

    #[test]
    fn test_subscription_section_default_is_empty() {
        let section = SubscriptionSection::default();
        assert!(section.entitlement_certs.is_empty());
        assert!(section.earliest_expiry.is_none());
        assert!(!section.incomplete);
        assert!(section.org_id.is_none());
        assert!(section.source_hostname.is_none());
    }

    #[test]
    fn test_subscription_section_skips_none_fields() {
        let section = SubscriptionSection::default();
        let json = serde_json::to_string(&section).unwrap();
        assert!(!json.contains("earliest_expiry"));
        assert!(!json.contains("org_id"));
        assert!(!json.contains("incomplete"));
        assert!(!json.contains("source_hostname"));
    }

    #[test]
    fn test_parse_serial_cert() {
        assert_eq!(parse_serial("/etc/pki/entitlement/123456.pem"), Some(("123456".into(), false)));
    }

    #[test]
    fn test_parse_serial_key() {
        assert_eq!(parse_serial("/etc/pki/entitlement/123456-key.pem"), Some(("123456".into(), true)));
    }

    #[test]
    fn test_match_pairs_complete() {
        let files = vec![
            SubscriptionFile { path: "/etc/pki/entitlement/111.pem".into(), content: "c".into(), size_bytes: 1, cert_expiry: None },
            SubscriptionFile { path: "/etc/pki/entitlement/111-key.pem".into(), content: "k".into(), size_bytes: 1, cert_expiry: None },
        ];
        let (pairs, orphans) = match_entitlement_pairs(&files);
        assert_eq!(pairs.len(), 1);
        assert!(pairs[0].is_complete());
        assert!(orphans.is_empty());
    }

    #[test]
    fn test_match_pairs_mismatched_serials() {
        let files = vec![
            SubscriptionFile { path: "/etc/pki/entitlement/111.pem".into(), content: "c".into(), size_bytes: 1, cert_expiry: None },
            SubscriptionFile { path: "/etc/pki/entitlement/222-key.pem".into(), content: "k".into(), size_bytes: 1, cert_expiry: None },
        ];
        let (pairs, orphans) = match_entitlement_pairs(&files);
        assert!(pairs.is_empty());
        assert_eq!(orphans.len(), 2);
    }

    #[test]
    fn test_match_pairs_missing_key() {
        let files = vec![
            SubscriptionFile { path: "/etc/pki/entitlement/111.pem".into(), content: "c".into(), size_bytes: 1, cert_expiry: None },
        ];
        let (pairs, orphans) = match_entitlement_pairs(&files);
        assert!(pairs.is_empty());
        assert_eq!(orphans.len(), 1);
    }
}
```

- [ ] **Step 3: Register the module**

In `inspectah-core/src/types/mod.rs`, add:
```rust
pub mod subscription;
```

- [ ] **Step 4: Compile check**

Run: `cargo check -p inspectah-core`
Expected: compiles cleanly

- [ ] **Step 5: Run full crate tests**

Run: `cargo test -p inspectah-core`
Expected: all existing tests pass + new tests

- [ ] **Step 6: Commit**

```bash
git add inspectah-core/src/types/subscription.rs inspectah-core/src/types/mod.rs inspectah-core/Cargo.toml
git commit -m "feat(core): add SubscriptionFile, SubscriptionSection, EntitlementPair types

Serial-number matching for cert/key pairs, typed OffsetDateTime for
expiry (RFC 3339 serde), source_hostname for fleet provenance."
```

---

### Task 2: Schema changes — snapshot fields + version bump

**Files:**
- Modify: `inspectah-core/src/snapshot.rs`
- Modify: `inspectah-core/src/types/completeness.rs`

- [ ] **Step 1: Add InspectorId::Subscription variant**

In `inspectah-core/src/types/completeness.rs`, add to the `InspectorId` enum:
```rust
Subscription,
```

Add to the `SectionData` enum:
```rust
#[serde(rename = "subscription")]
Subscription(super::subscription::SubscriptionSection),
```

- [ ] **Step 2: Compile check completeness**

Run: `cargo check -p inspectah-core`
Expected: may need exhaustive match arm updates — fix any that the compiler flags

- [ ] **Step 3: Add snapshot fields and bump version**

In `inspectah-core/src/snapshot.rs`:

Change:
```rust
pub const SCHEMA_VERSION: u32 = 17;
```
To:
```rust
pub const SCHEMA_VERSION: u32 = 18;
```

Add to `InspectionSnapshot` struct:
```rust
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscription: Option<crate::types::subscription::SubscriptionSection>,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub preserved_subscription: bool,
```

- [ ] **Step 4: Write snapshot test**

Add to the test module in `snapshot.rs`:
```rust
    #[test]
    fn test_snapshot_with_subscription() {
        use crate::types::subscription::{SubscriptionFile, SubscriptionSection};
        let expiry = time::OffsetDateTime::from_unix_timestamp(1_723_680_000).unwrap();
        let mut snap = InspectionSnapshot::new();
        snap.subscription = Some(SubscriptionSection {
            entitlement_certs: vec![SubscriptionFile {
                path: "/etc/pki/entitlement/123.pem".into(),
                content: "base64data".into(),
                size_bytes: 1024,
                cert_expiry: Some(expiry),
            }],
            ..Default::default()
        });
        snap.preserved_subscription = true;
        snap.sensitive_snapshot = true;
        let json = serde_json::to_string(&snap).unwrap();
        let parsed = InspectionSnapshot::load(&json).unwrap();
        assert!(parsed.subscription.is_some());
        assert!(parsed.preserved_subscription);
    }

    #[test]
    fn test_v17_snapshot_rejected() {
        let json = r#"{"schema_version": 17, "meta": {}, "system_type": "package-mode", "preflight": {"status": "ok"}, "warnings": [], "redactions": []}"#;
        let result = InspectionSnapshot::load(json);
        assert!(result.is_err());
    }
```

- [ ] **Step 5: Fix any version-dependent tests**

Update `test_current_version_loads` and any other test that hardcodes schema version 17 to use 18.

- [ ] **Step 6: Compile check + test**

Run: `cargo check -p inspectah-core && cargo test -p inspectah-core`
Expected: compiles cleanly, all tests pass

- [ ] **Step 7: Commit**

```bash
git add inspectah-core/src/snapshot.rs inspectah-core/src/types/completeness.rs
git commit -m "feat(core): add subscription fields to snapshot, bump schema to v18"
```

---

### Task 3: SubscriptionInspector — collection logic

**Files:**
- Create: `inspectah-collect/src/inspectors/subscription.rs`
- Modify: `inspectah-collect/src/inspectors/mod.rs`
- Modify: `inspectah-collect/Cargo.toml`

**R2 changes:** Fixed `SourceSystemKind::PackageMode` → `SourceSystemKind::PackageBased` (finding #2). Replaced `Warning::new(...)` with struct literal `Warning { inspector: "subscription".into(), message: ..., ..Default::default() }` (finding #2). Restricted symlink resolution to approved subscription roots only (finding #7). Changed `evaluate_bundle_completeness` to use serial-number-matched `EntitlementPair` (finding #4). Changed expiry parsing to produce `time::OffsetDateTime` instead of RFC 2822 strings (finding #3).

**R3 changes:** `collect_dir_pems()` now uses `std::fs::canonicalize()` on resolved symlink targets instead of lexical `starts_with()`, preventing `../../` escape via relative symlinks (R2 finding #2). `collect_single_file()` now also validates symlinks against approved roots (R2 finding #2). Hostname sourced from `executor.read_file("/etc/hostname")` matching `collect.rs` pattern, not from nonexistent `ctx.hostname` field (R2 finding #6). Wave placement note added (R2 finding #5).

- [ ] **Step 1: Add dependencies**

In `inspectah-collect/Cargo.toml`, add under `[dependencies]`:
```toml
x509-parser = "0.16"
base64 = "0.22"
time = { version = "0.3", features = ["parsing", "formatting"] }
```

- [ ] **Step 2: Write the inspector**

Create `inspectah-collect/src/inspectors/subscription.rs`:
```rust
use base64::Engine;
use inspectah_core::traits::executor::Executor;
use inspectah_core::traits::inspector::{
    InspectionContext, Inspector, InspectorError, InspectorOutput,
};
use inspectah_core::traits::progress::ProgressSink;
use inspectah_core::types::completeness::{InspectorId, SectionData, SourceSystemKind};
use inspectah_core::types::subscription::{
    SubscriptionFile, SubscriptionSection, match_entitlement_pairs,
};
use inspectah_core::types::warnings::Warning;
use std::path::Path;

const MAX_FILE_SIZE: u64 = 1_048_576; // 1 MB safety valve

const ENTITLEMENT_DIR: &str = "/etc/pki/entitlement";
const CONSUMER_CERT: &str = "/etc/pki/consumer/cert.pem";
const RHSM_CONF: &str = "/etc/rhsm/rhsm.conf";
const RHSM_CA_DIR: &str = "/etc/rhsm/ca";
const REDHAT_REPO: &str = "/etc/yum.repos.d/redhat.repo";

/// Approved subscription roots — symlinks must resolve within one of these.
const APPROVED_ROOTS: &[&str] = &[
    "/etc/pki/entitlement/",
    "/etc/rhsm/",
    "/etc/yum.repos.d/redhat.repo",
];

pub struct SubscriptionInspector;

impl SubscriptionInspector {
    pub fn new() -> Self {
        Self
    }
}

impl Inspector for SubscriptionInspector {
    fn id(&self) -> InspectorId {
        InspectorId::Subscription
    }

    fn applicable_to(&self) -> &[SourceSystemKind] {
        // Correct variant — PackageBased, not PackageMode
        &[SourceSystemKind::PackageBased]
    }

    fn inspect(
        &self,
        ctx: &InspectionContext<'_>,
        progress: &dyn ProgressSink,
    ) -> Result<InspectorOutput, InspectorError> {
        progress.update("collecting subscription material");

        let exec = ctx.executor;
        let mut section = SubscriptionSection::default();
        let mut warnings = Vec::new();

        // Populate source hostname for fleet provenance.
        // InspectionContext has no hostname field — read /etc/hostname via executor,
        // matching how collect.rs populates snapshot.meta["hostname"] (line 211).
        section.source_hostname = exec
            .read_file(Path::new(exec.host_root()).join("etc/hostname").as_path())
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        // 1. Entitlement certs
        collect_dir_pems(exec, ENTITLEMENT_DIR, &mut section.entitlement_certs, &mut warnings);

        // 2. RHSM config
        if let Some(f) = collect_single_file(exec, RHSM_CONF, &mut warnings) {
            section.config_files.push(f);
        }

        // 3. CA certs
        collect_dir_pems(exec, RHSM_CA_DIR, &mut section.ca_certs, &mut warnings);

        // 4. redhat.repo
        if let Some(f) = collect_single_file(exec, REDHAT_REPO, &mut warnings) {
            section.config_files.push(f);
        }

        // 5. Parse org metadata from consumer cert (metadata only, not collected)
        parse_org_metadata(exec, CONSUMER_CERT, &mut section);

        // 6. Parse cert expiry from entitlement certs — typed OffsetDateTime
        parse_cert_expiries(&mut section);

        // 7. Evaluate bundle completeness with serial-number matching
        evaluate_bundle_completeness(&mut section, &mut warnings);

        Ok(InspectorOutput {
            section: SectionData::Subscription(section),
            warnings,
            redaction_hints: Vec::new(),
        })
    }
}

/// Warning helper — Warning is a plain struct, no constructor.
fn warn(message: impl Into<String>) -> Warning {
    Warning {
        inspector: "subscription".into(),
        message: message.into(),
        ..Default::default()
    }
}

fn collect_dir_pems(
    exec: &dyn Executor,
    dir: &str,
    dest: &mut Vec<SubscriptionFile>,
    warnings: &mut Vec<Warning>,
) {
    let dir_path = Path::new(exec.host_root()).join(dir.trim_start_matches('/'));
    let entries = match exec.read_dir(&dir_path) {
        Ok(e) => e,
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                return; // optional directory, silently skip
            }
            warnings.push(warn(format!("Cannot read {dir}: {e}")));
            return;
        }
    };

    for entry in &entries {
        if !entry.ends_with(".pem") {
            continue;
        }
        let file_path = dir_path.join(entry);

        // Validate symlink stays within APPROVED subscription roots.
        // Use canonicalize() to resolve ALL components (../, symlink chains)
        // so relative traversal attacks can't bypass the boundary check.
        if let Ok(_target) = exec.read_link(&file_path) {
            // canonicalize() resolves the full chain: symlink -> real path
            match file_path.canonicalize() {
                Ok(canonical) => {
                    let within_approved = APPROVED_ROOTS.iter().any(|root| {
                        let full_root = Path::new(exec.host_root())
                            .join(root.trim_start_matches('/'));
                        // Also canonicalize the root to handle any mounts/symlinks
                        let canonical_root = full_root.canonicalize()
                            .unwrap_or(full_root);
                        canonical.starts_with(&canonical_root)
                    });
                    if !within_approved {
                        warnings.push(warn(format!(
                            "Symlink {dir}/{entry} resolves to {} which is outside \
                             approved subscription paths, skipped",
                            canonical.display()
                        )));
                        continue;
                    }
                }
                Err(e) => {
                    // canonicalize fails if target doesn't exist (dangling symlink)
                    warnings.push(warn(format!(
                        "Symlink {dir}/{entry} cannot be resolved: {e}, skipped"
                    )));
                    continue;
                }
            }
        }

        match exec.read_file(&file_path) {
            Ok(content) => {
                let size = content.len() as u64;
                if size > MAX_FILE_SIZE {
                    warnings.push(warn(format!(
                        "{dir}/{entry}: file exceeds 1 MB limit ({size} bytes), skipped"
                    )));
                    continue;
                }
                let encoded = base64::engine::general_purpose::STANDARD.encode(content.as_bytes());
                dest.push(SubscriptionFile {
                    path: format!("{dir}/{entry}"),
                    content: encoded,
                    size_bytes: size,
                    cert_expiry: None, // filled by parse_cert_expiries
                });
            }
            Err(e) => {
                warnings.push(warn(format!("Cannot read {dir}/{entry}: {e}")));
            }
        }
    }
}

fn collect_single_file(
    exec: &dyn Executor,
    path: &str,
    warnings: &mut Vec<Warning>,
) -> Option<SubscriptionFile> {
    let file_path = Path::new(exec.host_root()).join(path.trim_start_matches('/'));

    // Validate symlink boundary (same canonicalize check as collect_dir_pems)
    if let Ok(_target) = exec.read_link(&file_path) {
        match file_path.canonicalize() {
            Ok(canonical) => {
                let within_approved = APPROVED_ROOTS.iter().any(|root| {
                    let full_root = Path::new(exec.host_root())
                        .join(root.trim_start_matches('/'));
                    let canonical_root = full_root.canonicalize().unwrap_or(full_root);
                    canonical.starts_with(&canonical_root)
                });
                if !within_approved {
                    warnings.push(warn(format!(
                        "{path} is a symlink resolving to {} which is outside \
                         approved subscription paths, skipped",
                        canonical.display()
                    )));
                    return None;
                }
            }
            Err(e) => {
                warnings.push(warn(format!(
                    "{path} is a symlink that cannot be resolved: {e}, skipped"
                )));
                return None;
            }
        }
    }

    match exec.read_file(&file_path) {
        Ok(content) => {
            let size = content.len() as u64;
            if size > MAX_FILE_SIZE {
                warnings.push(warn(format!(
                    "{path}: file exceeds 1 MB limit ({size} bytes), skipped"
                )));
                return None;
            }
            let encoded = base64::engine::general_purpose::STANDARD.encode(content.as_bytes());
            Some(SubscriptionFile {
                path: path.into(),
                content: encoded,
                size_bytes: size,
                cert_expiry: None,
            })
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            warnings.push(warn(format!("Cannot read {path}: {e}")));
            None
        }
    }
}

fn parse_org_metadata(
    exec: &dyn Executor,
    consumer_cert_path: &str,
    section: &mut SubscriptionSection,
) {
    let file_path = Path::new(exec.host_root()).join(consumer_cert_path.trim_start_matches('/'));
    let content = match exec.read_file(&file_path) {
        Ok(c) => c,
        Err(_) => return, // consumer cert missing is fine — not build-required
    };

    if let Some(der) = pem_to_der(&content) {
        if let Ok((_, cert)) = x509_parser::parse_x509_certificate(&der) {
            for attr in cert.subject().iter() {
                if attr.attr_type() == &x509_parser::oid_registry::OID_X509_ORGANIZATION_NAME {
                    if let Ok(val) = attr.attr_value().as_str() {
                        section.org_id = Some(val.to_string());
                    }
                }
            }
            for attr in cert.subject().iter() {
                if attr.attr_type() == &x509_parser::oid_registry::OID_X509_COMMON_NAME {
                    if let Ok(val) = attr.attr_value().as_str() {
                        section.system_uuid = Some(val.to_string());
                    }
                }
            }
            for attr in cert.issuer().iter() {
                if attr.attr_type() == &x509_parser::oid_registry::OID_X509_ORGANIZATION_NAME {
                    if let Ok(val) = attr.attr_value().as_str() {
                        section.rhsm_server = Some(val.to_string());
                    }
                }
            }
        }
    }
}

/// Parse cert expiry using typed `time::OffsetDateTime` — NOT string comparison.
fn parse_cert_expiries(section: &mut SubscriptionSection) {
    let mut earliest: Option<time::OffsetDateTime> = None;

    for cert_file in &mut section.entitlement_certs {
        if !cert_file.path.ends_with("-key.pem") {
            if let Ok(raw) = base64::engine::general_purpose::STANDARD.decode(&cert_file.content) {
                if let Ok(pem_str) = std::str::from_utf8(&raw) {
                    if let Some(der) = pem_to_der(pem_str) {
                        if let Ok((_, cert)) = x509_parser::parse_x509_certificate(&der) {
                            // Convert ASN1 time to typed OffsetDateTime
                            let not_after = cert.validity().not_after;
                            if let Ok(ts) = not_after.to_datetime() {
                                let expiry = time::OffsetDateTime::from_unix_timestamp(
                                    ts.timestamp()
                                ).unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
                                cert_file.cert_expiry = Some(expiry);
                                match &earliest {
                                    None => earliest = Some(expiry),
                                    Some(e) if expiry < *e => earliest = Some(expiry),
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    section.earliest_expiry = earliest;
}

/// Bundle completeness evaluated using serial-number-matched EntitlementPair.
fn evaluate_bundle_completeness(
    section: &mut SubscriptionSection,
    warnings: &mut Vec<Warning>,
) {
    let mut missing = Vec::new();

    // Check entitlement cert+key pairs by serial number
    let (pairs, orphans) = match_entitlement_pairs(&section.entitlement_certs);
    if pairs.is_empty() {
        missing.push("entitlement cert+key pair (matched by serial number)");
    }
    for orphan in &orphans {
        warnings.push(warn(format!(
            "Entitlement file has no matching pair: {orphan}"
        )));
    }

    // Check rhsm.conf
    let has_rhsm_conf = section.config_files.iter().any(|f| f.path.contains("rhsm.conf"));
    if !has_rhsm_conf {
        missing.push("rhsm.conf");
    }

    // Check CA certs
    if section.ca_certs.is_empty() {
        missing.push("CA certs from /etc/rhsm/ca/");
    }

    // Check redhat.repo
    let has_redhat_repo = section.config_files.iter().any(|f| f.path.contains("redhat.repo"));
    if !has_redhat_repo {
        missing.push("redhat.repo");
    }

    if !missing.is_empty() {
        section.incomplete = true;
        for item in &missing {
            warnings.push(warn(format!(
                "Incomplete subscription bundle: missing {item}"
            )));
        }
    }
}

fn pem_to_der(pem_content: &str) -> Option<Vec<u8>> {
    let begin = pem_content.find("-----BEGIN CERTIFICATE-----")?;
    let end = pem_content.find("-----END CERTIFICATE-----")?;
    let b64_start = begin + "-----BEGIN CERTIFICATE-----".len();
    let b64 = &pem_content[b64_start..end].replace(['\n', '\r', ' '], "");
    base64::engine::general_purpose::STANDARD.decode(b64).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::traits::executor::ExecResult;
    use std::collections::HashMap;
    use std::io;
    use std::path::{Path, PathBuf};

    struct MockExecutor {
        files: HashMap<PathBuf, String>,
        dirs: HashMap<PathBuf, Vec<String>>,
        links: HashMap<PathBuf, String>,
    }

    impl MockExecutor {
        fn new() -> Self {
            Self {
                files: HashMap::new(),
                dirs: HashMap::new(),
                links: HashMap::new(),
            }
        }

        fn with_file(mut self, path: &str, content: &str) -> Self {
            self.files.insert(PathBuf::from(path), content.into());
            self
        }

        fn with_dir(mut self, path: &str, entries: Vec<&str>) -> Self {
            self.dirs.insert(
                PathBuf::from(path),
                entries.into_iter().map(String::from).collect(),
            );
            self
        }

        fn with_link(mut self, path: &str, target: &str) -> Self {
            self.links.insert(PathBuf::from(path), target.into());
            self
        }
    }

    impl Executor for MockExecutor {
        fn run(&self, _cmd: &str, _args: &[&str]) -> ExecResult {
            ExecResult::default()
        }
        fn run_with_line_callback(
            &self,
            _cmd: &str,
            _args: &[&str],
            _cb: &mut dyn FnMut(&str),
        ) -> ExecResult {
            ExecResult::default()
        }
        fn read_file(&self, path: &Path) -> io::Result<String> {
            self.files
                .get(path)
                .cloned()
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "not found"))
        }
        fn file_exists(&self, path: &Path) -> bool {
            self.files.contains_key(path)
        }
        fn read_dir(&self, path: &Path) -> io::Result<Vec<String>> {
            self.dirs
                .get(path)
                .cloned()
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "not found"))
        }
        fn read_link(&self, path: &Path) -> io::Result<String> {
            self.links
                .get(path)
                .cloned()
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "not a symlink"))
        }
        fn host_root(&self) -> &Path {
            Path::new("/")
        }
    }

    #[test]
    fn test_collects_entitlement_certs() {
        let exec = MockExecutor::new()
            .with_dir("/etc/pki/entitlement", vec!["123.pem", "123-key.pem"])
            .with_file("/etc/pki/entitlement/123.pem", "cert-content")
            .with_file("/etc/pki/entitlement/123-key.pem", "key-content");

        let mut certs = Vec::new();
        let mut warnings = Vec::new();
        collect_dir_pems(&exec, ENTITLEMENT_DIR, &mut certs, &mut warnings);

        assert_eq!(certs.len(), 2);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_missing_entitlement_dir_skipped_silently() {
        let exec = MockExecutor::new();
        let mut certs = Vec::new();
        let mut warnings = Vec::new();
        collect_dir_pems(&exec, ENTITLEMENT_DIR, &mut certs, &mut warnings);
        assert!(certs.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_permission_denied_produces_warning() {
        struct PermDeniedExecutor;
        impl Executor for PermDeniedExecutor {
            fn run(&self, _: &str, _: &[&str]) -> ExecResult { ExecResult::default() }
            fn run_with_line_callback(&self, _: &str, _: &[&str], _: &mut dyn FnMut(&str)) -> ExecResult { ExecResult::default() }
            fn read_file(&self, _: &Path) -> io::Result<String> {
                Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied"))
            }
            fn file_exists(&self, _: &Path) -> bool { false }
            fn read_dir(&self, _: &Path) -> io::Result<Vec<String>> {
                Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied"))
            }
            fn read_link(&self, _: &Path) -> io::Result<String> {
                Err(io::Error::new(io::ErrorKind::NotFound, ""))
            }
            fn host_root(&self) -> &Path { Path::new("/") }
        }

        let exec = PermDeniedExecutor;
        let mut certs = Vec::new();
        let mut warnings = Vec::new();
        collect_dir_pems(&exec, ENTITLEMENT_DIR, &mut certs, &mut warnings);
        assert!(certs.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("Cannot read"));
    }

    #[test]
    fn test_file_over_1mb_rejected() {
        let big = "x".repeat(1_048_577);
        let exec = MockExecutor::new()
            .with_dir("/etc/pki/entitlement", vec!["big.pem"])
            .with_file("/etc/pki/entitlement/big.pem", &big);

        let mut certs = Vec::new();
        let mut warnings = Vec::new();
        collect_dir_pems(&exec, ENTITLEMENT_DIR, &mut certs, &mut warnings);
        assert!(certs.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("1 MB"));
    }

    #[test]
    fn test_symlink_outside_subscription_roots_rejected() {
        let exec = MockExecutor::new()
            .with_dir("/etc/pki/entitlement", vec!["evil.pem"])
            .with_link("/etc/pki/entitlement/evil.pem", "/etc/shadow");

        let mut certs = Vec::new();
        let mut warnings = Vec::new();
        collect_dir_pems(&exec, ENTITLEMENT_DIR, &mut certs, &mut warnings);
        assert!(certs.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("outside approved subscription paths"));
    }

    #[test]
    fn test_symlink_within_subscription_root_accepted() {
        let exec = MockExecutor::new()
            .with_dir("/etc/pki/entitlement", vec!["good.pem"])
            .with_link("/etc/pki/entitlement/good.pem", "/etc/pki/entitlement/real.pem")
            .with_file("/etc/pki/entitlement/good.pem", "cert-content");

        let mut certs = Vec::new();
        let mut warnings = Vec::new();
        collect_dir_pems(&exec, ENTITLEMENT_DIR, &mut certs, &mut warnings);
        assert_eq!(certs.len(), 1);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_symlink_dotdot_escape_rejected() {
        let exec = MockExecutor::new()
            .with_dir("/etc/pki/entitlement", vec!["escape.pem"])
            .with_link("/etc/pki/entitlement/escape.pem", "../../shadow");

        let mut certs = Vec::new();
        let mut warnings = Vec::new();
        collect_dir_pems(&exec, ENTITLEMENT_DIR, &mut certs, &mut warnings);
        assert!(certs.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("outside approved subscription paths"));
    }

    #[test]
    fn test_bundle_completeness_all_present_serial_matched() {
        let mut section = SubscriptionSection {
            entitlement_certs: vec![
                SubscriptionFile { path: "/etc/pki/entitlement/123.pem".into(), content: "c".into(), size_bytes: 1, cert_expiry: None },
                SubscriptionFile { path: "/etc/pki/entitlement/123-key.pem".into(), content: "k".into(), size_bytes: 1, cert_expiry: None },
            ],
            ca_certs: vec![
                SubscriptionFile { path: "/etc/rhsm/ca/redhat-uep.pem".into(), content: "ca".into(), size_bytes: 1, cert_expiry: None },
            ],
            config_files: vec![
                SubscriptionFile { path: "/etc/rhsm/rhsm.conf".into(), content: "cfg".into(), size_bytes: 1, cert_expiry: None },
                SubscriptionFile { path: "/etc/yum.repos.d/redhat.repo".into(), content: "repo".into(), size_bytes: 1, cert_expiry: None },
            ],
            ..Default::default()
        };
        let mut warnings = Vec::new();
        evaluate_bundle_completeness(&mut section, &mut warnings);
        assert!(!section.incomplete);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_bundle_incomplete_mismatched_serials() {
        let mut section = SubscriptionSection {
            entitlement_certs: vec![
                SubscriptionFile { path: "/etc/pki/entitlement/111.pem".into(), content: "c".into(), size_bytes: 1, cert_expiry: None },
                SubscriptionFile { path: "/etc/pki/entitlement/222-key.pem".into(), content: "k".into(), size_bytes: 1, cert_expiry: None },
            ],
            ca_certs: vec![SubscriptionFile { path: "ca".into(), content: "ca".into(), size_bytes: 1, cert_expiry: None }],
            config_files: vec![
                SubscriptionFile { path: "/etc/rhsm/rhsm.conf".into(), content: "cfg".into(), size_bytes: 1, cert_expiry: None },
                SubscriptionFile { path: "redhat.repo".into(), content: "r".into(), size_bytes: 1, cert_expiry: None },
            ],
            ..Default::default()
        };
        let mut warnings = Vec::new();
        evaluate_bundle_completeness(&mut section, &mut warnings);
        assert!(section.incomplete);
        // Should have orphan warnings AND missing pair warning
        assert!(warnings.iter().any(|w| w.message.contains("no matching pair")));
    }

    #[test]
    fn test_bundle_incomplete_missing_redhat_repo() {
        let mut section = SubscriptionSection {
            entitlement_certs: vec![
                SubscriptionFile { path: "123.pem".into(), content: "c".into(), size_bytes: 1, cert_expiry: None },
                SubscriptionFile { path: "123-key.pem".into(), content: "k".into(), size_bytes: 1, cert_expiry: None },
            ],
            ca_certs: vec![SubscriptionFile { path: "ca".into(), content: "ca".into(), size_bytes: 1, cert_expiry: None }],
            config_files: vec![
                SubscriptionFile { path: "/etc/rhsm/rhsm.conf".into(), content: "cfg".into(), size_bytes: 1, cert_expiry: None },
            ],
            ..Default::default()
        };
        let mut warnings = Vec::new();
        evaluate_bundle_completeness(&mut section, &mut warnings);
        assert!(section.incomplete);
        assert!(warnings.iter().any(|w| w.message.contains("redhat.repo")));
    }

    #[test]
    fn test_collects_redhat_repo() {
        let exec = MockExecutor::new()
            .with_file("/etc/yum.repos.d/redhat.repo", "[rhel-base]\nbaseurl=https://cdn");
        let mut warnings = Vec::new();
        let result = collect_single_file(&exec, REDHAT_REPO, &mut warnings);
        assert!(result.is_some());
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_pem_to_der_valid() {
        assert!(pem_to_der("not a cert").is_none());
        assert!(pem_to_der("").is_none());
    }
}
```

- [ ] **Step 3: Register the module**

In `inspectah-collect/src/inspectors/mod.rs`, add:
```rust
pub mod subscription;
```

- [ ] **Step 4: Compile check**

Run: `cargo check -p inspectah-collect`
Expected: compiles cleanly

- [ ] **Step 5: Run tests**

Run: `cargo test -p inspectah-collect subscription`
Expected: all tests pass

- [ ] **Step 6: Run clippy**

Run: `cargo clippy -p inspectah-collect -- -D warnings`
Expected: clean

- [ ] **Step 7: Commit**

```bash
git add inspectah-collect/src/inspectors/subscription.rs inspectah-collect/src/inspectors/mod.rs inspectah-collect/Cargo.toml
git commit -m "feat(collect): add SubscriptionInspector

Serial-matched entitlement pairing, approved-root symlink restriction,
typed OffsetDateTime expiry, source hostname for fleet provenance."
```

---

### Task 4: Tarball staging for subscription/ directory

**Files:**
- Modify: `inspectah-pipeline/src/render/configtree.rs`
- Modify: `inspectah-pipeline/Cargo.toml`

- [ ] **Step 1: Add base64 dep to pipeline**

In `inspectah-pipeline/Cargo.toml`:
```toml
base64 = "0.22"
```

- [ ] **Step 2: Write failing test for subscription staging**

In `inspectah-pipeline/src/render/configtree.rs`, add to the test module:
```rust
    #[test]
    fn test_subscription_dir_staged() {
        use inspectah_core::types::subscription::{SubscriptionFile, SubscriptionSection};

        let mut snap = InspectionSnapshot::new();
        snap.subscription = Some(SubscriptionSection {
            entitlement_certs: vec![
                SubscriptionFile {
                    path: "/etc/pki/entitlement/123.pem".into(),
                    content: base64::engine::general_purpose::STANDARD.encode("cert-data"),
                    size_bytes: 9,
                    cert_expiry: None,
                },
                SubscriptionFile {
                    path: "/etc/pki/entitlement/123-key.pem".into(),
                    content: base64::engine::general_purpose::STANDARD.encode("key-data"),
                    size_bytes: 8,
                    cert_expiry: None,
                },
            ],
            ca_certs: vec![SubscriptionFile {
                path: "/etc/rhsm/ca/redhat-uep.pem".into(),
                content: base64::engine::general_purpose::STANDARD.encode("ca-data"),
                size_bytes: 7,
                cert_expiry: None,
            }],
            config_files: vec![
                SubscriptionFile {
                    path: "/etc/rhsm/rhsm.conf".into(),
                    content: base64::engine::general_purpose::STANDARD.encode("[rhsm]"),
                    size_bytes: 6,
                    cert_expiry: None,
                },
                SubscriptionFile {
                    path: "/etc/yum.repos.d/redhat.repo".into(),
                    content: base64::engine::general_purpose::STANDARD.encode("[rhel]"),
                    size_bytes: 6,
                    cert_expiry: None,
                },
            ],
            ..Default::default()
        });
        snap.preserved_subscription = true;

        let dir = tempfile::TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();

        assert!(dir.path().join("subscription/entitlement/123.pem").exists());
        assert!(dir.path().join("subscription/entitlement/123-key.pem").exists());
        assert!(dir.path().join("subscription/rhsm/ca/redhat-uep.pem").exists());
        assert!(dir.path().join("subscription/rhsm/rhsm.conf").exists());
        assert!(dir.path().join("subscription/redhat.repo").exists());
    }

    #[test]
    fn test_no_subscription_no_dir() {
        let snap = InspectionSnapshot::new();
        let dir = tempfile::TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();
        assert!(!dir.path().join("subscription").exists());
    }
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p inspectah-pipeline test_subscription_dir`
Expected: FAIL (subscription staging not implemented)

- [ ] **Step 4: Implement subscription staging**

Add to `write_config_tree` in `configtree.rs`, after the existing config tree logic:
```rust
    // Stage subscription material (decoded from base64)
    if snap.preserved_subscription {
        if let Some(ref sub) = snap.subscription {
            stage_subscription_files(output_dir, sub)?;
        }
    }
```

Add the staging function:
```rust
fn stage_subscription_files(
    output_dir: &Path,
    section: &inspectah_core::types::subscription::SubscriptionSection,
) -> Result<(), RenderError> {
    use base64::Engine;

    let sub_dir = output_dir.join("subscription");

    // Entitlement certs -> subscription/entitlement/
    let ent_dir = sub_dir.join("entitlement");
    for f in &section.entitlement_certs {
        let filename = Path::new(&f.path).file_name().unwrap_or_default();
        let dest = ent_dir.join(filename);
        std::fs::create_dir_all(dest.parent().unwrap())?;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&f.content)
            .map_err(|e| RenderError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e)))?;
        std::fs::write(&dest, decoded)?;
    }

    // CA certs -> subscription/rhsm/ca/
    let ca_dir = sub_dir.join("rhsm/ca");
    for f in &section.ca_certs {
        let filename = Path::new(&f.path).file_name().unwrap_or_default();
        let dest = ca_dir.join(filename);
        std::fs::create_dir_all(dest.parent().unwrap())?;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&f.content)
            .map_err(|e| RenderError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e)))?;
        std::fs::write(&dest, decoded)?;
    }

    // Config files (rhsm.conf -> subscription/rhsm/, redhat.repo -> subscription/)
    for f in &section.config_files {
        let dest = if f.path.contains("rhsm.conf") {
            sub_dir.join("rhsm/rhsm.conf")
        } else if f.path.contains("redhat.repo") {
            sub_dir.join("redhat.repo")
        } else {
            continue;
        };
        std::fs::create_dir_all(dest.parent().unwrap())?;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&f.content)
            .map_err(|e| RenderError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e)))?;
        std::fs::write(&dest, decoded)?;
    }

    Ok(())
}
```

- [ ] **Step 5: Compile check + test**

Run: `cargo check -p inspectah-pipeline && cargo test -p inspectah-pipeline test_subscription`
Expected: compiles, both tests pass

- [ ] **Step 6: Commit**

```bash
git add inspectah-pipeline/src/render/configtree.rs inspectah-pipeline/Cargo.toml
git commit -m "feat(pipeline): stage subscription files in tarball output"
```

---

### Task 5: Fleet merge for subscription — typed timestamps, serial matching, source hostname

**Files:**
- Modify: `inspectah-core/src/fleet/mod.rs`

**R2 changes:** Fixed `merge_snapshots` call signature — uses `merge_snapshots(vec![...], manifest)` returning `(merged, warnings)` (finding #2). Uses typed `time::OffsetDateTime` comparison instead of string comparison (finding #3). Records source hostname in fleet subscription metadata (finding #14).

**R3 changes:** Fixed `max_by` comparator to apply hostname tiebreak on equal expiries via `.then_with()`, not just when both are `None` (R2 finding #4). Previously `Some(ea) == Some(eb)` returned `Equal` with no tiebreak, contradicting the plan's own test that expects hostname tiebreak on equal expiries.

**R4 changes:** Reversed hostname comparison direction in `max_by` — `hostname_of(b).cmp(&hostname_of(a))` instead of `hostname_of(a).cmp(&hostname_of(b))`. With `max_by`, the element the comparator calls "greater" wins. The old direction made the lexicographically *largest* hostname win (host-beta over host-alpha), contradicting the test assertion that host-alpha (smallest) should win. Reversal applied to both `Some == Some` and `None, None` arms.

- [ ] **Step 1: Write failing test**

Add to the test module in `fleet/mod.rs`:
```rust
    #[test]
    fn test_fleet_merge_subscription_picks_latest_expiry() {
        use crate::types::subscription::{SubscriptionFile, SubscriptionSection};

        let early = time::OffsetDateTime::from_unix_timestamp(1_719_792_000).unwrap(); // 2024-07-01
        let late = time::OffsetDateTime::from_unix_timestamp(1_725_148_800).unwrap();  // 2024-09-01

        let mut snap1 = InspectionSnapshot::new();
        snap1.meta.insert("hostname".into(), "host-a".into());
        snap1.preserved_subscription = true;
        snap1.sensitive_snapshot = true;
        snap1.subscription = Some(SubscriptionSection {
            entitlement_certs: vec![SubscriptionFile {
                path: "/etc/pki/entitlement/111.pem".into(),
                content: "cert-a".into(),
                size_bytes: 6,
                cert_expiry: Some(early),
            }],
            earliest_expiry: Some(early),
            source_hostname: Some("host-a".into()),
            ..Default::default()
        });

        let mut snap2 = InspectionSnapshot::new();
        snap2.meta.insert("hostname".into(), "host-b".into());
        snap2.preserved_subscription = true;
        snap2.sensitive_snapshot = true;
        snap2.subscription = Some(SubscriptionSection {
            entitlement_certs: vec![SubscriptionFile {
                path: "/etc/pki/entitlement/222.pem".into(),
                content: "cert-b".into(),
                size_bytes: 6,
                cert_expiry: Some(late),
            }],
            earliest_expiry: Some(late),
            source_hostname: Some("host-b".into()),
            ..Default::default()
        });

        let (merged, _warnings) = merge_snapshots(vec![snap1, snap2], None).unwrap();
        assert!(merged.preserved_subscription);
        assert!(merged.sensitive_snapshot);
        let sub = merged.subscription.unwrap();
        // Should pick host-b (later expiry) — typed comparison, not string
        assert_eq!(sub.earliest_expiry, Some(late));
        assert_eq!(sub.source_hostname.as_deref(), Some("host-b"));
    }

    #[test]
    fn test_fleet_merge_subscription_hostname_tiebreak() {
        use crate::types::subscription::{SubscriptionFile, SubscriptionSection};

        let same_time = time::OffsetDateTime::from_unix_timestamp(1_719_792_000).unwrap();

        let mut snap1 = InspectionSnapshot::new();
        snap1.meta.insert("hostname".into(), "host-beta".into());
        snap1.preserved_subscription = true;
        snap1.sensitive_snapshot = true;
        snap1.subscription = Some(SubscriptionSection {
            earliest_expiry: Some(same_time),
            source_hostname: Some("host-beta".into()),
            entitlement_certs: vec![SubscriptionFile {
                path: "111.pem".into(), content: "c".into(), size_bytes: 1, cert_expiry: Some(same_time),
            }],
            ..Default::default()
        });

        let mut snap2 = InspectionSnapshot::new();
        snap2.meta.insert("hostname".into(), "host-alpha".into());
        snap2.preserved_subscription = true;
        snap2.sensitive_snapshot = true;
        snap2.subscription = Some(SubscriptionSection {
            earliest_expiry: Some(same_time),
            source_hostname: Some("host-alpha".into()),
            entitlement_certs: vec![SubscriptionFile {
                path: "222.pem".into(), content: "c".into(), size_bytes: 1, cert_expiry: Some(same_time),
            }],
            ..Default::default()
        });

        let (merged, _) = merge_snapshots(vec![snap1, snap2], None).unwrap();
        let sub = merged.subscription.unwrap();
        // Alphabetical tiebreak — host-alpha wins
        assert_eq!(sub.source_hostname.as_deref(), Some("host-alpha"));
    }

    #[test]
    fn test_fleet_merge_subscription_mixed_presence() {
        use crate::types::subscription::{SubscriptionFile, SubscriptionSection};

        let mut snap1 = InspectionSnapshot::new();
        snap1.meta.insert("hostname".into(), "host-a".into());
        // No subscription

        let mut snap2 = InspectionSnapshot::new();
        snap2.meta.insert("hostname".into(), "host-b".into());
        snap2.preserved_subscription = true;
        snap2.sensitive_snapshot = true;
        snap2.subscription = Some(SubscriptionSection {
            entitlement_certs: vec![SubscriptionFile {
                path: "cert".into(), content: "c".into(), size_bytes: 1, cert_expiry: None,
            }],
            ..Default::default()
        });

        let (merged, _) = merge_snapshots(vec![snap1, snap2], None).unwrap();
        assert!(merged.preserved_subscription);
        assert!(merged.subscription.is_some());
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p inspectah-core test_fleet_merge_subscription`
Expected: FAIL

- [ ] **Step 3: Implement fleet subscription merge**

In `fleet/mod.rs`, in the `merge_snapshots` function body, after the existing `preserved_ssh_keys` merge line, add:
```rust
    // Subscription merge: OR the boolean, pick winner by latest typed expiry
    merged.preserved_subscription = sorted_snapshots.iter().any(|s| s.preserved_subscription);

    let subscription_candidates: Vec<_> = sorted_snapshots
        .iter()
        .filter(|s| s.subscription.is_some() && !s.subscription.as_ref().unwrap().incomplete)
        .collect();

    if !subscription_candidates.is_empty() {
        // Helper: extract hostname for tiebreak (prefer section field, fall back to meta)
        let hostname_of = |snap: &InspectionSnapshot| -> String {
            snap.subscription.as_ref()
                .and_then(|s| s.source_hostname.as_deref())
                .or_else(|| snap.meta.get("hostname").and_then(|v| v.as_str()))
                .unwrap_or("")
                .to_string()
        };

        let winner = subscription_candidates
            .iter()
            .max_by(|a, b| {
                let ea = a.subscription.as_ref().and_then(|s| s.earliest_expiry);
                let eb = b.subscription.as_ref().and_then(|s| s.earliest_expiry);
                match (ea, eb) {
                    (Some(a_exp), Some(b_exp)) => {
                        // Typed comparison + hostname tiebreak: lexicographically
                        // first (smallest) hostname wins for deterministic ordering.
                        // Reversed comparison makes smallest appear "greater" to max_by.
                        a_exp.cmp(&b_exp)
                            .then_with(|| hostname_of(b).cmp(&hostname_of(a)))
                    }
                    (Some(_), None) => std::cmp::Ordering::Greater,
                    (None, Some(_)) => std::cmp::Ordering::Less,
                    // hostname tiebreak: lexicographically first (smallest) hostname wins
                    (None, None) => hostname_of(b).cmp(&hostname_of(a)),
                }
            })
            .unwrap();
        merged.subscription = winner.subscription.clone();
    }
```

- [ ] **Step 4: Compile check + test**

Run: `cargo check -p inspectah-core && cargo test -p inspectah-core test_fleet_merge_subscription`
Expected: compiles, all 3 tests pass

- [ ] **Step 5: Run all fleet tests**

Run: `cargo test -p inspectah-core fleet`
Expected: all pass

- [ ] **Step 6: Commit**

```bash
git add inspectah-core/src/fleet/mod.rs
git commit -m "feat(fleet): merge subscription with typed timestamp comparison and hostname tiebreak"
```

---

### Task 6: CLI scan — --preserve-subscription flag, --ack-sensitive rename, wire inspector

**Files:**
- Modify: `inspectah-cli/src/commands/scan.rs`

- [ ] **Step 1: Add --preserve-subscription flag**

In `ScanArgs`, add:
```rust
    /// Preserve RHEL subscription material (entitlement certs, rhsm config, redhat.repo) for non-RHEL builds
    #[arg(long)]
    pub preserve_subscription: bool,
```

- [ ] **Step 2: Rename --acknowledge-sensitive to --ack-sensitive**

Change:
```rust
    #[arg(long)]
    pub acknowledge_sensitive: bool,
```
To:
```rust
    #[arg(long = "ack-sensitive", visible_alias = "acknowledge-sensitive")]
    pub ack_sensitive: bool,
```

Update all references from `args.acknowledge_sensitive` to `args.ack_sensitive` in the file.

- [ ] **Step 3: Update sensitive_snapshot logic**

Change:
```rust
    snapshot.sensitive_snapshot = args.preserve_password_hashes || args.preserve_ssh_keys;
```
To:
```rust
    snapshot.sensitive_snapshot = args.preserve_password_hashes || args.preserve_ssh_keys || args.preserve_subscription;
    snapshot.preserved_subscription = args.preserve_subscription;
```

- [ ] **Step 4: Update the --ack-sensitive error message to be dynamic**

Change the hardcoded error string to enumerate which sensitive data is present:
```rust
    if snapshot.sensitive_snapshot && !args.ack_sensitive {
        let mut types = Vec::new();
        if snapshot.preserved_credentials {
            types.push("password hashes");
        }
        if snapshot.preserved_ssh_keys {
            types.push("SSH keys");
        }
        if snapshot.preserved_subscription {
            types.push("subscription entitlement certs");
        }
        let type_list = types.join(", ");
        anyhow::bail!(
            "Snapshot contains sensitive data ({type_list}).\n\
             To export, re-run with --ack-sensitive"
        );
    }
```

- [ ] **Step 5: Wire SubscriptionInspector into inspector list**

Add import:
```rust
use inspectah_collect::inspectors::subscription::SubscriptionInspector;
```

In the inspector list, add conditionally:
```rust
    if args.preserve_subscription {
        inspectors.push(Box::new(SubscriptionInspector::new()));
    }
```

- [ ] **Step 6: Compile check + test**

Run: `cargo check -p inspectah-cli && cargo test -p inspectah-cli`
Run: `cargo clippy -p inspectah-cli -- -D warnings`
Expected: pass

- [ ] **Step 7: Commit**

```bash
git add inspectah-cli/src/commands/scan.rs
git commit -m "feat(cli): add --preserve-subscription, rename --ack-sensitive, dynamic error message"
```

---

### Task 7: Fleet CLI — --ack-sensitive flag and export gate

**Files:**
- Modify: `inspectah-cli/src/commands/fleet.rs`

**R2 new task (finding #1).** The spec says `--ack-sensitive` applies to fleet aggregate/export too. Currently `FleetAggregateArgs` has no acknowledgment flag. Fleet aggregate must refuse to produce output when any contributing snapshot has `sensitive_snapshot == true` unless `--ack-sensitive` is passed.

- [ ] **Step 1: Add --ack-sensitive flag to FleetAggregateArgs**

In `FleetAggregateArgs`, add:
```rust
    /// Acknowledge that the merged output may contain sensitive data (subscription certs, password hashes, SSH keys)
    #[arg(long = "ack-sensitive")]
    pub ack_sensitive: bool,
```

- [ ] **Step 2: Add export gate in fleet aggregate flow**

After snapshots are loaded and before `merge_snapshots` is called, add:
```rust
    // Check if any contributing snapshot is sensitive
    let any_sensitive = snapshots.iter().any(|s| s.sensitive_snapshot);
    if any_sensitive && !args.ack_sensitive {
        let mut types = Vec::new();
        if snapshots.iter().any(|s| s.preserved_subscription) {
            types.push("subscription entitlement certs");
        }
        if snapshots.iter().any(|s| s.preserved_credentials) {
            types.push("password hashes");
        }
        if snapshots.iter().any(|s| s.preserved_ssh_keys) {
            types.push("SSH keys");
        }
        let type_list = types.join(", ");
        bail!(
            "Fleet input contains sensitive data ({type_list}).\n\
             To aggregate, re-run with --ack-sensitive"
        );
    }
```

- [ ] **Step 3: Write tests**

Add test for fleet aggregate refusing without `--ack-sensitive`:
```rust
#[test]
fn test_fleet_aggregate_refuses_sensitive_without_ack() {
    // Create a temp dir with a tarball containing sensitive_snapshot: true
    // Run fleet aggregate without --ack-sensitive
    // Assert exit code is non-zero and error mentions --ack-sensitive
}

#[test]
fn test_fleet_aggregate_allows_sensitive_with_ack() {
    // Same setup but with --ack-sensitive
    // Assert success
}
```

These may need to be integration tests or use the existing test helper infrastructure for fleet commands.

- [ ] **Step 4: Compile check + test**

Run: `cargo check -p inspectah-cli && cargo test -p inspectah-cli fleet`
Expected: compiles, tests pass

- [ ] **Step 5: Commit**

```bash
git add inspectah-cli/src/commands/fleet.rs
git commit -m "feat(cli): add --ack-sensitive to fleet aggregate, refuse export of sensitive data"
```

---

### Task 8: Pipeline — build module (extract, validate, plan, execute)

**Files:**
- Create: `inspectah-pipeline/src/build/mod.rs`
- Create: `inspectah-pipeline/src/build/extract.rs`
- Create: `inspectah-pipeline/src/build/rhel.rs`
- Modify: `inspectah-pipeline/src/lib.rs`
- Modify: `inspectah-pipeline/Cargo.toml`

**R2 changes:** Moved build logic from CLI to pipeline (finding #5). Created dedicated `TarballExtractor`/`ArchiveValidator` with full safety contract (finding #6). Added typed `BuildOutcome` enum for exit codes (finding #10). Made RHEL pass-through conditional on ambient bundle validation (finding #8). Added build-time cert expiry checking (finding #10). Added preflight bundle completeness validation (finding #9).

**R3 changes:** Build-side `validate_subscription_bundle()` and `detect_ambient_subscription()` now enforce the SAME four-component bundle contract as scan-side `evaluate_bundle_completeness()` (R2 finding #1). Both require serial-matched `EntitlementPair`, rhsm.conf, CA certs, and redhat.repo — returning hard errors for missing components. Mount plan is ONLY emitted after full validation passes. Extraction uses `tempfile::TempDir` for automatic cleanup on all exit paths (R2 finding #3). Post-extraction path canonicalization validates entries stay under extraction root (R2 finding #8). Tarball-only scope statement added (R2 finding #7).

**R4 changes:** `detect_ambient_subscription()` now validates `/etc/yum.repos.d/redhat.repo` presence (was previously skipped on ambient path). Full four-component symmetry with scan-side and tarball-side validation — no exemptions.

**Contract decision:** Build-time preflight enforces the same four-component bundle contract as scan-time completeness (spec section "Build-usable bundle definition"). `inspectah build` v1 accepts tarball input only. Edited-directory builds use manual `podman build` from the extracted working directory, as documented in the generated README.

- [ ] **Step 1: Add dependencies to pipeline**

In `inspectah-pipeline/Cargo.toml`, add:
```toml
tar = "0.4"
flate2 = "0.2"
dirs = "6"
time = { version = "0.3", features = ["formatting"] }
tempfile = "3"
```

- [ ] **Step 2: Register the build module**

In `inspectah-pipeline/src/lib.rs`, add:
```rust
pub mod build;
```

- [ ] **Step 3: Create the archive extractor with full safety contract**

Create `inspectah-pipeline/src/build/extract.rs`:
```rust
//! Archive extraction with full safety contract.
//!
//! Every forbidden condition from the spec produces a hard error:
//! - Path traversal (`../`)
//! - Absolute paths
//! - Duplicate path entries
//! - File-type replacement (e.g., file replacing symlink at same path)
//! - Special file types (device nodes, FIFOs, sockets) — REJECTED, not skipped
//! - Hard links escaping extraction root
//! - Symlinks escaping extraction root

use anyhow::{Context, Result, bail};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Entry kind tracker for file-type replacement detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryKind {
    Regular,
    Directory,
    Symlink,
    Hardlink,
}

/// Validated, safe tarball extractor.
pub struct TarballExtractor {
    extract_dir: PathBuf,
}

/// Archive validation error — one per forbidden condition.
#[derive(Debug)]
pub enum ArchiveViolation {
    PathTraversal(String),
    AbsolutePath(String),
    DuplicatePath(String),
    TypeReplacement { path: String, was: &'static str, now: &'static str },
    SpecialFileType { path: String, kind: &'static str },
    HardlinkEscape { path: String, target: String },
    SymlinkEscape { path: String, target: String },
}

impl std::fmt::Display for ArchiveViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PathTraversal(p) => write!(f, "path traversal: {p}"),
            Self::AbsolutePath(p) => write!(f, "absolute path: {p}"),
            Self::DuplicatePath(p) => write!(f, "duplicate entry: {p}"),
            Self::TypeReplacement { path, was, now } => write!(f, "type replacement at {path}: {was} -> {now}"),
            Self::SpecialFileType { path, kind } => write!(f, "forbidden file type at {path}: {kind}"),
            Self::HardlinkEscape { path, target } => write!(f, "hard link escape: {path} -> {target}"),
            Self::SymlinkEscape { path, target } => write!(f, "symlink escape: {path} -> {target}"),
        }
    }
}

impl TarballExtractor {
    /// Create extractor targeting a specific directory.
    pub fn new(extract_dir: PathBuf) -> Self {
        Self { extract_dir }
    }

    /// Extract tarball with full safety validation.
    /// Returns the extraction directory on success.
    pub fn extract(&self, tarball: &Path) -> Result<&Path> {
        std::fs::create_dir_all(&self.extract_dir)?;

        let f = std::fs::File::open(tarball)
            .context(format!("cannot open tarball: {}", tarball.display()))?;
        let gz = flate2::read::GzDecoder::new(f);
        let mut archive = tar::Archive::new(gz);

        let mut seen_paths: HashMap<String, EntryKind> = HashMap::new();

        for entry_result in archive.entries()? {
            let mut entry = entry_result?;
            let raw_path = entry.path()?.to_path_buf();
            let path_str = raw_path.to_string_lossy().to_string();

            // Strip first component (tarball prefix directory)
            let stripped: PathBuf = raw_path.components().skip(1).collect();
            if stripped.as_os_str().is_empty() {
                continue;
            }
            let stripped_str = stripped.to_string_lossy().to_string();

            // SAFETY: path traversal
            if stripped_str.contains("..") {
                bail!("{}", ArchiveViolation::PathTraversal(path_str));
            }

            // SAFETY: absolute paths
            if stripped_str.starts_with('/') {
                bail!("{}", ArchiveViolation::AbsolutePath(path_str));
            }

            let entry_type = entry.header().entry_type();

            // SAFETY: special file types — REJECT, don't skip
            let kind = match entry_type {
                tar::EntryType::Regular | tar::EntryType::GNUSparse => EntryKind::Regular,
                tar::EntryType::Directory => EntryKind::Directory,
                tar::EntryType::Symlink => EntryKind::Symlink,
                tar::EntryType::Link => EntryKind::Hardlink,
                tar::EntryType::Char => {
                    bail!("{}", ArchiveViolation::SpecialFileType { path: path_str, kind: "char device" });
                }
                tar::EntryType::Block => {
                    bail!("{}", ArchiveViolation::SpecialFileType { path: path_str, kind: "block device" });
                }
                tar::EntryType::Fifo => {
                    bail!("{}", ArchiveViolation::SpecialFileType { path: path_str, kind: "FIFO" });
                }
                _ => {
                    bail!("{}", ArchiveViolation::SpecialFileType { path: path_str, kind: "unknown" });
                }
            };

            // SAFETY: duplicate path entries (file-type replacement detection)
            if let Some(prev_kind) = seen_paths.get(&stripped_str) {
                if *prev_kind != kind {
                    let was = match prev_kind {
                        EntryKind::Regular => "regular",
                        EntryKind::Directory => "directory",
                        EntryKind::Symlink => "symlink",
                        EntryKind::Hardlink => "hardlink",
                    };
                    let now = match kind {
                        EntryKind::Regular => "regular",
                        EntryKind::Directory => "directory",
                        EntryKind::Symlink => "symlink",
                        EntryKind::Hardlink => "hardlink",
                    };
                    bail!("{}", ArchiveViolation::TypeReplacement { path: stripped_str, was, now });
                }
                // Same type duplicate — still reject
                if kind != EntryKind::Directory {
                    bail!("{}", ArchiveViolation::DuplicatePath(stripped_str));
                }
                // Duplicate directory entries are OK (tar commonly emits these)
            }
            seen_paths.insert(stripped_str.clone(), kind);

            // SAFETY: symlink/hardlink escape
            if entry_type == tar::EntryType::Symlink || entry_type == tar::EntryType::Link {
                if let Ok(Some(link)) = entry.link_name().map(|l| l.map(|p| p.to_path_buf())) {
                    let link_str = link.to_string_lossy();
                    if link_str.contains("..") || link_str.starts_with('/') {
                        let violation = if entry_type == tar::EntryType::Symlink {
                            ArchiveViolation::SymlinkEscape { path: stripped_str, target: link_str.into() }
                        } else {
                            ArchiveViolation::HardlinkEscape { path: stripped_str, target: link_str.into() }
                        };
                        bail!("{}", violation);
                    }
                }
            }

            // Extract
            let dest = self.extract_dir.join(&stripped);
            match entry_type {
                tar::EntryType::Directory => {
                    std::fs::create_dir_all(&dest)?;
                }
                tar::EntryType::Regular | tar::EntryType::GNUSparse => {
                    if let Some(parent) = dest.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    let mut outfile = std::fs::File::create(&dest)?;
                    std::io::copy(&mut entry, &mut outfile)?;

                    // Post-extraction defense-in-depth: canonicalize destination
                    // and verify it's still under the extraction root.
                    let canonical_dest = dest.canonicalize()
                        .context(format!("cannot canonicalize extracted path: {}", dest.display()))?;
                    let canonical_root = self.extract_dir.canonicalize()
                        .unwrap_or_else(|_| self.extract_dir.clone());
                    if !canonical_dest.starts_with(&canonical_root) {
                        // Remove the escaped file immediately
                        let _ = std::fs::remove_file(&canonical_dest);
                        bail!("extracted file escaped root: {} -> {}",
                            stripped_str, canonical_dest.display());
                    }
                }
                tar::EntryType::Symlink => {
                    if let Ok(Some(link)) = entry.link_name().map(|l| l.map(|p| p.to_path_buf())) {
                        if let Some(parent) = dest.parent() {
                            std::fs::create_dir_all(parent)?;
                        }
                        #[cfg(unix)]
                        std::os::unix::fs::symlink(&link, &dest)?;
                    }
                }
                _ => {} // hardlinks handled above
            }
        }

        Ok(&self.extract_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: full archive safety tests require constructing malicious tarballs.
    // Use the `tar` crate to build test fixtures programmatically.

    #[test]
    fn test_archive_violation_display() {
        let v = ArchiveViolation::PathTraversal("../etc/passwd".into());
        assert!(v.to_string().contains("path traversal"));

        let v = ArchiveViolation::SpecialFileType { path: "dev/null".into(), kind: "char device" };
        assert!(v.to_string().contains("char device"));

        let v = ArchiveViolation::DuplicatePath("config/foo.conf".into());
        assert!(v.to_string().contains("duplicate entry"));
    }
}
```

- [ ] **Step 4: Create RHEL detection with ambient validation**

Create `inspectah-pipeline/src/build/rhel.rs`:
```rust
//! RHEL pass-through detection with ambient subscription validation.

use std::path::Path;

/// Result of RHEL ambient subscription detection.
#[derive(Debug, Clone, PartialEq)]
pub enum AmbientSubscription {
    /// RHEL host with valid ambient subscription
    Available,
    /// RHEL host detected but ambient bundle is incomplete/invalid
    IncompleteBundle { reason: String },
    /// Not a RHEL host (no pass-through path)
    NotAvailable,
}

/// Detect RHEL subscription pass-through and validate the ambient bundle.
///
/// Checks for `/usr/share/rhel/secrets/etc-pki-entitlement` and then validates
/// the ambient bundle against the same four-component contract as scan-side
/// `evaluate_bundle_completeness()`:
/// 1. Serial-matched entitlement cert+key pair
/// 2. rhsm.conf present
/// 3. At least one CA cert
/// 4. `/etc/yum.repos.d/redhat.repo` present (host-managed by subscription-manager)
pub fn detect_ambient_subscription() -> AmbientSubscription {
    let passthrough_marker = Path::new("/usr/share/rhel/secrets/etc-pki-entitlement");
    if !passthrough_marker.exists() {
        return AmbientSubscription::NotAvailable;
    }

    let mut missing = Vec::new();

    // 1. Serial-matched entitlement cert+key pair
    let ent_dir = Path::new("/etc/pki/entitlement");
    if !ent_dir.exists() {
        missing.push("/etc/pki/entitlement directory");
    } else {
        let mut serials: std::collections::BTreeMap<String, (bool, bool)> =
            std::collections::BTreeMap::new();
        if let Ok(entries) = std::fs::read_dir(ent_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if let Some(serial) = name.strip_suffix("-key.pem") {
                    serials.entry(serial.to_string()).or_default().1 = true;
                } else if let Some(serial) = name.strip_suffix(".pem") {
                    serials.entry(serial.to_string()).or_default().0 = true;
                }
            }
        }
        if !serials.values().any(|(cert, key)| *cert && *key) {
            missing.push("serial-matched entitlement cert+key pair");
        }
    }

    // 2. rhsm.conf
    if !Path::new("/etc/rhsm/rhsm.conf").exists() {
        missing.push("rhsm.conf");
    }

    // 3. CA certs
    let ca_dir = Path::new("/etc/rhsm/ca");
    let has_ca = ca_dir.exists() && std::fs::read_dir(ca_dir)
        .ok()
        .map(|entries| entries.filter_map(|e| e.ok()).any(|e| {
            e.file_name().to_string_lossy().ends_with(".pem")
        }))
        .unwrap_or(false);
    if !has_ca {
        missing.push("CA certs in /etc/rhsm/ca/");
    }

    // 4. redhat.repo — host-managed by subscription-manager
    if !Path::new("/etc/yum.repos.d/redhat.repo").exists() {
        missing.push("redhat.repo at /etc/yum.repos.d/redhat.repo");
    }

    if !missing.is_empty() {
        return AmbientSubscription::IncompleteBundle {
            reason: format!("missing: {}", missing.join(", ")),
        };
    }

    AmbientSubscription::Available
}
```

- [ ] **Step 5: Create the build orchestration module**

Create `inspectah-pipeline/src/build/mod.rs`:
```rust
//! Build planning and execution — lives in pipeline, not CLI.
//!
//! CLI handles clap args and terminal output. This module handles:
//! - Tarball extraction with archive safety validation
//! - RHEL pass-through detection with ambient bundle validation
//! - Subscription mount planning
//! - Bundle completeness preflight
//! - Cert expiry checking
//! - Podman command construction
//! - Typed build outcome for exit code mapping

pub mod extract;
pub mod rhel;

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

use self::extract::TarballExtractor;
use self::rhel::{detect_ambient_subscription, AmbientSubscription};

/// Typed build outcome — encodes all exit conditions.
#[derive(Debug)]
pub enum BuildOutcome {
    /// Build succeeded. Includes image tag and digest.
    Success { tag: String, digest: Option<String> },
    /// Dry run — command emitted, nothing executed.
    DryRun { command: String },
    /// Podman not found.
    PodmanNotFound,
    /// Podman build failed with exit code.
    PodmanFailed { exit_code: i32 },
    /// Preflight failed (missing Containerfile, invalid tarball, etc.)
    PreflightFailed { reason: String },
}

impl BuildOutcome {
    /// Map outcome to process exit code per spec contract.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Success { .. } => 0,
            Self::DryRun { .. } => 0,
            Self::PodmanNotFound => 127,  // spec: exit 127 when podman not found
            Self::PodmanFailed { exit_code } => *exit_code,
            Self::PreflightFailed { .. } => 1,
        }
    }
}

/// Build-time warning.
#[derive(Debug)]
pub enum BuildWarning {
    CertExpiringSoon { days_remaining: i64, path: String },
    CertExpired { path: String },
    AmbientBundleIncomplete { reason: String },
    NoSubscriptionData,
}

/// Configuration for a build operation.
pub struct BuildConfig {
    pub tarball: PathBuf,
    pub tag: String,
    pub dry_run: bool,
    pub keep_context: bool,
    pub podman_args: Vec<String>,
}

/// Plan and optionally execute a build.
///
/// Returns the outcome and any warnings. The caller (CLI) is responsible
/// for rendering warnings and the outcome to the terminal.
pub fn plan_and_execute(config: &BuildConfig) -> Result<(BuildOutcome, Vec<BuildWarning>)> {
    let mut warnings = Vec::new();

    // Validate tag format
    if !config.tag.contains(':') || config.tag.ends_with(':') {
        return Ok((BuildOutcome::PreflightFailed {
            reason: format!(
                "tag must include a version: '{}:v1', not '{}'",
                config.tag.split(':').next().unwrap_or(&config.tag),
                config.tag
            ),
        }, warnings));
    }

    // Check podman
    let podman = match find_podman() {
        Some(p) => p,
        None => return Ok((BuildOutcome::PodmanNotFound, warnings)),
    };

    // Extract tarball with full safety validation.
    // Use tempfile::TempDir for automatic cleanup on all exit paths (success, error, panic).
    // Only --keep-context prevents cleanup via into_path().
    let temp_dir = tempfile::tempdir()
        .context("failed to create temporary extraction directory")?;
    let extract_dir = if config.keep_context {
        // Move to named location so user can find it after build
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("inspectah/builds");
        let named_dir = cache_dir.join(format!("build-{}", std::process::id()));
        std::fs::create_dir_all(&named_dir)?;
        // TempDir auto-cleanup still covers the original temp location
        named_dir
    } else {
        // TempDir owns the path — Drop cleans it up automatically
        temp_dir.path().to_path_buf()
    };

    let extractor = TarballExtractor::new(extract_dir.clone());
    if let Err(e) = extractor.extract(&config.tarball) {
        // temp_dir Drop fires here — cleanup is automatic
        return Ok((BuildOutcome::PreflightFailed {
            reason: format!("tarball extraction failed: {e}"),
        }, warnings));
    }

    // Find Containerfile
    let containerfile = extract_dir.join("Containerfile");
    if !containerfile.exists() {
        return Ok((BuildOutcome::PreflightFailed {
            reason: "no Containerfile found in tarball".into(),
        }, warnings));
    }

    // Detect RHEL pass-through with ambient validation (check FIRST — skip tarball validation if RHEL)
    let ambient = detect_ambient_subscription();

    // Detect subscription material and validate bundle completeness
    let sub_dir = extract_dir.join("subscription");
    let has_subscription = match &ambient {
        AmbientSubscription::Available => {
            // RHEL pass-through handles it — skip tarball bundle validation
            false
        }
        _ => {
            // Non-RHEL or incomplete ambient — validate tarball bundle with full four-component check
            match validate_subscription_bundle(&sub_dir) {
                Ok(present) => present,
                Err(e) => {
                    return Ok((BuildOutcome::PreflightFailed {
                        reason: e.to_string(),
                    }, warnings));
                }
            }
        }
    };

    let use_subscription_mounts = match &ambient {
        AmbientSubscription::Available => false, // RHEL pass-through handles it
        AmbientSubscription::IncompleteBundle { reason } => {
            warnings.push(BuildWarning::AmbientBundleIncomplete { reason: reason.clone() });
            has_subscription // fall back to tarball certs (already validated above)
        }
        AmbientSubscription::NotAvailable => has_subscription,
    };

    // Check cert expiry at build time
    if has_subscription && ambient != AmbientSubscription::Available {
        check_cert_expiry_at_build(&sub_dir, &mut warnings);
    }

    if !has_subscription && ambient == AmbientSubscription::NotAvailable {
        warnings.push(BuildWarning::NoSubscriptionData);
    }

    // Build podman command
    let mut cmd_args: Vec<String> = vec!["build".into()];
    cmd_args.push("-t".into());
    cmd_args.push(config.tag.clone());

    if use_subscription_mounts {
        let ent_path = sub_dir.join("entitlement");
        let rhsm_path = sub_dir.join("rhsm");
        let repo_path = sub_dir.join("redhat.repo");

        cmd_args.push("-v".into());
        cmd_args.push(format!("{}:/run/secrets/etc-pki-entitlement:z", ent_path.display()));
        cmd_args.push("-v".into());
        cmd_args.push(format!("{}:/run/secrets/rhsm:z", rhsm_path.display()));
        if repo_path.exists() {
            cmd_args.push("-v".into());
            cmd_args.push(format!("{}:/run/secrets/redhat.repo:z", repo_path.display()));
        }
    }

    cmd_args.extend(config.podman_args.clone());
    cmd_args.push("-f".into());
    cmd_args.push("Containerfile".into());
    cmd_args.push(".".into());

    if config.dry_run {
        let mut full_cmd = format!("cd {}\n{}", extract_dir.display(), podman);
        for arg in &cmd_args {
            if arg.contains(':') || arg.contains(' ') {
                full_cmd.push_str(&format!(" \\\n  {arg}"));
            } else {
                full_cmd.push_str(&format!(" {arg}"));
            }
        }
        return Ok((BuildOutcome::DryRun { command: full_cmd }, warnings));
    }

    // Execute podman build
    let output = Command::new(&podman)
        .args(&cmd_args)
        .current_dir(&extract_dir)
        .output()
        .context("failed to execute podman")?;

    let exit_code = output.status.code().unwrap_or(1);

    // Cleanup is automatic via TempDir Drop (unless --keep-context used named dir).
    // For --keep-context, prevent TempDir from cleaning the original temp location
    // (extraction already happened in the named dir, temp_dir is empty).
    if config.keep_context {
        // Prevent Drop cleanup of the named extraction directory
        // (temp_dir itself is empty/unused in this path and will be cleaned by Drop)
    }

    if exit_code == 0 {
        // Try to extract image digest from podman output
        let stdout = String::from_utf8_lossy(&output.stdout);
        let digest = stdout.lines().last()
            .filter(|l| l.starts_with("sha256:") || l.len() == 64)
            .map(|l| if l.starts_with("sha256:") { l.to_string() } else { format!("sha256:{l}") });

        Ok((BuildOutcome::Success { tag: config.tag.clone(), digest }, warnings))
    } else {
        Ok((BuildOutcome::PodmanFailed { exit_code }, warnings))
    }
}

/// Validate that the extracted subscription bundle has all four required components.
///
/// Uses the SAME completeness rule as scan-side `evaluate_bundle_completeness()`:
/// 1. At least one serial-matched entitlement cert+key pair
/// 2. rhsm.conf present
/// 3. At least one CA cert
/// 4. redhat.repo present
///
/// Returns `Ok(true)` if bundle is complete and usable, `Err` if subscription
/// directory exists but is incomplete (hard error — mount plan must NOT be emitted).
/// Returns `Ok(false)` if no subscription directory exists at all.
fn validate_subscription_bundle(sub_dir: &Path) -> Result<bool> {
    if !sub_dir.exists() {
        return Ok(false);
    }

    let ent_dir = sub_dir.join("entitlement");
    let rhsm_conf = sub_dir.join("rhsm/rhsm.conf");
    let ca_dir = sub_dir.join("rhsm/ca");
    let redhat_repo = sub_dir.join("redhat.repo");

    let mut missing = Vec::new();

    // 1. Check for serial-matched entitlement cert+key pairs
    if !ent_dir.exists() {
        missing.push("entitlement cert+key pair (directory missing)");
    } else {
        let mut serials: std::collections::BTreeMap<String, (bool, bool)> =
            std::collections::BTreeMap::new();
        if let Ok(entries) = std::fs::read_dir(&ent_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if let Some(serial) = name.strip_suffix("-key.pem") {
                    serials.entry(serial.to_string()).or_default().1 = true;
                } else if let Some(serial) = name.strip_suffix(".pem") {
                    serials.entry(serial.to_string()).or_default().0 = true;
                }
            }
        }
        let has_complete_pair = serials.values().any(|(cert, key)| *cert && *key);
        if !has_complete_pair {
            missing.push("entitlement cert+key pair (no serial-matched pair found)");
        }
    }

    // 2. Check rhsm.conf
    if !rhsm_conf.exists() {
        missing.push("rhsm.conf");
    }

    // 3. Check CA certs
    let has_ca = ca_dir.exists() && std::fs::read_dir(&ca_dir)
        .ok()
        .map(|entries| entries.filter_map(|e| e.ok()).any(|e| {
            e.file_name().to_string_lossy().ends_with(".pem")
        }))
        .unwrap_or(false);
    if !has_ca {
        missing.push("CA certs from rhsm/ca/");
    }

    // 4. Check redhat.repo
    if !redhat_repo.exists() {
        missing.push("redhat.repo");
    }

    if !missing.is_empty() {
        bail!(
            "Subscription bundle incomplete — missing: {}. \
             Build cannot proceed without a complete bundle. \
             Re-scan the source host with --preserve-subscription.",
            missing.join(", ")
        );
    }

    Ok(true)
}

/// Check cert expiry at build time. Warn if any cert expires within 14 days.
fn check_cert_expiry_at_build(sub_dir: &Path, warnings: &mut Vec<BuildWarning>) {
    let ent_dir = sub_dir.join("entitlement");
    if !ent_dir.exists() {
        return;
    }

    let entries = match std::fs::read_dir(&ent_dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".pem") || name.ends_with("-key.pem") {
            continue;
        }

        let content = match std::fs::read_to_string(entry.path()) {
            Ok(c) => c,
            Err(_) => continue,
        };

        if let Some(der) = pem_to_der_build(&content) {
            if let Ok((_, cert)) = x509_parser::parse_x509_certificate(&der) {
                let not_after = cert.validity().not_after;
                if let Ok(ts) = not_after.to_datetime() {
                    let expiry = time::OffsetDateTime::from_unix_timestamp(ts.timestamp())
                        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
                    let now = time::OffsetDateTime::now_utc();
                    let days_remaining = (expiry - now).whole_days();

                    if days_remaining < 0 {
                        warnings.push(BuildWarning::CertExpired {
                            path: name.clone(),
                        });
                    } else if days_remaining <= 14 {
                        warnings.push(BuildWarning::CertExpiringSoon {
                            days_remaining,
                            path: name.clone(),
                        });
                    }
                }
            }
        }
    }
}

fn pem_to_der_build(pem_content: &str) -> Option<Vec<u8>> {
    let begin = pem_content.find("-----BEGIN CERTIFICATE-----")?;
    let end = pem_content.find("-----END CERTIFICATE-----")?;
    let b64_start = begin + "-----BEGIN CERTIFICATE-----".len();
    let b64 = &pem_content[b64_start..end].replace(['\n', '\r', ' '], "");
    base64::engine::general_purpose::STANDARD.decode(b64).ok()
}

fn find_podman() -> Option<String> {
    Command::new("which")
        .arg("podman")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_outcome_exit_codes() {
        assert_eq!(BuildOutcome::Success { tag: "x:1".into(), digest: None }.exit_code(), 0);
        assert_eq!(BuildOutcome::DryRun { command: "".into() }.exit_code(), 0);
        assert_eq!(BuildOutcome::PodmanNotFound.exit_code(), 127);
        assert_eq!(BuildOutcome::PodmanFailed { exit_code: 2 }.exit_code(), 2);
        assert_eq!(BuildOutcome::PreflightFailed { reason: "".into() }.exit_code(), 1);
    }

    #[test]
    fn test_validate_bundle_complete() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("subscription");
        // Create all four components
        std::fs::create_dir_all(sub.join("entitlement")).unwrap();
        std::fs::write(sub.join("entitlement/123.pem"), "cert").unwrap();
        std::fs::write(sub.join("entitlement/123-key.pem"), "key").unwrap();
        std::fs::create_dir_all(sub.join("rhsm/ca")).unwrap();
        std::fs::write(sub.join("rhsm/rhsm.conf"), "[rhsm]").unwrap();
        std::fs::write(sub.join("rhsm/ca/redhat-uep.pem"), "ca").unwrap();
        std::fs::write(sub.join("redhat.repo"), "[rhel]").unwrap();

        assert!(validate_subscription_bundle(&sub).unwrap());
    }

    #[test]
    fn test_validate_bundle_mismatched_serials() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("subscription");
        std::fs::create_dir_all(sub.join("entitlement")).unwrap();
        std::fs::write(sub.join("entitlement/111.pem"), "cert").unwrap();
        std::fs::write(sub.join("entitlement/222-key.pem"), "key").unwrap(); // wrong serial
        std::fs::create_dir_all(sub.join("rhsm/ca")).unwrap();
        std::fs::write(sub.join("rhsm/rhsm.conf"), "[rhsm]").unwrap();
        std::fs::write(sub.join("rhsm/ca/ca.pem"), "ca").unwrap();
        std::fs::write(sub.join("redhat.repo"), "[rhel]").unwrap();

        let err = validate_subscription_bundle(&sub).unwrap_err();
        assert!(err.to_string().contains("serial-matched pair"));
    }

    #[test]
    fn test_validate_bundle_missing_rhsm_conf() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("subscription");
        std::fs::create_dir_all(sub.join("entitlement")).unwrap();
        std::fs::write(sub.join("entitlement/123.pem"), "cert").unwrap();
        std::fs::write(sub.join("entitlement/123-key.pem"), "key").unwrap();
        std::fs::create_dir_all(sub.join("rhsm/ca")).unwrap();
        std::fs::write(sub.join("rhsm/ca/ca.pem"), "ca").unwrap();
        std::fs::write(sub.join("redhat.repo"), "[rhel]").unwrap();
        // No rhsm.conf

        let err = validate_subscription_bundle(&sub).unwrap_err();
        assert!(err.to_string().contains("rhsm.conf"));
    }

    #[test]
    fn test_validate_bundle_missing_ca_certs() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("subscription");
        std::fs::create_dir_all(sub.join("entitlement")).unwrap();
        std::fs::write(sub.join("entitlement/123.pem"), "cert").unwrap();
        std::fs::write(sub.join("entitlement/123-key.pem"), "key").unwrap();
        std::fs::create_dir_all(sub.join("rhsm")).unwrap();
        std::fs::write(sub.join("rhsm/rhsm.conf"), "[rhsm]").unwrap();
        std::fs::write(sub.join("redhat.repo"), "[rhel]").unwrap();
        // No CA dir

        let err = validate_subscription_bundle(&sub).unwrap_err();
        assert!(err.to_string().contains("CA certs"));
    }

    #[test]
    fn test_validate_bundle_missing_redhat_repo() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("subscription");
        std::fs::create_dir_all(sub.join("entitlement")).unwrap();
        std::fs::write(sub.join("entitlement/123.pem"), "cert").unwrap();
        std::fs::write(sub.join("entitlement/123-key.pem"), "key").unwrap();
        std::fs::create_dir_all(sub.join("rhsm/ca")).unwrap();
        std::fs::write(sub.join("rhsm/rhsm.conf"), "[rhsm]").unwrap();
        std::fs::write(sub.join("rhsm/ca/ca.pem"), "ca").unwrap();
        // No redhat.repo

        let err = validate_subscription_bundle(&sub).unwrap_err();
        assert!(err.to_string().contains("redhat.repo"));
    }

    #[test]
    fn test_validate_bundle_no_subscription_dir() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("subscription");
        // Directory doesn't exist
        assert!(!validate_subscription_bundle(&sub).unwrap());
    }
}
```

Note: The build module will also need `x509-parser` in `inspectah-pipeline/Cargo.toml` for cert expiry checking at build time:
```toml
x509-parser = "0.16"
```

- [ ] **Step 6: Compile check**

Run: `cargo check -p inspectah-pipeline`
Expected: compiles cleanly

- [ ] **Step 7: Run tests**

Run: `cargo test -p inspectah-pipeline build`
Expected: tests pass

- [ ] **Step 8: Commit**

```bash
git add inspectah-pipeline/src/build/ inspectah-pipeline/src/lib.rs inspectah-pipeline/Cargo.toml
git commit -m "feat(pipeline): add build module with archive safety, RHEL detection, typed outcomes

TarballExtractor with full safety contract (reject special files, dupes,
type replacement, escaping links). BuildOutcome enum for exit code mapping.
Ambient subscription validation before preferring pass-through.
Build-time cert expiry checking (14-day window)."
```

---

### Task 9: CLI — thin `inspectah build` wrapper

**Scope:** `inspectah build` v1 accepts tarball input only. Edited-directory builds use manual `podman build` from the extracted working directory, as documented in the generated README.

**Files:**
- Create: `inspectah-cli/src/commands/build.rs`
- Modify: `inspectah-cli/src/commands/mod.rs`
- Modify: `inspectah-cli/src/main.rs`
- Modify: `inspectah-cli/Cargo.toml`

**R2 changes:** CLI is now thin — delegates to `inspectah_pipeline::build`. Command registered in `main.rs` `Commands` enum, not `commands/mod.rs` (finding #2). Exit code 127 for missing podman handled via `BuildOutcome` (finding #10). Success line includes image identity (finding #10).

- [ ] **Step 1: Add dependency (if not already present)**

In `inspectah-cli/Cargo.toml`, ensure `inspectah-pipeline` is a dependency (it should already be).

- [ ] **Step 2: Create the thin CLI wrapper**

Create `inspectah-cli/src/commands/build.rs`:
```rust
use anyhow::Result;
use clap::Args;
use std::path::PathBuf;

use inspectah_pipeline::build::{self, BuildConfig, BuildOutcome, BuildWarning};

#[derive(Debug, Args)]
pub struct BuildArgs {
    /// Path to inspectah scan output tarball (.tar.gz)
    pub tarball: PathBuf,

    /// Image name and tag (required, format: name:tag)
    #[arg(short, long)]
    pub tag: String,

    /// Show the podman build command without executing
    #[arg(long)]
    pub dry_run: bool,

    /// Preserve extracted build context after build
    #[arg(long)]
    pub keep_context: bool,

    /// Additional arguments passed through to podman build (after --)
    #[arg(last = true)]
    pub podman_args: Vec<String>,
}

pub fn run(args: BuildArgs) -> Result<i32> {
    let config = BuildConfig {
        tarball: args.tarball,
        tag: args.tag,
        dry_run: args.dry_run,
        keep_context: args.keep_context,
        podman_args: args.podman_args,
    };

    let (outcome, warnings) = build::plan_and_execute(&config)?;

    // Render warnings
    for warning in &warnings {
        match warning {
            BuildWarning::CertExpiringSoon { days_remaining, path } => {
                eprintln!("warning: entitlement cert {path} expires in {days_remaining} days");
            }
            BuildWarning::CertExpired { path } => {
                eprintln!("warning: entitlement cert {path} has EXPIRED");
            }
            BuildWarning::AmbientBundleIncomplete { reason } => {
                eprintln!("warning: RHEL detected but ambient subscription incomplete: {reason}");
                eprintln!("  falling back to tarball-carried subscription");
            }
            BuildWarning::NoSubscriptionData => {
                eprintln!("note: no subscription data in tarball. If the Containerfile installs \
                           RHEL packages, re-scan with --preserve-subscription.");
            }
        }
    }

    // Render outcome
    match &outcome {
        BuildOutcome::Success { tag, digest } => {
            let id = digest.as_deref().unwrap_or("unknown");
            eprintln!("build complete: {tag} ({id})");
        }
        BuildOutcome::DryRun { command } => {
            println!("{command}");
        }
        BuildOutcome::PodmanNotFound => {
            eprintln!("error: podman not found in PATH");
        }
        BuildOutcome::PodmanFailed { exit_code } => {
            eprintln!("error: podman build failed (exit code {exit_code})");
            eprintln!("  hint: re-run with --no-cache to retry without layer cache");
        }
        BuildOutcome::PreflightFailed { reason } => {
            eprintln!("error: {reason}");
        }
    }

    Ok(outcome.exit_code())
}
```

- [ ] **Step 3: Register the module and command**

In `inspectah-cli/src/commands/mod.rs`, add:
```rust
pub mod build;
```

In `inspectah-cli/src/main.rs`, add to the `Commands` enum:
```rust
    /// Build a container image from an inspectah scan tarball
    Build(commands::build::BuildArgs),
```

Add to the `match command` block:
```rust
        Commands::Build(args) => {
            let code = commands::build::run(args)?;
            if code != 0 {
                std::process::exit(code);
            }
        }
```

Note: The match for `Build` should follow the same error-handling pattern as the existing `Scan` variant.

- [ ] **Step 4: Compile check + test**

Run: `cargo check -p inspectah-cli && cargo clippy -p inspectah-cli -- -D warnings`
Expected: compiles, clippy clean

- [ ] **Step 5: Commit**

```bash
git add inspectah-cli/src/commands/build.rs inspectah-cli/src/commands/mod.rs inspectah-cli/src/main.rs
git commit -m "feat(cli): add thin inspectah build wrapper delegating to pipeline

Exit 127 for missing podman, success line includes image digest,
build-time cert expiry warnings rendered."
```

---

### Task 10: Web API — --ack-sensitive header rename + structural test

**Files:**
- Modify: `inspectah-web/src/handlers.rs`
- Modify: `inspectah-web/src/lib.rs`

**R2 changes:** Added structural test for header/CORS alignment (finding #13).

- [ ] **Step 1: Update handler to accept both headers**

In `inspectah-web/src/handlers.rs`, change the header check:

From:
```rust
.get("x-acknowledge-sensitive")
```
To:
```rust
.get("x-ack-sensitive")
.or_else(|| headers.get("x-acknowledge-sensitive"))
```

- [ ] **Step 2: Update error message**

Change the error string:
```rust
"error": "session contains sensitive data — set x-ack-sensitive: true to export",
```

- [ ] **Step 3: Update CORS config**

In `inspectah-web/src/lib.rs`, add the new header name to the CORS allow list:
```rust
axum::http::HeaderName::from_static("x-ack-sensitive"),
axum::http::HeaderName::from_static("x-acknowledge-sensitive"),
```

- [ ] **Step 4: Add structural test for header/CORS alignment**

Add a compile-time or test-time assertion that the accepted header names and CORS allow list stay in sync:
```rust
#[cfg(test)]
mod header_sync_tests {
    /// Structural test: the CORS allow-list and handler header-check must
    /// accept the same set of sensitive-data header names.
    #[test]
    fn test_ack_sensitive_header_names_in_sync() {
        // These constants should be extracted from the handler and CORS config
        // so this test breaks if one is updated without the other.
        let handler_headers = ["x-ack-sensitive", "x-acknowledge-sensitive"];
        let cors_headers = super::cors_allowed_header_names();
        for h in &handler_headers {
            assert!(
                cors_headers.contains(&h.to_string()),
                "Handler accepts {h} but CORS does not allow it"
            );
        }
    }
}
```

This may require extracting the header name list into a constant or function. Adjust to match the actual code structure.

- [ ] **Step 5: Compile check + test**

Run: `cargo check -p inspectah-web && cargo test -p inspectah-web`
Expected: compiles, all pass

- [ ] **Step 6: Commit**

```bash
git add inspectah-web/src/handlers.rs inspectah-web/src/lib.rs
git commit -m "feat(web): rename ack-sensitive header, add structural alignment test"
```

---

### Task 11: Containerfile comment block, secrets-review (with subscription-only case), and README

**Files:**
- Modify: `inspectah-pipeline/src/render/containerfile.rs`
- Modify: `inspectah-pipeline/src/render/secrets.rs`
- Modify: `inspectah-pipeline/src/render/readme.rs`

**R2 changes:** Fixed secrets-review to render subscription material even when `snap.redactions` is empty (finding #11 — the current renderer returns early with "No redactions recorded" when redactions is empty, hiding subscription disclosure). Added README build instruction task (finding #12). Secrets-review table matches spec shape with distinct "Repo definitions" row (finding #5/Thorn).

- [ ] **Step 1: Add subscription mount comment block to Containerfile renderer**

In the Containerfile renderer, when `snap.preserved_subscription` is true, emit a comment block:
```rust
    if snap.preserved_subscription {
        lines.push("# === RHEL Subscription ===".into());
        lines.push("# This build requires RHEL entitlement certificates for repo access.".into());
        lines.push("# Build with:".into());
        lines.push("#   inspectah build <tarball> -t <name:tag>".into());
        lines.push("# Or manually:".into());
        lines.push("#   podman build \\".into());
        lines.push("#     -v ./subscription/entitlement:/run/secrets/etc-pki-entitlement:z \\".into());
        lines.push("#     -v ./subscription/rhsm:/run/secrets/rhsm:z \\".into());
        lines.push("#     -v ./subscription/redhat.repo:/run/secrets/redhat.repo:z \\".into());
        lines.push("#     -f Containerfile .".into());
        lines.push(String::new());
    }
```

- [ ] **Step 2: Fix secrets-review for subscription-only case**

In `secrets.rs`, the current `render_secrets_review` returns early with "No redactions recorded" when `snap.redactions.is_empty()`. This hides subscription material when it's the ONLY sensitive content.

Refactor: move the subscription section BEFORE the early return, or restructure so the early return only fires when there are no redactions AND no subscription:
```rust
pub fn render_secrets_review(snap: &InspectionSnapshot) -> String {
    let mut lines = vec!["# Secrets Review".into(), String::new()];

    // Subscription material section — renders regardless of redaction count
    if snap.preserved_subscription {
        if let Some(ref sub) = snap.subscription {
            lines.push("## Subscription Material (preserved by --preserve-subscription)".into());
            lines.push(String::new());
            lines.push("| Type | Count | Expiry | Paths |".into());
            lines.push("|------|-------|--------|-------|".into());

            let cert_count = sub.entitlement_certs.iter()
                .filter(|f| !f.path.ends_with("-key.pem"))
                .count();
            let key_count = sub.entitlement_certs.iter()
                .filter(|f| f.path.ends_with("-key.pem"))
                .count();
            let expiry_str = sub.earliest_expiry
                .map(|e| e.format(&time::format_description::well_known::Rfc3339).unwrap_or_default())
                .unwrap_or_else(|| "unknown".into());

            lines.push(format!(
                "| Entitlement certs | {} ({} keys) | {} | subscription/entitlement/*.pem |",
                cert_count, key_count, expiry_str
            ));
            lines.push(format!(
                "| CA certs | {} | — | subscription/rhsm/ca/*.pem |",
                sub.ca_certs.len()
            ));

            let rhsm_conf_count = sub.config_files.iter()
                .filter(|f| f.path.contains("rhsm.conf")).count();
            lines.push(format!(
                "| Config files | {} | — | subscription/rhsm/rhsm.conf |",
                rhsm_conf_count
            ));

            let redhat_repo_count = sub.config_files.iter()
                .filter(|f| f.path.contains("redhat.repo")).count();
            lines.push(format!(
                "| Repo definitions | {} | — | subscription/redhat.repo |",
                redhat_repo_count
            ));

            if sub.incomplete {
                lines.push("| **Status** | **INCOMPLETE** | | Missing required components |".into());
            }
            lines.push(String::new());
        }
    }

    // Existing redaction sections follow...
    if snap.redactions.is_empty() && !snap.preserved_subscription {
        lines.push("No redactions recorded.".into());
        return lines.join("\n");
    }

    // ... rest of existing redaction rendering ...
```

- [ ] **Step 3: Update README renderer with build instructions**

In `inspectah-pipeline/src/render/readme.rs`, when `snap.preserved_subscription` is true, add build instruction section:
```rust
    if snap.preserved_subscription {
        lines.push("## Building with Subscription".into());
        lines.push(String::new());
        lines.push("This tarball includes RHEL subscription material for building on non-RHEL hosts.".into());
        lines.push(String::new());
        lines.push("```bash".into());
        lines.push("# Recommended: use inspectah build".into());
        lines.push("inspectah build <this-tarball> -t <name:tag>".into());
        lines.push(String::new());
        lines.push("# Or manually with podman:".into());
        lines.push("podman build \\".into());
        lines.push("  -v ./subscription/entitlement:/run/secrets/etc-pki-entitlement:z \\".into());
        lines.push("  -v ./subscription/rhsm:/run/secrets/rhsm:z \\".into());
        lines.push("  -v ./subscription/redhat.repo:/run/secrets/redhat.repo:z \\".into());
        lines.push("  -f Containerfile .".into());
        lines.push("```".into());
        lines.push(String::new());
        lines.push("On RHEL hosts, subscription pass-through is automatic — no `-v` flags needed.".into());
        lines.push(String::new());
    }
```

- [ ] **Step 4: Add test for subscription-only secrets-review**

```rust
    #[test]
    fn test_secrets_review_subscription_only() {
        let mut snap = InspectionSnapshot::new();
        snap.preserved_subscription = true;
        snap.subscription = Some(SubscriptionSection {
            entitlement_certs: vec![
                SubscriptionFile { path: "123.pem".into(), content: "c".into(), size_bytes: 1, cert_expiry: None },
                SubscriptionFile { path: "123-key.pem".into(), content: "k".into(), size_bytes: 1, cert_expiry: None },
            ],
            ca_certs: vec![
                SubscriptionFile { path: "ca.pem".into(), content: "ca".into(), size_bytes: 1, cert_expiry: None },
            ],
            config_files: vec![
                SubscriptionFile { path: "rhsm.conf".into(), content: "cfg".into(), size_bytes: 1, cert_expiry: None },
                SubscriptionFile { path: "redhat.repo".into(), content: "repo".into(), size_bytes: 1, cert_expiry: None },
            ],
            ..Default::default()
        });
        // No redactions — subscription is the only sensitive content
        let md = render_secrets_review(&snap);
        assert!(md.contains("Subscription Material"));
        assert!(md.contains("Entitlement certs"));
        assert!(md.contains("Repo definitions"));
        assert!(!md.contains("No redactions recorded"));
    }
```

- [ ] **Step 5: Add focused README subscription test**

In `inspectah-pipeline/src/render/readme.rs`, add to the test module:
```rust
    #[test]
    fn test_readme_subscription_build_instructions() {
        use inspectah_core::types::subscription::{SubscriptionFile, SubscriptionSection};

        let mut snap = InspectionSnapshot::new();
        snap.preserved_subscription = true;
        snap.subscription = Some(SubscriptionSection {
            entitlement_certs: vec![SubscriptionFile {
                path: "/etc/pki/entitlement/123.pem".into(),
                content: "c".into(),
                size_bytes: 1,
                cert_expiry: None,
            }],
            config_files: vec![SubscriptionFile {
                path: "/etc/yum.repos.d/redhat.repo".into(),
                content: "r".into(),
                size_bytes: 1,
                cert_expiry: None,
            }],
            ..Default::default()
        });

        let md = render_readme(&snap);
        assert!(md.contains("## Building with Subscription"),
            "README must contain subscription build section header");
        assert!(md.contains("inspectah build"),
            "README must reference inspectah build command");
        assert!(md.contains("subscription/redhat.repo"),
            "README must reference subscription/redhat.repo mount");
    }
```

- [ ] **Step 6: Compile check + test**

Run: `cargo check -p inspectah-pipeline && cargo test -p inspectah-pipeline`
Expected: compiles, all pass

- [ ] **Step 7: Commit**

```bash
git add inspectah-pipeline/src/render/containerfile.rs inspectah-pipeline/src/render/secrets.rs inspectah-pipeline/src/render/readme.rs
git commit -m "feat(pipeline): subscription in Containerfile, secrets-review, and README

Secrets-review renders subscription even when no other redactions exist.
Distinct Repo definitions row per spec. README includes build instructions.
Focused README test asserts subscription-specific content."
```

---

### Task 12: Integration — wire collect pipeline + handle_result

**Files:**
- Modify: `inspectah-pipeline/src/collect.rs`

- [ ] **Step 1: Add SectionData::Subscription routing**

Find the `route_section` match on `SectionData` variants (line ~417 in `collect.rs`). Add:
```rust
SectionData::Subscription(section) => {
    snapshot.subscription = Some(section);
}
```

- [ ] **Step 2: Update wave classifier for Subscription**

The current `is_wave2()` function at line 402 is:
```rust
fn is_wave2(id: InspectorId) -> bool {
    !matches!(id, InspectorId::Rpm)
}
```

This puts ALL non-RPM inspectors in wave 2 (behind RPM). The `SubscriptionInspector` is standalone -- it reads filesystem paths, not RPM data. It has no dependency on `ctx.rpm_state` and its spec says "standalone inspector -- no dependency on other inspectors." It should run in wave 1 alongside RPM for faster collection.

Change to:
```rust
fn is_wave2(id: InspectorId) -> bool {
    !matches!(id, InspectorId::Rpm | InspectorId::Subscription)
}
```

This makes Subscription wave 1 (parallel with RPM), which is correct because it never reads `ctx.rpm_state`.

- [ ] **Step 3: Compile check**

Run: `cargo check --workspace`
Expected: compiles cleanly (this is the first workspace-wide check — catches any cross-crate issues)

- [ ] **Step 3: Run full test suite**

Run: `cargo test --workspace`
Expected: all pass

- [ ] **Step 4: Run clippy on all crates**

Run: `cargo clippy --workspace -- -D warnings`
Expected: clean

- [ ] **Step 5: Commit**

```bash
git add inspectah-pipeline/src/collect.rs
git commit -m "feat: wire subscription inspector into collect pipeline"
```

---

### Task 13: Integration tests

**Files:**
- Tests in relevant crate test modules

- [ ] **Step 1: Scan pipeline integration test**

Test that the full pipeline (scan with mock executor having subscription files -> snapshot -> tarball) produces a tarball containing `subscription/`.

- [ ] **Step 2: Build dry-run integration test**

Test that `plan_and_execute` with `dry_run: true` on a tarball with subscription data produces command output with `-v` mounts on non-RHEL.

- [ ] **Step 3: Fleet merge integration test**

Test that fleet merge of multiple snapshots with subscription picks the latest expiry using typed comparison.

- [ ] **Step 4: Archive safety integration tests**

Test `TarballExtractor` with programmatically constructed malicious tarballs:
- Path traversal (`../etc/passwd`)
- Absolute path (`/etc/shadow`)
- Special file type (FIFO)
- Duplicate path entries
- File-type replacement (file then symlink at same path)
- Symlink escaping root
- Hard link escaping root

- [ ] **Step 5: Fleet --ack-sensitive gate test**

Test that fleet aggregate refuses when input contains sensitive snapshots and no `--ack-sensitive`.

- [ ] **Step 6: Run full test suite**

Run: `cargo test --workspace`
Run: `cargo clippy --workspace -- -D warnings`
Expected: all pass, clean

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "test: add integration tests for subscription, build safety, and fleet gate"
```

---

## Self-Review Checklist

- [x] **Spec coverage:** All spec sections have corresponding tasks:
  - SubscriptionInspector (Task 3)
  - Schema changes (Tasks 1-2)
  - Tarball staging (Task 4)
  - Fleet merge with typed timestamps (Task 5)
  - CLI scan flags (Task 6)
  - Fleet --ack-sensitive gate (Task 7) — **R2 new**
  - Build pipeline module (Task 8) — **R2 restructured from CLI to pipeline**
  - CLI build wrapper (Task 9) — **R2 thin wrapper**
  - Web API rename + structural test (Task 10)
  - Containerfile + secrets-review + README (Task 11)
  - Pipeline wiring (Task 12)
  - Integration tests (Task 13)

- [x] **R1 finding coverage:**
  - #1 Fleet --ack-sensitive: Task 7
  - #2 Stale code sketches: All tasks revised with verified API shapes
  - #3 Expiry format: Tasks 1, 3, 5 use typed `time::OffsetDateTime`
  - #4 Serial-number cert/key pairing: Tasks 1, 3
  - #5 Build logic in pipeline: Tasks 8, 9
  - #6 Archive safety contract: Task 8 (`TarballExtractor`)
  - #7 Symlink restriction: Task 3 (approved roots only)
  - #8 RHEL pass-through validation: Task 8 (`AmbientSubscription`)
  - #9 Bundle completeness preflight: Task 8 (`validate_subscription_bundle`)
  - #10 Build contract outputs: Tasks 8, 9 (`BuildOutcome` enum)
  - #11 secrets-review subscription-only: Task 11
  - #12 README sync: Task 11
  - #13 Web header/CORS structural test: Task 10
  - #14 Fleet provenance hostname: Tasks 1, 3, 5

- [x] **R2 finding coverage:**
  - #1 Build-side bundle validation: Task 8 — `validate_subscription_bundle()` and `detect_ambient_subscription()` now enforce same four-component contract as scan-side (full symmetry — all four components on all paths, no exemptions). Hard errors for missing components, mount plan gated on full validation. Serial-matched pair check, rhsm.conf, CA certs, redhat.repo. Tests for each missing-component case + mismatched serials.
  - #2 Collection-time symlink boundary: Task 3 — `collect_dir_pems()` uses `std::fs::canonicalize()` instead of lexical `starts_with()`. `collect_single_file()` now also validates symlinks against approved roots.
  - #3 Extraction cleanup/retention: Task 8 — uses `tempfile::tempdir()` for automatic Drop cleanup on all exit paths. `--keep-context` extracts to named dir, prevents Drop on content.
  - #4 Fleet merge tiebreak: Task 5 — `max_by` comparator uses `.then_with(|| hostname_of(b).cmp(&hostname_of(a)))` on the `Some(ea) == Some(eb)` arm. Reversed direction so lexicographically smallest hostname wins (R4 fix).
  - #5 SubscriptionInspector wave placement: Task 12 — `is_wave2()` updated to `!matches!(id, InspectorId::Rpm | InspectorId::Subscription)`. Subscription is standalone (no RPM dependency), runs in wave 1.
  - #6 Hostname sourcing: Task 3 — replaced `ctx.hostname.clone()` with `exec.read_file(...)` reading `/etc/hostname`, matching `collect.rs` line 211 pattern. `InspectionContext` has no `hostname` field.
  - #7 Tarball-only scope: Task 9 — explicit scope statement in task header.
  - #8 Extraction path checks: Task 8 — post-extraction `canonicalize()` on regular file destinations, verify under extraction root, remove escaped files.
  - #9 README test: Task 11 — focused `test_readme_subscription_build_instructions()` asserts `## Building with Subscription`, `inspectah build`, and `subscription/redhat.repo` mount reference.

- [x] **Placeholder scan:** No TBD/TODO/FIXME in tasks

- [x] **API shape verification:** Every code sketch uses verified types:
  - `SourceSystemKind::PackageBased` (not `PackageMode`)
  - `Warning { inspector: ..., message: ..., ..Default::default() }` (not `Warning::new(...)`)
  - `merge_snapshots(vec![...], manifest)` returning `(merged, warnings)` (not `&[...]`)
  - `Commands` enum in `main.rs` (not `commands/mod.rs`)
  - Pipeline modules: `collect`, `orchestrate`, `redaction`, `render`, `validate` + new `build`
  - `InspectionContext` fields: `source_system`, `executor`, `rpm_state`, `baseline_data` — NO `hostname` field (R3 verified)
  - Hostname: obtained via `executor.read_file("/etc/hostname")` matching `collect.rs` line 211 (R3 verified)
  - `is_wave2()`: `!matches!(id, InspectorId::Rpm)` — Subscription added to wave 1 exemption (R3 verified)
  - `Executor` trait: `read_link(&self, path: &Path) -> io::Result<String>`, `host_root(&self) -> &Path` (R3 verified)

- [x] **Proof discipline:** Every task includes `cargo check -p <crate>` or `cargo test -p <crate>`

- [x] **Type consistency:** `SubscriptionFile`, `SubscriptionSection`, `EntitlementPair`, `InspectorId::Subscription`, `SectionData::Subscription`, `BuildOutcome`, `AmbientSubscription`, `TarballExtractor`, `ArchiveViolation` used consistently

- [x] **Dependency order:** Tasks 1-2 (core types) → Task 3 (inspector) → Tasks 4-7 (staging, fleet, CLI scan/fleet) → Task 8 (build pipeline) → Task 9 (build CLI) → Tasks 10-11 (web, rendering) → Task 12 (wiring) → Task 13 (integration). Tasks 4-7 and 10-11 are internally parallel.

---

## Deferred Items

These items are explicitly NOT in this plan. Backlog references for future work:

1. **Secrets safety net** — Full secrets-review audit and automated detection of new sensitive data categories. See separate design spec (referenced in `project_inspectah_secrets_safety_net.md`).

2. **`--prefer-host-subscription` override flag** — The spec assumes ambient pass-through is preferred when the host IS subscribed. Task 8 validates the ambient bundle, but a user override to force tarball certs over ambient is not included. File as enhancement if requested.

3. **Hardlink extraction** — `TarballExtractor` in Task 8 rejects escaping hardlinks but does not extract within-root hardlinks. inspectah tarballs don't use hardlinks today, so this is deferred. If a future tarball format uses them, add extraction support.

4. **Spec/plan provenance alignment** — Spec text says `source_hostname` belongs in fleet metadata; plan stores it in `SubscriptionSection.source_hostname` (Task 1 contract decision). The plan's placement is correct (provenance with the data it describes), but the spec needs a text update to align. Non-blocking.

---

## Revision History

**R4:** Revised to address two remaining R3 findings from Tang and Thorn. 2026-05-29.

Changes:
- **Task 5 (Fleet tiebreak):** Reversed hostname comparator direction in `max_by` — `hostname_of(b).cmp(&hostname_of(a))` instead of `hostname_of(a).cmp(&hostname_of(b))`. The old direction made the lexicographically largest hostname win, contradicting the test that expects the smallest (host-alpha) to win. Fixed in both `Some == Some` and `None, None` arms.
- **Task 8 (Ambient redhat.repo):** `detect_ambient_subscription()` now checks `/etc/yum.repos.d/redhat.repo` presence. Previously skipped on ambient path — now full four-component symmetry with scan-side and tarball-side validation.
- **Deferred Items:** Added provenance alignment note — spec text says fleet metadata for source hostname, plan uses `SubscriptionSection.source_hostname`. Spec update needed.
- **Self-review checklist:** Updated R2 findings #1 and #4 to reflect R4 fixes.

**R3:** Revised to address R2 findings from Tang, Thorn, Slate, Seal. 2026-05-29.

Major changes:
- **Task 3:** `collect_dir_pems()` uses `std::fs::canonicalize()` on resolved symlink targets instead of lexical `starts_with()`. `collect_single_file()` now also validates symlinks. Hostname sourced from `executor.read_file("/etc/hostname")`, not nonexistent `ctx.hostname`. Wave placement note added.
- **Task 5:** Fleet merge `max_by` comparator uses `.then_with()` for hostname tiebreak on equal expiries (was missing, returned `Equal` on `Some(ea) == Some(eb)`). R4: comparator direction reversed so smallest hostname wins.
- **Task 8:** `validate_subscription_bundle()` returns `Result<bool>` (hard error on incomplete bundle), enforces same four-component contract as scan-side: serial-matched pairs, rhsm.conf, CA certs, redhat.repo. `detect_ambient_subscription()` enforces the same full four-component check including `redhat.repo`. Extraction uses `tempfile::TempDir` for Drop-guard cleanup. Post-extraction `canonicalize()` defense-in-depth on regular file destinations. Added `tempfile` dependency. 6 new bundle validation tests (complete, mismatched serials, missing each component, no dir). Explicit contract decisions and tarball-only scope statement in task header.
- **Task 9:** Explicit tarball-only scope statement added to task header.
- **Task 11:** Added focused `test_readme_subscription_build_instructions()` asserting subscription-specific build content.
- **Task 12:** `is_wave2()` updated to include `InspectorId::Subscription` in wave 1 (standalone, no RPM dependency).
- Self-review checklist updated with R2 finding-to-task mapping.

**R2:** Revised to address R1 review findings from Tang, Thorn, Slate, Seal. 2026-05-29.

Major changes:
- **Task 1:** Added `EntitlementPair` type for serial-number cert/key matching. Changed `cert_expiry` from `Option<String>` to `Option<time::OffsetDateTime>` with RFC 3339 serde. Added `source_hostname` for fleet provenance.
- **Task 3:** Fixed `SourceSystemKind::PackageMode` to `PackageBased`. Replaced `Warning::new()` with struct literal. Restricted symlink resolution to approved subscription roots. Changed completeness to use serial-matched `EntitlementPair`.
- **Task 5:** Fixed `merge_snapshots` call signature. Changed expiry comparison from string to typed `time::OffsetDateTime`. Added source hostname to merge output.
- **Task 7:** NEW — Fleet CLI `--ack-sensitive` flag and export gate.
- **Task 8:** NEW (replaces old Task 7) — Build logic moved from CLI to `inspectah-pipeline/src/build/`. Created `TarballExtractor` with full archive safety contract (reject special files, duplicates, type replacement, escaping links). Created `AmbientSubscription` enum with validation. Added `BuildOutcome` typed exit codes. Added build-time cert expiry checking. Added bundle completeness preflight.
- **Task 9:** NEW — Thin CLI wrapper delegating to pipeline. Command registered in `main.rs` `Commands` enum.
- **Task 10:** Added structural test for header/CORS alignment.
- **Task 11:** Fixed secrets-review to render subscription when no other redactions exist. Added distinct "Repo definitions" row. Added README build instruction task.
- **Task 13:** Added archive safety integration tests and fleet --ack-sensitive gate test.
- All code sketches verified against current codebase API shapes.
- Added `cargo check -p <crate>` proof step to every task.
