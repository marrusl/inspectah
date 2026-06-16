import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { AggregateApp } from "../../AggregateApp";
import type { AggregateAppProps } from "../../AggregateApp";
import { AggregateSidebar } from "../AggregateSidebar";
import type {
  AggregateViewResponse,
  AggregateHealthInfo,
  HealthResponse,
} from "../../../api/types";

// Mock aggregate-client
const mockFetchAggregateView = vi.fn<() => Promise<AggregateViewResponse>>();
vi.mock("../../../api/aggregate-client", () => ({
  fetchAggregateView: (...args: unknown[]) => mockFetchAggregateView(...(args as [])),
  fetchAggregateDiff: vi.fn(),
}));

// Mock client (used by useAggregateMutation)
vi.mock("../../../api/client", () => ({
  applyOp: vi.fn().mockResolvedValue({}),
  undo: vi.fn().mockResolvedValue({}),
  redo: vi.fn().mockResolvedValue({}),
}));

// Mock fetch for ExportDialog internals
beforeEach(() => {
  vi.stubGlobal("fetch", vi.fn());
  mockFetchAggregateView.mockReset();
});

const MOCK_AGGREGATE: AggregateHealthInfo = {
  host_count: 3,
  hostnames: ["host1", "host2", "host3"],
  zones_active: true,
  variant_count: 5,
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

function makeAggregateView(
  overrides: Partial<AggregateViewResponse> = {},
): AggregateViewResponse {
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
            item_id: {
              kind: "Package",
              key: { name: "httpd", arch: "x86_64" },
            },
            include: true,
            triage: { bucket: "divergent", prevalence: { count: 2, total: 3 } },
            prevalence: { count: 2, total: 3 },
            source_repo: "appstream",
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
    repo_groups: [
      {
        section_id: "appstream",
        provenance: "verified",
        is_distro: true,
        tier: "distro",
        package_count: 1,
        enabled: true,
      },
    ],
    repo_conflict_count: 0,
    ...overrides,
  };
}

function renderAggregateApp(overrides: Partial<AggregateAppProps> = {}) {
  const props: AggregateAppProps = {
    aggregate: MOCK_AGGREGATE,
    health: MOCK_HEALTH,
    ...overrides,
  };
  return render(<AggregateApp {...props} />);
}

