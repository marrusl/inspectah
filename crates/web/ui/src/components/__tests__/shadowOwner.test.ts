import { describe, it, expect } from "vitest";
import { resolveShadowOwner, buildShadowOwners } from "../shadowOwner";
import type {
  ConfigFileEntry,
  DropInDecisionDto,
  TriageTag,
} from "../../api/types";

const TRIAGE: TriageTag = {
  triage: { mode: "single_host", site: null },
  primary_reason: "service_drop_in_present",
  annotations: [],
};

const DROPIN_PATH = "/etc/systemd/system/sshd.service.d/override.conf";

function configEntry(overrides: Partial<ConfigFileEntry> = {}): ConfigFileEntry {
  return {
    path: DROPIN_PATH,
    kind: "unowned",
    category: "other",
    content: "",
    rpm_va_flags: null,
    package: null,
    diff_against_rpm: null,
    disposition: { kind: "inventory" },
    tie: false,
    tie_winner: false,
    aggregate: null,
    ...overrides,
  };
}

function dropin(overrides: Partial<DropInDecisionDto> = {}): DropInDecisionDto {
  return {
    unit: "sshd.service",
    path: DROPIN_PATH,
    triage: TRIAGE,
    include: true,
    locked: false,
    ...overrides,
  };
}

describe("resolveShadowOwner", () => {
  it("returns null for an entry that owns its own decision", () => {
    expect(
      resolveShadowOwner(
        configEntry({ disposition: { kind: "actionable", include: true } }),
        [dropin()],
      ),
    ).toBeNull();
  });

  it("names the Service Drop-ins owner and routes to it by its row key", () => {
    const owner = resolveShadowOwner(
      configEntry({ shadowed_by: "units" }),
      [dropin()],
    );
    expect(owner).not.toBeNull();
    expect(owner!.label).toBe("Controlled in Service Drop-ins");
    expect(owner!.target).toEqual({
      sectionId: "units",
      itemId: `["dropin","${DROPIN_PATH}"]`,
    });
  });

  it("reports the drop-in owner's own include state", () => {
    const included = resolveShadowOwner(
      configEntry({ shadowed_by: "units" }),
      [dropin({ include: true })],
    );
    expect(included!.state).toEqual({ text: "Owner included", included: true });

    const excluded = resolveShadowOwner(
      configEntry({ shadowed_by: "units" }),
      [dropin({ include: false })],
    );
    expect(excluded!.state).toEqual({
      text: "Owner excluded",
      included: false,
    });
  });

  it("joins on the exact path, not a prefix", () => {
    const owner = resolveShadowOwner(
      configEntry({ shadowed_by: "units" }),
      [dropin({ path: `${DROPIN_PATH}.bak` })],
    );
    // Label still names the owning surface — the shadow is real — but there
    // is no row to route to or report state for.
    expect(owner!.target).toBeNull();
    expect(owner!.state).toBeNull();
  });

  it("explains where the decision went", () => {
    const services = resolveShadowOwner(
      configEntry({ shadowed_by: "units" }),
      [dropin()],
    );
    expect(services!.explanation).toContain("Service Drop-ins");
  });
});

describe("buildShadowOwners", () => {
  it("keys owners by config path and skips unshadowed entries", () => {
    const owners = buildShadowOwners(
      [
        { entry: configEntry({ shadowed_by: "units" }), triage: TRIAGE },
        { entry: configEntry({ path: "/etc/hosts" }), triage: TRIAGE },
      ],
      [dropin()],
    );
    expect([...owners.keys()]).toEqual([DROPIN_PATH]);
  });
});
