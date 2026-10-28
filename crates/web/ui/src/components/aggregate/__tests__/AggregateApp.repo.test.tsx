/**
 * Covers § 8.12's phase 0 activation block for the aggregate mount: the
 * live RepoBar, mounted by AggregateApp, must read the repo Switch from the
 * effective counts (not `enabled`/history) and post the direction those
 * counts derive through `handleAggregateRepoToggle`.
 *
 * Like MainContent.repo.test.tsx, `applyOp`/`undo`/`redo` here are a
 * *stateful* fake, not `vi.fn()`, so the test can tell which direction the
 * UI actually posted. `useAggregateMutation` ignores applyOp/undo/redo's own
 * resolved value and always refetches through `fetchAggregateView`, so the
 * fake's current view is what that mock returns on every call.
 */
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent, waitFor, act } from "@testing-library/react";
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
    enabled: true,
    ...overrides,
  };
}

function makeMemberItem(name: string, include: boolean): AggregateItem {
  return {
    item_id: { kind: "Package", key: { name, arch: "x86_64" } },
    include,
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
        ? repo.effective_included === repo.effective_member_count
        : repo.effective_included === 0;
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
      repo.enabled = include;
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

async function toggleMemberRow(nameArch: string, expectChecked: boolean) {
  fireEvent.click(screen.getByRole("checkbox", { name: nameArch }));
  await waitFor(() => {
    expect(screen.getByRole("checkbox", { name: nameArch })).toHaveProperty(
      "checked",
      expectChecked,
    );
  });
}
const excludeMemberRow = (nameArch: string) => toggleMemberRow(nameArch, false);

describe("RepoBar's Switch in AggregateApp reads members, not history", () => {
  function wireFake(fake: ReturnType<typeof makeFakeApi>) {
    mockFetchAggregateView.mockImplementation(() =>
      Promise.resolve(fake.currentView()),
    );
    mockApplyOp.mockImplementation(fake.applyOp);
    mockUndo.mockImplementation(fake.undo);
    mockRedo.mockImplementation(fake.redo);
  }

  it("state (a): reaches enabled history with both members excluded, then includes all, with undo/redo", async () => {
    const fake = makeFakeApi(makeView(makeVendorRepo()));
    wireFake(fake);

    renderAggregateApp();
    await waitForContent();

    // Reach the state through the two mounted member rows, not fixture surgery.
    await excludeMemberRow("a.x86_64");
    await excludeMemberRow("b.x86_64");
    expect(fake.repo("vendor")).toMatchObject({
      enabled: true,
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

  // PackageList's `disabledRepos` split (`PackageList.tsx:148-154`, not in
  // this task's Files list) hides a repo's member rows into the read-only
  // ExcludedZone whenever `enabled` is false, regardless of each member's
  // own include flag -- the same behavior MainContent.repo.test.tsx found
  // in the single-host mount. So state (b) -- excluded history with every
  // member individually included -- is reached the way the spec describes
  // it (server history from ordinary per-package decisions), not through a
  // live "exclude, then re-include" click sequence in this mount.
  it("state (b): excluded history with both members individually included, then excludes all", async () => {
    const fake = makeFakeApi(
      makeView(makeVendorRepo({ enabled: false, effective_included: 2 })),
    );
    wireFake(fake);

    renderAggregateApp();
    await waitForContent();

    expect(screen.getByRole("switch")).toBeChecked();
    expect(screen.getByText("2 of 2 packages included")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("switch"));
    await waitFor(() =>
      expect(fake.lastPayload()).toEqual({
        op: "SetInclude",
        target: {
          item_id: { kind: "Repo", key: { path: "vendor" } },
          include: false,
        },
      }),
    );
    expect(fake.opsLog()).toHaveLength(1);
    await waitFor(() => expect(screen.getByRole("switch")).not.toBeChecked());
    expect(screen.getByText("2 packages excluded")).toBeInTheDocument();
    expect(fake.repo("vendor")).toMatchObject({
      enabled: false,
      effective_included: 0,
    });
  });
});
