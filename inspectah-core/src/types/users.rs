use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UserGroupSection {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub users: Vec<serde_json::Value>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub groups: Vec<serde_json::Value>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub sudoers_rules: Vec<String>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub ssh_authorized_keys_refs: Vec<serde_json::Value>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub passwd_entries: Vec<String>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub shadow_entries: Vec<String>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub group_entries: Vec<String>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub gshadow_entries: Vec<String>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub subuid_entries: Vec<String>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
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
