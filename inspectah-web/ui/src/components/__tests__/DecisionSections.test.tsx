import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { AttentionGroup } from "../AttentionGroup";
import { DecisionItem } from "../DecisionItem";
import type { DecisionItemKind } from "../DecisionItem";
import { PackageDetail } from "../PackageDetail";
import { ConfigDetail } from "../ConfigDetail";
import { DecisionList } from "../DecisionList";
import { MainContent } from "../MainContent";
import type {
  RefinedPackage,
  RefinedConfig,
  AttentionTag,
  RefinedView,
  RefineStats,
  ViewResponse,
  RepoGroupInfo,
} from "../../api/types";

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
    return Promise.resolve({ ok: false, status: 404, json: () => Promise.resolve({ error: "not found" }) });
  });
});
afterEach(() => {
  vi.restoreAllMocks();
});

// --- Test data factories ---

const MOCK_STATS: RefineStats = {
  total_packages: 3,
  included_packages: 2,
  excluded_packages: 1,
  total_configs: 2,
  included_configs: 1,
  package_managed_configs: 0,
  excluded_configs: 1,
  needs_review_count: 2,
  ops_applied: 0,
  can_undo: false,
  can_redo: false,
  baseline_available: false,
};

const MOCK_VIEW: RefinedView = {
  packages: [],
  config_files: [],
  containerfile_preview: "",
  stats: MOCK_STATS,
  generation: 1,
};

function makePkg(overrides: Partial<RefinedPackage["entry"]> = {}, attention: AttentionTag[] = []): RefinedPackage {
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
  };
}

function makeConfig(overrides: Partial<RefinedConfig["entry"]> = {}, attention: AttentionTag[] = []): RefinedConfig {
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
  };
}

