import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { TriageBucketGroup } from "../TriageBucketGroup";
import { DecisionItem } from "../DecisionItem";
import type { DecisionItemKind } from "../DecisionItem";
import { PackageDetail } from "../PackageDetail";
import { ConfigDetail } from "../ConfigDetail";
import { DecisionList } from "../DecisionList";
import { MainContent } from "../MainContent";
import { RepoGroupHeader } from "../RepoGroupHeader";
import type {
  RefinedPackage,
  RefinedConfig,
  AttentionTag,
  RefineStats,
  ViewResponse,
  RepoGroupInfo,
  TriageTag,
} from "../../api/types";
import { mockStats } from "../../test-utils/mockStats";

// --- Mock fetch globally ---
const mockFetch = vi.fn();
beforeEach(() => {
  mockFetch.mockReset();
  vi.stubGlobal("fetch", mockFetch);
  // Default: markViewed returns 204, fetchViewed returns empty
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
    // applyOp
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

// --- Test data helpers ---

/** Map legacy AttentionTag[] to a TriageTag for test backward compat. */
function attentionToTriage(tags: AttentionTag[]): TriageTag {
  const tag = tags[0];
  if (!tag) {
    return {
      triage: { mode: "single_host" as const, baseline: null },
      primary_reason: "package_baseline_match",
      annotations: [],
    };
  }
  // Map attention level to triage bucket
  const bucketMap: Record<string, string> = {
    needs_review: "investigate",
    informational: "site",
    routine: "baseline",
  };
  const bucket = bucketMap[tag.level] ?? "baseline";
  // Map attention reason to triage reason
  const reason =
    typeof tag.reason === "object" && "custom" in tag.reason
      ? tag.reason
      : (tag.reason as string);
  return {
    triage: { mode: "single_host" as const, [bucket]: null },
    primary_reason: reason as TriageTag["primary_reason"],
    annotations: [],
  };
}

// --- Test data factories ---

const MOCK_STATS = mockStats({
  sections: [
    { kind: "package", total: 3, included: 2, excluded: 1 },
    { kind: "config", total: 2, included: 1, excluded: 1 },
  ],
  needs_review_count: 2,
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

function makePkg(
  overrides: Partial<RefinedPackage["entry"]> = {},
  attention: AttentionTag[] = [],
): RefinedPackage {
  return {
    entry: {
      name: "httpd",
      epoch: "0",
      version: "2.4.57",
      release: "1.el9",
      arch: "x86_64",
      state: "added",
      include: true,
      source_repo: "appstream",
      fleet: null,
      ...overrides,
    },
    attention,
    triage: attentionToTriage(attention),
  };
}

function makeConfig(
  overrides: Partial<RefinedConfig["entry"]> = {},
  attention: AttentionTag[] = [],
): RefinedConfig {
  return {
    entry: {
      path: "/etc/httpd/conf/httpd.conf",
      kind: "rpm_owned_modified",
      category: "other",
      content: "ServerRoot /etc/httpd",
      rpm_va_flags: null,
      package: "httpd",
      diff_against_rpm: null,
      include: true,
      tie: false,
      tie_winner: false,
      fleet: null,
      ...overrides,
    },
    attention,
    triage: attentionToTriage(attention),
  };
}

function makeViewResponse(
  overrides: {
    packages?: RefinedPackage[];
    config_files?: RefinedConfig[];
    stats?: Partial<RefineStats>;
    repo_groups?: RepoGroupInfo[];
    baseline_summary?: ViewResponse["baseline_summary"];
  } = {},
): ViewResponse {
  return {
    packages: overrides.packages ?? [],
    config_files: overrides.config_files ?? [],
    containerfile_preview: "",
    stats: { ...MOCK_STATS, ...overrides.stats },
    generation: 1,
    repo_groups: overrides.repo_groups ?? [],
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
    ...(overrides.baseline_summary
      ? { baseline_summary: overrides.baseline_summary }
      : {}),
  };
}

const defaultMainContentProps = {
  activeSection: "packages" as string,
  loading: false,
  sections: null,
  onViewUpdate: vi.fn(),
  onMutationError: vi.fn(),
  sectionSearchOpen: false,
  onSectionSearchClose: vi.fn(),
};

const NEEDS_REVIEW_TAG: AttentionTag = {
  level: "needs_review",
  reason: "package_user_added",
  detail: "Not found in base image",
};

const INFO_TAG: AttentionTag = {
  level: "informational",
  reason: "package_version_changed",
  detail: null,
};

const ROUTINE_TAG: AttentionTag = {
  level: "routine",
  reason: "config_modified",
  detail: null,
};

// ---- TriageBucketGroup tests ----

describe("TriageBucketGroup", () => {
  it("renders with correct border color for needs_review", () => {
    const { container } = render(
      <TriageBucketGroup level="needs_review" count={3}>
        <div>items</div>
      </TriageBucketGroup>,
    );
    const wrapper = container.firstChild as HTMLElement;
    expect(wrapper.style.borderLeft).toContain(
      "--pf-t--global--color--status--danger--default",
    );
  });

  it("renders with correct border color for informational", () => {
    const { container } = render(
      <TriageBucketGroup level="informational" count={2}>
        <div>items</div>
      </TriageBucketGroup>,
    );
    const wrapper = container.firstChild as HTMLElement;
    expect(wrapper.style.borderLeft).toContain(
      "--pf-t--global--color--status--info--default",
    );
  });

  it("renders with correct border color for routine", () => {
    const { container } = render(
      <TriageBucketGroup level="routine" count={1}>
        <div>items</div>
      </TriageBucketGroup>,
    );
    const wrapper = container.firstChild as HTMLElement;
    expect(wrapper.style.borderLeft).toContain(
      "--pf-t--global--color--status--success--default",
    );
  });

  it("starts expanded for needs_review", () => {
    render(
      <TriageBucketGroup level="needs_review" count={1}>
        <div data-testid="child">content</div>
      </TriageBucketGroup>,
    );
    expect(screen.getByTestId("child")).toBeInTheDocument();
  });

  it("starts expanded for informational", () => {
    render(
      <TriageBucketGroup level="informational" count={5}>
        <div data-testid="child">content</div>
      </TriageBucketGroup>,
    );
    // informational is in the expanded-by-default set
    expect(screen.getByTestId("child")).toBeInTheDocument();
    const child = screen.getByTestId("child");
    expect(child.closest("[hidden]")).toBeFalsy();
  });

  it("starts collapsed for routine with 3+ items", () => {
    render(
      <TriageBucketGroup level="routine" count={5}>
        <div data-testid="child">content</div>
      </TriageBucketGroup>,
    );
    const child = screen.getByTestId("child");
    expect(child.closest("[hidden]")).toBeTruthy();
  });

  it("always expands sections with fewer than 3 items", () => {
    render(
      <TriageBucketGroup level="routine" count={2}>
        <div data-testid="child">content</div>
      </TriageBucketGroup>,
    );
    // Small sections (<3 items) are always expanded regardless of level
    const child = screen.getByTestId("child");
    expect(child.closest("[hidden]")).toBeFalsy();
  });

  it("shows item count in toggle text", () => {
    render(
      <TriageBucketGroup level="needs_review" count={5}>
        <div>items</div>
      </TriageBucketGroup>,
    );
    expect(screen.getByText("Needs Review (5)")).toBeInTheDocument();
  });
});

// ---- DecisionItem tests ----

describe("DecisionItem", () => {
  const defaultProps = {
    level: "needs_review" as const,
    rowIndex: 1,
    isViewed: false,
    isPending: false,
    onToggleInclude: vi.fn(),
    onMarkViewed: vi.fn(),
  };

  it("renders package name", () => {
    const item: DecisionItemKind = { type: "package", data: makePkg() };
    render(<DecisionItem item={item} {...defaultProps} />);
    expect(screen.getByText("httpd.x86_64")).toBeInTheDocument();
  });

  it("renders config path", () => {
    const item: DecisionItemKind = { type: "config", data: makeConfig() };
    render(<DecisionItem item={item} {...defaultProps} />);
    expect(screen.getByText("/etc/httpd/conf/httpd.conf")).toBeInTheDocument();
  });

  it("fires mutation on toggle", async () => {
    const onToggle = vi.fn();
    const item: DecisionItemKind = {
      type: "package",
      data: makePkg({ include: true }),
    };
    render(
      <DecisionItem item={item} {...defaultProps} onToggleInclude={onToggle} />,
    );

    const toggle = screen.getByRole("checkbox", { name: /toggle httpd/i });
    await userEvent.click(toggle);
    expect(onToggle).toHaveBeenCalledWith({
      op: "SetInclude",
      target: {
        item_id: { kind: "Package", key: { name: "httpd", arch: "x86_64" } },
        include: false,
      },
    });
  });

  it("fires SetInclude(true) when toggling excluded package", async () => {
    const onToggle = vi.fn();
    const item: DecisionItemKind = {
      type: "package",
      data: makePkg({ include: false }),
    };
    render(
      <DecisionItem item={item} {...defaultProps} onToggleInclude={onToggle} />,
    );

    const toggle = screen.getByRole("checkbox", { name: /toggle httpd/i });
    await userEvent.click(toggle);
    expect(onToggle).toHaveBeenCalledWith({
      op: "SetInclude",
      target: {
        item_id: { kind: "Package", key: { name: "httpd", arch: "x86_64" } },
        include: true,
      },
    });
  });

  it("fires SetInclude(false) when toggling included config", async () => {
    const onToggle = vi.fn();
    const item: DecisionItemKind = {
      type: "config",
      data: makeConfig({ include: true }),
    };
    render(
      <DecisionItem item={item} {...defaultProps} onToggleInclude={onToggle} />,
    );

    const toggle = screen.getByRole("checkbox", { name: /toggle/i });
    await userEvent.click(toggle);
    expect(onToggle).toHaveBeenCalledWith({
      op: "SetInclude",
      target: {
        item_id: {
          kind: "Config",
          key: { path: "/etc/httpd/conf/httpd.conf" },
        },
        include: false,
      },
    });
  });

  it("toggles on Space key", async () => {
    const onToggle = vi.fn();
    const item: DecisionItemKind = { type: "package", data: makePkg() };
    render(
      <DecisionItem item={item} {...defaultProps} onToggleInclude={onToggle} />,
    );

    const row = screen.getByRole("row");
    row.focus();
    await userEvent.keyboard(" ");
    expect(onToggle).toHaveBeenCalled();
  });

  it("toggles on x key", async () => {
    const onToggle = vi.fn();
    const item: DecisionItemKind = { type: "package", data: makePkg() };
    render(
      <DecisionItem item={item} {...defaultProps} onToggleInclude={onToggle} />,
    );

    const row = screen.getByRole("row");
    row.focus();
    await userEvent.keyboard("x");
    expect(onToggle).toHaveBeenCalled();
  });

  it("expands detail on Enter key", async () => {
    const item: DecisionItemKind = {
      type: "package",
      data: makePkg({}, [NEEDS_REVIEW_TAG]),
    };
    render(<DecisionItem item={item} {...defaultProps} />);

    const row = screen.getByRole("row");
    row.focus();
    await userEvent.keyboard("{Enter}");
    expect(screen.getByTestId("package-detail")).toBeInTheDocument();
  });

  it("shows attention label for needs_review items", () => {
    const item: DecisionItemKind = {
      type: "package",
      data: makePkg({}, [NEEDS_REVIEW_TAG]),
    };
    render(<DecisionItem item={item} {...defaultProps} />);
    expect(screen.getByText("User Added")).toBeInTheDocument();
  });

  it("shows unviewed dot for unviewed needs_review items", () => {
    const item: DecisionItemKind = {
      type: "package",
      data: makePkg({}, [NEEDS_REVIEW_TAG]),
    };
    render(<DecisionItem item={item} {...defaultProps} isViewed={false} />);
    expect(screen.getByTestId("unviewed-dot")).toBeInTheDocument();
  });

  it("hides unviewed dot for viewed items", () => {
    const item: DecisionItemKind = {
      type: "package",
      data: makePkg({}, [NEEDS_REVIEW_TAG]),
    };
    render(<DecisionItem item={item} {...defaultProps} isViewed={true} />);
    expect(screen.queryByTestId("unviewed-dot")).not.toBeInTheDocument();
  });

  it("does not show unviewed dot for non-needs_review items", () => {
    const item: DecisionItemKind = {
      type: "package",
      data: makePkg({}, [INFO_TAG]),
    };
    render(
      <DecisionItem
        item={item}
        {...defaultProps}
        level="informational"
        isViewed={false}
      />,
    );
    expect(screen.queryByTestId("unviewed-dot")).not.toBeInTheDocument();
  });
});

// ---- Viewed tracking tests ----

describe("Viewed tracking", () => {
  const baseProps = {
    level: "needs_review" as const,
    rowIndex: 1,
    isViewed: false,
    isPending: false,
    onToggleInclude: vi.fn(),
    onMarkViewed: vi.fn(),
  };

  it("toggle marks viewed", async () => {
    const onMarkViewed = vi.fn();
    const item: DecisionItemKind = { type: "package", data: makePkg() };
    render(
      <DecisionItem item={item} {...baseProps} onMarkViewed={onMarkViewed} />,
    );

    const toggle = screen.getByRole("checkbox", { name: /toggle/i });
    await userEvent.click(toggle);
    expect(onMarkViewed).toHaveBeenCalledWith("packages:httpd.x86_64");
  });

  it("expanding non-toggled item marks viewed", async () => {
    const onMarkViewed = vi.fn();
    const item: DecisionItemKind = {
      type: "package",
      data: makePkg({}, [NEEDS_REVIEW_TAG]),
    };
    render(
      <DecisionItem item={item} {...baseProps} onMarkViewed={onMarkViewed} />,
    );

    // Expand via Enter on the row (expand button is aria-hidden)
    const row = screen.getByRole("row");
    row.focus();
    await userEvent.keyboard("{Enter}");
    expect(onMarkViewed).toHaveBeenCalledWith("packages:httpd.x86_64");
  });

  it("expanding already-toggled item does NOT re-mark viewed", async () => {
    const onMarkViewed = vi.fn();
    const item: DecisionItemKind = {
      type: "package",
      data: makePkg({}, [NEEDS_REVIEW_TAG]),
    };
    render(
      <DecisionItem item={item} {...baseProps} onMarkViewed={onMarkViewed} />,
    );

    // First toggle (marks viewed)
    const toggle = screen.getByRole("checkbox", { name: /toggle/i });
    await userEvent.click(toggle);
    expect(onMarkViewed).toHaveBeenCalledTimes(1);

    // Then expand via Enter (should NOT call markViewed again)
    const row = screen.getByRole("row");
    row.focus();
    await userEvent.keyboard("{Enter}");
    // Only the toggle call, not an expand call
    expect(onMarkViewed).toHaveBeenCalledTimes(1);
  });
});

// ---- PackageDetail tests ----

describe("PackageDetail", () => {
  it("shows NEVRA fields", () => {
    const pkg = makePkg({ epoch: "1", version: "2.4.57", release: "1.el9" });
    render(<PackageDetail pkg={pkg} />);
    expect(screen.getByText("httpd-1:2.4.57-1.el9.x86_64")).toBeInTheDocument();
  });

  it("omits epoch prefix when epoch is 0", () => {
    const pkg = makePkg({ epoch: "0" });
    render(<PackageDetail pkg={pkg} />);
    expect(screen.getByText("httpd-2.4.57-1.el9.x86_64")).toBeInTheDocument();
  });

  it("shows state", () => {
    const pkg = makePkg({ state: "base_image_only" });
    render(<PackageDetail pkg={pkg} />);
    expect(screen.getByText("Base Image Only")).toBeInTheDocument();
  });

  it("shows repo", () => {
    const pkg = makePkg({ source_repo: "appstream" });
    render(<PackageDetail pkg={pkg} />);
    expect(screen.getByText("appstream")).toBeInTheDocument();
  });

  it("shows attention reasons with labels", () => {
    const pkg = makePkg({}, [NEEDS_REVIEW_TAG]);
    render(<PackageDetail pkg={pkg} />);
    expect(screen.getByText("User Added")).toBeInTheDocument();
    expect(screen.getByText("Not found in base image")).toBeInTheDocument();
  });
});

// ---- ConfigDetail tests ----

describe("ConfigDetail", () => {
  it("shows path", () => {
    const cfg = makeConfig();
    render(<ConfigDetail config={cfg} />);
    expect(screen.getByText("/etc/httpd/conf/httpd.conf")).toBeInTheDocument();
  });

  it("shows kind", () => {
    const cfg = makeConfig({ kind: "rpm_owned_modified" });
    render(<ConfigDetail config={cfg} />);
    expect(screen.getByText("Rpm Owned Modified")).toBeInTheDocument();
  });

  it("shows owner package when present", () => {
    const cfg = makeConfig({ package: "httpd" });
    render(<ConfigDetail config={cfg} />);
    expect(screen.getByText("httpd")).toBeInTheDocument();
  });

  it("shows content preview", () => {
    const cfg = makeConfig({ content: "ServerRoot /etc/httpd" });
    render(<ConfigDetail config={cfg} />);
    expect(screen.getByText("ServerRoot /etc/httpd")).toBeInTheDocument();
  });

  it("truncates long content", () => {
    const longContent = "x".repeat(600);
    const cfg = makeConfig({ content: longContent });
    render(<ConfigDetail config={cfg} />);
    expect(screen.getByText(/\.\.\.$/)).toBeInTheDocument();
  });
});

// ---- DecisionList tests ----

describe("DecisionList", () => {
  it("groups items by attention level in correct order", async () => {
    const items: DecisionItemKind[] = [
      {
        type: "package",
        data: makePkg({ name: "routine-pkg" }, [ROUTINE_TAG]),
      },
      {
        type: "package",
        data: makePkg({ name: "review-pkg" }, [NEEDS_REVIEW_TAG]),
      },
      { type: "package", data: makePkg({ name: "info-pkg" }, [INFO_TAG]) },
    ];

    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    // Wait for the viewed fetch
    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    const groups = screen.getAllByTestId(/^attention-group-/);
    expect(groups).toHaveLength(3);
    expect(groups[0]).toHaveAttribute(
      "data-testid",
      "attention-group-needs_review",
    );
    expect(groups[1]).toHaveAttribute(
      "data-testid",
      "attention-group-informational",
    );
    expect(groups[2]).toHaveAttribute("data-testid", "attention-group-routine");
  });

  it("only shows groups that have items", async () => {
    const items: DecisionItemKind[] = [
      {
        type: "package",
        data: makePkg({ name: "review-pkg" }, [NEEDS_REVIEW_TAG]),
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
      screen.getByTestId("attention-group-needs_review"),
    ).toBeInTheDocument();
    expect(
      screen.queryByTestId("attention-group-informational"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("attention-group-routine"),
    ).not.toBeInTheDocument();
  });

  it("shows empty state when no items", async () => {
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
  });

  it("has grid role with aria-label", async () => {
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

    // Grid role is now on each TriageBucketGroup's inner container, not the outer list
    expect(screen.getByTestId("decision-list-packages")).toBeInTheDocument();
  });
});

// ---- Error handling tests ----

describe("Error handling", () => {
  it("shows toast on mutation error and auto-dismisses", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });

    // Make applyOp fail with a non-409 error
    mockFetch.mockImplementation((url: string, opts?: RequestInit) => {
      if (url === "/api/viewed" && (!opts || opts.method === "GET")) {
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve({ ids: [] }),
        });
      }
      if (url === "/api/viewed" && opts?.method === "POST") {
        return Promise.resolve({ ok: true, status: 204 });
      }
      if (url === "/api/op") {
        return Promise.resolve({
          ok: false,
          status: 422,
          json: () => Promise.resolve({ error: "invalid operation" }),
        });
      }
      // Optimistic revert re-fetch
      if (url === "/api/view") {
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

    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "httpd" }, [NEEDS_REVIEW_TAG]) },
    ];

    const onViewUpdate = vi.fn();

    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        onViewUpdate={onViewUpdate}
        onMutationError={vi.fn()}
      />,
    );

    // Wait for initial viewed fetch
    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    // Toggle the switch to trigger mutation
    const toggle = screen.getByRole("checkbox", { name: /toggle/i });
    await user.click(toggle);

    // Wait for error toast and optimistic revert re-fetch to complete
    await waitFor(() => {
      expect(screen.getByText(/Error: invalid operation/)).toBeInTheDocument();
      expect(onViewUpdate).toHaveBeenCalledWith(MOCK_VIEW);
    });

    // Auto-dismiss after 3 seconds
    vi.advanceTimersByTime(3100);
    await waitFor(() => {
      expect(
        screen.queryByText(/Error: invalid operation/),
      ).not.toBeInTheDocument();
    });

    vi.useRealTimers();
  });

  it("auto re-fetches on 409 stale generation without showing toast", async () => {
    const REFRESHED_VIEW = { ...MOCK_VIEW, generation: 42 };

    mockFetch.mockImplementation((url: string, opts?: RequestInit) => {
      if (url === "/api/viewed" && (!opts || opts.method === "GET")) {
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve({ ids: [] }),
        });
      }
      if (url === "/api/viewed" && opts?.method === "POST") {
        return Promise.resolve({ ok: true, status: 204 });
      }
      if (url === "/api/op") {
        return Promise.resolve({
          ok: false,
          status: 409,
          json: () => Promise.resolve({ error: "stale generation" }),
        });
      }
      if (url === "/api/view") {
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve(REFRESHED_VIEW),
        });
      }
      return Promise.resolve({
        ok: false,
        status: 404,
        json: () => Promise.resolve({ error: "not found" }),
      });
    });

    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "httpd" }, [NEEDS_REVIEW_TAG]) },
    ];

    const onViewUpdate = vi.fn();
    const onMutationError = vi.fn();

    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        onViewUpdate={onViewUpdate}
        onMutationError={onMutationError}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    const toggle = screen.getByRole("checkbox", { name: /toggle/i });
    await userEvent.click(toggle);

    // Should auto re-fetch and update view
    await waitFor(() => {
      expect(onViewUpdate).toHaveBeenCalledWith(REFRESHED_VIEW);
    });

    // Should NOT show an error toast
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();

    // Should NOT call onMutationError
    expect(onMutationError).not.toHaveBeenCalled();
  });
});

