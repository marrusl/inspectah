import { useState, useCallback, useEffect, useRef } from "react";
import {
  Page,
  PageSection,
  EmptyState,
  EmptyStateBody,
  EmptyStateFooter,
  Button,
} from "@patternfly/react-core";
import type { ViewResponse } from "./api/types";
import { fetchOps } from "./api/client";
import type { AnnotatedTimelineEntry } from "./api/types";
import { useView } from "./hooks/useView";
import { useSections } from "./hooks/useSections";
import { useHealth } from "./hooks/useHealth";
import { useMutation } from "./hooks/useMutation";
import { Sidebar } from "./components/Sidebar";
import { MainContent } from "./components/MainContent";
import { AppShell } from "./components/AppShell";
import { AggregateApp } from "./components/AggregateApp";
import "highlight.js/styles/github.css";
import "./App.css";

/**
 * Top-level router. Uses only useHealth to decide between aggregate and
 * single-host mode. Aggregate sessions fork here and never instantiate
 * the single-host hooks (useView, useSections).
 *
 * Gate: returns null until health resolves at least once, so aggregate
 * sessions never transiently mount SingleHostApp during the loading
 * window. Tests mock fetch synchronously, so null is never visible.
 */
function App() {
  const health = useHealth();

  // Wait for health to resolve before choosing a path. Without this,
  // aggregate sessions would transiently mount SingleHostApp (and its
  // useView/useSections hooks) on first paint while health.data is null.
  if (!health.data && !health.error) {
    return null;
  }

  if (health.data?.aggregate) {
    return <AggregateApp aggregate={health.data.aggregate} health={health.data} />;
  }

  return <SingleHostApp healthFromRouter={health} />;
}

/**
 * Single-host refine UI. All single-host hooks (useView, useSections,
 * useMutation) live here and are never instantiated in aggregate mode.
 */
