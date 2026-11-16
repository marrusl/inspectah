/**
 * The planted units fixture (§ 10.1): the kitchen-sink recording plus one
 * hand-made record per case the scan lacks. Each `it` finds its record and
 * asserts the fields that make it that case, so a later edit that drops
 * or blurs a case fails here before it silently weakens the tests that
 * load it. Where each string came from is in `load.ts`'s header.
 */
import { describe, it, expect } from "vitest";
import { loadPlantedAfterInclude, loadUnitFields } from "./load";

const planted = loadUnitFields("planted");
const kitchenSink = loadUnitFields("kitchen-sink");

const LOCKED_SLUG = "service-image-mode-incompatible";
const U8_UNIT = "install-dropin-demo.service";
const U8_DROPIN = "/etc/systemd/system/install-dropin-demo.service.d/install.conf";
const SHARED_UNIT = "data-demo.mount";
const MULTI_LINK_UNIT = "multi-link-demo.service";
const DUP_LINK = "dup-link-demo.service";
const DUP_LINK_OTHER = "dup-link-demo-other.service";
const DUP_STATE = "dup-state-demo.service";
const DUP_STATE_DROPIN = "/etc/systemd/system/dup-state-demo.service.d/override.conf";

function unitRows(name: string) {
  return planted.unit_states.filter((r) => r.unit === name);
}

function unitRow(name: string) {
  const rows = unitRows(name);
  expect(rows).toHaveLength(1);
  return rows[0];
}

