use inspectah_core::types::FindingKind;
#[test]
fn omitted_include_defaults_to_true() {
    let json = r#"{"schema_version": 31, "rpm": {"packages_added": [{"name": "httpd", "arch": "x86_64", "state": "added"}]}}"#;
    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    assert!(
        snap.rpm.as_ref().unwrap().packages_added[0]
            .disposition
            .is_included(),
        "omitted include must default to true"
    );
}

#[test]
fn explicit_false_preserved() {
    let json = r#"{"schema_version": 31, "rpm": {"packages_added": [{"name": "httpd", "arch": "x86_64", "state": "added", "disposition": {"kind": "actionable", "include": false}}]}}"#;
    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    assert!(
        !snap.rpm.as_ref().unwrap().packages_added[0]
            .disposition
            .is_included(),
        "explicit include: false must be preserved"
    );
}

#[test]
fn explicit_true_preserved() {
    let json = r#"{"schema_version": 31, "rpm": {"packages_added": [{"name": "httpd", "arch": "x86_64", "state": "added", "disposition": {"kind": "actionable", "include": true}}]}}"#;
    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    assert!(
        snap.rpm.as_ref().unwrap().packages_added[0]
            .disposition
            .is_included()
    );
}

#[test]
fn omitted_config_include_defaults_to_true() {
    let json = r#"{"schema_version": 31, "config": {"files": [{"path": "/etc/httpd/conf/httpd.conf", "kind": "rpm_owned_modified", "category": "other"}]}}"#;
    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    assert!(
        snap.config.as_ref().unwrap().files[0]
            .disposition
            .is_included(),
        "omitted config include must default to true"
    );
}

#[test]
fn explicit_config_false_preserved() {
    let json = r#"{"schema_version": 31, "config": {"files": [{"path": "/etc/httpd/conf/httpd.conf", "kind": "rpm_owned_modified", "category": "other", "disposition": {"kind": "actionable", "include": false}}]}}"#;
    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    assert!(
        !snap.config.as_ref().unwrap().files[0]
            .disposition
            .is_included(),
        "explicit config include: false must be preserved"
    );
}

#[test]
fn base_image_only_include_false_preserved() {
    let json = r#"{"schema_version": 31, "rpm": {"packages_added": [], "base_image_only": [{"name": "kernel", "arch": "x86_64", "state": "base_image_only", "disposition": {"kind": "actionable", "include": false}}]}}"#;
    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    assert!(
        !snap.rpm.as_ref().unwrap().base_image_only[0]
            .disposition
            .is_included()
    );
}

#[test]
fn base_image_only_omitted_include_defaults_true() {
    let json = r#"{"schema_version": 31, "rpm": {"packages_added": [], "base_image_only": [{"name": "kernel", "arch": "x86_64", "state": "base_image_only"}]}}"#;
    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    assert!(
        snap.rpm.as_ref().unwrap().base_image_only[0]
            .disposition
            .is_included(),
        "omitted base_image_only include must default to true"
    );
}

#[test]
fn mixed_present_and_absent_includes() {
    let json = r#"{"schema_version": 31, "rpm": {"packages_added": [
        {"name": "httpd", "arch": "x86_64", "state": "added", "disposition": {"kind": "actionable", "include": false}},
        {"name": "vim", "arch": "x86_64", "state": "added", "disposition": {"kind": "actionable", "include": true}},
        {"name": "curl", "arch": "x86_64", "state": "added"}
    ]}}"#;
    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    let pkgs = &snap.rpm.as_ref().unwrap().packages_added;
    assert!(
        !pkgs[0].disposition.is_included(),
        "httpd: explicit false preserved"
    );
    assert!(
        pkgs[1].disposition.is_included(),
        "vim: explicit true preserved"
    );
    assert!(
        pkgs[2].disposition.is_included(),
        "curl: omitted defaulted to true"
    );
}

