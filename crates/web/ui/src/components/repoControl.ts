import { ApiError } from "../api/types";
import type {
  KeyRejection,
  RepoGroupInfo,
  RepoKeeper,
  RepoOwnedEntry,
} from "../api/types";

/**
 * The repo row's switch, its texts, and the words the Repositories section
 * and the Configuration rows use for the files and keys a repo owns
 * (process-docs/specs/proposed/2026-11-21-repo-section-owns-repo-files-and-keys.md,
 * § 5.5, § 7.2 and § 7.3). `RepoBar` and `RepoGroupHeader` both read these,
 * so the two rows cannot disagree.
 */

/** The reveal id of a repo row: `repo:<section_id>`. */
export const REPO_ITEM_PREFIX = "repo:";

/** The test id prefix of a repo row in the Repositories section. */
export const REPO_ROW_TESTID_PREFIX = "repo-bar-row-";

export function repoItemId(sectionId: string): string {
  return `${REPO_ITEM_PREFIX}${sectionId}`;
}

/** A repo row shows a switch exactly when the server gives it a control. */
export function hasRepoSwitch(repo: RepoGroupInfo): boolean {
  return repo.control !== "none";
}

/**
 * Whether the switch reads on (§ 5.5). The server never refuses the
 * direction it posts: checked posts exclude, which moves the decision;
 * unchecked posts include, which moves the decision, an excluded file or
 * key, or, for a full control, an excluded member. Both count terms are
 * zero-safe, so a repo with no members reads by its files alone.
 */
export function repoSwitchChecked(repo: RepoGroupInfo): boolean {
  const membersAll = repo.effective_included === repo.effective_member_count;
  const carriageAll = repo.carriage_included === repo.carriage_count;
  return (
    repo.kept && carriageAll && (repo.control === "carriage_only" || membersAll)
  );
}

/** "<N> of <M> packages included", or "<M> packages excluded", with the
 *  unlisted members after it. */
function memberCountText(repo: RepoGroupInfo): string {
  const { effective_included: included, effective_member_count: total } =
    repo;
  const base =
    included === 0
      ? `${total} packages excluded`
      : `${included} of ${total} packages included`;
  return repo.effective_unlisted > 0
    ? `${base}, ${repo.effective_unlisted} not listed`
    : base;
}

function carriageText(repo: RepoGroupInfo): string {
  return `repo files ${repo.carriage_included} of ${repo.carriage_count} carried`;
}

/**
 * The count text a switch row shows as its label (§ 7.2): members for a
 * full control that has some, with the files after them when some are
 * left out; the files alone with no members or for a carriage-only
 * control; and `excluded in refine` once the operator excluded the repo.
 */
export function repoCountText(repo: RepoGroupInfo): string {
  const parts: string[] = [];
  if (repo.control === "full" && repo.effective_member_count > 0) {
    parts.push(memberCountText(repo));
    if (
      repo.carriage_count > 0 &&
      repo.carriage_included < repo.carriage_count
    ) {
      parts.push(carriageText(repo));
    }
  } else {
    parts.push(carriageText(repo));
  }
  if (!repo.kept) parts.push("excluded in refine");
  return parts.join(", ");
}

/** What a row with no switch says instead (§ 7.2). */
export function repoNoControlText(repo: RepoGroupInfo): string {
  if (repo.is_distro) return "always included";
  switch (repo.provenance) {
    case "disabled":
      return "disabled on host";
    case "stale":
      return "repo id not found in any .repo file";
    case "unknown":
      return "installed outside any repo";
    default:
      return "nothing to decide: no package to change and no file or key the export can carry";
  }
}

/** The text a carriage-only row shows beside its switch, so the operator
 *  knows the switch leaves its packages alone. */
export const CARRIAGE_ONLY_NOTE = "switch moves files and keys only";

/**
 * The badge a carriage-only row keeps (§ 7.2): a distro repo's packages
 * are always included and a host-disabled repo's follow their own route,
 * whatever its switch says.
 */
export function carriageOnlyBadge(repo: RepoGroupInfo): string | null {
  if (repo.control !== "carriage_only") return null;
  if (repo.is_distro) return "always included";
  if (repo.provenance === "disabled") return "disabled on host";
  return null;
}

/** The last segment of a path. */
export function basename(path: string): string {
  const trimmed = path.replace(/\/+$/, "");
  const slash = trimmed.lastIndexOf("/");
  return slash === -1 ? trimmed : trimmed.slice(slash + 1);
}

/** What the scan's refusal of a key says, completing "because it ...". */
const REJECTION_BECAUSE: Record<KeyRejection, string> = {
  not_pgp: "is not a PGP public key",
  private_key_material: "holds private key material",
};

function keeperIds(keepers: RepoKeeper[]): string {
  return keepers.map((k) => k.section_id).join(", ");
}

function withoutFinalPeriod(text: string): string {
  return text.trim().replace(/\.$/, "");
}

/**
 * The state word the owns line gives an entry (§ 7.2): nothing when it
 * simply ships. A refused key and a withheld file say why, since nothing
 * else in the Repositories section does.
 */
export function ownedStateWord(entry: RepoOwnedEntry): string {
  const e = entry.export;
  switch (e.state) {
    case "ships":
      return e.kept_by.length > 0 ? `kept by ${keeperIds(e.kept_by)}` : "";
    case "not_shipped":
      return "not shipped";
    case "base_image":
      return "base image";
    case "default_repo_file":
      return "default repo file";
    case "rejected":
      return `rejected: ${REJECTION_BECAUSE[e.reason]}`;
    case "withheld":
      return `withheld: ${withoutFinalPeriod(e.reason)}`;
    case "no_owner":
      return "no owner";
  }
}

