# Phase 6: Base Image Selection & Baseline Extraction — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace hardcoded Containerfile FROM with auto-detected/overridable base image, extract real baseline packages from the target image, and upgrade classification to use accurate baseline data.

**Architecture:** New `baseline` modules in `inspectah-core` (pure resolution/normalization logic) and `inspectah-collect` (image pull + extraction via nsenter). Snapshot schema adds top-level `target_image` and `baseline` fields independent of `RpmSection`. Classification matrix in `inspectah-refine` upgraded with exhaustive `PackageState × baseline mode` coverage. `inspectah-refine` is the canonical owner of derived baseline summary. Pipeline order: resolve → normalize ref → pull + extract → scan host.

**Tech Stack:** Rust (edition 2021), serde/serde_json for schema, clap for CLI, axum for web API, existing `Executor` trait for host commands. Workspace version `0.8.0-alpha.1`, current `SCHEMA_VERSION = 14` (bumps to 15).

**Spec:** `docs/specs/proposed/2026-05-17-phase6-base-image-selection-design.md` (revision 3, approved round 3)

**Execution:** SDD cadence — Tang implements, Thorn code quality review.

---

## File Map

### New files

| File | Responsibility |
|------|---------------|
| `inspectah-core/src/baseline.rs` | `BaseImageResolution`, `ResolutionStrategy`, `NormalizedImageRef`, `BaselineData`, `BaselinePackageEntry`, `IncompatibleServiceEntry`, `resolve_base_image()`, `normalize_image_ref()`, incompatible services constant |
| `inspectah-collect/src/baseline.rs` | `extract_baseline()` — nsenter + podman orchestration with entrypoint override, container lifecycle, NEVRA parsing |
| `inspectah-collect/tests/baseline_test.rs` | Extraction tests with mock executor: NEVRA parsing, command ordering, cleanup on failure, mixed-arch |
| `inspectah-refine/src/baseline_summary.rs` | `BaselineSummary` derivation from refine session state |

### Modified files

| File | Changes |
|------|---------|
| `inspectah-core/src/lib.rs` | Add `pub mod baseline;` |
| `inspectah-core/src/snapshot.rs` | Add `target_image` and `baseline` fields to `InspectionSnapshot`, bump `SCHEMA_VERSION` to 15, update `migrate()` |
| `inspectah-collect/src/lib.rs` | Add `pub mod baseline;` |
| `inspectah-collect/src/executor/mock.rs` | Add command-order recording (`Vec<String>`) for proof seam |
| `inspectah-refine/src/lib.rs` | Add `pub mod baseline_summary;` |
| `inspectah-refine/src/attention.rs` | Add `PackageUserAdded` → Routine, `PackageVersionChanged` → NeedsReview, `ServiceImageModeIncompatible` attention reasons; update classification with exhaustive matrix |
| `inspectah-refine/src/normalize.rs` | Baseline-aware service filtering (incompatible service exclusion) |
| `inspectah-refine/src/session.rs` | Materialize `BaselineData` into session, derive `BaselineSummary` |
| `inspectah-pipeline/src/render/containerfile.rs` | Replace `base_image_from_snapshot()` to read `target_image` field, remove hardcoded fallback |
| `inspectah-cli/src/commands/scan.rs` | Add `--base-image` and `--no-baseline` flags, wire resolution + extraction before host scan, progress output |
| `inspectah-web/src/handlers.rs` | Add `baseline_summary` to `ViewResponse`, serialize from `RefineSession` |

---

## Task 1: Core Baseline Types

**Files:**
- Create: `inspectah-core/src/baseline.rs`
- Modify: `inspectah-core/src/lib.rs`

- [ ] **Step 1: Write type definition tests**

