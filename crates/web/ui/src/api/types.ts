// TypeScript types mirroring inspectah Rust DTOs.
// Source of truth: inspectah-refine/src/types.rs, inspectah-core/src/types/,
// inspectah-web/src/handlers.rs

import type { ExportWarning } from "./exportWarnings";

// --- Finding disposition (inspectah-core/src/types/finding.rs) ---

/** Rust: AdvisoryType — #[serde(rename_all = "snake_case")] */
export type AdvisoryType =
  | "unbacked_var_dir"
  | "cross_tree_symlink"
  | "modernization"
  | "content_not_captured";

/**
 * Rust: FindingKind — #[serde(tag = "kind", rename_all = "snake_case")]
 *
 * Three dispositions, not a boolean. Only `actionable` carries an `include`
 * key, so reading `include` off the other two yields `undefined` — which is
 * why this is a discriminated union and not `{ kind: string; include?: boolean }`.
 * Use the helpers in `api/disposition.ts` rather than reading fields directly.
 */
export type Disposition =
  | { kind: "actionable"; include: boolean }
  | { kind: "advisory"; advisory_type: AdvisoryType; rationale: string }
  | { kind: "inventory" };

// --- Package types (inspectah-core/src/types/rpm.rs) ---

/** Rust: #[serde(rename_all = "snake_case")] */
export type PackageState =
  "added" | "base_image_only" | "modified" | "local_install" | "no_repo";

export interface AggregatePrevalence {
  count: number;
  total: number;
  hosts: string[];
}

export interface PackageEntry {
  name: string;
  epoch: string;
  version: string;
  release: string;
  arch: string;
  state: PackageState;
  disposition: Disposition;
  locked?: boolean;
  acknowledged?: boolean;
  source_repo: string;
  aggregate: AggregatePrevalence | null;
  /** Triage annotation for repo-less packages (from Plan 2 backend). */
  repoless_annotation?: string;
  /** True if cached RPM was found in /var/cache/dnf/ (from Plan 2 backend). */
  repoless_cached?: boolean;
  /** Version of the uploaded RPM when it differs from the installed version. */
  uploaded_version?: string;
}

// --- Config types (inspectah-core/src/types/config.rs) ---

/** Rust: #[serde(rename_all = "snake_case")] */
export type ConfigFileKind =
  "rpm_owned_default" | "rpm_owned_modified" | "unowned" | "orphaned";

/** Rust: #[serde(rename_all = "snake_case")] */
export type ConfigCategory =
  | "tmpfiles"
  | "environment"
  | "audit"
  | "library_path"
  | "journal"
  | "logrotate"
  | "automount"
  | "sysctl"
  | "crypto_policy"
  | "identity"
  | "limits"
  | "other";

/**
 * Rust: ConfigShadow — #[serde(rename_all = "snake_case")]
 * Which other section's decision carries this file's content.
 */
export type ConfigShadow = "units";

export interface ConfigFileEntry {
  path: string;
  kind: ConfigFileKind;
  category: ConfigCategory;
  content: string;
  rpm_va_flags: string | null;
  package: string | null;
  diff_against_rpm: string | null;
  disposition: Disposition;
  locked?: boolean;
  attention_reason?: string | null;
  tie: boolean;
  tie_winner: boolean;
  aggregate: AggregatePrevalence | null;
  /**
   * Set when another section's decision carries this file. `disposition`
   * is `{ kind: "inventory" }` whenever this is present, and the key is
   * omitted entirely (not null) when the file owns its own decision.
   */
  shadowed_by?: ConfigShadow;
}

// --- Refine types (inspectah-refine/src/types.rs) ---

export interface PackageTarget {
  name: string;
  arch: string;
}

/** Rust: #[serde(rename_all = "snake_case")] */
export type AttentionLevel = "needs_review" | "informational" | "routine";

/**
 * Rust: #[serde(rename_all = "snake_case")]
 * The Custom variant serializes as { "custom": "detail string" }
 */
export type AttentionReason =
  | "package_baseline_match"
  | "package_user_added"
  | "package_version_changed"
  | "package_provenance_unavailable"
  | "package_local_install"
  | "package_no_repo_source"
  | "config_default"
  | "config_baseline_match"
  | "config_modified"
  | "config_unowned"
  | "config_orphaned"
  | "sensitive_path"
  | { custom: string };

export interface AttentionTag {
  level: AttentionLevel;
  reason: AttentionReason;
  detail: string | null;
}

// --- Triage types (inspectah-refine/src/types.rs) ---

/** Rust: #[serde(rename_all = "snake_case")] */
export type TriageBucket = "baseline" | "site" | "investigate";

/** Rust: #[serde(rename_all = "snake_case")] */
export type AggregateBucket =
  "investigate" | "divergent" | "partial" | "universal";

export interface Prevalence {
  count: number;
  total: number;
}

export interface AggregateTriage {
  bucket: AggregateBucket;
  prevalence: Prevalence;
}

