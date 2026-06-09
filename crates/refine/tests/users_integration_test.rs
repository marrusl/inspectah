//! End-to-end integration tests for user/group materialization pipeline.
//!
//! These tests exercise the full round-trip: snapshot construction,
//! refinement ops, sensitivity detection, tarball export, content
//! verification, and preview-export parity.

use std::collections::BTreeSet;
use std::path::Path;

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::redaction::RedactionState;
use inspectah_core::types::users::{UserContainerfileStrategy, UserGroupSection};
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::{RefinementOp, UserPasswordOp};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Collect all file entries from a tarball as a sorted set of paths.
/// Directories are excluded — only regular file paths.
fn tarball_file_set(tarball_path: &Path) -> BTreeSet<String> {
    let file = std::fs::File::open(tarball_path).unwrap();
    let gz = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);
    let mut files = BTreeSet::new();
    for entry in archive.entries().unwrap() {
        let entry = entry.unwrap();
        if entry.header().entry_type().is_file() {
            let path = entry.path().unwrap().to_string_lossy().into_owned();
            files.insert(path);
        }
    }
    files
}

/// Extract a specific file's content from a tarball as a String.
fn read_tarball_file(tarball_path: &Path, filename: &str) -> String {
    let file = std::fs::File::open(tarball_path).unwrap();
    let gz = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);
    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        let path = entry.path().unwrap().to_string_lossy().into_owned();
        if path == filename {
            let mut content = String::new();
            std::io::Read::read_to_string(&mut entry, &mut content).unwrap();
            return content;
        }
    }
    panic!("file '{filename}' not found in tarball");
}

/// Build a snapshot with alice (user), docker (group), SSH key, password hash,
/// and a valid redaction state so tarball import accepts it.
fn snapshot_with_alice() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot {
        schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
        sensitive_snapshot: true,
        rpm: Some(Default::default()),
        users_groups: Some(UserGroupSection {
            users: vec![serde_json::json!({
                "name": "alice",
                "uid": 1001,
                "gid": 1001,
                "include": true,
                "containerfile_strategy": "skip",
                "password_choice": "preserve",
                "password_hash": "$6$rounds=5000$saltsalt$hashhashhashhash",
                "home": "/home/alice",
                "shell": "/bin/bash",
                "ssh_keys": ["ssh-rsa AAAAB3testkey alice@host"],
                "source": "custom",
                "groups": ["docker"],
                "supplementary_groups": ["docker"]
            })],
            groups: vec![serde_json::json!({
                "name": "docker",
                "gid": 1010,
                "source": "custom",
                "include": true
            })],
            ..Default::default()
        }),
        ..Default::default()
    };
    snap.redaction_state = Some(RedactionState::FullyRedacted {
        redacted_by: "inspectah 0.8.0".into(),
        config_hash: "abc123".into(),
    });
    snap
}

// ---------------------------------------------------------------------------
// Test 1: Full pipeline — users/groups materialization
// ---------------------------------------------------------------------------

