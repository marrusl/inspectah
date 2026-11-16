/**
 * Outcomes, counts, groups and labels of the units section, on the model
 * (§ 6.1 to § 6.4, § 6.6 items 2, 11, 12, § 6.8, § 7). U1, U2, and U3,
 * U4, U5, U20, U21 before anything renders them.
 */
import { describe, it, expect } from "vitest";
import type { DeclinedView, LinkRelation, ManualLinkCause, ServiceDecisionDto } from "../../api/types";
import {
  GROUP_ORDER,
  GROUP_TITLES,
  OUTCOME_LABELS,
  RELATION_LABELS,
  UNDETERMINED_CAUSE_LABEL,
  causeLabel,
  followUpFlagText,
  groupUnits,
  keyFields,
  outcomeCounts,
  outcomeOf,
  type GroupId,
  type Outcome,
  type UnitGroupModel,
  type UnitRecord,
} from "../unitOutcome";
import { loadUnitFields } from "../../__tests__/fixtures/systemd-units/load";

const kitchenSink = loadUnitFields("kitchen-sink");
const planted = loadUnitFields("planted");

const DECLINED: DeclinedView = { renders: "fixme", reason: "Test", text: "a reason" };

function unitRow(over: Partial<ServiceDecisionDto>): ServiceDecisionDto {
  return { ...kitchenSink.unit_states.find((r) => r.unit === "httpd.service")!, ...over };
}

function group(groups: UnitGroupModel[], id: GroupId): UnitGroupModel {
  const found = groups.find((g) => g.id === id);
  expect(found).toBeDefined();
  return found!;
}

const heads = (g: UnitGroupModel) => g.branches.map((b) => b.head.record.data.unit);

describe("U1: a row's outcome (§ 6.1)", () => {
  const target = kitchenSink.default_target!;
  it.each<[string, UnitRecord, Outcome | null]>([
    ["declined, excluded unit row", { kind: "unit_state", data: unitRow({ declined: DECLINED, include: false }) }, "needs_follow_up"],
    ["declined, included unit row", { kind: "unit_state", data: unitRow({ declined: DECLINED }) }, "needs_follow_up"],
    ["excluded unit row", { kind: "unit_state", data: unitRow({ include: false }) }, "excluded"],
    ["carried unit row", { kind: "unit_state", data: unitRow({}) }, "carried"],
    ["locked, excluded unit row", { kind: "unit_state", data: unitRow({ include: false, locked: true }) }, "excluded"],
    ["declined, excluded target", { kind: "default_target", data: { ...target, declined: DECLINED, include: false } }, "needs_follow_up"],
    ["excluded target", { kind: "default_target", data: { ...target, include: false } }, "excluded"],
    ["carried target", { kind: "default_target", data: target }, "carried"],
    ["mount finding", { kind: "mount_finding", data: planted.mount_findings[0] }, "needs_follow_up"],
    ["manual link", { kind: "manual_link", data: planted.manual_links[0] }, "needs_follow_up"],
    ["undetermined install", { kind: "undetermined_install", data: planted.undetermined_installs[0] }, "needs_follow_up"],
    ["drop-in", { kind: "dropin", data: planted.unit_dropins[0] }, null],
  ])("%s", (_name, record, expected) => {
    expect(outcomeOf(record)).toBe(expected);
  });

  it("files the planted locked row as excluded", () => {
    const locked = planted.unit_states.find((r) => r.locked)!;
    expect(outcomeOf({ kind: "unit_state", data: locked })).toBe("excluded");
  });
});

describe("U2: outcome counts (§ 6.4)", () => {
  it("counts the kitchen-sink recording", () => {
    expect(outcomeCounts(keyFields(kitchenSink))).toEqual({
      all: 23,
      needs_follow_up: 9,
      carried: 14,
      excluded: 0,
    });
  });

  it("does not count drop-ins", () => {
    const more = {
      ...kitchenSink,
      unit_dropins: [...kitchenSink.unit_dropins, ...planted.unit_dropins],
    };
    expect(outcomeCounts(keyFields(more))).toEqual(outcomeCounts(keyFields(kitchenSink)));
  });
});

