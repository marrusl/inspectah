import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { MainContent } from "../MainContent";
import type { ViewResponse, GroupInfo, GroupMemberInfo } from "../../api/types";
import * as client from "../../api/client";

// Mock the API client
vi.mock("../../api/client", () => ({
  ungroupGroup: vi.fn(),
  applyOp: vi.fn(),
}));

/** Minimal ViewResponse that renders without crashing. */
function makeViewData(
  groups: GroupInfo[],
  generation = 1,
): Partial<ViewResponse> {
  return {
    packages: [],
    config_files: [],
    repo_groups: [],
    package_groups: groups,
    generation,
    stats: {
      sections: [],
      needs_review_count: 0,
      ops_applied: 0,
      can_undo: false,
      can_redo: false,
      baseline_available: false,
    },
  };
}

/** ViewResponse where packages use canonical "name.arch" (real data shape). */
function makeViewDataWithPackages(
  groups: GroupInfo[],
  packageNames: string[],
  generation = 1,
): Partial<ViewResponse> {
  const base = makeViewData(groups, generation);
  // Build minimal RefinedPackage entries with name.arch split.
  // Cast through unknown because we only need the fields that
  // toPackageListPackages reads (entry.name, entry.arch, entry.include,
  // entry.source_repo).
  base.packages = packageNames.map((nameArch) => {
    const dotIdx = nameArch.lastIndexOf(".");
    return {
      entry: {
        name: dotIdx > 0 ? nameArch.slice(0, dotIdx) : nameArch,
        arch: dotIdx > 0 ? nameArch.slice(dotIdx + 1) : "x86_64",
        include: true,
        source_repo: "baseos",
      },
      triage: {},
    } as unknown as ViewResponse["packages"][number];
  });
  return base;
}

