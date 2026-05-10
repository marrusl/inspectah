use serde::{Deserialize, Serialize};
use super::fleet::FleetPrevalence;
use super::config::ConfigFileEntry;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipPackage {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub name: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub version: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NonRpmItem {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub path: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub name: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub method: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub confidence: String,
    #[serde(default)]
    pub include: bool,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub acknowledged: bool,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub lang: String,
    #[serde(default)]
    pub r#static: bool,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub version: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub shared_libs: Vec<String>,
    #[serde(default)]
    pub system_site_packages: bool,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub packages: Vec<PipPackage>,
    #[serde(default)]
    pub has_c_extensions: bool,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub git_remote: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub git_commit: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub git_branch: String,
    pub files: Option<serde_json::Value>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub content: String,
    pub fleet: Option<FleetPrevalence>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string", skip_serializing_if = "String::is_empty")]
    pub review_status: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string", skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NonRpmSoftwareSection {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub items: Vec<NonRpmItem>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub env_files: Vec<ConfigFileEntry>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nonrpm_section_roundtrip() {
        let section = NonRpmSoftwareSection {
            items: vec![NonRpmItem {
                path: "/opt/app/bin".to_string(),
                name: "custom-app".to_string(),
                method: "binary".to_string(),
                confidence: "high".to_string(),
                include: true,
                acknowledged: false,
                lang: "c".to_string(),
                r#static: true,
                version: "1.0.0".to_string(),
                shared_libs: vec!["/lib64/libc.so.6".to_string()],
                system_site_packages: false,
                packages: vec![],
                has_c_extensions: false,
                git_remote: String::new(),
                git_commit: String::new(),
                git_branch: String::new(),
                files: None,
                content: String::new(),
                fleet: None,
                review_status: String::new(),
                notes: String::new(),
            }],
            env_files: vec![],
        };

        let json = serde_json::to_string(&section).unwrap();
        let parsed: NonRpmSoftwareSection = serde_json::from_str(&json).unwrap();
        assert_eq!(section, parsed);
    }
}
