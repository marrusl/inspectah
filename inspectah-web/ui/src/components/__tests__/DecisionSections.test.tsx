import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { AttentionGroup } from "../AttentionGroup";
import { DecisionItem } from "../DecisionItem";
import type { DecisionItemKind } from "../DecisionItem";
import { PackageDetail } from "../PackageDetail";
import { ConfigDetail } from "../ConfigDetail";
import { DecisionList } from "../DecisionList";
import type {
  RefinedPackage,
  RefinedConfig,
  AttentionTag,
  RefinedView,
  RefineStats,
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
  excluded_configs: 1,
  needs_review_count: 2,
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

const NEEDS_REVIEW_TAG: AttentionTag = {
  level: "needs_review",
  reason: "package_not_in_baseline",
  detail: "Not found in base image",
};

const INFO_TAG: AttentionTag = {
  level: "informational",
  reason: "package_state_changed",
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
    expect(wrapper.style.borderLeft).toContain("--pf-t--global--color--status--warning--default");
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
    expect(screen.getByText("Package Not In Baseline")).toBeInTheDocument();
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
    expect(onMarkViewed).toHaveBeenCalledWith("pkg:httpd:x86_64");
  });

  it("expanding non-toggled item marks viewed", async () => {
    const onMarkViewed = vi.fn();
    const item: DecisionItemKind = {
      type: "package",
      data: makePkg({}, [NEEDS_REVIEW_TAG]),
    };
    render(<DecisionItem item={item} {...baseProps} onMarkViewed={onMarkViewed} />);

    const expandBtn = screen.getByRole("button", { name: /expand/i });
    await userEvent.click(expandBtn);
    expect(onMarkViewed).toHaveBeenCalledWith("pkg:httpd:x86_64");
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

    // Then expand (should NOT call markViewed again)
    const expandBtn = screen.getByRole("button", { name: /expand/i });
    await userEvent.click(expandBtn);
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
    expect(screen.getByText("Package Not In Baseline")).toBeInTheDocument();
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

    expect(screen.getByText("No items in this section.")).toBeInTheDocument();
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

    expect(screen.getByRole("grid", { name: "Packages decisions" })).toBeInTheDocument();
  });
});

// ---- Error handling tests ----

describe("Error handling", () => {
  it("shows toast on mutation error and auto-dismisses", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });

    // Make applyOp fail
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
          json: () => Promise.resolve({ error: "generation mismatch" }),
        });
      }
      return Promise.resolve({ ok: false, status: 404, json: () => Promise.resolve({ error: "not found" }) });
    });

    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "httpd" }, [NEEDS_REVIEW_TAG]) },
    ];

    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        onViewUpdate={vi.fn()}
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

    // Wait for error toast to appear
    await waitFor(() => {
      expect(screen.getByText(/Error: generation mismatch/)).toBeInTheDocument();
    });

    // Auto-dismiss after 3 seconds
    vi.advanceTimersByTime(3100);
    await waitFor(() => {
      expect(screen.queryByText(/Error: generation mismatch/)).not.toBeInTheDocument();
    });

    vi.useRealTimers();
  });
});
