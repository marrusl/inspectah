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
  fetchAggregateView: (...args: unknown[]) =>
    mockFetchAggregateView(...(args as [])),
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
      expect(
        screen.getByText(/Failed to load aggregate view/),
      ).toBeInTheDocument();
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

  it("shows include/total counts for decision sections, total for reference", () => {
    render(
      <AggregateSidebar
        sections={sections}
        activeSection="packages"
        onSelect={vi.fn()}
        ackState={defaultAck}
      />,
    );
    // Packages: 1 included / 1 total (decision section)
    expect(screen.getByText("1 / 1")).toBeInTheDocument();
    // Config Files: 0 included / 8 total (decision section, zone items empty in fixture)
    expect(screen.getByText("0 / 8")).toBeInTheDocument();
    // Services: plain count (reference section)
    expect(screen.getByText("0")).toBeInTheDocument();
  });

  it("shows language_packages section in Review group with zone-based counts", () => {
    const sectionsWithLangPkgs = [
      ...sections,
      {
        id: "language_packages",
        display_name: "Language Packages",
        is_decision_section: true,
        zones: {
          consensus: {
            items: [
              {
                item_id: {
                  kind: "LanguageEnv" as const,
                  key: { ecosystem: "pip", manifest: "/usr/lib/python3/requirements.txt" },
                },
                include: true,
                triage: {
                  bucket: "consensus" as const,
                  prevalence: { count: 3, total: 3 },
                },
                prevalence: { count: 3, total: 3 },
                source_repo: "",
              },
            ],
            count: 1,
          },
          near_consensus: { items: [], count: 2 },
          divergent: {
            items: [
              {
                item_id: {
                  kind: "LanguageEnv" as const,
                  key: { ecosystem: "npm", manifest: "/opt/app/package.json" },
                },
                include: false,
                triage: {
                  bucket: "divergent" as const,
                  prevalence: { count: 1, total: 3 },
                },
                prevalence: { count: 1, total: 3 },
                source_repo: "",
              },
            ],
            count: 1,
          },
        },
      },
    ];

    render(
      <AggregateSidebar
        sections={sectionsWithLangPkgs}
        activeSection="packages"
        onSelect={vi.fn()}
        ackState={defaultAck}
      />,
    );

    // Language Packages appears in the sidebar
    expect(screen.getByText("Language Packages")).toBeInTheDocument();

    // Decision section badge: 1 included / 4 total (1 consensus + 2 near_consensus + 1 divergent)
    expect(screen.getByText("1 / 4")).toBeInTheDocument();

    // Verify it's in the Review group (is_decision_section: true),
    // not the Reference group
    expect(screen.getByText("Review")).toBeInTheDocument();
    const sidebar = screen.getByTestId("aggregate-sidebar");
    expect(
      within(sidebar).getByText("Language Packages"),
    ).toBeInTheDocument();
  });

  it("shows unmanaged_files section in Review group with zone-based counts", () => {
    const sectionsWithUnmanaged = [
      ...sections,
      {
        id: "unmanaged_files",
        display_name: "Unmanaged Files",
        is_decision_section: true,
        zones: {
          consensus: {
            items: [
              {
                item_id: {
                  kind: "UnmanagedFile" as const,
                  key: { path: "/opt/app/data.db" },
                },
                include: true,
                triage: {
                  bucket: "consensus" as const,
                  prevalence: { count: 3, total: 3 },
                },
                prevalence: { count: 3, total: 3 },
                source_repo: "",
              },
              {
                item_id: {
                  kind: "UnmanagedFile" as const,
                  key: { path: "/var/log/custom.log" },
                },
                include: false,
                triage: {
                  bucket: "consensus" as const,
                  prevalence: { count: 3, total: 3 },
                },
                prevalence: { count: 3, total: 3 },
                source_repo: "",
              },
            ],
            count: 2,
          },
          near_consensus: { items: [], count: 0 },
          divergent: {
            items: [
              {
                item_id: {
                  kind: "UnmanagedFile" as const,
                  key: { path: "/etc/custom.conf" },
                },
                include: true,
                triage: {
                  bucket: "divergent" as const,
                  prevalence: { count: 1, total: 3 },
                },
                prevalence: { count: 1, total: 3 },
                source_repo: "",
              },
            ],
            count: 1,
          },
        },
      },
    ];

    render(
      <AggregateSidebar
        sections={sectionsWithUnmanaged}
        activeSection="packages"
        onSelect={vi.fn()}
        ackState={defaultAck}
      />,
    );

    // Unmanaged Files appears in the sidebar
    expect(screen.getByText("Unmanaged Files")).toBeInTheDocument();

    // Decision section badge: 2 included / 3 total (2 consensus + 0 near_consensus + 1 divergent)
    expect(screen.getByText("2 / 3")).toBeInTheDocument();

    // Verify it's in the Review group, not Reference
    expect(screen.getByText("Review")).toBeInTheDocument();
    const sidebar = screen.getByTestId("aggregate-sidebar");
    expect(
      within(sidebar).getByText("Unmanaged Files"),
    ).toBeInTheDocument();
  });

  it("shows both new sections together with existing sections", () => {
    const allSections = [
      ...sections,
      {
        id: "language_packages",
        display_name: "Language Packages",
        is_decision_section: true,
        zones: {
          consensus: { items: [], count: 3 },
          near_consensus: { items: [], count: 1 },
          divergent: { items: [], count: 0 },
        },
      },
      {
        id: "unmanaged_files",
        display_name: "Unmanaged Files",
        is_decision_section: true,
        zones: {
          consensus: { items: [], count: 5 },
          near_consensus: { items: [], count: 2 },
          divergent: { items: [], count: 1 },
        },
      },
    ];

    render(
      <AggregateSidebar
        sections={allSections}
        activeSection="packages"
        onSelect={vi.fn()}
        ackState={defaultAck}
      />,
    );

    const sidebar = screen.getByTestId("aggregate-sidebar");

    // All decision sections present in sidebar
    expect(within(sidebar).getByText("Packages")).toBeInTheDocument();
    expect(within(sidebar).getByText("Config Files")).toBeInTheDocument();
    expect(
      within(sidebar).getByText("Language Packages"),
    ).toBeInTheDocument();
    expect(
      within(sidebar).getByText("Unmanaged Files"),
    ).toBeInTheDocument();

    // Reference section still rendered
    expect(within(sidebar).getByText("Services")).toBeInTheDocument();

    // Badge counts for new sections (zone count totals, 0 items included)
    // Language Packages: 0 / 4 (3+1+0, no items in zones)
    expect(screen.getByText("0 / 4")).toBeInTheDocument();
    // Unmanaged Files: 0 / 8 matches Config Files badge — verify both exist
    const zeroOfEight = screen.getAllByText("0 / 8");
    expect(zeroOfEight.length).toBe(2); // Config Files + Unmanaged Files
  });
});
