use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind, ConfigSection};
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
    assert_eq!(packages[0].attention[0].reason, AttentionReason::PackageNotInBaseline);
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
