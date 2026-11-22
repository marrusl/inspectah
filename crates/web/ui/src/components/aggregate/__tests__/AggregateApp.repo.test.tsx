/**
 * Covers § 8.12's phase 0 activation block for the aggregate mount, as the
 * repo ownership spec's § 5.5 amends it: the live RepoBar, mounted by
 * AggregateApp, reads the repo Switch from the decision (`kept`) and the
 * counts, and posts the direction they derive through
 * `handleAggregateRepoToggle`.
 *
 * Like MainContent.repo.test.tsx, `applyOp`/`undo`/`redo` here are a
 * *stateful* fake, not `vi.fn()`, so the test can tell which direction the
 * UI actually posted. `useAggregateMutation` ignores applyOp/undo/redo's own
 * resolved value and always refetches through `fetchAggregateView`, so the
 * fake's current view is what that mock returns on every call.
 */
import { describe, it, expect, vi } from "vitest";
import {
  render,
  screen,
  fireEvent,
  waitFor,
  act,
  within,
} from "@testing-library/react";
import { AggregateApp } from "../../AggregateApp";
import type { AggregateAppProps } from "../../AggregateApp";
import type {
  AggregateHealthInfo,
  AggregateItem,
  AggregateSection,
  AggregateViewResponse,
  HealthResponse,
  RefinementOp,
  RepoGroupInfo,
} from "../../../api/types";
import { ApiError } from "../../../api/types";

const mockFetchAggregateView = vi.fn<() => Promise<AggregateViewResponse>>();
vi.mock("../../../api/aggregate-client", () => ({
  fetchAggregateView: (...args: unknown[]) =>
    mockFetchAggregateView(...(args as [])),
  fetchAggregateDiff: vi.fn(),
}));

const mockApplyOp = vi.fn();
const mockUndo = vi.fn();
const mockRedo = vi.fn();
vi.mock("../../../api/client", () => ({
  applyOp: (...args: unknown[]) => mockApplyOp(...(args as [RefinementOp])),
  undo: () => mockUndo(),
  redo: () => mockRedo(),
}));

// --- Fixtures ---

const MOCK_AGGREGATE: AggregateHealthInfo = {
  host_count: 3,
  hostnames: ["host1", "host2", "host3"],
  zones_active: true,
  variant_count: 5,
  label: "test-aggregate",
  merged_at: "2025-01-01T00:00:00Z",
};

const MOCK_HEALTH: HealthResponse = {
  status: "ok",
  host: {
    hostname: "host1",
    os_name: "RHEL",
    os_version: "9.4",
    os_id: "rhel",
    system_type: "physical",
    schema_version: 1,
  },
  completeness: "full",
  policy: { distro_repos: [] },
  aggregate: MOCK_AGGREGATE,
  session_is_sensitive: false,
};

function makeVendorRepo(overrides: Partial<RepoGroupInfo> = {}): RepoGroupInfo {
  return {
    section_id: "vendor",
    provenance: "verified",
    is_distro: false,
    tier: "third_party",
    package_count: 2,
    effective_member_count: 2,
    effective_included: 2,
    effective_unlisted: 0,
    control: "full",
    kept: true,
    carriage_count: 0,
    carriage_included: 0,
    owns: [],
    ...overrides,
  };
}

function makeMemberItem(name: string, include: boolean): AggregateItem {
  return {
    item_id: { kind: "Package", key: { name, arch: "x86_64" } },
    include,
    locked: false,
    triage: { bucket: "universal", prevalence: { count: 3, total: 3 } },
    prevalence: { count: 3, total: 3 },
    source_repo: "vendor",
  };
}

