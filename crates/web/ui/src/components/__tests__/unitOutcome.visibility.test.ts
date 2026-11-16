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
  emptyStateFor,
  filterBarDrawn,
  filterBarPresentation,
  firstShownRow,
  globalSearchText,
  groupUnits,
  hiddenByFilter,
  hiddenByQuery,
  keyFields,
  liveRegionText,
  locateRow,
  outcomeCounts,
  pinsInView,
  revealPlan,
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

describe("revealPlan: the reveal's steps in order (§ 6.11)", () => {
  const OPEN: Record<GroupId, boolean> = {
    default_target: false,
    services: false,
    sockets: false,
    timers: false,
    paths: false,
    other_units: false,
    reported: false,
  };
  const CUPS_SOCKET = JSON.stringify(["unit_state", "cups.socket"]);
  const TMP_MOUNT = JSON.stringify(["unit_state", "tmp.mount"]);
  const reveal = (key: string, over: Partial<ShowState> = {}, collapsed = OPEN) =>
    revealPlan(ksGroups, key, { ...state(over), collapsed });

  it("changes nothing when nothing hides the row", () => {
    expect(reveal(CUPS_SOCKET)).toEqual({
      clearQuery: false,
      filterToAll: false,
      openGroup: null,
      clearPins: false,
    });
  });

  it("clears the query alone when only the query hides the row, keeping the filter", () => {
    // tmp.mount needs follow-up, so the filter shows it; "httpd" does not match it.
    expect(reveal(TMP_MOUNT, { filter: "needs_follow_up", query: "httpd" })).toEqual({
      clearQuery: true,
      filterToAll: false,
      openGroup: null,
      clearPins: true,
    });
  });

  it("sets the filter alone when only the filter hides the row, keeping the query", () => {
    expect(reveal(CUPS_SOCKET, { filter: "needs_follow_up", query: "socket" })).toEqual({
      clearQuery: false,
      filterToAll: true,
      openGroup: null,
      clearPins: true,
    });
  });

  it("does both when both hide the row", () => {
    expect(reveal(CUPS_SOCKET, { filter: "needs_follow_up", query: "httpd" })).toEqual({
      clearQuery: true,
      filterToAll: true,
      openGroup: null,
      clearPins: true,
    });
  });

  it("keeps the query, the filter and the pins for a pinned row", () => {
    const pins = new Set([headKey(ksGroups, "cups.socket")]);
    expect(reveal(CUPS_SOCKET, { filter: "needs_follow_up", query: "httpd", pins })).toEqual({
      clearQuery: false,
      filterToAll: false,
      openGroup: null,
      clearPins: false,
    });
  });

  it("opens the row's group only when it is collapsed", () => {
    expect(reveal(CUPS_SOCKET, {}, { ...OPEN, sockets: true, timers: true })?.openGroup).toBe("sockets");
    expect(reveal(CUPS_SOCKET, {}, { ...OPEN, timers: true })?.openGroup).toBeNull();
  });

  it("plans nothing for a key no row has", () => {
    expect(reveal(JSON.stringify(["unit_state", "nope.service"]))).toBeNull();
  });
});

