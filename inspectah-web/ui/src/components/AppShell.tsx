import { useState, useCallback, useEffect, useRef } from "react";
import { Page } from "@patternfly/react-core";
import { useKeyboard } from "../hooks/useKeyboard";
import { StatsBar } from "./StatsBar";
import { ContainerfilePanel } from "./ContainerfilePanel";
import { ShortcutOverlay } from "./ShortcutOverlay";
import { GlobalSearch } from "./GlobalSearch";
import type { GlobalSearchHandle } from "./GlobalSearch";
import { ExportDialog } from "./ExportDialog";
import type { RefineStats, ReferenceSection } from "../api/types";
import type { DecisionItemKind } from "./DecisionItem";
import type { UserDecision } from "../api/types";
import type { FleetSummary } from "./StatsBar";

const LS_PANEL_KEY = "inspectah-cf-panel-open";

function readPanelPref(): boolean {
  try {
    const v = localStorage.getItem(LS_PANEL_KEY);
    if (v === "false") return false;
    return true; // default open
  } catch {
    return true;
  }
}

/** Compute initial panel state synchronously to avoid flash on narrow viewports. */
function initialPanelOpen(): boolean {
  const savedOpen = readPanelPref();
  if (
    typeof window !== "undefined" &&
    window.matchMedia("(max-width: 1279px)").matches
  ) {
    return false;
  }
  return savedOpen;
}

export interface AppShellProps {
  /** Section navigation sidebar content. */
  sidebar: React.ReactNode;
  /** Main content area -- receives shell state including section search and the search slot for sidebar. */
  children: (shellState: {
    sectionSearchOpen: boolean;
    onSectionSearchClose: () => void;
    filterClearCounter: number;
    searchSlot: React.ReactNode;
  }) => React.ReactNode;
  /** Containerfile panel preview content. */
  containerfilePreview?: string | null;
  /** Whether containerfile data is still loading. */
  containerfileLoading?: boolean;
  /** Stats for the StatsBar. */
  stats: RefineStats | null;
  /** Generation counter for export dialog staleness detection. */
  generation: number;
  /** Whether the session contains sensitive data. */
  sessionIsSensitive: boolean;
  /** Undo callback. */
  onUndo: () => void;
  /** Redo callback. */
  onRedo: () => void;
  /** Called after a successful export to refresh view. */
  onExportComplete: (view: import("../api/types").ViewResponse) => void;
  /** Whether a mutation is pending (disables undo/redo buttons). */
  isPending?: boolean;
  /** Number of NeedsReview items viewed (for StatsBar triage progress). */
  viewedNeedsReviewCount?: number;
  /** Section list for keyboard 1-9 navigation (unused by shell -- delegated to useKeyboard). */
  activeSection: string;
  /** Callback when section changes via keyboard shortcut. */
  onNavigateSection: (sectionId: string) => void;
  /** GlobalSearch data props. */
  searchPackageItems: DecisionItemKind[];
  searchConfigItems: DecisionItemKind[];
  searchUserDecisions?: UserDecision[];
  searchContextSections: ReferenceSection[] | null;
  /** GlobalSearch result navigation. */
  onSearchNavigate: (sectionId: string, itemId: string) => void;
  /** Extra shortcuts appended to the overlay. */
  extraShortcuts?: Array<{ key: string; description: string }>;
  /** Fleet-specific toolbar additions. */
  toolbarExtra?: React.ReactNode;
  /** Hamburger button for mobile responsive layout. */
  hamburger?: React.ReactNode;
  /** Fleet-mode one-line summary for StatsBar. */
  fleetSummary?: FleetSummary;
  /** When true, Containerfile panel defaults to open regardless of viewport width. */
  isFleetMode?: boolean;
}

/**
 * Shared application shell providing:
 * - StatsBar (toolbar with undo/redo/export)
 * - GlobalSearch (Ctrl+K)
 * - Section search state (/ key)
 * - ShortcutOverlay (? key)
 * - ExportDialog (Ctrl+Shift+E)
 * - ContainerfilePanel (Ctrl+E)
 * - useKeyboard bindings
 *
 * Used by both single-host mode (App.tsx) and fleet mode (FleetApp).
 */
