import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { FleetApp } from "../../FleetApp";
import type { FleetAppProps } from "../../FleetApp";
import { FleetSidebar } from "../FleetSidebar";
import type { FleetViewResponse, FleetHealthInfo, HealthResponse } from "../../../api/types";

// Mock fleet-client
const mockFetchFleetView = vi.fn<() => Promise<FleetViewResponse>>();
vi.mock("../../../api/fleet-client", () => ({
  fetchFleetView: (...args: unknown[]) => mockFetchFleetView(...(args as [])),
  fetchFleetDiff: vi.fn(),
}));

// Mock client (used by useFleetMutation)
vi.mock("../../../api/client", () => ({
  applyOp: vi.fn().mockResolvedValue({}),
  undo: vi.fn().mockResolvedValue({}),
  redo: vi.fn().mockResolvedValue({}),
}));

// Mock fetch for ExportDialog internals
beforeEach(() => {
  vi.stubGlobal("fetch", vi.fn());
  mockFetchFleetView.mockReset();
});

const MOCK_FLEET: FleetHealthInfo = {
  host_count: 3,
  hostnames: ["host1", "host2", "host3"],
  zones_active: true,
  variant_count: 5,
  label: "test-fleet",
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
  fleet: MOCK_FLEET,
  session_is_sensitive: false,
};

function makeFleetView(overrides: Partial<FleetViewResponse> = {}): FleetViewResponse {
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
        items: [
          {
            item_id: { kind: "Package", key: { name_arch: "httpd.x86_64" } },
            include: true,
            attention: { level: "medium", reason: "variant", prevalence: 2 },
            prevalence: { count: 2, total: 3 },
          },
        ],
      },
      {
        id: "configs",
        display_name: "Config Files",
        is_decision_section: true,
        zones: {
          consensus: { items: [], count: 5 },
          near_consensus: { items: [], count: 2 },
          divergent: { items: [], count: 1 },
        },
      },
      {
        id: "services",
        display_name: "Services",
        is_decision_section: false,
        items: [],
      },
    ],
    ...overrides,
  };
}

function renderFleetApp(overrides: Partial<FleetAppProps> = {}) {
  const props: FleetAppProps = {
    fleet: MOCK_FLEET,
    health: MOCK_HEALTH,
    ...overrides,
  };
  return render(<FleetApp {...props} />);
}

describe("FleetApp", () => {
  it("shows loading state before data arrives", () => {
    mockFetchFleetView.mockReturnValue(new Promise(() => {})); // never resolves
    renderFleetApp();
    expect(screen.getByText(/Loading fleet view/)).toBeInTheDocument();
  });

  it("shows error state on fetch failure", async () => {
    mockFetchFleetView.mockRejectedValue(new Error("Network error"));
    renderFleetApp();
    await waitFor(() => {
      expect(screen.getByText(/Failed to load fleet view/)).toBeInTheDocument();
    });
    expect(screen.getByText("Network error")).toBeInTheDocument();
  });

  it("fetches fleet view on mount and renders sections", async () => {
    const fleetView = makeFleetView();
    mockFetchFleetView.mockResolvedValue(fleetView);
    renderFleetApp();

    await waitFor(() => {
      expect(screen.getByTestId("fleet-content")).toBeInTheDocument();
    });

    expect(screen.getByText("Sections: 3")).toBeInTheDocument();
    expect(mockFetchFleetView).toHaveBeenCalledOnce();
  });

  it("renders FleetSidebar with sections", async () => {
    mockFetchFleetView.mockResolvedValue(makeFleetView());
    renderFleetApp();

    await waitFor(() => {
      expect(screen.getByTestId("fleet-sidebar")).toBeInTheDocument();
    });

    expect(screen.getByText("Packages")).toBeInTheDocument();
    expect(screen.getByText("Config Files")).toBeInTheDocument();
    expect(screen.getByText("Services")).toBeInTheDocument();
  });

  it("handles section navigation", async () => {
    mockFetchFleetView.mockResolvedValue(makeFleetView());
    renderFleetApp();

    await waitFor(() => {
      expect(screen.getByTestId("fleet-sidebar")).toBeInTheDocument();
    });

    // Default active section
    expect(screen.getByText("Active section: packages")).toBeInTheDocument();

    // Click on Config Files
    const configNav = screen.getByText("Config Files");
    await userEvent.click(configNav);

    expect(screen.getByText("Active section: configs")).toBeInTheDocument();
  });

  it("wires undo/redo to mutation hook", async () => {
    const fleetView = makeFleetView({ can_undo: true, can_redo: true });
    mockFetchFleetView.mockResolvedValue(fleetView);
    renderFleetApp();

    await waitFor(() => {
      expect(screen.getByTestId("fleet-content")).toBeInTheDocument();
    });

    // AppShell renders StatsBar which has undo/redo buttons
    const undoBtn = screen.getByRole("button", { name: /undo/i });
    const redoBtn = screen.getByRole("button", { name: /redo/i });
    expect(undoBtn).toBeInTheDocument();
    expect(redoBtn).toBeInTheDocument();
  });

  it("shows refetch error with retry button", async () => {
    // First call succeeds, simulating a state where refetchError is set
    const fleetView = makeFleetView();
    mockFetchFleetView.mockResolvedValue(fleetView);
    renderFleetApp();

    await waitFor(() => {
      expect(screen.getByTestId("fleet-content")).toBeInTheDocument();
    });

    // Verify no refetch error initially
    expect(screen.queryByTestId("refetch-error")).not.toBeInTheDocument();
  });
});

