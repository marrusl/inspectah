import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor, act } from "@testing-library/react";
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

/** ViewResponse with real package entries (name.arch canonical form). */
function makeViewDataWithPackages(
  groups: GroupInfo[],
  packageNames: string[],
  generation = 1,
): Partial<ViewResponse> {
  const base = makeViewData(groups, generation);
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

describe("Deferred post-ungroup focus", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("post-ungroup focus lands on the real package row after re-render", async () => {
    // Start with a group containing "httpd"
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

    const initialView = makeViewData([mockGroup]);
    // After ungroup, the API returns individual packages (no groups)
    const postUngroupView = makeViewDataWithPackages(
      [],
      ["httpd.x86_64", "mod_ssl.x86_64"],
      2,
    );

    const onViewUpdate = vi.fn();

    vi.mocked(client.ungroupGroup).mockResolvedValue(
      postUngroupView as ViewResponse,
    );

    const { rerender } = render(
      <MainContent
        activeSection="packages"
        loading={false}
        viewData={initialView as ViewResponse}
        sections={null}
        onViewUpdate={onViewUpdate}
        onMutationError={vi.fn()}
        sectionSearchOpen={false}
        onSectionSearchClose={vi.fn()}
      />,
    );

    // Click the Ungroup button
    const ungroupBtn = screen.getByRole("button", {
      name: "Ungroup web-server",
    });
    fireEvent.click(ungroupBtn);

    // Wait for the API call to resolve
    await waitFor(() => {
      expect(onViewUpdate).toHaveBeenCalledWith(postUngroupView);
    });

    // Simulate what App.tsx does: pass the updated viewData back to MainContent.
    // This triggers the useEffect that watches pendingFocusTarget + viewData.
    await act(async () => {
      rerender(
        <MainContent
          activeSection="packages"
          loading={false}
          viewData={postUngroupView as ViewResponse}
          sections={null}
          onViewUpdate={onViewUpdate}
          onMutationError={vi.fn()}
          sectionSearchOpen={false}
          onSectionSearchClose={vi.fn()}
        />,
      );
    });

    // The first member's package row should now exist and have focus
    const focusedRow = screen.getByTestId("package-row-httpd.x86_64");
    expect(focusedRow).toBeDefined();
    expect(document.activeElement).toBe(focusedRow);
  });

  it("focus selector does not match prefix-ambiguous sibling rows", async () => {
    // Group contains "httpd" — but the view also has "httpd-tools.x86_64".
    // The dot boundary in the selector must prevent "httpd" from matching
    // "httpd-tools.x86_64" via startsWith.
    const members: GroupMemberInfo[] = [
      { name: "httpd", locked: false, overlap_groups: [] , in_base_image: false},
    ];

    const mockGroup: GroupInfo = {
      name: "web-only",
      member_count: 1,
      added_count: 0, locked_count: 0,
      optional_spillover_count: 0,
      render_state: "renderable",
      degradation_reason: null,
      members,
    };

    // Initial view: group + httpd-tools as an individual package
    const initialView = makeViewDataWithPackages(
      [mockGroup],
      ["httpd-tools.x86_64"],
      1,
    );
    // After ungroup: both httpd and httpd-tools as individual packages
    const postUngroupView = makeViewDataWithPackages(
      [],
      ["httpd.x86_64", "httpd-tools.x86_64"],
      2,
    );

    const onViewUpdate = vi.fn();

    vi.mocked(client.ungroupGroup).mockResolvedValue(
      postUngroupView as ViewResponse,
    );

    const { rerender } = render(
      <MainContent
        activeSection="packages"
        loading={false}
        viewData={initialView as ViewResponse}
        sections={null}
        onViewUpdate={onViewUpdate}
        onMutationError={vi.fn()}
        sectionSearchOpen={false}
        onSectionSearchClose={vi.fn()}
      />,
    );

    // Click the Ungroup button
    const ungroupBtn = screen.getByRole("button", {
      name: "Ungroup web-only",
    });
    fireEvent.click(ungroupBtn);

    // Wait for the API call to resolve
    await waitFor(() => {
      expect(onViewUpdate).toHaveBeenCalledWith(postUngroupView);
    });

    // Re-render with the post-ungroup data
    await act(async () => {
      rerender(
        <MainContent
          activeSection="packages"
          loading={false}
          viewData={postUngroupView as ViewResponse}
          sections={null}
          onViewUpdate={onViewUpdate}
          onMutationError={vi.fn()}
          sectionSearchOpen={false}
          onSectionSearchClose={vi.fn()}
        />,
      );
    });

    // Focus must be on httpd.x86_64, NOT httpd-tools.x86_64
    const correctRow = screen.getByTestId("package-row-httpd.x86_64");
    const ambiguousRow = screen.getByTestId("package-row-httpd-tools.x86_64");

    expect(document.activeElement).toBe(correctRow);
    expect(document.activeElement).not.toBe(ambiguousRow);
  });

  it("focus selector handles dotted package names without collision", async () => {
    // Group contains "python3" — but the view also has "python3.11.x86_64".
    // Exact-match selector must prevent "python3" from matching "python3.11.x86_64".
    const members: GroupMemberInfo[] = [
      { name: "python3", locked: false, overlap_groups: [] , in_base_image: false},
    ];

    const mockGroup: GroupInfo = {
      name: "python-base",
      member_count: 1,
      added_count: 0, locked_count: 0,
      optional_spillover_count: 0,
      render_state: "renderable",
      degradation_reason: null,
      members,
    };

    // Initial view: group + python3.11 as an individual package
    const initialView = makeViewDataWithPackages(
      [mockGroup],
      ["python3.11.x86_64"],
      1,
    );
    // After ungroup: both python3 and python3.11 as individual packages
    const postUngroupView = makeViewDataWithPackages(
      [],
      ["python3.x86_64", "python3.11.x86_64"],
      2,
    );

    const onViewUpdate = vi.fn();

    vi.mocked(client.ungroupGroup).mockResolvedValue(
      postUngroupView as ViewResponse,
    );

    const { rerender } = render(
      <MainContent
        activeSection="packages"
        loading={false}
        viewData={initialView as ViewResponse}
        sections={null}
        onViewUpdate={onViewUpdate}
        onMutationError={vi.fn()}
        sectionSearchOpen={false}
        onSectionSearchClose={vi.fn()}
      />,
    );

    // Click the Ungroup button
    const ungroupBtn = screen.getByRole("button", {
      name: "Ungroup python-base",
    });
    fireEvent.click(ungroupBtn);

    // Wait for the API call to resolve
    await waitFor(() => {
      expect(onViewUpdate).toHaveBeenCalledWith(postUngroupView);
    });

    // Re-render with the post-ungroup data
    await act(async () => {
      rerender(
        <MainContent
          activeSection="packages"
          loading={false}
          viewData={postUngroupView as ViewResponse}
          sections={null}
          onViewUpdate={onViewUpdate}
          onMutationError={vi.fn()}
          sectionSearchOpen={false}
          onSectionSearchClose={vi.fn()}
        />,
      );
    });

    // Focus must be on python3.x86_64, NOT python3.11.x86_64
    const correctRow = screen.getByTestId("package-row-python3.x86_64");
    const ambiguousRow = screen.getByTestId("package-row-python3.11.x86_64");

    expect(document.activeElement).toBe(correctRow);
    expect(document.activeElement).not.toBe(ambiguousRow);
  });
});