/**
 * Rust: #[serde(tag = "mode")] with internally-tagged enum.
 * SingleHost(TriageBucket) serializes as {"mode":"single_host","<bucket>":null}
 * Aggregate(AggregateTriage) serializes as {"mode":"aggregate","bucket":"...","prevalence":{...}}
 */
export type Triage =
  | { mode: "single_host"; baseline?: null; site?: null; investigate?: null }
  | ({ mode: "aggregate" } & AggregateTriage);

/**
 * Rust: #[serde(rename_all = "snake_case")]
 * Unit variants serialize as strings; RequiresProjectedPackage has content.
 */
export type TriageAnnotation =
  | "sensitive_path"
  | "first_boot_provisioned"
  | { requires_projected_package: { name: string } };

/**
 * Rust: #[serde(rename_all = "snake_case")]
 * Typed classification reason.
 */
export type TriageReason =
  | "package_baseline_match"
  | "package_user_added"
  | "package_version_changed"
  | "package_provenance_unavailable"
  | "package_local_install"
  | "package_no_repo_source"
  | "package_config_captured"
  | "config_default"
  | "config_baseline_match"
  | "config_modified"
  | "config_unowned"
  | "config_orphaned"
  | "service_baseline_match"
  | "service_non_default_state"
  | "service_unknown_origin"
  | "service_drop_in_present"
  | "quadlet_user_deployed"
  | "quadlet_present_in_base_image"
  | "flatpak_provisioned_on_first_boot"
  | "flatpak_incomplete_provenance"
  | "tuned_baseline_match"
  | "tuned_non_default_profile"
  | "tuned_custom_profile"
  | "tuned_unusual_state"
  | "sensitive_path"
  | "package_platform_plumbing"
  | "package_installer_default"
  | "package_installer_promoted_service"
  | "package_installer_promoted_config"
  | "package_installer_ambiguous"
  | "package_installer_evidence_unavailable"
  | { custom: string };

export interface TriageTag {
  triage: Triage;
  primary_reason: TriageReason;
  annotations: TriageAnnotation[];
}

export interface RefinedPackage {
  entry: PackageEntry;
  /** @deprecated Legacy attention tags; use triage instead. */
  attention?: AttentionTag[];
  triage: TriageTag;
}

export interface RefinedConfig {
  entry: ConfigFileEntry;
  /** @deprecated Legacy attention tags; use triage instead. */
  attention?: AttentionTag[];
  triage: TriageTag;
  /**
   * Present when the export does not write the file this row offers
   * (spec § 11.1). Only `/api/view` sets it.
   */
  withheld?: WithheldView;
}

export interface BaselineSummary {
  image_ref: string;
  image_digest: string;
  strategy: string;
  baseline_count: number;
  user_added_count: number;
  review_count: number;
}

export interface SectionStats {
  kind: string;
  total: number;
  /** Actionable findings the user is keeping. */
  included: number;
  /** Actionable findings the user excluded. A decision, not a state. */
  excluded: number;
  /**
   * Display-only findings the user was never offered a decision on.
   * The three buckets partition the section:
   * `included + excluded + advisory === total`.
   *
   * Optional only so the many `SectionStats` fixtures that predate this
   * field need not restate a zero. The server always sends it (`usize`,
   * not `Option`), so a missing value means a hand-built fixture, never
   * real data -- and reading it as 0 renders one bucket less rather than
   * asserting a decision the user did not make.
   */
  advisory?: number;
}

export interface RefineStats {
  sections: SectionStats[];
  needs_review_count: number;
  ops_applied: number;
  can_undo: boolean;
  can_redo: boolean;
  baseline_available: boolean;
}

export interface RefinedView {
  packages: RefinedPackage[];
  config_files: RefinedConfig[];
  containerfile_preview: string;
  stats: RefineStats;
  generation: number;
  baseline_summary?: BaselineSummary;
}

/**
 * Rust: #[serde(tag = "op", content = "target")]
 * JSON: {"op": "SetInclude", "target": {"item_id": {...}, "include": true}}
 */
export type RefinementOp =
  | { op: "SetInclude"; target: { item_id: ItemId; include: boolean } }
  | { op: "UserStrategy"; target: { username: string; strategy: string } }
  | { op: "UserPassword"; target: UserPasswordOp }
  | { op: "SelectVariant"; target: { item_id: ItemId; target: string } }
  | {
      op: "EditVariant";
      target: { item_id: ItemId; content: string; based_on: string | null };
    }
  | { op: "DiscardVariant"; target: { item_id: ItemId; variant: string } }
  // Legacy ops kept for backward compat during migration
  | { op: "ExcludePackage"; target: PackageTarget }
  | { op: "IncludePackage"; target: PackageTarget }
  | { op: "ExcludeConfig"; target: { path: string } }
  | { op: "IncludeConfig"; target: { path: string } }
  | { op: "ExcludeRepo"; target: { section_id: string } }
  | { op: "IncludeRepo"; target: { section_id: string } };

/** Rust: #[serde(tag = "directive")] */
export type ViewDirective = {
  directive: "UngroupGroup";
  group_name: string;
};

/**
 * Rust: #[serde(tag = "kind")]
 * A single timeline entry sent to /api/op — either a refinement op or a view directive.
 */
