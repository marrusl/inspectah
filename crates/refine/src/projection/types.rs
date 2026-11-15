use serde::{Deserialize, Serialize};

use crate::baseline_summary::BaselineSummary;
use crate::types::{
    ItemId, RefinedFlatpak, RefinedQuadlet, RefinedTunedSelection, RepoProvenance, RepoTier,
    TriageTag,
};
use inspectah_core::types::FindingKind;
use inspectah_core::types::config::{ConfigCategory, ConfigShadow};
use inspectah_core::types::containers::ComposeService;
use inspectah_core::types::rpm::{VersionChange, VersionChangeDirection};
use inspectah_core::types::storage::VarDirBacking;
use inspectah_core::types::units::{PresetDefault, SystemdDropIn, UnitName, UnitState};
use inspectah_core::types::users::UserGroupDecision;
use inspectah_pipeline::export_warning::ExportWarning;
use inspectah_pipeline::render::unit_plan::{AdvisoryReason, OmissionReason};
use inspectah_pipeline::render::var_dirs::EffectiveBacking;

// ── Decision projection ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionProjection {
    /// One row per unit state change (§ 11.1).
    pub unit_states: Vec<UnitStateView>,
    /// One row per drop-in or full shadow, whether or not its unit has a
    /// state change row.
    pub unit_dropins: Vec<DropInView>,
    /// One row per administrator's mount, automount or swap unit. These
    /// rows have no toggle.
    pub mount_findings: Vec<MountFindingView>,
    /// The included config rows whose file the export does not write, by
    /// the row's own `path`: only an included row offers a file, so the
    /// row is the included config entry at that path.
    pub config_withheld: Vec<ConfigWithheldView>,
    pub quadlets: Vec<RefinedQuadlet>,
    pub flatpaks: Vec<RefinedFlatpak>,
    pub tuned: Vec<RefinedTunedSelection>,
    pub repo_groups: Vec<RepoGroup>,
    pub version_changes: Vec<VersionChange>,
    pub users_groups: Vec<UserGroupDecision>,
    pub is_sensitive: bool,
    pub baseline_summary: Option<BaselineSummary>,
    /// The packages the export leaves out as not carried, as its
    /// `# Not carried` block lists them and in that order.
    pub not_carried: Vec<NotCarriedRow>,
    /// The notice the export sends for the rows that own an enabled unit.
    pub not_carried_notice: Option<ExportWarning>,
    /// Every request for a unit inspectah generates that the plan does not
    /// write, for the scheduled or flatpak row that asked for it (§ 11.1).
    pub declined_generated: Vec<DeclinedGeneratedView>,
    /// One row per generated-timer name in the scheduled section, with
    /// the include flag its `SetInclude` sets (§ 8): the one control of the
    /// units generated under that name.
    pub generated_timers: Vec<GeneratedTimerView>,
}

/// A generated-timer row's decision (§ 8, § 11.1). Two cron entries of one
/// file generate units of one name, which are one row: `SetInclude` on the
/// name decides them together.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratedTimerView {
    /// The `generated_timer_units` name, the row's `ItemId::GeneratedTimer`
    /// key.
    pub name: String,
    /// Whether the export carries the units of this name. Also false when
    /// the cron job they were generated from is excluded, which excludes
    /// them with it.
    pub include: bool,
    pub locked: bool,
}

/// The row a generated unit's request came from (§ 11.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "row", rename_all = "snake_case")]
pub enum GeneratedRowView {
    /// A scheduled `generated_timer_units` row; `name` is its name, the
    /// row's id in the scheduled section.
    Scheduled { name: String },
    /// The flatpak applications, which share one provisioning service.
    Flatpak,
}

/// A generated unit's request the plan declined, and the row to show it on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclinedGeneratedView {
    pub row: GeneratedRowView,
    /// The unit the request would have enabled.
    pub unit: UnitName,
    pub declined: DeclinedView,
}

// ── Unit rows (§ 11.1) ───────────────────────────────────────────

/// Where the unit file behind a row is on the source host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FragmentView {
    /// Under the vendor directory only (for a mount finding: with no
    /// package known to own it).
    Vendor,
    /// Under `/etc/systemd/system` only.
    Local,
    /// A vendor file with a copy in `/etc/systemd/system` that replaces it.
    Shadowed,
    /// No unit file: a mask on a name that has none.
    Absent,
}

