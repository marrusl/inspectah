use serde::{Deserialize, Serialize};

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
