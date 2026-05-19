//! Users/Groups inspector: non-system users and groups, sudoers, SSH key refs.
//!
//! Parses passwd/group/shadow/gshadow/subuid/subgid under host_root.
//! Classifies users by login shell:
//!   - Valid login shell → `interactive`
//!   - No valid login shell → `non-interactive`
//!
//! Each user gets default `containerfile_strategy: "skip"` and
//! `password_choice: "none"` — downstream UI/renderers override these.

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
    /// When true, store raw password hashes for users with status `password_set`.
    pub preserve_password_hashes: bool,
    /// When true, store full SSH key content (parsed key lines) per user.
    pub preserve_ssh_keys: bool,
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
        let passwd_text =
            exec.read_file(Path::new("/etc/passwd"))
                .map_err(|e| InspectorError::Failed {
                    reason: format!("cannot read /etc/passwd: {e}"),
                })?;
        parse_passwd(&passwd_text, &mut section, &mut non_system_users);

        // Classify users and set defaults.
        for user in &mut section.users {
            let classification = classify_user(user);
            if let serde_json::Value::Object(map) = user {
                map.insert(
                    "classification".to_string(),
                    serde_json::Value::String(classification),
                );
                // Set default containerfile_strategy and password_choice.
                map.entry("containerfile_strategy")
                    .or_insert_with(|| serde_json::Value::String("skip".to_string()));
                map.entry("password_choice")
                    .or_insert_with(|| serde_json::Value::String("none".to_string()));
            }
        }

        // -------------------------------------------------------------------
        // /etc/shadow — match by username from passwd
        // -------------------------------------------------------------------
        match exec.read_file(Path::new("/etc/shadow")) {
            Ok(shadow_text) => {
                parse_shadow(
                    &shadow_text,
                    &mut section,
                    &non_system_users,
                    &mut hints,
                    self.options.preserve_password_hashes,
                );
            }
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                degraded_reasons.push("cannot read /etc/shadow: permission denied".to_string());
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Silent skip — unusual but valid.
            }
            Err(e) => {
                degraded_reasons.push(format!("cannot read /etc/shadow: {e}"));
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

        // Note: group strategies removed — downstream renderers decide provisioning
        // method based on user classification (interactive/non-interactive).

        // -------------------------------------------------------------------
        // /etc/gshadow — match by group name
        // -------------------------------------------------------------------
        if let Ok(gshadow_text) = exec.read_file(Path::new("/etc/gshadow")) {
            parse_gshadow(&gshadow_text, &mut section, &non_system_groups);
        }

        // -------------------------------------------------------------------
        // /etc/subuid and /etc/subgid
        // -------------------------------------------------------------------
        parse_subid_file(
            exec,
            "/etc/subuid",
            &mut section.subuid_entries,
            &non_system_users,
        );
        parse_subid_file(
            exec,
            "/etc/subgid",
            &mut section.subgid_entries,
            &non_system_users,
        );

        // -------------------------------------------------------------------
        // /etc/sudoers and /etc/sudoers.d/*
        // -------------------------------------------------------------------
        parse_sudoers(exec, &mut section, &mut hints);

        // -------------------------------------------------------------------
        // SSH authorized_keys per user
        // -------------------------------------------------------------------
        collect_ssh_keys(exec, &mut section, self.options.preserve_ssh_keys);

        // -------------------------------------------------------------------
        // Enrichment: supplementary groups, has_sudo, has_subuid, rationale
        // -------------------------------------------------------------------

        // Collect supplementary groups from ALL group entries (including system
        // GIDs). parse_group only stores GID >= 1000, so we re-scan the raw
        // /etc/group text for membership across all GIDs.
        if let Ok(group_text) = exec.read_file(Path::new("/etc/group")) {
            enrich_supplementary_groups(&group_text, &mut section);
        }

        // Mark users with sudo access.
        enrich_sudo_flags(&mut section);

        // Mark users with subuid allocations.
        enrich_subuid_flags(&mut section);

        // LAST: compute classification_rationale with all signals available.
        for user in &mut section.users {
            let rationale = build_classification_rationale(user);
            if let serde_json::Value::Object(map) = user {
                map.insert(
                    "classification_rationale".to_string(),
                    serde_json::Value::String(rationale),
                );
            }
        }

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
    preserve_hashes: bool,
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

        // Determine password status from field 1 — NEVER store the hash
        // unless preserve_hashes is explicitly true.
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

        // When preserve_hashes is true AND status is password_set, store the
        // raw hash on the matching user entry.
        if preserve_hashes && status == "password_set" {
            for user in &mut section.users {
                let uname = user.get("name").and_then(|v| v.as_str()).unwrap_or("");
                if uname == username {
                    if let serde_json::Value::Object(map) = user {
                        map.insert(
                            "password_hash".to_string(),
                            serde_json::Value::String(hash_field.to_string()),
                        );
                    }
                    break;
                }
            }
        }

        // Build safe shadow entry: replace hash field with status string.
        // Format: username:STATUS:field2:field3:field4:field5:field6:field7:field8
        let remaining_fields: Vec<&str> = if parts.len() > 2 {
            parts[2..].to_vec()
        } else {
            vec![]
        };
        let safe_entry = format!("{}:{}:{}", username, status, remaining_fields.join(":"));
        section.shadow_entries.push(safe_entry);

        // Enrich user entry with password_status.
        for user in &mut section.users {
            let uname = user.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if uname == username {
                if let serde_json::Value::Object(map) = user {
                    map.insert(
                        "password_status".to_string(),
                        serde_json::Value::String(status.to_string()),
                    );
                }
                break;
            }
        }
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

        let source = if gid >= 1000 { "custom" } else { "system" };

        section.groups.push(serde_json::json!({
            "name": group_name,
            "gid": gid,
            "members": members,
            "include": true,
            "source": source,
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
                if pattern == &"PASSWORD"
                    && (upper.contains("NOPASSWD") || upper.contains("PASSWD:"))
                {
                    continue;
                }
                hints.push(RedactionHint {
                    path: "sudoers".to_string(),
                    reason: format!("sudoers rule matches secret pattern '{pattern}'"),
                    confidence: None,
                });
                break;
            }
        }
    }
}