#[test]
fn empty_snapshot_loads() {
    let json = r#"{"schema_version": 31}"#;
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
    let json = r#"{"schema_version": 31, "rpm": {"packages_added": [
        {"name": "httpd", "arch": "x86_64", "state": "added", "disposition": {"kind": "actionable", "include": true}},
        {"name": "vim", "arch": "x86_64", "state": "added", "disposition": {"kind": "actionable", "include": true}}
    ], "base_image_only": [
        {"name": "kernel", "arch": "x86_64", "state": "base_image_only", "disposition": {"kind": "actionable", "include": false}}
    ]}, "config": {"files": [
        {"path": "/etc/httpd/conf/httpd.conf", "kind": "rpm_owned_modified", "category": "other", "disposition": {"kind": "actionable", "include": true}}
    ]}}"#;
    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    let rpm = snap.rpm.as_ref().unwrap();
    assert!(rpm.packages_added[0].disposition.is_included());
    assert!(rpm.packages_added[1].disposition.is_included());
    assert!(!rpm.base_image_only[0].disposition.is_included());
    assert!(
        snap.config.as_ref().unwrap().files[0]
            .disposition
            .is_included()
    );
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
            disposition: FindingKind::excluded(),
            ..Default::default()
        }],
        ..Default::default()
    });
    snap.baseline = Some(make_baseline(&["glibc"]));
    let pkgs = classify_packages(&snap);
    normalize_package_defaults(&mut snap, &pkgs);
    assert!(
        snap.rpm.as_ref().unwrap().packages_added[0]
            .disposition
            .is_included()
    );
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
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        ..Default::default()
    });
    let pkgs = classify_packages(&snap);
    normalize_package_defaults(&mut snap, &pkgs);
    assert!(
        !snap.rpm.as_ref().unwrap().packages_added[0]
            .disposition
            .is_included()
    );
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
                disposition: FindingKind::included(),
                ..Default::default()
            },
            PackageEntry {
                name: "apr".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "appstream".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
        ],
        leaf_packages: Some(vec!["httpd.x86_64".into()]),
        ..Default::default()
    });
    let pkgs = classify_packages(&snap);
    normalize_package_defaults(&mut snap, &pkgs);
    let rpm = snap.rpm.as_ref().unwrap();
    assert!(
        rpm.packages_added[0].disposition.is_included(),
        "httpd is leaf"
    );
    assert!(
        !rpm.packages_added[1].disposition.is_included(),
        "apr is non-leaf, hidden"
    );
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
                disposition: FindingKind::excluded(),
                ..Default::default()
            },
            PackageEntry {
                name: "glibc".into(),
                arch: "i686".into(),
                state: PackageState::Added,
                source_repo: "baseos".into(),
                disposition: FindingKind::excluded(),
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
        rpm.packages_added[0].disposition.is_included(),
        "x86_64 leaf must stay included"
    );
    assert!(
        !rpm.packages_added[1].disposition.is_included(),
        "i686 auto package must stay excluded"
    );
}

#[test]
fn test_tier1_configs_include_false_not_copied() {
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        withheld: Vec::new(),
        files: vec![
            ConfigFileEntry {
                path: "/etc/default.conf".into(),
                kind: ConfigFileKind::RpmOwnedDefault,
                disposition: FindingKind::included(),
                ..Default::default()
            },
            ConfigFileEntry {
                path: "/etc/baseline.conf".into(),
                kind: ConfigFileKind::BaselineMatch,
                disposition: FindingKind::included(),
                ..Default::default()
            },
            ConfigFileEntry {
                path: "/etc/custom.conf".into(),
                kind: ConfigFileKind::Unowned,
                disposition: FindingKind::included(),
                ..Default::default()
            },
        ],
    });
    let configs = classify_configs(&snap);
    normalize_config_defaults(&mut snap, &configs);
    let files = &snap.config.as_ref().unwrap().files;
    assert!(
        !files[0].disposition.is_included(),
        "RpmOwnedDefault must not be copied"
    );
    assert!(
        !files[1].disposition.is_included(),
        "BaselineMatch must not be copied"
    );
    assert!(files[2].disposition.is_included(), "Unowned must be copied");
}

#[test]
fn test_orphaned_configs_include_false() {
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        withheld: Vec::new(),
        files: vec![ConfigFileEntry {
            path: "/etc/old.conf".into(),
            kind: ConfigFileKind::Orphaned,
            disposition: FindingKind::included(),
            ..Default::default()
        }],
    });
    let configs = classify_configs(&snap);
    normalize_config_defaults(&mut snap, &configs);
    assert!(
        !snap.config.as_ref().unwrap().files[0]
            .disposition
            .is_included()
    );
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
            disposition: FindingKind::excluded(),
            ..Default::default()
        }],
        leaf_packages: None, // no leaf data
        ..Default::default()
    });
    let pkgs = classify_packages(&snap);
    normalize_package_defaults(&mut snap, &pkgs);
    assert!(
        snap.rpm.as_ref().unwrap().packages_added[0]
            .disposition
            .is_included(),
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
                disposition: FindingKind::excluded(),
                ..Default::default()
            },
            PackageEntry {
                name: "apr".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "appstream".into(),
                disposition: FindingKind::excluded(),
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
        rpm.packages_added[0].disposition.is_included(),
        "httpd: Site leaf, included"
    );
    assert!(
        !rpm.packages_added[1].disposition.is_included(),
        "apr: Site non-leaf, excluded by leaf filter"
    );
}

