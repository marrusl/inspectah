#[test]
fn omitted_include_defaults_to_true() {
    let json = r#"{"schema_version": 14, "rpm": {"packages_added": [{"name": "httpd", "arch": "x86_64", "state": "added"}]}}"#;
    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    assert!(
        snap.rpm.as_ref().unwrap().packages_added[0].include,
        "omitted include must default to true"
    );
}

#[test]
fn explicit_false_preserved() {
    let json = r#"{"schema_version": 14, "rpm": {"packages_added": [{"name": "httpd", "arch": "x86_64", "state": "added", "include": false}]}}"#;
    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    assert!(
        !snap.rpm.as_ref().unwrap().packages_added[0].include,
        "explicit include: false must be preserved"
    );
}

#[test]
fn explicit_true_preserved() {
    let json = r#"{"schema_version": 14, "rpm": {"packages_added": [{"name": "httpd", "arch": "x86_64", "state": "added", "include": true}]}}"#;
    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    assert!(snap.rpm.as_ref().unwrap().packages_added[0].include);
}

#[test]
fn omitted_config_include_defaults_to_true() {
    let json = r#"{"schema_version": 14, "config": {"files": [{"path": "/etc/httpd/conf/httpd.conf", "kind": "rpm_owned_modified", "category": "other"}]}}"#;
    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    assert!(
        snap.config.as_ref().unwrap().files[0].include,
        "omitted config include must default to true"
    );
}

#[test]
fn explicit_config_false_preserved() {
    let json = r#"{"schema_version": 14, "config": {"files": [{"path": "/etc/httpd/conf/httpd.conf", "kind": "rpm_owned_modified", "category": "other", "include": false}]}}"#;
    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    assert!(
        !snap.config.as_ref().unwrap().files[0].include,
        "explicit config include: false must be preserved"
    );
}

#[test]
fn base_image_only_include_false_preserved() {
    let json = r#"{"schema_version": 14, "rpm": {"packages_added": [], "base_image_only": [{"name": "kernel", "arch": "x86_64", "state": "base_image_only", "include": false}]}}"#;
    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    assert!(!snap.rpm.as_ref().unwrap().base_image_only[0].include);
}

#[test]
fn base_image_only_omitted_include_defaults_true() {
    let json = r#"{"schema_version": 14, "rpm": {"packages_added": [], "base_image_only": [{"name": "kernel", "arch": "x86_64", "state": "base_image_only"}]}}"#;
    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    assert!(
        snap.rpm.as_ref().unwrap().base_image_only[0].include,
        "omitted base_image_only include must default to true"
    );
}

#[test]
fn mixed_present_and_absent_includes() {
    let json = r#"{"schema_version": 14, "rpm": {"packages_added": [
        {"name": "httpd", "arch": "x86_64", "state": "added", "include": false},
        {"name": "vim", "arch": "x86_64", "state": "added", "include": true},
        {"name": "curl", "arch": "x86_64", "state": "added"}
    ]}}"#;
    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    let pkgs = &snap.rpm.as_ref().unwrap().packages_added;
    assert!(!pkgs[0].include, "httpd: explicit false preserved");
    assert!(pkgs[1].include, "vim: explicit true preserved");
    assert!(pkgs[2].include, "curl: omitted defaulted to true");
}

#[test]
fn empty_snapshot_loads() {
    let json = r#"{"schema_version": 14}"#;
    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    assert!(snap.rpm.is_none());
    assert!(snap.config.is_none());
}

#[test]
fn reject_below_floor_schema() {
    let json = r#"{"schema_version": 10}"#;
    let result = inspectah_refine::normalize::load_for_refine(json);
    assert!(result.is_err(), "schema_version 10 must be rejected");
}

#[test]
fn reject_future_schema() {
    let json = r#"{"schema_version": 999}"#;
    let result = inspectah_refine::normalize::load_for_refine(json);
    assert!(result.is_err(), "schema_version 999 must be rejected");
}

#[test]
fn go_emitted_snapshot_roundtrip() {
    let json = r#"{"schema_version": 14, "rpm": {"packages_added": [
        {"name": "httpd", "arch": "x86_64", "state": "added", "include": true},
        {"name": "vim", "arch": "x86_64", "state": "added", "include": true}
    ], "base_image_only": [
        {"name": "kernel", "arch": "x86_64", "state": "base_image_only", "include": false}
    ]}, "config": {"files": [
        {"path": "/etc/httpd/conf/httpd.conf", "kind": "rpm_owned_modified", "category": "other", "include": true}
    ]}}"#;
    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    let rpm = snap.rpm.as_ref().unwrap();
    assert!(rpm.packages_added[0].include);
    assert!(rpm.packages_added[1].include);
    assert!(!rpm.base_image_only[0].include);
    assert!(snap.config.as_ref().unwrap().files[0].include);
}

// --- Tier-aware normalize defaults tests ---

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind, ConfigSection};
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
use inspectah_refine::attention::{compute_config_attention, compute_package_attention};
use inspectah_refine::normalize::{normalize_config_defaults, normalize_package_defaults};

