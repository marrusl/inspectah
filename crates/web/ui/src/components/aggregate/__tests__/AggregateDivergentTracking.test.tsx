import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { AggregateApp } from "../../AggregateApp";
import type {
  FleetViewResponse,
  FleetHealthInfo,
  HealthResponse,
  FleetItem,
} from "../../../api/types";

// Mock aggregate-client
const mockFetchFleetView = vi.fn<() => Promise<FleetViewResponse>>();
vi.mock("../../../api/fleet-client", () => ({
  fetchFleetView: (...args: unknown[]) => mockFetchFleetView(...(args as [])),
  fetchFleetDiff: vi.fn(),
}));

// Mock client (used by useAggregateMutation)
vi.mock("../../../api/client", () => ({
  applyOp: vi.fn().mockResolvedValue({}),
  undo: vi.fn().mockResolvedValue({}),
  redo: vi.fn().mockResolvedValue({}),
}));

beforeEach(() => {
  vi.stubGlobal("fetch", vi.fn());
  mockFetchFleetView.mockReset();
});

const MOCK_AGGREGATE: FleetHealthInfo = {
  host_count: 3,
  hostnames: ["host1", "host2", "host3"],
  zones_active: true,
  variant_count: 0,
  label: "test-aggregate",
  merged_at: "2025-01-01T00:00:00Z",
};

const MOCK_HEALTH: HealthResponse = {
  status: "ok",
  host: {
    hostname: "host1",
    os_name: "RHEL",
    os_version: "9.4",
    os_id: "rhel",
    system_type: "physical",
    schema_version: 1,
  },
  completeness: "full",
  policy: { distro_repos: [] },
  aggregate: MOCK_AGGREGATE,
  session_is_sensitive: false,
};

/** A divergent config item for testing. */
function makeDivergentItem(path: string, include = true): FleetItem {
  return {
    item_id: { kind: "Config", key: { path } },
    include,
    triage: { bucket: "divergent", prevalence: { count: 1, total: 3 } },
    prevalence: { count: 1, total: 3 },
    source_repo: "",
  };
}

function makeAggregateViewWithDivergent(
  divergentItems: FleetItem[],
  extraServiceItems: FleetItem[] = [],
): FleetViewResponse {
  return {
    generation: 1,
    can_undo: false,
    can_redo: false,
    containerfile_preview: "FROM ubi9",
    session_is_sensitive: false,
    summary: {
      host_count: 3,
      actionable_variant_items: [],
      informational_variant_count: 0,
    },
    sections: [
      {
        id: "packages",
        display_name: "Packages",
        is_decision_section: true,
        items: [],
      },
      {
        id: "configs",
        display_name: "Config Files",
        is_decision_section: true,
        zones: {
          consensus: { items: [], count: 0 },
          near_consensus: { items: [], count: 0 },
          divergent: { items: divergentItems, count: divergentItems.length },
        },
      },
      {
        id: "services",
        display_name: "Services",
        is_decision_section: true,
        items: extraServiceItems,
      },
    ],
    repo_groups: [],
    repo_conflict_count: 0,
  };
}

describe("AggregateApp divergent review tracking", () => {
  it("shows divergent progress when divergent items exist", async () => {
    const view = makeAggregateViewWithDivergent([
      makeDivergentItem("/etc/foo.conf"),
      makeDivergentItem("/etc/bar.conf"),
      makeDivergentItem("/etc/baz.conf"),
    ]);
    mockFetchFleetView.mockResolvedValue(view);

    render(<AggregateApp aggregate={MOCK_AGGREGATE} health={MOCK_HEALTH} />);

    await waitFor(() => {
      expect(screen.getByTestId("divergent-progress")).toBeInTheDocument();
    });

    // All 3 divergent items should be unconfirmed initially
    expect(screen.getByTestId("divergent-progress")).toHaveTextContent(
      "Divergent: 3 (3 unconfirmed)",
    );
  });

  it("hides divergent progress when no divergent items exist", async () => {
    const view = makeAggregateViewWithDivergent([]);
    mockFetchFleetView.mockResolvedValue(view);

    render(<AggregateApp aggregate={MOCK_AGGREGATE} health={MOCK_HEALTH} />);

    await waitFor(() => {
      expect(screen.getByTestId("aggregate-content")).toBeInTheDocument();
    });

    expect(screen.queryByTestId("divergent-progress")).not.toBeInTheDocument();
  });

  it("marks divergent item as confirmed on SetInclude toggle", async () => {
    const divergentItem = makeDivergentItem("/etc/foo.conf");
    const view = makeAggregateViewWithDivergent([
      divergentItem,
      makeDivergentItem("/etc/bar.conf"),
    ]);
    // First call: initial load. Second call: after mutation refetch.
    mockFetchFleetView.mockResolvedValue(view);

    render(<AggregateApp aggregate={MOCK_AGGREGATE} health={MOCK_HEALTH} />);

    await waitFor(() => {
      expect(screen.getByTestId("divergent-progress")).toHaveTextContent(
        "Divergent: 2 (2 unconfirmed)",
      );
    });

    // Navigate to configs section to interact with the divergent item
    await userEvent.click(screen.getByText("Config Files"));

    // Find the toggle for /etc/foo.conf — the AggregateItemRow renders a switch
    await waitFor(() => {
      expect(screen.getByText("/etc/foo.conf")).toBeInTheDocument();
    });

    // Click the toggle switch for this item
    const toggle = screen.getByRole("switch", { name: /\/etc\/foo\.conf/i });
    await userEvent.click(toggle);

    // After toggling, the unconfirmed count should drop by 1
    await waitFor(() => {
      expect(screen.getByTestId("divergent-progress")).toHaveTextContent(
        "Divergent: 2 (1 unconfirmed)",
      );
    });
  });

  it("does not add non-divergent items to confirmed set", async () => {
    // Put a non-divergent service item in a flat section (no zones)
    const serviceItem: FleetItem = {
      item_id: { kind: "Service", key: { unit: "httpd.service" } },
      include: true,
      triage: { bucket: "universal", prevalence: { count: 3, total: 3 } },
      prevalence: { count: 3, total: 3 },
      source_repo: "",
    };
    const view = makeAggregateViewWithDivergent(
      [makeDivergentItem("/etc/divergent.conf")],
      [serviceItem],
    );
    mockFetchFleetView.mockResolvedValue(view);

    render(<AggregateApp aggregate={MOCK_AGGREGATE} health={MOCK_HEALTH} />);

    await waitFor(() => {
      expect(screen.getByTestId("divergent-progress")).toHaveTextContent(
        "Divergent: 1 (1 unconfirmed)",
      );
    });

    // Navigate to services section (flat items, always visible)
    await userEvent.click(screen.getByText("Services"));

    await waitFor(() => {
      expect(screen.getByText("httpd.service")).toBeInTheDocument();
    });

    // Toggle the service item (non-divergent)
    const toggle = screen.getByRole("switch", { name: /httpd\.service/i });
    await userEvent.click(toggle);

    // Divergent count should remain unchanged — non-divergent toggle doesn't affect it
    expect(screen.getByTestId("divergent-progress")).toHaveTextContent(
      "Divergent: 1 (1 unconfirmed)",
    );
  });
});
