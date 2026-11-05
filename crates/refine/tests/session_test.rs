use inspectah_core::types::FindingKind;
mod helpers;

use inspectah_core::baseline::BaselineData;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::aggregate::{AggregatePrevalence, AggregateSnapshotMeta};
use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind, ConfigSection};
use inspectah_core::types::group_render::GroupRenderState;
use inspectah_core::types::rpm::{
    InstalledGroup, PackageEntry, PackageState, RepoFile, RpmSection,
};
use inspectah_refine::group_state;
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::{ItemId, RefineError, RefinementOp};

use helpers::*;

fn test_snapshot() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.baseline = Some(empty_baseline());
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
                name: "glibc".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "baseos".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
            PackageEntry {
                name: "glibc".into(),
                arch: "i686".into(),
                state: PackageState::Added,
                source_repo: "baseos".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
        ],
        ..Default::default()
    });
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/httpd/conf/httpd.conf".into(),
            kind: ConfigFileKind::RpmOwnedModified,
            disposition: FindingKind::included(),
            ..Default::default()
        }],
    });
    snap
}

fn empty_baseline() -> BaselineData {
    BaselineData {
        image_digest: "sha256:test".into(),
        packages: std::collections::HashMap::new(),
        extracted_at: "2026-05-17T00:00:00Z".into(),
    }
}

fn aggregate(count: i32, total: i32, hosts: &[&str]) -> AggregatePrevalence {
    AggregatePrevalence {
        count,
        total,
        hosts: hosts.iter().map(|host| host.to_string()).collect(),
        ..Default::default()
    }
}

#[test]
fn new_session_has_generation_zero() {
    let session = RefineSession::new(test_snapshot());
    assert_eq!(session.view().generation, 0);
}

#[test]
fn new_session_is_not_dirty() {
    let session = RefineSession::new(test_snapshot());
    assert!(!session.is_dirty());
}

#[test]
fn new_session_has_correct_stats() {
    let session = RefineSession::new(test_snapshot());
    let view = session.view();
    assert_eq!(view.stats.total_packages(), 3);
    assert_eq!(view.stats.included_packages(), 3);
    assert_eq!(view.stats.excluded_packages(), 0);
    assert_eq!(view.stats.total_configs(), 1);
    assert_eq!(view.stats.included_configs(), 1);
    assert_eq!(view.stats.excluded_configs(), 0);
}

#[test]
fn apply_exclude_package() {
    let mut session = RefineSession::new(test_snapshot());
    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Package {
                name: "httpd".into(),
                arch: "x86_64".into(),
            },
            include: false,
        })
        .unwrap();

    assert_eq!(session.view().generation, 1);
    assert_eq!(session.view().stats.excluded_packages(), 1);
    assert_eq!(session.view().stats.included_packages(), 2);
    assert!(session.is_dirty());
}

#[test]
fn apply_unknown_target_returns_error() {
    let mut session = RefineSession::new(test_snapshot());
    let result = session.apply(RefinementOp::SetInclude {
        item_id: ItemId::Package {
            name: "nonexistent".into(),
            arch: "x86_64".into(),
        },
        include: false,
    });
    assert!(matches!(result, Err(RefineError::UnknownTarget(_))));
}

#[test]
fn apply_wrong_arch_returns_error() {
    let mut session = RefineSession::new(test_snapshot());
    let result = session.apply(RefinementOp::SetInclude {
        item_id: ItemId::Package {
            name: "glibc".into(),
            arch: "s390x".into(),
        },
        include: false,
    });
    assert!(matches!(result, Err(RefineError::UnknownTarget(_))));
}

#[test]
fn idempotent_exclude_is_noop() {
    let mut session = RefineSession::new(test_snapshot());
    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Package {
                name: "httpd".into(),
                arch: "x86_64".into(),
            },
            include: false,
        })
        .unwrap();
    let gen_after_first = session.view().generation;

    // Second exclude of the same target should be a no-op
    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Package {
                name: "httpd".into(),
                arch: "x86_64".into(),
            },
            include: false,
        })
        .unwrap();

    assert_eq!(session.view().generation, gen_after_first);
    assert_eq!(session.ops_history().len(), 1);
}

#[test]
fn undo_reverts_to_original() {
    let mut session = RefineSession::new(test_snapshot());
    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Package {
                name: "httpd".into(),
                arch: "x86_64".into(),
            },
            include: false,
        })
        .unwrap();
    session.undo().unwrap();

    assert!(!session.is_dirty());
    assert_eq!(session.view().stats.excluded_packages(), 0);
    assert_eq!(session.view().generation, 2); // apply=1, undo=2
}

#[test]
fn undo_on_empty_returns_error() {
    let mut session = RefineSession::new(test_snapshot());
    assert!(matches!(session.undo(), Err(RefineError::NothingToUndo)));
}

#[test]
fn redo_after_undo() {
    let mut session = RefineSession::new(test_snapshot());
    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Package {
                name: "httpd".into(),
                arch: "x86_64".into(),
            },
            include: false,
        })
        .unwrap();
    session.undo().unwrap();
    session.redo().unwrap();

    assert!(session.is_dirty());
    assert_eq!(session.view().stats.excluded_packages(), 1);
    assert_eq!(session.view().generation, 3);
}

#[test]
fn redo_with_nothing_undone_returns_error() {
    let mut session = RefineSession::new(test_snapshot());
    assert!(matches!(session.redo(), Err(RefineError::NothingToRedo)));
}

#[test]
fn apply_after_undo_truncates_redo_history() {
    let mut session = RefineSession::new(test_snapshot());
    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Package {
                name: "httpd".into(),
                arch: "x86_64".into(),
            },
            include: false,
        })
        .unwrap();
    session.undo().unwrap();

    // Apply a different op -- should truncate the undone op
    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Config {
                path: "/etc/httpd/conf/httpd.conf".into(),
            },
            include: false,
        })
        .unwrap();

    assert!(matches!(session.redo(), Err(RefineError::NothingToRedo)));
    assert_eq!(session.ops_history().len(), 1);
}

#[test]
fn multiarch_targeting() {
    let mut session = RefineSession::new(test_snapshot());
    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Package {
                name: "glibc".into(),
                arch: "i686".into(),
            },
            include: false,
        })
        .unwrap();

    let view = session.view();
    let glibc_x86 = view
        .packages
        .iter()
        .find(|p| p.entry.name == "glibc" && p.entry.arch == "x86_64")
        .unwrap();
    let glibc_i686 = view
        .packages
        .iter()
        .find(|p| p.entry.name == "glibc" && p.entry.arch == "i686")
        .unwrap();

    assert!(glibc_x86.entry.disposition.is_included());
    assert!(!glibc_i686.entry.disposition.is_included());
}