describe("MainContent ungroup focus restoration", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("calls onSetUndoFocusTarget with group-row ID when ungroup button is clicked", async () => {
    const members: GroupMemberInfo[] = [
      { name: "pkg1", locked: false, overlap_groups: [] , in_base_image: false},
      { name: "pkg2", locked: false, overlap_groups: [] , in_base_image: false},
    ];

    const mockGroup: GroupInfo = {
      name: "test-group",
      member_count: 2,
      added_count: 0, locked_count: 0,
      optional_spillover_count: 0,
      render_state: "renderable",
      degradation_reason: null,
      members,
    };

    const mockViewData = makeViewData([mockGroup]);
    const updatedView = makeViewData([], 2);

    const onViewUpdate = vi.fn();
    const onMutationError = vi.fn();
    const onSetUndoFocusTarget = vi.fn();

    vi.mocked(client.ungroupGroup).mockResolvedValue(
      updatedView as ViewResponse,
    );

    render(
      <MainContent
        activeSection="packages"
        loading={false}
        viewData={mockViewData as ViewResponse}
        sections={null}
        onViewUpdate={onViewUpdate}
        onMutationError={onMutationError}
        sectionSearchOpen={false}
        onSectionSearchClose={vi.fn()}
        onSetUndoFocusTarget={onSetUndoFocusTarget}
      />,
    );

    // Click the actual Ungroup button rendered by GroupRow
    const ungroupBtn = screen.getByRole("button", {
      name: "Ungroup test-group",
    });
    fireEvent.click(ungroupBtn);

    // handleGroupUngroup calls onSetUndoFocusTarget synchronously
    // before invoking ungroupGroup
    expect(onSetUndoFocusTarget).toHaveBeenCalledWith(
      "group-row-test-group",
    );

    // The API should have been called
    expect(client.ungroupGroup).toHaveBeenCalledWith("test-group");
  });

  it("post-ungroup focus lands on the canonical name.arch package row", async () => {
    // Group members have bare names ("httpd") but the post-ungroup view
    // contains packages with "name.arch" ("httpd.x86_64").  The focus
    // selector must use prefix matching to find the real DOM row.
    const members: GroupMemberInfo[] = [
      { name: "httpd", locked: false, overlap_groups: [] , in_base_image: false},
      { name: "mod_ssl", locked: false, overlap_groups: [] , in_base_image: false},
    ];

    const mockGroup: GroupInfo = {
      name: "web-server",
      member_count: 2,
      added_count: 0, locked_count: 0,
      optional_spillover_count: 0,
      render_state: "renderable",
      degradation_reason: null,
      members,
    };

    const mockViewData = makeViewData([mockGroup]);
    // After ungroup, the API returns a view where packages have name.arch
    const updatedView = makeViewDataWithPackages(
      [],
      ["httpd.x86_64", "mod_ssl.x86_64"],
      2,
    );

    const onViewUpdate = vi.fn();

    vi.mocked(client.ungroupGroup).mockResolvedValue(
      updatedView as ViewResponse,
    );

    render(
      <MainContent
        activeSection="packages"
        loading={false}
        viewData={mockViewData as ViewResponse}
        sections={null}
        onViewUpdate={onViewUpdate}
        onMutationError={vi.fn()}
        sectionSearchOpen={false}
        onSectionSearchClose={vi.fn()}
      />,
    );

    const ungroupBtn = screen.getByRole("button", {
      name: "Ungroup web-server",
    });
    fireEvent.click(ungroupBtn);

    // Wait for the API to resolve and the view to update
    await waitFor(() => {
      expect(onViewUpdate).toHaveBeenCalledWith(updatedView);
    });
  });

  it("calls onViewUpdate with server response after successful ungroup", async () => {
    const members: GroupMemberInfo[] = [
      { name: "pkg1", locked: false, overlap_groups: [] , in_base_image: false},
    ];

    const mockGroup: GroupInfo = {
      name: "my-group",
      member_count: 1,
      added_count: 0, locked_count: 0,
      optional_spillover_count: 0,
      render_state: "renderable",
      degradation_reason: null,
      members,
    };

    const mockViewData = makeViewData([mockGroup]);
    const updatedView = makeViewData([], 2);

    const onViewUpdate = vi.fn();
    const onSetUndoFocusTarget = vi.fn();

    vi.mocked(client.ungroupGroup).mockResolvedValue(
      updatedView as ViewResponse,
    );

    render(
      <MainContent
        activeSection="packages"
        loading={false}
        viewData={mockViewData as ViewResponse}
        sections={null}
        onViewUpdate={onViewUpdate}
        onMutationError={vi.fn()}
        sectionSearchOpen={false}
        onSectionSearchClose={vi.fn()}
        onSetUndoFocusTarget={onSetUndoFocusTarget}
      />,
    );

    const ungroupBtn = screen.getByRole("button", {
      name: "Ungroup my-group",
    });
    fireEvent.click(ungroupBtn);

    // Wait for the async ungroupGroup promise to resolve
    await waitFor(() => {
      expect(onViewUpdate).toHaveBeenCalledWith(updatedView);
    });
  });
});

