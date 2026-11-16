/**
 * The units section's rules, decided without React: row keys and
 * occurrence keys (spec § 6.11; nothing else spells a key), outcomes and
 * counts (§ 6.1, § 6.4), groups (§ 6.2, § 6.8), labels (§ 7), and what
 * the filter, search and pins show (§ 6.4, § 6.11, § 6.12).
 */
import type {
  DeclinedView,
  DefaultTargetView,
  DropInDecisionDto,
  LinkRelation,
  ManualLinkCause,
  ManualLinkView,
  MountFindingDto,
  ServiceDecisionDto,
  UndeterminedInstallView,
} from "../api/types";

export type RowKind =
  | "unit_state"
  | "dropin"
  | "default_target"
  | "manual_link"
  | "undetermined_install"
  | "mount_finding";

/** `JSON.stringify([kind, ...identity])`, § 6.11's row key. */
export type RowKey = string;
/** `${RowKey}#${ordinal}`, § 6.11's occurrence key. Nothing parses one. */
export type OccurrenceKey = string;

/** The six unit fields of the view response. */
export interface UnitFields {
  unit_states: ServiceDecisionDto[];
  unit_dropins: DropInDecisionDto[];
  default_target?: DefaultTargetView;
  mount_findings: MountFindingDto[];
  manual_links: ManualLinkView[];
  undetermined_installs: UndeterminedInstallView[];
}

export type UnitRecord =
  | { kind: "unit_state"; data: ServiceDecisionDto }
  | { kind: "dropin"; data: DropInDecisionDto }
  | { kind: "default_target"; data: DefaultTargetView }
  | { kind: "manual_link"; data: ManualLinkView }
  | { kind: "undetermined_install"; data: UndeterminedInstallView }
  | { kind: "mount_finding"; data: MountFindingDto };

export type RecordOf<K extends RowKind> = Extract<UnitRecord, { kind: K }>;

export interface Keyed<R extends UnitRecord = UnitRecord> {
  record: R;
  rowKey: RowKey;
  occurrenceKey: OccurrenceKey;
  /** How many records before this one carry its row key: 0 for the first. */
  ordinal: number;
}

export interface KeyedFields {
  unitStates: Keyed<RecordOf<"unit_state">>[];
  dropins: Keyed<RecordOf<"dropin">>[];
  defaultTarget?: Keyed<RecordOf<"default_target">>;
  mountFindings: Keyed<RecordOf<"mount_finding">>[];
  manualLinks: Keyed<RecordOf<"manual_link">>[];
  undeterminedInstalls: Keyed<RecordOf<"undetermined_install">>[];
}

/** How many identity parts follow the kind in each kind's row key. */
const IDENTITY_PARTS: Record<RowKind, number> = {
  unit_state: 1,
  dropin: 1,
  default_target: 0,
  manual_link: 3,
  undetermined_install: 1,
  mount_finding: 1,
};

/** The first character of a row key; a unit name never holds it. */
const ROW_KEY_START = "[";

function identity(record: UnitRecord): string[] {
  switch (record.kind) {
    case "unit_state":
      return [record.data.unit];
    case "dropin":
      return [record.data.path];
    case "default_target":
      return [];
    case "manual_link":
      return [record.data.relation, record.data.parent, record.data.unit];
    case "undetermined_install":
      return [record.data.unit];
    case "mount_finding":
      return [record.data.unit];
  }
}

/** The row's kind and complete identity, JSON-encoded (§ 6.11). */
export function rowKey(record: UnitRecord): RowKey {
  return JSON.stringify([record.kind, ...identity(record)]);
}

function isRowKind(value: string): value is RowKind {
  return Object.prototype.hasOwnProperty.call(IDENTITY_PARTS, value);
}

/**
 * The row key an item id names (§ 6.11, "Which row an item id names"): a
 * bare unit name is a unit row; an id starting with `[` must be a row key
 * as `rowKey` writes it, or it names no row.
 */
