/**
 * A toggle never hides what the operator acted on (spec § 6.4 pinning,
 * § 6.13 Focus after a toggle). U8 (a), (b), (d), (e) for a filter change
 * and the bar ceasing to be drawn; two branches with one row key; a pinned
 * record leaving the view; a failed op.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { useState } from "react";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ServiceSectionWithGuard as ServiceSection } from "../../test-utils/withPendingGuard";
import type { ViewResponse } from "../../api/types";
import { emptyView } from "../../test-utils/emptyView";
import type { UnitFields } from "../unitOutcome";
import {
  loadPlantedAfterInclude,
  loadUnitFields,
} from "../../__tests__/fixtures/systemd-units/load";

const mockFetch = vi.fn();
beforeEach(() => {
  mockFetch.mockReset();
  vi.stubGlobal("fetch", mockFetch);
});
afterEach(() => {
  vi.restoreAllMocks();
});

const INSTALL_DROPIN = "/etc/systemd/system/install-dropin-demo.service.d/install.conf";

/** Sends a new view to the section, as an Undo or a refresh would. */
let pushView: (fields: UnitFields) => void = () => {
  throw new Error("Harness not rendered");
};

function fieldsOf(view: ViewResponse): UnitFields {
  return {
    unit_states: view.unit_states,
    unit_dropins: view.unit_dropins,
    default_target: view.default_target,
    mount_findings: view.mount_findings,
    manual_links: view.manual_links,
    undetermined_installs: view.undetermined_installs,
  };
}

/** The section's owner: it shows each view the section hands up. */
function Harness({ initial, onMutationError }: { initial: UnitFields; onMutationError: (e: Error) => void }) {
  const [fields, setFields] = useState(initial);
  pushView = setFields;
  return (
    <ServiceSection
      services={fields.unit_states}
      dropins={fields.unit_dropins}
      mountFindings={fields.mount_findings}
      manualLinks={fields.manual_links}
      undeterminedInstalls={fields.undetermined_installs}
      defaultTarget={fields.default_target}
      onViewUpdate={(view) => setFields(fieldsOf(view))}
      onMutationError={onMutationError}
    />
  );
}

function renderUnits(fields: UnitFields) {
  const onMutationError = vi.fn();
  render(<Harness initial={fields} onMutationError={onMutationError} />);
  return { onMutationError };
}

/** The next op returns this view. */
function respondWith(fields: UnitFields) {
  mockFetch.mockResolvedValueOnce({
    ok: true,
    json: () => Promise.resolve(emptyView({ ...fields })),
  });
}

/** The next op's response is held until the returned function is called. */
function respondLater(fields: UnitFields): () => Promise<void> {
  let resolve!: (value: unknown) => void;
  mockFetch.mockReturnValueOnce(new Promise((r) => (resolve = r)));
  return async () => {
    await act(async () => {
      resolve({ ok: true, json: () => Promise.resolve(emptyView({ ...fields })) });
    });
  };
}

function bar(): HTMLElement {
  return screen.getByRole("group", { name: "Show" });
}

async function select(label: string) {
  await userEvent.click(within(bar()).getByRole("button", { name: new RegExp(`^${label} \\d+$`) }));
}

/** A filter value's count, read from its button. */
function count(label: string): number {
  const button = within(bar()).getByRole("button", { name: new RegExp(`^${label} \\d+$`) });
  return Number((button.textContent ?? "").replace(/^\D+/, ""));
}

function header(title: string): HTMLElement {
  const titleEl = Array.from(document.querySelectorAll('[id^="unit-group-title-"]')).find(
    (el) => el.textContent === title,
  );
  expect(titleEl, `no group titled ${title}`).toBeDefined();
  return titleEl!.closest("button") as HTMLElement;
}

function groupGrid(title: string): HTMLElement {
  return document.getElementById(header(title).getAttribute("aria-controls")!) as HTMLElement;
}

function headerCount(title: string): number {
  return Number(header(title).querySelector(".pf-v6-c-badge")!.textContent);
}

