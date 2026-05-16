use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind, ConfigSection};
use inspectah_core::types::redaction::{RedactionHint, RedactionState, Confidence};
use inspectah_refine::types::{AttentionLevel, AttentionReason};

#[test]
fn package_added_gets_needs_review() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "httpd".into(), arch: "x86_64".into(),
            state: PackageState::Added, include: true,
            ..Default::default()
        }],
        ..Default::default()
    });
    let packages = inspectah_refine::attention::compute_package_attention(&snap);
    assert_eq!(packages.len(), 1);
    assert_eq!(packages[0].attention[0].level, AttentionLevel::NeedsReview);
    assert_eq!(packages[0].attention[0].reason, AttentionReason::PackageNoRepoSource);
}

#[test]
fn package_local_install_gets_needs_review() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "custom-tool".into(), arch: "x86_64".into(),
            state: PackageState::LocalInstall, include: true,
            ..Default::default()
        }],
        ..Default::default()
    });
    let packages = inspectah_refine::attention::compute_package_attention(&snap);
    assert_eq!(packages[0].attention[0].level, AttentionLevel::NeedsReview);
    assert_eq!(packages[0].attention[0].reason, AttentionReason::PackageLocalInstall);
}

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
fn sensitive_path_adds_extra_tag() {
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/ssh/sshd_config".into(),
            kind: ConfigFileKind::RpmOwnedModified,
            include: true, ..Default::default()
        }],
    });
    let configs = inspectah_refine::attention::compute_config_attention(&snap);
    assert_eq!(configs[0].attention.len(), 2);
    assert!(configs[0].attention.iter().any(|t| t.reason == AttentionReason::SensitivePath));
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
    // Should have the normal ConfigModified tag PLUS the hint tag
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
    // No hint tags — only the normal ConfigModified tag
    assert!(
        configs[0].attention.iter().all(|t| t.reason != AttentionReason::Custom("unresolved redaction hint".into())),
        "FullyRedacted snapshot must not produce hint attention tags"
    );
}