export type TimelineEntry =
  ({ kind: "Op" } & RefinementOp) | ({ kind: "View" } & ViewDirective);

/**
 * Flat shape returned by /api/ops history (Rust uses #[serde(flatten)]).
 * Each entry carries an `active` flag indicating whether it is ahead of the cursor.
 */
export type AnnotatedTimelineEntry =
  | ({ kind: "Op"; active: boolean } & RefinementOp)
  | ({ kind: "View"; active: boolean } & ViewDirective);

/** Rust: #[serde(tag = "choice")] */
export type UserPasswordOp =
  | { choice: "New"; username: string; hash?: string | null }
  | { choice: "None"; username: string }
  | { choice: "Preserve"; username: string };

export interface ChangesSummary {
  packages_included: PackageTarget[];
  packages_excluded: PackageTarget[];
  configs_included: string[];
  configs_excluded: string[];
  repos_excluded: string[];
  variants_changed: number;
  is_dirty: boolean;
}

/**
 * Rust: #[serde(flatten)] on op field merges RefinementOp fields
 * into the top-level object alongside `active`.
 * JSON: {"op": "ExcludePackage", "target": {"name": "httpd", "arch": "x86_64"}, "active": true}
 */
export interface AnnotatedOp {
  op: string;
  target: unknown;
  active: boolean;
}

// --- Web handler types (inspectah-web/src/handlers.rs) ---

export interface ContextItem {
  id: string;
  title: string;
  subtitle: string | null;
  detail: string | null;
  searchable_text: string;
  /**
   * On a scheduled timer row only: the unit the timer is (`<name>.timer`).
   * The row has no include flag; the timer's state is its unit row's.
   */
  unit?: string;
  /** With `unit`: whether `unit_states` holds a row for that unit. */
  unit_row?: boolean;
  /**
   * On a generated timer row only: the generated-timer name, which the
   * view's `generated_timers` and `declined_generated` are keyed by.
   */
  generated_timer?: string;
}

export interface ContextSubsection {
  id: string;
  display_name: string;
  items: ContextItem[];
}

export interface ReferenceSection {
  id: string;
  display_name: string;
  items: ContextItem[];
  subsections?: ContextSubsection[];
  empty_reason?: string;
  /** True when the network section has ifcfg-format connections. */
  has_ifcfg?: boolean;
  /** Deprecation note text when ifcfg connections are present. */
  ifcfg_note?: string;
}

/** Rust: #[serde(rename_all = "snake_case")] */
export type RepoProvenance =
  | "verified"
  | "incomplete"
  | "disabled"
  | "stale"
  | "unknown";

/** Rust: #[serde(rename_all = "snake_case")] */
export type RepoTier = "distro" | "official_optional" | "third_party";

export interface RepoSourceEntry {
  repo: string;
  host_count: number;
}

export interface VersionChangeEntry {
  name: string;
  arch: string;
  host_version: string;
  base_version: string;
  host_epoch: string;
  base_epoch: string;
  direction: "upgrade" | "downgrade";
}

export interface RepoGroupInfo {
  section_id: string;
  provenance: RepoProvenance;
  is_distro: boolean;
  tier: RepoTier;
  package_count: number;
  /** In `packages_added`, actionable, and not locked. The repo control's
   *  effective member set. */
  effective_member_count: number;
  /** Of the effective members, those currently included. */
  effective_included: number;
  /** Of the effective members, those absent from the visible package list. */
  effective_unlisted: number;
  enabled: boolean;
}

export interface AggregateHealthInfo {
  host_count: number;
  hostnames: string[];
  zones_active: boolean;
  variant_count: number;
  label: string;
  merged_at: string;
}

export interface HealthResponse {
  status: string;
  host: {
    hostname: string;
    os_name: string;
    os_version: string;
    os_id: string;
    system_type: string;
    schema_version: number;
  };
  completeness: string;
  policy: { distro_repos: string[] };
  aggregate: AggregateHealthInfo | null;
  session_is_sensitive: boolean;
}

// --- Unit decision types (crates/web/src/web_types.rs) ---

/** Where a unit's file is on the source host (Rust: `FragmentView`). */
export type UnitFragment = "vendor" | "local" | "shadowed" | "absent";

/**
 * Why the export writes no command for a unit row (Rust: `DeclinedView`).
 * `text` is the Containerfile line after its marker and is shown as given;
 * `renders` selects only how the row looks.
 */
export interface DeclinedView {
  renders: "omitted" | "fixme" | "advisory";
  /** The `OmissionReason` variant name. Not display copy. */
  reason: string;
  text: string;
}

/**
 * The row a generated unit's request came from (Rust: `GeneratedRowView`,
 * tagged on `row`): a scheduled generated-timer row by its name, or the
 * flatpak applications, which share one provisioning service.
 */
export type GeneratedRowView =
  | { row: "scheduled"; name: string }
  | { row: "flatpak" };

