import { useState, useCallback } from "react";
import { Page } from "@patternfly/react-core";
import type { RefinedView } from "./api/types";
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
  const [globalSearchOpen, setGlobalSearchOpen] = useState(false);
  const [sectionSearchOpen, setSectionSearchOpen] = useState(false);

  const view = useView();
  const sections = useSections();
  const health = useHealth();

  const onMutationSuccess = useCallback(
    (_result: RefinedView) => {
      // After successful mutation, refetch view data
      view.invalidate();
    },
    [view.invalidate],
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
    // Export will be implemented in Task 8
    console.log("Export triggered");
  }, []);

  const handleToggleShortcuts = useCallback(() => {
    setShortcutsOpen((prev) => !prev);
  }, []);

  const handleOpenGlobalSearch = useCallback(() => {
    setGlobalSearchOpen(true);
  }, []);

  const handleOpenSectionSearch = useCallback(() => {
    setSectionSearchOpen(true);
  }, []);

  const handleNavigateFromGlobalSearch = useCallback(
    (sectionId: string, _itemId: string) => {
      setActiveSection(sectionId);
      setGlobalSearchOpen(false);
    },
    [],
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

  return (
    <Page className="inspectah-page">
      <StatsBar
        stats={view.data?.stats ?? null}
        onUndo={mutation.undo}
        onRedo={mutation.redo}
        onExport={handleExport}
        isPending={mutation.isPending}
      />
      <div className="inspectah-layout">
        <div className="inspectah-layout__sidebar">
          <Sidebar
            activeSection={activeSection}
            onSelect={setActiveSection}
            stats={view.data?.stats ?? null}
            sections={sections.data}
            health={health.data}
          />
        </div>
        <div className="inspectah-layout__main">
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
      <ShortcutOverlay
        isOpen={shortcutsOpen}
        onClose={() => setShortcutsOpen(false)}
      />
      <GlobalSearch
        isOpen={globalSearchOpen}
        onClose={() => setGlobalSearchOpen(false)}
        packageItems={view.data ? view.data.packages.map((p) => ({ type: "package" as const, data: p })) : []}
        configItems={view.data ? view.data.config_files.map((c) => ({ type: "config" as const, data: c })) : []}
        contextSections={sections.data}
        onNavigate={handleNavigateFromGlobalSearch}
      />
    </Page>
  );
}

export default App;
