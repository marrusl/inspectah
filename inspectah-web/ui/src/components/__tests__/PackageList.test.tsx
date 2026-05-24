import { render, screen, fireEvent, within } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { PackageList } from "../PackageList";
import type { RepoGroupInfo } from "../../api/types";

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

function makeFleetPkg(
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
    const pkgs = [
      makePkg("bash", "baseos"),
      makePkg("nginx", "epel"),
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

  // --- Fleet layout ---

  it("fleet: renders repo inline with name, prevalence in right column", () => {
    const pkgs = [
      makeFleetPkg("httpd", "appstream", 3, 5),
    ];
    const repos = [
      { ...distroRepo, section_id: "appstream" },
    ];
    render(
      <PackageList
        mode="fleet"
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
      makePkg("nginx", "epel"),       // third_party
      makePkg("bash", "baseos"),      // distro
      makePkg("devel", "crb"),        // official_optional
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

  // --- Sort: prevalence ascending (fleet default) ---

  it("sorts by prevalence ascending — rarest first (fleet default)", () => {
    const pkgs = [
      makeFleetPkg("httpd", "baseos", 5, 5),
      makeFleetPkg("nginx", "epel", 1, 5),
      makeFleetPkg("curl", "baseos", 3, 5),
    ];
    render(
      <PackageList
        mode="fleet"
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
    expect(repoText).toHaveStyle({ color: "var(--pf-t--global--text--color--subtle)" });
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
    expect(screen.getByRole("columnheader", { name: /packages/i })).toBeInTheDocument();
    expect(screen.getByRole("columnheader", { name: /repo/i })).toBeInTheDocument();
  });

  it("fleet: SortHeader shows Packages / Prevalence", () => {
    render(
      <PackageList
        mode="fleet"
        packages={[]}
        repoGroups={allRepos}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    expect(screen.getByRole("columnheader", { name: /packages/i })).toBeInTheDocument();
    expect(screen.getByRole("columnheader", { name: /prevalence/i })).toBeInTheDocument();
  });

  // --- Dismissed-state wiring ---

  it("reports dismissed count to parent via onDismissedCountChange", () => {
    const onDismissedCountChange = vi.fn();
    const pkgs = [
      makeFleetPkg("httpd", "baseos", 3, 5, true, {
        repo_conflict: [
          { repo: "baseos", host_count: 2 },
          { repo: "appstream", host_count: 1 },
        ],
      }),
    ];
    render(
      <PackageList
        mode="fleet"
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
      makeFleetPkg("httpd", "baseos", 3, 5, true, {
        repo_conflict: [
          { repo: "baseos", host_count: 2 },
          { repo: "appstream", host_count: 1 },
        ],
      }),
    ];
    const { rerender } = render(
      <PackageList
        mode="fleet"
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
        mode="fleet"
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

  // --- Fleet conflict-first sorting ---

  it("fleet: packages with undismissed repo_conflict sort before others in same prevalence group", () => {
    const pkgs = [
      makeFleetPkg("aaa-clean", "baseos", 3, 5),
      makeFleetPkg("bbb-conflict", "baseos", 3, 5, true, {
        repo_conflict: [
          { repo: "baseos", host_count: 2 },
          { repo: "epel", host_count: 1 },
        ],
      }),
    ];
    render(
      <PackageList
        mode="fleet"
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
    const pkgs = [
      makePkg("zsh", "baseos"),
      makePkg("bash", "baseos"),
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
    // Default: left column, asc → bash first
    let rows = screen.getAllByTestId(/^package-row-/);
    expect(rows[0]).toHaveAttribute("data-testid", "package-row-bash");

    // Click left column again → toggle to desc → zsh first
    const pkgHeader = screen.getByRole("columnheader", { name: /packages/i });
    fireEvent.click(pkgHeader);
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
    const pkgs = [
      makePkg("bash", "baseos"),
      makePkg("nginx", "epel"),
    ];
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

  // --- Fleet conflict popover in rows ---

  it("fleet: renders RepoConflictPopover trigger for packages with repo_conflict", () => {
    const pkgs = [
      makeFleetPkg("httpd", "baseos", 3, 5, true, {
        repo_conflict: [
          { repo: "baseos", host_count: 2 },
          { repo: "appstream", host_count: 1 },
        ],
      }),
      makeFleetPkg("curl", "baseos", 5, 5),
    ];
    render(
      <PackageList
        mode="fleet"
        packages={pkgs}
        repoGroups={allRepos}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
      />,
    );
    // httpd has conflict — popover trigger should be present
    const httpdRow = screen.getByTestId("package-row-httpd");
    expect(within(httpdRow).getByRole("button", { name: /repo conflict/i })).toBeInTheDocument();
    // curl has no conflict — no popover trigger
    const curlRow = screen.getByTestId("package-row-curl");
    expect(within(curlRow).queryByRole("button", { name: /repo conflict/i })).not.toBeInTheDocument();
  });

  it("fleet: dismissing a conflict hides popover trigger and reports count", () => {
    const onDismissedCountChange = vi.fn();
    const pkgs = [
      makeFleetPkg("httpd", "baseos", 3, 5, true, {
        repo_conflict: [
          { repo: "baseos", host_count: 2 },
          { repo: "appstream", host_count: 1 },
        ],
      }),
    ];
    render(
      <PackageList
        mode="fleet"
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
    expect(screen.queryByRole("button", { name: /repo conflict/i })).not.toBeInTheDocument();
    // Dismissed count reported as 1
    expect(onDismissedCountChange).toHaveBeenCalledWith(1);
  });

  it("fleet: onRestoreDismissed clears dismissals and re-shows popover trigger", () => {
    const onDismissedCountChange = vi.fn();
    const pkgs = [
      makeFleetPkg("httpd", "baseos", 3, 5, true, {
        repo_conflict: [
          { repo: "baseos", host_count: 2 },
          { repo: "appstream", host_count: 1 },
        ],
      }),
    ];
    const { rerender } = render(
      <PackageList
        mode="fleet"
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
    expect(screen.queryByRole("button", { name: /repo conflict/i })).not.toBeInTheDocument();

    // Restore dismissed via prop toggle
    rerender(
      <PackageList
        mode="fleet"
        packages={pkgs}
        repoGroups={allRepos}
        onToggle={vi.fn()}
        onRepoToggle={vi.fn()}
        onDismissedCountChange={onDismissedCountChange}
        onRestoreDismissed={true}
      />,
    );
    // Popover trigger reappears
    expect(screen.getByRole("button", { name: /repo conflict/i })).toBeInTheDocument();
    // Count reset to 0
    expect(onDismissedCountChange).toHaveBeenCalledWith(0);
  });
});
