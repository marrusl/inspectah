// TypeScript types mirroring inspectah Rust DTOs.
// Source of truth: inspectah-refine/src/types.rs, inspectah-core/src/types/,
// inspectah-web/src/handlers.rs

// --- Package types (inspectah-core/src/types/rpm.rs) ---

/** Rust: #[serde(rename_all = "snake_case")] */
export type PackageState =
  | "added"
  | "base_image_only"
  | "modified"
  | "local_install"
  | "no_repo";

export interface FleetPrevalence {
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
  include: boolean;
  acknowledged?: boolean;
  source_repo: string;
  fleet: FleetPrevalence | null;
}

// --- Config types (inspectah-core/src/types/config.rs) ---

/** Rust: #[serde(rename_all = "snake_case")] */
export type ConfigFileKind =
  | "rpm_owned_default"
  | "rpm_owned_modified"
  | "unowned"
  | "orphaned";

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

export interface ConfigFileEntry {
  path: string;
  kind: ConfigFileKind;
  category: ConfigCategory;
  content: string;
  rpm_va_flags: string | null;
  package: string | null;
  diff_against_rpm: string | null;
  include: boolean;
  tie: boolean;
  tie_winner: boolean;
  fleet: FleetPrevalence | null;
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
export type FleetBucket = "investigate" | "divergent" | "partial" | "universal";

export interface Prevalence {
  count: number;
  total: number;
}

export interface FleetTriage {
  bucket: FleetBucket;
  prevalence: Prevalence;
}

/**
 * Rust: #[serde(tag = "mode")] with internally-tagged enum.
 * SingleHost(TriageBucket) serializes as {"mode":"single_host","<bucket>":null}
 * Fleet(FleetTriage) serializes as {"mode":"fleet","bucket":"...","prevalence":{...}}
 */
export type Triage =
  | { mode: "single_host"; baseline?: null; site?: null; investigate?: null }
  | ({ mode: "fleet" } & FleetTriage);

/**
 * Rust: #[serde(rename_all = "snake_case")]
 * Unit variants serialize as strings; RequiresProjectedPackage has content.
 */
export type TriageAnnotation =
  | "sensitive_path"
  | "first_boot_provisioned"
  | { requires_projected_package: { name: string } }
  | "runtime_only_observation";

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
  | "sysctl_baseline_match"
  | "sysctl_file_backed_override"
  | "sysctl_no_baseline"
  | "tuned_baseline_match"
  | "tuned_non_default_profile"
  | "tuned_custom_profile"
  | "tuned_unusual_state"
  | "sensitive_path"
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
}

export interface BaselineSummary {
  image_ref: string;
  image_digest: string;
  strategy: string;
  baseline_count: number;
  user_added_count: number;
  review_count: number;
}

export interface RefineStats {
  total_packages: number;
  included_packages: number;
  excluded_packages: number;
  total_configs: number;
  included_configs: number;
  package_managed_configs: number;
  excluded_configs: number;
  needs_review_count: number;
  ops_applied: number;
  can_undo: boolean;
  can_redo: boolean;
  /** @deprecated Use baseline_summary instead. Kept for backward compatibility. */
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
  | { op: "EditVariant"; target: { item_id: ItemId; content: string; based_on: string | null } }
  | { op: "DiscardVariant"; target: { item_id: ItemId; variant: string } }
  // Legacy ops kept for backward compat during migration
  | { op: "ExcludePackage"; target: PackageTarget }
  | { op: "IncludePackage"; target: PackageTarget }
  | { op: "ExcludeConfig"; target: { path: string } }
  | { op: "IncludeConfig"; target: { path: string } }
  | { op: "ExcludeRepo"; target: { section_id: string } }
  | { op: "IncludeRepo"; target: { section_id: string } };

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
}

export interface ContextSubsection {
  id: string;
  display_name: string;
  items: ContextItem[];
}

export interface ContextSection {
  id: string;
  display_name: string;
  items: ContextItem[];
  subsections?: ContextSubsection[];
  empty_reason?: string;
}

/** Rust: #[serde(rename_all = "snake_case")] */
export type RepoProvenance = "verified" | "incomplete" | "unknown";

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
  enabled: boolean;
}

export interface FleetHealthInfo {
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
  fleet: FleetHealthInfo | null;
  session_is_sensitive: boolean;
}

