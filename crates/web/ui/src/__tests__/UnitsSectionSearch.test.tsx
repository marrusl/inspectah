/**
 * Section search in the units section (spec § 6.12, § 6.9 row 5, § 6.4
 * counts, § 9.2). U24.
 *
 * Mounts `App` with `fetch` as the only thing replaced, opens the units
 * section, and opens section search with `/` as an operator does.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import App from "../App";
import type { ReferenceSection, ViewResponse } from "../api/types";
import { emptyView } from "../test-utils/emptyView";
import {
  groupUnits,
  keyFields,
  showGroups,
  shownRowCount,
  type FilterValue,
  type UnitFields,
} from "../components/unitOutcome";
import { loadUnitFields } from "./fixtures/systemd-units/load";

const kitchenSink = loadUnitFields("kitchen-sink");

const NO_MATCH_TITLE = "No items match your search";
const STRING_14 = "No units match this filter.";

/** A word of cups.service's declined.text, and of no other field of its row. */
const DECLINED_WORD = "socket";
/** Words of the upholds link's advisory line, and of no other row. */
const ADVISORY_WORDS = "restart loop";
const UPHELD_LINK_KEY = JSON.stringify(["manual_link", "upholds", "multi-user.target", "driftify-upheld.service"]);

const HEALTH = {
  status: "ready",
  host: {
    hostname: "testhost",
    os_name: "RHEL",
    os_version: "10.2",
    os_id: "rhel",
    system_type: "physical",
    schema_version: 1,
  },
  completeness: "full",
  policy: { distro_repos: ["baseos", "appstream"] },
};

const GROUPS = [
  {
    slug: "system",
    label: "System",
    sections: [{ id: "units", label: "Systemd units", is_triage: true }],
    has_actionable_sections: true,
  },
];

const SECTIONS: ReferenceSection[] = [];

/** Requests the app sent that would change the session. */
let mutatingRequests: string[] = [];

function serve(view: ViewResponse) {
  const routes: Record<string, unknown> = {
    "/api/health": HEALTH,
    "/api/groups": GROUPS,
    "/api/view": view,
    "/api/snapshot/sections": SECTIONS,
    "/api/viewed": { ids: [] },
    "/api/ops": [],
  };
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      const method = init?.method ?? "GET";
      if (method !== "GET" && url !== "/api/viewed") {
        mutatingRequests.push(`${method} ${url}`);
      }
      if (!(url in routes)) {
        return { ok: false, status: 404, json: async () => ({ error: url }) };
      }
      return { ok: true, status: 200, json: async () => routes[url] };
    }),
  );
}

beforeEach(() => {
  mutatingRequests = [];
});

afterEach(() => {
  vi.unstubAllGlobals();
  expect(mutatingRequests).toEqual([]);
});

