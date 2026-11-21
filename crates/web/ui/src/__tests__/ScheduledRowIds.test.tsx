/**
 * Scheduled rows that share a name are distinct rows (spec § 11.1). The
 * server gives each row an id of its kind and its source, so no two rows
 * share a key, DOM id or search entry; the declined note joins on the
 * generated row's `generated_timer` name, not on an id; and search and
 * reveal land on the row searched for.
 *
 * Mounts `App` with `fetch` as the only thing replaced. The ids are the
 * ones the server sends: see `api_test::each_scheduled_row_has_its_own_id`.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import App from "../App";
import type { DeclinedGeneratedView, ReferenceSection } from "../api/types";
import { emptyView } from "../test-utils/emptyView";

/** The ids the server sends: each row's kind and source. */
const ETC_TIMER_ID = "timer:/etc/systemd/system/backup.timer";
const VENDOR_TIMER_ID = "timer:/usr/lib/systemd/system/backup.timer";
const PREFIXED_TIMER_ID =
  "timer:/etc/systemd/system/generated:cron-backup.timer";
const GENERATED_ID = "generated:0:cron-backup";
/**
 * A timer template and two of its instances, each instance described from
 * the template's file, so all three record one path: see
 * `api_test::a_timer_template_and_its_instances_have_their_own_ids`.
 */
const WORKER_TEMPLATE_PATH = "etc/systemd/system/worker@.timer";
const WORKER_TEMPLATE_ID = `timer:${WORKER_TEMPLATE_PATH}`;
const WORKER_A_ID = `timer-instance:${JSON.stringify([WORKER_TEMPLATE_PATH, "worker@a"])}`;
const WORKER_B_ID = `timer-instance:${JSON.stringify([WORKER_TEMPLATE_PATH, "worker@b"])}`;

/** The text the server sends for the collision, as the plan writes it. */
const COLLISION_TEXT =
  "cron-backup.timer (inspectah generates this unit, and a unit collected " +
  "from the host, cron-backup.timer, has the name of one of its files, so " +
  "the generated files are not written and the unit is not enabled here; " +
  "to carry the generated unit, exclude the collected unit and its file)";

const DECLINED: DeclinedGeneratedView = {
  row: { row: "scheduled", name: "cron-backup" },
  unit: "cron-backup.timer",
  declined: {
    renders: "fixme",
    reason: "GeneratedNameCollision",
    text: COLLISION_TEXT,
  },
};

/**
 * Every pair of rows that once shared an id: a shadowed timer's `/etc`
 * and vendor descriptions, and a collected timer named
 * `generated:cron-backup.timer` beside the generated `cron-backup` row.
 */
