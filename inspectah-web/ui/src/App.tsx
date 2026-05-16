import { useState, useCallback, useEffect, useRef, useMemo } from "react";
import {
  Page,
  PageSection,
  EmptyState,
  EmptyStateBody,
  EmptyStateFooter,
  Button,
} from "@patternfly/react-core";
import type { RefinedView } from "./api/types";
import { fetchViewed } from "./api/client";
import { useView } from "./hooks/useView";
import { useSections } from "./hooks/useSections";
import { useHealth } from "./hooks/useHealth";
import { useMutation } from "./hooks/useMutation";
import { useKeyboard } from "./hooks/useKeyboard";
import { Sidebar } from "./components/Sidebar";
import { StatsBar } from "./components/StatsBar";
import { ContainerfilePanel } from "./components/ContainerfilePanel";
import { MainContent } from "./components/MainContent";
import { ShortcutOverlay } from "./components/ShortcutOverlay";
import { GlobalSearch } from "./components/GlobalSearch";
import type { GlobalSearchHandle } from "./components/GlobalSearch";
import { ExportDialog } from "./components/ExportDialog";
import "highlight.js/styles/github.css";
import "./App.css";

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

function App() {
  const [activeSection, setActiveSection] = useState("packages");
  const [cfPanelOpen, setCfPanelOpen] = useState(readPanelPref);
  const [shortcutsOpen, setShortcutsOpen] = useState(false);
  const [sectionSearchOpen, setSectionSearchOpen] = useState(false);
  const [exportDialogOpen, setExportDialogOpen] = useState(false);
  const [sidebarOverlayOpen, setSidebarOverlayOpen] = useState(false);
  const [isMobile, setIsMobile] = useState(false);
  const mainContentRef = useRef<HTMLDivElement>(null);
  const globalSearchRef = useRef<GlobalSearchHandle>(null);

  // Focus main content area when active section changes
  useEffect(() => {
    mainContentRef.current?.focus();
  }, [activeSection]);

  // Responsive breakpoint: < 1024px hides sidebar, shows hamburger
  useEffect(() => {
    const mql = window.matchMedia("(max-width: 1023px)");
    const handler = (e: MediaQueryListEvent | MediaQueryList) => {
      setIsMobile(e.matches);
      if (!e.matches) setSidebarOverlayOpen(false);
    };
    handler(mql);
    mql.addEventListener("change", handler);
    return () => mql.removeEventListener("change", handler);
  }, []);

  const view = useView();
  const sections = useSections();
  const health = useHealth();

  // Track viewed item IDs for triage progress
  const [viewedIds, setViewedIds] = useState<Set<string>>(new Set());

  const refreshViewed = useCallback(() => {
    fetchViewed()
      .then((resp) => setViewedIds(new Set(resp.ids)))
      .catch(() => {/* ignore – non-critical */});
  }, []);

  // Fetch viewed IDs on mount
  useEffect(() => {
    refreshViewed();
  }, [refreshViewed]);

  // Compute how many NeedsReview items have been viewed
  const viewedNeedsReviewCount = useMemo(() => {
    if (!view.data) return 0;
    let count = 0;
    for (const pkg of view.data.packages) {
      if (pkg.attention.some((a) => a.level === "needs_review")) {
        const id = `packages:${pkg.entry.name}.${pkg.entry.arch}`;
        if (viewedIds.has(id)) count++;
      }
    }
    for (const cfg of view.data.config_files) {
      if (cfg.attention.some((a) => a.level === "needs_review")) {
        const id = `configs:${cfg.entry.path}`;
        if (viewedIds.has(id)) count++;
      }
    }
    return count;
  }, [view.data, viewedIds]);

  const onMutationSuccess = useCallback(
    (_result: RefinedView) => {
      // After successful mutation, refetch view data and viewed IDs
      view.invalidate();
      refreshViewed();
    },
    [view.invalidate, refreshViewed],
  );

  const onMutationError = useCallback(
    (err: Error) => {
      // Refetch to restore correct state after error
      view.invalidate();
      console.error("Mutation failed:", err.message);
    },
    [view.invalidate],
  );

  const mutation = useMutation(onMutationSuccess, onMutationError);

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

  const handleNavigateFromGlobalSearch = useCallback(
    (sectionId: string, _itemId: string) => {
      setActiveSection(sectionId);
    },
    [],
  );

  const handleSidebarSelect = useCallback(
    (sectionId: string) => {
      setActiveSection(sectionId);
      if (isMobile) setSidebarOverlayOpen(false);
    },
    [isMobile],
  );

  const handleExportViewUpdate = useCallback(
    (_updatedView: RefinedView) => {
      view.invalidate();
    },
    [view.invalidate],
  );

  useKeyboard({
    onUndo: mutation.undo,
    onRedo: mutation.redo,
    onTogglePanel: togglePanel,
    onExport: handleExport,
    onSectionChange: setActiveSection,
    onOpenSearch: handleOpenSectionSearch,
    onOpenGlobalSearch: handleOpenGlobalSearch,
    onOpenShortcuts: handleToggleShortcuts,
  });

  const viewLoading = view.loading && view.data === null;

  // Initial load error: show full-page error with retry when first fetch fails
  const initialLoadError =
    !view.loading && view.error && view.data === null
      ? view.error
      : !health.loading && health.error && health.data === null
        ? health.error
        : null;

  if (initialLoadError) {
    return (
      <Page className="inspectah-page">
        <PageSection>
          <EmptyState
            titleText="Failed to load"
            headingLevel="h2"
            data-testid="initial-load-error"
          >
            <EmptyStateBody>
              {initialLoadError.message || "Could not connect to the inspectah server."}
            </EmptyStateBody>
            <EmptyStateFooter>
              <Button
                variant="primary"
                onClick={() => {
                  view.refetch();
                }}
              >
                Retry
              </Button>
            </EmptyStateFooter>
          </EmptyState>
        </PageSection>
      </Page>
    );
  }

  const searchSlot = (
    <GlobalSearch
      ref={globalSearchRef}
      packageItems={view.data ? view.data.packages.map((p) => ({ type: "package" as const, data: p })) : []}
      configItems={view.data ? view.data.config_files.map((c) => ({ type: "config" as const, data: c })) : []}
      contextSections={sections.data}
      onNavigate={handleNavigateFromGlobalSearch}
    />
  );

  return (
    <Page className="inspectah-page">
      <StatsBar
        stats={view.data?.stats ?? null}
        viewedNeedsReviewCount={viewedNeedsReviewCount}
        onUndo={mutation.undo}
        onRedo={mutation.redo}
        onExport={handleExport}
        isPending={mutation.isPending}
        hamburger={
          isMobile ? (
            <button
              type="button"
              className="inspectah-hamburger"
              aria-label={sidebarOverlayOpen ? "Close navigation" : "Open navigation"}
              aria-expanded={sidebarOverlayOpen}
              aria-controls="inspectah-sidebar-overlay"
              onClick={() => setSidebarOverlayOpen((prev) => !prev)}
            >
              &#x2630;
            </button>
          ) : undefined
        }
      />
      <div className="inspectah-layout">
        {!isMobile && (
          <div className="inspectah-layout__sidebar">
            <Sidebar
              activeSection={activeSection}
              onSelect={handleSidebarSelect}
              stats={view.data?.stats ?? null}
              sections={sections.data}
              health={health.data}
              searchSlot={searchSlot}
            />
          </div>
        )}
        <div className="inspectah-layout__main" ref={mainContentRef} tabIndex={-1}>
          <MainContent
            activeSection={activeSection}
            loading={viewLoading}
            viewData={view.data}
            sections={sections.data}
            onViewUpdate={() => view.invalidate()}
            onMutationError={(err) => console.error("Mutation failed:", err.message)}
            sectionSearchOpen={sectionSearchOpen}
            onSectionSearchClose={() => setSectionSearchOpen(false)}
          />
        </div>
        <ContainerfilePanel
          content={view.data?.containerfile_preview ?? null}
          isOpen={cfPanelOpen}
          onToggle={togglePanel}
          loading={viewLoading}
        />
      </div>
      {isMobile && sidebarOverlayOpen && (
        <Sidebar
          activeSection={activeSection}
          onSelect={handleSidebarSelect}
          stats={view.data?.stats ?? null}
          sections={sections.data}
          health={health.data}
          overlay
          onClose={() => setSidebarOverlayOpen(false)}
          searchSlot={searchSlot}
        />
      )}
      <ShortcutOverlay
        isOpen={shortcutsOpen}
        onClose={() => setShortcutsOpen(false)}
      />
      <ExportDialog
        isOpen={exportDialogOpen}
        onClose={() => setExportDialogOpen(false)}
        stats={view.data?.stats ?? null}
        generation={view.data?.generation ?? 0}
        onViewUpdate={handleExportViewUpdate}
      />
    </Page>
  );
}

export default App;