function unitRows(title: string): string[] {
  return Array.from(
    groupGrid(title).querySelectorAll<HTMLElement>('[data-testid^="service-item-"]'),
  ).map((r) => r.getAttribute("aria-label") ?? "");
}

/** The outcome labels in a row's badge cells. */
function outcomeLabels(row: HTMLElement): string[] {
  return Array.from(row.querySelectorAll(".pf-v6-c-label"))
    .map((l) => l.textContent ?? "")
    .filter((t) => ["FIXME", "Advisory", "Omitted"].includes(t));
}

function withUnit(fields: UnitFields, unit: string, change: (row: UnitFields["unit_states"][number]) => void) {
  const row = fields.unit_states.find((r) => r.unit === unit);
  expect(row, `no unit row ${unit}`).toBeDefined();
  change(row!);
  return fields;
}

/** Kitchen-sink after excluding `cups.service`: its declined cleared. */
function cupsExcluded(): UnitFields {
  return withUnit(loadUnitFields("kitchen-sink"), "cups.service", (r) => {
    delete r.declined;
    r.include = false;
  });
}

/** Under "Needs follow-up", toggles cups.service; the view excludes it. */
async function toggleCupsUnderFollowUp() {
  const result = renderUnits(loadUnitFields("kitchen-sink"));
  await select("Needs follow-up");
  respondWith(cupsExcluded());
  await userEvent.click(screen.getByRole("checkbox", { name: "Toggle cups.service" }));
  await waitFor(() =>
    expect(screen.getByRole("checkbox", { name: "Toggle cups.service" })).not.toBeChecked(),
  );
  return result;
}

describe("U8 (a): toggling a row under a filter keeps it in place", () => {
  it("keeps cups.service in place and focused, without its outcome label, with counts updated", async () => {
    renderUnits(loadUnitFields("kitchen-sink"));
    await select("Needs follow-up");
    const group = screen.getByTestId("service-group-cups.service");
    const before = group.previousElementSibling;
    const after = group.nextElementSibling;
    // cups.service is the first Services row under this value.
    expect(before).toBeNull();
    expect(after).not.toBeNull();
    const row = screen.getByTestId("service-item-cups.service");
    expect(outcomeLabels(row)).toEqual(["Advisory"]);

    respondWith(cupsExcluded());
    await userEvent.click(within(row).getByRole("checkbox", { name: "Toggle cups.service" }));
    await waitFor(() => expect(count("Excluded")).toBe(1));

    const groupNow = screen.getByTestId("service-group-cups.service");
    expect(groupNow.previousElementSibling).toBe(before);
    expect(groupNow.nextElementSibling).toBe(after);
    const rowNow = screen.getByTestId("service-item-cups.service");
    expect(rowNow.contains(document.activeElement)).toBe(true);
    expect(outcomeLabels(rowNow)).toEqual([]);
    expect(count("Needs follow-up")).toBe(8);
    expect(count("Excluded")).toBe(1);
  });

  it("counts the pinned row in its group, and the count follows the next view", async () => {
    await toggleCupsUnderFollowUp();
    expect(unitRows("Services")).toContain("cups.service");
    expect(headerCount("Services")).toBe(unitRows("Services").length);
    expect(headerCount("Services")).toBe(5);

    // An Undo clears another Services row's declined, with no toggle on it.
    act(() => pushView(withUnit(cupsExcluded(), "iscsid.service", (r) => delete r.declined)));
    expect(unitRows("Services")).toContain("cups.service");
    expect(unitRows("Services")).not.toContain("iscsid.service");
    expect(headerCount("Services")).toBe(unitRows("Services").length);
    expect(headerCount("Services")).toBe(4);
  });
});

