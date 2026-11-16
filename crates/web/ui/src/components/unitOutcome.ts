/**
 * The units section's rules, decided without React: row keys and
 * occurrence keys (spec § 6.11). Nothing else spells a key.
 */
import type {
  DefaultTargetView,
  DropInDecisionDto,
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
    return { record, rowKey: key, occurrenceKey: `${key}#${ordinal}` };
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
