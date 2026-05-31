use serde::{Deserialize, Serialize};

use crate::baseline_summary::BaselineSummary;
use crate::types::{
    RepoProvenance, RepoTier, RefinedDropIn, RefinedFlatpak, RefinedQuadlet, RefinedServiceState,
    RefinedSysctl, RefinedTunedSelection,
};
use inspectah_core::types::containers::ComposeService;
use inspectah_core::types::rpm::{VersionChange, VersionChangeDirection};
use inspectah_core::types::services::{PresetDefault, ServiceUnitState};
use inspectah_core::types::users::UserGroupDecision;
use inspectah_pipeline::render::service_intent::AdvisoryReason;

// ── Decision projection ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionProjection {
    pub service_states: Vec<RefinedServiceState>,
    pub service_dropins: Vec<RefinedDropIn>,
    pub quadlets: Vec<RefinedQuadlet>,
    pub flatpaks: Vec<RefinedFlatpak>,
    pub sysctls: Vec<RefinedSysctl>,
    pub tuned: Vec<RefinedTunedSelection>,
    pub repo_groups: Vec<RepoGroup>,
    pub version_changes: Vec<VersionChange>,
    pub users_groups: Vec<UserGroupDecision>,
    pub is_sensitive: bool,
    pub baseline_summary: Option<BaselineSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoGroup {
    pub section_id: String,
    pub provenance: RepoProvenance,
    pub is_distro: bool,
    pub tier: RepoTier,
    pub package_count: usize,
    pub enabled: bool,
}

// ── Reference projection ─────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ReferenceProjection {
    pub services: RefServices,
    pub version_changes: RefVersionChanges,
    pub containers: RefContainers,
    pub kernel_boot: RefKernelBoot,
    pub network: RefNetwork,
    pub storage: RefStorage,
    pub scheduled_tasks: Vec<GenericRefItem>,
    pub non_rpm_software: Vec<GenericRefItem>,
    pub selinux: Vec<GenericRefItem>,
}

// ── Services ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct RefServices {
    pub divergent: Vec<RefServiceItem>,
    pub preset_matched_with_dropins: Vec<RefServiceItem>,
    pub preset_unknown_enabled: Vec<RefServiceItem>,
    pub preset_unknown_disabled: Vec<RefServiceItem>,
    pub standalone_dropins: Vec<RefDropInItem>,
    pub omitted: Vec<RefOmittedService>,
    pub advisories: Vec<RefServiceAdvisory>,
    pub warnings: Vec<RefServiceWarning>,
}