#[test]
fn exclude_config_file() {
    let mut session = RefineSession::new(test_snapshot());
    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Config {
                path: "/etc/httpd/conf/httpd.conf".into(),
            },
            include: false,
        })
        .unwrap();

    let view = session.view();
    assert_eq!(view.stats.excluded_configs(), 1);
    assert!(session.is_dirty());
}

#[test]
fn pending_changes_tracks_excludes() {
    let mut session = RefineSession::new(test_snapshot());
    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Package {
                name: "httpd".into(),
                arch: "x86_64".into(),
            },
            include: false,
        })
        .unwrap();

    let changes = session.pending_changes();
    let pkg_section = changes
        .section(inspectah_refine::types::SectionKind::Package)
        .expect("package section should exist");
    assert_eq!(pkg_section.excluded.len(), 1);
    match &pkg_section.excluded[0] {
        inspectah_refine::types::ItemId::Package { name, .. } => assert_eq!(name, "httpd"),
        other => panic!("expected Package ItemId, got {:?}", other),
    }
    assert!(changes.is_dirty);
}

#[test]
fn exclude_then_include_returns_to_clean() {
    let mut session = RefineSession::new(test_snapshot());
    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Package {
                name: "httpd".into(),
                arch: "x86_64".into(),
            },
            include: false,
        })
        .unwrap();
    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Package {
                name: "httpd".into(),
                arch: "x86_64".into(),
            },
            include: true,
        })
        .unwrap();

    // State-based dirty: not dirty because state matches original
    assert!(!session.is_dirty());
}

#[test]
fn undo_all_then_redo_all() {
    let mut session = RefineSession::new(test_snapshot());
    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Package {
                name: "httpd".into(),
                arch: "x86_64".into(),
            },
            include: false,
        })
        .unwrap();
    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Config {
                path: "/etc/httpd/conf/httpd.conf".into(),
            },
            include: false,
        })
        .unwrap();

    let view_after_ops = session.view().clone();

    session.undo().unwrap();
    session.undo().unwrap();
    assert!(!session.is_dirty());

    session.redo().unwrap();
    session.redo().unwrap();

    // Stats should match the fully-applied state
    assert_eq!(
        session.view().stats.excluded_packages(),
        view_after_ops.stats.excluded_packages()
    );
    assert_eq!(
        session.view().stats.excluded_configs(),
        view_after_ops.stats.excluded_configs()
    );
}

#[test]
fn stale_generation_export_rejected() {
    let session = RefineSession::new(test_snapshot());
    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("output.tar.gz");

    // The generation check lives in `export_plan`, so a stale caller is
    // refused before anything is rendered rather than after.
    let result = session.export_plan(999).map(|_| ());
    assert!(matches!(
        result,
        Err(RefineError::StaleGeneration {
            expected: 999,
            actual: 0
        })
    ));
    assert!(
        !tarball_path.exists(),
        "a refused export must not have written a tarball"
    );
}

// -- Viewed ID validation tests ---

#[test]
fn mark_viewed_accepts_valid_packages_id() {
    let mut session = RefineSession::new(test_snapshot());
    assert!(session.mark_viewed("packages:httpd.x86_64").is_ok());
    assert!(session.is_viewed("packages:httpd.x86_64"));
}

#[test]
fn mark_viewed_accepts_valid_configs_id() {
    let mut session = RefineSession::new(test_snapshot());
    assert!(
        session
            .mark_viewed("configs:/etc/httpd/conf/httpd.conf")
            .is_ok()
    );
    assert!(session.is_viewed("configs:/etc/httpd/conf/httpd.conf"));
}

#[test]
fn mark_viewed_accepts_all_valid_sections() {
    let mut session = RefineSession::new(test_snapshot());
    let sections = [
        "packages",
        "configs",
        "services",
        "containers",
        "users_groups",
        "network",
        "storage",
        "scheduled_tasks",
        "non_rpm_software",
        "kernel_boot",
        "selinux",
    ];
    for section in sections {
        let id = format!("{section}:test_item");
        assert!(
            session.mark_viewed(&id).is_ok(),
            "section '{section}' should be valid"
        );
    }
}

#[test]
fn mark_viewed_rejects_missing_colon() {
    let mut session = RefineSession::new(test_snapshot());
    let result = session.mark_viewed("packages-httpd.x86_64");
    assert!(matches!(result, Err(RefineError::BadRequest(_))));
}

#[test]
fn mark_viewed_rejects_empty_item_id() {
    let mut session = RefineSession::new(test_snapshot());
    let result = session.mark_viewed("packages:");
    assert!(matches!(result, Err(RefineError::BadRequest(_))));
}

#[test]
fn mark_viewed_rejects_invalid_section() {
    let mut session = RefineSession::new(test_snapshot());
    let result = session.mark_viewed("pkg:httpd.x86_64");
    assert!(matches!(result, Err(RefineError::BadRequest(_))));
}

#[test]
fn mark_viewed_rejects_cfg_prefix() {
    let mut session = RefineSession::new(test_snapshot());
    let result = session.mark_viewed("cfg:/etc/httpd/conf/httpd.conf");
    assert!(matches!(result, Err(RefineError::BadRequest(_))));
}

// -- Non-leaf Tier 2 view filtering tests --

#[test]
fn test_non_leaf_tier2_excluded_from_view() {
    // Empty baseline (no packages in it) makes all packages Site (user-added).
    // Non-leaf Site packages are hidden from the view because dnf resolves
    // them automatically.
    let mut snap = InspectionSnapshot::new();
    snap.baseline = Some(empty_baseline());
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
    let session = RefineSession::new(snap);
    let view = session.view();
    assert!(
        view.packages.iter().any(|p| p.entry.name == "httpd"),
        "leaf package must appear in view"
    );
    assert!(
        !view.packages.iter().any(|p| p.entry.name == "apr"),
        "non-leaf Tier 2 package must be filtered from view"
    );
}

#[test]
fn test_non_leaf_needs_review_stays_visible_and_counted_with_leaf_data() {
    let mut snap = InspectionSnapshot::new();
    snap.baseline = Some(empty_baseline());
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
                name: "mystery".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: String::new(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
        ],
        leaf_packages: Some(vec!["httpd.x86_64".into()]),
        auto_packages: Some(vec!["mystery.x86_64".into()]),
        ..Default::default()
    });

    let session = RefineSession::new(snap);
    let view = session.view();
    let mystery = view
        .packages
        .iter()
        .find(|pkg| pkg.entry.name == "mystery" && pkg.entry.arch == "x86_64")
        .expect("needs-review package must stay visible");

    assert!(
        !mystery.entry.disposition.is_included(),
        "needs-review package stays excluded by default"
    );
    assert_eq!(view.stats.total_packages(), 2);
    assert_eq!(view.stats.included_packages(), 1);
    assert_eq!(view.stats.excluded_packages(), 1);
    assert_eq!(view.stats.needs_review_count, 1);
}

