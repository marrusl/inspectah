/**
 * The units section's outcome filter (spec § 6.4, § 6.9 rows 1 and 4,
 * § 6.13 The filter and Announcements; § 7 strings 7, 8, 9, 14, 19 to 22).
 * U5 (no flag under a filter), U6, U7, U9, U23 (filter rows), U33 and
 * U35 (a) filter counts.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ServiceSection } from "../ServiceSection";
import type { PendingGuard } from "../../hooks/usePendingGuard";
import type { DefaultTargetView } from "../../api/types";
import type { UnitFields } from "../unitOutcome";
import { loadUnitFields } from "../../__tests__/fixtures/systemd-units/load";

const HINT = "Needs follow-up: the Containerfile writes a comment for the row and no command.";
const STRING_12 = "No other unit differs from its preset, and no drop-in was found.";
const STRING_14 = "No units match this filter.";

const stubGuard: PendingGuard = {
  pendingKeys: new Set<string>(),
  runGuarded: vi.fn(),
} as unknown as PendingGuard;

beforeEach(() => {
  vi.stubGlobal("fetch", vi.fn());
});
afterEach(() => {
  vi.restoreAllMocks();
});

function section(fields: UnitFields) {
  return (
    <ServiceSection
      services={fields.unit_states}
      dropins={fields.unit_dropins}
      mountFindings={fields.mount_findings}
      manualLinks={fields.manual_links}
      undeterminedInstalls={fields.undetermined_installs}
      defaultTarget={fields.default_target}
      onViewUpdate={vi.fn()}
      onMutationError={vi.fn()}
      pendingGuard={stubGuard}
    />
  );
}

function renderUnits(fields: UnitFields) {
  return render(section(fields));
}

function bar(): HTMLElement | null {
  return screen.queryByRole("group", { name: "Show" });
}

/** The four filter buttons' texts, in order. */
function barTexts(): string[] {
  return within(bar()!)
    .getAllByRole("button")
    .map((b) => b.textContent ?? "");
}

function filterButton(label: string): HTMLElement {
  return within(bar()!).getByRole("button", { name: new RegExp(`^${label} \\d+$`) });
}

async function select(label: string) {
  await userEvent.click(filterButton(label));
}

function selected(): string {
  const pressed = within(bar()!)
    .getAllByRole("button")
    .filter((b) => b.getAttribute("aria-pressed") === "true");
  expect(pressed).toHaveLength(1);
  return (pressed[0].textContent ?? "").replace(/ \d+$/, "");
}

function liveRegion(): HTMLElement | null {
  return document.querySelector<HTMLElement>('[aria-live="polite"]');
}

