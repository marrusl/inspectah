// inspectah-web/src/web_types.rs
//
// Presentation-layer DTOs returned by the web API. Extracted from handlers.rs
// so that contract snapshot tests and future consumers can reference them
// without pulling in handler internals.

use inspectah_refine::baseline_summary::BaselineSummary;
use inspectah_refine::types::{RefinedView, RepoProvenance, RepoTier, TriageTag};
use inspectah_core::types::users::UserGroupDecision;
use serde::Serialize;

// -- Reference section DTOs (presentation layer only) ---------------------

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ReferenceSection {
    pub id: String,
    pub display_name: String,
    pub items: Vec<ContextItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subsections: Vec<ContextSubsection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub empty_reason: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ContextSubsection {
    pub id: String,
    pub display_name: String,
    pub items: Vec<ContextItem>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ContextItem {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub detail: Option<String>,
    pub searchable_text: String,
}

/// Create a `ReferenceSection` with empty subsections.
pub fn reference_section(id: &str, display_name: &str, items: Vec<ContextItem>) -> ReferenceSection {
    ReferenceSection {
        id: id.to_string(),
        display_name: display_name.to_string(),
        items,
        subsections: Vec::new(),
        empty_reason: None,
    }
}

// -- Repo group + view response DTOs --------------------------------------

#[derive(Serialize, Clone, Debug)]
pub struct RepoGroupInfo {
    pub section_id: String,
    pub provenance: RepoProvenance,
    pub is_distro: bool,
    pub tier: RepoTier,
    pub package_count: usize,
    pub enabled: bool,
}

/// A classified service state change, projected for the view response.
#[derive(Serialize, Clone, Debug)]
pub struct ServiceDecisionDto {
    pub unit: String,
    pub triage: TriageTag,
    pub include: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owning_package: Option<String>,
}

/// A classified service drop-in override, projected for the view response.
#[derive(Serialize, Clone, Debug)]
pub struct DropInDecisionDto {
    pub unit: String,
    pub path: String,
    pub triage: TriageTag,
    pub include: bool,
}

/// A classified quadlet unit, projected for the view response.
#[derive(Serialize, Clone, Debug)]
pub struct QuadletDecisionDto {
    pub path: String,
    pub name: String,
    pub image: String,
    pub triage: TriageTag,
    pub include: bool,
}

/// A classified flatpak app, projected for the view response.
#[derive(Serialize, Clone, Debug)]
pub struct FlatpakDecisionDto {
    pub app_id: String,
    pub remote: String,
    pub branch: String,
    pub triage: TriageTag,
    pub include: bool,
    pub lifecycle: String,
}

/// A classified sysctl override, projected for the view response.
#[derive(Serialize, Clone, Debug)]
pub struct SysctlDecisionDto {
    pub key: String,
    pub runtime: String,
    pub default: String,
    pub source: String,
    pub triage: TriageTag,
    pub include: bool,
}

/// A classified tuned profile selection, projected for the view response.
#[derive(Serialize, Clone, Debug)]
pub struct TunedDecisionDto {
    pub active_profile: String,
    pub custom_profiles: Vec<String>,
    pub triage: TriageTag,
    pub include: bool,
}

#[derive(Serialize)]
pub struct ViewResponse {
    #[serde(flatten)]
    pub view: RefinedView,
    pub repo_groups: Vec<RepoGroupInfo>,
    pub baseline_summary: Option<BaselineSummary>,
    pub version_changes: Vec<VersionChangeEntry>,
    pub service_states: Vec<ServiceDecisionDto>,
    pub service_dropins: Vec<DropInDecisionDto>,
    pub quadlets: Vec<QuadletDecisionDto>,
    pub flatpaks: Vec<FlatpakDecisionDto>,
    pub sysctls: Vec<SysctlDecisionDto>,
    pub tuned: Vec<TunedDecisionDto>,
    pub users_groups_decisions: Vec<UserGroupDecision>,
    pub session_is_sensitive: bool,
}

#[derive(Serialize)]
pub struct VersionChangeEntry {
    pub name: String,
    pub arch: String,
    pub host_version: String,
    pub base_version: String,
    pub host_epoch: String,
    pub base_epoch: String,
    pub direction: String,
}