#[test]
fn test_user_included_non_leaf_package_stays_visible_under_leaf_filter() {
    let mut snap = InspectionSnapshot::new();
    snap.baseline = Some(empty_baseline());
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
        auto_packages: Some(vec!["apr.x86_64".into()]),
        ..Default::default()
    });

    let mut session = RefineSession::new(snap);
    assert!(
        !session
            .view()
            .packages
            .iter()
            .any(|pkg| pkg.entry.name == "apr"),
        "non-leaf package starts hidden"
    );

    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Package {
                name: "apr".into(),
                arch: "x86_64".into(),
            },
            include: true,
        })
        .unwrap();

    let apr = session
        .view()
        .packages
        .iter()
        .find(|pkg| pkg.entry.name == "apr" && pkg.entry.arch == "x86_64")
        .expect("manually included non-leaf package must stay visible");
    assert!(apr.entry.disposition.is_included());
}

#[test]
fn test_leaf_data_unavailable_shows_all_packages_in_view() {
    let mut snap = InspectionSnapshot::new();
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
                name: "glibc".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "baseos".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
        ],
        leaf_packages: None,
        auto_packages: None,
        ..Default::default()
    });

    let session = RefineSession::new(snap);
    assert_eq!(session.view().packages.len(), 2);
    assert_eq!(session.view().stats.total_packages(), 2);
}

#[test]
fn test_multiarch_leaf_truth_does_not_leak_across_arches_in_view_stats() {
    let mut snap = InspectionSnapshot::new();
    snap.baseline = Some(empty_baseline());
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "glibc".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "baseos".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
            PackageEntry {
                name: "glibc".into(),
                arch: "i686".into(),
                state: PackageState::Added,
                source_repo: "baseos".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
        ],
        leaf_packages: Some(vec!["glibc.x86_64".into()]),
        auto_packages: Some(vec!["glibc.i686".into()]),
        ..Default::default()
    });

    let session = RefineSession::new(snap);
    let view = session.view();

    assert_eq!(
        view.packages.len(),
        1,
        "only the matching arch should remain visible"
    );
    assert_eq!(view.packages[0].entry.arch, "x86_64");
    assert_eq!(view.stats.total_packages(), 1);
    assert_eq!(view.stats.included_packages(), 1);
    assert_eq!(view.stats.excluded_packages(), 0);
}

#[test]
fn test_aggregate_snapshot_skips_leaf_only_filter() {
    let mut snap = InspectionSnapshot::new();
    snap.baseline = Some(empty_baseline());
    snap.aggregate_meta = Some(AggregateSnapshotMeta {
        label: "test-aggregate".into(),
        host_count: 5,
        hostnames: vec![
            "host-a".into(),
            "host-b".into(),
            "host-c".into(),
            "host-d".into(),
            "host-e".into(),
        ],
        merged_at: "2026-01-01T00:00:00Z".into(),
        baseline_provisional: false,
        section_host_counts: Default::default(),
    });
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "appstream".into(),
                disposition: FindingKind::included(),
                aggregate: Some(aggregate(3, 5, &["host-a", "host-b", "host-c"])),
                ..Default::default()
            },
            PackageEntry {
                name: "apr".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "appstream".into(),
                disposition: FindingKind::included(),
                aggregate: Some(aggregate(2, 5, &["host-d", "host-e"])),
                ..Default::default()
            },
        ],
        leaf_packages: Some(vec!["httpd.x86_64".into()]),
        auto_packages: Some(vec!["apr.x86_64".into()]),
        ..Default::default()
    });

    let session = RefineSession::new(snap);
    let view = session.view();

    assert_eq!(
        view.packages.len(),
        2,
        "aggregate snapshots should keep all packages visible"
    );
    assert!(view.packages.iter().any(|pkg| pkg.entry.name == "httpd"));
    assert!(view.packages.iter().any(|pkg| pkg.entry.name == "apr"));
}

// The aggregate Containerfile-preview test that lived here was deleted on
// 2026-11-04. It had been #[ignore]d as a pre-existing failure with no expiry,
// so it pinned nothing while keeping the suite green. The defect it covered is
// real and is tracked in the backlog as
// inspectah-aggregate-preview-has-no-dnf-install-block, which records what the
// test asserted; write the test again with the fix.

// -- Normalization at construction tests --

#[test]
fn test_session_normalizes_at_construction() {
    let mut snap = InspectionSnapshot::new();
    // Put glibc.x86_64 in the baseline so it classifies as Tier 1 (Baseline).
    let mut baseline_pkgs = std::collections::HashMap::new();
    baseline_pkgs.insert(
        "glibc.x86_64".into(),
        inspectah_core::baseline::BaselinePackageEntry {
            name: "glibc".into(),
            version: "2.34".into(),
            release: "1.el9".into(),
            arch: "x86_64".into(),
            epoch: None,
        },
    );
    snap.baseline = Some(BaselineData {
        image_digest: "sha256:test".into(),
        packages: baseline_pkgs,
        extracted_at: "2026-01-01T00:00:00Z".into(),
    });
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "glibc".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            source_repo: "baseos".into(),
            disposition: FindingKind::excluded(),
            ..Default::default()
        }],
        baseline_package_names: Some(vec!["glibc".into()]),
        ..Default::default()
    });
    let session = RefineSession::new(snap);
    let view = session.view();
    assert!(
        view.packages[0].entry.disposition.is_included(),
        "Tier 1 should be auto-included after normalization"
    );
    assert!(
        session.snapshot().rpm.as_ref().unwrap().packages_added[0]
            .disposition
            .is_included(),
        "Original snapshot must reflect normalized state"
    );
}

#[test]
fn test_session_preview_export_parity() {
    let mut snap = InspectionSnapshot::new();
    snap.baseline = Some(empty_baseline());
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "httpd".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            source_repo: "appstream".into(),
            disposition: FindingKind::excluded(),
            ..Default::default()
        }],
        baseline_package_names: Some(vec![]),
        ..Default::default()
    });
    let session = RefineSession::new(snap);
    assert!(
        session.view().packages[0].entry.disposition.is_included(),
        "View should show Tier 2 as included"
    );
    assert!(
        session
            .snapshot_projected()
            .rpm
            .as_ref()
            .unwrap()
            .packages_added[0]
            .disposition
            .is_included(),
        "Projected snapshot must agree with view"
    );
    assert!(
        session.view().containerfile_preview.contains("httpd"),
        "Preview must render included package"
    );
}

#[test]
fn test_session_baseline_available_in_stats() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        baseline_package_names: Some(vec!["glibc".into()]),
        ..Default::default()
    });
    let session = RefineSession::new(snap);
    assert!(session.view().stats.baseline_available);

    let snap_no_baseline = InspectionSnapshot::new();
    let session2 = RefineSession::new(snap_no_baseline);
    assert!(!session2.view().stats.baseline_available);
}

