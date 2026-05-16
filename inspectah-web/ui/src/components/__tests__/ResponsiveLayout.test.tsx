import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Sidebar } from "../Sidebar";
import type { RefineStats, HealthResponse, ContextSection } from "../../api/types";

const MOCK_STATS: RefineStats = {
  total_packages: 10,
  included_packages: 8,
  excluded_packages: 2,
  total_configs: 5,
  included_configs: 3,
  package_managed_configs: 2,
  excluded_configs: 2,
  needs_review_count: 3,
  ops_applied: 1,
  can_undo: true,
  can_redo: false,
  baseline_available: true,
};

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
};

const MOCK_SECTIONS: ContextSection[] = [
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
    expect(
      screen.getByLabelText("Section navigation"),
    ).toHaveAttribute("id", "inspectah-sidebar-overlay");
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
