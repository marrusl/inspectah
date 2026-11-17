/**
 * The units section's rows (spec § 6.5 last bullet, § 6.6 items 1 to 4,
 * 6, 8 to 12, § 7 strings 15 to 18 and 23). U11, U12, U13, U14, U17,
 * U18, U19, U20 (render), U22.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, within } from "@testing-library/react";
import { ServiceSection } from "../ServiceSection";
import type { PendingGuard } from "../../hooks/usePendingGuard";
import type { LinkRelation, ManualLinkCause, ManualLinkView } from "../../api/types";
import { causeLabel, type UnitFields } from "../unitOutcome";
import { loadUnitFields } from "../../__tests__/fixtures/systemd-units/load";

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

const kitchenSink = loadUnitFields("kitchen-sink");
const planted = loadUnitFields("planted");

const SUBTLE_TEXT = "--pf-t--global--text--color--subtle";

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

/** Planted with one unit row changed. */
function plantedWith(unit: string, over: Partial<UnitFields["unit_states"][number]>): UnitFields {
  return {
    ...planted,
    unit_states: planted.unit_states.map((r) => (r.unit === unit ? { ...r, ...over } : r)),
  };
}

const LABEL_COLOURS = ["blue", "teal", "green", "orange", "purple", "red", "orangered", "yellow"];

/** A compact outline Label of a colour; grey is PatternFly's default, with no modifier. */
function expectOutlineLabel(label: Element | null, colour: string) {
  expect(label).not.toBeNull();
  expect(label).toHaveClass("pf-m-outline", "pf-m-compact");
  for (const other of LABEL_COLOURS) {
    if (other === colour) expect(label).toHaveClass(`pf-m-${other}`);
    else expect(label).not.toHaveClass(`pf-m-${other}`);
  }
}

const unitRow = (unit: string) => screen.getByTestId(`service-item-${unit}`);
const nameCell = (row: HTMLElement) =>
  row.querySelector(".inspectah-decision-row__name") as HTMLElement;
const badgeCells = (row: HTMLElement) =>
  Array.from(row.querySelectorAll<HTMLElement>(".inspectah-decision-row__badge"));

/** The group grid whose header title is `title`. */
function groupGrid(title: string): HTMLElement {
  const titleEl = Array.from(document.querySelectorAll('[id^="unit-group-title-"]')).find(
    (el) => el.textContent === title,
  )!;
  const id = titleEl.closest("button")!.getAttribute("aria-controls")!;
  return document.getElementById(id) as HTMLElement;
}

function linkRows(link: ManualLinkView): HTMLElement[] {
  return screen.getAllByTestId(
    `manual-link-${JSON.stringify([link.relation, link.parent, link.unit])}`,
  );
}

describe("subtle unit state text keeps its contrast (§ 6.13 Themes)", () => {
  // An Also= member's links and a mount finding's state are small italic
  // text. Dimmed by opacity they fell below 4.5:1; the subtle text token
  // holds its contrast in both themes.
  it("colours an Also= member's links and a mount finding's state with the subtle token", () => {
    renderUnits(planted);
    const mount = planted.mount_findings.find((f) => f.state !== null)!;
    const els = [
      ...screen.getAllByTestId("also-member-links"),
      screen.getByTestId(`mount-finding-state-${mount.unit}`),
    ];
    expect(els.length).toBeGreaterThan(1);
    for (const el of els) {
      expect(el.getAttribute("style") ?? "", el.textContent ?? "").toContain(SUBTLE_TEXT);
    }
  });
});

describe("U11: state against preset (§ 6.6 item 1, string 16)", () => {
  it("shows the state, then preset in subtle text, with no parentheses, italic or opacity", () => {
    renderUnits(kitchenSink);
    const state = screen.getByTestId("service-state-httpd.service");
    expect(state.textContent).toBe("enabled preset: disable");
    expect(state.textContent).not.toMatch(/[()]/);
    const preset = within(state).getByText("preset: disable");
    for (const el of [state, preset]) {
      expect(el.style.fontStyle).not.toBe("italic");
      expect(el.style.opacity === "" || Number(el.style.opacity) >= 1).toBe(true);
    }
    expect(preset.getAttribute("style") ?? "").toContain(SUBTLE_TEXT);
  });

  it("shows the state alone when default_state is null", () => {
    renderUnits(plantedWith("httpd.service", { default_state: null }));
    expect(screen.getByTestId("service-state-httpd.service").textContent).toBe("enabled");
  });
});

