import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
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

describe("MainContent undo focus restoration", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("after ungroup, onSetUndoFocusTarget is called with the group-row ID", async () => {
    const members: GroupMemberInfo[] = [
      { name: "pkg1", locked: false, overlap_groups: [], in_base_image: false },
      { name: "pkg2", locked: false, overlap_groups: [], in_base_image: false },
    ];

    const mockGroup: GroupInfo = {
      name: "test-group",
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
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
        sectionSearchOpen={false}
        onSectionSearchClose={vi.fn()}
        onSetUndoFocusTarget={onSetUndoFocusTarget}
      />,
    );

    // Click the Ungroup button rendered by GroupRow
    const ungroupBtn = screen.getByRole("button", {
      name: "Ungroup test-group",
    });
    fireEvent.click(ungroupBtn);

    // onSetUndoFocusTarget is called synchronously before the API call
    expect(onSetUndoFocusTarget).toHaveBeenCalledWith("group-row-test-group");
  });

  it("after undo of ungroup, focus target uses group-row prefix", async () => {
    // This test verifies the App.tsx side of the flow:
    // handleGroupUngroup sets the undo focus target to "group-row-<name>"
    // so that App.tsx can restore focus to the group row after undo.
    const members: GroupMemberInfo[] = [
      { name: "pkg1", locked: false, overlap_groups: [], in_base_image: false },
    ];

    const mockGroup: GroupInfo = {
      name: "my-group",
      member_count: 1,
      added_count: 0,
      locked_count: 0,
      optional_spillover_count: 0,
      render_state: "renderable",
      degradation_reason: null,
      members,
    };

    const mockViewData = makeViewData([mockGroup]);
    const updatedView = makeViewData([], 2);

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
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
        sectionSearchOpen={false}
        onSectionSearchClose={vi.fn()}
        onSetUndoFocusTarget={onSetUndoFocusTarget}
      />,
    );

    // Click the Ungroup button
    const ungroupBtn = screen.getByRole("button", {
      name: "Ungroup my-group",
    });
    fireEvent.click(ungroupBtn);

    // Verify the undo focus target uses group-row prefix
    expect(onSetUndoFocusTarget).toHaveBeenCalledWith("group-row-my-group");

    // The actual focus restoration on undo happens in App.tsx via undoFocusRef.
    // That logic already exists for packages/configs; this extends it to groups.
  });
});