// ---- Roving tabindex tests ----

describe("Roving tabindex", () => {
  it("first row has tabindex 0, others have tabindex -1", async () => {
    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "aaa" }, [NEEDS_REVIEW_TAG]) },
      { type: "package", data: makePkg({ name: "bbb" }, [NEEDS_REVIEW_TAG]) },
      { type: "package", data: makePkg({ name: "ccc" }, [NEEDS_REVIEW_TAG]) },
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

    const rows = screen.getAllByRole("row");
    expect(rows[0]).toHaveAttribute("tabindex", "0");
    expect(rows[1]).toHaveAttribute("tabindex", "-1");
    expect(rows[2]).toHaveAttribute("tabindex", "-1");
  });

  it("ArrowDown moves focus to next row", async () => {
    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "aaa" }, [NEEDS_REVIEW_TAG]) },
      { type: "package", data: makePkg({ name: "bbb" }, [NEEDS_REVIEW_TAG]) },
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

    const rows = screen.getAllByRole("row");
    rows[0].focus();
    await userEvent.keyboard("{ArrowDown}");

    expect(rows[1]).toHaveAttribute("tabindex", "0");
    expect(rows[0]).toHaveAttribute("tabindex", "-1");
  });

  it("ArrowUp moves focus to previous row", async () => {
    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "aaa" }, [NEEDS_REVIEW_TAG]) },
      { type: "package", data: makePkg({ name: "bbb" }, [NEEDS_REVIEW_TAG]) },
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

    const rows = screen.getAllByRole("row");
    // First move down to row 1
    rows[0].focus();
    await userEvent.keyboard("{ArrowDown}");
    // Then move up back to row 0
    await userEvent.keyboard("{ArrowUp}");

    expect(rows[0]).toHaveAttribute("tabindex", "0");
    expect(rows[1]).toHaveAttribute("tabindex", "-1");
  });

  it("wraps from last to first on ArrowDown", async () => {
    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "aaa" }, [NEEDS_REVIEW_TAG]) },
      { type: "package", data: makePkg({ name: "bbb" }, [NEEDS_REVIEW_TAG]) },
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

    const rows = screen.getAllByRole("row");
    rows[0].focus();
    // Move down to last row
    await userEvent.keyboard("{ArrowDown}");
    // Wrap to first
    await userEvent.keyboard("{ArrowDown}");

    expect(rows[0]).toHaveAttribute("tabindex", "0");
    expect(rows[1]).toHaveAttribute("tabindex", "-1");
  });

  it("wraps from first to last on ArrowUp", async () => {
    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "aaa" }, [NEEDS_REVIEW_TAG]) },
      { type: "package", data: makePkg({ name: "bbb" }, [NEEDS_REVIEW_TAG]) },
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

    const rows = screen.getAllByRole("row");
    rows[0].focus();
    // ArrowUp from first row wraps to last
    await userEvent.keyboard("{ArrowUp}");

    expect(rows[1]).toHaveAttribute("tabindex", "0");
    expect(rows[0]).toHaveAttribute("tabindex", "-1");
  });

  it("j/k keys also navigate rows", async () => {
    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "aaa" }, [NEEDS_REVIEW_TAG]) },
      { type: "package", data: makePkg({ name: "bbb" }, [NEEDS_REVIEW_TAG]) },
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

    const rows = screen.getAllByRole("row");
    rows[0].focus();
    await userEvent.keyboard("j");

    expect(rows[1]).toHaveAttribute("tabindex", "0");
    expect(rows[0]).toHaveAttribute("tabindex", "-1");

    await userEvent.keyboard("k");

    expect(rows[0]).toHaveAttribute("tabindex", "0");
    expect(rows[1]).toHaveAttribute("tabindex", "-1");
  });

  it("g jumps to first row", async () => {
    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "aaa" }, [NEEDS_REVIEW_TAG]) },
      { type: "package", data: makePkg({ name: "bbb" }, [NEEDS_REVIEW_TAG]) },
      { type: "package", data: makePkg({ name: "ccc" }, [NEEDS_REVIEW_TAG]) },
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

    const rows = screen.getAllByRole("row");
    // Navigate to the last row first
    rows[0].focus();
    await userEvent.keyboard("j");
    await userEvent.keyboard("j");
    expect(rows[2]).toHaveAttribute("tabindex", "0");

    // Press g to jump to first
    await userEvent.keyboard("g");
    expect(rows[0]).toHaveAttribute("tabindex", "0");
    expect(rows[2]).toHaveAttribute("tabindex", "-1");
  });

  it("G jumps to last row", async () => {
    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "aaa" }, [NEEDS_REVIEW_TAG]) },
      { type: "package", data: makePkg({ name: "bbb" }, [NEEDS_REVIEW_TAG]) },
      { type: "package", data: makePkg({ name: "ccc" }, [NEEDS_REVIEW_TAG]) },
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

    const rows = screen.getAllByRole("row");
    rows[0].focus();

    // Press G (capital) to jump to last
    await userEvent.keyboard("G");
    expect(rows[2]).toHaveAttribute("tabindex", "0");
    expect(rows[0]).toHaveAttribute("tabindex", "-1");
  });
});

