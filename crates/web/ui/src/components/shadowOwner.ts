import type {
  ConfigFileEntry,
  DropInDecisionDto,
  RefinedConfig,
  RepoOwnedEntry,
} from "../api/types";
import { rowKey } from "./unitOutcome";
import {
  owningSwitch,
  ownedShips,
  repoItemId,
  repoOwnedExplanation,
} from "./repoControl";

/** Section ID the Service Drop-ins rows render under. */
const SERVICES_SECTION_ID = "units";

/** Section ID the Repositories rows render under. */
const PACKAGES_SECTION_ID = "packages";

/** The label of a row the repo section decides (spec § 7.3); the server's
 *  `DECIDED_UNDER_REPOSITORIES` summary uses the same words. */
export const DECIDED_UNDER_REPOSITORIES = "Decided under Repositories";

/**
 * Where a shadowed config file's include decision actually lives.
 *
 * A config entry whose path is also carried by another section is
 * `Inventory`: the session refuses `SetInclude` on it with 422. Without
 * this, the row shows a bare "Inventory" badge and the operator has no
 * way to tell which control replaced the one that vanished.
 */
export interface ShadowOwner {
  /** Names the owning surface, e.g. "Controlled in Systemd units". */
  label: string;
  /**
   * The owning row's current state. Null when no owning row is present
   * in this view — the shadow is still real, but there is nothing to
   * report state for.
   */
  state: { text: string; included: boolean } | null;
  /** One sentence telling the operator where the decision went. */
  explanation: string;
  /** Route to the owning row. Null when no owning row was found. */
  target: { sectionId: string; itemId: string } | null;
  /** The link's text; "Go to owner" when absent. */
  linkText?: string;
}

/**
 * Resolve the owner of a shadowed config entry.
 *
 * Each join is on a list of path spellings the server gives, so the client
 * never normalizes a path: a `"units"` row joins the drop-in whose
 * `config_paths` holds its `path`, and a `"repositories"` row the
 * `repo_owned` entry whose `recorded_paths` does (an ostree scan's
 * relative spelling and a `gpgkey=` written with `//` included).
 * Returns null when the entry is not shadowed.
 */
export function resolveShadowOwner(
  entry: ConfigFileEntry,
  dropins: DropInDecisionDto[],
  repoOwned: RepoOwnedEntry[] = [],
): ShadowOwner | null {
  if (entry.shadowed_by === "units") {
    const owner = dropins.find((d) => d.config_paths.includes(entry.path));
    return {
      // Spec § 7 string 26: the units section's name (§ 6.14).
      label: "Controlled in Systemd units",
      state: owner
        ? {
            text: owner.include ? "Owner included" : "Owner excluded",
            included: owner.include,
          }
        : null,
      explanation:
        "The Systemd units row for this same path carries the file into " +
        "the image. Decide it there.",
      // The drop-in's row key (spec § 6.11), not its bare path: a bare
      // id names a unit row.
      target: owner
        ? {
            sectionId: SERVICES_SECTION_ID,
            itemId: rowKey({ kind: "dropin", data: owner }),
          }
        : null,
    };
  }

  if (entry.shadowed_by === "repositories") {
    const owned = repoOwned.find((e) => e.recorded_paths.includes(entry.path));
    if (!owned) {
      // The server lists every destination it shadows, so this is a view
      // that disagrees with itself; say where the decision is anyway.
      return {
        label: DECIDED_UNDER_REPOSITORIES,
        state: null,
        explanation:
          "A repo's switch under Repositories, in Packages, decides whether this file ships.",
        target: null,
      };
    }
    const ships = ownedShips(owned);
    const switchOwner = owningSwitch(owned);
    return {
      label: DECIDED_UNDER_REPOSITORIES,
      state: { text: ships ? "Ships" : "Not shipped", included: ships },
      explanation: repoOwnedExplanation(owned),
      target: switchOwner
        ? { sectionId: PACKAGES_SECTION_ID, itemId: repoItemId(switchOwner) }
        : null,
      linkText: switchOwner ? `Go to repo ${switchOwner}` : undefined,
    };
  }

  return null;
}

/** Resolve every shadowed config in a view, keyed by config path. */
export function buildShadowOwners(
  configs: RefinedConfig[],
  dropins: DropInDecisionDto[],
  repoOwned: RepoOwnedEntry[] = [],
): Map<string, ShadowOwner> {
  const owners = new Map<string, ShadowOwner>();
  for (const cfg of configs) {
    const owner = resolveShadowOwner(cfg.entry, dropins, repoOwned);
    if (owner) owners.set(cfg.entry.path, owner);
  }
  return owners;
}
