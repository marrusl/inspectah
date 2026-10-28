/**
 * Covers § 8.12's phase 0 activation block for the single-host mount:
 * the live RepoBar, mounted by MainContent, must read the repo Switch from
 * the effective counts (not `enabled`/history) and post the direction those
 * counts derive -- not `!repo.enabled` -- through `handleRepoToggle`.
 *
 * Unlike MainContent.ungroup.test.tsx, `applyOp` here is a *stateful* fake,
 * not `vi.fn()`: a case asserting only that the op fired would not catch the
 * historical-direction bug this task fixes, since both the old and new code
 * call `applyOp` at least once. The fake applies § 3.2's Repo arm (reject a
 * same-state op with a 422, otherwise move every effective member and
 * `effective_included`, and set `enabled` to the posted direction) and an
 * accepted package-level `SetInclude` (moves its own member and the repo's
 * count, leaves `enabled` alone), so the test can tell which direction the
 * UI actually posted and whether the server accepted it.
 */
import { useState } from "react";
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { MainContent } from "../MainContent";
import type { MainContentProps } from "../MainContent";
import type {
  RefinedPackage,
  RefinementOp,
  RepoGroupInfo,
  ViewResponse,
} from "../../api/types";
import { ApiError } from "../../api/types";
import * as client from "../../api/client";

vi.mock("../../api/client", () => ({
  ungroupGroup: vi.fn(),
  applyOp: vi.fn(),
}));

// --- Fixtures ---

/** One "vendor" repo with two effective (actionable, unlocked) members. */
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

function makeMemberPackage(
  name: string,
  include: boolean,
): RefinedPackage {
  return {
    entry: {
      name,
      epoch: "0",
      version: "1.0",
      release: "1",
      arch: "x86_64",
      state: "added",
      disposition: { kind: "actionable", include },
      source_repo: "vendor",
      aggregate: null,
    },
    triage: {
      triage: { mode: "single_host", baseline: null },
      primary_reason: "package_baseline_match",
      annotations: [],
    },
  };
}

function makeView(repo: RepoGroupInfo): ViewResponse {
  return {
    packages: [makeMemberPackage("a", true), makeMemberPackage("b", true)],
    config_files: [],
    repo_groups: [repo],
    package_groups: [],
    generation: 1,
    stats: {
      sections: [],
      needs_review_count: 0,
      ops_applied: 0,
      can_undo: false,
      can_redo: false,
      baseline_available: false,
    },
    version_changes: [],
    service_states: [],
    service_dropins: [],
    quadlets: [],
    flatpaks: [],
    sysctls: [],
    tuned: [],
    users_groups_decisions: [],
    session_is_sensitive: false,
  } as unknown as ViewResponse;
}

// --- Stateful fake for applyOp: § 3.2's Repo arm, in one line ---

function makeFakeApi(initialView: ViewResponse) {
  const view: ViewResponse = structuredClone(initialView);
  const opsLog: RefinementOp[] = [];
  const calls: RefinementOp[] = [];

  function findRepo(sectionId: string): RepoGroupInfo | undefined {
    return view.repo_groups.find((r) => r.section_id === sectionId);
  }

  async function applyOp(op: RefinementOp): Promise<ViewResponse> {
    calls.push(op);
    if (op.op !== "SetInclude") {
      throw new Error(`fake does not support op: ${op.op}`);
    }
    const { item_id, include } = op.target;

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
      for (const pkg of view.packages) {
        if (
          pkg.entry.source_repo === repo.section_id &&
          pkg.entry.disposition.kind === "actionable" &&
          !pkg.entry.locked
        ) {
          pkg.entry.disposition = { kind: "actionable", include };
        }
      }
      repo.effective_included = include ? repo.effective_member_count : 0;
      repo.enabled = include;
      opsLog.push(op);
      return structuredClone(view);
    }

    if (item_id.kind === "Package") {
      const pkg = view.packages.find(
        (p) =>
          p.entry.name === item_id.key.name && p.entry.arch === item_id.key.arch,
      );
      if (!pkg) throw new Error(`unknown package: ${item_id.key.name}`);
      const wasIncluded =
        pkg.entry.disposition.kind === "actionable" &&
        pkg.entry.disposition.include;
      pkg.entry.disposition = { kind: "actionable", include };
      const repo = findRepo(pkg.entry.source_repo);
      if (repo && wasIncluded !== include) {
        repo.effective_included += include ? 1 : -1;
      }
      opsLog.push(op);
      return structuredClone(view);
    }

    throw new Error(`fake does not support target: ${item_id.kind}`);
  }

  return {
    applyOp,
    repo: (sectionId: string) => findRepo(sectionId),
    opsLog: () => opsLog,
    lastPayload: () => calls[calls.length - 1],
  };
}