/** A generated-timer row's decision (Rust: `GeneratedTimerDto`). */
export interface GeneratedTimerDto {
  /** The `generated_timer_units` name, the `ItemId::GeneratedTimer` key. */
  name: string;
  include: boolean;
  locked: boolean;
  /**
   * The excluded cron job this unit is generated from. While set, the
   * server refuses including the row.
   */
  cron_job_excluded?: string;
}

/** A generated unit's request the export declined, and its row. */
export interface DeclinedGeneratedView {
  row: GeneratedRowView;
  /** The unit the request would have enabled. */
  unit: string;
  declined: DeclinedView;
}

/**
 * Why the export does not write a file a row offered (Rust:
 * `WithheldView`, tagged on `outcome`). `text` is shown as given.
 */
export type WithheldView =
  | { outcome: "withheld"; why: string; text: string }
  | { outcome: "refused"; rule: string; text: string };

/**
 * An administrator's mount, automount or swap unit (Rust:
 * `MountFindingView`). No `include`: the row has no toggle.
 */
export interface MountFindingDto {
  unit: string;
  /** `mount`, `automount` or `swap`, lowercase as served. */
  kind: string;
  state: string | null;
  mount_point: string | null;
  fragment: UnitFragment;
  /** The config row that holds the unit file and its toggle. */
  unit_file_path: string | null;
  /** The Containerfile's advisory lines, shown as given, one per line. */
  advisories: string[];
}

/** A classified unit state change, projected for the view response. */
export interface ServiceDecisionDto {
  unit: string;
  /** The type suffix, lowercase as served (`service`, `socket`, ...). */
  kind: string;
  /** The template, when the unit is an instance. */
  instance_of?: string;
  fragment: UnitFragment;
  /** The `/etc` unit file, for a local or shadowed fragment. */
  unit_file_path?: string;
  /** Present when the export writes no command for the row. */
  declined?: DeclinedView;
  triage: TriageTag;
  include: boolean;
  locked: boolean;
  attention_reason?: string | null;
  owning_package?: string | null;
  default_state?: string | null;
  current_state: string;
}

/** A classified service drop-in override, projected for the view response. */
export interface DropInDecisionDto {
  unit: string;
  path: string;
  triage: TriageTag;
  include: boolean;
  locked: boolean;
  attention_reason?: string | null;
  /** Shadow type (e.g. "full_shadow", "drop_in"). */
  shadow_type?: string | null;
  /** Rationale text for the shadow override. */
  shadow_rationale?: string | null;
  /** The parent unit's kind, lowercase as served. */
  kind?: string;
  /** Present when the export does not write the file this row offers. */
  withheld?: WithheldView;
}

// --- Sysctl panel types (crates/web/src/web_types.rs) ---
//
// The read-only sysctl panel. Nothing here is a decision: no disposition,
// no item id, nothing an op can target. Each `/etc` sysctl file is decided
// by its Configuration Files row, which `config_path` names. Values are raw
// host strings.

/** Rust: SysctlViewDto, tagged on `state`. */
export type SysctlViewDto =
  | ({ state: "host" } & SysctlHostView)
  | { state: "aggregate" }
  | { state: "unavailable"; reason: string };

/** Rust: SysctlHostView. */
export interface SysctlHostView {
  explanation: SysctlExplanationDto;
  vendor_replacements: VendorReplacementDto[];
  not_carried: NotCarriedDto[];
  as_written: AsWrittenLineDto[];
  /** Every collected file, sorted by (name, dir). */
  files: SysctlFileDto[];
  tuned_active: string | null;
}

/** Rust: SysctlExplanationDto, tagged on `state`. */
export type SysctlExplanationDto =
  | {
      state: "resolved";
      settings: SysctlSettingDto[];
      /** The `differs` rows of `settings`, in key order. */
      drift: SysctlDriftDto[];
    }
  | { state: "unknown"; inputs: UnknownInputDto[] };

/** Rust: SysctlLineRefDto. `path` is the directory joined with the name. */
export interface SysctlLineRefDto {
  path: string;
  line: number;
  /** Set when the file was read through a link. */
  link_target: string | null;
}

/** Rust: SysctlSettingDto. */
export interface SysctlSettingDto {
  key: string;
  outcome: SysctlOutcomeDto;
  /** Earlier entries for this key, in apply order. */
  overwritten: SysctlLineRefDto[];
}

/** Rust: SysctlOutcomeDto, tagged on `kind`. */
export type SysctlOutcomeDto =
  | {
      kind: "writes";
      value: string;
      from: SysctlLineRefDto;
      runtime: RuntimeDto;
    }
  | {
      kind: "cancelled";
      by: SysctlLineRefDto;
      /** Null when the kernel did not list the key. */
      runtime: string | null;
    };

/** Rust: RuntimeDto, tagged on `kind`. */
export type RuntimeDto =
  | { kind: "matches" }
  | { kind: "differs"; runtime: string }
  | { kind: "not_exposed" };

/** Rust: SysctlDriftDto. */
export interface SysctlDriftDto {
  key: string;
  file_value: string;
  runtime: string;
  from: SysctlLineRefDto;
}

/** Rust: UnknownInputDto, tagged on `kind`. */
export type UnknownInputDto =
  | { kind: "outside_link"; path: string; target: string }
  | { kind: "redacted_file"; path: string }
  | { kind: "redacted_runtime"; key: string };

