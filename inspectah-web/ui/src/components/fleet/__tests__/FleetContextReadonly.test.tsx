/**
 * Context read-only proof test (#4).
 *
 * Proves that clicking a context-section item (is_decision_section === false)
 * does NOT open the editable VariantView.
 */
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { FleetApp } from "../../FleetApp";
import type {
  FleetViewResponse,
  FleetHealthInfo,
  HealthResponse,
} from "../../../api/types";

// ---------------------------------------------------------------------------
// Module mocks
// ---------------------------------------------------------------------------

const mockFetchFleetView = vi.fn<() => Promise<FleetViewResponse>>();
vi.mock("../../../api/fleet-client", () => ({
  fetchFleetView: (...args: unknown[]) => mockFetchFleetView(...(args as [])),
  fetchFleetDiff: vi.fn().mockResolvedValue({}),
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

const MOCK_FLEET: FleetHealthInfo = {
  host_count: 3,
  hostnames: ["host1", "host2", "host3"],
  zones_active: false,
  variant_count: 0,
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

function makeViewWithReferenceSection(): FleetViewResponse {
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
        items: [],
      },
      {
        id: "services",
        display_name: "Services",
        is_decision_section: false,
        items: [
          {
            item_id: { kind: "Service", key: { unit: "httpd.service" } },
            include: true,
            triage: {
              bucket: "universal" as const,
              prevalence: { count: 3, total: 3 },
            },
            prevalence: { count: 3, total: 3 },
            source_repo: "",
            variants: {
              count: 2,
              selected: "aaa",
              options: [
                {
                  hash: "aaa",
                  hosts: ["host1", "host2"],
                  host_count: 2,
                  selected: true,
                },
                {
                  hash: "bbb",
                  hosts: ["host3"],
                  host_count: 1,
                  selected: false,
                },
              ],
            },
          },
        ],
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

describe("context section read-only behavior", () => {
  it("clicking a context-section item does not open VariantView", async () => {
    mockFetchFleetView.mockResolvedValue(makeViewWithReferenceSection());
    const user = userEvent.setup();

    render(<FleetApp fleet={MOCK_FLEET} health={MOCK_HEALTH} />);

    await waitFor(() => {
      expect(screen.getByTestId("fleet-content")).toBeInTheDocument();
    });

    // Default active section is packages (empty). Navigate to Services.
    await user.click(screen.getByText("Services"));

    // Verify the item row renders
    const itemRow = screen.getByTestId("fleet-item-row");
    expect(itemRow).toBeInTheDocument();
    expect(screen.getByText("httpd.service")).toBeInTheDocument();

    // The item has variants but is_decision_section is false,
    // so the variants button should show as readonly (span, not button)
    const variantButton = itemRow.querySelector(
      "button.fleet-item-row__variants",
    );
    expect(variantButton).toBeNull(); // No clickable variant button

    // Click the row itself
    await user.click(itemRow);

    // VariantView must NOT appear
    expect(screen.queryByTestId("variant-view")).not.toBeInTheDocument();

    // No toggle switch should be present for context sections
    const toggleSwitch = itemRow.querySelector(".fleet-item-row__toggle");
    expect(toggleSwitch).toBeNull();
  });
});
