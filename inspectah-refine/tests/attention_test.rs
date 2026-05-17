use inspectah_core::baseline::{BaselineData, BaselinePackageEntry};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection, VersionChange, VersionChangeDirection};
use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind, ConfigSection};
use inspectah_core::types::redaction::{RedactionHint, RedactionState, Confidence};
use inspectah_refine::attention::compute_config_attention;
use inspectah_refine::types::{AttentionLevel, AttentionReason};

// ---------------------------------------------------------------------------
// Helper: build a snapshot with one package and optional baseline
// Phase 6: baseline data goes in top-level snap.baseline, not rpm.baseline_package_names
// ---------------------------------------------------------------------------
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
    // Phase 6: baseline data goes in top-level snap.baseline
    if let Some(names) = baseline_names {
        snap.baseline = Some(BaselineData {
            image_digest: "sha256:test".into(),
            packages: names.iter().map(|n| {
                let key = format!("{}.x86_64", n);
                (key, BaselinePackageEntry {
                    name: n.clone(),
                    epoch: Some("0".into()),
                    version: "1.0".into(),
                    release: "1.el9".into(),
                    arch: "x86_64".into(),
                })
            }).collect(),
            extracted_at: "2026-05-17T00:00:00Z".into(),
        });
    }
    snap
}

// ---------------------------------------------------------------------------
// Package classification matrix tests
// ---------------------------------------------------------------------------

#[test]
fn test_added_baseline_match_is_tier1() {
    let snap = make_snap_with_package(
        "glibc", PackageState::Added, "baseos",
        Some(vec!["glibc".into()]),
    );
    let pkgs = inspectah_refine::attention::compute_package_attention(&snap);
    assert_eq!(pkgs.len(), 1);
    assert_eq!(pkgs[0].attention[0].level, AttentionLevel::Routine);
    assert_eq!(pkgs[0].attention[0].reason, AttentionReason::PackageBaselineMatch);
}

#[test]
fn test_added_not_in_baseline_known_repo_is_routine_user_added() {
    let snap = make_snap_with_package(
        "httpd", PackageState::Added, "appstream",
        Some(vec!["glibc".into()]),
    );
    let pkgs = inspectah_refine::attention::compute_package_attention(&snap);
    assert_eq!(pkgs[0].attention[0].level, AttentionLevel::Routine);
    assert_eq!(pkgs[0].attention[0].reason, AttentionReason::PackageUserAdded);
}

#[test]
fn test_added_not_in_baseline_empty_repo_is_tier3() {
    let snap = make_snap_with_package(
        "mystery", PackageState::Added, "",
        Some(vec!["glibc".into()]),
    );
    let pkgs = inspectah_refine::attention::compute_package_attention(&snap);
    assert_eq!(pkgs[0].attention[0].level, AttentionLevel::NeedsReview);
    assert_eq!(pkgs[0].attention[0].reason, AttentionReason::PackageNoRepoSource);
}

#[test]
fn test_added_no_baseline_known_repo_is_provenance_unavailable() {
    let snap = make_snap_with_package("httpd", PackageState::Added, "appstream", None);
    let pkgs = inspectah_refine::attention::compute_package_attention(&snap);
    assert_eq!(pkgs[0].attention[0].level, AttentionLevel::Informational);
    assert_eq!(pkgs[0].attention[0].reason, AttentionReason::PackageProvenanceUnavailable);
}

#[test]
fn test_added_no_baseline_empty_repo_is_tier3() {
    let snap = make_snap_with_package("mystery", PackageState::Added, "", None);
    let pkgs = inspectah_refine::attention::compute_package_attention(&snap);
    assert_eq!(pkgs[0].attention[0].level, AttentionLevel::NeedsReview);
    assert_eq!(pkgs[0].attention[0].reason, AttentionReason::PackageNoRepoSource);
}

#[test]
fn test_modified_upgrade_in_baseline_is_routine() {
    // Upgrades are normal maintenance — Routine.
    let mut snap = make_snap_with_package(
        "glibc", PackageState::Modified, "baseos",
        Some(vec!["glibc".into()]),
    );
    snap.rpm.as_mut().unwrap().version_changes.push(VersionChange {
        name: "glibc".into(),
        direction: VersionChangeDirection::Upgrade,
        ..Default::default()
    });
    let pkgs = inspectah_refine::attention::compute_package_attention(&snap);
    assert_eq!(pkgs[0].attention[0].level, AttentionLevel::Routine);
    assert_eq!(pkgs[0].attention[0].reason, AttentionReason::PackageVersionChanged);
    assert_eq!(pkgs[0].attention[0].detail.as_deref(), Some("Upgrade"));
}

