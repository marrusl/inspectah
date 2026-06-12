import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import { DecisionList } from "../DecisionList";
import type { DecisionItemKind } from "../DecisionItem";
import type {
  RefinedPackage,
  AttentionTag,
  ViewResponse,
} from "../../api/types";
import { mockStats } from "../../test-utils/mockStats";

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

const MOCK_STATS = mockStats({
  sections: [
    { kind: "package", total: 0, included: 0, excluded: 0 },
    { kind: "config", total: 0, included: 0, excluded: 0 },
  ],
});

const MOCK_VIEW: ViewResponse = {
  packages: [],
  config_files: [],
  containerfile_preview: "",
  stats: MOCK_STATS,
  generation: 1,
  repo_groups: [],
  version_changes: [],
  service_states: [],
  service_dropins: [],
  quadlets: [],
  flatpaks: [],
  sysctls: [],
  tuned: [],
  users_groups_decisions: [],
  package_groups: [],
  session_is_sensitive: false,
};

const ROUTINE_TAG: AttentionTag = {
  level: "routine",
  reason: "config_modified",
  detail: null,
};

function attentionToTriage(
  tags: AttentionTag[],
): import("../../api/types").TriageTag {
  const bucketMap: Record<string, string> = {
    needs_review: "investigate",
    informational: "site",
    routine: "baseline",
  };
  const tag = tags[0];
  const bucket = tag ? (bucketMap[tag.level] ?? "baseline") : "baseline";
  const reason = tag
    ? typeof tag.reason === "string"
      ? tag.reason
      : "package_baseline_match"
    : "package_baseline_match";
  return {
    triage: { mode: "single_host" as const, [bucket]: null },
    primary_reason: reason as any,
    annotations: [],
  };
}

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
    triage: attentionToTriage(attention),
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

    expect(screen.getByText("No items in this section")).toBeInTheDocument();
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

    expect(screen.queryByTestId("completion-message")).not.toBeInTheDocument();
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

    expect(screen.queryByTestId("completion-message")).not.toBeInTheDocument();
  });

  it("does not show completion message when unviewed NeedsReview items exist", async () => {
    const NEEDS_REVIEW_TAG: AttentionTag = {
      level: "needs_review",
      reason: "package_user_added",
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

    expect(screen.queryByTestId("completion-message")).not.toBeInTheDocument();
  });

  it("shows completion message when all NeedsReview items have been viewed", async () => {
    const NEEDS_REVIEW_TAG: AttentionTag = {
      level: "needs_review",
      reason: "package_user_added",
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

describe("Packages section renders unified components", () => {
  it("renders PackageList for packages section", async () => {
    const { MainContent } = await import("../MainContent");

    const viewData: ViewResponse = {
      packages: [makePkg({ name: "httpd" }, [ROUTINE_TAG])],
      config_files: [],
      containerfile_preview: "",
      stats: mockStats({
        sections: [
          { kind: "package", total: 1, included: 1, excluded: 0 },
          { kind: "config", total: 0, included: 0, excluded: 0 },
        ],
      }),
      generation: 1,
      repo_groups: [],
      version_changes: [],
      service_states: [],
      service_dropins: [],
      quadlets: [],
      flatpaks: [],
      sysctls: [],
      tuned: [],
      users_groups_decisions: [],
      package_groups: [],
      session_is_sensitive: false,
    };

    render(
      <MainContent
        activeSection="packages"
        loading={false}
        viewData={viewData}
        sections={[]}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
        sectionSearchOpen={false}
        onSectionSearchClose={vi.fn()}
      />,
    );

    // Packages now render via unified PackageList
    expect(screen.getByTestId("package-list")).toBeInTheDocument();
    expect(screen.getByText("httpd.x86_64")).toBeInTheDocument();
  });
});

describe("Version Changes empty states", () => {
  it("renders no_baseline empty state", async () => {
    const { MainContent } = await import("../MainContent");
    const sections = [
      {
        id: "version_changes",
        display_name: "Version Changes",
        items: [],
        empty_reason: "no_baseline",
      },
    ];
    render(
      <MainContent
        activeSection="version_changes"
        loading={false}
        viewData={{ ...MOCK_VIEW }}
        sections={sections}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
        sectionSearchOpen={false}
        onSectionSearchClose={vi.fn()}
      />,
    );
    expect(screen.getByText(/requires a baseline/)).toBeInTheDocument();
  });

  it("renders zero_drift empty state", async () => {
    const { MainContent } = await import("../MainContent");
    const sections = [
      {
        id: "version_changes",
        display_name: "Version Changes",
        items: [],
        empty_reason: "zero_drift",
      },
    ];
    render(
      <MainContent
        activeSection="version_changes"
        loading={false}
        viewData={{ ...MOCK_VIEW }}
        sections={sections}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
        sectionSearchOpen={false}
        onSectionSearchClose={vi.fn()}
      />,
    );
    expect(screen.getByText(/All packages match/)).toBeInTheDocument();
  });

  it("renders data_unavailable empty state", async () => {
    const { MainContent } = await import("../MainContent");
    const sections = [
      {
        id: "version_changes",
        display_name: "Version Changes",
        items: [],
        empty_reason: "data_unavailable",
      },
    ];
    render(
      <MainContent
        activeSection="version_changes"
        loading={false}
        viewData={{ ...MOCK_VIEW }}
        sections={sections}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
        sectionSearchOpen={false}
        onSectionSearchClose={vi.fn()}
      />,
    );
    expect(screen.getByText(/not available/)).toBeInTheDocument();
  });
});

// Leaf dependency tree integration test removed — DependencyModal
// was removed as part of the unified package/repo refactor.
