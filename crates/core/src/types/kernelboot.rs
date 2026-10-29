use super::FindingKind;
use serde::{Deserialize, Serialize};

use super::aggregate::AggregatePrevalence;

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
    pub disposition: FindingKind,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
    pub aggregate: Option<AggregatePrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KernelModule {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub size: String,
    #[serde(default)]
    pub used_by: String,
    // A loaded module is informational: nothing in the Containerfile
    // mounts a decision on it, so an absent key must not read back as
    // an actionable, included finding.
    #[serde(default = "crate::types::finding::default_finding_inventory")]
    pub disposition: FindingKind,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
    pub aggregate: Option<AggregatePrevalence>,
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
    pub loaded_modules: Vec<KernelModule>,
    #[serde(default)]
    pub tuned_active: String,
    // Field-level `serde(default)` uses `FindingKind::default()` (actionable,
    // included). A tuned profile that a snapshot never recorded a decision for
    // must not be baked into the Containerfile, so the default is excluded.
    #[serde(default = "crate::types::finding::default_finding_excluded")]
    pub tuned_disposition: FindingKind,
    pub locale: Option<String>,
    pub timezone: Option<String>,
    #[serde(default)]
    pub alternatives: Vec<AlternativeEntry>,
}

/// Returns true if the given tuned profile is a stock default that tuned's
/// recommendation engine would auto-select. These profiles ship with the
/// tuned package and are not admin-configured. Setting them to
/// profile_mode=manual in the Containerfile would override the automatic
/// selection, which is almost never the intent.
pub fn is_stock_tuned_profile(name: &str) -> bool {
    matches!(
        name,
        "throughput-performance"
            | "virtual-guest"
            | "balanced"
            | "desktop"
            | "latency-performance"
            | "network-latency"
            | "network-throughput"
            | "powersave"
            | "hpc-compute"
            | "intel-sst"
            | "optimize-serial-console"
            | "aws"
            | "atomic-guest"
            | "atomic-host"
    )
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
                disposition: FindingKind::included(),
                locked: false,
                aggregate: None,
            }],
            loaded_modules: vec![],
            tuned_active: "virtual-guest".to_string(),
            tuned_disposition: FindingKind::included(),
            locale: Some("en_US.UTF-8".to_string()),
            timezone: Some("America/New_York".to_string()),
            alternatives: vec![],
        };

        let json = serde_json::to_string(&section).unwrap();
        let parsed: KernelBootSection = serde_json::from_str(&json).unwrap();
        assert_eq!(section, parsed);
    }

    #[test]
    fn absent_tuned_disposition_deserializes_as_excluded() {
        let json = r#"{"cmdline":"quiet","tuned_active":"my-custom-profile"}"#;
        let section: KernelBootSection = serde_json::from_str(json).unwrap();
        assert!(
            !section.tuned_disposition.is_included(),
            "absent tuned_disposition must not deserialize as included — an \
             undecided profile would be baked into the Containerfile"
        );
        assert_eq!(section.tuned_disposition, FindingKind::excluded());
    }

    #[test]
    fn sysctl_override_without_include_deserializes_as_true() {
        let json = r#"{"key":"kernel.sysrq","runtime":"16","default":"0","source":"/etc/sysctl.d/99-custom.conf"}"#;
        let so: SysctlOverride = serde_json::from_str(json).unwrap();
        assert!(
            so.disposition.is_included(),
            "missing include field should deserialize as true"
        );
    }
}
