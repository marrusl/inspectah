/**
 * Reaching a row of the units section (spec § 6.11): the reveal's steps 2
 * and 3, the lookup by row key, U26's global search case (reveal half),
 * U25's bare-name and "Go to owner" clauses.
 *
 * Mounts `App` with `fetch` as the only thing replaced. A navigation is
 * the call `App` hands `ServiceSection` as `onNavigateToItem`, which is
 * `handleNavigateFromSearch`: the wrapper below records it from the real
 * component's props, so the call is the one an `Also=` link or a units
 * search result makes, into a section that stays mounted.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import App from "../App";
import type { ReferenceSection, RefinedConfig, ServiceDecisionDto, ViewResponse } from "../api/types";
import { emptyView } from "../test-utils/emptyView";
import type { UnitFields } from "../components/unitOutcome";
import { loadUnitFields } from "./fixtures/systemd-units/load";

const navigation = vi.hoisted(() => ({
  navigate: undefined as undefined | ((sectionId: string, itemId: string) => void),
}));

vi.mock("../components/ServiceSection", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../components/ServiceSection")>();
  const { createElement } = await import("react");
  return {
    ...actual,
    ServiceSection: (props: Parameters<typeof actual.ServiceSection>[0]) => {
      navigation.navigate = props.onNavigateToItem;
      return createElement(actual.ServiceSection, props);
    },
  };
});

const kitchenSink = loadUnitFields("kitchen-sink");
const planted = loadUnitFields("planted");

/** A valid unit name holding a literal backslash, as systemd escapes a dash. */
const ESCAPED_UNIT = "backup\\x2djob.timer";

/** The full shadow whose config row "Go to owner" leaves from (an orphan in Services). */
const SHADOW_PATH = "/etc/systemd/system/sshd.service";

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

const SECTIONS: ReferenceSection[] = [];

function shadowedConfig(path: string): RefinedConfig {
  return {
    entry: {
      path,
      kind: "unowned",
      category: "other",
      content: "",
      rpm_va_flags: null,
      package: null,
      diff_against_rpm: null,
      disposition: { kind: "inventory" },
      tie: false,
      tie_winner: false,
      aggregate: null,
      shadowed_by: "units",
    },
    triage: {
      triage: { mode: "single_host", site: null },
      primary_reason: "config_modified",
      annotations: [],
    },
  };
}

/** Requests the app sent that would change the session. */
let mutatingRequests: string[] = [];

const originalScrollIntoView = Element.prototype.scrollIntoView;

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
  navigation.navigate = undefined;
  // jsdom does not implement scrollIntoView, which the reveal calls.
  Element.prototype.scrollIntoView = vi.fn();
});

afterEach(() => {
  vi.unstubAllGlobals();
  Element.prototype.scrollIntoView = originalScrollIntoView;
  expect(mutatingRequests).toEqual([]);
});

async function openSection(name: RegExp, heading: string) {
  const nav = await screen.findByRole("navigation", { name: "Sections" });
  await userEvent.click(await within(nav).findByRole("link", { name }));
  expect(await screen.findByRole("heading", { level: 2, name: heading })).toBeInTheDocument();
}

/** Mounts `App` on the units section with these unit fields. */
async function openUnits(fields: UnitFields, extra: Partial<ViewResponse> = {}) {
  serve(emptyView({ ...fields, ...extra }));
  render(<App />);
  await openSection(/^Systemd units/, "Systemd units");
  await screen.findByTestId("service-section");
}

function bar(): HTMLElement {
  return screen.getByRole("group", { name: "Show" });
}

async function selectFilter(label: string) {
  await userEvent.click(within(bar()).getByRole("button", { name: new RegExp(`^${label} \\d+$`) }));
}

function selectedFilter(): string {
  const pressed = within(bar())
    .getAllByRole("button")
    .filter((b) => b.getAttribute("aria-pressed") === "true");
  expect(pressed).toHaveLength(1);
  return (pressed[0].textContent ?? "").replace(/ \d+$/, "");
}

function groupHeader(title: string): HTMLElement {
  return screen.getByRole("button", { name: new RegExp(`^${title}\\b`) });
}

async function collapse(title: string) {
  const header = groupHeader(title);
  await userEvent.click(header);
  expect(header).toHaveAttribute("aria-expanded", "false");
}

/** In the DOM and under no `hidden` ancestor. */
function isShown(el: Element | null): boolean {
  return el !== null && el.closest("[hidden]") === null;
}

/** Sends the navigation `App` gave the section. */
function navigate(itemId: string) {
  expect(navigation.navigate).toBeTypeOf("function");
  act(() => navigation.navigate!("units", itemId));
}

