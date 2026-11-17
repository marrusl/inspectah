/**
 * Where focus goes when the control holding it is removed (spec § 6.9
 * rows 4 and 5, § 6.13), decided on the model.
 */
import { describe, it, expect } from "vitest";
import {
  focusAfterRemoval,
  groupUnits,
  keyFields,
  rovingTabStop,
  showGroups,
  shownRowOrder,
  type FocusLoss,
  type FocusRemoval,
  type GroupId,
} from "../unitOutcome";
import { loadUnitFields } from "../../__tests__/fixtures/systemd-units/load";

const kitchenSink = loadUnitFields("kitchen-sink");
const groups = groupUnits(keyFields(kitchenSink));
const shown = showGroups(groups, { filter: "all", query: "", pins: new Set() });
const NONE_COLLAPSED = {} as Record<GroupId, boolean>;
const order = shownRowOrder(shown, NONE_COLLAPSED);

function removal(loss: FocusLoss, over: Partial<FocusRemoval> = {}): FocusRemoval {
  return {
    loss,
    focusInside: true,
    barDrawn: true,
    order,
    tabStop: rovingTabStop(order, order[3]),
    shown,
    ...over,
  };
}

describe("focus after a removal (§ 6.9 rows 4 and 5, § 6.13)", () => {
  it("leaves focus where it is when it was not inside what was removed", () => {
    for (const loss of ["clear_query", "show_all", "row_or_bar"] as const) {
      expect(focusAfterRemoval(removal(loss, { focusInside: false })), loss).toBeNull();
    }
  });

  it("sends Clear filter's focus to the section search box", () => {
    expect(focusAfterRemoval(removal("clear_query"))).toEqual({ kind: "search" });
  });

  it("sends Show all's focus to the bar's All button while the bar is drawn", () => {
    expect(focusAfterRemoval(removal("show_all"))).toEqual({ kind: "filter_all" });
  });

  it("sends Show all's focus to the first shown row with no bar, not the tab stop", () => {
    expect(order[3]).not.toBe(order[0]);
    expect(focusAfterRemoval(removal("show_all", { barDrawn: false }))).toEqual({ kind: "row", key: order[0] });
  });

  it("hands a removed row's or bar's focus to the row holding the tab stop", () => {
    expect(focusAfterRemoval(removal("row_or_bar"))).toEqual({ kind: "row", key: order[3] });
  });

  it("hands it to the first group header when no row holds the tab stop", () => {
    const all = Object.fromEntries(shown.map((g) => [g.id, true])) as Record<GroupId, boolean>;
    const none = shownRowOrder(shown, all);
    expect(none).toEqual([]);
    const target = focusAfterRemoval(
      removal("row_or_bar", { order: none, tabStop: rovingTabStop(none, order[3]) }),
    );
    expect(target).toEqual({ kind: "group_header", groupId: shown[0].id });
    expect(
      focusAfterRemoval(removal("show_all", { barDrawn: false, order: none, tabStop: null })),
    ).toEqual({ kind: "group_header", groupId: shown[0].id });
  });

  it("has nowhere to send focus when no group is drawn", () => {
    expect(focusAfterRemoval(removal("row_or_bar", { order: [], tabStop: null, shown: [] }))).toBeNull();
  });
});
