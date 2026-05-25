use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::redaction::RedactionState;
use inspectah_refine::types::RefineError;
use tempfile::tempdir;

fn make_test_snapshot(redaction: Option<RedactionState>) -> String {
    let mut snap = InspectionSnapshot::new();
    snap.redaction_state = redaction;
    serde_json::to_string_pretty(&snap).unwrap()
}

fn write_flat_tarball(dir: &std::path::Path, snap_json: &str) -> std::path::PathBuf {
    let snap_path = dir.join("inspection-snapshot.json");
    std::fs::write(&snap_path, snap_json).unwrap();

    let tarball_path = dir.join("test.tar.gz");
    let f = std::fs::File::create(&tarball_path).unwrap();
    let gz = flate2::write::GzEncoder::new(f, flate2::Compression::default());
    let mut tar = tar::Builder::new(gz);
    tar.append_path_with_name(&snap_path, "inspection-snapshot.json")
        .unwrap();
    tar.finish().unwrap();
    tarball_path
}

fn write_prefixed_tarball(dir: &std::path::Path, snap_json: &str) -> std::path::PathBuf {
    let snap_path = dir.join("inspection-snapshot.json");
    std::fs::write(&snap_path, snap_json).unwrap();

    let tarball_path = dir.join("test.tar.gz");
    let f = std::fs::File::create(&tarball_path).unwrap();
    let gz = flate2::write::GzEncoder::new(f, flate2::Compression::default());
    let mut tar = tar::Builder::new(gz);
    tar.append_path_with_name(
        &snap_path,
        "hostname-20260515-1430/inspection-snapshot.json",
    )
    .unwrap();
    tar.finish().unwrap();
    tarball_path
}

#[test]
fn load_flat_tarball() {
    let dir = tempdir().unwrap();
    let snap_json = make_test_snapshot(Some(RedactionState::FullyRedacted {
        redacted_by: "inspectah 0.8.0".into(),
        config_hash: "abc123".into(),
    }));
    let tarball = write_flat_tarball(dir.path(), &snap_json);

    let session = inspectah_refine::tarball::from_tarball(&tarball).unwrap();
    assert_eq!(session.view().generation, 0);
}

#[test]
fn load_prefixed_tarball() {
    let dir = tempdir().unwrap();
    let snap_json = make_test_snapshot(Some(RedactionState::FullyRedacted {
        redacted_by: "inspectah 0.8.0".into(),
        config_hash: "abc123".into(),
    }));
    let tarball = write_prefixed_tarball(dir.path(), &snap_json);

    let session = inspectah_refine::tarball::from_tarball(&tarball).unwrap();
    assert_eq!(session.view().generation, 0);
}

#[test]
fn reject_raw_redaction_state() {
    let dir = tempdir().unwrap();
    let snap_json = make_test_snapshot(Some(RedactionState::Raw));
    let tarball = write_flat_tarball(dir.path(), &snap_json);

    let result = inspectah_refine::tarball::from_tarball(&tarball);
    assert!(matches!(result, Err(RefineError::UntrustedSnapshot(_))));
}

#[test]
fn accept_partially_redacted() {
    let dir = tempdir().unwrap();
    let snap_json = make_test_snapshot(Some(RedactionState::PartiallyRedacted {
        redacted_by: "inspectah 0.8.0".into(),
        config_hash: "abc123".into(),
        unresolved_count: 2,
        unresolved_hints: Vec::new(),
    }));
    let tarball = write_flat_tarball(dir.path(), &snap_json);

    let session = inspectah_refine::tarball::from_tarball(&tarball).unwrap();
    assert_eq!(session.view().generation, 0);
}

#[test]
fn reject_unknown_redaction_state() {
    let dir = tempdir().unwrap();
    let snap_json = make_test_snapshot(Some(RedactionState::Unknown));
    let tarball = write_flat_tarball(dir.path(), &snap_json);

    let result = inspectah_refine::tarball::from_tarball(&tarball);
    assert!(matches!(result, Err(RefineError::UntrustedSnapshot(_))));
}

#[test]
fn reject_absent_redaction_state() {
    let dir = tempdir().unwrap();
    let snap_json = make_test_snapshot(None);
    let tarball = write_flat_tarball(dir.path(), &snap_json);

    let result = inspectah_refine::tarball::from_tarball(&tarball);
    assert!(matches!(result, Err(RefineError::UntrustedSnapshot(_))));
}

#[test]
fn reject_path_traversal() {
    let dir = tempdir().unwrap();
    let tarball_path = dir.path().join("evil.tar.gz");
    let f = std::fs::File::create(&tarball_path).unwrap();
    let gz = flate2::write::GzEncoder::new(f, flate2::Compression::default());
    let mut tar = tar::Builder::new(gz);

    // Add a file with path traversal — set_path rejects ".." so
    // we write the path bytes directly into the header's name field.
    let content = b"malicious content";
    let mut header = tar::Header::new_ustar();
    // Write the traversal path directly into the ustar name field.
    {
        let raw = header.as_old_mut();
        let path_bytes = b"../../etc/passwd";
        raw.name[..path_bytes.len()].copy_from_slice(path_bytes);
        raw.name[path_bytes.len()] = 0;
    }
    header.set_size(content.len() as u64);
    header.set_mode(0o644);
    header.set_entry_type(tar::EntryType::Regular);
    header.set_cksum();
    tar.append(&header, &content[..]).unwrap();
    // Finalize both tar and gzip streams
    let gz = tar.into_inner().unwrap();
    gz.finish().unwrap();

    let result = inspectah_refine::tarball::from_tarball(&tarball_path);
    assert!(matches!(result, Err(RefineError::ArchiveSafety(_))));
}

#[test]
fn reject_missing_snapshot_json() {
    let dir = tempdir().unwrap();
    let tarball_path = dir.path().join("empty.tar.gz");
    let f = std::fs::File::create(&tarball_path).unwrap();
    let gz = flate2::write::GzEncoder::new(f, flate2::Compression::default());
    let mut tar = tar::Builder::new(gz);

    // Add a dummy file that isn't inspection-snapshot.json
    let content = b"not a snapshot";
    let mut header = tar::Header::new_ustar();
    header.set_path("readme.txt").unwrap();
    header.set_size(content.len() as u64);
    header.set_mode(0o644);
    header.set_entry_type(tar::EntryType::Regular);
    header.set_cksum();
    tar.append(&header, &content[..]).unwrap();
    // Finalize both tar and gzip streams
    let gz = tar.into_inner().unwrap();
    gz.finish().unwrap();

    let result = inspectah_refine::tarball::from_tarball(&tarball_path);
    assert!(matches!(result, Err(RefineError::SnapshotLoad(_))));
}