#[test]
fn test_modified_downgrade_in_baseline_is_needs_review() {
    // Downgrades are unusual — NeedsReview.
    let mut snap = make_snap_with_package(
        "glibc", PackageState::Modified, "baseos",
        Some(vec!["glibc".into()]),
    );
    snap.rpm.as_mut().unwrap().version_changes.push(VersionChange {
        name: "glibc".into(),
        direction: VersionChangeDirection::Downgrade,
        ..Default::default()
    });
    let pkgs = inspectah_refine::attention::compute_package_attention(&snap);
    assert_eq!(pkgs[0].attention[0].level, AttentionLevel::NeedsReview);
    assert_eq!(pkgs[0].attention[0].reason, AttentionReason::PackageVersionChanged);
    assert_eq!(pkgs[0].attention[0].detail.as_deref(), Some("Downgrade"));
}

#[test]
fn test_modified_no_version_change_entry_defaults_to_routine() {
    // Modified with no matching VersionChange entry — defaults to Routine.
    let snap = make_snap_with_package(
        "httpd", PackageState::Modified, "appstream",
        Some(vec!["glibc".into()]),
    );
    let pkgs = inspectah_refine::attention::compute_package_attention(&snap);
    assert_eq!(pkgs[0].attention[0].level, AttentionLevel::Routine);
    assert_eq!(pkgs[0].attention[0].reason, AttentionReason::PackageVersionChanged);
}

#[test]
fn test_local_install_always_tier3() {
    for baseline in [Some(vec!["glibc".into()]), None] {
        for repo in ["appstream", ""] {
            let snap = make_snap_with_package(
                "custom", PackageState::LocalInstall, repo, baseline.clone(),
            );
            let pkgs = inspectah_refine::attention::compute_package_attention(&snap);
            assert_eq!(pkgs[0].attention[0].level, AttentionLevel::NeedsReview,
                "LocalInstall should always be NeedsReview (repo={repo:?}, baseline={baseline:?})");
            assert_eq!(pkgs[0].attention[0].reason, AttentionReason::PackageLocalInstall,
                "LocalInstall should always be PackageLocalInstall (repo={repo:?}, baseline={baseline:?})");
        }
    }
}

#[test]
fn test_no_repo_always_tier3() {
    for baseline in [Some(vec!["glibc".into()]), None] {
        let snap = make_snap_with_package("orphan", PackageState::NoRepo, "", baseline.clone());
        let pkgs = inspectah_refine::attention::compute_package_attention(&snap);
        assert_eq!(pkgs[0].attention[0].level, AttentionLevel::NeedsReview,
            "NoRepo should always be NeedsReview (baseline={baseline:?})");
        assert_eq!(pkgs[0].attention[0].reason, AttentionReason::PackageNoRepoSource,
            "NoRepo should always be PackageNoRepoSource (baseline={baseline:?})");
    }
}

// ---------------------------------------------------------------------------
// Existing config attention tests (preserved from Task 1)
// ---------------------------------------------------------------------------

#[test]
fn config_modified_gets_needs_review() {
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/httpd/conf/httpd.conf".into(),
            kind: ConfigFileKind::RpmOwnedModified,
            include: true, ..Default::default()
        }],
    });
    let configs = inspectah_refine::attention::compute_config_attention(&snap);
    assert_eq!(configs[0].attention[0].level, AttentionLevel::NeedsReview);
    assert_eq!(configs[0].attention[0].reason, AttentionReason::ConfigModified);
}

#[test]
fn config_rpm_default_gets_routine() {
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/logrotate.conf".into(),
            kind: ConfigFileKind::RpmOwnedDefault,
            include: true, ..Default::default()
        }],
    });
    let configs = inspectah_refine::attention::compute_config_attention(&snap);
    assert_eq!(configs[0].attention[0].level, AttentionLevel::Routine);
}

#[test]
fn sensitive_path_adds_extra_tag_for_tier2() {
    // Tier 2 (Unowned) at a sensitive path -> promoted with SensitivePath tag
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/ssh/custom_config".into(),
            kind: ConfigFileKind::Unowned,
            include: true, ..Default::default()
        }],
    });
    let configs = inspectah_refine::attention::compute_config_attention(&snap);
    assert_eq!(configs[0].attention.len(), 2);
    assert!(configs[0].attention.iter().any(|t| t.reason == AttentionReason::SensitivePath));
}

