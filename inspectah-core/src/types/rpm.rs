use serde::{Deserialize, Serialize};
use super::fleet::FleetPrevalence;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageState {
    #[default]
    Added,
    BaseImageOnly,
    Modified,
    LocalInstall,
    NoRepo,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VersionChangeDirection {
    #[default]
    Upgrade,
    Downgrade,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PackageEntry {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub name: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub epoch: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub version: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub release: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub arch: String,
    pub state: PackageState,
    #[serde(default)]
    pub include: bool,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub acknowledged: bool,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub source_repo: String,
    pub fleet: Option<FleetPrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct VersionChange {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub name: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub arch: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub host_version: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub base_version: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub host_epoch: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub base_epoch: String,
    pub direction: VersionChangeDirection,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EnabledModuleStream {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub module_name: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub stream: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub profiles: Vec<String>,
    #[serde(default)]
    pub include: bool,
    #[serde(default)]
    pub baseline_match: bool,
    pub fleet: Option<FleetPrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct VersionLockEntry {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub raw_pattern: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub name: String,
    #[serde(default)]
    pub epoch: i32,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub version: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub release: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub arch: String,
    #[serde(default)]
    pub include: bool,
    pub fleet: Option<FleetPrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RpmVaEntry {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub path: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub flags: String,
    pub package: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnverifiablePackage {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub name: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoStatus {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub repo_id: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub repo_name: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub error: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub affected_packages: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OstreePackageOverride {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub name: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub from_nevra: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub to_nevra: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RepoFile {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub path: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub content: String,
    #[serde(default)]
    pub is_default_repo: bool,
    #[serde(default)]
    pub include: bool,
    pub fleet: Option<FleetPrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RpmSection {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub packages_added: Vec<PackageEntry>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub base_image_only: Vec<PackageEntry>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub rpm_va: Vec<RpmVaEntry>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub repo_files: Vec<RepoFile>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub gpg_keys: Vec<RepoFile>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub dnf_history_removed: Vec<String>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub version_changes: Vec<VersionChange>,
    pub leaf_packages: Option<Vec<String>>,
    pub auto_packages: Option<Vec<String>>,
    #[serde(default)]
    pub leaf_dep_tree: serde_json::Value,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub module_streams: Vec<EnabledModuleStream>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub version_locks: Vec<VersionLockEntry>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub module_stream_conflicts: Vec<String>,
    pub baseline_module_streams: Option<std::collections::HashMap<String, String>>,
    pub versionlock_command_output: Option<String>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub multiarch_packages: Vec<String>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub duplicate_packages: Vec<String>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub repo_providing_packages: Vec<String>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub ostree_overrides: Vec<OstreePackageOverride>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub ostree_removals: Vec<String>,
    pub base_image: Option<String>,
    pub baseline_package_names: Option<Vec<String>>,
    #[serde(default)]
    pub no_baseline: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_package_entry_roundtrip() {
        let entry = PackageEntry {
            name: "httpd".into(),
            epoch: "0".into(),
            version: "2.4.57".into(),
            release: "5.el9".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            include: true,
            source_repo: "appstream".into(),
            ..Default::default()
        };
        let json = serde_json::to_string(&entry).unwrap();
        let parsed: PackageEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(entry, parsed);
    }

    #[test]
    fn test_package_state_json_values() {
        assert_eq!(serde_json::to_string(&PackageState::Added).unwrap(), r#""added""#);
        assert_eq!(serde_json::to_string(&PackageState::BaseImageOnly).unwrap(), r#""base_image_only""#);
        assert_eq!(serde_json::to_string(&PackageState::LocalInstall).unwrap(), r#""local_install""#);
        assert_eq!(serde_json::to_string(&PackageState::NoRepo).unwrap(), r#""no_repo""#);
    }

    #[test]
    fn test_rpm_section_default_roundtrip() {
        let section = RpmSection::default();
        let json = serde_json::to_string(&section).unwrap();
        let parsed: RpmSection = serde_json::from_str(&json).unwrap();
        assert_eq!(section, parsed);
    }

    #[test]
    fn test_rpm_section_with_data() {
        let section = RpmSection {
            packages_added: vec![PackageEntry {
                name: "vim-enhanced".into(),
                state: PackageState::Added,
                include: true,
                ..Default::default()
            }],
            version_changes: vec![VersionChange {
                name: "bash".into(),
                arch: "x86_64".into(),
                host_version: "5.2.26".into(),
                base_version: "5.2.15".into(),
                direction: VersionChangeDirection::Upgrade,
                ..Default::default()
            }],
            base_image: Some("registry.redhat.io/rhel9/rhel-bootc:9.4".into()),
            ..Default::default()
        };
        let json = serde_json::to_string_pretty(&section).unwrap();
        let parsed: RpmSection = serde_json::from_str(&json).unwrap();
        assert_eq!(section, parsed);
    }
}
