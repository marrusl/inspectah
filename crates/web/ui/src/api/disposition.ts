// Predicates over a finding's disposition.
//
// These mirror `FindingKind::is_included()` and the non-toggleable guard in
// inspectah-core, so the UI answers "is this in the image?" and "can the user
// change that?" the same way the session does. Reading `.include` directly is
// what produced the bug these replace: Advisory and Inventory carry no such
// key, and `?? true` read the absence as a decision the user had made.

import type { AdvisoryType, Disposition } from "./types";

/** True when this finding is baked into the exported image. */
export function isIncluded(disposition: Disposition): boolean {
  return disposition.kind === "actionable" && disposition.include;
}

/** True when the user can change the finding's include state. */
export function isToggleable(disposition: Disposition): boolean {
  return disposition.kind === "actionable";
}

const ADVISORY_TYPE_LABELS: Record<AdvisoryType, string> = {
  unbacked_var_dir: "Unbacked /var directory",
  cross_tree_symlink: "Cross-tree symlink",
  modernization: "Modernization",
};

/** Human-readable name for an advisory type. */
export function advisoryTypeLabel(advisoryType: AdvisoryType): string {
  return ADVISORY_TYPE_LABELS[advisoryType];
}
