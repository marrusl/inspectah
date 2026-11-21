/**
 * Global search over the units section in single-host mode (spec § 6.12
 * Global search, Search text travels on the entry, Duplicates, True
 * duplicates; § 6.15). U25, U31's global half and U26's global search
 * case.
 *
 * Mounts `App` with `fetch` as the only thing replaced, types into the
 * real global search box and selects a real result, as an operator does.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import App from "../App";
import type { ReferenceSection, ViewResponse } from "../api/types";
import { emptyView } from "../test-utils/emptyView";
import type { UnitFields } from "../components/unitOutcome";
import { loadUnitFields } from "./fixtures/systemd-units/load";

const kitchenSink = loadUnitFields("kitchen-sink");
const planted = loadUnitFields("planted");

/** § 7 string 1, the label of every single-host units result. */
const UNITS_LABEL = "Systemd units";

/** A title in the `units` reference section and nowhere else. */
const REFERENCE_ONLY = "refonly-zebra.service";
/** The same word in another reference section, which stays indexed. */
const OTHER_REFERENCE = "refonly-zebra-mount";

const LIMITS_DROPIN = "/etc/systemd/system/limits-demo.slice.d/override.conf";

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
    sections: [
      { id: "configs", label: "Config Files", is_triage: true },
      { id: "units", label: "Systemd units", is_triage: true },
    ],
    has_actionable_sections: true,
  },
];

function referenceSection(id: string, display_name: string, title: string): ReferenceSection {
  return {
    id,
    display_name,
    items: [{ id: `ref:${title}`, title, subtitle: null, detail: null, searchable_text: title }],
  };
}

const SECTIONS: ReferenceSection[] = [
  referenceSection("units", "Services", REFERENCE_ONLY),
  referenceSection("storage", "Storage", OTHER_REFERENCE),
];

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

