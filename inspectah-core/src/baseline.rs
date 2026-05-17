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

        assert_eq!(normalized.as_str(), "registry.redhat.io/rhel9/rhel-bootc:9.6");
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
}