/** Rust: VendorReplacementDto. */
export interface VendorReplacementDto {
  action: ReplacementAction;
  /** `/etc/sysctl.d/<name>`. */
  path: string;
  /** `/usr/lib/sysctl.d/<name>`. */
  vendor_path: string;
  carriage: CarriageDto;
}

/** Rust: ReplacementAction. */
export type ReplacementAction = "shadows" | "masks";

/** Rust: CarriageDto, tagged on `kind`. */
export type CarriageDto =
  | { kind: "carried"; config_path: string }
  | { kind: "excluded"; config_path: string }
  | { kind: "advisory"; config_path: string }
  | { kind: "refused"; config_path: string; reason: string }
  | { kind: "no_config_entry" };

/** Rust: NotCarriedDto. */
export interface NotCarriedDto {
  path: string;
  reason: NotCarriedReason;
}

/** Rust: NotCarriedReason, tagged on `kind`. */
export type NotCarriedReason =
  | { kind: "runtime_dir" }
  | { kind: "usr_local_lib" }
  | { kind: "excluded"; config_path: string }
  | { kind: "advisory"; config_path: string }
  | { kind: "refused"; config_path: string; reason: string }
  | { kind: "no_config_entry" };

/** Rust: AsWrittenLineDto, tagged on `kind`. */
export type AsWrittenLineDto =
  | {
      kind: "glob_assign";
      at: SysctlLineRefDto;
      pattern: string;
      value: string;
    }
  | { kind: "glob_exclude"; at: SysctlLineRefDto; pattern: string }
  | { kind: "unparsed"; at: SysctlLineRefDto; text: string };

/** Rust: inspectah_core SysctlDir. */
export type SysctlDir = "etc" | "run" | "usr_local_lib" | "usr_lib";

/** Rust: SysctlFileDto. */
export interface SysctlFileDto {
  path: string;
  dir: SysctlDir;
  body: SysctlFileBodyDto;
  /** Set when `carriage()` names a config entry. */
  config_path: string | null;
}

/** Rust: SysctlFileBodyDto, tagged on `kind`. */
export type SysctlFileBodyDto =
  | { kind: "read"; link_target: string | null }
  | { kind: "mask" }
  | { kind: "outside_link"; target: string };

/** A classified tuned profile selection, projected for the view response. */
export interface TunedDecisionDto {
  active_profile: string;
  custom_profiles: string[];
  triage: TriageTag;
  include: boolean;
  locked: boolean;
}

// --- Container decision types (inspectah-web/src/handlers.rs) ---

/** A classified quadlet unit, projected for the view response. */
export interface QuadletDecisionDto {
  path: string;
  name: string;
  image: string;
  triage: TriageTag;
  include: boolean;
  locked: boolean;
  content?: string | null;
}

/** A classified flatpak app, projected for the view response. */
export interface FlatpakDecisionDto {
  app_id: string;
  remote: string;
  branch: string;
  triage: TriageTag;
  include: boolean;
  locked: boolean;
  lifecycle: string;
}

// --- Package group types (inspectah-web/src/web_types.rs) ---

/** A single member of an installed DNF group. */
export interface GroupMemberInfo {
  name: string;
  locked: boolean;
  overlap_groups: string[];
  in_base_image: boolean;
}

/** Summary of an installed DNF group and its rendering state. */
export interface GroupInfo {
  name: string;
  member_count: number;
  /** In `packages_added`, actionable, and not locked (§ 3.7). */
  effective_member_count: number;
  /** Of the effective members, those currently included. */
  effective_included: number;
  added_count: number;
  locked_count: number;
  optional_spillover_count: number;
  render_state: "renderable" | "excluded" | "ungrouped" | "degraded" | "inert";
  degradation_reason: string | null;
  members: GroupMemberInfo[];
}

/** Provenance of a package in the individual zone due to group rendering. */
export interface PackageProvenance {
  kind: "optional_spillover" | "ungrouped_member" | "degraded_member";
  group_name: string;
}

/**
 * View endpoint response: RefinedView's wire fields, listed by the server
 * with a `withheld` on each config row, and the decision lists beside them.
 */