#[test]
fn test_tier1_configs_not_in_containerfile() {
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        files: vec![
            ConfigFileEntry {
                path: "/etc/default.conf".into(),
                kind: ConfigFileKind::RpmOwnedDefault,
                disposition: FindingKind::included(),
                ..Default::default()
            },
            ConfigFileEntry {
                path: "/etc/custom.conf".into(),
                kind: ConfigFileKind::Unowned,
                disposition: FindingKind::included(),
                content: "custom content".into(),
                ..Default::default()
            },
        ],
    });
    let session = RefineSession::new(snap);
    let preview = &session.view().containerfile_preview;
    assert!(
        !preview.contains("default.conf"),
        "Tier 1 config must not appear in Containerfile"
    );
}

// -- Repo cascade tests (Task 7) --

#[test]
fn test_exclude_repo_cascades_packages_in_view() {
    let snap = make_snap_with_repos();
    let mut session = RefineSession::new(snap);
    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Repo {
                path: "epel".into(),
            },
            include: false,
        })
        .unwrap();
    let epel_pkg = session
        .view()
        .packages
        .iter()
        .find(|p| p.entry.name == "epel-release")
        .unwrap();
    assert!(
        !epel_pkg.entry.disposition.is_included(),
        "epel package must be excluded in view"
    );
}

#[test]
fn test_exclude_repo_cascades_in_projected_snapshot() {
    let snap = make_snap_with_repos();
    let mut session = RefineSession::new(snap);
    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Repo {
                path: "epel".into(),
            },
            include: false,
        })
        .unwrap();
    let projected = session.snapshot_projected();
    let epel_pkg = projected
        .rpm
        .as_ref()
        .unwrap()
        .packages_added
        .iter()
        .find(|p| p.name == "epel-release")
        .unwrap();
    assert!(
        !epel_pkg.disposition.is_included(),
        "epel package must be excluded in projected snapshot"
    );
    let orig_pkg = session
        .snapshot()
        .rpm
        .as_ref()
        .unwrap()
        .packages_added
        .iter()
        .find(|p| p.name == "epel-release")
        .unwrap();
    assert!(
        orig_pkg.disposition.is_included(),
        "original snapshot must be unchanged"
    );
}

#[test]
fn test_exclude_repo_rejects_distro_repo() {
    let snap = make_snap_with_repos();
    let mut session = RefineSession::new(snap);
    let result = session.apply(RefinementOp::SetInclude {
        item_id: ItemId::Repo {
            path: "baseos".into(),
        },
        include: false,
    });
    assert!(result.is_err());
}

#[test]
fn test_exclude_repo_rejects_incomplete_provenance() {
    let mut snap = make_snap_with_repos();
    snap.rpm
        .as_mut()
        .unwrap()
        .packages_added
        .push(PackageEntry {
            name: "custom".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            source_repo: "no-repo-file".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        });
    let mut session = RefineSession::new(snap);
    let result = session.apply(RefinementOp::SetInclude {
        item_id: ItemId::Repo {
            path: "no-repo-file".into(),
        },
        include: false,
    });
    assert!(result.is_err());
}

#[test]
fn test_exclude_repo_is_dirty_with_repo_tracking() {
    let snap = make_snap_with_repos();
    let mut session = RefineSession::new(snap);
    assert!(!session.is_dirty());
    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Repo {
                path: "epel".into(),
            },
            include: false,
        })
        .unwrap();
    assert!(session.is_dirty());
    let changes = session.pending_changes();
    assert!(changes.repos_excluded().contains(&"epel".to_string()));
}

#[test]
fn test_shared_repo_file_retained_until_last_section() {
    let snap = make_snap_with_multi_section_third_party();
    let mut session = RefineSession::new(snap);

    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Repo {
                path: "custom-a".into(),
            },
            include: false,
        })
        .unwrap();
    let projected = session.snapshot_projected();
    let repo_file = projected
        .rpm
        .as_ref()
        .unwrap()
        .repo_files
        .iter()
        .find(|rf| rf.path.contains("custom-multi"))
        .unwrap();
    assert!(
        repo_file.disposition.is_included(),
        "shared repo file must stay while custom-b is enabled"
    );
    let gpg = projected
        .rpm
        .as_ref()
        .unwrap()
        .gpg_keys
        .iter()
        .find(|k| k.path.contains("RPM-GPG-KEY-custom"))
        .unwrap();
    assert!(
        gpg.disposition.is_included(),
        "shared GPG key must stay while custom-b is enabled"
    );

    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Repo {
                path: "custom-b".into(),
            },
            include: false,
        })
        .unwrap();
    let projected2 = session.snapshot_projected();
    let gpg2 = projected2
        .rpm
        .as_ref()
        .unwrap()
        .gpg_keys
        .iter()
        .find(|k| k.path.contains("RPM-GPG-KEY-custom"))
        .unwrap();
    assert!(
        !gpg2.disposition.is_included(),
        "GPG key excluded once all sections excluded"
    );
}

#[test]
fn test_exclude_repo_then_per_package_then_include_repo() {
    let snap = make_snap_with_repos();
    let mut session = RefineSession::new(snap);
    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Repo {
                path: "epel".into(),
            },
            include: false,
        })
        .unwrap();
    assert!(
        !session
            .view()
            .packages
            .iter()
            .find(|p| p.entry.name == "epel-release")
            .unwrap()
            .entry
            .disposition
            .is_included()
    );

    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Package {
                name: "epel-release".into(),
                arch: "noarch".into(),
            },
            include: true,
        })
        .unwrap();
    assert!(
        session
            .view()
            .packages
            .iter()
            .find(|p| p.entry.name == "epel-release")
            .unwrap()
            .entry
            .disposition
            .is_included()
    );

    // "epel" has exactly one effective member (epel-release), now
    // individually included -- a repo-level include is a genuine no-op
    // under the § 3.2 member-based rule (position 29), even though the
    // repo's own exclusion history still says excluded. Before Task 7 this
    // op read only that history, accepted the op, and recorded a third
    // timeline entry; the member-based rule refuses it instead, which is
    // exactly the bug position 29 closes (a history-reading control posting
    // a direction the members already hold).
    let before = session.timeline_len();
    assert!(matches!(
        session.apply(RefinementOp::SetInclude {
            item_id: ItemId::Repo {
                path: "epel".into(),
            },
            include: true,
        }),
        Err(RefineError::NoopTarget(_))
    ));
    assert_eq!(
        session.timeline_len(),
        before,
        "a refused op must not record"
    );
    assert!(
        session
            .view()
            .packages
            .iter()
            .find(|p| p.entry.name == "epel-release")
            .unwrap()
            .entry
            .disposition
            .is_included(),
        "the refused op changed nothing"
    );

    session.undo().unwrap();
    assert!(
        !session
            .view()
            .packages
            .iter()
            .find(|p| p.entry.name == "epel-release")
            .unwrap()
            .entry
            .disposition
            .is_included(),
        "undo reverts the per-package include, the only op that actually ran"
    );
}