describe("U12: the outcome label (§ 6.6 item 2, § 6.5, string 15)", () => {
  it.each<[string, string, string]>([
    ["tmp.mount", "FIXME", "orange"],
    ["cups.service", "Advisory", "blue"],
    ["wg-quick@wg0.service", "Omitted", "grey"],
  ])("labels %s %s, a compact %s outline label in the badge cell", (unit, text, colour) => {
    renderUnits(planted);
    const row = unitRow(unit);
    const label = within(row).getByText(text).closest(".pf-v6-c-label") as HTMLElement;
    expectOutlineLabel(label, colour);
    expect(badgeCells(row).some((cell) => cell.contains(label))).toBe(true);
  });

  it("labels planted's declined default target", () => {
    renderUnits(planted);
    const row = screen.getByTestId("default-target");
    const label = within(row).getByText("Omitted").closest(".pf-v6-c-label") as HTMLElement;
    expectOutlineLabel(label, "grey");
    expect(badgeCells(row).some((cell) => cell.contains(label))).toBe(true);
  });

  it("puts the outcome label after the triage badge on driftify-also-main.service", () => {
    renderUnits(kitchenSink);
    const cells = badgeCells(unitRow("driftify-also-main.service"));
    const triage = cells.findIndex((cell) => cell.textContent === "Unknown Origin");
    const outcome = cells.findIndex((cell) => cell.textContent === "Advisory");
    expect(triage).toBeGreaterThanOrEqual(0);
    expect(outcome).toBeGreaterThan(triage);
  });

  it("gives an undeclined row no outcome label", () => {
    renderUnits(planted);
    const row = unitRow("httpd.service");
    for (const word of ["FIXME", "Advisory", "Omitted"]) {
      expect(within(row).queryByText(word)).toBeNull();
    }
  });
});

describe("U13: no Non-Default State badge (§ 6.6 item 3)", () => {
  it("drops Non-Default State, keeps Unknown Origin and the drop-in badges", () => {
    renderUnits(kitchenSink);
    const section = screen.getByTestId("service-section");
    expect(section.textContent).not.toContain("Non-Default State");
    expect(within(unitRow("driftify-also-main.service")).getByText("Unknown Origin")).toBeInTheDocument();
    expect(
      within(
        screen.getByTestId("dropin-item-/etc/systemd/system/httpd.service.d/override.conf"),
      ).getByText("Drop-in Override"),
    ).toBeInTheDocument();
    expect(
      within(screen.getByTestId("dropin-item-/etc/systemd/system/sshd.service")).getByText(
        "Shadow override",
      ),
    ).toBeInTheDocument();
  });
});

describe("U14: one reason per row (§ 6.6 item 4)", () => {
  it("shows cups.service's declined text and not its attention_reason", () => {
    renderUnits(kitchenSink);
    const cups = kitchenSink.unit_states.find((r) => r.unit === "cups.service")!;
    expect(cups.attention_reason).not.toBe(cups.declined!.text);
    const row = unitRow("cups.service");
    expect(within(row).getByText(cups.declined!.text)).toBeInTheDocument();
    expect(within(row).queryByText(cups.attention_reason!)).toBeNull();
    expect(screen.queryByTestId("attention-service-cups.service")).toBeNull();
  });

  it("shows an undeclined row's attention_reason", () => {
    renderUnits(kitchenSink);
    const getty = kitchenSink.unit_states.find((r) => r.unit === "getty@tty2.service")!;
    expect(getty.declined).toBeUndefined();
    expect(screen.getByTestId("attention-service-getty@tty2.service").textContent).toBe(
      getty.attention_reason,
    );
  });
});

describe("U17: an instance's template on its first line (§ 6.6 item 8)", () => {
  it("puts Instance of after the package word", () => {
    renderUnits(planted);
    const name = nameCell(unitRow("wg-quick@wg0.service")).textContent ?? "";
    const pkg = name.indexOf("(wireguard-tools)");
    const instance = name.indexOf("Instance of wg-quick@.service");
    expect(pkg).toBeGreaterThanOrEqual(0);
    expect(instance).toBeGreaterThan(pkg);
  });
});