async function openSection(name: RegExp, heading: string) {
  const nav = await screen.findByRole("navigation", { name: "Sections" });
  await userEvent.click(await within(nav).findByRole("link", { name }));
  expect(await screen.findByRole("heading", { level: 2, name: heading })).toBeInTheDocument();
  // Opening a section moves focus into main content on the next frame. Let
  // that land now: landing after the search box is clicked, it takes the
  // keystrokes and the query stays empty. Frames run in order, so ours runs
  // after it.
  await act(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
}

/** Mounts `App` on the units section with these unit fields. */
async function openUnits(fields: UnitFields) {
  serve(emptyView({ ...fields }));
  render(<App />);
  await openSection(/^Systemd units/, "Systemd units");
  await screen.findByTestId("service-section");
}

interface Result {
  option: HTMLElement;
  label: string;
  title: string;
}

/** Types a query into the global search box and reads the results. */
async function globalSearch(query: string): Promise<Result[]> {
  const input = screen.getByLabelText("Search all sections");
  await userEvent.clear(input);
  await userEvent.type(input, query);
  const list = screen.getByTestId("global-search-results");
  return within(list)
    .queryAllByRole("option")
    .map((option) => {
      const [label, title] = Array.from(option.querySelectorAll("span")).map((s) => s.textContent ?? "");
      return { option, label, title };
    });
}

const titles = (results: Result[]) => results.map((r) => r.title);

/** The one result with this title; fails on none or several. */
function only(results: Result[], title: string): Result {
  const found = results.filter((r) => r.title === title);
  expect(found, `results titled ${title}`).toHaveLength(1);
  return found[0];
}

/** Selects a result and waits for the row the key names to hold focus, shown. */
async function selectAndExpectFocus(result: Result, key: string) {
  await userEvent.click(result.option);
  await waitFor(() => expect(document.activeElement?.getAttribute("data-unit-key")).toBe(key));
  expect(document.activeElement!.closest("[hidden]")).toBeNull();
}

const unitKey = (unit: string) => JSON.stringify(["unit_state", unit]);

describe("U25: every units row is a result under its row key (§ 6.12)", () => {
  it.each<[string, string, string, string]>([
    ["a unit_states row", "wg-quick@wg0", "wg-quick@wg0.service", unitKey("wg-quick@wg0.service")],
    ["a drop-in", "limits-demo", LIMITS_DROPIN, JSON.stringify(["dropin", LIMITS_DROPIN])],
    ["the default target", "default target", "graphical.target (default target)", '["default_target"]'],
    [
      "a manual link",
      "driftify-static",
      "driftify-static.service (wants multi-user.target)",
      JSON.stringify(["manual_link", "wants", "multi-user.target", "driftify-static.service"]),
    ],
    [
      "an undetermined install",
      "foreign",
      "demo.service (foreign name)",
      JSON.stringify(["undetermined_install", "demo.service"]),
    ],
    ["a mount finding", "/data/demo", "data-demo.mount (/data/demo)", JSON.stringify(["mount_finding", "data-demo.mount"])],
  ])("%s focuses its own row", async (_kind, query, title, key) => {
    await openUnits(planted);
    await selectAndExpectFocus(only(await globalSearch(query), title), key);
  });

  it("indexes no entry of the units reference section, and still the others", async () => {
    await openUnits(planted);
    const results = await globalSearch("refonly-zebra");
    expect(titles(results)).toEqual([OTHER_REFERENCE]);
    expect(results[0].label).toBe("Storage");
  });

  it("finds an omitted unit by a word of its reason", async () => {
    await openUnits(planted);
    // wg-quick@wg0.service's declined.text: "... provision the secret on the deployed host ...".
    const results = await globalSearch("provision the secret");
    expect(titles(results)).toContain("wg-quick@wg0.service");
  });
});

describe("U25: one unit name in four kinds (§ 6.12 Duplicates)", () => {
  const SHARED = "data-demo.mount";
  const FOUR: [string, string][] = [
    [SHARED, unitKey(SHARED)],
    [`${SHARED} (/data/demo)`, JSON.stringify(["mount_finding", SHARED])],
    [`${SHARED} (wants multi-user.target)`, JSON.stringify(["manual_link", "wants", "multi-user.target", SHARED])],
    [`${SHARED} (specifier)`, JSON.stringify(["undetermined_install", SHARED])],
  ];

  it("gives four results, each focusing its own row", async () => {
    await openUnits(planted);
    const results = await globalSearch(SHARED);
    expect(titles(results).sort()).toEqual(FOUR.map(([title]) => title).sort());
    for (const [title, key] of FOUR) {
      await selectAndExpectFocus(only(await globalSearch(SHARED), title), key);
    }
  });

  it("finds the undetermined install, not the unit row, by a word only in its reason", async () => {
    await openUnits(planted);
    const results = await globalSearch("specifier");
    expect(titles(results)).toContain(`${SHARED} (specifier)`);
    expect(titles(results)).not.toContain(SHARED);
  });
});

describe("U25: a unit name that holds another kind's test id (§ 6.11)", () => {
  it("gives two results with different ids and texts, each focusing its own row", async () => {
    await openUnits(planted);
    const both = await globalSearch("demo.service");
    const unitRow = only(both, "undetermined-install-demo.service");
    const install = only(both, "demo.service (foreign name)");
    expect(unitRow.option).not.toBe(install.option);

    expect(titles(await globalSearch("foreign"))).not.toContain("undetermined-install-demo.service");
    expect(titles(await globalSearch("undetermined-install-demo"))).not.toContain("demo.service (foreign name)");

    await selectAndExpectFocus(
      only(await globalSearch("demo.service"), "undetermined-install-demo.service"),
      unitKey("undetermined-install-demo.service"),
    );
    expect(screen.getByTestId("service-item-undetermined-install-demo.service")).toHaveFocus();
    await selectAndExpectFocus(
      only(await globalSearch("demo.service"), "demo.service (foreign name)"),
      JSON.stringify(["undetermined_install", "demo.service"]),
    );
  });
});

describe("U25: several links for one unit (§ 6.12 Duplicates)", () => {
  const UNIT = "multi-link-demo.service";
  const linkKey = (relation: string, parent: string) => JSON.stringify(["manual_link", relation, parent, UNIT]);
  const LINKS: [string, string][] = [
    [`${UNIT} (wants multi-user.target)`, linkKey("wants", "multi-user.target")],
    [`${UNIT} (wants graphical.target)`, linkKey("wants", "graphical.target")],
    [`${UNIT} (requires multi-user.target)`, linkKey("requires", "multi-user.target")],
  ];
  const linkTitles = (results: Result[]) => titles(results).filter((t) => t.startsWith(UNIT));

  it("gives three results, and graphical and requires each list one of them", async () => {
    await openUnits(planted);
    expect(linkTitles(await globalSearch(UNIT)).sort()).toEqual(LINKS.map(([t]) => t).sort());
    expect(linkTitles(await globalSearch("graphical"))).toEqual([`${UNIT} (wants graphical.target)`]);
    expect(linkTitles(await globalSearch("requires"))).toEqual([`${UNIT} (requires multi-user.target)`]);
  });

  it("lists only the requires link for its relation phrase, required by (§ 6.12)", async () => {
    await openUnits(planted);
    const results = await globalSearch("required by");
    expect(linkTitles(results)).toEqual([`${UNIT} (requires multi-user.target)`]);
    expect(titles(results).filter((t) => t.includes("(wants "))).toEqual([]);
  });

  it("focuses each link's own row", async () => {
    await openUnits(planted);
    for (const [title, key] of LINKS) {
      await selectAndExpectFocus(only(await globalSearch(UNIT), title), key);
    }
  });
});

describe("U25: true duplicates (§ 6.12)", () => {
  it("lists dup-link-demo.service twice, either result focusing the first link row", async () => {
    await openUnits(planted);
    const title = "dup-link-demo.service (wants multi-user.target)";
    const key = JSON.stringify(["manual_link", "wants", "multi-user.target", "dup-link-demo.service"]);
    const testId = `manual-link-${JSON.stringify(["wants", "multi-user.target", "dup-link-demo.service"])}`;
    const results = (await globalSearch("dup-link-demo.service")).filter((r) => r.title === title);
    expect(results).toHaveLength(2);
    expect(screen.getAllByTestId(testId)).toHaveLength(2);
    for (const index of [0, 1]) {
      const pick = (await globalSearch("dup-link-demo.service")).filter((r) => r.title === title)[index];
      await selectAndExpectFocus(pick, key);
      expect(document.activeElement).toBe(screen.getAllByTestId(testId)[0]);
    }
  });

  it("lists dup-state-demo.service's unit rows twice, either result focusing the first", async () => {
    await openUnits(planted);
    const unit = "dup-state-demo.service";
    expect((await globalSearch(unit)).filter((r) => r.title === unit)).toHaveLength(2);
    expect(screen.getAllByTestId(`service-item-${unit}`)).toHaveLength(2);
    for (const index of [0, 1]) {
      const pick = (await globalSearch(unit)).filter((r) => r.title === unit)[index];
      await selectAndExpectFocus(pick, unitKey(unit));
      expect(document.activeElement).toBe(screen.getAllByTestId(`service-item-${unit}`)[0]);
    }
  });
});

describe("true duplicates that differ outside their identity (§ 6.12)", () => {
  const FIRST = "first-twin-sentinel";
  const SECOND = "second-twin-sentinel";

  /** Planted, with each dup-link-demo.service link given its own advisory line. */
  function twinsWithSentinels(): UnitFields {
    const fields = loadUnitFields("planted");
    const twins = fields.manual_links.filter((l) => l.unit === "dup-link-demo.service");
    expect(twins).toHaveLength(2);
    twins[0].advisories = [...twins[0].advisories, `# ${FIRST}`];
    twins[1].advisories = [...twins[1].advisories, `# ${SECOND}`];
    return fields;
  }

  it("finds each link's own entry by its own sentinel, and either result reveals the first link row", async () => {
    await openUnits(twinsWithSentinels());
    const title = "dup-link-demo.service (wants multi-user.target)";
    const key = JSON.stringify(["manual_link", "wants", "multi-user.target", "dup-link-demo.service"]);
    const testId = `manual-link-${JSON.stringify(["wants", "multi-user.target", "dup-link-demo.service"])}`;
    for (const sentinel of [FIRST, SECOND]) {
      const results = await globalSearch(sentinel);
      // One entry each: an entry carrying its twin's text would find two, or none.
      expect(results.map((r) => [r.label, r.title]), sentinel).toEqual([[UNITS_LABEL, title]]);
      await selectAndExpectFocus(results[0], key);
      const [firstRow, secondRow] = screen.getAllByTestId(testId);
      expect(document.activeElement).toBe(firstRow);
      expect(firstRow.textContent).toContain(FIRST);
      expect(secondRow.textContent).toContain(SECOND);
    }
  });
});

describe("selecting a units result leaves no pending focus behind", () => {
  it.each<[string, string, string, string]>([
    ["a drop-in", "limits-demo", LIMITS_DROPIN, JSON.stringify(["dropin", LIMITS_DROPIN])],
    ["a unit row", "wg-quick@wg0", "wg-quick@wg0.service", unitKey("wg-quick@wg0.service")],
    ["a reported row", "foreign", "demo.service (foreign name)", JSON.stringify(["undetermined_install", "demo.service"])],
  ])("%s: a later section change focuses main content", async (_kind, query, title, key) => {
    serve(emptyView({ ...planted }));
    render(<App />);
    await openSection(/^Config Files/, "Config Files");
    await selectAndExpectFocus(only(await globalSearch(query), title), key);
    expect(screen.getByRole("heading", { level: 2, name: "Systemd units" })).toBeInTheDocument();
    await openSection(/^Config Files/, "Config Files");
    await waitFor(() =>
      expect(document.activeElement).toBe(screen.getByRole("heading", { level: 2, name: "Config Files" })),
    );
  });
});

describe("U31: the label of units results (§ 6.12, § 6.14)", () => {
  it('labels every single-host units result "Systemd units" and none "Services"', async () => {
    await openUnits(planted);
    const results = await globalSearch(".service");
    expect(results.length).toBeGreaterThan(0);
    expect(new Set(results.map((r) => r.label))).toEqual(new Set([UNITS_LABEL]));
    expect(results.map((r) => r.label)).not.toContain("Services");
  });
});

describe("U26: a global search result selected from within the units section (§ 6.11)", () => {
  it("shows and focuses cockpit.socket under Needs follow-up with Sockets collapsed", async () => {
    await openUnits(kitchenSink);
    const sockets = screen.getByRole("button", { name: /^Sockets\b/ });
    await userEvent.click(sockets);
    expect(sockets).toHaveAttribute("aria-expanded", "false");
    const bar = screen.getByRole("group", { name: "Show" });
    await userEvent.click(within(bar).getByRole("button", { name: /^Needs follow-up \d+$/ }));
    expect(screen.queryByTestId("service-item-cockpit.socket")).toBeNull();

    await selectAndExpectFocus(
      only(await globalSearch("cockpit.socket"), "cockpit.socket"),
      unitKey("cockpit.socket"),
    );
    expect(screen.getByTestId("service-item-cockpit.socket")).toHaveFocus();
    expect(within(bar).getByRole("button", { name: /^All \d+$/ })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: /^Sockets\b/ })).toHaveAttribute("aria-expanded", "true");
  });
});

