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
    triage: { bucket: "universal" as const, prevalence: { count: 3, total: 3 } },
    prevalence: { count: 3, total: 3 },
    source_repo: "",
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
    unconfirm: vi.fn(),
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

  it("shows 'Mark as reviewed' button when unreviewed", () => {
    render(
      <VariantView
        item={makeItem()}
        ack={makeAck()}
        onSelectVariant={vi.fn()}
        diffHook={makeDiffHook()}
      />,
    );

    expect(screen.getByRole("button", { name: /mark as reviewed/i })).toBeInTheDocument();
  });

  it("calls ack.confirm when 'Mark as reviewed' clicked", async () => {
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

    await user.click(screen.getByRole("button", { name: /mark as reviewed/i }));
    expect(confirm).toHaveBeenCalledWith(configItemId);
  });

  it("shows 'Reviewed' indicator when item is acked", () => {
    render(
      <VariantView
        item={makeItem()}
        ack={makeAck({ isAcked: () => true })}
        onSelectVariant={vi.fn()}
        diffHook={makeDiffHook()}
      />,
    );

    expect(screen.getByTestId("variant-reviewed-indicator")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /mark as reviewed/i })).not.toBeInTheDocument();
  });

  it("calls ack.unconfirm when 'Reviewed' indicator clicked", async () => {
    const user = userEvent.setup();
    const unconfirm = vi.fn();

    render(
      <VariantView
        item={makeItem()}
        ack={makeAck({ isAcked: () => true, unconfirm })}
        onSelectVariant={vi.fn()}
        diffHook={makeDiffHook()}
      />,
    );

    await user.click(screen.getByTestId("variant-reviewed-indicator"));
    expect(unconfirm).toHaveBeenCalledWith(configItemId);
  });

  it("does NOT auto-confirm when variant changed — user must explicitly review", async () => {
    const user = userEvent.setup();
    const markChanged = vi.fn();
    const confirm = vi.fn();

    render(
      <VariantView
        item={makeItem()}
        ack={makeAck({ markChanged, confirm })}
        onSelectVariant={vi.fn()}
        diffHook={makeDiffHook()}
      />,
    );

    const radios = screen.getAllByRole("radio");
    await user.click(radios[1]);

    expect(markChanged).not.toHaveBeenCalled();
    expect(confirm).not.toHaveBeenCalled();
  });

  it("shows 'Diff vs selected' link on non-selected variants", () => {
    render(
      <VariantView
        item={makeItem()}
        ack={makeAck()}
        onSelectVariant={vi.fn()}
        diffHook={makeDiffHook()}
      />,
    );

    const diffLinks = screen.getAllByRole("button", { name: /diff vs selected/i });
    // Two non-selected variants should each have a link
    expect(diffLinks).toHaveLength(2);
  });

  it("does not show 'Diff vs selected' on the selected variant", () => {
    render(
      <VariantView
        item={makeItem()}
        ack={makeAck()}
        onSelectVariant={vi.fn()}
        diffHook={makeDiffHook()}
      />,
    );

    // Selected variant shows "Selected" indicator, not diff link
    expect(screen.getByTestId("variant-selected-indicator")).toBeInTheDocument();
    // Total diff links = number of non-selected variants (2)
    const diffLinks = screen.getAllByRole("button", { name: /diff vs selected/i });
    expect(diffLinks).toHaveLength(2);
  });

  it("does not show a global Compare button", () => {
    render(
      <VariantView
        item={makeItem()}
        ack={makeAck()}
        onSelectVariant={vi.fn()}
        diffHook={makeDiffHook()}
      />,
    );

    expect(screen.queryByRole("button", { name: /^compare$/i })).not.toBeInTheDocument();
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
    const singleHostLabels = screen.getAllByText("1 host:");
    expect(singleHostLabels).toHaveLength(2);
    expect(screen.getByText("2 hosts:")).toBeInTheDocument();
  });

  it("calls diffHook.fetchDiff with correct args when 'Diff vs selected' clicked", async () => {
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

    const diffLinks = screen.getAllByRole("button", { name: /diff vs selected/i });
    // Click first diff link (bbb222 row)
    await user.click(diffLinks[0]);

    // Should diff between selected (aaa111) and clicked row (bbb222)
    expect(fetchDiff).toHaveBeenCalledWith(configItemId, "aaa111", "bbb222");
  });

  it("shows DiffDrawer with operand header when 'Diff vs selected' clicked", async () => {
    const user = userEvent.setup();
    const sampleDiff = {
      base_hash: "aaa111",
      target_hash: "bbb222",
      base_hosts: ["host-a"],
      target_hosts: ["host-b", "host-c"],
      hunks: [],
      stats: { total_changes: 0, insertions: 0, deletions: 0 },
    };

    render(
      <VariantView
        item={makeItem()}
        ack={makeAck()}
        onSelectVariant={vi.fn()}
        diffHook={makeDiffHook({ diff: sampleDiff })}
      />,
    );

    // DiffDrawer not shown initially
    expect(screen.queryByTestId("diff-drawer")).not.toBeInTheDocument();

    const diffLinks = screen.getAllByRole("button", { name: /diff vs selected/i });
    await user.click(diffLinks[0]);

    // DiffDrawer now visible with descriptive header
    expect(screen.getByTestId("diff-drawer")).toBeInTheDocument();
    const title = screen.getByTestId("diff-drawer-title");
    expect(title.textContent).toContain("bbb222");
    expect(title.textContent).toContain("aaa111");
    expect(title.textContent).toContain("[selected]");
  });

  it("does not show 'Diff vs selected' links when only 1 variant option", () => {
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
        diffHook={makeDiffHook()}
      />,
    );

    expect(screen.queryByRole("button", { name: /diff vs selected/i })).not.toBeInTheDocument();
  });

  it("returns null for items without variants", () => {
    const noVariantsItem = makeItem({ variants: undefined });

    const { container } = render(
      <VariantView
        item={noVariantsItem}
        ack={makeAck()}
        onSelectVariant={vi.fn()}
        diffHook={makeDiffHook()}
      />,
    );

    expect(container.firstChild).toBeNull();
  });
});