const SECTIONS: ReferenceSection[] = [
  {
    id: "scheduled_tasks",
    display_name: "Scheduled Tasks",
    items: [
      {
        id: ETC_TIMER_ID,
        title: "backup",
        subtitle: "daily",
        detail: null,
        searchable_text: "backup daily local backup timer",
        unit: "backup.timer",
        unit_row: false,
      },
      {
        id: VENDOR_TIMER_ID,
        title: "backup",
        subtitle: "weekly",
        detail: null,
        searchable_text: "backup weekly vendor backup timer",
        unit: "backup.timer",
        unit_row: false,
      },
      {
        id: PREFIXED_TIMER_ID,
        title: "generated:cron-backup",
        subtitle: "hourly",
        detail: null,
        searchable_text: "generated:cron-backup hourly timer",
        unit: "generated:cron-backup.timer",
        unit_row: false,
      },
      {
        id: GENERATED_ID,
        title: "cron-backup",
        subtitle: "0 2 * * *",
        detail: null,
        searchable_text: "cron-backup 0 2 * * * generated-timer",
        generated_timer: "cron-backup",
      },
      ...(
        [
          [WORKER_TEMPLATE_ID, "worker@"],
          [WORKER_A_ID, "worker@a"],
          [WORKER_B_ID, "worker@b"],
        ] as const
      ).map(([id, name]) => ({
        id,
        title: name,
        subtitle: "daily",
        detail: "Worker %i\n/usr/local/bin/worker %i",
        searchable_text: `${name} daily Worker %i\n/usr/local/bin/worker %i timer`,
        unit: `${name}.timer`,
        unit_row: false,
      })),
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
  expect(mutatingRequests).toEqual([]);
});

/** Open Scheduled Tasks and return its rows. */
async function scheduledRows() {
  render(<App />);
  const nav = await screen.findByRole("navigation", { name: "Sections" });
  await userEvent.click(
    await within(nav).findByRole("link", { name: /^Scheduled Tasks/ }),
  );
  const list = await screen.findByRole("list", {
    name: "Scheduled Tasks context items",
  });
  return within(list).getAllByRole("listitem");
}

/** Search all sections for `query`, select its one result. */
async function searchAndSelect(query: string) {
  const input = await screen.findByLabelText("Search all sections");
  await userEvent.type(input, query);
  const results = await screen.findByTestId("global-search-results");
  const options = within(results).getAllByRole("option");
  expect(options).toHaveLength(1);
  await userEvent.click(options[0]);
}

describe("scheduled rows that share a name", () => {
  it("are distinct rows, with the declined note under the generated one", async () => {
    const rows = await scheduledRows();
    expect(rows.map((r) => r.getAttribute("data-testid"))).toEqual([
      `context-item-${ETC_TIMER_ID}`,
      `context-item-${VENDOR_TIMER_ID}`,
      `context-item-${PREFIXED_TIMER_ID}`,
      `context-item-${GENERATED_ID}`,
      `context-item-${WORKER_TEMPLATE_ID}`,
      `context-item-${WORKER_A_ID}`,
      `context-item-${WORKER_B_ID}`,
    ]);
    for (const id of [
      ETC_TIMER_ID,
      VENDOR_TIMER_ID,
      PREFIXED_TIMER_ID,
      GENERATED_ID,
      WORKER_TEMPLATE_ID,
      WORKER_A_ID,
      WORKER_B_ID,
    ]) {
      // Compared as strings: an id is host data, and an instance's holds
      // quotes, which would end a CSS attribute selector built from it.
      expect(
        Array.from(document.querySelectorAll("[id]")).filter(
          (el) => el.id === `context-item-${id}`,
        ),
      ).toHaveLength(1);
    }
    const notes = screen.getAllByText(COLLISION_TEXT);
    expect(notes).toHaveLength(1);
    expect(rows[3]).toContainElement(notes[0]);
  });

  it("search reaches and focuses the generated row", async () => {
    render(<App />);
    await searchAndSelect("generated-timer");
    const row = await screen.findByTestId(`context-item-${GENERATED_ID}`);
    await waitFor(() => expect(row).toHaveFocus());
  });

  it("search reaches and focuses the vendor description of a shadowed timer", async () => {
    render(<App />);
    await searchAndSelect("vendor backup");
    const row = await screen.findByTestId(`context-item-${VENDOR_TIMER_ID}`);
    await waitFor(() => expect(row).toHaveFocus());
    expect(
      screen.getByTestId(`context-item-${ETC_TIMER_ID}`),
    ).not.toHaveFocus();
  });

  it("search reaches the collected timer whose name starts with generated:", async () => {
    render(<App />);
    await searchAndSelect("hourly");
    const row = await screen.findByTestId(`context-item-${PREFIXED_TIMER_ID}`);
    await waitFor(() => expect(row).toHaveFocus());
  });

  it.each([
    ["worker@a", WORKER_A_ID],
    ["worker@b", WORKER_B_ID],
  ])(
    "search reaches and focuses instance %s's own row, not its template's",
    async (query, id) => {
      render(<App />);
      await searchAndSelect(query);
      const row = await screen.findByTestId(`context-item-${id}`);
      await waitFor(() => expect(row).toHaveFocus());
      for (const other of [WORKER_TEMPLATE_ID, WORKER_A_ID, WORKER_B_ID]) {
        if (other !== id) {
          expect(screen.getByTestId(`context-item-${other}`)).not.toHaveFocus();
        }
      }
    },
  );
});
