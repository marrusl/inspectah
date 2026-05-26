#[test]
fn omitted_include_defaults_to_true() {
    let json = r#"{"schema_version": 17, "rpm": {"packages_added": [{"name": "httpd", "arch": "x86_64", "state": "added"}]}}"#;
    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    assert!(
        snap.rpm.as_ref().unwrap().packages_added[0].include,
        "omitted include must default to true"
    );
}

#[test]
fn explicit_false_preserved() {
    let json = r#"{"schema_version": 17, "rpm": {"packages_added": [{"name": "httpd", "arch": "x86_64", "state": "added", "include": false}]}}"#;
    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    assert!(
        !snap.rpm.as_ref().unwrap().packages_added[0].include,
        "explicit include: false must be preserved"
    );
}

#[test]
fn explicit_true_preserved() {
    let json = r#"{"schema_version": 17, "rpm": {"packages_added": [{"name": "httpd", "arch": "x86_64", "state": "added", "include": true}]}}"#;
    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    assert!(snap.rpm.as_ref().unwrap().packages_added[0].include);
}

#[test]
fn omitted_config_include_defaults_to_true() {
    let json = r#"{"schema_version": 17, "config": {"files": [{"path": "/etc/httpd/conf/httpd.conf", "kind": "rpm_owned_modified", "category": "other"}]}}"#;
    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    assert!(
        snap.config.as_ref().unwrap().files[0].include,
        "omitted config include must default to true"
    );
}

#[test]
fn explicit_config_false_preserved() {
    let json = r#"{"schema_version": 17, "config": {"files": [{"path": "/etc/httpd/conf/httpd.conf", "kind": "rpm_owned_modified", "category": "other", "include": false}]}}"#;
    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    assert!(
        !snap.config.as_ref().unwrap().files[0].include,
        "explicit config include: false must be preserved"
    );
}

#[test]
fn base_image_only_include_false_preserved() {
    let json = r#"{"schema_version": 17, "rpm": {"packages_added": [], "base_image_only": [{"name": "kernel", "arch": "x86_64", "state": "base_image_only", "include": false}]}}"#;
    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    assert!(!snap.rpm.as_ref().unwrap().base_image_only[0].include);
}

#[test]
fn base_image_only_omitted_include_defaults_true() {
    let json = r#"{"schema_version": 17, "rpm": {"packages_added": [], "base_image_only": [{"name": "kernel", "arch": "x86_64", "state": "base_image_only"}]}}"#;
    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    assert!(
        snap.rpm.as_ref().unwrap().base_image_only[0].include,
        "omitted base_image_only include must default to true"
    );
}

#[test]
fn mixed_present_and_absent_includes() {
    let json = r#"{"schema_version": 17, "rpm": {"packages_added": [
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
    let json = r#"{"schema_version": 17}"#;
    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    assert!(snap.rpm.is_none());
    assert!(snap.config.is_none());
}

#[test]
fn reject_below_floor_schema() {
    let json = r#"{"schema_version": 16}"#;
    let result = inspectah_refine::normalize::load_for_refine(json);
    assert!(result.is_err(), "schema_version 16 must be rejected");
}

#[test]
fn reject_future_schema() {
    let json = r#"{"schema_version": 999}"#;
    let result = inspectah_refine::normalize::load_for_refine(json);
    assert!(result.is_err(), "schema_version 999 must be rejected");
}

#[test]
fn snapshot_with_all_sections_roundtrip() {
    let json = r#"{"schema_version": 17, "rpm": {"packages_added": [
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

use inspectah_core::baseline::{BaselineData, BaselinePackageEntry};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind, ConfigSection};
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
use inspectah_refine::classify::{classify_configs, classify_packages};
use inspectah_refine::normalize::{normalize_config_defaults, normalize_package_defaults};

/// Helper: build a BaselineData with the given package names (all x86_64).
fn make_baseline(names: &[&str]) -> BaselineData {
    BaselineData {
        image_digest: "sha256:test".into(),
        packages: names
            .iter()
            .map(|n| {
                let key = format!("{}.x86_64", n);
                (
                    key,
                    BaselinePackageEntry {
                        name: n.to_string(),
                        epoch: Some("0".into()),
                        version: "1.0".into(),
                        release: "1.el9".into(),
                        arch: "x86_64".into(),
                    },
                )
            })
            .collect(),
        extracted_at: "2026-05-17T00:00:00Z".into(),
    }
}

#[test]
fn test_tier1_packages_include_true() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "glibc".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            source_repo: "baseos".into(),
            include: false,
            ..Default::default()
        }],
        ..Default::default()
    });
    snap.baseline = Some(make_baseline(&["glibc"]));
    let pkgs = classify_packages(&snap);
    normalize_package_defaults(&mut snap, &pkgs);
    assert!(snap.rpm.as_ref().unwrap().packages_added[0].include);
}

