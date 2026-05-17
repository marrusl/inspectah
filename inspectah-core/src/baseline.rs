//! Base image resolution and baseline snapshot types for Phase 6.
//!
//! This module defines:
//! - Resolution strategies for discovering the base image reference
//! - Baseline package data extracted from the base image
//! - Target image identity (canonical ref + strategy)
//! - Version floor constants for RHEL and Fedora bootc support
//! - Incompatible systemd services constant

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

use crate::types::os::OsRelease;

/// Strategy used to resolve the base image reference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResolutionStrategy {
    /// User provided via --base-image CLI flag
    CliOverride,
    /// Extracted from /usr/share/ublue-os/image-info.json
    UniversalBlue,
    /// Extracted from bootc status
    BootcStatus,
    /// Extracted from /usr/lib/fedora-atomic-desktop/image-info.json
    FedoraAtomicDesktop,
    /// Derived from /etc/os-release (RHEL only, last resort)
    OsRelease,
}

/// Raw base image resolution result before normalization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BaseImageResolution {
    pub image_ref: String,
    pub strategy: ResolutionStrategy,
}

/// Normalized, validated container image reference.
///
/// Guarantees:
/// - Not empty
/// - No invalid characters
/// - Fully qualified (registry/namespace/image:tag or @digest)
/// - Not a local-only reference (localhost:*, unix:*)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NormalizedImageRef {
    ref_string: String,
}

impl NormalizedImageRef {
    /// Construct from a pre-validated string.
    ///
    /// Caller must ensure the string passed validation.
    pub fn from_validated(s: String) -> Self {
        Self { ref_string: s }
    }

    /// Access the normalized reference as a string slice.
    pub fn as_str(&self) -> &str {
        &self.ref_string
    }
}

impl fmt::Display for NormalizedImageRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.ref_string)
    }
}

/// A single package entry in the baseline snapshot.
///
/// Represents a package NEVRA (Name-Epoch-Version-Release-Arch).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BaselinePackageEntry {
    pub name: String,
    pub epoch: Option<String>,
    pub version: String,
    pub release: String,
    pub arch: String,
}

/// Baseline package data extracted from the base image.
///
/// Contains the package inventory snapshot and extraction metadata.
/// Does NOT store the image ref — that lives in `TargetImageIdentity`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BaselineData {
    pub image_digest: String,
    pub packages: HashMap<String, BaselinePackageEntry>,
    pub extracted_at: String,
}

/// Canonical target image identity for the entire system.
///
/// Stores the normalized image reference and the strategy used to resolve it.
/// This is the single authoritative source for "what base image are we scanning?"
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TargetImageIdentity {
    pub image_ref: String,
    pub strategy: ResolutionStrategy,
}

/// Universal Blue metadata from /usr/share/ublue-os/image-info.json.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

/// A systemd service known to be incompatible with immutable /usr.
#[derive(Debug, Clone, PartialEq)]
pub struct IncompatibleServiceEntry {
    pub unit: &'static str,
    pub reason: &'static str,
}

/// Systemd services incompatible with immutable /usr.
///
/// These are package-manager services that assume a mutable /usr.
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

/// Minimum RHEL bootc versions: (major, min_version).
///
/// RHEL 9.6+ and RHEL 10.0+ support bootc.
pub const RHEL_BOOTC_MIN: &[(&str, &str)] = &[("9", "9.6"), ("10", "10.0")];

/// Minimum Fedora bootc version.
///
/// Fedora 41+ supports bootc.
pub const FEDORA_BOOTC_MIN: u32 = 41;

/// Errors during base image resolution.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolutionError {
    MalformedUblueMetadata { path: String, reason: String },
    UnknownDistro { id: String },
    NoResolution(String),
}

impl fmt::Display for ResolutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedUblueMetadata { path, reason } => {
                write!(f, "malformed ublue metadata at {}: {}", path, reason)
            }
            Self::UnknownDistro { id } => write!(f, "unknown distro: {}", id),
            Self::NoResolution(msg) => write!(f, "no resolution: {}", msg),
        }
    }
}

impl std::error::Error for ResolutionError {}

/// Errors during image reference normalization.
#[derive(Debug, Clone, PartialEq)]
pub enum NormalizationError {
    Empty,
    InvalidCharacters(String),
    NotFullyQualified(String),
    LocalOnly(String),
}

impl fmt::Display for NormalizationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "image reference is empty"),
            Self::InvalidCharacters(s) => write!(f, "invalid characters in reference: {}", s),
            Self::NotFullyQualified(s) => write!(f, "reference not fully qualified: {}", s),
            Self::LocalOnly(s) => write!(f, "local-only reference not allowed: {}", s),
        }
    }
}

impl std::error::Error for NormalizationError {}

// ---------------------------------------------------------------------------
// Transport prefixes stripped from UBlue image-ref values
// ---------------------------------------------------------------------------

const TRANSPORT_PREFIXES: &[&str] = &[
    "ostree-image-signed:docker://",
    "docker://",
    "containers-storage:",
];

