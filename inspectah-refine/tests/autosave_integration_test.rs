use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
use inspectah_refine::autosave::{load_session, session_file_path};
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::{PackageTarget, RefinementOp};

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
        ],
        ..Default::default()
    });
    snap
}

fn make_session_with_tarball_path() -> (RefineSession, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let tarball = dir.path().join("test.tar.gz");
    std::fs::write(&tarball, b"fake").unwrap();
    let snap = test_snapshot();
    let session = RefineSession::new_with_tarball(snap, tarball);
    (session, dir)
}

fn exclude_httpd() -> RefinementOp {
    RefinementOp::ExcludePackage(PackageTarget {
        name: "httpd".into(),
        arch: "x86_64".into(),
    })
}

fn exclude_glibc() -> RefinementOp {
    RefinementOp::ExcludePackage(PackageTarget {
        name: "glibc".into(),
        arch: "x86_64".into(),
    })
}

#[test]
fn session_file_created_after_first_apply() {
    let (mut session, dir) = make_session_with_tarball_path();
    let tarball = dir.path().join("test.tar.gz");
    let session_path = session_file_path(&tarball);

    assert!(!session_path.exists(), "session file should not exist before any apply");

    session.apply(exclude_httpd()).unwrap();

    assert!(session_path.exists(), "session file should exist after apply");
}

#[test]
fn session_file_updated_after_undo() {
    let (mut session, dir) = make_session_with_tarball_path();
    let tarball = dir.path().join("test.tar.gz");

    session.apply(exclude_httpd()).unwrap();

    // Verify cursor is 1 after apply
    let state = load_session(&tarball).unwrap().unwrap();
    assert_eq!(state.cursor, 1);
    assert_eq!(state.ops.len(), 1);

    session.undo().unwrap();

    // After undo, cursor should be 0 but ops still present
    let state = load_session(&tarball).unwrap().unwrap();
    assert_eq!(state.cursor, 0);
    assert_eq!(state.ops.len(), 1);
}

#[test]
fn session_file_updated_after_redo() {
    let (mut session, dir) = make_session_with_tarball_path();
    let tarball = dir.path().join("test.tar.gz");

    session.apply(exclude_httpd()).unwrap();
    session.undo().unwrap();
    session.redo().unwrap();

    let state = load_session(&tarball).unwrap().unwrap();
    assert_eq!(state.cursor, 1);
    assert_eq!(state.ops.len(), 1);
}

#[test]
fn replay_from_session_reconstructs_cursor() {
    let (mut session, dir) = make_session_with_tarball_path();
    let tarball = dir.path().join("test.tar.gz");

    // Apply two ops, undo one
    session.apply(exclude_httpd()).unwrap();
    session.apply(exclude_glibc()).unwrap();
    session.undo().unwrap();

    // Persisted state should have 2 ops, cursor at 1
    let state = load_session(&tarball).unwrap().unwrap();
    assert_eq!(state.ops.len(), 2);
    assert_eq!(state.cursor, 1);
}

#[test]
fn noop_apply_does_not_trigger_save() {
    let (mut session, dir) = make_session_with_tarball_path();
    let tarball = dir.path().join("test.tar.gz");
    let session_path = session_file_path(&tarball);

    // Excluding a package that is already excluded is a noop
    session.apply(exclude_httpd()).unwrap();
    assert!(session_path.exists());

    let mtime_before = std::fs::metadata(&session_path).unwrap().modified().unwrap();

    // Small sleep to ensure mtime would differ
    std::thread::sleep(std::time::Duration::from_millis(50));

    // Re-excluding httpd is a noop — should not re-save
    session.apply(exclude_httpd()).unwrap();

    let mtime_after = std::fs::metadata(&session_path).unwrap().modified().unwrap();
    assert_eq!(mtime_before, mtime_after, "noop apply should not update session file");
}

#[test]
fn tarball_path_round_trips_through_session_state() {
    let (mut session, dir) = make_session_with_tarball_path();
    let tarball = dir.path().join("test.tar.gz");

    session.apply(exclude_httpd()).unwrap();

    let state = load_session(&tarball).unwrap().unwrap();
    assert_eq!(state.tarball_path, tarball);
    assert_eq!(state.schema_version, 1);
}

#[test]
fn session_without_tarball_does_not_autosave() {
    // A session created with new() (no tarball) should not create any files
    let dir = tempfile::tempdir().unwrap();
    let snap = test_snapshot();
    let mut session = RefineSession::new(snap);

    session
        .apply(exclude_httpd())
        .unwrap();

    // No session file should exist anywhere in the temp dir
    let entries: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().contains("inspectah-session"))
        .collect();
    assert!(entries.is_empty(), "no session file should be created for tarball-less sessions");
}
