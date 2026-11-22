use inspectah_core::aggregate::validate::{
    AggregateValidationError, AggregateWarning, validate_snapshots,
};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::FindingKind;
use inspectah_core::types::os::{OsRelease, SystemType};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a minimal valid snapshot with a hostname and version_id.
fn make_snap(hostname: &str, version_id: &str) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.meta.insert(
        "hostname".to_string(),
        serde_json::Value::String(hostname.to_string()),
    );
    snap.os_release = Some(OsRelease {
        version_id: version_id.to_string(),
        name: "Red Hat Enterprise Linux".to_string(),
        ..Default::default()
    });
    // Give it at least one section so it's not "empty"
    snap.rpm = Some(inspectah_core::types::rpm::RpmSection::default());
    snap
}

fn make_snap_with_arch(hostname: &str, arch: &str) -> InspectionSnapshot {
    use inspectah_core::types::rpm::{PackageEntry, RpmSection};
    let mut snap = make_snap(hostname, "9.4");
    // Architecture is inferred from RPM package arch fields
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "kernel".into(),
            arch: arch.into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        ..Default::default()
    });
    snap
}

// ===========================================================================
// Hard errors
// ===========================================================================

#[test]
fn test_too_few_snapshots_zero() {
    let result = validate_snapshots(&[]);
    assert!(
        result
            .errors
            .iter()
            .any(|e| matches!(e, AggregateValidationError::TooFewSnapshots { count: 0 }))
    );
}

#[test]
fn test_too_few_snapshots_one() {
    let snap = make_snap("host-1", "9.4");
    let result = validate_snapshots(&[snap]);
    assert!(
        result
            .errors
            .iter()
            .any(|e| matches!(e, AggregateValidationError::TooFewSnapshots { count: 1 }))
    );
}

#[test]
fn test_two_snapshots_is_enough() {
    let a = make_snap("host-1", "9.4");
    let b = make_snap("host-2", "9.4");
    let result = validate_snapshots(&[a, b]);
    assert!(
        !result
            .errors
            .iter()
            .any(|e| matches!(e, AggregateValidationError::TooFewSnapshots { .. })),
        "2 snapshots should not trigger TooFewSnapshots"
    );
}

#[test]
fn test_schema_version_mismatch() {
    let mut a = make_snap("host-1", "9.4");
    let mut b = make_snap("host-2", "9.4");
    a.schema_version = 14;
    b.schema_version = 15;
    let result = validate_snapshots(&[a, b]);
    assert!(result.errors.iter().any(|e| matches!(
        e,
        AggregateValidationError::SchemaVersionMismatch { versions }
        if *versions == vec![14, 15]
    )));
}

#[test]
fn test_schema_version_match_ok() {
    let a = make_snap("host-1", "9.4");
    let b = make_snap("host-2", "9.4");
    // Both use InspectionSnapshot::new() which sets SCHEMA_VERSION
    let result = validate_snapshots(&[a, b]);
    assert!(
        !result
            .errors
            .iter()
            .any(|e| matches!(e, AggregateValidationError::SchemaVersionMismatch { .. })),
        "matching schema versions should not trigger error"
    );
}

#[test]
fn test_duplicate_hostname() {
    let a = make_snap("web-01", "9.4");
    let b = make_snap("web-01", "9.4");
    let result = validate_snapshots(&[a, b]);
    assert!(result.errors.iter().any(|e| matches!(
        e,
        AggregateValidationError::DuplicateHostname { hostname }
        if hostname == "web-01"
    )));
}

#[test]
fn test_unique_hostnames_ok() {
    let a = make_snap("web-01", "9.4");
    let b = make_snap("web-02", "9.4");
    let result = validate_snapshots(&[a, b]);
    assert!(
        !result
            .errors
            .iter()
            .any(|e| matches!(e, AggregateValidationError::DuplicateHostname { .. })),
        "unique hostnames should not trigger error"
    );
}

#[test]
fn test_architecture_mismatch() {
    let a = make_snap_with_arch("host-1", "x86_64");
    let b = make_snap_with_arch("host-2", "aarch64");
    let result = validate_snapshots(&[a, b]);
    assert!(result.errors.iter().any(|e| matches!(
        e,
        AggregateValidationError::ArchitectureMismatch { architectures }
        if architectures.len() == 2
    )));
}

#[test]
fn test_architecture_match_ok() {
    let a = make_snap_with_arch("host-1", "x86_64");
    let b = make_snap_with_arch("host-2", "x86_64");
    let result = validate_snapshots(&[a, b]);
    assert!(
        !result
            .errors
            .iter()
            .any(|e| matches!(e, AggregateValidationError::ArchitectureMismatch { .. })),
        "matching architectures should not trigger error"
    );
}

