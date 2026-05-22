import { useState, useCallback, useEffect, useRef, useMemo } from "react";
import {
  Page,
  PageSection,
  EmptyState,
  EmptyStateBody,
  EmptyStateFooter,
  Button,
} from "@patternfly/react-core";
import type { ViewResponse } from "./api/types";
import { fetchViewed, fetchOps } from "./api/client";
import type { AnnotatedOp } from "./api/types";
import { useView } from "./hooks/useView";
import { useSections } from "./hooks/useSections";
import { useHealth } from "./hooks/useHealth";
import { useMutation } from "./hooks/useMutation";
import { Sidebar } from "./components/Sidebar";
import { MainContent } from "./components/MainContent";
import { AppShell } from "./components/AppShell";
import { FleetApp } from "./components/FleetApp";
import "highlight.js/styles/github.css";
import "./App.css";

function App() {
  const [activeSection, setActiveSection] = useState("packages");
  const [sidebarOverlayOpen, setSidebarOverlayOpen] = useState(false);
  const [isMobile, setIsMobile] = useState(false);
  const [revealItemId, setRevealItemId] = useState<string | undefined>();
  const [searchNavCounter, setSearchNavCounter] = useState(0);
  const mainContentRef = useRef<HTMLDivElement>(null);
  const hamburgerRef = useRef<HTMLButtonElement>(null);
  const pendingFocusItemRef = useRef<string | null>(null);

  // Focus first item in section after section change.
  // Cascade: decision row > context item > section heading > main wrapper.
  useEffect(() => {
    requestAnimationFrame(() => {
      // If there's a pending item from search navigation, handle it separately
      if (pendingFocusItemRef.current) return;
      const container = mainContentRef.current;
      if (!container) return;

      // 1. Decision sections use role="row"
      const firstRow = container.querySelector(
        '[role="row"]',
      ) as HTMLElement | null;
      if (firstRow) { firstRow.focus(); return; }

      // 2. Context sections use ContextItem with data-testid
      const firstContextItem = container.querySelector(
        '[data-testid^="context-item-"]',
      ) as HTMLElement | null;
      if (firstContextItem) { firstContextItem.focus(); return; }

      // 3. Empty sections: focus the heading
      const heading = container.querySelector("h2, h3") as HTMLElement | null;
      if (heading) {
        heading.setAttribute("tabindex", "-1");
        heading.focus();
        return;
      }

      // 4. Ultimate fallback
      container.focus();
    });
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

  // Ref to hold the testid of the focused element before undo/redo
  const undoFocusRef = useRef<string | null>(null);

  const onMutationSuccess = useCallback(
    (_result: ViewResponse) => {
      // After successful mutation, refetch view data and viewed IDs
      view.invalidate();
      refreshViewed();

      // Restore focus after undo/redo
      const testId = undoFocusRef.current;
      if (testId) {
        undoFocusRef.current = null;
        requestAnimationFrame(() => {
          const el = document.querySelector(`[data-testid="${testId}"]`);
          if (el instanceof HTMLElement) {
            el.focus();
          } else {
            // Target not in DOM — navigate to its section and set pending focus
            const section = testId.includes("packages:") ? "packages" : "configs";
            const itemId = testId.replace("decision-item-", "");
            setActiveSection(section);
            pendingFocusItemRef.current = itemId;
          }
        });
      }
    },
    [view.invalidate, refreshViewed],
  );

  const onMutationError = useCallback(
    (err: Error) => {
      // Refetch to restore correct state after error
      view.invalidate();
      console.error("Mutation failed:", err.message);
      undoFocusRef.current = null;
    },
    [view.invalidate],
  );

  const mutation = useMutation(onMutationSuccess, onMutationError);

  /** Extract the decision-item testid from an annotated op. */
  function getItemTestIdFromOp(op: AnnotatedOp): string | null {
    if (op.op === "ExcludePackage" || op.op === "IncludePackage") {
      const t = op.target as { name: string; arch: string };
      return `decision-item-packages:${t.name}.${t.arch}`;
    }
    if (op.op === "ExcludeConfig" || op.op === "IncludeConfig") {
      const t = op.target as { path: string };
      return `decision-item-configs:${t.path}`;
    }
    return null;
  }

  /** Fetch ops to find the undo target, then fire undo. */
  const handleUndo = useCallback(() => {
    fetchOps()
      .then((ops) => {
        // The last active op is the one being undone
        const lastActive = [...ops].reverse().find((o) => o.active);
        undoFocusRef.current = lastActive ? getItemTestIdFromOp(lastActive) : null;
        mutation.undo();
      })
      .catch(() => {
        undoFocusRef.current = null;
        mutation.undo();
      });
  }, [mutation]);

  /** Fetch ops to find the redo target, then fire redo. */
  const handleRedo = useCallback(() => {
    fetchOps()
      .then((ops) => {
        // The first inactive op is the one being re-applied
        const firstInactive = ops.find((o) => !o.active);
        undoFocusRef.current = firstInactive ? getItemTestIdFromOp(firstInactive) : null;
        mutation.redo();
      })
      .catch(() => {
        undoFocusRef.current = null;
        mutation.redo();
      });
  }, [mutation]);

  const closeSidebarOverlay = useCallback(() => {
    setSidebarOverlayOpen(false);
    requestAnimationFrame(() => {
      hamburgerRef.current?.focus();
    });
  }, []);

  const handleNavigateFromSearch = useCallback(
    (sectionId: string, itemId: string) => {
      pendingFocusItemRef.current = itemId;
      setRevealItemId(itemId);
      setActiveSection(sectionId);
      setSearchNavCounter((c) => c + 1);
      // Close mobile overlay so the target item is visible
      if (isMobile && sidebarOverlayOpen) closeSidebarOverlay();
    },
    [isMobile, sidebarOverlayOpen, closeSidebarOverlay],
  );

  // Handle pending focus item after render (from search or undo/redo navigation).
  // The ref is NOT cleared until the element is found — this lets the effect retry
  // across re-renders (e.g., waiting for filter clear or view data refresh).
  useEffect(() => {
    const itemId = pendingFocusItemRef.current;
    if (!itemId) return;

    requestAnimationFrame(() => {
      const el = (
        document.querySelector(`[data-testid="decision-item-${itemId}"]`) ??
        document.querySelector(`[data-testid="context-item-${itemId}"]`)
      ) as HTMLElement | null;
      if (!el) return;

      pendingFocusItemRef.current = null;
      setRevealItemId(undefined);

      const hiddenAncestor = el.closest("[hidden]");
      if (hiddenAncestor) {
        const group = hiddenAncestor.closest("[data-testid^='attention-group-']");
        const toggle = group?.querySelector("button") as HTMLElement | null;
        toggle?.click();
        requestAnimationFrame(() => {
          el.scrollIntoView({ behavior: "smooth", block: "nearest" });
          el.classList.add("inspectah-highlight");
          el.focus();
          setTimeout(() => el.classList.remove("inspectah-highlight"), 1500);
        });
      } else {
        el.scrollIntoView({ behavior: "smooth", block: "nearest" });
        el.classList.add("inspectah-highlight");
        el.focus();
        setTimeout(() => el.classList.remove("inspectah-highlight"), 1500);
      }
    });
  }, [activeSection, view.data, searchNavCounter]);

  const handleSidebarSelect = useCallback(
    (sectionId: string) => {
      setActiveSection(sectionId);
      if (isMobile) closeSidebarOverlay();
    },
    [isMobile, closeSidebarOverlay],
  );

  const handleExportViewUpdate = useCallback(
    (_updatedView: ViewResponse) => {
      view.invalidate();
    },
    [view.invalidate],
  );

  const viewLoading = view.loading && view.data === null;

  // Fleet mode: fork early, before single-host error guards.
  // useView/useSections hooks still run (rules of hooks) but their results
  // are ignored — FleetApp fetches its own data from /api/fleet/view.
  if (health.data?.fleet) {
    return <FleetApp fleet={health.data.fleet} health={health.data} />;
  }

  // Initial load error: show full-page error with retry when first fetch fails
  // (single-host mode only — fleet mode already returned above)
  const initialLoadError =
    !view.loading && view.error && view.data === null
      ? view.error
      : !health.loading && health.error && health.data === null
        ? health.error
        : !sections.loading && sections.error && sections.data === null
          ? sections.error
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
                  health.refetch();
                  sections.refetch();
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

  // Single-host mode: use AppShell
  const packageItems = view.data
    ? view.data.packages.map((p) => ({ type: "package" as const, data: p }))
    : [];
  const configItems = view.data
    ? view.data.config_files.map((c) => ({ type: "config" as const, data: c }))
    : [];

  return (
    <>
      <AppShell
        sidebar={null}
        containerfilePreview={view.data?.containerfile_preview}
        containerfileLoading={viewLoading}
        stats={view.data?.stats ?? null}
        generation={view.data?.generation ?? 0}
        sessionIsSensitive={view.data?.session_is_sensitive ?? false}
        onUndo={handleUndo}
        onRedo={handleRedo}
        onExportComplete={handleExportViewUpdate}
        isPending={mutation.isPending}
        viewedNeedsReviewCount={viewedNeedsReviewCount}
        activeSection={activeSection}
        onNavigateSection={setActiveSection}
        searchPackageItems={packageItems}
        searchConfigItems={configItems}
        searchUserDecisions={view.data?.users_groups_decisions}
        searchContextSections={sections.data}
        onSearchNavigate={handleNavigateFromSearch}
        hamburger={
          isMobile ? (
            <button
              ref={hamburgerRef}
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
      >
        {({ sectionSearchOpen, onSectionSearchClose, filterClearCounter, searchSlot }) => (
          <>
            {!isMobile && (
              <div className="inspectah-layout__sidebar">
                <Sidebar
                  activeSection={activeSection}
                  onSelect={handleSidebarSelect}
                  stats={view.data?.stats ?? null}
                  sections={sections.data}
                  health={health.data}
                  userDecisionCount={view.data?.users_groups_decisions?.length}
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
                onSectionSearchClose={onSectionSearchClose}
                onViewedChange={refreshViewed}
                filterClearCounter={filterClearCounter}
                revealItemId={revealItemId}
              />
            </div>
            {isMobile && sidebarOverlayOpen && (
              <Sidebar
                activeSection={activeSection}
                onSelect={handleSidebarSelect}
                stats={view.data?.stats ?? null}
                sections={sections.data}
                health={health.data}
                userDecisionCount={view.data?.users_groups_decisions?.length}
                overlay
                onClose={closeSidebarOverlay}
                searchSlot={searchSlot}
              />
            )}
          </>
        )}
      </AppShell>
    </>
  );
}

export default App;