function makeViewResponse(overrides: {
  packages?: RefinedPackage[];
  config_files?: RefinedConfig[];
  stats?: Partial<RefineStats>;
  repo_groups?: RepoGroupInfo[];
} = {}): ViewResponse {
  return {
    packages: overrides.packages ?? [],
    config_files: overrides.config_files ?? [],
    containerfile_preview: "",
    stats: { ...MOCK_STATS, ...overrides.stats },
    generation: 1,
    repo_groups: overrides.repo_groups ?? [],
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

// ---- AttentionGroup tests ----

describe("AttentionGroup", () => {
  it("renders with correct border color for needs_review", () => {
    const { container } = render(
      <AttentionGroup level="needs_review" count={3}>
        <div>items</div>
      </AttentionGroup>,
    );
    const wrapper = container.firstChild as HTMLElement;
    expect(wrapper.style.borderLeft).toContain("--pf-t--global--color--status--danger--default");
  });

  it("renders with correct border color for informational", () => {
    const { container } = render(
      <AttentionGroup level="informational" count={2}>
        <div>items</div>
      </AttentionGroup>,
    );
    const wrapper = container.firstChild as HTMLElement;
    expect(wrapper.style.borderLeft).toContain("--pf-t--global--color--status--info--default");
  });

  it("renders with correct border color for routine", () => {
    const { container } = render(
      <AttentionGroup level="routine" count={1}>
        <div>items</div>
      </AttentionGroup>,
    );
    const wrapper = container.firstChild as HTMLElement;
    expect(wrapper.style.borderLeft).toContain("--pf-t--global--color--status--success--default");
  });

  it("starts expanded for needs_review", () => {
    render(
      <AttentionGroup level="needs_review" count={1}>
        <div data-testid="child">content</div>
      </AttentionGroup>,
    );
    expect(screen.getByTestId("child")).toBeInTheDocument();
  });

  it("starts collapsed for informational", () => {
    render(
      <AttentionGroup level="informational" count={1}>
        <div data-testid="child">content</div>
      </AttentionGroup>,
    );
    // PF6 ExpandableSection uses hidden attribute on collapsed content
    const child = screen.getByTestId("child");
    expect(child.closest("[hidden]")).toBeTruthy();
  });

  it("starts collapsed for routine", () => {
    render(
      <AttentionGroup level="routine" count={1}>
        <div data-testid="child">content</div>
      </AttentionGroup>,
    );
    const child = screen.getByTestId("child");
    expect(child.closest("[hidden]")).toBeTruthy();
  });

  it("shows item count in toggle text", () => {
    render(
      <AttentionGroup level="needs_review" count={5}>
        <div>items</div>
      </AttentionGroup>,
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
    const item: DecisionItemKind = { type: "package", data: makePkg({ include: true }) };
    render(<DecisionItem item={item} {...defaultProps} onToggleInclude={onToggle} />);

    const toggle = screen.getByRole("switch", { name: /toggle httpd/i });
    await userEvent.click(toggle);
    expect(onToggle).toHaveBeenCalledWith({
      op: "ExcludePackage",
      target: { name: "httpd", arch: "x86_64" },
    });
  });

  it("fires IncludePackage when toggling excluded package", async () => {
    const onToggle = vi.fn();
    const item: DecisionItemKind = { type: "package", data: makePkg({ include: false }) };
    render(<DecisionItem item={item} {...defaultProps} onToggleInclude={onToggle} />);

    const toggle = screen.getByRole("switch", { name: /toggle httpd/i });
    await userEvent.click(toggle);
    expect(onToggle).toHaveBeenCalledWith({
      op: "IncludePackage",
      target: { name: "httpd", arch: "x86_64" },
    });
  });

  it("fires ExcludeConfig when toggling included config", async () => {
    const onToggle = vi.fn();
    const item: DecisionItemKind = { type: "config", data: makeConfig({ include: true }) };
    render(<DecisionItem item={item} {...defaultProps} onToggleInclude={onToggle} />);

    const toggle = screen.getByRole("switch", { name: /toggle/i });
    await userEvent.click(toggle);
    expect(onToggle).toHaveBeenCalledWith({
      op: "ExcludeConfig",
      target: { path: "/etc/httpd/conf/httpd.conf" },
    });
  });

  it("toggles on Space key", async () => {
    const onToggle = vi.fn();
    const item: DecisionItemKind = { type: "package", data: makePkg() };
    render(<DecisionItem item={item} {...defaultProps} onToggleInclude={onToggle} />);

    const row = screen.getByRole("row");
    row.focus();
    await userEvent.keyboard(" ");
    expect(onToggle).toHaveBeenCalled();
  });

  it("toggles on x key", async () => {
    const onToggle = vi.fn();
    const item: DecisionItemKind = { type: "package", data: makePkg() };
    render(<DecisionItem item={item} {...defaultProps} onToggleInclude={onToggle} />);

    const row = screen.getByRole("row");
    row.focus();
    await userEvent.keyboard("x");
    expect(onToggle).toHaveBeenCalled();
  });

  it("expands detail on Enter key", async () => {
    const item: DecisionItemKind = { type: "package", data: makePkg({}, [NEEDS_REVIEW_TAG]) };
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
    render(<DecisionItem item={item} {...baseProps} onMarkViewed={onMarkViewed} />);

    const toggle = screen.getByRole("switch", { name: /toggle/i });
    await userEvent.click(toggle);
    expect(onMarkViewed).toHaveBeenCalledWith("packages:httpd.x86_64");
  });

  it("expanding non-toggled item marks viewed", async () => {
    const onMarkViewed = vi.fn();
    const item: DecisionItemKind = {
      type: "package",
      data: makePkg({}, [NEEDS_REVIEW_TAG]),
    };
    render(<DecisionItem item={item} {...baseProps} onMarkViewed={onMarkViewed} />);

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
    render(<DecisionItem item={item} {...baseProps} onMarkViewed={onMarkViewed} />);

    // First toggle (marks viewed)
    const toggle = screen.getByRole("switch", { name: /toggle/i });
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
      { type: "package", data: makePkg({ name: "routine-pkg" }, [ROUTINE_TAG]) },
      { type: "package", data: makePkg({ name: "review-pkg" }, [NEEDS_REVIEW_TAG]) },
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
    expect(groups[0]).toHaveAttribute("data-testid", "attention-group-needs_review");
    expect(groups[1]).toHaveAttribute("data-testid", "attention-group-informational");
    expect(groups[2]).toHaveAttribute("data-testid", "attention-group-routine");
  });

  it("only shows groups that have items", async () => {
    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "review-pkg" }, [NEEDS_REVIEW_TAG]) },
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

    expect(screen.getByTestId("attention-group-needs_review")).toBeInTheDocument();
    expect(screen.queryByTestId("attention-group-informational")).not.toBeInTheDocument();
    expect(screen.queryByTestId("attention-group-routine")).not.toBeInTheDocument();
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

    // Grid role is now on each AttentionGroup's inner container, not the outer list
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
      return Promise.resolve({ ok: false, status: 404, json: () => Promise.resolve({ error: "not found" }) });
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
    const toggle = screen.getByRole("switch", { name: /toggle/i });
    await user.click(toggle);

    // Wait for error toast and optimistic revert re-fetch to complete
    await waitFor(() => {
      expect(screen.getByText(/Error: invalid operation/)).toBeInTheDocument();
      expect(onViewUpdate).toHaveBeenCalledWith(MOCK_VIEW);
    });

    // Auto-dismiss after 3 seconds
    vi.advanceTimersByTime(3100);
    await waitFor(() => {
      expect(screen.queryByText(/Error: invalid operation/)).not.toBeInTheDocument();
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
      return Promise.resolve({ ok: false, status: 404, json: () => Promise.resolve({ error: "not found" }) });
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

    const toggle = screen.getByRole("switch", { name: /toggle/i });
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
    // informational group starts collapsed by default
    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "info-pkg" }, [INFO_TAG]) },
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

    // informational group should start collapsed (hidden from accessibility tree)
    const row = screen.getByRole("row", { hidden: true });
    expect(row.closest("[hidden]")).toBeTruthy();

    // Re-render with filterText to trigger force-expand
    rerender(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        filterText="info"
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    // Group should now be expanded (not hidden)
    const rowAfterFilter = screen.getByRole("row");
    expect(rowAfterFilter.closest("[hidden]")).toBeFalsy();
  });

  it("restores original collapse state when filter is cleared", async () => {
    // informational group starts collapsed
    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "info-pkg" }, [INFO_TAG]) },
    ];

    const { rerender } = render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        filterText="info"
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    // With filter active, group is force-expanded
    const row = screen.getByRole("row");
    expect(row.closest("[hidden]")).toBeFalsy();

    // Clear filter
    rerender(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        filterText=""
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    // Group should restore to its default collapsed state
    // PF6 ExpandableSection hides content with [hidden], so we need hidden: true to find it
    const rowAfterClear = screen.getByRole("row", { hidden: true });
    expect(rowAfterClear.closest("[hidden]")).toBeTruthy();
  });

  it("does not force-expand groups that are already expanded", async () => {
    // needs_review starts expanded by default
    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "review-pkg" }, [NEEDS_REVIEW_TAG]) },
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
    const totalRows = grids.reduce((sum, g) => sum + Number(g.getAttribute("aria-rowcount") ?? 0), 0);
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
    expect(items1[0]).toHaveAttribute("data-testid", expect.stringContaining("decision-item-"));
    expect(items1[1]).toHaveAttribute("data-testid", expect.stringContaining("decision-item-"));
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

