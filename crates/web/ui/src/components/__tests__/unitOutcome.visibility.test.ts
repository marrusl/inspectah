/**
 * What the units section's filter, search and pins show, on the model
 * (§ 6.3 item 3, § 6.4, § 6.9, § 6.11 the reveal's tests, § 6.12, § 7
 * strings 19 to 22). U7, U24 matching, U25 search text and U35 (b).
 */
import { describe, it, expect } from "vitest";
import type {
  DropInDecisionDto,
  ManualLinkView,
  MountFindingDto,
  ServiceDecisionDto,
  UndeterminedInstallView,
} from "../../api/types";
import {
  branchHeadOf,
  globalSearchText,
  groupUnits,
  hiddenByFilter,
  hiddenByQuery,
  keyFields,
  liveRegionText,
  locateRow,
  rowKey,
  sectionSearchText,
  showGroups,
  shownRowCount,
  type FilterValue,
  type GroupId,
  type OccurrenceKey,
  type ShowState,
  type ShownGroup,
  type UnitGroupModel,
} from "../unitOutcome";
import { loadUnitFields } from "../../__tests__/fixtures/systemd-units/load";

const kitchenSink = loadUnitFields("kitchen-sink");
const planted = loadUnitFields("planted");
const ksKeyed = keyFields(kitchenSink);
const ksGroups = groupUnits(ksKeyed);
const plantedKeyed = keyFields(planted);
const plantedGroups = groupUnits(plantedKeyed);

const HTTPD_LIMITS = "/etc/systemd/system/httpd.service.d/limits.conf";

/** Reported rows with no advisories, so each field must be searched itself. */
const FIELD_LINK: ManualLinkView = {
  relation: "upholds",
  parent: "fieldparent.target",
  unit: "fieldlinkunit.service",
  cause: "not_in_install",
  advisories: [],
};
const FIELD_INSTALL: UndeterminedInstallView = {
  unit: "fieldinstallunit.service",
  reason: "upheld_by",
  install_files: ["/fieldinstall"],
  advisories: [],
};
/** A drop-in whose unit is not a substring of its path. */
const FIELD_DROPIN: DropInDecisionDto = {
  ...kitchenSink.unit_dropins[0],
  unit: "fielddropinunit.service",
  path: "/etc/systemd/system/elsewhere.d/override.conf",
};
/** A mount finding whose kind is not a substring of its unit or mount point. */
const FIELD_MOUNT: MountFindingDto = {
  ...planted.mount_findings[0],
  unit: "data-demo.mount",
  kind: "fieldmountkind",
  advisories: [],
};

function state(over: Partial<ShowState> = {}): ShowState {
  return { filter: "all", query: "", pins: new Set<OccurrenceKey>(), ...over };
}

function shownGroup(shown: ShownGroup[], id: GroupId): ShownGroup {
  const found = shown.find((g) => g.id === id);
  expect(found).toBeDefined();
  return found!;
}

const heads = (g: ShownGroup | UnitGroupModel) => g.branches.map((b) => b.head.record.data.unit);
const reportedRows = (g: ShownGroup) => g.reported.flatMap((b) => b.rows);

function headKey(groups: UnitGroupModel[], unit: string): OccurrenceKey {
  for (const g of groups) {
    const branch = g.branches.find((b) => b.head.record.data.unit === unit);
    if (branch) return branch.head.occurrenceKey;
  }
  throw new Error(`no branch for ${unit}`);
}

const sshdOrphan = () => ksGroups.find((g) => g.id === "services")!.orphans[0].dropins[0];

