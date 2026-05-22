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

export interface RefinedPackage {
  entry: PackageEntry;
  attention: AttentionTag[];
}

export interface RefinedConfig {
  entry: ConfigFileEntry;
  attention: AttentionTag[];
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
 * JSON: {"op": "ExcludePackage", "target": {"name": "httpd", "arch": "x86_64"}}
 */
export type RefinementOp =
  | { op: "ExcludePackage"; target: PackageTarget }
  | { op: "IncludePackage"; target: PackageTarget }
  | { op: "ExcludeConfig"; target: { path: string } }
  | { op: "IncludeConfig"; target: { path: string } }
  | { op: "ExcludeRepo"; target: { section_id: string } }
  | { op: "IncludeRepo"; target: { section_id: string } };

export interface ChangesSummary {
  packages_included: PackageTarget[];
  packages_excluded: PackageTarget[];
  configs_included: string[];
  configs_excluded: string[];
  repos_excluded: string[];
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

/** View endpoint response: RefinedView + repo_groups. */
export interface ViewResponse extends RefinedView {
  repo_groups: RepoGroupInfo[];
  leaf_dep_tree: Record<string, string[]>;
  version_changes: VersionChangeEntry[];
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
export interface ItemIdConfig {
  kind: "Config";
  key: { path: string };
}

export interface ItemIdPackage {
  kind: "Package";
  key: { name_arch: string };
}

export type ItemId = ItemIdConfig | ItemIdPackage;

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

export interface FleetAttention {
  level: string; // "high" | "medium" | "low" | "none"
  reason: string;
  zone?: string; // "Consensus" | "NearConsensus" | "Divergent"
  prevalence: number;
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
  attention: FleetAttention;
  prevalence: FleetItemPrevalence;
  variants?: FleetVariants;
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
