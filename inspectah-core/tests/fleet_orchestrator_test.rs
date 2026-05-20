use inspectah_core::baseline::{BaselineData, ResolutionStrategy, TargetImageIdentity};
use inspectah_core::fleet::manifest::FleetManifest;
use inspectah_core::fleet::merge_snapshots;
use inspectah_core::fleet::validate::{FleetValidationError, FleetWarning};
use inspectah_core::snapshot::{InspectionSnapshot, SCHEMA_VERSION};
use inspectah_core::types::completeness::{Completeness, InspectorId};
use inspectah_core::types::config::{ConfigFileEntry, ConfigSection};
use inspectah_core::types::fleet::VariantSelection;
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::rpm::{PackageEntry, RpmSection};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_snap(hostname: &str) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.meta.insert(
        "hostname".to_string(),
        serde_json::Value::String(hostname.into()),
    );
    snap.os_release = Some(OsRelease {
        version_id: "9.4".into(),
        ..Default::default()
    });
    // Add a minimal section so validation doesn't reject as empty
    snap.rpm = Some(RpmSection::default());
    snap
}

fn make_snap_with_target(hostname: &str, image_ref: &str) -> InspectionSnapshot {
    let mut snap = make_snap(hostname);
    snap.target_image = Some(TargetImageIdentity {
        image_ref: image_ref.into(),
        strategy: ResolutionStrategy::OsRelease,
    });
    snap
}

// ---------------------------------------------------------------------------
// Basic merge
// ---------------------------------------------------------------------------

#[test]
fn test_merge_two_minimal_snapshots() {
    let s1 = make_snap("host-a");
    let s2 = make_snap("host-b");

    let (merged, warnings) = merge_snapshots(vec![s1, s2], None).unwrap();

    assert_eq!(merged.schema_version, SCHEMA_VERSION);
    let meta = merged.fleet_meta.as_ref().unwrap();
    assert_eq!(meta.host_count, 2);
    assert_eq!(meta.hostnames, vec!["host-a", "host-b"]);
    assert_eq!(meta.label, "fleet");
    assert!(!meta.merged_at.is_empty());
    assert!(warnings.is_empty());
}

#[test]
fn test_merge_sorts_by_hostname() {
    let s1 = make_snap("zebra");
    let s2 = make_snap("alpha");
    let s3 = make_snap("middle");

    let (merged, _) = merge_snapshots(vec![s1, s2, s3], None).unwrap();

    let meta = merged.fleet_meta.as_ref().unwrap();
    assert_eq!(meta.hostnames, vec!["alpha", "middle", "zebra"]);
}

#[test]
fn test_merge_with_manifest_label() {
    let s1 = make_snap("host-a");
    let s2 = make_snap("host-b");
    let manifest = FleetManifest {
        label: Some("web-tier".into()),
        baseline: None,
        sources: vec![],
    };

    let (merged, _) = merge_snapshots(vec![s1, s2], Some(&manifest)).unwrap();

    assert_eq!(merged.fleet_meta.as_ref().unwrap().label, "web-tier");
}

// ---------------------------------------------------------------------------
// Validation errors propagate
// ---------------------------------------------------------------------------

#[test]
fn test_merge_rejects_single_snapshot() {
    let s1 = make_snap("lonely");
    let result = merge_snapshots(vec![s1], None);
    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, FleetValidationError::TooFewSnapshots { .. }))
    );
}

#[test]
fn test_merge_rejects_schema_mismatch() {
    let mut s1 = make_snap("host-a");
    let mut s2 = make_snap("host-b");
    s1.schema_version = 15;
    s2.schema_version = 16;

    let result = merge_snapshots(vec![s1, s2], None);
    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, FleetValidationError::SchemaVersionMismatch { .. }))
    );
}

// ---------------------------------------------------------------------------
// Target image selection
// ---------------------------------------------------------------------------

#[test]
fn test_merge_selects_most_common_target_image() {
    let s1 = make_snap_with_target("host-a", "quay.io/rhel:9.4");
    let s2 = make_snap_with_target("host-b", "quay.io/rhel:9.4");
    let s3 = make_snap_with_target("host-c", "quay.io/rhel:9.3");

    let (merged, warnings) = merge_snapshots(vec![s1, s2, s3], None).unwrap();

    let ti = merged.target_image.unwrap();
    assert_eq!(ti.image_ref, "quay.io/rhel:9.4");
    assert_eq!(ti.strategy, ResolutionStrategy::OsRelease);

    // Should have a BaselineConflict warning
    assert!(
        warnings
            .iter()
            .any(|w| matches!(w, FleetWarning::BaselineConflict { .. }))
    );

    // baseline_provisional should be true since there were conflicts
    assert!(merged.fleet_meta.as_ref().unwrap().baseline_provisional);
}

