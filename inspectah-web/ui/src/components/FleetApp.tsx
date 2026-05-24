import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import { Button, Page, PageSection, EmptyState, EmptyStateBody, Spinner } from "@patternfly/react-core";
import type {
  FleetHealthInfo,
  HealthResponse,
  FleetViewResponse,
  FleetSection,
  FleetItem,
  ItemId,
  RefinementOp,
  RefineStats,
  ContextSection,
} from "../api/types";
import { fetchFleetView } from "../api/fleet-client";
import "../fleet.css";
import { useFleetMutation } from "../hooks/useFleetMutation";
import { useVariantAck } from "../hooks/useVariantAck";
import { useFleetDiff } from "../hooks/useFleetDiff";
import { useFleetFocusRecovery } from "../hooks/useFleetFocusRecovery";
import { AppShell } from "./AppShell";
import { FleetSidebar } from "./fleet/FleetSidebar";
import { FleetBanner } from "./fleet/FleetBanner";
import { FleetSectionContent } from "./fleet/FleetSection";
import { itemDisplayName } from "./fleet/FleetItemRow";
import { RepoBar } from "./RepoBar";
import { PackageList } from "./PackageList";
import type { PackageListPackage } from "./PackageList";

export interface FleetAppProps {
  fleet: FleetHealthInfo;
  health: HealthResponse;
}

/** Toolbar indicator showing unacked variant count. */
function AckProgress({ unackedCount, totalCount }: { unackedCount: number; totalCount: number }) {
  if (totalCount === 0) return null;
  return (
    <span className="fleet-ack-progress" data-testid="ack-progress">
      {unackedCount} of {totalCount} variants need review
    </span>
  );
}

/** Collect all FleetItems from a section (flat items or zone items). */
function sectionItems(section: FleetSection): FleetItem[] {
  if (section.items) return section.items;
  if (!section.zones) return [];
  return [
    ...section.zones.consensus.items,
    ...section.zones.near_consensus.items,
    ...section.zones.divergent.items,
  ];
}

/** Build ContextSection[] from fleet sections for GlobalSearch indexing. */
function buildFleetSearchSections(sections: FleetSection[]): ContextSection[] {
  return sections.map((s) => ({
    id: s.id,
    display_name: s.display_name,
    items: sectionItems(s).map((item) => {
      const name = itemDisplayName(item.item_id);
      return {
        id: JSON.stringify(item.item_id),
        title: name,
        subtitle: null,
        detail: null,
        searchable_text: name,
      };
    }),
  }));
}

/** Build the correct RefinementOp for a fleet item toggle. */
function buildToggleOp(itemId: ItemId, include: boolean): RefinementOp {
  if (itemId.kind === "Package") {
    const [name, arch] = itemId.key.name_arch.split(".");
    return include
      ? { op: "IncludePackage", target: { name, arch } }
      : { op: "ExcludePackage", target: { name, arch } };
  }
  if (itemId.kind === "Config") {
    return include
      ? { op: "IncludeConfig", target: { path: itemId.key.path } }
      : { op: "ExcludeConfig", target: { path: itemId.key.path } };
  }
  if (itemId.kind === "Repo") {
    return include
      ? { op: "IncludeRepo", target: { section_id: itemId.key.path } }
      : { op: "ExcludeRepo", target: { section_id: itemId.key.path } };
  }
  // Other item kinds don't have dedicated include/exclude ops;
  // fall back to ExcludeConfig with the item's display name as path.
  const name = itemDisplayName(itemId);
  return include
    ? { op: "IncludeConfig", target: { path: name } }
    : { op: "ExcludeConfig", target: { path: name } };
}