function groupTitles(): string[] {
  return Array.from(document.querySelectorAll('[id^="unit-group-title-"]')).map(
    (el) => el.textContent ?? "",
  );
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

/** The unit names of a group's unit rows, in order. */
function unitRows(title: string): string[] {
  return Array.from(groupGrid(title).querySelectorAll<HTMLElement>('[data-testid^="service-item-"]')).map(
    (r) => r.getAttribute("aria-label") ?? "",
  );
}

function reportedRows(): HTMLElement[] {
  return Array.from(groupGrid("Reported, not carried").querySelectorAll<HTMLElement>('[role="row"]'));
}

/** Every flag label in a group header. */
function flags(): string[] {
  return Array.from(document.querySelectorAll('[id^="unit-group-title-"]'))
    .map((el) => el.closest("button")!.querySelector(".pf-v6-c-label"))
    .filter((l): l is Element => l !== null)
    .map((l) => l.textContent ?? "");
}

/** Kitchen-sink with no row needing follow-up and `cups.service` excluded. */
function withoutFollowUp(): UnitFields {
  const fields = loadUnitFields("kitchen-sink");
  for (const row of fields.unit_states) delete row.declined;
  fields.unit_states.find((r) => r.unit === "cups.service")!.include = false;
  fields.manual_links = [];
  fields.undetermined_installs = [];
  fields.mount_findings = [];
  return fields;
}

/** Kitchen-sink with `cups.service`'s declined cleared and the row excluded. */
function cupsExcluded(): UnitFields {
  const fields = loadUnitFields("kitchen-sink");
  const cups = fields.unit_states.find((r) => r.unit === "cups.service")!;
  delete cups.declined;
  cups.include = false;
  return fields;
}

function stock10WithTarget(target: Partial<DefaultTargetView>): UnitFields {
  const fields = loadUnitFields("stock10");
  fields.default_target = { ...fields.default_target!, ...target };
  return fields;
}

const plantedDeclinedTarget = () => loadUnitFields("planted").default_target!;

describe("U6: when the bar is drawn", () => {
  it.each(["stock10", "stock9"] as const)("draws no bar on %s", (name) => {
    renderUnits(loadUnitFields(name));
    expect(bar()).toBeNull();
    expect(liveRegion()).toBeNull();
  });

  it("draws the kitchen-sink bar with its counts, Excluded disabled and the hint", () => {
    renderUnits(loadUnitFields("kitchen-sink"));
    expect(barTexts()).toEqual(["All 23", "Needs follow-up 9", "Carried 14", "Excluded 0"]);
    expect(filterButton("Excluded")).toBeDisabled();
    expect(screen.getByText(HINT)).toBeInTheDocument();
    expect(selected()).toBe("All");
  });

  it("draws the bar without the hint when something is excluded and nothing needs follow-up", () => {
    renderUnits(withoutFollowUp());
    expect(barTexts()).toEqual(["All 20", "Needs follow-up 0", "Carried 19", "Excluded 1"]);
    expect(screen.queryByText(HINT)).not.toBeInTheDocument();
  });

  // The heading is MainContent's: MainContent.unit-links.test.tsx checks heading, bar, group.
  it("sits before the first group", () => {
    renderUnits(loadUnitFields("kitchen-sink"));
    const firstHeader = header("Default target");
    expect(bar()!.compareDocumentPosition(firstHeader) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });
});

describe("U7: what each value shows, rendered", () => {
  it("Needs follow-up shows the declined branches, tmp.mount and the reported rows", async () => {
    renderUnits(loadUnitFields("kitchen-sink"));
    await select("Needs follow-up");
    expect(groupTitles()).toEqual(["Services", "Other units", "Reported, not carried"]);
    expect(unitRows("Services")).toEqual([
      "cups.service",
      "driftify-also-main.service",
      "iscsid.service",
      "virtnetworkd.service",
      "wg-quick@wg0.service",
    ]);
    expect(screen.queryByTestId("dropin-orphans")).not.toBeInTheDocument();
    expect(unitRows("Other units")).toEqual(["tmp.mount"]);
    expect(reportedRows()).toHaveLength(3);
  });

  it("Carried shows carried branches with their drop-ins, no orphans and no reported rows", async () => {
    renderUnits(loadUnitFields("kitchen-sink"));
    await select("Carried");
    expect(groupTitles()).toEqual(["Default target", "Services", "Sockets", "Timers"]);
    expect(screen.getByTestId("default-target")).toBeInTheDocument();
    const httpd = screen.getByTestId("service-group-httpd.service");
    expect(within(httpd).getAllByTestId(/^dropin-item-/)).toHaveLength(2);
    expect(screen.queryByTestId("dropin-orphans")).not.toBeInTheDocument();
    expect(screen.queryByTestId("service-item-cups.service")).not.toBeInTheDocument();
    expect(unitRows("Sockets")).toHaveLength(6);
  });

  it("Excluded shows the excluded row only, in its group", async () => {
    renderUnits(cupsExcluded());
    await select("Excluded");
    expect(groupTitles()).toEqual(["Services"]);
    expect(unitRows("Services")).toEqual(["cups.service"]);
  });

  it("All shows everything again, the orphan drop-ins included", async () => {
    renderUnits(loadUnitFields("kitchen-sink"));
    await select("Carried");
    await select("All");
    expect(groupTitles()).toEqual([
      "Default target",
      "Services",
      "Sockets",
      "Timers",
      "Other units",
      "Reported, not carried",
    ]);
    expect(screen.getByTestId("dropin-orphans")).toBeInTheDocument();
    expect(reportedRows()).toHaveLength(3);
  });
});

describe("U5: no follow-up flag under a filter", () => {
  it("shows flags under All and none under Needs follow-up", async () => {
    renderUnits(loadUnitFields("kitchen-sink"));
    expect(flags()).toEqual(["5 need follow-up", "1 needs follow-up"]);
    await select("Needs follow-up");
    expect(flags()).toEqual([]);
  });
});

describe("U9: persistence", () => {
  it("is All again after an unmount and a remount", async () => {
    const first = renderUnits(loadUnitFields("kitchen-sink"));
    await select("Carried");
    expect(selected()).toBe("Carried");
    first.unmount();
    renderUnits(loadUnitFields("kitchen-sink"));
    expect(selected()).toBe("All");
  });

  it("keeps the selected value across a view update", async () => {
    const { rerender } = renderUnits(loadUnitFields("kitchen-sink"));
    await select("Needs follow-up");
    rerender(section(cupsExcluded()));
    expect(selected()).toBe("Needs follow-up");
    expect(filterButton("Needs follow-up")).toHaveTextContent("Needs follow-up 8");
  });

  it("returns to All when the bar stops being drawn, and reads All when it returns", async () => {
    const { rerender } = renderUnits(cupsExcluded());
    await select("Excluded");
    const carried = loadUnitFields("kitchen-sink");
    for (const row of carried.unit_states) delete row.declined;
    carried.manual_links = [];
    carried.undetermined_installs = [];
    rerender(section(carried));
    expect(bar()).toBeNull();
    expect(unitRows("Services")).toContain("bluetooth.service");
    rerender(section(cupsExcluded()));
    expect(selected()).toBe("All");
    expect(liveRegion()!.textContent).toBe("");
  });

  it("stores the value neither in the URL nor in localStorage", async () => {
    // This environment's jsdom has no localStorage, so a stand-in records writes.
    const setItem = vi.fn();
    vi.stubGlobal("localStorage", {
      getItem: () => null,
      setItem,
      removeItem: vi.fn(),
      clear: vi.fn(),
      key: () => null,
      length: 0,
    });
    const href = window.location.href;
    renderUnits(loadUnitFields("kitchen-sink"));
    await select("Needs follow-up");
    await select("Carried");
    expect(window.location.href).toBe(href);
    expect(setItem).not.toHaveBeenCalled();
  });
});

describe("U35 (a): filter counts over true duplicates", () => {
  /** All, Needs follow-up, Carried, Excluded, as numbers. */
  function counts(): number[] {
    return barTexts().map((t) => Number(t.replace(/^\D+/, "")));
  }

  function countsOf(fields: UnitFields): number[] {
    const r = renderUnits(fields);
    const out = counts();
    r.unmount();
    return out;
  }

  it("counts both link records and both unit records", () => {
    const base = countsOf(loadUnitFields("planted"));

    const oneLink = loadUnitFields("planted");
    const dupLinks = oneLink.manual_links
      .map((l, i) => [l, i] as const)
      .filter(([l]) => l.unit === "dup-link-demo.service");
    expect(dupLinks).toHaveLength(2);
    oneLink.manual_links.splice(dupLinks[1][1], 1);
    const [all1, nf1, c1, e1] = countsOf(oneLink);
    expect([all1, nf1, c1, e1]).toEqual([base[0] - 1, base[1] - 1, base[2], base[3]]);

    const oneState = loadUnitFields("planted");
    const dupStates = oneState.unit_states
      .map((u, i) => [u, i] as const)
      .filter(([u]) => u.unit === "dup-state-demo.service");
    expect(dupStates).toHaveLength(2);
    oneState.unit_states.splice(dupStates[1][1], 1);
    const [all2, nf2, c2, e2] = countsOf(oneState);
    expect([all2, nf2, c2, e2]).toEqual([base[0] - 1, base[1], base[2] - 1, base[3]]);
  });

  it("counts and shows each of two unit records with one row key by its own outcome", async () => {
    const base = countsOf(loadUnitFields("planted"));
    // The second dup-state-demo.service record excluded, the first still carried.
    const split = loadUnitFields("planted");
    const twins = split.unit_states.filter((u) => u.unit === "dup-state-demo.service");
    expect(twins.map((u) => [u.include, u.declined])).toEqual([
      [true, undefined],
      [true, undefined],
    ]);
    twins[1].include = false;
    expect(countsOf(split)).toEqual([base[0], base[1], base[2] - 1, base[3] + 1]);

    renderUnits(split);
    const shownTwins = () =>
      screen
        .queryAllByTestId("service-item-dup-state-demo.service")
        .map((row) => (within(row).getByRole("checkbox") as HTMLInputElement).checked);
    expect(shownTwins()).toEqual([true, false]);
    await select("Excluded");
    expect(shownTwins()).toEqual([false]);
    await select("Carried");
    expect(shownTwins()).toEqual([true]);
    await select("Needs follow-up");
    expect(shownTwins()).toEqual([]);
  });
});

describe("U23: the filter's empty states (§ 6.9 rows 1 and 4)", () => {
  it("stock10 with the target excluded: All 1, Excluded 1 and string 12", () => {
    renderUnits(stock10WithTarget({ include: false }));
    expect(barTexts()).toEqual(["All 1", "Needs follow-up 0", "Carried 0", "Excluded 1"]);
    expect(screen.getByText(STRING_12)).toBeInTheDocument();
    expect(screen.queryByText(HINT)).not.toBeInTheDocument();
  });

  it("stock10 with the target declined: All 1, Needs follow-up 1, the hint and string 12", () => {
    renderUnits(stock10WithTarget(plantedDeclinedTarget()));
    expect(barTexts()).toEqual(["All 1", "Needs follow-up 1", "Carried 0", "Excluded 0"]);
    expect(screen.getByText(HINT)).toBeInTheDocument();
    expect(screen.getByText(STRING_12)).toBeInTheDocument();
  });

  it("a value left with no row shows string 14 and Show all, and string 12 still shows", async () => {
    const { rerender } = renderUnits(stock10WithTarget({ include: false }));
    await select("Excluded");
    expect(screen.getByTestId("default-target")).toBeInTheDocument();
    expect(screen.queryByText(STRING_14)).not.toBeInTheDocument();
    // An Undo: the target is no longer excluded, and now needs follow-up.
    rerender(section(stock10WithTarget(plantedDeclinedTarget())));
    expect(selected()).toBe("Excluded");
    expect(screen.queryByTestId("default-target")).not.toBeInTheDocument();
    expect(screen.getByText(STRING_14)).toBeInTheDocument();
    expect(screen.getByText(STRING_12)).toBeInTheDocument();
    // Under the bar, and before the sentence.
    const empty = screen.getByText(STRING_14);
    expect(bar()!.compareDocumentPosition(empty) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    await userEvent.click(screen.getByRole("button", { name: "Show all" }));
    expect(selected()).toBe("All");
    expect(screen.getByTestId("default-target")).toBeInTheDocument();
    expect(screen.queryByText(STRING_14)).not.toBeInTheDocument();
  });

  it("Show all hands its focus to the bar's All button (§ 6.13)", async () => {
    const { rerender } = renderUnits(stock10WithTarget({ include: false }));
    await select("Excluded");
    rerender(section(stock10WithTarget(plantedDeclinedTarget())));
    await userEvent.click(screen.getByRole("button", { name: "Show all" }));
    expect(screen.queryByRole("button", { name: "Show all" })).not.toBeInTheDocument();
    expect(document.activeElement).toBe(filterButton("All"));
  });

  it("Show all leaves focus where it is when it did not have it", async () => {
    const { rerender } = renderUnits(stock10WithTarget({ include: false }));
    await select("Excluded");
    rerender(section(stock10WithTarget(plantedDeclinedTarget())));
    const followUp = filterButton("Needs follow-up");
    act(() => followUp.focus());
    // A pointer that does not move focus, as Safari's click on a button.
    fireEvent.click(screen.getByRole("button", { name: "Show all" }));
    expect(selected()).toBe("All");
    expect(document.activeElement).toBe(followUp);
  });

  it("a kind group left empty shows string 14 on kitchen-sink after an Undo", async () => {
    const { rerender } = renderUnits(cupsExcluded());
    await select("Excluded");
    expect(unitRows("Services")).toEqual(["cups.service"]);
    rerender(section(loadUnitFields("kitchen-sink")));
    expect(groupTitles()).toEqual([]);
    expect(screen.getByText(STRING_14)).toBeInTheDocument();
    expect(screen.queryByText(STRING_12)).not.toBeInTheDocument();
  });

  it("shows no filter empty state under All", () => {
    renderUnits(loadUnitFields("kitchen-sink"));
    expect(screen.queryByText(STRING_14)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Show all" })).not.toBeInTheDocument();
  });
});

describe("U33: the live region", () => {
  it("is in the DOM and empty before any change", () => {
    renderUnits(loadUnitFields("kitchen-sink"));
    expect(liveRegion()).not.toBeNull();
    expect(liveRegion()!.textContent).toBe("");
  });

  it("states the shown count for each value, plural", async () => {
    renderUnits(loadUnitFields("kitchen-sink"));
    await select("Needs follow-up");
    expect(liveRegion()!.textContent).toBe("Showing 9 rows that need follow-up.");
    await select("Carried");
    expect(liveRegion()!.textContent).toBe("Showing 14 carried rows.");
    await select("All");
    // The rows now shown: 23 with an outcome and the sshd.service orphan
    // drop-in, while the bar still reads All 23.
    expect(liveRegion()!.textContent).toBe("Showing all 24 rows.");
    expect(filterButton("All")).toHaveTextContent("All 23");
  });

  it("counts an orphan drop-in that is the only row of its group", async () => {
    const fields = stock10WithTarget({ include: false });
    fields.unit_dropins = loadUnitFields("kitchen-sink").unit_dropins.filter(
      (d) => d.unit === "sshd.service",
    );
    renderUnits(fields);
    expect(groupTitles()).toEqual(["Default target", "Services"]);
    expect(unitRows("Services")).toEqual([]);
    await select("Excluded");
    expect(liveRegion()!.textContent).toBe("Showing 1 excluded row.");
    await select("All");
    expect(liveRegion()!.textContent).toBe("Showing all 2 rows.");
    expect(barTexts()[0]).toBe("All 1");
  });

  it("states the rows now shown when a reveal sets the filter to All", async () => {
    const fields = loadUnitFields("kitchen-sink");
    const { rerender } = renderUnits(fields);
    await select("Carried");
    rerender(
      <ServiceSection
        services={fields.unit_states}
        dropins={fields.unit_dropins}
        mountFindings={fields.mount_findings}
        manualLinks={fields.manual_links}
        undeterminedInstalls={fields.undetermined_installs}
        defaultTarget={fields.default_target}
        revealItemId="cups.service"
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
        pendingGuard={stubGuard}
      />,
    );
    expect(selected()).toBe("All");
    expect(liveRegion()!.textContent).toBe("Showing all 24 rows.");
  });

  it("counts the rows shown under a section query", async () => {
    const fields = loadUnitFields("kitchen-sink");
    render(
      <ServiceSection
        services={fields.unit_states}
        dropins={fields.unit_dropins}
        mountFindings={fields.mount_findings}
        manualLinks={fields.manual_links}
        undeterminedInstalls={fields.undetermined_installs}
        defaultTarget={fields.default_target}
        filterText="sshd"
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
        pendingGuard={stubGuard}
      />,
    );
    await select("Carried");
    expect(liveRegion()!.textContent).toBe("Showing 0 carried rows.");
    await select("All");
    // Only the sshd.service orphan drop-in matches, alone in Services.
    expect(groupTitles()).toEqual(["Services"]);
    expect(liveRegion()!.textContent).toBe("Showing all 1 row.");
  });

  it("states two excluded rows, plural", async () => {
    const fields = cupsExcluded();
    fields.unit_states.find((r) => r.unit === "bluetooth.service")!.include = false;
    renderUnits(fields);
    await select("Excluded");
    expect(liveRegion()!.textContent).toBe("Showing 2 excluded rows.");
  });

  it("states one excluded row and all 1 row, singular", async () => {
    renderUnits(stock10WithTarget({ include: false }));
    await select("Excluded");
    expect(liveRegion()!.textContent).toBe("Showing 1 excluded row.");
    await select("All");
    expect(liveRegion()!.textContent).toBe("Showing all 1 row.");
  });

  it("states one row that needs follow-up, singular", async () => {
    renderUnits(stock10WithTarget(plantedDeclinedTarget()));
    await select("Needs follow-up");
    expect(liveRegion()!.textContent).toBe("Showing 1 row that needs follow-up.");
  });

  it("states one carried row, singular", async () => {
    const fields = loadUnitFields("stock9");
    fields.unit_states[0].include = false;
    renderUnits(fields);
    await select("Carried");
    expect(liveRegion()!.textContent).toBe("Showing 1 carried row.");
  });
});
