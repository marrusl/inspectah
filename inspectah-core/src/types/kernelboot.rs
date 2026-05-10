use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigSnippet {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub path: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub content: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SysctlOverride {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub key: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub runtime: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub default: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub source: String,
    #[serde(default)]
    pub include: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KernelModule {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub name: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub size: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub used_by: String,
    #[serde(default)]
    pub include: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlternativeEntry {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub name: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub path: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub status: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct KernelBootSection {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub cmdline: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub grub_defaults: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub sysctl_overrides: Vec<SysctlOverride>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub modules_load_d: Vec<ConfigSnippet>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub modprobe_d: Vec<ConfigSnippet>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub dracut_conf: Vec<ConfigSnippet>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub loaded_modules: Vec<KernelModule>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub non_default_modules: Vec<KernelModule>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub tuned_active: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub tuned_custom_profiles: Vec<ConfigSnippet>,
    pub locale: Option<String>,
    pub timezone: Option<String>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub alternatives: Vec<AlternativeEntry>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kernelboot_section_roundtrip() {
        let section = KernelBootSection {
            cmdline: "quiet crashkernel=auto".to_string(),
            grub_defaults: "GRUB_TIMEOUT=5".to_string(),
            sysctl_overrides: vec![SysctlOverride {
                key: "kernel.sysrq".to_string(),
                runtime: "16".to_string(),
                default: "0".to_string(),
                source: "/etc/sysctl.d/99-custom.conf".to_string(),
                include: true,
            }],
            modules_load_d: vec![],
            modprobe_d: vec![],
            dracut_conf: vec![],
            loaded_modules: vec![],
            non_default_modules: vec![],
            tuned_active: "virtual-guest".to_string(),
            tuned_custom_profiles: vec![],
            locale: Some("en_US.UTF-8".to_string()),
            timezone: Some("America/New_York".to_string()),
            alternatives: vec![],
        };

        let json = serde_json::to_string(&section).unwrap();
        let parsed: KernelBootSection = serde_json::from_str(&json).unwrap();
        assert_eq!(section, parsed);
    }
}