#[test]
fn test_tier3_packages_include_false() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "mystery".into(),
            arch: "x86_64".into(),
            state: PackageState::LocalInstall,
            source_repo: "".into(),
            include: true,
            ..Default::default()
        }],
        ..Default::default()
    });
    let pkgs = classify_packages(&snap);
    normalize_package_defaults(&mut snap, &pkgs);
    assert!(!snap.rpm.as_ref().unwrap().packages_added[0].include);
}

#[test]
fn test_leaf_filtering_hides_non_leaf_site() {
    // Leaf filtering applies to Site (user-added) packages.
    // Baseline present (empty) puts us in verified mode where user-added
    // packages from recognized repos are Site. Non-leaf Site packages are hidden.
    let mut snap = InspectionSnapshot::new();
    snap.baseline = Some(make_baseline(&[]));
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "appstream".into(),
                include: true,
                ..Default::default()
            },
            PackageEntry {
                name: "apr".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "appstream".into(),
                include: true,
                ..Default::default()
            },
        ],
        leaf_packages: Some(vec!["httpd.x86_64".into()]),
        ..Default::default()
    });
    let pkgs = classify_packages(&snap);
    normalize_package_defaults(&mut snap, &pkgs);
    let rpm = snap.rpm.as_ref().unwrap();
    assert!(rpm.packages_added[0].include, "httpd is leaf");
    assert!(!rpm.packages_added[1].include, "apr is non-leaf, hidden");
}

#[test]
fn test_leaf_defaults_do_not_leak_across_arches() {
    let mut snap = InspectionSnapshot::new();
    snap.baseline = Some(make_baseline(&[]));
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "glibc".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "baseos".into(),
                include: false,
                ..Default::default()
            },
            PackageEntry {
                name: "glibc".into(),
                arch: "i686".into(),
                state: PackageState::Added,
                source_repo: "baseos".into(),
                include: false,
                ..Default::default()
            },
        ],
        leaf_packages: Some(vec!["glibc.x86_64".into()]),
        auto_packages: Some(vec!["glibc.i686".into()]),
        leaf_dep_tree: serde_json::json!({}),
        ..Default::default()
    });

    let pkgs = classify_packages(&snap);
    normalize_package_defaults(&mut snap, &pkgs);

    let rpm = snap.rpm.as_ref().unwrap();
    assert!(
        rpm.packages_added[0].include,
        "x86_64 leaf must stay included"
    );
    assert!(
        !rpm.packages_added[1].include,
        "i686 auto package must stay excluded"
    );
}

#[test]
fn test_tier1_configs_include_false_not_copied() {
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        files: vec![
            ConfigFileEntry {
                path: "/etc/default.conf".into(),
                kind: ConfigFileKind::RpmOwnedDefault,
                include: true,
                ..Default::default()
            },
            ConfigFileEntry {
                path: "/etc/baseline.conf".into(),
                kind: ConfigFileKind::BaselineMatch,
                include: true,
                ..Default::default()
            },
            ConfigFileEntry {
                path: "/etc/custom.conf".into(),
                kind: ConfigFileKind::Unowned,
                include: true,
                ..Default::default()
            },
        ],
    });
    let configs = classify_configs(&snap);
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
            path: "/etc/old.conf".into(),
            kind: ConfigFileKind::Orphaned,
            include: true,
            ..Default::default()
        }],
    });
    let configs = classify_configs(&snap);
    normalize_config_defaults(&mut snap, &configs);
    assert!(!snap.config.as_ref().unwrap().files[0].include);
}

#[test]
fn test_site_leaf_fallback_when_no_leaf_data() {
    // Use baseline present (empty) to get Site (user-added) classification.
    // Without leaf data, all Site packages should be visible.
    let mut snap = InspectionSnapshot::new();
    snap.baseline = Some(make_baseline(&[]));
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "httpd".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            source_repo: "appstream".into(),
            include: false,
            ..Default::default()
        }],
        leaf_packages: None, // no leaf data
        ..Default::default()
    });
    let pkgs = classify_packages(&snap);
    normalize_package_defaults(&mut snap, &pkgs);
    assert!(
        snap.rpm.as_ref().unwrap().packages_added[0].include,
        "without leaf data, all Site packages should be visible"
    );
}

#[test]
fn test_user_added_with_baseline_is_site_leaf_filtered() {
    // With baseline present, user-added packages from recognized repos
    // are Site. Leaf filtering applies: only leaf packages are included.
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "appstream".into(),
                include: false,
                ..Default::default()
            },
            PackageEntry {
                name: "apr".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "appstream".into(),
                include: false,
                ..Default::default()
            },
        ],
        leaf_packages: Some(vec!["httpd.x86_64".into()]),
        ..Default::default()
    });
    // Empty baseline (no packages) — presence of baseline puts us in verified mode
    snap.baseline = Some(make_baseline(&[]));
    let pkgs = classify_packages(&snap);
    normalize_package_defaults(&mut snap, &pkgs);
    let rpm = snap.rpm.as_ref().unwrap();
    assert!(
        rpm.packages_added[0].include,
        "httpd: Site leaf, included"
    );
    assert!(
        !rpm.packages_added[1].include,
        "apr: Site non-leaf, excluded by leaf filter"
    );
}