/** Mounts `App` on the units section with these unit fields. */
async function openUnits(fields: UnitFields) {
  serve(emptyView({ ...fields }));
  render(<App />);
  const nav = await screen.findByRole("navigation", { name: "Sections" });
  await userEvent.click(await within(nav).findByRole("link", { name: /^Systemd units/ }));
  await screen.findByTestId("service-section");
  // Let the section's own focus on opening settle before `/` is pressed.
  await act(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
}

/** Opens section search with `/`. */
async function openSearch(): Promise<HTMLInputElement> {
  await userEvent.keyboard("/");
  return (await screen.findByRole("textbox", { name: "Filter section items" })) as HTMLInputElement;
}

async function search(query: string): Promise<HTMLInputElement> {
  const box = await openSearch();
  await userEvent.clear(box);
  if (query !== "") await userEvent.type(box, query);
  return box;
}

function bar(): HTMLElement {
  return screen.getByRole("group", { name: "Show" });
}

function barTexts(): string[] {
  return within(bar())
    .getAllByRole("button")
    .map((b) => b.textContent ?? "");
}

async function selectFilter(label: string) {
  await userEvent.click(within(bar()).getByRole("button", { name: new RegExp(`^${label} \\d+$`) }));
}

/** The search box's result count, 0 when it shows none. */
function resultCount(): number {
  const text = within(screen.getByTestId("section-search")).queryByText(/^\d+ match(es)?$/)?.textContent;
  return text ? Number.parseInt(text, 10) : 0;
}

/** shownRowCount on the model, with no pins. */
function modelCount(fields: UnitFields, filter: FilterValue, query: string): number {
  return shownRowCount(
    showGroups(groupUnits(keyFields(fields)), { filter, query, pins: new Set() }),
  );
}

function rows(): HTMLElement[] {
  return Array.from(
    screen.getByTestId("service-section").querySelectorAll<HTMLElement>('[role="row"]'),
  );
}

function follows(a: Node, b: Node): boolean {
  return (a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0;
}

describe("U24: section search in the units section (§ 6.12, § 9.2)", () => {
  it("opens on / between the heading and the filter bar", async () => {
    await openUnits(kitchenSink);
    const box = await openSearch();
    const heading = screen.getByRole("heading", { level: 2, name: "Systemd units" });
    expect(follows(heading, box)).toBe(true);
    expect(follows(box, bar())).toBe(true);
    expect(box).toHaveFocus();
  });

  it("finds a unit row by a word of its declined text", async () => {
    await openUnits(kitchenSink);
    await search(DECLINED_WORD);
    expect(screen.getByTestId("service-item-cups.service")).toBeInTheDocument();
    expect(screen.queryByTestId("service-item-httpd.service")).not.toBeInTheDocument();
  });

  it("finds a parent by a drop-in path and shows all its drop-ins", async () => {
    await openUnits(kitchenSink);
    await search("override.conf");
    const httpd = screen.getByTestId("service-group-httpd.service");
    expect(within(httpd).getAllByTestId(/^dropin-item-/)).toHaveLength(2);
  });

  it("finds a reported row by a word of its advisory line, and only it", async () => {
    await openUnits(kitchenSink);
    await search(ADVISORY_WORDS);
    expect(rows().map((r) => r.getAttribute("data-unit-key"))).toEqual([UPHELD_LINK_KEY]);
  });

  it.each(["cups", DECLINED_WORD, "override.conf", "enabled", ADVISORY_WORDS])(
    "gives the search box shownRowCount as its result count (%s)",
    async (query) => {
      await openUnits(kitchenSink);
      await search(query);
      const expected = modelCount(kitchenSink, "all", query);
      expect(expected).toBeGreaterThan(0);
      expect(resultCount()).toBe(expected);
    },
  );

  it("counts orphan drop-ins in the result count", async () => {
    await openUnits(kitchenSink);
    // sshd.service is an orphan drop-in: one row, with no outcome.
    await search("sshd");
    expect(rows()).toHaveLength(1);
    expect(resultCount()).toBe(1);
  });

  it("shows the search empty state, not string 14, when a query matches nothing under a filter", async () => {
    await openUnits(kitchenSink);
    await selectFilter("Needs follow-up");
    const box = await search("zzz-matches-nothing");
    expect(screen.getByText(NO_MATCH_TITLE)).toBeInTheDocument();
    expect(screen.queryByText(STRING_14)).not.toBeInTheDocument();
    expect(rows()).toHaveLength(0);
    await userEvent.click(screen.getByRole("button", { name: "Clear filter" }));
    expect(box.value).toBe("");
    expect(screen.queryByText(NO_MATCH_TITLE)).not.toBeInTheDocument();
    expect(screen.getByTestId("service-item-cups.service")).toBeInTheDocument();
  });

  it("combines with the filter: a carried row's query under Needs follow-up shows only the empty state", async () => {
    await openUnits(kitchenSink);
    await selectFilter("Needs follow-up");
    await search("cockpit");
    expect(rows()).toHaveLength(0);
    expect(screen.getByText(NO_MATCH_TITLE)).toBeInTheDocument();
    expect(screen.queryByText(STRING_14)).not.toBeInTheDocument();
  });

  it("leaves the bar's counts as they are with no query", async () => {
    await openUnits(kitchenSink);
    const before = barTexts();
    await search("cups");
    expect(rows().length).toBeLessThan(modelCount(kitchenSink, "all", ""));
    expect(barTexts()).toEqual(before);
  });

  it("focuses the first shown row on ArrowDown from the box", async () => {
    await openUnits(kitchenSink);
    await search(DECLINED_WORD);
    await userEvent.keyboard("{ArrowDown}");
    expect(screen.getByTestId("service-item-cups.service")).toHaveFocus();
  });

  it("focuses a reported row on ArrowDown when only reported rows are shown", async () => {
    await openUnits(kitchenSink);
    await search(ADVISORY_WORDS);
    await userEvent.keyboard("{ArrowDown}");
    expect(document.activeElement?.getAttribute("data-unit-key")).toBe(UPHELD_LINK_KEY);
  });
});