describe("U8 (b): including a parent's [Install] drop-in under a filter", () => {
  it("keeps the parent's branch in place, the drop-in included and focused", async () => {
    renderUnits(loadUnitFields("planted"));
    await select("Needs follow-up");
    const followUp = count("Needs follow-up");
    const carried = count("Carried");
    const branch = screen.getByTestId("service-group-install-dropin-demo.service");
    const before = branch.previousElementSibling;
    const after = branch.nextElementSibling;
    expect(outcomeLabels(screen.getByTestId("service-item-install-dropin-demo.service"))).toEqual([
      "Omitted",
    ]);

    respondWith(loadPlantedAfterInclude());
    const dropin = screen.getByTestId(`dropin-item-${INSTALL_DROPIN}`);
    dropin.focus();
    await userEvent.keyboard("x");
    await waitFor(() => expect(count("Needs follow-up")).toBe(followUp - 1));

    const branchNow = screen.getByTestId("service-group-install-dropin-demo.service");
    expect(branchNow.previousElementSibling).toBe(before);
    expect(branchNow.nextElementSibling).toBe(after);
    const dropinNow = screen.getByTestId(`dropin-item-${INSTALL_DROPIN}`);
    expect(within(dropinNow).getByRole("checkbox", { name: `Toggle ${INSTALL_DROPIN}` })).toBeChecked();
    expect(document.activeElement).toBe(dropinNow);
    expect(outcomeLabels(screen.getByTestId("service-item-install-dropin-demo.service"))).toEqual([]);
    expect(count("Carried")).toBe(carried + 1);
  });
});

describe("pins accumulate while the operator works (§ 6.4)", () => {
  it("keeps the first toggled row shown after a second toggle", async () => {
    await toggleCupsUnderFollowUp();
    // The second toggle excludes iscsid.service too; cups.service stays excluded.
    respondWith(
      withUnit(cupsExcluded(), "iscsid.service", (r) => {
        delete r.declined;
        r.include = false;
      }),
    );
    await userEvent.click(screen.getByRole("checkbox", { name: "Toggle iscsid.service" }));
    await waitFor(() =>
      expect(screen.getByRole("checkbox", { name: "Toggle iscsid.service" })).not.toBeChecked(),
    );
    expect(count("Excluded")).toBe(2);
    expect(unitRows("Services")).toContain("cups.service");
    expect(unitRows("Services")).toContain("iscsid.service");
  });
});

describe("U8 (d): a row changed by an Undo follows the filter", () => {
  it("leaves Needs follow-up when its declined clears with no toggle on its branch", async () => {
    await toggleCupsUnderFollowUp();
    expect(unitRows("Services")).toContain("iscsid.service");
    act(() => pushView(withUnit(cupsExcluded(), "iscsid.service", (r) => delete r.declined)));
    expect(unitRows("Services")).not.toContain("iscsid.service");
    expect(unitRows("Services")).toContain("cups.service");
    expect(count("Needs follow-up")).toBe(7);
  });
});

describe("U8 (e): what clears pins", () => {
  it("a filter change drops the pin", async () => {
    await toggleCupsUnderFollowUp();
    expect(unitRows("Services")).toContain("cups.service");
    await select("Carried");
    await select("Needs follow-up");
    expect(screen.queryByTestId("service-item-cups.service")).not.toBeInTheDocument();
  });

  it("the bar ceasing to be drawn returns the filter to All and keeps the section working", async () => {
    await toggleCupsUnderFollowUp();
    // Nothing needs follow-up and nothing is excluded: no bar.
    const quiet = loadUnitFields("kitchen-sink");
    for (const row of quiet.unit_states) delete row.declined;
    quiet.manual_links = [];
    quiet.undetermined_installs = [];
    act(() => pushView(quiet));
    expect(screen.queryByRole("group", { name: "Show" })).toBeNull();
    expect(screen.getByTestId("service-item-cups.service")).toBeInTheDocument();
    // The bar returns, on "All": every row shown, cups.service among them.
    act(() => pushView(cupsExcluded()));
    expect(
      within(bar())
        .getAllByRole("button")
        .find((b) => b.getAttribute("aria-pressed") === "true")!.textContent,
    ).toMatch(/^All /);
    expect(screen.getByTestId("service-item-cups.service")).toBeInTheDocument();
  });
});