/// Known Fedora Atomic desktop variant IDs.
const FEDORA_ATOMIC_DESKTOP_VARIANTS: &[&str] = &[
    "silverblue",
    "kinoite",
    "sway-atomic",
    "budgie-atomic",
    "cosmic-atomic",
    "lxqt-atomic",
    "xfce-atomic",
];

/// UBlue metadata path.
pub const UBLUE_METADATA_PATH: &str = "/usr/share/ublue-os/image-info.json";

// ---------------------------------------------------------------------------
// Version clamping
// ---------------------------------------------------------------------------

/// Compare dot-separated integer version strings, return max(version, minimum).
///
/// If either string is unparseable, returns `minimum` (fail safe).
pub fn clamp_version(version: &str, minimum: &str) -> String {
    let v_parts = match parse_version_parts(version) {
        Some(p) => p,
        None => return minimum.to_string(),
    };
    let m_parts = match parse_version_parts(minimum) {
        Some(p) => p,
        None => return minimum.to_string(),
    };

    let max_len = v_parts.len().max(m_parts.len());
    for i in 0..max_len {
        let v = v_parts.get(i).copied().unwrap_or(0);
        let m = m_parts.get(i).copied().unwrap_or(0);
        if v < m {
            return minimum.to_string();
        }
        if v > m {
            return version.to_string();
        }
    }
    version.to_string()
}

/// Split a version string like "9.6" into [9, 6].
fn parse_version_parts(version: &str) -> Option<Vec<u32>> {
    if version.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    for p in version.split('.') {
        match p.parse::<u32>() {
            Ok(n) => parts.push(n),
            Err(_) => return None,
        }
    }
    Some(parts)
}

// ---------------------------------------------------------------------------
// Base image resolution
// ---------------------------------------------------------------------------

/// Strip known container transport prefixes from a reference string.
fn strip_transport_prefix(s: &str) -> &str {
    for prefix in TRANSPORT_PREFIXES {
        if let Some(rest) = s.strip_prefix(prefix) {
            return rest;
        }
    }
    s
}

/// Check whether a reference has a tag (`:` after the last `/`).
fn ref_has_tag(r: &str) -> bool {
    match r.rfind('/') {
        Some(slash_pos) => r[slash_pos..].contains(':'),
        None => r.contains(':'),
    }
}

/// Resolve the base image reference from available sources.
///
/// Resolution chain (first match wins):
/// 1. CLI override → `CliOverride`
/// 2. UBlue metadata → `UniversalBlue`
/// 3. bootc status ref → `BootcStatus`
/// 4. Fedora Atomic desktop (variant in known set) → `FedoraAtomicDesktop`
/// 5. os-release mapping with version clamping → `OsRelease`
pub fn resolve_base_image(
    os_release: &OsRelease,
    ublue: Option<&UblueMetadata>,
    bootc_status_ref: Option<&str>,
    cli_override: Option<&str>,
) -> Result<BaseImageResolution, ResolutionError> {
    // 1. CLI override
    if let Some(override_ref) = cli_override {
        if !override_ref.is_empty() {
            return Ok(BaseImageResolution {
                image_ref: override_ref.to_string(),
                strategy: ResolutionStrategy::CliOverride,
            });
        }
    }

    // 2. UBlue metadata
    if let Some(ub) = ublue {
        return resolve_ublue(ub);
    }

    // 3. bootc status
    if let Some(bref) = bootc_status_ref {
        if !bref.is_empty() {
            return Ok(BaseImageResolution {
                image_ref: bref.to_string(),
                strategy: ResolutionStrategy::BootcStatus,
            });
        }
    }

    // 4. Fedora Atomic desktop
    if os_release.id == "fedora" && !os_release.variant_id.is_empty() {
        if FEDORA_ATOMIC_DESKTOP_VARIANTS.contains(&os_release.variant_id.as_str()) {
            let image_ref = format!(
                "quay.io/fedora-ostree-desktops/{}:{}",
                os_release.variant_id, os_release.version_id
            );
            return Ok(BaseImageResolution {
                image_ref,
                strategy: ResolutionStrategy::FedoraAtomicDesktop,
            });
        }
    }

    // 5. os-release mapping with version clamping
    resolve_from_os_release(os_release)
}

