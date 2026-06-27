import { render, screen, fireEvent, within } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { PackageList } from "../PackageList";
import type { RepoGroupInfo, GroupInfo } from "../../api/types";

// --- Test data factories ---

interface TestPackage {
  name: string;
  source_repo: string;
  include: boolean;
  prevalence?: { count: number; total: number };
  repo_conflict?: { repo: string; host_count: number }[];
}

function makePkg(
  name: string,
  source_repo: string,
  include = true,
  extras?: Partial<TestPackage>,
): TestPackage {
  return { name, source_repo, include, ...extras };
}

function makeAggregatePkg(
  name: string,
  source_repo: string,
  count: number,
  total: number,
  include = true,
  extras?: Partial<TestPackage>,
): TestPackage {
  return {
    name,
    source_repo,
    include,
    prevalence: { count, total },
    ...extras,
  };
}

const distroRepo: RepoGroupInfo = {
  section_id: "baseos",
  provenance: "verified",
  is_distro: true,
  tier: "distro",
  package_count: 10,
  enabled: true,
};

const officialRepo: RepoGroupInfo = {
  section_id: "crb",
  provenance: "verified",
  is_distro: false,
  tier: "official_optional",
  package_count: 3,
  enabled: true,
};

const thirdPartyRepo: RepoGroupInfo = {
  section_id: "epel",
  provenance: "incomplete",
  is_distro: false,
  tier: "third_party",
  package_count: 2,
  enabled: true,
};

const allRepos = [distroRepo, officialRepo, thirdPartyRepo];

