import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Sidebar } from "../Sidebar";
import type {
  ReferenceSection,
  HealthResponse,
  ViewResponse,
} from "../../api/types";
import { mockStats } from "../../test-utils/mockStats";

const MOCK_STATS = mockStats({
  sections: [
    { kind: "package", total: 42, included: 38, excluded: 4 },
    { kind: "config", total: 15, included: 12, excluded: 3 },
  ],
  needs_review_count: 5,
  ops_applied: 2,
  can_undo: true,
  can_redo: false,
  baseline_available: true,
});

const MOCK_SECTIONS: ReferenceSection[] = [
  { id: "version_changes", display_name: "Version Changes", items: [] },
  { id: "containers", display_name: "Containers", items: [] },
  { id: "network", display_name: "Network", items: [] },
  { id: "storage", display_name: "Storage", items: [] },
  { id: "scheduled_tasks", display_name: "Scheduled Tasks", items: [] },
  { id: "non_rpm_software", display_name: "Non-RPM Software", items: [] },
  { id: "kernel_boot", display_name: "Kernel & Boot", items: [] },
  { id: "selinux", display_name: "Security & Access Control", items: [] },
];

/** Minimal ViewResponse for Sidebar badge counting. */
const MOCK_VIEW_DATA: ViewResponse = {
  packages: [],
  config_files: [],
  containerfile_preview: "",
  stats: MOCK_STATS,
  generation: 1,
  repo_groups: [],
  version_changes: [],
  service_states: [
    {
      unit: "sshd.service",
      triage: {
        triage: { mode: "single_host", baseline: null },
        primary_reason: "service_baseline_match",
        annotations: [],
      },
      include: true,
      current_state: "enabled",
    },
  ],
  service_dropins: [],
  quadlets: [],
  flatpaks: [],
  sysctls: [],
  tuned: [],
  users_groups_decisions: [],
  package_groups: [],
  session_is_sensitive: false,
};

const MOCK_HEALTH: HealthResponse = {
  status: "ok",
  host: {
    hostname: "testhost",
    os_name: "Red Hat Enterprise Linux",
    os_version: "9.4",
    os_id: "rhel",
    system_type: "rpm",
    schema_version: 1,
  },
  completeness: "full",
  policy: { distro_repos: ["baseos", "appstream"] },
  aggregate: null,
  session_is_sensitive: false,
};

