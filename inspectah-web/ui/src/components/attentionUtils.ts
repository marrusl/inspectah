import type { AttentionLevel, AttentionReason } from "../api/types";

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

/** Format an AttentionReason for display. */
export function formatReasonText(reason: AttentionReason): string {
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
    config_default: "Package Default",
    config_baseline_match: "Baseline Match",
    config_modified: "Modified",
    config_unowned: "Unowned",
    config_orphaned: "Orphaned",
    sensitive_path: "Sensitive Path",
  };
  return map[reason] ?? reason.split("_").map((w) => w.charAt(0).toUpperCase() + w.slice(1)).join(" ");
}

/** Highest-priority attention level from a list of tags. */
export function highestAttention(
  tags: { level: AttentionLevel }[],
): AttentionLevel {
  if (tags.some((t) => t.level === "needs_review")) return "needs_review";
  if (tags.some((t) => t.level === "informational")) return "informational";
  return "routine";
}
