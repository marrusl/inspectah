import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { GroupRow } from "../GroupRow";
import type { GroupInfo } from "../../api/types";

const mockGroup: GroupInfo = {
  name: "core",
  member_count: 8,
  added_count: 8,
  locked_count: 2,
  optional_spillover_count: 0,
  render_state: "renderable",
  degradation_reason: null,
  members: [
    { name: "bash", locked: true, overlap_groups: [], in_base_image: false },
    {
      name: "coreutils",
      locked: true,
      overlap_groups: [],
      in_base_image: false,
    },
    {
      name: "filesystem",
      locked: false,
      overlap_groups: [],
      in_base_image: false,
    },
    { name: "glibc", locked: false, overlap_groups: [], in_base_image: false },
    { name: "grep", locked: false, overlap_groups: [], in_base_image: false },
    { name: "sed", locked: false, overlap_groups: [], in_base_image: false },
    {
      name: "systemd",
      locked: false,
      overlap_groups: [],
      in_base_image: false,
    },
    {
      name: "util-linux",
      locked: false,
      overlap_groups: [],
      in_base_image: false,
    },
  ],
};

const smallGroup: GroupInfo = {
  name: "editors",
  member_count: 3,
  added_count: 0,
  locked_count: 0,
  optional_spillover_count: 0,
  render_state: "renderable",
  degradation_reason: null,
  members: [
    { name: "nano", locked: false, overlap_groups: [], in_base_image: false },
    {
      name: "vim-minimal",
      locked: false,
      overlap_groups: [],
      in_base_image: false,
    },
    { name: "vi", locked: false, overlap_groups: [], in_base_image: false },
  ],
};

const degradedGroup: GroupInfo = {
  name: "Container Management",
  member_count: 12,
  added_count: 0,
  locked_count: 0,
  optional_spillover_count: 0,
  render_state: "degraded",
  degradation_reason: "overlap with another group",
  members: [
    { name: "podman", locked: false, overlap_groups: [], in_base_image: false },
    {
      name: "buildah",
      locked: false,
      overlap_groups: [],
      in_base_image: false,
    },
  ],
};

const excludedWithSpillover: GroupInfo = {
  name: "multimedia",
  member_count: 5,
  added_count: 0,
  locked_count: 0,
  optional_spillover_count: 3,
  render_state: "excluded",
  degradation_reason: null,
  members: [
    { name: "ffmpeg", locked: false, overlap_groups: [], in_base_image: false },
    { name: "vlc", locked: false, overlap_groups: [], in_base_image: false },
  ],
};

