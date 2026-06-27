import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import {
  Button,
  Page,
  PageSection,
  EmptyState,
  EmptyStateBody,
  Spinner,
} from "@patternfly/react-core";
import type {
  AggregateHealthInfo,
  HealthResponse,
  AggregateViewResponse,
  AggregateSection,
  AggregateItem,
  ItemId,
  RefinementOp,
  RefineStats,
  ReferenceSection,
} from "../api/types";
import { fetchAggregateView } from "../api/aggregate-client";
import "../aggregate.css";
import { useAggregateMutation } from "../hooks/useAggregateMutation";
import { useVariantAck } from "../hooks/useVariantAck";
import { useAggregateDiff } from "../hooks/useAggregateDiff";
import { useAggregateFocusRecovery } from "../hooks/useAggregateFocusRecovery";
import {
  PrevalenceDisplayContext,
  usePrevalenceDisplayState,
} from "../hooks/usePrevalenceDisplay";
import { AppShell } from "./AppShell";
import { AggregateSidebar } from "./aggregate/AggregateSidebar";
import { AggregateBanner } from "./aggregate/AggregateBanner";
import { AggregateSectionContent } from "./aggregate/AggregateSection";
import { itemDisplayName } from "./aggregate/AggregateItemRow";
import { RepoBar } from "./RepoBar";
import { PackageList } from "./PackageList";
import type { PackageListPackage } from "./PackageList";

export interface AggregateAppProps {
  aggregate: AggregateHealthInfo;
  health: HealthResponse;
}

/** Toolbar indicator showing unacked variant count. */
function AckProgress({
  unackedCount,
  totalCount,
}: {
  unackedCount: number;
  totalCount: number;
}) {
  if (totalCount === 0) return null;
  return (
    <span className="aggregate-ack-progress" data-testid="ack-progress">
      {unackedCount} of {totalCount} variants need review
    </span>
  );
}

/** Toolbar indicator showing unconfirmed divergent item count. */
function DivergentProgress({
  unconfirmedCount,
  totalCount,
}: {
  unconfirmedCount: number;
  totalCount: number;
}) {
  if (totalCount === 0) return null;
  return (
    <span className="aggregate-ack-progress" data-testid="divergent-progress">
      Divergent: {totalCount} ({unconfirmedCount} unconfirmed)
    </span>
  );
}

/** Collect all AggregateItems from a section (flat items or zone items). */
function sectionItems(section: AggregateSection): AggregateItem[] {
  if (section.items) return section.items;
  if (!section.zones) return [];
  return [
    ...section.zones.consensus.items,
    ...section.zones.near_consensus.items,
    ...section.zones.divergent.items,
  ];
}

/** Collect all divergent-zone AggregateItems across all sections. */
function allDivergentItems(sections: AggregateSection[]): AggregateItem[] {
  const result: AggregateItem[] = [];
  for (const section of sections) {
    if (section.zones) {
      result.push(...section.zones.divergent.items);
    }
  }
  return result;
}

/** Serialize an ItemId to a stable string key for Set membership. */
function itemIdKey(id: ItemId): string {
  return JSON.stringify(id);
}