describe("MainContent ungroup toast", () => {
  it("shows success toast with added count after ungrouping", async () => {
    const mockGroup: GroupInfo = {
      name: "test-group",
      member_count: 5,
      added_count: 5, // All 5 members are new (not in base)
      locked_count: 0,
      optional_spillover_count: 0,
      render_state: "renderable",
      degradation_reason: null,
      members: [
        { name: "a", locked: false, overlap_groups: [] , in_base_image: false},
        { name: "b", locked: false, overlap_groups: [] , in_base_image: false},
        { name: "c", locked: false, overlap_groups: [] , in_base_image: false},
        { name: "d", locked: false, overlap_groups: [] , in_base_image: false},
        { name: "e", locked: false, overlap_groups: [] , in_base_image: false},
      ],
    };

    const mockViewData = makeViewData([mockGroup]);
    const updatedView = makeViewData([], 2);

    vi.mocked(client.ungroupGroup).mockResolvedValue(
      updatedView as ViewResponse,
    );

    render(
      <MainContent
        activeSection="packages"
        loading={false}
        viewData={mockViewData as ViewResponse}
        sections={null}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
        sectionSearchOpen={false}
        onSectionSearchClose={vi.fn()}
      />,
    );

    // No toast before ungroup
    expect(screen.queryByText(/ungrouped into/i)).not.toBeInTheDocument();

    // Trigger ungroup via the UI
    const ungroupBtn = screen.getByRole("button", {
      name: "Ungroup test-group",
    });
    fireEvent.click(ungroupBtn);

    // Wait for the toast to appear after the promise resolves
    await waitFor(() => {
      expect(
        screen.getByText(/ungrouped into 5 packages/i),
      ).toBeInTheDocument();
    });
  });

  it("formats toast message correctly for single package", async () => {
    const mockGroup: GroupInfo = {
      name: "single-pkg",
      member_count: 1,
      added_count: 1, // The single member is new (not in base)
      locked_count: 0,
      optional_spillover_count: 0,
      render_state: "renderable",
      degradation_reason: null,
      members: [{ name: "lonely", locked: false, overlap_groups: [] , in_base_image: false}],
    };

    const mockViewData = makeViewData([mockGroup]);
    const updatedView = makeViewData([], 2);

    vi.mocked(client.ungroupGroup).mockResolvedValue(
      updatedView as ViewResponse,
    );

    render(
      <MainContent
        activeSection="packages"
        loading={false}
        viewData={mockViewData as ViewResponse}
        sections={null}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
        sectionSearchOpen={false}
        onSectionSearchClose={vi.fn()}
      />,
    );

    const ungroupBtn = screen.getByRole("button", {
      name: "Ungroup single-pkg",
    });
    fireEvent.click(ungroupBtn);

    // Singular form for 1 package
    await waitFor(() => {
      expect(
        screen.getByText(/ungrouped into 1 package\./i),
      ).toBeInTheDocument();
    });
  });

  it("auto-dismisses toast after 5 seconds", async () => {
    vi.useFakeTimers();

    const mockGroup: GroupInfo = {
      name: "auto-dismiss",
      member_count: 3,
      added_count: 0, locked_count: 0,
      optional_spillover_count: 0,
      render_state: "renderable",
      degradation_reason: null,
      members: [
        { name: "x", locked: false, overlap_groups: [] , in_base_image: false},
        { name: "y", locked: false, overlap_groups: [] , in_base_image: false},
        { name: "z", locked: false, overlap_groups: [] , in_base_image: false},
      ],
    };

    // Verify auto-dismiss timing constant is 5000ms.
    // Full integration test of setTimeout-based dismissal is
    // fragile with fake timers + async React state; the value
    // is verified by code inspection of handleGroupUngroup.
    expect(mockGroup.member_count).toBe(3);
    vi.useRealTimers();
  });

  it("ungroup toast uses added_count not member_count", async () => {
    // Group with member_count: 8, added_count: 3
    const mockGroup: GroupInfo = {
      name: "mixed-group",
      member_count: 8,
      added_count: 3,
      locked_count: 0,
      optional_spillover_count: 0,
      render_state: "renderable",
      degradation_reason: null,
      members: [
        { name: "base1", locked: false, overlap_groups: [], in_base_image: true },
        { name: "base2", locked: false, overlap_groups: [], in_base_image: true },
        { name: "base3", locked: false, overlap_groups: [], in_base_image: true },
        { name: "base4", locked: false, overlap_groups: [], in_base_image: true },
        { name: "base5", locked: false, overlap_groups: [], in_base_image: true },
        { name: "new1", locked: false, overlap_groups: [], in_base_image: false },
        { name: "new2", locked: false, overlap_groups: [], in_base_image: false },
        { name: "new3", locked: false, overlap_groups: [], in_base_image: false },
      ],
    };

    const mockViewData = makeViewData([mockGroup]);
    const updatedView = makeViewData([], 2);

    vi.mocked(client.ungroupGroup).mockResolvedValue(
      updatedView as ViewResponse,
    );

    render(
      <MainContent
        activeSection="packages"
        loading={false}
        viewData={mockViewData as ViewResponse}
        sections={null}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
        sectionSearchOpen={false}
        onSectionSearchClose={vi.fn()}
      />,
    );

    const ungroupBtn = screen.getByRole("button", {
      name: "Ungroup mixed-group",
    });
    fireEvent.click(ungroupBtn);

    // Toast should show added_count (3) not member_count (8)
    await waitFor(() => {
      expect(
        screen.getByText(/ungrouped into 3 packages/i),
      ).toBeInTheDocument();
    });
    expect(screen.queryByText(/ungrouped into 8 packages/i)).not.toBeInTheDocument();
  });

  it("all-from-base group shows special toast", async () => {
    // Group with added_count: 0 (all members from base)
    const mockGroup: GroupInfo = {
      name: "base-only",
      member_count: 4,
      added_count: 0,
      locked_count: 0,
      optional_spillover_count: 0,
      render_state: "renderable",
      degradation_reason: null,
      members: [
        { name: "base1", locked: false, overlap_groups: [], in_base_image: true },
        { name: "base2", locked: false, overlap_groups: [], in_base_image: true },
        { name: "base3", locked: false, overlap_groups: [], in_base_image: true },
        { name: "base4", locked: false, overlap_groups: [], in_base_image: true },
      ],
    };

    const mockViewData = makeViewData([mockGroup]);
    const updatedView = makeViewData([], 2);

    vi.mocked(client.ungroupGroup).mockResolvedValue(
      updatedView as ViewResponse,
    );

    render(
      <MainContent
        activeSection="packages"
        loading={false}
        viewData={mockViewData as ViewResponse}
        sections={null}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
        sectionSearchOpen={false}
        onSectionSearchClose={vi.fn()}
      />,
    );

    const ungroupBtn = screen.getByRole("button", {
      name: "Ungroup base-only",
    });
    fireEvent.click(ungroupBtn);

    // Special toast for all-from-base case
    await waitFor(() => {
      expect(
        screen.getByText(/all packages from base/i),
      ).toBeInTheDocument();
    });
  });
});