/** Whether the export copies the entry. */
export function ownedShips(entry: RepoOwnedEntry): boolean {
  return entry.export.state === "ships";
}

/** "repo a" or "repo a, b". */
function repoNames(ids: string[]): string {
  return `repo ${ids.join(", ")}`;
}

/**
 * Why the export does or does not copy a repo-owned file or key (§ 7.3),
 * the sentence its Configuration row explains itself with and the
 * Repositories section's list of entries no switch decides.
 */
export function repoOwnedExplanation(entry: RepoOwnedEntry): string {
  const owners = entry.owners.map((o) => o.section_id);
  const e = entry.export;
  switch (e.state) {
    case "ships": {
      const base =
        `This file belongs to ${repoNames(owners)}. Its switch under ` +
        "Repositories, in Packages, decides whether the file ships.";
      if (e.kept_by.length === 0) return base;
      const excluded = entry.owners
        .filter((o) => !o.kept)
        .map((o) => o.section_id);
      const keepers = e.kept_by
        .map((k) =>
          k.enablerepo_package
            ? `${k.section_id}, whose package ${k.enablerepo_package} installs with --enablerepo`
            : k.section_id,
        )
        .join(" and ");
      return (
        `${base} Repo ${excluded.join(", ")} is excluded, but repo ` +
        `${keepers} still needs this file, so it ships.`
      );
    }
    case "not_shipped":
      return (
        `This file belongs to ${repoNames(owners)}, and the export leaves ` +
        "it out. Its switch under Repositories decides whether it ships."
      );
    case "base_image":
      return `The base image ships this key (${e.package}), so the export never copies it.`;
    case "default_repo_file":
      return (
        "This is a default repo file, which the export never copies. The " +
        "build is expected to get it from the base image or the " +
        "subscription mount."
      );
    case "rejected":
      return (
        "The scan did not store this file because it " +
        `${REJECTION_BECAUSE[e.reason]}, so the export never copies it.`
      );
    case "withheld":
      return `The export withholds this file: ${withoutFinalPeriod(e.reason)}.`;
    case "no_owner": {
      const cause = e.cause;
      const redacted =
        cause.kind === "redacted" ? cause.repo_files : cause.redacted_repo_files;
      if (entry.kind === "repo_file") {
        const ownPaths = new Set([entry.destination, ...entry.recorded_paths]);
        const changed = redacted.some((p) => ownPaths.has(p))
          ? "Redaction changed the [section] headers in this file"
          : "Its [section] headers were changed after the scan";
        return (
          `${changed}, so it defines no repo and the export never copies ` +
          "it. Review the redacted headers."
        );
      }
      if (cause.kind === "fleet") {
        return (
          "Every host that has this key had it because a repo file named " +
          "it, but no version of a repo file that the export copies names " +
          `it, so the export never copies it. Hosts with it: ${cause.hosts.join(", ")}.`
        );
      }
      const changed =
        cause.repo_files.length > 0
          ? `redaction changed that gpgkey= reference (${cause.repo_files.join(", ")})`
          : "that gpgkey= reference was changed after the scan";
      return (
        `The scan found this key because a repo file named it, but ${changed}, ` +
        "so no repo file the export copies names it and the export never " +
        "copies it. Review the redacted reference."
      );
    }
  }
}

/**
 * The first owner, by section id, that has a switch: the repo row a
 * Configuration row links to. Null when no owner has one.
 */
export function owningSwitch(entry: RepoOwnedEntry): string | null {
  const owner = [...entry.owners]
    .sort((a, b) => (a.section_id < b.section_id ? -1 : a.section_id > b.section_id ? 1 : 0))
    .find((o) => o.control !== "none");
  return owner ? owner.section_id : null;
}

/** The prefix of a 422 for a repo op that would change nothing. */
const NOTHING_TO_CHANGE_PREFIX = "nothing to change:";

/** The prefix of a 422 for an op on a row another section decides. */
export const INVENTORY_REFUSAL_PREFIX = "inventory finding is not toggleable:";

/** What a repo row shows when its request failed. */
export interface RepoFailure {
  /**
   * `nothing_to_change`: the server said the op moves nothing (422
   * `nothing to change:`), so the view was stale; the owner refetches it.
   * `failed`: anything else; the row keeps the server's state.
   */
  kind: "nothing_to_change" | "failed";
  text: string;
}

/** The server's reason after a 422 `prefix`, without its final period. */
function refusalReason(err: ApiError, prefix: string): string {
  return withoutFinalPeriod(err.message.slice(prefix.length));
}

export function repoFailure(err: unknown): RepoFailure {
  if (
    err instanceof ApiError &&
    err.status === 422 &&
    err.message.startsWith(NOTHING_TO_CHANGE_PREFIX)
  ) {
    return {
      kind: "nothing_to_change",
      text: `Nothing to change: ${refusalReason(err, NOTHING_TO_CHANGE_PREFIX)}.`,
    };
  }
  const message = err instanceof Error ? err.message : String(err);
  return { kind: "failed", text: `The change was not saved: ${message}` };
}

/**
 * The statement a 422 `inventory finding is not toggleable` becomes, or
 * null for any other error. The UI never offers that op; if one arrives
 * anyway it is information about the row, not a fault.
 */
export function inventoryRefusalText(err: unknown): string | null {
  if (
    err instanceof ApiError &&
    err.status === 422 &&
    err.message.startsWith(INVENTORY_REFUSAL_PREFIX)
  ) {
    const path = refusalReason(err, INVENTORY_REFUSAL_PREFIX);
    return `Nothing was changed: ${path} is decided by the section that owns it, not by this row.`;
  }
  return null;
}
