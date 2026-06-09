//! Integration tests for the user/group renderers (kickstart, TOML, Containerfile).

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::users::UserGroupSection;
use inspectah_pipeline::render::users;

/// Build a test snapshot with the given users and groups.
fn snap_with(users: Vec<serde_json::Value>, groups: Vec<serde_json::Value>) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.users_groups = Some(UserGroupSection {
        users,
        groups,
        ..Default::default()
    });
    snap
}

fn test_user() -> serde_json::Value {
    serde_json::json!({
        "name": "alice",
        "uid": 1001,
        "gid": 1001,
        "home": "/home/alice",
        "shell": "/bin/bash",
        "include": true,
        "containerfile_strategy": "useradd",
        "supplementary_groups": ["wheel", "docker"],
        "password_hash": "$6$rounds=65536$salt$hash",
        "ssh_keys": ["ssh-rsa AAAA... alice@host"]
    })
}

fn test_group() -> serde_json::Value {
    serde_json::json!({
        "name": "docker",
        "gid": 1010,
        "source": "custom",
        "include": true
    })
}

// ============================================================
// Kickstart
// ============================================================

#[test]
fn kickstart_renders_group_before_user() {
    let snap = snap_with(vec![test_user()], vec![test_group()]);
    let ks = users::render_kickstart(&snap);

    let group_pos = ks.find("group --name=docker").expect("group line missing");
    let user_pos = ks.find("user --name=alice").expect("user line missing");
    assert!(
        group_pos < user_pos,
        "group must come before user in kickstart output"
    );

    // Correct flags
    assert!(ks.contains("--gid=1010"), "group gid flag missing");
    assert!(ks.contains("--uid=1001"), "user uid flag missing");
    assert!(ks.contains("--gid=1001"), "user gid flag missing");
    assert!(
        ks.contains("--groups=wheel,docker"),
        "supplementary groups missing"
    );
    assert!(
        ks.contains("--iscrypted --password="),
        "password flag missing"
    );
    assert!(
        ks.contains("sshkey --username=alice"),
        "sshkey line missing"
    );
}

// ============================================================
// Blueprint TOML
// ============================================================

#[test]
fn toml_renders_group_and_user_blocks() {
    let snap = snap_with(vec![test_user()], vec![test_group()]);
    let toml = users::render_blueprint_toml(&snap);

    assert!(
        toml.contains("[[customizations.group]]"),
        "group block missing"
    );
    assert!(
        toml.contains("[[customizations.user]]"),
        "user block missing"
    );
    assert!(toml.contains("name = \"docker\""), "group name missing");
    assert!(toml.contains("gid = 1010"), "group gid missing");
    assert!(toml.contains("name = \"alice\""), "user name missing");
    assert!(toml.contains("uid = 1001"), "user uid missing");

    // Order: group block before user block
    let g_pos = toml.find("[[customizations.group]]").unwrap();
    let u_pos = toml.find("[[customizations.user]]").unwrap();
    assert!(g_pos < u_pos, "group block must precede user block");
}

#[test]
fn toml_multi_key_uses_first_with_comment() {
    let user = serde_json::json!({
        "name": "bob",
        "uid": 1002,
        "gid": 1002,
        "include": true,
        "containerfile_strategy": "useradd",
        "ssh_keys": ["ssh-rsa KEY1 bob@a", "ssh-rsa KEY2 bob@b", "ssh-ed25519 KEY3 bob@c"]
    });
    let snap = snap_with(vec![user], vec![]);
    let toml = users::render_blueprint_toml(&snap);

    assert!(
        toml.contains("key = \"ssh-rsa KEY1 bob@a\""),
        "first key missing"
    );
    assert!(
        toml.contains("# NOTE: 2 additional SSH key(s)"),
        "multi-key comment missing"
    );
}

// ============================================================
// Containerfile
// ============================================================

#[test]
fn containerfile_useradd_with_groups_and_ssh() {
    let snap = snap_with(vec![test_user()], vec![test_group()]);
    let lines = users::render_containerfile_users(&snap);
    let output = lines.join("\n");

    // Groups created first
    assert!(
        output.contains("RUN groupadd -g 1010 docker"),
        "groupadd missing"
    );
    // useradd with flags
    assert!(
        output.contains("RUN useradd -m -u 1001 -g 1001 -G wheel,docker"),
        "useradd with flags missing"
    );
    // Password warning comment
    assert!(
        output.contains("WARNING: Embedding password hashes"),
        "password warning missing"
    );
    // chpasswd
    assert!(output.contains("chpasswd -e"), "chpasswd missing");
    // SSH staging
    assert!(
        output.contains("install -d -m 700"),
        "ssh dir install missing"
    );
    assert!(
        output.contains("COPY users/home/alice/.ssh/authorized_keys"),
        "COPY ssh missing"
    );
    assert!(
        output.contains("chown alice:1001"),
        "chown with gid missing"
    );

    // Ordering: groupadd before useradd
    let groupadd_pos = output.find("RUN groupadd").unwrap();
    let useradd_pos = output.find("RUN useradd -m").unwrap();
    assert!(groupadd_pos < useradd_pos, "groupadd must precede useradd");
}

#[test]
fn containerfile_skip_users_produces_empty() {
    let user = serde_json::json!({
        "name": "sysuser",
        "uid": 999,
        "gid": 999,
        "include": true,
        "containerfile_strategy": "skip"
    });
    let snap = snap_with(vec![user], vec![]);
    let lines = users::render_containerfile_users(&snap);
    assert!(
        lines.is_empty(),
        "skip strategy should produce no output, got: {:?}",
        lines
    );
}