describe("Sidebar", () => {
  it("renders all 14 section items", () => {
    render(
      <Sidebar
        activeSection="packages"
        onSelect={vi.fn()}
        stats={MOCK_STATS}
        sections={MOCK_SECTIONS}
        health={MOCK_HEALTH}
        viewData={MOCK_VIEW_DATA}
      />,
    );

    // Review (decision) sections
    expect(screen.getByText("Packages")).toBeInTheDocument();
    expect(screen.getByText("Config Files")).toBeInTheDocument();
    expect(screen.getByText("Users & Groups")).toBeInTheDocument();
    expect(screen.getByText("Services")).toBeInTheDocument();
    expect(screen.getByText("Containers")).toBeInTheDocument();
    expect(screen.getByText("System Tuning")).toBeInTheDocument();
    // Reference (context) sections
    expect(screen.getByText("Version Changes")).toBeInTheDocument();
    expect(screen.getByText("Compose")).toBeInTheDocument();
    expect(screen.getByText("Network")).toBeInTheDocument();
    expect(screen.getByText("Storage")).toBeInTheDocument();
    expect(screen.getByText("Scheduled Tasks")).toBeInTheDocument();
    expect(screen.getByText("Non-RPM Software")).toBeInTheDocument();
    expect(screen.getByText("Kernel & Boot")).toBeInTheDocument();
    expect(screen.getByText("Security & Access Control")).toBeInTheDocument();
  });

  it("shows package and config counts from stats", () => {
    render(
      <Sidebar
        activeSection="packages"
        onSelect={vi.fn()}
        stats={MOCK_STATS}
        sections={MOCK_SECTIONS}
        health={MOCK_HEALTH}
      />,
    );

    expect(screen.getByText("42")).toBeInTheDocument();
    expect(screen.getByText("15")).toBeInTheDocument();
  });

  it("shows decision and context section item counts", () => {
    render(
      <Sidebar
        activeSection="packages"
        onSelect={vi.fn()}
        stats={MOCK_STATS}
        sections={MOCK_SECTIONS}
        health={MOCK_HEALTH}
        viewData={MOCK_VIEW_DATA}
        userDecisionCount={3}
      />,
    );

    // Services has 1 service_state in viewData; Users & Groups decision count is 3
    expect(screen.getByText("1")).toBeInTheDocument();
    expect(screen.getByText("3")).toBeInTheDocument();
    // Containers count: 0 quadlets + 0 flatpaks = 0
    const zeroBadges = screen.getAllByText("0");
    expect(zeroBadges.length).toBeGreaterThan(0);
  });

  it("shows '...' when data is loading", () => {
    render(
      <Sidebar
        activeSection="packages"
        onSelect={vi.fn()}
        stats={null}
        sections={null}
        health={null}
      />,
    );

    const dots = screen.getAllByText("...");
    expect(dots.length).toBeGreaterThan(0);
  });

  it("shows host info from health", () => {
    render(
      <Sidebar
        activeSection="packages"
        onSelect={vi.fn()}
        stats={MOCK_STATS}
        sections={MOCK_SECTIONS}
        health={MOCK_HEALTH}
      />,
    );

    expect(screen.getByText(/testhost/)).toBeInTheDocument();
    expect(screen.getByText(/9\.4/)).toBeInTheDocument();
  });

  it("renders hostname above nav groups", () => {
    render(
      <Sidebar
        activeSection="packages"
        onSelect={vi.fn()}
        stats={MOCK_STATS}
        sections={MOCK_SECTIONS}
        health={MOCK_HEALTH}
      />,
    );

    const host = screen.getByText(/testhost/);
    const nav = screen.getByLabelText("Sections");
    expect(
      host.compareDocumentPosition(nav) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it("hides hostname line when hostname is empty", () => {
    const emptyHostHealth: HealthResponse = {
      ...MOCK_HEALTH,
      host: { ...MOCK_HEALTH.host, hostname: "" },
    };
    render(
      <Sidebar
        activeSection="packages"
        onSelect={vi.fn()}
        stats={MOCK_STATS}
        sections={MOCK_SECTIONS}
        health={emptyHostHealth}
      />,
    );

    // OS info should still render
    expect(screen.getByText(/9\.4/)).toBeInTheDocument();
    // No bold hostname element should be present
    const hostBlock = document.querySelector(".inspectah-sidebar__host");
    const strong = hostBlock?.querySelector("strong");
    expect(strong).toBeNull();
  });

  it("calls onSelect when a nav item is clicked", async () => {
    const onSelect = vi.fn();
    render(
      <Sidebar
        activeSection="packages"
        onSelect={onSelect}
        stats={MOCK_STATS}
        sections={MOCK_SECTIONS}
        health={MOCK_HEALTH}
      />,
    );

    await userEvent.click(screen.getByText("Services"));
    expect(onSelect).toHaveBeenCalledWith("services");
  });

  it("shows Language Packages in review group unconditionally", () => {
    render(
      <Sidebar
        activeSection="packages"
        onSelect={vi.fn()}
        stats={MOCK_STATS}
        sections={MOCK_SECTIONS}
        health={MOCK_HEALTH}
        viewData={MOCK_VIEW_DATA}
        hasUnmanagedScan={true}
      />,
    );
    expect(screen.getByText("Language Packages")).toBeInTheDocument();
  });

  it("shows Unmanaged Files section unconditionally", () => {
    render(
      <Sidebar
        activeSection="packages"
        onSelect={vi.fn()}
        stats={MOCK_STATS}
        sections={MOCK_SECTIONS}
        health={MOCK_HEALTH}
        viewData={MOCK_VIEW_DATA}
        hasUnmanagedScan={true}
      />,
    );
    expect(screen.getByText("Unmanaged Files")).toBeInTheDocument();
  });

  it("shows 0 count for Language Packages when no data", () => {
    render(
      <Sidebar
        activeSection="packages"
        onSelect={vi.fn()}
        stats={MOCK_STATS}
        sections={MOCK_SECTIONS}
        health={MOCK_HEALTH}
        viewData={MOCK_VIEW_DATA}
        hasUnmanagedScan={true}
      />,
    );
    expect(screen.getByText("Language Packages")).toBeInTheDocument();
    // Should show 0 badge when no language package data
  });

  it("shows discoverability hint when unmanaged scan was not used", () => {
    render(
      <Sidebar
        activeSection="packages"
        onSelect={vi.fn()}
        stats={MOCK_STATS}
        sections={MOCK_SECTIONS}
        health={MOCK_HEALTH}
        viewData={MOCK_VIEW_DATA}
        hasUnmanagedScan={false}
      />,
    );
    expect(screen.getByText(/Re-run with/)).toBeInTheDocument();
    expect(screen.getByTestId("unmanaged-hint")).toBeInTheDocument();
  });

  it("hides discoverability hint when unmanaged scan was used", () => {
    render(
      <Sidebar
        activeSection="packages"
        onSelect={vi.fn()}
        stats={MOCK_STATS}
        sections={MOCK_SECTIONS}
        health={MOCK_HEALTH}
        viewData={MOCK_VIEW_DATA}
        hasUnmanagedScan={true}
      />,
    );
    expect(screen.queryByTestId("unmanaged-hint")).not.toBeInTheDocument();
  });

  it("places Language Packages and Unmanaged Files after Containers", () => {
    render(
      <Sidebar
        activeSection="packages"
        onSelect={vi.fn()}
        stats={MOCK_STATS}
        sections={MOCK_SECTIONS}
        health={MOCK_HEALTH}
        viewData={MOCK_VIEW_DATA}
        hasLanguagePackages={true}
        hasUnmanagedFiles={true}
        hasUnmanagedScan={true}
      />,
    );
    const containers = screen.getByText("Containers");
    const langPkgs = screen.getByText("Language Packages");
    const unmanagedFiles = screen.getByText("Unmanaged Files");
    const systemTuning = screen.getByText("System Tuning");
    // Containers before Language Packages
    expect(
      containers.compareDocumentPosition(langPkgs) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    // Language Packages before Unmanaged Files
    expect(
      langPkgs.compareDocumentPosition(unmanagedFiles) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    // Unmanaged Files before System Tuning
    expect(
      unmanagedFiles.compareDocumentPosition(systemTuning) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it("counts subsection items when top-level items is empty", () => {
    const sectionsWithSubsections: ReferenceSection[] = [
      {
        id: "network",
        display_name: "Network",
        items: [], // Empty top-level items
        subsections: [
          {
            id: "network_interfaces",
            display_name: "Network Interfaces",
            items: [
              {
                id: "eth0",
                title: "eth0",
                subtitle: null,
                detail: null,
                searchable_text: "eth0",
              },
              {
                id: "eth1",
                title: "eth1",
                subtitle: null,
                detail: null,
                searchable_text: "eth1",
              },
            ],
          },
          {
            id: "firewall",
            display_name: "Firewall",
            items: [
              {
                id: "rule1",
                title: "rule1",
                subtitle: null,
                detail: null,
                searchable_text: "rule1",
              },
            ],
          },
        ],
      },
      { id: "containers", display_name: "Containers", items: [] },
    ];

    render(
      <Sidebar
        activeSection="packages"
        onSelect={vi.fn()}
        stats={MOCK_STATS}
        sections={sectionsWithSubsections}
        health={MOCK_HEALTH}
      />,
    );

    // Network section should show "3" (2 + 1 subsection items)
    expect(screen.getByText("3")).toBeInTheDocument();
  });

  it("uses top-level items count when items are present", () => {
    const sectionsWithTopLevelItems: ReferenceSection[] = [
      {
        id: "network",
        display_name: "Network",
        items: [
          {
            id: "eth0",
            title: "eth0",
            subtitle: null,
            detail: null,
            searchable_text: "eth0",
          },
        ],
        subsections: [], // Empty subsections
      },
      { id: "containers", display_name: "Containers", items: [] },
    ];

    render(
      <Sidebar
        activeSection="packages"
        onSelect={vi.fn()}
        stats={MOCK_STATS}
        sections={sectionsWithTopLevelItems}
        health={MOCK_HEALTH}
      />,
    );

    // Network section should show "1" from top-level items
    expect(screen.getByText("1")).toBeInTheDocument();
  });
});
