/// <reference types="vite/client" />
/**
 * The machine-state block in the Configuration section, against a stubbed
 * fetch that answers with the views the real handlers return
 * (`machine-state.view.json` and, after an include, the recorded
 * `machine-state-included.view.json`), so the assertions are on the
 * request body the server receives and on what the operator sees after.
 */
import { useState } from "react";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MainContent } from "../MainContent";
import type { RefinedConfig, ViewResponse } from "../../api/types";
import { emptyView } from "../../test-utils/emptyView";
import { loadUnitFields } from "../../__tests__/fixtures/systemd-units/load";

const FIXTURES = import.meta.glob<{ default: ViewResponse }>(
  "../../__tests__/fixtures/system-tuning/machine-state*.view.json",
  { eager: true },
);

function recorded(name: string): ViewResponse {
  const entry =
    FIXTURES[`../../__tests__/fixtures/system-tuning/${name}.view.json`];
  if (!entry) throw new Error(`no recorded fixture ${name}.view.json`);
  return entry.default;
}

const VIEW = recorded("machine-state");
const INCLUDED = recorded("machine-state-included");

const HOSTNAME = "/etc/hostname";
const IQN = "/etc/iscsi/initiatorname.iscsi";
const KEYTAB = "/etc/krb5.keytab";

type Reply =
  | { status: 200; view: ViewResponse }
  | { status: number; error: string };

let replies: Reply[] = [];
let bodies: unknown[] = [];

beforeEach(() => {
  replies = [];
  bodies = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      if (url !== "/api/op") throw new Error(`unexpected fetch: ${url}`);
      bodies.push(JSON.parse(String(init?.body)));
      const reply = replies.shift();
      if (!reply) throw new Error("no reply queued");
      if ("view" in reply) {
        return { ok: true, status: 200, json: async () => reply.view };
      }
      return {
        ok: false,
        status: reply.status,
        json: async () => ({ error: reply.error }),
      };
    }),
  );
});
afterEach(() => {
  vi.unstubAllGlobals();
});

function Harness({
  initial,
  sectionSearchOpen = false,
}: {
  initial: ViewResponse;
  sectionSearchOpen?: boolean;
}) {
  const [viewData, setViewData] = useState(initial);
  return (
    <MainContent
      activeSection="configs"
      loading={false}
      viewData={viewData}
      sections={null}
      onViewUpdate={setViewData}
      onMutationError={vi.fn()}
      sectionSearchOpen={sectionSearchOpen}
      onSectionSearchClose={vi.fn()}
    />
  );
}

function rowEl(path: string): HTMLElement {
  return screen.getByTestId(`machine-state-row-${path}`);
}

async function expand(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("button", { name: /^Machine state/ }));
}