export interface ViewResponse extends RefinedView {
  repo_groups: RepoGroupInfo[];
  version_changes: VersionChangeEntry[];
  unit_states: ServiceDecisionDto[];
  unit_dropins: DropInDecisionDto[];
  /** The administrator's mount, automount and swap units. Always present. */
  mount_findings: MountFindingDto[];
  quadlets: QuadletDecisionDto[];
  flatpaks: FlatpakDecisionDto[];
  /**
   * Each request for a unit inspectah generates that the export does not
   * write, with the row it is shown on. Always present.
   */
  declined_generated: DeclinedGeneratedView[];
  /** Each generated-timer row's decision, by its name. Always present. */
  generated_timers: GeneratedTimerDto[];
  /** The read-only sysctl panel. Always present. */
  sysctl: SysctlViewDto;
  tuned: TunedDecisionDto[];
  users_groups_decisions: UserDecision[];
  package_groups: GroupInfo[];
  package_provenances?: Record<string, PackageProvenance>;
  session_is_sensitive: boolean;
  /** Language package environments (Tier 1 non-RPM). */
  language_packages?: LanguagePackageEnv[];
  /** Unmanaged file groups (Tier 2, flag-gated). Present only when --include-unmanaged was used. */
  unmanaged_files?: UnmanagedFileGroup[];
  /** Whether --include-unmanaged was used at scan time. Drives discoverability hint. */
  has_unmanaged_scan?: boolean;
  /**
   * Auto packages the export leaves out, in the `# Not carried` block's
   * order (unit owners first). `[]` both when nothing is listed and when
   * the classifier fell back; the payload does not say which. Contract:
   * process-docs/skills/not-carried-auto-packages.md, Refine projection
   * contract.
   */
  not_carried_packages: NotCarriedRow[];
  /**
   * The notice the next export puts first in `x-export-warnings`, byte for
   * byte, or null when no listed package owns an enabled unit.
   */
  not_carried_notice: ExportWarning | null;
}

/** One row of `not_carried_packages` (Rust: `NotCarriedRow`). */
export interface NotCarriedRow {
  /** The identity `SetInclude` takes. Pass it back verbatim. */
  item_id: ItemIdPackage;
  /** `name`, or `name.arch` when two arches of the name are listed. */
  display_name: string;
  source_repo: string;
  /** Enabled units the package owns, sorted. Display only. */
  enabled_units: string[];
  /** The server's "owns an enabled unit". Never derive it on the client. */
  owns_enabled_unit: boolean;
}

// --- User decision types (inspectah-core/src/types/users.rs) ---

/** User decision JSON shape from projected snapshot. */
export interface UserDecision {
  name: string;
  uid: number;
  gid: number;
  shell: string;
  home: string;
  include: boolean;
  classification: "interactive" | "non-interactive";
  containerfile_strategy: "skip" | "useradd";
  password_choice: "none" | "preserve" | "new";
  password_hash?: string;
  /** Enrichment: whether sudoers rules grant this user sudo access. */
  has_sudo?: boolean;
  /** Enrichment: whether this user has subuid allocations. */
  has_subuid?: boolean;
  /** Enrichment: number of SSH authorized keys found. */
  ssh_key_count?: number;
  /** Enrichment: full SSH key lines (only when preserve_ssh_keys is enabled). */
  ssh_keys?: string[];
  /** Enrichment: human-readable rationale for the classification. */
  classification_rationale?: string;
  /** Enrichment: supplementary group memberships (including system groups). */
  supplementary_groups?: string[];
  /** Enrichment: password status from /etc/shadow (locked, disabled, password_set, etc.). */
  password_status?: string;
}

/** Response from /api/user-preview. */
export interface UserPreviewResponse {
  kickstart: string;
  blueprint_toml: string;
  sensitive: boolean;
}

// --- Aggregate types (inspectah-web/src/handlers/aggregate.rs) ---

/** ItemId uses tag/content serde (Rust: #[serde(tag = "kind", content = "key")]) */
export interface ItemIdPackage {
  kind: "Package";
  key: { name: string; arch: string };
}

export interface ItemIdConfig {
  kind: "Config";
  key: { path: string };
}

export interface ItemIdRepo {
  kind: "Repo";
  key: { path: string };
}

export interface ItemIdModuleStream {
  kind: "ModuleStream";
  key: { module_stream: string };
}

export interface ItemIdVersionLock {
  kind: "VersionLock";
  key: { name_arch: string };
}

export interface ItemIdUnit {
  kind: "Unit";
  key: { unit: string };
}

export interface ItemIdDropIn {
  kind: "DropIn";
  key: { path: string };
}

export interface ItemIdQuadlet {
  kind: "Quadlet";
  key: { path: string };
}

export interface ItemIdCompose {
  kind: "Compose";
  key: { path: string };
}

export interface ItemIdFlatpak {
  kind: "Flatpak";
  key: { app_id: string; remote: string; branch: string };
}

export interface ItemIdNMConnection {
  kind: "NMConnection";
  key: { path: string };
}

export interface ItemIdFirewallZone {
  kind: "FirewallZone";
  key: { path: string };
}

export interface ItemIdKernelModule {
  kind: "KernelModule";
  key: { name: string };
}

export interface ItemIdTunedSelection {
  kind: "TunedSelection";
  key: { profile: string };
}

export interface ItemIdCronJob {
  kind: "CronJob";
  key: { path: string };
}

export interface ItemIdAtJob {
  kind: "AtJob";
  key: { file: string };
}

export interface ItemIdGeneratedTimer {
  kind: "GeneratedTimer";
  key: { name: string };
}

export interface ItemIdSelinuxPort {
  kind: "SelinuxPort";
  key: { protocol_port: string };
}

export interface ItemIdFstab {
  kind: "Fstab";
  key: { mount_point: string };
}

export interface ItemIdNonRpm {
  kind: "NonRpm";
  key: { name: string };
}

