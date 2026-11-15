/**
 * A generated timer row is decided on its own row (spec § 8, § 11.1).
 *
 * Mounts `App` with `fetch` as the only thing replaced. The stub keeps
 * the row's state the way the session does: `SetInclude` on
 * `GeneratedTimer` changes it and returns the new view, whose preview
 * carries the timer's enable only while the row is included.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import App from "../App";
import type { GeneratedTimerDto, ReferenceSection } from "../api/types";
import { emptyView } from "../test-utils/emptyView";

const NAME = "cron-backup";
const ROW_ID = `generated:0:${NAME}`;
const CRON_PATH = "/etc/cron.d/backup";
const ENABLE = "RUN systemctl enable -- \\\n    'cron-backup.timer'";

const SECTIONS: ReferenceSection[] = [
  {
    id: "scheduled_tasks",
    display_name: "Scheduled Tasks",
    items: [
      {
        id: `cron:${CRON_PATH}`,
        title: "backup",
        subtitle: "file",
        detail: null,
        searchable_text: "backup file cron",
      },
      {
        id: ROW_ID,
        title: NAME,
        subtitle: "0 2 * * *",
        detail: null,
        searchable_text: `${NAME} 0 2 * * * generated-timer`,
        generated_timer: NAME,
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
      { id: "scheduled_tasks", label: "Scheduled Tasks", is_triage: false },
    ],
    has_actionable_sections: true,
  },
];

/** The session's state of the generated row, which the stub updates. */
let row: GeneratedTimerDto;
/** Every `/api/op` body the app sent. */
let ops: unknown[];

function view() {
  return emptyView({
    generated_timers: [row],
    containerfile_preview: row.include ? ENABLE : "",
  });
}

beforeEach(() => {
  row = { name: NAME, include: true, locked: false };
  ops = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      if (url === "/api/op" && init?.method === "POST") {
        const body = JSON.parse(String(init.body));
        ops.push(body);
        row = { ...row, include: body.target.include };
        return { ok: true, status: 200, json: async () => view() };
      }
      const routes: Record<string, unknown> = {
        "/api/health": HEALTH,
        "/api/groups": GROUPS,
        "/api/view": view(),
        "/api/snapshot/sections": SECTIONS,
        "/api/viewed": { ids: [] },
        "/api/ops": [],
      };
      if (!(url in routes)) {
        return { ok: false, status: 404, json: async () => ({ error: url }) };
      }
      return { ok: true, status: 200, json: async () => routes[url] };
    }),
  );
});

afterEach(() => {
  vi.unstubAllGlobals();
});

async function generatedRow() {
  render(<App />);
  const nav = await screen.findByRole("navigation", { name: "Sections" });
  await userEvent.click(
    await within(nav).findByRole("link", { name: /^Scheduled Tasks/ }),
  );
  return screen.findByTestId(`context-item-${ROW_ID}`);
}

function setIncludeOp(include: boolean) {
  return {
    kind: "Op",
    op: "SetInclude",
    target: {
      item_id: { kind: "GeneratedTimer", key: { name: NAME } },
      include,
    },
  };
}

describe("generated timer row control", () => {
  it("excludes the row and includes it again", async () => {
    const rowEl = await generatedRow();
    const toggle = within(rowEl).getByRole("checkbox", {
      name: `Toggle ${NAME}`,
    });
    expect(toggle).toBeChecked();
    // The cron row has no control of its own here.
    expect(
      within(screen.getByTestId(`context-item-cron:${CRON_PATH}`)).queryByRole(
        "checkbox",
      ),
    ).toBeNull();

    await userEvent.click(toggle);
    await waitFor(() => expect(toggle).not.toBeChecked());
    expect(ops).toEqual([setIncludeOp(false)]);

    await userEvent.click(toggle);
    await waitFor(() => expect(toggle).toBeChecked());
    expect(ops).toEqual([setIncludeOp(false), setIncludeOp(true)]);
  });

  it("offers no toggle while its cron job is excluded, and says why", async () => {
    row = {
      name: NAME,
      include: false,
      locked: false,
      cron_job_excluded: CRON_PATH,
    };
    const rowEl = await generatedRow();
    expect(within(rowEl).queryByRole("checkbox")).toBeNull();
    expect(
      within(rowEl).getByText(
        `Excluded with its cron job ${CRON_PATH}; include the cron job to carry this unit.`,
      ),
    ).toBeInTheDocument();
    expect(ops).toEqual([]);
  });

  it("offers exclude while another source of the name is still included", async () => {
    row = {
      name: NAME,
      include: true,
      locked: false,
      cron_job_excluded: CRON_PATH,
    };
    const rowEl = await generatedRow();
    const toggle = within(rowEl).getByRole("checkbox", {
      name: `Toggle ${NAME}`,
    });
    expect(toggle).toBeChecked();
    expect(within(rowEl).queryByText(/Excluded with its cron job/)).toBeNull();
  });
});
