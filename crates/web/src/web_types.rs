// inspectah-web/src/web_types.rs
//
// Presentation-layer DTOs returned by the web API. Extracted from handlers.rs
// so that contract snapshot tests and future consumers can reference them
// without pulling in handler internals.

use std::collections::HashMap;

use inspectah_core::types::AdvisoryType;
use inspectah_core::types::sysctl::SysctlDir;
use inspectah_core::types::units::UnitName;
use inspectah_core::types::users::UserGroupDecision;
use inspectah_pipeline::export_warning::ExportWarning;
use inspectah_refine::baseline_summary::BaselineSummary;
use inspectah_refine::projection::{
    DeclinedGeneratedView, DeclinedView, FragmentView, MountFindingView, NotCarriedRow,
    WithheldView,
};
use inspectah_refine::types::{
    ItemId, RefineStats, RefinedConfig, RefinedPackage, RepoProvenance, RepoTier, TriageTag,
};
use serde::Serialize;

// -- Non-RPM DTOs (language packages + unmanaged files for the frontend) ----

/// A single language package within an environment.
#[derive(Serialize, Clone, Debug)]
pub struct LanguagePackageDto {
    pub name: String,
    pub detected_version: String,
    pub pinned: bool,
}

/// A language package environment (pip venv, npm project, gem project)
/// projected for the view response.
#[derive(Serialize, Clone, Debug)]
pub struct LanguagePackageEnvDto {
    pub ecosystem: String,
    pub path: String,
    pub method: String,
    pub packages: Vec<LanguagePackageDto>,
    pub confidence: String,
    pub manifest_basis: String,
    pub include: bool,
    pub has_c_extensions: bool,
    pub system_site_packages: bool,
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
    /// Why the export does not write this included file, as the config
    /// rows' `withheld` says it (§ 11.1). Absent when the file is written
    /// or the row is excluded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub withheld: Option<WithheldView>,
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

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct ContextItem {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub detail: Option<String>,
    pub searchable_text: String,
    /// The decision that governs this row, when the row is the place
    /// the decision is made. Absent on a row that only reports (§ 3.8).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<ContextDecision>,
    /// Present when the row states something the export cannot act on
    /// and the user cannot decide (§ 3.8).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub advisory: Option<ContextAdvisory>,
    /// On a scheduled timer row only: the unit the timer is, serialized
    /// as the row's `unit` and `unit_row` fields (§ 11.1). Absent on every
    /// other row.
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub unit_link: Option<ContextUnitLink>,
    /// On a generated timer row only: the generated-timer name, which the
    /// view's `generated_timers` and `declined_generated` are keyed by.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generated_timer: Option<String>,
}

/// The unit a scheduled timer row describes, so the row can link to the
/// unit row that holds the timer's state. The timer row itself has no
/// include flag.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ContextUnitLink {
    /// `<name>.timer`.
    pub unit: UnitName,
    /// Whether `unit_states` holds a row for `unit`.
    pub unit_row: bool,
}

/// The decision a reference row carries, keyed by the same [`ItemId`]
/// an `/api/op` payload names, so the row that shows a decision and the
/// op that changes it cannot disagree about which entity they mean.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ContextDecision {
    pub item_id: ItemId,
    pub include: bool,
    pub locked: bool,
}

/// An advisory on a reference row: a finding the export reports and no
/// one decides.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ContextAdvisory {
    pub advisory_type: AdvisoryType,
    pub rationale: String,
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
    /// The visible count over `view.packages` (§ 3.2). Never the
    /// denominator of a control -- see the three fields below for that.
    pub package_count: usize,
    /// In `packages_added`, actionable, and not locked (§ 3.2). The repo
    /// control's effective member set.
    pub effective_member_count: usize,
    /// Of the effective members, those currently included.
    pub effective_included: usize,
    /// Of the effective members, those absent from `view.packages` after
    /// its display filters.
    pub effective_unlisted: usize,
    /// `!repos_excluded().contains(section)`, the repo-exclusion history --
    /// not derived from the effective-member counts above (§ 3.2).
    pub enabled: bool,
}

/// A classified unit state change, projected for the view response
/// (§ 11.1). The name keeps "Service" from before units of every kind.
#[derive(Serialize, Clone, Debug)]
pub struct ServiceDecisionDto {
    pub unit: String,
    /// The type suffix, lowercase (`service`, `socket`, `mount`, ...).
    pub kind: String,
    /// The template, when the unit is an instance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_of: Option<String>,
    pub fragment: FragmentView,
    /// The `/etc` unit file, for a local or shadowed fragment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit_file_path: Option<String>,
    /// Present when the export writes no command for the row: how the
    /// Containerfile renders it, the reason's name and its text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub declined: Option<DeclinedView>,
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
    /// The parent unit's kind, lowercase; absent when the parent's name is
    /// not a unit name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Present when the export does not write the file this row offers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub withheld: Option<WithheldView>,
}

