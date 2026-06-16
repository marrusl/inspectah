use super::aggregate::AggregatePrevalence;
use serde::{Deserialize, Serialize};

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
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub epoch: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub release: String,
    #[serde(default)]
    pub arch: String,
    #[serde(default)]
    pub state: PackageState,
    #[serde(default = "crate::default_true")]
    pub include: bool,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub acknowledged: bool,
    #[serde(default)]
    pub source_repo: String,
    pub aggregate: Option<AggregatePrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct VersionChange {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub arch: String,
    #[serde(default)]
    pub host_version: String,
    #[serde(default)]
    pub base_version: String,
    #[serde(default)]
    pub host_epoch: String,
    #[serde(default)]
    pub base_epoch: String,
    pub direction: VersionChangeDirection,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EnabledModuleStream {
    #[serde(default)]
    pub module_name: String,
    #[serde(default)]
    pub stream: String,
    #[serde(default)]
    pub profiles: Vec<String>,
    #[serde(default = "crate::default_true")]
    pub include: bool,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
    #[serde(default)]
    pub baseline_match: bool,
    pub aggregate: Option<AggregatePrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct VersionLockEntry {
    #[serde(default)]
    pub raw_pattern: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub epoch: i32,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub release: String,
    #[serde(default)]
    pub arch: String,
    #[serde(default = "crate::default_true")]
    pub include: bool,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
    pub aggregate: Option<AggregatePrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RpmVaEntry {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub flags: String,
    pub package: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnverifiablePackage {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoStatus {
    #[serde(default)]
    pub repo_id: String,
    #[serde(default)]
    pub repo_name: String,
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub affected_packages: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OstreePackageOverride {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub from_nevra: String,
    #[serde(default)]
    pub to_nevra: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RepoFile {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub is_default_repo: bool,
    #[serde(default = "crate::default_true")]
    pub include: bool,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
    pub aggregate: Option<AggregatePrevalence>,
}

/// A single package's file ownership entry: package name and owned paths.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileOwnershipEntry {
    #[serde(default)]
    pub package_name: String,
    #[serde(default)]
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct InstalledGroup {
    #[serde(default)]
    pub name: String,
    #[serde(default, alias = "packages")]
    pub members: Vec<String>,
    #[serde(default)]
    pub optional_installed: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RpmSection {
    #[serde(default)]
    pub packages_added: Vec<PackageEntry>,
    #[serde(default)]
    pub base_image_only: Vec<PackageEntry>,
    #[serde(default)]
    pub rpm_va: Vec<RpmVaEntry>,
    #[serde(default)]
    pub repo_files: Vec<RepoFile>,
    #[serde(default)]
    pub gpg_keys: Vec<RepoFile>,
    #[serde(default)]
    pub dnf_history_removed: Vec<String>,
    #[serde(default)]
    pub version_changes: Vec<VersionChange>,
    /// Canonical `name.arch` identities for authoritative leaf packages.
    /// `None` means leaf classification was unavailable or degraded and
    /// downstream consumers must treat the snapshot as "leaf truth unavailable."
    pub leaf_packages: Option<Vec<String>>,
    /// Canonical `name.arch` identities for authoritative auto/transitive
    /// packages. This must be `None` whenever `leaf_packages` is `None`.
    pub auto_packages: Option<Vec<String>>,
    /// Maps authoritative leaf `name.arch` identities to their auto dependency
    /// `name.arch` identities. When classification is unavailable or degraded,
    /// this must serialize as an empty object.
    #[serde(default)]
    pub leaf_dep_tree: serde_json::Value,
    #[serde(default)]
    pub module_streams: Vec<EnabledModuleStream>,
    #[serde(default)]
    pub version_locks: Vec<VersionLockEntry>,
    #[serde(default)]
    pub module_stream_conflicts: Vec<String>,
    pub baseline_module_streams: Option<std::collections::HashMap<String, String>>,
    pub versionlock_command_output: Option<String>,
    #[serde(default)]
    pub multiarch_packages: Vec<String>,
    #[serde(default)]
    pub duplicate_packages: Vec<String>,
    #[serde(default)]
    pub repo_providing_packages: Vec<String>,
    #[serde(default)]
    pub ostree_overrides: Vec<OstreePackageOverride>,
    #[serde(default)]
    pub ostree_removals: Vec<String>,
    pub base_image: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_suppressed: Option<Vec<String>>,
    pub baseline_package_names: Option<Vec<String>>,
    /// Number of hosts with authoritative leaf classification data.
    /// Only meaningful for aggregated snapshots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leaf_authority_hosts: Option<u32>,
    /// Total number of hosts in the aggregate.
    /// Only meaningful for aggregated snapshots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leaf_total_hosts: Option<u32>,
    /// File ownership data from `rpm -qa --queryformat '%{NAME}\t[%{FILENAMES}\n]'`.
    /// Each entry maps a package name to the filesystem paths it owns.
    #[serde(default)]
    pub file_ownership: Vec<FileOwnershipEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_groups: Option<Vec<InstalledGroup>>,
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
        assert_eq!(
            serde_json::to_string(&PackageState::Added).unwrap(),
            r#""added""#
        );
        assert_eq!(
            serde_json::to_string(&PackageState::BaseImageOnly).unwrap(),
            r#""base_image_only""#
        );
        assert_eq!(
            serde_json::to_string(&PackageState::LocalInstall).unwrap(),
            r#""local_install""#
        );
        assert_eq!(
            serde_json::to_string(&PackageState::NoRepo).unwrap(),
            r#""no_repo""#
        );
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

    #[test]
    fn test_rpm_section_leaf_metadata_preserves_canonical_package_ids() {
        let section = RpmSection {
            leaf_packages: Some(vec!["vim.x86_64".into()]),
            auto_packages: Some(vec!["glibc.x86_64".into(), "ncurses.x86_64".into()]),
            leaf_dep_tree: serde_json::json!({
                "vim.x86_64": ["glibc.x86_64", "ncurses.x86_64"]
            }),
            ..Default::default()
        };

        let json = serde_json::to_value(&section).unwrap();
        assert_eq!(json["leaf_packages"], serde_json::json!(["vim.x86_64"]));
        assert_eq!(
            json["auto_packages"],
            serde_json::json!(["glibc.x86_64", "ncurses.x86_64"])
        );
        assert_eq!(
            json["leaf_dep_tree"],
            serde_json::json!({"vim.x86_64": ["glibc.x86_64", "ncurses.x86_64"]})
        );
    }

    #[test]
    fn test_baseline_suppressed_roundtrip() {
        let rpm = RpmSection {
            baseline_suppressed: Some(vec!["kernel.x86_64".into(), "dosfstools.x86_64".into()]),
            ..Default::default()
        };
        let json = serde_json::to_value(&rpm).unwrap();
        assert_eq!(
            json["baseline_suppressed"],
            serde_json::json!(["kernel.x86_64", "dosfstools.x86_64"])
        );
    }

    #[test]
    fn test_baseline_suppressed_none_when_absent() {
        let json = r#"{"packages_added":[],"version_changes":[],"leaf_dep_tree":{}}"#;
        let parsed: RpmSection = serde_json::from_str(json).unwrap();
        assert!(parsed.baseline_suppressed.is_none());
    }

    #[test]
    fn test_baseline_suppressed_some_empty_when_baseline_exists_but_nothing_suppressed() {
        let rpm = RpmSection {
            baseline_suppressed: Some(Vec::new()),
            ..Default::default()
        };
        let json = serde_json::to_value(&rpm).unwrap();
        assert_eq!(json["baseline_suppressed"], serde_json::json!([]));
    }

    #[test]
    fn test_rpm_section_leaf_metadata_serializes_unavailable_classification() {
        let section = RpmSection {
            leaf_packages: None,
            auto_packages: None,
            leaf_dep_tree: serde_json::json!({}),
            ..Default::default()
        };

        let json = serde_json::to_value(&section).unwrap();
        assert_eq!(json["leaf_packages"], serde_json::Value::Null);
        assert_eq!(json["auto_packages"], serde_json::Value::Null);
        assert_eq!(json["leaf_dep_tree"], serde_json::json!({}));
    }

    #[test]
    fn test_installed_group_roundtrip() {
        let group = InstalledGroup {
            name: "Container Management".into(),
            members: vec!["podman".into(), "buildah".into(), "skopeo".into()],
            ..Default::default()
        };
        let json = serde_json::to_string(&group).unwrap();
        let parsed: InstalledGroup = serde_json::from_str(&json).unwrap();
        assert_eq!(group, parsed);
    }

    #[test]
    fn installed_group_new_fields_round_trip() {
        let group = InstalledGroup {
            name: "Container Management".into(),
            members: vec!["podman".into(), "buildah".into()],
            optional_installed: vec!["python3-podman".into()],
        };
        let json = serde_json::to_string(&group).unwrap();
        let back: InstalledGroup = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "Container Management");
        assert_eq!(back.members, vec!["podman", "buildah"]);
        assert_eq!(back.optional_installed, vec!["python3-podman"]);
    }

    #[test]
    fn installed_group_old_format_loads_via_alias() {
        // Old snapshots use "packages" instead of "members"
        let json = r#"{"name":"Dev Tools","packages":["gcc","make"]}"#;
        let group: InstalledGroup = serde_json::from_str(json).unwrap();
        assert_eq!(group.members, vec!["gcc", "make"]);
        assert!(group.optional_installed.is_empty());
    }

    #[test]
    fn test_rpm_section_installed_groups_none_vs_empty() {
        let section_none = RpmSection {
            ..Default::default()
        };
        let json_none = serde_json::to_string(&section_none).unwrap();
        let parsed_none: RpmSection = serde_json::from_str(&json_none).unwrap();
        assert!(parsed_none.installed_groups.is_none());

        let section_empty = RpmSection {
            installed_groups: Some(vec![]),
            ..Default::default()
        };
        let json_empty = serde_json::to_string(&section_empty).unwrap();
        let parsed_empty: RpmSection = serde_json::from_str(&json_empty).unwrap();
        assert_eq!(parsed_empty.installed_groups, Some(vec![]));
    }
}
