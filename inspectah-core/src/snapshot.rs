use crate::types::completeness::Completeness;
use crate::types::config::ConfigSection;
use crate::types::containers::ContainerSection;
use crate::types::fleet::RepoSourceEntry;
use crate::types::kernelboot::KernelBootSection;
use crate::types::network::NetworkSection;
use crate::types::nonrpm::NonRpmSoftwareSection;
use crate::types::os::{OsRelease, SystemType};
use crate::types::preflight::PreflightResult;
use crate::types::redaction::{RedactionFinding, RedactionHint, RedactionState};
use crate::types::rpm::RpmSection;
use crate::types::scheduled::ScheduledTaskSection;
use crate::types::selinux::SelinuxSection;
use crate::types::services::ServiceSection;
use crate::types::storage::StorageSection;
use crate::types::users::UserGroupSection;
use crate::types::warnings::Warning;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const SCHEMA_VERSION: u32 = 18;

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
    /// Inspector-emitted hints about content that may need redaction.
    /// Consumed by the redaction engine to supplement pattern-based scanning.
    #[serde(default)]
    pub redaction_hints: Vec<RedactionHint>,
    /// Trust state for snapshot re-rendering. Only FullyRedacted skips redaction on import.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redaction_state: Option<RedactionState>,
    /// Artifact completeness based on inspector failure state.
    #[serde(default)]
    pub completeness: Completeness,
    /// Identity of the target image being inspected (canonical ref + resolution strategy).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_image: Option<crate::baseline::TargetImageIdentity>,
    /// Baseline package data resolved from the target image's base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline: Option<crate::baseline::BaselineData>,
    /// True if baseline resolution was attempted but failed or is unavailable.
    /// Distinguishes "no baseline" from "baseline not yet attempted".
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub no_baseline: bool,
    /// True if this snapshot intentionally retains credential material.
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub sensitive_snapshot: bool,
    /// True if password hashes were preserved by operator choice.
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub preserved_credentials: bool,
    /// True if SSH authorized keys were preserved by operator choice.
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub preserved_ssh_keys: bool,
    /// Subscription data. None if not collected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscription: Option<crate::types::subscription::SubscriptionSection>,
    /// True if subscription data was preserved by operator choice.
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub preserved_subscription: bool,
    /// Fleet snapshot metadata. None for single-host snapshots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fleet_meta: Option<crate::types::fleet::FleetSnapshotMeta>,
    /// Repo-source conflicts detected during fleet merge. Maps `name.arch`
    /// identity keys to the distinct repos with host counts. Empty for
    /// single-host snapshots.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub rpm_repo_conflicts: HashMap<String, Vec<RepoSourceEntry>>,
}

