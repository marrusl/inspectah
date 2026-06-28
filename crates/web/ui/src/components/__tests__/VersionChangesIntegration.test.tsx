import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import { MainContent } from "../MainContent";
import type {
  ViewResponse,
  ReferenceSection,
  VersionChangeEntry,
} from "../../api/types";

// Mock the API client (MainContent imports applyOp / ungroupGroup)
vi.mock("../../api/client", () => ({
  ungroupGroup: vi.fn(),
  applyOp: vi.fn(),
}));

const downgrade: VersionChangeEntry = {
  name: "httpd",
  arch: "x86_64",
  host_version: "2.4.57",
  base_version: "2.4.51",
  host_epoch: "",
  base_epoch: "",
  direction: "downgrade",
};
const upgrade: VersionChangeEntry = {
  name: "podman",
  arch: "x86_64",
  host_version: "4.6.1",
  base_version: "4.9.0",
  host_epoch: "",
  base_epoch: "",
  direction: "upgrade",
};

/** Minimal ViewResponse that renders for version_changes. */
function makeViewData(
  versionChanges: VersionChangeEntry[],
): Partial<ViewResponse> {
  return {
    packages: [],
    config_files: [],
    repo_groups: [],
    package_groups: [],
    version_changes: versionChanges,
    generation: 1,
    stats: {
      sections: [],
      needs_review_count: 0,
      ops_applied: 0,
      can_undo: false,
      can_redo: false,
      baseline_available: false,
    },
  };
}

/** Minimal sections array with a version_changes entry. */
function makeSections(
  itemCount: number,
  emptyReason?: string,
): ReferenceSection[] {
  return [
    {
      id: "version_changes",
      display_name: "Version Changes",
      items: Array.from({ length: itemCount }, (_, i) => ({
        id: `item-${i}`,
        title: `item ${i}`,
        subtitle: null,
        detail: `val ${i}`,
        searchable_text: `item ${i} val ${i}`,
      })),
      empty_reason: emptyReason,
    },
  ];
}

describe("VersionChanges integration in MainContent", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders VersionChangesTable instead of ContextList for version_changes", () => {
    const viewData = makeViewData([downgrade, upgrade]);
    const sections = makeSections(2);

    render(
      <MainContent
        activeSection="version_changes"
        loading={false}
        viewData={viewData as ViewResponse}
        sections={sections}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
        sectionSearchOpen={false}
        onSectionSearchClose={vi.fn()}
      />,
    );

    // VersionChangesTable renders group headers and data rows
    expect(screen.getByText(/Downgrades \(1\)/)).toBeInTheDocument();
    expect(screen.getByText(/Upgrades \(1\)/)).toBeInTheDocument();

    // Data rows have context-item testids
    expect(screen.getByTestId("context-item-httpd.x86_64")).toBeInTheDocument();
    expect(
      screen.getByTestId("context-item-podman.x86_64"),
    ).toBeInTheDocument();
  });

  it("plain section entry focuses first data row, not group header", () => {
    const viewData = makeViewData([downgrade, upgrade]);
    const sections = makeSections(2);

    render(
      <MainContent
        activeSection="version_changes"
        loading={false}
        viewData={viewData as ViewResponse}
        sections={sections}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
        sectionSearchOpen={false}
        onSectionSearchClose={vi.fn()}
      />,
    );

    // The data rows (context-item-*) should be focusable (tabIndex=-1),
    // and group header rows should NOT have context-item testids.
    const dataRows = screen.getAllByTestId(/^context-item-/);
    expect(dataRows.length).toBeGreaterThanOrEqual(1);

    // Group header rows use role="row" but do NOT have context-item testids
    // This validates the App.tsx focus query will find the right element
    const groupHeaders = document.querySelectorAll(
      ".inspectah-vc-group-header",
    );
    for (const header of groupHeaders) {
      expect(header.getAttribute("data-testid")).toBeNull();
    }
  });

  it("reveal navigation highlights the targeted data row", () => {
    const viewData = makeViewData([downgrade, upgrade]);
    const sections = makeSections(2);

    render(
      <MainContent
        activeSection="version_changes"
        loading={false}
        viewData={viewData as ViewResponse}
        sections={sections}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
        sectionSearchOpen={false}
        onSectionSearchClose={vi.fn()}
        revealItemId="podman.x86_64"
      />,
    );

    const revealedRow = screen.getByTestId("context-item-podman.x86_64");
    expect(revealedRow).toHaveClass("inspectah-vc-row--revealed");

    // The non-targeted row should NOT have the revealed class
    const otherRow = screen.getByTestId("context-item-httpd.x86_64");
    expect(otherRow).not.toHaveClass("inspectah-vc-row--revealed");
  });

  it("delegates empty state to VersionChangesTable", () => {
    const viewData = makeViewData([]);
    const sections = makeSections(0, "zero_drift");

    render(
      <MainContent
        activeSection="version_changes"
        loading={false}
        viewData={viewData as ViewResponse}
        sections={sections}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
        sectionSearchOpen={false}
        onSectionSearchClose={vi.fn()}
      />,
    );

    // VersionChangesTable handles empty state internally
    expect(screen.getByText(/match the target baseline/i)).toBeInTheDocument();
  });
});
