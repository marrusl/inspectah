//! User and group renderers — produces kickstart, blueprint TOML, and
//! Containerfile fragments from the enriched users/groups data.

use inspectah_core::snapshot::InspectionSnapshot;
use std::io;
use std::path::Path;

use super::comment::LINE_TERMINATORS;
use super::language_packages::sanitize_for_comment;
use super::safety::{
    AccountName, CryptHash, ExportPath, double_quoted_word_refusal, sanitize_shell_value,
};
use crate::build::extract::is_single_normal_component;
use inspectah_collect::inspectors::users::NON_SYSTEM_GID_MIN;

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

/// Why `useradd` cannot be emitted for `u`, or `None` when every field
/// on its line is proved: the name and each supplementary group are
/// [`AccountName`]s, and a home or shell, when present, is an
/// [`ExportPath`]. Dropping one field instead would create a user unlike
/// the host's, so a refusal costs the whole user.
fn useradd_refusal(u: &serde_json::Value) -> Option<&'static str> {
    if AccountName::parse(str_field(u, "name")).is_err() {
        return Some("the name is not a valid account name");
    }
    if supp_groups(u)
        .iter()
        .any(|g| AccountName::parse(g).is_err())
    {
        return Some("a supplementary group is not a valid group name");
    }
    let home = str_field(u, "home");
    if !home.is_empty() && ExportPath::parse(home).is_err() {
        return Some("the home directory is not a plain absolute path");
    }
    let shell = str_field(u, "shell");
    if !shell.is_empty() && ExportPath::parse(shell).is_err() {
        return Some("the login shell is not a plain absolute path");
    }
    None
}

/// The snapshot groups `u`'s `useradd` needs the Containerfile to create
/// first, or why it cannot have them. `useradd` exits 6 on a `-g` or `-G`
/// group that does not exist, so every group it names is created here or
/// left to the base image.
///
/// The base image is trusted with exactly what the users inspector does
/// not record: groups below [`NON_SYSTEM_GID_MIN`] (`wheel`, `adm`), and
/// any entry it classifies `source: "system"`. It reads all of
/// `/etc/group` for memberships but records only the regular range, so a
/// supplementary name missing from the snapshot is a system group. A
/// primary gid in the regular range with no entry has nothing to create.
///
/// A group the operator excluded refuses its users rather than being
/// created anyway: the exclusion is the operator's, and the FIXME names
/// the group so they can see what it costs.
fn groups_to_create_for<'a>(
    u: &serde_json::Value,
    groups: &'a [serde_json::Value],
) -> Result<Vec<&'a serde_json::Value>, String> {
    let mut needs = Vec::new();
    let gid = uid_field(u, "gid");
    if gid > 0 {
        match groups.iter().find(|g| uid_field(g, "gid") == gid) {
            Some(g) => needs.extend(group_to_create(g)?),
            None if gid < NON_SYSTEM_GID_MIN => {}
            None => return Err(format!("its primary group {gid} is not in the snapshot")),
        }
    }
    for name in supp_groups(u) {
        if let Some(g) = groups.iter().find(|g| str_field(g, "name") == name) {
            needs.extend(group_to_create(g)?);
        }
    }
    Ok(needs)
}

/// `g` when the Containerfile must create it, `None` when the base image
/// provides it, or why a user that names it cannot be created.
fn group_to_create(g: &serde_json::Value) -> Result<Option<&serde_json::Value>, String> {
    if str_field(g, "source") == "system" {
        return Ok(None);
    }
    let name = str_field(g, "name");
    if !g.get("include").and_then(|v| v.as_bool()).unwrap_or(true) {
        return Err(format!(
            "its group {} is excluded",
            sanitize_for_comment(name)
        ));
    }
    if AccountName::parse(name).is_err() {
        return Err(format!(
            "its group {} is not a valid group name",
            sanitize_for_comment(name)
        ));
    }
    Ok(Some(g))
}

