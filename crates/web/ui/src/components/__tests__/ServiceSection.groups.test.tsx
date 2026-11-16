/**
 * The units section drawn through its groups (spec § 6.2, § 6.3, § 6.5,
 * § 6.7, § 6.8, § 6.9, § 6.11, § 6.12, § 6.14), under "All" with no
 * filter bar unless a test selects a value. U3, U4, U5 (All), U10, U21,
 * U23 (rows), U31 (heading), U35 (a) to (c).
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, within, cleanup } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ServiceSection } from "../ServiceSection";
import { MainContent } from "../MainContent";
import type { PendingGuard } from "../../hooks/usePendingGuard";
import { emptyView } from "../../test-utils/emptyView";
import { keyFields, rowKey, type UnitFields } from "../unitOutcome";
import { loadUnitFields } from "../../__tests__/fixtures/systemd-units/load";

const STRING_6 =
  "inspectah reports these and does not carry them. No toggle: the lines under each row are the Containerfile's comments for it.";
const STRING_11 = "The target the source host boots to. Included, the image boots to it too.";
const STRING_12 = "No other unit differs from its preset, and no drop-in was found.";
const STRING_13 = "No unit differs from its preset, and no drop-in was found.";
const OLD_EMPTY = "No service state changes detected.";

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

function renderUnits(fields: UnitFields) {
  return render(
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
    />,
  );
}

/** The group titles in document order. */
function groupTitles(): string[] {
  return Array.from(document.querySelectorAll('[id^="unit-group-title-"]')).map(
    (el) => el.textContent ?? "",
  );
}

/** A group's header button, found by its title element. */
function header(title: string): HTMLElement {
  const titleEl = Array.from(document.querySelectorAll('[id^="unit-group-title-"]')).find(
    (el) => el.textContent === title,
  );
  expect(titleEl, `no group titled ${title}`).toBeDefined();
  return titleEl!.closest("button") as HTMLElement;
}

/** A group's grid, found through its header's aria-controls. */
function groupGrid(title: string): HTMLElement {
  const id = header(title).getAttribute("aria-controls")!;
  return document.getElementById(id) as HTMLElement;
}

/** The count badge's text in a header, or null when it has none. */
function headerCount(title: string): string | null {
  const badge = header(title).querySelector(".pf-v6-c-badge");
  return badge ? badge.textContent : null;
}

function rows(container: ParentNode = document): HTMLElement[] {
  return Array.from(container.querySelectorAll<HTMLElement>('[role="row"]'));
}

function rowsWithKey(key: string, container: ParentNode = document): HTMLElement[] {
  return rows(container).filter((r) => r.getAttribute("data-unit-key") === key);
}

describe("ServiceSection groups (U3)", () => {
  it("draws kitchen-sink's groups in order with their counts, and no Paths", () => {
    renderUnits(loadUnitFields("kitchen-sink"));
    expect(groupTitles()).toEqual([
      "Default target",
      "Services",
      "Sockets",
      "Timers",
      "Other units",
      "Reported, not carried",
    ]);
    expect(headerCount("Default target")).toBeNull();
    expect(headerCount("Services")).toBe("11");
    expect(headerCount("Sockets")).toBe("6");
    expect(headerCount("Timers")).toBe("1");
    expect(headerCount("Other units")).toBe("1");
    expect(headerCount("Reported, not carried")).toBe("3");
  });

  it("draws planted's Paths group and its slice under Other units", () => {
    renderUnits(loadUnitFields("planted"));
    expect(groupTitles()).toContain("Paths");
    expect(within(groupGrid("Paths")).getByTestId("service-item-watch-demo.path")).toBeInTheDocument();
    expect(
      within(groupGrid("Other units")).getByTestId("service-item-limits-demo.slice"),
    ).toBeInTheDocument();
  });

  it("opens every group by default", () => {
    renderUnits(loadUnitFields("kitchen-sink"));
    for (const title of groupTitles()) {
      expect(header(title)).toHaveAttribute("aria-expanded", "true");
      expect(groupGrid(title).hidden).toBe(false);
    }
  });

  it("collapses one group alone, and opens it again on a remount", async () => {
    const user = userEvent.setup();
    const ks = loadUnitFields("kitchen-sink");
    const { unmount } = renderUnits(ks);
    await user.click(header("Sockets"));
    expect(header("Sockets")).toHaveAttribute("aria-expanded", "false");
    expect(groupGrid("Sockets").hidden).toBe(true);
    for (const title of groupTitles().filter((t) => t !== "Sockets")) {
      expect(header(title)).toHaveAttribute("aria-expanded", "true");
      expect(groupGrid(title).hidden).toBe(false);
    }
    unmount();
    renderUnits(ks);
    expect(header("Sockets")).toHaveAttribute("aria-expanded", "true");
    expect(groupGrid("Sockets").hidden).toBe(false);
  });
});

