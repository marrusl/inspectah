/**
 * Portal idempotency proof test (#3).
 *
 * Proves that calling the portal navigation (banner/search → expand variant)
 * twice with the same target keeps the variant view open (doesn't toggle shut).
 */
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { AggregateApp } from "../../AggregateApp";
import type {
  FleetViewResponse,
  FleetHealthInfo,
  HealthResponse,
  FleetItem,
  ActionableVariantItem,
} from "../../../api/types";

// ---------------------------------------------------------------------------
// Module mocks
// ---------------------------------------------------------------------------

const mockFetchFleetView = vi.fn<() => Promise<FleetViewResponse>>();
vi.mock("../../../api/fleet-client", () => ({
  fetchFleetView: (...args: unknown[]) => mockFetchFleetView(...(args as [])),
  fetchFleetDiff: vi.fn().mockResolvedValue({
    base_hash: "aaa",
    target_hash: "bbb",
    base_hosts: ["host1"],
    target_hosts: ["host2"],
    hunks: [],
    stats: { total_changes: 0, insertions: 0, deletions: 0 },
  }),
}));

vi.mock("../../../api/client", () => ({
  applyOp: vi.fn().mockResolvedValue({}),
  undo: vi.fn().mockResolvedValue({}),
  redo: vi.fn().mockResolvedValue({}),
}));

// ---------------------------------------------------------------------------
// localStorage stub
// ---------------------------------------------------------------------------

function createStorageStub(): Storage {
  let store: Record<string, string> = {};
  return {
    getItem: (key: string) => store[key] ?? null,
    setItem: (key: string, value: string) => {
      store[key] = value;
    },
    removeItem: (key: string) => {
      delete store[key];
    },
    clear: () => {
      store = {};
    },
    get length() {
      return Object.keys(store).length;
    },
    key: (index: number) => Object.keys(store)[index] ?? null,
  };
}

if (typeof globalThis.localStorage === "undefined") {
  Object.defineProperty(globalThis, "localStorage", {
    value: createStorageStub(),
    writable: true,
  });
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

const MOCK_AGGREGATE: FleetHealthInfo = {
  host_count: 3,
  hostnames: ["host1", "host2", "host3"],
  zones_active: true,
  variant_count: 2,
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

function configItemWithVariants(): FleetItem {
  return {
    item_id: { kind: "Config", key: { path: "/etc/httpd/conf/httpd.conf" } },
    include: true,
    triage: {
      bucket: "divergent" as const,
      prevalence: { count: 2, total: 3 },
    },
    prevalence: { count: 2, total: 3 },
    source_repo: "",
    variants: {
      count: 2,
      selected: "aaa111",
      options: [
        {
          hash: "aaa111",
          hosts: ["host1", "host2"],
          host_count: 2,
          selected: true,
        },
        { hash: "bbb222", hosts: ["host3"], host_count: 1, selected: false },
      ],
    },
  };
}

function makeAggregateViewWithVariants(): FleetViewResponse {
  const item = configItemWithVariants();
  const actionable: ActionableVariantItem[] = [
    {
      item_id: item.item_id,
      section_id: "config_files",
      variant_count: 2,
      max_host_spread: 2,
    },
  ];

  return {
    generation: 1,
    can_undo: false,
    can_redo: false,
    containerfile_preview: "FROM ubi9",
    session_is_sensitive: false,
    summary: {
      host_count: 3,
      actionable_variant_items: actionable,
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
        id: "config_files",
        display_name: "Config Files",
        is_decision_section: true,
        items: [item],
      },
    ],
    repo_groups: [],
    repo_conflict_count: 0,
  };
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

beforeEach(() => {
  vi.stubGlobal("fetch", vi.fn());
  mockFetchFleetView.mockReset();
  localStorage.clear();
});

describe("portal idempotency", () => {
  it("calling banner navigate twice with same target keeps variant view open", async () => {
    const view = makeAggregateViewWithVariants();
    mockFetchFleetView.mockResolvedValue(view);

    render(<AggregateApp aggregate={MOCK_AGGREGATE} health={MOCK_HEALTH} />);

    // Wait for aggregate content
    await waitFor(() => {
      expect(screen.getByTestId("aggregate-content")).toBeInTheDocument();
    });

    // Navigate to config_files section so the banner shows the config item
    const configNav = screen.getByText("Config Files");
    await userEvent.click(configNav);

    // AggregateBanner should be visible with actionable variants for this section
    const banner = screen.getByTestId("aggregate-banner");
    expect(banner).toBeInTheDocument();

    // Find the banner navigate button by its aria-label (config path)
    const navButton = screen.getByRole("button", {
      name: /navigate to \/etc\/httpd/i,
    });
    expect(navButton).toBeInTheDocument();

    // First click — should open the variant view via portal navigation
    await act(async () => {
      navButton.click();
    });

    // Wait for variant view to appear
    await waitFor(() => {
      expect(screen.getByTestId("variant-view")).toBeInTheDocument();
    });

    // Second click — same target, should keep variant view open (not toggle shut)
    await act(async () => {
      navButton.click();
    });

    // Variant view must still be visible — NOT toggled shut
    expect(screen.getByTestId("variant-view")).toBeInTheDocument();
  });
});