function SingleHostApp({
  healthFromRouter,
}: {
  healthFromRouter: import("./hooks/useHealth").UseHealthResult;
}) {
  const [activeSection, setActiveSection] = useState("packages");
  const [sidebarOverlayOpen, setSidebarOverlayOpen] = useState(false);
  const [isMobile, setIsMobile] = useState(false);
  const [revealItemId, setRevealItemId] = useState<string | undefined>();
  const [searchNavCounter, setSearchNavCounter] = useState(0);
  const mainContentRef = useRef<HTMLDivElement>(null);
  const hamburgerRef = useRef<HTMLButtonElement>(null);
  const pendingFocusItemRef = useRef<string | null>(null);

  useEffect(() => {
    requestAnimationFrame(() => {
      if (pendingFocusItemRef.current) return;
      const container = mainContentRef.current;
      if (!container) return;

      // For version_changes, prefer context-item data rows over group header
      // rows (which also have role="row") per the approved spec.
      const preferContextItem = activeSection === "version_changes";

      if (preferContextItem) {
        const firstContextItem = container.querySelector(
          '[data-testid^="context-item-"]',
        ) as HTMLElement | null;
        if (firstContextItem) {
          firstContextItem.focus();
          return;
        }
      }

      const firstRow = container.querySelector(
        '[role="row"]',
      ) as HTMLElement | null;
      if (firstRow) {
        firstRow.focus();
        return;
      }

      if (!preferContextItem) {
        const firstContextItem = container.querySelector(
          '[data-testid^="context-item-"]',
        ) as HTMLElement | null;
        if (firstContextItem) {
          firstContextItem.focus();
          return;
        }
      }

      const heading = container.querySelector("h2, h3") as HTMLElement | null;
      if (heading) {
        heading.setAttribute("tabindex", "-1");
        heading.focus();
        return;
      }

      container.focus();
    });
  }, [activeSection]);

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
  const health = healthFromRouter;

  const undoFocusRef = useRef<string | null>(null);

  const onMutationSuccess = useCallback(
    (_result: ViewResponse) => {
      view.invalidate();

      const testId = undoFocusRef.current;
      if (testId) {
        undoFocusRef.current = null;
        requestAnimationFrame(() => {
          const el = document.querySelector(`[data-testid="${testId}"]`);
          if (el instanceof HTMLElement) {
            el.focus();
          } else {
            const section = testId.includes("packages:")
              ? "packages"
              : "configs";
            const itemId = testId.replace("decision-item-", "");
            setActiveSection(section);
            pendingFocusItemRef.current = itemId;
          }
        });
      }
    },
    [view.invalidate],
  );

  const onMutationError = useCallback(
    (err: Error) => {
      view.invalidate();
      console.error("Mutation failed:", err.message);
      undoFocusRef.current = null;
    },
    [view.invalidate],
  );

  const mutation = useMutation(onMutationSuccess, onMutationError);

  function getItemTestIdFromEntry(entry: AnnotatedTimelineEntry): string | null {
    if (entry.kind === "View") {
      if (entry.directive === "UngroupGroup") {
        return `group-row-${entry.group_name}`;
      }
      return null;
    }
    // kind === "Op"
    if (entry.op === "SetInclude") {
      const t = entry.target as {
        item_id: { kind: string; key: Record<string, string> };
        include: boolean;
      };
      if (t.item_id.kind === "Package") {
        return `decision-item-packages:${t.item_id.key.name}.${t.item_id.key.arch}`;
      }
      if (t.item_id.kind === "Config") {
        return `decision-item-configs:${t.item_id.key.path}`;
      }
    }
    // Legacy op format fallback
    if (entry.op === "ExcludePackage" || entry.op === "IncludePackage") {
      const t = entry.target as { name: string; arch: string };
      return `decision-item-packages:${t.name}.${t.arch}`;
    }
    if (entry.op === "ExcludeConfig" || entry.op === "IncludeConfig") {
      const t = entry.target as { path: string };
      return `decision-item-configs:${t.path}`;
    }
    return null;
  }

  const handleSetUndoFocusTarget = useCallback((testId: string | null) => {
    undoFocusRef.current = testId;
  }, []);

  const handleUndo = useCallback(() => {
    fetchOps()
      .then((ops) => {
        const lastActive = [...ops].reverse().find((o) => o.active);
        undoFocusRef.current = lastActive
          ? getItemTestIdFromEntry(lastActive)
          : null;
        mutation.undo();
      })
      .catch(() => {
        undoFocusRef.current = null;
        mutation.undo();
      });
  }, [mutation]);

  const handleRedo = useCallback(() => {
    fetchOps()
      .then((ops) => {
        const firstInactive = ops.find((o) => !o.active);
        undoFocusRef.current = firstInactive
          ? getItemTestIdFromEntry(firstInactive)
          : null;
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
      if (isMobile && sidebarOverlayOpen) closeSidebarOverlay();
    },
    [isMobile, sidebarOverlayOpen, closeSidebarOverlay],
  );

  useEffect(() => {
    const itemId = pendingFocusItemRef.current;
    if (!itemId) return;

    requestAnimationFrame(() => {
      const el = (document.querySelector(
        `[data-testid="decision-item-${itemId}"]`,
      ) ??
        document.querySelector(
          `[data-testid="context-item-${itemId}"]`,
        )) as HTMLElement | null;
      if (!el) return;

      pendingFocusItemRef.current = null;
      setRevealItemId(undefined);

      const hiddenAncestor = el.closest("[hidden]");
      if (hiddenAncestor) {
        const group = hiddenAncestor.closest(
          "[data-testid^='attention-group-']",
        );
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
              {initialLoadError.message ||
                "Could not connect to the inspectah server."}
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
              aria-label={
                sidebarOverlayOpen ? "Close navigation" : "Open navigation"
              }
              aria-expanded={sidebarOverlayOpen}
              aria-controls="inspectah-sidebar-overlay"
              onClick={() => setSidebarOverlayOpen((prev) => !prev)}
            >
              &#x2630;
            </button>
          ) : undefined
        }
      >
        {({
          sectionSearchOpen,
          onSectionSearchClose,
          filterClearCounter,
          searchSlot,
        }) => (
          <>
            {!isMobile && (
              <div className="inspectah-layout__sidebar">
                <Sidebar
                  activeSection={activeSection}
                  onSelect={handleSidebarSelect}
                  stats={view.data?.stats ?? null}
                  sections={sections.data}
                  health={health.data}
                  viewData={view.data}
                  userDecisionCount={view.data?.users_groups_decisions?.length}
                  searchSlot={searchSlot}
                />
              </div>
            )}
            <div
              className="inspectah-layout__main"
              ref={mainContentRef}
              tabIndex={-1}
            >
              <MainContent
                activeSection={activeSection}
                loading={viewLoading}
                viewData={view.data}
                sections={sections.data}
                onViewUpdate={() => view.invalidate()}
                onMutationError={(err) =>
                  console.error("Mutation failed:", err.message)
                }
                sectionSearchOpen={sectionSearchOpen}
                onSectionSearchClose={onSectionSearchClose}
                filterClearCounter={filterClearCounter}
                revealItemId={revealItemId}
                onSetUndoFocusTarget={handleSetUndoFocusTarget}
              />
            </div>
            {isMobile && sidebarOverlayOpen && (
              <Sidebar
                activeSection={activeSection}
                onSelect={handleSidebarSelect}
                stats={view.data?.stats ?? null}
                sections={sections.data}
                health={health.data}
                viewData={view.data}
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
