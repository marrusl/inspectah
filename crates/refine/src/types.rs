use std::collections::HashMap;

use inspectah_core::types::aggregate::{AggregateSnapshotMeta, PrevalenceZone, RepoSourceEntry};
use inspectah_core::types::config::ConfigFileEntry;
use inspectah_core::types::containers::{FlatpakApp, QuadletUnit};
use inspectah_core::types::kernelboot::SysctlOverride;
use inspectah_core::types::rpm::PackageEntry;
use inspectah_core::types::services::{ServiceStateChange, SystemdDropIn};
use inspectah_core::types::users::UserContainerfileStrategy;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PackageTarget {
    pub name: String,
    pub arch: String,
}

impl PackageTarget {
    pub fn matches(&self, entry: &PackageEntry) -> bool {
        self.name == entry.name && self.arch == entry.arch
    }
}

impl std::fmt::Display for PackageTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.name, self.arch)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ContentHash(String);

impl ContentHash {
    pub fn new(s: impl Into<String>) -> Result<Self, String> {
        let s = s.into();
        if s.len() != 64 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!(
                "invalid content hash: expected 64 hex chars, got {} chars",
                s.len()
            ));
        }
        Ok(Self(s))
    }

    pub fn from_content(content: &[u8]) -> Self {
        Self(format!("{:x}", Sha256::digest(content)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "key")]
pub enum ItemId {
    // RPM section
    Package {
        name: String,
        arch: String,
    },
    Repo {
        path: String,
    },
    ModuleStream {
        module_stream: String,
    },
    VersionLock {
        name_arch: String,
    },

    // Config section
    Config {
        path: String,
    },

    // Services section
    Service {
        unit: String,
    },
    DropIn {
        path: String,
    },

    // Containers section
    Quadlet {
        path: String,
    },
    Compose {
        path: String,
    },
    Flatpak {
        app_id: String,
        remote: String,
        branch: String,
    },

    // Network section
    NMConnection {
        path: String,
    },
    FirewallZone {
        path: String,
    },

    // Kernel/boot section
    KernelModule {
        name: String,
    },
    Sysctl {
        key: String,
    },
    TunedSelection {
        profile: String,
    },

    // Scheduled section
    CronJob {
        path: String,
    },
    SystemdTimer {
        name: String,
    },
    AtJob {
        file: String,
    },
    GeneratedTimer {
        name: String,
    },

    // SELinux section
    SelinuxPort {
        protocol_port: String,
    },

    // Storage section
    Fstab {
        mount_point: String,
    },

    // Non-RPM section
    NonRpm {
        name: String,
    },

    // Language packages section
    LanguageEnv {
        ecosystem: String,
        path: String,
    },

    // Group section (new — group-aware rendering)
    Group {
        name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", content = "target")]
pub enum RefinementOp {
    // Unified include/exclude — canonical form (v2)
    SetInclude {
        item_id: ItemId,
        include: bool,
    },

    UserStrategy {
        username: String,
        strategy: UserContainerfileStrategy,
    },
    UserPassword(UserPasswordOp),
    SelectVariant {
        item_id: ItemId,
        target: ContentHash,
    },
    EditVariant {
        item_id: ItemId,
        content: String,
        based_on: Option<ContentHash>,
    },
    DiscardVariant {
        item_id: ItemId,
        variant: ContentHash,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "choice")]
pub enum UserPasswordOp {
    New {
        username: String,
        hash: Option<String>,
    },
    None {
        username: String,
    },
    Preserve {
        username: String,
    },
}

/// A single entry in the session timeline — either a refinement operation
/// (mutates the projected snapshot) or a view directive (controls display
/// without changing the data plane).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum TimelineEntry {
    Op(RefinementOp),
    View(ViewDirective),
}

/// View-plane directives that control how data is displayed without
/// modifying the projected snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "directive")]
pub enum ViewDirective {
    UngroupGroup { group_name: String },
}

/// Timeline entry with active flag for undo/redo UI.
/// The `#[serde(flatten)]` ensures the JSON is flat:
/// `{"kind":"Op","op":"SetInclude",...,"active":true}` instead of nested.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotatedTimelineEntry {
    #[serde(flatten)]
    pub entry: TimelineEntry,
    pub active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepoProvenance {
    Verified,
    Incomplete,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepoTier {
    Distro,
    OfficialOptional,
    ThirdParty,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriageBucket {
    Baseline,
    Site,
    Investigate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AggregateBucket {
    Investigate,
    Divergent,
    Partial,
    Universal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prevalence {
    pub count: u32,
    pub total: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AggregateTriage {
    pub bucket: AggregateBucket,
    pub prevalence: Prevalence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode")]
pub enum Triage {
    #[serde(rename = "single_host")]
    SingleHost(TriageBucket),
    #[serde(rename = "aggregate")]
    Aggregate(AggregateTriage),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriageAnnotation {
    SensitivePath,
    FirstBootProvisioned,
    RequiresProjectedPackage { name: String },
    RuntimeOnlyObservation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriageReason {
    PackageBaselineMatch,
    PackageUserAdded,
    PackageVersionChanged,
    PackageProvenanceUnavailable,
    PackageLocalInstall,
    PackageNoRepoSource,
    PackageConfigCaptured,
    ConfigDefault,
    ConfigBaselineMatch,
    ConfigModified,
    ConfigUnowned,
    ConfigOrphaned,
    ServiceBaselineMatch,
    ServiceNonDefaultState,
    ServiceUnknownOrigin,
    ServiceDropInPresent,
    QuadletUserDeployed,
    QuadletPresentInBaseImage,
    FlatpakProvisionedOnFirstBoot,
    FlatpakIncompleteProvenance,
    SysctlBaselineMatch,
    SysctlFileBackedOverride,
    SysctlNoBaseline,
    TunedBaselineMatch,
    TunedNonDefaultProfile,
    TunedCustomProfile,
    TunedUnusualState,
    SensitivePath,
    PackagePlatformPlumbing,
    PackageInstallerDefault,
    PackageInstallerPromotedService,
    PackageInstallerPromotedConfig,
    PackageInstallerAmbiguous,
    PackageInstallerEvidenceUnavailable,
    Custom(String),
}

impl TriageReason {
    pub fn display_string(&self) -> &'static str {
        match self {
            Self::PackageBaselineMatch => "Matches base image",
            Self::PackageUserAdded => "User-added package",
            Self::PackageVersionChanged => "Version changed from base image",
            Self::PackageProvenanceUnavailable => "Unknown origin \u{2014} no baseline available",
            Self::PackageLocalInstall => "Locally installed RPM \u{2014} not from a repository",
            Self::PackageNoRepoSource => "Unknown origin \u{2014} no repository source",
            Self::PackageConfigCaptured => "Contents captured via config files",
            Self::ConfigDefault => "RPM default \u{2014} unmodified",
            Self::ConfigBaselineMatch => "Matches base image",
            Self::ConfigModified => "Modified from RPM default",
            Self::ConfigUnowned => "Not owned by any installed package",
            Self::ConfigOrphaned => "Orphaned \u{2014} owning package removed",
            Self::ServiceBaselineMatch => "Matches base image service state",
            Self::ServiceNonDefaultState => "Non-default service state",
            Self::ServiceUnknownOrigin => "Service not from any installed RPM",
            Self::ServiceDropInPresent => "Drop-in override present",
            Self::QuadletUserDeployed => "User-deployed container workload",
            Self::QuadletPresentInBaseImage => "Quadlet present in base image",
            Self::FlatpakProvisionedOnFirstBoot => "Flatpak provisioned at first boot",
            Self::FlatpakIncompleteProvenance => "Incomplete provenance for manifest",
            Self::SysctlBaselineMatch => "Matches base image kernel parameter",
            Self::SysctlFileBackedOverride => "Non-default kernel parameter",
            Self::SysctlNoBaseline => "No baseline available for comparison",
            Self::TunedBaselineMatch => "Matches base image tuned profile",
            Self::TunedNonDefaultProfile => "Non-default tuned profile",
            Self::TunedCustomProfile => "Custom profile in /etc/tuned/",
            Self::TunedUnusualState => "Tuned in unusual state",
            Self::SensitivePath => "Security-sensitive path \u{2014} verify before including",
            Self::PackagePlatformPlumbing => "Platform plumbing \u{2014} excluded by boot chain",
            Self::PackageInstallerDefault => {
                "Installed by Anaconda, no active customization detected"
            }
            Self::PackageInstallerPromotedService => {
                "Installer package with active service and config"
            }
            Self::PackageInstallerPromotedConfig => "Installer package with modified configuration",
            Self::PackageInstallerAmbiguous => {
                "Installed by Anaconda \u{2014} review for user intent"
            }
            Self::PackageInstallerEvidenceUnavailable => {
                "Installer package \u{2014} evidence unavailable"
            }
            Self::Custom(_) => "See detail",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TriageTag {
    pub triage: Triage,
    pub primary_reason: TriageReason,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<TriageAnnotation>,
}

impl TriageTag {
    /// Returns the single-host bucket, or maps aggregate buckets to the closest
    /// single-host equivalent for filtering/counting purposes.
    pub fn bucket(&self) -> TriageBucket {
        match &self.triage {
            Triage::SingleHost(b) => *b,
            Triage::Aggregate(ft) => match ft.bucket {
                AggregateBucket::Investigate => TriageBucket::Investigate,
                AggregateBucket::Divergent => TriageBucket::Investigate,
                AggregateBucket::Partial => TriageBucket::Site,
                AggregateBucket::Universal => TriageBucket::Baseline,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefinedPackage {
    pub entry: PackageEntry,
    pub triage: TriageTag,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefinedConfig {
    pub entry: ConfigFileEntry,
    pub triage: TriageTag,
}

/// A classified service state change with triage assignment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefinedServiceState {
    pub entry: ServiceStateChange,
    pub triage: TriageTag,
}

/// A classified systemd drop-in with triage assignment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefinedDropIn {
    pub entry: SystemdDropIn,
    pub triage: TriageTag,
}

/// A classified quadlet unit with triage assignment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefinedQuadlet {
    pub entry: QuadletUnit,
    pub triage: TriageTag,
}

/// A classified flatpak app with triage assignment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefinedFlatpak {
    pub entry: FlatpakApp,
    pub triage: TriageTag,
}

/// A classified sysctl override with triage assignment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefinedSysctl {
    pub entry: SysctlOverride,
    pub triage: TriageTag,
}

/// A classified tuned profile selection with triage assignment.
///
/// One entry per host: bundles active profile + custom profile files.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefinedTunedSelection {
    pub active_profile: String,
    pub custom_profiles: Vec<String>,
    pub triage: TriageTag,
    #[serde(default = "default_include")]
    pub include: bool,
}

fn default_include() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionKind {
    Package,
    Config,
    Repo,
    User,
    Service,
    Quadlet,
    Flatpak,
    Sysctl,
    Tuned,
    ComposeContext,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SectionStats {
    pub kind: SectionKind,
    pub total: usize,
    pub included: usize,
    pub excluded: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SectionChangeSummary {
    pub kind: SectionKind,
    pub included: Vec<ItemId>,
    pub excluded: Vec<ItemId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefineStats {
    pub sections: Vec<SectionStats>,
    pub needs_review_count: usize,
    pub ops_applied: usize,
    pub can_undo: bool,
    pub can_redo: bool,
    pub baseline_available: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefinedView {
    pub packages: Vec<RefinedPackage>,
    pub config_files: Vec<RefinedConfig>,
    pub containerfile_preview: String,
    pub stats: RefineStats,
    pub generation: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChangesSummary {
    pub sections: Vec<SectionChangeSummary>,
    pub variants_changed: usize,
    pub is_dirty: bool,
}

impl RefineStats {
    /// Look up a section's stats by kind. Returns zeros if the section is absent.
    pub fn section(&self, kind: SectionKind) -> &SectionStats {
        static EMPTY: SectionStats = SectionStats {
            kind: SectionKind::Package, // placeholder, caller matches on kind
            total: 0,
            included: 0,
            excluded: 0,
        };
        self.sections
            .iter()
            .find(|s| s.kind == kind)
            .unwrap_or(&EMPTY)
    }

    // Convenience accessors for the two sections that existing callers use most.
    pub fn total_packages(&self) -> usize {
        self.section(SectionKind::Package).total
    }
    pub fn included_packages(&self) -> usize {
        self.section(SectionKind::Package).included
    }
    pub fn excluded_packages(&self) -> usize {
        self.section(SectionKind::Package).excluded
    }
    pub fn total_configs(&self) -> usize {
        self.section(SectionKind::Config).total
    }
    pub fn included_configs(&self) -> usize {
        self.section(SectionKind::Config).included
    }
    pub fn excluded_configs(&self) -> usize {
        self.section(SectionKind::Config).excluded
    }
}

impl ChangesSummary {
    /// Look up a section's change summary by kind.
    pub fn section(&self, kind: SectionKind) -> Option<&SectionChangeSummary> {
        self.sections.iter().find(|s| s.kind == kind)
    }

    /// Convenience: collect excluded repo ItemIds as string section IDs.
    pub fn repos_excluded(&self) -> Vec<String> {
        self.section(SectionKind::Repo)
            .map(|s| {
                s.excluded
                    .iter()
                    .filter_map(|id| match id {
                        ItemId::Repo { path } => Some(path.clone()),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotatedOp {
    #[serde(flatten)]
    pub op: RefinementOp,
    pub active: bool,
}

/// Runtime context for aggregate-mode refine sessions.
///
/// Not serialized — this is derived from the snapshot at session creation time.
#[derive(Debug)]
pub struct AggregateContext {
    pub aggregate_meta: AggregateSnapshotMeta,
    pub zones: HashMap<ItemId, PrevalenceZone>,
    pub total_hosts: usize,
    /// false for aggregate-of-2 (zones suppressed, variant ops available),
    /// true for aggregate-of-3+ (zones active).
    pub zones_active: bool,
    /// Repo-source conflicts from the aggregate merge. Maps `name.arch` identity
    /// keys to the distinct repos with host counts. Only populated when the
    /// same package was installed from different repos across hosts.
    pub repo_conflicts: HashMap<String, Vec<RepoSourceEntry>>,
}

/// Operating mode of the refine session, determined at construction time
/// from the presence/absence of `AggregateSnapshotMeta` in the snapshot.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum RefineMode {
    SingleHost,
    Aggregate(AggregateContext),
}

#[derive(Debug, thiserror::Error)]
pub enum RefineError {
    #[error("unknown target: {0}")]
    UnknownTarget(String),
    #[error("nothing to undo")]
    NothingToUndo,
    #[error("nothing to redo")]
    NothingToRedo,
    #[error("stale generation: expected {expected}, got {actual}")]
    StaleGeneration { expected: u64, actual: u64 },
    #[error("render failed: {0}")]
    RenderFailed(String),
    #[error("tarball error: {0}")]
    TarballError(String),
    #[error("snapshot load error: {0}")]
    SnapshotLoad(String),
    #[error("stale tarball: saved hash {saved_hash} does not match current hash {current_hash}")]
    StaleTarball {
        saved_hash: String,
        current_hash: String,
    },
    #[error("untrusted snapshot: {0}")]
    UntrustedSnapshot(String),
    #[error("archive safety violation: {0}")]
    ArchiveSafety(String),
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod triage_tests {
    use super::*;

    #[test]
    fn triage_bucket_serde_roundtrip() {
        let buckets = vec![
            TriageBucket::Baseline,
            TriageBucket::Site,
            TriageBucket::Investigate,
        ];
        for b in buckets {
            let json = serde_json::to_string(&b).unwrap();
            let back: TriageBucket = serde_json::from_str(&json).unwrap();
            assert_eq!(b, back);
        }
    }

    #[test]
    fn aggregate_triage_serde_roundtrip() {
        let ft = AggregateTriage {
            bucket: AggregateBucket::Divergent,
            prevalence: Prevalence {
                count: 42,
                total: 50,
            },
        };
        let json = serde_json::to_string(&ft).unwrap();
        let back: AggregateTriage = serde_json::from_str(&json).unwrap();
        assert_eq!(ft.bucket, back.bucket);
        assert_eq!(ft.prevalence.count, 42);
        assert_eq!(ft.prevalence.total, 50);
    }

    #[test]
    fn triage_tag_with_annotations() {
        let tag = TriageTag {
            triage: Triage::SingleHost(TriageBucket::Investigate),
            primary_reason: TriageReason::PackageLocalInstall,
            annotations: vec![TriageAnnotation::SensitivePath],
        };
        let json = serde_json::to_string(&tag).unwrap();
        assert!(json.contains("investigate"));
        assert!(json.contains("sensitive_path"));
    }
}

#[cfg(test)]
mod item_id_tests {
    use super::*;

    #[test]
    fn set_include_with_package_serde_roundtrip() {
        let op = RefinementOp::SetInclude {
            item_id: ItemId::Package {
                name: "httpd".into(),
                arch: "x86_64".into(),
            },
            include: false,
        };
        let json = serde_json::to_string(&op).unwrap();
        let back: RefinementOp = serde_json::from_str(&json).unwrap();
        assert_eq!(op, back);
    }

    #[test]
    fn set_include_with_service_serde_roundtrip() {
        let op = RefinementOp::SetInclude {
            item_id: ItemId::Service {
                unit: "sshd.service".into(),
            },
            include: true,
        };
        let json = serde_json::to_string(&op).unwrap();
        let back: RefinementOp = serde_json::from_str(&json).unwrap();
        assert_eq!(op, back);
    }

    #[test]
    fn set_include_with_sysctl_serde_roundtrip() {
        let op = RefinementOp::SetInclude {
            item_id: ItemId::Sysctl {
                key: "net.ipv4.ip_forward".into(),
            },
            include: false,
        };
        let json = serde_json::to_string(&op).unwrap();
        let back: RefinementOp = serde_json::from_str(&json).unwrap();
        assert_eq!(op, back);
    }

    #[test]
    fn item_id_package_split_fields_serde_roundtrip() {
        let id = ItemId::Package {
            name: "vim-enhanced".into(),
            arch: "aarch64".into(),
        };
        let json = serde_json::to_string(&id).unwrap();
        assert!(json.contains("vim-enhanced"));
        assert!(json.contains("aarch64"));
        let back: ItemId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn item_id_flatpak_serde_roundtrip() {
        let id = ItemId::Flatpak {
            app_id: "org.mozilla.Firefox".into(),
            remote: "flathub".into(),
            branch: "stable".into(),
        };
        let json = serde_json::to_string(&id).unwrap();
        let back: ItemId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn item_id_tuned_selection_serde_roundtrip() {
        let id = ItemId::TunedSelection {
            profile: "throughput-performance".into(),
        };
        let json = serde_json::to_string(&id).unwrap();
        let back: ItemId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn item_id_group_round_trip() {
        let id = ItemId::Group {
            name: "Development Tools".into(),
        };
        let json = serde_json::to_string(&id).unwrap();
        assert!(json.contains("Group"));
        let back: ItemId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn test_anaconda_reason_serialization() {
        let cases = vec![
            (
                TriageReason::PackagePlatformPlumbing,
                "\"package_platform_plumbing\"",
            ),
            (
                TriageReason::PackageInstallerDefault,
                "\"package_installer_default\"",
            ),
            (
                TriageReason::PackageInstallerPromotedService,
                "\"package_installer_promoted_service\"",
            ),
            (
                TriageReason::PackageInstallerPromotedConfig,
                "\"package_installer_promoted_config\"",
            ),
            (
                TriageReason::PackageInstallerAmbiguous,
                "\"package_installer_ambiguous\"",
            ),
            (
                TriageReason::PackageInstallerEvidenceUnavailable,
                "\"package_installer_evidence_unavailable\"",
            ),
        ];
        for (reason, expected_json) in cases {
            let serialized = serde_json::to_string(&reason).unwrap();
            assert_eq!(
                serialized, expected_json,
                "serialization mismatch for {:?}",
                reason
            );
            let deserialized: TriageReason = serde_json::from_str(&serialized).unwrap();
            assert_eq!(
                deserialized, reason,
                "deserialization mismatch for {:?}",
                reason
            );
        }
    }
}

#[cfg(test)]
mod timeline_entry_tests {
    use super::*;

    #[test]
    fn timeline_entry_op_round_trip() {
        let entry = TimelineEntry::Op(RefinementOp::SetInclude {
            item_id: ItemId::Package {
                name: "httpd".into(),
                arch: "x86_64".into(),
            },
            include: false,
        });
        let json = serde_json::to_string(&entry).unwrap();
        let back: TimelineEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(entry, back);
    }

    #[test]
    fn timeline_entry_view_round_trip() {
        let entry = TimelineEntry::View(ViewDirective::UngroupGroup {
            group_name: "Container Management".into(),
        });
        let json = serde_json::to_string(&entry).unwrap();
        let back: TimelineEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(entry, back);
    }
}