describe("U18: a unit with no package (§ 6.6 item 9)", () => {
  it.each<[string, UnitFields, string, string]>([
    ["local", kitchenSink, "driftify-also-main.service", "(local)"],
    [
      "shadowed",
      plantedWith("dup-state-demo.service", { owning_package: null, fragment: "shadowed" }),
      "dup-state-demo.service",
      "(shadowed)",
    ],
    [
      "absent",
      plantedWith("watch-demo.path", { owning_package: null, fragment: "absent" }),
      "watch-demo.path",
      "(absent)",
    ],
    ["a package", kitchenSink, "httpd.service", "(httpd)"],
  ])("shows %s in parentheses", (_name, fields, unit, word) => {
    renderUnits(fields);
    for (const row of screen.getAllByTestId(`service-item-${unit}`)) {
      expect(nameCell(row).textContent).toContain(word);
    }
  });
});

describe("U19: kind chip only in Other units (§ 6.6 item 10)", () => {
  it("shows a chip on tmp.mount and the slice, none in a kind group", () => {
    renderUnits(planted);
    expect(unitRow("tmp.mount").querySelector(".inspectah-unit-kind")?.textContent).toBe("mount");
    expect(unitRow("limits-demo.slice").querySelector(".inspectah-unit-kind")?.textContent).toBe(
      "slice",
    );
    for (const title of ["Services", "Sockets", "Timers", "Paths"]) {
      const grid = groupGrid(title);
      expect(grid.querySelectorAll('[data-testid^="service-item-"]').length).toBeGreaterThan(0);
      expect(
        grid.querySelectorAll('[data-testid^="service-item-"] .inspectah-unit-kind'),
        title,
      ).toHaveLength(0);
    }
  });
});