// --- Service decision types (inspectah-web/src/handlers.rs) ---

/** A classified service state change, projected for the view response. */
export interface ServiceDecisionDto {
  unit: string;
  triage: TriageTag;
  include: boolean;
  owning_package?: string | null;
}

/** A classified service drop-in override, projected for the view response. */
export interface DropInDecisionDto {
  unit: string;
  path: string;
  triage: TriageTag;
  include: boolean;
}

/** View endpoint response: RefinedView + repo_groups. */
export interface ViewResponse extends RefinedView {
  repo_groups: RepoGroupInfo[];
  version_changes: VersionChangeEntry[];
  service_states: ServiceDecisionDto[];
  service_dropins: DropInDecisionDto[];
  users_groups_decisions: UserDecision[];
  session_is_sensitive: boolean;
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

// --- Fleet types (inspectah-web/src/handlers/fleet.rs) ---

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

export interface ItemIdService {
  kind: "Service";
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

export interface ItemIdSysctl {
  kind: "Sysctl";
  key: { key: string };
}

export interface ItemIdTunedSelection {
  kind: "TunedSelection";
  key: { profile: string };
}

export interface ItemIdCronJob {
  kind: "CronJob";
  key: { path: string };
}

export interface ItemIdSystemdTimer {
  kind: "SystemdTimer";
  key: { name: string };
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

export type ItemId =
  | ItemIdPackage
  | ItemIdConfig
  | ItemIdRepo
  | ItemIdModuleStream
  | ItemIdVersionLock
  | ItemIdService
  | ItemIdDropIn
  | ItemIdQuadlet
  | ItemIdCompose
  | ItemIdFlatpak
  | ItemIdNMConnection
  | ItemIdFirewallZone
  | ItemIdKernelModule
  | ItemIdSysctl
  | ItemIdTunedSelection
  | ItemIdCronJob
  | ItemIdSystemdTimer
  | ItemIdAtJob
  | ItemIdGeneratedTimer
  | ItemIdSelinuxPort
  | ItemIdFstab
  | ItemIdNonRpm;

export interface ActionableVariantItem {
  item_id: ItemId;
  section_id: string;
  variant_count: number;
  max_host_spread: number;
}

export interface FleetSummary {
  host_count: number;
  actionable_variant_items: ActionableVariantItem[];
  informational_variant_count: number;
}

/** @deprecated Use FleetTriageDto instead. */
export interface FleetAttention {
  level: string;
  reason: string;
  zone?: string;
  prevalence: number;
}

/** Fleet triage classification from the backend. */
export interface FleetTriageDto {
  bucket: FleetBucket;
  prevalence: Prevalence;
}

export interface FleetVariantOption {
  hash: string;
  hosts: string[];
  host_count: number;
  selected: boolean;
}

export interface FleetVariants {
  count: number;
  selected: string; // content hash
  options: FleetVariantOption[];
}

export interface FleetItemPrevalence {
  count: number;
  total: number;
}

export interface FleetItem {
  item_id: ItemId;
  include: boolean;
  triage: FleetTriageDto;
  /** @deprecated Use triage.prevalence instead. */
  prevalence: FleetItemPrevalence;
  variants?: FleetVariants;
  source_repo: string;
  repo_conflict?: RepoSourceEntry[];
}

export interface FleetZoneGroup {
  items: FleetItem[];
  count: number;
}

export interface FleetZones {
  consensus: FleetZoneGroup;
  near_consensus: FleetZoneGroup;
  divergent: FleetZoneGroup;
}

export interface FleetSection {
  id: string;
  display_name: string;
  is_decision_section: boolean;
  zones?: FleetZones;
  items?: FleetItem[];
}

export interface FleetViewResponse {
  generation: number;
  can_undo: boolean;
  can_redo: boolean;
  containerfile_preview: string;
  session_is_sensitive: boolean;
  summary: FleetSummary;
  sections: FleetSection[];
  repo_groups: RepoGroupInfo[];
  repo_conflict_count: number;
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

export interface FleetDiffRequest {
  item_id: ItemId;
  base: string;
  target: string;
}

export interface FleetDiffResponse {
  base_hash: string;
  target_hash: string;
  base_hosts: string[];
  target_hosts: string[];
  hunks: DiffHunk[];
  stats: DiffStats;
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
