use inspectah_core::baseline::{BaselineData, BaselinePackageEntry};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind, ConfigSection};
use inspectah_core::types::redaction::{Confidence, RedactionHint, RedactionState};
use inspectah_core::types::rpm::{
    PackageEntry, PackageState, RpmSection, VersionChange, VersionChangeDirection,
};
use inspectah_refine::classify::classify_configs;
use inspectah_refine::types::{Triage, TriageAnnotation, TriageBucket, TriageReason};

fn assert_bucket(tag: &inspectah_refine::types::TriageTag, expected: TriageBucket) {
    match &tag.triage {
        Triage::SingleHost(b) => assert_eq!(*b, expected),
        Triage::Aggregate(_) => panic!("expected SingleHost"),
    }
}

fn make_snap_with_package(
    name: &str,
    state: PackageState,
    source_repo: &str,
    baseline_names: Option<Vec<String>>,
) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: name.into(),
            arch: "x86_64".into(),
            state,
            source_repo: source_repo.into(),
            include: true,
            ..Default::default()
        }],
        ..Default::default()
    });
    if let Some(names) = baseline_names {
        snap.baseline = Some(BaselineData {
            image_digest: "sha256:test".into(),
            packages: names
                .iter()
                .map(|n| {
                    let key = format!("{}.x86_64", n);
                    (
                        key,
                        BaselinePackageEntry {
                            name: n.clone(),
                            epoch: Some("0".into()),
                            version: "1.0".into(),
                            release: "1.el9".into(),
                            arch: "x86_64".into(),
                        },
                    )
                })
                .collect(),
            extracted_at: "2026-05-17T00:00:00Z".into(),
        });
    }
    snap
}

#[test]
fn test_added_baseline_match_is_baseline() {
    let snap = make_snap_with_package(
        "glibc",
        PackageState::Added,
        "baseos",
        Some(vec!["glibc".into()]),
    );
    let pkgs = inspectah_refine::classify::classify_packages(&snap);
    assert_eq!(pkgs.len(), 1);
    assert_bucket(&pkgs[0].triage, TriageBucket::Baseline);
    assert_eq!(
        pkgs[0].triage.primary_reason,
        TriageReason::PackageBaselineMatch
    );
}

#[test]
fn test_added_not_in_baseline_known_repo_is_site() {
    let snap = make_snap_with_package(
        "httpd",
        PackageState::Added,
        "appstream",
        Some(vec!["glibc".into()]),
    );
    let pkgs = inspectah_refine::classify::classify_packages(&snap);
    assert_bucket(&pkgs[0].triage, TriageBucket::Site);
    assert_eq!(
        pkgs[0].triage.primary_reason,
        TriageReason::PackageUserAdded
    );
}

#[test]
fn test_added_not_in_baseline_empty_repo_is_investigate() {
    let snap = make_snap_with_package(
        "mystery",
        PackageState::Added,
        "",
        Some(vec!["glibc".into()]),
    );
    let pkgs = inspectah_refine::classify::classify_packages(&snap);
    assert_bucket(&pkgs[0].triage, TriageBucket::Investigate);
    assert_eq!(
        pkgs[0].triage.primary_reason,
        TriageReason::PackageNoRepoSource
    );
}

#[test]
fn test_added_no_baseline_known_repo_is_investigate() {
    let snap = make_snap_with_package("httpd", PackageState::Added, "appstream", None);
    let pkgs = inspectah_refine::classify::classify_packages(&snap);
    assert_bucket(&pkgs[0].triage, TriageBucket::Investigate);
    assert_eq!(
        pkgs[0].triage.primary_reason,
        TriageReason::PackageProvenanceUnavailable
    );
}

#[test]
fn test_added_no_baseline_empty_repo_is_investigate() {
    let snap = make_snap_with_package("mystery", PackageState::Added, "", None);
    let pkgs = inspectah_refine::classify::classify_packages(&snap);
    assert_bucket(&pkgs[0].triage, TriageBucket::Investigate);
    assert_eq!(
        pkgs[0].triage.primary_reason,
        TriageReason::PackageNoRepoSource
    );
}