export function AppShell({
  sidebar,
  children,
  containerfilePreview,
  containerfileLoading = false,
  stats,
  generation,
  sessionIsSensitive,
  onUndo,
  onRedo,
  onExportComplete,
  isPending = false,
  viewedNeedsReviewCount,
  activeSection,
  onNavigateSection,
  searchPackageItems,
  searchConfigItems,
  searchUserDecisions,
  searchContextSections,
  onSearchNavigate,
  extraShortcuts,
  toolbarExtra,
  hamburger,
  fleetSummary,
  isFleetMode = false,
}: AppShellProps) {
  const [cfPanelOpen, setCfPanelOpen] = useState(() =>
    isFleetMode ? readPanelPref() : initialPanelOpen(),
  );
  const [shortcutsOpen, setShortcutsOpen] = useState(false);
  const [sectionSearchOpen, setSectionSearchOpen] = useState(false);
  const [exportDialogOpen, setExportDialogOpen] = useState(false);
  const [filterClearCounter, setFilterClearCounter] = useState(0);
  const globalSearchRef = useRef<GlobalSearchHandle>(null);

  // Reset section search when active section changes
  useEffect(() => {
    setSectionSearchOpen(false);
  }, [activeSection]);

  const togglePanel = useCallback(() => {
    setCfPanelOpen((prev) => {
      const next = !prev;
      try {
        localStorage.setItem(LS_PANEL_KEY, String(next));
      } catch {
        /* ignore */
      }
      return next;
    });
  }, []);

  const handleExport = useCallback(() => {
    setExportDialogOpen(true);
  }, []);

  const handleToggleShortcuts = useCallback(() => {
    setShortcutsOpen((prev) => !prev);
  }, []);

  const handleOpenGlobalSearch = useCallback(() => {
    globalSearchRef.current?.focus();
  }, []);

  const handleOpenSectionSearch = useCallback(() => {
    setSectionSearchOpen(true);
  }, []);

  const handleSectionSearchClose = useCallback(() => {
    setSectionSearchOpen(false);
  }, []);

  const handleSearchNavigate = useCallback(
    (sectionId: string, itemId: string) => {
      setFilterClearCounter((c) => c + 1);
      onSearchNavigate(sectionId, itemId);
    },
    [onSearchNavigate],
  );

  useKeyboard({
    onUndo,
    onRedo,
    onTogglePanel: togglePanel,
    onExport: handleExport,
    onSectionChange: onNavigateSection,
    onOpenSearch: handleOpenSectionSearch,
    onOpenGlobalSearch: handleOpenGlobalSearch,
    onOpenShortcuts: handleToggleShortcuts,
  });

  const searchSlot = (
    <GlobalSearch
      ref={globalSearchRef}
      packageItems={searchPackageItems}
      configItems={searchConfigItems}
      userDecisions={searchUserDecisions}
      contextSections={searchContextSections}
      onNavigate={handleSearchNavigate}
    />
  );

  return (
    <Page className="inspectah-page" data-testid="app-shell">
      <StatsBar
        stats={stats}
        viewedNeedsReviewCount={viewedNeedsReviewCount}
        onUndo={onUndo}
        onRedo={onRedo}
        onExport={handleExport}
        isPending={isPending}
        hamburger={hamburger}
        fleetSummary={fleetSummary}
      />
      {toolbarExtra && (
        <div className="inspectah-toolbar-extra" data-testid="toolbar-extra">
          {toolbarExtra}
        </div>
      )}
      <div className="inspectah-layout">
        {sidebar}
        {children({
          sectionSearchOpen,
          onSectionSearchClose: handleSectionSearchClose,
          filterClearCounter,
          searchSlot,
        })}
        <ContainerfilePanel
          content={containerfilePreview ?? null}
          isOpen={cfPanelOpen}
          onToggle={togglePanel}
          loading={containerfileLoading}
          sessionIsSensitive={sessionIsSensitive}
        />
      </div>
      <ShortcutOverlay
        isOpen={shortcutsOpen}
        onClose={() => setShortcutsOpen(false)}
        extraShortcuts={extraShortcuts}
      />
      <ExportDialog
        isOpen={exportDialogOpen}
        onClose={() => setExportDialogOpen(false)}
        stats={stats}
        generation={generation}
        sessionIsSensitive={sessionIsSensitive}
        onViewUpdate={onExportComplete}
      />
    </Page>
  );
}