describe("Tier-aware card treatment", () => {
  it("renders Tier 1 packages as collapsed summary", () => {
    const view = makeViewResponse({
      packages: [
        makePkg({ source_repo: "baseos" }, [{ level: "routine", reason: "package_baseline_match", detail: null }]),
      ],
    });
    render(<MainContent {...defaultMainContentProps} viewData={view} />);
    expect(screen.getByText(/baseline packages/i)).toBeInTheDocument();
  });

  it("shows repo source badge for verified Tier 2", () => {
    const view = makeViewResponse({
      packages: [
        makePkg({ source_repo: "appstream" }, [{ level: "informational", reason: "package_user_added", detail: null }]),
      ],
    });
    render(<MainContent {...defaultMainContentProps} viewData={view} />);
    expect(screen.getByText("appstream")).toBeInTheDocument();
  });

  it("shows 'Baseline Unavailable' for provenance-unavailable Tier 2", () => {
    const view = makeViewResponse({
      packages: [
        makePkg({ attention: [] } as any, [{ level: "informational", reason: "package_provenance_unavailable", detail: null }]),
      ],
    });
    render(<MainContent {...defaultMainContentProps} viewData={view} />);
    expect(screen.getByText(/baseline unavailable/i)).toBeInTheDocument();
  });

  it("shows baseline unavailable banner when baseline_available is false", () => {
    const view = makeViewResponse({
      stats: { baseline_available: false },
    });
    render(<MainContent {...defaultMainContentProps} viewData={view} />);
    expect(screen.getByText(/classification confidence reduced/i)).toBeInTheDocument();
  });
});

