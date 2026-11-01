import type {
  ConfigFileEntry,
  DropInDecisionDto,
  RefinedConfig,
  SysctlDecisionDto,
} from "../api/types";

/** Section ID the Service Drop-ins rows render under. */
const SERVICES_SECTION_ID = "services";
/** Section ID the sysctl override rows render under. */
const KERNEL_BOOT_SECTION_ID = "kernel_boot";
/** Mirrors SYNTHESIZED_SYSCTL_FILE in inspectah-core. */
const SYNTHESIZED_SYSCTL_FILE = "/etc/sysctl.d/99-inspectah-migrated.conf";

/**
 * Where a shadowed config file's include decision actually lives.
 *
 * A config entry whose path is also carried by another section is
 * `Inventory`: the session refuses `SetInclude` on it with 422. Without
 * this, the row shows a bare "Inventory" badge and the operator has no
 * way to tell which control replaced the one that vanished.
 */
export interface ShadowOwner {
  /** Names the owning surface, e.g. "Controlled in Service Drop-ins". */
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
}

/**
 * Resolve the owner of a shadowed config entry.
 *
 * The join key is the path string itself: `service_dropins[].path` and
 * `sysctls[].source` are the same strings the shadow was computed from
 * in `shadow_matching_config_entries`, so no extra field is needed.
 * Returns null when the entry is not shadowed.
 */
export function resolveShadowOwner(
  entry: ConfigFileEntry,
  dropins: DropInDecisionDto[],
  sysctls: SysctlDecisionDto[],
): ShadowOwner | null {
  if (entry.shadowed_by === "services") {
    const owner = dropins.find((d) => d.path === entry.path);
    return {
      label: "Controlled in Service Drop-ins",
      state: owner
        ? {
            text: owner.include ? "Owner included" : "Owner excluded",
            included: owner.include,
          }
        : null,
      explanation:
        "The Service Drop-ins row for this same path carries the file into " +
        "the image. Decide it there.",
      target: owner
        ? { sectionId: SERVICES_SECTION_ID, itemId: owner.path }
        : null,
    };
  }

  if (entry.shadowed_by === "sysctl") {
    const keys = sysctls.filter((s) => s.source === entry.path);
    const includedCount = keys.filter((s) => s.include).length;
    return {
      label: "Merged into sysctl settings",
      state:
        keys.length > 0
          ? {
              text: `${includedCount} of ${keys.length} keys included`,
              included: includedCount > 0,
            }
          : null,
      explanation:
        `This file's keys are merged into ${SYNTHESIZED_SYSCTL_FILE}. ` +
        "Decide them key by key under Kernel & Boot.",
      target:
        keys.length > 0
          ? { sectionId: KERNEL_BOOT_SECTION_ID, itemId: keys[0].key }
          : null,
    };
  }

  return null;
}

/** Resolve every shadowed config in a view, keyed by config path. */
export function buildShadowOwners(
  configs: RefinedConfig[],
  dropins: DropInDecisionDto[],
  sysctls: SysctlDecisionDto[],
): Map<string, ShadowOwner> {
  const owners = new Map<string, ShadowOwner>();
  for (const cfg of configs) {
    const owner = resolveShadowOwner(cfg.entry, dropins, sysctls);
    if (owner) owners.set(cfg.entry.path, owner);
  }
  return owners;
}