#[test]
fn full_pipeline_users_groups_materialization() {
    // 1. Create snapshot with users, groups, SSH keys, password hash
    let snap = snapshot_with_alice();
    let mut session = RefineSession::new(snap);

    // 2. Apply UserStrategy::Useradd for alice
    session
        .apply(RefinementOp::UserStrategy {
            username: "alice".into(),
            strategy: UserContainerfileStrategy::Useradd,
        })
        .unwrap();

    // 3. Verify is_sensitive returns true (password hash with preserve choice)
    //    After applying useradd strategy, the password_choice stays "preserve"
    //    and password_hash is non-empty — but is_sensitive checks for
    //    password_choice == "new". So set a new password to trigger sensitivity.
    session
        .apply(RefinementOp::UserPassword(
            inspectah_refine::types::UserPasswordOp::New {
                username: "alice".into(),
                hash: Some("$6$rounds=5000$newsalt$newhash".into()),
            },
        ))
        .unwrap();

    assert!(
        session.is_sensitive(),
        "session must be sensitive when a user has password_choice='new' with a hash"
    );

    // 4. Export tarball
    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("output.tar.gz");
    session
        .export_tarball(&tarball_path, session.generation())
        .unwrap();

    // 5. Verify tarball contains expected user artifacts
    let files = tarball_file_set(&tarball_path);

    assert!(
        files.contains("inspectah-users.ks"),
        "tarball must contain inspectah-users.ks, got: {files:?}"
    );
    assert!(
        files.contains("inspectah-users.toml"),
        "tarball must contain inspectah-users.toml, got: {files:?}"
    );
    assert!(
        files.contains("users/home/alice/.ssh/authorized_keys"),
        "tarball must contain users/home/alice/.ssh/authorized_keys, got: {files:?}"
    );
    assert!(
        files.contains("Containerfile"),
        "tarball must contain Containerfile, got: {files:?}"
    );

    // 6. Verify Containerfile content
    let containerfile = read_tarball_file(&tarball_path, "Containerfile");

    assert!(
        containerfile.contains("groupadd"),
        "Containerfile must contain groupadd, got:\n{containerfile}"
    );
    assert!(
        containerfile.contains("useradd"),
        "Containerfile must contain useradd, got:\n{containerfile}"
    );
    assert!(
        containerfile.contains("chpasswd -e"),
        "Containerfile must contain chpasswd -e, got:\n{containerfile}"
    );
    assert!(
        containerfile.contains("COPY users/home/alice/.ssh/authorized_keys"),
        "Containerfile must contain COPY for SSH keys, got:\n{containerfile}"
    );
    assert!(
        containerfile.contains("install -d -m 700"),
        "Containerfile must contain install -d -m 700 for .ssh dir, got:\n{containerfile}"
    );

    // 7. Verify KS has groups before users
    let ks = read_tarball_file(&tarball_path, "inspectah-users.ks");
    let group_pos = ks
        .find("group --name=docker")
        .expect("KS must contain group directive");
    let user_pos = ks
        .find("user --name=alice")
        .expect("KS must contain user directive");
    assert!(
        group_pos < user_pos,
        "KS must render groups before users (group at {group_pos}, user at {user_pos})"
    );

    // 8. Verify re-import preserves projected containerfile_strategy
    let session2 = inspectah_refine::tarball::from_tarball(&tarball_path).unwrap();
    let projected = session2.snapshot_projected();
    let user = projected
        .users_groups
        .as_ref()
        .expect("re-imported snapshot must have users_groups")
        .users
        .iter()
        .find(|u| u.get("name").and_then(|v| v.as_str()) == Some("alice"))
        .expect("re-imported snapshot must contain alice");

    assert_eq!(
        user.get("containerfile_strategy").and_then(|v| v.as_str()),
        Some("useradd"),
        "re-imported snapshot must preserve containerfile_strategy=useradd"
    );
}

// ---------------------------------------------------------------------------
// Test 2: Preview-export parity for user artifacts
// ---------------------------------------------------------------------------

#[test]
fn preview_export_parity_for_user_artifacts() {
    // 1. Create snapshot and apply useradd
    let snap = snapshot_with_alice();
    let mut session = RefineSession::new(snap);

    session
        .apply(RefinementOp::UserStrategy {
            username: "alice".into(),
            strategy: UserContainerfileStrategy::Useradd,
        })
        .unwrap();

    // 2. Read the LIVE preview seam — containerfile_preview from view()
    let preview_containerfile = session.view().containerfile_preview.clone();

    // 3. Render KS and TOML from projected snapshot (same as /api/user-preview)
    let projected = session.snapshot_projected();
    let preview_ks = inspectah_pipeline::render::users::render_kickstart(&projected);
    let preview_toml = inspectah_pipeline::render::users::render_blueprint_toml(&projected);

    // 4. Export tarball
    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("output.tar.gz");
    session
        .export_tarball(&tarball_path, session.generation())
        .unwrap();

    // 5. Read the same files from the tarball
    let export_containerfile = read_tarball_file(&tarball_path, "Containerfile");
    let export_ks = read_tarball_file(&tarball_path, "inspectah-users.ks");
    let export_toml = read_tarball_file(&tarball_path, "inspectah-users.toml");

    // 6. Assert byte-equality between preview and export
    assert_eq!(
        preview_containerfile, export_containerfile,
        "Containerfile preview and export must be byte-identical"
    );
    assert_eq!(
        preview_ks, export_ks,
        "Kickstart preview and export must be byte-identical"
    );
    assert_eq!(
        preview_toml, export_toml,
        "Blueprint TOML preview and export must be byte-identical"
    );
}