export function rowKeyForItemId(itemId: string): RowKey | null {
  if (!itemId.startsWith(ROW_KEY_START)) return JSON.stringify(["unit_state", itemId]);
  let parsed: unknown;
  try {
    parsed = JSON.parse(itemId);
  } catch {
    return null;
  }
  if (!Array.isArray(parsed) || !parsed.every((part) => typeof part === "string")) return null;
  const [kind, ...parts] = parsed as string[];
  if (kind === undefined || !isRowKind(kind) || parts.length !== IDENTITY_PARTS[kind]) return null;
  // Compared as a string: only the bytes rowKey writes name a row.
  const key = JSON.stringify(parsed);
  return key === itemId ? key : null;
}

/** Keys one source list; ordinals count records with the same row key. */
function keyList<R extends UnitRecord>(records: R[]): Keyed<R>[] {
  const seen = new Map<RowKey, number>();
  return records.map((record) => {
    const key = rowKey(record);
    const ordinal = seen.get(key) ?? 0;
    seen.set(key, ordinal + 1);
    return { record, rowKey: key, occurrenceKey: `${key}#${ordinal}`, ordinal };
  });
}

/**
 * Assigns every record its row key and occurrence key, once per view
 * response, before grouping, filter or search sees a record (§ 6.11).
 */
export function keyFields(fields: UnitFields): KeyedFields {
  const wrap = <K extends RowKind>(kind: K, list: RecordOf<K>["data"][]) =>
    keyList(list.map((data) => ({ kind, data }) as RecordOf<K>));
  const target = fields.default_target;
  return {
    unitStates: wrap("unit_state", fields.unit_states),
    dropins: wrap("dropin", fields.unit_dropins),
    defaultTarget: target ? wrap("default_target", [target])[0] : undefined,
    mountFindings: wrap("mount_finding", fields.mount_findings),
    manualLinks: wrap("manual_link", fields.manual_links),
    undeterminedInstalls: wrap("undetermined_install", fields.undetermined_installs),
  };
}

// --- Outcomes and counts (§ 6.1, § 6.4) ---

export type Outcome = "needs_follow_up" | "carried" | "excluded";

/** A row's outcome; `null` for a drop-in, which follows its parent. */
export function outcomeOf(record: UnitRecord): Outcome | null {
  switch (record.kind) {
    case "unit_state":
    case "default_target":
      if (record.data.declined) return "needs_follow_up";
      return record.data.include ? "carried" : "excluded";
    case "dropin":
      return null;
    case "manual_link":
    case "undetermined_install":
    case "mount_finding":
      return "needs_follow_up";
  }
}

export interface OutcomeCounts {
  all: number;
  needs_follow_up: number;
  carried: number;
  excluded: number;
}

/** The filter's counts: every row with an outcome, drop-ins not counted. */
export function outcomeCounts(keyed: KeyedFields): OutcomeCounts {
  const counts: OutcomeCounts = { all: 0, needs_follow_up: 0, carried: 0, excluded: 0 };
  const rows: Keyed[] = [
    ...keyed.unitStates,
    ...(keyed.defaultTarget ? [keyed.defaultTarget] : []),
    ...keyed.mountFindings,
    ...keyed.manualLinks,
    ...keyed.undeterminedInstalls,
  ];
  for (const { record } of rows) {
    const outcome = outcomeOf(record);
    if (outcome === null) continue;
    counts.all += 1;
    counts[outcome] += 1;
  }
  return counts;
}

// --- Groups (§ 6.2, § 6.3, § 6.6 item 11, § 6.8) ---

export type GroupId =
  | "default_target"
  | "services"
  | "sockets"
  | "timers"
  | "paths"
  | "other_units"
  | "reported";

export const GROUP_ORDER: readonly GroupId[] = [
  "default_target",
  "services",
  "sockets",
  "timers",
  "paths",
  "other_units",
  "reported",
];

/** § 7 strings 2 to 5. */
export const GROUP_TITLES: Record<GroupId, string> = {
  default_target: "Default target",
  services: "Services",
  sockets: "Sockets",
  timers: "Timers",
  paths: "Paths",
  other_units: "Other units",
  reported: "Reported, not carried",
};

/** The kinds with a group of their own; every other kind is Other units. */
const KIND_GROUPS: Readonly<Record<string, GroupId>> = {
  service: "services",
  socket: "sockets",
  timer: "timers",
  path: "paths",
};

