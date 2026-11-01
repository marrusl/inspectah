import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { RepoBar } from "../RepoBar";
import type { RepoGroupInfo } from "../../api/types";

const mockRepos: RepoGroupInfo[] = [
  {
    section_id: "baseos",
    provenance: "verified",
    is_distro: true,
    tier: "distro",
    package_count: 12,
    effective_member_count: 12,
    effective_included: 12,
    effective_unlisted: 0,
    enabled: true,
  },
  {
    section_id: "appstream",
    provenance: "verified",
    is_distro: true,
    tier: "distro",
    package_count: 28,
    effective_member_count: 28,
    effective_included: 28,
    effective_unlisted: 0,
    enabled: true,
  },
  {
    section_id: "crb",
    provenance: "verified",
    is_distro: false,
    tier: "official_optional",
    package_count: 4,
    effective_member_count: 4,
    effective_included: 4,
    effective_unlisted: 0,
    enabled: true,
  },
  {
    section_id: "epel",
    provenance: "incomplete",
    is_distro: false,
    tier: "third_party",
    package_count: 8,
    effective_member_count: 8,
    effective_included: 8,
    effective_unlisted: 0,
    enabled: true,
  },
];

/** Factory for a toggleable baseline repo fixture, overridable per test. */
function repo(overrides: Partial<RepoGroupInfo>): RepoGroupInfo {
  return {
    section_id: "epel",
    provenance: "verified",
    is_distro: false,
    tier: "third_party",
    package_count: 2,
    effective_member_count: 2,
    effective_included: 1,
    effective_unlisted: 0,
    enabled: true,
    ...overrides,
  };
}

