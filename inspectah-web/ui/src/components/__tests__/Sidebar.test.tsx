import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Sidebar } from "../Sidebar";
import type { RefineStats, ContextSection, HealthResponse } from "../../api/types";

const MOCK_STATS: RefineStats = {
  total_packages: 42,
  included_packages: 38,
  excluded_packages: 4,
  total_configs: 15,
  included_configs: 12,
  package_managed_configs: 8,
  excluded_configs: 3,
  needs_review_count: 5,
  ops_applied: 2,
  can_undo: true,
  can_redo: false,
  baseline_available: true,
};

const MOCK_SECTIONS: ContextSection[] = [
  { id: "services", display_name: "Services", items: [{ id: "s1", title: "sshd", subtitle: null, detail: null, searchable_text: "sshd" }] },
  { id: "containers", display_name: "Containers", items: [] },
  { id: "users_groups", display_name: "Users & Groups", items: [{ id: "u1", title: "root", subtitle: null, detail: null, searchable_text: "root" }, { id: "u2", title: "nobody", subtitle: null, detail: null, searchable_text: "nobody" }] },
  { id: "network", display_name: "Network", items: [] },
  { id: "storage", display_name: "Storage", items: [] },
  { id: "scheduled_tasks", display_name: "Scheduled Tasks", items: [] },
  { id: "non_rpm_software", display_name: "Non-RPM Software", items: [] },
  { id: "kernel_boot", display_name: "Kernel & Boot", items: [] },
  { id: "selinux", display_name: "Security & Access Control", items: [] },
];

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
};

describe("Sidebar", () => {
  it("renders all 11 section items", () => {
    render(
      <Sidebar
        activeSection="packages"
        onSelect={vi.fn()}
        stats={MOCK_STATS}
        sections={MOCK_SECTIONS}
        health={MOCK_HEALTH}
      />,
    );

    expect(screen.getByText("Packages")).toBeInTheDocument();
    expect(screen.getByText("Config Files")).toBeInTheDocument();
    expect(screen.getByText("Services")).toBeInTheDocument();
    expect(screen.getByText("Containers")).toBeInTheDocument();
    expect(screen.getByText("Users & Groups")).toBeInTheDocument();
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

  it("shows context section item counts", () => {
    render(
      <Sidebar
        activeSection="packages"
        onSelect={vi.fn()}
        stats={MOCK_STATS}
        sections={MOCK_SECTIONS}
        health={MOCK_HEALTH}
      />,
    );

    // Services has 1 item, Users & Groups has 2
    expect(screen.getByText("1")).toBeInTheDocument();
    expect(screen.getByText("2")).toBeInTheDocument();
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
      host.compareDocumentPosition(nav) &
        Node.DOCUMENT_POSITION_FOLLOWING,
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
});
