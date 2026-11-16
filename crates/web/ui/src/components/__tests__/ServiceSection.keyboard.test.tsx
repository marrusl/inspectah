/**
 * The units section's keyboard (spec § 6.13): one roving tab stop held by
 * occurrence key, row movement across groups as DecisionList's keys, and
 * Space or `x` on a toggleable row. U32 (movement, roving) and U36.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { useState } from "react";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ServiceSectionWithGuard as ServiceSection } from "../../test-utils/withPendingGuard";
import type { DropInDecisionDto, ViewResponse } from "../../api/types";
import { emptyView } from "../../test-utils/emptyView";
import type { UnitFields } from "../unitOutcome";
import { loadUnitFields } from "../../__tests__/fixtures/systemd-units/load";

const mockFetch = vi.fn();
beforeEach(() => {
  mockFetch.mockReset();
  vi.stubGlobal("fetch", mockFetch);
});
afterEach(() => {
  vi.restoreAllMocks();
});

const HTTPD_OVERRIDE = "/etc/systemd/system/httpd.service.d/override.conf";
const DUP_STATE_DROPIN = "/etc/systemd/system/dup-state-demo.service.d/override.conf";
const linkTestId = (unit: string) => `manual-link-${JSON.stringify(["wants", "multi-user.target", unit])}`;

/** Sends a new view to the section, as an Undo or a refresh would. */
let pushView: (fields: UnitFields) => void = () => {
  throw new Error("Harness not rendered");
};

/** Sets the section query, as typing in the search box does. */
let setQuery: (query: string) => void = () => {
  throw new Error("Harness not rendered");
};

/** The function the section registers for ArrowDown from the search box (§ 6.12). */
let focusFirstRow: (() => void) | null = null;

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

const viewUpdates = vi.fn();

/** The section's owner: it shows each view the section hands up, and owns the query text. */
function Harness({ initial }: { initial: UnitFields }) {
  const [fields, setFields] = useState(initial);
  const [query, setQueryState] = useState("");
  pushView = setFields;
  setQuery = setQueryState;
  return (
    <ServiceSection
      services={fields.unit_states}
      dropins={fields.unit_dropins}
      mountFindings={fields.mount_findings}
      manualLinks={fields.manual_links}
      undeterminedInstalls={fields.undetermined_installs}
      defaultTarget={fields.default_target}
      filterText={query}
      onClearQuery={() => setQueryState("")}
      registerFocusFirstRow={(f) => (focusFirstRow = f)}
      onNavigateToItem={vi.fn()}
      onNavigateToConfig={vi.fn()}
      configPaths={new Set(["/etc/wireguard/wg0.conf"])}
      onViewUpdate={(view) => {
        viewUpdates();
        setFields(fieldsOf(view));
      }}
      onMutationError={vi.fn()}
    />
  );
}

function renderUnits(fields: UnitFields) {
  viewUpdates.mockReset();
  render(<Harness initial={fields} />);
}

/** The next op returns this view. */
function respondWith(fields: UnitFields) {
  mockFetch.mockResolvedValueOnce({
    ok: true,
    json: () => Promise.resolve(emptyView({ ...fields })),
  });
}

function rows(): HTMLElement[] {
  return Array.from(screen.getByTestId("service-section").querySelectorAll<HTMLElement>('[role="row"]'));
}

const isShown = (el: HTMLElement) => el.closest("[hidden]") === null;

/** The section's one tab stop, checked to be one and shown. */
function theStop(): HTMLElement {
  const stops = rows().filter((r) => r.tabIndex === 0);
  expect(stops, "rows with tabIndex 0").toHaveLength(1);
  expect(isShown(stops[0]), "the tab stop is shown").toBe(true);
  return stops[0];
}

/** The first shown row in document order. */
function firstShownRowEl(): HTMLElement {
  const first = rows().find(isShown);
  expect(first, "no row is shown").toBeDefined();
  return first!;
}

/** After a move: one tab stop, shown, and it is the focused element. */
function expectFocusedStop(row: HTMLElement) {
  expect(document.activeElement).toBe(row);
  expect(theStop()).toBe(row);
}

function header(title: string): HTMLElement {
  const titleEl = Array.from(document.querySelectorAll('[id^="unit-group-title-"]')).find(
    (el) => el.textContent === title,
  );
  expect(titleEl, `no group titled ${title}`).toBeDefined();
  return titleEl!.closest("button") as HTMLElement;
}

async function select(label: string) {
  const bar = screen.getByRole("group", { name: "Show" });
  await userEvent.click(within(bar).getByRole("button", { name: new RegExp(`^${label} \\d+$`) }));
}