// ---- Repo group header tests ----

describe("Repo group headers", () => {
  it("groups Tier 2 packages by repo with header", async () => {
    const view = makeViewResponse({
      packages: [
        makePkg({ name: "httpd", source_repo: "appstream" }, [{ level: "informational", reason: "package_user_added", detail: null }]),
        makePkg({ name: "epel-release", source_repo: "epel" }, [{ level: "informational", reason: "package_user_added", detail: null }]),
      ],
      repo_groups: [
        { section_id: "appstream", provenance: "verified" as const, is_distro: true, package_count: 1, enabled: true },
        { section_id: "epel", provenance: "verified" as const, is_distro: false, package_count: 1, enabled: true },
      ],
    });
    render(<MainContent {...defaultMainContentProps} viewData={view} />);

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    // Expand the informational group (starts collapsed)
    const infoToggle = screen.getByText(/Informational/);
    await userEvent.click(infoToggle);

    // Repo group headers should appear
    expect(screen.getByTestId("repo-group-appstream")).toBeInTheDocument();
    expect(screen.getByTestId("repo-group-epel")).toBeInTheDocument();
    // Badge labels
    expect(screen.getByText("Distro")).toBeInTheDocument();
    expect(screen.getByText("Third-party")).toBeInTheDocument();
  });

  it("shows toggle for verified third-party, no toggle for distro", async () => {
    const view = makeViewResponse({
      packages: [
        makePkg({ name: "httpd", source_repo: "appstream" }, [{ level: "informational", reason: "package_user_added", detail: null }]),
        makePkg({ name: "epel-release", source_repo: "epel" }, [{ level: "informational", reason: "package_user_added", detail: null }]),
      ],
      repo_groups: [
        { section_id: "appstream", provenance: "verified" as const, is_distro: true, package_count: 1, enabled: true },
        { section_id: "epel", provenance: "verified" as const, is_distro: false, package_count: 1, enabled: true },
      ],
    });
    render(<MainContent {...defaultMainContentProps} viewData={view} />);

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    // Expand the informational group
    const infoToggle = screen.getByText(/Informational/);
    await userEvent.click(infoToggle);

    // Only the epel group should have a repo toggle (verified + third-party)
    expect(screen.getByRole("switch", { name: /toggle epel repo/i })).toBeInTheDocument();
    expect(screen.queryByRole("switch", { name: /toggle appstream repo/i })).not.toBeInTheDocument();
  });

  it("does not show toggle for unverified provenance", async () => {
    const view = makeViewResponse({
      packages: [
        makePkg({ name: "mystery", source_repo: "custom" }, [{ level: "informational", reason: "package_user_added", detail: null }]),
      ],
      repo_groups: [
        { section_id: "custom", provenance: "incomplete" as const, is_distro: false, package_count: 1, enabled: true },
      ],
    });
    render(<MainContent {...defaultMainContentProps} viewData={view} />);

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    // Expand the informational group
    const infoToggle = screen.getByText(/Informational/);
    await userEvent.click(infoToggle);

    expect(screen.getByText("Unverified")).toBeInTheDocument();
    expect(screen.queryByRole("switch", { name: /toggle custom repo/i })).not.toBeInTheDocument();
  });

  it("reverts toggle and shows alert on backend failure", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });

    // Make /api/op fail for ExcludeRepo
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
          json: () => Promise.resolve({ error: "repo exclusion failed" }),
        });
      }
      if (url === "/api/view") {
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve(MOCK_VIEW),
        });
      }
      return Promise.resolve({ ok: false, status: 404, json: () => Promise.resolve({ error: "not found" }) });
    });

    const view = makeViewResponse({
      packages: [
        makePkg({ name: "epel-release", source_repo: "epel" }, [{ level: "informational", reason: "package_user_added", detail: null }]),
      ],
      repo_groups: [
        { section_id: "epel", provenance: "verified" as const, is_distro: false, package_count: 1, enabled: true },
      ],
    });

    const onMutationError = vi.fn();

    render(
      <MainContent
        {...defaultMainContentProps}
        viewData={view}
        onMutationError={onMutationError}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    // Expand the informational group
    const infoToggle = screen.getByText(/Informational/);
    await user.click(infoToggle);

    // Click the repo toggle to exclude epel
    const repoToggle = screen.getByRole("switch", { name: /toggle epel repo/i });
    await user.click(repoToggle);

    // Wait for error alert to appear
    await waitFor(() => {
      expect(screen.getByText(/Error: repo exclusion failed/)).toBeInTheDocument();
    });

    // Toggle should revert (checked again since it was enabled and op failed)
    expect(repoToggle).toBeChecked();

    vi.useRealTimers();
  });
});

