use super::rpm::{RepoStatus, UnverifiablePackage};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Go-compatible preflight result (stored in snapshot JSON).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PreflightResult {
    #[serde(default)]
    pub status: String,
    pub status_reason: Option<String>,
    #[serde(default)]
    pub available: Vec<String>,
    #[serde(default)]
    pub unavailable: Vec<String>,
    #[serde(default)]
    pub unverifiable: Vec<UnverifiablePackage>,
    #[serde(default)]
    pub direct_install: Vec<String>,
    #[serde(default)]
    pub repo_unreachable: Vec<RepoStatus>,
    #[serde(default)]
    pub base_image: String,
    #[serde(default)]
    pub repos_queried: Vec<String>,
    #[serde(default)]
    pub timestamp: String,
}

/// Pipeline-internal preflight mode (not serialized to snapshot).
#[derive(Debug, Clone)]
pub enum PreflightMode {
    Online { entitlement_dir: Option<PathBuf> },
    Manifest { path: PathBuf },
    Skip,
}

/// Render-time target context (not stored in snapshot).
#[derive(Debug, Clone)]
pub struct RenderTarget {
    pub system: super::system::TargetSystem,
    pub preflight: PreflightMode,
}