async function expectFocused(testId: string) {
  await waitFor(() => {
    const rows = screen.getAllByTestId(testId);
    expect(rows[0]).toHaveFocus();
  });
  expect(isShown(document.activeElement)).toBe(true);
}

describe("reveal step 2: the outcome filter (§ 6.11)", () => {
  it("sets the filter to All when it hides the destination", async () => {
    await openUnits(kitchenSink);
    await selectFilter("Needs follow-up");
    expect(isShown(screen.queryByTestId("service-item-cups.socket"))).toBe(false);
    navigate("cups.socket");
    await expectFocused("service-item-cups.socket");
    expect(selectedFilter()).toBe("All");
  });
});

describe("reveal step 3: collapse (§ 6.11, § 6.2)", () => {
  it("opens the destination's collapsed group", async () => {
    await openUnits(kitchenSink);
    await collapse("Sockets");
    expect(isShown(screen.getByTestId("service-item-cups.socket"))).toBe(false);
    navigate("cups.socket");
    await expectFocused("service-item-cups.socket");
    expect(groupHeader("Sockets")).toHaveAttribute("aria-expanded", "true");
  });
});

describe("U26: a units search result, reveal half (§ 6.11)", () => {
  it("shows and focuses a row key's row under a filter and in a collapsed group", async () => {
    await openUnits(kitchenSink);
    await collapse("Sockets");
    await selectFilter("Needs follow-up");
    expect(isShown(screen.queryByTestId("service-item-cockpit.socket"))).toBe(false);
    navigate(JSON.stringify(["unit_state", "cockpit.socket"]));
    await expectFocused("service-item-cockpit.socket");
    expect(selectedFilter()).toBe("All");
    expect(groupHeader("Sockets")).toHaveAttribute("aria-expanded", "true");
  });
});

describe("the reveal lookup (§ 6.11 Which row an item id names)", () => {
  it("reads a bare name as the unit row, not a reported row of that name", async () => {
    await openUnits(planted);
    // Planted's shared name: a unit row, a mount finding and an undetermined install.
    expect(screen.getByTestId("mount-finding-data-demo.mount")).toBeInTheDocument();
    expect(screen.getByTestId("undetermined-install-data-demo.mount")).toBeInTheDocument();
    navigate("data-demo.mount");
    await expectFocused("service-item-data-demo.mount");
  });

  it("focuses the undetermined install a row key names, not the unit row whose name holds its test id", async () => {
    await openUnits(planted);
    navigate(JSON.stringify(["undetermined_install", "demo.service"]));
    await waitFor(() =>
      expect(document.activeElement?.getAttribute("data-unit-key")).toBe(
        JSON.stringify(["undetermined_install", "demo.service"]),
      ),
    );
    expect(screen.getByTestId("service-item-undetermined-install-demo.service")).not.toHaveFocus();
  });

  it("reads the bare name undetermined-install-demo.service as its unit row", async () => {
    await openUnits(planted);
    navigate("undetermined-install-demo.service");
    await expectFocused("service-item-undetermined-install-demo.service");
  });

  it("focuses nothing, and throws nothing, for a [ id that does not parse", async () => {
    await openUnits(planted);
    // Let the section's own focus on opening settle, then start from nothing focused.
    await act(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
    act(() => (document.activeElement as HTMLElement | null)?.blur());
    expect(document.activeElement).toBe(document.body);
    navigate('["unit_state"');
    // Two frames: App's lookup has run.
    await act(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
    expect(document.activeElement).toBe(document.body);
  });

  it("finds a unit whose name holds a backslash by bare name and by row key", async () => {
    const row: ServiceDecisionDto = {
      ...kitchenSink.unit_states.find((r) => r.unit === "myapp-report.timer")!,
      unit: ESCAPED_UNIT,
    };
    await openUnits({ ...kitchenSink, unit_states: [...kitchenSink.unit_states, row] });
    navigate(ESCAPED_UNIT);
    await expectFocused(`service-item-${ESCAPED_UNIT}`);
    act(() => (document.activeElement as HTMLElement).blur());
    navigate(JSON.stringify(["unit_state", ESCAPED_UNIT]));
    await expectFocused(`service-item-${ESCAPED_UNIT}`);
  });
});

describe('U25: "Go to owner" (§ 6.11)', () => {
  it("focuses the shadowing drop-in's row", async () => {
    serve(emptyView({ ...kitchenSink, config_files: [shadowedConfig(SHADOW_PATH)] }));
    render(<App />);
    await openSection(/^Config Files/, "Config Files");
    await userEvent.click(await screen.findByTestId("shadow-owner-link"));
    expect(await screen.findByRole("heading", { level: 2, name: "Systemd units" })).toBeInTheDocument();
    await expectFocused(`dropin-item-${SHADOW_PATH}`);
  });
});
