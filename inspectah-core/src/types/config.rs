use super::fleet::FleetPrevalence;
use serde::{Deserialize, Serialize};

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
    pub include: bool,
    #[serde(default)]
    pub tie: bool,
    #[serde(default)]
    pub tie_winner: bool,
    pub fleet: Option<FleetPrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ConfigSection {
    #[serde(default)]
    pub files: Vec<ConfigFileEntry>,
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
                include: true,
                ..Default::default()
            }],
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