// ── Image-mode-incompatible units ─────────────────────────────────────

/// The normalizer still locks out the three package-manager units, their
/// drop-ins and their enabled inventory entries, and leaves any other unit
/// alone, the runtime updater `packagekit-offline-update.service` included:
/// it is excluded by default at collect, not locked here (§ 10.4).
#[test]
fn incompatible_units_are_locked_out_and_others_left_alone() {
    use inspectah_core::types::units::{
        PresetDefault, SystemdDropIn, UnitInventoryEntry, UnitName, UnitSection, UnitState,
        UnitStateChange,
    };
    use inspectah_refine::normalize::normalize_incompatible_services;

    const INCOMPATIBLE: [&str; 3] = [
        "dnf-makecache.service",
        "dnf-makecache.timer",
        "packagekit.service",
    ];
    const CONTROLS: [&str; 2] = ["httpd.service", "packagekit-offline-update.service"];

    let name = |unit: &str| UnitName::try_from(unit.to_string()).unwrap();
    let units: Vec<&str> = INCOMPATIBLE.iter().copied().chain(CONTROLS).collect();
    let mut snap = InspectionSnapshot::new();
    snap.units = Some(UnitSection {
        state_changes: units
            .iter()
            .map(|u| {
                UnitStateChange::new(
                    test_carried_unit(name(u), None),
                    UnitState::Enabled,
                    Some(PresetDefault::Disable),
                )
                .unwrap()
            })
            .collect(),
        inventory: units
            .iter()
            .map(|u| UnitInventoryEntry {
                unit: name(u),
                state: UnitState::Enabled,
                preset: Some(PresetDefault::Disable),
            })
            .collect(),
        drop_ins: units
            .iter()
            .map(|u| SystemdDropIn {
                unit: (*u).into(),
                path: format!("/etc/systemd/system/{u}.d/override.conf"),
                disposition: FindingKind::included(),
                ..Default::default()
            })
            .collect(),
        mount_findings: Vec::new(),
        manual_links: Vec::new(),
        undetermined_installs: Vec::new(),
        default_target: None,
        enabled_on_any_host: Vec::new(),
    });

    normalize_incompatible_services(&mut snap);
    let section = snap.units.unwrap();

    for unit in INCOMPATIBLE {
        let sc = section
            .state_changes
            .iter()
            .find(|s| s.unit().name().as_str() == unit)
            .unwrap_or_else(|| panic!("{unit} must keep its state row"));
        assert!(
            sc.locked && !sc.disposition.is_included(),
            "{unit}'s state row must be locked and excluded, got {sc:?}"
        );
        assert_eq!(
            sc.attention_reason.as_deref(),
            Some("service-image-mode-incompatible")
        );
        let di = section
            .drop_ins
            .iter()
            .find(|d| d.unit == unit)
            .unwrap_or_else(|| panic!("{unit} must keep its drop-in"));
        assert!(
            di.locked && !di.disposition.is_included(),
            "{unit}'s drop-in must be locked and excluded, got {di:?}"
        );
        assert!(
            !section.inventory.iter().any(|e| e.unit.as_str() == unit),
            "{unit}'s enabled inventory entry must be removed"
        );
    }

    // Positive controls: an ordinary unit and the runtime updater are
    // untouched everywhere.
    for unit in CONTROLS {
        let control = section
            .state_changes
            .iter()
            .find(|s| s.unit().name().as_str() == unit)
            .expect("the control unit keeps its state row");
        assert!(
            !control.locked && control.disposition.is_included(),
            "{unit}"
        );
        assert_eq!(control.attention_reason, None, "{unit}");
        let control_di = section
            .drop_ins
            .iter()
            .find(|d| d.unit == unit)
            .expect("the control unit keeps its drop-in");
        assert!(
            !control_di.locked && control_di.disposition.is_included(),
            "{unit}"
        );
        assert!(
            section.inventory.iter().any(|e| e.unit.as_str() == unit),
            "{unit} keeps its inventory entry"
        );
    }
}

/// A carried unit from the vendor directory, owned as given.
fn test_carried_unit(
    name: inspectah_core::types::units::UnitName,
    owning_package: Option<String>,
) -> inspectah_core::types::units::CarriedUnit {
    use inspectah_core::types::units::{CarriedUnit, UnitFragment, UnitSubject};
    let fragment = UnitFragment::Vendor {
        file: name.clone(),
        owning_package,
    };
    CarriedUnit::try_from(UnitSubject::new(name, fragment).expect("file is the unit"))
        .expect("a test unit is carried")
}
