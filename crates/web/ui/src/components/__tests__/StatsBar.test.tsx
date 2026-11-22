import { describe, it, expect, vi } from "vitest";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
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

  it("renders aggregate summary when aggregateSummary is provided", () => {
    render(
      <StatsBar
        stats={MOCK_STATS}
        onUndo={vi.fn()}
        onRedo={vi.fn()}
        onExport={vi.fn()}
        isPending={false}
        aggregateSummary={{
          hostCount: 5,
          hostnames: ["host-a", "host-b", "host-c", "host-d", "host-e"],
          totalItems: 2480,
          needsReviewCount: 27,
        }}
      />,
    );

    const summary = screen.getByTestId("aggregate-stats-summary");
    expect(summary).toBeInTheDocument();
    expect(summary).toHaveTextContent("5");
    expect(summary).toHaveTextContent("hosts");
    expect(summary).toHaveTextContent("2,480");
    expect(summary).toHaveTextContent("items");

    // Single-host counters must NOT be present
    expect(screen.queryByText(/Packages:/)).not.toBeInTheDocument();
    expect(screen.queryByText(/Configs:/)).not.toBeInTheDocument();
  });

  it("opens hostname popover when host count is clicked", async () => {
    render(
      <StatsBar
        stats={MOCK_STATS}
        onUndo={vi.fn()}
        onRedo={vi.fn()}
        onExport={vi.fn()}
        isPending={false}
        aggregateSummary={{
          hostCount: 3,
          hostnames: ["zulu-host", "alpha-host", "mid-host"],
          totalItems: 100,
          needsReviewCount: 0,
        }}
      />,
    );

    await userEvent.click(screen.getByTestId("aggregate-host-trigger"));

    const list = screen.getByTestId("aggregate-hostname-list");
    expect(list).toBeInTheDocument();

    // Hostnames should be sorted alphabetically
    const entries = list.querySelectorAll(".aggregate-hostname-entry");
    expect(entries).toHaveLength(3);
    expect(entries[0]).toHaveTextContent("alpha-host");
    expect(entries[1]).toHaveTextContent("mid-host");
    expect(entries[2]).toHaveTextContent("zulu-host");

    // Copy button should be present
    expect(screen.getByTestId("aggregate-hostname-copy")).toHaveTextContent(
      "Copy all",
    );
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
        aggregateSummary={{
          hostCount: 2,
          hostnames: ["beta", "alpha"],
          totalItems: 50,
          needsReviewCount: 0,
        }}
      />,
    );

    await userEvent.click(screen.getByTestId("aggregate-host-trigger"));
    await userEvent.click(screen.getByTestId("aggregate-hostname-copy"));

    expect(writeText).toHaveBeenCalledWith("alpha\nbeta");
  });

  // Advisories are display-only findings the user was never offered a
  // decision on. Folding them into `excluded` made the bar report a
  // decision the user never made -- a host whose only config finding is a
  // modernization advisory read "0 included / 1 excluded" beside the very
  // row that says the tool, not the user, put it there.
  it("counts advisories as their own bucket, not as exclusions", () => {
    render(
      <StatsBar
        stats={mockStats({
          sections: [
            { kind: "package", total: 10, included: 8, excluded: 2, advisory: 0 },
            { kind: "config", total: 3, included: 1, excluded: 0, advisory: 2 },
          ],
        })}
        onUndo={vi.fn()}
        onRedo={vi.fn()}
        onExport={vi.fn()}
        isPending={false}
      />,
    );

    expect(
      screen.getByText(/1 included .* 0 excluded .* 2 advisory/),
    ).toBeInTheDocument();
  });

  it("omits the advisory bucket entirely when a section has none", () => {
    render(
      <StatsBar
        stats={mockStats({
          sections: [
            { kind: "package", total: 10, included: 8, excluded: 2, advisory: 0 },
            { kind: "config", total: 4, included: 3, excluded: 1, advisory: 0 },
          ],
        })}
        onUndo={vi.fn()}
        onRedo={vi.fn()}
        onExport={vi.fn()}
        isPending={false}
      />,
    );

    // The overwhelmingly common host has no advisories; a zero-valued
    // third bucket would be noise on every one of them.
    expect(screen.queryByText(/advisory/)).not.toBeInTheDocument();
  });
});