/// Checks for ~/.ssh/authorized_keys for each user, counting keys.
/// When `preserve_keys` is false (default), only key count and path are stored.
/// When true, full key lines are stored in an `ssh_keys` array on the user entry.
fn collect_ssh_keys(exec: &dyn Executor, section: &mut UserGroupSection, preserve_keys: bool) {
    // Collect SSH data first, then apply to users — avoids double borrow.
    let mut ssh_data: Vec<(String, usize, String, Vec<String>)> = Vec::new();

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

        let key_lines: Vec<String> = content
            .lines()
            .filter(|l| {
                let trimmed = l.trim();
                !trimmed.is_empty() && !trimmed.starts_with('#')
            })
            .map(|l| l.trim().to_string())
            .collect();

        let key_count = key_lines.len();
        let username = user.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();

        ssh_data.push((username, key_count, auth_keys_path, key_lines));
    }

    for (username, key_count, auth_keys_path, key_lines) in &ssh_data {
        section.ssh_authorized_keys_refs.push(serde_json::json!({
            "user": username,
            "key_count": key_count,
            "path": auth_keys_path,
        }));

        // Enrich user entries with ssh_key_count (always) and ssh_keys (when preserving).
        for user in &mut section.users {
            let uname = user.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if uname == username {
                if let serde_json::Value::Object(map) = user {
                    map.insert(
                        "ssh_key_count".to_string(),
                        serde_json::Value::Number((*key_count).into()),
                    );
                    if preserve_keys {
                        let keys_json: Vec<serde_json::Value> = key_lines
                            .iter()
                            .map(|k| serde_json::Value::String(k.clone()))
                            .collect();
                        map.insert(
                            "ssh_keys".to_string(),
                            serde_json::Value::Array(keys_json),
                        );
                    }
                }
                break;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Enrichment helpers — run AFTER all collectors, BEFORE classification rationale
// ---------------------------------------------------------------------------

/// Scans ALL /etc/group entries (including system GIDs) to find supplementary
/// group memberships for each user. Sets `supplementary_groups` array on user.
fn enrich_supplementary_groups(group_text: &str, section: &mut UserGroupSection) {
    // Build map: username → list of group names where they appear as a member.
    let mut memberships: HashMap<String, Vec<String>> = HashMap::new();

    for line in group_text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() < 4 {
            continue;
        }
        let group_name = parts[0];
        let member_list = parts[3];
        if member_list.is_empty() {
            continue;
        }
        for member in member_list.split(',') {
            let member = member.trim();
            if !member.is_empty() {
                memberships
                    .entry(member.to_string())
                    .or_default()
                    .push(group_name.to_string());
            }
        }
    }

    // Apply to user entries.
    for user in &mut section.users {
        let username = user
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let groups = memberships.get(&username).cloned().unwrap_or_default();
        if let serde_json::Value::Object(map) = user {
            let groups_json: Vec<serde_json::Value> = groups
                .iter()
                .map(|g| serde_json::Value::String(g.clone()))
                .collect();
            map.insert(
                "supplementary_groups".to_string(),
                serde_json::Value::Array(groups_json),
            );
        }
    }
}

/// Sets `has_sudo: true` on users who appear in sudoers rules.
fn enrich_sudo_flags(section: &mut UserGroupSection) {
    // Collect usernames that appear in sudoers rules.
    let mut sudo_users: Vec<String> = Vec::new();
    for rule in &section.sudoers_rules {
        // Extract the first word of each rule — typically the user/group spec.
        let first_word = rule.split_whitespace().next().unwrap_or("");
        // Skip include directives and group specs (%group).
        if first_word.starts_with('#')
            || first_word.starts_with('@')
            || first_word.starts_with('%')
        {
            continue;
        }
        sudo_users.push(first_word.to_string());
    }

    for user in &mut section.users {
        let username = user
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        // Check direct user match OR group-based match (user in wheel/sudo group).
        let has_sudo = sudo_users.contains(&username)
            || user
                .get("supplementary_groups")
                .and_then(|v| v.as_array())
                .map(|groups| {
                    groups.iter().any(|g| {
                        let name = g.as_str().unwrap_or("");
                        // Check if any %group rule references a group the user belongs to.
                        section.sudoers_rules.iter().any(|rule| {
                            let first = rule.split_whitespace().next().unwrap_or("");
                            first == format!("%{name}")
                        })
                    })
                })
                .unwrap_or(false);
        if let serde_json::Value::Object(map) = user {
            map.insert(
                "has_sudo".to_string(),
                serde_json::Value::Bool(has_sudo),
            );
        }
    }
}

/// Sets `has_subuid: true` on users who have subuid allocations.
fn enrich_subuid_flags(section: &mut UserGroupSection) {
    let subuid_users: Vec<String> = section
        .subuid_entries
        .iter()
        .filter_map(|entry| entry.split(':').next().map(|s| s.to_string()))
        .collect();

    for user in &mut section.users {
        let username = user
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let has_subuid = subuid_users.contains(&username);
        if let serde_json::Value::Object(map) = user {
            map.insert(
                "has_subuid".to_string(),
                serde_json::Value::Bool(has_subuid),
            );
        }
    }
}

/// Builds a human-readable classification rationale from all enrichment signals.
/// Called LAST after all collectors and enrichments have run.
fn build_classification_rationale(user: &serde_json::Value) -> String {
    let mut parts: Vec<String> = Vec::new();

    // Shell type.
    if let Some(shell) = user.get("shell").and_then(|v| v.as_str()) {
        let shell_name = shell.rsplit('/').next().unwrap_or(shell);
        parts.push(format!("shell={shell_name}"));
    }

    // Home directory.
    if let Some(home) = user.get("home").and_then(|v| v.as_str()) {
        parts.push(format!("home={home}"));
    }

    // Password status.
    if let Some(status) = user.get("password_status").and_then(|v| v.as_str()) {
        parts.push(format!("password={status}"));
    }

    // Sudo access.
    if let Some(true) = user.get("has_sudo").and_then(|v| v.as_bool()) {
        parts.push("sudo=yes".to_string());
    } else {
        parts.push("sudo=no".to_string());
    }

    // SSH key count.
    if let Some(count) = user.get("ssh_key_count").and_then(|v| v.as_u64()) {
        parts.push(format!("ssh_keys={count}"));
    }

    // Supplementary groups.
    if let Some(groups) = user.get("supplementary_groups").and_then(|v| v.as_array()) {
        if !groups.is_empty() {
            let names: Vec<&str> = groups
                .iter()
                .filter_map(|g| g.as_str())
                .collect();
            parts.push(format!("groups={}", names.join("+")));
        }
    }

    parts.join(", ")
}

// ---------------------------------------------------------------------------
// User classification — two-strategy auto-detect (Rust model)
// ---------------------------------------------------------------------------

/// Classifies a user as interactive or non-interactive based on login shell.
///
///   - Valid login shell → `interactive`
///   - No valid login shell (nologin, /bin/false, unknown) → `non-interactive`
fn classify_user(user: &serde_json::Value) -> String {
    let shell = user.get("shell").and_then(|v| v.as_str()).unwrap_or("");

    if VALID_LOGIN_SHELLS.contains(&shell) {
        "interactive".to_string()
    } else {
        "non-interactive".to_string()
    }
}

// assign_group_strategies removed — groups no longer carry a strategy
// field. Downstream renderers decide provisioning based on user classification.

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
        assert!(!non_system.contains_key("low")); // 999 excluded
        assert!(non_system.contains_key("min")); // 1000 included
        assert!(non_system.contains_key("mid")); // 30000 included
        assert!(!non_system.contains_key("max_excl")); // 60000 excluded
        assert!(!non_system.contains_key("high")); // 65534 excluded
    }

    // -----------------------------------------------------------------------
    // classify_user tests
    // -----------------------------------------------------------------------

    #[test]
    fn classify_user_valid_shell_interactive() {
        let user = serde_json::json!({"shell": "/bin/bash"});
        assert_eq!(classify_user(&user), "interactive");
    }

    #[test]
    fn classify_user_nologin_non_interactive() {
        let user = serde_json::json!({"shell": "/sbin/nologin"});
        assert_eq!(classify_user(&user), "non-interactive");
    }

    #[test]
    fn classify_user_unknown_shell_non_interactive() {
        let user = serde_json::json!({"shell": "/usr/local/bin/custom"});
        assert_eq!(classify_user(&user), "non-interactive");
    }

    #[test]
    fn classify_user_bin_false_non_interactive() {
        let user = serde_json::json!({"shell": "/bin/false"});
        assert_eq!(classify_user(&user), "non-interactive");
    }

    #[test]
    fn classify_user_zsh_interactive() {
        let user = serde_json::json!({"shell": "/bin/zsh"});
        assert_eq!(classify_user(&user), "interactive");
    }

    #[test]
    fn classify_user_fish_interactive() {
        let user = serde_json::json!({"shell": "/usr/bin/fish"});
        assert_eq!(classify_user(&user), "interactive");
    }

    // -----------------------------------------------------------------------
    // strategy override tests
    // -----------------------------------------------------------------------

    #[test]
    fn classification_interactive_with_login_shell() {
        let exec = MockExecutor::new().with_file(
            "/etc/passwd",
            "alice:x:1000:1000:Alice:/home/alice:/bin/bash\n",
        );

        let source = pkg_source();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };
        let inspector = UsersGroupsInspector::new();
        let result = inspector.inspect(&ctx);

        let output = match result {
            Ok(o) => o,
            Err(InspectorError::Degraded { partial, .. }) => *partial,
            Err(e) => panic!("unexpected error: {e}"),
        };
        if let SectionData::UsersGroups(section) = &output.section {
            assert_eq!(section.users[0]["classification"], "interactive");
            assert_eq!(section.users[0]["containerfile_strategy"], "skip");
            assert_eq!(section.users[0]["password_choice"], "none");
        } else {
            panic!("expected UsersGroups section");
        }
    }

    #[test]
    fn classification_non_interactive_with_nologin() {
        let exec = MockExecutor::new().with_file(
            "/etc/passwd",
            "bob:x:1001:1001:Bob:/home/bob:/sbin/nologin\n",
        );

        let source = pkg_source();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };
        let inspector = UsersGroupsInspector::new();
        let result = inspector.inspect(&ctx);

        let output = match result {
            Ok(o) => o,
            Err(InspectorError::Degraded { partial, .. }) => *partial,
            Err(e) => panic!("unexpected error: {e}"),
        };
        if let SectionData::UsersGroups(section) = &output.section {
            assert_eq!(section.users[0]["classification"], "non-interactive");
            assert_eq!(section.users[0]["containerfile_strategy"], "skip");
            assert_eq!(section.users[0]["password_choice"], "none");
        } else {
            panic!("expected UsersGroups section");
        }
    }

    // -----------------------------------------------------------------------
    // group strategy tests
    // -----------------------------------------------------------------------

    #[test]
    fn groups_have_no_strategy_field() {
        // Groups no longer carry a strategy field — downstream renderers
        // decide provisioning method based on user classification.
        let exec = MockExecutor::new()
            .with_file(
                "/etc/passwd",
                "alice:x:1000:1000:Alice:/home/alice:/bin/bash\n",
            )
            .with_file("/etc/group", "alice:x:1000:\n");

        let source = pkg_source();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };
        let result = UsersGroupsInspector::new().inspect(&ctx);

        let output = match result {
            Ok(o) => o,
            Err(InspectorError::Degraded { partial, .. }) => *partial,
            Err(e) => panic!("unexpected error: {e}"),
        };
        if let SectionData::UsersGroups(section) = &output.section {
            assert!(
                section.groups[0].get("strategy").is_none(),
                "groups should not have a strategy field"
            );
        } else {
            panic!("expected UsersGroups section");
        }
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
        parse_shadow(text, &mut section, &non_system, &mut hints, false);

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
        parse_shadow(text, &mut section, &non_system, &mut hints, false);

        assert!(section.shadow_entries[0].contains(":locked:"));
    }

    #[test]
    fn shadow_disabled_account() {
        let text = "bob:*:19700:0:99999:7:::\n";
        let mut section = UserGroupSection::default();
        let non_system = HashMap::from([("bob".to_string(), true)]);
        let mut hints = Vec::new();
        parse_shadow(text, &mut section, &non_system, &mut hints, false);

        assert!(section.shadow_entries[0].contains(":disabled:"));
    }

    #[test]
    fn shadow_no_hash_stored() {
        let text = "alice:$6$rounds=5000$salt$hashvalue:19700:0:99999:7:::\n";
        let mut section = UserGroupSection::default();
        let non_system = HashMap::from([("alice".to_string(), true)]);
        let mut hints = Vec::new();
        parse_shadow(text, &mut section, &non_system, &mut hints, false);

        let entry = &section.shadow_entries[0];
        // Must contain status, not the hash.
        assert!(entry.contains(":password_set:"));
        // Must NOT contain any hash prefix.
        assert!(
            !entry.contains("$6$"),
            "shadow entry must not contain hash: {entry}"
        );
        assert!(
            !entry.contains("$y$"),
            "shadow entry must not contain yescrypt hash"
        );
        assert!(
            !entry.contains("$5$"),
            "shadow entry must not contain sha256 hash"
        );
        assert!(
            !entry.contains("$2b$"),
            "shadow entry must not contain bcrypt hash"
        );
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
        parse_shadow(text, &mut section, &non_system, &mut hints, false);

        let json = serde_json::to_string(&section).expect("serialize");
        assert!(
            !json.contains("$6$"),
            "JSON must not contain $6$ hash: {json}"
        );
        assert!(
            !json.contains("$y$"),
            "JSON must not contain $y$ hash: {json}"
        );
        assert!(
            !json.contains("$5$"),
            "JSON must not contain $5$ hash: {json}"
        );
        assert!(
            !json.contains("$2b$"),
            "JSON must not contain $2b$ hash: {json}"
        );
        assert!(
            !json.contains("longhashabcdef"),
            "JSON must not contain hash body"
        );
    }

    #[test]
    fn shadow_permission_denied_degraded() {
        let exec = MockExecutor::new()
            .with_file(
                "/etc/passwd",
                "alice:x:1000:1000:Alice:/home/alice:/bin/bash\n",
            )
            .with_file_error("/etc/shadow", std::io::ErrorKind::PermissionDenied);

        let source = pkg_source();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
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
            .with_file(
                "/etc/passwd",
                "alice:x:1000:1000:Alice:/home/alice:/bin/bash\n",
            )
            .with_file("/etc/group", "alice:x:1000:\n");
        // /etc/shadow not registered → NotFound from MockExecutor.

        let source = pkg_source();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };
        let result = UsersGroupsInspector::new().inspect(&ctx);

        // Should succeed — missing shadow is a silent skip.
        assert!(
            result.is_ok(),
            "missing shadow should not cause failure: {result:?}"
        );
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
        assert!(
            !entry.contains("$6$"),
            "gshadow must not contain hash: {entry}"
        );
        assert!(
            entry.contains(":!:"),
            "gshadow must have ! as password field"
        );
    }

    #[test]
    fn gshadow_no_hash_in_json() {
        // Negative test: serialize and assert no hash prefixes.
        let text = "\
grp1:$6$salt$hash1:admin1:mem1,mem2
grp2:$y$salt$hash2:admin2:mem3
";
        let mut section = UserGroupSection::default();
        let non_system = HashMap::from([("grp1".to_string(), true), ("grp2".to_string(), true)]);
        parse_gshadow(text, &mut section, &non_system);

        let json = serde_json::to_string(&section).expect("serialize");
        assert!(
            !json.contains("$6$"),
            "JSON must not contain $6$ hash: {json}"
        );
        assert!(
            !json.contains("$y$"),
            "JSON must not contain $y$ hash: {json}"
        );
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
            .with_file(
                "/etc/subuid",
                "alice:100000:65536\nbob:165536:65536\nroot:0:65536\n",
            )
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
        assert!(
            section
                .sudoers_rules
                .iter()
                .any(|r| r.starts_with("#includedir"))
        );
        // Defaults and comments should be excluded.
        assert!(
            !section
                .sudoers_rules
                .iter()
                .any(|r| r.starts_with("Defaults"))
        );
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
        let exec = MockExecutor::new().with_file(
            "/etc/sudoers",
            "deploy ALL=(ALL) /usr/bin/env DB_PASSWORD=secret /opt/deploy.sh\n",
        );

        let mut section = UserGroupSection::default();
        let mut hints = Vec::new();
        parse_sudoers(&exec, &mut section, &mut hints);

        assert!(
            !hints.is_empty(),
            "PASSWORD in sudoers should produce a RedactionHint"
        );
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

        assert!(
            hints.is_empty(),
            "NOPASSWD directives should NOT produce false-positive hints"
        );
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

        collect_ssh_keys(&exec, &mut section, false);

        assert_eq!(section.ssh_authorized_keys_refs.len(), 1);
        let ref_entry = &section.ssh_authorized_keys_refs[0];
        assert_eq!(ref_entry["user"], "alice");
        assert_eq!(ref_entry["key_count"], 2);
        assert_eq!(ref_entry["path"], "/home/alice/.ssh/authorized_keys");

        // Must NOT contain any key content.
        let json = serde_json::to_string(ref_entry).expect("serialize");
        assert!(
            !json.contains("AAAAB3"),
            "SSH ref must not contain key content"
        );
        assert!(
            !json.contains("ssh-rsa"),
            "SSH ref must not contain key type"
        );
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
        collect_ssh_keys(&exec, &mut section, false);

        assert!(
            section.ssh_authorized_keys_refs.is_empty(),
            "inaccessible SSH should be skipped"
        );
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
            baseline_data: None,
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
            baseline_data: None,
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

    // -----------------------------------------------------------------------
    // classification_rationale tests
    // -----------------------------------------------------------------------

    #[test]
    fn classification_rationale_includes_all_enrichments() {
        // Build a fully-enriched user with all signals present.
        let user = serde_json::json!({
            "name": "alice",
            "uid": 1000,
            "gid": 1000,
            "shell": "/bin/bash",
            "home": "/home/alice",
            "password_status": "password_set",
            "has_sudo": true,
            "ssh_key_count": 2,
            "supplementary_groups": ["wheel", "docker"],
        });

        let rationale = build_classification_rationale(&user);

        // Verify ALL signals appear in the rationale.
        assert!(
            rationale.contains("shell=bash"),
            "rationale should include shell: {rationale}"
        );
        assert!(
            rationale.contains("home=/home/alice"),
            "rationale should include home: {rationale}"
        );
        assert!(
            rationale.contains("password=password_set"),
            "rationale should include password status: {rationale}"
        );
        assert!(
            rationale.contains("sudo=yes"),
            "rationale should include sudo: {rationale}"
        );
        assert!(
            rationale.contains("ssh_keys=2"),
            "rationale should include ssh key count: {rationale}"
        );
        assert!(
            rationale.contains("groups=wheel+docker"),
            "rationale should include groups: {rationale}"
        );
    }

    #[test]
    fn classification_rationale_no_sudo() {
        let user = serde_json::json!({
            "name": "bob",
            "shell": "/sbin/nologin",
            "home": "/home/bob",
            "has_sudo": false,
        });

        let rationale = build_classification_rationale(&user);
        assert!(
            rationale.contains("sudo=no"),
            "rationale should show sudo=no: {rationale}"
        );
        assert!(
            rationale.contains("shell=nologin"),
            "rationale should include shell: {rationale}"
        );
    }

    // -----------------------------------------------------------------------
    // password hash preservation tests
    // -----------------------------------------------------------------------

    #[test]
    fn shadow_preserve_hashes_stores_hash() {
        let text = "alice:$6$rounds=5000$salt$hashvalue:19700:0:99999:7:::\n";
        let mut section = UserGroupSection::default();
        section.users.push(serde_json::json!({
            "name": "alice",
            "uid": 1000,
            "gid": 1000,
            "shell": "/bin/bash",
            "home": "/home/alice",
            "include": true,
        }));
        let non_system = HashMap::from([("alice".to_string(), true)]);
        let mut hints = Vec::new();

        parse_shadow(text, &mut section, &non_system, &mut hints, true);

        // Hash should be stored on the user entry.
        let hash = section.users[0]
            .get("password_hash")
            .and_then(|v| v.as_str());
        assert_eq!(
            hash,
            Some("$6$rounds=5000$salt$hashvalue"),
            "hash should be preserved"
        );

        // password_status should also be set.
        let status = section.users[0]
            .get("password_status")
            .and_then(|v| v.as_str());
        assert_eq!(status, Some("password_set"));
    }

    #[test]
    fn shadow_no_preserve_hashes_omits_hash() {
        let text = "alice:$6$rounds=5000$salt$hashvalue:19700:0:99999:7:::\n";
        let mut section = UserGroupSection::default();
        section.users.push(serde_json::json!({
            "name": "alice",
            "uid": 1000,
            "gid": 1000,
            "shell": "/bin/bash",
            "home": "/home/alice",
            "include": true,
        }));
        let non_system = HashMap::from([("alice".to_string(), true)]);
        let mut hints = Vec::new();

        parse_shadow(text, &mut section, &non_system, &mut hints, false);

        // Hash must NOT be stored.
        assert!(
            section.users[0].get("password_hash").is_none(),
            "hash should not be stored when preserve is false"
        );

        // password_status should still be set.
        let status = section.users[0]
            .get("password_status")
            .and_then(|v| v.as_str());
        assert_eq!(status, Some("password_set"));
    }

    #[test]
    fn shadow_preserve_hashes_locked_no_hash() {
        // Locked accounts should NOT get password_hash even with preserve=true.
        let text = "bob:!!:19700:0:99999:7:::\n";
        let mut section = UserGroupSection::default();
        section.users.push(serde_json::json!({
            "name": "bob",
            "uid": 1001,
            "gid": 1001,
            "shell": "/bin/bash",
            "home": "/home/bob",
            "include": true,
        }));
        let non_system = HashMap::from([("bob".to_string(), true)]);
        let mut hints = Vec::new();

        parse_shadow(text, &mut section, &non_system, &mut hints, true);

        assert!(
            section.users[0].get("password_hash").is_none(),
            "locked accounts should not get password_hash"
        );
        let status = section.users[0]
            .get("password_status")
            .and_then(|v| v.as_str());
        assert_eq!(status, Some("locked"));
    }

    // -----------------------------------------------------------------------
    // SSH key content preservation tests
    // -----------------------------------------------------------------------

    #[test]
    fn ssh_preserve_keys_stores_content() {
        let mut section = UserGroupSection::default();
        section.users.push(serde_json::json!({
            "name": "alice",
            "home": "/home/alice",
        }));

        let exec = MockExecutor::new().with_file(
            "/home/alice/.ssh/authorized_keys",
            "ssh-rsa AAAAB3... alice@laptop\nssh-ed25519 AAAAC3... alice@work\n",
        );

        collect_ssh_keys(&exec, &mut section, true);

        // ssh_key_count should always be set.
        assert_eq!(section.users[0]["ssh_key_count"], 2);

        // ssh_keys array should contain the key lines.
        let keys = section.users[0]["ssh_keys"]
            .as_array()
            .expect("ssh_keys should be an array");
        assert_eq!(keys.len(), 2);
        assert!(keys[0].as_str().unwrap().starts_with("ssh-rsa"));
        assert!(keys[1].as_str().unwrap().starts_with("ssh-ed25519"));
    }

    #[test]
    fn ssh_no_preserve_keys_omits_content() {
        let mut section = UserGroupSection::default();
        section.users.push(serde_json::json!({
            "name": "alice",
            "home": "/home/alice",
        }));

        let exec = MockExecutor::new().with_file(
            "/home/alice/.ssh/authorized_keys",
            "ssh-rsa AAAAB3... alice@laptop\n",
        );

        collect_ssh_keys(&exec, &mut section, false);

        // ssh_key_count should be set.
        assert_eq!(section.users[0]["ssh_key_count"], 1);

        // ssh_keys array should NOT be present.
        assert!(
            section.users[0].get("ssh_keys").is_none(),
            "ssh_keys should not be stored when preserve is false"
        );
    }

    // -----------------------------------------------------------------------
    // group source field tests
    // -----------------------------------------------------------------------

    #[test]
    fn group_source_custom_for_high_gid() {
        let text = "devs:x:1001:alice,bob\n";
        let mut section = UserGroupSection::default();
        let mut non_system = HashMap::new();
        parse_group(text, &mut section, &mut non_system);

        assert_eq!(section.groups.len(), 1);
        assert_eq!(section.groups[0]["source"], "custom");
    }

    // -----------------------------------------------------------------------
    // supplementary groups tests
    // -----------------------------------------------------------------------

    #[test]
    fn supplementary_groups_includes_system_groups() {
        // wheel (GID 10) and docker (GID 999) are system groups — they won't
        // appear in section.groups, but supplementary_groups should still list them.
        let group_text = "\
root:x:0:
wheel:x:10:alice
docker:x:999:alice,bob
alice:x:1000:
bob:x:1001:
devs:x:1002:alice,bob
";
        let mut section = UserGroupSection::default();
        section.users.push(serde_json::json!({
            "name": "alice",
            "uid": 1000,
            "gid": 1000,
            "shell": "/bin/bash",
            "home": "/home/alice",
            "include": true,
        }));
        section.users.push(serde_json::json!({
            "name": "bob",
            "uid": 1001,
            "gid": 1001,
            "shell": "/bin/bash",
            "home": "/home/bob",
            "include": true,
        }));

        enrich_supplementary_groups(group_text, &mut section);

        // Alice should have wheel, docker, devs.
        let alice_groups = section.users[0]["supplementary_groups"]
            .as_array()
            .expect("alice supplementary_groups");
        let alice_names: Vec<&str> = alice_groups
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(
            alice_names.contains(&"wheel"),
            "alice should be in wheel: {alice_names:?}"
        );
        assert!(
            alice_names.contains(&"docker"),
            "alice should be in docker: {alice_names:?}"
        );
        assert!(
            alice_names.contains(&"devs"),
            "alice should be in devs: {alice_names:?}"
        );

        // Bob should have docker, devs (not wheel).
        let bob_groups = section.users[1]["supplementary_groups"]
            .as_array()
            .expect("bob supplementary_groups");
        let bob_names: Vec<&str> = bob_groups
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(
            bob_names.contains(&"docker"),
            "bob should be in docker: {bob_names:?}"
        );
        assert!(
            bob_names.contains(&"devs"),
            "bob should be in devs: {bob_names:?}"
        );
        assert!(
            !bob_names.contains(&"wheel"),
            "bob should NOT be in wheel: {bob_names:?}"
        );
    }

    // -----------------------------------------------------------------------
    // sudo enrichment tests
    // -----------------------------------------------------------------------

    #[test]
    fn enrich_sudo_direct_user_rule() {
        let mut section = UserGroupSection::default();
        section.users.push(serde_json::json!({
            "name": "alice",
            "supplementary_groups": [],
        }));
        section
            .sudoers_rules
            .push("alice ALL=(ALL) ALL".to_string());

        enrich_sudo_flags(&mut section);

        assert_eq!(section.users[0]["has_sudo"], true);
    }

    #[test]
    fn enrich_sudo_group_based() {
        let mut section = UserGroupSection::default();
        section.users.push(serde_json::json!({
            "name": "alice",
            "supplementary_groups": ["wheel"],
        }));
        section
            .sudoers_rules
            .push("%wheel ALL=(ALL) ALL".to_string());

        enrich_sudo_flags(&mut section);

        assert_eq!(section.users[0]["has_sudo"], true);
    }

    #[test]
    fn enrich_sudo_no_match() {
        let mut section = UserGroupSection::default();
        section.users.push(serde_json::json!({
            "name": "bob",
            "supplementary_groups": [],
        }));
        section
            .sudoers_rules
            .push("alice ALL=(ALL) ALL".to_string());

        enrich_sudo_flags(&mut section);

        assert_eq!(section.users[0]["has_sudo"], false);
    }

    // -----------------------------------------------------------------------
    // subuid enrichment tests
    // -----------------------------------------------------------------------

    #[test]
    fn enrich_subuid_flag() {
        let mut section = UserGroupSection::default();
        section.users.push(serde_json::json!({
            "name": "alice",
        }));
        section
            .subuid_entries
            .push("alice:100000:65536".to_string());

        enrich_subuid_flags(&mut section);

        assert_eq!(section.users[0]["has_subuid"], true);
    }

    #[test]
    fn enrich_subuid_no_allocation() {
        let mut section = UserGroupSection::default();
        section.users.push(serde_json::json!({
            "name": "bob",
        }));
        section
            .subuid_entries
            .push("alice:100000:65536".to_string());

        enrich_subuid_flags(&mut section);

        assert_eq!(section.users[0]["has_subuid"], false);
    }

    // -----------------------------------------------------------------------
    // full integration: enrichment order in inspect()
    // -----------------------------------------------------------------------

    #[test]
    fn inspect_enrichment_order_all_signals() {
        let exec = MockExecutor::new()
            .with_file(
                "/etc/passwd",
                "alice:x:1000:1000:Alice:/home/alice:/bin/bash\n",
            )
            .with_file(
                "/etc/shadow",
                "alice:$6$salt$hash:19700:0:99999:7:::\n",
            )
            .with_file(
                "/etc/group",
                "root:x:0:\nwheel:x:10:alice\nalice:x:1000:\n",
            )
            .with_file(
                "/etc/subuid",
                "alice:100000:65536\n",
            )
            .with_file(
                "/etc/sudoers",
                "%wheel ALL=(ALL) ALL\n",
            )
            .with_file(
                "/home/alice/.ssh/authorized_keys",
                "ssh-rsa AAAAB3... alice@laptop\n",
            );

        let source = pkg_source();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };
        let result = UsersGroupsInspector::new().inspect(&ctx);

        // May be Degraded due to missing gshadow — extract partial.
        let output = match result {
            Ok(o) => o,
            Err(InspectorError::Degraded { partial, .. }) => *partial,
            Err(e) => panic!("unexpected error: {e:?}"),
        };

        if let SectionData::UsersGroups(section) = &output.section {
            let alice = &section.users[0];

            // All enrichment fields should be present.
            assert_eq!(alice["has_sudo"], true, "alice in wheel with %wheel rule");
            assert_eq!(alice["has_subuid"], true, "alice has subuid allocation");
            assert_eq!(alice["ssh_key_count"], 1);
            assert_eq!(alice["password_status"], "password_set");

            let groups = alice["supplementary_groups"]
                .as_array()
                .expect("supplementary_groups");
            let group_names: Vec<&str> = groups
                .iter()
                .filter_map(|v| v.as_str())
                .collect();
            assert!(
                group_names.contains(&"wheel"),
                "should include system group wheel: {group_names:?}"
            );

            // classification_rationale should include all signals.
            let rationale = alice["classification_rationale"]
                .as_str()
                .expect("rationale");
            assert!(rationale.contains("shell=bash"), "rationale: {rationale}");
            assert!(rationale.contains("sudo=yes"), "rationale: {rationale}");
            assert!(rationale.contains("ssh_keys=1"), "rationale: {rationale}");
            assert!(rationale.contains("wheel"), "rationale: {rationale}");
        } else {
            panic!("expected UsersGroups section");
        }
    }
}
