use serde::{Deserialize, Serialize};

use super::fleet::FleetPrevalence;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigSnippet {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub content: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SysctlOverride {
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub runtime: String,
    #[serde(default)]
    pub default: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub include: bool,
    pub fleet: Option<FleetPrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KernelModule {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub size: String,
    #[serde(default)]
    pub used_by: String,
    #[serde(default)]
    pub include: bool,
    pub fleet: Option<FleetPrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlternativeEntry {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub status: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct KernelBootSection {
    #[serde(default)]
    pub cmdline: String,
    #[serde(default)]
    pub grub_defaults: String,
    #[serde(default)]
    pub sysctl_overrides: Vec<SysctlOverride>,
    #[serde(default)]
    pub modules_load_d: Vec<ConfigSnippet>,
    #[serde(default)]
    pub modprobe_d: Vec<ConfigSnippet>,
    #[serde(default)]
    pub dracut_conf: Vec<ConfigSnippet>,
    #[serde(default)]
    pub loaded_modules: Vec<KernelModule>,
    #[serde(default)]
    pub non_default_modules: Vec<KernelModule>,
    #[serde(default)]
    pub tuned_active: String,
    #[serde(default)]
    pub tuned_custom_profiles: Vec<ConfigSnippet>,
    pub locale: Option<String>,
    pub timezone: Option<String>,
    #[serde(default)]
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
                fleet: None,
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