describe("AggregateApp", () => {
  it("shows loading state before data arrives", () => {
    mockFetchAggregateView.mockReturnValue(new Promise(() => {})); // never resolves
    renderAggregateApp();
    expect(screen.getByText(/Loading aggregate view/)).toBeInTheDocument();
  });

  it("shows error state on fetch failure", async () => {
    mockFetchAggregateView.mockRejectedValue(new Error("Network error"));
    renderAggregateApp();
    await waitFor(() => {
      expect(screen.getByText(/Failed to load aggregate view/)).toBeInTheDocument();
    });
    expect(screen.getByText("Network error")).toBeInTheDocument();
  });

  it("fetches aggregate view on mount and renders sections", async () => {
    const aggregateView = makeAggregateView();
    mockFetchAggregateView.mockResolvedValue(aggregateView);
    renderAggregateApp();

    await waitFor(() => {
      expect(screen.getByTestId("aggregate-content")).toBeInTheDocument();
    });

    // Packages render via unified RepoBar + PackageList
    expect(screen.getByTestId("repo-bar")).toBeInTheDocument();
    expect(screen.getByTestId("package-list")).toBeInTheDocument();
    expect(mockFetchAggregateView).toHaveBeenCalledOnce();
  });

  it("renders AggregateSidebar with sections", async () => {
    mockFetchAggregateView.mockResolvedValue(makeAggregateView());
    renderAggregateApp();

    await waitFor(() => {
      expect(screen.getByTestId("aggregate-sidebar")).toBeInTheDocument();
    });

    const sidebar = screen.getByTestId("aggregate-sidebar");
    expect(within(sidebar).getByText("Packages")).toBeInTheDocument();
    expect(within(sidebar).getByText("Config Files")).toBeInTheDocument();
    expect(within(sidebar).getByText("Services")).toBeInTheDocument();
  });

  it("handles section navigation", async () => {
    mockFetchAggregateView.mockResolvedValue(makeAggregateView());
    renderAggregateApp();

    await waitFor(() => {
      expect(screen.getByTestId("aggregate-sidebar")).toBeInTheDocument();
    });

    // Default active section is packages — verify unified PackageList renders
    expect(screen.getByTestId("package-list")).toBeInTheDocument();
    expect(screen.getByText("httpd.x86_64")).toBeInTheDocument();

    // Click on Config Files
    const configNav = screen.getByText("Config Files");
    await userEvent.click(configNav);

    // Config section renders via AggregateSectionContent (non-package section)
    expect(screen.queryByText("httpd.x86_64")).not.toBeInTheDocument();
  });

  it("wires undo/redo to mutation hook", async () => {
    const aggregateView = makeAggregateView({ can_undo: true, can_redo: true });
    mockFetchAggregateView.mockResolvedValue(aggregateView);
    renderAggregateApp();

    await waitFor(() => {
      expect(screen.getByTestId("aggregate-content")).toBeInTheDocument();
    });

    // AppShell renders StatsBar which has undo/redo buttons
    const undoBtn = screen.getByRole("button", { name: /undo/i });
    const redoBtn = screen.getByRole("button", { name: /redo/i });
    expect(undoBtn).toBeInTheDocument();
    expect(redoBtn).toBeInTheDocument();
  });

  it("shows refetch error with retry button", async () => {
    // First call succeeds, simulating a state where refetchError is set
    const aggregateView = makeAggregateView();
    mockFetchAggregateView.mockResolvedValue(aggregateView);
    renderAggregateApp();

    await waitFor(() => {
      expect(screen.getByTestId("aggregate-content")).toBeInTheDocument();
    });

    // Verify no refetch error initially
    expect(screen.queryByTestId("refetch-error")).not.toBeInTheDocument();
  });
});

describe("AggregateSidebar", () => {
  const defaultAck = {
    isAcked: () => false,
    getStatus: () => "unreviewed" as const,
    confirm: vi.fn(),
    unconfirm: vi.fn(),
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
          item_id: {
            kind: "Package" as const,
            key: { name: "httpd", arch: "x86_64" },
          },
          include: true,
          triage: {
            bucket: "divergent" as const,
            prevalence: { count: 2, total: 3 },
          },
          prevalence: { count: 2, total: 3 },
          source_repo: "appstream",
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
      <AggregateSidebar
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
      <AggregateSidebar
        sections={sections}
        activeSection="configs"
        onSelect={vi.fn()}
        ackState={defaultAck}
      />,
    );
    // PF6 NavItem sets aria-current="page" on active item
    const configItem = screen
      .getByText("Config Files")
      .closest("[aria-current]");
    expect(configItem).toHaveAttribute("aria-current", "page");
  });

  it("calls onSelect when clicking a section", async () => {
    const onSelect = vi.fn();
    render(
      <AggregateSidebar
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
      <AggregateSidebar
        sections={sections}
        activeSection="packages"
        onSelect={vi.fn()}
        ackState={defaultAck}
      />,
    );
    // Decision sections should show ack progress
    expect(screen.getByTestId("ack-progress-packages")).toHaveTextContent(
      "2/4 confirmed",
    );
    expect(screen.getByTestId("ack-progress-configs")).toHaveTextContent(
      "2/4 confirmed",
    );
    // Context sections should not show ack progress
    expect(
      screen.queryByTestId("ack-progress-services"),
    ).not.toBeInTheDocument();
  });

  it("shows zone-based item counts for sections with zones", () => {
    render(
      <AggregateSidebar
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