#[test]
fn test_merge_manifest_baseline_override() {
    let s1 = make_snap_with_target("host-a", "quay.io/rhel:9.3");
    let s2 = make_snap_with_target("host-b", "quay.io/rhel:9.4");
    let manifest = FleetManifest {
        label: None,
        baseline: Some("registry.redhat.io/rhel9/rhel-bootc:9.6".into()),
        sources: vec![],
    };

    let (merged, _) = merge_snapshots(vec![s1, s2], Some(&manifest)).unwrap();

    let ti = merged.target_image.unwrap();
    assert_eq!(ti.image_ref, "registry.redhat.io/rhel9/rhel-bootc:9.6");
    assert_eq!(ti.strategy, ResolutionStrategy::CliOverride);
    assert!(!merged.fleet_meta.as_ref().unwrap().baseline_provisional);
}

#[test]
fn test_merge_unanimous_target_not_provisional() {
    let s1 = make_snap_with_target("host-a", "quay.io/rhel:9.4");
    let s2 = make_snap_with_target("host-b", "quay.io/rhel:9.4");

    let (merged, _) = merge_snapshots(vec![s1, s2], None).unwrap();

    assert!(!merged.fleet_meta.as_ref().unwrap().baseline_provisional);
}

// ---------------------------------------------------------------------------
// Baseline selection
// ---------------------------------------------------------------------------

#[test]
fn test_merge_selects_baseline_from_matching_host() {
    let mut s1 = make_snap_with_target("host-a", "quay.io/rhel:9.4");
    s1.baseline = Some(BaselineData {
        image_digest: "sha256:abc".into(),
        packages: HashMap::new(),
        extracted_at: "2026-01-01T00:00:00Z".into(),
    });
    let s2 = make_snap_with_target("host-b", "quay.io/rhel:9.4");

    let (merged, _) = merge_snapshots(vec![s1, s2], None).unwrap();

    assert!(merged.baseline.is_some());
    assert_eq!(merged.baseline.unwrap().image_digest, "sha256:abc");
    assert!(!merged.no_baseline);
}

#[test]
fn test_merge_no_baseline_sets_flag() {
    let s1 = make_snap("host-a");
    let s2 = make_snap("host-b");

    let (merged, _) = merge_snapshots(vec![s1, s2], None).unwrap();

    assert!(merged.baseline.is_none());
    assert!(merged.no_baseline);
}

// ---------------------------------------------------------------------------
// Completeness merge
// ---------------------------------------------------------------------------

#[test]
fn test_merge_completeness_all_complete() {
    let s1 = make_snap("host-a");
    let s2 = make_snap("host-b");

    let (merged, _) = merge_snapshots(vec![s1, s2], None).unwrap();
    assert_eq!(merged.completeness, Completeness::Complete);
}

#[test]
fn test_merge_completeness_partial_propagates() {
    let mut s1 = make_snap("host-a");
    s1.completeness = Completeness::Partial {
        degraded_sections: vec![InspectorId::Config],
        reason: "config timeout".into(),
    };
    let s2 = make_snap("host-b");

    let (merged, _) = merge_snapshots(vec![s1, s2], None).unwrap();

    match &merged.completeness {
        Completeness::Partial {
            degraded_sections,
            reason,
        } => {
            assert!(degraded_sections.contains(&InspectorId::Config));
            assert!(reason.contains("partial"));
        }
        other => panic!("expected Partial, got {:?}", other),
    }
}

