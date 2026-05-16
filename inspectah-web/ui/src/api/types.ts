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
  | "config_modified"
  | "config_unowned"
  | "config_orphaned"
  | "sensitive_path"
  | "package_not_in_baseline"
  | "package_local_install"
  | "package_state_changed"
  | "package_no_repo"
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

export interface RefineStats {
  total_packages: number;
  included_packages: number;
  excluded_packages: number;
  total_configs: number;
  included_configs: number;
  excluded_configs: number;
  needs_review_count: number;
  ops_applied: number;
  can_undo: boolean;
  can_redo: boolean;
}

export interface RefinedView {
  packages: RefinedPackage[];
  config_files: RefinedConfig[];
  containerfile_preview: string;
  stats: RefineStats;
  generation: number;
}

/**
 * Rust: #[serde(tag = "op", content = "target")]
 * JSON: {"op": "ExcludePackage", "target": {"name": "httpd", "arch": "x86_64"}}
 */
export type RefinementOp =
  | { op: "ExcludePackage"; target: PackageTarget }
  | { op: "IncludePackage"; target: PackageTarget }
  | { op: "ExcludeConfig"; target: { path: string } }
  | { op: "IncludeConfig"; target: { path: string } };

export interface ChangesSummary {
  packages_included: PackageTarget[];
  packages_excluded: PackageTarget[];
  configs_included: string[];
  configs_excluded: string[];
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

export interface ContextSection {
  id: string;
  display_name: string;
  items: ContextItem[];
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
