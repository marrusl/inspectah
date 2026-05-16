import type { AttentionLevel, AttentionReason } from "../api/types";

/** Map attention level to PatternFly Label color prop. */
export function attentionLabelColor(
  level: AttentionLevel,
): "red" | "orange" | "green" {
  switch (level) {
    case "needs_review":
      return "red";
    case "informational":
      return "orange";
    case "routine":
      return "green";
  }
}

/** Format an AttentionReason for display. */
export function formatReasonText(reason: AttentionReason): string {
  if (typeof reason === "object" && "custom" in reason) {
    return reason.custom;
  }
  return reason
    .split("_")
    .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
    .join(" ");
}

/** Highest-priority attention level from a list of tags. */
export function highestAttention(
  tags: { level: AttentionLevel }[],
): AttentionLevel {
  if (tags.some((t) => t.level === "needs_review")) return "needs_review";
  if (tags.some((t) => t.level === "informational")) return "informational";
  return "routine";
}