describe("U20: reported rows rendered (§ 6.6 items 11, 12, strings 17, 18)", () => {
  it("keeps driftify-upheld.service's link and undetermined rows side by side", () => {
    renderUnits(kitchenSink);
    const link = kitchenSink.manual_links.find((l) => l.unit === "driftify-upheld.service")!;
    expect(linkRows(link)[0].nextElementSibling).toBe(
      screen.getByTestId("undetermined-install-driftify-upheld.service"),
    );
  });

  it("shows wanted by before the parent, and the cause in the badge cell", () => {
    renderUnits(kitchenSink);
    const link = kitchenSink.manual_links.find((l) => l.unit === "driftify-static.service")!;
    const row = linkRows(link)[0];
    const relation = within(row).getByTestId("manual-link-relation");
    expect(relation.textContent).toBe("wanted by");
    const parent = within(row).getByTestId("manual-link-parent");
    expect(parent.textContent).toBe("multi-user.target");
    expect(relation.compareDocumentPosition(parent) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    const cause = within(row).getByText("Link its [Install] does not make");
    expect(badgeCells(row).some((cell) => cell.contains(cause))).toBe(true);
    expectOutlineLabel(cause.closest(".pf-v6-c-label"), "grey");
  });

  const RELATION_WORDS: Record<LinkRelation, string> = {
    wants: "wanted by",
    requires: "required by",
    upholds: "upheld by",
    alias: "has alias",
    also: "named in Also= by",
  };
  const CAUSE_WORDS: Record<string, string> = {
    static_unit: "Link its [Install] does not make",
    not_in_install: "Link made by hand",
    partial_install: "Only some of its links",
    host_device: "Tied to a host device",
    host_install_only: "Only the /etc files name it",
    vendor_install_only: "Only the package unit names it",
    undetermined: "[Install] cannot be relied on",
  };

  it("names every planted relation and cause with strings 17 and 18", () => {
    renderUnits(planted);
    const seen = new Set<string>();
    for (const link of planted.manual_links) {
      const cause = typeof link.cause === "string" ? link.cause : "undetermined";
      seen.add(link.relation).add(cause);
      for (const row of linkRows(link)) {
        expect(within(row).getByTestId("manual-link-relation").textContent).toBe(
          RELATION_WORDS[link.relation],
        );
        const label = within(row).getByText(CAUSE_WORDS[cause]);
        expect(badgeCells(row).some((cell) => cell.contains(label))).toBe(true);
      }
    }
    // The cases U20 names by string are among them.
    expect(seen).toContain("alias");
    expect(seen).toContain("vendor_install_only");
  });

  it.each<Exclude<ManualLinkCause, object>>(["not_in_install", "partial_install", "host_device"])(
    "names a %s link with its string 18 label",
    (cause) => {
      // No fixture carries these causes, so the link is built here.
      const link: ManualLinkView = {
        unit: "synthetic-cause-demo.service",
        relation: "wants",
        parent: "multi-user.target",
        cause,
        advisories: [],
      };
      renderUnits({ ...planted, manual_links: [...planted.manual_links, link] });
      const [row] = linkRows(link);
      const label = within(row).getByText(CAUSE_WORDS[cause]);
      expect(label.textContent).toBe(causeLabel(cause));
      expect(badgeCells(row).some((cell) => cell.contains(label))).toBe(true);
      expectOutlineLabel(label.closest(".pf-v6-c-label"), "grey");
    },
  );

  it("gives an undetermined-install row the undetermined label and today's reason words", () => {
    renderUnits(kitchenSink);
    const row = screen.getByTestId("undetermined-install-driftify-upheld.service");
    const label = within(row).getByText("[Install] cannot be relied on");
    expect(badgeCells(row).some((cell) => cell.contains(label))).toBe(true);
    expectOutlineLabel(label.closest(".pf-v6-c-label"), "grey");
    expect(within(row).getByTestId("undetermined-install-reason").textContent).toBe("upheld by");
  });
});

describe("U22: Unit excluded (§ 6.6 item 6, string 23)", () => {
  it("shows Unit excluded on a drop-in whose parent is excluded, and never Service excluded", () => {
    const excluded = {
      ...kitchenSink,
      unit_states: kitchenSink.unit_states.map((r) =>
        r.unit === "httpd.service" ? { ...r, include: false } : r,
      ),
    };
    renderUnits(excluded);
    const path = "/etc/systemd/system/httpd.service.d/override.conf";
    expect(screen.getByTestId(`dropin-excluded-badge-${path}`).textContent).toBe("Unit excluded");
    expect(screen.queryByText("Service excluded")).toBeNull();
  });
});

/**
 * The selectors of App.css's units-only narrow-width rules (§ 6.13 Narrow
 * widths), as written there. Vitest serves CSS, `?raw` included, as an
 * empty string, so they are listed here; P7 proves the rules themselves.
 */
const UNITS_SCOPE = ".inspectah-units-section";
const UNITS_SCOPED_SELECTORS = [
  ".inspectah-units-section .inspectah-decision-row__main",
  ".inspectah-units-section .inspectah-decision-row__name",
  ".inspectah-units-section .inspectah-decision-row__name > *",
  ".inspectah-units-section .inspectah-decision-row__badge",
  ".inspectah-units-section .inspectah-unit-filter .pf-v6-c-toggle-group",
];

describe("narrow widths: the rules that reflow rows and the bar keep applying (§ 6.13)", () => {
  it("draws the section, every row's main line and the bar under the selectors of its scoped rules", () => {
    renderUnits(kitchenSink);
    const section = screen.getByTestId("service-section");
    expect(section).toHaveClass(UNITS_SCOPE.slice(1));
    for (const selector of UNITS_SCOPED_SELECTORS) {
      expect(section.querySelectorAll(selector.slice(UNITS_SCOPE.length + 1)).length, selector).toBeGreaterThan(0);
    }
    for (const row of Array.from(section.querySelectorAll('[role="row"]'))) {
      expect(row.querySelector(":scope > .inspectah-decision-row__main"), row.getAttribute("data-testid") ?? "").not.toBeNull();
    }
  });

  it("draws a mount finding's unit file in a badge cell of the section, which the badge rule wraps", () => {
    renderUnits(planted);
    const mount = planted.mount_findings.find((f) => f.unit_file_path !== null)!;
    const file = screen.getByTestId(`mount-finding-file-${mount.unit}`);
    expect(file.textContent).toBe(mount.unit_file_path);
    const cell = file.closest(".inspectah-decision-row__badge");
    expect(cell).not.toBeNull();
    expect(screen.getByTestId("service-section").contains(cell)).toBe(true);
  });
});