/// How the Containerfile writes a command the plan declined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeclinedRenders {
    /// An `# Omitted:` line.
    Omitted,
    /// A `# FIXME:` line (§ 10.3).
    Fixme,
    /// An advisory block. § 10.2 rule 7 renders a declined command this
    /// way; no reason the plan gives today does.
    Advisory,
}

/// Why the export writes no command for a row, as the plan decided it
/// against the session's current snapshot (§ 11.1). Independent of the
/// row's `attention_reason`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclinedView {
    pub renders: DeclinedRenders,
    /// The `OmissionReason` variant's name (`OmissionReason::name`).
    pub reason: String,
    /// The Containerfile line's text after its marker, unescaped
    /// (`declined_line`).
    pub text: String,
}

/// Why the export does not write a file a row offered (§ 11.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum WithheldView {
    /// The stager withholds the file: the `WithheldWhy` variant's name and
    /// its text.
    Withheld { why: String, text: String },
    /// The offer's path is not a destination the stager accepts: the
    /// `PathError` variant's name and its text.
    Refused { rule: String, text: String },
}

/// One unit state change as refine shows it (§ 11.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnitStateView {
    pub unit: UnitName,
    /// The type suffix, lowercase (`service`, `mount`, ...).
    pub kind: String,
    /// The template, when the unit is an instance (`getty@.service` for
    /// `getty@tty2.service`).
    pub instance_of: Option<UnitName>,
    pub current_state: UnitState,
    pub default_state: Option<PresetDefault>,
    pub fragment: FragmentView,
    /// The `/etc` unit file the row was recorded from, for a local or
    /// shadowed fragment.
    pub unit_file_path: Option<String>,
    pub owning_package: Option<String>,
    pub include: bool,
    pub locked: bool,
    pub triage: TriageTag,
    pub attention_reason: Option<String>,
    /// Present when the plan writes no command for the row.
    pub declined: Option<DeclinedView>,
}

/// One drop-in or full shadow row (§ 11.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DropInView {
    pub entry: SystemdDropIn,
    pub triage: TriageTag,
    /// The parent unit's kind, lowercase; `None` when the parent's name is
    /// not a unit name.
    pub kind: Option<String>,
    /// Present when the export does not write the file this row offers.
    pub withheld: Option<WithheldView>,
}

/// An included config row whose file the export does not write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigWithheldView {
    /// The config row's `path`, as the row records it.
    pub path: String,
    pub withheld: WithheldView,
}

/// One administrator's mount, automount or swap unit (§ 11.1). No
/// `include`: the row has no toggle. No mount source: `unit_file_path`
/// names the config row that holds the scrubbed file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MountFindingView {
    pub unit: UnitName,
    /// `mount`, `automount` or `swap`.
    pub kind: String,
    /// The recorded state, enabled or masked; `None` when it is neither
    /// (the collector records no other state for these units).
    pub state: Option<UnitState>,
    pub mount_point: Option<String>,
    /// `local`, `shadowed`, `vendor` for an unowned vendor unit, `absent`
    /// for a mask with no unit file.
    pub fragment: FragmentView,
    /// The path of the config row that holds the unit file and its toggle,
    /// when the snapshot has one.
    pub unit_file_path: Option<String>,
    /// § 10.6's lines for the finding, as the Containerfile writes them
    /// before escaping.
    pub advisories: Vec<String>,
}

/// One package the export leaves out as not carried: an auto package
/// nothing the image installs needs (`not_carried_packages`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotCarriedRow {
    /// The package's identity, as a `SetInclude` op names it. Including
    /// it carries the package; excluding it again lists it here again.
    pub item_id: ItemId,
    /// The name the `# Not carried` block prints: `name`, or `name.arch`
    /// when the list holds more than one arch of that name.
    pub display_name: String,
    /// Repository the package was installed from.
    pub source_repo: String,
    /// Units the package owns that the unit inventory records as
    /// enabled, sorted.
    pub enabled_units: Vec<String>,
    /// The package owns an enabled unit. Exactly these rows are the
    /// packages `not_carried_notice` names, so a client reads this and
    /// never decides it from `enabled_units` or the unit rows.
    pub owns_enabled_unit: bool,
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
    pub collected: CollectedSections,
}

