use serde::{Deserialize, Serialize};
use super::rpm::{UnverifiablePackage, RepoStatus};
use std::path::PathBuf;

/// Go-compatible preflight result (stored in snapshot JSON).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PreflightResult {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub status: String,
    pub status_reason: Option<String>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub available: Vec<String>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub unavailable: Vec<String>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub unverifiable: Vec<UnverifiablePackage>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub direct_install: Vec<String>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub repo_unreachable: Vec<RepoStatus>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub base_image: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub repos_queried: Vec<String>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
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