/// Resolve a UBlue metadata struct to a base image reference.
fn resolve_ublue(ub: &UblueMetadata) -> Result<BaseImageResolution, ResolutionError> {
    // Path A: image-ref present
    if let Some(ref raw_ref) = ub.image_ref {
        if !raw_ref.is_empty() {
            let stripped = strip_transport_prefix(raw_ref);
            if ref_has_tag(stripped) {
                // Already tagged — use as-is
                return Ok(BaseImageResolution {
                    image_ref: stripped.to_string(),
                    strategy: ResolutionStrategy::UniversalBlue,
                });
            }
            // Tagless — need image-tag to combine
            if let Some(ref tag) = ub.image_tag {
                if !tag.is_empty() {
                    return Ok(BaseImageResolution {
                        image_ref: format!("{}:{}", stripped, tag),
                        strategy: ResolutionStrategy::UniversalBlue,
                    });
                }
            }
            // Tagless ref without image-tag → fail closed
            return Err(ResolutionError::MalformedUblueMetadata {
                path: UBLUE_METADATA_PATH.to_string(),
                reason: "image-ref has no tag and no image-tag field".to_string(),
            });
        }
    }

    // Path B: no image-ref — synthesize from vendor/name/tag
    let vendor = ub.image_vendor.as_deref().unwrap_or("");
    let name = ub.image_name.as_deref().unwrap_or("");
    let tag = ub.image_tag.as_deref().unwrap_or("");

    if !vendor.is_empty() && !name.is_empty() && !tag.is_empty() {
        return Ok(BaseImageResolution {
            image_ref: format!("ghcr.io/{}/{}:{}", vendor, name, tag),
            strategy: ResolutionStrategy::UniversalBlue,
        });
    }

    // Missing fields → fail closed
    Err(ResolutionError::MalformedUblueMetadata {
        path: UBLUE_METADATA_PATH.to_string(),
        reason: "missing required fields for synthesis (need vendor, name, tag)".to_string(),
    })
}

/// Map os-release fields to a base image with version clamping.
fn resolve_from_os_release(os_release: &OsRelease) -> Result<BaseImageResolution, ResolutionError> {
    let id = os_release.id.as_str();
    let version_id = os_release.version_id.as_str();
    let major = version_id.split('.').next().unwrap_or("");

    match id {
        "rhel" => {
            // Find the minimum version for this major
            let effective = RHEL_BOOTC_MIN
                .iter()
                .find(|(maj, _)| *maj == major)
                .map(|(_, min)| clamp_version(version_id, min))
                .unwrap_or_else(|| version_id.to_string());
            Ok(BaseImageResolution {
                image_ref: format!("registry.redhat.io/rhel{}/rhel-bootc:{}", major, effective),
                strategy: ResolutionStrategy::OsRelease,
            })
        }
        "centos" => Ok(BaseImageResolution {
            image_ref: format!("quay.io/centos-bootc/centos-bootc:stream{}", major),
            strategy: ResolutionStrategy::OsRelease,
        }),
        "fedora" => {
            let effective = clamp_version(major, &FEDORA_BOOTC_MIN.to_string());
            Ok(BaseImageResolution {
                image_ref: format!("quay.io/fedora/fedora-bootc:{}", effective),
                strategy: ResolutionStrategy::OsRelease,
            })
        }
        _ => Err(ResolutionError::UnknownDistro { id: id.to_string() }),
    }
}

// ---------------------------------------------------------------------------
// Image reference normalization
// ---------------------------------------------------------------------------

/// Shell metacharacters that are forbidden in container image references.
const SHELL_METACHARACTERS: &[char] = &[
    '$', '`', '|', ';', '&', '(', ')', '{', '}', '<', '>', '\n', '\r', '!', '#',
];