/// Which snapshot sections the scan collected at all. An empty
/// reference section means one of two different things, and only the
/// snapshot can tell them apart: the inspector collected nothing
/// (`data_unavailable`) or it collected a section that projects to no
/// rows (`zero_drift`) (§ 3.8).
#[derive(Debug, Clone, Copy, Default)]
pub struct CollectedSections {
    pub services: bool,
    pub containers: bool,
    pub network: bool,
    pub storage: bool,
    pub scheduled_tasks: bool,
    pub non_rpm_software: bool,
    pub kernel_boot: bool,
    pub selinux: bool,
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
    pub current_state: UnitState,
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
    pub reason: OmissionReason,
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

#[derive(Debug, Clone, PartialEq)]
pub enum EmptyReason {
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
    /// The decision the active tuned profile travels under, so the row
    /// that shows it is the row that decides it (§ 3.8).
    pub tuned_disposition: FindingKind,
    pub locale: Option<String>,
    pub timezone: Option<String>,
    /// Pointers at the `config.files` entries this section's subject
    /// matter lives in. The row mirrors the entry's decision rather than
    /// restating its content, so there is one decision per file and this
    /// surface is not a second place to make it.
    pub customizations_from_config: Vec<RefConfigRef>,
    pub alternatives: Vec<RefAlternativeEntry>,
}

/// A reference to a `config.files` entry, carried by whichever section
/// the file's subject matter belongs to.
#[derive(Debug, Clone)]
pub struct RefConfigRef {
    pub path: String,
    pub category: ConfigCategory,
    pub disposition: FindingKind,
    /// A locked entry's `SetInclude` is refused as a silent no-op, so a
    /// pointer row that dropped this would render a live control whose
    /// activation does nothing and says nothing.
    pub locked: bool,
    pub shadowed_by: Option<ConfigShadow>,
}

/// What a pointer row says about where the file's decision lives.
/// Every surface that renders one reads this, so they cannot diverge.
pub fn config_ref_summary(entry: &RefConfigRef) -> String {
    match entry.shadowed_by {
        Some(ConfigShadow::Units) => CONTROLLED_IN_SERVICE_DROP_INS.to_string(),
        None => DECIDED_UNDER_CONFIG_FILES.to_string(),
    }
}

/// Summary on a row that only points at the Configuration Files row
/// where the file is actually decided.
pub const DECIDED_UNDER_CONFIG_FILES: &str = "decided under Configuration Files";

/// Summary on a row whose file is carried by a `units.drop_ins`
/// entry. The Service Drop-ins row keyed by the same path is the one
/// decision, and the Configuration Files row is inventory.
pub const CONTROLLED_IN_SERVICE_DROP_INS: &str = "controlled in Service Drop-ins";

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
    /// True when any connection was collected from ifcfg format
    /// (`/etc/sysconfig/network-scripts/`).
    pub has_ifcfg: bool,
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
    /// Paths with no tmpfiles.d or systemd backing — no declarative lifecycle management.
    pub unbacked_var_paths: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RefFstabEntry {
    pub device: String,
    pub mount_point: String,
    pub fstype: String,
    pub options: String,
    pub include: bool,
    pub locked: bool,
    pub attention_reason: Option<String>,
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
    pub mode: Option<String>,
    pub owner_name: Option<String>,
    pub group_name: Option<String>,
    /// What created the directory on the source host, as collected.
    pub source: Option<VarDirBacking>,
    /// What the export can say about that creator reaching the image
    /// (§ 3.4). The view follows this, not `source`: a verdict the
    /// export cannot honour is not a backing.
    pub outcome: EffectiveBacking,
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
    /// On a scheduled timer row only: the unit the timer is, so the row can
    /// link to its control. `None` on every other row, and on a timer
    /// whose `<name>.timer` is not a unit name.
    pub unit_link: Option<TimerUnitLink>,
}

/// The unit a scheduled timer row describes (§ 11.1). The row has no
/// include flag: the timer's state is its unit row's, when it has one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimerUnitLink {
    /// `<name>.timer`.
    pub unit: UnitName,
    /// Whether `unit_states` holds a row for `unit`: one row per state
    /// change, so this is whether the snapshot records a state change for
    /// the timer.
    pub unit_row: bool,
}
