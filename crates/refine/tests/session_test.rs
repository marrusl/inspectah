mod helpers;

use inspectah_core::baseline::BaselineData;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind, ConfigSection};
use inspectah_core::types::fleet::FleetPrevalence;
use inspectah_core::types::rpm::{PackageEntry, PackageState, RepoFile, RpmSection};
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::{ItemId, RefineError, RefinementOp};

use helpers::*;

fn test_snapshot() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
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
                name: "glibc".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "baseos".into(),
                include: true,
                ..Default::default()
            },
            PackageEntry {
                name: "glibc".into(),
                arch: "i686".into(),
                state: PackageState::Added,
                source_repo: "baseos".into(),
                include: true,
                ..Default::default()
            },
        ],
        ..Default::default()
    });
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/httpd/conf/httpd.conf".into(),
            kind: ConfigFileKind::RpmOwnedModified,
            include: true,
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

fn fleet(count: i32, total: i32, hosts: &[&str]) -> FleetPrevalence {
    FleetPrevalence {
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

    assert!(glibc_x86.entry.include);
    assert!(!glibc_i686.entry.include);
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

    let result = session.export_tarball(&tarball_path, 999);
    assert!(matches!(
        result,
        Err(RefineError::StaleGeneration {
            expected: 999,
            actual: 0
        })
    ));
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
    // Use degraded mode (no baseline) to produce Tier 2 (Informational)
    // packages. With baseline present, user-added packages from recognized
    // repos are now Routine (Tier 1) and always included.
    let mut snap = InspectionSnapshot::new();
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
        baseline_package_names: None, // degraded mode -> Informational
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
                name: "mystery".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: String::new(),
                include: true,
                ..Default::default()
            },
        ],
        baseline_package_names: None,
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
        !mystery.entry.include,
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
        baseline_package_names: None,
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
    assert!(apr.entry.include);
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
                include: true,
                ..Default::default()
            },
            PackageEntry {
                name: "glibc".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "baseos".into(),
                include: true,
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
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "glibc".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "baseos".into(),
                include: true,
                ..Default::default()
            },
            PackageEntry {
                name: "glibc".into(),
                arch: "i686".into(),
                state: PackageState::Added,
                source_repo: "baseos".into(),
                include: true,
                ..Default::default()
            },
        ],
        baseline_package_names: None,
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
fn test_fleet_snapshot_skips_leaf_only_filter() {
    let mut snap = InspectionSnapshot::new();
    snap.baseline = Some(empty_baseline());
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "appstream".into(),
                include: true,
                fleet: Some(fleet(3, 5, &["host-a", "host-b", "host-c"])),
                ..Default::default()
            },
            PackageEntry {
                name: "apr".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "appstream".into(),
                include: true,
                fleet: Some(fleet(2, 5, &["host-d", "host-e"])),
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
        "fleet snapshots should keep all packages visible"
    );
    assert!(view.packages.iter().any(|pkg| pkg.entry.name == "httpd"));
    assert!(view.packages.iter().any(|pkg| pkg.entry.name == "apr"));
}

#[test]
fn test_fleet_snapshot_preview_skips_leaf_only_filter() {
    let mut snap = InspectionSnapshot::new();
    snap.baseline = Some(empty_baseline());
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "appstream".into(),
                include: true,
                fleet: Some(fleet(3, 5, &["host-a", "host-b", "host-c"])),
                ..Default::default()
            },
            PackageEntry {
                name: "apr".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "appstream".into(),
                include: true,
                fleet: Some(fleet(2, 5, &["host-d", "host-e"])),
                ..Default::default()
            },
        ],
        leaf_packages: Some(vec!["httpd.x86_64".into()]),
        auto_packages: Some(vec!["apr.x86_64".into()]),
        ..Default::default()
    });

    let session = RefineSession::new(snap);
    let preview = &session.view().containerfile_preview;
    let install_line = preview
        .lines()
        .find(|line| line.starts_with("RUN dnf install -y"))
        .expect("fleet preview must include an install line");

    assert!(
        install_line.contains("httpd"),
        "fleet preview must still include httpd, got: {install_line}"
    );
    assert!(
        install_line.contains("apr"),
        "fleet preview must not apply leaf-only filtering, got: {install_line}"
    );
}

// -- Normalization at construction tests --

#[test]
fn test_session_normalizes_at_construction() {
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
        baseline_package_names: Some(vec!["glibc".into()]),
        ..Default::default()
    });
    let session = RefineSession::new(snap);
    let view = session.view();
    assert!(
        view.packages[0].entry.include,
        "Tier 1 should be auto-included after normalization"
    );
    assert!(
        session.snapshot().rpm.as_ref().unwrap().packages_added[0].include,
        "Original snapshot must reflect normalized state"
    );
}

#[test]
fn test_session_preview_export_parity() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "httpd".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            source_repo: "appstream".into(),
            include: false,
            ..Default::default()
        }],
        baseline_package_names: Some(vec![]),
        ..Default::default()
    });
    let session = RefineSession::new(snap);
    assert!(
        session.view().packages[0].entry.include,
        "View should show Tier 2 as included"
    );
    assert!(
        session
            .snapshot_projected()
            .rpm
            .as_ref()
            .unwrap()
            .packages_added[0]
            .include,
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
                include: true,
                ..Default::default()
            },
            ConfigFileEntry {
                path: "/etc/custom.conf".into(),
                kind: ConfigFileKind::Unowned,
                include: true,
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
        !epel_pkg.entry.include,
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
        !epel_pkg.include,
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
    assert!(orig_pkg.include, "original snapshot must be unchanged");
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
            include: true,
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
        repo_file.include,
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
        gpg.include,
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
    assert!(!gpg2.include, "GPG key excluded once all sections excluded");
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
            .include
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
            .include
    );

    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Repo {
                path: "epel".into(),
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
            .include
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
            .include,
        "per-package include is still active after undoing repo include"
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
            .include
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
            .include
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
            .include
    );
}

#[test]
fn test_exclude_repo_case_insensitive() {
    // Build a snapshot where packages have source_repo in mixed case ("Epel-Testing")
    // but SetInclude(exclude repo) uses lowercase ("epel-testing"). The exclude must still match.
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "httpd".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            source_repo: "Epel-Testing".into(), // Mixed case
            include: true,
            ..Default::default()
        }],
        repo_files: vec![RepoFile {
            path: "/etc/yum.repos.d/epel-testing.repo".into(),
            content: "[epel-testing]\nname=EPEL Testing\n".into(),
            include: true,
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
        !httpd.include,
        "SetInclude(exclude repo) must match case-insensitively"
    );
}

#[test]
fn test_include_repo_case_insensitive() {
    // Verify SetInclude(include repo) also works case-insensitively after an SetInclude(exclude repo)
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "nginx".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            source_repo: "Epel-Testing".into(),
            include: true,
            ..Default::default()
        }],
        repo_files: vec![RepoFile {
            path: "/etc/yum.repos.d/epel-testing.repo".into(),
            content: "[epel-testing]\nname=EPEL Testing\ngpgcheck=1\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL\n".into(),
            include: true,
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
        nginx.include,
        "SetInclude(include repo) must match case-insensitively after SetInclude(exclude repo)"
    );
}