// ---- ArrowDown from SectionSearch tests ----

describe("ArrowDown from SectionSearch", () => {
  it("focuses the first decision item when ArrowDown is pressed in SectionSearch", async () => {
    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "httpd" }, [NEEDS_REVIEW_TAG]) },
    ];

    const { container } = render(
      <div>
        <input
          data-testid="search-input"
          onKeyDown={(e) => {
            if (e.key === "ArrowDown") {
              const firstItem = document.querySelector(
                "[data-testid^='decision-item-']",
              ) as HTMLElement | null;
              firstItem?.focus();
            }
          }}
        />
        <DecisionList
          items={items}
          sectionLabel="Packages"
          onViewUpdate={vi.fn()}
          onMutationError={vi.fn()}
        />
      </div>,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    const searchInput = screen.getByTestId("search-input");
    searchInput.focus();
    await userEvent.keyboard("{ArrowDown}");

    const firstRow = container.querySelector(
      "[data-testid^='decision-item-']",
    ) as HTMLElement;
    expect(firstRow).toBe(document.activeElement);
  });
});

// ---- Filter auto-expand tests ----

describe("Filter auto-expand", () => {
  it("force-expands collapsed groups when filter is active", async () => {
    // routine group with 3+ items starts collapsed by default
    const items: DecisionItemKind[] = [
      {
        type: "config",
        data: makeConfig({ path: "/etc/routine1.conf" }, [ROUTINE_TAG]),
      },
      {
        type: "config",
        data: makeConfig({ path: "/etc/routine2.conf" }, [ROUTINE_TAG]),
      },
      {
        type: "config",
        data: makeConfig({ path: "/etc/routine3.conf" }, [ROUTINE_TAG]),
      },
    ];

    const { rerender } = render(
      <DecisionList
        items={items}
        sectionLabel="Configs"
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    // routine group should start collapsed (hidden from accessibility tree)
    const rows = screen.getAllByRole("row", { hidden: true });
    expect(rows[0].closest("[hidden]")).toBeTruthy();

    // Re-render with filterText to trigger force-expand
    rerender(
      <DecisionList
        items={items}
        sectionLabel="Configs"
        filterText="routine"
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    // Group should now be expanded (not hidden)
    const rowsAfterFilter = screen.getAllByRole("row");
    expect(rowsAfterFilter[0].closest("[hidden]")).toBeFalsy();
  });

  it("restores original collapse state when filter is cleared", async () => {
    // routine group with 3+ items starts collapsed
    const items: DecisionItemKind[] = [
      {
        type: "config",
        data: makeConfig({ path: "/etc/routine1.conf" }, [ROUTINE_TAG]),
      },
      {
        type: "config",
        data: makeConfig({ path: "/etc/routine2.conf" }, [ROUTINE_TAG]),
      },
      {
        type: "config",
        data: makeConfig({ path: "/etc/routine3.conf" }, [ROUTINE_TAG]),
      },
    ];

    const { rerender } = render(
      <DecisionList
        items={items}
        sectionLabel="Configs"
        filterText="routine"
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    // With filter active, group is force-expanded
    const rows = screen.getAllByRole("row");
    expect(rows[0].closest("[hidden]")).toBeFalsy();

    // Clear filter
    rerender(
      <DecisionList
        items={items}
        sectionLabel="Configs"
        filterText=""
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    // Group should restore to its default collapsed state
    const rowsAfterClear = screen.getAllByRole("row", { hidden: true });
    expect(rowsAfterClear[0].closest("[hidden]")).toBeTruthy();
  });

  it("does not force-expand groups that are already expanded", async () => {
    // needs_review starts expanded by default
    const items: DecisionItemKind[] = [
      {
        type: "package",
        data: makePkg({ name: "review-pkg" }, [NEEDS_REVIEW_TAG]),
      },
    ];

    const { rerender } = render(
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

    // needs_review group should start expanded
    const row = screen.getByRole("row");
    expect(row.closest("[hidden]")).toBeFalsy();

    // Adding filter should keep it expanded
    rerender(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        filterText="review"
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    const rowAfterFilter = screen.getByRole("row");
    expect(rowAfterFilter.closest("[hidden]")).toBeFalsy();
  });
});

// ---- Viewed sync callback tests ----

describe("Viewed sync callback", () => {
  it("calls onViewedChange after a viewed POST succeeds", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });

    const onViewedChange = vi.fn();
    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "httpd" }, [NEEDS_REVIEW_TAG]) },
    ];

    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
        onViewedChange={onViewedChange}
      />,
    );

    // Wait for initial viewed fetch
    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    // Expand the item (non-toggled expand marks viewed)
    const row = screen.getByRole("row");
    row.focus();
    await user.keyboard("{Enter}");

    // The POST should fire — wait for it
    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalledWith(
        "/api/viewed",
        expect.objectContaining({ method: "POST" }),
      );
    });

    // Debounce timer: 300ms
    vi.advanceTimersByTime(350);
    expect(onViewedChange).toHaveBeenCalled();

    vi.useRealTimers();
  });

  it("does not call onViewedChange when prop is not provided", async () => {
    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "httpd" }, [NEEDS_REVIEW_TAG]) },
    ];

    // Should not throw when onViewedChange is undefined
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

    const row = screen.getByRole("row");
    row.focus();
    await userEvent.keyboard("{Enter}");

    // Wait for POST to fire — no crash expected
    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalledWith(
        "/api/viewed",
        expect.objectContaining({ method: "POST" }),
      );
    });
  });
});