#[test]
fn test_exclude_repo_undo_redo() {
    let snap = make_snap_with_repos();
    let mut session = RefineSession::new(snap);
    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Repo {
                path: "epel".into(),
            },
            include: false,
        })
        .unwrap();
    assert!(
        !session
            .view()
            .packages
            .iter()
            .find(|p| p.entry.name == "epel-release")
            .unwrap()
            .entry
            .disposition
            .is_included()
    );
    session.undo().unwrap();
    assert!(
        session
            .view()
            .packages
            .iter()
            .find(|p| p.entry.name == "epel-release")
            .unwrap()
            .entry
            .disposition
            .is_included()
    );
    session.redo().unwrap();
    assert!(
        !session
            .view()
            .packages
            .iter()
            .find(|p| p.entry.name == "epel-release")
            .unwrap()
            .entry
            .disposition
            .is_included()
    );
}

#[test]
fn test_exclude_repo_case_insensitive() {
    // Build a snapshot where packages have source_repo in mixed case ("Epel-Testing")
    // but SetInclude(exclude repo) uses lowercase ("epel-testing"). The exclude must still match.
    let mut snap = InspectionSnapshot::new();
    // A baseline (even empty) is required so httpd normalizes to the Site
    // bucket, defaulting to included (no `leaf_packages` means every Site
    // package defaults to leaf), rather than Investigate -> excluded with no
    // baseline at all -- otherwise the exclude op below would already be a
    // no-op under the § 3.2 member-based rule and never exercise the
    // case-insensitive match this test is for.
    snap.baseline = Some(empty_baseline());
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "httpd".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            source_repo: "Epel-Testing".into(), // Mixed case
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        repo_files: vec![RepoFile {
            path: "/etc/yum.repos.d/epel-testing.repo".into(),
            content: "[epel-testing]\nname=EPEL Testing\n".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        baseline_package_names: Some(vec![]),
        ..Default::default()
    });
    let mut session = RefineSession::new(snap);
    // SetInclude(exclude repo) with lowercase section_id (as the UI sends after RepoIndex lowercasing)
    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Repo {
                path: "epel-testing".into(),
            },
            include: false,
        })
        .unwrap();
    let projected = session.snapshot_projected();
    let httpd = projected
        .rpm
        .as_ref()
        .unwrap()
        .packages_added
        .iter()
        .find(|p| p.name == "httpd")
        .unwrap();
    assert!(
        !httpd.disposition.is_included(),
        "SetInclude(exclude repo) must match case-insensitively"
    );
}

#[test]
fn test_include_repo_case_insensitive() {
    // Verify SetInclude(include repo) also works case-insensitively after an SetInclude(exclude repo)
    let mut snap = InspectionSnapshot::new();
    // See the baseline comment in `test_exclude_repo_case_insensitive`: without
    // it, nginx normalizes to excluded at construction and the exclude op
    // below is already a no-op under the § 3.2 member-based rule.
    snap.baseline = Some(empty_baseline());
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "nginx".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            source_repo: "Epel-Testing".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        repo_files: vec![RepoFile {
            path: "/etc/yum.repos.d/epel-testing.repo".into(),
            content: "[epel-testing]\nname=EPEL Testing\ngpgcheck=1\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL\n".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        baseline_package_names: Some(vec![]),
        ..Default::default()
    });
    let mut session = RefineSession::new(snap);
    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Repo {
                path: "epel-testing".into(),
            },
            include: false,
        })
        .unwrap();
    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Repo {
                path: "epel-testing".into(),
            },
            include: true,
        })
        .unwrap();
    let projected = session.snapshot_projected();
    let nginx = projected
        .rpm
        .as_ref()
        .unwrap()
        .packages_added
        .iter()
        .find(|p| p.name == "nginx")
        .unwrap();
    assert!(
        nginx.disposition.is_included(),
        "SetInclude(include repo) must match case-insensitively after SetInclude(exclude repo)"
    );
}

// -- § 3.7: group NoopTarget and Inert render state --------------------------

/// Mirrors the § 3.7 host example: "Container Management" has three declared
/// members. `buildah` and `containernetworking-plugins` are present and
/// non-leaf, so an empty baseline (all packages Site-bucket) plus an empty
/// `leaf_packages` list normalizes both to excluded at construction. `podman`
/// is declared but absent from `packages_added` (baseline-suppressed), so it
/// never enters the effective set.
fn container_management_session() -> RefineSession {
    let mut snap = InspectionSnapshot::new();
    snap.baseline = Some(empty_baseline());
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "buildah".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "appstream".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
            PackageEntry {
                name: "containernetworking-plugins".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "appstream".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
            // Leaf, non-member package: survives normalization as included,
            // so the Containerfile always has a non-empty `RUN dnf install`
            // block to assert `active_install_list` against — otherwise the
            // "buildah must not be in the active install list" checks below
            // would hold vacuously against an empty parse.
            PackageEntry {
                name: "vim".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "appstream".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
        ],
        leaf_packages: Some(vec!["vim.x86_64".into()]),
        auto_packages: Some(vec![
            "buildah.x86_64".into(),
            "containernetworking-plugins.x86_64".into(),
        ]),
        baseline_suppressed: Some(vec!["podman.x86_64".into()]),
        installed_groups: Some(vec![InstalledGroup {
            name: "Container Management".into(),
            members: vec![
                "buildah".into(),
                "containernetworking-plugins".into(),
                "podman".into(),
            ],
            ..Default::default()
        }]),
        ..Default::default()
    });
    RefineSession::new(snap)
}

/// A group whose two declared members are both present and locked, so the
/// effective set (present, actionable, not locked) is empty.
fn all_locked_group_session() -> RefineSession {
    let mut snap = InspectionSnapshot::new();
    snap.baseline = Some(empty_baseline());
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "glibc".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "baseos".into(),
                disposition: FindingKind::included(),
                locked: true,
                ..Default::default()
            },
            PackageEntry {
                name: "systemd".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "baseos".into(),
                disposition: FindingKind::included(),
                locked: true,
                ..Default::default()
            },
        ],
        installed_groups: Some(vec![InstalledGroup {
            name: "Locked Group".into(),
            members: vec!["glibc".into(), "systemd".into()],
            ..Default::default()
        }]),
        ..Default::default()
    });
    RefineSession::new(snap)
}

fn set_include_group(name: &str, include: bool) -> RefinementOp {
    RefinementOp::SetInclude {
        item_id: ItemId::Group {
            name: name.to_string(),
        },
        include,
    }
}

fn containerfile_of(s: &RefineSession) -> String {
    s.view().containerfile_preview.clone()
}

/// Extracts package names from `RUN dnf install -y` blocks only (never
/// `dnf group install`), so a caller can confirm a package did not slip
/// into the individual install list.
fn active_install_list(containerfile: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_block = false;
    for line in containerfile.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("RUN dnf install -y") {
            in_block = true;
            continue;
        }
        if !in_block {
            continue;
        }
        if trimmed.starts_with("&&") || trimmed.is_empty() {
            in_block = false;
            continue;
        }
        names.push(trimmed.trim_end_matches('\\').trim().to_string());
    }
    names
}

