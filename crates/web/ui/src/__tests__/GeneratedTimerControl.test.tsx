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
import { mockStats } from "../test-utils/mockStats";

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
      { id: "units", label: "Services", is_triage: true },
      { id: "scheduled_tasks", label: "Scheduled Tasks", is_triage: false },
    ],
    has_actionable_sections: true,
  },
];

/** The session's state of the generated row, which the stub updates. */
let row: GeneratedTimerDto;
/** Every `/api/op` body the app sent. */
let ops: unknown[];
/**
 * When set, the next `/api/op` waits on this promise and fails with its
 * value as the server's error, instead of applying.
 */
let failNextOp: Promise<string> | null;

function view() {
  return emptyView({
    generated_timers: [row],
    containerfile_preview: row.include ? ENABLE : "",
    // Undo and Redo stay available, so a test can step either way.
    stats: mockStats({ ops_applied: 1, can_undo: true, can_redo: true }),
  });
}

/** History steps the app sent, in order. */
let steps: string[];

beforeEach(() => {
  row = { name: NAME, include: true, locked: false };
  ops = [];
  failNextOp = null;
  steps = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      if (url === "/api/op" && init?.method === "POST") {
        const body = JSON.parse(String(init.body));
        ops.push(body);
        if (failNextOp) {
          const failing = failNextOp;
          failNextOp = null;
          const error = await failing;
          return { ok: false, status: 409, json: async () => ({ error }) };
        }
        row = { ...row, include: body.target.include };
        return { ok: true, status: 200, json: async () => view() };
      }
      // A step through history that changes only the generated row: no
      // user record is in either view.
      if (
        (url === "/api/undo" || url === "/api/redo") &&
        init?.method === "POST"
      ) {
        steps.push(url);
        row = { ...row, include: !row.include };
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

async function openSection(name: RegExp) {
  const nav = await screen.findByRole("navigation", { name: "Sections" });
  await userEvent.click(await within(nav).findByRole("link", { name }));
}

async function generatedRow() {
  render(<App />);
  await openSection(/^Scheduled Tasks/);
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

  it("keeps a failure across a section switch, announces it, and retries", async () => {
    let fail!: (error: string) => void;
    failNextOp = new Promise<string>((resolve) => {
      fail = resolve;
    });
    const rowEl = await generatedRow();
    const toggle = within(rowEl).getByRole("checkbox", {
      name: `Toggle ${NAME}`,
    });
    await userEvent.click(toggle);
    // Pending: the toggle is disabled while the request is in flight.
    await waitFor(() => expect(toggle).toBeDisabled());

    // The operator leaves the section before the request settles.
    await openSection(/^Services/);
    fail("the session changed underneath this request");
    await openSection(/^Scheduled Tasks/);

    const back = await screen.findByTestId(`context-item-${ROW_ID}`);
    const message = await within(back).findByText(
      `Could not exclude ${NAME}: the session changed underneath this request`,
    );
    expect(message.closest("[aria-live]")).not.toBeNull();
    const retry = within(back).getByRole("checkbox", {
      name: `Toggle ${NAME}`,
    });
    expect(retry).toBeChecked();
    expect(retry).toBeEnabled();

    // Retry: the request goes out again, succeeds, and the failure clears.
    await userEvent.click(retry);
    await waitFor(() => expect(retry).not.toBeChecked());
    expect(within(back).queryByText(/Could not exclude/)).toBeNull();
    expect(ops).toEqual([setIncludeOp(false), setIncludeOp(false)]);
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

  describe.each(["Undo", "Redo"])("a timer-only %s", (step) => {
    it("ends a failure already on the row", async () => {
      let fail!: (error: string) => void;
      failNextOp = new Promise<string>((resolve) => {
        fail = resolve;
      });
      const rowEl = await generatedRow();
      await userEvent.click(
        within(rowEl).getByRole("checkbox", { name: `Toggle ${NAME}` }),
      );
      fail("the session changed underneath this request");
      expect(
        await within(rowEl).findByText(/^Could not exclude/),
      ).toBeInTheDocument();

      await userEvent.click(screen.getByRole("button", { name: step }));
      await waitFor(() => expect(steps).toHaveLength(1));
      await waitFor(() =>
        expect(
          within(screen.getByTestId(`context-item-${ROW_ID}`)).queryByText(
            /^Could not exclude/,
          ),
        ).toBeNull(),
      );
    });

    it("records nothing for a request that fails after it", async () => {
      let fail!: (error: string) => void;
      failNextOp = new Promise<string>((resolve) => {
        fail = resolve;
      });
      const rowEl = await generatedRow();
      await userEvent.click(
        within(rowEl).getByRole("checkbox", { name: `Toggle ${NAME}` }),
      );
      // The step lands while the toggle's request is still in flight.
      await userEvent.click(screen.getByRole("button", { name: step }));
      await waitFor(() => expect(steps).toHaveLength(1));
      fail("the session changed underneath this request");

      const back = screen.getByTestId(`context-item-${ROW_ID}`);
      const toggle = within(back).getByRole("checkbox", {
        name: `Toggle ${NAME}`,
      });
      await waitFor(() => expect(toggle).toBeEnabled());
      expect(within(back).queryByText(/^Could not/)).toBeNull();
    });
  });
});
