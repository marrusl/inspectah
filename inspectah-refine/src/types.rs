use inspectah_core::types::config::ConfigFileEntry;
use inspectah_core::types::rpm::PackageEntry;
use inspectah_core::types::users::UserContainerfileStrategy;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", content = "target")]
pub enum RefinementOp {
    ExcludePackage(PackageTarget),
    IncludePackage(PackageTarget),
    ExcludeConfig { path: PathBuf },
    IncludeConfig { path: PathBuf },
    ExcludeRepo { section_id: String },
    IncludeRepo { section_id: String },
    UserStrategy { username: String, strategy: UserContainerfileStrategy },
    UserPassword(UserPasswordOp),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "choice")]
pub enum UserPasswordOp {
    New { username: String, hash: Option<String> },
    None { username: String },
    Preserve { username: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefinedConfig {
    pub entry: ConfigFileEntry,
    pub attention: Vec<AttentionTag>,
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
    pub is_dirty: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotatedOp {
    #[serde(flatten)]
    pub op: RefinementOp,
    pub active: bool,
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
    #[error("untrusted snapshot: {0}")]
    UntrustedSnapshot(String),
    #[error("archive safety violation: {0}")]
    ArchiveSafety(String),
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
