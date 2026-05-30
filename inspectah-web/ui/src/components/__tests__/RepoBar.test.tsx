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
    enabled: true,
  },
  {
    section_id: "appstream",
    provenance: "verified",
    is_distro: true,
    tier: "distro",
    package_count: 28,
    enabled: true,
  },
  {
    section_id: "crb",
    provenance: "verified",
    is_distro: false,
    tier: "official_optional",
    package_count: 4,
    enabled: true,
  },
  {
    section_id: "epel",
    provenance: "incomplete",
    is_distro: false,
    tier: "third_party",
    package_count: 8,
    enabled: true,
  },
];

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

  it("calls onToggle with section_id when toggle is clicked", () => {
    const onToggle = vi.fn();
    render(<RepoBar repos={mockRepos} onToggle={onToggle} />);
    const epelLabel = screen.getByLabelText(/epel \(8\)/i);
    fireEvent.click(epelLabel);
    expect(onToggle).toHaveBeenCalledWith("epel");
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
});