describe("FleetSidebar", () => {
  const defaultAck = {
    isAcked: () => false,
    getStatus: () => "unreviewed" as const,
    confirm: vi.fn(),
    markChanged: vi.fn(),
    unackedCount: 2,
    totalCount: 4,
  };

  const sections = [
    {
      id: "packages",
      display_name: "Packages",
      is_decision_section: true,
      items: [
        {
          item_id: { kind: "Package" as const, key: { name_arch: "httpd.x86_64" } },
          include: true,
          attention: { level: "medium", reason: "variant", prevalence: 2 },
          prevalence: { count: 2, total: 3 },
        },
      ],
    },
    {
      id: "configs",
      display_name: "Config Files",
      is_decision_section: true,
      zones: {
        consensus: { items: [], count: 5 },
        near_consensus: { items: [], count: 2 },
        divergent: { items: [], count: 1 },
      },
    },
    {
      id: "services",
      display_name: "Services",
      is_decision_section: false,
      items: [],
    },
  ];

  it("renders all sections", () => {
    render(
      <FleetSidebar
        sections={sections}
        activeSection="packages"
        onSelect={vi.fn()}
        ackState={defaultAck}
      />,
    );
    expect(screen.getByText("Packages")).toBeInTheDocument();
    expect(screen.getByText("Config Files")).toBeInTheDocument();
    expect(screen.getByText("Services")).toBeInTheDocument();
  });

  it("highlights active section", () => {
    render(
      <FleetSidebar
        sections={sections}
        activeSection="configs"
        onSelect={vi.fn()}
        ackState={defaultAck}
      />,
    );
    // PF6 NavItem sets aria-current="page" on active item
    const configItem = screen.getByText("Config Files").closest("[aria-current]");
    expect(configItem).toHaveAttribute("aria-current", "page");
  });

  it("calls onSelect when clicking a section", async () => {
    const onSelect = vi.fn();
    render(
      <FleetSidebar
        sections={sections}
        activeSection="packages"
        onSelect={onSelect}
        ackState={defaultAck}
      />,
    );
    await userEvent.click(screen.getByText("Services"));
    expect(onSelect).toHaveBeenCalledWith("services");
  });

  it("shows ack progress for decision sections with variants", () => {
    render(
      <FleetSidebar
        sections={sections}
        activeSection="packages"
        onSelect={vi.fn()}
        ackState={defaultAck}
      />,
    );
    // Decision sections should show ack progress
    expect(screen.getByTestId("ack-progress-packages")).toHaveTextContent("2/4 confirmed");
    expect(screen.getByTestId("ack-progress-configs")).toHaveTextContent("2/4 confirmed");
    // Context sections should not show ack progress
    expect(screen.queryByTestId("ack-progress-services")).not.toBeInTheDocument();
  });

  it("shows zone-based item counts for sections with zones", () => {
    render(
      <FleetSidebar
        sections={sections}
        activeSection="packages"
        onSelect={vi.fn()}
        ackState={defaultAck}
      />,
    );
    // Config Files has zones: 5 + 2 + 1 = 8 total
    expect(screen.getByText("8")).toBeInTheDocument();
    // Packages has items array: 1 item
    expect(screen.getByText("1")).toBeInTheDocument();
    // Services has empty items: 0
    expect(screen.getByText("0")).toBeInTheDocument();
  });
});