describe("MainContent: the machine-state block", () => {
  it("sits at the foot of Configuration, after the decision list", () => {
    render(<Harness initial={VIEW} />);
    const block = screen.getByTestId("machine-state-group");
    const sysctlRow = screen.getByTestId(
      "decision-item-configs:/etc/sysctl.d/90-x.conf",
    );
    expect(
      sysctlRow.compareDocumentPosition(block) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    // Class rows are in no other list.
    expect(
      screen.queryByTestId(`decision-item-configs:${HOSTNAME}`),
    ).toBeNull();
  });

  it("stays rendered under the all-excluded empty state", () => {
    const excluded: RefinedConfig[] = VIEW.config_files.map((c) => ({
      ...c,
      entry: { ...c.entry, disposition: { kind: "actionable", include: false } },
    }));
    render(<Harness initial={{ ...VIEW, config_files: excluded }} />);
    expect(screen.getByTestId("config-all-excluded")).toBeInTheDocument();
    expect(screen.getByTestId("machine-state-group")).toBeInTheDocument();
  });

  it("counts block matches in a search and keeps the block", async () => {
    const user = userEvent.setup();
    render(<Harness initial={VIEW} sectionSearchOpen />);
    await user.type(screen.getByLabelText("Filter section items"), "keytab");
    expect(screen.queryByText("No items match your search")).toBeNull();
    expect(rowEl(KEYTAB)).toBeVisible();
    expect(screen.getByText("1 match")).toBeInTheDocument();

    await user.clear(screen.getByLabelText("Filter section items"));
    await user.type(screen.getByLabelText("Filter section items"), "no-such-path");
    expect(screen.getByText("No items match your search")).toBeInTheDocument();
    expect(screen.getByTestId("machine-state-group")).toBeInTheDocument();
  });

  it("sends the row's item_id verbatim after the warn dialog, and focus lands on Exclude", async () => {
    const user = userEvent.setup();
    render(<Harness initial={VIEW} />);
    await expand(user);
    replies.push({ status: 200, view: INCLUDED });

    await user.click(
      within(rowEl(IQN)).getByRole("button", { name: `Include ${IQN}…` }),
    );
    await user.click(
      await screen.findByRole("button", { name: "Include in image" }),
    );

    expect(bodies).toEqual([
      {
        kind: "Op",
        op: "SetInclude",
        target: {
          item_id: { kind: "Config", key: { path: IQN } },
          include: true,
        },
      },
    ]);
    const exclude = await within(rowEl(IQN)).findByRole("button", {
      name: `Exclude ${IQN}`,
    });
    await waitFor(() => expect(exclude).toHaveFocus());
    expect(rowEl(IQN)).toHaveTextContent("Included by you");
    expect(screen.getByRole("status")).toHaveTextContent(
      `${IQN} included in the image`,
    );
  });

  it("shows a not-carried refusal on the row and leaves it as the server has it", async () => {
    const user = userEvent.setup();
    render(<Harness initial={VIEW} />);
    await expand(user);
    replies.push({
      status: 422,
      error: `not carried: ${HOSTNAME} is withheld`,
    });

    const include = within(rowEl(HOSTNAME)).getByRole("button", {
      name: `Include ${HOSTNAME}`,
    });
    await user.click(include);

    const message = await within(rowEl(HOSTNAME)).findByTestId(
      `machine-state-failure-${HOSTNAME}`,
    );
    expect(message).toHaveTextContent(
      `inspectah does not carry this file: ${HOSTNAME} is withheld. Nothing was changed.`,
    );
    // Not alarming: an info message, not a danger or warning.
    expect(message.className).toMatch(/pf-m-info/);
    await waitFor(() => expect(include).toBeEnabled());
    expect(include).toHaveTextContent("Include");
    expect(rowEl(HOSTNAME)).not.toHaveTextContent("Included by you");
  });

  it("shows any other failure on the row as not saved", async () => {
    const user = userEvent.setup();
    render(<Harness initial={VIEW} />);
    await expand(user);
    replies.push({ status: 500, error: "internal error" });

    await user.click(
      within(rowEl(HOSTNAME)).getByRole("button", {
        name: `Include ${HOSTNAME}`,
      }),
    );
    const message = await within(rowEl(HOSTNAME)).findByTestId(
      `machine-state-failure-${HOSTNAME}`,
    );
    expect(message).toHaveTextContent(
      "The change was not saved: internal error",
    );
    expect(message.className).toMatch(/pf-m-warning/);
  });

  it("links a unit's required_config path to the row that explains it", async () => {
    // The planted wg-quick@demo-a.service reads this path; here it is a
    // machine-state row, not a config row.
    const DEMO_A = "/etc/wireguard/demo-a.conf";
    const row = { ...VIEW.machine_state[0], path: DEMO_A };
    const onNavigateToItem = vi.fn();
    render(
      <MainContent
        activeSection="units"
        loading={false}
        viewData={emptyView({
          ...loadUnitFields("planted"),
          machine_state: [row],
        })}
        sections={null}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
        sectionSearchOpen={false}
        onSectionSearchClose={vi.fn()}
        onNavigateToItem={onNavigateToItem}
      />,
    );
    const link = within(
      screen.getByTestId("required-config-wg-quick@demo-a.service"),
    ).getByTestId("required-config-path");
    expect(link.tagName).toBe("A");
    await userEvent.click(link);
    expect(onNavigateToItem).toHaveBeenCalledWith(
      "config",
      `machine-state:${DEMO_A}`,
    );
  });
});