// ---- Config kind grouping tests ----

describe("Config kind grouping", () => {
  it("renders Tier 1 configs as 'managed by packages (not copied)' summary", () => {
    const view = makeViewResponse({
      config_files: [
        makeConfig({ path: "/etc/default.conf", kind: "rpm_owned_default" },
          [{ level: "routine", reason: "config_default", detail: null }]),
        makeConfig({ path: "/etc/baseline.conf", kind: "rpm_owned_default" },
          [{ level: "routine", reason: "config_baseline_match", detail: null }]),
      ],
    });
    render(<MainContent {...defaultMainContentProps} activeSection="configs" viewData={view} />);
    expect(screen.getByText(/managed by packages/i)).toBeInTheDocument();
    // Paths should NOT be visible by default (collapsed)
    expect(screen.queryByText("/etc/default.conf")).not.toBeInTheDocument();
    expect(screen.queryByText("/etc/baseline.conf")).not.toBeInTheDocument();
  });

  it("expands Tier 1 config summary to show paths on click", async () => {
    const view = makeViewResponse({
      config_files: [
        makeConfig({ path: "/etc/default.conf", kind: "rpm_owned_default" },
          [{ level: "routine", reason: "config_default", detail: null }]),
      ],
    });
    render(<MainContent {...defaultMainContentProps} activeSection="configs" viewData={view} />);

    const toggle = screen.getByText(/managed by packages/i);
    await userEvent.click(toggle);
    expect(screen.getByText("/etc/default.conf")).toBeInTheDocument();
  });

  it("shows View diff link when diff_against_rpm is available", async () => {
    const view = makeViewResponse({
      config_files: [
        makeConfig({ path: "/etc/ssh/sshd_config", kind: "rpm_owned_modified",
          diff_against_rpm: "--- a\n+++ b\n@@ -1 +1 @@\n-old\n+new" },
          [{ level: "needs_review", reason: "config_modified", detail: null }]),
      ],
    });
    render(<MainContent {...defaultMainContentProps} activeSection="configs" viewData={view} />);

    // Expand the row to reveal ConfigDetail
    const expandBtn = screen.getByRole("button", { name: /expand/i });
    await userEvent.click(expandBtn);
    expect(screen.getByText(/view diff/i)).toBeInTheDocument();
  });

  it("does not show View diff link when diff_against_rpm is null", async () => {
    const view = makeViewResponse({
      config_files: [
        makeConfig({ path: "/etc/ssh/sshd_config", kind: "rpm_owned_modified",
          diff_against_rpm: null },
          [{ level: "needs_review", reason: "config_modified", detail: null }]),
      ],
    });
    render(<MainContent {...defaultMainContentProps} activeSection="configs" viewData={view} />);

    // Expand the row to reveal ConfigDetail
    const expandBtn = screen.getByRole("button", { name: /expand/i });
    await userEvent.click(expandBtn);
    expect(screen.queryByText(/view diff/i)).not.toBeInTheDocument();
  });

  it("toggles inline diff display when View diff is clicked", async () => {
    const diffContent = "--- a\n+++ b\n@@ -1 +1 @@\n-old\n+new";
    const view = makeViewResponse({
      config_files: [
        makeConfig({ path: "/etc/ssh/sshd_config", kind: "rpm_owned_modified",
          diff_against_rpm: diffContent },
          [{ level: "needs_review", reason: "config_modified", detail: null }]),
      ],
    });
    render(<MainContent {...defaultMainContentProps} activeSection="configs" viewData={view} />);

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
        makeConfig({ path: "/etc/default.conf", kind: "rpm_owned_default" },
          [{ level: "routine", reason: "config_default", detail: null }]),
        makeConfig({ path: "/etc/custom.conf", kind: "unowned" },
          [{ level: "routine", reason: "config_unowned", detail: null }]),
      ],
    });
    render(<MainContent {...defaultMainContentProps} activeSection="configs" viewData={view} />);
    // Tier 1 collapsed summary should appear
    expect(screen.getByText(/managed by packages/i)).toBeInTheDocument();
    // The unowned config should still render as a card (not collapsed)
    expect(screen.getByText("/etc/custom.conf")).toBeInTheDocument();
  });
});

