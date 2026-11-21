use inspectah_core::baseline::{BaselineData, BaselinePackageEntry};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::FindingKind;
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
        ..Default::default()
    });
    // Provide baseline data so normalize_package_defaults classifies both
    // packages as Baseline (include=true) rather than Investigate
    // (include=false), keeping exclude ops effective.
    let mut packages = std::collections::HashMap::new();
    packages.insert(
        "httpd".into(),
        BaselinePackageEntry {
            name: "httpd".into(),
            epoch: None,
            version: "2.4.57".into(),
            release: "5.el9".into(),
            arch: "x86_64".into(),
        },
    );
    packages.insert(
        "glibc".into(),
        BaselinePackageEntry {
            name: "glibc".into(),
            epoch: None,
            version: "2.34".into(),
            release: "83.el9".into(),
            arch: "x86_64".into(),
        },
    );
    snap.baseline = Some(BaselineData {
        image_digest: "sha256:test".into(),
        packages,
        extracted_at: "2024-01-01T00:00:00Z".into(),
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
    assert_eq!(state.timeline.len(), 1);

    session.undo().unwrap();

    // After undo, cursor should be 0 but ops still present
    let state = load_session(&tarball).unwrap().unwrap();
    assert_eq!(state.cursor, 0);
    assert_eq!(state.timeline.len(), 1);
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
    assert_eq!(state.timeline.len(), 1);
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
    assert_eq!(state.timeline.len(), 2);
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
    assert_eq!(state.schema_version, 3);
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
    assert_eq!(state.timeline.len(), 3, "sidecar must have all 3 ops");
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
        state.timeline.len(),
        3,
        "sidecar after resume must still have all 3 ops (redo tail preserved)"
    );
    assert_eq!(state.cursor, 2, "sidecar cursor must remain at 2");
}

// ── Saved sessions naming the retired `Service` kind ─────────────────

const STATE_ROW_UNIT: &str = "httpd.service";
const SHADOW_ONLY_UNIT: &str = "sshd.service";

/// A snapshot with one unit state change and one unit whose only finding
/// is a full shadow (no state row).
fn units_snapshot() -> InspectionSnapshot {
    use inspectah_core::types::ShadowType;
    use inspectah_core::types::units::{
        PresetDefault, SystemdDropIn, UnitName, UnitSection, UnitState, UnitStateChange,
    };

    let mut snap = test_snapshot_redacted();
    snap.units = Some(UnitSection {
        state_changes: vec![
            UnitStateChange::new(
                test_carried_unit(
                    UnitName::try_from(STATE_ROW_UNIT.to_string()).unwrap(),
                    None,
                ),
                UnitState::Enabled,
                Some(PresetDefault::Disable),
            )
            .unwrap(),
        ],
        inventory: vec![],
        drop_ins: vec![SystemdDropIn {
            unit: SHADOW_ONLY_UNIT.into(),
            path: format!("/etc/systemd/system/{SHADOW_ONLY_UNIT}"),
            content: "[Service]\nExecStart=/usr/sbin/sshd -D\n".into(),
            disposition: FindingKind::included(),
            shadow_type: Some(ShadowType::FullShadow),
            ..Default::default()
        }],
        mount_findings: Vec::new(),
        manual_links: Vec::new(),
        undetermined_installs: Vec::new(),
        default_target: None,
        enabled_on_any_host: Vec::new(),
    });
    snap
}

/// Replace every occurrence of `from` inside `value` with `to`.
fn replace_value(value: &mut serde_json::Value, from: &serde_json::Value, to: &serde_json::Value) {
    if value == from {
        *value = to.clone();
        return;
    }
    match value {
        serde_json::Value::Array(items) => {
            items.iter_mut().for_each(|v| replace_value(v, from, to));
        }
        serde_json::Value::Object(map) => {
            map.values_mut().for_each(|v| replace_value(v, from, to));
        }
        _ => {}
    }
}

/// Write a v3 session file for `tarball` holding one exclude op whose item
/// id is `item_id`, given in its wire form so a retired kind can be written.
fn write_session_excluding(tarball: &std::path::Path, item_id: serde_json::Value) {
    use inspectah_refine::autosave::{SessionState, compute_tarball_hash};
    use inspectah_refine::types::TimelineEntry;

    let placeholder = ItemId::Package {
        name: "placeholder".into(),
        arch: "noarch".into(),
    };
    let state = SessionState {
        schema_version: 3,
        tarball_path: tarball.to_path_buf(),
        tarball_hash: compute_tarball_hash(tarball).unwrap(),
        timeline: vec![TimelineEntry::Op(RefinementOp::SetInclude {
            item_id: placeholder.clone(),
            include: false,
        })],
        cursor: 1,
        saved_at: "0s".into(),
    };
    let mut json = serde_json::to_value(&state).unwrap();
    let from = serde_json::to_value(&placeholder).unwrap();
    replace_value(&mut json, &from, &item_id);
    assert!(
        json.to_string().contains(&item_id.to_string()),
        "the session file must hold the item id it was asked to write"
    );
    std::fs::write(session_file_path(tarball), json.to_string()).unwrap();
}

/// A session saved before the rename holds `{"kind":"Service",...}`.
/// Resuming it is refused, and the refusal names the kind; nothing is
/// migrated or pruned. The unit here has no state row (its only finding is
/// a full shadow), the case whose old synthetic row is gone. The same op
/// under the `Unit` kind, for a unit with a state row, is the positive
/// control.
#[test]
fn a_saved_session_naming_the_service_kind_is_refused_on_resume() {
    let dir = tempfile::tempdir().unwrap();
    let tarball = dir.path().join("test.tar.gz");
    write_real_tarball(&tarball, &units_snapshot());

    write_session_excluding(
        &tarball,
        serde_json::json!({"kind": "Service", "key": {"unit": SHADOW_ONLY_UNIT}}),
    );
    match RefineSession::resume_from(&tarball) {
        Err(RefineError::SnapshotLoad(msg)) => assert!(
            msg.contains("`Service`"),
            "the refusal must name the refused kind, got: {msg}"
        ),
        Err(other) => panic!("expected a SnapshotLoad refusal, got: {other}"),
        Ok(_) => panic!("a saved `Service` op must not resume"),
    }

    // Positive control: a saved `Unit` op resumes and applies.
    write_session_excluding(
        &tarball,
        serde_json::json!({"kind": "Unit", "key": {"unit": STATE_ROW_UNIT}}),
    );
    let resumed = RefineSession::resume_from(&tarball)
        .expect("a saved `Unit` op must resume")
        .expect("the session file exists");
    let units = resumed.snapshot_projected().units.unwrap();
    let row = units
        .state_changes
        .iter()
        .find(|s| s.unit().name().as_str() == STATE_ROW_UNIT)
        .expect("the state row is present");
    assert!(
        !row.disposition.is_included(),
        "the resumed exclude must apply to {STATE_ROW_UNIT}"
    );
}

/// A carried unit from the vendor directory, owned as given.
fn test_carried_unit(
    name: inspectah_core::types::units::UnitName,
    owning_package: Option<String>,
) -> inspectah_core::types::units::CarriedUnit {
    use inspectah_core::types::units::{CarriedUnit, UnitFragment, UnitSubject};
    let fragment = UnitFragment::Vendor {
        file: name.clone(),
        owning_package,
    };
    CarriedUnit::try_from(UnitSubject::new(name, fragment).expect("file is the unit"))
        .expect("a test unit is carried")
}