describe("emptyStateFor: § 6.9 rows 1 to 5", () => {
  const none: ShownGroup[] = [];
  const some = showGroups(ksGroups, state());
  const stock10 = keyFields(loadUnitFields("stock10"));
  const noTarget = keyFields({ ...loadUnitFields("stock10"), default_target: undefined });
  /** stock10 plus one row of the given field: the host is no longer quiet. */
  const withOne = (field: keyof Omit<typeof kitchenSink, "default_target">) =>
    keyFields({ ...loadUnitFields("stock10"), [field]: [kitchenSink[field][0] ?? planted[field][0]] });

  it("row 1: the target and no other row gives the with-target sentence", () => {
    expect(emptyStateFor(stock10, "all", "", showGroups(groupUnits(stock10), state()))).toEqual({
      notice: null,
      quietHost: "with_target",
    });
  });

  it("row 2: no row at all gives the no-target sentence", () => {
    expect(emptyStateFor(noTarget, "all", "", none)).toEqual({ notice: null, quietHost: "without_target" });
  });

  it.each([
    "unit_states",
    "unit_dropins",
    "mount_findings",
    "manual_links",
    "undetermined_installs",
  ] as const)("rows 1 and 2 do not apply with a %s row", (field) => {
    const keyed = withOne(field);
    expect(emptyStateFor(keyed, "all", "", showGroups(groupUnits(keyed), state())).quietHost).toBeNull();
  });

  it("row 3: a kind with no row draws no group", () => {
    expect(groupUnits(stock10).map((g) => g.id)).toEqual(["default_target"]);
  });

  it("row 4: a value that shows no row, with no query, gives the filter state", () => {
    expect(emptyStateFor(ksKeyed, "excluded", "", none)).toEqual({ notice: "filter", quietHost: null });
    expect(emptyStateFor(ksKeyed, "excluded", "   ", none).notice).toBe("filter");
  });

  it("row 4 with row 1: a value that hides the target keeps the sentence", () => {
    expect(emptyStateFor(stock10, "excluded", "", none)).toEqual({ notice: "filter", quietHost: "with_target" });
  });

  it("row 5: a query that shows no row gives the search state, over the filter's, under any value", () => {
    expect(emptyStateFor(ksKeyed, "all", "zzz", none).notice).toBe("search");
    expect(emptyStateFor(ksKeyed, "needs_follow_up", "zzz", none).notice).toBe("search");
    expect(emptyStateFor(stock10, "excluded", "zzz", none)).toEqual({ notice: "search", quietHost: "with_target" });
  });

  it("no notice when a row is shown, or under All with no query", () => {
    expect(emptyStateFor(ksKeyed, "needs_follow_up", "cups", some).notice).toBeNull();
    expect(emptyStateFor(ksKeyed, "excluded", "", some).notice).toBeNull();
    expect(emptyStateFor(ksKeyed, "all", "", none).notice).toBeNull();
  });
});

describe("filterBarPresentation (§ 6.4 values, counts, disabled values and the hint)", () => {
  const counts = (nf: number, carried: number, excluded: number) => ({
    all: nf + carried + excluded,
    needs_follow_up: nf,
    carried,
    excluded,
  });

  it("gives each value in the bar's order with its label and count", () => {
    const bar = filterBarPresentation(counts(9, 14, 0), "all");
    expect(bar.items.map((i) => [i.value, i.label, i.count])).toEqual([
      ["all", "All", 23],
      ["needs_follow_up", "Needs follow-up", 9],
      ["carried", "Carried", 14],
      ["excluded", "Excluded", 0],
    ]);
  });

  it("marks the selected value and only it", () => {
    const bar = filterBarPresentation(counts(9, 14, 0), "carried");
    expect(bar.items.map((i) => i.selected)).toEqual([false, false, true, false]);
  });

  it("disables a value with count 0, unless it is the selected one", () => {
    expect(filterBarPresentation(counts(9, 14, 0), "all").items.map((i) => i.disabled)).toEqual([
      false,
      false,
      false,
      true,
    ]);
    expect(filterBarPresentation(counts(9, 14, 0), "excluded").items.map((i) => i.disabled)).toEqual([
      false,
      false,
      false,
      false,
    ]);
    expect(filterBarPresentation(counts(0, 0, 1), "all").items.map((i) => i.disabled)).toEqual([
      false,
      true,
      true,
      false,
    ]);
  });

  it("shows the hint while a row needs follow-up, and only then", () => {
    expect(filterBarPresentation(counts(1, 0, 0), "all").hint).toBe(true);
    expect(filterBarPresentation(counts(0, 5, 1), "all").hint).toBe(false);
    expect(filterBarPresentation(counts(0, 5, 1), "excluded").hint).toBe(false);
  });
});