#[test]
fn test_modified_upgrade_in_baseline_is_site() {
    let mut snap = make_snap_with_package(
        "glibc",
        PackageState::Modified,
        "baseos",
        Some(vec!["glibc".into()]),
    );
    snap.rpm
        .as_mut()
        .unwrap()
        .version_changes
        .push(VersionChange {
            name: "glibc".into(),
            arch: "x86_64".into(),
            direction: VersionChangeDirection::Upgrade,
            ..Default::default()
        });
    let pkgs = inspectah_refine::classify::classify_packages(&snap);
    assert_bucket(&pkgs[0].triage, TriageBucket::Site);
    assert_eq!(
        pkgs[0].triage.primary_reason,
        TriageReason::PackageVersionChanged
    );
}

#[test]
fn test_modified_downgrade_in_baseline_is_investigate() {
    let mut snap = make_snap_with_package(
        "glibc",
        PackageState::Modified,
        "baseos",
        Some(vec!["glibc".into()]),
    );
    snap.rpm
        .as_mut()
        .unwrap()
        .version_changes
        .push(VersionChange {
            name: "glibc".into(),
            arch: "x86_64".into(),
            direction: VersionChangeDirection::Downgrade,
            ..Default::default()
        });
    let pkgs = inspectah_refine::classify::classify_packages(&snap);
    assert_bucket(&pkgs[0].triage, TriageBucket::Investigate);
    assert_eq!(
        pkgs[0].triage.primary_reason,
        TriageReason::PackageVersionChanged
    );
}

#[test]
fn test_modified_no_version_change_entry_defaults_to_site() {
    let snap = make_snap_with_package(
        "httpd",
        PackageState::Modified,
        "appstream",
        Some(vec!["glibc".into()]),
    );
    let pkgs = inspectah_refine::classify::classify_packages(&snap);
    assert_bucket(&pkgs[0].triage, TriageBucket::Site);
    assert_eq!(
        pkgs[0].triage.primary_reason,
        TriageReason::PackageVersionChanged
    );
}

#[test]
fn test_local_install_always_investigate() {
    for baseline in [Some(vec!["glibc".into()]), None] {
        for repo in ["appstream", ""] {
            let snap = make_snap_with_package(
                "custom",
                PackageState::LocalInstall,
                repo,
                baseline.clone(),
            );
            let pkgs = inspectah_refine::classify::classify_packages(&snap);
            assert_bucket(&pkgs[0].triage, TriageBucket::Investigate);
            assert_eq!(
                pkgs[0].triage.primary_reason,
                TriageReason::PackageLocalInstall
            );
        }
    }
}

#[test]
fn test_no_repo_always_investigate() {
    for baseline in [Some(vec!["glibc".into()]), None] {
        let snap = make_snap_with_package("orphan", PackageState::NoRepo, "", baseline.clone());
        let pkgs = inspectah_refine::classify::classify_packages(&snap);
        assert_bucket(&pkgs[0].triage, TriageBucket::Investigate);
        assert_eq!(
            pkgs[0].triage.primary_reason,
            TriageReason::PackageNoRepoSource
        );
    }
}

#[test]
fn config_modified_gets_site() {
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/httpd/conf/httpd.conf".into(),
            kind: ConfigFileKind::RpmOwnedModified,
            include: true,
            ..Default::default()
        }],
    });
    let configs = classify_configs(&snap);
    assert_bucket(&configs[0].triage, TriageBucket::Site);
    assert_eq!(
        configs[0].triage.primary_reason,
        TriageReason::ConfigModified
    );
}

#[test]
fn config_rpm_default_gets_baseline() {
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/logrotate.conf".into(),
            kind: ConfigFileKind::RpmOwnedDefault,
            include: true,
            ..Default::default()
        }],
    });
    let configs = classify_configs(&snap);
    assert_bucket(&configs[0].triage, TriageBucket::Baseline);
}

#[test]
fn sensitive_path_adds_annotation() {
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/ssh/custom_config".into(),
            kind: ConfigFileKind::Unowned,
            include: true,
            ..Default::default()
        }],
    });
    let configs = classify_configs(&snap);
    assert!(
        configs[0]
            .triage
            .annotations
            .contains(&TriageAnnotation::SensitivePath)
    );
}

#[test]
fn sensitive_path_annotation_on_modified_too() {
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/ssh/sshd_config".into(),
            kind: ConfigFileKind::RpmOwnedModified,
            include: true,
            ..Default::default()
        }],
    });
    let configs = classify_configs(&snap);
    assert!(
        configs[0]
            .triage
            .annotations
            .contains(&TriageAnnotation::SensitivePath)
    );
}