/// `s` as a TOML basic string, quotes included.
///
/// Every value in the blueprint fragment goes through this, so none can
/// close its string and start a key. Escaping keeps the TOML whole; it
/// says nothing about what the blueprint's consumer does with the value.
/// So the account names, home, shell and hash are proved first, as the
/// kickstart and Containerfile prove them, and only the SSH key relies on
/// this escaping alone: a key holds spaces and is carried as it is.
///
/// `"` and `\` get a backslash, and every control character and line
/// terminator is written as a `\uXXXX` escape. U+0085, U+2028 and
/// U+2029 are legal raw in a TOML string, but an editor may break the
/// line there, so they are escaped too.
fn toml_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() || LINE_TERMINATORS.contains(&c) => {
                out.push_str(&format!("\\u{:04X}", c as u32))
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
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
        // Every value on a kickstart directive line is proved, as on the
        // Containerfile's `groupadd` and `useradd` lines: a space adds an
        // option and a line break adds a directive.
        if AccountName::parse(name).is_err() {
            lines.push(format!(
                "# FIXME: group {} not created: the name is not a valid group name",
                sanitize_for_comment(name)
            ));
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
        // A refusal costs the whole user, and its keys, as in the
        // Containerfile: dropping one field would create a different user.
        if let Some(reason) = useradd_refusal(u) {
            lines.push(format!(
                "# FIXME: user {} not created: {reason}",
                sanitize_for_comment(name)
            ));
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

        // Password (conditional). A refused hash costs only the password.
        let pw = str_field(u, "password_hash");
        let mut pw_fixme = None;
        if !pw.is_empty() {
            match CryptHash::parse(pw) {
                Ok(hash) => opts.push_str(&format!(" --iscrypted --password={}", hash.as_str())),
                Err(_) => {
                    pw_fixme = Some(format!(
                        "# FIXME: password for user {name} not set: the hash is not a crypt(3) string"
                    ))
                }
            }
        }

        lines.push(opts);
        lines.extend(pw_fixme);

        // SSH keys: inside double quotes, so only what ends or acts there
        // is refused; a key's spaces are legitimate.
        let keys = ssh_keys(u);
        for key in &keys {
            match double_quoted_word_refusal(key) {
                None => lines.push(format!("sshkey --username={name} \"{key}\"")),
                Some(why) => lines.push(format!(
                    "# FIXME: SSH key for user {name} not written: the key {why}: {}",
                    sanitize_for_comment(key)
                )),
            }
        }
    }

    super::comment::join_lines(&lines)
}

// ---------------------------------------------------------------------------
// 2. Blueprint TOML renderer
// ---------------------------------------------------------------------------

/// Render a blueprint TOML fragment for user/group provisioning.
///
/// Emits `[[customizations.group]]` blocks, then `[[customizations.user]]` blocks.
/// A group, user or password the kickstart refuses gets the same `# FIXME`
/// here and is left out, so the two fragments carry the same accounts.
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
        // TOML escaping keeps the name in its string, but the blueprint's
        // consumer still creates the account, so the name gets the proof
        // the kickstart and the Containerfile give it.
        if AccountName::parse(name).is_err() {
            lines.push(format!(
                "# FIXME: group {} not created: the name is not a valid group name",
                sanitize_for_comment(name)
            ));
            continue;
        }
        lines.push("[[customizations.group]]".into());
        lines.push(format!("name = {}", toml_string(name)));
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
        // The same users as the kickstart: a refusal costs the whole user.
        if let Some(reason) = useradd_refusal(u) {
            lines.push(format!(
                "# FIXME: user {} not created: {reason}",
                sanitize_for_comment(name)
            ));
            continue;
        }
        let uid = uid_field(u, "uid");
        let gid = uid_field(u, "gid");
        let groups_list = supp_groups(u);
        let home = str_field(u, "home");
        let shell = str_field(u, "shell");

        lines.push("[[customizations.user]]".into());
        lines.push(format!("name = {}", toml_string(name)));
        if uid > 0 {
            lines.push(format!("uid = {uid}"));
        }
        if gid > 0 {
            lines.push(format!("gid = {gid}"));
        }
        if !home.is_empty() {
            lines.push(format!("home = {}", toml_string(home)));
        }
        if !shell.is_empty() {
            lines.push(format!("shell = {}", toml_string(shell)));
        }
        if !groups_list.is_empty() {
            lines.push(format!(
                "groups = [{}]",
                groups_list
                    .iter()
                    .map(|g| toml_string(g))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }

        // Password. A refused hash costs only the password, as in the
        // kickstart.
        let pw = str_field(u, "password_hash");
        if !pw.is_empty() {
            match CryptHash::parse(pw) {
                Ok(hash) => lines.push(format!("password = {}", toml_string(hash.as_str()))),
                Err(_) => lines.push(format!(
                    "# FIXME: password for user {name} not set: the hash is not a crypt(3) string"
                )),
            }
        }

        // SSH key — blueprint supports a single `key` field
        let keys = ssh_keys(u);
        if let Some(first_key) = keys.first() {
            lines.push(format!("key = {}", toml_string(first_key)));
            if keys.len() > 1 {
                lines.push(format!(
                    "# NOTE: {} additional SSH key(s) cannot be expressed in blueprint TOML (single key only)",
                    keys.len() - 1
                ));
            }
        }

        lines.push(String::new());
    }

    super::comment::join_lines(&lines)
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

    let mut body: Vec<String> = Vec::new();

    // Decide each user first, so a `groupadd` is emitted only for a group
    // a rendered `useradd` names, and every one of those comes before it.
    // A refused user gets no `chpasswd` entry and no SSH key lines either:
    // each would name an account that does not exist.
    let all_groups: &[serde_json::Value] = snap
        .users_groups
        .as_ref()
        .map_or(&[], |ug| ug.groups.as_slice());
    let mut refused_users: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut user_fixmes: Vec<String> = Vec::new();
    let mut groups_to_create: Vec<&serde_json::Value> = Vec::new();
    for u in &useradd_users {
        let name = str_field(u, "name");
        if name.is_empty() {
            continue;
        }
        let needs = useradd_refusal(u)
            .map(|r| Err(r.to_string()))
            .unwrap_or_else(|| groups_to_create_for(u, all_groups));
        match needs {
            Ok(needs) => {
                for g in needs {
                    if !groups_to_create.iter().any(|seen| std::ptr::eq(*seen, g)) {
                        groups_to_create.push(g);
                    }
                }
            }
            Err(reason) => {
                user_fixmes.push(format!(
                    "# FIXME: user {} not created: {reason}",
                    sanitize_for_comment(name)
                ));
                refused_users.insert(name);
            }
        }
    }

    // In snapshot order, so the output does not depend on user order.
    for g in all_groups {
        if !groups_to_create
            .iter()
            .any(|needed| std::ptr::eq(*needed, g))
        {
            continue;
        }
        // Proved by `groups_to_create_for`.
        let name = str_field(g, "name");
        let gid = uid_field(g, "gid");
        if gid > 0 {
            body.push(format!("RUN groupadd -g {gid} {name}"));
        } else {
            body.push(format!("RUN groupadd {name}"));
        }
    }
    body.extend(user_fixmes);

    // useradd lines.
    for u in &useradd_users {
        let name = str_field(u, "name");
        if name.is_empty() || refused_users.contains(name) {
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
        // Proved by `useradd_refusal`, and quoted as well.
        if !shell.is_empty() {
            cmd.push_str(&format!(" -s '{shell}'"));
        }
        if !home.is_empty() {
            cmd.push_str(&format!(" -d '{home}'"));
        }
        cmd.push_str(&format!(" {name}"));
        body.push(cmd);
    }

    // chpasswd for users with password hashes
    let pw_users: Vec<_> = useradd_users
        .iter()
        .filter(|u| !str_field(u, "password_hash").is_empty())
        .filter(|u| !refused_users.contains(str_field(u, "name")))
        .collect();
    if !pw_users.is_empty() {
        body.push(
            "# WARNING: Embedding password hashes in a Containerfile is a security risk.".into(),
        );
        body.push("# Consider using a secrets manager or deploy-time provisioning instead.".into());
        let mut chpasswd_entries: Vec<String> = Vec::new();
        for u in &pw_users {
            let name = str_field(u, "name");
            match CryptHash::parse(str_field(u, "password_hash")) {
                // Both halves are proved free of quotes, `:` and line
                // ends, so each record is one single-quoted word.
                Ok(hash) => chpasswd_entries.push(format!("'{name}:{}'", hash.as_str())),
                Err(_) => body.push(format!(
                    "# FIXME: password for user {} not set: the hash is not a crypt(3) string",
                    sanitize_for_comment(name)
                )),
            }
        }
        // One record per `printf` argument: `echo` does not expand `\n`
        // under bash, which would have joined every record into one.
        if !chpasswd_entries.is_empty() {
            body.push(format!(
                "RUN printf '%s\\n' {} | chpasswd -e",
                chpasswd_entries.join(" ")
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
        // `stage_ssh_keys` refuses this name, so there is no file to copy.
        if !is_single_normal_component(name) {
            body.push(format!(
                "# FIXME: SSH keys for user {} not staged: the name is not a single path component",
                sanitize_for_comment(name)
            ));
            continue;
        }
        if refused_users.contains(name) {
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

        body.push(format!(
            "RUN install -d -m 700 -o {name} -g {gid} '{home}/.ssh'"
        ));
        body.push(format!(
            "COPY users/home/{name}/.ssh/authorized_keys {home}/.ssh/authorized_keys"
        ));
        body.push(format!(
            "RUN chown {ownership} '{home}/.ssh/authorized_keys' && chmod 600 '{home}/.ssh/authorized_keys'"
        ));
    }

    lines.extend(super::containerfile::section("Users and Groups", body));
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
        // The name comes from the snapshot and is joined into the path:
        // `..` in it would stage the keys outside the export.
        if !is_single_normal_component(name) {
            eprintln!(
                "inspectah: warning: SSH keys for user {} not staged: \
                 the name is not a single path component",
                name.escape_debug()
            );
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
    fn snap_with(
        users: Vec<serde_json::Value>,
        groups: Vec<serde_json::Value>,
    ) -> InspectionSnapshot {
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

    /// `test_user`'s primary group. Every regular-range user on a host has
    /// one, and without it the Containerfile refuses the user.
    fn test_primary_group() -> serde_json::Value {
        serde_json::json!({
            "name": "alice",
            "gid": 1001,
            "source": "custom",
            "include": true
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
        assert!(
            ks.contains("--homedir=/home/alice"),
            "missing --homedir: {ks}"
        );
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

        assert!(
            toml.contains("key = \"ssh-rsa KEY1 bob@a\""),
            "first key missing"
        );
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
        let snap = snap_with(vec![test_user()], vec![test_primary_group(), test_group()]);
        let lines = render_containerfile_users(&snap);
        let output = lines.join("\n");

        // Groups come first
        assert!(
            output.contains("RUN groupadd -g 1010 docker"),
            "groupadd missing"
        );
        // useradd with flags
        assert!(
            output.contains("RUN useradd -m -u 1001 -g 1001 -G wheel,docker"),
            "useradd missing"
        );
        // Password warning
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
        assert!(
            output.contains("-m"),
            "useradd must include -m to create home dir: {output}"
        );
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

    /// Every group a rendered `useradd` names, by `-g` gid or `-G` name,
    /// is either created earlier in the block or left to the base image.
    /// `useradd` exits 6 on a group that exists in neither, so a user whose
    /// group the block cannot create is refused with a FIXME instead.
    #[test]
    fn useradd_never_names_a_group_nothing_creates() {
        use serde_json::json;
        let group = |name: &str, gid: u32, source: &str, include: bool| json!({"name": name, "gid": gid, "source": source, "include": include});
        let user = |gid: u32, supp: &[&str]| {
            json!({
                "name": "alice", "uid": 1001, "gid": gid,
                "shell": "/bin/bash", "home": "/home/alice",
                "containerfile_strategy": "useradd",
                "supplementary_groups": supp,
            })
        };
        // (case, user, groups, the useradd line or None when refused,
        //  the groupadd lines in order, a FIXME the output must carry)
        type Case = (
            &'static str,
            serde_json::Value,
            Vec<serde_json::Value>,
            Option<&'static str>,
            Vec<&'static str>,
            Option<&'static str>,
        );
        let cases: Vec<Case> = vec![
            (
                "primary gid with no matching group",
                user(1001, &[]),
                vec![],
                None,
                vec![],
                Some(
                    "# FIXME: user alice not created: its primary group 1001 is not in the snapshot",
                ),
            ),
            (
                "primary group excluded in refine",
                user(1001, &[]),
                vec![group("alice", 1001, "custom", false)],
                None,
                vec![],
                Some("# FIXME: user alice not created: its group alice is excluded"),
            ),
            (
                "supplementary group excluded in refine",
                user(1001, &["docker"]),
                vec![
                    group("alice", 1001, "custom", true),
                    group("docker", 1010, "custom", false),
                ],
                None,
                vec![],
                Some("# FIXME: user alice not created: its group docker is excluded"),
            ),
            (
                "supplementary group with an invalid name",
                user(1001, &["dock er"]),
                vec![
                    group("alice", 1001, "custom", true),
                    group("dock er", 1010, "custom", true),
                ],
                None,
                vec![],
                Some("# FIXME: user alice not created:"),
            ),
            (
                "primary and supplementary both present",
                user(1001, &["docker"]),
                vec![
                    group("alice", 1001, "custom", true),
                    group("docker", 1010, "custom", true),
                ],
                Some("RUN useradd -m -u 1001 -g 1001 -G docker"),
                vec!["RUN groupadd -g 1001 alice", "RUN groupadd -g 1010 docker"],
                None,
            ),
            (
                "system group such as wheel, left to the base",
                user(1001, &["wheel"]),
                vec![group("alice", 1001, "custom", true)],
                Some("RUN useradd -m -u 1001 -g 1001 -G wheel"),
                vec!["RUN groupadd -g 1001 alice"],
                None,
            ),
            (
                "primary gid in the system range, left to the base",
                user(100, &[]),
                vec![],
                Some("RUN useradd -m -u 1001 -g 100 "),
                vec![],
                None,
            ),
            (
                "a group the snapshot classifies as system",
                user(1001, &["adm"]),
                vec![
                    group("alice", 1001, "custom", true),
                    group("adm", 4, "system", true),
                ],
                Some("RUN useradd -m -u 1001 -g 1001 -G adm"),
                vec!["RUN groupadd -g 1001 alice"],
                None,
            ),
        ];

        for (case, u, groups, useradd, groupadds, fixme) in cases {
            let snap = snap_with(vec![u], groups);
            let cf = render_containerfile_users(&snap).join("\n");
            let active = active_lines(&cf);
            let rendered_groupadds: Vec<&str> = active
                .iter()
                .copied()
                .filter(|l| l.starts_with("RUN groupadd"))
                .collect();
            assert_eq!(
                rendered_groupadds, groupadds,
                "{case}: groupadd lines: {cf}"
            );
            let useradd_line = active.iter().position(|l| l.starts_with("RUN useradd"));
            match useradd {
                Some(prefix) => {
                    let at = useradd_line.unwrap_or_else(|| panic!("{case}: no useradd: {cf}"));
                    assert!(active[at].starts_with(prefix), "{case}: {cf}");
                    let last_groupadd = active.iter().rposition(|l| l.starts_with("RUN groupadd"));
                    assert!(
                        last_groupadd.is_none_or(|g| g < at),
                        "{case}: a groupadd follows the useradd: {cf}"
                    );
                }
                None => assert!(
                    useradd_line.is_none(),
                    "{case}: refused user rendered: {cf}"
                ),
            }
            if let Some(fixme) = fixme {
                assert!(cf.contains(fixme), "{case}: missing {fixme:?}: {cf}");
            }
        }
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

    /// The user name is joined into `users/home/{name}/.ssh`, so a name
    /// carrying `..` reaches outside the export. Real user names are one
    /// component; any other is refused by the stager and by the
    /// Containerfile, which says so in a FIXME instead of a `COPY` of a
    /// file nobody wrote.
    #[test]
    fn ssh_staging_names_cannot_write_outside_the_export() {
        use crate::render::victim_sandbox::VictimSandbox;

        // `users/home/` is two levels below the export root, so three
        // `..` reach the sandbox and the victim's directory.
        for hostile in ["../../../victim", "..", "a/../../../../victim"] {
            let s = VictimSandbox::new();
            let mut user = test_user();
            user["name"] = serde_json::json!(hostile);
            user["home"] = serde_json::json!("/home/x");
            let snap = snap_with(vec![user, test_user()], vec![test_primary_group()]);

            let before = s.outside();
            let result = stage_ssh_keys(&snap, &s.out());
            s.assert_contained(&before, hostile);
            result.expect("a refused name is skipped, not an error");
            assert!(
                s.out()
                    .join("users/home/alice/.ssh/authorized_keys")
                    .exists(),
                "{hostile}: the ordinary user beside it is still staged"
            );

            let cf = render_containerfile_users(&snap).join("\n");
            assert!(
                !cf.contains(&format!("COPY users/home/{hostile}/")),
                "{hostile}: COPY of an unstaged file: {cf}"
            );
            assert!(
                cf.contains(&format!("# FIXME: SSH keys for user {hostile} not staged")),
                "{hostile}: no FIXME for the refused user: {cf}"
            );
            assert!(
                cf.contains("COPY users/home/alice/.ssh/authorized_keys"),
                "{cf}"
            );
        }
    }

    /// The active (non-comment, non-blank) lines: what a build runs.
    fn active_lines(output: &str) -> Vec<&str> {
        output
            .lines()
            .filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty())
            .collect()
    }

    /// Every field on the `groupadd`, `useradd` and `chpasswd` lines comes
    /// from the snapshot. One hostile field per case: it may cost that
    /// user, group or password a FIXME, but it never adds a command, and
    /// the ordinary user beside it is still created.
    #[test]
    fn hostile_user_fields_add_no_command() {
        use serde_json::json;
        let cases = [
            (
                "home",
                json!("/home/mallory; id > /tmp/inspectah-user-injection #"),
            ),
            ("shell", json!("/bin/sh; id #")),
            ("shell", json!("/bin/sh\nRUN id")),
            ("supplementary_groups", json!(["wheel;id"])),
            ("supplementary_groups", json!(["-g0"])),
            ("password_hash", json!("$6$salt$hash'; id; echo '")),
            ("password_hash", json!("$6$salt$hash\nRUN id")),
            ("password_hash", json!("$6$salt$ha:sh")),
            ("name", json!("-omallory")),
            ("name", json!("mall*ry")),
        ];
        for (field, value) in cases {
            let mut user = test_user();
            user["name"] = json!("mallory");
            user["uid"] = json!(1002);
            user[field] = value.clone();
            let mut hostile_group = test_group();
            hostile_group["name"] = json!("-g0");
            // Needed only when a case names it as a supplementary group.
            hostile_group["gid"] = json!(4242);
            let snap = snap_with(
                vec![user, test_user()],
                vec![test_primary_group(), test_group(), hostile_group],
            );
            let cf = render_containerfile_users(&snap).join("\n");
            let active = active_lines(&cf);
            let what = format!("{field} = {value}");

            assert!(
                !active.iter().any(|l| l.contains("id >")
                    || l.contains("; id")
                    || l.contains("RUN id")
                    || l.contains("-g0")
                    || l.contains("-omallory")
                    || l.contains('*')),
                "{what}: a hostile field reached a command: {active:#?}"
            );
            assert!(
                active
                    .iter()
                    .any(|l| l.starts_with("RUN useradd -m -u 1001 -g 1001 -G wheel,docker")),
                "{what}: the ordinary user was not created: {active:#?}"
            );
            assert!(cf.contains("# FIXME:"), "{what}: no FIXME: {cf}");
        }
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
        assert!(
            !key_file.exists(),
            "should not create file for user without keys"
        );
    }
}