/** Kitchen-sink after excluding `cups.service`: its declined cleared. */
function cupsExcluded(): UnitFields {
  const fields = loadUnitFields("kitchen-sink");
  const cups = fields.unit_states.find((r) => r.unit === "cups.service")!;
  delete cups.declined;
  cups.include = false;
  return fields;
}

/**
 * Serves the view, and answers each op with `afterOp`, which the view then
 * serves too. Each response is a fresh copy, as a server's is.
 */
function serveWithOps(view: ViewResponse, afterOp: ViewResponse): { ops: () => number } {
  let ops = 0;
  let current = view;
  const routes: Record<string, () => unknown> = {
    "/api/health": () => HEALTH,
    "/api/groups": () => GROUPS,
    "/api/view": () => structuredClone(current),
    "/api/snapshot/sections": () => SECTIONS,
    "/api/viewed": () => ({ ids: [] }),
    "/api/ops": () => [],
  };
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      const method = init?.method ?? "GET";
      if (method === "POST" && url === "/api/op") {
        ops += 1;
        current = afterOp;
        return { ok: true, status: 200, json: async () => structuredClone(afterOp) };
      }
      if (method !== "GET" && url !== "/api/viewed") mutatingRequests.push(`${method} ${url}`);
      if (!(url in routes)) return { ok: false, status: 404, json: async () => ({ error: url }) };
      return { ok: true, status: 200, json: async () => routes[url]() };
    }),
  );
  return { ops: () => ops };
}