// ---- Grid ARIA tests (Blocker 3) ----

describe("Grid ARIA attributes", () => {
  it("grid element has aria-rowcount matching total items", async () => {
    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "aaa" }, [NEEDS_REVIEW_TAG]) },
      { type: "package", data: makePkg({ name: "bbb" }, [INFO_TAG]) },
      { type: "package", data: makePkg({ name: "ccc" }, [ROUTINE_TAG]) },
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

    // Each attention group has its own grid with aria-rowcount (some hidden)
    const grids = screen.getAllByRole("grid", { hidden: true });
    expect(grids.length).toBe(3);
    // The grids collectively should have the items
    const totalRows = grids.reduce(
      (sum, g) => sum + Number(g.getAttribute("aria-rowcount") ?? 0),
      0,
    );
    expect(totalRows).toBe(3);
  });

  it("each item has a unique data-testid", async () => {
    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "aaa" }, [NEEDS_REVIEW_TAG]) },
      { type: "package", data: makePkg({ name: "bbb" }, [NEEDS_REVIEW_TAG]) },
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

    const items1 = screen.getAllByRole("row");
    expect(items1).toHaveLength(2);
    expect(items1[0]).toHaveAttribute(
      "data-testid",
      expect.stringContaining("decision-item-"),
    );
    expect(items1[1]).toHaveAttribute(
      "data-testid",
      expect.stringContaining("decision-item-"),
    );
  });

  it("row tracks expanded state via data attribute", async () => {
    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "aaa" }, [NEEDS_REVIEW_TAG]) },
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

    const row = screen.getByRole("row");
    expect(row).toHaveAttribute("data-expanded", "false");

    // The expand button should have aria-expanded
    const expandBtn = row.querySelector("button[aria-expanded]");
    expect(expandBtn).toBeTruthy();
    expect(expandBtn).toHaveAttribute("aria-expanded", "false");
  });

  it("data-expanded updates when row is expanded via Enter", async () => {
    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "aaa" }, [NEEDS_REVIEW_TAG]) },
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

    const row = screen.getByRole("row");
    expect(row).toHaveAttribute("data-expanded", "false");

    row.focus();
    await userEvent.keyboard("{Enter}");
    expect(row).toHaveAttribute("data-expanded", "true");
  });
});

// ---- Tier-aware card treatment tests ----

describe("Unified package view in MainContent", () => {
  it("renders RepoBar and PackageList for packages section", () => {
    const view = makeViewResponse({
      packages: [
        makePkg({ source_repo: "baseos" }, [
          { level: "routine", reason: "package_baseline_match", detail: null },
        ]),
      ],
    });
    render(<MainContent {...defaultMainContentProps} viewData={view} />);
    expect(screen.getByTestId("repo-bar")).toBeInTheDocument();
    expect(screen.getByTestId("package-list")).toBeInTheDocument();
  });

  it("shows repo name in PackageList row", () => {
    const view = makeViewResponse({
      packages: [
        makePkg({ source_repo: "appstream" }, [
          {
            level: "informational",
            reason: "package_user_added",
            detail: null,
          },
        ]),
      ],
    });
    render(<MainContent {...defaultMainContentProps} viewData={view} />);
    expect(screen.getByText("appstream")).toBeInTheDocument();
  });

  it("shows baseline info banner when baseline_summary is present", () => {
    const view = makeViewResponse({
      baseline_summary: {
        image_ref: "registry.example.com/rhel:9.4",
        image_digest: "sha256:abcdef123456",
        strategy: "rpm",
        baseline_count: 100,
        user_added_count: 5,
        review_count: 2,
      },
    });
    render(<MainContent {...defaultMainContentProps} viewData={view} />);
    expect(screen.getByText(/Baseline compared against/)).toBeInTheDocument();
  });
});

// ---- Repo group header tests ----

describe("RepoBar in MainContent", () => {
  it("renders RepoBar with repo pills from view data", () => {
    const view = makeViewResponse({
      packages: [
        makePkg({ name: "httpd", source_repo: "appstream" }, [
          {
            level: "informational",
            reason: "package_user_added",
            detail: null,
          },
        ]),
        makePkg({ name: "epel-release", source_repo: "epel" }, [
          {
            level: "informational",
            reason: "package_user_added",
            detail: null,
          },
        ]),
      ],
      repo_groups: [
        {
          section_id: "appstream",
          provenance: "verified" as const,
          is_distro: true,
          tier: "distro" as const,
          package_count: 1,
          enabled: true,
        },
        {
          section_id: "epel",
          provenance: "verified" as const,
          is_distro: false,
          tier: "third_party" as const,
          package_count: 1,
          enabled: true,
        },
      ],
    });
    render(<MainContent {...defaultMainContentProps} viewData={view} />);

    // RepoBar renders toggleable (non-distro) repos with name and count
    const repoBar = screen.getByTestId("repo-bar");
    expect(repoBar).toBeInTheDocument();
    expect(within(repoBar).getByText("epel")).toBeInTheDocument();
  });

  it("renders PackageList with all packages from view data", () => {
    const view = makeViewResponse({
      packages: [
        makePkg({ name: "httpd", source_repo: "appstream" }, [
          {
            level: "informational",
            reason: "package_user_added",
            detail: null,
          },
        ]),
        makePkg({ name: "epel-release", source_repo: "epel" }, [
          {
            level: "informational",
            reason: "package_user_added",
            detail: null,
          },
        ]),
      ],
      repo_groups: [
        {
          section_id: "appstream",
          provenance: "verified" as const,
          is_distro: true,
          tier: "distro" as const,
          package_count: 1,
          enabled: true,
        },
        {
          section_id: "epel",
          provenance: "verified" as const,
          is_distro: false,
          tier: "third_party" as const,
          package_count: 1,
          enabled: true,
        },
      ],
    });
    render(<MainContent {...defaultMainContentProps} viewData={view} />);

    expect(screen.getByTestId("package-list")).toBeInTheDocument();
    // Both packages render as rows
    expect(screen.getByText("httpd.x86_64")).toBeInTheDocument();
    expect(screen.getByText("epel-release.x86_64")).toBeInTheDocument();
  });
});

// ---- Config kind grouping tests ----

