import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { StatsBar } from "../StatsBar";
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

describe("StatsBar", () => {
  it("renders package and config stats", () => {
    render(
      <StatsBar
        stats={MOCK_STATS}
        onUndo={vi.fn()}
        onRedo={vi.fn()}
        onExport={vi.fn()}
        isPending={false}
      />,
    );

    expect(screen.getByText(/8 included .* 2 excluded/)).toBeInTheDocument();
    expect(screen.getByText(/3 included .* 2 excluded/)).toBeInTheDocument();
  });

  it("renders triage progress based on NeedsReview count", () => {
    render(
      <StatsBar
        stats={MOCK_STATS}
        onUndo={vi.fn()}
        onRedo={vi.fn()}
        onExport={vi.fn()}
        isPending={false}
      />,
    );

    expect(screen.getByText(/3 items remaining/i)).toBeInTheDocument();
  });

  it("shows remaining NeedsReview items minus viewed count", () => {
    render(
      <StatsBar
        stats={MOCK_STATS}
        viewedNeedsReviewCount={1}
        onUndo={vi.fn()}
        onRedo={vi.fn()}
        onExport={vi.fn()}
        isPending={false}
      />,
    );

    expect(screen.getByText(/2 items remaining/i)).toBeInTheDocument();
  });

  it("shows completion message when all NeedsReview items triaged", () => {
    render(
      <StatsBar
        stats={MOCK_STATS}
        viewedNeedsReviewCount={3}
        onUndo={vi.fn()}
        onRedo={vi.fn()}
        onExport={vi.fn()}
        isPending={false}
      />,
    );

    expect(screen.getByText(/all actionable items reviewed/i)).toBeInTheDocument();
  });

  it("renders dashes when stats are null", () => {
    render(
      <StatsBar
        stats={null}
        onUndo={vi.fn()}
        onRedo={vi.fn()}
        onExport={vi.fn()}
        isPending={false}
      />,
    );

    const dashes = screen.getAllByText(/-/);
    expect(dashes.length).toBeGreaterThan(0);
  });

  it("disables undo when can_undo is false", () => {
    render(
      <StatsBar
        stats={{ ...MOCK_STATS, can_undo: false }}
        onUndo={vi.fn()}
        onRedo={vi.fn()}
        onExport={vi.fn()}
        isPending={false}
      />,
    );

    expect(screen.getByLabelText("Undo")).toBeDisabled();
  });

  it("disables redo when can_redo is false", () => {
    render(
      <StatsBar
        stats={MOCK_STATS}
        onUndo={vi.fn()}
        onRedo={vi.fn()}
        onExport={vi.fn()}
        isPending={false}
      />,
    );

    // MOCK_STATS has can_redo: false
    expect(screen.getByLabelText("Redo")).toBeDisabled();
  });

  it("disables undo/redo when isPending is true", () => {
    render(
      <StatsBar
        stats={{ ...MOCK_STATS, can_undo: true, can_redo: true }}
        onUndo={vi.fn()}
        onRedo={vi.fn()}
        onExport={vi.fn()}
        isPending={true}
      />,
    );

    expect(screen.getByLabelText("Undo")).toBeDisabled();
    expect(screen.getByLabelText("Redo")).toBeDisabled();
  });

  it("calls onUndo when undo button is clicked", async () => {
    const onUndo = vi.fn();
    render(
      <StatsBar
        stats={MOCK_STATS}
        onUndo={onUndo}
        onRedo={vi.fn()}
        onExport={vi.fn()}
        isPending={false}
      />,
    );

    await userEvent.click(screen.getByLabelText("Undo"));
    expect(onUndo).toHaveBeenCalled();
  });

  it("calls onExport when export button is clicked", async () => {
    const onExport = vi.fn();
    render(
      <StatsBar
        stats={MOCK_STATS}
        onUndo={vi.fn()}
        onRedo={vi.fn()}
        onExport={onExport}
        isPending={false}
      />,
    );

    await userEvent.click(screen.getByText("Export"));
    expect(onExport).toHaveBeenCalled();
  });

  it("shows remaining count when items need review", () => {
    const stats = { ...MOCK_STATS, needs_review_count: 12 };
    render(
      <StatsBar
        stats={stats}
        onUndo={vi.fn()}
        onRedo={vi.fn()}
        onExport={vi.fn()}
        isPending={false}
      />,
    );
    expect(screen.getByText(/12 items remaining/i)).toBeInTheDocument();
  });

  it("shows completion state when all items reviewed", () => {
    const stats = { ...MOCK_STATS, needs_review_count: 0 };
    render(
      <StatsBar
        stats={stats}
        onUndo={vi.fn()}
        onRedo={vi.fn()}
        onExport={vi.fn()}
        isPending={false}
      />,
    );
    expect(screen.getByText(/all actionable items reviewed/i)).toBeInTheDocument();
  });

  it("renders fleet summary when fleetSummary is provided", () => {
    render(
      <StatsBar
        stats={MOCK_STATS}
        onUndo={vi.fn()}
        onRedo={vi.fn()}
        onExport={vi.fn()}
        isPending={false}
        fleetSummary={{ hostCount: 5, hostnames: ["host-a", "host-b", "host-c", "host-d", "host-e"], totalItems: 2480, needsReviewCount: 27 }}
      />,
    );

    const summary = screen.getByTestId("fleet-stats-summary");
    expect(summary).toBeInTheDocument();
    expect(summary).toHaveTextContent("5");
    expect(summary).toHaveTextContent("hosts");
    expect(summary).toHaveTextContent("2,480");
    expect(summary).toHaveTextContent("items");
    expect(summary).toHaveTextContent("27 need review");

    // Single-host counters must NOT be present
    expect(screen.queryByText(/Packages:/)).not.toBeInTheDocument();
    expect(screen.queryByText(/Configs:/)).not.toBeInTheDocument();
  });

  it("shows all-reviewed label in fleet summary when needsReviewCount is 0", () => {
    render(
      <StatsBar
        stats={MOCK_STATS}
        onUndo={vi.fn()}
        onRedo={vi.fn()}
        onExport={vi.fn()}
        isPending={false}
        fleetSummary={{ hostCount: 3, hostnames: ["a", "b", "c"], totalItems: 100, needsReviewCount: 0 }}
      />,
    );

    expect(screen.getByTestId("fleet-stats-summary")).toHaveTextContent("All reviewed");
  });

  it("opens hostname popover when host count is clicked", async () => {
    render(
      <StatsBar
        stats={MOCK_STATS}
        onUndo={vi.fn()}
        onRedo={vi.fn()}
        onExport={vi.fn()}
        isPending={false}
        fleetSummary={{ hostCount: 3, hostnames: ["zulu-host", "alpha-host", "mid-host"], totalItems: 100, needsReviewCount: 0 }}
      />,
    );

    await userEvent.click(screen.getByTestId("fleet-host-trigger"));

    const list = screen.getByTestId("fleet-hostname-list");
    expect(list).toBeInTheDocument();

    // Hostnames should be sorted alphabetically
    const entries = list.querySelectorAll(".fleet-hostname-entry");
    expect(entries).toHaveLength(3);
    expect(entries[0]).toHaveTextContent("alpha-host");
    expect(entries[1]).toHaveTextContent("mid-host");
    expect(entries[2]).toHaveTextContent("zulu-host");

    // Copy button should be present
    expect(screen.getByTestId("fleet-hostname-copy")).toHaveTextContent("Copy all");
  });

  it("copies hostnames to clipboard when copy button is clicked", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });

    render(
      <StatsBar
        stats={MOCK_STATS}
        onUndo={vi.fn()}
        onRedo={vi.fn()}
        onExport={vi.fn()}
        isPending={false}
        fleetSummary={{ hostCount: 2, hostnames: ["beta", "alpha"], totalItems: 50, needsReviewCount: 0 }}
      />,
    );

    await userEvent.click(screen.getByTestId("fleet-host-trigger"));
    await userEvent.click(screen.getByTestId("fleet-hostname-copy"));

    expect(writeText).toHaveBeenCalledWith("alpha\nbeta");
  });
});