describe("two branches with one row key", () => {
  it("toggling the second dup-state-demo.service pins only the second", async () => {
    renderUnits(loadUnitFields("planted"));
    await select("Carried");
    const rowsBefore = screen.getAllByTestId("service-item-dup-state-demo.service");
    expect(rowsBefore).toHaveLength(2);

    // The view after the toggle has both records excluded: only the pinned
    // occurrence may stay under "Carried".
    const after = loadUnitFields("planted");
    for (const r of after.unit_states.filter((u) => u.unit === "dup-state-demo.service")) {
      r.include = false;
    }
    respondWith(after);
    await userEvent.click(
      within(rowsBefore[1]).getByRole("checkbox", { name: "Toggle dup-state-demo.service" }),
    );
    await waitFor(() =>
      expect(screen.getAllByTestId("service-item-dup-state-demo.service")).toHaveLength(1),
    );
    const left = screen.getByTestId("service-item-dup-state-demo.service");
    expect(left).toBe(rowsBefore[1]);
    expect(left.contains(document.activeElement)).toBe(true);
  });
});

describe("a pinned record that leaves the view", () => {
  it("drops the pin, and the record coming back follows the filter", async () => {
    await toggleCupsUnderFollowUp();
    const without = cupsExcluded();
    without.unit_states = without.unit_states.filter((r) => r.unit !== "cups.service");
    expect(() => act(() => pushView(without))).not.toThrow();
    expect(screen.queryByTestId("service-item-cups.service")).not.toBeInTheDocument();
    // Its occurrence key is back, but its pin is gone: excluded, it is hidden.
    act(() => pushView(cupsExcluded()));
    expect(screen.queryByTestId("service-item-cups.service")).not.toBeInTheDocument();
    expect(count("Excluded")).toBe(1);
  });
});

describe("focus after a toggle (§ 6.13)", () => {
  it("returns focus to the toggled row when it was lost while the op ran", async () => {
    renderUnits(loadUnitFields("kitchen-sink"));
    await select("Needs follow-up");
    const finish = respondLater(cupsExcluded());
    const row = screen.getByTestId("service-item-cups.service");
    row.focus();
    await userEvent.keyboard(" ");
    // A browser moves focus to the body when the focused control is
    // disabled while its request runs.
    (document.activeElement as HTMLElement).blur();
    expect(document.activeElement).toBe(document.body);
    await finish();
    expect(document.activeElement).toBe(screen.getByTestId("service-item-cups.service"));
  });

  it("leaves focus alone when the operator moved it elsewhere", async () => {
    renderUnits(loadUnitFields("kitchen-sink"));
    await select("Needs follow-up");
    const finish = respondLater(cupsExcluded());
    screen.getByTestId("service-item-cups.service").focus();
    await userEvent.keyboard(" ");
    const other = screen.getByTestId("service-item-iscsid.service");
    other.focus();
    await finish();
    expect(document.activeElement).toBe(other);
  });
});

describe("a failed op", () => {
  it("keeps the branch shown and focused, the counts unchanged, and reports the failure", async () => {
    const { onMutationError } = renderUnits(loadUnitFields("kitchen-sink"));
    await select("Needs follow-up");
    let reject!: (err: Error) => void;
    mockFetch.mockReturnValueOnce(new Promise((_, r) => (reject = r)));
    const row = screen.getByTestId("service-item-cups.service");
    row.focus();
    await userEvent.keyboard(" ");
    expect(mockFetch).toHaveBeenCalledTimes(1);
    // Focus lost while the op ran, as a browser may do.
    row.blur();
    expect(document.activeElement).toBe(document.body);
    await act(async () => reject(new Error("network down")));

    expect(onMutationError).toHaveBeenCalledWith(new Error("network down"));
    const rowNow = screen.getByTestId("service-item-cups.service");
    expect(rowNow).toBe(row);
    expect(document.activeElement).toBe(rowNow);
    expect(count("Needs follow-up")).toBe(9);
    expect(count("Excluded")).toBe(0);
    expect(within(rowNow).getByRole("checkbox", { name: "Toggle cups.service" })).toBeChecked();
  });
});
