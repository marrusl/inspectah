/**
 * A scheduled timer row links to its unit row (spec § 11.1, T20).
 *
 * The timer row is a description: its state is decided on the unit row
 * and its file on the config row, so the row offers no control of its
 * own. Mounts `App` with `fetch` as the only thing replaced, opens
 * Scheduled Tasks, follows the timer's link, and checks the unit row it
 * lands on.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import App from "../App";
import type { ReferenceSection, ServiceDecisionDto } from "../api/types";
import { emptyView } from "../test-utils/emptyView";

const TIMER_UNIT = "backup.timer";
const UNROWED_UNIT = "cleanup.timer";
/**
 * A valid unit name holding a literal backslash, as systemd escapes a
 * dash. In a CSS selector the backslash would start an escape.
 */
const ESCAPED_UNIT = "backup\\x2djob.timer";

const UNIT_ROW: ServiceDecisionDto = {
  unit: TIMER_UNIT,
  kind: "timer",
  fragment: "local",
  triage: {
    triage: { mode: "single_host", site: null },
    primary_reason: "service_non_default_state",
    annotations: [],
  },
  include: true,
  locked: false,
  current_state: "enabled",
  required_config: [],
  data_paths: [],
};

const ESCAPED_UNIT_ROW: ServiceDecisionDto = {
  ...UNIT_ROW,
  unit: ESCAPED_UNIT,
};

const SECTIONS: ReferenceSection[] = [
  {
    id: "scheduled_tasks",
    display_name: "Scheduled Tasks",
    items: [
      {
        id: "/etc/cron.d/nightly",
        title: "nightly",
        subtitle: "file",
        detail: null,
        searchable_text: "nightly file cron",
      },
      {
        id: "backup",
        title: "backup",
        subtitle: "daily",
        detail: null,
        searchable_text: "backup daily timer",
        unit: TIMER_UNIT,
        unit_row: true,
      },
      {
        id: "backup\\x2djob",
        title: "backup\\x2djob",
        subtitle: "hourly",
        detail: null,
        searchable_text: "backup\\x2djob hourly timer",
        unit: ESCAPED_UNIT,
        unit_row: true,
      },
      {
        id: "cleanup",
        title: "cleanup",
        subtitle: "weekly",
        detail: null,
        searchable_text: "cleanup weekly timer",
        unit: UNROWED_UNIT,
        unit_row: false,
      },
    ],
  },
];

const HEALTH = {
  status: "ready",
  host: {
    hostname: "testhost",
    os_name: "RHEL",
    os_version: "9.4",
    os_id: "rhel",
    system_type: "physical",
    schema_version: 1,
  },
  completeness: "full",
  policy: { distro_repos: ["baseos", "appstream"] },
};

const GROUPS = [
  {
    slug: "services-scheduling",
    label: "Services & Scheduling",
    sections: [
      { id: "units", label: "Services", is_triage: true },
      { id: "scheduled_tasks", label: "Scheduled Tasks", is_triage: false },
    ],
    has_actionable_sections: true,
  },
];

/** Requests the app sent that would change the session. */
let mutatingRequests: string[] = [];

