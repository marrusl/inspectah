import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RoutineSummary } from "../RoutineSummary";
import type { DecisionItemKind } from "../DecisionItem";
import type { RefinedPackage, AttentionTag } from "../../api/types";

// Mock fetch for useViewed
const mockFetch = vi.fn();
beforeEach(() => {
  mockFetch.mockReset();
  vi.stubGlobal("fetch", mockFetch);
  mockFetch.mockImplementation(() =>
    Promise.resolve({ ok: true, json: () => Promise.resolve({ ids: [] }) }),
  );
});
afterEach(() => {
  vi.restoreAllMocks();
});

const ROUTINE_TAG: AttentionTag = { level: "routine", reason: "package_baseline_match", detail: null };

function makePkg(name: string): DecisionItemKind {
  return {
    type: "package",
    data: {
      entry: {
        name,
        epoch: "0",
        version: "1.0",
        release: "1.el9",
        arch: "x86_64",
        state: "added",
        include: true,
        source_repo: "baseos",
        fleet: null,
      },
      attention: [ROUTINE_TAG],
    },
  };
}

describe("RoutineSummary", () => {
  it("renders '+ N routine' text", () => {
    const items = [makePkg("glibc"), makePkg("bash"), makePkg("coreutils")];
    render(
      <RoutineSummary
        items={items}
        onToggleInclude={vi.fn()}
        onMarkViewed={vi.fn()}
        viewedIds={new Set()}
        isPending={false}
      />,
    );
    expect(screen.getByText("+ 3 routine")).toBeInTheDocument();
  });

  it("starts collapsed by default", () => {
    const items = [makePkg("glibc")];
    render(
      <RoutineSummary
        items={items}
        onToggleInclude={vi.fn()}
        onMarkViewed={vi.fn()}
        viewedIds={new Set()}
        isPending={false}
      />,
    );
    expect(screen.queryByText("glibc.x86_64")).not.toBeInTheDocument();
  });

  it("expands to show real DecisionItem rows on click", async () => {
    const items = [makePkg("glibc"), makePkg("bash")];
    render(
      <RoutineSummary
        items={items}
        onToggleInclude={vi.fn()}
        onMarkViewed={vi.fn()}
        viewedIds={new Set()}
        isPending={false}
      />,
    );

    await userEvent.click(screen.getByText("+ 2 routine"));

    // Real DecisionItem rows render with role="row"
    const rows = screen.getAllByRole("row");
    expect(rows.length).toBeGreaterThanOrEqual(2);
    expect(screen.getByText("glibc.x86_64")).toBeInTheDocument();
    expect(screen.getByText("bash.x86_64")).toBeInTheDocument();
  });

  it("expanded routine packages retain include/exclude toggle", async () => {
    const onToggle = vi.fn();
    const items = [makePkg("glibc")];
    render(
      <RoutineSummary
        items={items}
        onToggleInclude={onToggle}
        onMarkViewed={vi.fn()}
        viewedIds={new Set()}
        isPending={false}
      />,
    );

    await userEvent.click(screen.getByText("+ 1 routine"));

    // Toggle switch should be present on the real DecisionItem
    const toggle = screen.getByRole("switch", { name: /toggle/i });
    expect(toggle).toBeInTheDocument();
    await userEvent.click(toggle);
    expect(onToggle).toHaveBeenCalled();
  });

  it("expanded routine packages track viewed state", async () => {
    const onMarkViewed = vi.fn();
    const items = [makePkg("glibc")];
    render(
      <RoutineSummary
        items={items}
        onToggleInclude={vi.fn()}
        onMarkViewed={onMarkViewed}
        viewedIds={new Set()}
        isPending={false}
      />,
    );

    await userEvent.click(screen.getByText("+ 1 routine"));

    // DecisionItem's expand button triggers handleExpand which calls onMarkViewed
    const expandBtn = screen.getByRole("button", { name: /expand glibc/i });
    await userEvent.click(expandBtn);
    expect(onMarkViewed).toHaveBeenCalled();
  });

  it("auto-expands when forceExpanded is true", () => {
    const items = [makePkg("glibc")];
    render(
      <RoutineSummary
        items={items}
        forceExpanded={true}
        onToggleInclude={vi.fn()}
        onMarkViewed={vi.fn()}
        viewedIds={new Set()}
        isPending={false}
      />,
    );
    expect(screen.getByText("glibc.x86_64")).toBeInTheDocument();
  });

  it("auto-expands when revealItemId matches an item", () => {
    const items = [makePkg("glibc")];
    render(
      <RoutineSummary
        items={items}
        revealItemId="packages:glibc.x86_64"
        onToggleInclude={vi.fn()}
        onMarkViewed={vi.fn()}
        viewedIds={new Set()}
        isPending={false}
      />,
    );
    expect(screen.getByText("glibc.x86_64")).toBeInTheDocument();
  });

  it("has correct data-testid", () => {
    const items = [makePkg("glibc")];
    render(
      <RoutineSummary
        items={items}
        onToggleInclude={vi.fn()}
        onMarkViewed={vi.fn()}
        viewedIds={new Set()}
        isPending={false}
      />,
    );
    expect(screen.getByTestId("routine-summary")).toBeInTheDocument();
  });

  it("has aria-expanded attribute", () => {
    const items = [makePkg("glibc")];
    render(
      <RoutineSummary
        items={items}
        onToggleInclude={vi.fn()}
        onMarkViewed={vi.fn()}
        viewedIds={new Set()}
        isPending={false}
      />,
    );
    const button = screen.getByRole("button");
    expect(button).toHaveAttribute("aria-expanded", "false");
  });
});
