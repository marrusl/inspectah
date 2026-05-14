//! Users/Groups inspector: non-system users and groups, sudoers, SSH key refs.
//!
//! Parses passwd/group/shadow/gshadow/subuid/subgid under host_root.
//! Uses a two-strategy auto-detect model (diverges from Go's three-way model):
//!   - Valid login shell → `blueprint` (auto)
//!   - No valid login shell → `sysusers` (auto)
//!   - `useradd` and `kickstart` → override-only (via UserGroupOptions)

use inspectah_core::traits::executor::Executor;
use inspectah_core::traits::inspector::{
    InspectionContext, Inspector, InspectorError, InspectorOutput,
};
use inspectah_core::types::completeness::{InspectorId, SectionData, SourceSystemKind};
use inspectah_core::types::redaction::RedactionHint;
use inspectah_core::types::users::UserGroupSection;
use inspectah_core::types::warnings::Warning;
use std::collections::HashMap;
use std::path::Path;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const NON_SYSTEM_UID_MIN: u32 = 1000;
const NON_SYSTEM_UID_MAX: u32 = 60000; // exclusive
const NON_SYSTEM_GID_MIN: u32 = 1000;
const NON_SYSTEM_GID_MAX: u32 = 60000; // exclusive

const VALID_LOGIN_SHELLS: &[&str] = &[
    "/bin/bash",
    "/bin/zsh",
    "/bin/sh",
    "/bin/fish",
    "/bin/tcsh",
    "/bin/csh",
    "/usr/bin/bash",
    "/usr/bin/zsh",
    "/usr/bin/fish",
];

/// Env var name patterns that suggest sensitive content (for sudoers redaction).
const SECRET_PATTERNS: &[&str] = &["PASSWORD", "SECRET", "TOKEN", "KEY", "CREDENTIAL"];

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

/// Internal options for the users/groups inspector.
#[derive(Debug, Clone, Default)]
pub struct UserGroupOptions {
    /// Override the auto-detected strategy for all users and groups.
    /// Accepts "useradd" or "kickstart" — bypasses the shell-based auto-detect.
    pub strategy_override: Option<String>,
}

// ---------------------------------------------------------------------------
// Inspector
// ---------------------------------------------------------------------------

/// Inspects users, groups, sudoers rules, SSH key references, and sub{uid,gid}
/// mappings on package-based RHEL systems.
pub struct UsersGroupsInspector {
    options: UserGroupOptions,
}

impl UsersGroupsInspector {
    pub fn new() -> Self {
        Self {
            options: UserGroupOptions::default(),
        }
    }

    pub fn with_options(options: UserGroupOptions) -> Self {
        Self { options }
    }
}

impl Default for UsersGroupsInspector {
    fn default() -> Self {
        Self::new()
    }
}

impl Inspector for UsersGroupsInspector {
    fn id(&self) -> InspectorId {
        InspectorId::UsersGroups
    }

    fn applicable_to(&self) -> &[SourceSystemKind] {
        &[SourceSystemKind::PackageBased]
    }

    fn inspect(&self, ctx: &InspectionContext<'_>) -> Result<InspectorOutput, InspectorError> {
        let exec = ctx.executor;
        let warnings: Vec<Warning> = Vec::new();
        let mut hints: Vec<RedactionHint> = Vec::new();
        let mut degraded_reasons: Vec<String> = Vec::new();

        let mut section = UserGroupSection::default();

        // -------------------------------------------------------------------
        // /etc/passwd — non-system users (UID >= 1000 and < 60000)
        // -------------------------------------------------------------------
        let mut non_system_users: HashMap<String, bool> = HashMap::new();
        let passwd_text = exec
            .read_file(Path::new("/etc/passwd"))
            .map_err(|e| InspectorError::Failed {
                reason: format!("cannot read /etc/passwd: {e}"),
            })?;
        parse_passwd(&passwd_text, &mut section, &mut non_system_users);

        // Classify and assign strategies (two-strategy auto-detect).
        for user in &mut section.users {
            let classification = classify_user(user);
            let strategy = match &self.options.strategy_override {
                Some(ovr) => ovr.clone(),
                None => match classification.as_str() {
                    "blueprint" => "blueprint".to_string(),
                    _ => "sysusers".to_string(),
                },
            };
            if let serde_json::Value::Object(ref mut map) = user {
                map.insert(
                    "classification".to_string(),
                    serde_json::Value::String(classification),
                );
                map.insert(
                    "strategy".to_string(),
                    serde_json::Value::String(strategy),
                );
            }
        }

        // -------------------------------------------------------------------
        // /etc/shadow — match by username from passwd
        // -------------------------------------------------------------------
        match exec.read_file(Path::new("/etc/shadow")) {
            Ok(shadow_text) => {
                parse_shadow(&shadow_text, &mut section, &non_system_users, &mut hints);
            }
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                degraded_reasons.push("cannot read /etc/shadow: permission denied".to_string());
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Silent skip — unusual but valid.
            }
            Err(e) => {
                degraded_reasons
                    .push(format!("cannot read /etc/shadow: {e}"));
            }
        }