describe("planted.units.json (§ 10.1)", () => {
  it("has a locked row with the image-mode-incompatible slug", () => {
    const row = planted.unit_states.find((r) => r.attention_reason === LOCKED_SLUG);
    expect(row).toBeDefined();
    expect(row!.locked).toBe(true);
    expect(row!.include).toBe(false);
  });

  it("has an excluded runtime updater", () => {
    const row = unitRow("dnf-automatic.timer");
    expect(row.include).toBe(false);
    expect(row.locked).toBe(false);
    expect(row.attention_reason).toMatch(/^This unit updates packages in place/);
  });

  it("has a path unit row", () => {
    expect(planted.unit_states.some((r) => r.kind === "path")).toBe(true);
  });

  it("has a slice unit row", () => {
    expect(planted.unit_states.some((r) => r.kind === "slice")).toBe(true);
  });

  it("has an administrator's mount finding", () => {
    const finding = planted.mount_findings.find((m) => m.unit === SHARED_UNIT);
    expect(finding).toBeDefined();
    expect(finding!.kind).toBe("mount");
    expect(finding!.fragment).toBe("local");
    expect(finding!.advisories.length).toBeGreaterThan(0);
  });

  it("has a withheld drop-in", () => {
    const dropin = planted.unit_dropins.find((d) => d.withheld !== undefined);
    expect(dropin).toBeDefined();
    expect(dropin!.include).toBe(true);
    expect(dropin!.withheld!.outcome).toBe("withheld");
  });

  it("has an orphan drop-in with no kind", () => {
    const units = new Set(planted.unit_states.map((r) => r.unit));
    const orphan = planted.unit_dropins.find((d) => d.kind === undefined);
    expect(orphan).toBeDefined();
    expect(units.has(orphan!.unit)).toBe(false);
  });

  it("has a link with relation alias", () => {
    expect(planted.manual_links.some((l) => l.relation === "alias")).toBe(true);
  });

  it("has a link with cause vendor_install_only", () => {
    expect(planted.manual_links.some((l) => l.cause === "vendor_install_only")).toBe(true);
  });

  it("has a declined default target in place of the recorded one", () => {
    expect(planted.default_target).toBeDefined();
    expect(planted.default_target!.declined).not.toBeNull();
    expect(planted.default_target!.declined!.text.length).toBeGreaterThan(0);
  });

  it("has two instance rows with a required_config path each", () => {
    const rows = planted.unit_states.filter(
      (r) => r.instance_of !== undefined && r.required_config.length === 1 && !r.declined,
    );
    expect(rows.length).toBeGreaterThanOrEqual(2);
    expect(new Set(rows.map((r) => r.required_config[0])).size).toBe(rows.length);
  });

  it("has the masked getty@.service beside its instance getty@tty2.service", () => {
    const template = unitRow("getty@.service");
    expect(template.current_state).toBe("masked");
    expect(template.instance_of).toBeUndefined();
    expect(unitRow("getty@tty2.service").instance_of).toBe("getty@.service");
  });

  it("holds one unit name in a unit row, a mount finding, an undetermined install and a manual link", () => {
    expect(unitRows(SHARED_UNIT)).toHaveLength(1);
    expect(planted.mount_findings.filter((m) => m.unit === SHARED_UNIT)).toHaveLength(1);
    expect(planted.undetermined_installs.filter((u) => u.unit === SHARED_UNIT)).toHaveLength(1);
    expect(planted.manual_links.some((l) => l.unit === SHARED_UNIT)).toBe(true);
  });

  it("has the U8 pair: declined by its excluded [Install] drop-in, then cleared", () => {
    const before = unitRow(U8_UNIT);
    expect(before.install_drop_ins).toEqual([U8_DROPIN]);
    expect(before.declined?.reason).toBe("InstallEvidenceChanged");
    expect(before.declined?.text).toContain(U8_DROPIN);
    const dropin = planted.unit_dropins.filter((d) => d.path === U8_DROPIN);
    expect(dropin).toHaveLength(1);
    expect(dropin[0].include).toBe(false);

    const after = loadPlantedAfterInclude();
    const afterRow = after.unit_states.filter((r) => r.unit === U8_UNIT);
    expect(afterRow).toHaveLength(1);
    expect(afterRow[0].declined).toBeUndefined();
    expect(after.unit_dropins.filter((d) => d.path === U8_DROPIN)[0].include).toBe(true);
    // Nothing else differs between the two views.
    const strip = (fields: typeof planted) => ({
      ...fields,
      unit_states: fields.unit_states.filter((r) => r.unit !== U8_UNIT),
      unit_dropins: fields.unit_dropins.filter((d) => d.path !== U8_DROPIN),
    });
    expect(strip(after)).toEqual(strip(planted));
  });

  it("has undetermined-install-demo.service as a unit row beside an undetermined install for demo.service", () => {
    expect(unitRows("undetermined-install-demo.service")).toHaveLength(1);
    expect(planted.undetermined_installs.some((u) => u.unit === "demo.service")).toBe(true);
  });

  it("has three manual links for one unit, by relation and parent", () => {
    const links = planted.manual_links
      .filter((l) => l.unit === MULTI_LINK_UNIT)
      .map((l) => [l.relation, l.parent]);
    expect(links).toEqual([
      ["wants", "multi-user.target"],
      ["wants", "graphical.target"],
      ["requires", "multi-user.target"],
    ]);
  });

  it("has two identical wants links for dup-link-demo.service and one for dup-link-demo-other.service", () => {
    const dup = planted.manual_links.filter((l) => l.unit === DUP_LINK);
    expect(dup).toHaveLength(2);
    expect(dup[0]).toEqual(dup[1]);
    expect([dup[0].relation, dup[0].parent]).toEqual(["wants", "multi-user.target"]);
    const other = planted.manual_links.filter((l) => l.unit === DUP_LINK_OTHER);
    expect(other).toHaveLength(1);
    expect([other[0].relation, other[0].parent]).toEqual(["wants", "multi-user.target"]);
    expect(DUP_LINK_OTHER < DUP_LINK).toBe(true);
  });

  it("has dup-state-demo.service twice, carried, and its drop-in twice", () => {
    const rows = unitRows(DUP_STATE);
    expect(rows).toHaveLength(2);
    expect(rows[0]).toEqual(rows[1]);
    expect(rows[0].include).toBe(true);
    expect(rows[0].declined).toBeUndefined();
    const dropins = planted.unit_dropins.filter((d) => d.path === DUP_STATE_DROPIN);
    expect(dropins).toHaveLength(2);
    expect(dropins[0]).toEqual(dropins[1]);
  });

  it("keeps every kitchen-sink record unchanged", () => {
    for (const row of kitchenSink.unit_states) expect(planted.unit_states).toContainEqual(row);
    for (const d of kitchenSink.unit_dropins) expect(planted.unit_dropins).toContainEqual(d);
    for (const m of kitchenSink.mount_findings) expect(planted.mount_findings).toContainEqual(m);
    for (const l of kitchenSink.manual_links) expect(planted.manual_links).toContainEqual(l);
    for (const u of kitchenSink.undetermined_installs) {
      expect(planted.undetermined_installs).toContainEqual(u);
    }
  });

  it("holds the dup-* names in the dup-* records alone", () => {
    const isDup = (unit: string) => unit.startsWith("dup-");
    const others = [
      ...planted.unit_states.filter((r) => !isDup(r.unit)),
      ...planted.unit_dropins.filter((d) => !isDup(d.unit)),
      ...planted.mount_findings.filter((m) => !isDup(m.unit)),
      ...planted.manual_links.filter((l) => !isDup(l.unit)),
      ...planted.undetermined_installs.filter((u) => !isDup(u.unit)),
      planted.default_target,
    ];
    for (const record of others) {
      const text = JSON.stringify(record);
      expect(text).not.toContain("dup-link-demo");
      expect(text).not.toContain("dup-state-demo");
    }
  });

  it("serves the planted fields without the U8 after-include view", () => {
    expect(Object.keys(planted).sort()).toEqual([
      "default_target",
      "manual_links",
      "mount_findings",
      "undetermined_installs",
      "unit_dropins",
      "unit_states",
    ]);
  });
});