export interface UnitBranch {
  head: Keyed<RecordOf<"unit_state">>;
  dropins: Keyed<RecordOf<"dropin">>[];
}

export interface OrphanBlock {
  unit: string;
  dropins: Keyed<RecordOf<"dropin">>[];
}

export type ReportedRow = Keyed<RecordOf<"mount_finding" | "manual_link" | "undetermined_install">>;

export interface ReportedBlock {
  unit: string;
  rows: ReportedRow[];
}

export interface UnitGroupModel {
  id: GroupId;
  target?: Keyed<RecordOf<"default_target">>;
  branches: UnitBranch[];
  orphans: OrphanBlock[];
  reported: ReportedBlock[];
  /** § 6.3 item 4's N, kind groups only, else 0. */
  followUpTotal: number;
}

function groupOfKind(kind: string | undefined): GroupId {
  if (kind !== undefined && Object.prototype.hasOwnProperty.call(KIND_GROUPS, kind)) {
    return KIND_GROUPS[kind];
  }
  return "other_units";
}

/** Plain string order (code-point), § 6.2; with a stable sort, ties keep response order. */
function byCodePoint(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0;
}

function emptyGroup(id: GroupId): UnitGroupModel {
  return { id, branches: [], orphans: [], reported: [], followUpTotal: 0 };
}

/**
 * Places every row by kind and name only (§ 6.2): never by outcome, so a
 * toggle never moves a row. Groups with no row are left out.
 */
export function groupUnits(keyed: KeyedFields): UnitGroupModel[] {
  const groups = new Map<GroupId, UnitGroupModel>(GROUP_ORDER.map((id) => [id, emptyGroup(id)]));
  const at = (id: GroupId) => groups.get(id)!;

  if (keyed.defaultTarget) at("default_target").target = keyed.defaultTarget;

  // Drop-ins nest under the first branch, in response order, of their unit.
  const firstBranch = new Map<string, UnitBranch>();
  for (const head of keyed.unitStates) {
    const branch: UnitBranch = { head, dropins: [] };
    if (!firstBranch.has(head.record.data.unit)) firstBranch.set(head.record.data.unit, branch);
    const g = at(groupOfKind(head.record.data.kind));
    g.branches.push(branch);
    if (outcomeOf(head.record) === "needs_follow_up") g.followUpTotal += 1;
  }

  const orphanBlocks = new Map<string, OrphanBlock>();
  for (const dropin of keyed.dropins) {
    const unit = dropin.record.data.unit;
    const parent = firstBranch.get(unit);
    if (parent) {
      parent.dropins.push(dropin);
      continue;
    }
    let block = orphanBlocks.get(unit);
    if (!block) {
      block = { unit, dropins: [] };
      orphanBlocks.set(unit, block);
      // The block's group is its first drop-in's `kind`: that is the parent's kind, shared by all.
      at(groupOfKind(dropin.record.data.kind)).orphans.push(block);
    }
    block.dropins.push(dropin);
  }

  // Per unit: its mount finding, then its links, then its undetermined install.
  const reportedBlocks = new Map<string, ReportedBlock>();
  const reportedRows: ReportedRow[] = [
    ...keyed.mountFindings,
    ...keyed.manualLinks,
    ...keyed.undeterminedInstalls,
  ];
  for (const row of reportedRows) {
    const unit = row.record.data.unit;
    let block = reportedBlocks.get(unit);
    if (!block) {
      block = { unit, rows: [] };
      reportedBlocks.set(unit, block);
    }
    block.rows.push(row);
  }
  at("reported").reported = [...reportedBlocks.values()].sort((a, b) => byCodePoint(a.unit, b.unit));

  for (const g of groups.values()) {
    g.branches.sort((a, b) => byCodePoint(a.head.record.data.unit, b.head.record.data.unit));
  }

  return GROUP_ORDER.map(at).filter(
    (g) => g.target || g.branches.length > 0 || g.orphans.length > 0 || g.reported.length > 0,
  );
}

// --- Labels (§ 7) ---