export interface ItemIdGroup {
  kind: "Group";
  key: { name: string };
}

export interface ItemIdLanguageEnv {
  kind: "LanguageEnv";
  key: { ecosystem: string; path: string };
}

export interface ItemIdUnmanagedFile {
  kind: "UnmanagedFile";
  key: { path: string };
}

/**
 * A language package environment (pip venv, npm project, gem project).
 * Identity contract: matches Plan 1's ItemId::LanguageEnv { ecosystem, path }.
 */
export interface LanguagePackageEnv {
  /** Ecosystem identifier: "pip" | "npm" | "gem". */
  ecosystem: "pip" | "npm" | "gem";
  /** Absolute path to the environment root. */
  path: string;
  /** Method string from Plan 1 contract. */
  method: string;
  /** Structured package entries in this environment. */
  packages: LanguagePackageDto[];
  /** Confidence level from collector. */
  confidence: "high" | "medium" | "low";
  /** How the environment was discovered (e.g., "requirements.txt", "dist-info", "package-lock.json", "Gemfile.lock"). */
  manifest_basis: string;
  /** Whether to include in export. */
  include: boolean;
  /** Whether any package in the environment has C extensions. */
  has_c_extensions: boolean;
  /** Whether the environment inherits system site-packages (pip venvs). */
  system_site_packages: boolean;
}

/**
 * Provenance signals for an unmanaged file.
 * Matches Plan 2's ProvenanceSignals struct — carry through for
 * reviewability, don't narrow at the DTO layer.
 */
export interface ProvenanceSignals {
  /** File type classification. */
  file_type:
    | "elf_binary"
    | "jar"
    | "script"
    | "data_file"
    | "config"
    | "symlink"
    | "other";
  /** Last-modified timestamp (seconds since epoch). */
  last_modified: number;
  /** Filesystem UID. */
  uid: number;
  /** Filesystem GID. */
  gid: number;
  /** Octal file permissions (e.g., "0755"). */
  permissions: string;
  /** True when file's mtime is newer than system install date. */
  mutability: boolean;
  /** True when file lives on a read-write mount point. */
  writable_mount: boolean;
  /** True when file is under a systemd service's WorkingDirectory. */
  service_working_dir: boolean;
}

/** A single unmanaged file discovered by --include-unmanaged. */
export interface UnmanagedFileItem {
  /** Absolute file path (matches Plan 1's ItemId::UnmanagedFile { path }). */
  path: string;
  /** File size in bytes. */
  size: number;
  /** Whether path is under /var. */
  is_var_path: boolean;
  /** Whether to include in export. */
  include: boolean;
  /** Provenance signals from Plan 2's backend. */
  provenance: ProvenanceSignals;
  /**
   * Why the export does not write this included file, as a config row's
   * `withheld` says it. `text` is shown as given.
   */
  withheld?: WithheldView;
}

/** Directory group for unmanaged files. */
export interface UnmanagedFileGroup {
  /** Parent directory path. */
  directory: string;
  /** Items in this directory. */
  items: UnmanagedFileItem[];
}

// ---------------------------------------------------------------------------
// Aggregate section metadata DTOs (mirrors Rust LanguagePackageMetadata, etc.)
// ---------------------------------------------------------------------------

/** Section-specific metadata for language package aggregate items. */
export interface LanguagePackageMetadata {
  ecosystem: string;
  confidence: string;
  package_count: number;
  manifest_basis: string | null;
  packages: LanguagePackageDto[];
}

export interface LanguagePackageDto {
  name: string;
  detected_version: string;
  pinned: boolean;
}

/** Section-specific metadata for unmanaged file aggregate items. */
export interface UnmanagedFileMetadata {
  file_type: string;
  size: number;
  under_var: boolean;
  provenance: UnmanagedFileProvenanceDto;
}

export interface UnmanagedFileProvenanceDto {
  last_modified: number;
  uid: number;
  gid: number;
  permissions: string;
  writable_mount: boolean;
  mutability: boolean;
  service_working_dir: boolean;
}

// ---------------------------------------------------------------------------
// Aggregate variant payload DTOs (mirrors Rust variant payload structs)
// ---------------------------------------------------------------------------

/** Variant payload for language packages — package-list diff inputs. */
export interface LanguagePackageVariantPayload {
  variant_packages: VariantPackageList[];
}

export interface VariantPackageList {
  content_hash: string;
  hosts: string[];
  host_count: number;
  selected: boolean;
  packages: LanguagePackageDto[];
}

/** Variant payload for unmanaged files — metadata comparison inputs. */
export interface UnmanagedFileVariantPayload {
  variant_metadata: VariantFileMetadata[];
}

export interface VariantFileMetadata {
  content_hash: string;
  hosts: string[];
  host_count: number;
  selected: boolean;
  size: number;
  last_modified: number;
}

/**
 * Row state for repo-less RPM packages.
 * Derived from Plan 2's backend fields (repoless_annotation, repoless_cached)
 * combined with local upload state. See spec "RPM Upload Row Contract".
 */
