use crate::snapshot::InspectionSnapshot;
use std::collections::{HashMap, HashSet};

// ---------------------------------------------------------------------------
// Error and warning types
// ---------------------------------------------------------------------------

/// Hard errors that prevent fleet aggregation from proceeding.
#[derive(Debug, Clone, PartialEq)]
pub enum FleetValidationError {
    TooFewSnapshots { count: usize },
    SchemaVersionMismatch { versions: Vec<u32> },
    DuplicateHostname { hostname: String },
    ArchitectureMismatch { architectures: Vec<String> },
    EmptySnapshot { hostname: String },
    OsMajorVersionMismatch { versions: Vec<String> },
}

/// Warnings that allow aggregation but signal potential issues.
#[derive(Debug, Clone, PartialEq)]
pub enum FleetWarning {
    StaleScanDates {
        spread_description: String,
    },
    BaselineConflict {
        distribution: Vec<(String, usize)>,
        selected: String,
    },
    MinorVersionSpread {
        versions: Vec<String>,
    },
    SystemTypeMismatch {
        types: Vec<String>,
    },
}

/// Result of fleet pre-merge validation.
#[derive(Debug, Clone, Default)]
pub struct FleetValidationResult {
    pub errors: Vec<FleetValidationError>,
    pub warnings: Vec<FleetWarning>,
}