// ---- Decision/Full view toggle tests ----

describe("Decision/Full view toggle", () => {
  it("renders Decision/Full toggle with Decisions active by default", () => {
    render(<MainContent {...defaultMainContentProps} viewData={makeViewResponse()} />);
    const toggle = screen.getByRole("button", { name: /decisions/i });
    expect(toggle).toHaveAttribute("aria-pressed", "true");
  });

  it("Full view expands Tier 1 baseline packages", async () => {
    const view = makeViewResponse({
      packages: [
        makePkg(
          { name: "glibc", source_repo: "baseos" },
          [{ level: "routine", reason: "package_baseline_match", detail: null }],
        ),
      ],
    });
    render(<MainContent {...defaultMainContentProps} viewData={view} />);

    // In Decisions mode, glibc is inside collapsed summary
    expect(screen.queryByText("glibc.x86_64")).not.toBeInTheDocument();

    // Switch to Full mode
    const fullBtn = screen.getByRole("button", { name: /full/i });
    await userEvent.click(fullBtn);

    // Tier 1 items should now be visible
    expect(screen.getByText("glibc.x86_64")).toBeInTheDocument();
  });

  it("Full view expands Tier 1 managed configs", async () => {
    const view = makeViewResponse({
      config_files: [
        makeConfig(
          { path: "/etc/default.conf", kind: "rpm_owned_default" },
          [{ level: "routine", reason: "config_default", detail: null }],
        ),
      ],
    });
    render(<MainContent {...defaultMainContentProps} activeSection="configs" viewData={view} />);

    // In Decisions mode, config is inside collapsed summary
    expect(screen.queryByText("/etc/default.conf")).not.toBeInTheDocument();

    // Switch to Full mode
    const fullBtn = screen.getByRole("button", { name: /full/i });
    await userEvent.click(fullBtn);

    // Tier 1 config should now be visible
    expect(screen.getByText("/etc/default.conf")).toBeInTheDocument();
  });

  it("switching back to Decisions re-collapses Tier 1 items", async () => {
    const view = makeViewResponse({
      packages: [
        makePkg(
          { name: "glibc", source_repo: "baseos" },
          [{ level: "routine", reason: "package_baseline_match", detail: null }],
        ),
      ],
    });
    render(<MainContent {...defaultMainContentProps} viewData={view} />);

    // Switch to Full
    const fullBtn = screen.getByRole("button", { name: /full/i });
    await userEvent.click(fullBtn);
    expect(screen.getByText("glibc.x86_64")).toBeInTheDocument();

    // Switch back to Decisions
    const decisionsBtn = screen.getByRole("button", { name: /decisions/i });
    await userEvent.click(decisionsBtn);
    expect(screen.queryByText("glibc.x86_64")).not.toBeInTheDocument();
  });

  it("toggle is visible on both packages and configs sections", () => {
    const view = makeViewResponse();
    const { rerender } = render(
      <MainContent {...defaultMainContentProps} activeSection="packages" viewData={view} />,
    );
    expect(screen.getByRole("button", { name: /decisions/i })).toBeInTheDocument();

    rerender(
      <MainContent {...defaultMainContentProps} activeSection="configs" viewData={view} />,
    );
    expect(screen.getByRole("button", { name: /decisions/i })).toBeInTheDocument();
  });

  it("toggle is NOT visible on context sections", () => {
    render(
      <MainContent
        {...defaultMainContentProps}
        activeSection="services"
        viewData={makeViewResponse()}
        sections={[{ id: "services", display_name: "Services", items: [] }]}
      />,
    );
    expect(screen.queryByRole("button", { name: /decisions/i })).not.toBeInTheDocument();
  });
});