#[derive(Debug, Clone)]
pub struct RefServiceItem {
    pub unit: String,
    pub current_state: ServiceUnitState,
    pub default_state: Option<PresetDefault>,
    pub owning_package: Option<String>,
    pub dropin_contents: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RefDropInItem {
    pub unit: String,
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct RefOmittedService {
    pub unit: String,
    pub package: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct RefServiceAdvisory {
    pub unit: String,
    pub owning_package: String,
    pub reasons: Vec<AdvisoryReason>,
}

#[derive(Debug, Clone)]
pub struct RefServiceWarning {
    pub unit: String,
    pub message: String,
}

// ── Version changes ──────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct RefVersionChanges {
    pub downgrades: Vec<RefVersionChangeItem>,
    pub upgrades: Vec<RefVersionChangeItem>,
    pub empty_reason: Option<EmptyReason>,
}

#[derive(Debug, Clone)]
pub struct RefVersionChangeItem {
    pub name: String,
    pub arch: String,
    pub host_version: String,
    pub base_version: String,
    pub host_epoch: String,
    pub base_epoch: String,
    pub direction: VersionChangeDirection,
}

#[derive(Debug, Clone)]
pub enum EmptyReason {
    NoBaseline,
    ZeroDrift,
    DataUnavailable,
}

// ── Containers ───────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct RefContainers {
    pub quadlets: Vec<RefQuadletItem>,
    pub compose_files: Vec<RefComposeItem>,
    pub running_containers: Vec<RefRunningContainerItem>,
    pub flatpaks: Vec<RefFlatpakRefItem>,
}

#[derive(Debug, Clone)]
pub struct RefQuadletItem {
    pub name: String,
    pub image: String,
    pub path: String,
    pub content: String,
    pub ports: Vec<String>,
    pub volumes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RefComposeItem {
    pub path: String,
    pub services: Vec<ComposeService>,
    pub include: bool,
}

#[derive(Debug, Clone)]
pub struct RefRunningContainerItem {
    pub id: String,
    pub name: String,
    pub image: String,
    pub status: String,
    pub env: Vec<String>,
    pub mounts: Vec<ContainerMount>,
    pub restart_policy: String,
}

#[derive(Debug, Clone)]
pub struct ContainerMount {
    pub mount_type: String,
    pub source: String,
    pub destination: String,
}

#[derive(Debug, Clone)]
pub struct RefFlatpakRefItem {
    pub app_id: String,
    pub origin: String,
    pub branch: String,
    pub remote: String,
    pub remote_url: String,
}

// ── Kernel/boot ──────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct RefKernelBoot {
    pub cmdline: Option<String>,
    pub grub_defaults: Option<String>,
    pub tuned_active: Option<String>,
    pub locale: Option<String>,
    pub timezone: Option<String>,
    pub sysctl_overrides: Vec<RefSysctlOverride>,
    pub non_default_modules: Vec<RefKernelModule>,
    pub modules_load_d: Vec<RefConfigSnippet>,
    pub modprobe_d: Vec<RefConfigSnippet>,
    pub dracut_conf: Vec<RefConfigSnippet>,
    pub custom_tuned_profiles: Vec<RefConfigSnippet>,
    pub alternatives: Vec<RefAlternativeEntry>,
}

#[derive(Debug, Clone)]
pub struct RefSysctlOverride {
    pub key: String,
    pub runtime: String,
    pub default: String,
    pub source: String,
}

#[derive(Debug, Clone)]
pub struct RefKernelModule {
    pub name: String,
    pub size: String,
    pub used_by: String,
}

#[derive(Debug, Clone)]
pub struct RefConfigSnippet {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct RefAlternativeEntry {
    pub name: String,
    pub path: String,
    pub status: String,
}

// ── Network ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct RefNetwork {
    pub connections: Vec<RefNMConnection>,
    pub firewall_zones: Vec<RefFirewallZone>,
    pub firewall_direct_rules: Vec<RefFirewallDirectRule>,
    pub static_routes: Vec<RefStaticRoute>,
    pub ip_routes: Vec<String>,
    pub ip_rules: Vec<String>,
    pub resolv_provenance: String,
    pub hosts_additions: Vec<String>,
    pub proxy_env: Vec<RefProxyEnv>,
}

#[derive(Debug, Clone)]
pub struct RefNMConnection {
    pub name: String,
    pub conn_type: String,
    pub method: String,
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct RefFirewallZone {
    pub name: String,
    pub path: String,
    pub content: String,
    pub services: Vec<String>,
    pub ports: Vec<String>,
    pub rich_rules: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RefFirewallDirectRule {
    pub ipv: String,
    pub table: String,
    pub chain: String,
    pub priority: String,
    pub args: String,
}

#[derive(Debug, Clone)]
pub struct RefStaticRoute {
    pub path: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct RefProxyEnv {
    pub source: String,
    pub line: String,
}

// ── Storage ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct RefStorage {
    pub fstab_entries: Vec<RefFstabEntry>,
    pub mount_points: Vec<RefMountPoint>,
    pub lvm_volumes: Vec<RefLvmVolume>,
    pub var_directories: Vec<RefVarDirectory>,
    pub credential_refs: Vec<RefCredentialRef>,
}

#[derive(Debug, Clone)]
pub struct RefFstabEntry {
    pub device: String,
    pub mount_point: String,
    pub fstype: String,
    pub options: String,
}

#[derive(Debug, Clone)]
pub struct RefMountPoint {
    pub target: String,
    pub source: String,
    pub fstype: String,
    pub options: String,
}

#[derive(Debug, Clone)]
pub struct RefLvmVolume {
    pub vg_name: String,
    pub lv_name: String,
    pub lv_size: String,
}

#[derive(Debug, Clone)]
pub struct RefVarDirectory {
    pub path: String,
    pub size_estimate: String,
    pub recommendation: String,
}

#[derive(Debug, Clone)]
pub struct RefCredentialRef {
    pub credential_path: String,
    pub mount_point: String,
    pub source: String,
}

// ── Generic ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct GenericRefItem {
    pub id: String,
    pub key: String,
    pub summary: Option<String>,
    pub content: Option<String>,
    pub tags: Vec<String>,
}
