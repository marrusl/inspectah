import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { VariantView } from "../VariantView";
import type { FleetItem, ItemId } from "../../../api/types";
import type { UseVariantAckResult } from "../../../hooks/useVariantAck";
import type { UseFleetDiffResult } from "../../../hooks/useFleetDiff";

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

describe("VariantView", () => {
  it("renders radio options for each variant", () => {
    render(
      <VariantView
        item={makeItem()}
        ack={makeAck()}
        onSelectVariant={vi.fn()}
        diffHook={makeDiffHook()}
      />,
    );

    const radios = screen.getAllByRole("radio");
    expect(radios).toHaveLength(3);
  });

  it("pre-selects current variant", () => {
    render(
      <VariantView
        item={makeItem()}
        ack={makeAck()}
        onSelectVariant={vi.fn()}
        diffHook={makeDiffHook()}
      />,
    );

    const radios = screen.getAllByRole("radio");
    // First option (aaa111) is selected
    expect(radios[0]).toBeChecked();
    expect(radios[1]).not.toBeChecked();
    expect(radios[2]).not.toBeChecked();
  });

  it("calls onSelectVariant when different variant selected", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();

    render(
      <VariantView
        item={makeItem()}
        ack={makeAck()}
        onSelectVariant={onSelect}
        diffHook={makeDiffHook()}
      />,
    );

    const radios = screen.getAllByRole("radio");
    await user.click(radios[1]);

    expect(onSelect).toHaveBeenCalledWith(configItemId, "bbb222");
  });

  it("shows Confirm button", () => {
    render(
      <VariantView
        item={makeItem()}
        ack={makeAck()}
        onSelectVariant={vi.fn()}
        diffHook={makeDiffHook()}
      />,
    );

    expect(screen.getByRole("button", { name: /confirm/i })).toBeInTheDocument();
  });

  it("calls ack.confirm when Confirm clicked", async () => {
    const user = userEvent.setup();
    const confirm = vi.fn();

    render(
      <VariantView
        item={makeItem()}
        ack={makeAck({ confirm })}
        onSelectVariant={vi.fn()}
        diffHook={makeDiffHook()}
      />,
    );

    await user.click(screen.getByRole("button", { name: /confirm/i }));
    expect(confirm).toHaveBeenCalledWith(configItemId);
  });

  it("auto-confirms via ack.markChanged when variant changed", async () => {
    const user = userEvent.setup();
    const markChanged = vi.fn();

    render(
      <VariantView
        item={makeItem()}
        ack={makeAck({ markChanged })}
        onSelectVariant={vi.fn()}
        diffHook={makeDiffHook()}
      />,
    );

    const radios = screen.getAllByRole("radio");
    await user.click(radios[1]);

    expect(markChanged).toHaveBeenCalledWith(configItemId);
  });

  it("shows Compare button", () => {
    render(
      <VariantView
        item={makeItem()}
        ack={makeAck()}
        onSelectVariant={vi.fn()}
        diffHook={makeDiffHook()}
      />,
    );

    expect(screen.getByRole("button", { name: /compare/i })).toBeInTheDocument();
  });

  it("shows host count for each variant option", () => {
    render(
      <VariantView
        item={makeItem()}
        ack={makeAck()}
        onSelectVariant={vi.fn()}
        diffHook={makeDiffHook()}
      />,
    );

    // Two options have 1 host each (aaa111, ccc333)
    const singleHostLabels = screen.getAllByText("1 host");
    expect(singleHostLabels).toHaveLength(2);
    expect(screen.getByText("2 hosts")).toBeInTheDocument();
  });
});