describe("U7: what each filter value shows (§ 6.4)", () => {
  it("All shows everything", () => {
    const shown = showGroups(ksGroups, state());
    expect(shown.map((g) => g.id)).toEqual([
      "default_target",
      "services",
      "sockets",
      "timers",
      "other_units",
      "reported",
    ]);
    expect(shownGroup(shown, "services").orphans.map((o) => o.unit)).toEqual(["sshd.service"]);
    // 19 unit rows, the target, the sshd.service orphan, 3 reported rows.
    expect(shownRowCount(shown)).toBe(24);
  });

  it("Needs follow-up shows the declined branches and the reported rows", () => {
    const shown = showGroups(ksGroups, state({ filter: "needs_follow_up" }));
    expect(shown.map((g) => g.id)).toEqual(["services", "other_units", "reported"]);
    expect(heads(shownGroup(shown, "services"))).toEqual([
      "cups.service",
      "driftify-also-main.service",
      "iscsid.service",
      "virtnetworkd.service",
      "wg-quick@wg0.service",
    ]);
    expect(shownGroup(shown, "services").orphans).toEqual([]);
    expect(heads(shownGroup(shown, "other_units"))).toEqual(["tmp.mount"]);
    expect(reportedRows(shownGroup(shown, "reported"))).toHaveLength(3);
  });

  it("Carried shows carried branches with their drop-ins, no orphans and no reported rows", () => {
    const shown = showGroups(ksGroups, state({ filter: "carried" }));
    expect(shown.map((g) => g.id)).toEqual(["default_target", "services", "sockets", "timers"]);
    const services = shownGroup(shown, "services");
    expect(services.orphans).toEqual([]);
    const httpd = services.branches.find((b) => b.head.record.data.unit === "httpd.service")!;
    expect(httpd.dropins).toHaveLength(2);
    expect(heads(shownGroup(shown, "sockets"))).toHaveLength(6);
  });

  it("Excluded shows nothing on kitchen-sink", () => {
    expect(showGroups(ksGroups, state({ filter: "excluded" }))).toEqual([]);
  });
});

describe("pins (§ 6.4)", () => {
  const httpd = headKey(ksGroups, "httpd.service");

  it("shows a pinned carried branch in its place under Needs follow-up, with its drop-ins", () => {
    const shown = showGroups(ksGroups, state({ filter: "needs_follow_up", pins: new Set([httpd]) }));
    const services = shownGroup(shown, "services");
    expect(heads(services)).toEqual([
      "cups.service",
      "driftify-also-main.service",
      "httpd.service",
      "iscsid.service",
      "virtnetworkd.service",
      "wg-quick@wg0.service",
    ]);
    const branch = services.branches.find((b) => b.head.occurrenceKey === httpd)!;
    expect(branch.dropins).toHaveLength(2);
  });

  it("draws a group that holds only a pinned branch", () => {
    const socket = headKey(ksGroups, "cockpit.socket");
    const shown = showGroups(ksGroups, state({ filter: "needs_follow_up", pins: new Set([socket]) }));
    expect(heads(shownGroup(shown, "sockets"))).toEqual(["cockpit.socket"]);
  });

  it("shows a pinned branch whatever the query says", () => {
    const shown = showGroups(ksGroups, state({ query: "cups", pins: new Set([httpd]) }));
    expect(heads(shownGroup(shown, "services"))).toEqual(["cups.service", "httpd.service"]);
  });

  it.each<FilterValue>(["needs_follow_up", "carried", "excluded"])(
    "shows a pinned orphan drop-in under %s",
    (filter) => {
      const orphan = sshdOrphan();
      const shown = showGroups(ksGroups, state({ filter, pins: new Set([orphan.occurrenceKey]) }));
      expect(shownGroup(shown, "services").orphans.map((o) => o.dropins)).toEqual([[orphan]]);
    },
  );

  it("maps a nested drop-in to its parent's head, and leaves heads, the target and orphans", () => {
    const limits = ksKeyed.dropins.find((d) => d.record.data.path === HTTPD_LIMITS)!;
    expect(branchHeadOf(ksGroups, limits.occurrenceKey)).toBe(httpd);
    expect(branchHeadOf(ksGroups, httpd)).toBe(httpd);
    expect(branchHeadOf(ksGroups, '["default_target"]#0')).toBe('["default_target"]#0');
    expect(branchHeadOf(ksGroups, sshdOrphan().occurrenceKey)).toBe(sshdOrphan().occurrenceKey);
  });
});