struct TestGroupInfo {
    render_state: String,
    effective_member_count: usize,
}

/// A minimal stand-in for `web::web_types::GroupInfo`, computed independently
/// here since `crates/web` isn't a dependency of this integration test.
fn group_info(s: &RefineSession, name: &str) -> TestGroupInfo {
    let render_state = match s.render_context().group_states.get(name) {
        Some(GroupRenderState::Renderable) | None => "renderable",
        Some(GroupRenderState::Excluded) => "excluded",
        Some(GroupRenderState::Ungrouped) => "ungrouped",
        Some(GroupRenderState::Inert) => "inert",
        Some(GroupRenderState::Degraded { .. }) => "degraded",
    }
    .to_string();

    let projected = s.snapshot_projected();
    let (packages_added, members): (Vec<PackageEntry>, Vec<String>) = projected
        .rpm
        .as_ref()
        .map(|r| {
            let members = r
                .installed_groups
                .as_ref()
                .and_then(|gs| gs.iter().find(|g| g.name == name))
                .map(|g| g.members.clone())
                .unwrap_or_default();
            (r.packages_added.clone(), members)
        })
        .unwrap_or_default();

    let effective_member_count = group_state::effective_members(&packages_added, &members).len();

    TestGroupInfo {
        render_state,
        effective_member_count,
    }
}

#[test]
fn group_exclude_on_an_already_excluded_set_is_refused() {
    // § 8.8: Container Management, two non-leaf present members, one
    // baseline-suppressed. Both present members normalize to excluded at
    // construction, so a fresh exclude is a no-op before any op runs.
    let mut s = container_management_session();
    match s.apply(set_include_group("Container Management", false)) {
        Err(RefineError::NoopTarget(msg)) => assert_eq!(
            msg,
            "all 2 effective members of 'Container Management' are already excluded"
        ),
        other => panic!("expected NoopTarget, got {other:?}"),
    }
    assert_eq!(s.timeline_len(), 0);
}

#[test]
fn group_include_then_exclude_moves_the_render_state_and_the_export() {
    let mut s = container_management_session();
    s.apply(set_include_group("Container Management", true))
        .unwrap();
    assert_eq!(s.timeline_len(), 1);
    // The group is now Renderable: `dnf group install` carries the group
    // name, and its members are suppressed from the individual install list
    // by design (see containerfile.rs's `group_member_suppressed`).
    let cf = containerfile_of(&s);
    assert!(cf.contains("dnf group install") && cf.contains("\"Container Management\""));
    // Falsifies both the suppression claim above and `active_install_list`
    // itself: the list is non-empty (vim, a leaf non-member) and excludes
    // buildah, a renderable group's member suppressed from individual
    // install. Without `vim` in the fixture this would hold vacuously —
    // there would be no `RUN dnf install` block to parse at all.
    let installed = active_install_list(&cf);
    assert!(
        !installed.is_empty(),
        "vim must appear in the active install list"
    );
    assert!(!installed.contains(&"buildah".to_string()));

    // A second include is the "already included" branch — exact message,
    // not just the refusal kind, since Task 7 and the frontend assert on it.
    match s.apply(set_include_group("Container Management", true)) {
        Err(RefineError::NoopTarget(msg)) => assert_eq!(
            msg,
            "all 2 effective members of 'Container Management' are already included"
        ),
        other => panic!("expected NoopTarget, got {other:?}"),
    }
    assert_eq!(
        s.timeline_len(),
        1,
        "the refused re-include must not record"
    );

    s.apply(set_include_group("Container Management", false))
        .unwrap();
    assert_eq!(
        group_info(&s, "Container Management").render_state,
        "excluded"
    );
    let cf = containerfile_of(&s);
    // Pre-fix, the group's effective set was built from the leaf-filtered
    // display list: both members are non-leaf and excluded, so they drop out
    // of that list entirely, the group reads as having zero members, and the
    // old code's fallthrough treats zero members as Renderable — `dnf group
    // install` would still be emitted for a group installing nothing.
    assert!(!cf.contains("dnf group install"));
    assert!(!active_install_list(&cf).contains(&"buildah".to_string()));

    s.undo().unwrap();
    assert!(containerfile_of(&s).contains("dnf group install"));
    s.redo().unwrap();
    assert!(!containerfile_of(&s).contains("dnf group install"));
}

#[test]
fn a_group_whose_present_members_are_all_locked_is_inert() {
    let mut s = all_locked_group_session();
    let gi = group_info(&s, "Locked Group");
    assert_eq!(gi.render_state, "inert");
    assert_eq!(gi.effective_member_count, 0);
    assert!(!containerfile_of(&s).contains("dnf group install"));
    match s.apply(set_include_group("Locked Group", true)) {
        Err(RefineError::NoopTarget(msg)) => {
            assert_eq!(
                msg,
                "nothing to decide in 'Locked Group': all 2 members are locked"
            )
        }
        other => panic!("expected NoopTarget, got {other:?}"),
    }
    assert!(matches!(
        s.apply(set_include_group("Locked Group", false)),
        Err(RefineError::NoopTarget(_))
    ));
}

// -- Task 7: repo effective members and the member-based no-op rule --------

/// A `Verified` repo "vendor" with two actionable, unlocked members ("a" and
/// "b"), no repo op applied yet. A baseline (even empty) is required so both
/// packages normalize to the Site bucket, defaulting to included (no
/// `leaf_packages` means every Site package defaults to leaf) -- without it
/// both would normalize to Investigate -> excluded at construction and every
/// op below would already be a no-op before the test gets to exercise one.
fn verified_repo_session() -> RefineSession {
    let mut snap = InspectionSnapshot::new();
    snap.baseline = Some(empty_baseline());
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "a".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "vendor".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
            PackageEntry {
                name: "b".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "vendor".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
        ],
        repo_files: vec![RepoFile {
            path: "/etc/yum.repos.d/vendor.repo".into(),
            content: "[vendor]\nname=Vendor\nbaseurl=http://example.invalid/vendor\n".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        ..Default::default()
    });
    RefineSession::new(snap)
}

/// The same `Verified` "vendor" repo, but both declared members are present
/// and locked, so the effective set (present, actionable, not locked) is
/// empty even though the index knows about and accepts the repo.
fn all_members_locked_repo_session() -> RefineSession {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "a".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "vendor".into(),
                disposition: FindingKind::included(),
                locked: true,
                ..Default::default()
            },
            PackageEntry {
                name: "b".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "vendor".into(),
                disposition: FindingKind::included(),
                locked: true,
                ..Default::default()
            },
        ],
        repo_files: vec![RepoFile {
            path: "/etc/yum.repos.d/vendor.repo".into(),
            content: "[vendor]\nname=Vendor\nbaseurl=http://example.invalid/vendor\n".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        ..Default::default()
    });
    RefineSession::new(snap)
}