impl FleetValidationResult {
    /// Returns true if there are no hard errors.
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Validation logic
// ---------------------------------------------------------------------------

/// Validate a collection of parsed snapshots before fleet aggregation.
///
/// Checks for hard errors (block merge) and warnings (proceed with caution).
/// This operates on already-parsed snapshots — unparseable files are detected
/// by the CLI layer during tarball loading, not here.
pub fn validate_snapshots(snapshots: &[InspectionSnapshot]) -> FleetValidationResult {
    let mut result = FleetValidationResult::default();

    // --- Hard error: too few snapshots ---
    if snapshots.len() < 2 {
        result.errors.push(FleetValidationError::TooFewSnapshots {
            count: snapshots.len(),
        });
        // With fewer than 2 snapshots, most other checks are meaningless,
        // but we continue to report as many problems as possible.
    }

    // --- Hard error: schema version mismatch ---
    {
        let versions: HashSet<u32> = snapshots.iter().map(|s| s.schema_version).collect();
        if versions.len() > 1 {
            let mut sorted: Vec<u32> = versions.into_iter().collect();
            sorted.sort_unstable();
            result
                .errors
                .push(FleetValidationError::SchemaVersionMismatch { versions: sorted });
        }
    }

    // --- Hard error: duplicate hostnames ---
    {
        let mut seen: HashMap<String, usize> = HashMap::new();
        for snap in snapshots {
            let hostname = extract_hostname(snap);
            *seen.entry(hostname).or_insert(0) += 1;
        }
        for (hostname, count) in &seen {
            if *count > 1 {
                result.errors.push(FleetValidationError::DuplicateHostname {
                    hostname: hostname.clone(),
                });
            }
        }
    }

    // --- Hard error: architecture mismatch ---
    {
        let architectures: HashSet<String> = snapshots
            .iter()
            .filter_map(|s| extract_meta_string(s, "architecture"))
            .collect();
        if architectures.len() > 1 {
            let mut sorted: Vec<String> = architectures.into_iter().collect();
            sorted.sort();
            result
                .errors
                .push(FleetValidationError::ArchitectureMismatch {
                    architectures: sorted,
                });
        }
    }

    // --- Hard error: OS major version mismatch ---
    {
        let major_versions: HashSet<String> = snapshots
            .iter()
            .filter_map(extract_os_major_version)
            .collect();
        if major_versions.len() > 1 {
            let mut sorted: Vec<String> = major_versions.into_iter().collect();
            sorted.sort();
            result
                .errors
                .push(FleetValidationError::OsMajorVersionMismatch { versions: sorted });
        }
    }

    // --- Hard error: empty snapshots ---
    for snap in snapshots {
        if is_empty_snapshot(snap) {
            result.errors.push(FleetValidationError::EmptySnapshot {
                hostname: extract_hostname(snap),
            });
        }
    }

    // --- Warning: minor version spread ---
    {
        let minor_versions: HashSet<String> = snapshots
            .iter()
            .filter_map(|s| {
                s.os_release
                    .as_ref()
                    .map(|r| r.version_id.clone())
                    .filter(|v| !v.is_empty())
            })
            .collect();
        if minor_versions.len() > 1 {
            let mut sorted: Vec<String> = minor_versions.into_iter().collect();
            sorted.sort();
            result
                .warnings
                .push(FleetWarning::MinorVersionSpread { versions: sorted });
        }
    }

    // --- Warning: system type mismatch ---
    {
        let types: HashSet<String> = snapshots
            .iter()
            .map(|s| format!("{:?}", s.system_type))
            .collect();
        if types.len() > 1 {
            let mut sorted: Vec<String> = types.into_iter().collect();
            sorted.sort();
            result
                .warnings
                .push(FleetWarning::SystemTypeMismatch { types: sorted });
        }
    }

    // --- Warning: baseline conflict (multiple target images) ---
    {
        let mut image_counts: HashMap<String, usize> = HashMap::new();
        for snap in snapshots {
            if let Some(ref ti) = snap.target_image {
                *image_counts.entry(ti.image_ref.clone()).or_insert(0) += 1;
            }
        }
        if image_counts.len() > 1 {
            let mut distribution: Vec<(String, usize)> = image_counts.into_iter().collect();
            distribution.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            let selected = distribution[0].0.clone();
            result.warnings.push(FleetWarning::BaselineConflict {
                distribution,
                selected,
            });
        }
    }

    // --- Warning: stale scan dates ---
    {
        let timestamps: Vec<&str> = snapshots
            .iter()
            .filter_map(|s| s.meta.get("timestamp").and_then(|v| v.as_str()))
            .collect();
        if timestamps.len() >= 2 {
            // Simple lexicographic comparison works for ISO-8601 date strings
            if let (Some(min), Some(max)) = (timestamps.iter().min(), timestamps.iter().max())
                && min != max
            {
                result.warnings.push(FleetWarning::StaleScanDates {
                    spread_description: format!("Scan dates range from {} to {}", min, max),
                });
            }
        }
    }

    result
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Extract hostname from snapshot metadata. Falls back to "<unknown>".
pub fn extract_hostname(snap: &InspectionSnapshot) -> String {
    extract_meta_string(snap, "hostname").unwrap_or_else(|| "<unknown>".to_string())
}

/// Extract a string value from the snapshot's meta HashMap.
fn extract_meta_string(snap: &InspectionSnapshot, key: &str) -> Option<String> {
    snap.meta
        .get(key)
        .and_then(|v| v.as_str())
        .map(String::from)
}

/// Extract the OS major version from os_release.version_id (e.g., "9" from "9.4").
fn extract_os_major_version(snap: &InspectionSnapshot) -> Option<String> {
    snap.os_release.as_ref().and_then(|r| {
        let vid = &r.version_id;
        if vid.is_empty() {
            return None;
        }
        Some(vid.split('.').next().unwrap_or(vid).to_string())
    })
}

/// A snapshot is "empty" when ALL section fields are None.
fn is_empty_snapshot(snap: &InspectionSnapshot) -> bool {
    snap.rpm.is_none()
        && snap.config.is_none()
        && snap.services.is_none()
        && snap.network.is_none()
        && snap.storage.is_none()
        && snap.scheduled_tasks.is_none()
        && snap.containers.is_none()
        && snap.non_rpm_software.is_none()
        && snap.kernel_boot.is_none()
        && snap.selinux.is_none()
        && snap.users_groups.is_none()
        && snap.os_release.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_hostname_from_meta() {
        let mut snap = InspectionSnapshot::new();
        snap.meta.insert(
            "hostname".to_string(),
            serde_json::Value::String("web-01".to_string()),
        );
        assert_eq!(extract_hostname(&snap), "web-01");
    }

    #[test]
    fn test_extract_hostname_missing() {
        let snap = InspectionSnapshot::new();
        assert_eq!(extract_hostname(&snap), "<unknown>");
    }

    #[test]
    fn test_extract_os_major_version() {
        let mut snap = InspectionSnapshot::new();
        snap.os_release = Some(crate::types::os::OsRelease {
            version_id: "9.4".to_string(),
            ..Default::default()
        });
        assert_eq!(extract_os_major_version(&snap), Some("9".to_string()));
    }

    #[test]
    fn test_extract_os_major_version_no_dot() {
        let mut snap = InspectionSnapshot::new();
        snap.os_release = Some(crate::types::os::OsRelease {
            version_id: "9".to_string(),
            ..Default::default()
        });
        assert_eq!(extract_os_major_version(&snap), Some("9".to_string()));
    }

    #[test]
    fn test_is_empty_snapshot() {
        let snap = InspectionSnapshot::new();
        assert!(is_empty_snapshot(&snap));
    }

    #[test]
    fn test_is_not_empty_with_rpm() {
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(crate::types::rpm::RpmSection::default());
        assert!(!is_empty_snapshot(&snap));
    }
}