describe("U24: section search matching (§ 6.12)", () => {
  const allUnits = (shown: ShownGroup[]) => shown.flatMap(heads);

  it("matches a unit row by its declined text, lowercasing the query", () => {
    const shown = showGroups(ksGroups, state({ query: "CUPS.SOCKET (NAMED BY" }));
    expect(allUnits(shown)).toEqual(["cups.service"]);
    expect(shownRowCount(shown)).toBe(1);
  });

  it("matches a parent by a drop-in path and shows all its drop-ins", () => {
    const shown = showGroups(ksGroups, state({ query: "limits.conf" }));
    expect(allUnits(shown)).toEqual(["httpd.service"]);
    expect(shownGroup(shown, "services").branches[0].dropins).toHaveLength(2);
  });

  it("matches a reported row by an advisory line", () => {
    const shown = showGroups(ksGroups, state({ query: "restart loop" }));
    expect(shown.map((g) => g.id)).toEqual(["reported"]);
    const rows = reportedRows(shown[0]);
    expect(rows.map((r) => r.record.kind)).toEqual(["manual_link"]);
    expect(rows[0].record.data.unit).toBe("driftify-upheld.service");
  });

  it("treats a blank query as no query", () => {
    expect(showGroups(ksGroups, state({ query: "  " }))).toEqual(showGroups(ksGroups, state()));
  });

  it("builds a unit row's text from every field § 6.12 lists, and its drop-ins' paths", () => {
    const row: ServiceDecisionDto = {
      ...kitchenSink.unit_states[0],
      unit: "Field-Unit.service",
      kind: "fieldkind",
      owning_package: "fieldpkg",
      current_state: "fieldcurrent",
      default_state: "fielddefault",
      instance_of: "fieldtemplate@.service",
      declined: { renders: "fixme", reason: "R", text: "fielddeclined" },
      attention_reason: "fieldattention",
    };
    const keyed = keyFields({ ...kitchenSink, unit_states: [row] });
    const branch = groupUnits(keyed).find((g) => g.id === "other_units")!.branches[0];
    branch.dropins = [ksKeyed.dropins[0]];
    const text = sectionSearchText(branch);
    for (const word of [
      "field-unit.service",
      "fieldkind",
      "fieldpkg",
      "fieldcurrent",
      "fielddefault",
      "fieldtemplate@.service",
      "fielddeclined",
      "fieldattention",
      ksKeyed.dropins[0].record.data.path,
    ]) {
      expect(text).toContain(word);
    }
    const local = keyFields({
      ...kitchenSink,
      unit_states: [{ ...row, owning_package: null, fragment: "shadowed" }],
    }).unitStates[0];
    expect(sectionSearchText(local)).toContain("shadowed");
  });

  it("builds the target's, an orphan's and a reported row's text", () => {
    expect(sectionSearchText(ksKeyed.defaultTarget!)).toContain("graphical.target");
    const orphan = sectionSearchText(sshdOrphan());
    expect(orphan).toContain("/etc/systemd/system/sshd.service");
    expect(orphan).toContain("sshd.service");
    const link = sectionSearchText(ksKeyed.manualLinks[1]);
    for (const word of ["driftify-upheld.service", "multi-user.target", "upholds", "restart loop"]) {
      expect(link).toContain(word);
    }
    const undetermined = sectionSearchText(ksKeyed.undeterminedInstalls[0]);
    expect(undetermined).toContain("/etc/systemd/system/driftify-upheld.service");
    expect(undetermined).toContain("check them with systemctl is-enabled");
  });

  it("builds a reported row's text from each field, with no advisory to lean on", () => {
    const keyed = keyFields({
      ...kitchenSink,
      manual_links: [FIELD_LINK],
      undetermined_installs: [FIELD_INSTALL],
    });
    const link = keyed.manualLinks[0];
    for (const word of ["fieldlinkunit.service", "fieldparent.target", "upholds"]) {
      expect(sectionSearchText(link)).toContain(word);
      expect(globalSearchText(link.record)).toContain(word);
    }
    const install = keyed.undeterminedInstalls[0];
    for (const word of ["fieldinstallunit.service", "/fieldinstall"]) {
      expect(sectionSearchText(install)).toContain(word);
      expect(globalSearchText(install.record)).toContain(word);
    }
  });

  it("gives a drop-in's unit even when its path does not hold it", () => {
    const dropin = keyFields({ ...kitchenSink, unit_dropins: [FIELD_DROPIN] }).dropins[0];
    expect(sectionSearchText(dropin)).toContain("fielddropinunit.service");
    expect(globalSearchText(dropin.record)).toContain("fielddropinunit.service");
  });
});