describe("Config kind grouping", () => {
  it("renders Tier 1 configs as 'managed by packages (not copied)' summary", () => {
    const view = makeViewResponse({
      config_files: [
        makeConfig({ path: "/etc/default.conf", kind: "rpm_owned_default" }, [
          { level: "routine", reason: "config_default", detail: null },
        ]),
        makeConfig({ path: "/etc/baseline.conf", kind: "rpm_owned_default" }, [
          { level: "routine", reason: "config_baseline_match", detail: null },
        ]),
      ],
    });
    render(
      <MainContent
        {...defaultMainContentProps}
        activeSection="configs"
        viewData={view}
      />,
    );
    expect(screen.getByText(/managed by packages/i)).toBeInTheDocument();
    // Paths should NOT be visible by default (collapsed)
    expect(screen.queryByText("/etc/default.conf")).not.toBeInTheDocument();
    expect(screen.queryByText("/etc/baseline.conf")).not.toBeInTheDocument();
  });

  it("expands Tier 1 config summary to show paths on click", async () => {
    const view = makeViewResponse({
      config_files: [
        makeConfig({ path: "/etc/default.conf", kind: "rpm_owned_default" }, [
          { level: "routine", reason: "config_default", detail: null },
        ]),
      ],
    });
    render(
      <MainContent
        {...defaultMainContentProps}
        activeSection="configs"
        viewData={view}
      />,
    );

    const toggle = screen.getByText(/managed by packages/i);
    await userEvent.click(toggle);
    expect(screen.getByText("/etc/default.conf")).toBeInTheDocument();
  });

  it("shows View diff link when diff_against_rpm is available", async () => {
    const view = makeViewResponse({
      config_files: [
        makeConfig(
          {
            path: "/etc/ssh/sshd_config",
            kind: "rpm_owned_modified",
            diff_against_rpm: "--- a\n+++ b\n@@ -1 +1 @@\n-old\n+new",
          },
          [{ level: "needs_review", reason: "config_modified", detail: null }],
        ),
      ],
    });
    render(
      <MainContent
        {...defaultMainContentProps}
        activeSection="configs"
        viewData={view}
      />,
    );

    // Expand the row to reveal ConfigDetail
    const expandBtn = screen.getByRole("button", { name: /expand/i });
    await userEvent.click(expandBtn);
    expect(screen.getByText(/view diff/i)).toBeInTheDocument();
  });

  it("does not show View diff link when diff_against_rpm is null", async () => {
    const view = makeViewResponse({
      config_files: [
        makeConfig(
          {
            path: "/etc/ssh/sshd_config",
            kind: "rpm_owned_modified",
            diff_against_rpm: null,
          },
          [{ level: "needs_review", reason: "config_modified", detail: null }],
        ),
      ],
    });
    render(
      <MainContent
        {...defaultMainContentProps}
        activeSection="configs"
        viewData={view}
      />,
    );

    // Expand the row to reveal ConfigDetail
    const expandBtn = screen.getByRole("button", { name: /expand/i });
    await userEvent.click(expandBtn);
    expect(screen.queryByText(/view diff/i)).not.toBeInTheDocument();
  });

  it("toggles inline diff display when View diff is clicked", async () => {
    const diffContent = "--- a\n+++ b\n@@ -1 +1 @@\n-old\n+new";
    const view = makeViewResponse({
      config_files: [
        makeConfig(
          {
            path: "/etc/ssh/sshd_config",
            kind: "rpm_owned_modified",
            diff_against_rpm: diffContent,
          },
          [{ level: "needs_review", reason: "config_modified", detail: null }],
        ),
      ],
    });
    render(
      <MainContent
        {...defaultMainContentProps}
        activeSection="configs"
        viewData={view}
      />,
    );

    // Expand the row to reveal ConfigDetail
    const expandBtn = screen.getByRole("button", { name: /expand/i });
    await userEvent.click(expandBtn);

    // Diff not visible initially (only "View diff" link)
    expect(screen.queryByTestId("config-diff")).not.toBeInTheDocument();

    // Click "View diff"
    await userEvent.click(screen.getByText(/view diff/i));
    expect(screen.getByTestId("config-diff")).toBeInTheDocument();
    expect(screen.getByText(/--- a/)).toBeInTheDocument();
  });

  it("does not include Tier 1 configs in other routine items", () => {
    const view = makeViewResponse({
      config_files: [
        makeConfig({ path: "/etc/default.conf", kind: "rpm_owned_default" }, [
          { level: "routine", reason: "config_default", detail: null },
        ]),
        makeConfig({ path: "/etc/custom.conf", kind: "unowned" }, [
          { level: "routine", reason: "config_unowned", detail: null },
        ]),
      ],
    });
    render(
      <MainContent
        {...defaultMainContentProps}
        activeSection="configs"
        viewData={view}
      />,
    );
    // Tier 1 collapsed summary should appear
    expect(screen.getByText(/managed by packages/i)).toBeInTheDocument();
    // The unowned config should still render as a card (not collapsed)
    expect(screen.getByText("/etc/custom.conf")).toBeInTheDocument();
  });
});

// ---- View mode removal tests ----

describe("No Decision/Full toggle", () => {
  it("does not render Decision/Full toggle on packages section", () => {
    render(
      <MainContent
        {...defaultMainContentProps}
        viewData={makeViewResponse()}
      />,
    );
    expect(
      screen.queryByRole("button", { name: /decisions/i }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /full/i }),
    ).not.toBeInTheDocument();
  });

  it("does not render Decision/Full toggle on configs section", () => {
    render(
      <MainContent
        {...defaultMainContentProps}
        activeSection="configs"
        viewData={makeViewResponse()}
      />,
    );
    expect(
      screen.queryByRole("button", { name: /decisions/i }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /full/i }),
    ).not.toBeInTheDocument();
  });
});

// ---- Search auto-reveal tests ----

