use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum UserContainerfileStrategy {
    #[default]
    Skip,
    Useradd,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum UserPasswordChoice {
    #[default]
    None,
    Preserve,
    New,
}

/// Typed representation of a user decision as surfaced in `ViewResponse`.
///
/// Deserialised from the `serde_json::Value` objects in
/// `UserGroupSection.users`; serialises to the same JSON shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserGroupDecision {
    pub name: String,
    pub uid: u64,
    pub gid: u64,
    pub shell: String,
    pub home: String,
    #[serde(default = "default_true")]
    pub include: bool,
    pub classification: String,
    pub containerfile_strategy: UserContainerfileStrategy,
    pub password_choice: UserPasswordChoice,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_sudo: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_subuid: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_key_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_keys: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classification_rationale: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supplementary_groups: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password_status: Option<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UserGroupSection {
    #[serde(default)]
    pub users: Vec<serde_json::Value>,
    #[serde(default)]
    pub groups: Vec<serde_json::Value>,
    #[serde(default)]
    pub sudoers_rules: Vec<String>,
    #[serde(default)]
    pub ssh_authorized_keys_refs: Vec<serde_json::Value>,
    #[serde(default)]
    pub passwd_entries: Vec<String>,
    #[serde(default)]
    pub shadow_entries: Vec<String>,
    #[serde(default)]
    pub group_entries: Vec<String>,
    #[serde(default)]
    pub gshadow_entries: Vec<String>,
    #[serde(default)]
    pub subuid_entries: Vec<String>,
    #[serde(default)]
    pub subgid_entries: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_containerfile_strategy_roundtrip() {
        let skip: UserContainerfileStrategy = serde_json::from_str("\"skip\"").unwrap();
        assert_eq!(skip, UserContainerfileStrategy::Skip);
        let useradd: UserContainerfileStrategy = serde_json::from_str("\"useradd\"").unwrap();
        assert_eq!(useradd, UserContainerfileStrategy::Useradd);
    }

    #[test]
    fn user_password_choice_roundtrip() {
        let none: UserPasswordChoice = serde_json::from_str("\"none\"").unwrap();
        assert_eq!(none, UserPasswordChoice::None);
        let preserve: UserPasswordChoice = serde_json::from_str("\"preserve\"").unwrap();
        assert_eq!(preserve, UserPasswordChoice::Preserve);
        let new: UserPasswordChoice = serde_json::from_str("\"new\"").unwrap();
        assert_eq!(new, UserPasswordChoice::New);
    }

    #[test]
    fn user_decision_enum_defaults() {
        assert_eq!(
            UserContainerfileStrategy::default(),
            UserContainerfileStrategy::Skip
        );
        assert_eq!(UserPasswordChoice::default(), UserPasswordChoice::None);
    }

    #[test]
    fn test_usergroup_section_roundtrip() {
        let section = UserGroupSection {
            users: vec![serde_json::json!({
                "name": "testuser",
                "uid": 1000,
                "gid": 1000
            })],
            groups: vec![serde_json::json!({
                "name": "testgroup",
                "gid": 1000
            })],
            sudoers_rules: vec!["testuser ALL=(ALL) NOPASSWD:ALL".to_string()],
            ssh_authorized_keys_refs: vec![],
            passwd_entries: vec!["testuser:x:1000:1000::/home/testuser:/bin/bash".to_string()],
            shadow_entries: vec![],
            group_entries: vec!["testgroup:x:1000:".to_string()],
            gshadow_entries: vec![],
            subuid_entries: vec![],
            subgid_entries: vec![],
        };

        let json = serde_json::to_string(&section).unwrap();
        let parsed: UserGroupSection = serde_json::from_str(&json).unwrap();
        assert_eq!(section, parsed);
    }

    #[test]
    fn user_group_decision_roundtrip_full() {
        let decision = UserGroupDecision {
            name: "alice".to_string(),
            uid: 1000,
            gid: 1000,
            shell: "/bin/bash".to_string(),
            home: "/home/alice".to_string(),
            include: true,
            classification: "interactive".to_string(),
            containerfile_strategy: UserContainerfileStrategy::Useradd,
            password_choice: UserPasswordChoice::Preserve,
            password_hash: Some("$6$rounds=...".to_string()),
            has_sudo: Some(true),
            has_subuid: Some(false),
            ssh_key_count: Some(2),
            ssh_keys: Some(vec!["ssh-ed25519 AAAA...".to_string()]),
            classification_rationale: Some("interactive shell".to_string()),
            supplementary_groups: Some(vec!["wheel".to_string(), "docker".to_string()]),
            password_status: Some("password_set".to_string()),
        };

        let json = serde_json::to_string(&decision).unwrap();
        let parsed: UserGroupDecision = serde_json::from_str(&json).unwrap();
        assert_eq!(decision, parsed);
    }

    #[test]
    fn user_group_decision_from_collector_json() {
        // Simulates the JSON shape produced by the collector.
        let val = serde_json::json!({
            "name": "bob",
            "uid": 1001,
            "gid": 1001,
            "shell": "/sbin/nologin",
            "home": "/home/bob",
            "include": true,
            "classification": "non-interactive",
            "containerfile_strategy": "skip",
            "password_choice": "none"
        });

        let decision: UserGroupDecision = serde_json::from_value(val).unwrap();
        assert_eq!(decision.name, "bob");
        assert_eq!(
            decision.containerfile_strategy,
            UserContainerfileStrategy::Skip
        );
        assert_eq!(decision.password_choice, UserPasswordChoice::None);
        assert!(decision.has_sudo.is_none());
        assert!(decision.supplementary_groups.is_none());
    }

    #[test]
    fn user_group_decision_include_defaults_true() {
        // When `include` is missing from JSON, it should default to true.
        let val = serde_json::json!({
            "name": "charlie",
            "uid": 1002,
            "gid": 1002,
            "shell": "/bin/bash",
            "home": "/home/charlie",
            "classification": "interactive",
            "containerfile_strategy": "skip",
            "password_choice": "none"
        });

        let decision: UserGroupDecision = serde_json::from_value(val).unwrap();
        assert!(decision.include);
    }
}