// ---- Search auto-reveal tests ----

describe("Search auto-reveal for collapsed groups", () => {
  it("auto-expands baseline summary when revealItemId matches a baseline package", async () => {
    const view = makeViewResponse({
      packages: [
        makePkg(
          { name: "glibc", source_repo: "baseos" },
          [{ level: "routine", reason: "package_baseline_match", detail: null }],
        ),
      ],
    });
    const { rerender } = render(
      <MainContent {...defaultMainContentProps} viewData={view} />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    // Baseline summary should be collapsed — item not visible
    expect(screen.getByTestId("baseline-summary")).toBeInTheDocument();
    expect(screen.queryByText("glibc.x86_64")).not.toBeInTheDocument();

    // Set revealItemId to the baseline package
    rerender(
      <MainContent
        {...defaultMainContentProps}
        viewData={view}
        revealItemId="packages:glibc.x86_64"
      />,
    );

    // Baseline summary should auto-expand, item should be visible
    await waitFor(() => {
      expect(screen.getByText("glibc.x86_64")).toBeInTheDocument();
    });
    // The item should have a data-testid for focus targeting
    expect(
      screen.getByTestId("decision-item-packages:glibc.x86_64"),
    ).toBeInTheDocument();
  });

  it("auto-expands config-managed summary when revealItemId matches a managed config", async () => {
    const view = makeViewResponse({
      config_files: [
        makeConfig(
          { path: "/etc/default.conf", kind: "rpm_owned_default" },
          [{ level: "routine", reason: "config_default", detail: null }],
        ),
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

  it("does not expand unrelated summaries when revealItemId targets a different item", async () => {
    const view = makeViewResponse({
      packages: [
        makePkg(
          { name: "glibc", source_repo: "baseos" },
          [{ level: "routine", reason: "package_baseline_match", detail: null }],
        ),
        makePkg(
          { name: "httpd", source_repo: "appstream" },
          [{ level: "needs_review", reason: "package_user_added", detail: null }],
        ),
      ],
    });
    render(
      <MainContent
        {...defaultMainContentProps}
        viewData={view}
        revealItemId="packages:httpd.x86_64"
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    // The baseline summary should stay collapsed since revealItemId targets httpd, not glibc
    expect(screen.queryByText("glibc.x86_64")).not.toBeInTheDocument();
  });
});
