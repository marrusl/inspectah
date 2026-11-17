/**
 * The units section's state, apart from its rendering (spec § 8, § 2 item
 * 12): the filter value, the live region text, the pins, collapse state,
 * the reveal's steps as applied, the roving tab stop and focus after a
 * toggle. It holds React state and calls `unitOutcome.ts` for every rule;
 * it decides none itself.
 *
 * Order matters. The state adjustments made during render run in this
 * order, each reading what the one before left: (1) the filter returns to
 * "All" when the bar stops being drawn; (2) a new view prunes the pins to
 * the occurrence keys it holds; (3) a query change clears every pin; (4)
 * the reveal applies its steps, reading the filter, query, pins and
 * collapse state as (1) to (3) leave them; (5) the tab stop moves to a
 * shown row. Keep them in this order.
 */
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { RefinementOp, ViewResponse } from "../api/types";
import { applyOp } from "../api/client";
import type { PendingGuard } from "./usePendingGuard";
import {
  GROUP_ORDER,
  branchHeadOf,
  emptyStateFor,
  filterBarDrawn,
  groupUnits,
  keyFields,
  liveRegionText,
  outcomeCounts,
  pinsInView,
  revealPlan,
  rovingTabStop,
  rowKeyForItemId,
  rowMoveTarget,
  showGroups,
  shownRowCount,
  shownRowOrder,
  type EmptyStates,
  type FilterValue,
  type GroupId,
  type KeyedFields,
  type OccurrenceKey,
  type OutcomeCounts,
  type ShownGroup,
  type UnitFields,
  type UnitGroupModel,
} from "../components/unitOutcome";

/** No section query. */
const NO_QUERY = "";

/** No pinned branch. */
const NO_PINS: ReadonlySet<OccurrenceKey> = new Set();

/** Hands a toggleable row's element to the section, by occurrence key. */
export type RowRef = (el: HTMLDivElement | null) => void;

/** Every group open. */
const NONE_COLLAPSED = Object.fromEntries(GROUP_ORDER.map((id) => [id, false])) as Record<
  GroupId,
  boolean
>;

export interface UnitsSectionStateInput {
  /** The six unit fields of the view response. */
  fields: UnitFields;
  /** The section query (§ 6.12), owned by `MainContent`; empty for none. */
  filterText?: string;
  /** The item a navigation is revealing (§ 6.11). */
  revealItemId?: string;
  /** Receives `shownRowCount` for the search box's result count (§ 6.12). */
  onResultCount?: (n: number) => void;
  /** Clears the section query: reveal step 1 (§ 6.11). */
  onClearQuery?: () => void;
  /** Receives the function that focuses the first shown row, and `null` on unmount. */
  registerFocusFirstRow?: (focusFirstRow: (() => void) | null) => void;
  /** Sends a toggle's op under its guard key. */
  runGuarded: PendingGuard["runGuarded"];
  onViewUpdate: (view: ViewResponse) => void;
  onMutationError: (err: Error) => void;
}

export interface UnitsSectionState {
  keyed: KeyedFields;
  counts: OutcomeCounts;
  /** Whether the outcome filter bar is drawn (§ 6.4). */
  barDrawn: boolean;
  filter: FilterValue;
  /** Changes the filter value, as the operator does: clears the pins and announces. */
  changeFilter: (value: FilterValue) => void;
  /** The live region's text (§ 6.4 Announcement). */
  announcement: string;
  /** The groups drawn, each holding its shown rows. */
  shown: ShownGroup[];
  /** § 6.9, decided by `emptyStateFor`. */
  emptyState: EmptyStates;
  collapsed: Record<GroupId, boolean>;
  toggleGroup: (id: GroupId) => void;
  /** The row holding the section's one tab stop (§ 6.13). */
  tabStop: OccurrenceKey | null;
  /** The ref callback for a row, by occurrence key. */
  rowRef: (key: OccurrenceKey) => RowRef;
  /** The section's `onFocus`: the tab stop follows focus onto a row. */
  handleFocus: (e: React.FocusEvent) => void;
  /** The section's `onKeyDown`: row movement keys (§ 6.13). */
  handleKeyDown: (e: React.KeyboardEvent) => void;
  /**
   * Sends a toggle's op under its guard key, pinning the branch that holds
   * the toggled row first (§ 6.4), and returns focus to that row after.
   */
  sendInclude: (key: string, op: RefinementOp, toggled: OccurrenceKey) => void;
}