beforeEach(() => {
  mutatingRequests = [];
  const routes: Record<string, unknown> = {
    "/api/health": HEALTH,
    "/api/groups": GROUPS,
    "/api/view": emptyView({ unit_states: [UNIT_ROW, ESCAPED_UNIT_ROW] }),
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
});

afterEach(() => {
  vi.unstubAllGlobals();
  expect(mutatingRequests).toEqual([]);
});

async function openScheduledTasks() {
  render(<App />);
  const nav = await screen.findByRole("navigation", { name: "Sections" });
  await userEvent.click(
    await within(nav).findByRole("link", { name: /^Scheduled Tasks/ }),
  );
  expect(
    await screen.findByRole("heading", { level: 2, name: "Scheduled Tasks" }),
  ).toBeInTheDocument();
  return screen.getByRole("list", { name: "Scheduled Tasks context items" });
}

describe("scheduled timer row", () => {
  it("offers no include control on any scheduled row", async () => {
    const list = await openScheduledTasks();
    expect(within(list).getAllByRole("listitem")).toHaveLength(4);
    expect(within(list).queryAllByRole("checkbox")).toHaveLength(0);
    expect(within(list).queryAllByRole("switch")).toHaveLength(0);
  });

  it("names a unit with no row as text", async () => {
    const list = await openScheduledTasks();
    const row = within(list).getByTestId("context-item-cleanup");
    expect(within(row).getByText(UNROWED_UNIT)).toBeInTheDocument();
    expect(within(row).queryByRole("link")).toBeNull();
  });

  it("opens the timer's unit row from its link", async () => {
    const list = await openScheduledTasks();
    await userEvent.click(within(list).getByRole("link", { name: TIMER_UNIT }));
    expect(
      await screen.findByRole("heading", { level: 2, name: "Systemd units" }),
    ).toBeInTheDocument();
    // The app reveals and focuses the row it was sent to.
    const row = await screen.findByTestId(`service-item-${TIMER_UNIT}`);
    await waitFor(() => expect(row).toHaveFocus());
  });

  it("opens a unit row whose name holds a backslash", async () => {
    const list = await openScheduledTasks();
    await userEvent.click(
      within(list).getByRole("link", { name: ESCAPED_UNIT }),
    );
    expect(
      await screen.findByRole("heading", { level: 2, name: "Systemd units" }),
    ).toBeInTheDocument();
    const row = await screen.findByTestId(`service-item-${ESCAPED_UNIT}`);
    await waitFor(() => expect(row).toHaveFocus());
  });
});

describe("U27: the units section resets on remount (§ 6.11 Routes)", () => {
  /** A service row the export declines, so the filter bar is drawn. */
  const DECLINED_ROW: ServiceDecisionDto = {
    ...UNIT_ROW,
    unit: "declined-demo.service",
    kind: "service",
    declined: {
      renders: "advisory",
      reason: "InstallEvidenceChanged",
      text: "declined-demo.service is not carried.",
    },
  };

  it("shows All, every group open, and the unit row focused", async () => {
    // Serve a view with the declined row; every other route as above.
    const served = fetch;
    vi.stubGlobal(
      "fetch",
      vi.fn(async (url: string, init?: RequestInit) =>
        url === "/api/view"
          ? {
              ok: true,
              status: 200,
              json: async () =>
                emptyView({ unit_states: [UNIT_ROW, ESCAPED_UNIT_ROW, DECLINED_ROW] }),
            }
          : served(url, init),
      ),
    );
    render(<App />);
    const nav = await screen.findByRole("navigation", { name: "Sections" });
    await userEvent.click(await within(nav).findByRole("link", { name: /^Services/ }));
    const header = (title: string) =>
      within(screen.getByTestId("service-section")).getByRole("button", {
        name: new RegExp(`^${title}\\b`),
      });
    for (const title of ["Timers", "Services"]) {
      await userEvent.click(await waitFor(() => header(title)));
      expect(header(title)).toHaveAttribute("aria-expanded", "false");
    }
    const bar = screen.getByRole("group", { name: "Show" });
    // "Carried" keeps the timer row, so a filter that survived the remount
    // would still read "Carried" after the reveal: nothing hides the row.
    await userEvent.click(within(bar).getByRole("button", { name: /^Carried \d+$/ }));
    expect(screen.getByTestId(`service-item-${TIMER_UNIT}`)).toBeInTheDocument();

    await userEvent.click(
      await within(nav).findByRole("link", { name: /^Scheduled Tasks/ }),
    );
    const list = await screen.findByRole("list", { name: "Scheduled Tasks context items" });
    await userEvent.click(within(list).getByRole("link", { name: TIMER_UNIT }));

    const row = await screen.findByTestId(`service-item-${TIMER_UNIT}`);
    await waitFor(() => expect(row).toHaveFocus());
    const pressed = within(screen.getByRole("group", { name: "Show" }))
      .getAllByRole("button")
      .filter((b) => b.getAttribute("aria-pressed") === "true");
    expect(pressed.map((b) => (b.textContent ?? "").replace(/ \d+$/, ""))).toEqual(["All"]);
    for (const title of ["Timers", "Services"]) {
      expect(header(title)).toHaveAttribute("aria-expanded", "true");
    }
  });
});
