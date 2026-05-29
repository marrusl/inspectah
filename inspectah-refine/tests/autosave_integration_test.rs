use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::redaction::RedactionState;
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
use inspectah_refine::autosave::{load_session, session_file_path};
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::{ItemId, RefineError, RefinementOp};

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

/// Build a snapshot that passes `from_tarball()` provenance validation.
fn test_snapshot_redacted() -> InspectionSnapshot {
    let mut snap = test_snapshot();
    snap.redaction_state = Some(RedactionState::FullyRedacted {
        redacted_by: "inspectah-test".into(),
        config_hash: "abc".into(),
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

/// Create a real `.tar.gz` file containing a valid `inspection-snapshot.json`,
/// suitable for round-tripping through `from_tarball()` / `resume_from()`.
fn write_real_tarball(path: &std::path::Path, snap: &InspectionSnapshot) {
    use flate2::Compression;
    use flate2::write::GzEncoder;

    let json = serde_json::to_string_pretty(snap).unwrap();
    let f = std::fs::File::create(path).unwrap();
    let gz = GzEncoder::new(f, Compression::default());
    let mut tar = tar::Builder::new(gz);

    let json_bytes = json.as_bytes();
    let mut header = tar::Header::new_gnu();
    header.set_path("inspection-snapshot.json").unwrap();
    header.set_size(json_bytes.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    tar.append(&header, json_bytes).unwrap();
    tar.finish().unwrap();
}

/// Create a session backed by a real tarball that can round-trip through
/// `resume_from()`.
fn make_session_with_real_tarball() -> (RefineSession, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let tarball = dir.path().join("test.tar.gz");
    let snap = test_snapshot_redacted();
    write_real_tarball(&tarball, &snap);

    let mut session = inspectah_refine::tarball::from_tarball(&tarball).unwrap();
    session.set_tarball_path(tarball);
    (session, dir)
}

fn exclude_httpd() -> RefinementOp {
    RefinementOp::SetInclude {
        item_id: ItemId::Package {
            name: "httpd".into(),
            arch: "x86_64".into(),
        },
        include: false,
    }
}

fn exclude_glibc() -> RefinementOp {
    RefinementOp::SetInclude {
        item_id: ItemId::Package {
            name: "glibc".into(),
            arch: "x86_64".into(),
        },
        include: false,
    }
}

#[test]
fn session_file_created_after_first_apply() {
    let (mut session, dir) = make_session_with_tarball_path();
    let tarball = dir.path().join("test.tar.gz");
    let session_path = session_file_path(&tarball);

    assert!(
        !session_path.exists(),
        "session file should not exist before any apply"
    );

    session.apply(exclude_httpd()).unwrap();

    assert!(
        session_path.exists(),
        "session file should exist after apply"
    );
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

    let mtime_before = std::fs::metadata(&session_path)
        .unwrap()
        .modified()
        .unwrap();

    // Small sleep to ensure mtime would differ
    std::thread::sleep(std::time::Duration::from_millis(50));

    // Re-excluding httpd is a noop — should not re-save
    session.apply(exclude_httpd()).unwrap();

    let mtime_after = std::fs::metadata(&session_path)
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(
        mtime_before, mtime_after,
        "noop apply should not update session file"
    );
}

#[test]
fn tarball_path_round_trips_through_session_state() {
    let (mut session, dir) = make_session_with_tarball_path();
    let tarball = dir.path().join("test.tar.gz");

    session.apply(exclude_httpd()).unwrap();

    let state = load_session(&tarball).unwrap().unwrap();
    assert_eq!(state.tarball_path, tarball);
    assert_eq!(state.schema_version, 2);
}

#[test]
fn session_without_tarball_does_not_autosave() {
    // A session created with new() (no tarball) should not create any files
    let dir = tempfile::tempdir().unwrap();
    let snap = test_snapshot();
    let mut session = RefineSession::new(snap);

    session.apply(exclude_httpd()).unwrap();

    // No session file should exist anywhere in the temp dir
    let entries: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .contains("inspectah-session")
        })
        .collect();
    assert!(
        entries.is_empty(),
        "no session file should be created for tarball-less sessions"
    );
}

#[test]
fn stale_tarball_detected_on_resume() {
    let (mut session, dir) = make_session_with_tarball_path();
    let tarball = dir.path().join("test.tar.gz");

    // Apply an op to create a sidecar
    session.apply(exclude_httpd()).unwrap();

    // Verify sidecar exists
    assert!(session_file_path(&tarball).exists());

    // Modify the tarball content to make it stale
    std::fs::write(&tarball, b"modified-content-different-hash").unwrap();

    // Attempt to resume — should fail with StaleTarball
    let result = RefineSession::resume_from(&tarball);
    match result {
        Err(RefineError::StaleTarball {
            saved_hash,
            current_hash,
        }) => {
            assert_ne!(
                saved_hash, current_hash,
                "hashes must differ for stale detection"
            );
        }
        Err(other) => panic!("expected StaleTarball error, got: {other}"),
        Ok(_) => panic!("resume_from must fail on stale tarball"),
    }
}

#[test]
fn resume_preserves_redo_tail() {
    let (mut session, dir) = make_session_with_real_tarball();
    let tarball = dir.path().join("test.tar.gz");

    // Apply 3 ops
    session.apply(exclude_httpd()).unwrap();
    session.apply(exclude_glibc()).unwrap();
    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Package {
                name: "httpd".into(),
                arch: "x86_64".into(),
            },
            include: true,
        })
        .unwrap();

    // Undo once: cursor=2, ops=3
    session.undo().unwrap();
    assert_eq!(session.cursor(), 2);
    assert!(session.can_redo());

    // Verify persisted state
    let state = load_session(&tarball).unwrap().unwrap();
    assert_eq!(state.ops.len(), 3, "sidecar must have all 3 ops");
    assert_eq!(state.cursor, 2, "sidecar cursor must be 2");

    // Drop session and resume
    drop(session);
    let resumed = RefineSession::resume_from(&tarball).unwrap().unwrap();

    // Verify redo tail is preserved
    assert_eq!(
        resumed.ops_history().len(),
        3,
        "resumed session must have all 3 ops"
    );
    assert!(resumed.can_redo(), "resumed session must be able to redo");
    assert_eq!(resumed.cursor(), 2, "resumed cursor must be 2");
}

#[test]
fn resume_does_not_truncate_redo_on_autosave() {
    let (mut session, dir) = make_session_with_real_tarball();
    let tarball = dir.path().join("test.tar.gz");

    // Apply 3 ops, undo once
    session.apply(exclude_httpd()).unwrap();
    session.apply(exclude_glibc()).unwrap();
    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Package {
                name: "httpd".into(),
                arch: "x86_64".into(),
            },
            include: true,
        })
        .unwrap();
    session.undo().unwrap();

    // Drop and resume
    drop(session);
    let _resumed = RefineSession::resume_from(&tarball).unwrap().unwrap();

    // After resume, the autosave triggered by resume_from should preserve
    // all 3 ops in the sidecar (not truncate to cursor)
    let state = load_session(&tarball).unwrap().unwrap();
    assert_eq!(
        state.ops.len(),
        3,
        "sidecar after resume must still have all 3 ops (redo tail preserved)"
    );
    assert_eq!(state.cursor, 2, "sidecar cursor must remain at 2");
}
