import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { AggregateSectionContent } from "../FleetSection";
import type { FleetSection, FleetItem, ItemId } from "../../../api/types";
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

function makeItem(
  overrides: Partial<FleetItem> & { item_id: ItemId },
): FleetItem {
  return {
    include: true,
    triage: {
      bucket: "universal" as const,
      prevalence: { count: 2, total: 3 },
    },
    prevalence: { count: 2, total: 3 },
    source_repo: "",
    ...overrides,
  };
}

const pkgItem = (
  name: string,
  overrides: Partial<FleetItem> = {},
): FleetItem => {
  const [n, a] = name.includes(".") ? name.split(".") : [name, "x86_64"];
  return makeItem({
    item_id: { kind: "Package", key: { name: n, arch: a } },
    ...overrides,
  });
};

const cfgItem = (path: string, overrides: Partial<FleetItem> = {}): FleetItem =>
  makeItem({
    item_id: { kind: "Config", key: { path } },
    ...overrides,
  });

describe("AggregateSectionContent", () => {
  it("renders zone groups when zones are present", () => {
    const section: FleetSection = {
      id: "packages",
      display_name: "Packages",
      is_decision_section: true,
      zones: {
        consensus: { items: [pkgItem("httpd.x86_64")], count: 1 },
        near_consensus: { items: [pkgItem("nginx.x86_64")], count: 1 },
        divergent: { items: [pkgItem("curl.x86_64")], count: 1 },
      },
    };

    render(
      <AggregateSectionContent
        section={section}
        filterText=""
        isDecisionSection={true}
        onToggle={vi.fn()}
        ack={defaultAck}
      />,
    );

    expect(screen.getByTestId("zone-consensus")).toBeInTheDocument();
    expect(screen.getByTestId("zone-near_consensus")).toBeInTheDocument();
    expect(screen.getByTestId("zone-divergent")).toBeInTheDocument();
  });

  it("renders flat items when zones are null", () => {
    const section: FleetSection = {
      id: "packages",
      display_name: "Packages",
      is_decision_section: true,
      items: [pkgItem("httpd.x86_64"), pkgItem("nginx.x86_64")],
    };

    render(
      <AggregateSectionContent
        section={section}
        filterText=""
        isDecisionSection={true}
        onToggle={vi.fn()}
        ack={defaultAck}
      />,
    );

    expect(screen.queryByTestId("zone-consensus")).not.toBeInTheDocument();
    expect(screen.getByText("httpd.x86_64")).toBeInTheDocument();
    expect(screen.getByText("nginx.x86_64")).toBeInTheDocument();
  });

  it("filters items by filterText", () => {
    const section: FleetSection = {
      id: "packages",
      display_name: "Packages",
      is_decision_section: true,
      items: [
        pkgItem("httpd.x86_64"),
        pkgItem("nginx.x86_64"),
        pkgItem("curl.x86_64"),
      ],
    };

    render(
      <AggregateSectionContent
        section={section}
        filterText="http"
        isDecisionSection={true}
        onToggle={vi.fn()}
        ack={defaultAck}
      />,
    );

    expect(screen.getByText("httpd.x86_64")).toBeInTheDocument();
    expect(screen.queryByText("nginx.x86_64")).not.toBeInTheDocument();
    expect(screen.queryByText("curl.x86_64")).not.toBeInTheDocument();
  });

  it("suppresses zone headers when only one zone has items", () => {
    const section: FleetSection = {
      id: "configs",
      display_name: "Config Files",
      is_decision_section: true,
      zones: {
        consensus: { items: [cfgItem("/etc/httpd/conf/httpd.conf")], count: 1 },
        near_consensus: { items: [], count: 0 },
        divergent: { items: [], count: 0 },
      },
    };

    render(
      <AggregateSectionContent
        section={section}
        filterText=""
        isDecisionSection={true}
        onToggle={vi.fn()}
        ack={defaultAck}
      />,
    );

    // Zone headers should be suppressed when only one zone has items
    expect(screen.queryByTestId("zone-consensus")).not.toBeInTheDocument();
    expect(screen.queryByTestId("zone-near_consensus")).not.toBeInTheDocument();
    expect(screen.queryByTestId("zone-divergent")).not.toBeInTheDocument();
    // But the item itself should render
    expect(screen.getByText("/etc/httpd/conf/httpd.conf")).toBeInTheDocument();
  });

  it("renders AggregateItemRow for each item", () => {
    const section: FleetSection = {
      id: "packages",
      display_name: "Packages",
      is_decision_section: true,
      items: [pkgItem("httpd.x86_64"), pkgItem("nginx.x86_64")],
    };

    render(
      <AggregateSectionContent
        section={section}
        filterText=""
        isDecisionSection={true}
        onToggle={vi.fn()}
        ack={defaultAck}
      />,
    );

    // Each item should have a data-item-id attribute
    const rows = screen.getAllByTestId("aggregate-item-row");
    expect(rows).toHaveLength(2);
  });

  it("renders nothing when section is undefined", () => {
    const { container } = render(
      <AggregateSectionContent
        section={undefined}
        filterText=""
        isDecisionSection={false}
        onToggle={vi.fn()}
        ack={defaultAck}
      />,
    );

    expect(container.firstChild).toBeNull();
  });

  it("filters items within zone groups", () => {
    const section: FleetSection = {
      id: "configs",
      display_name: "Config Files",
      is_decision_section: true,
      zones: {
        consensus: {
          items: [
            cfgItem("/etc/httpd/conf/httpd.conf"),
            cfgItem("/etc/sysctl.conf"),
          ],
          count: 2,
        },
        near_consensus: { items: [cfgItem("/etc/nginx/nginx.conf")], count: 1 },
        divergent: { items: [], count: 0 },
      },
    };

    render(
      <AggregateSectionContent
        section={section}
        filterText="httpd"
        isDecisionSection={true}
        onToggle={vi.fn()}
        ack={defaultAck}
      />,
    );

    expect(screen.getByText("/etc/httpd/conf/httpd.conf")).toBeInTheDocument();
    expect(screen.queryByText("/etc/sysctl.conf")).not.toBeInTheDocument();
    expect(screen.queryByText("/etc/nginx/nginx.conf")).not.toBeInTheDocument();
  });
});
