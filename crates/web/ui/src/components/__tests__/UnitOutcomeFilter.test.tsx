/**
 * The units outcome filter bar on its own (spec § 6.4, § 6.13 The filter,
 * Announcements; § 7 strings 7, 8 and 9).
 */
import { describe, it, expect, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { UnitOutcomeFilter } from "../UnitOutcomeFilter";
import type { FilterValue, OutcomeCounts } from "../unitOutcome";

const HINT = "Needs follow-up: the Containerfile writes a comment for the row and no command.";
const KITCHEN_SINK: OutcomeCounts = { all: 23, needs_follow_up: 9, carried: 14, excluded: 0 };

function renderFilter(
  counts: OutcomeCounts = KITCHEN_SINK,
  value: FilterValue = "all",
  announcement = "",
) {
  const onChange = vi.fn();
  render(
    <UnitOutcomeFilter
      counts={counts}
      value={value}
      onChange={onChange}
      announcement={announcement}
    />,
  );
  return onChange;
}

function group(): HTMLElement {
  return screen.getByRole("group", { name: "Show" });
}

function item(name: RegExp): HTMLElement {
  return within(group()).getByRole("button", { name });
}

describe("UnitOutcomeFilter", () => {
  it("labels the bar Show and the toggle group Show", () => {
    renderFilter();
    expect(group()).toBeInTheDocument();
    expect(screen.getByText("Show", { selector: "span" })).toBeInTheDocument();
  });

  it("shows four values in order, each with its count in its text", () => {
    renderFilter();
    expect(within(group()).getAllByRole("button").map((b) => b.textContent)).toEqual([
      "All 23",
      "Needs follow-up 9",
      "Carried 14",
      "Excluded 0",
    ]);
  });

  it("marks the selected value pressed and the others not", () => {
    renderFilter(KITCHEN_SINK, "carried");
    expect(item(/^Carried/)).toHaveAttribute("aria-pressed", "true");
    for (const name of [/^All/, /^Needs follow-up/, /^Excluded/]) {
      expect(item(name)).toHaveAttribute("aria-pressed", "false");
    }
  });

  it("disables a value with count 0 unless it is selected", () => {
    renderFilter();
    expect(item(/^Excluded/)).toBeDisabled();
    expect(item(/^Carried/)).toBeEnabled();
  });

  it("keeps a selected value with count 0 enabled", () => {
    renderFilter(KITCHEN_SINK, "excluded");
    expect(item(/^Excluded/)).toBeEnabled();
  });

  it("shows the hint while a row needs follow-up, and not otherwise", () => {
    renderFilter();
    expect(screen.getByText(HINT)).toBeInTheDocument();
  });

  it("shows no hint when nothing needs follow-up", () => {
    renderFilter({ all: 3, needs_follow_up: 0, carried: 2, excluded: 1 });
    expect(screen.queryByText(HINT)).not.toBeInTheDocument();
  });

  it("calls onChange with the value clicked", async () => {
    const onChange = renderFilter();
    await userEvent.click(item(/^Needs follow-up/));
    expect(onChange).toHaveBeenCalledWith("needs_follow_up");
  });

  it("does not call onChange when the selected value is clicked again", async () => {
    const onChange = renderFilter(KITCHEN_SINK, "carried");
    await userEvent.click(item(/^Carried/));
    expect(onChange).not.toHaveBeenCalled();
  });

  it("renders a polite live region holding the announcement, empty or not", () => {
    renderFilter();
    const region = document.querySelector('[aria-live="polite"]');
    expect(region).not.toBeNull();
    expect(region!.textContent).toBe("");
  });

  it("puts the announcement in the live region", () => {
    renderFilter(KITCHEN_SINK, "carried", "Showing 14 carried rows.");
    expect(document.querySelector('[aria-live="polite"]')!.textContent).toBe(
      "Showing 14 carried rows.",
    );
  });
});
