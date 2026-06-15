import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { GroupRow } from "../GroupRow";
import type { GroupInfo } from "../../api/types";

const mockGroup: GroupInfo = {
  name: "core",
  member_count: 8,
  added_count: 0, locked_count: 2,
  optional_spillover_count: 0,
  render_state: "renderable",
  degradation_reason: null,
  members: [
    { name: "bash", locked: true, overlap_groups: [] , in_base_image: false},
    { name: "coreutils", locked: true, overlap_groups: [] , in_base_image: false},
    { name: "filesystem", locked: false, overlap_groups: [] , in_base_image: false},
    { name: "glibc", locked: false, overlap_groups: [] , in_base_image: false},
    { name: "grep", locked: false, overlap_groups: [] , in_base_image: false},
    { name: "sed", locked: false, overlap_groups: [] , in_base_image: false},
    { name: "systemd", locked: false, overlap_groups: [] , in_base_image: false},
    { name: "util-linux", locked: false, overlap_groups: [] , in_base_image: false},
  ],
};

const smallGroup: GroupInfo = {
  name: "editors",
  member_count: 3,
  added_count: 0, locked_count: 0,
  optional_spillover_count: 0,
  render_state: "renderable",
  degradation_reason: null,
  members: [
    { name: "nano", locked: false, overlap_groups: [] , in_base_image: false},
    { name: "vim-minimal", locked: false, overlap_groups: [] , in_base_image: false},
    { name: "vi", locked: false, overlap_groups: [] , in_base_image: false},
  ],
};

const degradedGroup: GroupInfo = {
  name: "Container Management",
  member_count: 12,
  added_count: 0, locked_count: 0,
  optional_spillover_count: 0,
  render_state: "degraded",
  degradation_reason: "overlap with another group",
  members: [
    { name: "podman", locked: false, overlap_groups: [] , in_base_image: false},
    { name: "buildah", locked: false, overlap_groups: [] , in_base_image: false},
  ],
};

const excludedWithSpillover: GroupInfo = {
  name: "multimedia",
  member_count: 5,
  added_count: 0, locked_count: 0,
  optional_spillover_count: 3,
  render_state: "excluded",
  degradation_reason: null,
  members: [
    { name: "ffmpeg", locked: false, overlap_groups: [] , in_base_image: false},
    { name: "vlc", locked: false, overlap_groups: [] , in_base_image: false},
  ],
};

