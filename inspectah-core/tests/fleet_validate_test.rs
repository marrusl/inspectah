use inspectah_core::fleet::validate::{
    validate_snapshots, FleetValidationError, FleetWarning,
};
use inspectah_core::snapshot::InspectionSnapshot;
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
    let mut snap = make_snap(hostname, "9.4");
    snap.meta.insert(
        "architecture".to_string(),
        serde_json::Value::String(arch.to_string()),
    );
    snap
}

// ===========================================================================
// Hard errors
// ===========================================================================

#[test]
fn test_too_few_snapshots_zero() {
    let result = validate_snapshots(&[]);
    assert!(result
        .errors
        .iter()
        .any(|e| matches!(e, FleetValidationError::TooFewSnapshots { count: 0 })));
}

#[test]
fn test_too_few_snapshots_one() {
    let snap = make_snap("host-1", "9.4");
    let result = validate_snapshots(&[snap]);
    assert!(result
        .errors
        .iter()
        .any(|e| matches!(e, FleetValidationError::TooFewSnapshots { count: 1 })));
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
            .any(|e| matches!(e, FleetValidationError::TooFewSnapshots { .. })),
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
        FleetValidationError::SchemaVersionMismatch { versions }
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
            .any(|e| matches!(e, FleetValidationError::SchemaVersionMismatch { .. })),
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
        FleetValidationError::DuplicateHostname { hostname }
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
            .any(|e| matches!(e, FleetValidationError::DuplicateHostname { .. })),
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
        FleetValidationError::ArchitectureMismatch { architectures }
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
            .any(|e| matches!(e, FleetValidationError::ArchitectureMismatch { .. })),
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
        FleetValidationError::OsMajorVersionMismatch { versions }
        if *versions == vec!["8".to_string(), "9".to_string()]
    )));
}

#[test]
fn test_os_major_version_match_ok() {
    let a = make_snap("host-1", "9.4");
    let b = make_snap("host-2", "9.5");
    let result = validate_snapshots(&[a, b]);
    assert!(
        !result.errors.iter().any(
            |e| matches!(e, FleetValidationError::OsMajorVersionMismatch { .. })
        ),
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
        FleetValidationError::EmptySnapshot { hostname }
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
            .any(|e| matches!(e, FleetValidationError::EmptySnapshot { .. })),
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
        FleetWarning::MinorVersionSpread { versions }
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
            .any(|w| matches!(w, FleetWarning::MinorVersionSpread { .. })),
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
        FleetWarning::SystemTypeMismatch { types }
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
            .any(|w| matches!(w, FleetWarning::SystemTypeMismatch { .. })),
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
        FleetWarning::BaselineConflict {
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
            .any(|w| matches!(w, FleetWarning::BaselineConflict { .. })),
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
    assert!(result.warnings.iter().any(|w| matches!(
        w,
        FleetWarning::StaleScanDates { spread_description }
        if spread_description.contains("2026-01-01") && spread_description.contains("2026-03-15")
    )));
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
            .any(|w| matches!(w, FleetWarning::StaleScanDates { .. })),
        "same timestamps should not trigger stale scan warning"
    );
}

// ===========================================================================
// Combined / integration scenarios
// ===========================================================================

#[test]
fn test_valid_fleet_passes_cleanly() {
    let a = make_snap("host-1", "9.4");
    let b = make_snap("host-2", "9.4");
    let result = validate_snapshots(&[a, b]);
    assert!(result.is_ok(), "valid fleet should have no errors");
    // MinorVersionSpread should not fire since both are 9.4
    assert!(result.warnings.is_empty(), "valid fleet should have no warnings");
}

#[test]
fn test_multiple_errors_reported() {
    // Schema mismatch + duplicate hostname + architecture mismatch
    let mut a = make_snap("web-01", "9.4");
    let mut b = make_snap("web-01", "8.9");
    a.schema_version = 14;
    b.schema_version = 15;
    a.meta.insert(
        "architecture".to_string(),
        serde_json::Value::String("x86_64".to_string()),
    );
    b.meta.insert(
        "architecture".to_string(),
        serde_json::Value::String("aarch64".to_string()),
    );
    let result = validate_snapshots(&[a, b]);
    assert!(result.errors.len() >= 3, "should report schema, hostname, and architecture errors");
    assert!(result
        .errors
        .iter()
        .any(|e| matches!(e, FleetValidationError::SchemaVersionMismatch { .. })));
    assert!(result
        .errors
        .iter()
        .any(|e| matches!(e, FleetValidationError::DuplicateHostname { .. })));
    assert!(result
        .errors
        .iter()
        .any(|e| matches!(e, FleetValidationError::ArchitectureMismatch { .. })));
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

#[test]
fn test_missing_hostname_defaults_to_unknown() {
    let mut a = InspectionSnapshot::new();
    let mut b = InspectionSnapshot::new();
    // No hostname in meta, but give them rpm sections so they aren't empty
    a.rpm = Some(inspectah_core::types::rpm::RpmSection::default());
    b.rpm = Some(inspectah_core::types::rpm::RpmSection::default());
    let result = validate_snapshots(&[a, b]);
    // Both have "<unknown>" hostname, so duplicate should fire
    assert!(result.errors.iter().any(|e| matches!(
        e,
        FleetValidationError::DuplicateHostname { hostname }
        if hostname == "<unknown>"
    )));
}
