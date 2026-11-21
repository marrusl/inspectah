use super::FindingKind;
use super::aggregate::AggregatePrevalence;
use super::repo::{RepoAttribution, RepoId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

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
    #[serde(default)]
    pub disposition: FindingKind,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub acknowledged: bool,
    #[serde(default)]
    pub source_repo: String,
    /// How `source_repo` relates to the host's `.repo` sections. See
    /// [`RepoAttribution`]; `source_repo` keeps its historical meaning
    /// (the resolved id for `Exact`/`Resolved`, the recorded value
    /// otherwise).
    #[serde(default)]
    pub repo_attribution: RepoAttribution,
    /// The repo sections that serve this package by name, when it also
    /// provides the repository configuration those sections come from.
    /// Non-empty means repo-enabling: the host's enabled repos proved
    /// they serve it, so the scan leaves it out of the repo-less set.
    ///
    /// It does *not* on its own mean the export will install it by name.
    /// Includes are decided after the scan, and an export that drops the
    /// serving definition demotes the package back to the repo-less
    /// rendering -- keeping this field, which stays true, and gaining a
    /// `repoless_annotation` beside it. `render::repo_enabling` is the
    /// one place that decides between the two.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repo_enabling_from: Vec<RepoId>,
    /// Triage annotation for repo-less packages.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub repoless_annotation: String,
    /// True if a cached RPM was found in /var/cache/dnf/.
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub repoless_cached: bool,
    /// Set when the refine session staged an upload for this package;
    /// distinct from `repoless_cached`, which also covers RPMs found in the
    /// dnf cache.
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub uploaded: bool,
    /// Path to the cached RPM file in /var/cache/dnf/.
    /// Survives serialization into the snapshot so the CLI bundler
    /// can read it when creating the tarball.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_path: Option<String>,
    /// Version of the uploaded RPM (from filename), if different from installed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uploaded_version: Option<String>,
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
    #[serde(default)]
    pub disposition: FindingKind,
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
    #[serde(default)]
    pub disposition: FindingKind,
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

/// A captured `.repo` file (in `RpmSection::repo_files`) or GPG key file
/// (in `RpmSection::gpg_keys`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RepoFile {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub is_default_repo: bool,
    #[serde(default)]
    pub disposition: FindingKind,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
    pub aggregate: Option<AggregatePrevalence>,
    /// For a GPG key: the `gpgkey=` paths that named it, as the repo file
    /// writes them (dnf variables unexpanded, `file://` removed), where
    /// they differ from `path`. EL10's `epel.repo` names
    /// `RPM-GPG-KEY-EPEL-$releasever_major`; the key is recorded at
    /// `RPM-GPG-KEY-EPEL-10` with that literal here. Empty for a repo file,
    /// for a key named literally, and in snapshots collected before the
    /// field existed. Match a section's key paths with
    /// [`RepoFile::is_key_named_by`], never by `path` alone.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub gpgkey_references: BTreeSet<String>,
    /// For a GPG key the base image already ships: the package whose name
    /// is in the baseline and which owns the path on the source host, where
    /// `rpm -V` found the file unmodified. The scan records such a key
    /// excluded, because the image has the base image's own copy; copying
    /// the host's over it would replace a newer major's key with an older
    /// one. `None` for a key the export carries by default, for a repo
    /// file, and in snapshots collected before the field existed.
    ///
    /// The match is by package name only, so it assumes the base's package
    /// of that name ships the same path, at whatever version the base has.
    /// A base version that renamed or dropped the file would leave the
    /// image without the key. Checking that needs the base image's file
    /// lists, which the baseline does not carry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shipped_by_base: Option<String>,
}

impl RepoFile {
    /// Whether this captured GPG key is the file a `gpgkey=` path names,
    /// written the way `RepoSectionDef::gpg_key_paths` holds it: its
    /// expanded path, or a literal path recorded in `gpgkey_references`.
    ///
    /// The one place a section's key reference is matched to a captured
    /// key. In an aggregate one literal can name several keys (an EL9 and
    /// an EL10 host expand `$releasever_major` differently), and each of
    /// them matches.
    pub fn is_key_named_by(&self, gpgkey_path: &str) -> bool {
        self.path == gpgkey_path || self.gpgkey_references.contains(gpgkey_path)
    }

    /// Why the scan leaves this key out of the export, when the reason is
    /// that the base image ships it. The one wording every surface uses.
    pub fn base_image_ships_reason(&self) -> Option<String> {
        self.shipped_by_base.as_ref().map(|package| {
            format!("the base image ships it ({package}, unmodified on the source host)")
        })
    }

    /// Whether the built image has this file at its path: the export
    /// copies it, or the base image already ships it.
    pub fn is_in_image(&self) -> bool {
        self.disposition.is_included() || self.shipped_by_base.is_some()
    }
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
    /// Maps every authoritative leaf and auto `name.arch` identity to the
    /// added identities installing it installs, leaves and auto packages
    /// alike, the package itself left out. Whatever the export installs
    /// brings these with it: a leaf, an auto package carried because it
    /// owns an enabled unit, or one an operator included.
    ///
    /// `leaf_dep_tree` is the auto part of a leaf's entry. Unlike it, this
    /// keeps a dependency the operator also installed, which is what lets
    /// an absence claim rest on it.
    ///
    /// `None` whenever `auto_packages` is `None`. For an aggregate it maps
    /// each package the merge kept to its closures on every host with leaf
    /// data, joined, and is `None` when no host had leaf data.
    #[serde(default)]
    pub dep_closures: Option<BTreeMap<String, Vec<String>>>,
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

    /// A key answers its own path and each recorded literal path; a
    /// snapshot without the field still loads, and an empty set is not
    /// written, so a key named literally serializes as it always did.
    #[test]
    fn a_key_is_named_by_its_path_or_a_recorded_reference() {
        const REFERENCE: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-$releasever_major";
        let key = RepoFile {
            path: "/etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-10".into(),
            gpgkey_references: [REFERENCE.to_string()].into(),
            ..Default::default()
        };
        for (gpgkey_path, expected) in [
            ("/etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-10", true),
            (REFERENCE, true),
            ("/etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9", false),
        ] {
            assert_eq!(key.is_key_named_by(gpgkey_path), expected, "{gpgkey_path}");
        }

        let json = serde_json::to_value(&key).unwrap();
        assert_eq!(json["gpgkey_references"], serde_json::json!([REFERENCE]));
        assert_eq!(serde_json::from_value::<RepoFile>(json).unwrap(), key);

        let literal = RepoFile {
            path: "/etc/pki/rpm-gpg/KEY".into(),
            ..Default::default()
        };
        let json = serde_json::to_value(&literal).unwrap();
        assert!(json.get("gpgkey_references").is_none(), "{json}");
        let old: RepoFile = serde_json::from_str(
            r#"{"path": "/etc/pki/rpm-gpg/KEY", "content": "", "is_default_repo": false}"#,
        )
        .unwrap();
        assert!(old.gpgkey_references.is_empty());
    }

    #[test]
    fn test_package_entry_roundtrip() {
        let entry = PackageEntry {
            name: "httpd".into(),
            epoch: "0".into(),
            version: "2.4.57".into(),
            release: "5.el9".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            disposition: FindingKind::included(),
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
                disposition: FindingKind::included(),
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
