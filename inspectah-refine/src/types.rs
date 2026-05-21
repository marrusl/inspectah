use std::collections::HashMap;
use std::path::PathBuf;

use inspectah_core::types::config::ConfigFileEntry;
use inspectah_core::types::fleet::{FleetSnapshotMeta, PrevalenceZone};
use inspectah_core::types::rpm::PackageEntry;
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
    Package { name_arch: String },
    Repo { path: String },
    ModuleStream { module_stream: String },
    VersionLock { name_arch: String },

    // Config section
    Config { path: String },

    // Services section
    Service { unit: String },
    DropIn { path: String },

    // Containers section
    Quadlet { path: String },
    Compose { path: String },

    // Network section
    NMConnection { path: String },
    FirewallZone { path: String },

    // Kernel/boot section
    KernelModule { name: String },
    Sysctl { key: String },

    // Scheduled section
    CronJob { path: String },
    SystemdTimer { name: String },
    AtJob { file: String },
    GeneratedTimer { name: String },

    // SELinux section
    SelinuxPort { protocol_port: String },

    // Storage section
    Fstab { mount_point: String },

    // Non-RPM section
    NonRpm { name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", content = "target")]
pub enum RefinementOp {
    ExcludePackage(PackageTarget),
    IncludePackage(PackageTarget),
    ExcludeConfig {
        path: PathBuf,
    },
    IncludeConfig {
        path: PathBuf,
    },
    ExcludeRepo {
        section_id: String,
    },
    IncludeRepo {
        section_id: String,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionLevel {
    NeedsReview,
    Informational,
    Routine,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionReason {
    PackageBaselineMatch,
    PackageUserAdded,
    PackageVersionChanged,
    PackageProvenanceUnavailable,
    PackageLocalInstall,
    PackageNoRepoSource,
    ConfigDefault,
    ConfigBaselineMatch,
    ConfigModified,
    ConfigUnowned,
    ConfigOrphaned,
    SensitivePath,
    ServiceImageModeIncompatible,
    Custom(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepoProvenance {
    Verified,
    Incomplete,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttentionTag {
    pub level: AttentionLevel,
    pub reason: AttentionReason,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefinedPackage {
    pub entry: PackageEntry,
    pub attention: Vec<AttentionTag>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fleet_attention: Option<FleetAttention>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefinedConfig {
    pub entry: ConfigFileEntry,
    pub attention: Vec<AttentionTag>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fleet_attention: Option<FleetAttention>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefineStats {
    pub total_packages: usize,
    pub included_packages: usize,
    pub excluded_packages: usize,
    pub total_configs: usize,
    pub included_configs: usize,
    pub package_managed_configs: usize,
    pub excluded_configs: usize,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangesSummary {
    pub packages_included: Vec<PackageTarget>,
    pub packages_excluded: Vec<PackageTarget>,
    pub configs_included: Vec<String>,
    pub configs_excluded: Vec<String>,
    pub repos_excluded: Vec<String>,
    pub variants_changed: usize,
    pub is_dirty: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotatedOp {
    #[serde(flatten)]
    pub op: RefinementOp,
    pub active: bool,
}

/// Runtime context for fleet-mode refine sessions.
///
/// Not serialized — this is derived from the snapshot at session creation time.
#[derive(Debug)]
pub struct FleetContext {
    pub fleet_meta: FleetSnapshotMeta,
    pub zones: HashMap<ItemId, PrevalenceZone>,
    pub total_hosts: usize,
    /// false for fleet-of-2 (zones suppressed, variant ops available),
    /// true for fleet-of-3+ (zones active).
    pub zones_active: bool,
}

/// Operating mode of the refine session, determined at construction time
/// from the presence/absence of `FleetSnapshotMeta` in the snapshot.
#[derive(Debug)]
pub enum RefineMode {
    SingleHost,
    Fleet(FleetContext),
}

/// Fleet-aware attention score combining zone placement, attention level,
/// and raw prevalence count. Ord sorts by zone first (Divergent < Consensus),
/// then attention (NeedsReview < Informational < Routine), then prevalence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetAttention {
    pub zone: PrevalenceZone,
    pub attention: AttentionLevel,
    pub prevalence: u32,
}

impl Ord for FleetAttention {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.zone
            .cmp(&other.zone)
            .then(self.attention.cmp(&other.attention))
            .then(self.prevalence.cmp(&other.prevalence))
    }
}

impl PartialOrd for FleetAttention {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Attention score that works for both single-host and fleet modes.
#[derive(Debug, Clone)]
pub enum AttentionScore {
    SingleHost(AttentionLevel),
    Fleet(FleetAttention),
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