        // -------------------------------------------------------------------
        // /etc/group — non-system groups (GID >= 1000 and < 60000)
        // -------------------------------------------------------------------
        let mut non_system_groups: HashMap<String, bool> = HashMap::new();
        match exec.read_file(Path::new("/etc/group")) {
            Ok(group_text) => {
                parse_group(&group_text, &mut section, &mut non_system_groups);
            }
            Err(e) => {
                degraded_reasons.push(format!("cannot read /etc/group: {e}"));
            }
        }

        // Assign strategies to groups: override > follow primary user > default sysusers.
        assign_group_strategies(&mut section, &self.options.strategy_override);

        // -------------------------------------------------------------------
        // /etc/gshadow — match by group name
        // -------------------------------------------------------------------
        if let Ok(gshadow_text) = exec.read_file(Path::new("/etc/gshadow")) {
            parse_gshadow(&gshadow_text, &mut section, &non_system_groups);
        }

        // -------------------------------------------------------------------
        // /etc/subuid and /etc/subgid
        // -------------------------------------------------------------------
        parse_subid_file(exec, "/etc/subuid", &mut section.subuid_entries, &non_system_users);
        parse_subid_file(exec, "/etc/subgid", &mut section.subgid_entries, &non_system_users);

        // -------------------------------------------------------------------
        // /etc/sudoers and /etc/sudoers.d/*
        // -------------------------------------------------------------------
        parse_sudoers(exec, &mut section, &mut hints);

        // -------------------------------------------------------------------
        // SSH authorized_keys per user
        // -------------------------------------------------------------------
        collect_ssh_keys(exec, &mut section);

        // -------------------------------------------------------------------
        // Return
        // -------------------------------------------------------------------
        let output = InspectorOutput {
            section: SectionData::UsersGroups(section),
            warnings,
            redaction_hints: hints,
        };

        if degraded_reasons.is_empty() {
            Ok(output)
        } else {
            Err(InspectorError::Degraded {
                partial: Box::new(output),
                reason: degraded_reasons.join("; "),
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Parsing helpers
// ---------------------------------------------------------------------------

/// Extracts non-system users from /etc/passwd content.
fn parse_passwd(
    text: &str,
    section: &mut UserGroupSection,
    non_system_users: &mut HashMap<String, bool>,
) {
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() < 7 {
            continue;
        }
        let uid: u32 = match parts[2].parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        if !(NON_SYSTEM_UID_MIN..NON_SYSTEM_UID_MAX).contains(&uid) {
            continue;
        }
        let username = parts[0];
        non_system_users.insert(username.to_string(), true);

        let gid: serde_json::Value = match parts[3].parse::<u32>() {
            Ok(g) => serde_json::Value::Number(g.into()),
            Err(_) => serde_json::Value::Null,
        };

        section.users.push(serde_json::json!({
            "name": username,
            "uid": uid,
            "gid": gid,
            "shell": parts[6],
            "home": parts[5],
            "include": true,
        }));
        section.passwd_entries.push(line.to_string());
    }
}

/// Extracts shadow entries for non-system users.
///
/// SECURITY-CRITICAL: The hash field (field 1) is NEVER stored. Only the
/// first characters are inspected to determine account status, then the
/// field is replaced with the status string in the stored entry.
fn parse_shadow(
    text: &str,
    section: &mut UserGroupSection,
    non_system_users: &HashMap<String, bool>,
    hints: &mut Vec<RedactionHint>,
) {
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() < 2 {
            continue;
        }
        let username = parts[0];
        if !non_system_users.contains_key(username) {
            continue;
        }

        // Determine password status from field 1 — NEVER store the hash.
        let hash_field = parts[1];
        let status = if hash_field.starts_with("!!") || hash_field == "!" {
            "locked"
        } else if hash_field == "*" {
            "disabled"
        } else if hash_field.is_empty() {
            "no_password"
        } else if hash_field.starts_with('$') {
            "password_set"
        } else {
            "unknown"
        };

        // Emit redaction hint for real password hashes.
        if status == "password_set" {
            hints.push(RedactionHint {
                path: "/etc/shadow".to_string(),
                reason: format!("shadow entry for user '{username}' contains password hash"),
                confidence: None,
            });
        }

        // Build safe shadow entry: replace hash field with status string.
        // Format: username:STATUS:field2:field3:field4:field5:field6:field7:field8
        let remaining_fields: Vec<&str> = if parts.len() > 2 {
            parts[2..].to_vec()
        } else {
            vec![]
        };
        let safe_entry = format!(
            "{}:{}:{}",
            username,
            status,
            remaining_fields.join(":")
        );
        section.shadow_entries.push(safe_entry);
    }
}

/// Extracts non-system groups from /etc/group content.
fn parse_group(
    text: &str,
    section: &mut UserGroupSection,
    non_system_groups: &mut HashMap<String, bool>,
) {
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() < 3 {
            continue;
        }
        let gid: u32 = match parts[2].parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        if !(NON_SYSTEM_GID_MIN..NON_SYSTEM_GID_MAX).contains(&gid) {
            continue;
        }
        let group_name = parts[0];
        non_system_groups.insert(group_name.to_string(), true);

        let members: Vec<serde_json::Value> = if parts.len() > 3 && !parts[3].is_empty() {
            parts[3]
                .split(',')
                .map(|m| serde_json::Value::String(m.to_string()))
                .collect()
        } else {
            vec![]
        };

        section.groups.push(serde_json::json!({
            "name": group_name,
            "gid": gid,
            "members": members,
            "include": true,
        }));
        // Store raw entry — matches how passwd_entries is populated.
        section.group_entries.push(line.to_string());
    }
}

/// Extracts gshadow entries for non-system groups.
///
/// SECURITY-CRITICAL: The password/hash field (field 1) is always replaced
/// with `!` regardless of content. Only admin and member lists are preserved.
fn parse_gshadow(
    text: &str,
    section: &mut UserGroupSection,
    non_system_groups: &HashMap<String, bool>,
) {
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() < 4 {
            continue;
        }
        let group_name = parts[0];
        if !non_system_groups.contains_key(group_name) {
            continue;
        }

        // Build safe gshadow entry: replace password field with !
        // Format: group:!:admins:members
        let safe_entry = format!("{}:!:{}:{}", parts[0], parts[2], parts[3]);
        section.gshadow_entries.push(safe_entry);
    }
}