describe("ServiceSection sort and templates (U4)", () => {
  it("orders Services rows by unit name in code-point order", () => {
    renderUnits(loadUnitFields("planted"));
    const ids = Array.from(
      groupGrid("Services").querySelectorAll('[data-testid^="service-item-"]'),
    ).map((el) => el.getAttribute("data-testid")!);
    const sorted = [...ids].sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
    expect(ids).toEqual(sorted);
    expect(ids).toContain("service-item-getty@.service");
  });

  it("keeps an instance out of its template's group and renders it the same without the template", () => {
    const planted = loadUnitFields("planted");
    renderUnits(planted);
    const template = screen.getByTestId("service-group-getty@.service");
    const instance = screen.getByTestId("service-item-getty@tty2.service");
    expect(template.contains(instance)).toBe(false);
    const withTemplate = instance.outerHTML;
    cleanup();

    renderUnits({
      ...planted,
      unit_states: planted.unit_states.filter((r) => r.unit !== "getty@.service"),
    });
    expect(screen.getByTestId("service-item-getty@tty2.service").outerHTML).toBe(withTemplate);
  });
});

describe("ServiceSection follow-up flag under All (U5)", () => {
  it("flags Services and Other units only", () => {
    renderUnits(loadUnitFields("kitchen-sink"));
    expect(within(header("Services")).getByText("5 need follow-up")).toBeInTheDocument();
    expect(within(header("Other units")).getByText("1 needs follow-up")).toBeInTheDocument();
    for (const title of ["Sockets", "Timers", "Default target", "Reported, not carried"]) {
      expect(header(title).textContent).not.toMatch(/follow-up/);
    }
  });
});

describe("ServiceSection default target group (U10)", () => {
  function targetRow(): HTMLElement {
    const row = within(groupGrid("Default target")).getByTestId("default-target");
    return row;
  }

  it("shows the explanation on an included target, with no kind chip", () => {
    renderUnits(loadUnitFields("kitchen-sink"));
    const row = targetRow();
    expect(within(row).getByText(STRING_11)).toBeInTheDocument();
    expect(row.querySelector(".inspectah-unit-kind")).toBeNull();
  });

  it("shows the explanation on an excluded target", () => {
    const stock10 = loadUnitFields("stock10");
    renderUnits({ ...stock10, default_target: { ...stock10.default_target!, include: false } });
    expect(within(targetRow()).getByText(STRING_11)).toBeInTheDocument();
    expect(targetRow().querySelector(".inspectah-unit-kind")).toBeNull();
  });

  it("shows the explanation and the declined text on a declined target", () => {
    const planted = loadUnitFields("planted");
    renderUnits(planted);
    const row = targetRow();
    expect(within(row).getByText(STRING_11)).toBeInTheDocument();
    expect(within(row).getByTestId("declined-default-target")).toHaveTextContent(
      planted.default_target!.declined!.text,
    );
    expect(row.querySelector(".inspectah-unit-kind")).toBeNull();
  });
});

