import { useCallback, useMemo, useState, useRef, useEffect } from "react";
import {
  Alert,
  AlertGroup,
  AlertActionCloseButton,
  AlertVariant,
  EmptyState,
  EmptyStateBody,
} from "@patternfly/react-core";
import { AngleRightIcon, AngleDownIcon } from "@patternfly/react-icons";
import type {
  AttentionLevel,
  RefinementOp,
  ViewResponse,
  RepoGroupInfo,
} from "../api/types";
import { fetchView } from "../api/client";
import { ApiError } from "../api/types";
import { useMutation } from "../hooks/useMutation";
import { useViewed } from "../hooks/useViewed";
import { AttentionGroup } from "./AttentionGroup";
import { RepoGroupHeader } from "./RepoGroupHeader";
import { DecisionItem, itemId as getItemId } from "./DecisionItem";
import type { DecisionItemKind } from "./DecisionItem";
import { highestAttention } from "./attentionUtils";
import type { ViewMode } from "./MainContent";

interface GroupedItems {
  needs_review: DecisionItemKind[];
  informational: DecisionItemKind[];
  routine: DecisionItemKind[];
}

function groupByAttention(items: DecisionItemKind[]): GroupedItems {
  const groups: GroupedItems = {
    needs_review: [],
    informational: [],
    routine: [],
  };
  for (const item of items) {
    const level =
      item.data.attention.length > 0
        ? highestAttention(item.data.attention)
        : "routine";
    groups[level].push(item);
  }
  return groups;
}

interface ToastEntry {
  id: number;
  message: string;
  variant: AlertVariant;
}