describe("§ 6.3 counts under filter and search", () => {
  it("counts unit rows and reported rows, not drop-ins or orphans, and the target as 0", () => {
    const shown = showGroups(ksGroups, state());
    const counts = Object.fromEntries(shown.map((g) => [g.id, g.count]));
    expect(counts).toEqual({
      default_target: 0,
      services: 11,
      sockets: 6,
      timers: 1,
      other_units: 1,
      reported: 3,
    });
  });

  it("counts each reported row of a duplicate once", () => {
    const shown = showGroups(plantedGroups, state({ query: "dup-link-demo.service" }));
    expect(shownGroup(shown, "reported").count).toBe(2);
  });

  it("counts under a query, while the flag counts every follow-up row", () => {
    const shown = showGroups(ksGroups, state({ query: "cups" }));
    const services = shownGroup(shown, "services");
    expect(services.count).toBe(1);
    expect(services.followUp).toBe(5);
    expect(shownGroup(shown, "sockets").followUp).toBeNull();
  });

  it("shows no flag under any value but All", () => {
    const shown = showGroups(ksGroups, state({ filter: "needs_follow_up" }));
    expect(shown.map((g) => g.followUp)).toEqual([null, null, null]);
    expect(shownGroup(shown, "services").count).toBe(5);
  });

  it("shows no flag on the default target or reported group", () => {
    const shown = showGroups(plantedGroups, state());
    expect(shownGroup(shown, "default_target").followUp).toBeNull();
    expect(shownGroup(shown, "reported").followUp).toBeNull();
  });
});

describe("U25: global search text (§ 6.12 table)", () => {
  const unitRow = (unit: string) =>
    plantedKeyed.unitStates.find((k) => k.record.data.unit === unit)!.record;

  it("gives the default target its name and the words default target", () => {
    const text = globalSearchText(plantedKeyed.defaultTarget!.record);
    expect(text).toContain("graphical.target");
    expect(text).toContain("default target");
  });

  it("holds an undetermined install's reason, with _ as a space, apart from the unit row's", () => {
    const install = plantedKeyed.undeterminedInstalls.find(
      (k) => k.record.data.unit === "data-demo.mount",
    )!.record;
    expect(globalSearchText(install)).toContain("specifier");
    expect(globalSearchText(unitRow("data-demo.mount"))).not.toContain("specifier");
    const foreign = plantedKeyed.undeterminedInstalls.find((k) => k.record.data.unit === "demo.service")!;
    expect(globalSearchText(foreign.record)).toContain("foreign name");
  });

  it("tells the three links of one unit apart", () => {
    const links = plantedKeyed.manualLinks
      .filter((k) => k.record.data.unit === "multi-link-demo.service")
      .map((k) => globalSearchText(k.record));
    expect(links).toHaveLength(3);
    expect(links.filter((t) => t.includes("graphical"))).toHaveLength(1);
    expect(links.filter((t) => t.includes("requires"))).toHaveLength(1);
  });

  it("leaves drop-in paths out of a unit row's text", () => {
    const httpd = ksKeyed.unitStates.find((k) => k.record.data.unit === "httpd.service")!;
    expect(globalSearchText(httpd.record)).toContain("httpd");
    expect(globalSearchText(httpd.record)).not.toContain("override.conf");
  });

  it("gives a drop-in its path and unit, and a mount finding its kind and mount point", () => {
    const limits = ksKeyed.dropins.find((d) => d.record.data.path === HTTPD_LIMITS)!;
    expect(globalSearchText(limits.record)).toContain(HTTPD_LIMITS);
    expect(globalSearchText(limits.record)).toContain("httpd.service");
    const mount = globalSearchText(plantedKeyed.mountFindings[0].record);
    for (const word of ["data-demo.mount", "mount", "/data/demo", "is enabled on the source host"]) {
      expect(mount).toContain(word);
    }
    expect(globalSearchText({ kind: "mount_finding", data: FIELD_MOUNT })).toContain("fieldmountkind");
  });

  it("is lowercased", () => {
    const pk = plantedKeyed.unitStates.find((k) => k.record.data.unit === "packagekit.service")!;
    const text = globalSearchText(pk.record);
    expect(text).toBe(text.toLowerCase());
    expect(text).toContain("packagekit");
  });
});

describe("U35 (b) on the model", () => {
  it.each<[string, number]>([
    ["dup-link-demo", 3],
    ["dup-link-demo.service", 2],
  ])("query %s shows %i reported rows", (query, n) => {
    const shown = showGroups(plantedGroups, state({ query }));
    expect(shown.map((g) => g.id)).toEqual(["reported"]);
    expect(reportedRows(shown[0])).toHaveLength(n);
    expect(shownRowCount(shown)).toBe(n);
  });
});

