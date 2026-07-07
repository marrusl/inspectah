// inspectah-web/src/web_types.rs
//
// Presentation-layer DTOs returned by the web API. Extracted from handlers.rs
// so that contract snapshot tests and future consumers can reference them
// without pulling in handler internals.

use std::collections::HashMap;

use inspectah_core::types::users::UserGroupDecision;
use inspectah_refine::baseline_summary::BaselineSummary;
use inspectah_refine::types::{RefinedView, RepoProvenance, RepoTier, TriageTag};
use serde::Serialize;

// -- Non-RPM DTOs (language packages + unmanaged files for the frontend) ----

/// A language package environment (pip venv, npm project, gem project)
/// projected for the view response.
#[derive(Serialize, Clone, Debug)]
pub struct LanguagePackageEnvDto {
    pub ecosystem: String,
    pub path: String,
    pub method: String,
    pub packages: Vec<String>,
    pub confidence: String,
    pub manifest_basis: String,
    pub include: bool,
}

/// Provenance signals for an unmanaged file.
#[derive(Serialize, Clone, Debug)]
pub struct ProvenanceSignalsDto {
    pub file_type: String,
    pub last_modified: u64,
    pub uid: u32,
    pub gid: u32,
    pub permissions: String,
    pub mutability: bool,
    pub writable_mount: bool,
    pub service_working_dir: bool,
}

/// A single unmanaged file discovered by --include-unmanaged.
#[derive(Serialize, Clone, Debug)]
pub struct UnmanagedFileItemDto {
    pub path: String,
    pub size: u64,
    pub is_var_path: bool,
    pub include: bool,
    pub provenance: ProvenanceSignalsDto,
}

/// Directory group for unmanaged files.
#[derive(Serialize, Clone, Debug)]
pub struct UnmanagedFileGroupDto {
    pub directory: String,
    pub items: Vec<UnmanagedFileItemDto>,
}

// -- Reference section DTOs (presentation layer only) ---------------------

/// Serde helper: skip serializing `false` booleans.
fn is_false(v: &bool) -> bool {
    !*v
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ReferenceSection {
    pub id: String,
    pub display_name: String,
    pub items: Vec<ContextItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subsections: Vec<ContextSubsection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub empty_reason: Option<String>,
    /// True when the network section has ifcfg-format connections.
    /// Only meaningful for the `network` section; false for all others.
    #[serde(default, skip_serializing_if = "is_false")]
    pub has_ifcfg: bool,
    /// Deprecation note text when ifcfg connections are present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ifcfg_note: Option<String>,
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
pub fn reference_section(
    id: &str,
    display_name: &str,
    items: Vec<ContextItem>,
) -> ReferenceSection {
    ReferenceSection {
        id: id.to_string(),
        display_name: display_name.to_string(),
        items,
        subsections: Vec::new(),
        empty_reason: None,
        has_ifcfg: false,
        ifcfg_note: None,
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
    pub locked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attention_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owning_package: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_state: Option<String>,
    pub current_state: String,
    /// Present when a full-shadow drop-in overrides this service unit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shadow_type: Option<String>,
    /// Rationale text for the shadow override (displayed below the toggle).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shadow_rationale: Option<String>,
}

/// A classified service drop-in override, projected for the view response.
#[derive(Serialize, Clone, Debug)]
pub struct DropInDecisionDto {
    pub unit: String,
    pub path: String,
    pub triage: TriageTag,
    pub include: bool,
    pub locked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attention_reason: Option<String>,
    /// Shadow type (e.g. "full_shadow", "drop_in") when this drop-in
    /// overrides a service unit file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shadow_type: Option<String>,
    /// Rationale text for the shadow override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shadow_rationale: Option<String>,
}

/// A classified quadlet unit, projected for the view response.
#[derive(Serialize, Clone, Debug)]
pub struct QuadletDecisionDto {
    pub path: String,
    pub name: String,
    pub image: String,
    pub triage: TriageTag,
    pub include: bool,
    pub locked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

/// A classified flatpak app, projected for the view response.
#[derive(Serialize, Clone, Debug)]
pub struct FlatpakDecisionDto {
    pub app_id: String,
    pub remote: String,
    pub branch: String,
    pub triage: TriageTag,
    pub include: bool,
    pub locked: bool,
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
    pub locked: bool,
}

/// A classified tuned profile selection, projected for the view response.
#[derive(Serialize, Clone, Debug)]
pub struct TunedDecisionDto {
    pub active_profile: String,
    pub custom_profiles: Vec<String>,
    pub triage: TriageTag,
    pub include: bool,
    pub locked: bool,
}

// -- Package group DTOs (group rendering for the web view) ------------------

/// Summary of an installed DNF group and its rendering state.
#[derive(Serialize, Clone, Debug)]
pub struct GroupInfo {
    pub name: String,
    pub member_count: usize,
    pub added_count: usize,
    pub locked_count: usize,
    pub optional_spillover_count: usize,
    pub render_state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degradation_reason: Option<String>,
    pub members: Vec<GroupMemberInfo>,
}

/// A single member of an installed group.
#[derive(Serialize, Clone, Debug)]
pub struct GroupMemberInfo {
    pub name: String,
    pub locked: bool,
    pub overlap_groups: Vec<String>,
    pub in_base_image: bool,
}

/// Provenance of a package that appears in the individual zone due to
/// group rendering decisions (spillover, ungrouped, or degraded).
#[derive(Serialize, Clone, Debug)]
pub struct PackageProvenance {
    pub kind: String,
    pub group_name: String,
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
    pub package_groups: Vec<GroupInfo>,
    /// Per-package provenance keyed by `"name.arch"` for packages that appear
    /// in the individual zone due to group rendering decisions.
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub package_provenances: HashMap<String, PackageProvenance>,
    pub session_is_sensitive: bool,
    /// Language package environments (Tier 1 non-RPM). Empty when absent.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub language_packages: Vec<LanguagePackageEnvDto>,
    /// Unmanaged file groups (Tier 2, flag-gated). Empty when absent.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unmanaged_files: Vec<UnmanagedFileGroupDto>,
    /// Whether --include-unmanaged was used at scan time.
    pub has_unmanaged_scan: bool,
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

// -- Group metadata DTOs (sidebar section groups for the frontend) --------

/// Metadata for a single section group, consumed by the frontend sidebar.
#[derive(Serialize, Clone, Debug)]
pub struct GroupMetaDto {
    pub slug: String,
    pub label: String,
    pub sections: Vec<SectionMetaDto>,
    pub has_actionable_sections: bool,
}

/// Metadata for a single section within a group.
#[derive(Serialize, Clone, Debug)]
pub struct SectionMetaDto {
    pub id: String,
    pub label: String,
    pub is_triage: bool,
}