impl InspectionSnapshot {
    pub fn new() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            ..Default::default()
        }
    }

    /// Minimum accepted schema version.
    const MIN_SCHEMA: u32 = SCHEMA_VERSION;

    pub fn load(json: &str) -> Result<Self, SnapshotError> {
        let snap: Self = serde_json::from_str(json)?;
        if snap.schema_version < Self::MIN_SCHEMA || snap.schema_version > SCHEMA_VERSION {
            return Err(SnapshotError::UnsupportedVersion(snap.schema_version));
        }
        Ok(snap)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error("unsupported schema version: {0} (accepted: {max})", max = crate::snapshot::SCHEMA_VERSION)]
    UnsupportedVersion(u32),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::baseline::{
        BaselineData, BaselinePackageEntry, ResolutionStrategy, TargetImageIdentity,
    };
    use crate::types::rpm::{PackageEntry, PackageState};

    #[test]
    fn test_empty_snapshot_roundtrip() {
        let snap = InspectionSnapshot {
            schema_version: SCHEMA_VERSION,
            ..Default::default()
        };
        let json = serde_json::to_string_pretty(&snap).unwrap();
        let parsed: InspectionSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(snap.schema_version, parsed.schema_version);
        assert_eq!(snap.system_type, parsed.system_type);
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
    fn test_snapshot_carries_trust_state() {
        let mut snap = InspectionSnapshot::new();
        snap.redaction_state = Some(RedactionState::FullyRedacted {
            redacted_by: "inspectah 0.8.0".into(),
            config_hash: "abc123".into(),
        });
        snap.completeness = Completeness::Complete;
        let json = serde_json::to_string(&snap).unwrap();
        let parsed: InspectionSnapshot = serde_json::from_str(&json).unwrap();
        assert!(parsed.redaction_state.is_some());
        assert_eq!(parsed.completeness, Completeness::Complete);
    }

    #[test]
    fn test_current_version_loads() {
        let json = format!(
            r#"{{"schema_version": {}, "meta": {{}}, "system_type": "package-mode", "preflight": {{"status": "ok"}}, "warnings": [], "redactions": []}}"#,
            SCHEMA_VERSION
        );
        let snap = InspectionSnapshot::load(&json).unwrap();
        assert_eq!(snap.schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn test_old_version_rejected() {
        let json = r#"{"schema_version": 16, "meta": {}, "system_type": "package-mode", "preflight": {"status": "ok"}, "warnings": [], "redactions": []}"#;
        let result = InspectionSnapshot::load(json);
        assert!(result.is_err(), "old schema versions must be rejected");
    }

    #[test]
    fn test_future_version_rejected() {
        let json = r#"{"schema_version": 20, "meta": {}, "system_type": "package-mode", "preflight": {"status": "ok"}, "warnings": [], "redactions": []}"#;
        let result = InspectionSnapshot::load(json);
        assert!(
            result.is_err(),
            "future versions must be rejected, not silently partially-deserialized"
        );
    }

    #[test]
    fn test_snapshot_with_target_image_and_baseline() {
        let mut snap = InspectionSnapshot::new();
        snap.target_image = Some(TargetImageIdentity {
            image_ref: "quay.io/fedora/fedora-bootc:41".to_string(),
            strategy: ResolutionStrategy::OsRelease,
        });

        let mut packages = std::collections::HashMap::new();
        packages.insert(
            "systemd".to_string(),
            BaselinePackageEntry {
                name: "systemd".to_string(),
                epoch: Some("0".to_string()),
                version: "256.7".to_string(),
                release: "1.fc41".to_string(),
                arch: "x86_64".to_string(),
            },
        );

        snap.baseline = Some(BaselineData {
            image_digest: "sha256:abc123def456".to_string(),
            packages,
            extracted_at: "2026-05-16T12:00:00Z".to_string(),
        });

        let json = serde_json::to_string(&snap).unwrap();
        let parsed: InspectionSnapshot = serde_json::from_str(&json).unwrap();

        assert!(parsed.target_image.is_some());
        assert_eq!(
            parsed.target_image.as_ref().unwrap().image_ref,
            "quay.io/fedora/fedora-bootc:41"
        );
        assert!(parsed.baseline.is_some());
        assert_eq!(parsed.baseline.as_ref().unwrap().packages.len(), 1);
        assert!(!parsed.no_baseline);
    }

    #[test]
    fn test_target_image_stores_normalized_ref() {
        let mut snap = InspectionSnapshot::new();
        snap.target_image = Some(TargetImageIdentity {
            image_ref: "quay.io/fedora/fedora-bootc:41".to_string(),
            strategy: ResolutionStrategy::OsRelease,
        });

        let json = serde_json::to_string(&snap).unwrap();
        let parsed: InspectionSnapshot = serde_json::from_str(&json).unwrap();

        assert_eq!(
            parsed.target_image.unwrap().image_ref,
            "quay.io/fedora/fedora-bootc:41"
        );
    }

    #[test]
    fn test_degraded_snapshot_target_image_present_no_baseline() {
        let mut snap = InspectionSnapshot::new();
        snap.target_image = Some(TargetImageIdentity {
            image_ref: "quay.io/fedora/fedora-bootc:41".to_string(),
            strategy: ResolutionStrategy::OsRelease,
        });
        snap.baseline = None;
        snap.no_baseline = true;

        let json = serde_json::to_string(&snap).unwrap();
        let parsed: InspectionSnapshot = serde_json::from_str(&json).unwrap();

        assert!(parsed.target_image.is_some());
        assert!(parsed.baseline.is_none());
        assert!(parsed.no_baseline);
    }

    #[test]
    fn test_degraded_snapshot_both_null() {
        let mut snap = InspectionSnapshot::new();
        snap.target_image = None;
        snap.baseline = None;
        snap.no_baseline = true;

        let json = serde_json::to_string(&snap).unwrap();
        let parsed: InspectionSnapshot = serde_json::from_str(&json).unwrap();

        assert!(parsed.target_image.is_none());
        assert!(parsed.baseline.is_none());
        assert!(parsed.no_baseline);
    }

    #[test]
    fn test_snapshot_with_fleet_meta_roundtrip() {
        use std::collections::BTreeMap;

        let mut snap = InspectionSnapshot::new();
        snap.fleet_meta = Some(crate::types::fleet::FleetSnapshotMeta {
            label: "web-tier".into(),
            host_count: 25,
            hostnames: vec!["web-01".into(), "web-02".into()],
            merged_at: "2026-05-20T15:30:00Z".into(),
            baseline_provisional: true,
            section_host_counts: BTreeMap::from([("rpm".into(), 25usize), ("config".into(), 23)]),
        });
        let json = serde_json::to_string(&snap).unwrap();
        let parsed: InspectionSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(snap.fleet_meta, parsed.fleet_meta);
    }

    #[test]
    fn test_snapshot_without_fleet_meta_omits_field() {
        let snap = InspectionSnapshot::new();
        let json = serde_json::to_string(&snap).unwrap();
        assert!(!json.contains("fleet_meta"));
    }

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
}
