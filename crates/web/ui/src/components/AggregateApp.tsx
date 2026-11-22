import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import {
  Alert,
  AlertActionCloseButton,
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
  LanguagePackageMetadata,
  UnmanagedFileMetadata,
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
import {
  MACHINE_STATE_ITEM_PREFIX,
  MachineStateGroup,
  revealMachineStateRow,
} from "./MachineStateGroup";

/** The fleet's rows offer no control, so the block never sends an op. */
const NO_MACHINE_STATE_OP = () => {};
import { AggregateSidebar } from "./aggregate/AggregateSidebar";
import { AggregateBanner } from "./aggregate/AggregateBanner";
import { AggregateSectionContent } from "./aggregate/AggregateSection";
import { itemDisplayName } from "./aggregate/AggregateItemRow";
import { RepoBar } from "./RepoBar";
import {
  inventoryRefusalText,
  repoFailure,
  type RepoFailure,
} from "./repoControl";
import { SysctlPanel } from "./SysctlPanel";
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

/**
 * Build enriched searchable text for a single aggregate item.
 * For language_packages and unmanaged_files, appends section_metadata
 * fields so that searches like "flask" or "elf" match the right items.
 */
function itemSearchableText(
  sectionId: string,
  displayName: string,
  item: AggregateItem,
): string {
  if (sectionId === "language_packages" && item.section_metadata) {
    const meta = item.section_metadata as unknown as LanguagePackageMetadata;
    const packageNames = (meta.packages ?? []).map((p) => p.name).join(" ");
    return [
      displayName,
      meta.ecosystem,
      packageNames,
      meta.manifest_basis ?? "",
    ]
      .filter(Boolean)
      .join(" ");
  }
  if (sectionId === "unmanaged_files" && item.section_metadata) {
    const meta = item.section_metadata as unknown as UnmanagedFileMetadata;
    return [displayName, meta.file_type].filter(Boolean).join(" ");
  }
  return displayName;
}

/** Build ReferenceSection[] from aggregate sections for GlobalSearch indexing. */
export function buildAggregateSearchSections(
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
        searchable_text: itemSearchableText(s.id, name, item),
      };
    }),
  }));
}

/** Build the correct RefinementOp for a aggregate item toggle. */
function buildToggleOp(itemId: ItemId, include: boolean): RefinementOp {
  return { op: "SetInclude", target: { item_id: itemId, include } };
}