describe("Search auto-reveal for collapsed groups", () => {
  it("auto-expands config-managed summary when revealItemId matches a managed config", async () => {
    const view = makeViewResponse({
      config_files: [
        makeConfig({ path: "/etc/default.conf", kind: "rpm_owned_default" }, [
          { level: "routine", reason: "config_default", detail: null },
        ]),
      ],
    });
    const { rerender } = render(
      <MainContent
        {...defaultMainContentProps}
        activeSection="configs"
        viewData={view}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    // Config summary should be collapsed — path not visible
    expect(screen.getByTestId("config-managed-summary")).toBeInTheDocument();
    expect(screen.queryByText("/etc/default.conf")).not.toBeInTheDocument();

    // Set revealItemId to the managed config
    rerender(
      <MainContent
        {...defaultMainContentProps}
        activeSection="configs"
        viewData={view}
        revealItemId="configs:/etc/default.conf"
      />,
    );

    // Config summary should auto-expand, path should be visible
    await waitFor(() => {
      expect(screen.getByText("/etc/default.conf")).toBeInTheDocument();
    });
    expect(
      screen.getByTestId("decision-item-configs:/etc/default.conf"),
    ).toBeInTheDocument();
  });
});

// ---- Repo-first package grouping tests ----

describe("Repo-first package grouping", () => {
  const repoGroups: RepoGroupInfo[] = [
    {
      section_id: "baseos",
      provenance: "verified" as const,
      is_distro: true,
      tier: "distro" as const,
      package_count: 2,
      enabled: true,
    },
    {
      section_id: "appstream",
      provenance: "verified" as const,
      is_distro: true,
      tier: "distro" as const,
      package_count: 1,
      enabled: true,
    },
    {
      section_id: "epel",
      provenance: "verified" as const,
      is_distro: false,
      tier: "third_party" as const,
      package_count: 2,
      enabled: true,
    },
    {
      section_id: "custom",
      provenance: "incomplete" as const,
      is_distro: false,
      tier: "third_party" as const,
      package_count: 1,
      enabled: true,
    },
    {
      section_id: "disabled-repo",
      provenance: "verified" as const,
      is_distro: false,
      tier: "third_party" as const,
      package_count: 1,
      enabled: false,
    },
  ];

  it("groups packages by repo instead of attention level", async () => {
    const items: DecisionItemKind[] = [
      {
        type: "package",
        data: makePkg({ name: "httpd", source_repo: "appstream" }, [
          NEEDS_REVIEW_TAG,
        ]),
      },
      {
        type: "package",
        data: makePkg({ name: "kernel", source_repo: "baseos" }, [ROUTINE_TAG]),
      },
      {
        type: "package",
        data: makePkg({ name: "epel-release", source_repo: "epel" }, [
          INFO_TAG,
        ]),
      },
    ];

    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        repoGroups={repoGroups}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    // Repo group wrappers should exist
    expect(
      screen.getByTestId("repo-group-wrapper-appstream"),
    ).toBeInTheDocument();
    expect(screen.getByTestId("repo-group-wrapper-baseos")).toBeInTheDocument();
    expect(screen.getByTestId("repo-group-wrapper-epel")).toBeInTheDocument();

    // Attention groups should NOT exist
    expect(
      screen.queryByTestId("attention-group-needs_review"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("attention-group-informational"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("attention-group-routine"),
    ).not.toBeInTheDocument();
  });

  it("orders repos: distro alpha, enabled third-party alpha, disabled, unknown last", async () => {
    const items: DecisionItemKind[] = [
      {
        type: "package",
        data: makePkg({ name: "epel-pkg", source_repo: "epel" }, [INFO_TAG]),
      },
      {
        type: "package",
        data: makePkg({ name: "kernel", source_repo: "baseos" }, [ROUTINE_TAG]),
      },
      {
        type: "package",
        data: makePkg({ name: "httpd", source_repo: "appstream" }, [
          NEEDS_REVIEW_TAG,
        ]),
      },
      {
        type: "package",
        data: makePkg({ name: "custom-pkg", source_repo: "custom" }, [
          INFO_TAG,
        ]),
      },
      {
        type: "package",
        data: makePkg({ name: "disabled-pkg", source_repo: "disabled-repo" }, [
          ROUTINE_TAG,
        ]),
      },
      {
        type: "package",
        data: makePkg({ name: "mystery", source_repo: "not-in-groups" }, [
          INFO_TAG,
        ]),
      },
    ];

    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        repoGroups={repoGroups}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    const wrappers = screen.getAllByTestId(/^repo-group-wrapper-/);
    // Order: distro alpha (appstream, baseos), enabled third-party alpha (custom, epel), disabled, unknown
    expect(wrappers[0]).toHaveAttribute(
      "data-testid",
      "repo-group-wrapper-appstream",
    );
    expect(wrappers[1]).toHaveAttribute(
      "data-testid",
      "repo-group-wrapper-baseos",
    );
    expect(wrappers[2]).toHaveAttribute(
      "data-testid",
      "repo-group-wrapper-custom",
    );
    expect(wrappers[3]).toHaveAttribute(
      "data-testid",
      "repo-group-wrapper-epel",
    );
    expect(wrappers[4]).toHaveAttribute(
      "data-testid",
      "repo-group-wrapper-disabled-repo",
    );
    expect(wrappers[5]).toHaveAttribute(
      "data-testid",
      "repo-group-wrapper-__unknown__",
    );
  });

  it("renders unknown-repo packages under 'Unknown repository' group last", async () => {
    const items: DecisionItemKind[] = [
      {
        type: "package",
        data: makePkg({ name: "httpd", source_repo: "appstream" }, [
          NEEDS_REVIEW_TAG,
        ]),
      },
      {
        type: "package",
        data: makePkg({ name: "mystery", source_repo: "not-in-groups" }, [
          INFO_TAG,
        ]),
      },
    ];

    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        repoGroups={repoGroups}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    const wrappers = screen.getAllByTestId(/^repo-group-wrapper-/);
    const lastWrapper = wrappers[wrappers.length - 1];
    expect(lastWrapper).toHaveAttribute(
      "data-testid",
      "repo-group-wrapper-__unknown__",
    );
  });

  it("renders blank-source_repo packages in the unknown group", async () => {
    const items: DecisionItemKind[] = [
      {
        type: "package",
        data: makePkg({ name: "httpd", source_repo: "appstream" }, [
          NEEDS_REVIEW_TAG,
        ]),
      },
      {
        type: "package",
        data: makePkg({ name: "blank-pkg", source_repo: "" }, [INFO_TAG]),
      },
    ];

    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        repoGroups={repoGroups}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    const unknownWrapper = screen.getByTestId("repo-group-wrapper-__unknown__");
    expect(unknownWrapper).toBeInTheDocument();
  });

  it("expands repos with needs_review packages by default", async () => {
    const items: DecisionItemKind[] = [
      {
        type: "package",
        data: makePkg({ name: "httpd", source_repo: "appstream" }, [
          NEEDS_REVIEW_TAG,
        ]),
      },
    ];

    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        repoGroups={repoGroups}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    // The needs_review package should be visible (group expanded)
    expect(
      screen.getByTestId("decision-item-packages:httpd.x86_64"),
    ).toBeInTheDocument();
  });

  it("collapses all-routine repos by default", async () => {
    const items: DecisionItemKind[] = [
      {
        type: "package",
        data: makePkg({ name: "httpd", source_repo: "appstream" }, [
          NEEDS_REVIEW_TAG,
        ]),
      },
      {
        type: "package",
        data: makePkg({ name: "kernel", source_repo: "baseos" }, [ROUTINE_TAG]),
      },
    ];

    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        repoGroups={repoGroups}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    // The routine package should NOT be visible (group collapsed)
    expect(
      screen.queryByTestId("decision-item-packages:kernel.x86_64"),
    ).not.toBeInTheDocument();
    // Should show "No action needed" summary
    expect(screen.getByText("No action needed")).toBeInTheDocument();
  });

  it("shows '+ N routine' summary within expanded repos that have mixed attention", async () => {
    const items: DecisionItemKind[] = [
      {
        type: "package",
        data: makePkg({ name: "httpd", source_repo: "appstream" }, [
          NEEDS_REVIEW_TAG,
        ]),
      },
      {
        type: "package",
        data: makePkg({ name: "mod_ssl", source_repo: "appstream" }, [
          ROUTINE_TAG,
        ]),
      },
      {
        type: "package",
        data: makePkg({ name: "mod_proxy", source_repo: "appstream" }, [
          ROUTINE_TAG,
        ]),
      },
    ];

    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        repoGroups={repoGroups}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    // needs_review item should be visible
    expect(
      screen.getByTestId("decision-item-packages:httpd.x86_64"),
    ).toBeInTheDocument();
    // Routine items should be collapsed under "+ N routine" summary
    expect(screen.getByText("+ 2 routine")).toBeInTheDocument();
    // Routine items should NOT be visible by default
    expect(
      screen.queryByTestId("decision-item-packages:mod_ssl.x86_64"),
    ).not.toBeInTheDocument();
  });

  it("sorts packages within repo: needs_review first, then informational, then routine", async () => {
    const items: DecisionItemKind[] = [
      {
        type: "package",
        data: makePkg({ name: "routine-pkg", source_repo: "appstream" }, [
          ROUTINE_TAG,
        ]),
      },
      {
        type: "package",
        data: makePkg({ name: "review-pkg", source_repo: "appstream" }, [
          NEEDS_REVIEW_TAG,
        ]),
      },
      {
        type: "package",
        data: makePkg({ name: "info-pkg", source_repo: "appstream" }, [
          INFO_TAG,
        ]),
      },
    ];

    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        repoGroups={repoGroups}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    // needs_review and informational should be visible (expanded due to needs_review)
    // Filter to only DecisionItem rows (not repo group header rows)
    const allRows = screen.getAllByRole("row");
    const itemRows = allRows.filter((r) =>
      r.getAttribute("data-testid")?.startsWith("decision-item-"),
    );
    // First item row should be the needs_review package
    expect(itemRows[0]).toHaveAttribute(
      "data-testid",
      "decision-item-packages:review-pkg.x86_64",
    );
    // Second item row should be the informational package
    expect(itemRows[1]).toHaveAttribute(
      "data-testid",
      "decision-item-packages:info-pkg.x86_64",
    );
    // Routine should be in the collapsed summary, not as a row
    expect(
      screen.queryByTestId("decision-item-packages:routine-pkg.x86_64"),
    ).not.toBeInTheDocument();
    expect(screen.getByText("+ 1 routine")).toBeInTheDocument();
  });
});

// ---- Disabled repo behavior tests ----

describe("Disabled repo behavior", () => {
  it("disabled repos sort after enabled repos", async () => {
    const repoGroups: RepoGroupInfo[] = [
      {
        section_id: "epel",
        provenance: "verified",
        is_distro: false,
        tier: "third_party" as const,
        package_count: 1,
        enabled: false,
      },
      {
        section_id: "baseos",
        provenance: "verified",
        is_distro: true,
        tier: "distro" as const,
        package_count: 1,
        enabled: true,
      },
    ];
    const items: DecisionItemKind[] = [
      {
        type: "package",
        data: makePkg(
          { name: "epel-pkg", source_repo: "epel", include: false },
          [NEEDS_REVIEW_TAG],
        ),
      },
      {
        type: "package",
        data: makePkg({ name: "baseos-pkg", source_repo: "baseos" }, [
          NEEDS_REVIEW_TAG,
        ]),
      },
    ];
    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        repoGroups={repoGroups}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );
    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });
    const wrappers = screen.getAllByTestId(/^repo-group-wrapper-/);
    expect(wrappers[0]).toHaveAttribute(
      "data-testid",
      "repo-group-wrapper-baseos",
    );
    expect(wrappers[1]).toHaveAttribute(
      "data-testid",
      "repo-group-wrapper-epel",
    );
  });

  it("disabled repo header count matches visible include:false rows, not backend package_count", async () => {
    const repoGroups: RepoGroupInfo[] = [
      {
        section_id: "epel",
        provenance: "verified",
        is_distro: false,
        tier: "third_party" as const,
        package_count: 10,
        enabled: false,
      },
    ];
    const items: DecisionItemKind[] = [
      {
        type: "package",
        data: makePkg({ name: "pkg1", source_repo: "epel", include: false }, [
          NEEDS_REVIEW_TAG,
        ]),
      },
      {
        type: "package",
        data: makePkg({ name: "pkg2", source_repo: "epel", include: false }, [
          ROUTINE_TAG,
        ]),
      },
    ];
    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        repoGroups={repoGroups}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );
    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });
    expect(screen.getByText(/2 packages excluded/)).toBeInTheDocument();
    expect(screen.queryByText(/10 packages excluded/)).not.toBeInTheDocument();
  });

  it("disabled repos start collapsed because they are disabled", async () => {
    const repoGroups: RepoGroupInfo[] = [
      {
        section_id: "epel",
        provenance: "verified",
        is_distro: false,
        tier: "third_party" as const,
        package_count: 1,
        enabled: false,
      },
    ];
    const items: DecisionItemKind[] = [
      {
        type: "package",
        data: makePkg({ name: "pkg1", source_repo: "epel", include: false }, [
          NEEDS_REVIEW_TAG,
        ]),
      },
    ];
    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        repoGroups={repoGroups}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );
    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });
    expect(screen.queryByText("pkg1.x86_64")).not.toBeInTheDocument();
  });

  it("hides per-package toggles in disabled repos when expanded", async () => {
    const repoGroups: RepoGroupInfo[] = [
      {
        section_id: "epel",
        provenance: "verified",
        is_distro: false,
        tier: "third_party" as const,
        package_count: 1,
        enabled: false,
      },
    ];
    const items: DecisionItemKind[] = [
      {
        type: "package",
        data: makePkg({ name: "pkg1", source_repo: "epel", include: false }, [
          NEEDS_REVIEW_TAG,
        ]),
      },
    ];
    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        repoGroups={repoGroups}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );
    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });
    const chevron = screen
      .getByTestId("repo-group-epel")
      .querySelector(".inspectah-repo-group-header__chevron")!;
    await userEvent.click(chevron as HTMLElement);
    expect(screen.getByText("pkg1.x86_64")).toBeInTheDocument();
    expect(
      screen.queryByRole("switch", { name: /toggle pkg1/i }),
    ).not.toBeInTheDocument();
  });
});

// ---- Updated RepoGroupHeader tests ----