describe("ServiceSection orphan drop-ins (U21)", () => {
  it("puts sshd.service's orphan block in Services, after its last unit row", () => {
    renderUnits(loadUnitFields("kitchen-sink"));
    const services = groupGrid("Services");
    const block = within(services).getByTestId("dropin-orphan-group-sshd.service");
    const unitRows = services.querySelectorAll('[data-testid^="service-item-"]');
    const last = unitRows[unitRows.length - 1];
    expect(last.compareDocumentPosition(block) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    const orphans = block.closest('[data-testid="dropin-orphans"]') as HTMLElement;
    expect(orphans).toHaveTextContent("Other drop-in overrides");
    expect(orphans).toHaveTextContent("These overrides have no unit-state decision row.");
  });

  it.each(["kitchen-sink", "planted"] as const)(
    "gives nothing outside a row a role a rowgroup may not own, with %s (aria-required-children)",
    (fixture) => {
      renderUnits(loadUnitFields(fixture));
      const rowgroups = Array.from(document.querySelectorAll('[role="rowgroup"]'));
      expect(rowgroups.length).toBeGreaterThan(0);
      const outsideRows = rowgroups.flatMap((rg) =>
        Array.from(rg.querySelectorAll("*")).filter((el) => el.closest('[role="row"]') === null),
      );
      expect(outsideRows.length).toBeGreaterThan(0);
      for (const el of outsideRows) {
        // Text the block shows outside its rows is generic: no heading,
        // list or other element with a role of its own.
        expect(el.getAttribute("role"), el.outerHTML.slice(0, 80)).toBeNull();
        expect(["DIV", "SPAN", "P"], el.outerHTML.slice(0, 80)).toContain(el.tagName);
      }
    },
  );

  it("puts planted's orphan with no kind in Other units", () => {
    const planted = loadUnitFields("planted");
    const orphan = planted.unit_dropins.find((d) => d.kind === undefined)!;
    renderUnits(planted);
    expect(
      within(groupGrid("Other units")).getByTestId(`dropin-orphan-group-${orphan.unit}`),
    ).toBeInTheDocument();
  });
});

describe("ServiceSection Reported, not carried (§ 6.7)", () => {
  it("shows string 6 under its header and every reported row's test id", () => {
    const ks = loadUnitFields("kitchen-sink");
    renderUnits(ks);
    const description = screen.getByText(STRING_6);
    expect(description.closest('[role="grid"]')).toBeNull();
    expect(
      header("Reported, not carried").compareDocumentPosition(description) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    const grid = groupGrid("Reported, not carried");
    for (const link of ks.manual_links) {
      expect(
        within(grid).getByTestId(
          `manual-link-${JSON.stringify([link.relation, link.parent, link.unit])}`,
        ),
      ).toBeInTheDocument();
    }
    for (const r of ks.undetermined_installs) {
      expect(within(grid).getByTestId(`undetermined-install-${r.unit}`)).toBeInTheDocument();
    }
  });
});

describe("ServiceSection empty states (U23 rows)", () => {
  it("shows the Default target group and string 12 on stock10", () => {
    renderUnits(loadUnitFields("stock10"));
    expect(groupTitles()).toEqual(["Default target"]);
    expect(screen.getByText(STRING_12)).toBeInTheDocument();
    expect(screen.queryByText(STRING_13)).toBeNull();
  });

  it("shows string 13 on an empty view, and never the old sentence", () => {
    renderUnits({
      unit_states: [],
      unit_dropins: [],
      mount_findings: [],
      manual_links: [],
      undetermined_installs: [],
    });
    expect(screen.getByText(STRING_13)).toBeInTheDocument();
    expect(screen.queryByText(STRING_12)).toBeNull();
    expect(screen.queryByText(OLD_EMPTY)).toBeNull();
    expect(groupTitles()).toEqual([]);
  });

  it("shows Services 1 on stock9", () => {
    renderUnits(loadUnitFields("stock9"));
    expect(groupTitles()).toEqual(["Default target", "Services"]);
    expect(headerCount("Services")).toBe("1");
    expect(screen.queryByText(STRING_12)).toBeNull();
  });
});

describe("ServiceSection heading and name (U31 heading)", () => {
  it("heads the units section 'Systemd units' with no shadow count", () => {
    const ks = loadUnitFields("kitchen-sink");
    expect(ks.unit_dropins.some((d) => d.shadow_type === "full_shadow")).toBe(true);
    render(
      <MainContent
        activeSection="units"
        loading={false}
        sections={null}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
        sectionSearchOpen={false}
        onSectionSearchClose={vi.fn()}
        viewData={emptyView({ ...ks })}
      />,
    );
    expect(screen.getByRole("heading", { level: 2 })).toHaveTextContent(/^Systemd units$/);
  });

  it("names the section's container 'Systemd units'", () => {
    renderUnits(loadUnitFields("kitchen-sink"));
    expect(screen.getByRole("region", { name: "Systemd units" })).toBe(
      screen.getByTestId("service-section"),
    );
  });
});

describe("ServiceSection true duplicates (U35 a)", () => {
  const DUP_LINK_KEY = JSON.stringify(["manual_link", "wants", "multi-user.target", "dup-link-demo.service"]);
  const DUP_LINK_OTHER_KEY = JSON.stringify([
    "manual_link",
    "wants",
    "multi-user.target",
    "dup-link-demo-other.service",
  ]);
  const DUP_STATE_KEY = JSON.stringify(["unit_state", "dup-state-demo.service"]);
  const DUP_DROPIN_KEY = JSON.stringify([
    "dropin",
    "/etc/systemd/system/dup-state-demo.service.d/override.conf",
  ]);

  it("renders one row per record and nests the drop-ins under the first occurrence", () => {
    renderUnits(loadUnitFields("planted"));
    expect(rowsWithKey(DUP_LINK_KEY)).toHaveLength(2);
    expect(rowsWithKey(DUP_LINK_OTHER_KEY)).toHaveLength(1);
    expect(rowsWithKey(DUP_STATE_KEY)).toHaveLength(2);

    const dropinRows = rowsWithKey(DUP_DROPIN_KEY);
    expect(dropinRows).toHaveLength(2);
    const [first, second] = screen.getAllByTestId("service-group-dup-state-demo.service");
    expect(rowsWithKey(DUP_DROPIN_KEY, first)).toHaveLength(2);
    expect(rowsWithKey(DUP_DROPIN_KEY, second)).toHaveLength(0);
  });

  it("counts the Reported header as its shown rows", () => {
    renderUnits(loadUnitFields("planted"));
    expect(headerCount("Reported, not carried")).toBe(
      String(rows(groupGrid("Reported, not carried")).length),
    );
  });
});

describe("ServiceSection true duplicates under search and filter (U35 b, c)", () => {
  const DUP_LINK_KEY = JSON.stringify(["manual_link", "wants", "multi-user.target", "dup-link-demo.service"]);
  const DUP_LINK_OTHER_KEY = JSON.stringify([
    "manual_link",
    "wants",
    "multi-user.target",
    "dup-link-demo-other.service",
  ]);
  const DUP_STATE_KEY = JSON.stringify(["unit_state", "dup-state-demo.service"]);
  const DUP_DROPIN_KEY = JSON.stringify([
    "dropin",
    "/etc/systemd/system/dup-state-demo.service.d/override.conf",
  ]);

  function section(fields: UnitFields, filterText: string, onResultCount: (n: number) => void) {
    return (
      <ServiceSection
        services={fields.unit_states}
        dropins={fields.unit_dropins}
        mountFindings={fields.mount_findings}
        manualLinks={fields.manual_links}
        undeterminedInstalls={fields.undetermined_installs}
        defaultTarget={fields.default_target}
        filterText={filterText}
        onResultCount={onResultCount}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
        pendingGuard={stubGuard}
      />
    );
  }

  /** U35 (a)'s rows: one per record, drop-ins under the first occurrence. */
  function expectU35aRows() {
    expect(rowsWithKey(DUP_LINK_KEY)).toHaveLength(2);
    expect(rowsWithKey(DUP_LINK_OTHER_KEY)).toHaveLength(1);
    expect(rowsWithKey(DUP_STATE_KEY)).toHaveLength(2);
    expect(rowsWithKey(DUP_DROPIN_KEY)).toHaveLength(2);
  }

  it("keeps the row count, the Reported count and the result count equal as a query narrows and is restored", () => {
    const planted = loadUnitFields("planted");
    const onResultCount = vi.fn();
    const { rerender } = render(section(planted, "", onResultCount));
    for (const [query, n] of [
      ["dup-link-demo", 3],
      ["dup-link-demo.service", 2],
      ["dup-link-demo", 3],
    ] as const) {
      rerender(section(planted, query, onResultCount));
      const shown = rows(screen.getByTestId("service-section"));
      expect(shown).toHaveLength(n);
      expect(headerCount("Reported, not carried")).toBe(String(n));
      expect(onResultCount).toHaveBeenLastCalledWith(n);
    }
    rerender(section(planted, "", onResultCount));
    expectU35aRows();
  });

  it("shows both unit occurrences and their two drop-ins under Carried, and All restores U35 (a)", async () => {
    render(section(loadUnitFields("planted"), "", vi.fn()));
    const bar = screen.getByRole("group", { name: "Show" });
    await userEvent.click(within(bar).getByRole("button", { name: /^Carried \d+$/ }));
    expect(rowsWithKey(DUP_STATE_KEY)).toHaveLength(2);
    expect(rowsWithKey(DUP_DROPIN_KEY)).toHaveLength(2);
    const services = groupGrid("Services");
    expect(headerCount("Services")).toBe(
      String(services.querySelectorAll('[data-testid^="service-item-"]').length),
    );
    await userEvent.click(within(bar).getByRole("button", { name: /^All \d+$/ }));
    expectU35aRows();
  });
});

describe("ServiceSection grid structure: controls sit in cells", () => {
  /** Every required_config path of a fixture, so each renders as a link. */
  function requiredConfigPaths(fields: UnitFields): ReadonlySet<string> {
    return new Set(fields.unit_states.flatMap((r) => r.required_config));
  }

  it.each(["kitchen-sink", "planted"] as const)(
    "every link and button inside a role=row has a role=gridcell ancestor inside that row (%s)",
    (name) => {
      const fields = loadUnitFields(name);
      render(
        <ServiceSection
          services={fields.unit_states}
          dropins={fields.unit_dropins}
          mountFindings={fields.mount_findings}
          manualLinks={fields.manual_links}
          undeterminedInstalls={fields.undetermined_installs}
          defaultTarget={fields.default_target}
          onNavigateToItem={vi.fn()}
          onNavigateToConfig={vi.fn()}
          configPaths={requiredConfigPaths(fields)}
          onViewUpdate={vi.fn()}
          onMutationError={vi.fn()}
          pendingGuard={stubGuard}
        />,
      );
      const controls = rows().flatMap((row) =>
        Array.from(row.querySelectorAll<HTMLElement>("a, button, input")).map((el) => ({ row, el })),
      );
      // The fixtures draw Also= links and needed-configuration links.
      expect(controls.filter(({ el }) => el.tagName === "A").length).toBeGreaterThan(0);
      const outside = controls
        .filter(({ row, el }) => {
          const cell = el.closest('[role="gridcell"]');
          return cell === null || cell.closest('[role="row"]') !== row;
        })
        .map(({ row, el }) => `${row.getAttribute("aria-label")}: ${el.textContent}`);
      expect(outside).toEqual([]);
    },
  );
});

describe("ServiceSection data-unit-key", () => {
  it("gives every row its record's row key", () => {
    const planted = loadUnitFields("planted");
    renderUnits(planted);
    const keyed = keyFields(planted);
    const expected = [
      ...(keyed.defaultTarget ? [keyed.defaultTarget] : []),
      ...keyed.unitStates,
      ...keyed.dropins,
      ...keyed.mountFindings,
      ...keyed.manualLinks,
      ...keyed.undeterminedInstalls,
    ].map((k) => rowKey(k.record));
    const actual = rows(screen.getByTestId("service-section")).map((r) =>
      r.getAttribute("data-unit-key"),
    );
    expect([...actual].sort()).toEqual([...expected].sort());
  });
});