/// Reads a subuid or subgid file, appending entries for non-system users.
fn parse_subid_file(
    exec: &dyn Executor,
    path: &str,
    entries: &mut Vec<String>,
    non_system_users: &HashMap<String, bool>,
) {
    let text = match exec.read_file(Path::new(path)) {
        Ok(t) => t,
        Err(_) => return,
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let username = match line.split(':').next() {
            Some(u) => u,
            None => continue,
        };
        if non_system_users.contains_key(username) {
            entries.push(line.to_string());
        }
    }
}

/// Reads /etc/sudoers and /etc/sudoers.d/* for sudo rules.
fn parse_sudoers(
    exec: &dyn Executor,
    section: &mut UserGroupSection,
    hints: &mut Vec<RedactionHint>,
) {
    // Main sudoers file.
    if let Ok(text) = exec.read_file(Path::new("/etc/sudoers")) {
        extract_sudoers_rules(&text, section, hints);
    }

    // Drop-in directory.
    let dir_entries = match exec.read_dir(Path::new("/etc/sudoers.d")) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry_name in &dir_entries {
        if entry_name.starts_with('.') {
            continue;
        }
        let path = format!("/etc/sudoers.d/{entry_name}");
        if let Ok(text) = exec.read_file(Path::new(&path)) {
            extract_sudoers_rules(&text, section, hints);
        }
    }
}

/// Parses sudoers content for non-comment, non-Defaults rules.
/// Includes `#includedir` and `@includedir` directives.
fn extract_sudoers_rules(
    text: &str,
    section: &mut UserGroupSection,
    hints: &mut Vec<RedactionHint>,
) {
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Include directives are kept as rules (they are structural).
        if line.starts_with("#includedir") || line.starts_with("@includedir") {
            section.sudoers_rules.push(line.to_string());
            continue;
        }
        // Skip comments and Defaults lines.
        if line.starts_with('#') || line.starts_with("Defaults") {
            continue;
        }
        section.sudoers_rules.push(line.to_string());

        // Emit redaction hints for lines matching secret patterns.
        let upper = line.to_uppercase();
        for pattern in SECRET_PATTERNS {
            if upper.contains(pattern) {
                // Skip false positive: NOPASSWD/PASSWD are policy directives, not secrets.
                if pattern == &"PASSWORD" && (upper.contains("NOPASSWD") || upper.contains("PASSWD:")) {
                    continue;
                }
                hints.push(RedactionHint {
                    path: "sudoers".to_string(),
                    reason: format!(
                        "sudoers rule matches secret pattern '{pattern}'"
                    ),
                    confidence: None,
                });
                break;
            }
        }
    }
}

/// Checks for ~/.ssh/authorized_keys for each user, counting keys.
/// SECURITY-CRITICAL: Only key count and path are stored, NEVER key content.
fn collect_ssh_keys(exec: &dyn Executor, section: &mut UserGroupSection) {
    for user in &section.users {
        let home = match user.get("home").and_then(|v| v.as_str()) {
            Some(h) if !h.is_empty() => h,
            _ => continue,
        };
        let auth_keys_path = format!("{home}/.ssh/authorized_keys");
        let content = match exec.read_file(Path::new(&auth_keys_path)) {
            Ok(c) => c,
            Err(_) => continue, // SSH dir inaccessible or file doesn't exist.
        };

        let key_count = content
            .lines()
            .filter(|l| {
                let trimmed = l.trim();
                !trimmed.is_empty() && !trimmed.starts_with('#')
            })
            .count();

        let username = user.get("name").and_then(|v| v.as_str()).unwrap_or("");

        section.ssh_authorized_keys_refs.push(serde_json::json!({
            "user": username,
            "key_count": key_count,
            "path": auth_keys_path,
        }));
    }
}