In `inspectah-core/src/baseline.rs`, add the module with types and inline tests:

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResolutionStrategy {
    CliOverride,
    UniversalBlue,
    BootcStatus,
    FedoraAtomicDesktop,
    OsRelease,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BaseImageResolution {
    pub image_ref: String,
    pub strategy: ResolutionStrategy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedImageRef {
    ref_string: String,
}

impl NormalizedImageRef {
    pub fn as_str(&self) -> &str {
        &self.ref_string
    }
}

impl std::fmt::Display for NormalizedImageRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.ref_string)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BaselinePackageEntry {
    pub name: String,
    #[serde(default)]
    pub epoch: Option<String>,
    pub version: String,
    pub release: String,
    pub arch: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BaselineData {
    pub resolution: BaseImageResolution,
    pub normalized_ref: NormalizedImageRef,
    pub image_digest: String,
    pub packages: HashMap<String, BaselinePackageEntry>,
    pub extracted_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TargetImageIdentity {
    pub image_ref: String,
    pub strategy: ResolutionStrategy,
}

pub struct IncompatibleServiceEntry {
    pub unit: &'static str,
    pub reason: &'static str,
}

pub const INCOMPATIBLE_SERVICES: &[IncompatibleServiceEntry] = &[
    IncompatibleServiceEntry {
        unit: "dnf-makecache.service",
        reason: "package-manager service incompatible with immutable /usr",
    },
    IncompatibleServiceEntry {
        unit: "dnf-makecache.timer",
        reason: "package-manager timer incompatible with immutable /usr",
    },
    IncompatibleServiceEntry {
        unit: "packagekit.service",
        reason: "package-manager service incompatible with immutable /usr",
    },
    IncompatibleServiceEntry {
        unit: "packagekit-offline-update.service",
        reason: "package-manager service incompatible with immutable /usr",
    },
];

#[derive(Debug, thiserror::Error)]
pub enum ResolutionError {
    #[error("Universal Blue metadata at {path} is malformed: {reason}")]
    MalformedUblueMetadata { path: String, reason: String },
    #[error("no bootc base image mapping for OS ID={id}")]
    UnknownDistro { id: String },
    #[error("base image resolution failed: {0}")]
    NoResolution(String),
}

#[derive(Debug, thiserror::Error)]
pub enum NormalizationError {
    #[error("image ref is empty")]
    Empty,
    #[error("image ref contains invalid characters: {0}")]
    InvalidCharacters(String),
    #[error("image ref must be fully qualified with a registry component: {0}")]
    NotFullyQualified(String),
    #[error("local-only image ref not supported for baseline extraction: {0}")]
    LocalOnly(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_data_roundtrip() {
        let data = BaselineData {
            resolution: BaseImageResolution {
                image_ref: "registry.redhat.io/rhel9/rhel-bootc:9.6".into(),
                strategy: ResolutionStrategy::OsRelease,
            },
            normalized_ref: NormalizedImageRef {
                ref_string: "registry.redhat.io/rhel9/rhel-bootc:9.6".into(),
            },
            image_digest: "sha256:abc123".into(),
            packages: HashMap::from([(
                "bash.x86_64".into(),
                BaselinePackageEntry {
                    name: "bash".into(),
                    epoch: Some("0".into()),
                    version: "5.2.26".into(),
                    release: "3.el9".into(),
                    arch: "x86_64".into(),
                },
            )]),
            extracted_at: "2026-05-17T01:00:00Z".into(),
        };
        let json = serde_json::to_string(&data).unwrap();
        let parsed: BaselineData = serde_json::from_str(&json).unwrap();
        assert_eq!(data, parsed);
    }

    #[test]
    fn resolution_strategy_serde() {
        assert_eq!(
            serde_json::to_string(&ResolutionStrategy::OsRelease).unwrap(),
            "\"os-release\""
        );
        assert_eq!(
            serde_json::to_string(&ResolutionStrategy::CliOverride).unwrap(),
            "\"cli-override\""
        );
        assert_eq!(
            serde_json::to_string(&ResolutionStrategy::FedoraAtomicDesktop).unwrap(),
            "\"fedora-atomic-desktop\""
        );
    }

    #[test]
    fn incompatible_services_list() {
        let units: Vec<&str> = INCOMPATIBLE_SERVICES.iter().map(|s| s.unit).collect();
        assert!(units.contains(&"dnf-makecache.service"));
        assert!(units.contains(&"dnf-makecache.timer"));
        assert!(units.contains(&"packagekit.service"));
        assert!(units.contains(&"packagekit-offline-update.service"));
        assert_eq!(units.len(), 4);
    }

    #[test]
    fn target_image_identity_roundtrip() {
        let ti = TargetImageIdentity {
            image_ref: "registry.redhat.io/rhel9/rhel-bootc:9.6".into(),
            strategy: ResolutionStrategy::OsRelease,
        };
        let json = serde_json::to_string(&ti).unwrap();
        let parsed: TargetImageIdentity = serde_json::from_str(&json).unwrap();
        assert_eq!(ti, parsed);
    }
}
```

- [ ] **Step 2: Add module to lib.rs**

In `inspectah-core/src/lib.rs`, add:
```rust
pub mod baseline;
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p inspectah-core -- baseline`
Expected: all 4 tests pass.

- [ ] **Step 4: Commit**

```bash
git add inspectah-core/src/baseline.rs inspectah-core/src/lib.rs
git commit -m "feat(core): add Phase 6 baseline types and incompatible services constant"
```

---

## Task 2: Base Image Resolution Chain

**Files:**
- Modify: `inspectah-core/src/baseline.rs`

- [ ] **Step 1: Write resolution tests**

Add to the `tests` module in `inspectah-core/src/baseline.rs`:

```rust
use super::*;
use crate::types::os::OsRelease;

fn os_release(id: &str, version_id: &str, variant_id: &str) -> OsRelease {
    OsRelease {
        id: id.into(),
        version_id: version_id.into(),
        variant_id: variant_id.into(),
        ..Default::default()
    }
}

#[test]
fn resolve_cli_override_wins() {
    let os = os_release("fedora", "41", "silverblue");
    let result = resolve_base_image(
        &os,
        None,  // no ublue
        None,  // no bootc status
        Some("registry.example.com/custom:latest"),
    );
    let res = result.unwrap();
    assert_eq!(res.strategy, ResolutionStrategy::CliOverride);
    assert_eq!(res.image_ref, "registry.example.com/custom:latest");
}

#[test]
fn resolve_fedora_atomic_before_generic_fedora() {
    let os = os_release("fedora", "41", "silverblue");
    let result = resolve_base_image(&os, None, None, None);
    let res = result.unwrap();
    assert_eq!(res.strategy, ResolutionStrategy::FedoraAtomicDesktop);
    assert_eq!(res.image_ref, "quay.io/fedora-ostree-desktops/silverblue:41");
}

#[test]
fn resolve_generic_fedora_no_variant() {
    let os = os_release("fedora", "41", "");
    let result = resolve_base_image(&os, None, None, None);
    let res = result.unwrap();
    assert_eq!(res.strategy, ResolutionStrategy::OsRelease);
    assert_eq!(res.image_ref, "quay.io/fedora/fedora-bootc:41");
}

#[test]
fn resolve_centos_stream() {
    let os = os_release("centos", "9", "");
    let result = resolve_base_image(&os, None, None, None);
    let res = result.unwrap();
    assert_eq!(res.image_ref, "quay.io/centos-bootc/centos-bootc:stream9");
}

#[test]
fn resolve_rhel() {
    let os = os_release("rhel", "9.6", "");
    let result = resolve_base_image(&os, None, None, None);
    let res = result.unwrap();
    assert_eq!(res.image_ref, "registry.redhat.io/rhel9/rhel-bootc:9.6");
}

#[test]
fn resolve_unknown_distro_fails() {
    let os = os_release("ubuntu", "24.04", "");
    let result = resolve_base_image(&os, None, None, None);
    assert!(result.is_err());
}

#[test]
fn resolve_bootc_status_before_os_release() {
    let os = os_release("rhel", "9.6", "");
    let bootc_ref = Some("registry.redhat.io/rhel9/rhel-bootc:9.6");
    let result = resolve_base_image(&os, None, bootc_ref, None);
    let res = result.unwrap();
    assert_eq!(res.strategy, ResolutionStrategy::BootcStatus);
}

#[test]
fn resolve_ublue_tagless_ref_combined_with_tag() {
    let ublue = UblueMetadata {
        image_ref: Some("ostree-image-signed:docker://ghcr.io/ublue-os/bazzite".into()),
        image_tag: Some("stable".into()),
        image_name: Some("bazzite".into()),
        image_vendor: Some("ublue-os".into()),
    };
    let os = os_release("fedora", "41", "");
    let result = resolve_base_image(&os, Some(&ublue), None, None);
    let res = result.unwrap();
    assert_eq!(res.strategy, ResolutionStrategy::UniversalBlue);
    assert_eq!(res.image_ref, "ghcr.io/ublue-os/bazzite:stable");
}

#[test]
fn resolve_ublue_tagged_ref_used_as_is() {
    let ublue = UblueMetadata {
        image_ref: Some("ghcr.io/ublue-os/bazzite:40".into()),
        image_tag: Some("stable".into()),
        image_name: None,
        image_vendor: None,
    };
    let os = os_release("fedora", "41", "");
    let result = resolve_base_image(&os, Some(&ublue), None, None);
    let res = result.unwrap();
    assert_eq!(res.image_ref, "ghcr.io/ublue-os/bazzite:40");
}

#[test]
fn resolve_ublue_synthesis_fallback() {
    let ublue = UblueMetadata {
        image_ref: None,
        image_tag: Some("stable".into()),
        image_name: Some("bazzite".into()),
        image_vendor: Some("ublue-os".into()),
    };
    let os = os_release("fedora", "41", "");
    let result = resolve_base_image(&os, Some(&ublue), None, None);
    let res = result.unwrap();
    assert_eq!(res.image_ref, "ghcr.io/ublue-os/bazzite:stable");
}

#[test]
fn resolve_ublue_tagless_no_image_tag_fails() {
    let ublue = UblueMetadata {
        image_ref: Some("ostree-image-signed:docker://ghcr.io/ublue-os/bazzite".into()),
        image_tag: None,
        image_name: None,
        image_vendor: None,
    };
    let os = os_release("fedora", "41", "");
    let result = resolve_base_image(&os, Some(&ublue), None, None);
    assert!(result.is_err());
}

#[test]
fn resolve_all_known_desktop_variants() {
    for variant in &["silverblue", "kinoite", "sway-atomic", "budgie-atomic",
                      "cosmic-atomic", "lxqt-atomic", "xfce-atomic"] {
        let os = os_release("fedora", "41", variant);
        let result = resolve_base_image(&os, None, None, None);
        let res = result.unwrap();
        assert_eq!(res.strategy, ResolutionStrategy::FedoraAtomicDesktop);
        assert!(res.image_ref.contains(variant));
    }
}
```

- [ ] **Step 2: Run tests — verify they fail**

Run: `cargo test -p inspectah-core -- resolve`
Expected: FAIL — `resolve_base_image` and `UblueMetadata` not defined.

- [ ] **Step 3: Implement resolution chain**

Add to `inspectah-core/src/baseline.rs` (above `#[cfg(test)]`):

```rust
use crate::types::os::OsRelease;

const FEDORA_ATOMIC_DESKTOPS: &[&str] = &[
    "silverblue", "kinoite", "sway-atomic", "budgie-atomic",
    "cosmic-atomic", "lxqt-atomic", "xfce-atomic",
];

const TRANSPORT_PREFIXES: &[&str] = &[
    "ostree-image-signed:docker://",
    "docker://",
    "containers-storage:",
];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UblueMetadata {
    #[serde(rename = "image-ref")]
    pub image_ref: Option<String>,
    #[serde(rename = "image-tag")]
    pub image_tag: Option<String>,
    #[serde(rename = "image-name")]
    pub image_name: Option<String>,
    #[serde(rename = "image-vendor")]
    pub image_vendor: Option<String>,
}

fn strip_transport_prefix(raw: &str) -> &str {
    for prefix in TRANSPORT_PREFIXES {
        if let Some(stripped) = raw.strip_prefix(prefix) {
            return stripped;
        }
    }
    raw
}

fn has_tag(image_ref: &str) -> bool {
    // A ref has a tag if there's a colon after the last slash
    match image_ref.rfind('/') {
        Some(slash_pos) => image_ref[slash_pos..].contains(':'),
        None => image_ref.contains(':'),
    }
}

fn resolve_ublue(ublue: &UblueMetadata) -> Result<BaseImageResolution, ResolutionError> {
    if let Some(ref raw_ref) = ublue.image_ref {
        let stripped = strip_transport_prefix(raw_ref);
        let resolved = if has_tag(stripped) {
            stripped.to_string()
        } else {
            match &ublue.image_tag {
                Some(tag) if !tag.is_empty() => format!("{stripped}:{tag}"),
                _ => {
                    return Err(ResolutionError::MalformedUblueMetadata {
                        path: "/usr/share/ublue-os/image-info.json".into(),
                        reason: "image-ref is tagless and no image-tag provided".into(),
                    });
                }
            }
        };
        return Ok(BaseImageResolution {
            image_ref: resolved,
            strategy: ResolutionStrategy::UniversalBlue,
        });
    }

    match (&ublue.image_vendor, &ublue.image_name, &ublue.image_tag) {
        (Some(vendor), Some(name), Some(tag))
            if !vendor.is_empty() && !name.is_empty() && !tag.is_empty() =>
        {
            Ok(BaseImageResolution {
                image_ref: format!("ghcr.io/{vendor}/{name}:{tag}"),
                strategy: ResolutionStrategy::UniversalBlue,
            })
        }
        _ => Err(ResolutionError::MalformedUblueMetadata {
            path: "/usr/share/ublue-os/image-info.json".into(),
            reason: "missing required fields for synthesis (need image-vendor, image-name, image-tag)".into(),
        }),
    }
}

pub fn resolve_base_image(
    os_release: &OsRelease,
    ublue: Option<&UblueMetadata>,
    bootc_status_ref: Option<&str>,
    cli_override: Option<&str>,
) -> Result<BaseImageResolution, ResolutionError> {
    // 1. CLI override
    if let Some(image_ref) = cli_override {
        return Ok(BaseImageResolution {
            image_ref: image_ref.to_string(),
            strategy: ResolutionStrategy::CliOverride,
        });
    }

    // 2. Universal Blue
    if let Some(ublue) = ublue {
        return resolve_ublue(ublue);
    }

    // 3. bootc status
    if let Some(ref_str) = bootc_status_ref {
        if !ref_str.is_empty() {
            return Ok(BaseImageResolution {
                image_ref: strip_transport_prefix(ref_str).to_string(),
                strategy: ResolutionStrategy::BootcStatus,
            });
        }
    }

    // 4. Fedora Atomic desktop (BEFORE generic os-release)
    if os_release.id == "fedora" {
        if FEDORA_ATOMIC_DESKTOPS.contains(&os_release.variant_id.as_str()) {
            return Ok(BaseImageResolution {
                image_ref: format!(
                    "quay.io/fedora-ostree-desktops/{}:{}",
                    os_release.variant_id, os_release.version_id
                ),
                strategy: ResolutionStrategy::FedoraAtomicDesktop,
            });
        }
    }

    // 5. Generic os-release mapping
    let major = os_release
        .version_id
        .split('.')
        .next()
        .unwrap_or(&os_release.version_id);

    match os_release.id.as_str() {
        "fedora" => Ok(BaseImageResolution {
            image_ref: format!("quay.io/fedora/fedora-bootc:{}", os_release.version_id),
            strategy: ResolutionStrategy::OsRelease,
        }),
        "centos" => Ok(BaseImageResolution {
            image_ref: format!("quay.io/centos-bootc/centos-bootc:stream{major}"),
            strategy: ResolutionStrategy::OsRelease,
        }),
        "rhel" => Ok(BaseImageResolution {
            image_ref: format!(
                "registry.redhat.io/rhel{major}/rhel-bootc:{}",
                os_release.version_id
            ),
            strategy: ResolutionStrategy::OsRelease,
        }),
        _ => Err(ResolutionError::UnknownDistro {
            id: os_release.id.clone(),
        }),
    }
}
```

- [ ] **Step 4: Run tests — verify they pass**

Run: `cargo test -p inspectah-core -- baseline`
Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add inspectah-core/src/baseline.rs
git commit -m "feat(core): implement base image resolution chain with UBlue and Fedora Atomic support"
```

---

## Task 3: Ref Normalization Gate

**Files:**
- Modify: `inspectah-core/src/baseline.rs`

- [ ] **Step 1: Write normalization tests**

Add to `tests` module in `inspectah-core/src/baseline.rs`:

```rust
#[test]
fn normalize_strips_transport_prefix() {
    let result = normalize_image_ref("ostree-image-signed:docker://ghcr.io/ublue-os/bazzite:stable").unwrap();
    assert_eq!(result.as_str(), "ghcr.io/ublue-os/bazzite:stable");
}

#[test]
fn normalize_rejects_empty() {
    assert!(normalize_image_ref("").is_err());
}

#[test]
fn normalize_rejects_whitespace() {
    assert!(normalize_image_ref("registry.redhat.io/rhel9/rhel-bootc:9.6 ; rm -rf /").is_err());
}

#[test]
fn normalize_rejects_bare_ref() {
    assert!(normalize_image_ref("rhel-bootc:9.6").is_err());
}

#[test]
fn normalize_appends_latest_when_no_tag() {
    let result = normalize_image_ref("registry.redhat.io/rhel9/rhel-bootc").unwrap();
    assert_eq!(result.as_str(), "registry.redhat.io/rhel9/rhel-bootc:latest");
}

#[test]
fn normalize_preserves_digest() {
    let input = "registry.redhat.io/rhel9/rhel-bootc@sha256:abc123";
    let result = normalize_image_ref(input).unwrap();
    assert_eq!(result.as_str(), input);
}

#[test]
fn normalize_preserves_tag() {
    let input = "registry.redhat.io/rhel9/rhel-bootc:9.6";
    let result = normalize_image_ref(input).unwrap();
    assert_eq!(result.as_str(), input);
}

#[test]
fn normalize_rejects_localhost() {
    assert!(normalize_image_ref("localhost/myimage:latest").is_err());
}

#[test]
fn normalize_rejects_containers_storage() {
    assert!(normalize_image_ref("containers-storage:myimage:latest").is_err());
}

#[test]
fn normalize_rejects_shell_metacharacters() {
    assert!(normalize_image_ref("registry.example.com/image:$(whoami)").is_err());
}
```

- [ ] **Step 2: Run tests — verify they fail**

Run: `cargo test -p inspectah-core -- normalize_`
Expected: FAIL — `normalize_image_ref` not defined.

- [ ] **Step 3: Implement normalize_image_ref**

Add to `inspectah-core/src/baseline.rs`:

```rust
const SHELL_METACHARACTERS: &[char] = &['$', '`', '|', ';', '&', '(', ')', '{', '}', '<', '>',
                                         '\n', '\r', '!', '#'];

pub fn normalize_image_ref(raw: &str) -> Result<NormalizedImageRef, NormalizationError> {
    if raw.is_empty() {
        return Err(NormalizationError::Empty);
    }

    let stripped = strip_transport_prefix(raw);

    if stripped.contains(char::is_whitespace) || stripped.contains(SHELL_METACHARACTERS.as_slice()) {
        return Err(NormalizationError::InvalidCharacters(raw.into()));
    }

    if stripped.starts_with("localhost/") || stripped.starts_with("containers-storage:") {
        return Err(NormalizationError::LocalOnly(raw.into()));
    }

    // Must contain at least one slash (registry/repo)
    if !stripped.contains('/') {
        return Err(NormalizationError::NotFullyQualified(raw.into()));
    }

    // If it has a digest (@sha256:), preserve as-is
    if stripped.contains('@') {
        return Ok(NormalizedImageRef {
            ref_string: stripped.to_string(),
        });
    }

    // If it has a tag (colon after last slash), preserve
    if has_tag(stripped) {
        return Ok(NormalizedImageRef {
            ref_string: stripped.to_string(),
        });
    }

    // No tag, no digest — append :latest
    Ok(NormalizedImageRef {
        ref_string: format!("{stripped}:latest"),
    })
}
```

- [ ] **Step 4: Run tests — verify they pass**

Run: `cargo test -p inspectah-core -- baseline`
Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add inspectah-core/src/baseline.rs
git commit -m "feat(core): add ref normalization gate with transport stripping and validation"
```

---

## Task 4: Snapshot Schema Changes

**Files:**
- Modify: `inspectah-core/src/snapshot.rs`

- [ ] **Step 1: Write schema tests**

Add to the existing `tests` module in `inspectah-core/src/snapshot.rs`:

```rust
use crate::baseline::{
    BaseImageResolution, BaselineData, BaselinePackageEntry,
    NormalizedImageRef, ResolutionStrategy, TargetImageIdentity,
};

#[test]
fn snapshot_with_target_image_roundtrip() {
    let mut snap = InspectionSnapshot::new();
    snap.target_image = Some(TargetImageIdentity {
        image_ref: "registry.redhat.io/rhel9/rhel-bootc:9.6".into(),
        strategy: ResolutionStrategy::OsRelease,
    });
    let json = serde_json::to_string(&snap).unwrap();
    let parsed: InspectionSnapshot = serde_json::from_str(&json).unwrap();
    assert!(parsed.target_image.is_some());
    assert_eq!(
        parsed.target_image.unwrap().image_ref,
        "registry.redhat.io/rhel9/rhel-bootc:9.6"
    );
}

#[test]
fn snapshot_with_baseline_roundtrip() {
    let mut snap = InspectionSnapshot::new();
    snap.target_image = Some(TargetImageIdentity {
        image_ref: "registry.redhat.io/rhel9/rhel-bootc:9.6".into(),
        strategy: ResolutionStrategy::OsRelease,
    });
    snap.baseline = Some(BaselineData {
        resolution: BaseImageResolution {
            image_ref: "registry.redhat.io/rhel9/rhel-bootc:9.6".into(),
            strategy: ResolutionStrategy::OsRelease,
        },
        normalized_ref: NormalizedImageRef::from_validated(
            "registry.redhat.io/rhel9/rhel-bootc:9.6".into(),
        ),
        image_digest: "sha256:abc123".into(),
        packages: std::collections::HashMap::from([(
            "bash.x86_64".into(),
            BaselinePackageEntry {
                name: "bash".into(),
                epoch: Some("0".into()),
                version: "5.2.26".into(),
                release: "3.el9".into(),
                arch: "x86_64".into(),
            },
        )]),
        extracted_at: "2026-05-17T01:00:00Z".into(),
    });
    let json = serde_json::to_string(&snap).unwrap();
    let parsed: InspectionSnapshot = serde_json::from_str(&json).unwrap();
    assert!(parsed.baseline.is_some());
    assert_eq!(parsed.baseline.unwrap().packages.len(), 1);
}

#[test]
fn degraded_snapshot_no_baseline() {
    let mut snap = InspectionSnapshot::new();
    snap.target_image = Some(TargetImageIdentity {
        image_ref: "registry.redhat.io/rhel9/rhel-bootc:9.6".into(),
        strategy: ResolutionStrategy::OsRelease,
    });
    snap.no_baseline = true;
    assert!(snap.baseline.is_none());
    assert!(snap.target_image.is_some());
    let json = serde_json::to_string(&snap).unwrap();
    let parsed: InspectionSnapshot = serde_json::from_str(&json).unwrap();
    assert!(parsed.no_baseline);
    assert!(parsed.target_image.is_some());
}

#[test]
fn pre_phase6_snapshot_migration() {
    // Simulate a Phase 5 snapshot (no target_image/baseline/no_baseline fields)
    let json = r#"{
        "schema_version": 14,
        "meta": {},
        "system_type": "package-mode",
        "preflight": {"status": "ok"},
        "warnings": [],
        "redactions": []
    }"#;
    let mut snap: InspectionSnapshot = serde_json::from_str(json).unwrap();
    migrate(&mut snap);
    assert_eq!(snap.schema_version, 15);
    assert!(snap.target_image.is_none());
    assert!(snap.baseline.is_none());
    // Pre-Phase-6 snapshots with no fields default to no_baseline=false
    // (serde default). The refine layer treats missing baseline + no_baseline=false
    // the same as degraded mode for display purposes.
}
```

- [ ] **Step 2: Run tests — verify they fail**

Run: `cargo test -p inspectah-core -- snapshot`
Expected: FAIL — `target_image` and `baseline` fields not on `InspectionSnapshot`.

- [ ] **Step 3: Add fields to InspectionSnapshot and update migration**

In `inspectah-core/src/snapshot.rs`, add fields to the struct and bump the version:

```rust
pub const SCHEMA_VERSION: u32 = 15;
```

Add to `InspectionSnapshot` struct (after `completeness`):

```rust
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_image: Option<crate::baseline::TargetImageIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline: Option<crate::baseline::BaselineData>,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub no_baseline: bool,
```

Add `NormalizedImageRef::from_validated` constructor (in `baseline.rs`):

```rust
impl NormalizedImageRef {
    pub fn from_validated(ref_string: String) -> Self {
        Self { ref_string }
    }
}
```

Update `migrate()` to handle v14→v15:

```rust
pub fn migrate(snap: &mut InspectionSnapshot) {
    if snap.schema_version >= SCHEMA_VERSION {
        return;
    }
    snap.schema_version = SCHEMA_VERSION;
}
```

- [ ] **Step 4: Run tests — verify they pass**

Run: `cargo test -p inspectah-core`
Expected: all tests pass (including existing ones — serde(default) handles missing fields).

- [ ] **Step 5: Commit**

```bash
git add inspectah-core/src/snapshot.rs inspectah-core/src/baseline.rs
git commit -m "feat(core): add target_image and baseline to snapshot schema, bump to v15"
```

---

## Task 5: Mock Executor Order Recording + Baseline Extraction

**Files:**
- Modify: `inspectah-collect/src/executor/mock.rs`
- Create: `inspectah-collect/src/baseline.rs`
- Modify: `inspectah-collect/src/lib.rs`
- Create: `inspectah-collect/tests/baseline_test.rs`

- [ ] **Step 1: Add order recording to MockExecutor**

In `inspectah-collect/src/executor/mock.rs`, add a `command_log` field:

```rust
use std::sync::Mutex;

pub struct MockExecutor {
    // ... existing fields ...
    command_log: Mutex<Vec<String>>,
}
```

Initialize in `new()`:
```rust
command_log: Mutex::new(Vec::new()),
```

Record in `run()`:
```rust
fn run(&self, cmd: &str, args: &[&str]) -> ExecResult {
    let full_cmd = format!("{} {}", cmd, args.join(" "));
    self.command_log.lock().unwrap().push(full_cmd.clone());
    // ... existing lookup logic ...
}
```

Add accessor:
```rust
pub fn command_log(&self) -> Vec<String> {
    self.command_log.lock().unwrap().clone()
}
```

- [ ] **Step 2: Write baseline extraction tests**

Create `inspectah-collect/tests/baseline_test.rs`:

```rust
use inspectah_collect::baseline::extract_baseline;
use inspectah_collect::executor::mock::MockExecutor;
use inspectah_core::baseline::{NormalizedImageRef, ResolutionStrategy, BaseImageResolution};
use inspectah_core::traits::executor::ExecResult;

fn mock_rpm_qa_output() -> String {
    "bash\t0\t5.2.26\t3.el9\tx86_64\n\
     coreutils\t0\t9.1\t13.el9\tx86_64\n\
     glibc\t0\t2.34\t83.el9\tx86_64\n"
        .to_string()
}

fn success(stdout: &str) -> ExecResult {
    ExecResult {
        stdout: stdout.to_string(),
        stderr: String::new(),
        exit_code: 0,
    }
}

fn failure(stderr: &str) -> ExecResult {
    ExecResult {
        stdout: String::new(),
        stderr: stderr.to_string(),
        exit_code: 1,
    }
}

#[test]
fn extract_baseline_happy_path() {
    let image_ref = "registry.redhat.io/rhel9/rhel-bootc:9.6";
    let executor = MockExecutor::new()
        .with_command(
            &format!("nsenter -t 1 -m -u -i -n -- podman pull {image_ref}"),
            success(""),
        )
        .with_command_prefix(
            "nsenter -t 1 -m -u -i -n -- podman create",
            success("container-id-123"),
        )
        .with_command_prefix(
            "nsenter -t 1 -m -u -i -n -- podman start",
            success(""),
        )
        .with_command_prefix(
            "nsenter -t 1 -m -u -i -n -- podman exec",
            success(&mock_rpm_qa_output()),
        )
        .with_command_prefix(
            "nsenter -t 1 -m -u -i -n -- podman inspect",
            success("sha256:abc123def456"),
        )
        .with_command_prefix(
            "nsenter -t 1 -m -u -i -n -- podman rm",
            success(""),
        );

    let resolution = BaseImageResolution {
        image_ref: image_ref.into(),
        strategy: ResolutionStrategy::OsRelease,
    };
    let normalized = NormalizedImageRef::from_validated(image_ref.into());

    let result = extract_baseline(&executor, &resolution, &normalized).unwrap();
    assert_eq!(result.packages.len(), 3);
    assert!(result.packages.contains_key("bash.x86_64"));
    assert_eq!(result.packages["bash.x86_64"].version, "5.2.26");
    assert_eq!(result.image_digest, "sha256:abc123def456");
}

#[test]
fn extract_baseline_command_order() {
    // Same setup as happy path, verify order
    let image_ref = "registry.redhat.io/rhel9/rhel-bootc:9.6";
    let executor = MockExecutor::new()
        .with_command(
            &format!("nsenter -t 1 -m -u -i -n -- podman pull {image_ref}"),
            success(""),
        )
        .with_command_prefix("nsenter -t 1 -m -u -i -n -- podman create", success("cid"))
        .with_command_prefix("nsenter -t 1 -m -u -i -n -- podman start", success(""))
        .with_command_prefix("nsenter -t 1 -m -u -i -n -- podman exec", success(&mock_rpm_qa_output()))
        .with_command_prefix("nsenter -t 1 -m -u -i -n -- podman inspect", success("sha256:abc"))
        .with_command_prefix("nsenter -t 1 -m -u -i -n -- podman rm", success(""));

    let resolution = BaseImageResolution {
        image_ref: image_ref.into(),
        strategy: ResolutionStrategy::OsRelease,
    };
    let normalized = NormalizedImageRef::from_validated(image_ref.into());

    let _ = extract_baseline(&executor, &resolution, &normalized).unwrap();

    let log = executor.command_log();
    assert!(log[0].contains("podman pull"), "first: pull");
    assert!(log[1].contains("podman create"), "second: create");
    assert!(log[2].contains("podman start"), "third: start");
    assert!(log[3].contains("podman exec"), "fourth: exec (rpm -qa)");
    assert!(log[4].contains("podman inspect"), "fifth: inspect (digest)");
    assert!(log[5].contains("podman rm"), "last: cleanup");
}

#[test]
fn extract_baseline_cleanup_on_pull_failure() {
    let image_ref = "registry.redhat.io/rhel9/rhel-bootc:9.6";
    let executor = MockExecutor::new()
        .with_command(
            &format!("nsenter -t 1 -m -u -i -n -- podman pull {image_ref}"),
            failure("unauthorized: authentication required"),
        );

    let resolution = BaseImageResolution {
        image_ref: image_ref.into(),
        strategy: ResolutionStrategy::OsRelease,
    };
    let normalized = NormalizedImageRef::from_validated(image_ref.into());

    let result = extract_baseline(&executor, &resolution, &normalized);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("Authentication") || err.contains("pull"));
}

#[test]
fn extract_baseline_cleanup_on_exec_failure() {
    let image_ref = "registry.redhat.io/rhel9/rhel-bootc:9.6";
    let executor = MockExecutor::new()
        .with_command(
            &format!("nsenter -t 1 -m -u -i -n -- podman pull {image_ref}"),
            success(""),
        )
        .with_command_prefix("nsenter -t 1 -m -u -i -n -- podman create", success("cid"))
        .with_command_prefix("nsenter -t 1 -m -u -i -n -- podman start", success(""))
        .with_command_prefix("nsenter -t 1 -m -u -i -n -- podman exec", failure("rpm not found"))
        .with_command_prefix("nsenter -t 1 -m -u -i -n -- podman rm", success(""));

    let resolution = BaseImageResolution {
        image_ref: image_ref.into(),
        strategy: ResolutionStrategy::OsRelease,
    };
    let normalized = NormalizedImageRef::from_validated(image_ref.into());

    let result = extract_baseline(&executor, &resolution, &normalized);
    assert!(result.is_err());

    let log = executor.command_log();
    assert!(log.last().unwrap().contains("podman rm"), "cleanup must run even on exec failure");
}
```

- [ ] **Step 3: Implement extract_baseline**

Create `inspectah-collect/src/baseline.rs` and wire up in `lib.rs`. The implementation follows the spec: nsenter prefix, podman create with --entrypoint override and --network none, NEVRA extraction, digest capture via `.Digest`, cleanup guard.

Key implementation points:
- Container name: `inspectah-baseline-{unix_timestamp}`
- `--entrypoint '["sleep", "infinity"]'` on `podman create`
- `--network none` on `podman create`
- `rpm -qa --queryformat '%{NAME}\t%{EPOCH}\t%{VERSION}\t%{RELEASE}\t%{ARCH}\n'`
- `podman inspect --format '{{.Digest}}'` for repository-side digest
- Drop guard struct that runs `podman rm -f` on drop
- `MockExecutor` needs `with_command_prefix` for flexible matching (new addition alongside order recording)

- [ ] **Step 4: Run tests**

Run: `cargo test -p inspectah-collect -- baseline`
Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add inspectah-collect/src/baseline.rs inspectah-collect/src/lib.rs \
       inspectah-collect/src/executor/mock.rs inspectah-collect/tests/baseline_test.rs
git commit -m "feat(collect): implement baseline extraction with entrypoint override and order proof"
```

---

## Task 6: Package Classification Matrix + Service Flagging

**Files:**
- Modify: `inspectah-refine/src/attention.rs`
- Modify: `inspectah-refine/src/normalize.rs`

- [ ] **Step 1: Write exhaustive classification tests**

Add to or create tests in `inspectah-refine/src/attention.rs` covering every cell of the spec's exhaustive matrix (section 4). Each test asserts the correct `AttentionLevel` and `AttentionReason` for a specific `PackageState × provenance × baseline mode` combination. 14 cells total from the matrix.

Key assertions:
- `Added` + recognized repo + in baseline → `Routine` / `PackageBaselineMatch`
- `Added` + recognized repo + NOT in baseline → `Routine` / `PackageUserAdded`
- `Added` + no repo → `NeedsReview` / `PackageNoRepoSource` (critical)
- `Modified` + recognized repo + in baseline → `NeedsReview` / `PackageVersionChanged`
- `Modified` + no repo → `NeedsReview` / `PackageNoRepoSource` (critical)
- `LocalInstall` → `NeedsReview` / `PackageNoRepoSource` (critical)
- `NoRepo` → `NeedsReview` / `PackageNoRepoSource` (critical)
- `BaseImageOnly` → not rendered
- All `Added` in degraded mode → `NeedsReview` / `PackageProvenanceUnavailable`

- [ ] **Step 2: Write incompatible service flagging tests**

Add to `inspectah-refine/src/normalize.rs`:
- Test that `dnf-makecache.service` in `state_changes` gets `include: false` + `ServiceImageModeIncompatible` reason
- Test that `httpd.service` is NOT flagged
- Test that flagged services are removed from `enabled_units`
- Test that UI, preview, and export all see the same normalized state

- [ ] **Step 3: Implement classification and flagging**

Add new `AttentionReason` variants to the enum in `attention.rs`:
- `PackageBaselineMatch`
- `PackageUserAdded`
- `PackageVersionChanged`
- `PackageProvenanceUnavailable`
- `PackageNoRepoSource`
- `ServiceImageModeIncompatible`

Update the classification function to accept `Option<&BaselineData>` and implement the exhaustive matrix.

In `normalize.rs`, add incompatible service filtering that reads from `INCOMPATIBLE_SERVICES` in core and modifies `ServiceSection` state changes.

- [ ] **Step 4: Run tests**

Run: `cargo test -p inspectah-refine`
Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add inspectah-refine/src/attention.rs inspectah-refine/src/normalize.rs
git commit -m "feat(refine): exhaustive classification matrix and incompatible service flagging"
```

---

## Task 7: Containerfile Dynamic FROM + BaselineSummary

**Files:**
- Modify: `inspectah-pipeline/src/render/containerfile.rs`
- Create: `inspectah-refine/src/baseline_summary.rs`
- Modify: `inspectah-refine/src/lib.rs`
- Modify: `inspectah-refine/src/session.rs`
- Modify: `inspectah-web/src/handlers.rs`

- [ ] **Step 1: Write FROM line tests**

In `inspectah-pipeline/src/render/containerfile.rs`, update `base_image_from_snapshot` tests:

```rust
#[test]
fn from_line_uses_target_image() {
    let mut snap = InspectionSnapshot::new();
    snap.target_image = Some(TargetImageIdentity {
        image_ref: "registry.redhat.io/rhel9/rhel-bootc:9.6".into(),
        strategy: ResolutionStrategy::OsRelease,
    });
    assert_eq!(
        base_image_from_snapshot(&snap),
        "registry.redhat.io/rhel9/rhel-bootc:9.6"
    );
}

#[test]
fn from_line_degraded_with_target_image() {
    let mut snap = InspectionSnapshot::new();
    snap.target_image = Some(TargetImageIdentity {
        image_ref: "quay.io/centos-bootc/centos-bootc:stream9".into(),
        strategy: ResolutionStrategy::OsRelease,
    });
    snap.no_baseline = true;
    // Even in degraded mode, FROM uses the resolved target
    assert_eq!(
        base_image_from_snapshot(&snap),
        "quay.io/centos-bootc/centos-bootc:stream9"
    );
}

#[test]
fn from_line_no_target_image_omitted() {
    let snap = InspectionSnapshot::new();
    // No target_image, no rpm.base_image — falls back to legacy or omission
    let result = base_image_from_snapshot(&snap);
    // Legacy: still returns hardcoded fallback for backward compat with Go snapshots
    assert!(!result.is_empty());
}
```

- [ ] **Step 2: Update base_image_from_snapshot**

```rust
pub fn base_image_from_snapshot(snap: &InspectionSnapshot) -> String {
    // Phase 6: prefer top-level target_image
    if let Some(ref ti) = snap.target_image {
        return ti.image_ref.clone();
    }
    // Backward compat: fall back to rpm.base_image (Go snapshots)
    if let Some(rpm) = &snap.rpm {
        if let Some(ref base) = rpm.base_image {
            if !base.is_empty() {
                return base.clone();
            }
        }
    }
    "registry.redhat.io/rhel9/rhel-bootc:9.4".to_string()
}
```

- [ ] **Step 3: Implement BaselineSummary**

Create `inspectah-refine/src/baseline_summary.rs`:

```rust
use serde::{Deserialize, Serialize};
use inspectah_core::snapshot::InspectionSnapshot;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BaselineSummary {
    pub image_ref: String,
    pub image_digest: String,
    pub strategy: String,
    pub baseline_count: usize,
    pub user_added_count: usize,
    pub review_count: usize,
}

pub fn derive_baseline_summary(snap: &InspectionSnapshot) -> Option<BaselineSummary> {
    let baseline = snap.baseline.as_ref()?;
    let target = snap.target_image.as_ref()?;

    // Counts are derived from classification results on the snapshot's packages
    // (filled by the classification pass in attention.rs)
    let baseline_count = baseline.packages.len();

    // user_added_count and review_count come from the classified packages
    // in snap.rpm.packages_added — count by attention_reason
    let (mut user_added, mut review) = (0usize, 0usize);
    if let Some(ref rpm) = snap.rpm {
        for pkg in &rpm.packages_added {
            // Classification sets include=true for auto-included,
            // leaves include=false for NeedsReview
            if pkg.include && !baseline.packages.contains_key(
                &format!("{}.{}", pkg.name, pkg.arch)
            ) {
                user_added += 1;
            }
            if !pkg.include {
                review += 1;
            }
        }
    }

    Some(BaselineSummary {
        image_ref: target.image_ref.clone(),
        image_digest: baseline.image_digest.clone(),
        strategy: serde_json::to_string(&target.strategy)
            .unwrap_or_default()
            .trim_matches('"')
            .to_string(),
        baseline_count,
        user_added_count: user_added,
        review_count: review,
    })
}
```

- [ ] **Step 4: Wire into RefineSession and ViewResponse**

In `inspectah-refine/src/session.rs`, add a method:
```rust
pub fn baseline_summary(&self) -> Option<BaselineSummary> {
    derive_baseline_summary(&self.snapshot)
}
```

In `inspectah-web/src/handlers.rs`, add to `ViewResponse`:
```rust
pub baseline_summary: Option<BaselineSummary>,
```

Update `build_view_response`:
```rust
ViewResponse {
    view,
    repo_groups,
    baseline_summary: session.baseline_summary(),
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p inspectah-pipeline -- containerfile && cargo test -p inspectah-refine -- baseline_summary && cargo test -p inspectah-web`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add inspectah-pipeline/src/render/containerfile.rs \
       inspectah-refine/src/baseline_summary.rs inspectah-refine/src/lib.rs \
       inspectah-refine/src/session.rs inspectah-web/src/handlers.rs
git commit -m "feat(render): dynamic FROM from target_image, BaselineSummary in ViewResponse"
```

---

## Task 8: CLI Integration

**Files:**
- Modify: `inspectah-cli/src/commands/scan.rs`

- [ ] **Step 1: Add CLI flags**

Add to `ScanArgs`:

```rust
/// Target base image for cross-distro conversion
#[arg(long)]
pub base_image: Option<String>,

/// Skip baseline extraction (degraded classification mode)
#[arg(long)]
pub no_baseline: bool,
```

- [ ] **Step 2: Add flag validation**

In the `run` function, before any pipeline work:

```rust
if args.base_image.is_some() && args.no_baseline {
    anyhow::bail!(
        "Cannot specify both --base-image and --no-baseline. \
         Use --base-image to set the target image, or --no-baseline to skip baseline extraction."
    );
}
```

- [ ] **Step 3: Wire resolution + extraction into pipeline**

Before the existing `collect()` call, add:

```rust
// --- Phase 6: resolve target image ---
eprintln!("Resolving target image...");
let ublue_metadata = read_ublue_metadata(&executor);
let bootc_ref = read_bootc_status_ref(&executor);
let resolution = if args.no_baseline {
    // Still resolve for FROM line, but don't extract
    resolve_base_image(&os_release, ublue_metadata.as_ref(), bootc_ref.as_deref(), args.base_image.as_deref())
        .ok()
} else {
    Some(resolve_base_image(
        &os_release,
        ublue_metadata.as_ref(),
        bootc_ref.as_deref(),
        args.base_image.as_deref(),
    )?)
};

if let Some(ref res) = resolution {
    eprintln!("Resolving target image... {} ({})", res.image_ref,
        serde_json::to_string(&res.strategy).unwrap_or_default().trim_matches('"'));
}

let baseline_data = if !args.no_baseline {
    let res = resolution.as_ref().unwrap();
    let normalized = normalize_image_ref(&res.image_ref)?;
    eprintln!("Normalizing image reference... ok");
    eprintln!("Pulling target image...");
    let data = extract_baseline(&executor, res, &normalized)?;
    eprintln!("Pulling target image... done");
    eprintln!("Extracting baseline... {} packages", data.packages.len());
    Some(data)
} else {
    None
};

// Set snapshot fields
if let Some(ref res) = resolution {
    snapshot.target_image = Some(TargetImageIdentity {
        image_ref: res.image_ref.clone(),
        strategy: res.strategy.clone(),
    });
}
if let Some(ref data) = baseline_data {
    snapshot.baseline = Some(data.clone());
}
snapshot.no_baseline = args.no_baseline;
```

Add helper functions for reading UBlue metadata and bootc status from the executor (read_file for `/usr/share/ublue-os/image-info.json`, executor.run for `bootc status --json`).

- [ ] **Step 4: Add progress output for host scan**

Wrap inspector calls with `[N/11]` stderr output:

```rust
let inspectors: Vec<(&str, Box<dyn Inspector>)> = vec![
    ("RPM packages", Box::new(RpmInspector::new())),
    ("Services", Box::new(ServicesInspector::new())),
    // ... etc
];

for (i, (name, inspector)) in inspectors.iter().enumerate() {
    eprint!("\r  [{}/{}] {}", i + 1, inspectors.len(), name);
    // ... run inspector ...
}
eprintln!();
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p inspectah-cli`
Expected: compilation succeeds, existing tests pass. (CLI integration tests are manual.)

- [ ] **Step 6: Commit**

```bash
git add inspectah-cli/src/commands/scan.rs
git commit -m "feat(cli): add --base-image and --no-baseline flags with progress output"
```

---

## Task 9: Web UI Verification Banner

**Files:**
- Modify: `inspectah-web/frontend/src/App.tsx` (or equivalent banner component)

- [ ] **Step 1: Add banner rendering**

In the Packages section header area, read `baseline_summary` from the API response:

```tsx
{viewData.baseline_summary ? (
  <Alert variant="info" isInline title={
    `Baseline compared against ${viewData.baseline_summary.image_ref} ` +
    `(${viewData.baseline_summary.image_digest.substring(0, 19)}…) — ` +
    `${viewData.baseline_summary.baseline_count} in base image, ` +
    `${viewData.baseline_summary.user_added_count} user-installed, ` +
    `${viewData.baseline_summary.review_count} require review`
  } />
) : (
  <Alert variant="warning" isInline
    title="Baseline unavailable — all added packages shown as NeedsReview" />
)}
```

- [ ] **Step 2: Test in browser**

Start the dev server, load a scan with baseline data, verify:
- Banner shows with correct image ref, digest prefix, and counts
- Degraded mode shows warning banner
- Existing UI functionality is not regressed

- [ ] **Step 3: Commit**

```bash
git add inspectah-web/frontend/src/
git commit -m "feat(web): add verification banner for baseline comparison status"
```

---

## Task 10: Integration Tests and Round-Trip Proofs

**Files:**
- Modify/create: `inspectah-refine/tests/` or inline tests
- Modify: existing integration test files

- [ ] **Step 1: Snapshot round-trip test**

Test that `BaselineData` with full NEVRA serializes → deserializes → produces identical classification and `BaselineSummary`.

- [ ] **Step 2: Degraded FROM persistence test**

Test that a `--no-baseline` snapshot with resolved `target_image` preserves the correct FROM line after export and reimport (the round 2 blocker 3 proof).

- [ ] **Step 3: Pre-Phase-6 migration test**

Test that a Phase 5 snapshot (schema v14, no `target_image`/`baseline` fields) deserializes and migrates to v15 with correct defaults.

- [ ] **Step 4: Service surface agreement test**

Test that an incompatible service is:
- Excluded from `enabled_units` in Containerfile render
- Flagged with badge in service state changes
- Absent from export tarball enabled units

All three surfaces read from the same normalized state.

- [ ] **Step 5: Preview/export parity test**

Test that the Containerfile preview and the exported tarball Containerfile agree on FROM line, package list, and service enablement.

- [ ] **Step 6: Commit**

```bash
git add inspectah-refine/tests/ inspectah-pipeline/src/render/containerfile.rs
git commit -m "test: integration tests for baseline round-trip, degraded FROM, and surface agreement"
```

---

## Summary

| Task | Crate | What |
|------|-------|------|
| 1 | core | Baseline types, incompatible services constant |
| 2 | core | Resolution chain (5 strategies, UBlue tag combination) |
| 3 | core | Ref normalization gate |
| 4 | core | Snapshot schema v15 (target_image, baseline, migration) |
| 5 | collect | Baseline extraction (nsenter, entrypoint override, order proof) |
| 6 | refine | Exhaustive classification matrix + service flagging |
| 7 | pipeline + refine + web | Dynamic FROM, BaselineSummary, ViewResponse |
| 8 | cli | --base-image, --no-baseline, progress output |
| 9 | web | Verification banner component |
| 10 | cross-crate | Integration tests and round-trip proofs |

Tasks 1-4 are pure types/logic with no I/O — can be developed and tested without a VM. Task 5 requires MockExecutor enhancements. Tasks 6-7 are the core behavior changes. Task 8 wires everything together. Tasks 9-10 are polish and proof.