#[test]
fn empty_snapshot_returns_empty() {
    let snap = InspectionSnapshot::new();
    assert!(inspectah_refine::classify::classify_packages(&snap).is_empty());
    assert!(inspectah_refine::classify::classify_configs(&snap).is_empty());
}

#[test]
fn unresolved_hints_surface_as_investigate() {
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/myapp/config".into(),
            kind: ConfigFileKind::RpmOwnedModified,
            include: true,
            ..Default::default()
        }],
    });
    snap.redaction_state = Some(RedactionState::PartiallyRedacted {
        redacted_by: "inspectah 0.8.0".into(),
        config_hash: "abc".into(),
        unresolved_count: 1,
        unresolved_hints: vec![RedactionHint {
            path: "/etc/myapp/config".into(),
            reason: "file content may contain credentials (matched 'password')".into(),
            confidence: Some(Confidence::Medium),
        }],
    });
    let configs = classify_configs(&snap);
    assert_eq!(configs.len(), 1);
    assert_bucket(&configs[0].triage, TriageBucket::Investigate);
    assert!(matches!(
        configs[0].triage.primary_reason,
        TriageReason::Custom(_)
    ));
}

#[test]
fn fully_redacted_snapshot_no_hint_tags() {
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/myapp/config".into(),
            kind: ConfigFileKind::RpmOwnedModified,
            include: true,
            ..Default::default()
        }],
    });
    snap.redaction_state = Some(RedactionState::FullyRedacted {
        redacted_by: "inspectah 0.8.0".into(),
        config_hash: "abc".into(),
    });
    let configs = classify_configs(&snap);
    assert_eq!(configs.len(), 1);
    // FullyRedacted should not produce Investigate override
    assert_bucket(&configs[0].triage, TriageBucket::Site);
}

fn make_snap_with_config(path: &str, kind: ConfigFileKind) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: path.into(),
            kind,
            include: true,
            ..Default::default()
        }],
    });
    snap
}

#[test]
fn test_config_rpm_owned_default_is_baseline() {
    let snap = make_snap_with_config(
        "/etc/httpd/conf/httpd.conf",
        ConfigFileKind::RpmOwnedDefault,
    );
    let configs = classify_configs(&snap);
    assert_bucket(&configs[0].triage, TriageBucket::Baseline);
    assert_eq!(
        configs[0].triage.primary_reason,
        TriageReason::ConfigDefault
    );
}

#[test]
fn test_config_baseline_match_is_baseline() {
    let snap = make_snap_with_config("/etc/sysconfig/network", ConfigFileKind::BaselineMatch);
    let configs = classify_configs(&snap);
    assert_bucket(&configs[0].triage, TriageBucket::Baseline);
    assert_eq!(
        configs[0].triage.primary_reason,
        TriageReason::ConfigBaselineMatch
    );
}

#[test]
fn test_config_unowned_is_site() {
    let snap = make_snap_with_config("/etc/custom.conf", ConfigFileKind::Unowned);
    let configs = classify_configs(&snap);
    assert_bucket(&configs[0].triage, TriageBucket::Site);
    assert_eq!(
        configs[0].triage.primary_reason,
        TriageReason::ConfigUnowned
    );
}

#[test]
fn test_config_rpm_owned_modified_is_site() {
    let snap = make_snap_with_config("/etc/ssh/sshd_config", ConfigFileKind::RpmOwnedModified);
    let configs = classify_configs(&snap);
    assert_bucket(&configs[0].triage, TriageBucket::Site);
    assert_eq!(
        configs[0].triage.primary_reason,
        TriageReason::ConfigModified
    );
}

#[test]
fn test_config_sensitive_path_annotation() {
    // Unowned at a sensitive path -> gets SensitivePath annotation
    let snap = make_snap_with_config("/etc/ssh/custom_keys", ConfigFileKind::Unowned);
    let configs = classify_configs(&snap);
    assert!(
        configs[0]
            .triage
            .annotations
            .contains(&TriageAnnotation::SensitivePath)
    );

    // RpmOwnedDefault at a sensitive path that IS an os-default -> no annotation
    let snap2 = make_snap_with_config(
        "/etc/pki/ca-trust/cert.pem",
        ConfigFileKind::RpmOwnedDefault,
    );
    let configs2 = classify_configs(&snap2);
    assert!(
        !configs2[0]
            .triage
            .annotations
            .contains(&TriageAnnotation::SensitivePath)
    );
}