/** A callback whose promise the test settles by hand. */
function heldCallback() {
  let settle: () => void = () => {};
  const callback = vi.fn(
    () =>
      new Promise<void>((resolve) => {
        settle = resolve;
      }),
  );
  return { callback, settle: () => settle() };
}

/**
 * Reset goes out directly, not through App's useMutation. The toolbar
 * button stays disabled until the reset settles, and a repeat confirm is
 * dropped.
 */
describe("StatsBar reset pending guard", () => {
  function renderWithReset(onReset: () => Promise<void>) {
    render(
      <StatsBar
        stats={MOCK_STATS}
        onUndo={vi.fn()}
        onRedo={vi.fn()}
        onExport={vi.fn()}
        onReset={onReset}
        isPending={false}
      />,
    );
  }

  const resetButton = () => screen.getByTestId("reset-defaults-btn");
  const openConfirm = () => fireEvent.click(resetButton());

  it("sends one reset when confirmed twice", () => {
    const { callback } = heldCallback();
    renderWithReset(callback);
    openConfirm();
    const confirm = screen.getByTestId("reset-confirm-yes");

    act(() => {
      confirm.click();
      confirm.click();
    });

    expect(callback).toHaveBeenCalledTimes(1);
    expect(resetButton()).toBeDisabled();
  });

  it("re-enables reset once the request settles", async () => {
    const { callback, settle } = heldCallback();
    renderWithReset(callback);
    openConfirm();
    act(() => screen.getByTestId("reset-confirm-yes").click());
    expect(resetButton()).toBeDisabled();

    settle();
    await waitFor(() => expect(resetButton()).toBeEnabled());

    openConfirm();
    act(() => screen.getByTestId("reset-confirm-yes").click());
    expect(callback).toHaveBeenCalledTimes(2);
  });

  /**
   * Confirming disables the toolbar trigger for as long as the reset runs,
   * so the modal cannot hand focus back to it. Focus moves to Export
   * instead, an enabled control in the same toolbar.
   */
  it("moves focus to Export when reset is confirmed from the keyboard", async () => {
    const user = userEvent.setup();
    const { callback } = heldCallback();
    renderWithReset(callback);

    resetButton().focus();
    await user.keyboard("{Enter}");
    screen.getByTestId("reset-confirm-yes").focus();
    await user.keyboard("{Enter}");

    expect(callback).toHaveBeenCalledTimes(1);
    expect(resetButton()).toBeDisabled();
    expect(screen.getByRole("button", { name: "Export" })).toHaveFocus();
  });

  /**
   * Reset clears the whole timeline, so Undo cannot bring the work back,
   * and it clears user and password choices as well as include and
   * exclude ones. The confirm has to say both before the operator agrees.
   */
  it("says what reset clears and that it cannot be undone", () => {
    const { callback } = heldCallback();
    renderWithReset(callback);
    openConfirm();

    const modal = screen.getByTestId("reset-confirm-modal");
    expect(modal).toHaveTextContent(/include and exclude choices/);
    expect(modal).toHaveTextContent(/user account choices/);
    expect(modal).toHaveTextContent(/password/);
    expect(modal).toHaveTextContent(/cannot be undone/);
  });

  it("returns focus to the trigger when reset is cancelled", async () => {
    const user = userEvent.setup();
    const { callback } = heldCallback();
    renderWithReset(callback);

    resetButton().focus();
    await user.keyboard("{Enter}");
    screen.getByRole("button", { name: "Cancel" }).focus();
    await user.keyboard("{Enter}");

    expect(callback).not.toHaveBeenCalled();
    expect(screen.queryByTestId("reset-confirm-modal")).toBeNull();
    expect(resetButton()).toHaveFocus();
  });
});