/// A `Verified` "vendor" repo with no package attributed to it at all --
/// the index knows the section (and accepts it), but the effective set is
/// empty because nothing installed from it.
fn repo_known_to_index_with_no_packages() -> RefineSession {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![],
        repo_files: vec![RepoFile {
            path: "/etc/yum.repos.d/vendor.repo".into(),
            content: "[vendor]\nname=Vendor\nbaseurl=http://example.invalid/vendor\n".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        ..Default::default()
    });
    RefineSession::new(snap)
}

/// No `rpm` section at all: `RepoIndex::build` returns an empty index, and
/// any id resolves to `Unknown` provenance.
fn session_with_no_rpm_section() -> RefineSession {
    RefineSession::new(InspectionSnapshot::new())
}

fn set_include_pkg(canonical_id: &str, include: bool) -> RefinementOp {
    let (name, arch) = canonical_id
        .rsplit_once('.')
        .expect("canonical package id has a dot");
    RefinementOp::SetInclude {
        item_id: ItemId::Package {
            name: name.to_string(),
            arch: arch.to_string(),
        },
        include,
    }
}

fn set_include_repo(section_id: &str, include: bool) -> RefinementOp {
    RefinementOp::SetInclude {
        item_id: ItemId::Repo {
            path: section_id.to_string(),
        },
        include,
    }
}

/// Canonical `name.arch` ids of every currently-included package in the
/// view -- the install list an exported Containerfile would carry.
fn install_list(s: &RefineSession) -> Vec<String> {
    s.view()
        .packages
        .iter()
        .filter(|p| p.entry.disposition.is_included())
        .map(|p| format!("{}.{}", p.entry.name, p.entry.arch))
        .collect()
}

/// Whether the projected snapshot's repo file ending in `basename` is
/// included -- the export carries it.
fn export_carries_repo_file(s: &RefineSession, basename: &str) -> bool {
    s.snapshot_projected()
        .rpm
        .as_ref()
        .map(|r| {
            r.repo_files
                .iter()
                .any(|rf| rf.path.ends_with(basename) && rf.disposition.is_included())
        })
        .unwrap_or(false)
}

#[test]
fn repo_include_all_records_when_one_member_was_individually_excluded() {
    // § 8.8 "Repo regression". Pre-task: `is_op_noop`'s `ItemId::Repo` arm
    // read only `excluded_sections_at` (repo-exclusion history), which is
    // empty here (no repo op has run) -- so `SetInclude(Repo, true)` below
    // read as "already not excluded", `apply`'s idempotency shortcut
    // returned `Ok(())` silently, and the timeline never grew: this
    // assertion fails against pre-task code.
    let mut s = verified_repo_session(); // two actionable unlocked members, no repo op yet
    s.apply(set_include_pkg("b.x86_64", false)).unwrap();
    let before = s.timeline_len();
    s.apply(set_include_repo("vendor", true)).unwrap();
    assert_eq!(s.timeline_len(), before + 1);
    assert!(install_list(&s).contains(&"b.x86_64".to_string()));
    assert!(
        !s.pending_changes()
            .repos_excluded()
            .contains(&"vendor".to_string())
    );
    s.undo().unwrap();
    assert!(!install_list(&s).contains(&"b.x86_64".to_string()));
    s.redo().unwrap();
    assert!(install_list(&s).contains(&"b.x86_64".to_string()));
    // Both members are now included: a fresh include-all is a real no-op.
    assert!(matches!(
        s.apply(set_include_repo("vendor", true)),
        Err(RefineError::NoopTarget(_))
    ));
}

#[test]
fn history_and_members_disagree_in_both_directions() {
    // § 8.8, position 29. State (a): enabled history, both members
    // individually excluded. Pre-task: the history-reading arm saw no repo
    // op (empty `excluded_sections_at`) and refused BOTH directions as
    // already-satisfied for `include: true` and accepted-but-recorded-
    // nothing for `include: false` depending on direction read -- in
    // particular the `include: false` case below would have returned
    // `Ok(())` with no timeline growth instead of `NoopTarget`, so this
    // assertion fails against pre-task code.
    let mut a = verified_repo_session();
    a.apply(set_include_pkg("a.x86_64", false)).unwrap();
    a.apply(set_include_pkg("b.x86_64", false)).unwrap();
    assert!(matches!(
        a.apply(set_include_repo("vendor", false)),
        Err(RefineError::NoopTarget(_))
    ));
    let n = a.timeline_len();
    a.apply(set_include_repo("vendor", true)).unwrap();
    assert_eq!(a.timeline_len(), n + 1);
    assert_eq!(install_list(&a).len(), 2);
    a.undo().unwrap();
    assert_eq!(install_list(&a).len(), 0);
    a.redo().unwrap();
    assert_eq!(install_list(&a).len(), 2);

    // State (b): excluded history, both members individually included.
    // Pre-task: `include: true` here read as "section not in excluded set"
    // -> false -> not a no-op, so it would have been accepted instead of
    // refused -- this assertion fails against pre-task code too.
    let mut b = verified_repo_session();
    b.apply(set_include_repo("vendor", false)).unwrap();
    b.apply(set_include_pkg("a.x86_64", true)).unwrap();
    b.apply(set_include_pkg("b.x86_64", true)).unwrap();
    assert!(matches!(
        b.apply(set_include_repo("vendor", true)),
        Err(RefineError::NoopTarget(_))
    ));
    b.apply(set_include_repo("vendor", false)).unwrap();
    assert_eq!(install_list(&b).len(), 0);
    assert!(!export_carries_repo_file(&b, "vendor.repo"));
}

#[test]
fn a_repo_with_no_effective_member_is_refused_in_both_directions() {
    // § 8.8 + position 26. A repo the index KNOWS and accepts, whose
    // effective set is empty. Pre-task: with no repo-exclude op in the
    // timeline, `include: true` read as "not excluded" -> no-op -> silent
    // `Ok(())`; `include: false` read as "not yet excluded" -> not a no-op
    // -> accepted and recorded, cascading onto a repo file no package
    // justifies deciding through this control (position 26's bug). Either
    // branch fails this assertion against pre-task code.
    //
    // Review round 1: a single `starts_with("nothing to decide")` shared by
    // both fixtures cannot tell the two reasons apart -- it passes whether
    // `present_count` is always 0, always positive, or correct. Each fixture
    // is paired with its own full expected message and asserted with
    // `assert_eq!` instead, which would fail if `present_count` collapsed to
    // either constant.
    for (mut s, expected) in [
        (
            all_members_locked_repo_session(),
            "nothing to decide in 'vendor': all 2 members are locked",
        ),
        (
            repo_known_to_index_with_no_packages(),
            "nothing to decide in 'vendor': no package is attributed to it",
        ),
    ] {
        for include in [true, false] {
            match s.apply(set_include_repo("vendor", include)) {
                Err(RefineError::NoopTarget(m)) => assert_eq!(m, expected),
                other => panic!("expected NoopTarget, got {other:?}"),
            }
        }
        assert_eq!(s.timeline_len(), 0);
    }
}

