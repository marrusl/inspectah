/**
 * Portal navigation tests.
 *
 * Proves that calling the portal navigation (banner/search → expand variant)
 * twice with the same target keeps the variant view open (doesn't toggle shut)
 * (#3), and that a target the user navigated away from is not revealed when
 * they come back to its section.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import {
  render,
  screen,
  waitFor,
  act,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { AggregateApp } from "../../AggregateApp";
import type {
  AggregateViewResponse,
  AggregateHealthInfo,
  HealthResponse,
  AggregateItem,
  ActionableVariantItem,
} from "../../../api/types";

// ---------------------------------------------------------------------------
// Module mocks
// ---------------------------------------------------------------------------

const mockFetchAggregateView = vi.fn<() => Promise<AggregateViewResponse>>();
vi.mock("../../../api/aggregate-client", () => ({
  fetchAggregateView: (...args: unknown[]) =>
    mockFetchAggregateView(...(args as [])),
  fetchAggregateDiff: vi.fn().mockResolvedValue({
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

const MOCK_AGGREGATE: AggregateHealthInfo = {
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

function configItemWithVariants(): AggregateItem {
  return {
    item_id: { kind: "Config", key: { path: "/etc/httpd/conf/httpd.conf" } },
    include: true,
    locked: false,
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

function makeAggregateViewWithVariants(): AggregateViewResponse {
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
    sysctl: { state: "aggregate" },
  };
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

beforeEach(() => {
  vi.stubGlobal("fetch", vi.fn());
  mockFetchAggregateView.mockReset();
  localStorage.clear();
});

describe("portal idempotency", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("calling banner navigate twice with same target keeps variant view open", async () => {
    const view = makeAggregateViewWithVariants();
    mockFetchAggregateView.mockResolvedValue(view);
    const scroll = vi.spyOn(Element.prototype, "scrollIntoView");

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

    // Wait for variant view to appear and the first reveal to finish
    await waitFor(() => {
      expect(screen.getByTestId("variant-view")).toBeInTheDocument();
    });
    await waitFor(() => {
      expect(scroll).toHaveBeenCalledTimes(1);
    });

    // Second click — same target, should keep variant view open (not toggle shut)
    await act(async () => {
      navButton.click();
    });

    // Wait for the second click's own reveal frame, so it runs while the
    // tree is still mounted rather than racing teardown.
    await waitFor(() => {
      expect(scroll).toHaveBeenCalledTimes(2);
    });

    // Variant view must still be visible — NOT toggled shut
    expect(screen.getByTestId("variant-view")).toBeInTheDocument();
  });
});

describe("superseded portal target", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  /** Hold animation frames so a reveal stays pending until flushed. */
  function holdAnimationFrames() {
    const held = new Map<number, FrameRequestCallback>();
    let nextId = 1;
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
      const id = nextId++;
      held.set(id, cb);
      return id;
    });
    vi.spyOn(window, "cancelAnimationFrame").mockImplementation((id) => {
      held.delete(id);
    });
    return {
      get pending() {
        return held.size;
      },
      /** Run held frames, including any they schedule, until none remain. */
      flush() {
        while (held.size > 0) {
          const batch = [...held.values()];
          held.clear();
          act(() => {
            for (const cb of batch) cb(0);
          });
        }
      },
      release() {
        vi.mocked(window.requestAnimationFrame).mockRestore();
        vi.mocked(window.cancelAnimationFrame).mockRestore();
      },
    };
  }

  it("does not reveal a target the user left before its reveal ran", async () => {
    mockFetchAggregateView.mockResolvedValue(makeAggregateViewWithVariants());
    const scroll = vi.spyOn(Element.prototype, "scrollIntoView");
    const user = userEvent.setup();

    render(<AggregateApp aggregate={MOCK_AGGREGATE} health={MOCK_HEALTH} />);
    await waitFor(() => {
      expect(screen.getByTestId("aggregate-content")).toBeInTheDocument();
    });
    const sidebar = screen.getByTestId("aggregate-sidebar");
    await user.click(within(sidebar).getByText("Config Files"));

    // Navigate from the banner, but hold the reveal frame so the user can
    // leave the section before it runs.
    const frames = holdAnimationFrames();
    await act(async () => {
      screen
        .getByRole("button", { name: /navigate to \/etc\/httpd/i })
        .click();
    });
    expect(frames.pending).toBeGreaterThan(0);

    // Leave for Packages, which unmounts the section and cancels the
    // reveal, then come back.
    await user.click(within(sidebar).getByText("Packages"));
    await user.click(within(sidebar).getByText("Config Files"));
    frames.flush();

    // The returning section must not reveal the abandoned target.
    expect(scroll).not.toHaveBeenCalled();
    expect(document.querySelector(".inspectah-highlight")).toBeNull();

    // A fresh banner request afterwards still reveals its target.
    frames.release();
    await act(async () => {
      screen
        .getByRole("button", { name: /navigate to \/etc\/httpd/i })
        .click();
    });
    const target = document.querySelector(
      `[data-item-id='${JSON.stringify(configItemWithVariants().item_id)}']`,
    );
    expect(target).not.toBeNull();
    await waitFor(() => {
      expect(scroll).toHaveBeenCalledTimes(1);
    });
    expect(scroll.mock.contexts[0]).toBe(target);
    expect(target).toHaveClass("inspectah-highlight");
  });
});