describe("U3: groups and their order (§ 6.2)", () => {
  it("gives kitchen-sink's groups in order, with their rows", () => {
    const groups = groupUnits(keyFields(kitchenSink));
    expect(groups.map((g) => g.id)).toEqual([
      "default_target",
      "services",
      "sockets",
      "timers",
      "other_units",
      "reported",
    ]);
    expect(group(groups, "default_target").target?.record.data.target).toBe("graphical.target");
    expect(group(groups, "services").branches).toHaveLength(11);
    expect(group(groups, "sockets").branches).toHaveLength(6);
    expect(group(groups, "timers").branches).toHaveLength(1);
    expect(heads(group(groups, "other_units"))).toEqual(["tmp.mount"]);
    const reported = group(groups, "reported").reported.flatMap((b) => b.rows);
    expect(reported).toHaveLength(3);
  });

  it("draws Paths for planted, and puts the slice in Other units", () => {
    const groups = groupUnits(keyFields(planted));
    expect(heads(group(groups, "paths"))).toEqual(["watch-demo.path"]);
    expect(heads(group(groups, "other_units"))).toContain("limits-demo.slice");
  });

  it("orders the groups as GROUP_ORDER and titles them with strings 2 to 5", () => {
    expect(GROUP_ORDER).toEqual([
      "default_target",
      "services",
      "sockets",
      "timers",
      "paths",
      "other_units",
      "reported",
    ]);
    expect(GROUP_TITLES).toEqual({
      default_target: "Default target",
      services: "Services",
      sockets: "Sockets",
      timers: "Timers",
      paths: "Paths",
      other_units: "Other units",
      reported: "Reported, not carried",
    });
  });

  it.each(["swap", "automount", "widget"])("puts a unit of kind %s in Other units", (kind) => {
    const fields = { ...kitchenSink, unit_states: [unitRow({ unit: `odd-demo.${kind}`, kind })] };
    const groups = groupUnits(keyFields(fields));
    expect(heads(group(groups, "other_units"))).toEqual([`odd-demo.${kind}`]);
  });
});

describe("U4: sort and nesting (§ 6.2)", () => {
  const groups = groupUnits(keyFields(planted));

  it("sorts branch heads by unit in code-point order", () => {
    for (const g of groups) {
      const names = heads(g);
      const sorted = [...names].sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
      expect(names).toEqual(sorted);
    }
  });

  it.each<[string, string, string[]]>([
    ["alpha.service", "Zeta.service", ["Zeta.service", "alpha.service"]],
  ])("sorts %s and %s by code point, not by locale", (first, second, expected) => {
    const fields = {
      ...kitchenSink,
      unit_states: [unitRow({ unit: first, kind: "service" }), unitRow({ unit: second, kind: "service" })],
    };
    expect(heads(group(groupUnits(keyFields(fields)), "services"))).toEqual(expected);
  });

  it("sorts a reversed pair by name", () => {
    const fields = {
      ...kitchenSink,
      unit_states: [
        unitRow({ unit: "a.b.service", kind: "service" }),
        unitRow({ unit: "a-b.service", kind: "service" }),
      ],
    };
    expect(heads(group(groupUnits(keyFields(fields)), "services"))).toEqual(["a-b.service", "a.b.service"]);
  });

  it("keeps a template and its instance as sibling branches", () => {
    const services = heads(group(groups, "services"));
    const template = services.indexOf("getty@.service");
    expect(template).toBeGreaterThanOrEqual(0);
    expect(services[template + 1]).toBe("getty@tty2.service");
    expect(services).toContain("wg-quick@wg0.service");
  });

  it("nests both duplicated drop-ins under the first of two duplicated heads", () => {
    const branches = group(groups, "services").branches;
    const at = branches.findIndex((b) => b.head.record.data.unit === "dup-state-demo.service");
    const [first, second] = [branches[at], branches[at + 1]];
    expect(second.head.record.data.unit).toBe("dup-state-demo.service");
    expect(first.head.occurrenceKey.endsWith("#0")).toBe(true);
    expect(second.head.occurrenceKey.endsWith("#1")).toBe(true);
    expect(first.dropins.map((d) => d.occurrenceKey)).toEqual([
      '["dropin","/etc/systemd/system/dup-state-demo.service.d/override.conf"]#0',
      '["dropin","/etc/systemd/system/dup-state-demo.service.d/override.conf"]#1',
    ]);
    expect(second.dropins).toEqual([]);
  });

  it("keeps drop-ins in server order under their parent", () => {
    const httpd = group(groupUnits(keyFields(kitchenSink)), "services").branches.find(
      (b) => b.head.record.data.unit === "httpd.service",
    )!;
    expect(httpd.dropins.map((d) => d.record.data.path)).toEqual([
      "/etc/systemd/system/httpd.service.d/override.conf",
      "/etc/systemd/system/httpd.service.d/limits.conf",
    ]);
  });
});

