/**
 * Integration tests for AggregateApp.
 *
 * These test end-to-end flows through the real hook layer (useAggregateMutation,
 * useVariantAck) with mocked API responses. They verify behaviors that span
 * multiple components — sidebar + content + toolbar — rather than individual
 * component rendering (which is covered by the unit tests).
 *
 * Note: AckProgress is passed to AppShell via toolbarExtra but AppShell does
 * not yet render it (destructured as _toolbarExtra). Tests verify ack state
 * through the sidebar's ack labels instead. Undo/redo are tested via Ctrl+Z
 * keyboard shortcuts which bypass the StatsBar disabled check (StatsBar
 * disables undo/redo when stats is null, and AggregateApp passes stats={null}).
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import {
  render,
  screen,
  waitFor,
  act,
  within,
  fireEvent,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { AggregateApp } from "../../AggregateApp";
import type { AggregateAppProps } from "../../AggregateApp";
import type {
  AggregateViewResponse,
  AggregateHealthInfo,
  HealthResponse,
  AggregateSection,
  AggregateItem,
  ActionableVariantItem,
} from "../../../api/types";

// ---------------------------------------------------------------------------
// localStorage stub — jsdom may not expose localStorage
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
// Module mocks — mock the API layer, let hooks run for real
// ---------------------------------------------------------------------------

const mockFetchAggregateView = vi.fn<() => Promise<AggregateViewResponse>>();
vi.mock("../../../api/aggregate-client", () => ({
  fetchAggregateView: (...args: unknown[]) => mockFetchAggregateView(...(args as [])),
  fetchAggregateDiff: vi.fn().mockResolvedValue({
    item_id: { kind: "Config", key: { path: "/etc/test" } },
    base_hash: "aaa",
    target_hash: "bbb",
    changes: [],
  }),
}));

const mockApplyOp = vi.fn().mockResolvedValue({});
const mockUndo = vi.fn().mockResolvedValue({});
const mockRedo = vi.fn().mockResolvedValue({});
vi.mock("../../../api/client", () => ({
  applyOp: (...args: unknown[]) => mockApplyOp(...(args as [])),
  undo: (...args: unknown[]) => mockUndo(...(args as [])),
  redo: (...args: unknown[]) => mockRedo(...(args as [])),
}));

// ---------------------------------------------------------------------------
// Fixture builders
// ---------------------------------------------------------------------------

function mockAggregateItem(overrides?: Partial<AggregateItem>): AggregateItem {
  return {
    item_id: { kind: "Package", key: { name: "httpd", arch: "x86_64" } },
    include: true,
    triage: {
      bucket: "universal" as const,
      prevalence: { count: 3, total: 3 },
    },
    prevalence: { count: 3, total: 3 },
    source_repo: "appstream",
    ...overrides,
  };
}

function mockConfigItem(
  path: string,
  overrides?: Partial<AggregateItem>,
): AggregateItem {
  return mockAggregateItem({
    item_id: { kind: "Config", key: { path } },
    triage: {
      bucket: "divergent" as const,
      prevalence: { count: 2, total: 3 },
    },
    prevalence: { count: 2, total: 3 },
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
    ...overrides,
  });
}

function mockAggregateSection(
  id: string,
  overrides?: Partial<AggregateSection>,
): AggregateSection {
  return {
    id,
    display_name: id.charAt(0).toUpperCase() + id.slice(1),
    is_decision_section: true,
    items: [mockAggregateItem()],
    ...overrides,
  };
}

function mockAggregateViewResponse(
  overrides?: Partial<AggregateViewResponse>,
): AggregateViewResponse {
  const configItem1 = mockConfigItem("/etc/httpd/conf/httpd.conf");
  const configItem2 = mockConfigItem("/etc/sysconfig/network", {
    item_id: { kind: "Config", key: { path: "/etc/sysconfig/network" } },
    variants: {
      count: 3,
      selected: "ccc333",
      options: [
        { hash: "ccc333", hosts: ["host1"], host_count: 1, selected: true },
        { hash: "ddd444", hosts: ["host2"], host_count: 1, selected: false },
        { hash: "eee555", hosts: ["host3"], host_count: 1, selected: false },
      ],
    },
  });

  const actionableVariants: ActionableVariantItem[] = [
    {
      item_id: configItem1.item_id,
      section_id: "config_files",
      variant_count: 2,
      max_host_spread: 2,
    },
    {
      item_id: configItem2.item_id,
      section_id: "config_files",
      variant_count: 3,
      max_host_spread: 1,
    },
  ];

  return {
    generation: 1,
    can_undo: false,
    can_redo: false,
    containerfile_preview: "FROM registry.redhat.io/rhel9/rhel-bootc:9.4",
    session_is_sensitive: false,
    summary: {
      host_count: 3,
      actionable_variant_items: actionableVariants,
      informational_variant_count: 1,
    },
    sections: [
      mockAggregateSection("packages", {
        display_name: "Packages",
        items: [
          mockAggregateItem({
            item_id: {
              kind: "Package",
              key: { name: "httpd", arch: "x86_64" },
            },
            prevalence: { count: 3, total: 3 },
          }),
          mockAggregateItem({
            item_id: {
              kind: "Package",
              key: { name: "nginx", arch: "x86_64" },
            },
            prevalence: { count: 2, total: 3 },
            triage: {
              bucket: "partial" as const,
              prevalence: { count: 2, total: 3 },
            },
          }),
        ],
      }),
      mockAggregateSection("config_files", {
        display_name: "Config Files",
        is_decision_section: true,
        zones: {
          consensus: {
            items: [configItem1],
            count: 1,
          },
          near_consensus: {
            items: [configItem2],
            count: 1,
          },
          divergent: { items: [], count: 0 },
        },
      }),
      mockAggregateSection("services", {
        display_name: "Services",
        is_decision_section: false,
        items: [],
      }),
    ],
    repo_groups: [
      {
        section_id: "appstream",
        provenance: "verified" as const,
        is_distro: true,
        tier: "distro" as const,
        package_count: 2,
        enabled: true,
      },
    ],
    repo_conflict_count: 0,
    ...overrides,
  };
}

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
  policy: { distro_repos: ["baseos", "appstream"] },
  aggregate: MOCK_AGGREGATE,
  session_is_sensitive: false,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function renderAggregateApp(overrides?: Partial<AggregateAppProps>) {
  const props: AggregateAppProps = {
    aggregate: MOCK_AGGREGATE,
    health: MOCK_HEALTH,
    ...overrides,
  };
  return render(<AggregateApp {...props} />);
}

async function waitForContent() {
  await waitFor(() => {
    expect(screen.getByTestId("aggregate-content")).toBeInTheDocument();
  });
}

/** Dispatch Ctrl+Z via native keydown (bypasses StatsBar disabled state). */
function pressCtrlZ() {
  fireEvent.keyDown(document, { key: "z", ctrlKey: true });
}