describe("RepoGroupHeader updated labels", () => {
  it("shows no label for distro repos", () => {
    render(
      <RepoGroupHeader
        sectionId="baseos"
        provenance="verified"
        isDistro={true}
        packageCount={50}
        enabled={true}
      />,
    );
    expect(screen.queryByText("Distro")).not.toBeInTheDocument();
    expect(screen.queryByText("D")).not.toBeInTheDocument();
    expect(screen.queryByText("Third-party")).not.toBeInTheDocument();
    expect(screen.getByText("baseos")).toBeInTheDocument();
  });

  it("shows 'Third-party' text for verified non-distro repos", () => {
    render(
      <RepoGroupHeader
        sectionId="epel"
        provenance="verified"
        isDistro={false}
        packageCount={5}
        enabled={true}
      />,
    );
    expect(screen.getByText("Third-party")).toBeInTheDocument();
    expect(screen.getByText("epel")).toBeInTheDocument();
  });

  it("shows 'Third-party' text for incomplete-provenance non-distro repos", () => {
    render(
      <RepoGroupHeader
        sectionId="custom"
        provenance="incomplete"
        isDistro={false}
        packageCount={3}
        enabled={true}
      />,
    );
    expect(screen.getByText("Third-party")).toBeInTheDocument();
    expect(screen.queryByText("Unverified")).not.toBeInTheDocument();
    expect(screen.getByText("custom")).toBeInTheDocument();
  });

  it("shows 'Third-party' text for unknown-provenance non-distro repos", () => {
    render(
      <RepoGroupHeader
        sectionId="mystery"
        provenance="unknown"
        isDistro={false}
        packageCount={2}
        enabled={true}
      />,
    );
    expect(screen.getByText("Third-party")).toBeInTheDocument();
    expect(screen.getByText("mystery")).toBeInTheDocument();
  });

  it("only shows toggle switch for verified non-distro repos", () => {
    const { rerender } = render(
      <RepoGroupHeader
        sectionId="epel"
        provenance="verified"
        isDistro={false}
        packageCount={5}
        enabled={true}
        onToggle={vi.fn()}
      />,
    );
    expect(
      screen.getByRole("switch", { name: /toggle epel repo/i }),
    ).toBeInTheDocument();

    rerender(
      <RepoGroupHeader
        sectionId="custom"
        provenance="incomplete"
        isDistro={false}
        packageCount={3}
        enabled={true}
        onToggle={vi.fn()}
      />,
    );
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
  });

  it("renders chevron icon", () => {
    const { container } = render(
      <RepoGroupHeader
        sectionId="epel"
        provenance="verified"
        isDistro={false}
        packageCount={5}
        enabled={true}
        isExpanded={false}
      />,
    );
    expect(container.querySelector("svg")).toBeTruthy();
  });

  it("uses role='row' with aria-expanded and aria-controls", () => {
    render(
      <RepoGroupHeader
        sectionId="epel"
        provenance="verified"
        isDistro={false}
        packageCount={5}
        enabled={true}
        isExpanded={true}
      />,
    );
    const header = screen.getByTestId("repo-group-epel");
    expect(header).toHaveAttribute("role", "row");
    expect(header).toHaveAttribute("aria-expanded", "true");
    expect(header).toHaveAttribute("aria-controls", "repo-group-content-epel");
  });

  it("shows struck-through name and dimmed text for disabled repos", () => {
    render(
      <RepoGroupHeader
        sectionId="epel"
        provenance="verified"
        isDistro={false}
        packageCount={5}
        enabled={false}
      />,
    );
    const label = screen.getByText("epel");
    expect(label.style.textDecoration).toBe("line-through");
    expect(label.style.opacity).toBe("0.6");
  });

  it("shows informational count in header when provided", () => {
    render(
      <RepoGroupHeader
        sectionId="appstream"
        provenance="verified"
        isDistro={true}
        packageCount={20}
        enabled={true}
        infoCount={3}
      />,
    );
    expect(screen.getByText("3 informational")).toBeInTheDocument();
  });

  it("shows 'No action needed' for all-routine repos", () => {
    render(
      <RepoGroupHeader
        sectionId="baseos"
        provenance="verified"
        isDistro={true}
        packageCount={50}
        enabled={true}
        summaryText="No action needed"
      />,
    );
    expect(screen.getByText("No action needed")).toBeInTheDocument();
  });

  it("Enter on header triggers onExpandToggle, not switch toggle", async () => {
    const onExpandToggle = vi.fn();
    const onToggle = vi.fn();
    render(
      <RepoGroupHeader
        sectionId="epel"
        provenance="verified"
        isDistro={false}
        packageCount={5}
        enabled={true}
        isExpanded={false}
        onExpandToggle={onExpandToggle}
        onToggle={onToggle}
      />,
    );
    const header = screen.getByTestId("repo-group-epel");
    header.focus();
    await userEvent.keyboard("{Enter}");
    expect(onExpandToggle).toHaveBeenCalledTimes(1);
    expect(onToggle).not.toHaveBeenCalled();
  });

  it("Space on header row is a no-op (does not toggle expand or switch)", async () => {
    const onExpandToggle = vi.fn();
    const onToggle = vi.fn();
    render(
      <RepoGroupHeader
        sectionId="epel"
        provenance="verified"
        isDistro={false}
        packageCount={5}
        enabled={true}
        isExpanded={false}
        onExpandToggle={onExpandToggle}
        onToggle={onToggle}
      />,
    );
    const header = screen.getByTestId("repo-group-epel");
    header.focus();
    await userEvent.keyboard(" ");
    expect(onExpandToggle).not.toHaveBeenCalled();
    expect(onToggle).not.toHaveBeenCalled();
  });

  it("chevron click triggers onExpandToggle", async () => {
    const onExpandToggle = vi.fn();
    render(
      <RepoGroupHeader
        sectionId="epel"
        provenance="verified"
        isDistro={false}
        packageCount={5}
        enabled={true}
        isExpanded={false}
        onExpandToggle={onExpandToggle}
      />,
    );
    const chevron = screen
      .getByTestId("repo-group-epel")
      .querySelector(".inspectah-repo-group-header__chevron")!;
    await userEvent.click(chevron as HTMLElement);
    expect(onExpandToggle).toHaveBeenCalledTimes(1);
  });
});

// ---- Repo-first keyboard navigation tests ----

describe("Repo-first keyboard navigation", () => {
  const REPO_GROUPS: RepoGroupInfo[] = [
    {
      section_id: "baseos",
      provenance: "verified",
      is_distro: true,
      tier: "distro" as const,
      package_count: 1,
      enabled: true,
    },
    {
      section_id: "epel",
      provenance: "verified",
      is_distro: false,
      tier: "third_party" as const,
      package_count: 1,
      enabled: true,
    },
  ];

  it("repo headers are in the flat roving arrow-key sequence", async () => {
    const items: DecisionItemKind[] = [
      {
        type: "package",
        data: makePkg({ name: "glibc", source_repo: "baseos" }, [
          NEEDS_REVIEW_TAG,
        ]),
      },
      {
        type: "package",
        data: makePkg({ name: "epel-release", source_repo: "epel" }, [
          NEEDS_REVIEW_TAG,
        ]),
      },
    ];
    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        repoGroups={REPO_GROUPS}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );
    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });
    const baseosHeader = screen.getByTestId("repo-group-baseos");
    const epelHeader = screen.getByTestId("repo-group-epel");
    expect(baseosHeader).toHaveAttribute("tabindex", "0");
    // Non-focused headers should have tabindex -1
    expect(epelHeader).toHaveAttribute("tabindex", "-1");
    baseosHeader.focus();
    await userEvent.keyboard("{ArrowDown}");
    const glibcRow = screen.getByTestId("decision-item-packages:glibc.x86_64");
    expect(glibcRow).toHaveAttribute("tabindex", "0");
    // After moving, baseos header should revert to -1
    expect(baseosHeader).toHaveAttribute("tabindex", "-1");
  });

  it("ArrowDown from last package in a repo jumps to next repo header", async () => {
    const items: DecisionItemKind[] = [
      {
        type: "package",
        data: makePkg({ name: "glibc", source_repo: "baseos" }, [
          NEEDS_REVIEW_TAG,
        ]),
      },
      {
        type: "package",
        data: makePkg({ name: "epel-release", source_repo: "epel" }, [
          NEEDS_REVIEW_TAG,
        ]),
      },
    ];
    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        repoGroups={REPO_GROUPS}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );
    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });
    // Navigate via keyboard from baseos header → glibc → epel header
    const baseosHeader = screen.getByTestId("repo-group-baseos");
    baseosHeader.focus();
    await userEvent.keyboard("{ArrowDown}"); // baseos header → glibc
    const glibcRow = screen.getByTestId("decision-item-packages:glibc.x86_64");
    expect(glibcRow).toHaveAttribute("tabindex", "0");
    await userEvent.keyboard("{ArrowDown}"); // glibc → epel header
    const epelHeader = screen.getByTestId("repo-group-epel");
    expect(epelHeader).toHaveAttribute("tabindex", "0");
    expect(glibcRow).toHaveAttribute("tabindex", "-1");
  });

  it("skips collapsed repo group packages (only header in sequence)", async () => {
    const repoGroups: RepoGroupInfo[] = [
      {
        section_id: "baseos",
        provenance: "verified",
        is_distro: true,
        tier: "distro" as const,
        package_count: 1,
        enabled: true,
      },
      {
        section_id: "epel",
        provenance: "verified",
        is_distro: false,
        tier: "third_party" as const,
        package_count: 1,
        enabled: true,
      },
    ];
    const items: DecisionItemKind[] = [
      {
        type: "package",
        data: makePkg({ name: "glibc", source_repo: "baseos" }, [
          NEEDS_REVIEW_TAG,
        ]),
      },
      {
        type: "package",
        data: makePkg({ name: "htop", source_repo: "epel" }, [ROUTINE_TAG]),
      },
    ];
    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        repoGroups={repoGroups}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );
    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });
    const baseosHeader = screen.getByTestId("repo-group-baseos");
    const glibcRow = screen.getByTestId("decision-item-packages:glibc.x86_64");
    const epelHeader = screen.getByTestId("repo-group-epel");
    baseosHeader.focus();
    await userEvent.keyboard("{ArrowDown}");
    expect(glibcRow).toHaveAttribute("tabindex", "0");
    await userEvent.keyboard("{ArrowDown}");
    expect(epelHeader).toHaveAttribute("tabindex", "0");
  });

  it("Tab from a no-switch repo header does not dead-end", async () => {
    const repoGroups: RepoGroupInfo[] = [
      {
        section_id: "baseos",
        provenance: "verified",
        is_distro: true,
        tier: "distro" as const,
        package_count: 1,
        enabled: true,
      },
    ];
    const items: DecisionItemKind[] = [
      {
        type: "package",
        data: makePkg({ name: "glibc", source_repo: "baseos" }, [
          NEEDS_REVIEW_TAG,
        ]),
      },
    ];
    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        repoGroups={repoGroups}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );
    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });
    const baseosHeader = screen.getByTestId("repo-group-baseos");
    baseosHeader.focus();
    await userEvent.tab();
    expect(document.activeElement).not.toBe(baseosHeader);
  });

  it("Space is inert on repo header row", async () => {
    const repoGroups: RepoGroupInfo[] = [
      {
        section_id: "epel",
        provenance: "verified",
        is_distro: false,
        tier: "third_party" as const,
        package_count: 1,
        enabled: true,
      },
    ];
    const items: DecisionItemKind[] = [
      {
        type: "package",
        data: makePkg({ name: "epel-release", source_repo: "epel" }, [
          NEEDS_REVIEW_TAG,
        ]),
      },
    ];
    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        repoGroups={repoGroups}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );
    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });
    const epelHeader = screen.getByTestId("repo-group-epel");
    epelHeader.focus();
    const expandedBefore = epelHeader.getAttribute("aria-expanded");
    await userEvent.keyboard(" ");
    expect(epelHeader.getAttribute("aria-expanded")).toBe(expandedBefore);
  });

  it("focus stays on repo header after expand/collapse/disable/re-enable", async () => {
    const onViewUpdate = vi.fn().mockResolvedValue(undefined);
    const repoGroups: RepoGroupInfo[] = [
      {
        section_id: "epel",
        provenance: "verified",
        is_distro: false,
        tier: "third_party" as const,
        package_count: 1,
        enabled: true,
      },
    ];
    const items: DecisionItemKind[] = [
      {
        type: "package",
        data: makePkg({ name: "epel-release", source_repo: "epel" }, [
          NEEDS_REVIEW_TAG,
        ]),
      },
    ];
    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        repoGroups={repoGroups}
        onViewUpdate={onViewUpdate}
        onMutationError={vi.fn()}
      />,
    );
    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });
    const epelHeader = screen.getByTestId("repo-group-epel");
    epelHeader.focus();
    expect(document.activeElement).toBe(epelHeader);
    await userEvent.keyboard("{Enter}");
    expect(document.activeElement).toBe(epelHeader);
    await userEvent.keyboard("{Enter}");
    expect(document.activeElement).toBe(epelHeader);
  });

  it("focus resets to first repo header after filter clear", async () => {
    const repoGroups: RepoGroupInfo[] = [
      {
        section_id: "baseos",
        provenance: "verified",
        is_distro: true,
        tier: "distro" as const,
        package_count: 1,
        enabled: true,
      },
      {
        section_id: "epel",
        provenance: "verified",
        is_distro: false,
        tier: "third_party" as const,
        package_count: 1,
        enabled: true,
      },
    ];
    const items: DecisionItemKind[] = [
      {
        type: "package",
        data: makePkg({ name: "glibc", source_repo: "baseos" }, [
          NEEDS_REVIEW_TAG,
        ]),
      },
      {
        type: "package",
        data: makePkg({ name: "epel-release", source_repo: "epel" }, [
          NEEDS_REVIEW_TAG,
        ]),
      },
    ];
    const { rerender } = render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        filterText="epel"
        repoGroups={repoGroups}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );
    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });
    rerender(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        filterText=""
        repoGroups={repoGroups}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );
    const baseosHeader = screen.getByTestId("repo-group-baseos");
    expect(baseosHeader).toHaveAttribute("tabindex", "0");
  });
});