/// A config row for the view response: the refined row as before, and
/// why the export does not write the file it offers, when it does not
/// (§ 11.1).
#[derive(Serialize, Clone, Debug)]
pub struct ConfigFileDto {
    #[serde(flatten)]
    pub row: RefinedConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub withheld: Option<WithheldView>,
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

// -- Sysctl panel DTOs (read-only; built by `sysctl_view::project_sysctl`) --
//
// The sysctl panel explains the host's sysctl search path. It holds no
// decision: no disposition, no item id and nothing an op can target. Each
// `/etc` sysctl file is decided by its Configuration Files row, which
// `config_path` names. Values are raw host strings; the frontend renders
// them as text.

/// The sysctl panel for a view.
#[derive(Serialize, Clone, Debug)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SysctlViewDto {
    /// Single host with a collected section.
    Host(SysctlHostView),
    /// Fleet aggregate: no explanation is merged.
    Aggregate,
    /// Single host whose `kernel_boot` or `sysctl` is `None`; `reason`
    /// is the kernel/boot degradation.
    Unavailable { reason: String },
}

/// One host's sysctl explanation and the facts derived from it.
#[derive(Serialize, Clone, Debug)]
pub struct SysctlHostView {
    pub explanation: SysctlExplanationDto,
    pub vendor_replacements: Vec<VendorReplacementDto>,
    /// The sysctl files the image does not get.
    pub not_carried: Vec<NotCarriedDto>,
    /// Glob lines and unparsed lines, reported as written.
    pub as_written: Vec<AsWrittenLineDto>,
    /// Every collected file, sorted by (name, dir).
    pub files: Vec<SysctlFileDto>,
    pub tuned_active: Option<String>,
}

/// What systemd-sysctl would do on the host, or why it cannot be stated.
#[derive(Serialize, Clone, Debug)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SysctlExplanationDto {
    /// `drift` holds the `differs` rows of `settings`, in key order.
    Resolved {
        settings: Vec<SysctlSettingDto>,
        drift: Vec<SysctlDriftDto>,
    },
    Unknown {
        inputs: Vec<UnknownInputDto>,
    },
}

/// A line in a sysctl file. `path` is the directory joined with the name;
/// `link_target` is set when the file was read through a link.
#[derive(Serialize, Clone, Debug)]
pub struct SysctlLineRefDto {
    pub path: String,
    pub line: u32,
    pub link_target: Option<String>,
}

/// The outcome for one concrete key.
#[derive(Serialize, Clone, Debug)]
pub struct SysctlSettingDto {
    /// The canonical `SysctlKey`.
    pub key: String,
    pub outcome: SysctlOutcomeDto,
    /// Earlier entries for this key, in apply order.
    pub overwritten: Vec<SysctlLineRefDto>,
}

/// What the last entry for a key does.
#[derive(Serialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SysctlOutcomeDto {
    Writes {
        value: String,
        from: SysctlLineRefDto,
        runtime: RuntimeDto,
    },
    /// The last entry is an exclusion; `runtime` is `None` when the kernel
    /// did not list the key.
    Cancelled {
        by: SysctlLineRefDto,
        runtime: Option<String>,
    },
}

/// How the kernel's value compares with the value written.
#[derive(Serialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuntimeDto {
    Matches,
    Differs { runtime: String },
    NotExposed,
}

/// A key the kernel runs at a value other than the one the files request.
#[derive(Serialize, Clone, Debug)]
pub struct SysctlDriftDto {
    pub key: String,
    pub file_value: String,
    pub runtime: String,
    pub from: SysctlLineRefDto,
}

/// One input that makes the explanation unknown.
#[derive(Serialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UnknownInputDto {
    OutsideLink {
        path: String,
        target: String,
    },
    RedactedFile {
        path: String,
    },
    /// `key` is the canonical `SysctlKey`.
    RedactedRuntime {
        key: String,
    },
}

/// An `/etc/sysctl.d` file that replaces the same-named vendor file.
#[derive(Serialize, Clone, Debug)]
pub struct VendorReplacementDto {
    pub action: ReplacementAction,
    /// `/etc/sysctl.d/<name>`.
    pub path: String,
    /// `/usr/lib/sysctl.d/<name>`.
    pub vendor_path: String,
    pub carriage: CarriageDto,
}

/// Whether the admin file replaces the vendor file or masks it.
#[derive(Serialize, Clone, Copy, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ReplacementAction {
    Shadows,
    Masks,
}

/// How an `/etc` sysctl file reaches the image. `config_path` is the
/// Configuration Files row the panel links to: the file's own path, or
/// `/etc/sysctl.conf` for the package's `99-sysctl.conf` link.
#[derive(Serialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CarriageDto {
    Carried {
        config_path: String,
    },
    /// Included, but the export refuses `config_path`; `reason` is the
    /// exporter's refusal text.
    Refused {
        config_path: String,
        reason: String,
    },
    Excluded {
        config_path: String,
    },
    Advisory {
        config_path: String,
    },
    NoConfigEntry,
}