/** § 7 string 15, by `declined.renders`: the Containerfile's own prefixes. */
export const OUTCOME_LABELS: Record<DeclinedView["renders"], "FIXME" | "Advisory" | "Omitted"> = {
  fixme: "FIXME",
  advisory: "Advisory",
  omitted: "Omitted",
};

/** § 7 string 17, shown before a link's parent. */
export const RELATION_LABELS: Record<LinkRelation, string> = {
  wants: "wanted by",
  requires: "required by",
  upholds: "upheld by",
  alias: "has alias",
  also: "named in Also= by",
};

/** § 7 string 18 for `undetermined`, and for every undetermined-install row. */
export const UNDETERMINED_CAUSE_LABEL = "[Install] cannot be relied on";

/** § 7 string 18 for the causes that serialize as a bare string. */
const CAUSE_LABELS: Record<Exclude<ManualLinkCause, object>, string> = {
  static_unit: "Link its [Install] does not make",
  not_in_install: "Link made by hand",
  partial_install: "Only some of its links",
  host_device: "Tied to a host device",
  host_install_only: "Only the /etc files name it",
  vendor_install_only: "Only the package unit names it",
};

/** A link's cause label. `cause` is externally tagged: branch on `typeof`. */
export function causeLabel(cause: ManualLinkCause): string {
  return typeof cause === "string" ? CAUSE_LABELS[cause] : UNDETERMINED_CAUSE_LABEL;
}

/** § 7 string 10; `null` when no row of the group needs follow-up. */
export function followUpFlagText(n: number): string | null {
  if (n === 0) return null;
  return n === 1 ? `${n} needs follow-up` : `${n} need follow-up`;
}

// --- Filter, search and pins (§ 6.3 item 3, § 6.4, § 6.11, § 6.12) ---

export type FilterValue = "all" | Outcome;

export interface ShowState {
  filter: FilterValue;
  query: string;
  pins: ReadonlySet<OccurrenceKey>;
}

export interface ShownGroup {
  id: GroupId;
  target?: Keyed<RecordOf<"default_target">>;
  branches: UnitBranch[];
  orphans: OrphanBlock[];
  reported: ReportedBlock[];
  /** § 6.3 item 3: shown unit rows and reported rows; 0 for the default target. */
  count: number;
  /** § 6.3 item 4: `null` unless the filter is "All" and N > 0. */
  followUp: number | null;
}

/** Joins a row's search fields; a newline keeps a match inside one field. */
const SEARCH_FIELD_SEPARATOR = "\n";

function searchText(fields: (string | null | undefined)[]): string {
  return fields
    .filter((f): f is string => typeof f === "string")
    .join(SEARCH_FIELD_SEPARATOR)
    .toLowerCase();
}

/** A unit row's fields of § 6.12, without its drop-ins' paths. */
function unitStateFields(row: ServiceDecisionDto): (string | null | undefined)[] {
  return [
    row.unit,
    row.kind,
    row.owning_package ?? row.fragment,
    row.current_state,
    row.default_state,
    row.instance_of,
    row.declined?.text,
    row.attention_reason,
  ];
}

/** An undetermined install's reason as its row shows it: `_` as a space. */
function reasonWords(reason: string): string {
  return reason.replace(/_/g, " ");
}

function isBranch(value: UnitBranch | Keyed): value is UnitBranch {
  return "head" in value;
}

/** Section search text (§ 6.12) of a branch or one row, lowercased. */
export function sectionSearchText(branchOrRow: UnitBranch | Keyed): string {
  if (isBranch(branchOrRow)) {
    const { head, dropins } = branchOrRow;
    return searchText([...unitStateFields(head.record.data), ...dropins.map((d) => d.record.data.path)]);
  }
  const record = branchOrRow.record;
  switch (record.kind) {
    case "unit_state":
      return searchText(unitStateFields(record.data));
    case "default_target":
      return searchText([record.data.target]);
    case "dropin":
      return searchText([record.data.path, record.data.unit]);
    case "manual_link":
      return searchText([record.data.unit, record.data.parent, record.data.relation, ...record.data.advisories]);
    case "undetermined_install":
      return searchText([record.data.unit, ...record.data.install_files, ...record.data.advisories]);
    case "mount_finding":
      return searchText([record.data.unit, ...record.data.advisories]);
  }
}