#[test]
fn test_os_major_version_mismatch() {
    let a = make_snap("host-1", "9.4");
    let b = make_snap("host-2", "8.9");
    let result = validate_snapshots(&[a, b]);
    assert!(result.errors.iter().any(|e| matches!(
        e,
        AggregateValidationError::OsMajorVersionMismatch { versions }
        if *versions == vec!["8".to_string(), "9".to_string()]
    )));
}

#[test]
fn test_os_major_version_match_ok() {
    let a = make_snap("host-1", "9.4");
    let b = make_snap("host-2", "9.5");
    let result = validate_snapshots(&[a, b]);
    assert!(
        !result
            .errors
            .iter()
            .any(|e| matches!(e, AggregateValidationError::OsMajorVersionMismatch { .. })),
        "same major version should not trigger error"
    );
}

#[test]
fn test_empty_snapshot_detected() {
    let mut a = make_snap("host-1", "9.4");
    let b = make_snap("host-2", "9.4");
    // Make 'a' empty: clear all sections including os_release
    a.rpm = None;
    a.os_release = None;
    let result = validate_snapshots(&[a, b]);
    assert!(result.errors.iter().any(|e| matches!(
        e,
        AggregateValidationError::EmptySnapshot { hostname }
        if hostname == "host-1"
    )));
}

#[test]
fn test_non_empty_snapshot_ok() {
    let a = make_snap("host-1", "9.4");
    let b = make_snap("host-2", "9.4");
    let result = validate_snapshots(&[a, b]);
    assert!(
        !result
            .errors
            .iter()
            .any(|e| matches!(e, AggregateValidationError::EmptySnapshot { .. })),
        "snapshot with rpm section should not be empty"
    );
}

// ===========================================================================
// Warnings
// ===========================================================================

#[test]
fn test_minor_version_spread_warning() {
    let a = make_snap("host-1", "9.4");
    let b = make_snap("host-2", "9.5");
    let result = validate_snapshots(&[a, b]);
    assert!(result.warnings.iter().any(|w| matches!(
        w,
        AggregateWarning::MinorVersionSpread { versions }
        if versions.len() == 2
    )));
}

#[test]
fn test_no_minor_version_spread_when_same() {
    let a = make_snap("host-1", "9.4");
    let b = make_snap("host-2", "9.4");
    let result = validate_snapshots(&[a, b]);
    assert!(
        !result
            .warnings
            .iter()
            .any(|w| matches!(w, AggregateWarning::MinorVersionSpread { .. })),
        "same version should not trigger minor version spread"
    );
}

#[test]
fn test_system_type_mismatch_warning() {
    let mut a = make_snap("host-1", "9.4");
    let mut b = make_snap("host-2", "9.4");
    a.system_type = SystemType::PackageMode;
    b.system_type = SystemType::Bootc;
    let result = validate_snapshots(&[a, b]);
    assert!(result.warnings.iter().any(|w| matches!(
        w,
        AggregateWarning::SystemTypeMismatch { types }
        if types.len() == 2
    )));
}

#[test]
fn test_no_system_type_mismatch_when_same() {
    let mut a = make_snap("host-1", "9.4");
    let mut b = make_snap("host-2", "9.4");
    a.system_type = SystemType::PackageMode;
    b.system_type = SystemType::PackageMode;
    let result = validate_snapshots(&[a, b]);
    assert!(
        !result
            .warnings
            .iter()
            .any(|w| matches!(w, AggregateWarning::SystemTypeMismatch { .. })),
        "same system type should not trigger warning"
    );
}

#[test]
fn test_baseline_conflict_warning() {
    use inspectah_core::baseline::{ResolutionStrategy, TargetImageIdentity};

    let mut a = make_snap("host-1", "9.4");
    let mut b = make_snap("host-2", "9.4");
    let mut c = make_snap("host-3", "9.4");
    a.target_image = Some(TargetImageIdentity {
        image_ref: "quay.io/rhel/rhel-bootc:9.4".to_string(),
        strategy: ResolutionStrategy::OsRelease,
    });
    b.target_image = Some(TargetImageIdentity {
        image_ref: "quay.io/rhel/rhel-bootc:9.4".to_string(),
        strategy: ResolutionStrategy::OsRelease,
    });
    c.target_image = Some(TargetImageIdentity {
        image_ref: "quay.io/rhel/rhel-bootc:9.5".to_string(),
        strategy: ResolutionStrategy::OsRelease,
    });
    let result = validate_snapshots(&[a, b, c]);
    assert!(result.warnings.iter().any(|w| match w {
        AggregateWarning::BaselineConflict {
            distribution,
            selected,
        } => {
            // The 9.4 image has 2 hosts, so it should be selected
            selected == "quay.io/rhel/rhel-bootc:9.4" && distribution.len() == 2
        }
        _ => false,
    }));
}

