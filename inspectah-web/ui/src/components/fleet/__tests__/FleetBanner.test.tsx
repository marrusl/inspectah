import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { FleetBanner } from "../FleetBanner";
import type { FleetSummary, ItemId } from "../../../api/types";
import type { UseVariantAckResult } from "../../../hooks/useVariantAck";

const defaultAck: UseVariantAckResult = {
  isAcked: () => false,
  getStatus: () => "unreviewed" as const,
  confirm: vi.fn(),
  markChanged: vi.fn(),
  unackedCount: 0,
  totalCount: 0,
};

function makeSummary(overrides: Partial<FleetSummary> = {}): FleetSummary {
  return {
    host_count: 3,
    actionable_variant_items: [],
    informational_variant_count: 0,
    ...overrides,
  };
}

const configItem = (
  path: string,
  sectionId: string,
  variantCount: number,
) => ({
  item_id: { kind: "Config" as const, key: { path } },
  section_id: sectionId,
  variant_count: variantCount,
  max_host_spread: 2,
});

const pkgItem = (
  nameArch: string,
  sectionId: string,
  variantCount: number,
) => ({
  item_id: { kind: "Package" as const, key: { name_arch: nameArch } },
  section_id: sectionId,
  variant_count: variantCount,
  max_host_spread: 2,
});

describe("FleetBanner", () => {
  it("does not render when no actionable variant items", () => {
    const summary = makeSummary({ actionable_variant_items: [] });
    const { container } = render(
      <FleetBanner
        summary={summary}
        ackState={defaultAck}
        onNavigate={vi.fn()}
      />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("renders danger state when no items reviewed", () => {
    const summary = makeSummary({
      actionable_variant_items: [
        configItem("/etc/httpd/conf/httpd.conf", "config_files", 3),
        pkgItem("httpd.x86_64", "packages", 2),
      ],
    });
    const ack: UseVariantAckResult = {
      ...defaultAck,
      unackedCount: 2,
      totalCount: 2,
    };

    render(
      <FleetBanner summary={summary} ackState={ack} onNavigate={vi.fn()} />,
    );

    const banner = screen.getByTestId("fleet-banner");
    expect(banner).toHaveAttribute("data-severity", "danger");
    expect(
      screen.getByText("2 config items have variants requiring review"),
    ).toBeInTheDocument();
  });

  it("renders warning state when some items reviewed", () => {
    const items = [
      configItem("/etc/httpd/conf/httpd.conf", "config_files", 3),
      pkgItem("httpd.x86_64", "packages", 2),
    ];
    const summary = makeSummary({ actionable_variant_items: items });
    const ack: UseVariantAckResult = {
      ...defaultAck,
      isAcked: (id: ItemId) => id.kind === "Config",
      unackedCount: 1,
      totalCount: 2,
    };

    render(
      <FleetBanner summary={summary} ackState={ack} onNavigate={vi.fn()} />,
    );

    const banner = screen.getByTestId("fleet-banner");
    expect(banner).toHaveAttribute("data-severity", "warning");
  });

  it("renders success state when all items reviewed", () => {
    const items = [
      configItem("/etc/httpd/conf/httpd.conf", "config_files", 3),
    ];
    const summary = makeSummary({ actionable_variant_items: items });
    const ack: UseVariantAckResult = {
      ...defaultAck,
      isAcked: () => true,
      unackedCount: 0,
      totalCount: 1,
    };

    render(
      <FleetBanner summary={summary} ackState={ack} onNavigate={vi.fn()} />,
    );

    const banner = screen.getByTestId("fleet-banner");
    expect(banner).toHaveAttribute("data-severity", "success");
    expect(
      screen.getByText("All 1 variants reviewed"),
    ).toBeInTheDocument();
  });

  it("shows item names with section tags", () => {
    const items = [
      configItem("/etc/sysctl.conf", "config_files", 2),
      pkgItem("nginx.x86_64", "packages", 3),
    ];
    const summary = makeSummary({ actionable_variant_items: items });
    const ack: UseVariantAckResult = {
      ...defaultAck,
      unackedCount: 2,
      totalCount: 2,
    };

    render(
      <FleetBanner summary={summary} ackState={ack} onNavigate={vi.fn()} />,
    );

    expect(screen.getByText("[Config]")).toBeInTheDocument();
    expect(screen.getByText("/etc/sysctl.conf")).toBeInTheDocument();
    expect(screen.getByText("[Packages]")).toBeInTheDocument();
    expect(screen.getByText("nginx.x86_64")).toBeInTheDocument();
  });

  it("calls onNavigate when item clicked", async () => {
    const user = userEvent.setup();
    const items = [
      configItem("/etc/httpd/conf/httpd.conf", "config_files", 3),
    ];
    const summary = makeSummary({ actionable_variant_items: items });
    const onNavigate = vi.fn();
    const ack: UseVariantAckResult = {
      ...defaultAck,
      unackedCount: 1,
      totalCount: 1,
    };

    render(
      <FleetBanner summary={summary} ackState={ack} onNavigate={onNavigate} />,
    );

    const link = screen.getByRole("button", {
      name: /\/etc\/httpd\/conf\/httpd\.conf/,
    });
    await user.click(link);

    expect(onNavigate).toHaveBeenCalledWith("config_files", {
      kind: "Config",
      key: { path: "/etc/httpd/conf/httpd.conf" },
    });
  });

  it("shows informational variant count when present", () => {
    const items = [
      configItem("/etc/httpd/conf/httpd.conf", "config_files", 3),
    ];
    const summary = makeSummary({
      actionable_variant_items: items,
      informational_variant_count: 5,
    });
    const ack: UseVariantAckResult = {
      ...defaultAck,
      unackedCount: 1,
      totalCount: 1,
    };

    render(
      <FleetBanner summary={summary} ackState={ack} onNavigate={vi.fn()} />,
    );

    expect(
      screen.getByText(
        "5 additional items in other sections have variants (read-only)",
      ),
    ).toBeInTheDocument();
  });

  it("shows correct variant count per item", () => {
    const items = [
      configItem("/etc/sysctl.conf", "config_files", 4),
      pkgItem("httpd.x86_64", "packages", 2),
    ];
    const summary = makeSummary({ actionable_variant_items: items });
    const ack: UseVariantAckResult = {
      ...defaultAck,
      unackedCount: 2,
      totalCount: 2,
    };

    render(
      <FleetBanner summary={summary} ackState={ack} onNavigate={vi.fn()} />,
    );

    expect(screen.getByText(/4 variants/)).toBeInTheDocument();
    expect(screen.getByText(/2 variants/)).toBeInTheDocument();
  });
});
