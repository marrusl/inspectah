import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { FleetItemRow } from "../FleetItemRow";
import type { FleetItem, ItemId } from "../../../api/types";
import type { UseVariantAckResult } from "../../../hooks/useVariantAck";

const defaultAck: UseVariantAckResult = {
  isAcked: () => false,
  getStatus: () => "unreviewed" as const,
  confirm: vi.fn(),
  unconfirm: vi.fn(),
  markChanged: vi.fn(),
  unackedCount: 0,
  totalCount: 0,
};

function makeItem(overrides: Partial<FleetItem> & { item_id: ItemId }): FleetItem {
  return {
    include: true,
    attention: { level: "none", reason: "", prevalence: 1 },
    prevalence: { count: 2, total: 3 },
    ...overrides,
  };
}

describe("FleetItemRow", () => {
  it("renders item name from Package item_id", () => {
    const item = makeItem({
      item_id: { kind: "Package", key: { name_arch: "httpd.x86_64" } },
    });

    render(
      <FleetItemRow
        item={item}
        isDecisionSection={true}
        onToggle={vi.fn()}
        ack={defaultAck}
      />,
    );

    expect(screen.getByText("httpd.x86_64")).toBeInTheDocument();
  });

  it("renders item name from Config item_id", () => {
    const item = makeItem({
      item_id: { kind: "Config", key: { path: "/etc/httpd/conf/httpd.conf" } },
    });

    render(
      <FleetItemRow
        item={item}
        isDecisionSection={true}
        onToggle={vi.fn()}
        ack={defaultAck}
      />,
    );

    expect(screen.getByText("/etc/httpd/conf/httpd.conf")).toBeInTheDocument();
  });

  it("renders prevalence chip", () => {
    const item = makeItem({
      item_id: { kind: "Package", key: { name_arch: "httpd.x86_64" } },
      prevalence: { count: 8, total: 12 },
    });

    render(
      <FleetItemRow
        item={item}
        isDecisionSection={true}
        onToggle={vi.fn()}
        ack={defaultAck}
      />,
    );

    expect(screen.getByText("8/12 hosts")).toBeInTheDocument();
  });

  it("renders variant count when variants exist", () => {
    const item = makeItem({
      item_id: { kind: "Package", key: { name_arch: "httpd.x86_64" } },
      variants: {
        count: 3,
        selected: "abc123",
        options: [
          { hash: "abc123", hosts: ["h1"], host_count: 1, selected: true },
          { hash: "def456", hosts: ["h2"], host_count: 1, selected: false },
          { hash: "ghi789", hosts: ["h3"], host_count: 1, selected: false },
        ],
      },
    });

    render(
      <FleetItemRow
        item={item}
        isDecisionSection={true}
        onToggle={vi.fn()}
        ack={defaultAck}
      />,
    );

    expect(screen.getByText("3 variants")).toBeInTheDocument();
  });

  it("does not render variant indicator when no variants", () => {
    const item = makeItem({
      item_id: { kind: "Package", key: { name_arch: "httpd.x86_64" } },
    });

    render(
      <FleetItemRow
        item={item}
        isDecisionSection={true}
        onToggle={vi.fn()}
        ack={defaultAck}
      />,
    );

    expect(screen.queryByText(/variants/)).not.toBeInTheDocument();
  });

  it("renders toggle for decision sections", () => {
    const item = makeItem({
      item_id: { kind: "Package", key: { name_arch: "httpd.x86_64" } },
    });

    render(
      <FleetItemRow
        item={item}
        isDecisionSection={true}
        onToggle={vi.fn()}
        ack={defaultAck}
      />,
    );

    expect(screen.getByRole("switch")).toBeInTheDocument();
  });

  it("does not render toggle for context sections", () => {
    const item = makeItem({
      item_id: { kind: "Package", key: { name_arch: "httpd.x86_64" } },
    });

    render(
      <FleetItemRow
        item={item}
        isDecisionSection={false}
        onToggle={vi.fn()}
        ack={defaultAck}
      />,
    );

    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
  });

  it("sets data-item-id attribute", () => {
    const itemId: ItemId = { kind: "Package", key: { name_arch: "httpd.x86_64" } };
    const item = makeItem({ item_id: itemId });

    render(
      <FleetItemRow
        item={item}
        isDecisionSection={true}
        onToggle={vi.fn()}
        ack={defaultAck}
      />,
    );

    const row = screen.getByTestId("fleet-item-row");
    expect(row).toHaveAttribute("data-item-id", JSON.stringify(itemId));
  });

  it("calls onToggle when toggle is switched", async () => {
    const user = userEvent.setup();
    const onToggle = vi.fn();
    const item = makeItem({
      item_id: { kind: "Package", key: { name_arch: "httpd.x86_64" } },
      include: true,
    });

    render(
      <FleetItemRow
        item={item}
        isDecisionSection={true}
        onToggle={onToggle}
        ack={defaultAck}
      />,
    );

    const toggle = screen.getByRole("switch", { name: /toggle httpd/i });
    await user.click(toggle);

    expect(onToggle).toHaveBeenCalledWith(
      { kind: "Package", key: { name_arch: "httpd.x86_64" } },
      false,
    );
  });

  it("calls onExpandVariant when variant indicator clicked", async () => {
    const user = userEvent.setup();
    const onExpand = vi.fn();
    const item = makeItem({
      item_id: { kind: "Package", key: { name_arch: "httpd.x86_64" } },
      variants: {
        count: 3,
        selected: "abc123",
        options: [
          { hash: "abc123", hosts: ["h1"], host_count: 1, selected: true },
          { hash: "def456", hosts: ["h2"], host_count: 1, selected: false },
          { hash: "ghi789", hosts: ["h3"], host_count: 1, selected: false },
        ],
      },
    });

    render(
      <FleetItemRow
        item={item}
        isDecisionSection={true}
        onToggle={vi.fn()}
        ack={defaultAck}
        onExpandVariant={onExpand}
      />,
    );

    const variantBtn = screen.getByText("3 variants");
    await user.click(variantBtn);

    expect(onExpand).toHaveBeenCalledWith({
      kind: "Package",
      key: { name_arch: "httpd.x86_64" },
    });
  });

  it("does not render attention badges in fleet item rows", () => {
    const levels = ["needs_review", "informational", "routine"];
    for (const level of levels) {
      const item = makeItem({
        item_id: { kind: "Package", key: { name_arch: `test-${level}.x86_64` } },
        attention: { level, reason: "test", prevalence: 1 },
      });

      const { unmount } = render(
        <FleetItemRow
          item={item}
          isDecisionSection={true}
          onToggle={vi.fn()}
          ack={defaultAck}
        />,
      );

      expect(screen.queryByTestId("attention-badge")).not.toBeInTheDocument();
      unmount();
    }
  });

  it("does not render attention badge for none level", () => {
    const item = makeItem({
      item_id: { kind: "Package", key: { name_arch: "httpd.x86_64" } },
      attention: { level: "none", reason: "", prevalence: 1 },
    });

    render(
      <FleetItemRow
        item={item}
        isDecisionSection={true}
        onToggle={vi.fn()}
        ack={defaultAck}
      />,
    );

    expect(screen.queryByTestId("attention-badge")).not.toBeInTheDocument();
  });
});