#[test]
fn test_merge_completeness_incomplete_overrides_partial() {
    let mut s1 = make_snap("host-a");
    s1.completeness = Completeness::Partial {
        degraded_sections: vec![InspectorId::Config],
        reason: "config degraded".into(),
    };
    let mut s2 = make_snap("host-b");
    s2.completeness = Completeness::Incomplete {
        failed_sections: vec![InspectorId::Rpm],
        degraded_sections: vec![],
        reason: "rpm failed".into(),
    };

    let (merged, _) = merge_snapshots(vec![s1, s2], None).unwrap();

    match &merged.completeness {
        Completeness::Incomplete {
            failed_sections,
            degraded_sections,
            ..
        } => {
            assert!(failed_sections.contains(&InspectorId::Rpm));
            assert!(degraded_sections.contains(&InspectorId::Config));
        }
        other => panic!("expected Incomplete, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Trust-sensitive fields
// ---------------------------------------------------------------------------

#[test]
fn test_merge_sensitive_flags_or_semantics() {
    let mut s1 = make_snap("host-a");
    s1.sensitive_snapshot = true;
    s1.preserved_credentials = false;
    s1.preserved_ssh_keys = false;

    let mut s2 = make_snap("host-b");
    s2.sensitive_snapshot = false;
    s2.preserved_credentials = true;
    s2.preserved_ssh_keys = true;

    let (merged, _) = merge_snapshots(vec![s1, s2], None).unwrap();

    assert!(merged.sensitive_snapshot);
    assert!(merged.preserved_credentials);
    assert!(merged.preserved_ssh_keys);
}

#[test]
fn test_merge_clears_redaction_state() {
    let mut s1 = make_snap("host-a");
    s1.redaction_state = Some(
        inspectah_core::types::redaction::RedactionState::FullyRedacted {
            redacted_by: "inspectah 0.8.0".into(),
            config_hash: "abc123".into(),
        },
    );
    let s2 = make_snap("host-b");

    let (merged, _) = merge_snapshots(vec![s1, s2], None).unwrap();
    assert!(merged.redaction_state.is_none());
}

// ---------------------------------------------------------------------------
// os_release from first sorted host
// ---------------------------------------------------------------------------

#[test]
fn test_merge_os_release_from_first_sorted_host() {
    let mut s1 = make_snap("host-b");
    s1.os_release = Some(OsRelease {
        version_id: "9.4".into(),
        name: "Red Hat Enterprise Linux".into(),
        ..Default::default()
    });
    let mut s2 = make_snap("host-a");
    s2.os_release = Some(OsRelease {
        version_id: "9.4".into(),
        name: "RHEL from host-a".into(),
        ..Default::default()
    });

    let (merged, _) = merge_snapshots(vec![s1, s2], None).unwrap();

    // host-a sorts first, so its os_release should be used
    let os = merged.os_release.unwrap();
    assert_eq!(os.name, "RHEL from host-a");
}

// ---------------------------------------------------------------------------
// Section host counts
// ---------------------------------------------------------------------------

#[test]
fn test_merge_section_host_counts() {
    let mut s1 = make_snap("host-a");
    s1.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/foo.conf".into(),
            content: "x".into(),
            ..Default::default()
        }],
    });

    let s2 = make_snap("host-b");
    // s2 has rpm (from make_snap) but no config

    let (merged, _) = merge_snapshots(vec![s1, s2], None).unwrap();

    let counts = &merged.fleet_meta.as_ref().unwrap().section_host_counts;
    assert_eq!(counts.get("rpm"), Some(&2));
    assert_eq!(counts.get("config"), Some(&1));
}

// ---------------------------------------------------------------------------
// Section data flows through adapters
// ---------------------------------------------------------------------------

#[test]
fn test_merge_rpm_section_flows_through() {
    let mut s1 = make_snap("host-a");
    s1.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "httpd".into(),
            arch: "x86_64".into(),
            ..Default::default()
        }],
        ..Default::default()
    });

    let mut s2 = make_snap("host-b");
    s2.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                ..Default::default()
            },
            PackageEntry {
                name: "nginx".into(),
                arch: "x86_64".into(),
                ..Default::default()
            },
        ],
        ..Default::default()
    });

    let (merged, _) = merge_snapshots(vec![s1, s2], None).unwrap();

    let rpm = merged.rpm.unwrap();
    assert_eq!(rpm.packages_added.len(), 2);
    let httpd = rpm
        .packages_added
        .iter()
        .find(|p| p.name == "httpd")
        .unwrap();
    assert_eq!(httpd.fleet.as_ref().unwrap().count, 2);
}

#[test]
fn test_merge_config_variants_flow_through() {
    let mut s1 = make_snap("host-a");
    s1.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/app.conf".into(),
            content: "version_a".into(),
            ..Default::default()
        }],
    });

    let mut s2 = make_snap("host-b");
    s2.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/app.conf".into(),
            content: "version_b".into(),
            ..Default::default()
        }],
    });

    let (merged, _) = merge_snapshots(vec![s1, s2], None).unwrap();

    let config = merged.config.unwrap();
    assert_eq!(config.files.len(), 2);
    assert!(
        config
            .files
            .iter()
            .any(|f| f.variant_selection == VariantSelection::Selected)
    );
    assert!(
        config
            .files
            .iter()
            .any(|f| f.variant_selection == VariantSelection::Alternative)
    );
}

// ---------------------------------------------------------------------------
// Merged snapshot serialization roundtrip
// ---------------------------------------------------------------------------

#[test]
fn test_merged_snapshot_serializes_and_deserializes() {
    let s1 = make_snap("host-a");
    let s2 = make_snap("host-b");

    let (merged, _) = merge_snapshots(vec![s1, s2], None).unwrap();

    let json = serde_json::to_string(&merged).unwrap();
    let parsed: InspectionSnapshot = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed.schema_version, SCHEMA_VERSION);
    assert!(parsed.fleet_meta.is_some());
    assert_eq!(parsed.fleet_meta.as_ref().unwrap().host_count, 2);
}