/** Global search text of one record (§ 6.12's table), lowercased. */
export function globalSearchText(record: UnitRecord): string {
  switch (record.kind) {
    case "unit_state":
      return searchText(unitStateFields(record.data));
    case "default_target":
      return searchText([record.data.target, "default target"]);
    case "dropin":
      return searchText([record.data.path, record.data.unit]);
    case "manual_link":
      return searchText([record.data.unit, record.data.parent, record.data.relation, ...record.data.advisories]);
    case "undetermined_install":
      return searchText([
        record.data.unit,
        reasonWords(record.data.reason),
        ...record.data.install_files,
        ...record.data.advisories,
      ]);
    case "mount_finding":
      return searchText([
        record.data.unit,
        record.data.kind,
        record.data.mount_point,
        ...record.data.advisories,
      ]);
  }
}

/**
 * One unit of visibility: a unit row with its drop-ins, the default
 * target, one orphan drop-in, or one reported row. `outcome` is `null` for
 * an orphan, which only "All" shows.
 */
interface VisibilityBranch {
  groupId: GroupId;
  head: OccurrenceKey;
  outcome: Outcome | null;
  text: string;
  /** The branch's rows in section order, head first. */
  rows: Keyed[];
}

/** A group's branches in section order: target, unit branches, orphans, reported. */
function visibilityBranches(group: UnitGroupModel): VisibilityBranch[] {
  const out: VisibilityBranch[] = [];
  const single = (row: Keyed, outcome: Outcome | null) =>
    out.push({ groupId: group.id, head: row.occurrenceKey, outcome, text: sectionSearchText(row), rows: [row] });
  if (group.target) single(group.target, outcomeOf(group.target.record));
  for (const branch of group.branches) {
    out.push({
      groupId: group.id,
      head: branch.head.occurrenceKey,
      outcome: outcomeOf(branch.head.record),
      text: sectionSearchText(branch),
      rows: [branch.head, ...branch.dropins],
    });
  }
  for (const block of group.orphans) for (const dropin of block.dropins) single(dropin, null);
  for (const block of group.reported) for (const row of block.rows) single(row, outcomeOf(row.record));
  return out;
}

/** A blank query searches nothing, as in the other sections. */
function failsQuery(branch: VisibilityBranch, query: string): boolean {
  if (query.trim() === "") return false;
  return !branch.text.includes(query.toLowerCase());
}

function failsFilter(branch: VisibilityBranch, filter: FilterValue): boolean {
  if (filter === "all") return false;
  return branch.outcome !== filter;
}

function isShown(branch: VisibilityBranch, state: ShowState): boolean {
  if (state.pins.has(branch.head)) return true;
  return !failsFilter(branch, state.filter) && !failsQuery(branch, state.query);
}

/**
 * The groups drawn under the filter, the query and the pins, each holding
 * only its shown rows, in place (§ 6.2, § 6.4). Undrawn groups are omitted.
 */
export function showGroups(groups: UnitGroupModel[], state: ShowState): ShownGroup[] {
  const out: ShownGroup[] = [];
  for (const group of groups) {
    const shown = new Set(
      visibilityBranches(group)
        .filter((b) => isShown(b, state))
        .map((b) => b.head),
    );
    const target = group.target && shown.has(group.target.occurrenceKey) ? group.target : undefined;
    const branches = group.branches.filter((b) => shown.has(b.head.occurrenceKey));
    const orphans = group.orphans
      .map((block) => ({ unit: block.unit, dropins: block.dropins.filter((d) => shown.has(d.occurrenceKey)) }))
      .filter((block) => block.dropins.length > 0);
    const reported = group.reported
      .map((block) => ({ unit: block.unit, rows: block.rows.filter((r) => shown.has(r.occurrenceKey)) }))
      .filter((block) => block.rows.length > 0);
    if (!target && branches.length === 0 && orphans.length === 0 && reported.length === 0) continue;
    out.push({
      id: group.id,
      ...(target ? { target } : {}),
      branches,
      orphans,
      reported,
      count: branches.length + reported.reduce((n, block) => n + block.rows.length, 0),
      followUp: state.filter === "all" && group.followUpTotal > 0 ? group.followUpTotal : null,
    });
  }
  return out;
}

