use crate::types::completeness::Completeness;
use crate::types::config::ConfigSection;
use crate::types::containers::ContainerSection;
use crate::types::kernelboot::KernelBootSection;
use crate::types::network::NetworkSection;
use crate::types::nonrpm::NonRpmSoftwareSection;
use crate::types::os::{OsRelease, SystemType};
use crate::types::preflight::PreflightResult;
use crate::types::redaction::{RedactionFinding, RedactionState};
use crate::types::rpm::RpmSection;
use crate::types::scheduled::ScheduledTaskSection;
use crate::types::selinux::SelinuxSection;
use crate::types::services::ServiceSection;
use crate::types::storage::StorageSection;
use crate::types::users::UserGroupSection;
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
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub warnings: Vec<Warning>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
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

    /// Minimum schema version we can migrate from (Go v12).
    const MIN_SCHEMA: u32 = 12;

    pub fn load(json: &str) -> Result<Self, SnapshotError> {
        let snap: Self = serde_json::from_str(json)?;
        if snap.schema_version < Self::MIN_SCHEMA || snap.schema_version > SCHEMA_VERSION {
            return Err(SnapshotError::UnsupportedVersion(snap.schema_version));
        }
        Ok(snap)
    }
}

/// Migrate a snapshot to the current schema version.
///
/// v12 -> v13: no structural changes needed, just field defaults
/// v13 -> v14: no structural changes needed, serde(default) handles missing fields
pub fn migrate(snap: &mut InspectionSnapshot) {
    if snap.schema_version >= SCHEMA_VERSION {
        return;
    }
    // Structural compatibility is handled by serde(default) on all fields.
    // Bump version to current.
    snap.schema_version = SCHEMA_VERSION;
}

#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error("unsupported schema version: {0} (accepted: 12-{max})", max = crate::snapshot::SCHEMA_VERSION)]
    UnsupportedVersion(u32),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::rpm::{PackageEntry, PackageState};
    use crate::types::warnings::WarningSeverity;

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
        // Minimal Go v13 structure -- all sections null
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
    }

    #[test]
    fn test_v13_snapshot_loads() {
        let json = r#"{"schema_version": 13, "meta": {}, "system_type": "package-mode", "preflight": {"status": "ok"}, "warnings": [], "redactions": []}"#;
        let snap = InspectionSnapshot::load(json).unwrap();
        assert_eq!(snap.schema_version, 13);
    }

    #[test]
    fn test_v11_snapshot_rejected() {
        let json = r#"{"schema_version": 11, "meta": {}, "system_type": "package-mode", "preflight": {"status": "ok"}, "warnings": [], "redactions": []}"#;
        let result = InspectionSnapshot::load(json);
        assert!(result.is_err(), "v11 is below the accepted range (12-14)");
    }

    #[test]
    fn test_future_version_rejected() {
        let json = r#"{"schema_version": 20, "meta": {}, "system_type": "package-mode", "preflight": {"status": "ok"}, "warnings": [], "redactions": []}"#;
        let result = InspectionSnapshot::load(json);
        assert!(result.is_err(), "future versions must be rejected, not silently partially-deserialized");
    }

    #[test]
    fn test_migrate_bumps_version() {
        let json = r#"{"schema_version": 13, "meta": {}, "system_type": "package-mode", "preflight": {"status": "ok"}, "warnings": [], "redactions": []}"#;
        let mut snap = InspectionSnapshot::load(json).unwrap();
        migrate(&mut snap);
        assert_eq!(snap.schema_version, SCHEMA_VERSION);
    }
}