describe("U5: the follow-up flag (§ 6.3 item 4)", () => {
  it("counts every follow-up unit row of a kind group", () => {
    const groups = groupUnits(keyFields(kitchenSink));
    const totals = Object.fromEntries(groups.map((g) => [g.id, g.followUpTotal]));
    expect(totals).toEqual({
      default_target: 0,
      services: 5,
      sockets: 0,
      timers: 0,
      other_units: 1,
      reported: 0,
    });
  });

  it("gives no total to a declined default target", () => {
    expect(group(groupUnits(keyFields(planted)), "default_target").followUpTotal).toBe(0);
  });

  it.each<[number, string | null]>([
    [1, "1 needs follow-up"],
    [5, "5 need follow-up"],
    [0, null],
  ])("followUpFlagText(%i)", (n, expected) => {
    expect(followUpFlagText(n)).toBe(expected);
  });
});

describe("U20: reported rows and their labels (§ 6.6 items 2, 11, 12)", () => {
  it("sorts reported blocks by unit, links before the undetermined install", () => {
    const reported = group(groupUnits(keyFields(kitchenSink)), "reported").reported;
    expect(reported.map((b) => b.unit)).toEqual(["driftify-static.service", "driftify-upheld.service"]);
    expect(reported[1].rows.map((r) => r.record.kind)).toEqual(["manual_link", "undetermined_install"]);
  });

  it("orders the shared name's block mount finding, links, undetermined install", () => {
    const reported = group(groupUnits(keyFields(planted)), "reported").reported;
    const units = reported.map((b) => b.unit);
    expect(units).toEqual([...units].sort((a, b) => (a < b ? -1 : a > b ? 1 : 0)));
    const shared = reported.find((b) => b.unit === "data-demo.mount")!;
    expect(shared.rows.map((r) => r.record.kind)).toEqual([
      "mount_finding",
      "manual_link",
      "undetermined_install",
    ]);
  });

  it("keeps a unit's links in server order", () => {
    const reported = group(groupUnits(keyFields(planted)), "reported").reported;
    const multi = reported.find((b) => b.unit === "multi-link-demo.service")!;
    expect(
      multi.rows.map((r) => (r.record.kind === "manual_link" ? r.record.data.parent : "")),
    ).toEqual(["multi-user.target", "graphical.target", "multi-user.target"]);
  });

  it("names every relation with string 17", () => {
    const expected: Record<LinkRelation, string> = {
      wants: "wanted by",
      requires: "required by",
      upholds: "upheld by",
      alias: "has alias",
      also: "named in Also= by",
    };
    expect(RELATION_LABELS).toEqual(expected);
  });

  it.each<[ManualLinkCause, string]>([
    ["static_unit", "Link its [Install] does not make"],
    ["not_in_install", "Link made by hand"],
    ["partial_install", "Only some of its links"],
    ["host_device", "Tied to a host device"],
    ["host_install_only", "Only the /etc files name it"],
    ["vendor_install_only", "Only the package unit names it"],
    [{ undetermined: { reason: "upheld_by" } }, "[Install] cannot be relied on"],
    [{ undetermined: { reason: "specifier" } }, "[Install] cannot be relied on"],
  ])("names the cause %j with string 18", (cause, expected) => {
    expect(causeLabel(cause)).toBe(expected);
  });

  it("labels every undetermined-install row with string 18's undetermined label", () => {
    expect(UNDETERMINED_CAUSE_LABEL).toBe("[Install] cannot be relied on");
  });

  it("names every outcome label with string 15", () => {
    expect(OUTCOME_LABELS).toEqual({ fixme: "FIXME", advisory: "Advisory", omitted: "Omitted" });
  });
});

describe("U21: orphan drop-ins (§ 6.8)", () => {
  it("puts sshd.service's full shadow in Services as an orphan block", () => {
    const services = group(groupUnits(keyFields(kitchenSink)), "services");
    expect(services.orphans.map((o) => o.unit)).toEqual(["sshd.service"]);
    expect(services.orphans[0].dropins.map((d) => d.record.data.shadow_type)).toEqual(["full_shadow"]);
  });

  it("puts the planted drop-in with no kind in Other units", () => {
    const other = group(groupUnits(keyFields(planted)), "other_units");
    expect(other.orphans.map((o) => o.unit)).toEqual(["orphan-demo"]);
  });

  it("nests a drop-in whose unit has a row, wherever that row's group is", () => {
    const other = group(groupUnits(keyFields(planted)), "other_units");
    const slice = other.branches.find((b) => b.head.record.data.unit === "limits-demo.slice")!;
    expect(slice.dropins.map((d) => d.record.data.path)).toEqual([
      "/etc/systemd/system/limits-demo.slice.d/override.conf",
    ]);
  });
});