function makeView(repo: RepoGroupInfo): AggregateViewResponse {
  const packagesSection: AggregateSection = {
    id: "packages",
    display_name: "Packages",
    is_decision_section: true,
    items: [makeMemberItem("a", true), makeMemberItem("b", true)],
  };
  return {
    generation: 1,
    can_undo: false,
    can_redo: false,
    containerfile_preview: "FROM ubi9",
    session_is_sensitive: false,
    summary: {
      host_count: 3,
      actionable_variant_items: [],
      informational_variant_count: 0,
    },
    sections: [packagesSection],
    repo_groups: [repo],
    repo_conflict_count: 0,
    machine_state: [],
    machine_state_steps: [],
    machine_state_notices: [],
    sysctl: { state: "aggregate" },
  };
}

// --- Stateful fake: applies § 3.2's Repo arm, plus undo/redo ---

function makeFakeApi(initialView: AggregateViewResponse) {
  let view: AggregateViewResponse = structuredClone(initialView);
  const opsLog: RefinementOp[] = [];
  const calls: RefinementOp[] = [];
  const history: { view: AggregateViewResponse; op: RefinementOp }[] = [];
  const redoHistory: { view: AggregateViewResponse; op: RefinementOp }[] = [];

  function findRepo(sectionId: string): RepoGroupInfo | undefined {
    return view.repo_groups.find((r) => r.section_id === sectionId);
  }

  function packageItems(): AggregateItem[] {
    return view.sections.find((s) => s.id === "packages")?.items ?? [];
  }

  async function applyOp(op: RefinementOp): Promise<unknown> {
    calls.push(op);
    if (op.op !== "SetInclude") {
      throw new Error(`fake does not support op: ${op.op}`);
    }
    const { item_id, include } = op.target;
    const beforeSnapshot = structuredClone(view);

    if (item_id.kind === "Repo") {
      const repo = findRepo(item_id.key.path);
      if (!repo) throw new Error(`unknown repo: ${item_id.key.path}`);
      const alreadyHolds = include
        ? repo.kept && repo.effective_included === repo.effective_member_count
        : !repo.kept && repo.effective_included === 0;
      if (alreadyHolds) {
        throw new ApiError(422, {
          error: `nothing to change: ${item_id.key.path}`,
        });
      }
      for (const item of packageItems()) {
        if (item.source_repo === repo.section_id && !item.locked) {
          item.include = include;
        }
      }
      repo.effective_included = include ? repo.effective_member_count : 0;
      repo.kept = include;
    } else if (item_id.kind === "Package") {
      const item = packageItems().find(
        (i) =>
          i.item_id.kind === "Package" &&
          i.item_id.key.name === item_id.key.name &&
          i.item_id.key.arch === item_id.key.arch,
      );
      if (!item) throw new Error(`unknown package: ${item_id.key.name}`);
      const wasIncluded = item.include;
      item.include = include;
      const repo = findRepo(item.source_repo);
      if (repo && wasIncluded !== include) {
        repo.effective_included += include ? 1 : -1;
      }
    } else {
      throw new Error(`fake does not support target: ${item_id.kind}`);
    }

    opsLog.push(op);
    history.push({ view: beforeSnapshot, op });
    redoHistory.length = 0;
    return {};
  }

  async function undo(): Promise<unknown> {
    const entry = history.pop();
    if (!entry) throw new Error("nothing to undo");
    redoHistory.push({ view: structuredClone(view), op: entry.op });
    view = entry.view;
    opsLog.pop();
    return {};
  }

  async function redo(): Promise<unknown> {
    const entry = redoHistory.pop();
    if (!entry) throw new Error("nothing to redo");
    history.push({ view: structuredClone(view), op: entry.op });
    view = entry.view;
    opsLog.push(entry.op);
    return {};
  }

  return {
    applyOp,
    undo,
    redo,
    currentView: () => structuredClone(view),
    repo: (sectionId: string) => findRepo(sectionId),
    opsLog: () => opsLog,
    lastPayload: () => calls[calls.length - 1],
  };
}

function renderAggregateApp(overrides: Partial<AggregateAppProps> = {}) {
  const props: AggregateAppProps = {
    aggregate: MOCK_AGGREGATE,
    health: MOCK_HEALTH,
    ...overrides,
  };
  return render(<AggregateApp {...props} />);
}

