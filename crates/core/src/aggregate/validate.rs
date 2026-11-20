use crate::snapshot::InspectionSnapshot;
use std::collections::{BTreeMap, HashMap, HashSet};

// ---------------------------------------------------------------------------
// Error and warning types
// ---------------------------------------------------------------------------

/// Hard errors that prevent aggregate from proceeding.
#[derive(Debug, Clone, PartialEq)]
pub enum AggregateValidationError {
    TooFewSnapshots {
        count: usize,
    },
    SchemaVersionMismatch {
        versions: Vec<u32>,
    },
    DuplicateHostname {
        hostname: String,
    },
    /// Two snapshots with no hostname and identical content: the same
    /// snapshot given twice. `scanned_at` is their scan timestamp, `None`
    /// when they recorded none.
    DuplicateUnnamedScan {
        scanned_at: Option<String>,
    },
    ArchitectureMismatch {
        architectures: Vec<String>,
    },
    EmptySnapshot {
        hostname: String,
    },
    OsMajorVersionMismatch {
        versions: Vec<String>,
    },
}

/// Warnings that allow aggregation but signal potential issues.
#[derive(Debug, Clone, PartialEq)]
pub enum AggregateWarning {
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

/// Result of aggregate pre-merge validation.
#[derive(Debug, Clone, Default)]
pub struct AggregateValidationResult {
    pub errors: Vec<AggregateValidationError>,
    pub warnings: Vec<AggregateWarning>,
}

impl AggregateValidationResult {
    /// Returns true if there are no hard errors.
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Validation logic
// ---------------------------------------------------------------------------

/// Validate a collection of parsed snapshots before aggregation.
///
/// Checks for hard errors (block merge) and warnings (proceed with caution).
/// This operates on already-parsed snapshots — unparseable files are detected
/// by the CLI layer during tarball loading, not here.
pub fn validate_snapshots(snapshots: &[InspectionSnapshot]) -> AggregateValidationResult {
    let mut result = AggregateValidationResult::default();

    // --- Hard error: too few snapshots ---
    if snapshots.len() < 2 {
        result
            .errors
            .push(AggregateValidationError::TooFewSnapshots {
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
                .push(AggregateValidationError::SchemaVersionMismatch { versions: sorted });
        }
    }

    // --- Hard error: duplicate hosts ---
    {
        let mut seen: BTreeMap<HostIdentity, usize> = BTreeMap::new();
        for snap in snapshots {
            *seen.entry(HostIdentity::of(snap)).or_insert(0) += 1;
        }
        for (identity, count) in seen {
            if count > 1 {
                result.errors.push(match identity {
                    HostIdentity::Named(hostname) => {
                        AggregateValidationError::DuplicateHostname { hostname }
                    }
                    HostIdentity::Unnamed { scanned_at, .. } => {
                        AggregateValidationError::DuplicateUnnamedScan { scanned_at }
                    }
                });
            }
        }
    }

    // --- Hard error: architecture mismatch ---
    {
        let architectures: HashSet<String> =
            snapshots.iter().filter_map(extract_architecture).collect();
        if architectures.len() > 1 {
            let mut sorted: Vec<String> = architectures.into_iter().collect();
            sorted.sort();
            result
                .errors
                .push(AggregateValidationError::ArchitectureMismatch {
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
                .push(AggregateValidationError::OsMajorVersionMismatch { versions: sorted });
        }
    }

    // --- Hard error: empty snapshots ---
    for snap in snapshots {
        if is_empty_snapshot(snap) {
            result.errors.push(AggregateValidationError::EmptySnapshot {
                hostname: HostIdentity::of(snap).to_string(),
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
                .push(AggregateWarning::MinorVersionSpread { versions: sorted });
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
                .push(AggregateWarning::SystemTypeMismatch { types: sorted });
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
            result.warnings.push(AggregateWarning::BaselineConflict {
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
                result.warnings.push(AggregateWarning::StaleScanDates {
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

/// What identifies one host's snapshot in an aggregate.
///
/// A snapshot that recorded a hostname is that host; two snapshots with
/// one hostname are the same host scanned twice, or two hosts that cannot
/// be told apart, and either way a duplicate. A snapshot with no hostname
/// carries no host identity at all (the scan never records the
/// machine-id), so it is identified by its content: a digest of the whole
/// snapshot. Two unnamed snapshots are a duplicate only when their content
/// is identical, which is the same snapshot given twice. A timestamp is
/// not an identity: independent hosts can record the same one.
///
/// The variants never compare equal to each other, and the derived order
/// puts named hosts first, by name, then unnamed ones by scan time and,
/// where scan times tie, by digest, so the order never depends on the
/// order the snapshots were given in.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HostIdentity {
    /// The snapshot's `meta["hostname"]`.
    Named(String),
    /// No hostname.
    Unnamed {
        /// The snapshot's `meta["timestamp"]`, if it has one.
        scanned_at: Option<String>,
        /// SHA-256 of the snapshot's canonical JSON, in hex.
        content_digest: String,
    },
}

impl HostIdentity {
    /// The identity a snapshot's metadata, or failing that its content,
    /// gives it.
    pub fn of(snap: &InspectionSnapshot) -> Self {
        match extract_meta_string(snap, "hostname").filter(|h| !h.trim().is_empty()) {
            Some(hostname) => Self::Named(hostname),
            None => Self::Unnamed {
                scanned_at: extract_meta_string(snap, "timestamp"),
                content_digest: content_digest(snap),
            },
        }
    }
}

/// How a message names the host: its hostname, or for an unnamed host
/// `<unnamed, scanned <time>>`. Not unique among unnamed hosts; the
/// aggregate's host list uses [`host_labels`].
impl std::fmt::Display for HostIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Named(hostname) => f.write_str(hostname),
            Self::Unnamed {
                scanned_at: Some(ts),
                ..
            } => write!(f, "<unnamed, scanned {ts}>"),
            Self::Unnamed {
                scanned_at: None, ..
            } => f.write_str("<unnamed>"),
        }
    }
}

/// The aggregate's host labels, one per identity, for identities already
/// in canonical order.
///
/// A named host is its hostname. Unnamed hosts are numbered in that order,
/// `<unnamed N, scanned <time>>` or `<unnamed N>`, so two unnamed hosts
/// never share a label, even when their timestamps tie, and the number
/// sits before the timestamp, where no timestamp text can reach it.
pub fn host_labels(identities: &[HostIdentity]) -> Vec<String> {
    let mut unnamed = 0;
    identities
        .iter()
        .map(|identity| match identity {
            HostIdentity::Named(hostname) => hostname.clone(),
            HostIdentity::Unnamed { scanned_at, .. } => {
                unnamed += 1;
                match scanned_at {
                    Some(ts) => format!("<unnamed {unnamed}, scanned {ts}>"),
                    None => format!("<unnamed {unnamed}>"),
                }
            }
        })
        .collect()
}

/// SHA-256 of the snapshot's canonical JSON. The snapshot goes through
/// `serde_json::Value`, whose objects keep their keys sorted, so two
/// snapshots equal field for field get one digest whatever order their
/// hash maps iterate in.
fn content_digest(snap: &InspectionSnapshot) -> String {
    use sha2::{Digest, Sha256};
    let canonical = serde_json::to_value(snap)
        .and_then(|value| serde_json::to_vec(&value))
        .expect("an InspectionSnapshot serializes to JSON, as every snapshot write relies on");
    format!("{:x}", Sha256::digest(&canonical))
}

/// Extract a string value from the snapshot's meta HashMap.
fn extract_meta_string(snap: &InspectionSnapshot, key: &str) -> Option<String> {
    snap.meta
        .get(key)
        .and_then(|v| v.as_str())
        .map(String::from)
}

/// Infer the host hardware architecture from RPM package arch fields.
///
/// Looks at `packages_added` for the dominant hardware architecture,
/// filtering out `noarch` and sub-architectures like `i686` that appear
/// alongside a primary arch. Returns None if the RPM section is missing
/// or contains no packages with a hardware architecture.
fn extract_architecture(snap: &InspectionSnapshot) -> Option<String> {
    let rpm = snap.rpm.as_ref()?;
    let mut arch_counts: HashMap<String, usize> = HashMap::new();
    for pkg in &rpm.packages_added {
        if pkg.arch.is_empty() || pkg.arch == "noarch" {
            continue;
        }
        *arch_counts.entry(pkg.arch.clone()).or_insert(0) += 1;
    }
    // Return the most frequent hardware architecture
    arch_counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(arch, _)| arch)
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
        && snap.units.is_none()
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
    use crate::types::FindingKind;

    #[test]
    fn test_host_identity_and_display() {
        // (case, hostname, timestamp, named?, expected display)
        type Case<'a> = (&'a str, Option<&'a str>, Option<&'a str>, bool, &'a str);
        let cases: &[Case] = &[
            (
                "named",
                Some("web-01"),
                Some("2026-11-19T10:00:00+00:00"),
                true,
                "web-01",
            ),
            (
                "unnamed with a timestamp",
                None,
                Some("2026-11-19T10:00:00+00:00"),
                false,
                "<unnamed, scanned 2026-11-19T10:00:00+00:00>",
            ),
            (
                "empty hostname is unnamed",
                Some(" "),
                None,
                false,
                "<unnamed>",
            ),
        ];
        for (case, hostname, timestamp, named, display) in cases {
            let mut snap = InspectionSnapshot::new();
            if let Some(h) = hostname {
                snap.meta.insert("hostname".into(), serde_json::json!(h));
            }
            if let Some(ts) = timestamp {
                snap.meta.insert("timestamp".into(), serde_json::json!(ts));
            }
            let actual = HostIdentity::of(&snap);
            match (&actual, named) {
                (HostIdentity::Named(h), true) => assert_eq!(Some(h.as_str()), *hostname),
                (
                    HostIdentity::Unnamed {
                        scanned_at,
                        content_digest,
                    },
                    false,
                ) => {
                    assert_eq!(scanned_at.as_deref(), *timestamp, "case: {case}");
                    assert_eq!(content_digest.len(), 64, "case: {case}: SHA-256 hex");
                }
                _ => panic!("case: {case}: unexpected identity {actual:?}"),
            }
            assert_eq!(actual.to_string(), *display, "case: {case}");
        }
    }

    /// The digest is over content, not over hash-map iteration order: two
    /// snapshots built with their meta keys inserted in opposite orders
    /// digest equally, and any change to content changes the digest.
    #[test]
    fn test_content_digest_is_canonical() {
        let keys = [
            "timestamp",
            "inspectah_version",
            "hostname_source",
            "a",
            "b",
            "c",
        ];
        let mut forward = InspectionSnapshot::new();
        for k in keys {
            forward.meta.insert(k.into(), serde_json::json!(k));
        }
        let mut reverse = InspectionSnapshot::new();
        for k in keys.iter().rev() {
            reverse.meta.insert((*k).into(), serde_json::json!(k));
        }
        assert_eq!(content_digest(&forward), content_digest(&reverse));
        reverse
            .meta
            .insert("a".into(), serde_json::json!("changed"));
        assert_ne!(content_digest(&forward), content_digest(&reverse));
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

    // --- Architecture extraction tests ---

    fn make_pkg(name: &str, arch: &str) -> crate::types::rpm::PackageEntry {
        crate::types::rpm::PackageEntry {
            name: name.into(),
            arch: arch.into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }
    }

    fn snap_with_arch_packages(arches: &[(&str, &str)]) -> InspectionSnapshot {
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(crate::types::rpm::RpmSection {
            packages_added: arches.iter().map(|(n, a)| make_pkg(n, a)).collect(),
            ..Default::default()
        });
        snap
    }

    #[test]
    fn test_extract_architecture_from_rpm() {
        let snap = snap_with_arch_packages(&[
            ("kernel", "x86_64"),
            ("glibc", "x86_64"),
            ("tzdata", "noarch"),
        ]);
        assert_eq!(extract_architecture(&snap), Some("x86_64".to_string()));
    }

    #[test]
    fn test_extract_architecture_aarch64() {
        let snap = snap_with_arch_packages(&[("kernel", "aarch64"), ("glibc", "aarch64")]);
        assert_eq!(extract_architecture(&snap), Some("aarch64".to_string()));
    }

    #[test]
    fn test_extract_architecture_noarch_only() {
        let snap = snap_with_arch_packages(&[("tzdata", "noarch"), ("ca-certificates", "noarch")]);
        assert_eq!(extract_architecture(&snap), None);
    }

    #[test]
    fn test_extract_architecture_no_rpm_section() {
        let snap = InspectionSnapshot::new();
        assert_eq!(extract_architecture(&snap), None);
    }

    #[test]
    fn test_extract_architecture_empty_packages() {
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(crate::types::rpm::RpmSection::default());
        assert_eq!(extract_architecture(&snap), None);
    }

    #[test]
    fn test_mixed_architecture_validation_fires() {
        let mut snap1 = snap_with_arch_packages(&[("kernel", "x86_64")]);
        snap1.meta.insert(
            "hostname".into(),
            serde_json::Value::String("web-01".into()),
        );
        snap1.os_release = Some(crate::types::os::OsRelease {
            version_id: "9.4".into(),
            ..Default::default()
        });

        let mut snap2 = snap_with_arch_packages(&[("kernel", "aarch64")]);
        snap2.meta.insert(
            "hostname".into(),
            serde_json::Value::String("web-02".into()),
        );
        snap2.os_release = Some(crate::types::os::OsRelease {
            version_id: "9.4".into(),
            ..Default::default()
        });

        let result = validate_snapshots(&[snap1, snap2]);
        let arch_errors: Vec<_> = result
            .errors
            .iter()
            .filter(|e| matches!(e, AggregateValidationError::ArchitectureMismatch { .. }))
            .collect();
        assert_eq!(arch_errors.len(), 1);
        if let AggregateValidationError::ArchitectureMismatch { architectures } = &arch_errors[0] {
            assert_eq!(architectures, &["aarch64", "x86_64"]);
        }
    }

    #[test]
    fn test_same_architecture_no_error() {
        let mut snap1 = snap_with_arch_packages(&[("kernel", "x86_64"), ("glibc", "x86_64")]);
        snap1.meta.insert(
            "hostname".into(),
            serde_json::Value::String("web-01".into()),
        );
        snap1.os_release = Some(crate::types::os::OsRelease {
            version_id: "9.4".into(),
            ..Default::default()
        });

        let mut snap2 = snap_with_arch_packages(&[("httpd", "x86_64"), ("glibc", "x86_64")]);
        snap2.meta.insert(
            "hostname".into(),
            serde_json::Value::String("web-02".into()),
        );
        snap2.os_release = Some(crate::types::os::OsRelease {
            version_id: "9.4".into(),
            ..Default::default()
        });

        let result = validate_snapshots(&[snap1, snap2]);
        let arch_errors: Vec<_> = result
            .errors
            .iter()
            .filter(|e| matches!(e, AggregateValidationError::ArchitectureMismatch { .. }))
            .collect();
        assert!(arch_errors.is_empty());
    }

    #[test]
    fn test_missing_rpm_section_skips_arch_check() {
        // One snapshot has RPM data, the other doesn't — should not fire mismatch
        let mut snap1 = snap_with_arch_packages(&[("kernel", "x86_64")]);
        snap1.meta.insert(
            "hostname".into(),
            serde_json::Value::String("web-01".into()),
        );
        snap1.os_release = Some(crate::types::os::OsRelease {
            version_id: "9.4".into(),
            ..Default::default()
        });

        let mut snap2 = InspectionSnapshot::new();
        snap2.meta.insert(
            "hostname".into(),
            serde_json::Value::String("web-02".into()),
        );
        snap2.os_release = Some(crate::types::os::OsRelease {
            version_id: "9.4".into(),
            ..Default::default()
        });

        let result = validate_snapshots(&[snap1, snap2]);
        let arch_errors: Vec<_> = result
            .errors
            .iter()
            .filter(|e| matches!(e, AggregateValidationError::ArchitectureMismatch { .. }))
            .collect();
        assert!(arch_errors.is_empty());
    }
}
