/**
 * Row keys, item ids and occurrence keys of the units section (§ 6.11,
 * § 6.12 true duplicates).
 */
import { describe, it, expect } from "vitest";
import type {
  DefaultTargetView,
  DropInDecisionDto,
  ManualLinkView,
  MountFindingDto,
  ServiceDecisionDto,
  UndeterminedInstallView,
} from "../../api/types";
import { keyFields, rowKey, rowKeyForItemId, type UnitRecord } from "../unitOutcome";
import { loadUnitFields } from "../../__tests__/fixtures/systemd-units/load";

const planted = loadUnitFields("planted");

const SHARED_UNIT = "data-demo.mount";
const DUP_LINK = "dup-link-demo.service";
const DUP_LINK_OTHER = "dup-link-demo-other.service";
const DUP_STATE = "dup-state-demo.service";
const DUP_STATE_DROPIN = "/etc/systemd/system/dup-state-demo.service.d/override.conf";

function unitState(unit: string): ServiceDecisionDto {
  const row = planted.unit_states.find((r) => r.kind === "service");
  return { ...row!, unit };
}

function dropin(path: string): DropInDecisionDto {
  return { ...planted.unit_dropins[0], path };
}

const target: DefaultTargetView = {
  target: "graphical.target",
  include: true,
  locked: false,
  declined: null,
};

const link: ManualLinkView = {
  relation: "wants",
  parent: "multi-user.target",
  unit: "driftify-static.service",
  cause: "static_unit",
  advisories: [],
};

const undetermined: UndeterminedInstallView = {
  unit: "driftify-upheld.service",
  reason: "upheld_by",
  install_files: [],
  advisories: [],
};

const mount: MountFindingDto = {
  unit: "tmp.mount",
  kind: "mount",
  state: null,
  mount_point: null,
  fragment: "vendor",
  unit_file_path: null,
  advisories: [],
};