async function waitForContent() {
  await waitFor(() => {
    expect(screen.getByTestId("aggregate-content")).toBeInTheDocument();
  });
}

/** Dispatch Ctrl+Z via native keydown (bypasses StatsBar disabled state;
 *  pattern from AggregateApp.integration.test.tsx). */
function pressCtrlZ() {
  fireEvent.keyDown(document, { key: "z", ctrlKey: true });
}
function pressCtrlShiftZ() {
  fireEvent.keyDown(document, { key: "z", ctrlKey: true, shiftKey: true });
}

async function excludeMemberRow(nameArch: string) {
  fireEvent.click(screen.getByRole("checkbox", { name: nameArch }));
  await waitFor(() => {
    expect(screen.getByRole("checkbox", { name: nameArch })).toHaveProperty(
      "checked",
      false,
    );
  });
}

/**
 * Scoped to ExcludedZone, not the whole document: a disabled repo's members
 * only have a checkbox there today, but the assertion should say that's
 * where it clicked rather than relying on it being the only checkbox with
 * this name in the document.
 */
async function includeExcludedZoneMember(nameArch: string) {
  const zone = () => within(screen.getByTestId("excluded-zone"));
  fireEvent.click(zone().getByRole("checkbox", { name: nameArch }));
  await waitFor(() => {
    expect(zone().getByRole("checkbox", { name: nameArch })).toHaveProperty(
      "checked",
      true,
    );
  });
}

