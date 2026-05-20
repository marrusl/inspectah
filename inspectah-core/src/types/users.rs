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
}