/** Build ReferenceSection[] from aggregate sections for GlobalSearch indexing. */
function buildAggregateSearchSections(
  sections: AggregateSection[],
): ReferenceSection[] {
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

/** Build the correct RefinementOp for a aggregate item toggle. */
function buildToggleOp(itemId: ItemId, include: boolean): RefinementOp {
  return { op: "SetInclude", target: { item_id: itemId, include } };
}

export function AggregateApp({ aggregate, health: _health }: AggregateAppProps) {
  const prevalenceDisplay = usePrevalenceDisplayState();
  const [view, setView] = useState<AggregateViewResponse | null>(null);
  const [activeSection, setActiveSection] = useState("packages");
  const [error, setError] = useState<string | null>(null);
  const [expandedItemId, setExpandedItemId] = useState<ItemId | null>(null);
  const [filterText, setFilterText] = useState("");
  const [pendingNavTarget, setPendingNavTarget] = useState<{
    sectionId: string;
    itemId: ItemId;
  } | null>(null);
  useEffect(() => {
    fetchAggregateView()
      .then(setView)
      .catch((e) => setError(e.message));
  }, []);

  const { mutate, undo, redo, isPending, refetchError, retry } =
    useAggregateMutation(setView, (err) => setError(err.message));

  // --- Divergent review tracking (session-layer state) ---
  const [confirmedDivergentIds, setConfirmedDivergentIds] = useState<
    Set<string>
  >(() => new Set());

  const actionableIds =
    view?.summary.actionable_variant_items.map((v) => v.item_id) ?? [];
  const ack = useVariantAck(aggregate.label, aggregate.merged_at, actionableIds);
  const diffHook = useAggregateDiff();

  // Restore focus to the last-focused aggregate item after view updates
  useAggregateFocusRecovery(view?.generation ?? null);

  // Portal flow: when pendingNavTarget is set (by banner or search),
  // switch the active section. AggregateSectionContent handles the rest:
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

  // Build a set of divergent item keys for fast membership checks
  const divergentKeySet = useMemo(() => {
    if (!view) return new Set<string>();
    return new Set(
      allDivergentItems(view.sections).map((i) => itemIdKey(i.item_id)),
    );
  }, [view]);

  /** Mark a divergent item as confirmed in session state. */
  const confirmDivergent = useCallback(
    (itemId: ItemId) => {
      const key = itemIdKey(itemId);
      if (!divergentKeySet.has(key)) return;
      setConfirmedDivergentIds((prev) => {
        if (prev.has(key)) return prev;
        const next = new Set(prev);
        next.add(key);
        return next;
      });
    },
    [divergentKeySet],
  );

  const handleToggle = useCallback(
    (itemId: ItemId, include: boolean) => {
      mutate(buildToggleOp(itemId, include));
      confirmDivergent(itemId);
    },
    [mutate, confirmDivergent],
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
      mutate({
        op: "SelectVariant",
        target: { item_id: itemId, target: hash },
      });
      confirmDivergent(itemId);
    },
    [mutate, confirmDivergent],
  );

  const handleBannerNavigate = useCallback(
    (sectionId: string, itemId: ItemId) => {
      setPendingNavTarget({ sectionId, itemId });
    },
    [],
  );

  const handleSearchNavigate = useCallback(
    (sectionId: string, itemId: string) => {
      // itemId was serialized via JSON.stringify in buildAggregateSearchSections
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

  // --- Dismissed conflict state (bridges PackageList → RepoBar) ---
  const [dismissedCount, setDismissedCount] = useState(0);
  const [restoreDismissed, setRestoreDismissed] = useState(false);

  const handleRestoreDismissed = useCallback(() => {
    setRestoreDismissed(true);
    // Reset the flag after PackageList consumes it
    Promise.resolve().then(() => setRestoreDismissed(false));
  }, []);

  // --- Package / repo toggle handlers for unified PackageList ---

  // Convert AggregateItem[] from the packages section to PackageListPackage[]
  const aggregatePackages: PackageListPackage[] = useMemo(() => {
    if (!view) return [];
    const pkgSection = view.sections.find((s) => s.id === "packages");
    if (!pkgSection) return [];
    const items = sectionItems(pkgSection);
    return items.map((item) => ({
      name: itemDisplayName(item.item_id),
      source_repo: item.source_repo,
      include: item.include,
      prevalence: {
        count: item.prevalence.count,
        total: item.prevalence.total,
      },
      repo_conflict: item.repo_conflict,
    }));
  }, [view]);

  // Package toggle for unified PackageList: "name.arch" string → aggregate ItemId toggle
  const handleAggregatePackageToggle = useCallback(
    (nameArch: string) => {
      if (!view) return;
      const pkgSection = view.sections.find((s) => s.id === "packages");
      if (!pkgSection) return;
      const items = sectionItems(pkgSection);
      const item = items.find((i) => itemDisplayName(i.item_id) === nameArch);
      if (!item) return;
      // handleToggle already calls confirmDivergent
      handleToggle(item.item_id, !item.include);
    },
    [view, handleToggle],
  );

  // Repo toggle for unified RepoBar
  const handleAggregateRepoToggle = useCallback(
    (sectionId: string) => {
      if (!view) return;
      const repo = view.repo_groups.find((r) => r.section_id === sectionId);
      if (!repo) return;
      mutate({
        op: "SetInclude",
        target: {
          item_id: { kind: "Repo", key: { path: sectionId } },
          include: !repo.enabled,
        },
      });
    },
    [view, mutate],
  );

  // Loading state
  if (!view && !error) {
    return (
      <Page className="inspectah-page" data-testid="aggregate-app">
        <PageSection>
          <EmptyState titleText="Loading aggregate view..." headingLevel="h2">
            <Spinner size="xl" />
          </EmptyState>
        </PageSection>
      </Page>
    );
  }

  // Error state (no data at all)
  if (error && !view) {
    return (
      <Page className="inspectah-page" data-testid="aggregate-app">
        <PageSection>
          <EmptyState titleText="Failed to load aggregate view" headingLevel="h2">
            <EmptyStateBody>
              {error}
              <br />
              <Button
                variant="link"
                onClick={() => {
                  setError(null);
                  fetchAggregateView()
                    .then(setView)
                    .catch((e) => setError(e.message));
                }}
              >
                Retry
              </Button>
            </EmptyStateBody>
          </EmptyState>
        </PageSection>
      </Page>
    );
  }

  // view is guaranteed non-null past this point
  const aggregateView = view!;

  const activeAggregateSection = aggregateView.sections.find(
    (s) => s.id === activeSection,
  );

  const searchContextSections = buildAggregateSearchSections(aggregateView.sections);

  // Compute aggregate-level stats from section data
  const aggregateSectionCounts = aggregateView.sections.reduce(
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
    {
      totalPkg: 0,
      inclPkg: 0,
      exclPkg: 0,
      totalCfg: 0,
      inclCfg: 0,
      exclCfg: 0,
    },
  );

  const aggregateStats: RefineStats = {
    sections: [
      {
        kind: "package",
        total: aggregateSectionCounts.totalPkg,
        included: aggregateSectionCounts.inclPkg,
        excluded: aggregateSectionCounts.exclPkg,
      },
      {
        kind: "config",
        total: aggregateSectionCounts.totalCfg,
        included: aggregateSectionCounts.inclCfg,
        excluded: aggregateSectionCounts.exclCfg,
      },
    ],
    needs_review_count: ack.unackedCount,
    ops_applied: 0,
    baseline_available: false,
    can_undo: aggregateView.can_undo,
    can_redo: aggregateView.can_redo,
  };

  // Compute total items across all sections
  const totalItems = aggregateView.sections.reduce(
    (sum, section) => sum + sectionItems(section).length,
    0,
  );

  // Compute divergent review progress
  const totalDivergent = divergentKeySet.size;
  const unconfirmedDivergent =
    totalDivergent -
    [...divergentKeySet].filter((key) => confirmedDivergentIds.has(key)).length;

  return (
    <PrevalenceDisplayContext.Provider value={prevalenceDisplay}>
    <div data-testid="aggregate-app">
      <AppShell
        sidebar={null}
        containerfilePreview={aggregateView.containerfile_preview}
        stats={aggregateStats}
        generation={aggregateView.generation}
        sessionIsSensitive={aggregateView.session_is_sensitive}
        onUndo={undo}
        onRedo={redo}
        onExportComplete={() => {
          fetchAggregateView().then(setView);
        }}
        isPending={isPending}
        activeSection={activeSection}
        onNavigateSection={setActiveSection}
        searchPackageItems={[]}
        searchConfigItems={[]}
        searchContextSections={searchContextSections}
        onSearchNavigate={handleSearchNavigate}
        toolbarExtra={
          <>
            <AckProgress
              unackedCount={ack.unackedCount}
              totalCount={ack.totalCount}
            />
            <DivergentProgress
              unconfirmedCount={unconfirmedDivergent}
              totalCount={totalDivergent}
            />
          </>
        }
        extraShortcuts={[{ key: "c", description: "Compare variants" }]}
        aggregateSummary={{
          hostCount: aggregate.host_count,
          hostnames: aggregate.hostnames,
          totalItems,
          needsReviewCount: ack.unackedCount,
        }}
        isAggregateMode
        sectionIds={aggregateView.sections.map((s) => s.id)}
      >
        {({
          sectionSearchOpen,
          onSectionSearchClose,
          filterClearCounter,
          searchSlot,
        }) => {
          // Sync filterText reset with shell's filterClearCounter
          if (filterClearCounter !== filterClearRef.current) {
            filterClearRef.current = filterClearCounter;
            // Schedule state update outside render via microtask
            Promise.resolve().then(() => setFilterText(""));
          }

          return (
            <>
              <div className="inspectah-layout__sidebar">
                <AggregateSidebar
                  sections={aggregateView.sections}
                  activeSection={activeSection}
                  onSelect={setActiveSection}
                  ackState={ack}
                  searchSlot={searchSlot}
                />
              </div>
              <div
                className="inspectah-layout__main aggregate-content"
                data-testid="aggregate-content"
              >
                <AggregateBanner
                  summary={aggregateView.summary}
                  ackState={ack}
                  onNavigate={handleBannerNavigate}
                  activeSection={activeSection}
                />
                {sectionSearchOpen && (
                  <div
                    className="aggregate-section-search"
                    data-testid="aggregate-section-search"
                  >
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
                    <Button variant="link" onClick={retry}>
                      Retry
                    </Button>
                  </div>
                )}
                {activeSection === "packages" ? (
                  <>
                    <RepoBar
                      repos={aggregateView.repo_groups}
                      onToggle={handleAggregateRepoToggle}
                      conflictCount={aggregateView.repo_conflict_count}
                      dismissedCount={dismissedCount}
                      onRestoreDismissed={handleRestoreDismissed}
                    />
                    <PackageList
                      mode="aggregate"
                      packages={aggregatePackages}
                      repoGroups={aggregateView.repo_groups}
                      onToggle={handleAggregatePackageToggle}
                      onRepoToggle={handleAggregateRepoToggle}
                      onDismissedCountChange={setDismissedCount}
                      onRestoreDismissed={restoreDismissed}
                    />
                  </>
                ) : (
                  <AggregateSectionContent
                    section={activeAggregateSection}
                    filterText={filterText}
                    isDecisionSection={
                      activeAggregateSection?.is_decision_section ?? false
                    }
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
    </PrevalenceDisplayContext.Provider>
  );
}