describe("RepoBar's Switch in AggregateApp reads the decision and the members", () => {
  function wireFake(fake: ReturnType<typeof makeFakeApi>) {
    mockFetchAggregateView.mockImplementation(() =>
      Promise.resolve(fake.currentView()),
    );
    mockApplyOp.mockImplementation(fake.applyOp);
    mockUndo.mockImplementation(fake.undo);
    mockRedo.mockImplementation(fake.redo);
  }

  it("state (a): kept with both members excluded, then includes all, with undo/redo", async () => {
    const fake = makeFakeApi(makeView(makeVendorRepo()));
    wireFake(fake);

    renderAggregateApp();
    await waitForContent();

    // Reach the state through the two mounted member rows, not fixture surgery.
    await excludeMemberRow("a.x86_64");
    await excludeMemberRow("b.x86_64");
    expect(fake.repo("vendor")).toMatchObject({
      kept: true,
      effective_member_count: 2,
      effective_included: 0,
    });
    expect(screen.getByRole("switch")).not.toBeChecked();
    expect(screen.getByText("2 packages excluded")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("switch"));
    await waitFor(() =>
      expect(fake.lastPayload()).toEqual({
        op: "SetInclude",
        target: {
          item_id: { kind: "Repo", key: { path: "vendor" } },
          include: true,
        },
      }),
    );
    expect(fake.opsLog()).toHaveLength(3); // two member ops, one repo op
    await waitFor(() => expect(screen.getByRole("switch")).toBeChecked());
    expect(screen.getByText("2 of 2 packages included")).toBeInTheDocument();

    // Ctrl+Z restores the pre-repo-op state: unchecked, log popped.
    await act(async () => {
      pressCtrlZ();
    });
    await waitFor(() => expect(mockUndo).toHaveBeenCalledOnce());
    await waitFor(() => expect(screen.getByRole("switch")).not.toBeChecked());
    expect(screen.getByText("2 packages excluded")).toBeInTheDocument();
    expect(fake.opsLog()).toHaveLength(2);

    // Ctrl+Shift+Z re-applies it.
    await act(async () => {
      pressCtrlShiftZ();
    });
    await waitFor(() => expect(mockRedo).toHaveBeenCalledOnce());
    await waitFor(() => expect(screen.getByRole("switch")).toBeChecked());
    expect(screen.getByText("2 of 2 packages included")).toBeInTheDocument();
    expect(fake.opsLog()).toHaveLength(3);
  });

  // PackageList's `disabledRepos` split (`PackageList.tsx`) routes an
  // excluded repo's members into ExcludedZone, which wires per-row
  // checkboxes through the same `onToggle` the normal zone uses, so each
  // member stays individually clickable while the repo stays excluded.
  it("state (b): excluded with both members individually included, then kept again", async () => {
    const fake = makeFakeApi(makeView(makeVendorRepo()));
    wireFake(fake);

    renderAggregateApp();
    await waitForContent();

    fireEvent.click(screen.getByRole("switch"));
    await waitFor(() => expect(screen.getByRole("switch")).not.toBeChecked());
    expect(fake.repo("vendor")).toMatchObject({
      kept: false,
      effective_included: 0,
    });

    await includeExcludedZoneMember("a.x86_64");
    await includeExcludedZoneMember("b.x86_64");
    expect(fake.repo("vendor")).toMatchObject({
      kept: false,
      effective_member_count: 2,
      effective_included: 2,
    });
    expect(screen.getByRole("switch")).not.toBeChecked();
    expect(
      screen.getByText("2 of 2 packages included, excluded in refine"),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("switch"));
    await waitFor(() =>
      expect(fake.lastPayload()).toEqual({
        op: "SetInclude",
        target: {
          item_id: { kind: "Repo", key: { path: "vendor" } },
          include: true,
        },
      }),
    );
    expect(fake.opsLog()).toHaveLength(4); // repo exclude, two member includes, repo include
    await waitFor(() => expect(screen.getByRole("switch")).toBeChecked());
    expect(fake.repo("vendor")).toMatchObject({
      kept: true,
      effective_included: 2,
    });
  });

  it("shows a refused repo op on the row and refetches the view", async () => {
    const fake = makeFakeApi(makeView(makeVendorRepo()));
    wireFake(fake);
    renderAggregateApp();
    await waitForContent();

    mockApplyOp.mockRejectedValueOnce(
      new ApiError(422, {
        error:
          "nothing to change: repo 'vendor' is already excluded, with no effective member included and nothing more to drop",
      }),
    );
    const fetchesBefore = mockFetchAggregateView.mock.calls.length;
    fireEvent.click(screen.getByRole("switch"));
    const notice = await screen.findByTestId("repo-failure-vendor");
    expect(notice).toHaveTextContent(
      "Nothing to change: repo 'vendor' is already excluded, with no effective member included and nothing more to drop.",
    );
    expect(notice.className).toMatch(/info/);
    await waitFor(() =>
      expect(mockFetchAggregateView.mock.calls.length).toBeGreaterThan(
        fetchesBefore,
      ),
    );
    // The page stays up: a refused op is not a load failure.
    expect(screen.getByTestId("aggregate-content")).toBeInTheDocument();
  });

  // A refused repo op's recovery fetch runs outside the mutation queue, so
  // a later op's fetch can answer first. Whichever order they answer in,
  // the newer generation stays on screen.
  it("keeps the newer view when a refused op's recovery fetch answers last", async () => {
    const kept = makeView(makeVendorRepo());
    mockFetchAggregateView.mockResolvedValueOnce(kept);
    renderAggregateApp();
    await waitForContent();
    expect(screen.getByRole("switch")).toBeChecked();
    const fetchesBefore = mockFetchAggregateView.mock.calls.length;

    let answerRecovery!: (view: AggregateViewResponse) => void;
    let answerQueue!: (view: AggregateViewResponse) => void;
    mockFetchAggregateView
      .mockReturnValueOnce(new Promise((r) => (answerRecovery = r)))
      .mockReturnValueOnce(new Promise((r) => (answerQueue = r)));
    mockApplyOp
      .mockRejectedValueOnce(
        new ApiError(422, {
          error: "nothing to change: repo 'vendor' is already kept",
        }),
      )
      .mockResolvedValueOnce({});

    // The refused op starts the recovery fetch, which stays out.
    fireEvent.click(screen.getByRole("switch"));
    await screen.findByTestId("repo-failure-vendor");
    await waitFor(() =>
      expect(mockFetchAggregateView.mock.calls.length).toBe(fetchesBefore + 1),
    );

    // The next op is accepted, and its fetch stays out too.
    fireEvent.click(screen.getByRole("switch"));
    await waitFor(() =>
      expect(mockFetchAggregateView.mock.calls.length).toBe(fetchesBefore + 2),
    );

    const excluded = makeView(
      makeVendorRepo({ kept: false, effective_included: 0 }),
    );
    excluded.generation = 11;
    const stale = { ...structuredClone(kept), generation: 10 };

    await act(async () => answerQueue(excluded));
    await waitFor(() => expect(screen.getByRole("switch")).not.toBeChecked());
    await act(async () => answerRecovery(stale));

    expect(screen.getByRole("switch")).not.toBeChecked();
    expect(
      screen.getByText("2 packages excluded, excluded in refine"),
    ).toBeInTheDocument();
  });
});

