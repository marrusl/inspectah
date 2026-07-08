import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Sidebar } from "../Sidebar";
import type {
  ReferenceSection,
  HealthResponse,
  ViewResponse,
  SectionGroupMeta,
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
  { id: "containers", display_name: "Containers", items: [] },
  { id: "network", display_name: "Network", items: [] },
  { id: "storage", display_name: "Storage", items: [] },
  { id: "scheduled_tasks", display_name: "Scheduled Tasks", items: [] },
  { id: "non_rpm_software", display_name: "Non-RPM Software", items: [] },
  { id: "kernel_boot", display_name: "Kernel & Boot", items: [] },
  { id: "selinux", display_name: "Security & Access Control", items: [] },
];

/** Groups matching the backend SectionGroup::all_in_order() output. */
const MOCK_GROUPS: SectionGroupMeta[] = [
  {
    slug: "packages-group",
    label: "Packages",
    sections: [{ id: "packages", label: "Packages", is_triage: true }],
    has_actionable_sections: true,
  },
  {
    slug: "system-config",
    label: "System Configuration",
    sections: [
      { id: "config", label: "Configuration Files", is_triage: true },
      { id: "kernel_boot", label: "Kernel & Boot", is_triage: false },
      { id: "selinux", label: "Security & Access Control", is_triage: false },
    ],
    has_actionable_sections: true,
  },
  {
    slug: "services-scheduling",
    label: "Services & Scheduling",
    sections: [
      { id: "services", label: "Services", is_triage: true },
      { id: "scheduled_tasks", label: "Scheduled Tasks", is_triage: false },
      { id: "containers", label: "Containers", is_triage: true },
    ],
    has_actionable_sections: true,
  },
  {
    slug: "identity",
    label: "Users & Identity",
    sections: [
      { id: "users_groups", label: "Users & Groups", is_triage: true },
    ],
    has_actionable_sections: true,
  },
  {
    slug: "network-group",
    label: "Network",
    sections: [{ id: "network", label: "Network", is_triage: false }],
    has_actionable_sections: false,
  },
  {
    slug: "storage-group",
    label: "Storage",
    sections: [{ id: "storage", label: "Storage", is_triage: false }],
    has_actionable_sections: false,
  },
  {
    slug: "software",
    label: "Software & Files",
    sections: [
      { id: "non_rpm_software", label: "Non-RPM Software", is_triage: true },
      { id: "unmanaged_files", label: "Unmanaged Files", is_triage: true },
    ],
    has_actionable_sections: true,
  },
  {
    slug: "secrets",
    label: "Secrets & Subscription",
    sections: [
      { id: "secrets", label: "Secrets", is_triage: false },
      { id: "subscription", label: "Subscription", is_triage: false },
    ],
    has_actionable_sections: false,
  },
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

// Mock localStorage for expand/collapse state persistence tests
const mockStorage: Record<string, string> = {};
const localStorageMock = {
  getItem: (key: string) => mockStorage[key] ?? null,
  setItem: (key: string, value: string) => {
    mockStorage[key] = value;
  },
  removeItem: (key: string) => {
    delete mockStorage[key];
  },
  clear: () => {
    for (const key of Object.keys(mockStorage)) delete mockStorage[key];
  },
  get length() {
    return Object.keys(mockStorage).length;
  },
  key: (_index: number) => null as string | null,
};

beforeEach(() => {
  localStorageMock.clear();
  vi.stubGlobal("localStorage", localStorageMock);
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("Sidebar", () => {
  it("renders 8 groups with all sections from API", () => {
    render(
      <Sidebar
        activeSection="packages"
        onSelect={vi.fn()}
        stats={MOCK_STATS}
        sections={MOCK_SECTIONS}
        health={MOCK_HEALTH}
        viewData={MOCK_VIEW_DATA}
        groups={MOCK_GROUPS}
      />,
    );

    // Singleton groups render their section label as a NavItem
    expect(screen.getByText("Packages")).toBeInTheDocument();
    expect(screen.getByText("Users & Groups")).toBeInTheDocument();

    // Multi-section groups render as expandable group headings
    expect(screen.getByText("System Configuration")).toBeInTheDocument();
    expect(screen.getByText("Services & Scheduling")).toBeInTheDocument();
    expect(screen.getByText("Software & Files")).toBeInTheDocument();
    expect(screen.getByText("Secrets & Subscription")).toBeInTheDocument();

    // Sections within multi-section groups
    expect(screen.getByText("Configuration Files")).toBeInTheDocument();
    expect(screen.getByText("Kernel & Boot")).toBeInTheDocument();
    expect(screen.getByText("Security & Access Control")).toBeInTheDocument();
    expect(screen.getByText("Services")).toBeInTheDocument();
    expect(screen.getByText("Scheduled Tasks")).toBeInTheDocument();
    expect(screen.getByText("Containers")).toBeInTheDocument();
    expect(screen.getByText("Non-RPM Software")).toBeInTheDocument();
    expect(screen.getByText("Unmanaged Files")).toBeInTheDocument();
    expect(screen.getByText("Secrets")).toBeInTheDocument();
    expect(screen.getByText("Subscription")).toBeInTheDocument();
  });

  it("does not render retired sections (system_tuning, version_changes)", () => {
    render(
      <Sidebar
        activeSection="packages"
        onSelect={vi.fn()}
        stats={MOCK_STATS}
        sections={MOCK_SECTIONS}
        health={MOCK_HEALTH}
        viewData={MOCK_VIEW_DATA}
        groups={MOCK_GROUPS}
      />,
    );

    expect(screen.queryByText("System Tuning")).not.toBeInTheDocument();
    expect(screen.queryByText("Version Changes")).not.toBeInTheDocument();
  });

  it("shows package and config counts from stats", () => {
    render(
      <Sidebar
        activeSection="packages"
        onSelect={vi.fn()}
        stats={MOCK_STATS}
        sections={MOCK_SECTIONS}
        health={MOCK_HEALTH}
        groups={MOCK_GROUPS}
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
        groups={MOCK_GROUPS}
      />,
    );

    // Services has 1 service_state in viewData
    expect(screen.getByText("1")).toBeInTheDocument();
    // Users & Groups decision count is 3
    expect(screen.getByText("3")).toBeInTheDocument();
    // Containers count: 0 quadlets + 0 flatpaks = 0
    const zeroBadges = screen.getAllByText("0");
    expect(zeroBadges.length).toBeGreaterThan(0);
  });

  it("shows skeleton placeholders when groups are loading", () => {
    render(
      <Sidebar
        activeSection="packages"
        onSelect={vi.fn()}
        stats={null}
        sections={null}
        health={null}
        groups={null}
      />,
    );

    // When groups is null, skeletons are shown instead of nav items
    expect(screen.queryByText("Packages")).not.toBeInTheDocument();
  });

  it("shows host info from health", () => {
    render(
      <Sidebar
        activeSection="packages"
        onSelect={vi.fn()}
        stats={MOCK_STATS}
        sections={MOCK_SECTIONS}
        health={MOCK_HEALTH}
        groups={MOCK_GROUPS}
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
        groups={MOCK_GROUPS}
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
        groups={MOCK_GROUPS}
      />,
    );

    expect(screen.getByText(/9\.4/)).toBeInTheDocument();
    const hostBlock = document.querySelector(".inspectah-sidebar__host");
    const strong = hostBlock?.querySelector("strong");
    expect(strong).toBeNull();
  });

  it("calls onSelect with section id when a nav item is clicked", async () => {
    const onSelect = vi.fn();
    render(
      <Sidebar
        activeSection="packages"
        onSelect={onSelect}
        stats={MOCK_STATS}
        sections={MOCK_SECTIONS}
        health={MOCK_HEALTH}
        groups={MOCK_GROUPS}
      />,
    );

    await userEvent.click(screen.getByText("Services"));
    expect(onSelect).toHaveBeenCalledWith("services");
  });

  it("passes API section id (config, not configs) on click", async () => {
    const onSelect = vi.fn();
    render(
      <Sidebar
        activeSection="packages"
        onSelect={onSelect}
        stats={MOCK_STATS}
        sections={MOCK_SECTIONS}
        health={MOCK_HEALTH}
        groups={MOCK_GROUPS}
      />,
    );

    await userEvent.click(screen.getByText("Configuration Files"));
    expect(onSelect).toHaveBeenCalledWith("config");
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
        groups={MOCK_GROUPS}
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
        groups={MOCK_GROUPS}
        hasUnmanagedScan={true}
      />,
    );
    expect(screen.queryByTestId("unmanaged-hint")).not.toBeInTheDocument();
  });

  it("renders singleton groups as plain NavItem (no group heading)", () => {
    render(
      <Sidebar
        activeSection="packages"
        onSelect={vi.fn()}
        stats={MOCK_STATS}
        sections={MOCK_SECTIONS}
        health={MOCK_HEALTH}
        groups={MOCK_GROUPS}
      />,
    );

    // Singleton groups: Packages, Users & Identity, Network, Storage
    // The "Packages" text should appear as a NavItem, not wrapped in a heading
    const packagesItem = screen.getByText("Packages").closest("a, button, li");
    expect(packagesItem).toBeTruthy();
  });

  it("renders multi-section groups as expandable", () => {
    render(
      <Sidebar
        activeSection="packages"
        onSelect={vi.fn()}
        stats={MOCK_STATS}
        sections={MOCK_SECTIONS}
        health={MOCK_HEALTH}
        groups={MOCK_GROUPS}
      />,
    );

    // NavExpandable renders a button with the group title
    const sysConfigBtn = screen
      .getByText("System Configuration")
      .closest("button");
    expect(sysConfigBtn).toBeTruthy();
  });

  it("expands all groups by default", () => {
    render(
      <Sidebar
        activeSection="packages"
        onSelect={vi.fn()}
        stats={MOCK_STATS}
        sections={MOCK_SECTIONS}
        health={MOCK_HEALTH}
        groups={MOCK_GROUPS}
      />,
    );

    // All child sections should be visible (expanded by default)
    expect(screen.getByText("Configuration Files")).toBeInTheDocument();
    expect(screen.getByText("Scheduled Tasks")).toBeInTheDocument();
    expect(screen.getByText("Secrets")).toBeInTheDocument();
  });

  it("persists collapsed state in localStorage", async () => {
    render(
      <Sidebar
        activeSection="packages"
        onSelect={vi.fn()}
        stats={MOCK_STATS}
        sections={MOCK_SECTIONS}
        health={MOCK_HEALTH}
        groups={MOCK_GROUPS}
      />,
    );

    // Click the System Configuration expandable to collapse it
    const sysConfigBtn = screen
      .getByText("System Configuration")
      .closest("button");
    expect(sysConfigBtn).toBeTruthy();
    await userEvent.click(sysConfigBtn!);

    // Verify localStorage was updated
    const stored = JSON.parse(
      localStorage.getItem("inspectah-sidebar-expanded") ?? "{}",
    );
    expect(stored["system-config"]).toBe(false);
  });

  it("restores collapsed state from localStorage", () => {
    localStorage.setItem(
      "inspectah-sidebar-expanded",
      JSON.stringify({ "system-config": false }),
    );

    render(
      <Sidebar
        activeSection="packages"
        onSelect={vi.fn()}
        stats={MOCK_STATS}
        sections={MOCK_SECTIONS}
        health={MOCK_HEALTH}
        groups={MOCK_GROUPS}
      />,
    );

    // System Configuration group should be collapsed.
    // NavExpandable renders the group title inside its toggle button
    // and sets aria-expanded on that button.
    const button = screen
      .getByText("System Configuration")
      .closest("button");
    expect(button).toBeTruthy();
    expect(button?.getAttribute("aria-expanded")).toBe("false");
  });

  it("counts subsection items when top-level items is empty", () => {
    const sectionsWithSubsections: ReferenceSection[] = [
      {
        id: "network",
        display_name: "Network",
        items: [],
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
    ];

    render(
      <Sidebar
        activeSection="packages"
        onSelect={vi.fn()}
        stats={MOCK_STATS}
        sections={sectionsWithSubsections}
        health={MOCK_HEALTH}
        groups={MOCK_GROUPS}
      />,
    );

    // Network section should show "3" (2 + 1 subsection items)
    expect(screen.getByText("3")).toBeInTheDocument();
  });
});
