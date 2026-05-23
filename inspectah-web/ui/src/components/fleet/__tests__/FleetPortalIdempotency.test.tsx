/**
 * Portal idempotency proof test (#3).
 *
 * Proves that calling the portal navigation (banner/search → expand variant)
 * twice with the same target keeps the variant view open (doesn't toggle shut).
 */
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, act } from "@testing-library/react";
import { FleetApp } from "../../FleetApp";
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
    setItem: (key: string, value: string) => { store[key] = value; },
    removeItem: (key: string) => { delete store[key]; },
    clear: () => { store = {}; },
    get length() { return Object.keys(store).length; },
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

const MOCK_FLEET: FleetHealthInfo = {
  host_count: 3,
  hostnames: ["host1", "host2", "host3"],
  zones_active: true,
  variant_count: 2,
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

function packageItemWithVariants(): FleetItem {
  return {
    item_id: { kind: "Package", key: { name_arch: "httpd.x86_64" } },
    include: true,
    attention: { level: "high", reason: "variant", prevalence: 2 },
    prevalence: { count: 2, total: 3 },
    variants: {
      count: 2,
      selected: "aaa111",
      options: [
        { hash: "aaa111", hosts: ["host1", "host2"], host_count: 2, selected: true },
        { hash: "bbb222", hosts: ["host3"], host_count: 1, selected: false },
      ],
    },
  };
}

function makeFleetViewWithVariants(): FleetViewResponse {
  const item = packageItemWithVariants();
  const actionable: ActionableVariantItem[] = [
    {
      item_id: item.item_id,
      section_id: "packages",
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
        items: [item],
      },
    ],
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
    const view = makeFleetViewWithVariants();
    mockFetchFleetView.mockResolvedValue(view);

    render(<FleetApp fleet={MOCK_FLEET} health={MOCK_HEALTH} />);

    // Wait for fleet content
    await waitFor(() => {
      expect(screen.getByTestId("fleet-content")).toBeInTheDocument();
    });

    // FleetBanner should be visible with actionable variants
    const banner = screen.getByTestId("fleet-banner");
    expect(banner).toBeInTheDocument();

    // Find the banner navigate button by its aria-label
    const navButton = screen.getByRole("button", { name: /navigate to httpd/i });
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