describe("U26: a reveal lands after the section query it clears has settled (§ 6.11)", () => {
  it("shows and focuses cups.service that only its pin showed, again on a second selection, and a later toggle keeps the filter", async () => {
    const served = serveWithOps(emptyView({ ...kitchenSink }), emptyView({ ...cupsExcluded() }));
    render(<App />);
    await openSection(/^Systemd units/, "Systemd units");
    await screen.findByTestId("service-section");
    // Let the section's own focus on opening settle before `/` is pressed.
    await act(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
    const bar = () => screen.getByRole("group", { name: "Show" });
    const pressed = () =>
      within(bar())
        .getAllByRole("button")
        .filter((b) => b.getAttribute("aria-pressed") === "true")
        .map((b) => (b.textContent ?? "").replace(/ \d+$/, ""));
    await userEvent.click(within(bar()).getByRole("button", { name: /^Needs follow-up \d+$/ }));
    await userEvent.keyboard("/");
    const box = await screen.findByRole("textbox", { name: "Filter section items" });
    // cups.service's reason holds "socket"; excluded, its reason is gone.
    await userEvent.type(box, "socket");
    await userEvent.click(screen.getByRole("checkbox", { name: "Toggle cups.service" }));
    await waitFor(() => expect(screen.getByRole("checkbox", { name: "Toggle cups.service" })).not.toBeChecked());
    expect(served.ops()).toBe(1);
    // Only its pin shows it now: excluded, it neither needs follow-up nor matches the query.
    expect(screen.getByTestId("service-item-cups.service")).toBeInTheDocument();

    await selectAndExpectFocus(only(await globalSearch("cups.service"), "cups.service"), unitKey("cups.service"));
    expect(screen.getByTestId("service-item-cups.service")).toHaveFocus();
    expect(pressed()).toEqual(["All"]);

    await selectAndExpectFocus(only(await globalSearch("cups.service"), "cups.service"), unitKey("cups.service"));
    expect(screen.getByTestId("service-item-cups.service")).toHaveFocus();

    // A later toggle under Needs follow-up leaves the filter where the operator put it.
    await userEvent.click(within(bar()).getByRole("button", { name: /^Needs follow-up \d+$/ }));
    const iscsid = screen.getByRole("checkbox", { name: "Toggle iscsid.service" });
    await userEvent.click(iscsid);
    await waitFor(() => expect(served.ops()).toBe(2));
    await act(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
    expect(pressed()).toEqual(["Needs follow-up"]);
    expect(screen.queryByTestId("service-item-cups.service")).toBeNull();
    expect(screen.getByTestId("service-item-iscsid.service")).toBeInTheDocument();
  });
});