// --- Harness: MainContent is a controlled component, so a thin wrapper
// owns viewData and feeds applyOp's resolved view back through onViewUpdate,
// the way App.tsx's parent state does in production. ---

function Harness({
  initialView,
  onMutationError,
}: {
  initialView: ViewResponse;
  onMutationError: MainContentProps["onMutationError"];
}) {
  const [viewData, setViewData] = useState(initialView);
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

describe("RepoBar's Switch in MainContent reads members, not history", () => {
  it("state (a): reaches enabled history with both members excluded, then includes all", async () => {
    const initialView = makeView(makeVendorRepo());
    const fake = makeFakeApi(initialView);
    vi.mocked(client.applyOp).mockImplementation(fake.applyOp);
    const onMutationError = vi.fn();

    render(
      <Harness initialView={initialView} onMutationError={onMutationError} />,
    );

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
    expect(onMutationError).not.toHaveBeenCalled();
    expect(fake.opsLog()).toHaveLength(3); // two package ops, one repo op
    await waitFor(() => expect(screen.getByRole("switch")).toBeChecked());
    expect(screen.getByText("2 of 2 packages included")).toBeInTheDocument();
  });

  it("state (b): excluded history with both members individually included, then excludes all", async () => {
    // PackageList's `disabledRepos` split (`PackageList.tsx`) routes a
    // disabled repo's members into ExcludedZone, which now (task 9c) wires
    // per-row checkboxes through the same `onToggle` the normal zone uses,
    // so each member stays individually clickable while the repo stays
    // history-excluded -- reached here through a live sequence rather than
    // the fixture surgery the prior revision needed.
    const initialView = makeView(makeVendorRepo());
    const fake = makeFakeApi(initialView);
    vi.mocked(client.applyOp).mockImplementation(fake.applyOp);
    const onMutationError = vi.fn();

    render(
      <Harness initialView={initialView} onMutationError={onMutationError} />,
    );

    // Exclude the repo via the Switch, then re-include each member
    // individually through its row in ExcludedZone.
    fireEvent.click(screen.getByRole("switch"));
    await waitFor(() => expect(screen.getByRole("switch")).not.toBeChecked());
    expect(fake.repo("vendor")).toMatchObject({
      enabled: false,
      effective_included: 0,
    });

    await toggleMemberRow("a.x86_64", true);
    await toggleMemberRow("b.x86_64", true);
    expect(fake.repo("vendor")).toMatchObject({
      enabled: false,
      effective_member_count: 2,
      effective_included: 2,
    });
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
    expect(onMutationError).not.toHaveBeenCalled();
    expect(fake.opsLog()).toHaveLength(4); // repo exclude, two member includes, repo exclude
    await waitFor(() => expect(screen.getByRole("switch")).not.toBeChecked());
    expect(screen.getByText("2 packages excluded")).toBeInTheDocument();
    expect(fake.repo("vendor")).toMatchObject({
      enabled: false,
      effective_included: 0,
    });
  });
});
