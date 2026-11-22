use super::aggregate::{AggregatePrevalence, VariantSelection};
use super::{AdvisoryType, FindingKind};
use serde::{Deserialize, Serialize};

/// authselect's record of the selected profile and its features: the
/// profile id on the first line, one feature per line after it. The
/// export carries a config entry at this path as an `authselect select`
/// line, never as a copied file.
pub const AUTHSELECT_CONF_PATH: &str = "/etc/authselect/authselect.conf";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigFileKind {
    RpmOwnedDefault,
    RpmOwnedModified,
    #[default]
    Unowned,
    Orphaned,
    #[serde(alias = "baseline_match")]
    BaselineMatch,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigCategory {
    Tmpfiles,
    Environment,
    Audit,
    /// `/etc/pam.d/`.
    Pam,
    /// `/etc/tuned/`.
    Tuned,
    /// `/etc/modules-load.d/` and `/etc/modprobe.d/` — which kernel
    /// modules load and with what options.
    KernelModule,
    /// `/etc/dracut.conf.d/` — how the initramfs is built.
    Initramfs,
    LibraryPath,
    Journal,
    Logrotate,
    Automount,
    Sysctl,
    CryptoPolicy,
    Identity,
    Limits,
    #[default]
    Other,
}

/// The decision a config file's content travels under when it is not the
/// file's own. A shadowed entry is inventory: its content reaches the
/// image through the shadowing decision and never through a copy of the
/// file itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigShadow {
    /// The file's path appears in `units.drop_ins`, which materializes
    /// it under `drop-ins/` on that entry's own decision.
    ///
    /// Named for the owning set rather than for the field's name:
    /// `units.drop_ins` carries full unit shadows under
    /// `/etc/systemd/system/` as well as `*.service.d/*.conf`, so a
    /// `ServiceDropIn` spelling would promise less than the variant owns.
    Units,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ConfigFileEntry {
    #[serde(default)]
    pub path: String,
    pub kind: ConfigFileKind,
    pub category: ConfigCategory,
    #[serde(default)]
    pub content: String,
    pub rpm_va_flags: Option<String>,
    pub package: Option<String>,
    pub diff_against_rpm: Option<String>,
    #[serde(default)]
    pub disposition: FindingKind,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
    #[serde(default)]
    pub variant_selection: VariantSelection,
    pub aggregate: Option<AggregatePrevalence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attention_reason: Option<String>,
    /// Set when another section's decision carries this file's content.
    /// `disposition` is `Inventory` whenever this is `Some`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadowed_by: Option<ConfigShadow>,
}

impl ConfigFileEntry {
    /// Whether the collector captured this file's content.
    ///
    /// False for a `ContentNotCaptured` advisory: the file exists on the
    /// host, but `content` is empty because the read failed, not because
    /// the file is. Such an entry is no evidence of what the file holds.
    pub fn content_captured(&self) -> bool {
        !matches!(
            self.disposition,
            FindingKind::Advisory {
                advisory_type: AdvisoryType::ContentNotCaptured,
                ..
            }
        )
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ConfigSection {
    #[serde(default)]
    pub files: Vec<ConfigFileEntry>,
    /// Machine-state files whose content the snapshot does not hold.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub withheld: Vec<super::machine_state::WithheldConfigEntry>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_section_roundtrip() {
        let section = ConfigSection {
            files: vec![ConfigFileEntry {
                path: "/etc/httpd/conf/httpd.conf".into(),
                kind: ConfigFileKind::RpmOwnedModified,
                category: ConfigCategory::Other,
                content: "ServerRoot \"/etc/httpd\"".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let json = serde_json::to_string(&section).unwrap();
        let parsed: ConfigSection = serde_json::from_str(&json).unwrap();
        assert_eq!(section, parsed);
    }

    #[test]
    fn test_baseline_match_roundtrip() {
        assert_eq!(
            serde_json::to_string(&ConfigFileKind::BaselineMatch).unwrap(),
            r#""baseline_match""#
        );
        let parsed: ConfigFileKind = serde_json::from_str(r#""baseline_match""#).unwrap();
        assert_eq!(parsed, ConfigFileKind::BaselineMatch);
    }

    #[test]
    fn test_config_file_kind_values() {
        assert_eq!(
            serde_json::to_string(&ConfigFileKind::RpmOwnedDefault).unwrap(),
            r#""rpm_owned_default""#
        );
        assert_eq!(
            serde_json::to_string(&ConfigFileKind::RpmOwnedModified).unwrap(),
            r#""rpm_owned_modified""#
        );
    }
}