// ---------------------------------------------------------------------------
// Test 3: Collector-shaped snapshot preserves trust cues through projection
// ---------------------------------------------------------------------------

#[test]
fn collector_shaped_snapshot_preserves_trust_cues() {
    // Build a snapshot matching real collector output shape with all enrichment fields
    let mut snap = InspectionSnapshot::new();
    snap.users_groups = Some(UserGroupSection {
        users: vec![serde_json::json!({
            "name": "alice", "uid": 1000, "gid": 1000,
            "shell": "/bin/bash", "home": "/home/alice",
            "classification": "interactive",
            "classification_rationale": "bash shell, home at /home/alice, password set, has sudo, 1 SSH key, member of wheel",
            "password_status": "password_set",
            "has_sudo": true,
            "has_subuid": true,
            "ssh_key_count": 1,
            "supplementary_groups": ["wheel", "docker"],
            "containerfile_strategy": "skip",
            "password_choice": "none"
        })],
        groups: vec![serde_json::json!({"name": "alice", "gid": 1000, "source": "custom"})],
        ..Default::default()
    });

    let session = RefineSession::new(snap);
    let projected = session.snapshot_projected();
    let user = &projected.users_groups.as_ref().unwrap().users[0];

    // All trust-cue fields must survive projection
    assert_eq!(user["classification"], "interactive");
    assert!(
        user["classification_rationale"]
            .as_str()
            .unwrap()
            .contains("bash shell")
    );
    assert_eq!(user["has_sudo"], true);
    assert_eq!(user["has_subuid"], true);
    assert_eq!(user["ssh_key_count"], 1);
    assert_eq!(user["password_status"], "password_set");
    assert_eq!(user["containerfile_strategy"], "skip");
    assert_eq!(user["password_choice"], "none");
    let groups = user["supplementary_groups"].as_array().unwrap();
    assert!(groups.iter().any(|g| g == "wheel"));
}

// ---------------------------------------------------------------------------
// Test 4: Refine-time sensitivity upgrades redaction state
// ---------------------------------------------------------------------------

#[test]
fn refine_time_sensitivity_upgrades_redaction_state() {
    let mut snap = InspectionSnapshot::new();
    snap.redaction_state = Some(RedactionState::FullyRedacted {
        redacted_by: "inspectah 0.8.0".into(),
        config_hash: "abc".into(),
    });
    snap.users_groups = Some(UserGroupSection {
        users: vec![serde_json::json!({"name": "alice", "uid": 1000})],
        ..Default::default()
    });

    let mut session = RefineSession::new(snap);
    session
        .apply(RefinementOp::UserPassword(UserPasswordOp::New {
            username: "alice".into(),
            hash: Some("$6$salt$hash".into()),
        }))
        .unwrap();

    let projected = session.snapshot_projected();
    assert!(projected.sensitive_snapshot, "projected must be sensitive");
    assert!(
        matches!(
            projected.redaction_state,
            Some(RedactionState::SensitiveRetained { .. })
        ),
        "redaction_state must upgrade to SensitiveRetained"
    );
}

// ---------------------------------------------------------------------------
// Test 5: is_sensitive detects new password on projected state
// ---------------------------------------------------------------------------

#[test]
fn is_sensitive_detects_new_password() {
    let mut snap = InspectionSnapshot::new();
    snap.users_groups = Some(UserGroupSection {
        users: vec![serde_json::json!({"name": "bob", "uid": 1001})],
        ..Default::default()
    });

    let mut session = RefineSession::new(snap);
    assert!(
        !session.is_sensitive(),
        "session must not be sensitive before any password ops"
    );

    session
        .apply(RefinementOp::UserPassword(UserPasswordOp::New {
            username: "bob".into(),
            hash: Some("$6$rounds=5000$salt$hash".into()),
        }))
        .unwrap();

    assert!(
        session.is_sensitive(),
        "session must be sensitive after setting a new password"
    );
}