// ---------------------------------------------------------------------------
// User classification — two-strategy auto-detect (Rust model)
// ---------------------------------------------------------------------------

/// Classifies a user using the two-strategy auto-detect model.
///
/// This DIVERGES from Go's three-way model (service/human/ambiguous).
/// Rust uses shell-only classification:
///   - Valid login shell → `blueprint`
///   - No valid login shell (nologin, /bin/false, unknown) → `sysusers`
fn classify_user(user: &serde_json::Value) -> String {
    let shell = user.get("shell").and_then(|v| v.as_str()).unwrap_or("");

    if VALID_LOGIN_SHELLS.contains(&shell) {
        "blueprint".to_string()
    } else {
        "sysusers".to_string()
    }
}

/// Assigns migration strategies to groups.
///
/// Groups follow their primary user's strategy when possible:
///   - Override takes precedence over everything
///   - If a user shares the group's GID, the group inherits that user's strategy
///   - Groups with no primary user default to `sysusers`
fn assign_group_strategies(section: &mut UserGroupSection, override_strategy: &Option<String>) {
    // Build first-match map: GID → user entry.
    let mut user_by_gid: HashMap<u64, &serde_json::Value> = HashMap::new();
    for user in &section.users {
        if let Some(gid) = user.get("gid").and_then(|v| v.as_u64()) {
            user_by_gid.entry(gid).or_insert(user);
        }
    }

    for group in &mut section.groups {
        let strategy = if let Some(ovr) = override_strategy {
            ovr.clone()
        } else {
            let gid = group.get("gid").and_then(|v| v.as_u64()).unwrap_or(0);
            if let Some(primary_user) = user_by_gid.get(&gid) {
                primary_user
                    .get("strategy")
                    .and_then(|v| v.as_str())
                    .unwrap_or("sysusers")
                    .to_string()
            } else {
                "sysusers".to_string()
            }
        };
        if let serde_json::Value::Object(ref mut map) = group {
            map.insert(
                "strategy".to_string(),
                serde_json::Value::String(strategy),
            );
        }
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::mock::MockExecutor;
    use inspectah_core::types::completeness::SectionData;
    use inspectah_core::types::os::OsRelease;
    use inspectah_core::types::system::SourceSystem;

    // -----------------------------------------------------------------------
    // Helper: build a package-based InspectionContext
    // -----------------------------------------------------------------------
    fn pkg_source() -> SourceSystem {
        SourceSystem::PackageBased {
            os_release: OsRelease {
                name: "Red Hat Enterprise Linux".into(),
                version_id: "9.4".into(),
                id: "rhel".into(),
                ..Default::default()
            },
        }
    }

    // -----------------------------------------------------------------------
    // parse_passwd tests
    // -----------------------------------------------------------------------

    #[test]
    fn parse_passwd_non_system_users() {
        let text = "\
root:x:0:0:root:/root:/bin/bash
bin:x:1:1:bin:/bin:/sbin/nologin
alice:x:1000:1000:Alice:/home/alice:/bin/bash
bob:x:1001:1001:Bob:/home/bob:/usr/sbin/nologin
nobody:x:65534:65534:Kernel Overflow User:/:/sbin/nologin
";
        let mut section = UserGroupSection::default();
        let mut non_system = HashMap::new();
        parse_passwd(text, &mut section, &mut non_system);

        assert_eq!(section.users.len(), 2);
        assert!(non_system.contains_key("alice"));
        assert!(non_system.contains_key("bob"));
        assert!(!non_system.contains_key("root"));
        assert!(!non_system.contains_key("nobody"));
    }

    #[test]
    fn parse_passwd_boundary_uids() {
        let text = "\
low:x:999:999:Low:/home/low:/bin/bash
min:x:1000:1000:Min:/home/min:/bin/bash
mid:x:30000:30000:Mid:/home/mid:/bin/bash
max_excl:x:60000:60000:MaxExcl:/home/maxexcl:/bin/bash
high:x:65534:65534:High:/home/high:/bin/bash
";
        let mut section = UserGroupSection::default();
        let mut non_system = HashMap::new();
        parse_passwd(text, &mut section, &mut non_system);

        assert_eq!(section.users.len(), 2); // 1000 and 30000
        assert!(!non_system.contains_key("low"));    // 999 excluded
        assert!(non_system.contains_key("min"));     // 1000 included
        assert!(non_system.contains_key("mid"));     // 30000 included
        assert!(!non_system.contains_key("max_excl")); // 60000 excluded
        assert!(!non_system.contains_key("high"));   // 65534 excluded
    }

    // -----------------------------------------------------------------------
    // classify_user tests
    // -----------------------------------------------------------------------

    #[test]
    fn classify_user_valid_shell_blueprint() {
        let user = serde_json::json!({"shell": "/bin/bash"});
        assert_eq!(classify_user(&user), "blueprint");
    }

    #[test]
    fn classify_user_nologin_sysusers() {
        let user = serde_json::json!({"shell": "/sbin/nologin"});
        assert_eq!(classify_user(&user), "sysusers");
    }

    #[test]
    fn classify_user_unknown_shell_sysusers() {
        let user = serde_json::json!({"shell": "/usr/local/bin/custom"});
        assert_eq!(classify_user(&user), "sysusers");
    }

    #[test]
    fn classify_user_bin_false_sysusers() {
        let user = serde_json::json!({"shell": "/bin/false"});
        assert_eq!(classify_user(&user), "sysusers");
    }

    #[test]
    fn classify_user_zsh_blueprint() {
        let user = serde_json::json!({"shell": "/bin/zsh"});
        assert_eq!(classify_user(&user), "blueprint");
    }

    #[test]
    fn classify_user_fish_blueprint() {
        let user = serde_json::json!({"shell": "/usr/bin/fish"});
        assert_eq!(classify_user(&user), "blueprint");
    }

    // -----------------------------------------------------------------------
    // strategy override tests
    // -----------------------------------------------------------------------

    #[test]
    fn strategy_override_useradd() {
        let exec = MockExecutor::new()
            .with_file("/etc/passwd", "alice:x:1000:1000:Alice:/home/alice:/bin/bash\n");

        let source = pkg_source();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
        };
        let inspector = UsersGroupsInspector::with_options(UserGroupOptions {
            strategy_override: Some("useradd".to_string()),
        });
        let result = inspector.inspect(&ctx);

        // May be Degraded due to missing /etc/group, extract partial.
        let output = match result {
            Ok(o) => o,
            Err(InspectorError::Degraded { partial, .. }) => *partial,
            Err(e) => panic!("unexpected error: {e}"),
        };
        if let SectionData::UsersGroups(section) = &output.section {
            assert_eq!(section.users[0]["strategy"], "useradd");
        } else {
            panic!("expected UsersGroups section");
        }
    }

    #[test]
    fn strategy_override_kickstart() {
        let exec = MockExecutor::new()
            .with_file("/etc/passwd", "bob:x:1001:1001:Bob:/home/bob:/sbin/nologin\n");

        let source = pkg_source();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
        };
        let inspector = UsersGroupsInspector::with_options(UserGroupOptions {
            strategy_override: Some("kickstart".to_string()),
        });
        let result = inspector.inspect(&ctx);

        let output = match result {
            Ok(o) => o,
            Err(InspectorError::Degraded { partial, .. }) => *partial,
            Err(e) => panic!("unexpected error: {e}"),
        };
        if let SectionData::UsersGroups(section) = &output.section {
            assert_eq!(section.users[0]["strategy"], "kickstart");
        } else {
            panic!("expected UsersGroups section");
        }
    }

    // -----------------------------------------------------------------------
    // group strategy tests
    // -----------------------------------------------------------------------

    #[test]
    fn group_strategy_follows_primary_user() {
        let mut section = UserGroupSection::default();
        section.users.push(serde_json::json!({
            "name": "alice", "gid": 1000, "strategy": "blueprint"
        }));
        section.users.push(serde_json::json!({
            "name": "bob", "gid": 1001, "strategy": "sysusers"
        }));
        section.groups.push(serde_json::json!({"name": "alice", "gid": 1000}));
        section.groups.push(serde_json::json!({"name": "bob", "gid": 1001}));

        assign_group_strategies(&mut section, &None);

        assert_eq!(section.groups[0]["strategy"], "blueprint");
        assert_eq!(section.groups[1]["strategy"], "sysusers");
    }

    #[test]
    fn group_strategy_default_sysusers() {
        let mut section = UserGroupSection::default();
        // Group with no matching user.
        section.groups.push(serde_json::json!({"name": "orphan", "gid": 9999}));

        assign_group_strategies(&mut section, &None);

        assert_eq!(section.groups[0]["strategy"], "sysusers");
    }

    #[test]
    fn group_strategy_override() {
        let mut section = UserGroupSection::default();
        section.users.push(serde_json::json!({
            "name": "alice", "gid": 1000, "strategy": "blueprint"
        }));
        section.groups.push(serde_json::json!({"name": "alice", "gid": 1000}));

        assign_group_strategies(&mut section, &Some("useradd".to_string()));

        assert_eq!(section.groups[0]["strategy"], "useradd");
    }

    // -----------------------------------------------------------------------
    // shadow tests
    // -----------------------------------------------------------------------

    #[test]
    fn shadow_expiry_extraction() {
        let text = "alice:!!:19700:0:99999:7:::\n";
        let mut section = UserGroupSection::default();
        let non_system = HashMap::from([("alice".to_string(), true)]);
        let mut hints = Vec::new();
        parse_shadow(text, &mut section, &non_system, &mut hints);

        assert_eq!(section.shadow_entries.len(), 1);
        let entry = &section.shadow_entries[0];
        assert!(entry.starts_with("alice:locked:"));
        assert!(entry.contains("19700"));
    }

    #[test]
    fn shadow_locked_account() {
        let text = "alice:!!:19700:0:99999:7:::\n";
        let mut section = UserGroupSection::default();
        let non_system = HashMap::from([("alice".to_string(), true)]);
        let mut hints = Vec::new();
        parse_shadow(text, &mut section, &non_system, &mut hints);

        assert!(section.shadow_entries[0].contains(":locked:"));
    }

    #[test]
    fn shadow_disabled_account() {
        let text = "bob:*:19700:0:99999:7:::\n";
        let mut section = UserGroupSection::default();
        let non_system = HashMap::from([("bob".to_string(), true)]);
        let mut hints = Vec::new();
        parse_shadow(text, &mut section, &non_system, &mut hints);

        assert!(section.shadow_entries[0].contains(":disabled:"));
    }

    #[test]
    fn shadow_no_hash_stored() {
        let text = "alice:$6$rounds=5000$salt$hashvalue:19700:0:99999:7:::\n";
        let mut section = UserGroupSection::default();
        let non_system = HashMap::from([("alice".to_string(), true)]);
        let mut hints = Vec::new();
        parse_shadow(text, &mut section, &non_system, &mut hints);

        let entry = &section.shadow_entries[0];
        // Must contain status, not the hash.
        assert!(entry.contains(":password_set:"));
        // Must NOT contain any hash prefix.
        assert!(!entry.contains("$6$"), "shadow entry must not contain hash: {entry}");
        assert!(!entry.contains("$y$"), "shadow entry must not contain yescrypt hash");
        assert!(!entry.contains("$5$"), "shadow entry must not contain sha256 hash");
        assert!(!entry.contains("$2b$"), "shadow entry must not contain bcrypt hash");
    }

    #[test]
    fn shadow_no_hash_in_json() {
        // Negative test: serialize the entire section and assert no hash prefixes.
        let text = "\
alice:$6$rounds=5000$salt$longhashabcdef:19700:0:99999:7:::
bob:$y$j9T$salt$anotherhash:19700:0:99999:7:::
charlie:$5$salt$sha256hash:19700:0:99999:7:::
";
        let mut section = UserGroupSection::default();
        let non_system = HashMap::from([
            ("alice".to_string(), true),
            ("bob".to_string(), true),
            ("charlie".to_string(), true),
        ]);
        let mut hints = Vec::new();
        parse_shadow(text, &mut section, &non_system, &mut hints);

        let json = serde_json::to_string(&section).expect("serialize");
        assert!(!json.contains("$6$"), "JSON must not contain $6$ hash: {json}");
        assert!(!json.contains("$y$"), "JSON must not contain $y$ hash: {json}");
        assert!(!json.contains("$5$"), "JSON must not contain $5$ hash: {json}");
        assert!(!json.contains("$2b$"), "JSON must not contain $2b$ hash: {json}");
        assert!(!json.contains("longhashabcdef"), "JSON must not contain hash body");
    }

    #[test]
    fn shadow_permission_denied_degraded() {
        let exec = MockExecutor::new()
            .with_file("/etc/passwd", "alice:x:1000:1000:Alice:/home/alice:/bin/bash\n")
            .with_file_error("/etc/shadow", std::io::ErrorKind::PermissionDenied);

        let source = pkg_source();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
        };
        let result = UsersGroupsInspector::new().inspect(&ctx);

        match result {
            Err(InspectorError::Degraded { reason, .. }) => {
                assert!(reason.contains("permission denied"), "reason: {reason}");
            }
            other => panic!("expected Degraded, got: {other:?}"),
        }
    }

    #[test]
    fn shadow_not_found_silent_skip() {
        let exec = MockExecutor::new()
            .with_file("/etc/passwd", "alice:x:1000:1000:Alice:/home/alice:/bin/bash\n")
            .with_file("/etc/group", "alice:x:1000:\n");
        // /etc/shadow not registered → NotFound from MockExecutor.

        let source = pkg_source();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
        };
        let result = UsersGroupsInspector::new().inspect(&ctx);

        // Should succeed — missing shadow is a silent skip.
        assert!(result.is_ok(), "missing shadow should not cause failure: {result:?}");
    }

    // -----------------------------------------------------------------------
    // gshadow tests
    // -----------------------------------------------------------------------

    #[test]
    fn gshadow_strips_password_field() {
        let text = "alice:somesecret:alice:alice,bob\n";
        let mut section = UserGroupSection::default();
        let non_system = HashMap::from([("alice".to_string(), true)]);
        parse_gshadow(text, &mut section, &non_system);

        assert_eq!(section.gshadow_entries.len(), 1);
        assert_eq!(section.gshadow_entries[0], "alice:!:alice:alice,bob");
    }

    #[test]
    fn gshadow_no_hash_in_stored_entry() {
        let text = "mygroup:$6$secret$hash:admin1:member1,member2\n";
        let mut section = UserGroupSection::default();
        let non_system = HashMap::from([("mygroup".to_string(), true)]);
        parse_gshadow(text, &mut section, &non_system);

        let entry = &section.gshadow_entries[0];
        assert!(!entry.contains("$6$"), "gshadow must not contain hash: {entry}");
        assert!(entry.contains(":!:"), "gshadow must have ! as password field");
    }

    #[test]
    fn gshadow_no_hash_in_json() {
        // Negative test: serialize and assert no hash prefixes.
        let text = "\
grp1:$6$salt$hash1:admin1:mem1,mem2
grp2:$y$salt$hash2:admin2:mem3
";
        let mut section = UserGroupSection::default();
        let non_system = HashMap::from([
            ("grp1".to_string(), true),
            ("grp2".to_string(), true),
        ]);
        parse_gshadow(text, &mut section, &non_system);

        let json = serde_json::to_string(&section).expect("serialize");
        assert!(!json.contains("$6$"), "JSON must not contain $6$ hash: {json}");
        assert!(!json.contains("$y$"), "JSON must not contain $y$ hash: {json}");
    }

    // -----------------------------------------------------------------------
    // group tests
    // -----------------------------------------------------------------------

    #[test]
    fn group_non_system_groups() {
        let text = "\
root:x:0:
bin:x:1:
alice:x:1000:
wheel:x:1005:alice,charlie
nobody:x:65534:
";
        let mut section = UserGroupSection::default();
        let mut non_system = HashMap::new();
        parse_group(text, &mut section, &mut non_system);

        assert_eq!(section.groups.len(), 2); // alice (1000) and wheel (1005)
        assert!(non_system.contains_key("alice"));
        assert!(non_system.contains_key("wheel"));
        assert!(!non_system.contains_key("root"));
        assert!(!non_system.contains_key("nobody"));

        // Check members on wheel.
        let wheel = &section.groups[1];
        let members = wheel["members"].as_array().expect("members array");
        assert_eq!(members.len(), 2);
        assert_eq!(members[0], "alice");
        assert_eq!(members[1], "charlie");
    }

    #[test]
    fn gshadow_merges_members() {
        // gshadow stores admin and member lists separately.
        let text = "wheel:!:alice:alice,charlie\n";
        let mut section = UserGroupSection::default();
        let non_system = HashMap::from([("wheel".to_string(), true)]);
        parse_gshadow(text, &mut section, &non_system);

        assert_eq!(section.gshadow_entries.len(), 1);
        let entry = &section.gshadow_entries[0];
        assert_eq!(entry, "wheel:!:alice:alice,charlie");
    }

    // -----------------------------------------------------------------------
    // subuid/subgid tests
    // -----------------------------------------------------------------------

    #[test]
    fn subuid_subgid_parsing() {
        let exec = MockExecutor::new()
            .with_file("/etc/subuid", "alice:100000:65536\nbob:165536:65536\nroot:0:65536\n")
            .with_file("/etc/subgid", "alice:100000:65536\ncharlie:231072:65536\n");

        let non_system = HashMap::from([
            ("alice".to_string(), true),
            ("bob".to_string(), true),
            ("charlie".to_string(), true),
        ]);

        let mut subuid = Vec::new();
        let mut subgid = Vec::new();
        parse_subid_file(&exec, "/etc/subuid", &mut subuid, &non_system);
        parse_subid_file(&exec, "/etc/subgid", &mut subgid, &non_system);

        assert_eq!(subuid.len(), 2); // alice and bob, not root
        assert_eq!(subgid.len(), 2); // alice and charlie
        assert!(subuid.iter().all(|e| !e.starts_with("root:")));
    }

    // -----------------------------------------------------------------------
    // sudoers tests
    // -----------------------------------------------------------------------

    #[test]
    fn sudoers_rules_extracted() {
        let exec = MockExecutor::new()
            .with_file(
                "/etc/sudoers",
                "# Comment\nDefaults env_reset\nroot ALL=(ALL:ALL) ALL\n%wheel ALL=(ALL) ALL\n#includedir /etc/sudoers.d\n",
            );

        let mut section = UserGroupSection::default();
        let mut hints = Vec::new();
        parse_sudoers(&exec, &mut section, &mut hints);

        // Should have: root rule, %wheel rule, #includedir directive.
        assert_eq!(section.sudoers_rules.len(), 3);
        assert!(section.sudoers_rules.iter().any(|r| r.contains("root")));
        assert!(section.sudoers_rules.iter().any(|r| r.contains("%wheel")));
        assert!(section.sudoers_rules.iter().any(|r| r.starts_with("#includedir")));
        // Defaults and comments should be excluded.
        assert!(!section.sudoers_rules.iter().any(|r| r.starts_with("Defaults")));
    }

    #[test]
    fn sudoers_includedir_followed() {
        let exec = MockExecutor::new()
            .with_file(
                "/etc/sudoers",
                "root ALL=(ALL:ALL) ALL\n#includedir /etc/sudoers.d\n",
            )
            .with_dir("/etc/sudoers.d", vec!["webapp"])
            .with_file(
                "/etc/sudoers.d/webapp",
                "webapp ALL=(ALL) NOPASSWD: /usr/bin/systemctl restart webapp\n",
            );

        let mut section = UserGroupSection::default();
        let mut hints = Vec::new();
        parse_sudoers(&exec, &mut section, &mut hints);

        // Should have: root rule, #includedir, webapp rule.
        assert_eq!(section.sudoers_rules.len(), 3);
        assert!(section.sudoers_rules.iter().any(|r| r.contains("webapp")));
    }

    #[test]
    fn sudoers_redaction_hint_for_password() {
        let exec = MockExecutor::new()
            .with_file(
                "/etc/sudoers",
                "deploy ALL=(ALL) /usr/bin/env DB_PASSWORD=secret /opt/deploy.sh\n",
            );

        let mut section = UserGroupSection::default();
        let mut hints = Vec::new();
        parse_sudoers(&exec, &mut section, &mut hints);

        assert!(!hints.is_empty(), "PASSWORD in sudoers should produce a RedactionHint");
        assert!(hints[0].reason.contains("PASSWORD"));
    }

    #[test]
    fn sudoers_nopasswd_no_false_positive() {
        let exec = MockExecutor::new()
            .with_file(
                "/etc/sudoers",
                "webapp ALL=(ALL) NOPASSWD: /usr/bin/systemctl restart webapp\n%wheel ALL=(ALL) NOPASSWD: ALL\n",
            );

        let mut section = UserGroupSection::default();
        let mut hints = Vec::new();
        parse_sudoers(&exec, &mut section, &mut hints);

        assert!(hints.is_empty(), "NOPASSWD directives should NOT produce false-positive hints");
        assert_eq!(section.sudoers_rules.len(), 2);
    }

    // -----------------------------------------------------------------------
    // SSH key tests
    // -----------------------------------------------------------------------

    #[test]
    fn ssh_key_count_not_content() {
        let mut section = UserGroupSection::default();
        section.users.push(serde_json::json!({
            "name": "alice",
            "home": "/home/alice",
        }));

        let exec = MockExecutor::new()
            .with_file(
                "/home/alice/.ssh/authorized_keys",
                "ssh-rsa AAAAB3NzaC1yc2... alice@laptop\n# comment line\nssh-ed25519 AAAAC3NzaC1lZDI1NTE5... alice@work\n\n",
            );

        collect_ssh_keys(&exec, &mut section);

        assert_eq!(section.ssh_authorized_keys_refs.len(), 1);
        let ref_entry = &section.ssh_authorized_keys_refs[0];
        assert_eq!(ref_entry["user"], "alice");
        assert_eq!(ref_entry["key_count"], 2);
        assert_eq!(ref_entry["path"], "/home/alice/.ssh/authorized_keys");

        // Must NOT contain any key content.
        let json = serde_json::to_string(ref_entry).expect("serialize");
        assert!(!json.contains("AAAAB3"), "SSH ref must not contain key content");
        assert!(!json.contains("ssh-rsa"), "SSH ref must not contain key type");
    }

    #[test]
    fn ssh_dir_inaccessible() {
        let mut section = UserGroupSection::default();
        section.users.push(serde_json::json!({
            "name": "bob",
            "home": "/home/bob",
        }));

        // No file registered → read_file returns NotFound.
        let exec = MockExecutor::new();
        collect_ssh_keys(&exec, &mut section);

        assert!(section.ssh_authorized_keys_refs.is_empty(), "inaccessible SSH should be skipped");
    }

    // -----------------------------------------------------------------------
    // Fatal / edge case tests
    // -----------------------------------------------------------------------

    #[test]
    fn passwd_read_failure_fatal() {
        // No /etc/passwd registered → read_file returns NotFound → should be Failed.
        let exec = MockExecutor::new();
        let source = pkg_source();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
        };
        let result = UsersGroupsInspector::new().inspect(&ctx);

        match result {
            Err(InspectorError::Failed { reason }) => {
                assert!(reason.contains("passwd"), "reason: {reason}");
            }
            other => panic!("expected Failed, got: {other:?}"),
        }
    }

    #[test]
    fn empty_system_no_users() {
        let exec = MockExecutor::new()
            .with_file("/etc/passwd", "root:x:0:0:root:/root:/bin/bash\n")
            .with_file("/etc/group", "root:x:0:\n");

        let source = pkg_source();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
        };
        let result = UsersGroupsInspector::new().inspect(&ctx);

        let output = result.expect("should succeed");
        if let SectionData::UsersGroups(section) = &output.section {
            assert!(section.users.is_empty(), "no non-system users");
            assert!(section.groups.is_empty(), "no non-system groups");
            assert!(section.sudoers_rules.is_empty());
            assert!(section.ssh_authorized_keys_refs.is_empty());
        } else {
            panic!("expected UsersGroups section");
        }
    }
}