/// A sysctl file the image does not get, and why.
#[derive(Serialize, Clone, Debug)]
pub struct NotCarriedDto {
    pub path: String,
    pub reason: NotCarriedReason,
}

/// Why a sysctl file is not carried.
#[derive(Serialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NotCarriedReason {
    /// A file in `/run/sysctl.d`.
    RuntimeDir,
    /// A file in `/usr/local/lib/sysctl.d`, carried only on the
    /// unmanaged-file path.
    UsrLocalLib,
    /// Included, but the export refuses `config_path`; `reason` is the
    /// exporter's refusal text.
    Refused {
        config_path: String,
        reason: String,
    },
    Excluded {
        config_path: String,
    },
    Advisory {
        config_path: String,
    },
    NoConfigEntry,
}

/// A line reported as written: a glob, which is not expanded, or a line
/// the parser could not read.
#[derive(Serialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AsWrittenLineDto {
    GlobAssign {
        at: SysctlLineRefDto,
        pattern: String,
        value: String,
    },
    GlobExclude {
        at: SysctlLineRefDto,
        pattern: String,
    },
    Unparsed {
        at: SysctlLineRefDto,
        text: String,
    },
}

/// One collected sysctl file, without its content: the Configuration
/// Files row shows the content.
#[derive(Serialize, Clone, Debug)]
pub struct SysctlFileDto {
    pub path: String,
    pub dir: SysctlDir,
    pub body: SysctlFileBodyDto,
    /// Set when `carriage()` names a config entry.
    pub config_path: Option<String>,
}

/// What a collected file is.
#[derive(Serialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SysctlFileBodyDto {
    Read { link_target: Option<String> },
    Mask,
    OutsideLink { target: String },
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
    /// In `packages_added`, actionable, and not locked (§ 3.7).
    pub effective_member_count: usize,
    /// Of the effective members, those currently included.
    pub effective_included: usize,
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

/// A generated-timer row's decision as the view shows it.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct GeneratedTimerDto {
    /// The `generated_timer_units` name, the row's
    /// `ItemId::GeneratedTimer` key.
    pub name: String,
    pub include: bool,
    pub locked: bool,
    /// The excluded cron job this unit is generated from, which excludes
    /// it whatever its own row says. While set, including the row is
    /// refused: the cron job has to be included first.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cron_job_excluded: Option<String>,
}

/// The `/api/view` body. The first five fields are `RefinedView`'s wire
/// fields, listed here so each config row can carry its `withheld`; the
/// adapter destructures `RefinedView` whole, so a field added there does
/// not compile until it is placed here.
#[derive(Serialize)]
pub struct ViewResponse {
    pub packages: Vec<RefinedPackage>,
    pub config_files: Vec<ConfigFileDto>,
    pub containerfile_preview: String,
    pub stats: RefineStats,
    pub generation: u64,
    pub repo_groups: Vec<RepoGroupInfo>,
    pub baseline_summary: Option<BaselineSummary>,
    pub version_changes: Vec<VersionChangeEntry>,
    pub unit_states: Vec<ServiceDecisionDto>,
    pub unit_dropins: Vec<DropInDecisionDto>,
    /// The administrator's mount, automount and swap units. No `include`:
    /// these rows have no toggle.
    pub mount_findings: Vec<MountFindingView>,
    pub quadlets: Vec<QuadletDecisionDto>,
    pub flatpaks: Vec<FlatpakDecisionDto>,
    /// Each request for a unit inspectah generates that the export does
    /// not write, with the scheduled or flatpak row it is shown on
    /// (§ 11.1). Always present; empty when nothing is declined.
    pub declined_generated: Vec<DeclinedGeneratedView>,
    /// Each generated-timer row's decision, by its name (§ 8, § 11.1).
    /// Always present; empty when the host has no generated timer.
    pub generated_timers: Vec<GeneratedTimerDto>,
    /// The read-only sysctl panel. Always present.
    pub sysctl: SysctlViewDto,
    pub tuned: Vec<TunedDecisionDto>,
    pub users_groups_decisions: Vec<UserGroupDecision>,
    pub package_groups: Vec<GroupInfo>,
    /// Per-package provenance keyed by `"name.arch"` for packages that appear
    /// in the individual zone due to group rendering decisions.
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub package_provenances: HashMap<String, PackageProvenance>,
    /// Packages the export leaves out as not carried, in the order of its
    /// `# Not carried` block: unit owners first, then by name and arch.
    /// Always present; empty when nothing is listed or the snapshot cannot
    /// support the judgement. Contract:
    /// `process-docs/skills/not-carried-auto-packages.md`.
    pub not_carried_packages: Vec<NotCarriedRow>,
    /// The `notice` entry the export's `x-export-warnings` header will
    /// carry for the rows with `owns_enabled_unit`, or `null`.
    pub not_carried_notice: Option<ExportWarning>,
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
