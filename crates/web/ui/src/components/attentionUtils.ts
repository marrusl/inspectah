import type {
  AttentionLevel,
  AttentionReason,
  TriageTag,
  TriageReason,
} from "../api/types";

/** Map attention level to PatternFly Label color prop. */
export function attentionLabelColor(
  level: AttentionLevel,
): "red" | "blue" | "green" {
  switch (level) {
    case "needs_review":
      return "red";
    case "informational":
      return "blue";
    case "routine":
      return "green";
  }
}

/** Format an AttentionReason for display, optionally incorporating detail. */
export function formatReasonText(
  reason: AttentionReason,
  detail?: string | null,
): string {
  if (typeof reason === "object" && "custom" in reason) {
    return reason.custom;
  }
  // Version changed: use detail to show direction when available
  if (reason === "package_version_changed" && detail) {
    const lower = detail.toLowerCase();
    if (lower === "upgrade") return "Version Upgraded";
    if (lower === "downgrade") return "Version Downgraded";
  }
  const map: Record<string, string> = {
    package_baseline_match: "Baseline",
    package_user_added: "User Added",
    package_version_changed: "Version Changed",
    package_provenance_unavailable: "Baseline Unavailable",
    package_local_install: "Local Install",
    package_no_repo_source: "No Repo Source",
    config_default: "Package Default",
    config_baseline_match: "Baseline Match",
    config_modified: "Modified",
    config_unowned: "Unowned",
    config_orphaned: "Orphaned",
    sensitive_path: "Sensitive Path",
  };
  return (
    map[reason] ??
    reason
      .split("_")
      .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
      .join(" ")
  );
}

/** Highest-priority attention level from a list of tags. */
export function highestAttention(
  tags: { level: AttentionLevel }[],
): AttentionLevel {
  if (tags.some((t) => t.level === "needs_review")) return "needs_review";
  if (tags.some((t) => t.level === "informational")) return "informational";
  return "routine";
}

/**
 * Extract the triage bucket string from a TriageTag.
 * SingleHost: the bucket name is the key with null value (serde quirk).
 * Fleet: bucket is a direct field.
 */
export function extractTriageBucket(tag: TriageTag): string {
  if (tag.triage.mode === "fleet") {
    return tag.triage.bucket;
  }
  // SingleHost: {"mode":"single_host","<bucket>":null}
  for (const key of Object.keys(tag.triage)) {
    if (key !== "mode") return key;
  }
  return "baseline";
}

/**
 * Map a triage bucket to the legacy AttentionLevel for backward compat.
 * investigate → needs_review, site/divergent/partial → informational,
 * baseline/universal → routine.
 */
export function triageBucketToAttention(tag: TriageTag): AttentionLevel {
  const bucket = extractTriageBucket(tag);
  switch (bucket) {
    case "investigate":
      return "needs_review";
    case "site":
    case "divergent":
    case "partial":
      return "informational";
    case "baseline":
    case "universal":
      return "routine";
    default:
      return "routine";
  }
}

/** Map a triage bucket to a PatternFly Label color for compact bucket badges. */
export function triageBucketLabelColor(
  bucket: string,
): "red" | "blue" | "green" | "grey" {
  switch (bucket) {
    case "investigate":
      return "red";
    case "site":
    case "divergent":
    case "partial":
      return "grey";
    case "baseline":
    case "universal":
      return "green";
    default:
      return "grey";
  }
}

/** Capitalize a triage bucket name for display. */
export function formatTriageBucket(bucket: string): string {
  return bucket.charAt(0).toUpperCase() + bucket.slice(1);
}

/** Format a TriageReason for display. */
export function formatTriageReason(reason: TriageReason): string {
  if (typeof reason === "object" && "custom" in reason) {
    return reason.custom;
  }
  const map: Record<string, string> = {
    package_baseline_match: "Baseline",
    package_user_added: "User Added",
    package_version_changed: "Version Changed",
    package_provenance_unavailable: "Baseline Unavailable",
    package_local_install: "Local Install",
    package_no_repo_source: "No Repo Source",
    package_config_captured: "Config Captured",
    config_default: "Package Default",
    config_baseline_match: "Baseline Match",
    config_modified: "Modified",
    config_unowned: "Unowned",
    config_orphaned: "Orphaned",
    service_baseline_match: "Baseline Match",
    service_non_default_state: "Non-Default State",
    service_unknown_origin: "Unknown Origin",
    service_drop_in_present: "Drop-in Override",
    quadlet_user_deployed: "User Deployed",
    quadlet_present_in_base_image: "In Base Image",
    flatpak_provisioned_on_first_boot: "First Boot",
    flatpak_incomplete_provenance: "Incomplete Provenance",
    sysctl_baseline_match: "Baseline Match",
    sysctl_file_backed_override: "Non-Default",
    sysctl_no_baseline: "No Baseline",
    tuned_baseline_match: "Baseline Match",
    tuned_non_default_profile: "Non-Default Profile",
    tuned_custom_profile: "Custom Profile",
    tuned_unusual_state: "Unusual State",
    sensitive_path: "Sensitive Path",
    package_platform_plumbing: "Platform Plumbing",
    package_installer_default: "Installer Default",
    package_installer_promoted_service: "Promoted (Service)",
    package_installer_promoted_config: "Promoted (Config)",
    package_installer_ambiguous: "Installer (Review)",
    package_installer_evidence_unavailable: "Evidence Unavailable",
  };
  return (
    map[reason] ??
    reason
      .split("_")
      .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
      .join(" ")
  );
}
