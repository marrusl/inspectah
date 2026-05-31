use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::rpm::VersionChangeDirection;

use super::types::{EmptyReason, RefVersionChangeItem, RefVersionChanges};

/// Project version changes from snapshot into reference format.
///
/// Logic follows the three-state empty reason pattern from handlers:
/// - `DataUnavailable`: No rpm section exists
/// - `NoBaseline`: Rpm section exists but no baseline data
/// - `ZeroDrift`: Baseline exists but no version changes detected
pub fn project_ref_version_changes(snap: &InspectionSnapshot) -> RefVersionChanges {
    let rpm = match &snap.rpm {
        None => {
            return RefVersionChanges {
                downgrades: Vec::new(),
                upgrades: Vec::new(),
                empty_reason: Some(EmptyReason::DataUnavailable),
            };
        }
        Some(rpm) => rpm,
    };

    if rpm.version_changes.is_empty() {
        let reason = if snap.baseline.is_some() {
            EmptyReason::ZeroDrift
        } else {
            EmptyReason::NoBaseline
        };
        return RefVersionChanges {
            downgrades: Vec::new(),
            upgrades: Vec::new(),
            empty_reason: Some(reason),
        };
    }

    // Partition by direction
    let mut downgrades = Vec::new();
    let mut upgrades = Vec::new();

    for vc in &rpm.version_changes {
        let item = RefVersionChangeItem {
            name: vc.name.clone(),
            arch: vc.arch.clone(),
            host_version: vc.host_version.clone(),
            base_version: vc.base_version.clone(),
            host_epoch: vc.host_epoch.clone(),
            base_epoch: vc.base_epoch.clone(),
            direction: vc.direction.clone(),
        };

        match vc.direction {
            VersionChangeDirection::Downgrade => downgrades.push(item),
            VersionChangeDirection::Upgrade => upgrades.push(item),
        }
    }

    RefVersionChanges {
        downgrades,
        upgrades,
        empty_reason: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::baseline::BaselineData;
    use inspectah_core::types::rpm::{RpmSection, VersionChange};
    use std::collections::HashMap;

    #[test]
    fn test_empty_snapshot_returns_data_unavailable() {
        let snap = InspectionSnapshot {
            rpm: None,
            ..Default::default()
        };

        let result = project_ref_version_changes(&snap);

        assert!(result.downgrades.is_empty());
        assert!(result.upgrades.is_empty());
        assert_eq!(result.empty_reason, Some(EmptyReason::DataUnavailable));
    }

    #[test]
    fn test_no_baseline_returns_no_baseline() {
        let snap = InspectionSnapshot {
            rpm: Some(RpmSection {
                version_changes: Vec::new(),
                ..Default::default()
            }),
            baseline: None,
            ..Default::default()
        };

        let result = project_ref_version_changes(&snap);

        assert!(result.downgrades.is_empty());
        assert!(result.upgrades.is_empty());
        assert_eq!(result.empty_reason, Some(EmptyReason::NoBaseline));
    }

    #[test]
    fn test_baseline_with_no_changes_returns_zero_drift() {
        let snap = InspectionSnapshot {
            rpm: Some(RpmSection {
                version_changes: Vec::new(),
                ..Default::default()
            }),
            baseline: Some(BaselineData {
                image_digest: "sha256:test".to_string(),
                packages: HashMap::new(),
                extracted_at: "2024-01-01T00:00:00Z".to_string(),
            }),
            ..Default::default()
        };

        let result = project_ref_version_changes(&snap);

        assert!(result.downgrades.is_empty());
        assert!(result.upgrades.is_empty());
        assert_eq!(result.empty_reason, Some(EmptyReason::ZeroDrift));
    }

    #[test]
    fn test_partition_by_direction() {
        let snap = InspectionSnapshot {
            rpm: Some(RpmSection {
                version_changes: vec![
                    VersionChange {
                        name: "pkg1".to_string(),
                        arch: "x86_64".to_string(),
                        host_version: "2.0".to_string(),
                        base_version: "1.0".to_string(),
                        host_epoch: "0".to_string(),
                        base_epoch: "0".to_string(),
                        direction: VersionChangeDirection::Upgrade,
                    },
                    VersionChange {
                        name: "pkg2".to_string(),
                        arch: "x86_64".to_string(),
                        host_version: "1.0".to_string(),
                        base_version: "2.0".to_string(),
                        host_epoch: "0".to_string(),
                        base_epoch: "0".to_string(),
                        direction: VersionChangeDirection::Downgrade,
                    },
                    VersionChange {
                        name: "pkg3".to_string(),
                        arch: "aarch64".to_string(),
                        host_version: "3.0".to_string(),
                        base_version: "2.5".to_string(),
                        host_epoch: "1".to_string(),
                        base_epoch: "0".to_string(),
                        direction: VersionChangeDirection::Upgrade,
                    },
                ],
                ..Default::default()
            }),
            baseline: Some(BaselineData {
                image_digest: "sha256:test".to_string(),
                packages: HashMap::new(),
                extracted_at: "2024-01-01T00:00:00Z".to_string(),
            }),
            ..Default::default()
        };

        let result = project_ref_version_changes(&snap);

        assert_eq!(result.downgrades.len(), 1);
        assert_eq!(result.upgrades.len(), 2);
        assert_eq!(result.empty_reason, None);

        assert_eq!(result.downgrades[0].name, "pkg2");
        assert_eq!(result.upgrades[0].name, "pkg1");
        assert_eq!(result.upgrades[1].name, "pkg3");
    }
}
