use super::config::ConfigFileEntry;
use super::fleet::FleetPrevalence;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipPackage {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NonRpmItem {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub method: String,
    #[serde(default)]
    pub confidence: String,
    #[serde(default)]
    pub include: bool,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub acknowledged: bool,
    #[serde(default)]
    pub lang: String,
    #[serde(default)]
    pub r#static: bool,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub shared_libs: Vec<String>,
    #[serde(default)]
    pub system_site_packages: bool,
    #[serde(default)]
    pub packages: Vec<PipPackage>,
    #[serde(default)]
    pub has_c_extensions: bool,
    #[serde(default)]
    pub git_remote: String,
    #[serde(default)]
    pub git_commit: String,
    #[serde(default)]
    pub git_branch: String,
    pub files: Option<serde_json::Value>,
    #[serde(default)]
    pub content: String,
    pub fleet: Option<FleetPrevalence>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub review_status: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NonRpmSoftwareSection {
    #[serde(default)]
    pub items: Vec<NonRpmItem>,
    #[serde(default)]
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
                locked: false,
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
