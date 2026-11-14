/**
 * The Not carried group inside the packages view, against a stubbed fetch
 * so the assertion is on the request body the server actually receives.
 *
 * The fake server applies the projection contract
 * (process-docs/skills/not-carried-auto-packages.md, Refine projection
 * contract): an include removes the row and adds the package to `packages`
 * as an included decision; a re-exclude puts the row back.
 */
import { useState } from "react";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MainContent } from "../MainContent";
import type {
  NotCarriedRow,
  RefinedPackage,
  ViewResponse,
} from "../../api/types";
import { mockStats } from "../../test-utils/mockStats";

const FIREWALLD: NotCarriedRow = {
  item_id: { kind: "Package", key: { name: "firewalld", arch: "noarch" } },
  display_name: "firewalld",
  source_repo: "baseos",
  enabled_units: ["firewalld.service"],
  owns_enabled_unit: true,
};
const PYTHON3_FIREWALL: NotCarriedRow = {
  item_id: {
    kind: "Package",
    key: { name: "python3-firewall", arch: "noarch" },
  },
  display_name: "python3-firewall",
  source_repo: "baseos",
  enabled_units: [],
  owns_enabled_unit: false,
};

function packageFor(row: NotCarriedRow, include: boolean): RefinedPackage {
  return {
    entry: {
      name: row.item_id.key.name,
      epoch: "0",
      version: "1.3.4",
      release: "1.el10",
      arch: row.item_id.key.arch,
      state: "added",
      disposition: { kind: "actionable", include },
      source_repo: row.source_repo,
      aggregate: null,
    },
    triage: {
      triage: { mode: "single_host", baseline: null },
      primary_reason: "package_baseline_match",
      annotations: [],
    },
  } as RefinedPackage;
}

function makeView(
  rows: NotCarriedRow[],
  packages: RefinedPackage[],
): ViewResponse {
  return {
    packages,
    config_files: [],
    containerfile_preview: "",
    stats: mockStats(),
    generation: 1,
    repo_groups: [],
    version_changes: [],
    unit_states: [],
    unit_dropins: [],
    mount_findings: [],
    quadlets: [],
    flatpaks: [],
    declined_generated: [],
    sysctl: { state: "unavailable", reason: "not collected" },
    tuned: [],
    users_groups_decisions: [],
    package_groups: [],
    session_is_sensitive: false,
    not_carried_packages: rows,
    not_carried_notice: null,
  };
}

const ORIGINAL_ROWS = [FIREWALLD, PYTHON3_FIREWALL];

/** Applies SetInclude on a listed package the way the server does. */
function fakeServer() {
  const bodies: unknown[] = [];
  const fetchImpl = vi.fn(async (url: string, init?: RequestInit) => {
    if (url !== "/api/op") throw new Error(`unexpected fetch: ${url}`);
    const body = JSON.parse(String(init?.body));
    bodies.push(body);
    const { include } = body.target;
    // Included: the row leaves the list (python3-firewall stays: the known
    // limit) and firewalld is an included, changed decision. Re-excluded:
    // back to the original list, and the leaf filter drops the package.
    const view = include
      ? makeView([PYTHON3_FIREWALL], [packageFor(FIREWALLD, true)])
      : makeView(ORIGINAL_ROWS, []);
    return { ok: true, json: async () => view } as Response;
  });
  return { fetchImpl, bodies };
}

function Harness({
  initial,
  onMutationError = (err) => {
    throw err;
  },
}: {
  initial: ViewResponse;
  onMutationError?: (err: Error) => void;
}) {
  const [viewData, setViewData] = useState(initial);
  return (
    <MainContent
      activeSection="packages"
      loading={false}
      viewData={viewData}
      sections={null}
      onViewUpdate={setViewData}
      onMutationError={onMutationError}
      sectionSearchOpen={false}
      onSectionSearchClose={vi.fn()}
    />
  );
}

let server: ReturnType<typeof fakeServer>;
beforeEach(() => {
  server = fakeServer();
  vi.stubGlobal("fetch", server.fetchImpl);
});
afterEach(() => {
  vi.unstubAllGlobals();
});

async function expandGroup(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("button", { name: /not carried/i }));
}