/** Collapsed summary for Tier 1 baseline-match packages. */
function BaselineSummary({ count, items, revealItemId, defaultExpanded = false, filterActive = false }: { count: number; items: DecisionItemKind[]; revealItemId?: string; defaultExpanded?: boolean; filterActive?: boolean }) {
  const [isExpanded, setIsExpanded] = useState(defaultExpanded);

  // Sync with defaultExpanded when viewMode changes
  useEffect(() => {
    setIsExpanded(defaultExpanded);
  }, [defaultExpanded]);

  // Auto-expand when revealItemId matches an item in this summary
  useEffect(() => {
    if (!revealItemId) return;
    const match = items.some((item) => getItemId(item) === revealItemId);
    if (match && !isExpanded) {
      setIsExpanded(true);
    }
  }, [revealItemId, items, isExpanded]);

  // Auto-expand when section search filter is active and has matching items
  useEffect(() => {
    if (filterActive && !isExpanded) {
      setIsExpanded(true);
    }
  }, [filterActive]); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <div data-testid="baseline-summary" style={{ marginBottom: "var(--pf-t--global--spacer--sm)" }}>
      <button
        type="button"
        onClick={() => setIsExpanded((prev) => !prev)}
        aria-expanded={isExpanded}
        style={{
          background: "none",
          border: "none",
          cursor: "pointer",
          padding: "var(--pf-t--global--spacer--xs) 0",
          fontSize: "var(--pf-t--global--font--size--body--default)",
          color: "var(--pf-t--global--text--color--subtle)",
          display: "flex",
          alignItems: "center",
          gap: "var(--pf-t--global--spacer--xs)",
        }}
      >
        {isExpanded ? <AngleDownIcon /> : <AngleRightIcon />}
        {count} baseline packages (auto-included)
      </button>
      {isExpanded && (
        <ul style={{ listStyle: "none", padding: 0, margin: 0 }}>
          {items.map((item) => {
            const id = getItemId(item);
            const name = item.type === "package"
              ? `${item.data.entry.name}.${(item.data as any).entry.arch}`
              : (item.data as any).entry.path;
            return (
              <li
                key={name}
                data-testid={`decision-item-${id}`}
                tabIndex={-1}
                style={{
                  padding: "var(--pf-t--global--spacer--xs) var(--pf-t--global--spacer--md)",
                  color: "var(--pf-t--global--text--color--subtle)",
                  fontSize: "var(--pf-t--global--font--size--body--sm)",
                }}
              >
                {name}
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}

/** Collapsed summary for Tier 1 package-managed configs (config_default / config_baseline_match). */
function ConfigManagedSummary({ count, items, revealItemId, defaultExpanded = false, filterActive = false }: { count: number; items: DecisionItemKind[]; revealItemId?: string; defaultExpanded?: boolean; filterActive?: boolean }) {
  const [isExpanded, setIsExpanded] = useState(defaultExpanded);

  // Sync with defaultExpanded when viewMode changes
  useEffect(() => {
    setIsExpanded(defaultExpanded);
  }, [defaultExpanded]);

  // Auto-expand when revealItemId matches an item in this summary
  useEffect(() => {
    if (!revealItemId) return;
    const match = items.some((item) => getItemId(item) === revealItemId);
    if (match && !isExpanded) {
      setIsExpanded(true);
    }
  }, [revealItemId, items, isExpanded]);

  // Auto-expand when section search filter is active and has matching items
  useEffect(() => {
    if (filterActive && !isExpanded) {
      setIsExpanded(true);
    }
  }, [filterActive]); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <div data-testid="config-managed-summary" style={{ marginBottom: "var(--pf-t--global--spacer--sm)" }}>
      <button
        type="button"
        onClick={() => setIsExpanded((prev) => !prev)}
        aria-expanded={isExpanded}
        style={{
          background: "none",
          border: "none",
          cursor: "pointer",
          padding: "var(--pf-t--global--spacer--xs) 0",
          fontSize: "var(--pf-t--global--font--size--body--default)",
          color: "var(--pf-t--global--text--color--subtle)",
          display: "flex",
          alignItems: "center",
          gap: "var(--pf-t--global--spacer--xs)",
        }}
      >
        {isExpanded ? <AngleDownIcon /> : <AngleRightIcon />}
        {count} configs managed by packages (not copied)
      </button>
      {isExpanded && (
        <ul style={{ listStyle: "none", padding: 0, margin: 0 }}>
          {items.map((item) => {
            const id = getItemId(item);
            const path = item.type === "config"
              ? item.data.entry.path
              : "";
            return (
              <li
                key={path}
                data-testid={`decision-item-${id}`}
                tabIndex={-1}
                style={{
                  padding: "var(--pf-t--global--spacer--xs) var(--pf-t--global--spacer--md)",
                  color: "var(--pf-t--global--text--color--subtle)",
                  fontSize: "var(--pf-t--global--font--size--body--sm)",
                }}
              >
                {path}
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}

/** Attention reasons that indicate a config is package-managed (Tier 1). */
const CONFIG_MANAGED_REASONS = new Set(["config_default", "config_baseline_match"]);

export interface DecisionListProps {
  items: DecisionItemKind[];
  sectionLabel: string;
  /** Active filter text — when non-empty, groups with matching items are force-expanded. */
  filterText?: string;
  /** Repo group metadata from ViewResponse, used for informational tier sub-grouping. */
  repoGroups?: RepoGroupInfo[];
  /** When set, auto-expands any collapsed summary containing this item ID. */
  revealItemId?: string;
  /** Controls whether Tier 1 summaries start expanded ("full") or collapsed ("decisions"). */
  viewMode?: ViewMode;
  onViewUpdate: (view: ViewResponse) => void;
  onMutationError: (err: Error) => void;
  /** Called (debounced) after a viewed POST succeeds, so App can refresh its viewed count. */
  onViewedChange?: () => void;
}

export function DecisionList({
  items,
  sectionLabel,
  filterText = "",
  repoGroups = [],
  revealItemId,
  viewMode = "decisions",
  onViewUpdate,
  onMutationError,
  onViewedChange,
}: DecisionListProps) {
  const toastIdRef = useRef(0);
  const [toasts, setToasts] = useState<ToastEntry[]>([]);

  const handleSuccess = useCallback(
    (view: ViewResponse) => {
      onViewUpdate(view);
    },
    [onViewUpdate],
  );

  const handleError = useCallback(
    (err: Error) => {
      // Auto re-fetch on 409 "stale generation" instead of showing an error
      if (err instanceof ApiError && err.status === 409 && err.message.includes("stale generation")) {
        fetchView()
          .then((view) => onViewUpdate(view))
          .catch((refetchErr: unknown) => {
            onMutationError(refetchErr instanceof Error ? refetchErr : new Error(String(refetchErr)));
          });
        return;
      }

      const id = ++toastIdRef.current;
      const isNetwork =
        err.message.includes("fetch") ||
        err.message.includes("network") ||
        err.message.includes("Failed to fetch");
      const variant = isNetwork ? AlertVariant.danger : AlertVariant.warning;
      const message = isNetwork
        ? `Network error: ${err.message}`
        : `Error: ${err.message}`;
      setToasts((prev) => [...prev, { id, message, variant }]);

      // Auto-dismiss non-network errors after 3 seconds
      if (!isNetwork) {
        setTimeout(() => {
          setToasts((prev) => prev.filter((t) => t.id !== id));
        }, 3000);
      }

      // Optimistic revert: re-fetch server state to restore UI
      fetchView()
        .then((view) => onViewUpdate(view))
        .catch(() => {
          // If re-fetch also fails, the error toast is already visible
        });

      onMutationError(err);
    },
    [onMutationError, onViewUpdate],
  );

  const mutation = useMutation(handleSuccess, handleError);
  const { viewedIds, markAsViewed } = useViewed(onViewedChange);

  const dismissToast = useCallback((id: number) => {
    setToasts((prev) => prev.filter((t) => t.id !== id));
  }, []);

  const handleToggle = useCallback(
    (op: RefinementOp) => {
      mutation.mutate(op);
    },
    [mutation],
  );

  const handleRepoToggle = useCallback(
    (sectionId: string, enabled: boolean) => {
      const op: RefinementOp = enabled
        ? { op: "IncludeRepo", target: { section_id: sectionId } }
        : { op: "ExcludeRepo", target: { section_id: sectionId } };
      mutation.mutate(op);
    },
    [mutation],
  );

  // Build a lookup map for repo group metadata
  const repoGroupMap = useMemo(() => {
    const map = new Map<string, RepoGroupInfo>();
    for (const rg of repoGroups) {
      map.set(rg.section_id, rg);
    }
    return map;
  }, [repoGroups]);

  const grouped = groupByAttention(items);
  const levels: AttentionLevel[] = [
    "needs_review",
    "informational",
    "routine",
  ];

  // Build flat ordered list of item IDs for roving tabindex.
  // Exclude items inside collapsed Tier 1 summaries so j/k skips them.
  const summariesCollapsed = viewMode !== "full" && !filterText.trim();
  const flatItemIds = useMemo(() => {
    const ids: string[] = [];
    for (const level of levels) {
      const groupItems = grouped[level];
      if (level === "routine" && summariesCollapsed) {
        // Only include "other routine" items — skip baseline and config-managed
        // items that are hidden inside collapsed summaries.
        for (const item of groupItems) {
          const reason = item.data.attention.length > 0
            ? item.data.attention[0].reason
            : "";
          const isBaseline = reason === "package_baseline_match";
          const isConfigManaged = CONFIG_MANAGED_REASONS.has(
            typeof reason === "string" ? reason : "",
          );
          if (!isBaseline && !isConfigManaged) {
            ids.push(getItemId(item));
          }
        }
      } else {
        for (const item of groupItems) {
          ids.push(getItemId(item));
        }
      }
    }
    return ids;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [items, summariesCollapsed]);

  const [focusedIndex, setFocusedIndex] = useState(0);

  // Reset focused index when items change
  useEffect(() => {
    setFocusedIndex(0);
  }, [flatItemIds]);

  const handleRowKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      const total = flatItemIds.length;
      if (total === 0) return;

      let nextIndex: number | null = null;

      if (e.key === "ArrowDown" || e.key === "j") {
        e.preventDefault();
        nextIndex = (focusedIndex + 1) % total;
      } else if (e.key === "ArrowUp" || e.key === "k") {
        e.preventDefault();
        nextIndex = (focusedIndex - 1 + total) % total;
      } else if (e.key === "g") {
        e.preventDefault();
        nextIndex = 0;
      } else if (e.key === "G") {
        e.preventDefault();
        nextIndex = total - 1;
      }

      if (nextIndex !== null) {
        setFocusedIndex(nextIndex);
        const targetId = flatItemIds[nextIndex];
        const el = document.querySelector(
          `[data-testid="decision-item-${targetId}"]`,
        ) as HTMLElement | null;
        el?.focus();
      }
    },
    [flatItemIds, focusedIndex],
  );

  // Clean up auto-dismiss timers
  const timerIds = useRef<number[]>([]);
  useEffect(() => {
    return () => {
      timerIds.current.forEach(clearTimeout);
    };
  }, []);

  return (
    <div
      aria-label={`${sectionLabel} decisions`}
      data-testid={`decision-list-${sectionLabel.toLowerCase().replace(/\s+/g, "-")}`}
    >
      {toasts.length > 0 && (
        <AlertGroup isToast isLiveRegion>
          {toasts.map((toast) => (
            <Alert
              key={toast.id}
              variant={toast.variant}
              title={toast.message}
              actionClose={
                <AlertActionCloseButton
                  onClose={() => dismissToast(toast.id)}
                />
              }
            />
          ))}
        </AlertGroup>
      )}

      {(() => {
        let runningRowIndex = 0;
        return levels.map((level) => {
          const groupItems = grouped[level];
          if (groupItems.length === 0) return null;
          // Force-expand groups when a filter is active and this group has matching items
          const forceExpanded = filterText.trim().length > 0 && groupItems.length > 0;

          // Tier 1: routine items with baseline/managed reasons get collapsed summaries
          if (level === "routine") {
            const baselineItems = groupItems.filter(
              (item) => item.data.attention.length > 0 &&
                item.data.attention[0].reason === "package_baseline_match",
            );
            const configManagedItems = groupItems.filter(
              (item) => item.data.attention.length > 0 &&
                CONFIG_MANAGED_REASONS.has(
                  typeof item.data.attention[0].reason === "string"
                    ? item.data.attention[0].reason
                    : "",
                ),
            );
            const otherRoutine = groupItems.filter(
              (item) => item.data.attention.length === 0 ||
                (item.data.attention[0].reason !== "package_baseline_match" &&
                  !CONFIG_MANAGED_REASONS.has(
                    typeof item.data.attention[0].reason === "string"
                      ? item.data.attention[0].reason
                      : "",
                  )),
            );

            return (
              <AttentionGroup key={level} level={level} count={groupItems.length} forceExpanded={forceExpanded}>
                {baselineItems.length > 0 && (
                  <BaselineSummary count={baselineItems.length} items={baselineItems} revealItemId={revealItemId} defaultExpanded={viewMode === "full"} filterActive={forceExpanded} />
                )}
                {configManagedItems.length > 0 && (
                  <ConfigManagedSummary count={configManagedItems.length} items={configManagedItems} revealItemId={revealItemId} defaultExpanded={viewMode === "full"} filterActive={forceExpanded} />
                )}
                {otherRoutine.map((item) => {
                  runningRowIndex++;
                  const id = getItemId(item);
                  const flatIdx = flatItemIds.indexOf(id);
                  return (
                    <DecisionItem
                      key={id}
                      item={item}
                      level={level}
                      rowIndex={runningRowIndex}
                      isViewed={viewedIds.has(id)}
                      isPending={mutation.isPending}
                      tabIndex={flatIdx === focusedIndex ? 0 : -1}
                      onToggleInclude={handleToggle}
                      onMarkViewed={markAsViewed}
                      onKeyDown={handleRowKeyDown}
                    />
                  );
                })}
              </AttentionGroup>
            );
          }

          // Informational tier: sub-group by source_repo when repo_groups available
          if (level === "informational" && repoGroups.length > 0) {
            // Group items by source_repo
            const byRepo = new Map<string, DecisionItemKind[]>();
            for (const item of groupItems) {
              const repo = item.type === "package" ? item.data.entry.source_repo.toLowerCase() : "__other__";
              const list = byRepo.get(repo) ?? [];
              list.push(item);
              byRepo.set(repo, list);
            }
            // Order repos: distro first, then verified third-party, then unverified/unknown
            const repoOrder = [...byRepo.keys()].sort((a, b) => {
              const rgA = repoGroupMap.get(a);
              const rgB = repoGroupMap.get(b);
              const rankA = rgA?.is_distro ? 0 : rgA?.provenance === "verified" ? 1 : 2;
              const rankB = rgB?.is_distro ? 0 : rgB?.provenance === "verified" ? 1 : 2;
              return rankA - rankB;
            });

            return (
              <AttentionGroup key={level} level={level} count={groupItems.length} forceExpanded={forceExpanded}>
                {repoOrder.map((repo) => {
                  const repoItems = byRepo.get(repo) ?? [];
                  const rg = repoGroupMap.get(repo);
                  return (
                    <div key={repo}>
                      {rg && (
                        <RepoGroupHeader
                          sectionId={rg.section_id}
                          provenance={rg.provenance}
                          isDistro={rg.is_distro}
                          packageCount={repoItems.length}
                          enabled={rg.enabled}
                          onToggle={handleRepoToggle}
                        />
                      )}
                      {repoItems.map((item) => {
                        runningRowIndex++;
                        const id = getItemId(item);
                        const flatIdx = flatItemIds.indexOf(id);
                        return (
                          <DecisionItem
                            key={id}
                            item={item}
                            level={level}
                            rowIndex={runningRowIndex}
                            isViewed={viewedIds.has(id)}
                            isPending={mutation.isPending}
                            tabIndex={flatIdx === focusedIndex ? 0 : -1}
                            onToggleInclude={handleToggle}
                            onMarkViewed={markAsViewed}
                            onKeyDown={handleRowKeyDown}
                          />
                        );
                      })}
                    </div>
                  );
                })}
              </AttentionGroup>
            );
          }

          return (
            <AttentionGroup key={level} level={level} count={groupItems.length} forceExpanded={forceExpanded}>
              {groupItems.map((item) => {
                runningRowIndex++;
                const id = getItemId(item);
                const flatIdx = flatItemIds.indexOf(id);
                return (
                  <DecisionItem
                    key={id}
                    item={item}
                    level={level}
                    rowIndex={runningRowIndex}
                    isViewed={viewedIds.has(id)}
                    isPending={mutation.isPending}
                    tabIndex={flatIdx === focusedIndex ? 0 : -1}
                    onToggleInclude={handleToggle}
                    onMarkViewed={markAsViewed}
                    onKeyDown={handleRowKeyDown}
                  />
                );
              })}
            </AttentionGroup>
          );
        });
      })()}

      {items.length === 0 && (
        <EmptyState titleText="No items in this section" headingLevel="h3">
          <EmptyStateBody>
            There are no {sectionLabel.toLowerCase()} to triage.
          </EmptyStateBody>
        </EmptyState>
      )}

      {items.length > 0 &&
        grouped.needs_review.length > 0 &&
        grouped.needs_review.every((item) => viewedIds.has(getItemId(item))) && (
        <div
          style={{
            padding: "var(--pf-t--global--spacer--md)",
            textAlign: "center",
            color: "var(--pf-t--global--text--color--subtle)",
          }}
          data-testid="completion-message"
        >
          All items have been triaged.
        </div>
      )}
    </div>
  );
}
