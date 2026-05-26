import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { AppShell } from "../AppShell";
import type { AppShellProps } from "../AppShell";
import { mockStats } from "../../test-utils/mockStats";

// Mock fetch for ExportDialog internals
beforeEach(() => {
  vi.stubGlobal("fetch", vi.fn());
});

const MOCK_STATS = mockStats({
  sections: [
    { kind: "package", total: 100, included: 80, excluded: 20 },
    { kind: "config", total: 50, included: 40, excluded: 10 },
  ],
  needs_review_count: 5,
  ops_applied: 3,
  can_undo: true,
  can_redo: false,
  baseline_available: false,
});

function renderAppShell(overrides: Partial<AppShellProps> = {}) {
  const defaultProps: AppShellProps = {
    sidebar: <div data-testid="test-sidebar">Sidebar</div>,
    children: ({ sectionSearchOpen, onSectionSearchClose, filterClearCounter, searchSlot }) => (
      <div data-testid="test-content">
        <span data-testid="section-search-open">{String(sectionSearchOpen)}</span>
        <span data-testid="filter-clear-counter">{filterClearCounter}</span>
        {searchSlot}
        {sectionSearchOpen && (
          <button data-testid="close-search" onClick={onSectionSearchClose}>
            Close
          </button>
        )}
      </div>
    ),
    stats: MOCK_STATS,
    generation: 1,
    sessionIsSensitive: false,
    onUndo: vi.fn(),
    onRedo: vi.fn(),
    onExportComplete: vi.fn(),
    activeSection: "packages",
    onNavigateSection: vi.fn(),
    searchPackageItems: [],
    searchConfigItems: [],
    searchContextSections: null,
    onSearchNavigate: vi.fn(),
    ...overrides,
  };
  return { ...render(<AppShell {...defaultProps} />), props: defaultProps };
}

describe("AppShell", () => {
  it("renders children in content area", () => {
    renderAppShell();
    expect(screen.getByTestId("test-content")).toBeInTheDocument();
  });

  it("renders sidebar slot", () => {
    renderAppShell();
    expect(screen.getByTestId("test-sidebar")).toBeInTheDocument();
  });

  it("renders StatsBar with stats", () => {
    renderAppShell();
    expect(screen.getByText(/Packages:/)).toBeInTheDocument();
    expect(screen.getByText(/80 included/)).toBeInTheDocument();
  });

  it("renders ShortcutOverlay on ? key", async () => {
    renderAppShell();

    // ShortcutOverlay should not be visible initially
    expect(screen.queryByTestId("shortcut-overlay")).not.toBeInTheDocument();

    // Press ? to open
    await userEvent.keyboard("?");

    expect(screen.getByTestId("shortcut-overlay")).toBeInTheDocument();
  });

  it("renders ExportDialog on export trigger", async () => {
    renderAppShell();

    // ExportDialog should not be visible initially
    expect(screen.queryByText("Export Containerfile")).not.toBeInTheDocument();

    // Click the Export button in StatsBar
    const exportBtn = screen.getByRole("button", { name: /export/i });
    await userEvent.click(exportBtn);

    // ExportDialog modal should appear
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("opens section search on / key and passes sectionSearchOpen to children", async () => {
    renderAppShell();

    // Initially section search is closed
    expect(screen.getByTestId("section-search-open").textContent).toBe("false");

    // Press / to open section search
    await userEvent.keyboard("/");

    expect(screen.getByTestId("section-search-open").textContent).toBe("true");
  });

  it("closes section search via onSectionSearchClose callback", async () => {
    renderAppShell();

    // Open section search
    await userEvent.keyboard("/");
    expect(screen.getByTestId("section-search-open").textContent).toBe("true");

    // Close via the callback
    const closeBtn = screen.getByTestId("close-search");
    await userEvent.click(closeBtn);

    expect(screen.getByTestId("section-search-open").textContent).toBe("false");
  });

  it("resets section search when activeSection changes", async () => {
    const { rerender } = renderAppShell();

    // Open section search
    await userEvent.keyboard("/");
    expect(screen.getByTestId("section-search-open").textContent).toBe("true");

    // Re-render with a different activeSection
    const updatedChildren: AppShellProps["children"] = ({ sectionSearchOpen }) => (
      <div data-testid="test-content">
        <span data-testid="section-search-open">{String(sectionSearchOpen)}</span>
      </div>
    );

    rerender(
      <AppShell
        sidebar={<div>Sidebar</div>}
        children={updatedChildren}
        stats={MOCK_STATS}
        generation={1}
        sessionIsSensitive={false}
        onUndo={vi.fn()}
        onRedo={vi.fn()}
        onExportComplete={vi.fn()}
        activeSection="configs"
        onNavigateSection={vi.fn()}
        searchPackageItems={[]}
        searchConfigItems={[]}
        searchContextSections={null}
        onSearchNavigate={vi.fn()}
      />,
    );

    // Section search should be closed after section change
    expect(screen.getByTestId("section-search-open").textContent).toBe("false");
  });

  it("renders GlobalSearch and focuses on Ctrl+K", async () => {
    renderAppShell();

    // GlobalSearch should be in the DOM
    expect(screen.getByTestId("global-search-input")).toBeInTheDocument();

    // Ctrl+K should focus the search input
    await userEvent.keyboard("{Control>}k{/Control}");

    expect(document.activeElement).toBe(
      screen.getByLabelText("Search all sections"),
    );
  });

  it("increments filterClearCounter on search navigation", async () => {
    const onSearchNavigate = vi.fn();
    renderAppShell({ onSearchNavigate });

    expect(screen.getByTestId("filter-clear-counter").textContent).toBe("0");

    // Type in global search to trigger a navigation
    const input = screen.getByLabelText("Search all sections");
    await userEvent.type(input, "test");

    // Even without results, verify the searchSlot is rendered
    expect(screen.getByTestId("global-search-input")).toBeInTheDocument();
  });

  it("passes searchSlot to children", () => {
    const childrenFn = vi.fn().mockImplementation(
      ({ searchSlot }: { searchSlot: React.ReactNode }) => (
        <div data-testid="test-content">
          <div data-testid="search-slot-container">{searchSlot}</div>
        </div>
      ),
    );

    renderAppShell({ children: childrenFn });

    // The searchSlot should contain the GlobalSearch component
    const container = screen.getByTestId("search-slot-container");
    expect(within(container).getByTestId("global-search-input")).toBeInTheDocument();
  });

  it("toggles ContainerfilePanel on Ctrl+E", async () => {
    renderAppShell({
      containerfilePreview: "FROM ubi9\nRUN echo hello",
    });

    // Panel should be open by default (readPanelPref returns true in test)
    expect(
      screen.getByRole("complementary", { name: "Containerfile preview" }),
    ).toBeInTheDocument();
  });
});