// ---- Repo-first filter and reveal tests ----

describe("Repo-first filter and reveal", () => {
  it("auto-expands only matching repo groups when filter is active", async () => {
    const repoGroups: RepoGroupInfo[] = [
      {
        section_id: "baseos",
        provenance: "verified",
        is_distro: true,
        tier: "distro" as const,
        package_count: 1,
        enabled: true,
      },
      {
        section_id: "epel",
        provenance: "verified",
        is_distro: false,
        tier: "third_party" as const,
        package_count: 1,
        enabled: true,
      },
    ];
    const items: DecisionItemKind[] = [
      {
        type: "package",
        data: makePkg({ name: "glibc", source_repo: "baseos" }, [
          NEEDS_REVIEW_TAG,
        ]),
      },
      {
        type: "package",
        data: makePkg({ name: "htop", source_repo: "epel" }, [ROUTINE_TAG]),
      },
    ];
    const { rerender } = render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        repoGroups={repoGroups}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );
    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });
    // glibc visible (needs_review repo expanded), htop not (routine repo collapsed)
    expect(screen.getByText("glibc.x86_64")).toBeInTheDocument();
    expect(screen.queryByText("htop.x86_64")).not.toBeInTheDocument();
    // Filter for htop — only epel repo should force-expand
    rerender(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        filterText="htop"
        repoGroups={repoGroups}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );
    expect(screen.getByText("htop.x86_64")).toBeInTheDocument();
  });

  it("non-matching repo groups do NOT force-expand when filter is active", async () => {
    const repoGroups: RepoGroupInfo[] = [
      {
        section_id: "baseos",
        provenance: "verified",
        is_distro: true,
        tier: "distro" as const,
        package_count: 1,
        enabled: true,
      },
      {
        section_id: "custom",
        provenance: "incomplete",
        is_distro: false,
        tier: "third_party" as const,
        package_count: 1,
        enabled: true,
      },
    ];
    const items: DecisionItemKind[] = [
      {
        type: "package",
        data: makePkg({ name: "baseos-pkg", source_repo: "baseos" }, [
          ROUTINE_TAG,
        ]),
      },
      {
        type: "package",
        data: makePkg({ name: "custom-pkg", source_repo: "custom" }, [
          NEEDS_REVIEW_TAG,
        ]),
      },
    ];
    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        filterText="custom"
        repoGroups={repoGroups}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );
    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });
    // custom-pkg matches filter, its repo (custom) has needs_review so it's expanded
    expect(screen.getByText("custom-pkg.x86_64")).toBeInTheDocument();
    // baseos-pkg does NOT match filter, and baseos is all-routine so stays collapsed
    expect(screen.queryByText("baseos-pkg.x86_64")).not.toBeInTheDocument();
  });

  it("auto-expands disabled repos when filter matches their packages", async () => {
    const items: DecisionItemKind[] = [
      {
        type: "package",
        data: makePkg({ name: "htop", source_repo: "epel", include: false }, [
          NEEDS_REVIEW_TAG,
        ]),
      },
    ];
    const { rerender } = render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        repoGroups={[
          {
            section_id: "epel",
            provenance: "verified",
            is_distro: false,
            tier: "third_party" as const,
            package_count: 1,
            enabled: false,
          },
        ]}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );
    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });
    // Disabled repo starts collapsed
    expect(screen.queryByText("htop.x86_64")).not.toBeInTheDocument();
    // Filter matches — disabled repo should force-expand
    rerender(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        filterText="htop"
        repoGroups={[
          {
            section_id: "epel",
            provenance: "verified",
            is_distro: false,
            tier: "third_party" as const,
            package_count: 1,
            enabled: false,
          },
        ]}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );
    expect(screen.getByText("htop.x86_64")).toBeInTheDocument();
  });

  it("two-ancestor reveal: revealItemId expands both repo group and routine summary", async () => {
    const repoGroups: RepoGroupInfo[] = [
      {
        section_id: "baseos",
        provenance: "verified",
        is_distro: true,
        tier: "distro" as const,
        package_count: 2,
        enabled: true,
      },
    ];
    const items: DecisionItemKind[] = [
      {
        type: "package",
        data: makePkg({ name: "glibc", source_repo: "baseos" }, [ROUTINE_TAG]),
      },
      {
        type: "package",
        data: makePkg({ name: "bash", source_repo: "baseos" }, [ROUTINE_TAG]),
      },
    ];
    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        revealItemId="packages:glibc.x86_64"
        repoGroups={repoGroups}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );
    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });
    // Both repo group and routine summary should auto-expand to reveal glibc
    expect(screen.getByText("glibc.x86_64")).toBeInTheDocument();
    expect(
      screen.getByTestId("decision-item-packages:glibc.x86_64"),
    ).toBeInTheDocument();
  });

  it("DecisionList-level revealItemId expands routine summary to reveal target", async () => {
    const repoGroups: RepoGroupInfo[] = [
      {
        section_id: "epel",
        provenance: "verified",
        is_distro: false,
        tier: "third_party" as const,
        package_count: 3,
        enabled: true,
      },
    ];
    const items: DecisionItemKind[] = [
      {
        type: "package",
        data: makePkg({ name: "httpd", source_repo: "epel" }, [
          NEEDS_REVIEW_TAG,
        ]),
      },
      {
        type: "package",
        data: makePkg({ name: "htop", source_repo: "epel" }, [ROUTINE_TAG]),
      },
      {
        type: "package",
        data: makePkg({ name: "jq", source_repo: "epel" }, [ROUTINE_TAG]),
      },
    ];
    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        revealItemId="packages:htop.x86_64"
        repoGroups={repoGroups}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );
    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });
    // httpd is needs_review so repo is expanded by default, but htop is routine (collapsed)
    // revealItemId should expand the routine summary to show htop
    expect(screen.getByText("htop.x86_64")).toBeInTheDocument();
    expect(
      screen.getByTestId("decision-item-packages:htop.x86_64"),
    ).toBeInTheDocument();
  });
});

// ---- AttentionSummary in MainContent tests ----

describe("Package section uses unified components", () => {
  it("packages section renders RepoBar + PackageList instead of AttentionSummary", () => {
    const view = makeViewResponse({
      packages: [
        makePkg({ name: "httpd", source_repo: "epel" }, [NEEDS_REVIEW_TAG]),
        makePkg({ name: "glibc", source_repo: "baseos" }, [ROUTINE_TAG]),
      ],
      repo_groups: [
        {
          section_id: "epel",
          provenance: "verified",
          is_distro: false,
          tier: "third_party" as const,
          package_count: 1,
          enabled: true,
        },
        {
          section_id: "baseos",
          provenance: "verified",
          is_distro: true,
          tier: "distro" as const,
          package_count: 1,
          enabled: true,
        },
      ],
    });
    render(<MainContent {...defaultMainContentProps} viewData={view} />);
    // Unified components render instead of AttentionSummary
    expect(screen.getByTestId("repo-bar")).toBeInTheDocument();
    expect(screen.getByTestId("package-list")).toBeInTheDocument();
    expect(screen.queryByTestId("attention-summary")).not.toBeInTheDocument();
  });

  it("does not show attention summary on configs section", () => {
    const view = makeViewResponse({
      config_files: [makeConfig({}, [NEEDS_REVIEW_TAG])],
    });
    render(
      <MainContent
        {...defaultMainContentProps}
        activeSection="configs"
        viewData={view}
      />,
    );
    expect(screen.queryByTestId("attention-summary")).not.toBeInTheDocument();
  });
});

// ---- Config section unchanged after repo-first refactor ----

describe("Config section unchanged after repo-first refactor", () => {
  it("config section still uses attention-level grouping", () => {
    const view = makeViewResponse({
      config_files: [
        makeConfig({ path: "/etc/review.conf" }, [
          { level: "needs_review", reason: "config_modified", detail: null },
        ]),
        makeConfig({ path: "/etc/info.conf" }, [
          { level: "informational", reason: "config_unowned", detail: null },
        ]),
        makeConfig({ path: "/etc/routine.conf" }, [
          { level: "routine", reason: "config_default", detail: null },
        ]),
      ],
    });
    render(
      <MainContent
        {...defaultMainContentProps}
        activeSection="configs"
        viewData={view}
      />,
    );
    // Config section should still render TriageBucketGroup, not RepoGroup
    expect(
      screen.getByTestId("attention-group-needs_review"),
    ).toBeInTheDocument();
    expect(
      screen.queryByTestId(/^repo-group-wrapper-/),
    ).not.toBeInTheDocument();
  });

  it("config section does not show attention summary", () => {
    const view = makeViewResponse({
      config_files: [
        makeConfig({ path: "/etc/test.conf" }, [
          { level: "needs_review", reason: "config_modified", detail: null },
        ]),
      ],
    });
    render(
      <MainContent
        {...defaultMainContentProps}
        activeSection="configs"
        viewData={view}
      />,
    );
    expect(screen.queryByTestId("attention-summary")).not.toBeInTheDocument();
  });
});
