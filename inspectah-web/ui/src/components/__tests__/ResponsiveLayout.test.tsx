import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Sidebar } from "../Sidebar";
import type { HealthResponse, ReferenceSection } from "../../api/types";
import { mockStats } from "../../test-utils/mockStats";

const MOCK_STATS = mockStats({
  sections: [
    { kind: "package", total: 10, included: 8, excluded: 2 },
    { kind: "config", total: 5, included: 3, excluded: 2 },
  ],
  needs_review_count: 3,
  ops_applied: 1,
  can_undo: true,
  can_redo: false,
  baseline_available: true,
});

const MOCK_HEALTH: HealthResponse = {
  status: "ok",
  host: {
    hostname: "testhost",
    os_name: "RHEL",
    os_version: "9.3",
    os_id: "rhel",
    system_type: "physical",
    schema_version: 1,
  },
  completeness: "full",
  policy: { distro_repos: ["baseos", "appstream"] },
  fleet: null,
  session_is_sensitive: false,
};

const MOCK_SECTIONS: ReferenceSection[] = [
  { id: "services", display_name: "Services", items: [] },
];

describe("Sidebar overlay mode", () => {
  it("renders with overlay class and backdrop when overlay=true", () => {
    render(
      <Sidebar
        activeSection="packages"
        onSelect={vi.fn()}
        stats={MOCK_STATS}
        sections={MOCK_SECTIONS}
        health={MOCK_HEALTH}
        overlay
        onClose={vi.fn()}
      />,
    );

    expect(screen.getByTestId("sidebar-backdrop")).toBeInTheDocument();
    expect(screen.getByLabelText("Section navigation")).toHaveAttribute(
      "id",
      "inspectah-sidebar-overlay",
    );
  });

  it("does not render backdrop when overlay=false", () => {
    render(
      <Sidebar
        activeSection="packages"
        onSelect={vi.fn()}
        stats={MOCK_STATS}
        sections={MOCK_SECTIONS}
        health={MOCK_HEALTH}
      />,
    );

    expect(screen.queryByTestId("sidebar-backdrop")).not.toBeInTheDocument();
  });

  it("calls onClose when Escape is pressed in overlay mode", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();

    render(
      <Sidebar
        activeSection="packages"
        onSelect={vi.fn()}
        stats={MOCK_STATS}
        sections={MOCK_SECTIONS}
        health={MOCK_HEALTH}
        overlay
        onClose={onClose}
      />,
    );

    await user.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("calls onClose when backdrop is clicked", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();

    render(
      <Sidebar
        activeSection="packages"
        onSelect={vi.fn()}
        stats={MOCK_STATS}
        sections={MOCK_SECTIONS}
        health={MOCK_HEALTH}
        overlay
        onClose={onClose}
      />,
    );

    await user.click(screen.getByTestId("sidebar-backdrop"));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("does not call onClose when sidebar content is clicked", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();

    render(
      <Sidebar
        activeSection="packages"
        onSelect={vi.fn()}
        stats={MOCK_STATS}
        sections={MOCK_SECTIONS}
        health={MOCK_HEALTH}
        overlay
        onClose={onClose}
      />,
    );

    // Click on the nav itself, not the backdrop
    await user.click(screen.getByLabelText("Section navigation"));
    expect(onClose).not.toHaveBeenCalled();
  });

  it("calls onSelect when a section is clicked in overlay mode", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();

    render(
      <Sidebar
        activeSection="packages"
        onSelect={onSelect}
        stats={MOCK_STATS}
        sections={MOCK_SECTIONS}
        health={MOCK_HEALTH}
        overlay
        onClose={vi.fn()}
      />,
    );

    await user.click(screen.getByText("Config Files"));
    expect(onSelect).toHaveBeenCalledWith("configs");
  });
});

describe("StatsBar hamburger button", () => {
  // We test the hamburger via the StatsBar component since that's where it renders
  it("renders hamburger with correct aria attributes", async () => {
    // Import StatsBar for this test
    const { StatsBar } = await import("../StatsBar");

    const hamburger = (
      <button
        type="button"
        className="inspectah-hamburger"
        aria-label="Open navigation"
        aria-expanded={false}
        aria-controls="inspectah-sidebar-overlay"
      >
        &#x2630;
      </button>
    );

    render(
      <StatsBar
        stats={MOCK_STATS}
        onUndo={vi.fn()}
        onRedo={vi.fn()}
        onExport={vi.fn()}
        isPending={false}
        hamburger={hamburger}
      />,
    );

    const btn = screen.getByRole("button", { name: "Open navigation" });
    expect(btn).toBeInTheDocument();
    expect(btn).toHaveAttribute("aria-expanded", "false");
    expect(btn).toHaveAttribute("aria-controls", "inspectah-sidebar-overlay");
  });

  it("does not render hamburger when not provided", async () => {
    const { StatsBar } = await import("../StatsBar");

    render(
      <StatsBar
        stats={MOCK_STATS}
        onUndo={vi.fn()}
        onRedo={vi.fn()}
        onExport={vi.fn()}
        isPending={false}
      />,
    );

    expect(
      screen.queryByRole("button", { name: "Open navigation" }),
    ).not.toBeInTheDocument();
  });
});

describe("Repo group header classification label", () => {
  it("renders 'Third-party' classification label for non-distro repos", async () => {
    const { RepoGroupHeader } = await import("../RepoGroupHeader");

    render(
      <RepoGroupHeader
        sectionId="epel"
        provenance="verified"
        isDistro={false}
        packageCount={5}
        enabled={true}
        onToggle={vi.fn()}
      />,
    );

    // Non-distro repos show a classification label
    const classification = document.querySelector(
      ".inspectah-repo-group-header__classification",
    );
    expect(classification).toBeInTheDocument();
    expect(classification!.textContent).toContain("Third-party");
  });

  it("does not render classification label for distro repos", async () => {
    const { RepoGroupHeader } = await import("../RepoGroupHeader");

    render(
      <RepoGroupHeader
        sectionId="baseos"
        provenance="verified"
        isDistro={true}
        packageCount={10}
        enabled={true}
      />,
    );

    // Distro repos have no classification label
    const classification = document.querySelector(
      ".inspectah-repo-group-header__classification",
    );
    expect(classification).not.toBeInTheDocument();
  });

  it("header has tabindex=0 and role=row for keyboard access", async () => {
    const { RepoGroupHeader } = await import("../RepoGroupHeader");

    render(
      <RepoGroupHeader
        sectionId="epel"
        provenance="verified"
        isDistro={false}
        packageCount={3}
        enabled={true}
      />,
    );

    const header = screen.getByTestId("repo-group-epel");
    expect(header).toHaveAttribute("tabindex", "0");
    expect(header).toHaveAttribute("role", "row");
  });

  it("wraps toggle to second line via CSS class structure", async () => {
    const { RepoGroupHeader } = await import("../RepoGroupHeader");

    render(
      <RepoGroupHeader
        sectionId="epel"
        provenance="verified"
        isDistro={false}
        packageCount={3}
        enabled={true}
        onToggle={vi.fn()}
      />,
    );

    // The toggle wrapper should exist with the correct CSS class
    const toggleWrapper = document.querySelector(
      ".inspectah-repo-group-header__toggle",
    );
    expect(toggleWrapper).toBeInTheDocument();

    // The header itself should have the class for CSS-based responsive wrapping
    const header = screen.getByTestId("repo-group-epel");
    expect(header).toHaveClass("inspectah-repo-group-header");
  });
});
