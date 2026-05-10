use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OsRelease {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version_id: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub id_like: String,
    #[serde(default)]
    pub pretty_name: String,
    #[serde(default)]
    pub variant_id: String,
}

/// System type as stored in the snapshot JSON.
/// Uses explicit renames because Go values contain hyphens.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SystemType {
    #[default]
    #[serde(rename = "unknown", alias = "")]
    Unknown,
    #[serde(rename = "package-mode")]
    PackageMode,
    #[serde(rename = "rpm-ostree")]
    RpmOstree,
    #[serde(rename = "bootc")]
    Bootc,
}

/// rpm-ostree desktop/immutable variants.
/// Pipeline-internal — not stored directly in snapshot JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "variant", content = "value")]
pub enum OstreeVariant {
    #[serde(rename = "silverblue")]
    Silverblue,
    #[serde(rename = "kinoite")]
    Kinoite,
    #[serde(rename = "sericea")]
    Sericea,
    #[serde(rename = "onyx")]
    Onyx,
    #[serde(rename = "universal_blue")]
    UniversalBlue { image_ref: String },
    #[serde(rename = "centos_stream")]
    CentOSStream { major: u8 },
    #[serde(rename = "rhel")]
    Rhel { major: u8, minor: u8 },
    #[serde(rename = "unknown")]
    Unknown(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_os_release_roundtrip() {
        let os = OsRelease {
            name: "Red Hat Enterprise Linux".into(),
            version_id: "9.4".into(),
            version: "9.4 (Plow)".into(),
            id: "rhel".into(),
            id_like: "fedora".into(),
            pretty_name: "Red Hat Enterprise Linux 9.4 (Plow)".into(),
            variant_id: String::new(),
        };
        let json = serde_json::to_string(&os).unwrap();
        let parsed: OsRelease = serde_json::from_str(&json).unwrap();
        assert_eq!(os, parsed);
    }

    #[test]
    fn test_os_release_missing_fields() {
        let json = r#"{"name":"Fedora","id":"fedora"}"#;
        let os: OsRelease = serde_json::from_str(json).unwrap();
        assert_eq!(os.name, "Fedora");
        assert_eq!(os.version_id, ""); // missing → default empty string
    }

    #[test]
    fn test_empty_system_type_normalizes_to_unknown() {
        let parsed: SystemType = serde_json::from_str(r#""""#).unwrap();
        assert_eq!(parsed, SystemType::Unknown);
    }

    #[test]
    fn test_system_type_serde() {
        assert_eq!(
            serde_json::to_string(&SystemType::PackageMode).unwrap(),
            r#""package-mode""#
        );
        assert_eq!(
            serde_json::to_string(&SystemType::RpmOstree).unwrap(),
            r#""rpm-ostree""#
        );
        let parsed: SystemType = serde_json::from_str(r#""bootc""#).unwrap();
        assert_eq!(parsed, SystemType::Bootc);
    }

    #[test]
    fn test_ostree_variant_serde() {
        let json = serde_json::to_string(&OstreeVariant::Silverblue).unwrap();
        assert_eq!(json, r#"{"variant":"silverblue"}"#);

        let ub = OstreeVariant::UniversalBlue {
            image_ref: "ghcr.io/ublue-os/bazzite:latest".into(),
        };
        let json = serde_json::to_string(&ub).unwrap();
        let parsed: OstreeVariant = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ub);
    }
}