export function AggregateApp({
  aggregate,
  health: _health,
}: AggregateAppProps) {
  const prevalenceDisplay = usePrevalenceDisplayState();
  const [view, setView] = useState<AggregateViewResponse | null>(null);
  // Every fetched view is shown through this. Fetches can answer out of
  // order (a refused repo op's recovery fetch runs outside the mutation
  // queue), and the server's generation only grows, undo and redo
  // included, so an older view never replaces a newer one.
  const publishView = useCallback((next: AggregateViewResponse) => {
    setView((prev) =>
      prev !== null && next.generation < prev.generation ? prev : next,
    );
  }, []);
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
      .then(publishView)
      .catch((e) => setError(e.message));
  }, [publishView]);

  // A refused repo op is shown on its row, the way the single-host
  // Repositories section shows it; a `nothing to change:` means the view
  // was stale, so it is fetched again. A refused op on a row another
  // section decides is information about that row, never a failure.
  const [repoFailures, setRepoFailures] = useState<
    ReadonlyMap<string, RepoFailure>
  >(() => new Map());
  const [opNotice, setOpNotice] = useState<string | null>(null);
  const clearRepoFailure = useCallback((sectionId: string) => {
    setRepoFailures((prev) => {
      if (!prev.has(sectionId)) return prev;
      const next = new Map(prev);
      next.delete(sectionId);
      return next;
    });
  }, []);
  const handleMutationError = useCallback(
    (err: Error, op?: RefinementOp) => {
      const inventory = inventoryRefusalText(err);
      if (inventory !== null) {
        setOpNotice(inventory);
        return;
      }
      if (op?.op === "SetInclude" && op.target.item_id.kind === "Repo") {
        const sectionId = op.target.item_id.key.path;
        const failure = repoFailure(err);
        setRepoFailures((prev) => new Map(prev).set(sectionId, failure));
        if (failure.kind === "nothing_to_change") {
          fetchAggregateView()
            .then(publishView)
            .catch((e: unknown) =>
              setError(e instanceof Error ? e.message : String(e)),
            );
        }
        return;
      }
      setError(err.message);
    },
    [publishView],
  );
  const {
    mutate,
    undo: undoOp,
    redo: redoOp,
    isPending,
    refetchError,
    retry,
  } = useAggregateMutation(publishView, handleMutationError);
  // An Undo or Redo can replace any repo's state, so it ends every
  // failure the rows show.
  const undo = useCallback(() => {
    setRepoFailures(new Map());
    setOpNotice(null);
    undoOp();
  }, [undoOp]);
  const redo = useCallback(() => {
    setRepoFailures(new Map());
    setOpNotice(null);
    redoOp();
  }, [redoOp]);

  // --- Divergent review tracking (session-layer state) ---
  const [confirmedDivergentIds, setConfirmedDivergentIds] = useState<
    Set<string>
  >(() => new Set());

  const actionableIds =
    view?.summary.actionable_variant_items.map((v) => v.item_id) ?? [];
  const ack = useVariantAck(
    aggregate.label,
    aggregate.merged_at,
    actionableIds,
  );
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

  // Section navigation by the user (sidebar, number keys, a search result
  // with no item) supersedes any portal target still waiting to be revealed.
  // Without this, leaving the section before its reveal ran left the target
  // pending, and the next visit to that section revealed it.
  const handleSelectSection = useCallback((sectionId: string) => {
    setPendingNavTarget(null);
    setActiveSection(sectionId);
  }, []);

  // Track shell's filterClearCounter to reset filterText when
  // GlobalSearch navigation clears the section search.
  const filterClearRef = useRef(0);

  // A GlobalSearch pick of a machine-state row: its section opens with the
  // block expanded, and the row is scrolled to and focused. Each reveal is
  // consumed once the row has focus, so picking the same row again, after
  // the block was collapsed, is a new reveal.
  const [machineStateReveal, setMachineStateReveal] = useState<
    string | undefined
  >(undefined);
  // Read by the section-change focus below, which must not move focus to
  // the section's first row while a machine-state row is being revealed.
  const machineStateRevealRef = useRef<string | undefined>(undefined);
  machineStateRevealRef.current = machineStateReveal;

  // Clear section filter and close variant view when switching sections,
  // then move focus to the first reviewable item in the new section.
  useEffect(() => {
    setFilterText("");
    setExpandedItemId(null);
    // Defer focus to next frame so the section content has rendered.
    const timer = setTimeout(() => {
      if (machineStateRevealRef.current !== undefined) return;
      const firstRow = document.querySelector(
        '[data-testid="aggregate-content"] [data-testid="aggregate-item-row"]',
      ) as HTMLElement | null;
      if (firstRow) firstRow.focus();
    }, 0);
    return () => clearTimeout(timer);
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

  // Scroll to and focus the revealed machine-state row, then consume the
  // reveal. Declared after the section-change focus above, so on a section
  // change this timer runs after that one, which leaves focus alone while
  // a reveal is pending. The block expands in the render that set the
  // reveal, so the row is in the document by now.
  useEffect(() => {
    if (machineStateReveal === undefined) return;
    const timer = setTimeout(() => {
      if (revealMachineStateRow(machineStateReveal)) {
        setMachineStateReveal(undefined);
      }
    }, 0);
    return () => clearTimeout(timer);
  }, [machineStateReveal, activeSection, view]);

  const handleSearchNavigate = useCallback(
    (sectionId: string, itemId: string) => {
      setFilterText(""); // clear any active section filter
      if (itemId.startsWith(MACHINE_STATE_ITEM_PREFIX)) {
        setMachineStateReveal(itemId);
        handleSelectSection(sectionId);
        return;
      }
      // itemId was serialized via JSON.stringify in buildAggregateSearchSections
      try {
        const parsed = JSON.parse(itemId) as ItemId;
        setPendingNavTarget({ sectionId, itemId: parsed });
      } catch {
        // Fallback: just navigate to the section
        handleSelectSection(sectionId);
      }
    },
    [handleSelectSection],
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
    (sectionId: string, include: boolean) => {
      if (!view) return;
      const repo = view.repo_groups.find((r) => r.section_id === sectionId);
      if (!repo) return;
      clearRepoFailure(sectionId);
      mutate({
        op: "SetInclude",
        target: {
          item_id: { kind: "Repo", key: { path: sectionId } },
          include,
        },
      });
    },
    [view, mutate, clearRepoFailure],
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
          <EmptyState
            titleText="Failed to load aggregate view"
            headingLevel="h2"
          >
            <EmptyStateBody>
              {error}
              <br />
              <Button
                variant="link"
                onClick={() => {
                  setError(null);
                  fetchAggregateView()
                    .then(publishView)
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

  const searchContextSections = buildAggregateSearchSections(
    aggregateView.sections,
  );

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
            fetchAggregateView().then(publishView);
          }}
          isPending={isPending}
          activeSection={activeSection}
          onNavigateSection={handleSelectSection}
          searchPackageItems={[]}
          searchConfigItems={[]}
          searchContextSections={searchContextSections}
          onSearchNavigate={handleSearchNavigate}
          machineStateRows={aggregateView.machine_state}
          machineStateSteps={aggregateView.machine_state_steps}
          machineStateNotices={aggregateView.machine_state_notices}
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
                    onSelect={handleSelectSection}
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
                  {opNotice && (
                    <Alert
                      variant="info"
                      isInline
                      isPlain
                      title={opNotice}
                      data-testid="aggregate-op-notice"
                      actionClose={
                        <AlertActionCloseButton
                          aria-label="Dismiss the message"
                          onClose={() => setOpNotice(null)}
                        />
                      }
                    />
                  )}
                  {activeSection === "packages" ? (
                    <>
                      <RepoBar
                        repos={aggregateView.repo_groups}
                        onToggle={handleAggregateRepoToggle}
                        failures={repoFailures}
                        onDismissFailure={clearRepoFailure}
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
                    <>
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
                    {activeSection === "system_tuning" && (
                      <SysctlPanel sysctl={aggregateView.sysctl} />
                    )}
                    {(activeSection === "configs" ||
                      activeSection === "config_files") && (
                      // Every fleet row is withheld (`override: "never"`,
                      // no `item_id`), so the block offers no control and
                      // nothing here sends an op.
                      <MachineStateGroup
                        rows={aggregateView.machine_state}
                        stepCount={aggregateView.machine_state_steps.length}
                        filterText={filterText}
                        onSetInclude={NO_MACHINE_STATE_OP}
                        revealItemId={machineStateReveal}
                      />
                    )}
                    </>
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