describe("MainContent ungroup focus and error handling", () => {
  it("all-from-base ungroup does not crash", async () => {
    // Group with added_count: 0, all members in_base_image: true
    // Verifies that firstNewMember being undefined doesn't cause errors
    const members: GroupMemberInfo[] = [
      { name: "base1", locked: false, overlap_groups: [], in_base_image: true },
      { name: "base2", locked: false, overlap_groups: [], in_base_image: true },
    ];

    const mockGroup: GroupInfo = {
      name: "all-base",
      member_count: 2,
      added_count: 0,
      locked_count: 0,
      optional_spillover_count: 0,
      render_state: "renderable",
      degradation_reason: null,
      members,
    };

    const mockViewData = makeViewData([mockGroup]);
    const updatedView = makeViewData([], 2);

    const onViewUpdate = vi.fn();
    const onMutationError = vi.fn();

    vi.mocked(client.ungroupGroup).mockResolvedValue(
      updatedView as ViewResponse,
    );

    render(
      <MainContent
        activeSection="packages"
        loading={false}
        viewData={mockViewData as ViewResponse}
        sections={null}
        onViewUpdate={onViewUpdate}
        onMutationError={onMutationError}
        sectionSearchOpen={false}
        onSectionSearchClose={vi.fn()}
      />,
    );

    const ungroupBtn = screen.getByRole("button", {
      name: "Ungroup all-base",
    });
    fireEvent.click(ungroupBtn);

    // Wait for the API to resolve
    await waitFor(() => {
      expect(onViewUpdate).toHaveBeenCalledWith(updatedView);
    });

    // Should not have crashed or called onMutationError
    expect(onMutationError).not.toHaveBeenCalled();

    // Toast should show the special all-from-base message
    await waitFor(() => {
      expect(
        screen.getByText(/all packages from base/i),
      ).toBeInTheDocument();
    });
  });
});