describe("GroupRow", () => {
  it("renders group name and package count", () => {
    render(
      <GroupRow
        group={mockGroup}
        onToggle={vi.fn()}
        onUngroup={vi.fn()}
      />,
    );
    expect(screen.getByText("core")).toBeInTheDocument();
    expect(screen.getByText("8 packages")).toBeInTheDocument();
  });

  it("renders singular 'package' for count of 1", () => {
    const singlePkg: GroupInfo = {
      ...smallGroup,
      name: "single",
      member_count: 1,
      members: [{ name: "solo", locked: false, overlap_groups: [] , in_base_image: false}],
    };
    render(
      <GroupRow
        group={singlePkg}
        onToggle={vi.fn()}
        onUngroup={vi.fn()}
      />,
    );
    expect(screen.getByText("1 package")).toBeInTheDocument();
  });

  it("shows locked count when greater than zero", () => {
    render(
      <GroupRow
        group={mockGroup}
        onToggle={vi.fn()}
        onUngroup={vi.fn()}
      />,
    );
    expect(screen.getByText("2 locked")).toBeInTheDocument();
  });

  it("hides locked count when zero", () => {
    render(
      <GroupRow
        group={smallGroup}
        onToggle={vi.fn()}
        onUngroup={vi.fn()}
      />,
    );
    expect(screen.queryByText(/locked/)).not.toBeInTheDocument();
  });

  it("expand/collapse shows member list", () => {
    render(
      <GroupRow
        group={mockGroup}
        onToggle={vi.fn()}
        onUngroup={vi.fn()}
      />,
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

  it("truncates member list to first 5 with 'N more' indicator", () => {
    render(
      <GroupRow
        group={mockGroup}
        onToggle={vi.fn()}
        onUngroup={vi.fn()}
      />,
    );
    const expandBtn = screen.getByRole("button", { name: /expand/i });
    fireEvent.click(expandBtn);

    // First 5 alphabetically: bash, coreutils, filesystem, glibc, grep
    expect(screen.getByText("bash")).toBeInTheDocument();
    expect(screen.getByText("coreutils")).toBeInTheDocument();
    expect(screen.getByText("filesystem")).toBeInTheDocument();
    expect(screen.getByText("glibc")).toBeInTheDocument();
    expect(screen.getByText("grep")).toBeInTheDocument();

    // Remaining 3 truncated
    expect(screen.queryByText("sed")).not.toBeInTheDocument();
    expect(screen.getByText("3 more")).toBeInTheDocument();
  });

  it("shows all members when 5 or fewer", () => {
    render(
      <GroupRow
        group={smallGroup}
        onToggle={vi.fn()}
        onUngroup={vi.fn()}
      />,
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
      <GroupRow
        group={mockGroup}
        onToggle={vi.fn()}
        onUngroup={vi.fn()}
      />,
    );
    const expandBtn = screen.getByRole("button", { name: /expand/i });
    fireEvent.click(expandBtn);

    const bashRow = screen.getByTestId("group-member-bash");
    expect(bashRow).toHaveTextContent("locked");
  });

  it("ungroup button calls onUngroup", () => {
    const onUngroup = vi.fn();
    render(
      <GroupRow
        group={mockGroup}
        onToggle={vi.fn()}
        onUngroup={onUngroup}
      />,
    );
    const ungroupBtn = screen.getByRole("button", { name: /ungroup/i });
    fireEvent.click(ungroupBtn);
    expect(onUngroup).toHaveBeenCalledWith("core");
  });

  it("toggle calls onToggle", () => {
    const onToggle = vi.fn();
    render(
      <GroupRow
        group={mockGroup}
        onToggle={onToggle}
        onUngroup={vi.fn()}
        isIncluded={true}
      />,
    );
    const toggle = screen.getByRole("switch");
    fireEvent.click(toggle);
    expect(onToggle).toHaveBeenCalledWith("core", false);
  });

  it("toggle reflects isIncluded=false state", () => {
    render(
      <GroupRow
        group={mockGroup}
        onToggle={vi.fn()}
        onUngroup={vi.fn()}
        isIncluded={false}
      />,
    );
    const toggle = screen.getByRole("switch");
    expect(toggle).not.toBeChecked();
  });

  it("has left accent border via CSS class", () => {
    render(
      <GroupRow
        group={mockGroup}
        onToggle={vi.fn()}
        onUngroup={vi.fn()}
      />,
    );
    const row = screen.getByTestId("group-row-core");
    expect(row.className).toContain("inspectah-group-row");
  });

  it("members are read-only with no individual toggles", () => {
    render(
      <GroupRow
        group={mockGroup}
        onToggle={vi.fn()}
        onUngroup={vi.fn()}
      />,
    );
    const expandBtn = screen.getByRole("button", { name: /expand/i });
    fireEvent.click(expandBtn);

    // Only the group-level switch should exist, no per-member switches
    const switches = screen.getAllByRole("switch");
    expect(switches).toHaveLength(1);
  });

  // --- Task 26: Keyboard and ARIA ---

  it("group row has role=group with aria-label", () => {
    render(
      <GroupRow
        group={mockGroup}
        onToggle={vi.fn()}
        onUngroup={vi.fn()}
      />,
    );
    const row = screen.getByRole("group", { name: "core, 8 packages" });
    expect(row).toBeInTheDocument();
  });

  it("group row has tabIndex=-1 for programmatic focus", () => {
    render(
      <GroupRow
        group={mockGroup}
        onToggle={vi.fn()}
        onUngroup={vi.fn()}
      />,
    );
    const row = screen.getByTestId("group-row-core");
    expect(row).toHaveAttribute("tabindex", "-1");
  });

  it("chevron has aria-expanded", () => {
    render(
      <GroupRow
        group={mockGroup}
        onToggle={vi.fn()}
        onUngroup={vi.fn()}
      />,
    );
    const chevron = screen.getByRole("button", { name: /expand group/i });
    expect(chevron).toHaveAttribute("aria-expanded", "false");

    fireEvent.click(chevron);
    expect(chevron).toHaveAttribute("aria-expanded", "true");
  });

  it("Enter on group row toggles expansion", () => {
    render(
      <GroupRow
        group={mockGroup}
        onToggle={vi.fn()}
        onUngroup={vi.fn()}
      />,
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
      <GroupRow
        group={mockGroup}
        onToggle={vi.fn()}
        onUngroup={vi.fn()}
      />,
    );
    const chevron = screen.getByRole("button", { name: /expand group/i });

    // Pressing Enter on the chevron button should NOT trigger the
    // row-level keydown handler (event.target !== rowRef.current).
    fireEvent.keyDown(chevron, { key: "Enter" });
    expect(screen.queryByText("bash")).not.toBeInTheDocument();
  });

  // --- Task 27b: Degraded and excluded feedback ---

  it("degraded group has disabled toggle and ungroup", () => {
    render(
      <GroupRow
        group={degradedGroup}
        onToggle={vi.fn()}
        onUngroup={vi.fn()}
      />,
    );
    const toggle = screen.getByRole("switch");
    expect(toggle).toBeDisabled();

    const ungroupBtn = screen.getByRole("button", {
      name: /ungroup container management/i,
    });
    expect(ungroupBtn).toBeDisabled();
  });

  it("degraded group shows 'rendered individually' subtitle", () => {
    render(
      <GroupRow
        group={degradedGroup}
        onToggle={vi.fn()}
        onUngroup={vi.fn()}
      />,
    );
    expect(screen.getByText("rendered individually")).toBeInTheDocument();
  });

  it("degraded group has dimmed styling", () => {
    render(
      <GroupRow
        group={degradedGroup}
        onToggle={vi.fn()}
        onUngroup={vi.fn()}
      />,
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
    expect(
      screen.getByText("3 optional still included"),
    ).toBeInTheDocument();
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

  it("renderable group has enabled toggle and ungroup", () => {
    render(
      <GroupRow
        group={mockGroup}
        onToggle={vi.fn()}
        onUngroup={vi.fn()}
      />,
    );
    const toggle = screen.getByRole("switch");
    expect(toggle).not.toBeDisabled();

    const ungroupBtn = screen.getByRole("button", { name: /ungroup core/i });
    expect(ungroupBtn).not.toBeDisabled();
  });
});