#[test]
fn test_no_baseline_conflict_when_same() {
    use inspectah_core::baseline::{ResolutionStrategy, TargetImageIdentity};

    let mut a = make_snap("host-1", "9.4");
    let mut b = make_snap("host-2", "9.4");
    let img = TargetImageIdentity {
        image_ref: "quay.io/rhel/rhel-bootc:9.4".to_string(),
        strategy: ResolutionStrategy::OsRelease,
    };
    a.target_image = Some(img.clone());
    b.target_image = Some(img);
    let result = validate_snapshots(&[a, b]);
    assert!(
        !result
            .warnings
            .iter()
            .any(|w| matches!(w, AggregateWarning::BaselineConflict { .. })),
        "same target image should not trigger baseline conflict"
    );
}

#[test]
fn test_stale_scan_dates_warning() {
    let mut a = make_snap("host-1", "9.4");
    let mut b = make_snap("host-2", "9.4");
    a.meta.insert(
        "timestamp".to_string(),
        serde_json::Value::String("2026-01-01T00:00:00Z".to_string()),
    );
    b.meta.insert(
        "timestamp".to_string(),
        serde_json::Value::String("2026-03-15T00:00:00Z".to_string()),
    );
    let result = validate_snapshots(&[a, b]);
    let stale: Vec<&String> = result
        .warnings
        .iter()
        .filter_map(|w| match w {
            AggregateWarning::StaleScanDates { spread_description } => Some(spread_description),
            _ => None,
        })
        .collect();
    assert_eq!(
        stale,
        ["scans span 73 days, more than 7: from 2026-01-01T00:00:00Z to 2026-03-15T00:00:00Z"]
    );
}

/// Scans taken one after another are not stale: hosts scanned minutes
/// apart, as the scan writes its timestamp, and a spread of exactly the
/// threshold, raise no warning; a second past it does.
#[test]
fn stale_scan_dates_warn_only_past_the_threshold() {
    let stale = |first: &str, second: &str| {
        let mut a = make_snap("host-1", "9.4");
        let mut b = make_snap("host-2", "9.4");
        a.meta.insert("timestamp".into(), serde_json::json!(first));
        b.meta.insert("timestamp".into(), serde_json::json!(second));
        validate_snapshots(&[a, b])
            .warnings
            .iter()
            .any(|w| matches!(w, AggregateWarning::StaleScanDates { .. }))
    };
    assert!(!stale(
        "2026-11-05T04:26:03.552382773+00:00",
        "2026-11-05T04:38:47.338375489+00:00"
    ));
    assert!(!stale(
        "2026-11-01T00:00:00+00:00",
        "2026-11-08T00:00:00+00:00"
    ));
    assert!(stale(
        "2026-11-01T00:00:00+00:00",
        "2026-11-08T00:00:01+00:00"
    ));
    // Offsets are compared as instants, not as text.
    assert!(!stale(
        "2026-11-08T05:00:00+05:00",
        "2026-11-01T01:00:00+00:00"
    ));
}

#[test]
fn test_no_stale_scan_dates_when_same() {
    let mut a = make_snap("host-1", "9.4");
    let mut b = make_snap("host-2", "9.4");
    let ts = "2026-05-01T12:00:00Z";
    a.meta.insert(
        "timestamp".to_string(),
        serde_json::Value::String(ts.to_string()),
    );
    b.meta.insert(
        "timestamp".to_string(),
        serde_json::Value::String(ts.to_string()),
    );
    let result = validate_snapshots(&[a, b]);
    assert!(
        !result
            .warnings
            .iter()
            .any(|w| matches!(w, AggregateWarning::StaleScanDates { .. })),
        "same timestamps should not trigger stale scan warning"
    );
}

// ===========================================================================
// Combined / integration scenarios
// ===========================================================================

#[test]
fn test_valid_aggregate_passes_cleanly() {
    let a = make_snap("host-1", "9.4");
    let b = make_snap("host-2", "9.4");
    let result = validate_snapshots(&[a, b]);
    assert!(result.is_ok(), "valid aggregate should have no errors");
    // MinorVersionSpread should not fire since both are 9.4
    assert!(
        result.warnings.is_empty(),
        "valid aggregate should have no warnings"
    );
}

#[test]
fn test_multiple_errors_reported() {
    use inspectah_core::types::rpm::{PackageEntry, RpmSection};
    // Schema mismatch + duplicate hostname + architecture mismatch
    let mut a = make_snap("web-01", "9.4");
    let mut b = make_snap("web-01", "8.9");
    a.schema_version = 14;
    b.schema_version = 15;
    // Architecture is inferred from RPM package arch fields
    a.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "kernel".into(),
            arch: "x86_64".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        ..Default::default()
    });
    b.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "kernel".into(),
            arch: "aarch64".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        ..Default::default()
    });
    let result = validate_snapshots(&[a, b]);
    assert!(
        result.errors.len() >= 3,
        "should report schema, hostname, and architecture errors"
    );
    assert!(
        result
            .errors
            .iter()
            .any(|e| matches!(e, AggregateValidationError::SchemaVersionMismatch { .. }))
    );
    assert!(
        result
            .errors
            .iter()
            .any(|e| matches!(e, AggregateValidationError::DuplicateHostname { .. }))
    );
    assert!(
        result
            .errors
            .iter()
            .any(|e| matches!(e, AggregateValidationError::ArchitectureMismatch { .. }))
    );
}