#[test]
fn test_tier1_packages_include_true() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "glibc".into(), arch: "x86_64".into(),
            state: PackageState::Added, source_repo: "baseos".into(),
            include: false, ..Default::default()
        }],
        baseline_package_names: Some(vec!["glibc".into()]),
        ..Default::default()
    });
    let pkgs = compute_package_attention(&snap);
    normalize_package_defaults(&mut snap, &pkgs);
    assert!(snap.rpm.as_ref().unwrap().packages_added[0].include);
}

#[test]
fn test_tier3_packages_include_false() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "mystery".into(), arch: "x86_64".into(),
            state: PackageState::LocalInstall, source_repo: "".into(),
            include: true, ..Default::default()
        }],
        ..Default::default()
    });
    let pkgs = compute_package_attention(&snap);
    normalize_package_defaults(&mut snap, &pkgs);
    assert!(!snap.rpm.as_ref().unwrap().packages_added[0].include);
}

#[test]
fn test_leaf_filtering_hides_non_leaf_tier2() {
    // Leaf filtering applies to Tier 2 (Informational) packages.
    // With baseline present, user-added packages from recognized repos are
    // now Routine (Tier 1), so we use degraded mode (no baseline) to get
    // Informational/ProvenanceUnavailable classification for leaf filtering.
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry { name: "httpd".into(), arch: "x86_64".into(),
                state: PackageState::Added, source_repo: "appstream".into(),
                include: true, ..Default::default() },
            PackageEntry { name: "apr".into(), arch: "x86_64".into(),
                state: PackageState::Added, source_repo: "appstream".into(),
                include: true, ..Default::default() },
        ],
        baseline_package_names: None, // degraded mode -> Informational
        leaf_packages: Some(vec!["httpd".into()]),
        ..Default::default()
    });
    let pkgs = compute_package_attention(&snap);
    normalize_package_defaults(&mut snap, &pkgs);
    let rpm = snap.rpm.as_ref().unwrap();
    assert!(rpm.packages_added[0].include, "httpd is leaf");
    assert!(!rpm.packages_added[1].include, "apr is non-leaf, hidden");
}

#[test]
fn test_tier1_configs_include_false_not_copied() {
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        files: vec![
            ConfigFileEntry { path: "/etc/default.conf".into(),
                kind: ConfigFileKind::RpmOwnedDefault, include: true, ..Default::default() },
            ConfigFileEntry { path: "/etc/baseline.conf".into(),
                kind: ConfigFileKind::BaselineMatch, include: true, ..Default::default() },
            ConfigFileEntry { path: "/etc/custom.conf".into(),
                kind: ConfigFileKind::Unowned, include: true, ..Default::default() },
        ],
    });
    let configs = compute_config_attention(&snap);
    normalize_config_defaults(&mut snap, &configs);
    let files = &snap.config.as_ref().unwrap().files;
    assert!(!files[0].include, "RpmOwnedDefault must not be copied");
    assert!(!files[1].include, "BaselineMatch must not be copied");
    assert!(files[2].include, "Unowned must be copied");
}

#[test]
fn test_orphaned_configs_include_false() {
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/old.conf".into(), kind: ConfigFileKind::Orphaned,
            include: true, ..Default::default()
        }],
    });
    let configs = compute_config_attention(&snap);
    normalize_config_defaults(&mut snap, &configs);
    assert!(!snap.config.as_ref().unwrap().files[0].include);
}

#[test]
fn test_tier2_leaf_fallback_when_no_leaf_data() {
    // Use degraded mode (no baseline) to produce Tier 2 (Informational).
    // With baseline present, user-added packages are now Routine (Tier 1).
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry { name: "httpd".into(), arch: "x86_64".into(),
                state: PackageState::Added, source_repo: "appstream".into(),
                include: false, ..Default::default() },
        ],
        baseline_package_names: None, // degraded mode -> Informational
        leaf_packages: None, // no leaf data
        ..Default::default()
    });
    let pkgs = compute_package_attention(&snap);
    normalize_package_defaults(&mut snap, &pkgs);
    assert!(snap.rpm.as_ref().unwrap().packages_added[0].include,
        "without leaf data, all Tier 2 should be visible");
}

#[test]
fn test_user_added_with_baseline_is_routine_included() {
    // With baseline present, user-added packages from recognized repos
    // are Routine (Tier 1) and always included, regardless of leaf status.
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry { name: "httpd".into(), arch: "x86_64".into(),
                state: PackageState::Added, source_repo: "appstream".into(),
                include: false, ..Default::default() },
            PackageEntry { name: "apr".into(), arch: "x86_64".into(),
                state: PackageState::Added, source_repo: "appstream".into(),
                include: false, ..Default::default() },
        ],
        baseline_package_names: Some(vec![]),
        leaf_packages: Some(vec!["httpd".into()]),
        ..Default::default()
    });
    let pkgs = compute_package_attention(&snap);
    normalize_package_defaults(&mut snap, &pkgs);
    let rpm = snap.rpm.as_ref().unwrap();
    assert!(rpm.packages_added[0].include, "httpd: Routine, always included");
    assert!(rpm.packages_added[1].include, "apr: also Routine with baseline, always included");
}