describe("RepoBar", () => {
  it("renders REPOSITORIES header", () => {
    render(<RepoBar repos={mockRepos} onToggle={vi.fn()} />);
    expect(screen.getByText("Repositories")).toBeInTheDocument();
  });

  it("renders distro repos with 'always included' label", () => {
    render(<RepoBar repos={mockRepos} onToggle={vi.fn()} />);
    expect(screen.getByText("baseos")).toBeInTheDocument();
    expect(screen.getByText("appstream")).toBeInTheDocument();
    const alwaysLabels = screen.getAllByText("always included");
    expect(alwaysLabels).toHaveLength(2);
  });

  it("renders toggleable repos with Switch controls", () => {
    render(<RepoBar repos={mockRepos} onToggle={vi.fn()} />);
    expect(screen.getByText("crb")).toBeInTheDocument();
    expect(screen.getByText("epel")).toBeInTheDocument();
    const switches = screen.getAllByRole("switch");
    expect(switches.length).toBeGreaterThanOrEqual(2);
  });

  it("distro repos have no toggle", () => {
    render(<RepoBar repos={mockRepos} onToggle={vi.fn()} />);
    const alwaysLabels = screen.getAllByText("always included");
    expect(alwaysLabels).toHaveLength(2);
  });

  // The two states where `enabled` and the effective counts disagree,
  // verbatim from Task 7's build_repo_groups assertions (§ 3.2, position 29).
  const stateA: Partial<RepoGroupInfo> = {
    section_id: "vendor",
    enabled: true,
    effective_member_count: 2,
    effective_included: 0,
    effective_unlisted: 0,
  };
  const stateB: Partial<RepoGroupInfo> = {
    section_id: "vendor",
    enabled: false,
    effective_member_count: 2,
    effective_included: 2,
    effective_unlisted: 0,
  };

  it("reads the counts, not the history, and posts the derived direction", () => {
    const onToggle = vi.fn();
    const { rerender } = render(
      <RepoBar repos={[repo(stateA)]} onToggle={onToggle} />,
    );
    expect(screen.getByRole("switch")).not.toBeChecked();
    expect(screen.getByText("2 packages excluded")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("switch"));
    expect(onToggle).toHaveBeenCalledWith("vendor", true);

    rerender(<RepoBar repos={[repo(stateB)]} onToggle={onToggle} />);
    expect(screen.getByRole("switch")).toBeChecked();
    expect(screen.getByText("2 of 2 packages included")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("switch"));
    expect(onToggle).toHaveBeenLastCalledWith("vendor", false);
  });

  it("renders the not-listed clause and never the visible count", () => {
    render(
      <RepoBar
        repos={[
          repo({
            ...stateA,
            effective_included: 1,
            effective_unlisted: 1,
            package_count: 1,
          }),
        ]}
        onToggle={vi.fn()}
      />,
    );
    expect(
      screen.getByText("1 of 2 packages included, 1 not listed"),
    ).toBeInTheDocument();
    expect(screen.getByRole("switch")).not.toBeChecked();
    // package_count (1) never drives the row on its own -- the only "1"
    // on screen is inside the composed count text asserted above.
    expect(screen.queryByText("1")).not.toBeInTheDocument();
  });

  it.each([stateA, stateB])(
    "ignores a contradicting enabled bit",
    (state) => {
      const onToggle = vi.fn();
      // PatternFly's Switch stamps a unique generated OUIA id per mount, so
      // markup is compared with that attribute stripped out.
      const stripOuiaId = (html: string) =>
        html.replace(/data-ouia-component-id="[^"]*"/, "");

      const { container, unmount } = render(
        <RepoBar repos={[repo(state)]} onToggle={onToggle} />,
      );
      const before = stripOuiaId(container.innerHTML);
      const switchBefore = screen.getByRole("switch") as HTMLInputElement;
      const wasChecked = switchBefore.checked;
      fireEvent.click(switchBefore);
      expect(onToggle).toHaveBeenCalledWith(state.section_id, !wasChecked);
      unmount();

      const { container: afterContainer } = render(
        <RepoBar
          repos={[repo({ ...state, enabled: !state.enabled })]}
          onToggle={onToggle}
        />,
      );
      // Same rendered markup regardless of the contradicting `enabled` bit.
      expect(stripOuiaId(afterContainer.innerHTML)).toBe(before);
      fireEvent.click(screen.getByRole("switch"));
      expect(onToggle).toHaveBeenLastCalledWith(
        state.section_id,
        !wasChecked,
      );
    },
  );

  it("gives two same-count repos distinguishable accessible names", () => {
    // `aria-label` overrides the visible repo name in accessible-name
    // computation, so the name next to the switch does not disambiguate
    // for a user navigating by control. Identical counts are the worst
    // case: without the repo id in the label, both switches announce the
    // same string. Queried by name, not by role, on purpose.
    render(
      <RepoBar
        repos={[
          repo({ section_id: "epel", effective_included: 2 }),
          repo({ section_id: "vendor", effective_included: 2 }),
        ]}
        onToggle={vi.fn()}
      />,
    );
    const epelSwitch = screen.getByRole("switch", { name: /epel/ });
    const vendorSwitch = screen.getByRole("switch", { name: /vendor/ });
    expect(epelSwitch).not.toBe(vendorSwitch);
    // The count text added this phase stays in the accessible name.
    for (const el of [epelSwitch, vendorSwitch]) {
      expect(el).toHaveAccessibleName(
        expect.stringContaining("2 of 2 packages included"),
      );
    }
  });

  it("shows conflict count badge with aria-live when provided", () => {
    render(
      <RepoBar
        repos={mockRepos}
        onToggle={vi.fn()}
        conflictCount={3}
        dismissedCount={0}
        onRestoreDismissed={vi.fn()}
      />,
    );
    expect(screen.getByText(/3 conflicts/i)).toBeInTheDocument();
  });

  it("shows 'Show N dismissed' restore button when dismissedCount > 0", () => {
    const onRestore = vi.fn();
    render(
      <RepoBar
        repos={mockRepos}
        onToggle={vi.fn()}
        conflictCount={3}
        dismissedCount={2}
        onRestoreDismissed={onRestore}
      />,
    );
    const restoreBtn = screen.getByRole("button", {
      name: /show 2 dismissed/i,
    });
    expect(restoreBtn).toBeInTheDocument();
    fireEvent.click(restoreBtn);
    expect(onRestore).toHaveBeenCalled();
  });

  it("hides restore button when dismissedCount is 0", () => {
    render(
      <RepoBar
        repos={mockRepos}
        onToggle={vi.fn()}
        conflictCount={3}
        dismissedCount={0}
        onRestoreDismissed={vi.fn()}
      />,
    );
    expect(
      screen.queryByRole("button", { name: /show.*dismissed/i }),
    ).not.toBeInTheDocument();
  });

  it("badge shows visible conflict count (total minus dismissed)", () => {
    render(
      <RepoBar
        repos={mockRepos}
        onToggle={vi.fn()}
        conflictCount={5}
        dismissedCount={2}
        onRestoreDismissed={vi.fn()}
      />,
    );
    expect(screen.getByText(/3 conflicts/i)).toBeInTheDocument();
    expect(screen.queryByText(/5 conflicts/i)).not.toBeInTheDocument();
  });

  it("badge hidden when all conflicts are dismissed", () => {
    render(
      <RepoBar
        repos={mockRepos}
        onToggle={vi.fn()}
        conflictCount={3}
        dismissedCount={3}
        onRestoreDismissed={vi.fn()}
      />,
    );
    expect(screen.queryByText(/conflicts/i)).not.toBeInTheDocument();
  });

  it("badge uses singular 'conflict' when visibleConflicts is 1", () => {
    render(
      <RepoBar
        repos={mockRepos}
        onToggle={vi.fn()}
        conflictCount={2}
        dismissedCount={1}
        onRestoreDismissed={vi.fn()}
      />,
    );
    expect(screen.getByText("1 conflict")).toBeInTheDocument();
  });

  it("announces when conflicts are dismissed", () => {
    const { rerender } = render(
      <RepoBar
        repos={mockRepos}
        onToggle={vi.fn()}
        conflictCount={3}
        dismissedCount={0}
        onRestoreDismissed={vi.fn()}
      />,
    );
    rerender(
      <RepoBar
        repos={mockRepos}
        onToggle={vi.fn()}
        conflictCount={3}
        dismissedCount={2}
        onRestoreDismissed={vi.fn()}
      />,
    );
    const announcement = screen.getByTestId("repo-bar-announcement");
    expect(announcement).toHaveTextContent("2 conflicts dismissed");
  });

  it("announces singular 'conflict dismissed' for one dismissal", () => {
    const { rerender } = render(
      <RepoBar
        repos={mockRepos}
        onToggle={vi.fn()}
        conflictCount={3}
        dismissedCount={0}
        onRestoreDismissed={vi.fn()}
      />,
    );
    rerender(
      <RepoBar
        repos={mockRepos}
        onToggle={vi.fn()}
        conflictCount={3}
        dismissedCount={1}
        onRestoreDismissed={vi.fn()}
      />,
    );
    const announcement = screen.getByTestId("repo-bar-announcement");
    expect(announcement).toHaveTextContent("1 conflict dismissed");
  });

  it("announces when all conflicts are restored", () => {
    const { rerender } = render(
      <RepoBar
        repos={mockRepos}
        onToggle={vi.fn()}
        conflictCount={3}
        dismissedCount={3}
        onRestoreDismissed={vi.fn()}
      />,
    );
    rerender(
      <RepoBar
        repos={mockRepos}
        onToggle={vi.fn()}
        conflictCount={3}
        dismissedCount={0}
        onRestoreDismissed={vi.fn()}
      />,
    );
    const announcement = screen.getByTestId("repo-bar-announcement");
    expect(announcement).toHaveTextContent("All conflicts restored");
  });

  it("live region has assertive aria-live attribute", () => {
    render(<RepoBar repos={mockRepos} onToggle={vi.fn()} />);
    const announcement = screen.getByTestId("repo-bar-announcement");
    expect(announcement).toHaveAttribute("aria-live", "assertive");
    expect(announcement).toHaveAttribute("aria-atomic", "true");
  });

  it.each(["disabled", "stale"] as const)(
    "renders no switch and the state text for %s",
    (provenance) => {
      render(
        <RepoBar
          repos={[
            repo({
              provenance,
              effective_member_count: 2,
              effective_included: 1,
            }),
          ]}
          onToggle={vi.fn()}
        />,
      );
      expect(screen.queryByRole("switch")).toBeNull();
      expect(
        screen.getByText(
          provenance === "disabled"
            ? "disabled on host"
            : "repo id not found in any .repo file",
        ),
      ).toBeInTheDocument();
    },
  );

  it("renders no switch for a verified repo with no effective member", () => {
    render(
      <RepoBar
        repos={[
          repo({
            provenance: "verified",
            effective_member_count: 0,
            effective_included: 0,
          }),
        ]}
        onToggle={vi.fn()}
      />,
    );
    expect(screen.queryByRole("switch")).toBeNull();
    expect(
      screen.getByText("nothing to decide: all members are locked"),
    ).toBeInTheDocument();
  });

  it("keeps the switch for verified and incomplete with members", () => {
    render(
      <RepoBar
        repos={[
          repo({
            provenance: "incomplete",
            effective_member_count: 2,
            effective_included: 2,
          }),
        ]}
        onToggle={vi.fn()}
      />,
    );
    expect(screen.getByRole("switch")).toBeInTheDocument();
    expect(
      screen.getByText("repo definition incomplete"),
    ).toBeInTheDocument();
  });

  it("renders no switch for unknown provenance, with the commandline title swap", () => {
    render(
      <RepoBar
        repos={[
          repo({
            section_id: "@commandline",
            provenance: "unknown",
            effective_member_count: 0,
            effective_included: 0,
          }),
        ]}
        onToggle={vi.fn()}
      />,
    );
    expect(screen.queryByRole("switch")).toBeNull();
    expect(screen.getByText("Local / Manual installs")).toBeInTheDocument();
    expect(
      screen.getByText("installed outside any repo"),
    ).toBeInTheDocument();
  });
});