#[test]
fn a_repo_op_with_no_rpm_section_is_refused_on_provenance() {
    // Review Focus 3. This is a DIFFERENT refusal from the one above and
    // must not be folded into it: `RepoIndex::build` on `rpm: None` returns
    // an empty index, an id absent from it resolves to `Unknown`, and
    // `apply` runs `validate_target` before the no-op check -- so § 3.2's
    // provenance refusal fires first and the effective-set arm is never
    // reached. There is no valid repo in this fixture whose effective set
    // could be examined. This is unaffected by Task 7's change (it exercises
    // `validate_target`, not `is_op_noop`/`check_repo_noop`), so it already
    // passes against pre-task code -- it is here to prove the two refusals
    // stay distinct, not to prove the member-based rule fires.
    let mut s = session_with_no_rpm_section();
    for include in [true, false] {
        match s.apply(set_include_repo("vendor", include)) {
            // the message names the provenance; `validate_target` formats it with `{prov:?}`
            Err(RefineError::BadRequest(m)) => assert!(m.to_ascii_lowercase().contains("unknown")),
            other => panic!("expected BadRequest naming the provenance, got {other:?}"),
        }
    }
    assert_eq!(s.timeline_len(), 0);
}

/// § 3.3's demotion has to run inside `project_snapshot`, not only in the
/// renderer: the refine view keys the RPM upload control off
/// `repoless_annotation`, so a package whose serving repo the operator
/// just dropped has to grow one. A test of the predicate alone would
/// still pass with the call site deleted.
#[test]
fn excluding_the_serving_repo_annotates_the_package_in_the_projection() {
    let mut snap = InspectionSnapshot::new();
    snap.baseline = Some(empty_baseline());
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "vendor-release".into(),
                arch: "noarch".into(),
                state: PackageState::Added,
                source_repo: "@commandline".into(),
                disposition: FindingKind::included(),
                repo_enabling_from: vec![inspectah_core::types::repo::RepoId::new("bootstrap")],
                ..Default::default()
            },
            // § 3.2 refuses a repo toggle with no effective member, and a
            // package the section merely *serves* is attributed to
            // `@commandline`, not to it. The section needs a member of its
            // own before the operator can drop it at all.
            PackageEntry {
                name: "bootstrap-tool".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "bootstrap".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
        ],
        repo_files: vec![RepoFile {
            path: "/etc/yum.repos.d/bootstrap.repo".into(),
            content: "[bootstrap]\nbaseurl=https://example.invalid/bootstrap\nenabled=1\n".into(),
            is_default_repo: false,
            disposition: FindingKind::included(),
            locked: false,
            aggregate: None,
        }],
        ..Default::default()
    });

    let mut session = RefineSession::new(snap);
    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Package {
                name: "vendor-release".into(),
                arch: "noarch".into(),
            },
            include: true,
        })
        .expect("the package is includable");

    let annotation_of = |s: &RefineSession| {
        s.snapshot_projected()
            .rpm
            .expect("the projection keeps the rpm section")
            .packages_added
            .iter()
            .find(|p| p.name == "vendor-release")
            .expect("the projection keeps the package")
            .repoless_annotation
            .clone()
    };

    assert_eq!(
        annotation_of(&session),
        "",
        "the carried case must be clean, or the assertion below is free"
    );

    session
        .apply(set_include_repo("bootstrap", false))
        .expect("the section is toggleable");

    assert_eq!(
        annotation_of(&session),
        "No repo source: the repo that served vendor-release (bootstrap) \
         is not carried by this export",
    );
}

// ── A shadowed config file is not a second decision ──────────────────
//
// A `/etc/sysctl.d/` file whose keys produced sysctl overrides reaches
// the image only through those overrides. The file's own row is
// inventory, so both directions of a toggle on it are refused and
// neither extends the timeline, while the keys stay operable.

const SHADOWED_SYSCTL_FILE: &str = "/etc/sysctl.d/99-driftify.conf";

fn shadowed_sysctl_snapshot() -> InspectionSnapshot {
    use inspectah_core::types::kernelboot::{KernelBootSection, SysctlOverride};

    let mut snap = InspectionSnapshot::new();
    snap.baseline = Some(empty_baseline());
    snap.kernel_boot = Some(KernelBootSection {
        sysctl_overrides: vec![SysctlOverride {
            key: "vm.swappiness".into(),
            runtime: "60".into(),
            default: "10".into(),
            source: SHADOWED_SYSCTL_FILE.into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        ..Default::default()
    });
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: SHADOWED_SYSCTL_FILE.into(),
            kind: ConfigFileKind::Unowned,
            category: inspectah_core::types::config::ConfigCategory::Sysctl,
            content: "vm.swappiness = 10\n".into(),
            disposition: FindingKind::inventory(),
            // The pair `finalize_cross_inspector` actually writes. The
            // refusal's predicate is `is_inventory()` alone, but a
            // fixture that cannot occur is a worse guard than one that
            // can.
            shadowed_by: Some(inspectah_core::types::config::ConfigShadow::Sysctl),
            ..Default::default()
        }],
    });
    snap
}

#[test]
fn toggling_a_shadowed_config_file_is_refused_in_both_directions() {
    let mut session = RefineSession::new(shadowed_sysctl_snapshot());
    let before = session.timeline_len();

    for include in [true, false] {
        let result = session.apply(RefinementOp::SetInclude {
            item_id: ItemId::Config {
                path: SHADOWED_SYSCTL_FILE.into(),
            },
            include,
        });
        assert!(
            matches!(result, Err(RefineError::InventoryNotToggleable(_))),
            "SetInclude(Config, {include}) on a shadowed file must be refused, got {result:?}"
        );
        assert_eq!(
            session.timeline_len(),
            before,
            "a refused op must not extend the timeline"
        );
    }
}

#[test]
fn the_sysctl_key_behind_a_shadowed_file_stays_operable() {
    let mut session = RefineSession::new(shadowed_sysctl_snapshot());
    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Sysctl {
                key: "vm.swappiness".into(),
            },
            include: false,
        })
        .expect("the key is the decision and must stay toggleable");

    let dir = tempfile::tempdir().unwrap();
    inspectah_pipeline::render::configtree::write_config_tree(
        &session.snapshot_projected(),
        dir.path(),
    )
    .unwrap();

    assert!(
        !dir.path()
            .join("sysctl/etc/sysctl.d/99-inspectah-migrated.conf")
            .exists(),
        "excluding the only key must leave no synthesized drop-in"
    );
    assert!(
        !dir.path()
            .join("config/etc/sysctl.d/99-driftify.conf")
            .exists(),
        "the shadowed source file is inventory and never materializes"
    );
}