describe("MainContent: the Not carried group", () => {
  it("shows no group when the list is empty", () => {
    render(<Harness initial={makeView([], [])} />);
    expect(screen.queryByTestId("not-carried-group")).toBeNull();
  });

  it("renders the group in the packages view", () => {
    render(<Harness initial={makeView(ORIGINAL_ROWS, [])} />);
    expect(screen.getByTestId("not-carried-group")).toBeInTheDocument();
  });

  it("sends the contract's SetInclude op body and the row leaves the group", async () => {
    const user = userEvent.setup();
    render(<Harness initial={makeView(ORIGINAL_ROWS, [])} />);
    await expandGroup(user);

    await user.click(screen.getByRole("button", { name: "Include firewalld" }));

    expect(server.bodies).toEqual([
      {
        kind: "Op",
        op: "SetInclude",
        target: {
          item_id: {
            kind: "Package",
            key: { name: "firewalld", arch: "noarch" },
          },
          include: true,
        },
      },
    ]);
    await waitFor(() =>
      expect(
        screen.queryByTestId("not-carried-row-firewalld.noarch"),
      ).toBeNull(),
    );
    // The known limit: its dependency is still listed.
    expect(
      screen.getByTestId("not-carried-row-python3-firewall.noarch"),
    ).toBeInTheDocument();
    // And it is now an included row in the package list.
    expect(
      screen.getByRole("checkbox", { name: "firewalld.noarch" }),
    ).toBeChecked();
  });

  it("re-excluding from the package's own row brings the row back", async () => {
    const user = userEvent.setup();
    render(<Harness initial={makeView(ORIGINAL_ROWS, [])} />);
    await expandGroup(user);
    await user.click(screen.getByRole("button", { name: "Include firewalld" }));
    const checkbox = await screen.findByRole("checkbox", {
      name: "firewalld.noarch",
    });

    await user.click(checkbox);

    expect(server.bodies[1]).toEqual({
      kind: "Op",
      op: "SetInclude",
      target: {
        item_id: {
          kind: "Package",
          key: { name: "firewalld", arch: "noarch" },
        },
        include: false,
      },
    });
    const group = await screen.findByTestId("not-carried-group");
    await waitFor(() =>
      expect(
        within(group).getByTestId("not-carried-row-firewalld.noarch"),
      ).toBeInTheDocument(),
    );
  });

  it("sends one request for a double click and re-enables Include after a failure", async () => {
    // Hold the request open so the second click lands while it is in flight.
    const bodies: unknown[] = [];
    let fail: (err: Error) => void = () => {};
    vi.stubGlobal(
      "fetch",
      vi.fn((_url: string, init?: RequestInit) => {
        bodies.push(JSON.parse(String(init?.body)));
        return new Promise<Response>((_resolve, reject) => {
          fail = reject;
        });
      }),
    );
    const onMutationError = vi.fn();
    const user = userEvent.setup();
    render(
      <Harness
        initial={makeView(ORIGINAL_ROWS, [])}
        onMutationError={onMutationError}
      />,
    );
    await expandGroup(user);
    const include = screen.getByRole("button", { name: "Include firewalld" });

    await user.dblClick(include);

    expect(bodies).toHaveLength(1);
    expect(include).toBeDisabled();
    // Only the row in flight is held; the others stay clickable.
    expect(
      screen.getByRole("button", { name: "Include python3-firewall" }),
    ).toBeEnabled();

    fail(new Error("connection reset"));

    await waitFor(() => expect(include).toBeEnabled());
    expect(onMutationError).toHaveBeenCalledTimes(1);
  });

  it("ignores a second click that lands before the button re-renders", async () => {
    const bodies: unknown[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn((_url: string, init?: RequestInit) => {
        bodies.push(JSON.parse(String(init?.body)));
        return new Promise<Response>(() => {});
      }),
    );
    const user = userEvent.setup();
    render(<Harness initial={makeView(ORIGINAL_ROWS, [])} />);
    await expandGroup(user);
    const include = screen.getByRole("button", { name: "Include firewalld" });

    // Both clicks inside one act: React has not re-rendered, so the button is
    // still enabled in the DOM when the second click arrives.
    act(() => {
      include.click();
      include.click();
    });

    expect(bodies).toHaveLength(1);
  });
});