describe("firstShownRow: the first shown row in section order (§ 6.12, § 6.13)", () => {
  const OPEN: Record<GroupId, boolean> = {
    default_target: false,
    services: false,
    sockets: false,
    timers: false,
    paths: false,
    other_units: false,
    reported: false,
  };

  it("is the target under All", () => {
    expect(firstShownRow(showGroups(ksGroups, state()), OPEN)).toBe(ksKeyed.defaultTarget!.occurrenceKey);
  });

  it("skips a collapsed group", () => {
    const shown = showGroups(ksGroups, state());
    expect(firstShownRow(shown, { ...OPEN, default_target: true })).toBe(headKey(ksGroups, "bluetooth.service"));
  });

  it("reaches an orphan drop-in and a reported row", () => {
    const orphan = sshdOrphan();
    expect(firstShownRow(showGroups(ksGroups, state({ query: "sshd" })), OPEN)).toBe(orphan.occurrenceKey);
    const upheld = showGroups(ksGroups, state({ query: "restart loop" }));
    expect(upheld.map((g) => g.id)).toEqual(["reported"]);
    expect(firstShownRow(upheld, OPEN)).toBe(upheld[0].reported[0].rows[0].occurrenceKey);
  });

  it("is null when no row is shown, or every shown group is collapsed", () => {
    expect(firstShownRow([], OPEN)).toBeNull();
    const shown = showGroups(ksGroups, state({ query: "restart loop" }));
    expect(firstShownRow(shown, { ...OPEN, reported: true })).toBeNull();
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

describe("the filter bar and the live region's count (§ 6.4)", () => {
  it.each<[number, number, number, boolean]>([
    [0, 5, 0, false],
    [0, 5, 1, true],
    [1, 5, 0, true],
    [2, 0, 3, true],
  ])("needs follow-up %i, carried %i, excluded %i: drawn %s", (nf, carried, excluded, drawn) => {
    const counts = { all: nf + carried + excluded, needs_follow_up: nf, carried, excluded };
    expect(filterBarDrawn(counts)).toBe(drawn);
  });

  it("announces the rows now shown under All, the orphan drop-in included", () => {
    // 19 unit rows, the target, 3 reported rows and the sshd.service orphan,
    // while the All count is 23.
    expect(shownRowCount(showGroups(ksGroups, state()))).toBe(24);
    expect(outcomeCounts(ksKeyed).all).toBe(23);
  });

  it.each<[FilterValue, number]>([
    ["needs_follow_up", 9],
    ["carried", 14],
    ["excluded", 0],
  ])("matches the %s count with no pin", (filter, n) => {
    expect(shownRowCount(showGroups(ksGroups, state({ filter })))).toBe(n);
  });

  it("counts the target once and a pinned branch's head, not its drop-ins", () => {
    const httpd = headKey(ksGroups, "httpd.service");
    const shown = showGroups(ksGroups, state({ filter: "needs_follow_up", pins: new Set([httpd]) }));
    expect(shownRowCount(shown)).toBe(10);
    const target = showGroups(ksGroups, state({ filter: "carried", query: "graphical" }));
    expect(shownRowCount(target)).toBe(1);
  });

  it("counts an orphan drop-in that is the only row of its group", () => {
    const fields = loadUnitFields("stock10");
    fields.default_target = { ...fields.default_target!, include: false };
    fields.unit_dropins = kitchenSink.unit_dropins.filter((d) => d.unit === "sshd.service");
    const shown = showGroups(groupUnits(keyFields(fields)), state());
    expect(shown.map((g) => g.id)).toEqual(["default_target", "services"]);
    expect(shown[1].branches).toEqual([]);
    expect(shownRowCount(shown)).toBe(2);
    expect(shownRowCount(showGroups(ksGroups, state({ query: "sshd" })))).toBe(1);
  });
});

describe("pins kept by a new view (§ 6.4)", () => {
  const httpd = headKey(ksGroups, "httpd.service");
  const target = '["default_target"]#0';

  it("keeps the pins whose occurrence key the view holds: a head, the target, an orphan", () => {
    const pins = new Set([httpd, target, sshdOrphan().occurrenceKey]);
    expect(pinsInView(pins, ksKeyed)).toBe(pins);
  });

  it("drops a pin whose record left the view", () => {
    const without = keyFields({
      ...kitchenSink,
      unit_states: kitchenSink.unit_states.filter((r) => r.unit !== "httpd.service"),
    });
    expect([...pinsInView(new Set([httpd, target]), without)]).toEqual([target]);
  });

  it("drops the pin on a duplicate's second occurrence when that record leaves", () => {
    const second = plantedKeyed.unitStates.filter((k) => k.record.data.unit === "dup-state-demo.service")[1];
    const fewer = keyFields({
      ...planted,
      unit_states: planted.unit_states.filter((r) => r !== second.record.data),
    });
    // The first record keeps #0, so a row-key match would wrongly keep the pin.
    expect([...pinsInView(new Set([second.occurrenceKey]), fewer)]).toEqual([]);
  });
});
