/**
 * A generated timer row and the collected timer of its name are two rows
 * with two ids (spec § 11.1). The generated row's id names its kind, so
 * the rows share no key, DOM id or search entry, the declined note sits
 * under the generated row, and search reaches the generated row.
 *
 * Mounts `App` with `fetch` as the only thing replaced. The section is
 * the one the server sends for that host: see
 * `api_test::a_generated_scheduled_row_has_its_own_id`.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import App from "../App";
import type { DeclinedGeneratedView, ReferenceSection } from "../api/types";
import { emptyView } from "../test-utils/emptyView";

const COLLECTED_ID = "cron-backup";
const GENERATED_ID = "generated:cron-backup";

/** The text the server sends for the collision, as the plan writes it. */
const COLLISION_TEXT =
  "cron-backup.timer (inspectah generates this unit, and a unit collected " +
  "from the host has its name, so the generated files are not written and " +
  "the unit is not enabled here; to carry the generated unit, exclude the " +
  "collected unit and its file)";

const DECLINED: DeclinedGeneratedView = {
  row: { row: "scheduled", name: "cron-backup" },
  unit: "cron-backup.timer",
  declined: {
    renders: "fixme",
    reason: "GeneratedNameCollision",
    text: COLLISION_TEXT,
  },
};

const SECTIONS: ReferenceSection[] = [
  {
    id: "scheduled_tasks",
    display_name: "Scheduled Tasks",
    items: [
      {
        id: COLLECTED_ID,
        title: "cron-backup",
        subtitle: "daily",
        detail: "/usr/bin/vendor-backup",
        searchable_text: "cron-backup daily /usr/bin/vendor-backup timer",
        unit: "cron-backup.timer",
        unit_row: false,
      },
      {
        id: GENERATED_ID,
        title: "cron-backup",
        subtitle: "0 2 * * *",
        detail: null,
        searchable_text: "cron-backup 0 2 * * * generated-timer",
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

const originalScrollIntoView = Element.prototype.scrollIntoView;

beforeEach(() => {
  mutatingRequests = [];
  // jsdom does not implement scrollIntoView, which the app calls when it
  // reveals a row it navigated to.
  Element.prototype.scrollIntoView = vi.fn();
  const routes: Record<string, unknown> = {
    "/api/health": HEALTH,
    "/api/groups": GROUPS,
    "/api/view": emptyView({ declined_generated: [DECLINED] }),
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
  Element.prototype.scrollIntoView = originalScrollIntoView;
  expect(mutatingRequests).toEqual([]);
});

describe("a generated row and the collected timer of its name", () => {
  it("are two rows, with the declined note under the generated one", async () => {
    render(<App />);
    const nav = await screen.findByRole("navigation", { name: "Sections" });
    await userEvent.click(
      await within(nav).findByRole("link", { name: /^Scheduled Tasks/ }),
    );
    const list = await screen.findByRole("list", {
      name: "Scheduled Tasks context items",
    });
    const rows = within(list).getAllByRole("listitem");
    expect(rows.map((r) => r.getAttribute("data-testid"))).toEqual([
      `context-item-${COLLECTED_ID}`,
      `context-item-${GENERATED_ID}`,
    ]);
    expect(
      document.querySelectorAll(`[id="context-item-${COLLECTED_ID}"]`),
    ).toHaveLength(1);
    const note = within(list).getByText(COLLISION_TEXT);
    expect(rows[1]).toContainElement(note);
    expect(rows[0]).not.toContainElement(note);
  });

  it("search reaches and focuses the generated row", async () => {
    render(<App />);
    const input = await screen.findByLabelText("Search all sections");
    await userEvent.type(input, "generated-timer");
    const results = await screen.findByTestId("global-search-results");
    const options = within(results).getAllByRole("option");
    expect(options).toHaveLength(1);
    await userEvent.click(options[0]);
    const row = await screen.findByTestId(`context-item-${GENERATED_ID}`);
    await waitFor(() => expect(row).toHaveFocus());
    expect(
      screen.getByTestId(`context-item-${COLLECTED_ID}`),
    ).not.toHaveFocus();
  });
});
