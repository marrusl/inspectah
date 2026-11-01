import { describe, it, expect } from "vitest";
import { resolveShadowOwner, buildShadowOwners } from "../shadowOwner";
import type {
  ConfigFileEntry,
  DropInDecisionDto,
  SysctlDecisionDto,
  TriageTag,
} from "../../api/types";

const TRIAGE: TriageTag = {
  triage: { mode: "single_host", site: null },
  primary_reason: "service_drop_in_present",
  annotations: [],
};

const DROPIN_PATH = "/etc/systemd/system/sshd.service.d/override.conf";
const SYSCTL_PATH = "/etc/sysctl.d/90-network.conf";

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
    ...overrides,
  };
}

function sysctl(overrides: Partial<SysctlDecisionDto> = {}): SysctlDecisionDto {
  return {
    key: "net.ipv4.ip_forward",
    runtime: "1",
    default: "0",
    source: SYSCTL_PATH,
    triage: TRIAGE,
    include: true,
    ...overrides,
  };
}

describe("resolveShadowOwner", () => {
  it("returns null for an entry that owns its own decision", () => {
    expect(
      resolveShadowOwner(
        configEntry({ disposition: { kind: "actionable", include: true } }),
        [dropin()],
        [],
      ),
    ).toBeNull();
  });

  it("names the Service Drop-ins owner and routes to it by path", () => {
    const owner = resolveShadowOwner(
      configEntry({ shadowed_by: "services" }),
      [dropin()],
      [],
    );
    expect(owner).not.toBeNull();
    expect(owner!.label).toBe("Controlled in Service Drop-ins");
    expect(owner!.target).toEqual({
      sectionId: "services",
      itemId: DROPIN_PATH,
    });
  });

  it("reports the drop-in owner's own include state", () => {
    const included = resolveShadowOwner(
      configEntry({ shadowed_by: "services" }),
      [dropin({ include: true })],
      [],
    );
    expect(included!.state).toEqual({ text: "Owner included", included: true });

    const excluded = resolveShadowOwner(
      configEntry({ shadowed_by: "services" }),
      [dropin({ include: false })],
      [],
    );
    expect(excluded!.state).toEqual({
      text: "Owner excluded",
      included: false,
    });
  });

  it("joins on the exact path, not a prefix", () => {
    const owner = resolveShadowOwner(
      configEntry({ shadowed_by: "services" }),
      [dropin({ path: `${DROPIN_PATH}.bak` })],
      [],
    );
    // Label still names the owning surface — the shadow is real — but there
    // is no row to route to or report state for.
    expect(owner!.target).toBeNull();
    expect(owner!.state).toBeNull();
  });

  it("names the sysctl owner and counts how many of its keys are included", () => {
    const owner = resolveShadowOwner(
      configEntry({ path: SYSCTL_PATH, shadowed_by: "sysctl" }),
      [],
      [
        sysctl({ key: "net.ipv4.ip_forward", include: true }),
        sysctl({ key: "net.ipv4.tcp_syncookies", include: false }),
        sysctl({ key: "vm.swappiness", source: "/etc/sysctl.conf" }),
      ],
    );
    expect(owner!.label).toBe("Merged into sysctl settings");
    expect(owner!.state).toEqual({
      text: "1 of 2 keys included",
      included: true,
    });
    expect(owner!.target).toEqual({
      sectionId: "kernel_boot",
      itemId: "net.ipv4.ip_forward",
    });
  });

  it("marks a sysctl owner excluded when none of its keys are included", () => {
    const owner = resolveShadowOwner(
      configEntry({ path: SYSCTL_PATH, shadowed_by: "sysctl" }),
      [],
      [sysctl({ include: false })],
    );
    expect(owner!.state).toEqual({
      text: "0 of 1 keys included",
      included: false,
    });
  });

  it("names the sysctl owner even when no override row survived projection", () => {
    const owner = resolveShadowOwner(
      configEntry({ path: SYSCTL_PATH, shadowed_by: "sysctl" }),
      [],
      [],
    );
    expect(owner!.label).toBe("Merged into sysctl settings");
    expect(owner!.state).toBeNull();
    expect(owner!.target).toBeNull();
  });

  it("explains where the decision went for both owners", () => {
    const services = resolveShadowOwner(
      configEntry({ shadowed_by: "services" }),
      [dropin()],
      [],
    );
    expect(services!.explanation).toContain("Service Drop-ins");

    const keys = resolveShadowOwner(
      configEntry({ path: SYSCTL_PATH, shadowed_by: "sysctl" }),
      [],
      [sysctl()],
    );
    expect(keys!.explanation).toContain("99-inspectah-migrated.conf");
  });
});

describe("buildShadowOwners", () => {
  it("keys owners by config path and skips unshadowed entries", () => {
    const owners = buildShadowOwners(
      [
        { entry: configEntry({ shadowed_by: "services" }), triage: TRIAGE },
        { entry: configEntry({ path: "/etc/hosts" }), triage: TRIAGE },
      ],
      [dropin()],
      [],
    );
    expect([...owners.keys()]).toEqual([DROPIN_PATH]);
  });
});