/** The units section's state; `ServiceSection` renders what it returns. */
export function useUnitsSectionState({
  fields,
  filterText = NO_QUERY,
  revealItemId,
  onResultCount,
  onClearQuery,
  registerFocusFirstRow,
  runGuarded,
  onViewUpdate,
  onMutationError,
}: UnitsSectionStateInput): UnitsSectionState {
  const { unit_states, unit_dropins, default_target, mount_findings, manual_links, undetermined_installs } =
    fields;

  // Keys are assigned once per view response, then grouped (§ 6.11, § 6.2).
  const keyed = useMemo(
    () =>
      keyFields({
        unit_states,
        unit_dropins,
        default_target,
        mount_findings,
        manual_links,
        undetermined_installs,
      }),
    [unit_states, unit_dropins, default_target, mount_findings, manual_links, undetermined_installs],
  );
  const groups = useMemo(() => groupUnits(keyed), [keyed]);
  const counts = useMemo(() => outcomeCounts(keyed), [keyed]);
  const barDrawn = filterBarDrawn(counts);

  // The filter is "All" on mount and lives only here: not in the URL or
  // localStorage (§ 6.4 Persistence).
  const [filter, setFilter] = useState<FilterValue>("all");
  const [announcement, setAnnouncement] = useState("");
  // The bar stopped being drawn: the filter returns to "All", and the live
  // region starts empty when the bar returns. Adjusted during render, so
  // no frame shows a filter with no bar.
  if (!barDrawn && (filter !== "all" || announcement !== "")) {
    setFilter("all");
    setAnnouncement("");
  }

  // Pinned branches, by the occurrence key of the row heading each (§ 6.4,
  // § 12). They go with the section on unmount. The filter's return to
  // "All" above clears none: it follows a toggle.
  const [pins, setPins] = useState<ReadonlySet<OccurrenceKey>>(NO_PINS);
  // Each view keeps only the pins whose occurrence key it still holds.
  const [pinsCheckedFor, setPinsCheckedFor] = useState(keyed);
  if (pinsCheckedFor !== keyed) {
    setPinsCheckedFor(keyed);
    setPins((prev) => pinsInView(prev, keyed));
  }
  /** Clears every pin: a filter change, a query change or a reveal that changes either (§ 6.4). */
  const clearPins = useCallback(() => setPins(NO_PINS), []);

  // A query change clears every pin (§ 6.4), adjusted during render so no
  // frame shows the new query with the old pins.
  const [queryCheckedFor, setQueryCheckedFor] = useState(filterText);
  if (queryCheckedFor !== filterText) {
    setQueryCheckedFor(filterText);
    setPins(NO_PINS);
  }

  const shown = useMemo(
    () => showGroups(groups, { filter, query: filterText, pins }),
    [groups, filter, filterText, pins],
  );

  // The search box's result count (§ 6.12).
  useEffect(() => {
    onResultCount?.(shownRowCount(shown));
  }, [shown, onResultCount]);

  const changeFilter = useCallback(
    (value: FilterValue) => {
      setFilter(value);
      clearPins();
      const next = showGroups(groups, { filter: value, query: filterText, pins: NO_PINS });
      setAnnouncement(liveRegionText(value, shownRowCount(next)));
    },
    [groups, filterText, clearPins],
  );

  // Rows by occurrence key, so focus returns to the occurrence the operator
  // toggled (§ 6.13), never to a twin with its row key, and ArrowDown from
  // the search box reaches any kind of row.
  const rowElements = useRef(new Map<OccurrenceKey, HTMLDivElement>());
  const rowRef = useCallback(
    (key: OccurrenceKey): RowRef =>
      (el) => {
        if (el) rowElements.current.set(key, el);
        else rowElements.current.delete(key);
      },
    [],
  );
  const [focusRequest, setFocusRequest] = useState<{ key: OccurrenceKey } | null>(null);
  // After the op settles, the toggled row gets focus back if focus was lost
  // meanwhile (a browser blurs a control disabled while its request runs).
  // Focus the operator moved elsewhere stays where it is.
  useLayoutEffect(() => {
    if (!focusRequest) return;
    const active = document.activeElement;
    if (active === null || active === document.body) {
      rowElements.current.get(focusRequest.key)?.focus();
    }
  }, [focusRequest]);

  // Every group opens on mount; the state goes with the section (§ 6.2).
  const [collapsed, setCollapsed] = useState<Record<GroupId, boolean>>(NONE_COLLAPSED);
  const toggleGroup = useCallback(
    (id: GroupId) => setCollapsed((prev) => ({ ...prev, [id]: !prev[id] })),
    [],
  );

  // The reveal (§ 6.11): undo whatever hides the destination, adjusted
  // during render so the row is shown before App's reveal effect looks it
  // up. revealPlan decides all three steps. Rechecked when the view
  // changes, so a reveal sent before the view loaded still lands.
  const [revealChecked, setRevealChecked] = useState<{
    itemId: string | undefined;
    groups: UnitGroupModel[];
  }>({ itemId: undefined, groups });
  // Step 1 clears MainContent's query text, which may not be set while this
  // section renders, so it is handed over in a layout effect: still before
  // App's reveal looks the row up. Handled once, then reset.
  const [queryClearRequest, setQueryClearRequest] = useState<object | null>(null);
  if (revealChecked.itemId !== revealItemId || revealChecked.groups !== groups) {
    setRevealChecked({ itemId: revealItemId, groups });
    const key = revealItemId === undefined ? null : rowKeyForItemId(revealItemId);
    const plan =
      key === null ? null : revealPlan(groups, key, { filter, query: filterText, pins, collapsed });
    if (plan) {
      if (plan.clearQuery) setQueryClearRequest({});
      if (plan.filterToAll) {
        setFilter("all");
        const all = showGroups(groups, {
          filter: "all",
          query: plan.clearQuery ? NO_QUERY : filterText,
          pins: NO_PINS,
        });
        setAnnouncement(liveRegionText("all", shownRowCount(all)));
      }
      if (plan.clearPins) clearPins();
      const openGroup = plan.openGroup;
      if (openGroup !== null) setCollapsed((prev) => ({ ...prev, [openGroup]: false }));
    }
  }
  useLayoutEffect(() => {
    if (!queryClearRequest) return;
    setQueryClearRequest(null);
    onClearQuery?.();
  }, [queryClearRequest, onClearQuery]);

  // The shown rows in section order (§ 6.13): the tab stop, the movement
  // keys and ArrowDown from the search box all read it.
  const order = useMemo(() => shownRowOrder(shown, collapsed), [shown, collapsed]);

  // The one tab stop, held by occurrence key (§ 6.13). When the row holding
  // it stops being shown it moves to the first shown row, adjusted during
  // render so no frame shows a hidden tab stop.
  const [heldTabStop, setHeldTabStop] = useState<OccurrenceKey | null>(null);
  const tabStop = rovingTabStop(order, heldTabStop);
  if (tabStop !== heldTabStop) setHeldTabStop(tabStop);

  /** The occurrence key of a row element, read from the ref map: keys are never in the DOM. */
  const rowKeyOfElement = useCallback((el: EventTarget): OccurrenceKey | null => {
    for (const [key, row] of rowElements.current) {
      if (row === el) return key;
    }
    return null;
  }, []);

  // The tab stop follows focus onto a row, however it got there (a movement
  // key, a click, or App's reveal).
  const handleFocus = useCallback(
    (e: React.FocusEvent) => {
      const key = rowKeyOfElement(e.target);
      if (key !== null) setHeldTabStop(key);
    },
    [rowKeyOfElement],
  );

  // Movement keys act on a focused row only, as in DecisionList: never from
  // a checkbox, link or button inside one.
  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      const from = rowKeyOfElement(e.target);
      if (from === null) return;
      const to = rowMoveTarget(order, from, e.key);
      if (to === null) return;
      e.preventDefault();
      setHeldTabStop(to);
      rowElements.current.get(to)?.focus();
    },
    [rowKeyOfElement, order],
  );

  // ArrowDown from the search box (§ 6.12): the first shown row, of any kind.
  const focusFirstRow = useCallback(() => {
    const key = order[0];
    if (key !== undefined) rowElements.current.get(key)?.focus();
  }, [order]);
  useLayoutEffect(() => {
    if (!registerFocusFirstRow) return;
    registerFocusFirstRow(focusFirstRow);
    return () => registerFocusFirstRow(null);
  }, [registerFocusFirstRow, focusFirstRow]);

  // Every toggle the operator makes pins the branch that holds the row
  // (§ 6.4), before the op leaves, so a failed op leaves it shown too.
  const sendInclude = useCallback(
    (key: string, op: RefinementOp, toggled: OccurrenceKey) => {
      const head = branchHeadOf(groups, toggled);
      setPins((prev) => (prev.has(head) ? prev : new Set(prev).add(head)));
      runGuarded(key, () =>
        applyOp(op)
          .then(onViewUpdate)
          .catch((err) =>
            onMutationError(
              err instanceof Error ? err : new Error(String(err)),
            ),
          )
          .finally(() => setFocusRequest({ key: toggled })),
      );
    },
    [groups, runGuarded, onViewUpdate, onMutationError],
  );

  // § 6.9, decided by emptyStateFor.
  const emptyState = emptyStateFor(keyed, filter, filterText, shown);

  return {
    keyed,
    counts,
    barDrawn,
    filter,
    changeFilter,
    announcement,
    shown,
    emptyState,
    collapsed,
    toggleGroup,
    tabStop,
    rowRef,
    handleFocus,
    handleKeyDown,
    sendInclude,
  };
}
