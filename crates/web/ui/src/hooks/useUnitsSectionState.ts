/**
 * The units section's state, apart from its rendering (spec § 8, § 2 item
 * 12): the filter value, the live region text, the pins, collapse state,
 * the reveal's steps as applied, the roving tab stop and focus after a
 * toggle. It holds React state and calls `unitOutcome.ts` for every rule;
 * it decides none itself.
 *
 * Order matters. The state adjustments made during render run in this
 * order, each reading the effective values the one before left, not the
 * stored state the same pass is replacing: (1) the filter returns to
 * "All" when the bar stops being drawn; (2) a new view prunes the pins to
 * the occurrence keys it holds; (3) a query change clears every pin; (4)
 * the reveal applies its steps, reading the filter, query, pins and
 * collapse state as (1) to (3) leave them. The query it reads is the one
 * after a global search navigation's reset: `MainContent` clears its
 * text during its own render, before this section renders with the new
 * reveal. (5) The tab stop moves to a shown row of the order computed
 * from those same effective values. Keep them in this order.
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
} from "../components/unitOutcome";

/** No section query. */
const NO_QUERY = "";

/** No pinned branch. */
const NO_PINS: ReadonlySet<OccurrenceKey> = new Set();

/** Hands a toggleable row's element to the section, by occurrence key. */
export type RowRef = (el: HTMLDivElement | null) => void;

/**
 * A reveal as the section has taken it (§ 6.11): the item id, the query
 * when it arrived, and whether it still waits for a view holding its row.
 */
interface RevealProgress {
  itemId: string | undefined;
  query: string;
  pending: boolean;
}

/** No reveal. */
const NO_REVEAL: RevealProgress = { itemId: undefined, query: NO_QUERY, pending: false };

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
  // Pinned branches, by the occurrence key of the row heading each (§ 6.4,
  // § 12). They go with the section on unmount.
  const [pins, setPins] = useState<ReadonlySet<OccurrenceKey>>(NO_PINS);
  const [pinsCheckedFor, setPinsCheckedFor] = useState(keyed);
  const [queryCheckedFor, setQueryCheckedFor] = useState(filterText);
  // Every group opens on mount; the state goes with the section (§ 6.2).
  const [collapsed, setCollapsed] = useState<Record<GroupId, boolean>>(NONE_COLLAPSED);
  const [reveal, setReveal] = useState<RevealProgress>(NO_REVEAL);
  // Reveal step 1 clears MainContent's query text, which may not be set
  // while this section renders, so it is handed over in a layout effect:
  // still before App's reveal looks the row up. Handled once, then reset.
  const [queryClearRequest, setQueryClearRequest] = useState<object | null>(null);

  // (1) The bar stopped being drawn: the filter returns to "All", and the
  // live region starts empty when the bar returns. Clears no pin: it
  // follows a toggle.
  let effectiveFilter = filter;
  let effectiveAnnouncement = announcement;
  if (!barDrawn) {
    effectiveFilter = "all";
    effectiveAnnouncement = "";
  }
  // (2) Each view keeps only the pins whose occurrence key it still holds.
  let effectivePins = pins;
  if (pinsCheckedFor !== keyed) {
    setPinsCheckedFor(keyed);
    effectivePins = pinsInView(effectivePins, keyed);
  }
  // (3) A query change clears every pin (§ 6.4).
  if (queryCheckedFor !== filterText) {
    setQueryCheckedFor(filterText);
    effectivePins = NO_PINS;
  }
  // A query clear already asked for is the query in effect.
  let effectiveQuery = queryClearRequest ? NO_QUERY : filterText;
  let effectiveCollapsed = collapsed;

  // (4) The reveal (§ 6.11): undo whatever hides the destination, so the
  // row is shown before App's reveal effect looks it up. revealPlan
  // decides all three steps, against the values (1) to (3) leave. A new
  // reveal is pending until a view holds its row; it is then planned and
  // applied once, and never again on a later view. A pending reveal is
  // dropped when the operator changes the query, so it never clears a
  // query typed after it.
  let nextReveal = reveal;
  if (reveal.itemId !== revealItemId) {
    nextReveal = { itemId: revealItemId, query: filterText, pending: revealItemId !== undefined };
  } else if (reveal.pending && reveal.query !== filterText) {
    nextReveal = { ...reveal, pending: false };
  }
  if (nextReveal.pending && nextReveal.itemId !== undefined) {
    const key = rowKeyForItemId(nextReveal.itemId);
    const plan =
      key === null
        ? null
        : revealPlan(groups, key, {
            filter: effectiveFilter,
            query: effectiveQuery,
            pins: effectivePins,
            collapsed: effectiveCollapsed,
          });
    // An id that is no row key names no row in any view.
    if (key === null) nextReveal = { ...nextReveal, pending: false };
    if (plan) {
      nextReveal = { ...nextReveal, pending: false };
      if (plan.clearQuery) {
        setQueryClearRequest({});
        effectiveQuery = NO_QUERY;
      }
      if (plan.clearPins) effectivePins = NO_PINS;
      if (plan.filterToAll) {
        effectiveFilter = "all";
        const all = showGroups(groups, { filter: "all", query: effectiveQuery, pins: NO_PINS });
        effectiveAnnouncement = liveRegionText("all", shownRowCount(all));
      }
      const openGroup = plan.openGroup;
      if (openGroup !== null) effectiveCollapsed = { ...effectiveCollapsed, [openGroup]: false };
    }
  }
  if (nextReveal !== reveal) setReveal(nextReveal);

  // The adjustments above, stored during render so no frame shows a filter
  // with no bar, a new query with the old pins, or a hidden destination.
  if (effectiveFilter !== filter) setFilter(effectiveFilter);
  if (effectiveAnnouncement !== announcement) setAnnouncement(effectiveAnnouncement);
  if (effectivePins !== pins) setPins(effectivePins);
  if (effectiveCollapsed !== collapsed) setCollapsed(effectiveCollapsed);

  /** Clears every pin: a filter change (§ 6.4). */
  const clearPins = useCallback(() => setPins(NO_PINS), []);

  // Everything below reads the effective values, never the stored state
  // this pass has just replaced, so nothing is stored from a discarded one.
  const shown = useMemo(
    () => showGroups(groups, { filter: effectiveFilter, query: effectiveQuery, pins: effectivePins }),
    [groups, effectiveFilter, effectiveQuery, effectivePins],
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

  const toggleGroup = useCallback(
    (id: GroupId) => setCollapsed((prev) => ({ ...prev, [id]: !prev[id] })),
    [],
  );

  useLayoutEffect(() => {
    if (!queryClearRequest) return;
    setQueryClearRequest(null);
    onClearQuery?.();
  }, [queryClearRequest, onClearQuery]);

  // The shown rows in section order (§ 6.13): the tab stop, the movement
  // keys and ArrowDown from the search box all read it.
  const order = useMemo(() => shownRowOrder(shown, effectiveCollapsed), [shown, effectiveCollapsed]);

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
  const emptyState = emptyStateFor(keyed, effectiveFilter, effectiveQuery, shown);

  return {
    keyed,
    counts,
    barDrawn,
    filter: effectiveFilter,
    changeFilter,
    announcement: effectiveAnnouncement,
    shown,
    emptyState,
    collapsed: effectiveCollapsed,
    toggleGroup,
    tabStop,
    rowRef,
    handleFocus,
    handleKeyDown,
    sendInclude,
  };
}