// The fleet's Configuration rows for a repo's file and key, from the
// `repos-aggregate` session of
// `crates/web/tests/system_tuning_fixtures_test.rs`: both carry
// `shadowed_by: "repositories"`, and repo `custom` owns both.
const RECORDED = import.meta.glob<{ default: AggregateViewResponse }>(
  "../../../__tests__/fixtures/system-tuning/repos-aggregate.aggregate-view.json",
  { eager: true },
);

function recordedFleetView(): AggregateViewResponse {
  const [entry] = Object.values(RECORDED);
  if (!entry) throw new Error("no recorded repos-aggregate view");
  return structuredClone(entry.default);
}

describe("A fleet Configuration row a repo decides", () => {
  const PATHS = [
    "/etc/pki/rpm-gpg/RPM-GPG-KEY-custom",
    "/etc/yum.repos.d/custom.repo",
  ];

  function configRow(path: string): HTMLElement {
    const row = screen
      .getAllByTestId("aggregate-item-row")
      .find(
        (el) =>
          el.getAttribute("data-item-id") ===
          JSON.stringify({ key: { path }, kind: "Config" }),
      );
    if (!row) throw new Error(`no row for ${path}`);
    return row;
  }

  async function openConfiguration() {
    mockFetchAggregateView.mockImplementation(() =>
      Promise.resolve(recordedFleetView()),
    );
    renderAggregateApp();
    await waitForContent();
    fireEvent.click(
      within(screen.getByTestId("aggregate-sidebar")).getByText(
        "Configuration Files",
      ),
    );
    await waitFor(() => expect(configRow(PATHS[1])).toBeInTheDocument());
  }

  it("is read-only, says the repo section decides it, and sends no op", async () => {
    await openConfiguration();
    const opsBefore = mockApplyOp.mock.calls.length;

    for (const path of PATHS) {
      const row = configRow(path);
      expect(row).toHaveAttribute("data-shadowed-by", "repositories");
      expect(within(row).queryByRole("switch")).toBeNull();
      expect(within(row).queryByRole("checkbox")).toBeNull();
      expect(within(row).getByTestId("shadow-owner-badge")).toHaveTextContent(
        "Decided under Repositories",
      );
      expect(
        within(row).getByRole("button", { name: "Go to repo custom" }),
      ).toBeInTheDocument();

      // Neither a click nor Enter nor Space opens the row.
      fireEvent.click(row);
      fireEvent.keyDown(row, { key: "Enter" });
      fireEvent.keyDown(row, { key: " " });
    }
    expect(screen.queryByTestId("item-detail-pane")).toBeNull();
    expect(mockApplyOp.mock.calls.length).toBe(opsBefore);
    expect(screen.queryByTestId("aggregate-op-notice")).toBeNull();
  });

  it("reveals the repo's switch under Repositories", async () => {
    await openConfiguration();
    fireEvent.click(
      within(configRow(PATHS[0])).getByRole("button", {
        name: "Go to repo custom",
      }),
    );
    const repoRow = await screen.findByTestId("repo-bar-row-custom");
    await waitFor(() =>
      expect(within(repoRow).getByRole("switch")).toHaveFocus(),
    );
  });
});