/// Normalize and validate a container image reference.
///
/// Validation rules (in order):
/// 1. Non-empty, no whitespace, no shell metacharacters
/// 2. Strip known transport prefixes
/// 3. Reject localhost/ and post-strip containers-storage (local-only)
/// 4. Must be fully qualified (registry hostname contains . or :)
/// 5. If @ present → digest ref, preserve as-is
/// 6. If tag present → preserve
/// 7. No tag, no digest → append :latest
///
/// Returns a `NormalizedImageRef` guaranteed to be:
/// - Non-empty
/// - Free of invalid characters
/// - Fully qualified (registry/namespace/image:tag or @digest)
/// - Not a local-only reference
pub fn normalize_image_ref(raw: &str) -> Result<NormalizedImageRef, NormalizationError> {
    // 1. Non-empty
    if raw.is_empty() {
        return Err(NormalizationError::Empty);
    }

    // 1. No whitespace
    if raw.chars().any(|c| c.is_whitespace()) {
        return Err(NormalizationError::InvalidCharacters(raw.to_string()));
    }

    // 1. No shell metacharacters
    if raw.chars().any(|c| SHELL_METACHARACTERS.contains(&c)) {
        return Err(NormalizationError::InvalidCharacters(raw.to_string()));
    }

    // 2. Strip known transport prefixes
    let working = strip_transport_prefix(raw);

    // 3. Reject localhost/ without port (local-only)
    if working.starts_with("localhost/") {
        return Err(NormalizationError::LocalOnly(working.to_string()));
    }

    // 3. Reject post-strip containers-storage (local-only)
    // containers-storage: prefix was stripped, but the ref itself is local
    if raw.starts_with("containers-storage:") {
        return Err(NormalizationError::LocalOnly(working.to_string()));
    }

    // 4. Must be fully qualified: first component (before first /) must contain . or :
    // This validates it as a registry hostname, not a short name like "foo/bar:tag"
    // If there's no '/', the ref is definitely not fully qualified
    if !working.contains('/') {
        return Err(NormalizationError::NotFullyQualified(working.to_string()));
    }
    let first_component = working.split('/').next().unwrap_or("");
    if !first_component.contains('.') && !first_component.contains(':') {
        return Err(NormalizationError::NotFullyQualified(working.to_string()));
    }

    // 5. If @ present → digest ref, preserve as-is
    if working.contains('@') {
        return Ok(NormalizedImageRef::from_validated(working.to_string()));
    }

    // 6. If tag present → preserve
    if ref_has_tag(working) {
        return Ok(NormalizedImageRef::from_validated(working.to_string()));
    }

    // 7. No tag, no digest → append :latest
    let normalized = format!("{}:latest", working);
    Ok(NormalizedImageRef::from_validated(normalized))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_data_serde_roundtrip() {
        let mut packages = HashMap::new();
        packages.insert(
            "glibc".to_string(),
            BaselinePackageEntry {
                name: "glibc".to_string(),
                epoch: Some("0".to_string()),
                version: "2.34".to_string(),
                release: "100.el9".to_string(),
                arch: "x86_64".to_string(),
            },
        );
        packages.insert(
            "kernel".to_string(),
            BaselinePackageEntry {
                name: "kernel".to_string(),
                epoch: None,
                version: "5.14.0".to_string(),
                release: "503.el9".to_string(),
                arch: "x86_64".to_string(),
            },
        );

        let baseline = BaselineData {
            image_digest: "sha256:abc123".to_string(),
            packages,
            extracted_at: "2026-05-16T12:00:00Z".to_string(),
        };

        let json = serde_json::to_string(&baseline).unwrap();
        let roundtrip: BaselineData = serde_json::from_str(&json).unwrap();

        assert_eq!(baseline, roundtrip);
        assert_eq!(roundtrip.packages.len(), 2);
        assert_eq!(
            roundtrip.packages.get("glibc").unwrap().epoch,
            Some("0".to_string())
        );
        assert_eq!(roundtrip.packages.get("kernel").unwrap().epoch, None);
    }

    #[test]
    fn resolution_strategy_serde_kebab_case() {
        let strategies = vec![
            (ResolutionStrategy::CliOverride, "\"cli-override\""),
            (ResolutionStrategy::UniversalBlue, "\"universal-blue\""),
            (ResolutionStrategy::BootcStatus, "\"bootc-status\""),
            (
                ResolutionStrategy::FedoraAtomicDesktop,
                "\"fedora-atomic-desktop\"",
            ),
            (ResolutionStrategy::OsRelease, "\"os-release\""),
        ];

        for (strategy, expected_json) in strategies {
            let json = serde_json::to_string(&strategy).unwrap();
            assert_eq!(json, expected_json);

            let roundtrip: ResolutionStrategy = serde_json::from_str(&json).unwrap();
            assert_eq!(roundtrip, strategy);
        }
    }

    #[test]
    fn target_image_identity_roundtrip() {
        let identity = TargetImageIdentity {
            image_ref: "registry.redhat.io/rhel9/rhel-bootc:9.6".to_string(),
            strategy: ResolutionStrategy::BootcStatus,
        };

        let json = serde_json::to_string(&identity).unwrap();
        let roundtrip: TargetImageIdentity = serde_json::from_str(&json).unwrap();

        assert_eq!(identity, roundtrip);
    }

    #[test]
    fn incompatible_services_constant() {
        assert_eq!(INCOMPATIBLE_SERVICES.len(), 4);

        let unit_names: Vec<&str> = INCOMPATIBLE_SERVICES.iter().map(|e| e.unit).collect();
        assert!(unit_names.contains(&"dnf-makecache.service"));
        assert!(unit_names.contains(&"dnf-makecache.timer"));
        assert!(unit_names.contains(&"packagekit.service"));
        assert!(unit_names.contains(&"packagekit-offline-update.service"));

        for entry in INCOMPATIBLE_SERVICES {
            assert!(entry.reason.contains("package-manager"));
            assert!(entry.reason.contains("immutable /usr"));
        }
    }

    #[test]
    fn version_floor_constants() {
        assert_eq!(RHEL_BOOTC_MIN.len(), 2);
        assert_eq!(RHEL_BOOTC_MIN[0], ("9", "9.6"));
        assert_eq!(RHEL_BOOTC_MIN[1], ("10", "10.0"));

        assert_eq!(FEDORA_BOOTC_MIN, 41);
    }

    #[test]
    fn normalized_image_ref_display() {
        let normalized = NormalizedImageRef::from_validated(
            "registry.redhat.io/rhel9/rhel-bootc:9.6".to_string(),
        );

        assert_eq!(
            normalized.as_str(),
            "registry.redhat.io/rhel9/rhel-bootc:9.6"
        );
        assert_eq!(
            format!("{}", normalized),
            "registry.redhat.io/rhel9/rhel-bootc:9.6"
        );
    }

    #[test]
    fn ublue_metadata_serde() {
        let json = r#"{
            "image-ref": "ghcr.io/ublue-os/base-main:41",
            "image-tag": "41",
            "image-name": "base-main",
            "image-vendor": "ublue-os"
        }"#;

        let metadata: UblueMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(
            metadata.image_ref,
            Some("ghcr.io/ublue-os/base-main:41".to_string())
        );
        assert_eq!(metadata.image_tag, Some("41".to_string()));
        assert_eq!(metadata.image_name, Some("base-main".to_string()));
        assert_eq!(metadata.image_vendor, Some("ublue-os".to_string()));
    }

    // -----------------------------------------------------------------------
    // clamp_version tests
    // -----------------------------------------------------------------------

    #[test]
    fn clamp_version_below_minimum() {
        assert_eq!(clamp_version("9.4", "9.6"), "9.6");
    }

    #[test]
    fn clamp_version_at_minimum() {
        assert_eq!(clamp_version("9.6", "9.6"), "9.6");
    }

    #[test]
    fn clamp_version_above_minimum() {
        assert_eq!(clamp_version("9.8", "9.6"), "9.8");
    }

    #[test]
    fn clamp_version_different_major() {
        assert_eq!(clamp_version("10.0", "10.0"), "10.0");
    }

    #[test]
    fn clamp_version_unparseable_returns_minimum() {
        assert_eq!(clamp_version("abc", "9.6"), "9.6");
    }

    #[test]
    fn clamp_version_empty_returns_minimum() {
        assert_eq!(clamp_version("", "9.6"), "9.6");
    }

    #[test]
    fn clamp_version_single_component() {
        assert_eq!(clamp_version("40", "41"), "41");
        assert_eq!(clamp_version("42", "41"), "42");
    }

    // -----------------------------------------------------------------------
    // resolve_base_image tests
    // -----------------------------------------------------------------------

    fn make_os_release(id: &str, version_id: &str, variant_id: &str) -> OsRelease {
        OsRelease {
            id: id.to_string(),
            version_id: version_id.to_string(),
            variant_id: variant_id.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn resolution_cli_override_wins() {
        let os = make_os_release("rhel", "9.4", "");
        let result = resolve_base_image(&os, None, None, Some("my-custom:latest")).unwrap();
        assert_eq!(result.image_ref, "my-custom:latest");
        assert_eq!(result.strategy, ResolutionStrategy::CliOverride);
    }

    #[test]
    fn resolution_ublue_transport_prefixed_tagless_ref_with_tag() {
        let os = make_os_release("fedora", "41", "");
        let ub = UblueMetadata {
            image_ref: Some("ostree-image-signed:docker://ghcr.io/ublue-os/bazzite".to_string()),
            image_tag: Some("stable".to_string()),
            image_name: Some("bazzite".to_string()),
            image_vendor: Some("ublue-os".to_string()),
        };
        let result = resolve_base_image(&os, Some(&ub), None, None).unwrap();
        assert_eq!(result.image_ref, "ghcr.io/ublue-os/bazzite:stable");
        assert_eq!(result.strategy, ResolutionStrategy::UniversalBlue);
    }

    #[test]
    fn resolution_ublue_already_tagged_ref() {
        let os = make_os_release("fedora", "41", "silverblue");
        let ub = UblueMetadata {
            image_ref: Some("ghcr.io/ublue-os/bazzite:stable".to_string()),
            image_tag: None,
            image_name: Some("bazzite".to_string()),
            image_vendor: Some("ublue-os".to_string()),
        };
        let result = resolve_base_image(&os, Some(&ub), None, None).unwrap();
        assert_eq!(result.image_ref, "ghcr.io/ublue-os/bazzite:stable");
        assert_eq!(result.strategy, ResolutionStrategy::UniversalBlue);
    }

    #[test]
    fn resolution_ublue_synthesis_fallback() {
        let os = make_os_release("fedora", "40", "");
        let ub = UblueMetadata {
            image_ref: None,
            image_tag: Some("40".to_string()),
            image_name: Some("bazzite".to_string()),
            image_vendor: Some("ublue-os".to_string()),
        };
        let result = resolve_base_image(&os, Some(&ub), None, None).unwrap();
        assert_eq!(result.image_ref, "ghcr.io/ublue-os/bazzite:40");
        assert_eq!(result.strategy, ResolutionStrategy::UniversalBlue);
    }

    #[test]
    fn resolution_ublue_tagless_ref_no_image_tag_fails_closed() {
        let os = make_os_release("fedora", "41", "");
        let ub = UblueMetadata {
            image_ref: Some("ghcr.io/ublue-os/bazzite".to_string()),
            image_tag: None,
            image_name: Some("bazzite".to_string()),
            image_vendor: Some("ublue-os".to_string()),
        };
        let result = resolve_base_image(&os, Some(&ub), None, None);
        assert!(result.is_err());
        match result.unwrap_err() {
            ResolutionError::MalformedUblueMetadata { path, reason } => {
                assert_eq!(path, UBLUE_METADATA_PATH);
                assert!(reason.contains("no tag"));
            }
            other => panic!("expected MalformedUblueMetadata, got {:?}", other),
        }
    }

    #[test]
    fn resolution_ublue_malformed_metadata() {
        let os = make_os_release("fedora", "41", "");
        // No image-ref, no image-tag, missing vendor
        let ub = UblueMetadata {
            image_ref: None,
            image_tag: None,
            image_name: Some("bazzite".to_string()),
            image_vendor: None,
        };
        let result = resolve_base_image(&os, Some(&ub), None, None);
        assert!(result.is_err());
        match result.unwrap_err() {
            ResolutionError::MalformedUblueMetadata { .. } => {}
            other => panic!("expected MalformedUblueMetadata, got {:?}", other),
        }
    }

    #[test]
    fn resolution_bootc_status_ref() {
        let os = make_os_release("fedora", "41", "");
        let result =
            resolve_base_image(&os, None, Some("quay.io/fedora/fedora-bootc:41"), None).unwrap();
        assert_eq!(result.image_ref, "quay.io/fedora/fedora-bootc:41");
        assert_eq!(result.strategy, ResolutionStrategy::BootcStatus);
    }

    #[test]
    fn resolution_fedora_atomic_desktop_before_generic_fedora() {
        let os = make_os_release("fedora", "41", "silverblue");
        let result = resolve_base_image(&os, None, None, None).unwrap();
        assert_eq!(
            result.image_ref,
            "quay.io/fedora-ostree-desktops/silverblue:41"
        );
        assert_eq!(result.strategy, ResolutionStrategy::FedoraAtomicDesktop);
    }

    #[test]
    fn resolution_generic_fedora_no_variant() {
        let os = make_os_release("fedora", "42", "");
        let result = resolve_base_image(&os, None, None, None).unwrap();
        assert_eq!(result.image_ref, "quay.io/fedora/fedora-bootc:42");
        assert_eq!(result.strategy, ResolutionStrategy::OsRelease);
    }

    #[test]
    fn resolution_centos_stream() {
        let os = make_os_release("centos", "9", "");
        let result = resolve_base_image(&os, None, None, None).unwrap();
        assert_eq!(
            result.image_ref,
            "quay.io/centos-bootc/centos-bootc:stream9"
        );
        assert_eq!(result.strategy, ResolutionStrategy::OsRelease);
    }

    #[test]
    fn resolution_rhel() {
        let os = make_os_release("rhel", "9.6", "");
        let result = resolve_base_image(&os, None, None, None).unwrap();
        assert_eq!(result.image_ref, "registry.redhat.io/rhel9/rhel-bootc:9.6");
        assert_eq!(result.strategy, ResolutionStrategy::OsRelease);
    }

    #[test]
    fn resolution_rhel_version_floor_clamped() {
        // RHEL 9.4 → clamped to 9.6
        let os = make_os_release("rhel", "9.4", "");
        let result = resolve_base_image(&os, None, None, None).unwrap();
        assert_eq!(result.image_ref, "registry.redhat.io/rhel9/rhel-bootc:9.6");
    }

    #[test]
    fn resolution_rhel_version_floor_at_minimum() {
        // RHEL 9.6 → no clamping
        let os = make_os_release("rhel", "9.6", "");
        let result = resolve_base_image(&os, None, None, None).unwrap();
        assert_eq!(result.image_ref, "registry.redhat.io/rhel9/rhel-bootc:9.6");
    }

    #[test]
    fn resolution_rhel10_version_floor() {
        // RHEL 10.0 → at floor, no clamping
        let os = make_os_release("rhel", "10.0", "");
        let result = resolve_base_image(&os, None, None, None).unwrap();
        assert_eq!(
            result.image_ref,
            "registry.redhat.io/rhel10/rhel-bootc:10.0"
        );
    }

    #[test]
    fn resolution_fedora_version_floor_clamped() {
        // Fedora 40 → clamped to 41
        let os = make_os_release("fedora", "40", "");
        let result = resolve_base_image(&os, None, None, None).unwrap();
        assert_eq!(result.image_ref, "quay.io/fedora/fedora-bootc:41");
    }

    #[test]
    fn resolution_fedora_version_floor_above() {
        // Fedora 42 → above floor, no clamping
        let os = make_os_release("fedora", "42", "");
        let result = resolve_base_image(&os, None, None, None).unwrap();
        assert_eq!(result.image_ref, "quay.io/fedora/fedora-bootc:42");
    }

    #[test]
    fn resolution_unknown_distro() {
        let os = make_os_release("suse", "15.5", "");
        let result = resolve_base_image(&os, None, None, None);
        assert!(result.is_err());
        match result.unwrap_err() {
            ResolutionError::UnknownDistro { id } => assert_eq!(id, "suse"),
            other => panic!("expected UnknownDistro, got {:?}", other),
        }
    }

    #[test]
    fn resolution_all_seven_desktop_variants() {
        let variants = [
            "silverblue",
            "kinoite",
            "sway-atomic",
            "budgie-atomic",
            "cosmic-atomic",
            "lxqt-atomic",
            "xfce-atomic",
        ];
        for variant in &variants {
            let os = make_os_release("fedora", "41", variant);
            let result = resolve_base_image(&os, None, None, None).unwrap();
            assert_eq!(
                result.image_ref,
                format!("quay.io/fedora-ostree-desktops/{}:41", variant),
                "variant {} did not resolve correctly",
                variant
            );
            assert_eq!(result.strategy, ResolutionStrategy::FedoraAtomicDesktop);
        }
    }

    // -----------------------------------------------------------------------
    // Transport prefix stripping
    // -----------------------------------------------------------------------

    #[test]
    fn strip_transport_ostree_image_signed() {
        assert_eq!(
            strip_transport_prefix("ostree-image-signed:docker://ghcr.io/ublue-os/bazzite:stable"),
            "ghcr.io/ublue-os/bazzite:stable"
        );
    }

    #[test]
    fn strip_transport_docker() {
        assert_eq!(
            strip_transport_prefix("docker://ghcr.io/ublue-os/bazzite:stable"),
            "ghcr.io/ublue-os/bazzite:stable"
        );
    }

    #[test]
    fn strip_transport_containers_storage() {
        assert_eq!(
            strip_transport_prefix("containers-storage:localhost/myimage:latest"),
            "localhost/myimage:latest"
        );
    }

    #[test]
    fn strip_transport_none() {
        assert_eq!(
            strip_transport_prefix("ghcr.io/ublue-os/bazzite:stable"),
            "ghcr.io/ublue-os/bazzite:stable"
        );
    }

    // -----------------------------------------------------------------------
    // ref_has_tag
    // -----------------------------------------------------------------------

    #[test]
    fn ref_has_tag_with_tag() {
        assert!(ref_has_tag("ghcr.io/ublue-os/bazzite:stable"));
    }

    #[test]
    fn ref_has_tag_without_tag() {
        assert!(!ref_has_tag("ghcr.io/ublue-os/bazzite"));
    }

    // -----------------------------------------------------------------------
    // normalize_image_ref tests
    // -----------------------------------------------------------------------

    #[test]
    fn normalize_empty_ref() {
        let result = normalize_image_ref("");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), NormalizationError::Empty);
    }

    #[test]
    fn normalize_whitespace_rejected() {
        let cases = vec![
            " registry.redhat.io/rhel9/rhel-bootc:9.6",
            "registry.redhat.io/rhel9/rhel-bootc:9.6 ",
            "registry.redhat.io/rhel9/rhel-bootc :9.6",
            "registry.redhat.io/rhel9 /rhel-bootc:9.6",
        ];
        for case in cases {
            let result = normalize_image_ref(case);
            assert!(result.is_err(), "should reject whitespace in: {}", case);
            match result.unwrap_err() {
                NormalizationError::InvalidCharacters(_) => {}
                other => panic!("expected InvalidCharacters, got {:?}", other),
            }
        }
    }

    #[test]
    fn normalize_shell_metacharacters_rejected() {
        let metacharacters = vec![
            "$", "`", "|", ";", "&", "(", ")", "{", "}", "<", ">", "\n", "!", "#",
        ];
        for mc in metacharacters {
            let bad_ref = format!("registry.redhat.io/rhel9/rhel-bootc{}:9.6", mc);
            let result = normalize_image_ref(&bad_ref);
            assert!(
                result.is_err(),
                "should reject shell metacharacter '{}' in: {}",
                mc,
                bad_ref
            );
            match result.unwrap_err() {
                NormalizationError::InvalidCharacters(_) => {}
                other => panic!("expected InvalidCharacters for '{}', got {:?}", mc, other),
            }
        }
    }

    #[test]
    fn normalize_shell_metacharacters_in_tag() {
        let result = normalize_image_ref("registry.redhat.io/rhel9/rhel-bootc:9.6;echo");
        assert!(result.is_err());
        match result.unwrap_err() {
            NormalizationError::InvalidCharacters(_) => {}
            other => panic!("expected InvalidCharacters, got {:?}", other),
        }
    }

    #[test]
    fn normalize_strips_ostree_image_signed_prefix() {
        let result = normalize_image_ref(
            "ostree-image-signed:docker://registry.redhat.io/rhel9/rhel-bootc:9.6",
        )
        .unwrap();
        assert_eq!(result.as_str(), "registry.redhat.io/rhel9/rhel-bootc:9.6");
    }

    #[test]
    fn normalize_strips_docker_prefix() {
        let result =
            normalize_image_ref("docker://quay.io/centos-bootc/centos-bootc:stream9").unwrap();
        assert_eq!(result.as_str(), "quay.io/centos-bootc/centos-bootc:stream9");
    }

    #[test]
    fn normalize_strips_containers_storage_prefix() {
        // containers-storage is local-only → should be rejected AFTER stripping
        let result = normalize_image_ref("containers-storage:localhost/myimage:latest");
        assert!(result.is_err());
        match result.unwrap_err() {
            NormalizationError::LocalOnly(_) => {}
            other => panic!("expected LocalOnly, got {:?}", other),
        }
    }

    #[test]
    fn normalize_bare_ref_without_registry() {
        // rhel-bootc:9.6 has no registry → not fully qualified
        let result = normalize_image_ref("rhel-bootc:9.6");
        assert!(result.is_err());
        match result.unwrap_err() {
            NormalizationError::NotFullyQualified(_) => {}
            other => panic!("expected NotFullyQualified, got {:?}", other),
        }
    }

    #[test]
    fn normalize_namespace_only_ref() {
        // foo/bar:tag has no dot or port in first component → not a registry
        let result = normalize_image_ref("foo/bar:tag");
        assert!(result.is_err());
        match result.unwrap_err() {
            NormalizationError::NotFullyQualified(_) => {}
            other => panic!("expected NotFullyQualified, got {:?}", other),
        }
    }

    #[test]
    fn normalize_registry_with_dot_accepted() {
        let result = normalize_image_ref("registry.redhat.io/rhel9/rhel-bootc:9.6").unwrap();
        assert_eq!(result.as_str(), "registry.redhat.io/rhel9/rhel-bootc:9.6");
    }

    #[test]
    fn normalize_registry_with_port_accepted() {
        let result = normalize_image_ref("localhost:5000/myimage:latest").unwrap();
        assert_eq!(result.as_str(), "localhost:5000/myimage:latest");
    }

    #[test]
    fn normalize_localhost_without_port_rejected() {
        let result = normalize_image_ref("localhost/foo:tag");
        assert!(result.is_err());
        match result.unwrap_err() {
            NormalizationError::LocalOnly(_) => {}
            other => panic!("expected LocalOnly, got {:?}", other),
        }
    }

    #[test]
    fn normalize_no_tag_no_digest_appends_latest() {
        let result = normalize_image_ref("registry.redhat.io/rhel9/rhel-bootc").unwrap();
        assert_eq!(
            result.as_str(),
            "registry.redhat.io/rhel9/rhel-bootc:latest"
        );
    }

    #[test]
    fn normalize_digest_preserved() {
        let result =
            normalize_image_ref("registry.redhat.io/rhel9/rhel-bootc@sha256:abc123def456").unwrap();
        assert_eq!(
            result.as_str(),
            "registry.redhat.io/rhel9/rhel-bootc@sha256:abc123def456"
        );
    }

    #[test]
    fn normalize_tag_preserved() {
        let result = normalize_image_ref("registry.redhat.io/rhel9/rhel-bootc:9.6").unwrap();
        assert_eq!(result.as_str(), "registry.redhat.io/rhel9/rhel-bootc:9.6");
    }

    #[test]
    fn normalize_digest_with_tag_preserves_both() {
        let result =
            normalize_image_ref("registry.redhat.io/rhel9/rhel-bootc:9.6@sha256:abc123").unwrap();
        assert_eq!(
            result.as_str(),
            "registry.redhat.io/rhel9/rhel-bootc:9.6@sha256:abc123"
        );
    }

    #[test]
    fn normalize_quay_io() {
        let result = normalize_image_ref("quay.io/fedora/fedora-bootc:41").unwrap();
        assert_eq!(result.as_str(), "quay.io/fedora/fedora-bootc:41");
    }

    #[test]
    fn normalize_ghcr_io() {
        let result = normalize_image_ref("ghcr.io/ublue-os/bazzite:stable").unwrap();
        assert_eq!(result.as_str(), "ghcr.io/ublue-os/bazzite:stable");
    }

    // -----------------------------------------------------------------------
    // Step 8: UBlue fail-closed helper tests
    // -----------------------------------------------------------------------

    #[test]
    fn malformed_ublue_json_deserializes_to_none_fields() {
        // Malformed JSON at the UBlue metadata path: valid JSON but wrong shape
        let malformed_json = r#"{"not-a-field": 42, "garbage": true}"#;
        let metadata: UblueMetadata = serde_json::from_str(malformed_json).unwrap();
        // All fields should be None since none of the expected keys are present
        assert!(
            metadata.image_ref.is_none(),
            "image_ref must be None for malformed JSON"
        );
        assert!(
            metadata.image_tag.is_none(),
            "image_tag must be None for malformed JSON"
        );
        assert!(
            metadata.image_name.is_none(),
            "image_name must be None for malformed JSON"
        );
        assert!(
            metadata.image_vendor.is_none(),
            "image_vendor must be None for malformed JSON"
        );
    }

    #[test]
    fn invalid_ublue_json_produces_none() {
        // Completely invalid JSON (not parseable)
        let invalid_json = r#"not json at all {{{{"#;
        let result: Result<UblueMetadata, _> = serde_json::from_str(invalid_json);
        assert!(result.is_err(), "invalid JSON must produce a parse error");
    }

    #[test]
    fn resolve_with_malformed_ublue_metadata_returns_error() {
        let os = make_os_release("fedora", "41", "");
        // UblueMetadata with all None fields — cannot synthesize a ref
        let ub = UblueMetadata {
            image_ref: None,
            image_tag: None,
            image_name: None,
            image_vendor: None,
        };
        let result = resolve_base_image(&os, Some(&ub), None, None);
        assert!(
            result.is_err(),
            "malformed UblueMetadata must produce an error"
        );
        match result.unwrap_err() {
            ResolutionError::MalformedUblueMetadata { .. } => {}
            other => panic!("expected MalformedUblueMetadata, got {:?}", other),
        }
    }
}
