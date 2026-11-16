use inspectah_refine::autosave::{
    SessionState, compute_tarball_hash, load_session, save_session, session_file_path,
};
use inspectah_refine::types::ContentHash;
use std::path::PathBuf;

#[test]
fn session_file_path_strips_tar_gz() {
    let p = session_file_path(&PathBuf::from("/data/aggregate-web-2026-05-20.tar.gz"));
    assert_eq!(
        p.file_name().unwrap(),
        ".inspectah-session-aggregate-web-2026-05-20.json"
    );
    assert_eq!(p.parent().unwrap(), std::path::Path::new("/data"));
}

#[test]
fn session_file_path_strips_tgz() {
    let p = session_file_path(&PathBuf::from("/tmp/aggregate.tgz"));
    assert_eq!(p.file_name().unwrap(), ".inspectah-session-aggregate.json");
}

#[test]
fn session_state_serde_roundtrip() {
    let state = SessionState {
        schema_version: 3,
        tarball_path: PathBuf::from("/tmp/test.tar.gz"),
        tarball_hash: ContentHash::from_content(b"tarball"),
        timeline: vec![],
        cursor: 0,
        saved_at: "2026-05-20T00:00:00Z".into(),
    };
    let json = serde_json::to_string_pretty(&state).unwrap();
    let parsed: SessionState = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.schema_version, 3);
    assert_eq!(parsed.cursor, 0);
}

#[test]
fn atomic_save_and_load() {
    let dir = tempfile::tempdir().unwrap();
    let tarball = dir.path().join("test.tar.gz");
    std::fs::write(&tarball, b"fake tarball").unwrap();
    let state = SessionState {
        schema_version: 3,
        tarball_path: tarball.clone(),
        tarball_hash: ContentHash::from_content(b"fake tarball"),
        timeline: vec![],
        cursor: 0,
        saved_at: "2026-05-20T00:00:00Z".into(),
    };
    save_session(&state, &tarball).unwrap();
    let loaded = load_session(&tarball).unwrap();
    assert!(loaded.is_some());
    assert_eq!(loaded.unwrap().cursor, 0);
}

#[test]
fn load_returns_none_when_no_session_file() {
    let dir = tempfile::tempdir().unwrap();
    let tarball = dir.path().join("nosession.tar.gz");
    assert!(load_session(&tarball).unwrap().is_none());
}

#[test]
fn rejects_unknown_schema_version() {
    let dir = tempfile::tempdir().unwrap();
    let session_path = dir.path().join(".inspectah-session-test.json");
    std::fs::write(
        &session_path,
        r#"{"schema_version":99,"tarball_path":"/tmp/x","tarball_hash":"a","ops":[],"cursor":0,"saved_at":"x"}"#,
    )
    .unwrap();
    let tarball = dir.path().join("test.tar.gz");
    let result = load_session(&tarball);
    assert!(result.is_err());
}

#[test]
fn compute_tarball_hash_produces_valid_hash() {
    let dir = tempfile::tempdir().unwrap();
    let tarball = dir.path().join("test.tar.gz");
    std::fs::write(&tarball, b"content").unwrap();
    let hash = compute_tarball_hash(&tarball).unwrap();
    assert_eq!(hash.as_str().len(), 64);
}

#[test]
fn stale_detection_different_hash() {
    let dir = tempfile::tempdir().unwrap();
    let tarball = dir.path().join("test.tar.gz");
    std::fs::write(&tarball, b"original").unwrap();
    let state = SessionState {
        schema_version: 3,
        tarball_path: tarball.clone(),
        tarball_hash: ContentHash::from_content(b"original"),
        timeline: vec![],
        cursor: 0,
        saved_at: "2026-05-20T00:00:00Z".into(),
    };
    save_session(&state, &tarball).unwrap();
    std::fs::write(&tarball, b"modified").unwrap();
    let loaded = load_session(&tarball).unwrap().unwrap();
    let current_hash = compute_tarball_hash(&tarball).unwrap();
    assert_ne!(loaded.tarball_hash, current_hash); // stale!
}

/// A saved session holding a decision on the removed `SystemdTimer` kind
/// is refused when it is loaded, naming the kind, and the file is left as
/// it was. The same session with the op under the `GeneratedTimer` kind
/// loads (the positive control).
#[test]
fn a_saved_systemd_timer_decision_is_refused_on_load() {
    use inspectah_refine::types::{ItemId, RefinementOp, TimelineEntry};
    let dir = tempfile::tempdir().unwrap();
    let tarball = dir.path().join("test.tar.gz");
    std::fs::write(&tarball, b"fake tarball").unwrap();
    let state = SessionState {
        schema_version: 3,
        tarball_path: tarball.clone(),
        tarball_hash: ContentHash::from_content(b"fake tarball"),
        timeline: vec![TimelineEntry::Op(RefinementOp::SetInclude {
            item_id: ItemId::GeneratedTimer {
                name: "backup".into(),
            },
            include: false,
        })],
        cursor: 1,
        saved_at: "2026-11-14T00:00:00Z".into(),
    };
    save_session(&state, &tarball).unwrap();
    let loaded = load_session(&tarball)
        .expect("the control loads")
        .expect("a saved session");
    assert_eq!(loaded.timeline.len(), 1);

    let path = session_file_path(&tarball);
    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(saved.contains("\"GeneratedTimer\""), "{saved}");
    let old = saved.replace("\"GeneratedTimer\"", "\"SystemdTimer\"");
    std::fs::write(&path, &old).unwrap();
    let err = load_session(&tarball).expect_err("a SystemdTimer decision must be refused");
    assert!(
        err.to_string().contains("unknown variant `SystemdTimer`"),
        "the refusal names the kind: {err}"
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        old,
        "the file is kept"
    );
}