describe("§ 6.11 keys", () => {
  it.each<[string, UnitRecord, string]>([
    ["unit state", { kind: "unit_state", data: unitState("cups.service") }, '["unit_state","cups.service"]'],
    [
      "drop-in",
      { kind: "dropin", data: dropin("/etc/systemd/system/httpd.service.d/override.conf") },
      '["dropin","/etc/systemd/system/httpd.service.d/override.conf"]',
    ],
    ["default target", { kind: "default_target", data: target }, '["default_target"]'],
    [
      "manual link",
      { kind: "manual_link", data: link },
      '["manual_link","wants","multi-user.target","driftify-static.service"]',
    ],
    [
      "manual link, cause ignored",
      { kind: "manual_link", data: { ...link, cause: { undetermined: { reason: "specifier" } } } },
      '["manual_link","wants","multi-user.target","driftify-static.service"]',
    ],
    [
      "undetermined install",
      { kind: "undetermined_install", data: undetermined },
      '["undetermined_install","driftify-upheld.service"]',
    ],
    ["mount finding", { kind: "mount_finding", data: mount }, '["mount_finding","tmp.mount"]'],
  ])("rowKey of a %s", (_name, record, expected) => {
    expect(rowKey(record)).toBe(expected);
  });

  it.each<[string, string]>([
    ["cups.socket", '["unit_state","cups.socket"]'],
    ["backup\\x2djob.timer", '["unit_state","backup\\\\x2djob.timer"]'],
  ])("rowKeyForItemId reads the bare name %s as a unit row", (itemId, expected) => {
    expect(rowKeyForItemId(itemId)).toBe(expected);
  });

  it.each([
    '["unit_state","cups.service"]',
    '["dropin","/etc/systemd/system/httpd.service.d/override.conf"]',
    '["default_target"]',
    '["manual_link","wants","multi-user.target","driftify-static.service"]',
    '["undetermined_install","driftify-upheld.service"]',
    '["mount_finding","tmp.mount"]',
  ])("rowKeyForItemId gives the well-formed key %s back byte-identical", (key) => {
    expect(rowKeyForItemId(key)).toBe(key);
  });

  it.each(["[", '["nope","x"]', '["unit_state"]', '["manual_link","wants","x"]', "[1]"])(
    "rowKeyForItemId(%s) names no row",
    (itemId) => {
      expect(rowKeyForItemId(itemId)).toBeNull();
    },
  );

  it("keeps a prefixed unit name apart from the undetermined install it looks like", () => {
    const unitRow = rowKey({ kind: "unit_state", data: unitState("undetermined-install-demo.service") });
    const install = rowKey({
      kind: "undetermined_install",
      data: { ...undetermined, unit: "demo.service" },
    });
    expect(unitRow).not.toBe(install);
    expect(rowKeyForItemId("undetermined-install-demo.service")).toBe(unitRow);
  });

  describe("keyFields(planted)", () => {
    const keyed = keyFields(planted);

    it("numbers the two identical dup-link-demo.service links on one row key", () => {
      const dup = keyed.manualLinks.filter((k) => k.record.data.unit === DUP_LINK);
      expect(dup).toHaveLength(2);
      const key = '["manual_link","wants","multi-user.target","dup-link-demo.service"]';
      expect(dup.map((k) => k.rowKey)).toEqual([key, key]);
      expect(dup.map((k) => k.occurrenceKey)).toEqual([`${key}#0`, `${key}#1`]);
      const other = keyed.manualLinks.filter((k) => k.record.data.unit === DUP_LINK_OTHER);
      expect(other.map((k) => k.occurrenceKey)).toEqual([
        '["manual_link","wants","multi-user.target","dup-link-demo-other.service"]#0',
      ]);
    });

    it("numbers the duplicated unit row and its duplicated drop-in", () => {
      const rows = keyed.unitStates.filter((k) => k.record.data.unit === DUP_STATE);
      expect(rows.map((k) => k.occurrenceKey)).toEqual([
        '["unit_state","dup-state-demo.service"]#0',
        '["unit_state","dup-state-demo.service"]#1',
      ]);
      const dropins = keyed.dropins.filter((k) => k.record.data.path === DUP_STATE_DROPIN);
      const key = JSON.stringify(["dropin", DUP_STATE_DROPIN]);
      expect(dropins.map((k) => k.occurrenceKey)).toEqual([`${key}#0`, `${key}#1`]);
    });

    it("keys the default target", () => {
      expect(keyed.defaultTarget?.rowKey).toBe('["default_target"]');
      expect(keyed.defaultTarget?.occurrenceKey).toBe('["default_target"]#0');
    });

    it("gives every record a unique occurrence key", () => {
      const all = [
        ...keyed.unitStates,
        ...keyed.dropins,
        ...(keyed.defaultTarget ? [keyed.defaultTarget] : []),
        ...keyed.mountFindings,
        ...keyed.manualLinks,
        ...keyed.undeterminedInstalls,
      ].map((k) => k.occurrenceKey);
      const records =
        planted.unit_states.length +
        planted.unit_dropins.length +
        (planted.default_target ? 1 : 0) +
        planted.mount_findings.length +
        planted.manual_links.length +
        planted.undetermined_installs.length;
      expect(all).toHaveLength(records);
      expect(new Set(all).size).toBe(all.length);
    });

    it("gives the shared unit name four #0 keys of four kinds", () => {
      const keys = [
        ...keyed.unitStates,
        ...keyed.mountFindings,
        ...keyed.manualLinks,
        ...keyed.undeterminedInstalls,
      ]
        .filter((k) => k.record.data.unit === SHARED_UNIT)
        .map((k) => k.occurrenceKey);
      expect(keys).toEqual([
        '["unit_state","data-demo.mount"]#0',
        '["mount_finding","data-demo.mount"]#0',
        '["manual_link","wants","multi-user.target","data-demo.mount"]#0',
        '["undetermined_install","data-demo.mount"]#0',
      ]);
    });

    it("gives equal keys when called twice", () => {
      const again = keyFields(planted);
      const keysOf = (k: typeof keyed) => JSON.stringify(k);
      expect(keysOf(again)).toBe(keysOf(keyed));
    });
  });
});
