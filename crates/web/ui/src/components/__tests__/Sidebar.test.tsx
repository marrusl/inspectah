import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import {
  act,
  render,
  screen,
  fireEvent,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
// Rendered with a real pending guard, as its owner supplies one in the app.
import { SidebarWithGuard as Sidebar } from "../../test-utils/withPendingGuard";
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
      { id: "units", label: "Services", is_triage: true },
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
  unit_states: [
    {
      unit: "sshd.service",
      kind: "service",
      fragment: "vendor",
      triage: {
        triage: { mode: "single_host", baseline: null },
        primary_reason: "service_baseline_match",
        annotations: [],
      },
      include: true,
      locked: false,
      current_state: "enabled",
      required_config: [],
      data_paths: [],
    },
  ],
  unit_dropins: [],
  mount_findings: [],
  manual_links: [],
  undetermined_installs: [],
  quadlets: [],
  flatpaks: [],
  declined_generated: [],
  generated_timers: [],
  sysctl: { state: "unavailable", reason: "not collected" },
  tuned: [],
  users_groups_decisions: [],
  package_groups: [],
  session_is_sensitive: false,
  not_carried_packages: [],
  not_carried_notice: null,
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

  it("gives every section entry link semantics and keyboard activation", async () => {
    const onSelect = vi.fn();
    render(
      <Sidebar
        activeSection="packages"
        onSelect={onSelect}
        stats={MOCK_STATS}
        sections={MOCK_SECTIONS}
        health={MOCK_HEALTH}
        viewData={MOCK_VIEW_DATA}
        groups={MOCK_GROUPS}
      />,
    );
    const nav = screen.getByRole("navigation", { name: "Sections" });
    const sections = MOCK_GROUPS.flatMap((g) => g.sections);
    for (const sec of sections) {
      const link = within(nav).getByRole("link", {
        name: new RegExp(`^${sec.label.replace(/[&]/g, "\\$&")} `),
      });
      link.focus();
      expect(link).toHaveFocus();
      await userEvent.keyboard("{Enter}");
      expect(onSelect).toHaveBeenLastCalledWith(sec.id);
    }
    expect(onSelect).toHaveBeenCalledTimes(sections.length);
    // The hash is never followed: selection happens in place.
    expect(window.location.hash).toBe("");
  });

  it("lists System Tuning with a badge counting tuned rows only", () => {
    const groups = MOCK_GROUPS.map((g) =>
      g.slug === "system-config"
        ? {
            ...g,
            sections: [
              ...g.sections.slice(0, 2),
              { id: "system_tuning", label: "System Tuning", is_triage: true },
              ...g.sections.slice(2),
            ],
          }
        : g,
    );
    const tuned = {
      active_profile: "throughput-performance",
      custom_profiles: [],
      triage: {
        triage: { mode: "single_host" as const, baseline: null },
        primary_reason: "tuned_non_default_profile" as const,
        annotations: [],
      },
      include: true,
      locked: false,
    };
    const { rerender } = render(
      <Sidebar
        activeSection="packages"
        onSelect={vi.fn()}
        stats={MOCK_STATS}
        sections={MOCK_SECTIONS}
        health={MOCK_HEALTH}
        viewData={{ ...MOCK_VIEW_DATA, tuned: [tuned] }}
        groups={groups}
      />,
    );

    const entry = screen.getByRole("link", { name: /^System Tuning / });
    expect(within(entry).getByLabelText("1 decisions")).toBeInTheDocument();

    // A host with sysctl files and no tuned selection is cleared at 0.
    rerender(
      <Sidebar
        activeSection="packages"
        onSelect={vi.fn()}
        stats={MOCK_STATS}
        sections={MOCK_SECTIONS}
        health={MOCK_HEALTH}
        viewData={MOCK_VIEW_DATA}
        groups={groups}
      />,
    );
    expect(within(entry).getByLabelText("0 decisions")).toBeInTheDocument();
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

  describe("units section count", () => {
    const TRIAGE = {
      triage: { mode: "single_host" as const, baseline: null },
      primary_reason: "service_baseline_match" as const,
      annotations: [],
    };

    function renderUnits(viewData: ViewResponse) {
      render(
        <Sidebar
          activeSection="packages"
          onSelect={vi.fn()}
          stats={MOCK_STATS}
          sections={MOCK_SECTIONS}
          health={MOCK_HEALTH}
          viewData={viewData}
          groups={MOCK_GROUPS}
        />,
      );
      return screen.getByRole("link", { name: /^Services / });
    }

    it("announces an empty units section as cleared", () => {
      // Control: the same query finds the announcement when nothing is left.
      const entry = renderUnits({
        ...MOCK_VIEW_DATA,
        unit_states: [],
        unit_dropins: [],
      });
      expect(within(entry).getByLabelText("0 decisions")).toBeInTheDocument();
      expect(
        screen.getByText("Services: 0 decisions remaining"),
      ).toBeInTheDocument();
    });

    it("counts the default target, the only decision in a target-only view", () => {
      const entry = renderUnits({
        ...MOCK_VIEW_DATA,
        unit_states: [],
        unit_dropins: [],
        default_target: {
          target: "graphical.target",
          include: true,
          locked: false,
          declined: null,
        },
      });
      expect(within(entry).getByLabelText("1 decisions")).toBeInTheDocument();
      expect(
        screen.queryByText("Services: 0 decisions remaining"),
      ).not.toBeInTheDocument();
    });

    it("counts no target in the same view without one", () => {
      const entry = renderUnits({
        ...MOCK_VIEW_DATA,
        unit_states: [],
        unit_dropins: [],
      });
      expect(within(entry).getByLabelText("0 decisions")).toBeInTheDocument();
    });

    it("counts a full-shadow drop-in when the unit has no state row", () => {
      // A unit at its preset whose only change is a full shadow has no
      // state row: its one decision is the drop-in toggle.
      const entry = renderUnits({
        ...MOCK_VIEW_DATA,
        unit_states: [],
        unit_dropins: [
          {
            unit: "sshd.service",
            path: "/etc/systemd/system/sshd.service",
            triage: TRIAGE,
            include: true,
            locked: false,
            shadow_type: "full_shadow",
            shadow_rationale: null,
          },
        ],
      });
      expect(within(entry).getByLabelText("1 decisions")).toBeInTheDocument();
      expect(
        screen.queryByText("Services: 0 decisions remaining"),
      ).not.toBeInTheDocument();
    });

    it("counts a state row and its drop-in as two decisions", () => {
      // Each renders its own toggle, and Include all sends an op for each.
      const entry = renderUnits({
        ...MOCK_VIEW_DATA,
        unit_states: [
          {
            unit: "httpd.service",
            kind: "service",
            fragment: "vendor",
            triage: TRIAGE,
            include: true,
            locked: false,
            current_state: "enabled",
            required_config: [],
            data_paths: [],
          },
        ],
        unit_dropins: [
          {
            unit: "httpd.service",
            path: "/etc/systemd/system/httpd.service.d/override.conf",
            triage: TRIAGE,
            include: true,
            locked: false,
            shadow_type: "drop_in",
          },
        ],
      });
      expect(within(entry).getByLabelText("2 decisions")).toBeInTheDocument();
      expect(
        screen.queryByText("Services: 0 decisions remaining"),
      ).not.toBeInTheDocument();
    });

    it.each(["unit_states", "unit_dropins"])(
      "fails on a view without %s instead of counting it as empty",
      (field) => {
        // The tests above show the same view counted when the field is
        // present. A required field the server stops sending must not read
        // as an empty section.
        vi.spyOn(console, "error").mockImplementation(() => {});
        const view = { ...MOCK_VIEW_DATA } as Record<string, unknown>;
        delete view[field];
        expect(() => renderUnits(view as unknown as ViewResponse)).toThrow(
          TypeError,
        );
      },
    );

    it("does not count a mount finding, which has no toggle", () => {
      // The count beside it above shows a row with a toggle is counted.
      const entry = renderUnits({
        ...MOCK_VIEW_DATA,
        unit_states: [],
        unit_dropins: [],
        mount_findings: [
          {
            unit: "srv.mount",
            kind: "mount",
            state: "enabled",
            mount_point: "/srv",
            fragment: "local",
            unit_file_path: "/etc/systemd/system/srv.mount",
            advisories: ["# Advisory: srv.mount is enabled on the source host."],
          },
        ],
      });
      expect(within(entry).getByLabelText("0 decisions")).toBeInTheDocument();
      expect(
        screen.getByText("Services: 0 decisions remaining"),
      ).toBeInTheDocument();
    });
  });

  describe("Scheduled Tasks count (U29, spec § 9.4)", () => {
    /** The groups as the server sends them: Scheduled Tasks is a triage section. */
    const TRIAGE_GROUPS: SectionGroupMeta[] = MOCK_GROUPS.map((g) => ({
      ...g,
      sections: g.sections.map((s) =>
        s.id === "scheduled_tasks" ? { ...s, is_triage: true } : s,
      ),
    }));

    function item(id: string, generatedTimer?: string) {
      return {
        id,
        title: id,
        subtitle: null,
        detail: null,
        searchable_text: id,
        ...(generatedTimer === undefined ? {} : { generated_timer: generatedTimer }),
      };
    }

    /** Three generated-timer rows, one of them in a subsection, and a cron row without one. */
    const SCHEDULED: ReferenceSection = {
      id: "scheduled_tasks",
      display_name: "Scheduled Tasks",
      items: [item("timer-a", "cron-a"), item("timer-b", "cron-b"), item("cron-plain")],
      subsections: [
        { id: "user", display_name: "User", items: [item("timer-c", "cron-c")] },
      ],
    };
    const SECTIONS = MOCK_SECTIONS.map((s) => (s.id === "scheduled_tasks" ? SCHEDULED : s));

    const TIMERS = ["cron-a", "cron-b", "cron-c"].map((name) => ({
      name,
      include: true,
      locked: false,
    }));

    function renderScheduled(
      sections: ReferenceSection[] | null,
      viewData: ViewResponse | null,
    ) {
      render(
        <Sidebar
          activeSection="packages"
          onSelect={vi.fn()}
          stats={MOCK_STATS}
          sections={sections}
          health={MOCK_HEALTH}
          viewData={viewData}
          groups={TRIAGE_GROUPS}
        />,
      );
      return screen.getByRole("link", { name: /^Scheduled Tasks / });
    }

    it("counts each row whose generated timer the view holds", () => {
      const entry = renderScheduled(SECTIONS, { ...MOCK_VIEW_DATA, generated_timers: TIMERS });
      expect(within(entry).getByLabelText("3 decisions")).toBeInTheDocument();
    });

    it("leaves out a row excluded with its cron job, which shows a reason in place of a toggle", () => {
      const timers = TIMERS.map((t) =>
        t.name === "cron-b" ? { ...t, include: false, cron_job_excluded: "cron-b-job" } : t,
      );
      const entry = renderScheduled(SECTIONS, { ...MOCK_VIEW_DATA, generated_timers: timers });
      expect(within(entry).getByLabelText("2 decisions")).toBeInTheDocument();
    });

    it("counts an excluded row with no cron job excluded, which keeps its toggle", () => {
      const timers = TIMERS.map((t) => (t.name === "cron-b" ? { ...t, include: false } : t));
      const entry = renderScheduled(SECTIONS, { ...MOCK_VIEW_DATA, generated_timers: timers });
      expect(within(entry).getByLabelText("3 decisions")).toBeInTheDocument();
    });

    it("does not count a row whose generated timer the view does not hold", () => {
      const entry = renderScheduled(SECTIONS, {
        ...MOCK_VIEW_DATA,
        generated_timers: TIMERS.filter((t) => t.name !== "cron-c"),
      });
      expect(within(entry).getByLabelText("2 decisions")).toBeInTheDocument();
    });

    it.each<[string, ReferenceSection[] | null, ViewResponse | null]>([
      ["the sections", null, { ...MOCK_VIEW_DATA, generated_timers: TIMERS }],
      ["the view", SECTIONS, null],
    ])("reads ... before %s load", (_name, sections, viewData) => {
      const entry = renderScheduled(sections, viewData);
      expect(within(entry).getByLabelText("... decisions")).toBeInTheDocument();
    });

    it("reads 0 with no generated-timer row", () => {
      const entry = renderScheduled(MOCK_SECTIONS, { ...MOCK_VIEW_DATA, generated_timers: TIMERS });
      expect(within(entry).getByLabelText("0 decisions")).toBeInTheDocument();
    });
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
    expect(onSelect).toHaveBeenCalledWith("units");
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

  describe("keyboard navigation", () => {
    it("auto-expands collapsed group when activeSection changes to its child", () => {
      // Start with system-config collapsed
      localStorage.setItem(
        "inspectah-sidebar-expanded",
        JSON.stringify({ "system-config": false }),
      );

      const { rerender } = render(
        <Sidebar
          activeSection="packages"
          onSelect={vi.fn()}
          stats={MOCK_STATS}
          sections={MOCK_SECTIONS}
          health={MOCK_HEALTH}
          groups={MOCK_GROUPS}
        />,
      );

      // Verify collapsed
      const button = screen
        .getByText("System Configuration")
        .closest("button");
      expect(button?.getAttribute("aria-expanded")).toBe("false");

      // Simulate keyboard navigation to a section in the collapsed group
      rerender(
        <Sidebar
          activeSection="config"
          onSelect={vi.fn()}
          stats={MOCK_STATS}
          sections={MOCK_SECTIONS}
          health={MOCK_HEALTH}
          groups={MOCK_GROUPS}
        />,
      );

      // Group should auto-expand
      expect(button?.getAttribute("aria-expanded")).toBe("true");

      // localStorage should be updated
      const stored = JSON.parse(
        localStorage.getItem("inspectah-sidebar-expanded") ?? "{}",
      );
      expect(stored["system-config"]).toBe(true);
    });

    it("does not auto-expand already-expanded groups", () => {
      const setItemSpy = vi.spyOn(Storage.prototype, "setItem");

      render(
        <Sidebar
          activeSection="config"
          onSelect={vi.fn()}
          stats={MOCK_STATS}
          sections={MOCK_SECTIONS}
          health={MOCK_HEALTH}
          groups={MOCK_GROUPS}
        />,
      );

      // system-config is expanded by default; no localStorage write needed
      const writes = setItemSpy.mock.calls.filter(
        ([key]) => key === "inspectah-sidebar-expanded",
      );
      expect(writes.length).toBe(0);
    });

    it("does not auto-expand for singleton group sections", () => {
      localStorage.setItem(
        "inspectah-sidebar-expanded",
        JSON.stringify({}),
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

      // Singleton groups have no expandable parent — nothing to auto-expand
      // Just verify the item renders correctly
      expect(screen.getByText("Packages")).toBeInTheDocument();
    });

    it("preserves aria-current on active child when parent is collapsed", async () => {
      render(
        <Sidebar
          activeSection="config"
          onSelect={vi.fn()}
          stats={MOCK_STATS}
          sections={MOCK_SECTIONS}
          health={MOCK_HEALTH}
          groups={MOCK_GROUPS}
        />,
      );

      // config NavItem should have aria-current="page"
      const configLink = screen.getByText("Configuration Files").closest("a");
      expect(configLink?.getAttribute("aria-current")).toBe("page");

      // Collapse the group
      const toggleBtn = screen
        .getByText("System Configuration")
        .closest("button");
      await userEvent.click(toggleBtn!);

      // Note: auto-expand will re-expand because activeSection="config".
      // The key invariant is that aria-current is driven by activeSection
      // prop, not by expanded state. Verify the attribute is still set.
      expect(configLink?.getAttribute("aria-current")).toBe("page");
    });

    it("sets aria-current only on active NavItem, never on group heading", () => {
      render(
        <Sidebar
          activeSection="config"
          onSelect={vi.fn()}
          stats={MOCK_STATS}
          sections={MOCK_SECTIONS}
          health={MOCK_HEALTH}
          groups={MOCK_GROUPS}
        />,
      );

      // Active section has aria-current
      const configLink = screen.getByText("Configuration Files").closest("a");
      expect(configLink?.getAttribute("aria-current")).toBe("page");

      // Group heading button should NOT have aria-current
      const groupBtn = screen
        .getByText("System Configuration")
        .closest("button");
      expect(groupBtn?.getAttribute("aria-current")).toBeNull();
    });

    it("renders singleton NavItem with aria-current when active", () => {
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

      const packagesLink = screen.getByText("Packages").closest("a");
      expect(packagesLink?.getAttribute("aria-current")).toBe("page");
    });
  });

  describe("badge differentiation", () => {
    it("renders blue badges (default) for triage sections", () => {
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

      // Triage section badges should NOT have pf-m-read class (blue/unread state)
      const packageBadge = screen.getByLabelText("42 decisions");
      expect(packageBadge).toBeInTheDocument();
      expect(packageBadge).not.toHaveClass("pf-m-read");

      const configBadge = screen.getByLabelText("15 decisions");
      expect(configBadge).toBeInTheDocument();
      expect(configBadge).not.toHaveClass("pf-m-read");
    });

    it("renders grey badges (isRead) for reference sections", () => {
      render(
        <Sidebar
          activeSection="network"
          onSelect={vi.fn()}
          stats={MOCK_STATS}
          sections={MOCK_SECTIONS}
          health={MOCK_HEALTH}
          viewData={MOCK_VIEW_DATA}
          groups={MOCK_GROUPS}
        />,
      );

      // Reference section badges should have pf-m-read class (grey/read state)
      const referenceBadges = screen.getAllByLabelText("0 items");
      expect(referenceBadges.length).toBeGreaterThan(0);
      referenceBadges.forEach((badge) => {
        expect(badge).toHaveClass("pf-m-read");
      });
    });

    it("uses correct aria-labels for triage sections", () => {
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

      // Triage badges: "{count} decisions"
      expect(screen.getByLabelText("42 decisions")).toHaveTextContent("42");
      expect(screen.getByLabelText("15 decisions")).toHaveTextContent("15");
    });

    it("uses correct aria-labels for reference sections", () => {
      render(
        <Sidebar
          activeSection="network"
          onSelect={vi.fn()}
          stats={MOCK_STATS}
          sections={MOCK_SECTIONS}
          health={MOCK_HEALTH}
          viewData={MOCK_VIEW_DATA}
          groups={MOCK_GROUPS}
        />,
      );

      // Reference badges: "{count} items"
      const itemBadges = screen.getAllByLabelText("0 items");
      expect(itemBadges.length).toBeGreaterThan(0);
    });

    it("shows cleared-state '0' badge with aria-live for triage sections", () => {
      const emptyStats = mockStats({
        sections: [
          { kind: "package", total: 0, included: 0, excluded: 0 },
          { kind: "config", total: 0, included: 0, excluded: 0 },
        ],
        needs_review_count: 0,
        ops_applied: 0,
        can_undo: false,
        can_redo: false,
        baseline_available: false,
      });

      const emptyViewData: ViewResponse = {
        ...MOCK_VIEW_DATA,
        unit_states: [],
        quadlets: [],
        flatpaks: [],
        declined_generated: [],
        generated_timers: [],
      };

      render(
        <Sidebar
          activeSection="packages"
          onSelect={vi.fn()}
          stats={emptyStats}
          sections={MOCK_SECTIONS}
          health={MOCK_HEALTH}
          viewData={emptyViewData}
          userDecisionCount={0}
          groups={MOCK_GROUPS}
        />,
      );

      // Should show "0" badges for triage sections
      const decisionBadges = screen.getAllByLabelText("0 decisions");
      expect(decisionBadges.length).toBeGreaterThan(0);
      decisionBadges.forEach((badge) => {
        expect(badge).toHaveTextContent("0");
      });

      // Should have aria-live announcement for cleared triage section
      expect(
        screen.getByText("Packages: 0 decisions remaining"),
      ).toBeInTheDocument();

      // Should also show for config section
      expect(
        screen.getByText("Configuration Files: 0 decisions remaining"),
      ).toBeInTheDocument();
    });

    it("does not show aria-live for reference sections with 0 count", () => {
      render(
        <Sidebar
          activeSection="network"
          onSelect={vi.fn()}
          stats={MOCK_STATS}
          sections={MOCK_SECTIONS}
          health={MOCK_HEALTH}
          viewData={MOCK_VIEW_DATA}
          groups={MOCK_GROUPS}
        />,
      );

      // Reference sections with 0 items should exist
      const zeroBadges = screen.getAllByLabelText("0 items");
      expect(zeroBadges.length).toBeGreaterThan(0);

      // Should NOT have aria-live announcement for reference sections
      // (Network, Storage, Kernel & Boot, etc. are all reference sections)
      expect(
        screen.queryByText("Network: 0 decisions remaining"),
      ).not.toBeInTheDocument();
      expect(
        screen.queryByText("Storage: 0 decisions remaining"),
      ).not.toBeInTheDocument();
      expect(
        screen.queryByText("Kernel & Boot: 0 decisions remaining"),
      ).not.toBeInTheDocument();
    });

    it("does not show aria-live for triage sections with non-zero count", () => {
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

      // Triage section with items
      expect(screen.getByLabelText("42 decisions")).toBeInTheDocument();

      // Should NOT have aria-live announcement (only when count is 0)
      expect(
        screen.queryByText("Packages: 0 decisions remaining"),
      ).not.toBeInTheDocument();
    });

    it("aria-live announcement has correct screen reader class", () => {
      const emptyStats = mockStats({
        sections: [{ kind: "package", total: 0, included: 0, excluded: 0 }],
        needs_review_count: 0,
        ops_applied: 0,
        can_undo: false,
        can_redo: false,
        baseline_available: false,
      });

      render(
        <Sidebar
          activeSection="packages"
          onSelect={vi.fn()}
          stats={emptyStats}
          sections={MOCK_SECTIONS}
          health={MOCK_HEALTH}
          viewData={MOCK_VIEW_DATA}
          groups={MOCK_GROUPS}
        />,
      );

      const announcement = screen.getByText("Packages: 0 decisions remaining");
      expect(announcement).toHaveClass("pf-v6-screen-reader");
      expect(announcement.closest("span")?.getAttribute("aria-live")).toBe(
        "polite",
      );
    });
  });

  describe("batch-toggle kebab menu", () => {
    const onBatchToggle = vi.fn(() => Promise.resolve());

    beforeEach(() => {
      onBatchToggle.mockClear();
    });

    it("shows kebab menu on triage sections when onBatchToggle is provided", () => {
      render(
        <Sidebar
          activeSection="packages"
          onSelect={vi.fn()}
          stats={MOCK_STATS}
          sections={MOCK_SECTIONS}
          health={MOCK_HEALTH}
          viewData={MOCK_VIEW_DATA}
          groups={MOCK_GROUPS}
          onBatchToggle={onBatchToggle}
        />,
      );

      // Triage singleton groups should have kebab (except Identity)
      expect(screen.getByTestId("kebab-packages-group")).toBeInTheDocument();

      // Per-section kebabs on triage sections within multi-section groups
      expect(screen.getByTestId("kebab-config")).toBeInTheDocument();
      expect(screen.getByTestId("kebab-units")).toBeInTheDocument();

      // Non-triage sections should NOT have kebab
      expect(screen.queryByTestId("kebab-kernel_boot")).not.toBeInTheDocument();
    });

    it("suppresses kebab menu on Identity group (deferred #8)", () => {
      render(
        <Sidebar
          activeSection="users_groups"
          onSelect={vi.fn()}
          stats={MOCK_STATS}
          sections={MOCK_SECTIONS}
          health={MOCK_HEALTH}
          viewData={MOCK_VIEW_DATA}
          groups={MOCK_GROUPS}
          onBatchToggle={onBatchToggle}
        />,
      );

      // Identity group should NOT have kebab (batch-toggle handler only covers groups, not individual users)
      expect(screen.queryByTestId("kebab-identity")).not.toBeInTheDocument();
    });

    it("does not show kebab menu on reference-only groups", () => {
      render(
        <Sidebar
          activeSection="packages"
          onSelect={vi.fn()}
          stats={MOCK_STATS}
          sections={MOCK_SECTIONS}
          health={MOCK_HEALTH}
          viewData={MOCK_VIEW_DATA}
          groups={MOCK_GROUPS}
          onBatchToggle={onBatchToggle}
        />,
      );

      // Reference-only groups should NOT have kebab
      expect(
        screen.queryByTestId("kebab-network-group"),
      ).not.toBeInTheDocument();
      expect(
        screen.queryByTestId("kebab-storage-group"),
      ).not.toBeInTheDocument();
      expect(screen.queryByTestId("kebab-secrets")).not.toBeInTheDocument();
    });

    it("does not show kebab menu when onBatchToggle is not provided", () => {
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

      expect(
        screen.queryByTestId("kebab-packages-group"),
      ).not.toBeInTheDocument();
      expect(
        screen.queryByTestId("kebab-system-config"),
      ).not.toBeInTheDocument();
    });

    it("opens dropdown with Include all and Exclude all items", async () => {
      render(
        <Sidebar
          activeSection="config"
          onSelect={vi.fn()}
          stats={MOCK_STATS}
          sections={MOCK_SECTIONS}
          health={MOCK_HEALTH}
          viewData={MOCK_VIEW_DATA}
          groups={MOCK_GROUPS}
          onBatchToggle={onBatchToggle}
        />,
      );

      // Click the per-section config kebab
      await userEvent.click(screen.getByTestId("kebab-config"));

      // Menu items should appear
      expect(screen.getByText("Include all")).toBeInTheDocument();
      expect(screen.getByText("Exclude all")).toBeInTheDocument();
    });

    it("calls onBatchToggle with section id on Include all click", async () => {
      render(
        <Sidebar
          activeSection="config"
          onSelect={vi.fn()}
          stats={MOCK_STATS}
          sections={MOCK_SECTIONS}
          health={MOCK_HEALTH}
          viewData={MOCK_VIEW_DATA}
          groups={MOCK_GROUPS}
          onBatchToggle={onBatchToggle}
        />,
      );

      await userEvent.click(screen.getByTestId("kebab-config"));
      await userEvent.click(screen.getByRole("menuitem", { name: "Include all" }));

      expect(onBatchToggle).toHaveBeenCalledWith("config", true);
    });

    it("calls onBatchToggle with section id on Exclude all click", async () => {
      render(
        <Sidebar
          activeSection="config"
          onSelect={vi.fn()}
          stats={MOCK_STATS}
          sections={MOCK_SECTIONS}
          health={MOCK_HEALTH}
          viewData={MOCK_VIEW_DATA}
          groups={MOCK_GROUPS}
          onBatchToggle={onBatchToggle}
        />,
      );

      await userEvent.click(screen.getByTestId("kebab-config"));
      await userEvent.click(screen.getByRole("menuitem", { name: "Exclude all" }));

      expect(onBatchToggle).toHaveBeenCalledWith("config", false);
    });

    it("shows confirmation dialog when excluding packages", async () => {
      render(
        <Sidebar
          activeSection="packages"
          onSelect={vi.fn()}
          stats={MOCK_STATS}
          sections={MOCK_SECTIONS}
          health={MOCK_HEALTH}
          viewData={MOCK_VIEW_DATA}
          groups={MOCK_GROUPS}
          onBatchToggle={onBatchToggle}
        />,
      );

      await userEvent.click(screen.getByTestId("kebab-packages-group"));
      await userEvent.click(screen.getByRole("menuitem", { name: "Exclude all" }));

      // Should NOT call onBatchToggle yet
      expect(onBatchToggle).not.toHaveBeenCalled();

      // Confirmation dialog should appear
      expect(screen.getByTestId("packages-confirm-modal")).toBeInTheDocument();
      expect(
        screen.getByText(/critical runtime dependencies/),
      ).toBeInTheDocument();
    });

    it("calls onBatchToggle after confirming packages exclusion", async () => {
      render(
        <Sidebar
          activeSection="packages"
          onSelect={vi.fn()}
          stats={MOCK_STATS}
          sections={MOCK_SECTIONS}
          health={MOCK_HEALTH}
          viewData={MOCK_VIEW_DATA}
          groups={MOCK_GROUPS}
          onBatchToggle={onBatchToggle}
        />,
      );

      await userEvent.click(screen.getByTestId("kebab-packages-group"));
      await userEvent.click(screen.getByRole("menuitem", { name: "Exclude all" }));

      // Confirm
      await userEvent.click(screen.getByTestId("packages-confirm-yes"));

      expect(onBatchToggle).toHaveBeenCalledWith("packages-group", false);
    });

    it("does not call onBatchToggle when canceling packages exclusion", async () => {
      render(
        <Sidebar
          activeSection="packages"
          onSelect={vi.fn()}
          stats={MOCK_STATS}
          sections={MOCK_SECTIONS}
          health={MOCK_HEALTH}
          viewData={MOCK_VIEW_DATA}
          groups={MOCK_GROUPS}
          onBatchToggle={onBatchToggle}
        />,
      );

      await userEvent.click(screen.getByTestId("kebab-packages-group"));
      await userEvent.click(screen.getByRole("menuitem", { name: "Exclude all" }));

      // Cancel
      await userEvent.click(screen.getByText("Cancel"));

      expect(onBatchToggle).not.toHaveBeenCalled();
      expect(
        screen.queryByTestId("packages-confirm-modal"),
      ).not.toBeInTheDocument();
    });

    it("announces toggle completion via aria-live", async () => {
      render(
        <Sidebar
          activeSection="config"
          onSelect={vi.fn()}
          stats={MOCK_STATS}
          sections={MOCK_SECTIONS}
          health={MOCK_HEALTH}
          viewData={MOCK_VIEW_DATA}
          groups={MOCK_GROUPS}
          onBatchToggle={onBatchToggle}
        />,
      );

      await userEvent.click(screen.getByTestId("kebab-config"));
      await userEvent.click(screen.getByRole("menuitem", { name: "Include all" }));

      // Wait for the async handler to complete
      const announceEl = await screen.findByTestId("batch-toggle-announce");
      expect(announceEl).toHaveTextContent(
        "All items included in Configuration Files",
      );
      expect(announceEl.getAttribute("aria-live")).toBe("assertive");
    });

    it("names the default target as outside the units bulk toggle", async () => {
      const withTarget: ViewResponse = {
        ...MOCK_VIEW_DATA,
        default_target: {
          target: "graphical.target",
          include: true,
          locked: false,
          declined: null,
        },
      };
      render(
        <Sidebar
          activeSection="units"
          onSelect={vi.fn()}
          stats={MOCK_STATS}
          sections={MOCK_SECTIONS}
          health={MOCK_HEALTH}
          viewData={withTarget}
          groups={MOCK_GROUPS}
          onBatchToggle={onBatchToggle}
        />,
      );

      await userEvent.click(screen.getByTestId("kebab-units"));
      await userEvent.click(
        screen.getByRole("menuitem", { name: "Exclude all except the default target" }),
      );

      expect(onBatchToggle).toHaveBeenCalledWith("units", false);
      const announceEl = await screen.findByTestId("batch-toggle-announce");
      await vi.waitFor(() =>
        expect(announceEl).toHaveTextContent(
          "All items except the default target excluded from Services",
        ),
      );
      expect(announceEl.textContent).not.toMatch(/^All items excluded/);
    });

    it("keeps the plain units bulk labels when the view has no default target", async () => {
      render(
        <Sidebar
          activeSection="units"
          onSelect={vi.fn()}
          stats={MOCK_STATS}
          sections={MOCK_SECTIONS}
          health={MOCK_HEALTH}
          viewData={MOCK_VIEW_DATA}
          groups={MOCK_GROUPS}
          onBatchToggle={onBatchToggle}
        />,
      );

      await userEvent.click(screen.getByTestId("kebab-units"));
      await userEvent.click(screen.getByRole("menuitem", { name: "Exclude all" }));
      const announceEl = await screen.findByTestId("batch-toggle-announce");
      await vi.waitFor(() =>
        expect(announceEl).toHaveTextContent("All items excluded from Services"),
      );
    });

    it("does not include packages in safety gate for Include all", async () => {
      render(
        <Sidebar
          activeSection="packages"
          onSelect={vi.fn()}
          stats={MOCK_STATS}
          sections={MOCK_SECTIONS}
          health={MOCK_HEALTH}
          viewData={MOCK_VIEW_DATA}
          groups={MOCK_GROUPS}
          onBatchToggle={onBatchToggle}
        />,
      );

      await userEvent.click(screen.getByTestId("kebab-packages-group"));
      await userEvent.click(screen.getByRole("menuitem", { name: "Include all" }));

      // Include all on Packages should NOT trigger confirmation
      expect(
        screen.queryByTestId("packages-confirm-modal"),
      ).not.toBeInTheDocument();
      expect(onBatchToggle).toHaveBeenCalledWith("packages-group", true);
    });

    it("has accessible kebab toggle labels", () => {
      render(
        <Sidebar
          activeSection="packages"
          onSelect={vi.fn()}
          stats={MOCK_STATS}
          sections={MOCK_SECTIONS}
          health={MOCK_HEALTH}
          viewData={MOCK_VIEW_DATA}
          groups={MOCK_GROUPS}
          onBatchToggle={onBatchToggle}
        />,
      );

      expect(
        screen.getByLabelText("Actions for Packages"),
      ).toBeInTheDocument();
      // Per-section labels on multi-section groups
      expect(
        screen.getByLabelText("Actions for Configuration Files"),
      ).toBeInTheDocument();
      // Identity has no kebab menu (deferred #8)
      expect(
        screen.queryByLabelText("Actions for Users & Identity"),
      ).not.toBeInTheDocument();
    });

    // axe aria-required-children and listitem: a list may hold only list
    // items, so a row with a kebab is one <li> holding both controls.
    it("keeps every nav list made of list items when kebabs are shown", () => {
      const { container } = render(
        <Sidebar
          activeSection="packages"
          onSelect={vi.fn()}
          stats={MOCK_STATS}
          sections={MOCK_SECTIONS}
          health={MOCK_HEALTH}
          viewData={MOCK_VIEW_DATA}
          groups={MOCK_GROUPS}
          onBatchToggle={onBatchToggle}
        />,
      );

      const lists = container.querySelectorAll("nav ul");
      expect(lists.length).toBeGreaterThan(0);
      for (const list of lists) {
        for (const child of list.children) {
          expect(child.tagName).toBe("LI");
        }
      }

      for (const [label, kebabId] of [
        ["Packages", "kebab-packages-group"],
        ["Configuration Files", "kebab-config"],
      ]) {
        const row = screen.getByTestId(kebabId).closest("li");
        expect(row?.parentElement?.tagName).toBe("UL");
        expect(
          within(row as HTMLElement).getByRole("link", { name: new RegExp(label) }),
        ).toBeInTheDocument();
      }
    });

    it("tabs from a section link to its kebab, then to the next section", async () => {
      render(
        <Sidebar
          activeSection="packages"
          onSelect={vi.fn()}
          stats={MOCK_STATS}
          sections={MOCK_SECTIONS}
          health={MOCK_HEALTH}
          viewData={MOCK_VIEW_DATA}
          groups={MOCK_GROUPS}
          onBatchToggle={onBatchToggle}
        />,
      );

      screen.getByRole("link", { name: /^Configuration Files/ }).focus();
      await userEvent.tab();
      expect(screen.getByTestId("kebab-config")).toHaveFocus();
      await userEvent.tab();
      expect(screen.getByRole("link", { name: /^Kernel & Boot/ })).toHaveFocus();
    });
  });

  describe("Ctrl+Shift+A/X target filtering", () => {
    const onBatchToggle = vi.fn(() => Promise.resolve());

    beforeEach(() => {
      onBatchToggle.mockClear();
    });

    it("fires Ctrl+Shift+A from per-section NavItem within a multi-section group", () => {
      render(
        <Sidebar
          activeSection="config"
          onSelect={vi.fn()}
          stats={MOCK_STATS}
          sections={MOCK_SECTIONS}
          health={MOCK_HEALTH}
          viewData={MOCK_VIEW_DATA}
          groups={MOCK_GROUPS}
          onBatchToggle={onBatchToggle}
        />,
      );

      // Focus a child NavItem link inside the system-config group
      const configLink = screen.getByText("Configuration Files").closest("a");
      expect(configLink).toBeTruthy();
      configLink!.focus();

      // Fire Ctrl+Shift+A — should trigger the per-section batch toggle
      fireEvent.keyDown(configLink!, { key: "a", ctrlKey: true, shiftKey: true });

      expect(onBatchToggle).toHaveBeenCalledWith("config", true);
    });

    it("fires Ctrl+Shift+X from per-section NavItem within a multi-section group", () => {
      render(
        <Sidebar
          activeSection="units"
          onSelect={vi.fn()}
          stats={MOCK_STATS}
          sections={MOCK_SECTIONS}
          health={MOCK_HEALTH}
          viewData={MOCK_VIEW_DATA}
          groups={MOCK_GROUPS}
          onBatchToggle={onBatchToggle}
        />,
      );

      // Focus a child NavItem link inside the services-scheduling group
      const servicesLink = screen.getByText("Services").closest("a");
      expect(servicesLink).toBeTruthy();
      servicesLink!.focus();

      fireEvent.keyDown(servicesLink!, { key: "x", ctrlKey: true, shiftKey: true });

      expect(onBatchToggle).toHaveBeenCalledWith("units", false);
    });

    it("does NOT fire Ctrl+Shift+A from group heading button (no actionable wrapper)", () => {
      render(
        <Sidebar
          activeSection="config"
          onSelect={vi.fn()}
          stats={MOCK_STATS}
          sections={MOCK_SECTIONS}
          health={MOCK_HEALTH}
          viewData={MOCK_VIEW_DATA}
          groups={MOCK_GROUPS}
          onBatchToggle={onBatchToggle}
        />,
      );

      // Focus the group heading button (has aria-expanded)
      const headingBtn = screen
        .getByText("System Configuration")
        .closest("button");
      expect(headingBtn).toBeTruthy();
      headingBtn!.focus();

      fireEvent.keyDown(headingBtn!, { key: "a", ctrlKey: true, shiftKey: true });

      // Group heading no longer has an actionable wrapper — per-section only
      expect(onBatchToggle).not.toHaveBeenCalled();
    });

    it("fires Ctrl+Shift+A from singleton triage NavItem", () => {
      render(
        <Sidebar
          activeSection="packages"
          onSelect={vi.fn()}
          stats={MOCK_STATS}
          sections={MOCK_SECTIONS}
          health={MOCK_HEALTH}
          viewData={MOCK_VIEW_DATA}
          groups={MOCK_GROUPS}
          onBatchToggle={onBatchToggle}
        />,
      );

      // Focus the singleton Packages NavItem link
      const packagesLink = screen.getByText("Packages").closest("a");
      expect(packagesLink).toBeTruthy();
      packagesLink!.focus();

      fireEvent.keyDown(packagesLink!, { key: "a", ctrlKey: true, shiftKey: true });

      expect(onBatchToggle).toHaveBeenCalledWith("packages-group", true);
    });
  });
});

/**
 * A batch toggle goes out directly, not through App's useMutation. The
 * Ctrl+Shift+A/X shortcut reaches it without the kebab, so disabling the
 * kebab alone does not stop a repeat.
 */
describe("Sidebar batch toggle pending guard", () => {
  function renderSidebar(
    onBatchToggle: (slug: string, include: boolean) => Promise<void>,
  ) {
    render(
      <Sidebar
        activeSection="packages"
        onSelect={vi.fn()}
        stats={MOCK_STATS}
        sections={MOCK_SECTIONS}
        health={MOCK_HEALTH}
        viewData={MOCK_VIEW_DATA}
        groups={MOCK_GROUPS}
        onBatchToggle={onBatchToggle}
      />,
    );
  }

  const includeAll = () => {
    const link = screen.getByText("Packages").closest("a")!;
    link.focus();
    fireEvent.keyDown(link, { key: "a", ctrlKey: true, shiftKey: true });
  };

  it("sends one batch toggle when triggered twice", () => {
    const onBatchToggle = vi.fn(() => new Promise<void>(() => {}));
    renderSidebar(onBatchToggle);

    act(() => {
      includeAll();
      includeAll();
    });

    expect(onBatchToggle).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("kebab-packages-group")).toBeDisabled();
  });

  it("re-enables the group after the batch toggle fails", async () => {
    let fail: (err: Error) => void = () => {};
    const onBatchToggle = vi.fn(
      () =>
        new Promise<void>((_resolve, reject) => {
          fail = reject;
        }),
    );
    renderSidebar(onBatchToggle);
    const kebab = screen.getByTestId("kebab-packages-group");

    act(() => includeAll());
    expect(kebab).toBeDisabled();

    fail(new Error("connection reset"));
    await waitFor(() => expect(kebab).toBeEnabled());

    act(() => includeAll());
    expect(onBatchToggle).toHaveBeenCalledTimes(2);
  });
});
