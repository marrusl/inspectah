import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { DecisionList } from "../DecisionList";
import type { DecisionItemKind } from "../DecisionItem";
import type {
  RefinedPackage,
  AttentionTag,
  RefineStats,
  RefinedView,
} from "../../api/types";

// --- Mock fetch globally ---
const mockFetch = vi.fn();
beforeEach(() => {
  mockFetch.mockReset();
  vi.stubGlobal("fetch", mockFetch);
  mockFetch.mockImplementation((url: string, opts?: RequestInit) => {
    if (url === "/api/viewed" && opts?.method === "POST") {
      return Promise.resolve({ ok: true, status: 204 });
    }
    if (url === "/api/viewed" && (!opts || opts.method === "GET")) {
      return Promise.resolve({
        ok: true,
        json: () => Promise.resolve({ ids: [] }),
      });
    }
    if (url === "/api/op") {
      return Promise.resolve({
        ok: true,
        json: () => Promise.resolve(MOCK_VIEW),
      });
    }
    return Promise.resolve({
      ok: false,
      status: 404,
      json: () => Promise.resolve({ error: "not found" }),
    });
  });
});
afterEach(() => {
  vi.restoreAllMocks();
});

const MOCK_STATS: RefineStats = {
  total_packages: 0,
  included_packages: 0,
  excluded_packages: 0,
  total_configs: 0,
  included_configs: 0,
  excluded_configs: 0,
  needs_review_count: 0,
  ops_applied: 0,
  can_undo: false,
  can_redo: false,
};

const MOCK_VIEW: RefinedView = {
  packages: [],
  config_files: [],
  containerfile_preview: "",
  stats: MOCK_STATS,
  generation: 1,
};

const ROUTINE_TAG: AttentionTag = {
  level: "routine",
  reason: "config_modified",
  detail: null,
};

function makePkg(
  overrides: Partial<RefinedPackage["entry"]> = {},
  attention: AttentionTag[] = [],
): RefinedPackage {
  return {
    entry: {
      name: "test-pkg",
      epoch: "0",
      version: "1.0",
      release: "1.el9",
      arch: "x86_64",
      state: "added",
      include: true,
      source_repo: "baseos",
      fleet: null,
      ...overrides,
    },
    attention,
  };
}

describe("Empty state", () => {
  it("shows EmptyState component when items array is empty", async () => {
    render(
      <DecisionList
        items={[]}
        sectionLabel="Packages"
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    expect(
      screen.getByText("No items in this section"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("There are no packages to triage."),
    ).toBeInTheDocument();
  });

  it("uses section label in empty state body text", async () => {
    render(
      <DecisionList
        items={[]}
        sectionLabel="Config Files"
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    expect(
      screen.getByText("There are no config files to triage."),
    ).toBeInTheDocument();
  });
});

describe("Completion state", () => {
  it("does not show completion message when no NeedsReview items exist", async () => {
    // Routine-only items have nothing to triage
    const items: DecisionItemKind[] = [
      {
        type: "package",
        data: makePkg({ name: "pkg-a" }, [ROUTINE_TAG]),
      },
      {
        type: "package",
        data: makePkg({ name: "pkg-b" }, [ROUTINE_TAG]),
      },
    ];

    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    expect(
      screen.queryByTestId("completion-message"),
    ).not.toBeInTheDocument();
  });

  it("does not show completion message when items have no attention tags", async () => {
    // No attention tags means no NeedsReview items — nothing to triage
    const items: DecisionItemKind[] = [
      {
        type: "package",
        data: makePkg({ name: "pkg-a" }, []),
      },
    ];

    render(
      <DecisionList
        items={items}
        sectionLabel="Config Files"
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    expect(
      screen.queryByTestId("completion-message"),
    ).not.toBeInTheDocument();
  });

  it("does not show completion message when unviewed NeedsReview items exist", async () => {
    const NEEDS_REVIEW_TAG: AttentionTag = {
      level: "needs_review",
      reason: "package_not_in_baseline",
      detail: null,
    };

    const items: DecisionItemKind[] = [
      {
        type: "package",
        data: makePkg({ name: "pkg-a" }, [NEEDS_REVIEW_TAG]),
      },
      {
        type: "package",
        data: makePkg({ name: "pkg-b" }, [ROUTINE_TAG]),
      },
    ];

    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    expect(
      screen.queryByTestId("completion-message"),
    ).not.toBeInTheDocument();
  });

  it("shows completion message when all NeedsReview items have been viewed", async () => {
    const NEEDS_REVIEW_TAG: AttentionTag = {
      level: "needs_review",
      reason: "package_not_in_baseline",
      detail: null,
    };

    const items: DecisionItemKind[] = [
      {
        type: "package",
        data: makePkg({ name: "pkg-a" }, [NEEDS_REVIEW_TAG]),
      },
      {
        type: "package",
        data: makePkg({ name: "pkg-b" }, [ROUTINE_TAG]),
      },
    ];

    // Pre-seed viewed IDs so all NeedsReview items are already viewed
    mockFetch.mockImplementation((url: string, opts?: RequestInit) => {
      if (url === "/api/viewed" && opts?.method === "POST") {
        return Promise.resolve({ ok: true, status: 204 });
      }
      if (url === "/api/viewed" && (!opts || opts.method === "GET")) {
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve({ ids: ["packages:pkg-a.x86_64"] }),
        });
      }
      return Promise.resolve({
        ok: false,
        status: 404,
        json: () => Promise.resolve({ error: "not found" }),
      });
    });

    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    await waitFor(() => {
      expect(screen.getByTestId("completion-message")).toBeInTheDocument();
    });
    expect(
      screen.getByText("All items have been triaged."),
    ).toBeInTheDocument();
  });
});

describe("Filter empty state in MainContent", () => {
  it("shows 'No items match your search' with clear button", async () => {
    // Import MainContent for this test
    const { MainContent } = await import("../MainContent");

    const viewData: RefinedView = {
      packages: [makePkg({ name: "httpd" }, [ROUTINE_TAG])],
      config_files: [],
      containerfile_preview: "",
      stats: { ...MOCK_STATS, total_packages: 1, included_packages: 1 },
      generation: 1,
    };

    render(
      <MainContent
        activeSection="packages"
        loading={false}
        viewData={viewData}
        sections={[]}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
        sectionSearchOpen={true}
        onSectionSearchClose={vi.fn()}
      />,
    );

    // Wait for search input to render
    await waitFor(() => {
      expect(screen.getByTestId("section-search-input")).toBeInTheDocument();
    });

    // Type a filter that matches nothing - use the PF SearchInput's inner input
    const searchInput = screen.getByPlaceholderText("Filter items...");
    const user = userEvent.setup();
    await user.type(searchInput, "zzz-no-match");

    await waitFor(() => {
      expect(
        screen.getByText("No items match your search"),
      ).toBeInTheDocument();
    });

    // Clear filter button should exist
    expect(
      screen.getByRole("button", { name: "Clear filter" }),
    ).toBeInTheDocument();
  });
});