describe("GroupRow", () => {
  it("renders group name and package count", () => {
    render(
      <GroupRow group={mockGroup} onToggle={vi.fn()} onUngroup={vi.fn()} />,
    );
    expect(screen.getByText("core")).toBeInTheDocument();
    expect(screen.getByText("8 packages")).toBeInTheDocument();
  });

  it("renders singular 'package' for count of 1", () => {
    const singlePkg: GroupInfo = {
      ...smallGroup,
      name: "single",
      member_count: 1,
      added_count: 1,
      members: [
        {
          name: "solo",
          locked: false,
          overlap_groups: [],
          in_base_image: false,
        },
      ],
    };
    render(
      <GroupRow group={singlePkg} onToggle={vi.fn()} onUngroup={vi.fn()} />,
    );
    expect(screen.getByText("1 package")).toBeInTheDocument();
  });

  it("shows locked count when greater than zero", () => {
    render(
      <GroupRow group={mockGroup} onToggle={vi.fn()} onUngroup={vi.fn()} />,
    );
    expect(screen.getByText("2 locked")).toBeInTheDocument();
  });

  it("hides locked count when zero", () => {
    render(
      <GroupRow group={smallGroup} onToggle={vi.fn()} onUngroup={vi.fn()} />,
    );
    expect(screen.queryByText(/locked/)).not.toBeInTheDocument();
  });

  it("expand/collapse shows member list", () => {
    render(
      <GroupRow group={mockGroup} onToggle={vi.fn()} onUngroup={vi.fn()} />,
    );
    // Members hidden by default
    expect(screen.queryByText("bash")).not.toBeInTheDocument();

    // Click chevron to expand
    const expandBtn = screen.getByRole("button", { name: /expand/i });
    fireEvent.click(expandBtn);

    // Members visible
    expect(screen.getByText("bash")).toBeInTheDocument();
    expect(screen.getByText("coreutils")).toBeInTheDocument();

    // Click again to collapse
    fireEvent.click(expandBtn);
    expect(screen.queryByText("bash")).not.toBeInTheDocument();
  });

  it("truncates member list to first 5 with 'Show all' button", () => {
    render(
      <GroupRow group={mockGroup} onToggle={vi.fn()} onUngroup={vi.fn()} />,
    );
    const expandBtn = screen.getByRole("button", { name: /expand/i });
    fireEvent.click(expandBtn);

    // First 5 alphabetically: bash, coreutils, filesystem, glibc, grep
    expect(screen.getByText("bash")).toBeInTheDocument();
    expect(screen.getByText("coreutils")).toBeInTheDocument();
    expect(screen.getByText("filesystem")).toBeInTheDocument();
    expect(screen.getByText("glibc")).toBeInTheDocument();
    expect(screen.getByText("grep")).toBeInTheDocument();

    // Remaining 3 truncated, "Show all" button shown
    expect(screen.queryByText("sed")).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /show all 8 members/i }),
    ).toBeInTheDocument();
  });

  it("shows all members when 5 or fewer", () => {
    render(
      <GroupRow group={smallGroup} onToggle={vi.fn()} onUngroup={vi.fn()} />,
    );
    const expandBtn = screen.getByRole("button", { name: /expand/i });
    fireEvent.click(expandBtn);

    expect(screen.getByText("nano")).toBeInTheDocument();
    expect(screen.getByText("vi")).toBeInTheDocument();
    expect(screen.getByText("vim-minimal")).toBeInTheDocument();
    expect(screen.queryByText(/more$/)).not.toBeInTheDocument();
  });

  it("shows locked indicator on locked members", () => {
    render(
      <GroupRow group={mockGroup} onToggle={vi.fn()} onUngroup={vi.fn()} />,
    );
    const expandBtn = screen.getByRole("button", { name: /expand/i });
    fireEvent.click(expandBtn);

    const bashRow = screen.getByTestId("group-member-bash");
    expect(bashRow).toHaveTextContent("locked");
  });

  it("ungroup button calls onUngroup", () => {
    const onUngroup = vi.fn();
    render(
      <GroupRow group={mockGroup} onToggle={vi.fn()} onUngroup={onUngroup} />,
    );
    const ungroupBtn = screen.getByRole("button", { name: /ungroup/i });
    fireEvent.click(ungroupBtn);
    expect(onUngroup).toHaveBeenCalledWith("core");
  });

  it("no group-level toggle switch is rendered", () => {
    render(
      <GroupRow group={mockGroup} onToggle={vi.fn()} onUngroup={vi.fn()} />,
    );
    // Group-level toggle was removed — groups are managed via ungroup
    // or per-member actions, not a toggle switch.
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
  });

  it("has left accent border via CSS class", () => {
    render(
      <GroupRow group={mockGroup} onToggle={vi.fn()} onUngroup={vi.fn()} />,
    );
    const row = screen.getByTestId("group-row-core");
    expect(row.className).toContain("inspectah-group-row");
  });

  it("members are read-only with no individual toggles", () => {
    render(
      <GroupRow group={mockGroup} onToggle={vi.fn()} onUngroup={vi.fn()} />,
    );
    const expandBtn = screen.getByRole("button", { name: /expand/i });
    fireEvent.click(expandBtn);

    // No switches at all — group toggle removed, members are read-only
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
  });

  // --- Task 26: Keyboard and ARIA ---

  it("group row has role=group with aria-label", () => {
    render(
      <GroupRow group={mockGroup} onToggle={vi.fn()} onUngroup={vi.fn()} />,
    );
    const row = screen.getByRole("group", { name: "core, 8 packages" });
    expect(row).toBeInTheDocument();
  });

  it("group row has tabIndex=-1 for programmatic focus", () => {
    render(
      <GroupRow group={mockGroup} onToggle={vi.fn()} onUngroup={vi.fn()} />,
    );
    const row = screen.getByTestId("group-row-core");
    expect(row).toHaveAttribute("tabindex", "-1");
  });

  it("chevron has aria-expanded", () => {
    render(
      <GroupRow group={mockGroup} onToggle={vi.fn()} onUngroup={vi.fn()} />,
    );
    const chevron = screen.getByRole("button", { name: /expand group/i });
    expect(chevron).toHaveAttribute("aria-expanded", "false");

    fireEvent.click(chevron);
    expect(chevron).toHaveAttribute("aria-expanded", "true");
  });

  it("Enter on group row toggles expansion", () => {
    render(
      <GroupRow group={mockGroup} onToggle={vi.fn()} onUngroup={vi.fn()} />,
    );
    const row = screen.getByTestId("group-row-core");

    // Members hidden initially
    expect(screen.queryByText("bash")).not.toBeInTheDocument();

    // Press Enter on the group row itself
    fireEvent.keyDown(row, { key: "Enter" });
    expect(screen.getByText("bash")).toBeInTheDocument();

    // Press Enter again to collapse
    fireEvent.keyDown(row, { key: "Enter" });
    expect(screen.queryByText("bash")).not.toBeInTheDocument();
  });

  it("Enter on child buttons does not double-toggle expansion", () => {
    render(
      <GroupRow group={mockGroup} onToggle={vi.fn()} onUngroup={vi.fn()} />,
    );
    const chevron = screen.getByRole("button", { name: /expand group/i });

    // Pressing Enter on the chevron button should NOT trigger the
    // row-level keydown handler (event.target !== rowRef.current).
    fireEvent.keyDown(chevron, { key: "Enter" });
    expect(screen.queryByText("bash")).not.toBeInTheDocument();
  });

  // --- Task 27b: Degraded and excluded feedback ---

  it("degraded group has disabled ungroup button", () => {
    render(
      <GroupRow group={degradedGroup} onToggle={vi.fn()} onUngroup={vi.fn()} />,
    );
    // No toggle switch at all (removed)
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();

    const ungroupBtn = screen.getByRole("button", {
      name: /ungroup container management/i,
    });
    expect(ungroupBtn).toBeDisabled();
  });

  it("degraded group shows 'rendered individually' subtitle", () => {
    render(
      <GroupRow group={degradedGroup} onToggle={vi.fn()} onUngroup={vi.fn()} />,
    );
    expect(screen.getByText("rendered individually")).toBeInTheDocument();
  });

  it("degraded group has dimmed styling", () => {
    render(
      <GroupRow group={degradedGroup} onToggle={vi.fn()} onUngroup={vi.fn()} />,
    );
    const row = screen.getByTestId("group-row-Container Management");
    expect(row.className).toContain("inspectah-group-row--degraded");
  });

  it("excluded group shows optional count", () => {
    render(
      <GroupRow
        group={excludedWithSpillover}
        onToggle={vi.fn()}
        onUngroup={vi.fn()}
      />,
    );
    expect(screen.getByText("3 optional still included")).toBeInTheDocument();
  });

  it("excluded group without spillover hides optional count", () => {
    const excludedNoSpillover: GroupInfo = {
      ...excludedWithSpillover,
      optional_spillover_count: 0,
    };
    render(
      <GroupRow
        group={excludedNoSpillover}
        onToggle={vi.fn()}
        onUngroup={vi.fn()}
      />,
    );
    expect(
      screen.queryByText(/optional still included/),
    ).not.toBeInTheDocument();
  });

  it("has aria-live region for announcements", () => {
    const { container } = render(
      <GroupRow
        group={mockGroup}
        onToggle={vi.fn()}
        onUngroup={vi.fn()}
        announcement="Group toggled"
      />,
    );
    const liveRegion = container.querySelector("[aria-live='polite']");
    expect(liveRegion).toBeInTheDocument();
    expect(liveRegion).toHaveTextContent("Group toggled");
  });

  it("renderable group has enabled ungroup button", () => {
    render(
      <GroupRow group={mockGroup} onToggle={vi.fn()} onUngroup={vi.fn()} />,
    );
    // No toggle switch (removed)
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();

    const ungroupBtn = screen.getByRole("button", { name: /ungroup core/i });
    expect(ungroupBtn).not.toBeDisabled();
  });

  // --- Task 2: Header labels, base-image members, progressive disclosure ---

  it("shows 'all from base' when added_count is 0", () => {
    const allBaseGroup: GroupInfo = {
      ...smallGroup,
      added_count: 0,
      member_count: 3,
    };
    render(
      <GroupRow group={allBaseGroup} onToggle={vi.fn()} onUngroup={vi.fn()} />,
    );
    expect(screen.getByText("3 packages (all from base)")).toBeInTheDocument();
  });

  it("shows 'N new, M from base' for mixed groups", () => {
    const mixedGroup: GroupInfo = {
      ...smallGroup,
      name: "mixed",
      member_count: 5,
      added_count: 3,
      members: [
        {
          name: "alpha",
          locked: false,
          overlap_groups: [],
          in_base_image: false,
        },
        {
          name: "beta",
          locked: false,
          overlap_groups: [],
          in_base_image: false,
        },
        {
          name: "gamma",
          locked: false,
          overlap_groups: [],
          in_base_image: false,
        },
        {
          name: "delta",
          locked: false,
          overlap_groups: [],
          in_base_image: true,
        },
        {
          name: "epsilon",
          locked: false,
          overlap_groups: [],
          in_base_image: true,
        },
      ],
    };
    render(
      <GroupRow group={mixedGroup} onToggle={vi.fn()} onUngroup={vi.fn()} />,
    );
    expect(screen.getByText("3 new, 2 from base")).toBeInTheDocument();
  });

  it("shows 'N packages' when all are new", () => {
    const allNewGroup: GroupInfo = {
      ...smallGroup,
      name: "allnew",
      member_count: 3,
      added_count: 3,
    };
    render(
      <GroupRow group={allNewGroup} onToggle={vi.fn()} onUngroup={vi.fn()} />,
    );
    expect(screen.getByText("3 packages")).toBeInTheDocument();
  });

  it("renders base-image members with (from base) label", () => {
    const baseGroup: GroupInfo = {
      ...smallGroup,
      name: "basetest",
      member_count: 2,
      added_count: 1,
      members: [
        {
          name: "new-pkg",
          locked: false,
          overlap_groups: [],
          in_base_image: false,
        },
        {
          name: "base-pkg",
          locked: false,
          overlap_groups: [],
          in_base_image: true,
        },
      ],
    };
    render(
      <GroupRow
        group={baseGroup}
        onToggle={vi.fn()}
        onUngroup={vi.fn()}
        defaultExpanded={true}
      />,
    );
    const baseMember = screen.getByTestId("group-member-base-pkg");
    expect(baseMember).toHaveTextContent("(from base)");
    // New member should not have the label
    const newMember = screen.getByTestId("group-member-new-pkg");
    expect(newMember).not.toHaveTextContent("(from base)");
  });

  it("base-image members have aria-label", () => {
    const baseGroup: GroupInfo = {
      ...smallGroup,
      name: "ariatest",
      member_count: 2,
      added_count: 1,
      members: [
        {
          name: "new-pkg",
          locked: false,
          overlap_groups: [],
          in_base_image: false,
        },
        {
          name: "base-pkg",
          locked: false,
          overlap_groups: [],
          in_base_image: true,
        },
      ],
    };
    render(
      <GroupRow
        group={baseGroup}
        onToggle={vi.fn()}
        onUngroup={vi.fn()}
        defaultExpanded={true}
      />,
    );
    const baseMemberName = screen
      .getByTestId("group-member-base-pkg")
      .querySelector(".inspectah-group-row__member-name");
    expect(baseMemberName).toHaveAttribute(
      "aria-label",
      "base-pkg (from base image, no action needed)",
    );
    // New member should not have aria-label
    const newMemberName = screen
      .getByTestId("group-member-new-pkg")
      .querySelector(".inspectah-group-row__member-name");
    expect(newMemberName).not.toHaveAttribute("aria-label");
  });

  it("shows 'Show all N members' when list exceeds 5", () => {
    render(
      <GroupRow
        group={mockGroup}
        onToggle={vi.fn()}
        onUngroup={vi.fn()}
        defaultExpanded={true}
      />,
    );
    expect(
      screen.getByRole("button", { name: /show all 8 members/i }),
    ).toBeInTheDocument();
    // 6th member should not be visible yet
    expect(screen.queryByText("sed")).not.toBeInTheDocument();
  });

  it("clicking 'Show all' expands to full list", () => {
    render(
      <GroupRow
        group={mockGroup}
        onToggle={vi.fn()}
        onUngroup={vi.fn()}
        defaultExpanded={true}
      />,
    );
    // Initially truncated
    expect(screen.queryByText("sed")).not.toBeInTheDocument();

    // Click "Show all"
    const showAllBtn = screen.getByRole("button", {
      name: /show all 8 members/i,
    });
    fireEvent.click(showAllBtn);

    // All members visible
    expect(screen.getByText("sed")).toBeInTheDocument();
    expect(screen.getByText("systemd")).toBeInTheDocument();
    expect(screen.getByText("util-linux")).toBeInTheDocument();

    // Button now says "Show less"
    expect(
      screen.getByRole("button", { name: /show less/i }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /show all/i }),
    ).not.toBeInTheDocument();

    // Click "Show less" to collapse back
    fireEvent.click(screen.getByRole("button", { name: /show less/i }));
    expect(screen.queryByText("sed")).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /show all 8 members/i }),
    ).toBeInTheDocument();
  });
});