/** `resultCount` (§ 6.12): unit rows, the target, orphan drop-ins, reported rows. */
export function shownRowCount(shown: ShownGroup[]): number {
  let n = 0;
  for (const group of shown) {
    if (group.target) n += 1;
    n += group.branches.length;
    for (const block of group.orphans) n += block.dropins.length;
    for (const block of group.reported) n += block.rows.length;
  }
  return n;
}

/**
 * The live region's n (§ 6.4 Announcement): the shown rows that have an
 * outcome, the rows the filter counts (unit rows, the target, reported
 * rows), so "Showing all n rows." agrees with "All n". Drop-ins, nested or
 * orphan, are not counted.
 */
export function shownOutcomeRowCount(shown: ShownGroup[]): number {
  let n = 0;
  for (const group of shown) {
    if (group.target) n += 1;
    n += group.branches.length;
    for (const block of group.reported) n += block.rows.length;
  }
  return n;
}

/** § 6.4 When it is drawn: only while a row needs follow-up or is excluded. */
export function filterBarDrawn(counts: OutcomeCounts): boolean {
  return counts.needs_follow_up + counts.excluded > 0;
}

/** The pin a toggle makes: a nested drop-in pins its parent's branch (§ 6.4). */
export function branchHeadOf(groups: UnitGroupModel[], toggled: OccurrenceKey): OccurrenceKey {
  for (const group of groups) {
    for (const branch of group.branches) {
      if (branch.dropins.some((d) => d.occurrenceKey === toggled)) return branch.head.occurrenceKey;
    }
  }
  return toggled;
}

export interface RowLocation {
  groupId: GroupId;
  branchHead: OccurrenceKey;
  occurrenceKey: OccurrenceKey;
}

/** The first row in section order carrying the row key (§ 6.11's reveal lookup). */
export function locateRow(groups: UnitGroupModel[], key: RowKey): RowLocation | null {
  for (const group of groups) {
    for (const branch of visibilityBranches(group)) {
      const row = branch.rows.find((r) => r.rowKey === key);
      if (row) return { groupId: group.id, branchHead: branch.head, occurrenceKey: row.occurrenceKey };
    }
  }
  return null;
}

function branchAt(groups: UnitGroupModel[], loc: RowLocation): VisibilityBranch | undefined {
  const group = groups.find((g) => g.id === loc.groupId);
  return group ? visibilityBranches(group).find((b) => b.head === loc.branchHead) : undefined;
}

/** Reveal step 1 (§ 6.11): the query hides the row's branch, and it is not pinned. */
export function hiddenByQuery(groups: UnitGroupModel[], loc: RowLocation, state: ShowState): boolean {
  const branch = branchAt(groups, loc);
  if (!branch || state.pins.has(branch.head)) return false;
  return failsQuery(branch, state.query);
}

/** Reveal step 2 (§ 6.11): the filter hides the row's branch, and it is not pinned. */
export function hiddenByFilter(groups: UnitGroupModel[], loc: RowLocation, state: ShowState): boolean {
  const branch = branchAt(groups, loc);
  if (!branch || state.pins.has(branch.head)) return false;
  return failsFilter(branch, state.filter);
}

/** § 7 strings 19 to 22, by filter value: [one row, n rows]. */
const LIVE_REGION_TEMPLATES: Record<FilterValue, [string, (n: number) => string]> = {
  all: ["Showing all 1 row.", (n) => `Showing all ${n} rows.`],
  needs_follow_up: ["Showing 1 row that needs follow-up.", (n) => `Showing ${n} rows that need follow-up.`],
  carried: ["Showing 1 carried row.", (n) => `Showing ${n} carried rows.`],
  excluded: ["Showing 1 excluded row.", (n) => `Showing ${n} excluded rows.`],
};

/** The live region's text after a filter change (§ 6.4). */
export function liveRegionText(filter: FilterValue, n: number): string {
  const [one, many] = LIVE_REGION_TEMPLATES[filter];
  return n === 1 ? one : many(n);
}
