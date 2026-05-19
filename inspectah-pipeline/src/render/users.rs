//! User and group renderers — produces kickstart, blueprint TOML, and
//! Containerfile fragments from the enriched users/groups data.

use inspectah_core::snapshot::InspectionSnapshot;
use std::io;
use std::path::Path;

use super::safety::sanitize_shell_value;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract a string field from a serde_json::Value.
fn str_field<'a>(v: &'a serde_json::Value, key: &str) -> &'a str {
    v.get(key).and_then(|x| x.as_str()).unwrap_or("")
}

/// Extract a u32 numeric field from a serde_json::Value (JSON numbers are f64).
fn uid_field(v: &serde_json::Value, key: &str) -> u32 {
    v.get(key).and_then(|x| x.as_f64()).unwrap_or(0.0) as u32
}

/// Collect the `supplementary_groups` array as Vec<String>.
fn supp_groups(v: &serde_json::Value) -> Vec<String> {
    v.get("supplementary_groups")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|g| g.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Collect the `ssh_keys` array as Vec<String>.
fn ssh_keys(v: &serde_json::Value) -> Vec<String> {
    v.get("ssh_keys")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|k| k.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Return only custom groups (source == "custom") that have `include` true.
fn custom_groups(snap: &InspectionSnapshot) -> Vec<&serde_json::Value> {
    let ug = match &snap.users_groups {
        Some(u) => u,
        None => return Vec::new(),
    };
    ug.groups
        .iter()
        .filter(|g| {
            let src = str_field(g, "source");
            let include = g.get("include").and_then(|v| v.as_bool()).unwrap_or(true);
            src == "custom" && include
        })
        .collect()
}

/// Return users that are included (include != false).
fn included_users(snap: &InspectionSnapshot) -> Vec<&serde_json::Value> {
    let ug = match &snap.users_groups {
        Some(u) => u,
        None => return Vec::new(),
    };
    ug.users
        .iter()
        .filter(|u| u.get("include").and_then(|v| v.as_bool()).unwrap_or(true))
        .collect()
}

// ---------------------------------------------------------------------------
// 1. Kickstart renderer
// ---------------------------------------------------------------------------

/// Render a kickstart fragment for user/group provisioning.
///
/// Emits custom groups first, then user directives with `--uid`, `--gid`,
/// `--groups`, and conditional `--iscrypted --password=` and `sshkey` lines.
pub fn render_kickstart(snap: &InspectionSnapshot) -> String {
    let mut lines: Vec<String> = Vec::new();

    let groups = custom_groups(snap);
    let users = included_users(snap);

    if groups.is_empty() && users.is_empty() {
        return String::new();
    }

    // Groups first
    for g in &groups {
        let name = str_field(g, "name");
        let gid = uid_field(g, "gid");
        if name.is_empty() {
            continue;
        }
        if gid > 0 {
            lines.push(format!("group --name={name} --gid={gid}"));
        } else {
            lines.push(format!("group --name={name}"));
        }
    }

    // Users
    for u in &users {
        let name = str_field(u, "name");
        if name.is_empty() {
            continue;
        }
        let uid = uid_field(u, "uid");
        let gid = uid_field(u, "gid");
        let groups_list = supp_groups(u);

        let shell = str_field(u, "shell");
        let home = str_field(u, "home");

        let mut opts = format!("user --name={name}");
        if uid > 0 {
            opts.push_str(&format!(" --uid={uid}"));
        }
        if gid > 0 {
            opts.push_str(&format!(" --gid={gid}"));
        }
        if !shell.is_empty() {
            opts.push_str(&format!(" --shell={shell}"));
        }
        if !home.is_empty() {
            opts.push_str(&format!(" --homedir={home}"));
        }
        if !groups_list.is_empty() {
            opts.push_str(&format!(" --groups={}", groups_list.join(",")));
        }

        // Password (conditional)
        let pw = str_field(u, "password_hash");
        if !pw.is_empty() {
            opts.push_str(&format!(" --iscrypted --password={pw}"));
        }

        lines.push(opts);

        // SSH keys
        let keys = ssh_keys(u);
        for key in &keys {
            lines.push(format!("sshkey --username={name} \"{key}\""));
        }
    }

    lines.join("\n")
}

// ---------------------------------------------------------------------------
// 2. Blueprint TOML renderer
// ---------------------------------------------------------------------------

/// Render a blueprint TOML fragment for user/group provisioning.
///
/// Emits `[[customizations.group]]` blocks, then `[[customizations.user]]` blocks.
/// Single `key` field for the first SSH key; emits a comment when > 1 key.
pub fn render_blueprint_toml(snap: &InspectionSnapshot) -> String {
    let mut lines: Vec<String> = Vec::new();

    let groups = custom_groups(snap);
    let users = included_users(snap);

    if groups.is_empty() && users.is_empty() {
        return String::new();
    }

    // Groups
    for g in &groups {
        let name = str_field(g, "name");
        let gid = uid_field(g, "gid");
        if name.is_empty() {
            continue;
        }
        lines.push("[[customizations.group]]".into());
        lines.push(format!("name = \"{name}\""));
        if gid > 0 {
            lines.push(format!("gid = {gid}"));
        }
        lines.push(String::new());
    }

    // Users
    for u in &users {
        let name = str_field(u, "name");
        if name.is_empty() {
            continue;
        }
        let uid = uid_field(u, "uid");
        let gid = uid_field(u, "gid");
        let groups_list = supp_groups(u);
        let home = str_field(u, "home");
        let shell = str_field(u, "shell");

        lines.push("[[customizations.user]]".into());
        lines.push(format!("name = \"{name}\""));
        if uid > 0 {
            lines.push(format!("uid = {uid}"));
        }
        if gid > 0 {
            lines.push(format!("gid = {gid}"));
        }
        if !home.is_empty() {
            lines.push(format!("home = \"{home}\""));
        }
        if !shell.is_empty() {
            lines.push(format!("shell = \"{shell}\""));
        }
        if !groups_list.is_empty() {
            lines.push(format!(
                "groups = [{}]",
                groups_list
                    .iter()
                    .map(|g| format!("\"{g}\""))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }

        // Password
        let pw = str_field(u, "password_hash");
        if !pw.is_empty() {
            lines.push(format!("password = \"{pw}\""));
        }

        // SSH key — blueprint supports a single `key` field
        let keys = ssh_keys(u);
        if let Some(first_key) = keys.first() {
            lines.push(format!("key = \"{first_key}\""));
            if keys.len() > 1 {
                lines.push(format!(
                    "# NOTE: {} additional SSH key(s) cannot be expressed in blueprint TOML (single key only)",
                    keys.len() - 1
                ));
            }
        }

        lines.push(String::new());
    }

    lines.join("\n")
}

// ---------------------------------------------------------------------------
// 3. Containerfile users renderer
// ---------------------------------------------------------------------------

/// Render Containerfile directives for users where `containerfile_strategy == "useradd"`.
///
/// Order: custom groups (primary + supplementary) -> useradd -> chpasswd -> SSH key staging.
/// Uses actual GID for `.ssh` ownership.
pub fn render_containerfile_users(snap: &InspectionSnapshot) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();

    let users = included_users(snap);
    let useradd_users: Vec<_> = users
        .iter()
        .filter(|u| {
            let strategy = str_field(u, "containerfile_strategy");
            strategy == "useradd"
        })
        .collect();

    if useradd_users.is_empty() {
        return lines;
    }

    lines.push("# === Users and Groups ===".into());

    // Collect GIDs and supplementary group names needed by useradd users
    let mut needed_gids: std::collections::HashSet<u32> = std::collections::HashSet::new();
    let mut needed_supp_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    for u in &useradd_users {
        let gid = uid_field(u, "gid");
        if gid > 0 {
            needed_gids.insert(gid);
        }
        for g in supp_groups(u) {
            needed_supp_names.insert(g);
        }
    }

    // Only emit groupadd for custom groups that are either:
    // - The primary group (by GID) of a useradd user, OR
    // - A supplementary group (by name) of a useradd user
    let all_custom_groups = custom_groups(snap);
    for g in &all_custom_groups {
        let name = str_field(g, "name");
        let gid = uid_field(g, "gid");
        if name.is_empty() || sanitize_shell_value(name).is_none() {
            continue;
        }
        let is_primary = gid > 0 && needed_gids.contains(&gid);
        let is_supplementary = needed_supp_names.contains(name);
        if !is_primary && !is_supplementary {
            continue;
        }
        if gid > 0 {
            lines.push(format!("RUN groupadd -g {gid} {name}"));
        } else {
            lines.push(format!("RUN groupadd {name}"));
        }
    }

    // useradd lines
    for u in &useradd_users {
        let name = str_field(u, "name");
        if name.is_empty() || sanitize_shell_value(name).is_none() {
            continue;
        }
        let uid = uid_field(u, "uid");
        let gid = uid_field(u, "gid");
        let groups_list = supp_groups(u);
        let home = str_field(u, "home");
        let shell = str_field(u, "shell");

        let mut cmd = "RUN useradd -m".to_string();
        if uid > 0 {
            cmd.push_str(&format!(" -u {uid}"));
        }
        if gid > 0 {
            cmd.push_str(&format!(" -g {gid}"));
        }
        if !groups_list.is_empty() {
            cmd.push_str(&format!(" -G {}", groups_list.join(",")));
        }
        if !shell.is_empty() {
            cmd.push_str(&format!(" -s {shell}"));
        }
        if !home.is_empty() {
            cmd.push_str(&format!(" -d {home}"));
        }
        cmd.push_str(&format!(" {name}"));
        lines.push(cmd);
    }

    // chpasswd for users with password hashes
    let pw_users: Vec<_> = useradd_users
        .iter()
        .filter(|u| !str_field(u, "password_hash").is_empty())
        .collect();
    if !pw_users.is_empty() {
        lines.push(
            "# WARNING: Embedding password hashes in a Containerfile is a security risk.".into(),
        );
        lines.push(
            "# Consider using a secrets manager or deploy-time provisioning instead.".into(),
        );
        let mut chpasswd_entries: Vec<String> = Vec::new();
        for u in &pw_users {
            let name = str_field(u, "name");
            let pw = str_field(u, "password_hash");
            if sanitize_shell_value(name).is_some() {
                chpasswd_entries.push(format!("{name}:{pw}"));
            }
        }
        if !chpasswd_entries.is_empty() {
            lines.push(format!(
                "RUN echo '{}' | chpasswd -e",
                chpasswd_entries.join("\\n")
            ));
        }
    }

    // SSH key staging via COPY
    for u in &useradd_users {
        let name = str_field(u, "name");
        let keys = ssh_keys(u);
        if keys.is_empty() || sanitize_shell_value(name).is_none() {
            continue;
        }
        let uid = uid_field(u, "uid");
        let gid = uid_field(u, "gid");
        let home = str_field(u, "home");
        let home = if home.is_empty() {
            format!("/home/{name}")
        } else {
            home.to_string()
        };

        let ownership = if gid > 0 {
            format!("{name}:{gid}")
        } else {
            format!("{name}:{uid}")
        };

        lines.push(format!(
            "RUN install -d -m 700 -o {name} -g {gid} {home}/.ssh"
        ));
        lines.push(format!(
            "COPY users/home/{name}/.ssh/authorized_keys {home}/.ssh/authorized_keys"
        ));
        lines.push(format!(
            "RUN chown {ownership} {home}/.ssh/authorized_keys && chmod 600 {home}/.ssh/authorized_keys"
        ));
    }

    lines.push(String::new());
    lines
}

// ---------------------------------------------------------------------------
// 4. SSH key staging
// ---------------------------------------------------------------------------

/// Write generated `authorized_keys` files under `users/home/{name}/.ssh/`
/// in the output directory for users that have SSH keys.
pub fn stage_ssh_keys(snap: &InspectionSnapshot, output_dir: &Path) -> io::Result<()> {
    let users = included_users(snap);

    for u in &users {
        let name = str_field(u, "name");
        let keys = ssh_keys(u);
        if name.is_empty() || keys.is_empty() {
            continue;
        }

        let ssh_dir = output_dir.join(format!("users/home/{name}/.ssh"));
        std::fs::create_dir_all(&ssh_dir)?;

        let content = keys.join("\n") + "\n";
        std::fs::write(ssh_dir.join("authorized_keys"), content)?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::snapshot::InspectionSnapshot;
    use inspectah_core::types::users::UserGroupSection;

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

    // ----- Kickstart tests -----

    #[test]
    fn kickstart_renders_group_before_user() {
        let snap = snap_with(vec![test_user()], vec![test_group()]);
        let ks = render_kickstart(&snap);

        let group_pos = ks.find("group --name=docker").expect("group line missing");
        let user_pos = ks.find("user --name=alice").expect("user line missing");
        assert!(
            group_pos < user_pos,
            "group must come before user in kickstart"
        );

        // Verify flags
        assert!(ks.contains("--gid=1010"), "group gid missing");
        assert!(ks.contains("--uid=1001"), "user uid missing");
        assert!(ks.contains("--gid=1001"), "user gid missing");
        assert!(ks.contains("--groups=wheel,docker"), "supplementary groups missing");
        assert!(ks.contains("--iscrypted --password="), "password flag missing");
        assert!(ks.contains("sshkey --username=alice"), "sshkey line missing");
    }

    #[test]
    fn kickstart_includes_shell_and_homedir() {
        let mut snap = InspectionSnapshot::new();
        snap.users_groups = Some(UserGroupSection {
            users: vec![serde_json::json!({
                "name": "alice", "uid": 1000, "gid": 1000,
                "shell": "/bin/bash", "home": "/home/alice",
                "supplementary_groups": []
            })],
            ..Default::default()
        });
        let ks = render_kickstart(&snap);
        assert!(ks.contains("--shell=/bin/bash"), "missing --shell: {ks}");
        assert!(ks.contains("--homedir=/home/alice"), "missing --homedir: {ks}");
    }

    #[test]
    fn kickstart_empty_when_no_users() {
        let snap = InspectionSnapshot::new();
        assert!(render_kickstart(&snap).is_empty());
    }

    // ----- TOML tests -----

    #[test]
    fn toml_renders_group_and_user_blocks() {
        let snap = snap_with(vec![test_user()], vec![test_group()]);
        let toml = render_blueprint_toml(&snap);

        assert!(toml.contains("[[customizations.group]]"), "group block missing");
        assert!(toml.contains("[[customizations.user]]"), "user block missing");
        assert!(toml.contains("name = \"docker\""), "group name missing");
        assert!(toml.contains("gid = 1010"), "group gid missing");
        assert!(toml.contains("name = \"alice\""), "user name missing");
        assert!(toml.contains("uid = 1001"), "user uid missing");

        // Group block before user block
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
        let toml = render_blueprint_toml(&snap);

        assert!(toml.contains("key = \"ssh-rsa KEY1 bob@a\""), "first key missing");
        assert!(
            toml.contains("# NOTE: 2 additional SSH key(s)"),
            "multi-key comment missing"
        );
    }

    #[test]
    fn toml_empty_when_no_users() {
        let snap = InspectionSnapshot::new();
        assert!(render_blueprint_toml(&snap).is_empty());
    }

    // ----- Containerfile tests -----

    #[test]
    fn containerfile_useradd_with_groups_and_ssh() {
        let snap = snap_with(vec![test_user()], vec![test_group()]);
        let lines = render_containerfile_users(&snap);
        let output = lines.join("\n");

        // Groups come first
        assert!(output.contains("RUN groupadd -g 1010 docker"), "groupadd missing");
        // useradd with flags
        assert!(output.contains("RUN useradd -m -u 1001 -g 1001 -G wheel,docker"), "useradd missing");
        // Password warning
        assert!(output.contains("WARNING: Embedding password hashes"), "password warning missing");
        // chpasswd
        assert!(output.contains("chpasswd -e"), "chpasswd missing");
        // SSH staging
        assert!(output.contains("install -d -m 700"), "ssh dir install missing");
        assert!(output.contains("COPY users/home/alice/.ssh/authorized_keys"), "COPY ssh missing");
        assert!(output.contains("chown alice:1001"), "chown with gid missing");

        // Order: groupadd before useradd
        let groupadd_pos = output.find("RUN groupadd").unwrap();
        let useradd_pos = output.find("RUN useradd").unwrap();
        assert!(groupadd_pos < useradd_pos, "groupadd must precede useradd");
    }

    #[test]
    fn containerfile_useradd_creates_home_without_ssh_keys() {
        let mut snap = InspectionSnapshot::new();
        snap.users_groups = Some(UserGroupSection {
            users: vec![serde_json::json!({
                "name": "bob", "uid": 1001, "gid": 1001,
                "shell": "/bin/bash", "home": "/home/bob",
                "containerfile_strategy": "useradd",
                "supplementary_groups": []
            })],
            groups: vec![serde_json::json!({"name": "bob", "gid": 1001, "source": "custom"})],
            ..Default::default()
        });
        let cf = render_containerfile_users(&snap);
        let output = cf.join("\n");
        assert!(output.contains("-m"), "useradd must include -m to create home dir: {output}");
        assert!(output.contains("useradd"), "must contain useradd: {output}");
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
        let lines = render_containerfile_users(&snap);
        assert!(lines.is_empty(), "skip strategy should produce no output");
    }

    #[test]
    fn containerfile_no_users_groups_produces_empty() {
        let snap = InspectionSnapshot::new();
        let lines = render_containerfile_users(&snap);
        assert!(lines.is_empty());
    }

    #[test]
    fn containerfile_only_emits_groups_for_useradd_users() {
        let mut snap = InspectionSnapshot::new();
        snap.users_groups = Some(UserGroupSection {
            users: vec![
                serde_json::json!({
                    "name": "alice", "uid": 1000, "gid": 1000,
                    "shell": "/bin/bash", "home": "/home/alice",
                    "containerfile_strategy": "useradd",
                    "supplementary_groups": ["wheel"]
                }),
                serde_json::json!({
                    "name": "bob", "uid": 1001, "gid": 1001,
                    "shell": "/bin/bash", "home": "/home/bob",
                    "containerfile_strategy": "skip",
                    "supplementary_groups": []
                }),
            ],
            groups: vec![
                serde_json::json!({"name": "alice", "gid": 1000, "source": "custom"}),
                serde_json::json!({"name": "bob", "gid": 1001, "source": "custom"}),
            ],
            ..Default::default()
        });

        let cf = render_containerfile_users(&snap);
        let output = cf.join("\n");
        assert!(
            output.contains("groupadd -g 1000 alice"),
            "alice's group needed for useradd: {output}"
        );
        assert!(
            !output.contains("groupadd -g 1001 bob"),
            "bob's group should NOT be emitted (skip strategy): {output}"
        );
    }

    // ----- SSH staging tests -----

    #[test]
    fn stage_ssh_keys_creates_files() {
        let snap = snap_with(vec![test_user()], vec![]);
        let dir = tempfile::TempDir::new().unwrap();
        stage_ssh_keys(&snap, dir.path()).unwrap();

        let key_file = dir.path().join("users/home/alice/.ssh/authorized_keys");
        assert!(key_file.exists(), "authorized_keys file not created");
        let content = std::fs::read_to_string(&key_file).unwrap();
        assert!(content.contains("ssh-rsa AAAA"), "key content missing");
        assert!(content.ends_with('\n'), "should end with newline");
    }

    #[test]
    fn stage_ssh_keys_skips_users_without_keys() {
        let user = serde_json::json!({
            "name": "nokeys",
            "uid": 1003,
            "include": true
        });
        let snap = snap_with(vec![user], vec![]);
        let dir = tempfile::TempDir::new().unwrap();
        stage_ssh_keys(&snap, dir.path()).unwrap();

        let key_file = dir.path().join("users/home/nokeys/.ssh/authorized_keys");
        assert!(!key_file.exists(), "should not create file for user without keys");
    }
}
