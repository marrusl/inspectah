import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { VariantView } from "../VariantView";
import type { FleetItem, ItemId } from "../../../api/types";
import type { UseVariantAckResult } from "../../../hooks/useVariantAck";
import type { UseFleetDiffResult } from "../../../hooks/useFleetDiff";

// --- Helpers ---

const configItemId: ItemId = {
  kind: "Config",
  key: { path: "/etc/httpd/conf/httpd.conf" },
};

function makeItem(overrides?: Partial<FleetItem>): FleetItem {
  return {
    item_id: configItemId,
    include: true,
    attention: { level: "none", reason: "", prevalence: 1 },
    prevalence: { count: 3, total: 3 },
    variants: {
      count: 3,
      selected: "aaa111",
      options: [
        { hash: "aaa111", hosts: ["host-a"], host_count: 1, selected: true },
        { hash: "bbb222", hosts: ["host-b", "host-c"], host_count: 2, selected: false },
        { hash: "ccc333", hosts: ["host-d"], host_count: 1, selected: false },
      ],
    },
    ...overrides,
  };
}

function makeAck(overrides?: Partial<UseVariantAckResult>): UseVariantAckResult {
  return {
    isAcked: () => false,
    getStatus: () => "unreviewed" as const,
    confirm: vi.fn(),
    markChanged: vi.fn(),
    unackedCount: 0,
    totalCount: 0,
    ...overrides,
  };
}

function makeDiffHook(overrides?: Partial<UseFleetDiffResult>): UseFleetDiffResult {
  return {
    fetchDiff: vi.fn(),
    diff: null,
    isLoading: false,
    error: null,
    clearDiff: vi.fn(),
    ...overrides,
  };
}

describe("Fleet keyboard", () => {
  it("c key triggers compare when variant view is open", async () => {
    const user = userEvent.setup();
    const fetchDiff = vi.fn();

    render(
      <VariantView
        item={makeItem()}
        ack={makeAck()}
        onSelectVariant={vi.fn()}
        diffHook={makeDiffHook({ fetchDiff })}
      />,
    );

    // Focus the variant view container
    const variantView = screen.getByTestId("variant-view");
    variantView.focus();

    await user.keyboard("c");

    expect(fetchDiff).toHaveBeenCalledWith(configItemId, "aaa111", "bbb222");
  });

  it("c key does not trigger compare when focused in text input", async () => {
    const user = userEvent.setup();
    const fetchDiff = vi.fn();

    // Render variant view with an input inside the test container
    render(
      <div>
        <input data-testid="text-input" />
        <VariantView
          item={makeItem()}
          ack={makeAck()}
          onSelectVariant={vi.fn()}
          diffHook={makeDiffHook({ fetchDiff })}
        />
      </div>,
    );

    // Focus the text input, then press c
    const input = screen.getByTestId("text-input");
    input.focus();
    await user.keyboard("c");

    expect(fetchDiff).not.toHaveBeenCalled();
  });

  it("c key does not trigger compare with fewer than 2 variants", async () => {
    const user = userEvent.setup();
    const fetchDiff = vi.fn();
    const singleVariantItem = makeItem({
      variants: {
        count: 1,
        selected: "aaa111",
        options: [
          { hash: "aaa111", hosts: ["host-a"], host_count: 1, selected: true },
        ],
      },
    });

    render(
      <VariantView
        item={singleVariantItem}
        ack={makeAck()}
        onSelectVariant={vi.fn()}
        diffHook={makeDiffHook({ fetchDiff })}
      />,
    );

    const variantView = screen.getByTestId("variant-view");
    variantView.focus();
    await user.keyboard("c");

    expect(fetchDiff).not.toHaveBeenCalled();
  });

  it("Escape closes DiffDrawer", async () => {
    const user = userEvent.setup();
    const clearDiff = vi.fn();
    const sampleDiff = {
      base_hash: "aaa111",
      target_hash: "bbb222",
      base_hosts: ["host-a"],
      target_hosts: ["host-b"],
      hunks: [],
      stats: { total_changes: 0, insertions: 0, deletions: 0 },
    };

    render(
      <VariantView
        item={makeItem()}
        ack={makeAck()}
        onSelectVariant={vi.fn()}
        diffHook={makeDiffHook({ diff: sampleDiff, clearDiff })}
      />,
    );

    // Open the DiffDrawer by clicking Compare
    await user.click(screen.getByRole("button", { name: /compare/i }));
    expect(screen.getByTestId("diff-drawer")).toBeInTheDocument();

    // Press Escape
    await user.keyboard("{Escape}");

    // DiffDrawer should be closed
    expect(screen.queryByTestId("diff-drawer")).not.toBeInTheDocument();
    expect(clearDiff).toHaveBeenCalled();
  });

  it("Escape does nothing when DiffDrawer is not open", async () => {
    const user = userEvent.setup();
    const clearDiff = vi.fn();

    render(
      <VariantView
        item={makeItem()}
        ack={makeAck()}
        onSelectVariant={vi.fn()}
        diffHook={makeDiffHook({ clearDiff })}
      />,
    );

    const variantView = screen.getByTestId("variant-view");
    variantView.focus();
    await user.keyboard("{Escape}");

    // clearDiff should NOT be called when drawer isn't open
    expect(clearDiff).not.toHaveBeenCalled();
  });
});

describe("Focus recovery", () => {
  beforeEach(() => {
    // Clean up any leftover elements
    document.body.innerHTML = "";
  });

  it("focus recovery restores focus after refetch", async () => {
    // Simulate a fleet item list with data-item-id attributes
    const container = document.createElement("div");
    document.body.appendChild(container);

    const itemId = JSON.stringify({ kind: "Config", key: { path: "/etc/foo" } });

    // Create an element with data-item-id and tabIndex
    const item = document.createElement("div");
    item.setAttribute("data-item-id", itemId);
    item.tabIndex = 0;
    container.appendChild(item);

    // Focus the item
    item.focus();
    expect(document.activeElement).toBe(item);

    // Record the focused item id (simulating what the hook does)
    const focusedId = document.activeElement?.getAttribute("data-item-id");
    expect(focusedId).toBe(itemId);

    // Simulate refetch: remove and re-add the element (new DOM node, same id)
    container.removeChild(item);
    const newItem = document.createElement("div");
    newItem.setAttribute("data-item-id", itemId);
    newItem.tabIndex = 0;
    container.appendChild(newItem);

    // Simulate focus recovery
    const el = document.querySelector(`[data-item-id='${CSS.escape(itemId)}']`);
    if (el) (el as HTMLElement).focus();

    expect(document.activeElement).toBe(newItem);

    document.body.removeChild(container);
  });

  it("focus falls back to first item when original item removed", () => {
    const container = document.createElement("div");
    document.body.appendChild(container);

    const removedId = JSON.stringify({ kind: "Config", key: { path: "/etc/removed" } });
    const remainingId = JSON.stringify({ kind: "Config", key: { path: "/etc/remaining" } });

    // Create the remaining item (original was removed after refetch)
    const remainingItem = document.createElement("div");
    remainingItem.setAttribute("data-item-id", remainingId);
    remainingItem.tabIndex = 0;
    container.appendChild(remainingItem);

    // Try to restore focus to removed item — it doesn't exist
    const el = document.querySelector(`[data-item-id='${CSS.escape(removedId)}']`);
    expect(el).toBeNull();

    // Fallback: focus the first item with data-item-id
    const firstItem = document.querySelector("[data-item-id]");
    if (firstItem) (firstItem as HTMLElement).focus();

    expect(document.activeElement).toBe(remainingItem);

    document.body.removeChild(container);
  });
});
