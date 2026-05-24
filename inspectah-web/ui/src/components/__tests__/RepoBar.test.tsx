import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { RepoBar } from "../RepoBar";
import type { RepoGroupInfo } from "../../api/types";

const mockRepos: RepoGroupInfo[] = [
  { section_id: "baseos", provenance: "verified", is_distro: true, tier: "distro", package_count: 12, enabled: true },
  { section_id: "appstream", provenance: "verified", is_distro: true, tier: "distro", package_count: 28, enabled: true },
  { section_id: "crb", provenance: "verified", is_distro: false, tier: "official_optional", package_count: 4, enabled: true },
  { section_id: "epel", provenance: "incomplete", is_distro: false, tier: "third_party", package_count: 8, enabled: true },
];

describe("RepoBar", () => {
  it("renders distro repos with package counts in row 1", () => {
    render(<RepoBar repos={mockRepos} onToggle={vi.fn()} />);
    expect(screen.getByText(/baseos \(12\)/)).toBeInTheDocument();
    expect(screen.getByText(/appstream \(28\)/)).toBeInTheDocument();
  });

  it("renders toggleable repos as pills with package counts in row 2", () => {
    render(<RepoBar repos={mockRepos} onToggle={vi.fn()} />);
    const crbPill = screen.getByRole("switch", { name: /crb \(4\)/i });
    expect(crbPill).toBeInTheDocument();
    expect(crbPill.textContent).toContain("crb (4)");
    const epelPill = screen.getByRole("switch", { name: /epel \(8\)/i });
    expect(epelPill).toBeInTheDocument();
    expect(epelPill.textContent).toContain("epel (8)");
  });

  it("distro repos have no toggle", () => {
    render(<RepoBar repos={mockRepos} onToggle={vi.fn()} />);
    expect(screen.queryByRole("switch", { name: /baseos/i })).not.toBeInTheDocument();
  });

  it("calls onToggle with section_id when pill is clicked", () => {
    const onToggle = vi.fn();
    render(<RepoBar repos={mockRepos} onToggle={onToggle} />);
    fireEvent.click(screen.getByRole("switch", { name: /epel/i }));
    expect(onToggle).toHaveBeenCalledWith("epel");
  });

  it("shows conflict count badge with aria-live when provided", () => {
    render(<RepoBar repos={mockRepos} onToggle={vi.fn()} conflictCount={3} dismissedCount={0} onRestoreDismissed={vi.fn()} />);
    expect(screen.getByText(/3 conflicts/i)).toBeInTheDocument();
  });

  it("shows 'Show N dismissed' restore button when dismissedCount > 0", () => {
    const onRestore = vi.fn();
    render(<RepoBar repos={mockRepos} onToggle={vi.fn()} conflictCount={3} dismissedCount={2} onRestoreDismissed={onRestore} />);
    const restoreBtn = screen.getByRole("button", { name: /show 2 dismissed/i });
    expect(restoreBtn).toBeInTheDocument();
    fireEvent.click(restoreBtn);
    expect(onRestore).toHaveBeenCalled();
  });

  it("hides restore button when dismissedCount is 0", () => {
    render(<RepoBar repos={mockRepos} onToggle={vi.fn()} conflictCount={3} dismissedCount={0} onRestoreDismissed={vi.fn()} />);
    expect(screen.queryByRole("button", { name: /show.*dismissed/i })).not.toBeInTheDocument();
  });

  it("badge shows visible conflict count (total minus dismissed)", () => {
    render(<RepoBar repos={mockRepos} onToggle={vi.fn()} conflictCount={5} dismissedCount={2} onRestoreDismissed={vi.fn()} />);
    expect(screen.getByText(/3 conflicts/i)).toBeInTheDocument();
    expect(screen.queryByText(/5 conflicts/i)).not.toBeInTheDocument();
  });

  it("badge hidden when all conflicts are dismissed", () => {
    render(<RepoBar repos={mockRepos} onToggle={vi.fn()} conflictCount={3} dismissedCount={3} onRestoreDismissed={vi.fn()} />);
    expect(screen.queryByText(/conflicts/i)).not.toBeInTheDocument();
  });

  it("badge uses singular 'conflict' when visibleConflicts is 1", () => {
    render(<RepoBar repos={mockRepos} onToggle={vi.fn()} conflictCount={2} dismissedCount={1} onRestoreDismissed={vi.fn()} />);
    expect(screen.getByText("1 conflict")).toBeInTheDocument();
  });
});