#[test]
fn test_is_ok_with_only_warnings() {
    let mut a = make_snap("host-1", "9.4");
    let mut b = make_snap("host-2", "9.5");
    a.system_type = SystemType::PackageMode;
    b.system_type = SystemType::Bootc;
    let result = validate_snapshots(&[a, b]);
    assert!(result.is_ok(), "warnings-only should still be is_ok()");
    assert!(!result.warnings.is_empty(), "should have warnings");
}

/// A snapshot that recorded no hostname is identified by its content, not
/// by a shared placeholder name or by its timestamp: several unnamed hosts
/// aggregate, even when their timestamps tie, while the same snapshot
/// twice, or the same hostname twice, is still a duplicate.
#[test]
fn test_unnamed_snapshot_identity() {
    /// An unnamed snapshot: no hostname, optionally a scan timestamp.
    fn unnamed(scanned_at: Option<&str>) -> InspectionSnapshot {
        let mut snap = make_snap("", "9.4");
        snap.meta.remove("hostname");
        if let Some(ts) = scanned_at {
            snap.meta.insert(
                "timestamp".to_string(),
                serde_json::Value::String(ts.to_string()),
            );
        }
        snap
    }
    fn named(hostname: &str, scanned_at: &str) -> InspectionSnapshot {
        let mut snap = make_snap(hostname, "9.4");
        snap.meta.insert(
            "timestamp".to_string(),
            serde_json::Value::String(scanned_at.to_string()),
        );
        snap
    }
    /// The snapshot with one more package, so its content differs.
    fn with_package(mut snap: InspectionSnapshot, name: &str) -> InspectionSnapshot {
        let rpm = snap.rpm.get_or_insert_with(Default::default);
        rpm.packages_added
            .push(inspectah_core::types::rpm::PackageEntry {
                name: name.into(),
                arch: "x86_64".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            });
        snap
    }
    const T1: &str = "2026-11-19T10:00:00.000000001+00:00";
    const T2: &str = "2026-11-19T10:00:00.000000002+00:00";
    const T3: &str = "2026-11-19T10:05:00.000000000+00:00";

    let cases: Vec<(&str, Vec<InspectionSnapshot>, Vec<AggregateValidationError>)> = vec![
        (
            "several unnamed hosts",
            vec![unnamed(Some(T1)), unnamed(Some(T2)), unnamed(Some(T3))],
            vec![],
        ),
        (
            "named and unnamed mixed",
            vec![named("web-01", T1), unnamed(Some(T2)), named("web-02", T3)],
            vec![],
        ),
        (
            "unnamed scan with the same timestamp as a named one",
            vec![named("web-01", T1), unnamed(Some(T1))],
            vec![],
        ),
        (
            "two unnamed hosts with different content and one timestamp",
            vec![unnamed(Some(T1)), with_package(unnamed(Some(T1)), "httpd")],
            vec![],
        ),
        (
            "two unnamed hosts with different content and no timestamp",
            vec![unnamed(None), with_package(unnamed(None), "httpd")],
            vec![],
        ),
        (
            "the same unnamed snapshot twice",
            vec![unnamed(Some(T1)), unnamed(Some(T1)), unnamed(Some(T2))],
            vec![AggregateValidationError::DuplicateUnnamedScan {
                scanned_at: Some(T1.to_string()),
            }],
        ),
        (
            "the same unnamed snapshot twice, with no timestamp",
            vec![unnamed(None), unnamed(None)],
            vec![AggregateValidationError::DuplicateUnnamedScan { scanned_at: None }],
        ),
        (
            "the same named host twice, alongside unnamed ones",
            vec![named("web-01", T1), named("web-01", T2), unnamed(Some(T3))],
            vec![AggregateValidationError::DuplicateHostname {
                hostname: "web-01".to_string(),
            }],
        ),
    ];

    for (case, snapshots, expected) in cases {
        let result = validate_snapshots(&snapshots);
        let duplicates: Vec<AggregateValidationError> = result
            .errors
            .into_iter()
            .filter(|e| {
                matches!(
                    e,
                    AggregateValidationError::DuplicateHostname { .. }
                        | AggregateValidationError::DuplicateUnnamedScan { .. }
                )
            })
            .collect();
        assert_eq!(duplicates, expected, "case: {case}");
    }
}