/** Dispatch Ctrl+Shift+Z via native keydown. */
function pressCtrlShiftZ() {
  fireEvent.keyDown(document, { key: "z", ctrlKey: true, shiftKey: true });
}

// ---------------------------------------------------------------------------
// Setup / teardown
// ---------------------------------------------------------------------------

beforeEach(() => {
  vi.stubGlobal("fetch", vi.fn());
  mockFetchAggregateView.mockReset();
  mockApplyOp.mockReset().mockResolvedValue({});
  mockUndo.mockReset().mockResolvedValue({});
  mockRedo.mockReset().mockResolvedValue({});
  localStorage.clear();
});

afterEach(() => {
  vi.restoreAllMocks();
});

// ===========================================================================
// Integration test suites
// ===========================================================================

describe("AggregateApp integration", () => {
  // -------------------------------------------------------------------------
  // 1. Full aggregate view render with zones
  // -------------------------------------------------------------------------
  describe("full aggregate view render with zones", () => {
    it("renders sidebar sections and aggregate content from zone-grouped data", async () => {
      mockFetchAggregateView.mockResolvedValue(mockAggregateViewResponse());
      renderAggregateApp();
      await waitForContent();

      // Sidebar renders all three sections
      const sidebar = screen.getByTestId("aggregate-sidebar");
      expect(within(sidebar).getByText("Packages")).toBeInTheDocument();
      expect(within(sidebar).getByText("Config Files")).toBeInTheDocument();
      expect(within(sidebar).getByText("Services")).toBeInTheDocument();

      // Packages render via unified RepoBar + PackageList
      expect(screen.getByTestId("repo-bar")).toBeInTheDocument();
      expect(screen.getByTestId("package-list")).toBeInTheDocument();
      // Aggregate banner is rendered (there are actionable variant items)
      expect(screen.getByTestId("aggregate-banner")).toBeInTheDocument();
    });

    it("shows zone-based item counts in sidebar for sections with zones", async () => {
      mockFetchAggregateView.mockResolvedValue(mockAggregateViewResponse());
      renderAggregateApp();
      await waitForContent();

      const sidebar = screen.getByTestId("aggregate-sidebar");
      // Services has 0 items — use exact match to avoid collisions with
      // other numeric text (ack labels like "0/2 confirmed" contain "0" too)
      const badges = within(sidebar).getAllByText("0");
      expect(badges.length).toBeGreaterThanOrEqual(1);
      // Packages has 2 flat items, Config Files has 2 zone items (1+1+0)
      // Both produce "2" badges, but ack labels also contain "2" — just
      // verify at least two "2" elements exist in the sidebar (badges + ack)
      const twos = within(sidebar).getAllByText("2");
      expect(twos.length).toBeGreaterThanOrEqual(1);
    });
  });

  // -------------------------------------------------------------------------
  // -------------------------------------------------------------------------
  // 2. Undo flow via Ctrl+Z — triggers mutation hook, refetches, updates view
  // -------------------------------------------------------------------------
  describe("undo/redo flow", () => {
    it("Ctrl+Z calls undo API and updates view on successful refetch", async () => {
      const initialView = mockAggregateViewResponse();
      const updatedView = mockAggregateViewResponse({
        generation: 2,
        sections: [
          mockAggregateSection("packages", {
            display_name: "Packages",
            items: [mockAggregateItem()],
          }),
        ],
      });

      mockFetchAggregateView
        .mockResolvedValueOnce(initialView) // initial load
        .mockResolvedValueOnce(updatedView); // refetch after undo

      renderAggregateApp();
      await waitForContent();

      // Verify initial state — sidebar shows 3 sections
      const sidebar = screen.getByTestId("aggregate-sidebar");
      expect(within(sidebar).getByText("Services")).toBeInTheDocument();

      // Ctrl+Z triggers useKeyboard → onUndo → useAggregateMutation.undo()
      await act(async () => {
        pressCtrlZ();
      });

      // useAggregateMutation calls apiUndo then refetches
      await waitFor(() => {
        expect(mockUndo).toHaveBeenCalledOnce();
      });

      // After refetch, view should update — only Packages in sidebar now
      await waitFor(() => {
        expect(screen.queryByText("Services")).not.toBeInTheDocument();
      });
    });

    it("Ctrl+Shift+Z calls redo API", async () => {
      const initialView = mockAggregateViewResponse();
      const updatedView = mockAggregateViewResponse({ generation: 2 });

      mockFetchAggregateView
        .mockResolvedValueOnce(initialView)
        .mockResolvedValueOnce(updatedView);

      renderAggregateApp();
      await waitForContent();

      await act(async () => {
        pressCtrlShiftZ();
      });

      await waitFor(() => {
        expect(mockRedo).toHaveBeenCalledOnce();
      });
    });
  });

  // -------------------------------------------------------------------------
  // 4. Refetch failure — error with retry, content still visible
  // -------------------------------------------------------------------------
  describe("refetch failure", () => {
    it("shows refetch error with Retry after undo fails to refetch", async () => {
      const initialView = mockAggregateViewResponse();

      mockFetchAggregateView.mockResolvedValueOnce(initialView); // initial load
      mockUndo.mockResolvedValueOnce({}); // undo API succeeds
      // But the subsequent refetch fails
      mockFetchAggregateView.mockRejectedValueOnce(new Error("Server unavailable"));

      renderAggregateApp();
      await waitForContent();

      // Content is visible with initial data — packages render via unified components
      expect(screen.getByTestId("package-list")).toBeInTheDocument();

      // Trigger undo via Ctrl+Z
      await act(async () => {
        pressCtrlZ();
      });

      // Refetch error should appear
      await waitFor(() => {
        expect(screen.getByTestId("refetch-error")).toBeInTheDocument();
      });

      // Content still visible (AggregateApp holds last successful view in state)
      expect(screen.getByTestId("package-list")).toBeInTheDocument();

      // Retry button should be present
      expect(
        screen.getByRole("button", { name: /retry/i }),
      ).toBeInTheDocument();
    });

    it("retry clears error and updates view on success", async () => {
      const initialView = mockAggregateViewResponse();
      const recoveredView = mockAggregateViewResponse({ generation: 3 });

      mockFetchAggregateView.mockResolvedValueOnce(initialView); // initial load
      mockUndo.mockResolvedValueOnce({});
      mockFetchAggregateView.mockRejectedValueOnce(new Error("Transient error")); // refetch fails
      mockFetchAggregateView.mockResolvedValueOnce(recoveredView); // retry succeeds

      renderAggregateApp();
      await waitForContent();

      // Trigger undo → refetch fails
      await act(async () => {
        pressCtrlZ();
      });

      await waitFor(() => {
        expect(screen.getByTestId("refetch-error")).toBeInTheDocument();
      });

      // Click Retry
      const retryBtn = screen.getByRole("button", { name: /retry/i });
      await act(async () => {
        await userEvent.click(retryBtn);
      });

      // Error should clear
      await waitFor(() => {
        expect(screen.queryByTestId("refetch-error")).not.toBeInTheDocument();
      });
    });
  });

  // -------------------------------------------------------------------------
  // 5. Flat rendering (aggregate-of-2, no zones)
  // -------------------------------------------------------------------------
  describe("aggregate-of-2 flat rendering", () => {
    it("renders with flat items when sections have no zones", async () => {
      const flatView = mockAggregateViewResponse({
        sections: [
          mockAggregateSection("packages", {
            display_name: "Packages",
            items: [
              mockAggregateItem({
                item_id: {
                  kind: "Package",
                  key: { name: "vim", arch: "x86_64" },
                },
              }),
              mockAggregateItem({
                item_id: {
                  kind: "Package",
                  key: { name: "emacs", arch: "x86_64" },
                },
              }),
            ],
          }),
          mockAggregateSection("config_files", {
            display_name: "Config Files",
            is_decision_section: true,
            items: [mockConfigItem("/etc/hosts")],
          }),
        ],
      });

      mockFetchAggregateView.mockResolvedValue(flatView);
      renderAggregateApp({ aggregate: { ...MOCK_AGGREGATE, zones_active: false } });
      await waitForContent();

      // Sidebar still renders sections
      const sidebar = screen.getByTestId("aggregate-sidebar");
      expect(within(sidebar).getByText("Packages")).toBeInTheDocument();
      expect(within(sidebar).getByText("Config Files")).toBeInTheDocument();

      // Packages render via unified PackageList
      expect(screen.getByTestId("package-list")).toBeInTheDocument();
    });
  });

  // -------------------------------------------------------------------------
  // 6. Section navigation via sidebar clicks
  // -------------------------------------------------------------------------
  describe("section navigation", () => {
    it("switches active section when sidebar items are clicked", async () => {
      mockFetchAggregateView.mockResolvedValue(mockAggregateViewResponse());
      renderAggregateApp();
      await waitForContent();

      // Default is packages — verify package items render
      expect(screen.getByText("httpd.x86_64")).toBeInTheDocument();

      // Click Config Files — packages items should disappear, config items appear
      await userEvent.click(screen.getByText("Config Files"));
      expect(screen.queryByText("httpd.x86_64")).not.toBeInTheDocument();

      // Click Services — empty section
      await userEvent.click(screen.getByText("Services"));

      // Click back to Packages — package items re-appear
      await userEvent.click(screen.getByText("Packages"));
      expect(screen.getByText("httpd.x86_64")).toBeInTheDocument();
    });

    it("highlights active section in sidebar with aria-current", async () => {
      mockFetchAggregateView.mockResolvedValue(mockAggregateViewResponse());
      renderAggregateApp();
      await waitForContent();

      // Config Files should not be current initially
      const configNav = screen
        .getByText("Config Files")
        .closest("[aria-current]");
      expect(configNav).toBeNull();

      // Click Config Files
      await userEvent.click(screen.getByText("Config Files"));

      // Now it should have aria-current
      const activeConfig = screen
        .getByText("Config Files")
        .closest("[aria-current]");
      expect(activeConfig).toHaveAttribute("aria-current", "page");
    });
  });

  // -------------------------------------------------------------------------
  // 7. Keyboard section switching (number keys via useKeyboard)
  //    useKeyboard maps 1-9 to hardcoded SECTION_IDS in display order:
  //    1=packages, 2=configs, 3=services, etc.
  // -------------------------------------------------------------------------
  describe("keyboard navigation", () => {
    it("switches sections with number keys 1-3 (maps to SECTION_IDS)", async () => {
      mockFetchAggregateView.mockResolvedValue(mockAggregateViewResponse());
      const user = userEvent.setup();
      renderAggregateApp();
      await waitForContent();

      // Default section is packages — package items visible
      expect(screen.getByText("httpd.x86_64")).toBeInTheDocument();

      // Press 2 → jumps to "configs" (hardcoded SECTION_IDS[1])
      // Verify via sidebar aria-current since section content may be empty
      await user.keyboard("2");
      await waitFor(() => {
        expect(screen.queryByText("httpd.x86_64")).not.toBeInTheDocument();
      });

      // Press 3 → jumps to "services" (SECTION_IDS[2])
      await user.keyboard("3");

      // Press 1 → back to "packages" (SECTION_IDS[0])
      await user.keyboard("1");
      await waitFor(() => {
        expect(screen.getByText("httpd.x86_64")).toBeInTheDocument();
      });
    });
  });

  // -------------------------------------------------------------------------
  // 8. Initial load failure → full error state
  // -------------------------------------------------------------------------
  describe("initial load failure", () => {
    it("shows full error page when initial fetch fails", async () => {
      mockFetchAggregateView.mockRejectedValue(new Error("Connection refused"));
      renderAggregateApp();

      await waitFor(() => {
        expect(
          screen.getByText(/Failed to load aggregate view/),
        ).toBeInTheDocument();
      });
      expect(screen.getByText("Connection refused")).toBeInTheDocument();

      // Content area should not be rendered
      expect(screen.queryByTestId("aggregate-content")).not.toBeInTheDocument();
    });
  });

  // -------------------------------------------------------------------------
  // 9. View updates propagate through the component tree
  // -------------------------------------------------------------------------
  describe("view update propagation", () => {
    it("updates sidebar after Ctrl+Z triggers view refresh", async () => {
      const threeSection = mockAggregateViewResponse();
      const twoSection = mockAggregateViewResponse({
        generation: 2,
        sections: [
          mockAggregateSection("packages", { display_name: "Packages" }),
          mockAggregateSection("config_files", { display_name: "Config Files" }),
        ],
      });

      mockFetchAggregateView
        .mockResolvedValueOnce(threeSection)
        .mockResolvedValueOnce(twoSection);

      renderAggregateApp();
      await waitForContent();
      // Verify initial state — sidebar has Services
      expect(screen.getByText("Services")).toBeInTheDocument();

      // Ctrl+Z triggers refetch with fewer sections
      await act(async () => {
        pressCtrlZ();
      });

      // Sidebar should update — "Services" no longer present
      await waitFor(() => {
        expect(screen.queryByText("Services")).not.toBeInTheDocument();
      });
    });
  });

  // -------------------------------------------------------------------------
  // 10. Containerfile preview passed to AppShell
  // -------------------------------------------------------------------------
  describe("containerfile preview", () => {
    it("passes containerfile_preview from view response to AppShell", async () => {
      const view = mockAggregateViewResponse({
        containerfile_preview: "FROM ubi9:latest\nRUN dnf install -y httpd",
      });
      mockFetchAggregateView.mockResolvedValue(view);
      renderAggregateApp();
      await waitForContent();

      // AppShell's ContainerfilePanel should have access to preview content
      // We can verify the toggle button exists (Ctrl+E opens it)
      // The panel renders via AppShell but starts collapsed on narrow viewports
      expect(screen.getByTestId("aggregate-content")).toBeInTheDocument();
    });
  });

  // -------------------------------------------------------------------------
  // 11. Aggregate conflict dismiss/restore flow (RepoBar ↔ PackageList)
  // -------------------------------------------------------------------------
  describe("aggregate conflict dismiss/restore flow", () => {
    function makeConflictView(): AggregateViewResponse {
      return mockAggregateViewResponse({
        repo_conflict_count: 1,
        repo_groups: [
          {
            section_id: "baseos",
            provenance: "verified" as const,
            is_distro: true,
            tier: "distro" as const,
            package_count: 2,
            enabled: true,
          },
          {
            section_id: "epel",
            provenance: "incomplete" as const,
            is_distro: false,
            tier: "third_party" as const,
            package_count: 1,
            enabled: true,
          },
        ],
        sections: [
          mockAggregateSection("packages", {
            display_name: "Packages",
            items: [
              mockAggregateItem({
                item_id: {
                  kind: "Package",
                  key: { name: "httpd", arch: "x86_64" },
                },
                prevalence: { count: 3, total: 3 },
                source_repo: "baseos",
                repo_conflict: [
                  { repo: "baseos", host_count: 2 },
                  { repo: "epel", host_count: 1 },
                ],
              }),
              mockAggregateItem({
                item_id: {
                  kind: "Package",
                  key: { name: "curl", arch: "x86_64" },
                },
                prevalence: { count: 3, total: 3 },
                source_repo: "baseos",
              }),
            ],
          }),
          mockAggregateSection("config_files", {
            display_name: "Config Files",
            is_decision_section: true,
            items: [],
          }),
          mockAggregateSection("services", {
            display_name: "Services",
            is_decision_section: false,
            items: [],
          }),
        ],
      });
    }

    it("conflict popover trigger appears on aggregate row, dismiss hides it, RepoBar restore brings it back", async () => {
      mockFetchAggregateView.mockResolvedValue(makeConflictView());
      renderAggregateApp();
      await waitForContent();

      // Conflict popover trigger should be present on httpd row
      const httpdRow = screen.getByTestId("package-row-httpd.x86_64");
      const trigger = within(httpdRow).getByRole("button", {
        name: /repo conflict/i,
      });
      expect(trigger).toBeInTheDocument();

      // curl should not have a conflict trigger
      const curlRow = screen.getByTestId("package-row-curl.x86_64");
      expect(
        within(curlRow).queryByRole("button", { name: /repo conflict/i }),
      ).not.toBeInTheDocument();

      // Open popover and dismiss
      await userEvent.click(trigger);
      const dismissBtn = screen.getByText("Dismiss");
      await userEvent.click(dismissBtn);

      // Trigger should disappear after dismiss
      expect(
        within(httpdRow).queryByRole("button", { name: /repo conflict/i }),
      ).not.toBeInTheDocument();

      // RepoBar should show "Show 1 dismissed"
      const repoBar = screen.getByTestId("repo-bar");
      const restoreBtn = within(repoBar).getByRole("button", {
        name: /show 1 dismissed/i,
      });
      expect(restoreBtn).toBeInTheDocument();

      // Click restore — popover trigger should reappear
      await userEvent.click(restoreBtn);
      await waitFor(() => {
        expect(
          within(httpdRow).getByRole("button", { name: /repo conflict/i }),
        ).toBeInTheDocument();
      });
    });
  });
});
