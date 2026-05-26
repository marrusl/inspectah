/**
 * App-level router proof test (#2).
 *
 * Proves that when health returns a `fleet` field, App renders FleetApp
 * without ever mounting SingleHostApp or triggering /api/view fetch.
 */
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";

// Track whether fetchView was called — must be declared BEFORE vi.mock
const mockFetchView = vi.fn();
const mockFetchHealth = vi.fn();
const mockFetchSections = vi.fn();
const mockFetchFleetView = vi.fn();

vi.mock("../api/client", () => ({
  fetchHealth: (...args: unknown[]) => mockFetchHealth(...(args as [])),
  fetchView: (...args: unknown[]) => mockFetchView(...(args as [])),
  fetchSections: (...args: unknown[]) => mockFetchSections(...(args as [])),
  fetchViewed: vi.fn().mockResolvedValue({ ids: [] }),
  fetchOps: vi.fn().mockResolvedValue([]),
  applyOp: vi.fn().mockResolvedValue({}),
  undo: vi.fn().mockResolvedValue({}),
  redo: vi.fn().mockResolvedValue({}),
}));

vi.mock("../api/fleet-client", () => ({
  fetchFleetView: (...args: unknown[]) => mockFetchFleetView(...(args as [])),
  fetchFleetDiff: vi.fn().mockResolvedValue({}),
}));

// Stub fetch globally (for ExportDialog internals, etc.)
beforeEach(() => {
  vi.stubGlobal("fetch", vi.fn());
  mockFetchView.mockReset();
  mockFetchHealth.mockReset();
  mockFetchSections.mockReset();
  mockFetchFleetView.mockReset();
});

// Import App AFTER mocks are set up
import App from "../App";

describe("App router", () => {
  it("renders FleetApp when health reports fleet, never calls fetchView", async () => {
    // Health returns fleet metadata
    mockFetchHealth.mockResolvedValue({
      status: "ok",
      host: {
        hostname: "fleet-host",
        os_name: "RHEL",
        os_version: "9.4",
        os_id: "rhel",
        system_type: "physical",
        schema_version: 1,
      },
      completeness: "full",
      policy: { distro_repos: [] },
      fleet: {
        host_count: 3,
        hostnames: ["host1", "host2", "host3"],
        zones_active: true,
        variant_count: 5,
        label: "test-fleet",
        merged_at: "2025-01-01T00:00:00Z",
      },
      session_is_sensitive: false,
    });

    // Fleet view for FleetApp to render
    mockFetchFleetView.mockResolvedValue({
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
      ],
      repo_groups: [],
      repo_conflict_count: 0,
    });

    render(<App />);

    // Wait for FleetApp to render
    await waitFor(() => {
      expect(screen.getByTestId("fleet-app")).toBeInTheDocument();
    });

    // fetchView must NEVER have been called — SingleHostApp was never mounted
    expect(mockFetchView).not.toHaveBeenCalled();

    // fetchSections must NEVER have been called either
    expect(mockFetchSections).not.toHaveBeenCalled();

    // FleetApp's own fetch was called
    expect(mockFetchFleetView).toHaveBeenCalled();
  });

  it("renders SingleHostApp when health has no fleet field", async () => {
    mockFetchHealth.mockResolvedValue({
      status: "ok",
      host: {
        hostname: "single-host",
        os_name: "RHEL",
        os_version: "9.4",
        os_id: "rhel",
        system_type: "physical",
        schema_version: 1,
      },
      completeness: "full",
      policy: { distro_repos: [] },
      fleet: null,
      session_is_sensitive: false,
    });

    // SingleHostApp calls fetchView and fetchSections
    mockFetchView.mockResolvedValue({
      packages: [],
      config_files: [],
      containerfile_preview: "",
      stats: {
        total_packages: 0,
        included_packages: 0,
        excluded_packages: 0,
        total_configs: 0,
        included_configs: 0,
        package_managed_configs: 0,
        excluded_configs: 0,
        needs_review_count: 0,
        ops_applied: 0,
        can_undo: false,
        can_redo: false,
        baseline_available: false,
      },
      generation: 0,
      repo_groups: [],
      version_changes: [],
      service_states: [],
      service_dropins: [],
      quadlets: [],
      flatpaks: [],
      users_groups_decisions: [],
      session_is_sensitive: false,
    });

    mockFetchSections.mockResolvedValue([]);

    render(<App />);

    // Wait for SingleHostApp to render — it renders AppShell
    await waitFor(() => {
      expect(screen.getByTestId("app-shell")).toBeInTheDocument();
    });

    // fetchView WAS called — SingleHostApp mounted
    expect(mockFetchView).toHaveBeenCalled();

    // FleetApp was NOT rendered
    expect(screen.queryByTestId("fleet-app")).not.toBeInTheDocument();
  });
});