export type RpmUploadRowState =
  | "cached_excluded" // Cached RPM found, pre-excluded (no GPG)
  | "cached_included" // Cached RPM found, user-included
  | "needs_upload" // No RPM anywhere, needs user upload
  | "uploaded_excluded" // RPM uploaded via POST /api/upload-rpm, pre-excluded
  | "uploaded_included"; // RPM uploaded, user-included

export type ItemId =
  | ItemIdPackage
  | ItemIdConfig
  | ItemIdRepo
  | ItemIdModuleStream
  | ItemIdVersionLock
  | ItemIdUnit
  | ItemIdDropIn
  | ItemIdQuadlet
  | ItemIdCompose
  | ItemIdFlatpak
  | ItemIdNMConnection
  | ItemIdFirewallZone
  | ItemIdKernelModule
  | ItemIdTunedSelection
  | ItemIdCronJob
  | ItemIdAtJob
  | ItemIdGeneratedTimer
  | ItemIdSelinuxPort
  | ItemIdFstab
  | ItemIdNonRpm
  | ItemIdGroup
  | ItemIdLanguageEnv
  | ItemIdUnmanagedFile;

export interface ActionableVariantItem {
  item_id: ItemId;
  section_id: string;
  variant_count: number;
  max_host_spread: number;
}

export interface AggregateSummary {
  host_count: number;
  actionable_variant_items: ActionableVariantItem[];
  informational_variant_count: number;
}

/** @deprecated Use AggregateTriageDto instead. */
export interface AggregateAttention {
  level: string;
  reason: string;
  zone?: string;
  prevalence: number;
}

/** Aggregate triage classification from the backend. */
export interface AggregateTriageDto {
  bucket: AggregateBucket;
  prevalence: Prevalence;
}

export interface AggregateVariantOption {
  hash: string;
  hosts: string[];
  host_count: number;
  selected: boolean;
}

export interface AggregateVariants {
  count: number;
  selected: string; // content hash
  options: AggregateVariantOption[];
}

export interface AggregateItemPrevalence {
  count: number;
  total: number;
}

export interface AggregateItem {
  item_id: ItemId;
  include: boolean;
  locked: boolean;
  attention_reason?: string | null;
  triage: AggregateTriageDto;
  /** @deprecated Use triage.prevalence instead. */
  prevalence: AggregateItemPrevalence;
  variants?: AggregateVariants;
  source_repo: string;
  repo_conflict?: RepoSourceEntry[];
  /**
   * Section-specific per-item metadata, serialized as JSON.
   * Language packages: LanguagePackageMetadata.
   * Unmanaged files: UnmanagedFileMetadata.
   */
  section_metadata?: Record<string, unknown>;
  /**
   * Section-specific variant payload, serialized as JSON.
   * Only present when the item has variants across hosts.
   * Language packages: LanguagePackageVariantPayload.
   * Unmanaged files: UnmanagedFileVariantPayload.
   * Track C T12 reads this to render variant diff views.
   */
  variant_payload?: Record<string, unknown>;
}

export interface AggregateZoneGroup {
  items: AggregateItem[];
  count: number;
}

export interface AggregateZones {
  consensus: AggregateZoneGroup;
  near_consensus: AggregateZoneGroup;
  divergent: AggregateZoneGroup;
}

export interface AggregateSection {
  id: string;
  display_name: string;
  is_decision_section: boolean;
  zones?: AggregateZones;
  items?: AggregateItem[];
}

export interface AggregateViewResponse {
  generation: number;
  can_undo: boolean;
  can_redo: boolean;
  containerfile_preview: string;
  session_is_sensitive: boolean;
  summary: AggregateSummary;
  sections: AggregateSection[];
  repo_groups: RepoGroupInfo[];
  repo_conflict_count: number;
  /** The read-only sysctl panel; always `aggregate` on a fleet. */
  sysctl: SysctlViewDto;
}

export interface LineRange {
  start: number;
  count: number;
}

export interface DiffChange {
  kind: string; // "equal" | "delete" | "insert"
  content: string;
}

export interface DiffHunk {
  base_range: LineRange;
  target_range: LineRange;
  changes: DiffChange[];
}

export interface DiffStats {
  total_changes: number;
  insertions: number;
  deletions: number;
}

export interface AggregateDiffRequest {
  item_id: ItemId;
  base: string;
  target: string;
}

export interface AggregateDiffResponse {
  base_hash: string;
  target_hash: string;
  base_hosts: string[];
  target_hosts: string[];
  hunks: DiffHunk[];
  stats: DiffStats;
}

// --- Group metadata types (from GET /api/groups) ---

/** Metadata for a single section within a group. */
export interface SectionMetaDto {
  id: string;
  label: string;
  is_triage: boolean;
}

/** Metadata for a section group, consumed by the sidebar. */
export interface SectionGroupMeta {
  slug: string;
  label: string;
  sections: SectionMetaDto[];
  has_actionable_sections: boolean;
}

// --- Error type ---

export class ApiError extends Error {
  status: number;
  body: { error: string };

  constructor(status: number, body: { error: string }) {
    super(body.error);
    this.name = "ApiError";
    this.status = status;
    this.body = body;
  }
}