#[test]
fn sensitive_path_no_extra_tag_for_tier3() {
    // Tier 3 (RpmOwnedModified) at a sensitive path -> NOT promoted (already NeedsReview)
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/ssh/sshd_config".into(),
            kind: ConfigFileKind::RpmOwnedModified,
            include: true, ..Default::default()
        }],
    });
    let configs = inspectah_refine::attention::compute_config_attention(&snap);
    assert_eq!(configs[0].attention.len(), 1);
    assert!(!configs[0].attention.iter().any(|t| t.reason == AttentionReason::SensitivePath));
}

#[test]
fn empty_snapshot_returns_empty_attention() {
    let snap = InspectionSnapshot::new();
    assert!(inspectah_refine::attention::compute_package_attention(&snap).is_empty());
    assert!(inspectah_refine::attention::compute_config_attention(&snap).is_empty());
}

#[test]
fn unresolved_hints_surface_as_needs_review() {
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

    let configs = inspectah_refine::attention::compute_config_attention(&snap);
    assert_eq!(configs.len(), 1);
    let hint_tags: Vec<_> = configs[0].attention.iter()
        .filter(|t| t.reason == AttentionReason::Custom("unresolved redaction hint".into()))
        .collect();
    assert_eq!(hint_tags.len(), 1, "unresolved hint must produce a NeedsReview tag");
    assert_eq!(hint_tags[0].level, AttentionLevel::NeedsReview);
    assert!(hint_tags[0].detail.as_ref().unwrap().contains("password"));
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

    let configs = inspectah_refine::attention::compute_config_attention(&snap);
    assert_eq!(configs.len(), 1);
    assert!(
        configs[0].attention.iter().all(|t| t.reason != AttentionReason::Custom("unresolved redaction hint".into())),
        "FullyRedacted snapshot must not produce hint attention tags"
    );
}

// ---------------------------------------------------------------------------
// Helper: build a snapshot with one config file entry
// ---------------------------------------------------------------------------
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

// ---------------------------------------------------------------------------
// Config classification matrix tests
// ---------------------------------------------------------------------------

#[test]
fn test_config_rpm_owned_default_is_tier1() {
    let snap = make_snap_with_config("/etc/httpd/conf/httpd.conf", ConfigFileKind::RpmOwnedDefault);
    let configs = compute_config_attention(&snap);
    assert_eq!(configs[0].attention[0].level, AttentionLevel::Routine);
    assert_eq!(configs[0].attention[0].reason, AttentionReason::ConfigDefault);
}

#[test]
fn test_config_baseline_match_is_tier1() {
    let snap = make_snap_with_config("/etc/sysconfig/network", ConfigFileKind::BaselineMatch);
    let configs = compute_config_attention(&snap);
    assert_eq!(configs[0].attention[0].level, AttentionLevel::Routine);
    assert_eq!(configs[0].attention[0].reason, AttentionReason::ConfigBaselineMatch);
}

#[test]
fn test_config_unowned_is_tier2() {
    let snap = make_snap_with_config("/etc/custom.conf", ConfigFileKind::Unowned);
    let configs = compute_config_attention(&snap);
    assert_eq!(configs[0].attention[0].level, AttentionLevel::Informational);
    assert_eq!(configs[0].attention[0].reason, AttentionReason::ConfigUnowned);
}

#[test]
fn test_config_rpm_owned_modified_is_tier3() {
    let snap = make_snap_with_config("/etc/ssh/sshd_config", ConfigFileKind::RpmOwnedModified);
    let configs = compute_config_attention(&snap);
    assert_eq!(configs[0].attention[0].level, AttentionLevel::NeedsReview);
    assert_eq!(configs[0].attention[0].reason, AttentionReason::ConfigModified);
}

#[test]
fn test_config_sensitive_path_promotes_tier2_only() {
    // Tier 2 (Unowned) at a sensitive path -> promoted to Tier 3
    let snap = make_snap_with_config("/etc/ssh/custom_keys", ConfigFileKind::Unowned);
    let configs = compute_config_attention(&snap);
    assert!(configs[0].attention.iter().any(|t| t.reason == AttentionReason::SensitivePath));

    // Tier 1 (RpmOwnedDefault) at a sensitive path -> NOT promoted
    let snap2 = make_snap_with_config("/etc/pki/tls/cert.pem", ConfigFileKind::RpmOwnedDefault);
    let configs2 = compute_config_attention(&snap2);
    assert!(!configs2[0].attention.iter().any(|t| t.reason == AttentionReason::SensitivePath));
}