describe("§ 6.11 locating a row and what hides it", () => {
  it("finds the first occurrence of a duplicated link", () => {
    const key = '["manual_link","wants","multi-user.target","dup-link-demo.service"]';
    expect(locateRow(plantedGroups, key)).toEqual({
      groupId: "reported",
      branchHead: `${key}#0`,
      occurrenceKey: `${key}#0`,
    });
  });

  it("finds the first head of a duplicated unit row", () => {
    const key = '["unit_state","dup-state-demo.service"]';
    expect(locateRow(plantedGroups, key)).toEqual({
      groupId: "services",
      branchHead: `${key}#0`,
      occurrenceKey: `${key}#0`,
    });
  });

  it("finds a nested drop-in under its parent's head, and the target", () => {
    const limits = ksKeyed.dropins.find((d) => d.record.data.path === HTTPD_LIMITS)!;
    expect(locateRow(ksGroups, limits.rowKey)).toEqual({
      groupId: "services",
      branchHead: headKey(ksGroups, "httpd.service"),
      occurrenceKey: limits.occurrenceKey,
    });
    expect(locateRow(ksGroups, '["default_target"]')?.groupId).toBe("default_target");
  });

  it("names no row for a key no row has", () => {
    expect(locateRow(ksGroups, '["unit_state","nope.service"]')).toBeNull();
  });

  describe("hiddenByQuery and hiddenByFilter", () => {
    const httpdKey = rowKey(ksKeyed.unitStates.find((k) => k.record.data.unit === "httpd.service")!.record);
    const loc = locateRow(ksGroups, httpdKey)!;

    it("a carried row under Needs follow-up is hidden by the filter alone", () => {
      const s = state({ filter: "needs_follow_up" });
      expect(hiddenByFilter(ksGroups, loc, s)).toBe(true);
      expect(hiddenByQuery(ksGroups, loc, s)).toBe(false);
    });

    it("a row failing the query is hidden by the query alone", () => {
      const s = state({ query: "cups" });
      expect(hiddenByQuery(ksGroups, loc, s)).toBe(true);
      expect(hiddenByFilter(ksGroups, loc, s)).toBe(false);
    });

    it("a pinned row is hidden by neither", () => {
      const s = state({ filter: "needs_follow_up", query: "cups", pins: new Set([loc.branchHead]) });
      expect(hiddenByQuery(ksGroups, loc, s)).toBe(false);
      expect(hiddenByFilter(ksGroups, loc, s)).toBe(false);
    });

    it("a nested drop-in is judged by its branch", () => {
      const limits = locateRow(ksGroups, JSON.stringify(["dropin", HTTPD_LIMITS]))!;
      expect(hiddenByQuery(ksGroups, limits, state({ query: "httpd" }))).toBe(false);
      expect(hiddenByQuery(ksGroups, limits, state({ query: "cups" }))).toBe(true);
      expect(hiddenByFilter(ksGroups, limits, state({ filter: "carried" }))).toBe(false);
    });

    it("an orphan is hidden by any filter but All, a reported row by Carried", () => {
      const orphan = locateRow(ksGroups, sshdOrphan().rowKey)!;
      expect(hiddenByFilter(ksGroups, orphan, state())).toBe(false);
      expect(hiddenByFilter(ksGroups, orphan, state({ filter: "needs_follow_up" }))).toBe(true);
      const link = locateRow(ksGroups, ksKeyed.manualLinks[0].rowKey)!;
      expect(hiddenByFilter(ksGroups, link, state({ filter: "needs_follow_up" }))).toBe(false);
      expect(hiddenByFilter(ksGroups, link, state({ filter: "carried" }))).toBe(true);
    });
  });
});

describe("liveRegionText (§ 7 strings 19 to 22)", () => {
  it.each<[FilterValue, number, string]>([
    ["all", 23, "Showing all 23 rows."],
    ["all", 1, "Showing all 1 row."],
    ["needs_follow_up", 9, "Showing 9 rows that need follow-up."],
    ["needs_follow_up", 1, "Showing 1 row that needs follow-up."],
    ["carried", 14, "Showing 14 carried rows."],
    ["carried", 1, "Showing 1 carried row."],
    ["excluded", 2, "Showing 2 excluded rows."],
    ["excluded", 1, "Showing 1 excluded row."],
  ])("(%s, %i)", (filter, n, expected) => {
    expect(liveRegionText(filter, n)).toBe(expected);
  });
});
