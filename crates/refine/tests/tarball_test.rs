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
fn accept_raw_redaction_state() {
    let dir = tempdir().unwrap();
    let snap_json = make_test_snapshot(Some(RedactionState::Raw));
    let tarball = write_flat_tarball(dir.path(), &snap_json);

    let session = inspectah_refine::tarball::from_tarball(&tarball).unwrap();
    assert_eq!(session.view().generation, 0);
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
fn accept_unknown_redaction_state_with_warning() {
    let dir = tempdir().unwrap();
    let snap_json = make_test_snapshot(Some(RedactionState::Unknown));
    let tarball = write_flat_tarball(dir.path(), &snap_json);

    // Now accepts with warning instead of rejecting
    let session = inspectah_refine::tarball::from_tarball(&tarball).unwrap();
    assert_eq!(session.view().generation, 0);
}

#[test]
fn accept_absent_redaction_state_with_warning() {
    let dir = tempdir().unwrap();
    let snap_json = make_test_snapshot(None);
    let tarball = write_flat_tarball(dir.path(), &snap_json);

    // Now accepts with warning instead of rejecting
    let session = inspectah_refine::tarball::from_tarball(&tarball).unwrap();
    assert_eq!(session.view().generation, 0);
}

#[test]
fn reject_parent_dir_traversal() {
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
fn reject_absolute_path() {
    let dir = tempdir().unwrap();
    let tarball_path = dir.path().join("evil-abs.tar.gz");
    let f = std::fs::File::create(&tarball_path).unwrap();
    let gz = flate2::write::GzEncoder::new(f, flate2::Compression::default());
    let mut tar = tar::Builder::new(gz);

    // Craft a tar entry whose path starts with "/" — set_path rejects
    // absolute paths, so write the bytes directly into the header.
    let content = b"malicious content";
    let mut header = tar::Header::new_ustar();
    {
        let raw = header.as_old_mut();
        let path_bytes = b"/etc/shadow";
        raw.name[..path_bytes.len()].copy_from_slice(path_bytes);
        raw.name[path_bytes.len()] = 0;
    }
    header.set_size(content.len() as u64);
    header.set_mode(0o644);
    header.set_entry_type(tar::EntryType::Regular);
    header.set_cksum();
    tar.append(&header, &content[..]).unwrap();
    let gz = tar.into_inner().unwrap();
    gz.finish().unwrap();

    let result = inspectah_refine::tarball::from_tarball(&tarball_path);
    match result {
        Err(RefineError::ArchiveSafety(msg)) => {
            assert!(
                msg.contains("absolute path"),
                "expected 'absolute path' in error, got: {msg}"
            );
        }
        Err(other) => panic!("expected ArchiveSafety, got: {other:?}"),
        Ok(_) => panic!("expected error for absolute path, got Ok"),
    }
}

#[test]
fn reject_old_schema_version() {
    let dir = tempdir().unwrap();
    // Construct a minimal snapshot with an old schema version (v12).
    // Only schema_version is required for deserialization — all other
    // fields default. The schema floor is now SCHEMA_VERSION (current-only),
    // so anything below it must be rejected at the tarball import boundary.
    let snap_json = r#"{"schema_version": 12}"#.to_string();
    let tarball = write_flat_tarball(dir.path(), &snap_json);

    let result = inspectah_refine::tarball::from_tarball(&tarball);
    match result {
        Err(RefineError::SnapshotLoad(msg)) => {
            assert!(
                msg.contains("unsupported schema version"),
                "expected 'unsupported schema version' in error, got: {msg}"
            );
        }
        Err(other) => panic!("expected SnapshotLoad, got: {other:?}"),
        Ok(_) => panic!("expected error for old schema version, got Ok"),
    }
}

/// A schema-25 snapshot predates the `content_not_captured` advisory: a
/// config file the scan could not read is in it as an included entry with
/// empty content, which an export would copy over the real file. Refine
/// refuses to open it rather than export from it.
#[test]
fn reject_schema_25_with_its_empty_failed_capture() {
    let dir = tempdir().unwrap();
    let snap_json = r#"{"schema_version": 25,
        "redaction_state": {"state": "fully_redacted", "redacted_by": "t", "config_hash": "h"},
        "config": {"files": [{"path": "/etc/httpd/conf.d/ssl.conf",
            "kind": "rpm_owned_modified", "category": "other", "content": "",
            "disposition": {"kind": "actionable", "include": true}}]}}"#;
    let tarball = write_flat_tarball(dir.path(), snap_json);

    match inspectah_refine::tarball::from_tarball(&tarball) {
        Err(RefineError::SnapshotLoad(msg)) => assert!(
            msg.contains("unsupported schema version: 25"),
            "expected the schema refusal, got: {msg}"
        ),
        Err(other) => panic!("expected SnapshotLoad, got: {other:?}"),
        Ok(_) => panic!("a schema-25 snapshot must not open in refine"),
    }
}

/// A schema-26 snapshot predates the sysctl precedence model: its
/// per-key sysctl overrides may name files the host shadowed. Refine
/// refuses to open it.
#[test]
fn schema_26_snapshot_is_refused() {
    let dir = tempdir().unwrap();
    let snap_json =
        include_str!("../../../testdata/fixtures/snapshot/schema-26-sysctl-overrides.json");
    let tarball = write_flat_tarball(dir.path(), snap_json);

    match inspectah_refine::tarball::from_tarball(&tarball) {
        Err(RefineError::SnapshotLoad(msg)) => assert!(
            msg.contains("unsupported schema version: 26"),
            "expected the schema refusal, got: {msg}"
        ),
        Err(other) => panic!("expected SnapshotLoad, got: {other:?}"),
        Ok(_) => panic!("a schema-26 snapshot must not open in refine"),
    }
}

/// A schema-27 snapshot holds services only, under `services`: every
/// socket and timer state the host had is absent from it with nothing to
/// say so. Refine refuses to open it. The same section under `units` at
/// the current version, its three unit name lists replaced by the one
/// inventory, opens, so the version gate is what refuses it.
#[test]
fn schema_27_snapshot_is_refused() {
    let schema_27 =
        include_str!("../../../testdata/fixtures/snapshot/schema-27-services-section.json");

    let mut current: serde_json::Value = serde_json::from_str(schema_27).unwrap();
    let object = current.as_object_mut().unwrap();
    let mut section = object.remove("services").expect("fixture has `services`");
    let lists = section.as_object_mut().unwrap();
    for list in ["enabled_units", "disabled_units", "preset_matched_units"] {
        lists.remove(list).expect("schema 27 has the list");
    }
    lists.insert(
        "inventory".into(),
        serde_json::json!([{"unit": "httpd.service", "state": "enabled", "preset": "disable"}]),
    );
    lists.insert("mount_findings".into(), serde_json::json!([]));
    lists.insert("manual_links".into(), serde_json::json!([]));
    lists.insert("undetermined_installs".into(), serde_json::json!([]));
    let change = &mut lists.get_mut("state_changes").unwrap()[0];
    let owner = change
        .as_object_mut()
        .unwrap()
        .remove("owning_package")
        .expect("schema 27 state change has an owner");
    change["unit"] = serde_json::json!({
        "name": "httpd.service", "fragment": "vendor", "file": "httpd.service",
        "owning_package": owner
    });
    object.insert("units".into(), section);
    object.insert(
        "schema_version".into(),
        inspectah_core::snapshot::SCHEMA_VERSION.into(),
    );
    let dir = tempdir().unwrap();
    let tarball = write_flat_tarball(dir.path(), &current.to_string());
    let session = inspectah_refine::tarball::from_tarball(&tarball)
        .expect("the same section under `units` opens at the current version");
    let units = session
        .snapshot()
        .units
        .as_ref()
        .expect("the `units` section is present");
    assert_eq!(
        units.state_changes[0].unit().name().as_str(),
        "httpd.service"
    );

    let dir = tempdir().unwrap();
    let tarball = write_flat_tarball(dir.path(), schema_27);
    match inspectah_refine::tarball::from_tarball(&tarball) {
        Err(RefineError::SnapshotLoad(msg)) => assert!(
            msg.contains("unsupported schema version: 27"),
            "expected the schema refusal, got: {msg}"
        ),
        Err(other) => panic!("expected SnapshotLoad, got: {other:?}"),
        Ok(_) => panic!("a schema-27 snapshot must not open in refine"),
    }
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

/// Write `entries` into a gzip tarball, one header per row, appending
/// each row's bytes as given. Nothing here checks that a header's size
/// matches its data: that mismatch is what a hostile archive is made of.
fn write_raw_tarball(
    dir: &std::path::Path,
    entries: &[(&str, tar::EntryType, &[u8], &str)],
) -> std::path::PathBuf {
    let tarball_path = dir.join("raw.tar.gz");
    let f = std::fs::File::create(&tarball_path).unwrap();
    let gz = flate2::write::GzEncoder::new(f, flate2::Compression::default());
    let mut tar = tar::Builder::new(gz);
    for (name, entry_type, data, link) in entries {
        let mut header = tar::Header::new_ustar();
        header.set_path(name).unwrap();
        header.set_entry_type(*entry_type);
        header.set_mode(0o644);
        header.set_size(data.len() as u64);
        if !link.is_empty() {
            header.set_link_name(link).unwrap();
        }
        header.set_cksum();
        tar.append(&header, *data).unwrap();
    }
    tar.finish().unwrap();
    tarball_path
}

/// One PAX extended-header record, `"<len> <key>=<value>\n"`, where
/// `<len>` counts the whole record including its own digits.
fn pax_record(key: &str, value: &str) -> Vec<u8> {
    let body = format!(" {key}={value}\n");
    let mut len = body.len() + 1;
    while format!("{len}{body}").len() != len {
        len += 1;
    }
    format!("{len}{body}").into_bytes()
}

#[test]
fn pax_size_override_past_the_limit_is_refused() {
    // The ustar header says 0 bytes; the PAX record says 1 GiB, and the
    // tar reader extracts the PAX size. The limits must count that one.
    const PAX_SIZE: &str = "1073741824";
    let dir = tempdir().unwrap();
    let record = pax_record("size", PAX_SIZE);
    let tarball = write_raw_tarball(
        dir.path(),
        &[
            ("PaxHeaders/big", tar::EntryType::XHeader, &record, ""),
            ("big", tar::EntryType::Regular, b"", ""),
        ],
    );
    match inspectah_refine::tarball::from_tarball(&tarball) {
        Err(RefineError::ArchiveSafety(msg)) => {
            assert!(msg.contains("exceeds"), "unexpected message: {msg}");
        }
        Err(other) => panic!("expected ArchiveSafety, got: {other:?}"),
        Ok(_) => panic!("a 1 GiB PAX entry was accepted"),
    }
}

#[test]
fn a_symlinked_snapshot_is_refused_without_reading_it() {
    let dir = tempdir().unwrap();
    // A valid snapshot outside the archive: following the link would load
    // it, so loading it at all is the failure.
    let outside = dir.path().join("outside.json");
    std::fs::write(
        &outside,
        make_test_snapshot(Some(RedactionState::FullyRedacted {
            redacted_by: "inspectah 0.8.0".into(),
            config_hash: "abc123".into(),
        })),
    )
    .unwrap();
    // `/dev/zero` never ends: reading it would allocate without bound.
    for target in [outside.to_str().unwrap(), "/dev/zero"] {
        for name in [
            "inspection-snapshot.json",
            "host-20260515-1430/inspection-snapshot.json",
        ] {
            let tarball =
                write_raw_tarball(dir.path(), &[(name, tar::EntryType::Symlink, b"", target)]);
            match inspectah_refine::tarball::from_tarball(&tarball) {
                Err(RefineError::ArchiveSafety(msg)) => {
                    assert!(msg.contains("regular file"), "{name} -> {target}: {msg}");
                }
                Err(other) => panic!("{name} -> {target}: expected ArchiveSafety, got: {other:?}"),
                Ok(_) => panic!("{name} -> {target}: a symlinked snapshot was loaded"),
            }
        }
    }
}

#[test]
fn an_unmanaged_symlink_elsewhere_in_the_archive_still_loads() {
    let dir = tempdir().unwrap();
    let snap = make_test_snapshot(Some(RedactionState::FullyRedacted {
        redacted_by: "inspectah 0.8.0".into(),
        config_hash: "abc123".into(),
    }));
    let tarball = write_raw_tarball(
        dir.path(),
        &[
            (
                "inspection-snapshot.json",
                tar::EntryType::Regular,
                snap.as_bytes(),
                "",
            ),
            (
                "unmanaged/etc/alt",
                tar::EntryType::Symlink,
                b"",
                "/etc/alternatives/x",
            ),
        ],
    );
    inspectah_refine::tarball::from_tarball(&tarball).expect("load");
}