/** The body of the op sent, parsed. */
function sentOp(call: number): unknown {
  return JSON.parse(mockFetch.mock.calls[call][1].body as string);
}

describe("U32: row movement (§ 6.13)", () => {
  it("j and ArrowDown move to the next shown row, across groups", async () => {
    renderUnits(loadUnitFields("planted"));
    act(() => screen.getByTestId("default-target").focus());
    await userEvent.keyboard("j");
    expectFocusedStop(screen.getByTestId("service-item-bluetooth.service"));
    await userEvent.keyboard("{ArrowDown}");
    expectFocusedStop(screen.getByTestId("service-item-cups.service"));
  });

  it("k and ArrowUp move to the previous shown row, across groups", async () => {
    renderUnits(loadUnitFields("planted"));
    act(() => screen.getByTestId("service-item-cups.service").focus());
    await userEvent.keyboard("k");
    expectFocusedStop(screen.getByTestId("service-item-bluetooth.service"));
    await userEvent.keyboard("{ArrowUp}");
    expectFocusedStop(screen.getByTestId("default-target"));
  });

  it("skips a collapsed group", async () => {
    renderUnits(loadUnitFields("planted"));
    await userEvent.click(header("Services"));
    act(() => screen.getByTestId("default-target").focus());
    await userEvent.keyboard("j");
    expectFocusedStop(screen.getByTestId("service-item-cockpit.socket"));
    await userEvent.keyboard("k");
    expectFocusedStop(screen.getByTestId("default-target"));
  });

  it("visits a unit row's drop-ins after it", async () => {
    renderUnits(loadUnitFields("planted"));
    const httpd = screen.getByTestId("service-item-httpd.service");
    act(() => httpd.focus());
    await userEvent.keyboard("j");
    const first = document.activeElement as HTMLElement;
    expect(first.getAttribute("data-testid")).toMatch(/^dropin-item-\/etc\/systemd\/system\/httpd\.service\.d\//);
    expectFocusedStop(first);
    await userEvent.keyboard("j");
    const second = document.activeElement as HTMLElement;
    expect(second.getAttribute("data-testid")).toMatch(/^dropin-item-\/etc\/systemd\/system\/httpd\.service\.d\//);
    expect(second).not.toBe(first);
    expectFocusedStop(second);
  });

  it("g and G go to the first and the last shown row, a reported row", async () => {
    renderUnits(loadUnitFields("planted"));
    act(() => screen.getByTestId("service-item-cups.service").focus());
    await userEvent.keyboard("G");
    expectFocusedStop(screen.getByTestId(linkTestId("vendor-link-demo.service")));
    await userEvent.keyboard("g");
    expectFocusedStop(screen.getByTestId("default-target"));
  });

  it("moves between reported rows", async () => {
    renderUnits(loadUnitFields("planted"));
    act(() => screen.getByTestId(linkTestId("vendor-link-demo.service")).focus());
    await userEvent.keyboard("k");
    const previous = document.activeElement as HTMLElement;
    expect(previous.getAttribute("data-testid")).toMatch(/^manual-link-.*multi-link-demo\.service/);
    expectFocusedStop(previous);
  });

  it("does not move from a checkbox or a link inside a row", async () => {
    renderUnits(loadUnitFields("planted"));
    const checkbox = screen.getByRole("checkbox", { name: "Toggle bluetooth.service" });
    act(() => checkbox.focus());
    await userEvent.keyboard("j");
    expect(document.activeElement).toBe(checkbox);
    const link = within(screen.getByTestId("service-item-wg-quick@wg0.service")).getByRole("link");
    act(() => link.focus());
    await userEvent.keyboard("k");
    expect(document.activeElement).toBe(link);
  });
});

describe("U32: Space and x (§ 6.13 Toggle)", () => {
  it.each<[string, string, string, () => HTMLElement]>([
    ["a unit row", "x", "bluetooth.service", () => screen.getByTestId("service-item-bluetooth.service")],
    ["a unit row", " ", "bluetooth.service", () => screen.getByTestId("service-item-bluetooth.service")],
    ["a drop-in row", " ", HTTPD_OVERRIDE, () => screen.getByTestId(`dropin-item-${HTTPD_OVERRIDE}`)],
    ["a drop-in row", "x", HTTPD_OVERRIDE, () => screen.getByTestId(`dropin-item-${HTTPD_OVERRIDE}`)],
    ["the target row", "x", "DefaultTarget", () => screen.getByTestId("default-target")],
    ["the target row", " ", "DefaultTarget", () => screen.getByTestId("default-target")],
  ])("toggles %s with %j", async (_name, key, sent, row) => {
    const planted = loadUnitFields("planted");
    renderUnits(planted);
    respondWith(planted);
    act(() => row().focus());
    await userEvent.keyboard(key === " " ? " " : key);
    await waitFor(() => expect(viewUpdates).toHaveBeenCalledTimes(1));
    expect(mockFetch).toHaveBeenCalledTimes(1);
    expect(JSON.stringify(sentOp(0))).toContain(sent);
  });

  it("does nothing on a reported row", async () => {
    renderUnits(loadUnitFields("planted"));
    for (const testId of [
      linkTestId("vendor-link-demo.service"),
      "mount-finding-data-demo.mount",
      "undetermined-install-demo.service",
    ]) {
      act(() => screen.getByTestId(testId).focus());
      await userEvent.keyboard(" x");
    }
    expect(mockFetch).not.toHaveBeenCalled();
  });
});

describe("U32: the roving tab stop (§ 6.13)", () => {
  it("is one shown row, the first, on mount", () => {
    renderUnits(loadUnitFields("planted"));
    expect(theStop()).toBe(screen.getByTestId("default-target"));
  });

  it("is one row with a locked drop-in, the target and unit rows in the view", () => {
    const planted = loadUnitFields("planted");
    const httpdOverride = planted.unit_dropins.find((d) => d.path === HTTPD_OVERRIDE)!;
    const locked: DropInDecisionDto = {
      ...httpdOverride,
      path: "/etc/systemd/system/httpd.service.d/locked.conf",
      locked: true,
      attention_reason: "A locked drop-in.",
    };
    renderUnits({ ...planted, unit_dropins: [...planted.unit_dropins, locked] });
    expect(screen.getByTestId(`dropin-item-${locked.path}`)).toHaveAttribute("data-locked", "true");
    expect(theStop()).toBe(screen.getByTestId("default-target"));
  });

  it("moves to the first shown row when its group collapses", async () => {
    renderUnits(loadUnitFields("planted"));
    expect(theStop()).toBe(screen.getByTestId("default-target"));
    await userEvent.click(header("Default target"));
    expect(theStop()).toBe(firstShownRowEl());
    expect(theStop()).toBe(screen.getByTestId("service-item-bluetooth.service"));
  });

  it("moves to the first shown row when a filter change hides it", async () => {
    renderUnits(loadUnitFields("planted"));
    act(() => screen.getByTestId("service-item-bluetooth.service").focus());
    expect(theStop()).toBe(screen.getByTestId("service-item-bluetooth.service"));
    await select("Excluded");
    expect(screen.queryByTestId("service-item-bluetooth.service")).toBeNull();
    expect(theStop()).toBe(firstShownRowEl());
  });

  it("moves to the first shown row when a section query hides it", () => {
    renderUnits(loadUnitFields("planted"));
    expect(theStop()).toBe(screen.getByTestId("default-target"));
    act(() => setQuery("cockpit"));
    expect(screen.queryByTestId("default-target")).toBeNull();
    expect(theStop()).toBe(screen.getByTestId("service-item-cockpit.socket"));
  });

  it("is held by no row when no row is shown, and returns to the first when one is", () => {
    renderUnits(loadUnitFields("planted"));
    act(() => setQuery("no row matches this"));
    expect(rows()).toHaveLength(0);
    act(() => setQuery(""));
    expect(theStop()).toBe(screen.getByTestId("default-target"));
  });

  it("is held by no row while every shown group is collapsed", async () => {
    renderUnits(loadUnitFields("planted"));
    act(() => setQuery("cockpit"));
    await userEvent.click(header("Sockets"));
    expect(rows().filter((r) => r.tabIndex === 0)).toEqual([]);
  });

  it("is held by a reported row when only reported rows are shown", async () => {
    const planted = loadUnitFields("planted");
    renderUnits({
      ...planted,
      default_target: undefined,
      unit_states: planted.unit_states.filter((r) => r.unit === "packagekit.service" || r.unit === "bluetooth.service"),
      unit_dropins: [],
    });
    await select("Needs follow-up");
    expect(screen.queryAllByTestId(/^service-item-/)).toHaveLength(0);
    const stop = theStop();
    expect(stop).toBe(firstShownRowEl());
    expect(stop.getAttribute("data-testid")).toMatch(/^(manual-link|mount-finding|undetermined-install)-/);
  });

  it("follows focus, as App's reveal gives it", () => {
    renderUnits(loadUnitFields("planted"));
    const row = screen.getByTestId("service-item-cockpit.socket");
    act(() => row.focus());
    expectFocusedStop(row);
  });

  it("moves to the first shown row when a view drops the record holding it", () => {
    const planted = loadUnitFields("planted");
    renderUnits(planted);
    act(() => screen.getByTestId("service-item-cockpit.socket").focus());
    expect(theStop()).toBe(screen.getByTestId("service-item-cockpit.socket"));
    act(() =>
      pushView({ ...planted, unit_states: planted.unit_states.filter((r) => r.unit !== "cockpit.socket") }),
    );
    expect(screen.queryByTestId("service-item-cockpit.socket")).toBeNull();
    expect(theStop()).toBe(screen.getByTestId("default-target"));
  });

  it("leaves filter buttons, headers and in-row links as tab stops, and checkboxes focusable", () => {
    renderUnits(loadUnitFields("planted"));
    const section = screen.getByTestId("service-section");
    const buttons = within(screen.getByRole("group", { name: "Show" })).getAllByRole("button");
    expect(buttons.length).toBeGreaterThan(0);
    const headers = Array.from(section.querySelectorAll<HTMLElement>('button[aria-controls]'));
    expect(headers.length).toBeGreaterThan(1);
    const links = Array.from(section.querySelectorAll<HTMLElement>("[role='row'] a[href]"));
    expect(links.length).toBeGreaterThan(0);
    for (const el of [...buttons.filter((b) => !b.hasAttribute("disabled")), ...headers, ...links]) {
      expect(el.tabIndex, el.textContent ?? "").toBe(0);
    }
    const checkboxes = within(section)
      .getAllByRole("checkbox")
      .filter((c) => !(c as HTMLInputElement).disabled);
    expect(checkboxes.length).toBeGreaterThan(0);
    for (const c of checkboxes) {
      expect(c.tabIndex).toBe(0);
      act(() => c.focus());
      expect(document.activeElement).toBe(c);
    }
  });
});

describe("ArrowDown from the search box reads the same order (§ 6.12, § 6.13)", () => {
  it("focuses the first row of shownRowOrder, skipping a collapsed group, and makes it the tab stop", async () => {
    renderUnits(loadUnitFields("planted"));
    await userEvent.click(header("Default target"));
    act(() => focusFirstRow!());
    expectFocusedStop(screen.getByTestId("service-item-bluetooth.service"));
  });

  it("reaches a reported row when only reported rows are shown", () => {
    renderUnits(loadUnitFields("planted"));
    act(() => setQuery("vendor-link-demo"));
    act(() => focusFirstRow!());
    expectFocusedStop(screen.getByTestId(linkTestId("vendor-link-demo.service")));
  });
});

describe("U36: keyboard over true duplicates (§ 6.13)", () => {
  it("visits both link rows of one row key", async () => {
    renderUnits(loadUnitFields("planted"));
    const [first, second] = screen.getAllByTestId(linkTestId("dup-link-demo.service"));
    expect(first).not.toBe(second);
    expect(first.getAttribute("data-unit-key")).toBe(second.getAttribute("data-unit-key"));
    act(() => screen.getByTestId(linkTestId("dup-link-demo-other.service")).focus());
    await userEvent.keyboard("j");
    expectFocusedStop(first);
    await userEvent.keyboard("j");
    expectFocusedStop(second);
    await userEvent.keyboard("k");
    expectFocusedStop(first);
  });

  it("goes from the first unit row through its two drop-ins to the second unit row", async () => {
    renderUnits(loadUnitFields("planted"));
    const [firstUnit, secondUnit] = screen.getAllByTestId("service-item-dup-state-demo.service");
    const [firstDropin, secondDropin] = screen.getAllByTestId(`dropin-item-${DUP_STATE_DROPIN}`);
    act(() => firstUnit.focus());
    expectFocusedStop(firstUnit);
    await userEvent.keyboard("j");
    expectFocusedStop(firstDropin);
    await userEvent.keyboard("j");
    expectFocusedStop(secondDropin);
    await userEvent.keyboard("j");
    expectFocusedStop(secondUnit);
  });

  it("keeps focus and the tab stop on the second occurrence after x", async () => {
    const planted = loadUnitFields("planted");
    renderUnits(planted);
    const second = screen.getAllByTestId("service-item-dup-state-demo.service")[1];
    act(() => second.focus());
    respondWith(planted);
    await userEvent.keyboard("x");
    await waitFor(() => expect(viewUpdates).toHaveBeenCalledTimes(1));
    expect(JSON.stringify(sentOp(0))).toContain("dup-state-demo.service");
    const secondNow = screen.getAllByTestId("service-item-dup-state-demo.service")[1];
    expectFocusedStop(secondNow);
  });
});
