import { render, screen, waitFor } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { PackageList } from "../PackageList";
import type { RepoGroupInfo, GroupInfo } from "../../api/types";

// --- Test data ---

const distroRepo: RepoGroupInfo = {
  section_id: "baseos",
  provenance: "verified",
  is_distro: true,
  tier: "distro",
  package_count: 10,
  enabled: true,
};

const coreGroup: GroupInfo = {
  name: "core",
  member_count: 3,
  added_count: 0,
  locked_count: 0,
  optional_spillover_count: 0,
  render_state: "renderable",
  degradation_reason: null,
  members: [
    { name: "bash", locked: false, overlap_groups: [], in_base_image: false },
    {
      name: "coreutils",
      locked: false,
      overlap_groups: [],
      in_base_image: false,
    },
    {
      name: "systemd",
      locked: false,
      overlap_groups: [],
      in_base_image: false,
    },
  ],
};

const editorsGroup: GroupInfo = {
  name: "editors",
  member_count: 2,
  added_count: 0,
  locked_count: 0,
  optional_spillover_count: 0,
  render_state: "renderable",
  degradation_reason: null,
  members: [
    { name: "nano", locked: false, overlap_groups: [], in_base_image: false },
    { name: "vim", locked: false, overlap_groups: [], in_base_image: false },
  ],
};

const degradedGroup: GroupInfo = {
  name: "Degraded Group",
  member_count: 2,
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

const excludedGroup: GroupInfo = {
  name: "Excluded Group",
  member_count: 3,
  added_count: 0,
  locked_count: 0,
  optional_spillover_count: 2,
  render_state: "excluded",
  degradation_reason: null,
  members: [
    { name: "ffmpeg", locked: false, overlap_groups: [], in_base_image: false },
    { name: "vlc", locked: false, overlap_groups: [], in_base_image: false },
  ],
};

describe("PackageList - Task 27c: Search focus and count behavior", () => {
  it("search for group name focuses group row, not auto-expanded", async () => {
    render(
      <PackageList
        mode="single"
        packages={[
          { name: "bash", source_repo: "baseos", include: true },
          { name: "nano", source_repo: "baseos", include: true },
        ]}
        repoGroups={[distroRepo]}
        packageGroups={[coreGroup, editorsGroup]}
        searchQuery="core"
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );

    await waitFor(() => {
      const groupRow = screen.getByTestId("group-row-core");
      expect(document.activeElement).toBe(groupRow);
    });

    // Group should NOT be auto-expanded (only member matches trigger expansion)
    expect(screen.queryByTestId("group-member-bash")).not.toBeInTheDocument();
  });

  it("search for member name focuses member row inside auto-expanded group", async () => {
    render(
      <PackageList
        mode="single"
        packages={[
          { name: "bash", source_repo: "baseos", include: true },
          { name: "nano", source_repo: "baseos", include: true },
        ]}
        repoGroups={[distroRepo]}
        packageGroups={[coreGroup, editorsGroup]}
        searchQuery="nano"
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );

    // Group should be auto-expanded
    const memberRow = await screen.findByTestId("group-member-nano");
    expect(memberRow).toBeInTheDocument();

    // Member row should be focused
    await waitFor(() => {
      expect(document.activeElement).toBe(memberRow);
    });
  });

  it("summary bar shows unique package count not visible rows", () => {
    const overlappingGroup1: GroupInfo = {
      name: "group1",
      member_count: 3,
      added_count: 0,
      locked_count: 0,
      optional_spillover_count: 0,
      render_state: "renderable",
      degradation_reason: null,
      members: [
        {
          name: "shared-pkg",
          locked: false,
          overlap_groups: ["group2"],
          in_base_image: false,
        },
        {
          name: "pkg-a",
          locked: false,
          overlap_groups: [],
          in_base_image: false,
        },
        {
          name: "pkg-b",
          locked: false,
          overlap_groups: [],
          in_base_image: false,
        },
      ],
    };

    const overlappingGroup2: GroupInfo = {
      name: "group2",
      member_count: 2,
      added_count: 0,
      locked_count: 0,
      optional_spillover_count: 0,
      render_state: "renderable",
      degradation_reason: null,
      members: [
        {
          name: "shared-pkg",
          locked: false,
          overlap_groups: ["group1"],
          in_base_image: false,
        },
        {
          name: "pkg-c",
          locked: false,
          overlap_groups: [],
          in_base_image: false,
        },
      ],
    };

    render(
      <PackageList
        mode="single"
        packages={[
          { name: "individual-pkg", source_repo: "baseos", include: true },
        ]}
        repoGroups={[distroRepo]}
        packageGroups={[overlappingGroup1, overlappingGroup2]}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );

    const summary = screen.getByTestId("package-list-summary");
    // 2 groups, but only 4 unique packages (shared-pkg counted once)
    expect(summary).toHaveTextContent("2 groups (4 packages)");
    expect(summary).toHaveTextContent("1 other package");
  });

  it("excluded and degraded groups visible in zone, ungrouped filtered out", () => {
    const ungroupedGroup: GroupInfo = {
      name: "Ungrouped Group",
      member_count: 1,
      added_count: 0,
      locked_count: 0,
      optional_spillover_count: 0,
      render_state: "ungrouped",
      degradation_reason: null,
      members: [
        {
          name: "orphan",
          locked: false,
          overlap_groups: [],
          in_base_image: false,
        },
      ],
    };

    render(
      <PackageList
        mode="single"
        packages={[]}
        repoGroups={[distroRepo]}
        packageGroups={[degradedGroup, excludedGroup, ungroupedGroup]}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );

    // Degraded: visible in groups zone (all non-ungrouped states shown)
    expect(screen.getByTestId("group-row-Degraded Group")).toBeInTheDocument();

    // Excluded: visible in groups zone with toggle off
    expect(screen.getByTestId("group-row-Excluded Group")).toBeInTheDocument();

    // Ungrouped: NOT in groups zone
    expect(
      screen.queryByTestId("group-row-Ungrouped Group"),
    ).not.toBeInTheDocument();
  });

  it("filtered count during search shows matching groups and packages", () => {
    render(
      <PackageList
        mode="single"
        packages={[
          { name: "bash", source_repo: "baseos", include: true },
          { name: "nano", source_repo: "baseos", include: true },
          { name: "vim", source_repo: "baseos", include: true },
          { name: "grep", source_repo: "baseos", include: true },
          { name: "nano-utils", source_repo: "baseos", include: true },
        ]}
        repoGroups={[distroRepo]}
        packageGroups={[coreGroup, editorsGroup]}
        searchQuery="nano"
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );

    const summary = screen.getByTestId("package-list-summary");
    // During search: 1 group (editors contains nano),
    // 1 other (nano-utils matches; nano is suppressed as a group member)
    expect(summary).toHaveTextContent("1 group");
    expect(summary).toHaveTextContent("1 other package");
  });
});