describe("PackageList", () => {
  // --- Basic rendering ---

  it("renders package name and repo text for each package", () => {
    const pkgs = [makePkg("bash", "baseos"), makePkg("nginx", "epel")];
    render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    expect(screen.getByText("bash")).toBeInTheDocument();
    expect(screen.getByText("nginx")).toBeInTheDocument();
    expect(screen.getByText("baseos")).toBeInTheDocument();
    expect(screen.getByText("epel")).toBeInTheDocument();
  });

  // --- Single-machine layout ---

  it("single-machine: renders repo in a separate right column", () => {
    const pkgs = [makePkg("curl", "baseos")];
    render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    const row = screen.getByTestId("package-row-curl");
    const rightCol = within(row).getByTestId("right-column");
    expect(within(rightCol).getByText("baseos")).toBeInTheDocument();
  });

  // --- Aggregate layout ---

  it("aggregate: renders repo inline with name, prevalence in right column", () => {
    const pkgs = [makeAggregatePkg("httpd", "appstream", 3, 5)];
    const repos = [{ ...distroRepo, section_id: "appstream" }];
    render(
      <PackageList
        mode="aggregate"
        packages={pkgs}
        repoGroups={repos}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    const row = screen.getByTestId("package-row-httpd");
    const rightCol = within(row).getByTestId("right-column");
    expect(within(rightCol).getByText("3/5")).toBeInTheDocument();
    // Repo should appear inline (left side), not in the right column
    const leftCol = within(row).getByTestId("left-column");
    expect(within(leftCol).getByText("appstream")).toBeInTheDocument();
  });

  // --- Sort: alphabetical by package name ---

  it("sorts by package name ascending (single-machine default)", () => {
    const pkgs = [
      makePkg("zsh", "baseos"),
      makePkg("bash", "baseos"),
      makePkg("curl", "baseos"),
    ];
    render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    const rows = screen.getAllByTestId(/^package-row-/);
    expect(rows[0]).toHaveAttribute("data-testid", "package-row-bash");
    expect(rows[1]).toHaveAttribute("data-testid", "package-row-curl");
    expect(rows[2]).toHaveAttribute("data-testid", "package-row-zsh");
  });

  // --- Sort: repo tier-first (single-machine right column sort) ---

  it("sorts by repo tier-first when right column sort active (single-machine)", () => {
    const pkgs = [
      makePkg("nginx", "epel"), // third_party
      makePkg("bash", "baseos"), // distro
      makePkg("devel", "crb"), // official_optional
    ];
    render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    // Click the right column header ("Repo") to sort by repo tier
    const repoHeader = screen.getByRole("columnheader", { name: /repo/i });
    fireEvent.click(repoHeader);

    const rows = screen.getAllByTestId(/^package-row-/);
    // distro first, then official_optional, then third_party
    expect(rows[0]).toHaveAttribute("data-testid", "package-row-bash");
    expect(rows[1]).toHaveAttribute("data-testid", "package-row-devel");
    expect(rows[2]).toHaveAttribute("data-testid", "package-row-nginx");
  });

  // --- Sort: prevalence ascending (aggregate default) ---

  it("sorts by prevalence ascending — rarest first (aggregate default)", () => {
    const pkgs = [
      makeAggregatePkg("httpd", "baseos", 5, 5),
      makeAggregatePkg("nginx", "epel", 1, 5),
      makeAggregatePkg("curl", "baseos", 3, 5),
    ];
    render(
      <PackageList
        mode="aggregate"
        packages={pkgs}
        repoGroups={allRepos}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    const rows = screen.getAllByTestId(/^package-row-/);
    // rarest first: nginx (1/5), curl (3/5), httpd (5/5)
    expect(rows[0]).toHaveAttribute("data-testid", "package-row-nginx");
    expect(rows[1]).toHaveAttribute("data-testid", "package-row-curl");
    expect(rows[2]).toHaveAttribute("data-testid", "package-row-httpd");
  });

  // --- Checkbox toggle ---

  it("checkbox toggle calls onToggle with package name", () => {
    const onToggle = vi.fn();
    const pkgs = [makePkg("bash", "baseos")];
    render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        onToggle={onToggle}
        onRepoToggle={vi.fn()}
      />,
    );
    const checkbox = screen.getByRole("checkbox", { name: /bash/i });
    fireEvent.click(checkbox);
    expect(onToggle).toHaveBeenCalledWith("bash");
  });

  // --- ExcludedZone: packages from disabled repos ---

  it("shows excluded packages in ExcludedZone when a repo is disabled", () => {
    const disabledEpel: RepoGroupInfo = { ...thirdPartyRepo, enabled: false };
    const pkgs = [
      makePkg("bash", "baseos"),
      makePkg("nginx", "epel"),
      makePkg("jq", "epel"),
    ];
    render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={[distroRepo, officialRepo, disabledEpel]}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    const excluded = screen.getByTestId("excluded-zone");
    expect(within(excluded).getByText("nginx")).toBeInTheDocument();
    expect(within(excluded).getByText("jq")).toBeInTheDocument();
  });

  // --- Repo text styling ---

  it("renders distro repo text in muted color", () => {
    const pkgs = [makePkg("bash", "baseos")];
    render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    const row = screen.getByTestId("package-row-bash");
    const repoText = within(row).getByTestId("repo-text");
    expect(repoText).toHaveStyle({
      color: "var(--pf-t--global--text--color--subtle)",
    });
  });

  it("renders official-optional repo with green text and dotted underline", () => {
    const pkgs = [makePkg("devel", "crb")];
    render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    const row = screen.getByTestId("package-row-devel");
    const repoText = within(row).getByTestId("repo-text");
    expect(repoText).toHaveStyle({
      color: "var(--pf-t--global--color--status--success--default)",
      textDecorationStyle: "dotted",
    });
  });

  it("renders third-party repo with amber text and solid underline", () => {
    const pkgs = [makePkg("nginx", "epel")];
    render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    const row = screen.getByTestId("package-row-nginx");
    const repoText = within(row).getByTestId("repo-text");
    expect(repoText).toHaveStyle({
      color: "var(--pf-t--global--color--status--warning--default)",
      textDecorationStyle: "solid",
    });
  });

  // --- SortHeader labels per mode ---

  it("single-machine: SortHeader shows Packages / Repo", () => {
    render(
      <PackageList
        mode="single"
        packages={[]}
        repoGroups={allRepos}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    expect(
      screen.getByRole("columnheader", { name: /packages/i }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("columnheader", { name: /repo/i }),
    ).toBeInTheDocument();
  });

  it("aggregate: SortHeader shows Packages / Prevalence", () => {
    render(
      <PackageList
        mode="aggregate"
        packages={[]}
        repoGroups={allRepos}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    expect(
      screen.getByRole("columnheader", { name: /packages/i }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("columnheader", { name: /prevalence/i }),
    ).toBeInTheDocument();
  });

  // --- Dismissed-state wiring ---

  it("reports dismissed count to parent via onDismissedCountChange", () => {
    const onDismissedCountChange = vi.fn();
    const pkgs = [
      makeAggregatePkg("httpd", "baseos", 3, 5, true, {
        repo_conflict: [
          { repo: "baseos", host_count: 2 },
          { repo: "appstream", host_count: 1 },
        ],
      }),
    ];
    render(
      <PackageList
        mode="aggregate"
        packages={pkgs}
        repoGroups={allRepos}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
        onDismissedCountChange={onDismissedCountChange}
      />,
    );
    // Initially 0 dismissed
    expect(onDismissedCountChange).toHaveBeenCalledWith(0);
  });

  it("onRestoreDismissed clears all dismissed and resets count", () => {
    const onDismissedCountChange = vi.fn();
    const pkgs = [
      makeAggregatePkg("httpd", "baseos", 3, 5, true, {
        repo_conflict: [
          { repo: "baseos", host_count: 2 },
          { repo: "appstream", host_count: 1 },
        ],
      }),
    ];
    const { rerender } = render(
      <PackageList
        mode="aggregate"
        packages={pkgs}
        repoGroups={allRepos}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
        onDismissedCountChange={onDismissedCountChange}
        onRestoreDismissed={false}
      />,
    );
    // Trigger restore by passing true
    rerender(
      <PackageList
        mode="aggregate"
        packages={pkgs}
        repoGroups={allRepos}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
        onDismissedCountChange={onDismissedCountChange}
        onRestoreDismissed={true}
      />,
    );
    // After restore, dismissed count should be 0
    expect(onDismissedCountChange).toHaveBeenLastCalledWith(0);
  });

  // --- Aggregate conflict-first sorting ---

  it("aggregate: packages with undismissed repo_conflict sort before others in same prevalence group", () => {
    const pkgs = [
      makeAggregatePkg("aaa-clean", "baseos", 3, 5),
      makeAggregatePkg("bbb-conflict", "baseos", 3, 5, true, {
        repo_conflict: [
          { repo: "baseos", host_count: 2 },
          { repo: "epel", host_count: 1 },
        ],
      }),
    ];
    render(
      <PackageList
        mode="aggregate"
        packages={pkgs}
        repoGroups={allRepos}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    const rows = screen.getAllByTestId(/^package-row-/);
    // bbb-conflict has undismissed conflict → sorts before aaa-clean at same prevalence
    expect(rows[0]).toHaveAttribute("data-testid", "package-row-bbb-conflict");
    expect(rows[1]).toHaveAttribute("data-testid", "package-row-aaa-clean");
  });

  // --- Sort toggle direction ---

  it("clicking the active sort column toggles direction", () => {
    const pkgs = [makePkg("zsh", "baseos"), makePkg("bash", "baseos")];
    render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    // Default: left column, asc → bash first
    let rows = screen.getAllByTestId(/^package-row-/);
    expect(rows[0]).toHaveAttribute("data-testid", "package-row-bash");

    // Click left column button → toggle to desc → zsh first
    const pkgHeader = screen.getByRole("columnheader", { name: /packages/i });
    const pkgBtn = within(pkgHeader).getByRole("button");
    fireEvent.click(pkgBtn);
    rows = screen.getAllByTestId(/^package-row-/);
    expect(rows[0]).toHaveAttribute("data-testid", "package-row-zsh");
  });

  // --- hasEverToggled tracking ---

  it("ExcludedZone is hidden when no repo has ever been toggled", () => {
    const pkgs = [makePkg("bash", "baseos")];
    render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    expect(screen.queryByTestId("excluded-zone")).not.toBeInTheDocument();
  });

  it("excluded zone stays visible with 'No excluded packages' after repo re-enabled (latched)", () => {
    const disabledEpel: RepoGroupInfo = { ...thirdPartyRepo, enabled: false };
    const pkgs = [makePkg("bash", "baseos"), makePkg("nginx", "epel")];
    // Render with repo disabled — excluded zone appears
    const { rerender } = render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={[distroRepo, officialRepo, disabledEpel]}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    expect(screen.getByTestId("excluded-zone")).toBeInTheDocument();

    // Re-enable the repo — excluded zone should still be visible (latched)
    rerender(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    // Zone stays visible, showing "No excluded packages" message
    expect(screen.getByText("No excluded packages")).toBeInTheDocument();
  });

  // --- Aggregate conflict popover in rows ---

  it("aggregate: renders RepoConflictPopover trigger for packages with repo_conflict", () => {
    const pkgs = [
      makeAggregatePkg("httpd", "baseos", 3, 5, true, {
        repo_conflict: [
          { repo: "baseos", host_count: 2 },
          { repo: "appstream", host_count: 1 },
        ],
      }),
      makeAggregatePkg("curl", "baseos", 5, 5),
    ];
    render(
      <PackageList
        mode="aggregate"
        packages={pkgs}
        repoGroups={allRepos}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    // httpd has conflict — popover trigger should be present
    const httpdRow = screen.getByTestId("package-row-httpd");
    expect(
      within(httpdRow).getByRole("button", { name: /repo conflict/i }),
    ).toBeInTheDocument();
    // curl has no conflict — no popover trigger
    const curlRow = screen.getByTestId("package-row-curl");
    expect(
      within(curlRow).queryByRole("button", { name: /repo conflict/i }),
    ).not.toBeInTheDocument();
  });

  it("aggregate: dismissing a conflict hides popover trigger and reports count", () => {
    const onDismissedCountChange = vi.fn();
    const pkgs = [
      makeAggregatePkg("httpd", "baseos", 3, 5, true, {
        repo_conflict: [
          { repo: "baseos", host_count: 2 },
          { repo: "appstream", host_count: 1 },
        ],
      }),
    ];
    render(
      <PackageList
        mode="aggregate"
        packages={pkgs}
        repoGroups={allRepos}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
        onDismissedCountChange={onDismissedCountChange}
      />,
    );
    // Click popover trigger to open
    const trigger = screen.getByRole("button", { name: /repo conflict/i });
    fireEvent.click(trigger);
    // Click dismiss button inside popover
    const dismissBtn = screen.getByText("Dismiss");
    fireEvent.click(dismissBtn);
    // Popover trigger should disappear (dismissed)
    expect(
      screen.queryByRole("button", { name: /repo conflict/i }),
    ).not.toBeInTheDocument();
    // Dismissed count reported as 1
    expect(onDismissedCountChange).toHaveBeenCalledWith(1);
  });

  it("aggregate: onRestoreDismissed clears dismissals and re-shows popover trigger", () => {
    const onDismissedCountChange = vi.fn();
    const pkgs = [
      makeAggregatePkg("httpd", "baseos", 3, 5, true, {
        repo_conflict: [
          { repo: "baseos", host_count: 2 },
          { repo: "appstream", host_count: 1 },
        ],
      }),
    ];
    const { rerender } = render(
      <PackageList
        mode="aggregate"
        packages={pkgs}
        repoGroups={allRepos}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
        onDismissedCountChange={onDismissedCountChange}
        onRestoreDismissed={false}
      />,
    );
    // Dismiss the conflict
    fireEvent.click(screen.getByRole("button", { name: /repo conflict/i }));
    fireEvent.click(screen.getByText("Dismiss"));
    expect(
      screen.queryByRole("button", { name: /repo conflict/i }),
    ).not.toBeInTheDocument();

    // Restore dismissed via prop toggle
    rerender(
      <PackageList
        mode="aggregate"
        packages={pkgs}
        repoGroups={allRepos}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
        onDismissedCountChange={onDismissedCountChange}
        onRestoreDismissed={true}
      />,
    );
    // Popover trigger reappears
    expect(
      screen.getByRole("button", { name: /repo conflict/i }),
    ).toBeInTheDocument();
    // Count reset to 0
    expect(onDismissedCountChange).toHaveBeenCalledWith(0);
  });

  // --- Focus handoff after dismiss ---

  it("aggregate: focus moves to package checkbox after conflict dismiss", async () => {
    const pkgs = [
      makeAggregatePkg("httpd", "baseos", 3, 5, true, {
        repo_conflict: [
          { repo: "baseos", host_count: 2 },
          { repo: "appstream", host_count: 1 },
        ],
      }),
    ];
    render(
      <PackageList
        mode="aggregate"
        packages={pkgs}
        repoGroups={allRepos}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    // Open popover
    const trigger = screen.getByRole("button", { name: /repo conflict/i });
    fireEvent.click(trigger);
    // Click dismiss
    const dismissBtn = screen.getByText("Dismiss");
    fireEvent.click(dismissBtn);

    // Wait for rAF focus handoff
    await new Promise((r) => requestAnimationFrame(r));

    // Focus should land on the package checkbox
    const checkbox = screen.getByRole("checkbox", { name: "httpd" });
    expect(document.activeElement).toBe(checkbox);
  });

  // --- Package groups zone ---

  const coreGroup: GroupInfo = {
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

  const editorsGroup: GroupInfo = {
    name: "editors",
    member_count: 3,
    added_count: 0, locked_count: 0,
    optional_spillover_count: 2,
    render_state: "renderable",
    degradation_reason: null,
    members: [
      { name: "nano", locked: false, overlap_groups: [] , in_base_image: false},
      { name: "vim-minimal", locked: false, overlap_groups: [] , in_base_image: false},
      { name: "vi", locked: false, overlap_groups: [] , in_base_image: false},
    ],
  };

  const excludedGroup: GroupInfo = {
    name: "development",
    member_count: 5,
    added_count: 0, locked_count: 0,
    optional_spillover_count: 0,
    render_state: "excluded",
    degradation_reason: null,
    members: [],
  };

  const degradedGroup: GroupInfo = {
    name: "multimedia",
    member_count: 2,
    added_count: 0, locked_count: 0,
    optional_spillover_count: 0,
    render_state: "degraded",
    degradation_reason: "insufficient_members",
    members: [
      { name: "ffmpeg", locked: false, overlap_groups: [] , in_base_image: false},
      { name: "vlc", locked: false, overlap_groups: [] , in_base_image: false},
    ],
  };

  const ungroupedGroup: GroupInfo = {
    name: "leftovers",
    member_count: 1,
    added_count: 0, locked_count: 0,
    optional_spillover_count: 0,
    render_state: "ungrouped",
    degradation_reason: null,
    members: [{ name: "orphan-pkg", locked: false, overlap_groups: [] , in_base_image: false}],
  };

  it("renders groups zone above individual packages zone", () => {
    const pkgs = [makePkg("curl", "baseos"), makePkg("wget", "baseos")];
    render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        packageGroups={[coreGroup, editorsGroup]}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    const list = screen.getByTestId("package-list");
    const groupsZone = within(list).getByTestId("groups-zone");
    const individualZone = within(list).getByTestId("individual-packages-zone");
    const divider = within(list).getByTestId("zone-divider");

    // Groups zone exists and contains group rows
    expect(within(groupsZone).getByTestId("group-row-core")).toBeInTheDocument();
    expect(within(groupsZone).getByTestId("group-row-editors")).toBeInTheDocument();

    // Divider is present
    expect(divider).toHaveTextContent("Individual Packages");

    // Individual packages zone contains package rows
    expect(within(individualZone).getByTestId("package-row-curl")).toBeInTheDocument();
    expect(within(individualZone).getByTestId("package-row-wget")).toBeInTheDocument();

    // DOM order: groups-zone before zone-divider before individual-packages-zone
    const allNodes = Array.from(list.children);
    const groupsIdx = allNodes.indexOf(groupsZone);
    const dividerIdx = allNodes.indexOf(divider);
    const individualIdx = allNodes.indexOf(individualZone);
    expect(groupsIdx).toBeLessThan(dividerIdx);
    expect(dividerIdx).toBeLessThan(individualIdx);
  });

  it("summary bar shows correct counts", () => {
    const pkgs = [
      makePkg("curl", "baseos"),
      makePkg("wget", "baseos"),
      makePkg("jq", "baseos"),
    ];
    render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        packageGroups={[coreGroup, editorsGroup]}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    const summary = screen.getByTestId("package-list-summary");
    // 2 groups, 11 total group packages, 3 other packages, 2 optional from groups
    expect(summary).toHaveTextContent("2 groups");
    expect(summary).toHaveTextContent("11 packages");
    expect(summary).toHaveTextContent("3 other packages");
    expect(summary).toHaveTextContent("2 optional from groups");
  });

  it("summary bar shows singular forms correctly", () => {
    const singleGroup: GroupInfo = {
      ...coreGroup,
      member_count: 1,
      members: [{ name: "bash", locked: false, overlap_groups: [] , in_base_image: false}],
    };
    const pkgs = [makePkg("curl", "baseos")];
    render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        packageGroups={[singleGroup]}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    const summary = screen.getByTestId("package-list-summary");
    expect(summary).toHaveTextContent("1 group");
    expect(summary).toHaveTextContent("1 package");
    expect(summary).toHaveTextContent("1 other package");
  });

  it("summary bar hides optional count when zero", () => {
    const noSpillover: GroupInfo = {
      ...coreGroup,
      optional_spillover_count: 0,
    };
    const pkgs = [makePkg("curl", "baseos")];
    render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        packageGroups={[noSpillover]}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    const summary = screen.getByTestId("package-list-summary");
    expect(summary).not.toHaveTextContent("optional from groups");
  });

  it("shows excluded groups in groups zone (not filtered out)", () => {
    const pkgs = [makePkg("curl", "baseos")];
    render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        packageGroups={[coreGroup, excludedGroup]}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    const groupsZone = screen.getByTestId("groups-zone");
    expect(within(groupsZone).getByTestId("group-row-core")).toBeInTheDocument();
    expect(within(groupsZone).getByTestId("group-row-development")).toBeInTheDocument();
  });

  it("shows groups zone when only excluded groups exist", () => {
    const pkgs = [makePkg("curl", "baseos")];
    render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        packageGroups={[excludedGroup]}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    expect(screen.getByTestId("groups-zone")).toBeInTheDocument();
    expect(screen.getByTestId("zone-divider")).toBeInTheDocument();
    expect(screen.getByTestId("package-list-summary")).toBeInTheDocument();
  });

  it("hides groups zone when packageGroups is undefined", () => {
    const pkgs = [makePkg("curl", "baseos")];
    render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    expect(screen.queryByTestId("groups-zone")).not.toBeInTheDocument();
    expect(screen.queryByTestId("zone-divider")).not.toBeInTheDocument();
    // Summary is now always shown (even without groups)
    const summary = screen.getByTestId("package-list-summary");
    expect(summary).toHaveTextContent("1 package");
  });

  // Group-level toggle switch was removed in favor of ungroup action.
  // Groups are managed via the ungroup button or per-member toggles.

  it("GroupRow onUngroup is wired correctly", () => {
    const onGroupUngroup = vi.fn();
    const pkgs = [makePkg("curl", "baseos")];
    render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        packageGroups={[coreGroup]}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
        onGroupUngroup={onGroupUngroup}
      />,
    );
    const ungroupBtn = screen.getByRole("button", { name: /ungroup core/i });
    fireEvent.click(ungroupBtn);
    expect(onGroupUngroup).toHaveBeenCalledWith("core");
  });

  // --- Provenance badges ---

  it("shows provenance badge for optional_spillover packages", () => {
    const pkgs = [
      makePkg("curl", "baseos"),
      makePkg("wget", "baseos"),
      makePkg("optional-pkg", "baseos"),
    ];
    const packageProvenances = {
      "optional-pkg.x86_64": {
        kind: "optional_spillover" as const,
        group_name: "web-tools",
      },
    };
    render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        packageProvenances={packageProvenances}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );

    // curl and wget should not have badges
    const curlRow = screen.getByTestId("package-row-curl");
    expect(within(curlRow).queryByTestId("provenance-badge")).toBeNull();

    const wgetRow = screen.getByTestId("package-row-wget");
    expect(within(wgetRow).queryByTestId("provenance-badge")).toBeNull();

    // optional-pkg should have the badge
    const optionalRow = screen.getByTestId("package-row-optional-pkg");
    const badge = within(optionalRow).getByTestId("provenance-badge");
    expect(badge).toHaveTextContent('optional from "web-tools"');
  });

  it("shows provenance badge for ungrouped_member packages", () => {
    const pkgs = [makePkg("ungrouped-pkg", "baseos")];
    const packageProvenances = {
      "ungrouped-pkg.x86_64": {
        kind: "ungrouped_member" as const,
        group_name: "dev-libs",
      },
    };
    render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        packageProvenances={packageProvenances}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );

    const badge = screen.getByTestId("provenance-badge");
    expect(badge).toHaveTextContent('ungrouped from "dev-libs"');
  });

  it("shows provenance badge for degraded_member packages", () => {
    const pkgs = [makePkg("degraded-pkg", "baseos")];
    const packageProvenances = {
      "degraded-pkg.x86_64": {
        kind: "degraded_member" as const,
        group_name: "core-utils",
      },
    };
    render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        packageProvenances={packageProvenances}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );

    const badge = screen.getByTestId("provenance-badge");
    expect(badge).toHaveTextContent(
      'from "core-utils" (rendered individually)',
    );
  });

  // --- Search auto-expand groups ---

  it("searching for member package auto-expands the group", () => {
    const pkgs = [makePkg("curl", "baseos")];
    render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        packageGroups={[coreGroup, editorsGroup]}
        searchQuery="bash"
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    // "bash" is a member of coreGroup — group should be auto-expanded
    const coreRow = screen.getByTestId("group-row-core");
    // Member should be visible (group expanded)
    expect(within(coreRow).getByTestId("group-member-bash")).toBeInTheDocument();
    // The member should have data-search-match
    expect(
      within(coreRow).getByTestId("group-member-bash"),
    ).toHaveAttribute("data-search-match", "true");
    // editors group has no match for "bash" — it's filtered out entirely
    expect(screen.queryByTestId("group-row-editors")).not.toBeInTheDocument();
  });

  it("searching for group name highlights the group row", () => {
    const pkgs = [makePkg("curl", "baseos")];
    render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        packageGroups={[coreGroup, editorsGroup]}
        searchQuery="core"
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    const coreRow = screen.getByTestId("group-row-core");
    expect(coreRow).toHaveAttribute("data-search-match", "true");
    // editors group has no match for "core" — it's filtered out
    expect(screen.queryByTestId("group-row-editors")).not.toBeInTheDocument();
  });

  it("clearing search re-collapses auto-expanded groups", () => {
    const pkgs = [makePkg("curl", "baseos")];
    const { rerender } = render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        packageGroups={[coreGroup]}
        searchQuery="bash"
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    // Group is auto-expanded — member visible
    expect(screen.getByTestId("group-member-bash")).toBeInTheDocument();

    // Clear search
    rerender(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        packageGroups={[coreGroup]}
        searchQuery=""
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    // Group should re-collapse — member hidden
    expect(screen.queryByTestId("group-member-bash")).not.toBeInTheDocument();
  });

  it("manually expanded groups stay open after search clear", () => {
    const pkgs = [makePkg("curl", "baseos")];
    const { rerender } = render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        packageGroups={[coreGroup]}
        searchQuery=""
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    // Manually expand the group by clicking the chevron
    const expandBtn = screen.getByRole("button", { name: /expand/i });
    fireEvent.click(expandBtn);
    expect(screen.getByTestId("group-member-bash")).toBeInTheDocument();

    // Now search for something (auto-expand is irrelevant since user already expanded)
    rerender(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        packageGroups={[coreGroup]}
        searchQuery="xyz-no-match"
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );

    // Clear search — user-expanded group should remain open
    rerender(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        packageGroups={[coreGroup]}
        searchQuery=""
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    // Group stays open because user expanded it manually
    expect(screen.getByTestId("group-member-bash")).toBeInTheDocument();
  });

  it("search filters individual packages to only matching ones", () => {
    const pkgs = [
      makePkg("curl", "baseos"),
      makePkg("wget", "baseos"),
      makePkg("bash-completion", "baseos"),
    ];
    render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        searchQuery="curl"
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    // Only curl should appear in individual zone
    const individualZone = screen.getByTestId("individual-packages-zone");
    expect(within(individualZone).getByTestId("package-row-curl")).toBeInTheDocument();
    expect(within(individualZone).queryByTestId("package-row-wget")).not.toBeInTheDocument();
    expect(within(individualZone).queryByTestId("package-row-bash-completion")).not.toBeInTheDocument();
  });

  it("search hides non-matching groups from the groups zone", () => {
    const pkgs = [makePkg("curl", "baseos")];
    render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        packageGroups={[coreGroup, editorsGroup]}
        searchQuery="nano"
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    // "nano" is a member of editors — editors should be visible
    const groupsZone = screen.getByTestId("groups-zone");
    expect(within(groupsZone).getByTestId("group-row-editors")).toBeInTheDocument();
    // core has no match for "nano" — should be hidden
    expect(within(groupsZone).queryByTestId("group-row-core")).not.toBeInTheDocument();
  });

  // --- Renderable member suppression from individual zone ---

  it("renderable group members do not appear in the individual packages zone", () => {
    // bash and coreutils are members of the renderable coreGroup
    const pkgs = [
      makePkg("bash", "baseos"),
      makePkg("coreutils", "baseos"),
      makePkg("curl", "baseos"),
      makePkg("wget", "baseos"),
    ];
    render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        packageGroups={[coreGroup]}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    const individualZone = screen.getByTestId("individual-packages-zone");
    // bash and coreutils are suppressed — they belong to the renderable group
    expect(within(individualZone).queryByTestId("package-row-bash")).not.toBeInTheDocument();
    expect(within(individualZone).queryByTestId("package-row-coreutils")).not.toBeInTheDocument();
    // curl and wget are not group members — they appear individually
    expect(within(individualZone).getByTestId("package-row-curl")).toBeInTheDocument();
    expect(within(individualZone).getByTestId("package-row-wget")).toBeInTheDocument();
  });

  it("individual package count in summary reflects member suppression", () => {
    // 4 packages total, 2 are renderable group members → 2 other
    const pkgs = [
      makePkg("bash", "baseos"),
      makePkg("coreutils", "baseos"),
      makePkg("curl", "baseos"),
      makePkg("wget", "baseos"),
    ];
    render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        packageGroups={[coreGroup]}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    const summary = screen.getByTestId("package-list-summary");
    expect(summary).toHaveTextContent("2 other packages");
  });

  // --- Canonical name.arch suppression (real MainContent data shape) ---

  it("suppression works with canonical name.arch package identifiers", () => {
    // Real MainContent produces "name.arch" via toPackageListPackages.
    // GroupMemberInfo carries bare names.  Suppression must bridge the gap.
    const pkgs = [
      makePkg("bash.x86_64", "baseos"),
      makePkg("coreutils.x86_64", "baseos"),
      makePkg("curl.x86_64", "baseos"),
      makePkg("wget.noarch", "baseos"),
    ];
    render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        packageGroups={[coreGroup]}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    const individualZone = screen.getByTestId("individual-packages-zone");
    // bash and coreutils are group members — suppressed even with .arch suffix
    expect(
      within(individualZone).queryByTestId("package-row-bash.x86_64"),
    ).not.toBeInTheDocument();
    expect(
      within(individualZone).queryByTestId("package-row-coreutils.x86_64"),
    ).not.toBeInTheDocument();
    // curl and wget are not group members — present
    expect(
      within(individualZone).getByTestId("package-row-curl.x86_64"),
    ).toBeInTheDocument();
    expect(
      within(individualZone).getByTestId("package-row-wget.noarch"),
    ).toBeInTheDocument();
  });

  it("suppression handles all common RPM architectures", () => {
    const archGroup: GroupInfo = {
      name: "multi-arch",
      member_count: 3,
      added_count: 0, locked_count: 0,
      optional_spillover_count: 0,
      render_state: "renderable",
      degradation_reason: null,
      members: [
        { name: "libfoo", locked: false, overlap_groups: [] , in_base_image: false},
        { name: "libbar", locked: false, overlap_groups: [] , in_base_image: false},
        { name: "libbaz", locked: false, overlap_groups: [] , in_base_image: false},
      ],
    };
    const pkgs = [
      makePkg("libfoo.aarch64", "baseos"),
      makePkg("libbar.s390x", "baseos"),
      makePkg("libbaz.ppc64le", "baseos"),
      makePkg("libqux.x86_64", "baseos"),
    ];
    render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        packageGroups={[archGroup]}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    const individualZone = screen.getByTestId("individual-packages-zone");
    expect(
      within(individualZone).queryByTestId("package-row-libfoo.aarch64"),
    ).not.toBeInTheDocument();
    expect(
      within(individualZone).queryByTestId("package-row-libbar.s390x"),
    ).not.toBeInTheDocument();
    expect(
      within(individualZone).queryByTestId("package-row-libbaz.ppc64le"),
    ).not.toBeInTheDocument();
    // libqux is not a member — present
    expect(
      within(individualZone).getByTestId("package-row-libqux.x86_64"),
    ).toBeInTheDocument();
  });

  it("packages with dots in the name that are not arch suffixes are not falsely suppressed", () => {
    // A package named "python3.11" should NOT be suppressed unless a group
    // member is literally "python3.11" or the full name is "python3.11.x86_64"
    // and "python3" is a member.
    const pyGroup: GroupInfo = {
      name: "python-core",
      member_count: 1,
      added_count: 0, locked_count: 0,
      optional_spillover_count: 0,
      render_state: "renderable",
      degradation_reason: null,
      members: [{ name: "python3", locked: false, overlap_groups: [] , in_base_image: false}],
    };
    const pkgs = [
      makePkg("python3.11", "baseos"),  // dot-separated version, not an arch
      makePkg("python3.x86_64", "baseos"),  // actual arch suffix
    ];
    render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        packageGroups={[pyGroup]}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    const individualZone = screen.getByTestId("individual-packages-zone");
    // "python3.11" has "11" as suffix — not an arch, so not stripped → not suppressed
    expect(
      within(individualZone).getByTestId("package-row-python3.11"),
    ).toBeInTheDocument();
    // "python3.x86_64" strips to "python3" which IS a member → suppressed
    expect(
      within(individualZone).queryByTestId("package-row-python3.x86_64"),
    ).not.toBeInTheDocument();
  });

  // --- Excluded groups visible with toggle off ---

  it("excluded groups render alongside renderable groups", () => {
    const pkgs = [makePkg("curl", "baseos")];
    render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        packageGroups={[coreGroup, excludedGroup]}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    const groupsZone = screen.getByTestId("groups-zone");
    // Both renderable and excluded groups appear
    expect(within(groupsZone).getByTestId("group-row-core")).toBeInTheDocument();
    expect(within(groupsZone).getByTestId("group-row-development")).toBeInTheDocument();
  });

  // --- Degraded groups visible but dimmed ---

  it("degraded groups render with degraded styling", () => {
    const pkgs = [
      makePkg("curl", "baseos"),
      makePkg("ffmpeg", "baseos"),
      makePkg("vlc", "baseos"),
    ];
    render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        packageGroups={[coreGroup, degradedGroup]}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    const groupsZone = screen.getByTestId("groups-zone");
    const mmRow = within(groupsZone).getByTestId("group-row-multimedia");
    expect(mmRow).toBeInTheDocument();
    // Degraded group has the degraded CSS class
    expect(mmRow).toHaveClass("inspectah-group-row--degraded");
    // Degraded group shows "rendered individually" subtitle
    expect(within(mmRow).getByText("rendered individually")).toBeInTheDocument();
  });

  // --- Ungrouped groups NOT in groups zone ---

  it("ungrouped groups do not appear in the groups zone", () => {
    const pkgs = [makePkg("curl", "baseos"), makePkg("orphan-pkg", "baseos")];
    render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        packageGroups={[coreGroup, ungroupedGroup]}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    const groupsZone = screen.getByTestId("groups-zone");
    expect(within(groupsZone).getByTestId("group-row-core")).toBeInTheDocument();
    expect(within(groupsZone).queryByTestId("group-row-leftovers")).not.toBeInTheDocument();
  });

  // --- Summary counts with mixed group states ---

  it("summary counts only renderable group packages, includes all visible groups", () => {
    const pkgs = [makePkg("curl", "baseos")];
    render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        packageGroups={[coreGroup, excludedGroup, degradedGroup]}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    const summary = screen.getByTestId("package-list-summary");
    // 3 visible groups (core=renderable, development=excluded, multimedia=degraded)
    expect(summary).toHaveTextContent("3 groups");
    // Only renderable group (core=8) + degraded (2) = 10 in parenthetical, plus 1 other package
    expect(summary).toHaveTextContent("10 packages");
    expect(summary).toHaveTextContent("1 other package");
  });

  // --- Task 3: Summary label changes for group dependency visibility ---

  it("shows 'other packages' instead of 'individual packages' when groups exist", () => {
    const groupWithMembers: GroupInfo = {
      name: "core",
      member_count: 3,
      added_count: 3,
      locked_count: 0,
      optional_spillover_count: 0,
      render_state: "renderable",
      degradation_reason: null,
      members: [
        { name: "bash", locked: false, overlap_groups: [], in_base_image: false },
        { name: "grep", locked: false, overlap_groups: [], in_base_image: false },
        { name: "sed", locked: false, overlap_groups: [], in_base_image: false },
      ],
    };
    const pkgs = [
      makePkg("curl", "baseos"),
      makePkg("wget", "baseos"),
    ];
    render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        packageGroups={[groupWithMembers]}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    const summary = screen.getByTestId("package-list-summary");
    expect(summary).toHaveTextContent("other packages");
    expect(summary).not.toHaveTextContent("individual packages");
  });

  it("shows 'N packages' with no qualifier when no groups exist", () => {
    const pkgs = [
      makePkg("bash", "baseos"),
      makePkg("curl", "baseos"),
      makePkg("wget", "baseos"),
    ];
    render(
      <PackageList
        mode="single"
        packages={pkgs}
        repoGroups={allRepos}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    const summary = screen.getByTestId("package-list-summary");
    expect(summary).toHaveTextContent("3 packages");
    expect(summary).not.toHaveTextContent("other");
    expect(summary).not.toHaveTextContent("individual");
  });

  it("shows 'N new, M from base' in group parenthetical", () => {
    const mixedGroup: GroupInfo = {
      name: "core",
      member_count: 5,
      added_count: 3,
      locked_count: 0,
      optional_spillover_count: 0,
      render_state: "renderable",
      degradation_reason: null,
      members: [
        { name: "bash", locked: false, overlap_groups: [], in_base_image: true },
        { name: "grep", locked: false, overlap_groups: [], in_base_image: false },
        { name: "sed", locked: false, overlap_groups: [], in_base_image: false },
        { name: "systemd", locked: false, overlap_groups: [], in_base_image: true },
        { name: "util-linux", locked: false, overlap_groups: [], in_base_image: false },
      ],
    };
    render(
      <PackageList
        mode="single"
        packages={[]}
        repoGroups={allRepos}
        packageGroups={[mixedGroup]}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    const summary = screen.getByTestId("package-list-summary");
    expect(summary).toHaveTextContent("1 group (3 new, 2 from base)");
  });

  it("deduplicates overlapping group members in summary counts", () => {
    const group1: GroupInfo = {
      name: "core",
      member_count: 3,
      added_count: 3,
      locked_count: 0,
      optional_spillover_count: 0,
      render_state: "renderable",
      degradation_reason: null,
      members: [
        { name: "bash", locked: false, overlap_groups: ["editors"], in_base_image: false },
        { name: "grep", locked: false, overlap_groups: [], in_base_image: false },
        { name: "sed", locked: false, overlap_groups: [], in_base_image: false },
      ],
    };
    const group2: GroupInfo = {
      name: "editors",
      member_count: 2,
      added_count: 2,
      locked_count: 0,
      optional_spillover_count: 0,
      render_state: "renderable",
      degradation_reason: null,
      members: [
        { name: "bash", locked: false, overlap_groups: ["core"], in_base_image: false },
        { name: "vim", locked: false, overlap_groups: [], in_base_image: false },
      ],
    };
    render(
      <PackageList
        mode="single"
        packages={[]}
        repoGroups={allRepos}
        packageGroups={[group1, group2]}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    const summary = screen.getByTestId("package-list-summary");
    // 4 unique packages total (bash, grep, sed, vim), not 5
    expect(summary).toHaveTextContent("2 groups (4 packages)");
  });

  it("shows 'all from base' when all group members are from base image", () => {
    const baseOnlyGroup: GroupInfo = {
      name: "base-tools",
      member_count: 3,
      added_count: 0,
      locked_count: 0,
      optional_spillover_count: 0,
      render_state: "renderable",
      degradation_reason: null,
      members: [
        { name: "bash", locked: false, overlap_groups: [], in_base_image: true },
        { name: "systemd", locked: false, overlap_groups: [], in_base_image: true },
        { name: "util-linux", locked: false, overlap_groups: [], in_base_image: true },
      ],
    };
    render(
      <PackageList
        mode="single"
        packages={[]}
        repoGroups={allRepos}
        packageGroups={[baseOnlyGroup]}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    const summary = screen.getByTestId("package-list-summary");
    expect(summary).toHaveTextContent("1 group (all from base)");
  });

  it("shows package count with no 'from base' suffix when no base packages", () => {
    const newOnlyGroup: GroupInfo = {
      name: "new-tools",
      member_count: 2,
      added_count: 2,
      locked_count: 0,
      optional_spillover_count: 0,
      render_state: "renderable",
      degradation_reason: null,
      members: [
        { name: "kubectl", locked: false, overlap_groups: [], in_base_image: false },
        { name: "helm", locked: false, overlap_groups: [], in_base_image: false },
      ],
    };
    render(
      <PackageList
        mode="single"
        packages={[]}
        repoGroups={allRepos}
        packageGroups={[newOnlyGroup]}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    const summary = screen.getByTestId("package-list-summary");
    expect(summary).toHaveTextContent("1 group (2 packages)");
    expect(summary).not.toHaveTextContent("from base");
  });
});