export function FleetApp({ fleet, health: _health }: FleetAppProps) {
  const [view, setView] = useState<FleetViewResponse | null>(null);
  const [activeSection, setActiveSection] = useState("packages");
  const [error, setError] = useState<string | null>(null);
  const [expandedItemId, setExpandedItemId] = useState<ItemId | null>(null);
  const [filterText, setFilterText] = useState("");
  const [pendingNavTarget, setPendingNavTarget] = useState<{
    sectionId: string;
    itemId: ItemId;
  } | null>(null);
  useEffect(() => {
    fetchFleetView().then(setView).catch((e) => setError(e.message));
  }, []);

  const { mutate, undo, redo, isPending, refetchError, retry } = useFleetMutation(
    setView,
    (err) => setError(err.message),
  );

  const actionableIds = view?.summary.actionable_variant_items.map((v) => v.item_id) ?? [];
  const ack = useVariantAck(fleet.label, fleet.merged_at, actionableIds);
  const diffHook = useFleetDiff();

  // Restore focus to the last-focused fleet item after view updates
  useFleetFocusRecovery(view?.generation ?? null);

  // Portal flow: when pendingNavTarget is set (by banner or search),
  // switch the active section. FleetSectionContent handles the rest:
  // force-expand the zone, auto-expand variants, scroll, highlight, focus.
  useEffect(() => {
    if (!pendingNavTarget) return;
    setActiveSection(pendingNavTarget.sectionId);
  }, [pendingNavTarget]);

  const handleNavTargetConsumed = useCallback(() => {
    setPendingNavTarget(null);
  }, []);

  // Track shell's filterClearCounter to reset filterText when
  // GlobalSearch navigation clears the section search.
  const filterClearRef = useRef(0);

  // Clear section filter and close variant view when switching sections
  useEffect(() => {
    setFilterText("");
    setExpandedItemId(null);
  }, [activeSection]);

  const handleToggle = useCallback(
    (itemId: ItemId, include: boolean) => {
      mutate(buildToggleOp(itemId, include));
    },
    [mutate],
  );

  const handleExpandVariant = useCallback((itemId: ItemId) => {
    setExpandedItemId((prev) =>
      prev && JSON.stringify(prev) === JSON.stringify(itemId) ? null : itemId,
    );
  }, []);

  const handleForceExpandVariant = useCallback((itemId: ItemId) => {
    setExpandedItemId(itemId);
  }, []);

  const handleSelectVariant = useCallback(
    (itemId: ItemId, hash: string) => {
      mutate({ op: "SelectVariant", target: { item_id: itemId, target: hash } });
    },
    [mutate],
  );

  const handleBannerNavigate = useCallback(
    (sectionId: string, itemId: ItemId) => {
      setPendingNavTarget({ sectionId, itemId });
    },
    [],
  );

  const handleSearchNavigate = useCallback(
    (sectionId: string, itemId: string) => {
      // itemId was serialized via JSON.stringify in buildFleetSearchSections
      setFilterText(""); // clear any active section filter
      try {
        const parsed = JSON.parse(itemId) as ItemId;
        setPendingNavTarget({ sectionId, itemId: parsed });
      } catch {
        // Fallback: just navigate to the section
        setActiveSection(sectionId);
      }
    },
    [],
  );

  // --- Package / repo toggle handlers for unified PackageList ---

  // Convert FleetItem[] from the packages section to PackageListPackage[]
  const fleetPackages: PackageListPackage[] = useMemo(() => {
    if (!view) return [];
    const pkgSection = view.sections.find((s) => s.id === "packages");
    if (!pkgSection) return [];
    const items = sectionItems(pkgSection);
    return items.map((item) => ({
      name: itemDisplayName(item.item_id),
      source_repo: item.source_repo,
      include: item.include,
      prevalence: { count: item.prevalence.count, total: item.prevalence.total },
      repo_conflict: item.repo_conflict,
    }));
  }, [view]);

  // Package toggle for unified PackageList: "name.arch" string → fleet ItemId toggle
  const handleFleetPackageToggle = useCallback(
    (nameArch: string) => {
      if (!view) return;
      const pkgSection = view.sections.find((s) => s.id === "packages");
      if (!pkgSection) return;
      const items = sectionItems(pkgSection);
      const item = items.find((i) => itemDisplayName(i.item_id) === nameArch);
      if (!item) return;
      handleToggle(item.item_id, !item.include);
    },
    [view, handleToggle],
  );

  // Repo toggle for unified RepoBar
  const handleFleetRepoToggle = useCallback(
    (sectionId: string) => {
      if (!view) return;
      const repo = view.repo_groups.find((r) => r.section_id === sectionId);
      if (!repo) return;
      const op: RefinementOp = repo.enabled
        ? { op: "ExcludeRepo", target: { section_id: sectionId } }
        : { op: "IncludeRepo", target: { section_id: sectionId } };
      mutate(op);
    },
    [view, mutate],
  );

  // Loading state
  if (!view && !error) {
    return (
      <Page className="inspectah-page" data-testid="fleet-app">
        <PageSection>
          <EmptyState titleText="Loading fleet view..." headingLevel="h2">
            <Spinner size="xl" />
          </EmptyState>
        </PageSection>
      </Page>
    );
  }

  // Error state (no data at all)
  if (error && !view) {
    return (
      <Page className="inspectah-page" data-testid="fleet-app">
        <PageSection>
          <EmptyState titleText="Failed to load fleet view" headingLevel="h2">
            <EmptyStateBody>
              {error}
              <br />
              <Button variant="link" onClick={() => {
                setError(null);
                fetchFleetView().then(setView).catch((e) => setError(e.message));
              }}>
                Retry
              </Button>
            </EmptyStateBody>
          </EmptyState>
        </PageSection>
      </Page>
    );
  }

  // view is guaranteed non-null past this point
  const fleetView = view!;

  const activeFleetSection = fleetView.sections.find((s) => s.id === activeSection);

  const searchContextSections = buildFleetSearchSections(fleetView.sections);

  // Compute fleet-level stats from section data
  const fleetSectionCounts = fleetView.sections.reduce(
    (acc, section) => {
      const items = sectionItems(section);
      const included = items.filter((i) => i.include).length;
      const excluded = items.length - included;
      if (section.id === "packages") {
        acc.totalPkg = items.length;
        acc.inclPkg = included;
        acc.exclPkg = excluded;
      } else if (section.id === "config_files") {
        acc.totalCfg = items.length;
        acc.inclCfg = included;
        acc.exclCfg = excluded;
      }
      return acc;
    },
    { totalPkg: 0, inclPkg: 0, exclPkg: 0, totalCfg: 0, inclCfg: 0, exclCfg: 0 },
  );

  const fleetStats: RefineStats = {
    total_packages: fleetSectionCounts.totalPkg,
    included_packages: fleetSectionCounts.inclPkg,
    excluded_packages: fleetSectionCounts.exclPkg,
    total_configs: fleetSectionCounts.totalCfg,
    included_configs: fleetSectionCounts.inclCfg,
    excluded_configs: fleetSectionCounts.exclCfg,
    package_managed_configs: 0,
    needs_review_count: ack.unackedCount,
    ops_applied: 0,
    baseline_available: false,
    can_undo: fleetView.can_undo,
    can_redo: fleetView.can_redo,
  };

  // Compute total items across all sections
  const totalItems = fleetView.sections.reduce(
    (sum, section) => sum + sectionItems(section).length,
    0,
  );

  return (
    <div data-testid="fleet-app">
      <AppShell
        sidebar={null}
        containerfilePreview={fleetView.containerfile_preview}
        stats={fleetStats}
        generation={fleetView.generation}
        sessionIsSensitive={fleetView.session_is_sensitive}
        onUndo={undo}
        onRedo={redo}
        onExportComplete={() => {
          fetchFleetView().then(setView);
        }}
        isPending={isPending}
        activeSection={activeSection}
        onNavigateSection={setActiveSection}
        searchPackageItems={[]}
        searchConfigItems={[]}
        searchContextSections={searchContextSections}
        onSearchNavigate={handleSearchNavigate}
        toolbarExtra={<AckProgress unackedCount={ack.unackedCount} totalCount={ack.totalCount} />}
        extraShortcuts={[{ key: "c", description: "Compare variants" }]}
        fleetSummary={{
          hostCount: fleet.host_count,
          totalItems,
          needsReviewCount: ack.unackedCount,
        }}
        isFleetMode
      >
        {({ sectionSearchOpen, onSectionSearchClose, filterClearCounter, searchSlot }) => {
          // Sync filterText reset with shell's filterClearCounter
          if (filterClearCounter !== filterClearRef.current) {
            filterClearRef.current = filterClearCounter;
            // Schedule state update outside render via microtask
            Promise.resolve().then(() => setFilterText(""));
          }

          return (
          <>
          <div className="inspectah-layout__sidebar">
            <FleetSidebar
              sections={fleetView.sections}
              activeSection={activeSection}
              onSelect={setActiveSection}
              ackState={ack}
              searchSlot={searchSlot}
            />
          </div>
          <div className="inspectah-layout__main fleet-content" data-testid="fleet-content">
            <FleetBanner
              summary={fleetView.summary}
              ackState={ack}
              onNavigate={handleBannerNavigate}
              activeSection={activeSection}
            />
            {sectionSearchOpen && (
              <div className="fleet-section-search" data-testid="fleet-section-search">
                <input
                  type="text"
                  placeholder="Filter items..."
                  autoFocus
                  value={filterText}
                  onChange={(e) => setFilterText(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Escape") {
                      setFilterText("");
                      onSectionSearchClose();
                    }
                  }}
                  aria-label="Filter section items"
                />
              </div>
            )}
            {refetchError && (
              <div className="refetch-error" data-testid="refetch-error">
                {refetchError}
                <Button variant="link" onClick={retry}>Retry</Button>
              </div>
            )}
            {activeSection === "packages" ? (
              <>
                <RepoBar
                  repos={fleetView.repo_groups}
                  onToggle={handleFleetRepoToggle}
                  conflictCount={fleetView.repo_conflict_count}
                />
                <PackageList
                  mode="fleet"
                  packages={fleetPackages}
                  repoGroups={fleetView.repo_groups}
                  onToggle={handleFleetPackageToggle}
                  onRepoToggle={handleFleetRepoToggle}
                />
              </>
            ) : (
              <FleetSectionContent
                section={activeFleetSection}
                filterText={filterText}
                isDecisionSection={activeFleetSection?.is_decision_section ?? false}
                onToggle={handleToggle}
                ack={ack}
                onExpandVariant={handleExpandVariant}
                onForceExpandVariant={handleForceExpandVariant}
                pendingNavTarget={pendingNavTarget}
                onNavTargetConsumed={handleNavTargetConsumed}
                expandedItemId={expandedItemId}
                onSelectVariant={handleSelectVariant}
                diffHook={diffHook}
              />
            )}
          </div>
          </>
          );
        }}
      </AppShell>
    </div>
  );
}
