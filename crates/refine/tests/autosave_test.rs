use inspectah_refine::autosave::{
    SessionState, compute_tarball_hash, load_session, save_session, session_file_path,
};
use inspectah_refine::types::ContentHash;
use std::path::PathBuf;

#[test]
fn session_file_path_strips_tar_gz() {
    let p = session_file_path(&PathBuf::from("/data/fleet-web-2026-05-20.tar.gz"));
    assert_eq!(
        p.file_name().unwrap(),
        ".inspectah-session-fleet-web-2026-05-20.json"
    );
    assert_eq!(p.parent().unwrap(), std::path::Path::new("/data"));
}

#[test]
fn session_file_path_strips_tgz() {
    let p = session_file_path(&PathBuf::from("/tmp/fleet.tgz"));
    assert_eq!(p.file_name().unwrap(), ".inspectah-session-fleet.json");
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
