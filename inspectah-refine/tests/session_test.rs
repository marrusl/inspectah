use std::path::PathBuf;

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind, ConfigSection};
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::{PackageTarget, RefineError, RefinementOp};

fn test_snapshot() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                include: true,
                ..Default::default()
            },
            PackageEntry {
                name: "glibc".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                include: true,
                ..Default::default()
            },
            PackageEntry {
                name: "glibc".into(),
                arch: "i686".into(),
                state: PackageState::Added,
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
    assert_eq!(view.stats.total_packages, 3);
    assert_eq!(view.stats.included_packages, 3);
    assert_eq!(view.stats.excluded_packages, 0);
    assert_eq!(view.stats.total_configs, 1);
    assert_eq!(view.stats.included_configs, 1);
    assert_eq!(view.stats.excluded_configs, 0);
}

#[test]
fn apply_exclude_package() {
    let mut session = RefineSession::new(test_snapshot());
    session
        .apply(RefinementOp::ExcludePackage(PackageTarget {
            name: "httpd".into(),
            arch: "x86_64".into(),
        }))
        .unwrap();

    assert_eq!(session.view().generation, 1);
    assert_eq!(session.view().stats.excluded_packages, 1);
    assert_eq!(session.view().stats.included_packages, 2);
    assert!(session.is_dirty());
}

#[test]
fn apply_unknown_target_returns_error() {
    let mut session = RefineSession::new(test_snapshot());
    let result = session.apply(RefinementOp::ExcludePackage(PackageTarget {
        name: "nonexistent".into(),
        arch: "x86_64".into(),
    }));
    assert!(matches!(result, Err(RefineError::UnknownTarget(_))));
}

#[test]
fn apply_wrong_arch_returns_error() {
    let mut session = RefineSession::new(test_snapshot());
    let result = session.apply(RefinementOp::ExcludePackage(PackageTarget {
        name: "glibc".into(),
        arch: "s390x".into(),
    }));
    assert!(matches!(result, Err(RefineError::UnknownTarget(_))));
}

#[test]
fn idempotent_exclude_is_noop() {
    let mut session = RefineSession::new(test_snapshot());
    session
        .apply(RefinementOp::ExcludePackage(PackageTarget {
            name: "httpd".into(),
            arch: "x86_64".into(),
        }))
        .unwrap();
    let gen_after_first = session.view().generation;

    // Second exclude of the same target should be a no-op
    session
        .apply(RefinementOp::ExcludePackage(PackageTarget {
            name: "httpd".into(),
            arch: "x86_64".into(),
        }))
        .unwrap();

    assert_eq!(session.view().generation, gen_after_first);
    assert_eq!(session.ops_history().len(), 1);
}

#[test]
fn undo_reverts_to_original() {
    let mut session = RefineSession::new(test_snapshot());
    session
        .apply(RefinementOp::ExcludePackage(PackageTarget {
            name: "httpd".into(),
            arch: "x86_64".into(),
        }))
        .unwrap();
    session.undo().unwrap();

    assert!(!session.is_dirty());
    assert_eq!(session.view().stats.excluded_packages, 0);
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
        .apply(RefinementOp::ExcludePackage(PackageTarget {
            name: "httpd".into(),
            arch: "x86_64".into(),
        }))
        .unwrap();
    session.undo().unwrap();
    session.redo().unwrap();

    assert!(session.is_dirty());
    assert_eq!(session.view().stats.excluded_packages, 1);
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
        .apply(RefinementOp::ExcludePackage(PackageTarget {
            name: "httpd".into(),
            arch: "x86_64".into(),
        }))
        .unwrap();
    session.undo().unwrap();

    // Apply a different op -- should truncate the undone op
    session
        .apply(RefinementOp::ExcludeConfig {
            path: PathBuf::from("/etc/httpd/conf/httpd.conf"),
        })
        .unwrap();

    assert!(matches!(session.redo(), Err(RefineError::NothingToRedo)));
    assert_eq!(session.ops_history().len(), 1);
}

#[test]
fn multiarch_targeting() {
    let mut session = RefineSession::new(test_snapshot());
    session
        .apply(RefinementOp::ExcludePackage(PackageTarget {
            name: "glibc".into(),
            arch: "i686".into(),
        }))
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
        .apply(RefinementOp::ExcludeConfig {
            path: PathBuf::from("/etc/httpd/conf/httpd.conf"),
        })
        .unwrap();

    let view = session.view();
    assert_eq!(view.stats.excluded_configs, 1);
    assert!(session.is_dirty());
}

#[test]
fn pending_changes_tracks_excludes() {
    let mut session = RefineSession::new(test_snapshot());
    session
        .apply(RefinementOp::ExcludePackage(PackageTarget {
            name: "httpd".into(),
            arch: "x86_64".into(),
        }))
        .unwrap();

    let changes = session.pending_changes();
    assert_eq!(changes.packages_excluded.len(), 1);
    assert_eq!(changes.packages_excluded[0].name, "httpd");
    assert!(changes.is_dirty);
}

#[test]
fn exclude_then_include_returns_to_clean() {
    let mut session = RefineSession::new(test_snapshot());
    session
        .apply(RefinementOp::ExcludePackage(PackageTarget {
            name: "httpd".into(),
            arch: "x86_64".into(),
        }))
        .unwrap();
    session
        .apply(RefinementOp::IncludePackage(PackageTarget {
            name: "httpd".into(),
            arch: "x86_64".into(),
        }))
        .unwrap();

    // State-based dirty: not dirty because state matches original
    assert!(!session.is_dirty());
}

#[test]
fn undo_all_then_redo_all() {
    let mut session = RefineSession::new(test_snapshot());
    session
        .apply(RefinementOp::ExcludePackage(PackageTarget {
            name: "httpd".into(),
            arch: "x86_64".into(),
        }))
        .unwrap();
    session
        .apply(RefinementOp::ExcludeConfig {
            path: PathBuf::from("/etc/httpd/conf/httpd.conf"),
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
        session.view().stats.excluded_packages,
        view_after_ops.stats.excluded_packages
    );
    assert_eq!(
        session.view().stats.excluded_configs,
        view_after_ops.stats.excluded_configs
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
