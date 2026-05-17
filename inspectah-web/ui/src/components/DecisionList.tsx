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
import { RepoGroup } from "./RepoGroup";
import { RoutineSummary } from "./RoutineSummary";
import { DecisionItem, itemId as getItemId } from "./DecisionItem";
import type { DecisionItemKind } from "./DecisionItem";
import { highestAttention } from "./attentionUtils";

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
function BaselineSummary({ count, items, revealItemId, filterActive = false }: { count: number; items: DecisionItemKind[]; revealItemId?: string; filterActive?: boolean }) {
  const [isExpanded, setIsExpanded] = useState(false);

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
function ConfigManagedSummary({ count, items, revealItemId, filterActive = false }: { count: number; items: DecisionItemKind[]; revealItemId?: string; filterActive?: boolean }) {
  const [isExpanded, setIsExpanded] = useState(false);

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

/** Priority ordering for attention levels within a repo group. */
const ATTENTION_PRIORITY: Record<string, number> = {
  needs_review: 0,
  informational: 1,
  routine: 2,
};

interface RepoPartition {
  sectionId: string;
  repo: RepoGroupInfo | undefined;
  items: DecisionItemKind[];
  needsReview: DecisionItemKind[];
  informational: DecisionItemKind[];
  routine: DecisionItemKind[];
  hasNeedsReview: boolean;
  isUnknown: boolean;
}

export interface DecisionListProps {
  items: DecisionItemKind[];
  sectionLabel: string;
  /** Active filter text — when non-empty, groups with matching items are force-expanded. */
  filterText?: string;
  /** Repo group metadata from ViewResponse, used for informational tier sub-grouping. */
  repoGroups?: RepoGroupInfo[];
  /** When set, auto-expands any collapsed summary containing this item ID. */
  revealItemId?: string;
  onViewUpdate: (view: ViewResponse) => void;
  onMutationError: (err: Error) => void;
  /** Called (debounced) after a viewed POST succeeds, so App can refresh its viewed count. */
  onViewedChange?: () => void;
}

const EMPTY_REPO_GROUPS: RepoGroupInfo[] = [];

export function DecisionList({
  items,
  sectionLabel,
  filterText = "",
  repoGroups = EMPTY_REPO_GROUPS,
  revealItemId,
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

  // Build repo partitions when repo-first grouping is active
  const repoPartitions = useMemo((): RepoPartition[] => {
    if (repoGroups.length === 0) return [];

    // Group items by source_repo
    const byRepo = new Map<string, DecisionItemKind[]>();
    for (const item of items) {
      const rawRepo = item.type === "package" ? item.data.entry.source_repo : "";
      const repoKey = rawRepo && repoGroupMap.has(rawRepo.toLowerCase())
        ? rawRepo.toLowerCase()
        : "__unknown__";
      const list = byRepo.get(repoKey) ?? [];
      list.push(item);
      byRepo.set(repoKey, list);
    }

    // Build partition objects
    const partitions: RepoPartition[] = [];
    for (const [key, repoItems] of byRepo.entries()) {
      const rg = key === "__unknown__" ? undefined : repoGroupMap.get(key);
      const needsReview: DecisionItemKind[] = [];
      const informational: DecisionItemKind[] = [];
      const routine: DecisionItemKind[] = [];

      for (const item of repoItems) {
        const level = item.data.attention.length > 0
          ? highestAttention(item.data.attention)
          : "routine";
        if (level === "needs_review") needsReview.push(item);
        else if (level === "informational") informational.push(item);
        else routine.push(item);
      }

      partitions.push({
        sectionId: key,
        repo: rg,
        items: repoItems,
        needsReview,
        informational,
        routine,
        hasNeedsReview: needsReview.length > 0,
        isUnknown: key === "__unknown__",
      });
    }

    // Sort: distro alpha → enabled third-party alpha → disabled → unknown last
    partitions.sort((a, b) => {
      const rankA = a.isUnknown ? 4 : a.repo?.is_distro ? 0 : !a.repo?.enabled ? 3 : 1;
      const rankB = b.isUnknown ? 4 : b.repo?.is_distro ? 0 : !b.repo?.enabled ? 3 : 1;
      if (rankA !== rankB) return rankA - rankB;
      return a.sectionId.localeCompare(b.sectionId);
    });

    return partitions;
  }, [items, repoGroups, repoGroupMap]);

  // Build flat ordered list of item IDs for roving tabindex.
  // Exclude items inside collapsed Tier 1 summaries so j/k skips them.
  const summariesCollapsed = !filterText.trim();
  const flatItemIds = useMemo(() => {
    // Repo-first path: include repo headers + visible items
    if (repoGroups.length > 0) {
      const ids: string[] = [];
      for (const part of repoPartitions) {
        ids.push(`repo-header:${part.sectionId}`);
        if (part.isUnknown) {
          // Unknown group: all items always shown
          for (const item of part.items) {
            ids.push(getItemId(item));
          }
        } else {
          // needs_review + informational always in sequence
          for (const item of part.needsReview) ids.push(getItemId(item));
          for (const item of part.informational) ids.push(getItemId(item));
          // Routine items excluded from flat sequence unless filter matches them
          if (filterText.trim()) {
            for (const item of part.routine) ids.push(getItemId(item));
          }
        }
      }
      return ids;
    }

    // Attention-first path (configs): existing logic
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
  }, [items, summariesCollapsed, repoGroups, repoPartitions, filterText]);

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
        let el: HTMLElement | null;
        if (targetId.startsWith("repo-header:")) {
          const sectionId = targetId.slice("repo-header:".length);
          el = document.querySelector(
            `[data-testid="repo-group-${sectionId}"]`,
          ) as HTMLElement | null;
        } else {
          el = document.querySelector(
            `[data-testid="decision-item-${targetId}"]`,
          ) as HTMLElement | null;
        }
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

      {/* Repo-first grouping when repoGroups are provided (packages section) */}
      {repoGroups.length > 0 && (() => {
        let runningRowIndex = 0;
        const filterActive = filterText.trim().length > 0;

        return repoPartitions.map((part) => {
          // For unknown group: always expanded, show all items individually
          if (part.isUnknown) {
            const unknownRepo: RepoGroupInfo = {
              section_id: "__unknown__",
              provenance: "unknown",
              is_distro: false,
              package_count: part.items.length,
              enabled: true,
            };
            // Sort items within unknown group by attention priority
            const sortedItems = [...part.items].sort((a, b) => {
              const aLevel = a.data.attention.length > 0 ? highestAttention(a.data.attention) : "routine";
              const bLevel = b.data.attention.length > 0 ? highestAttention(b.data.attention) : "routine";
              return (ATTENTION_PRIORITY[aLevel] ?? 2) - (ATTENTION_PRIORITY[bLevel] ?? 2);
            });

            const unknownHeaderIdx = flatItemIds.indexOf(`repo-header:${part.sectionId}`);
            return (
              <RepoGroup
                key="__unknown__"
                repo={unknownRepo}
                defaultExpanded={true}
                forceExpanded={filterActive}
                revealItemId={revealItemId}
                itemIds={part.items.map(getItemId)}
                tabIndex={unknownHeaderIdx === focusedIndex ? 0 : -1}
                onRepoToggle={handleRepoToggle}
                onKeyDown={handleRowKeyDown}
              >
                {sortedItems.map((item) => {
                  runningRowIndex++;
                  const id = getItemId(item);
                  const level = item.data.attention.length > 0
                    ? highestAttention(item.data.attention)
                    : "routine";
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
              </RepoGroup>
            );
          }

          const repo = part.repo!;
          const isDisabled = !repo.enabled;
          const hasNeedsReview = part.needsReview.length > 0;
          const hasInfo = part.informational.length > 0;
          const allRoutine = !hasNeedsReview && !hasInfo;

          // For disabled repos, count visible include:false rows instead of backend package_count
          const headerPackageCount = isDisabled
            ? part.items.filter((item) => item.type === "package" && !item.data.entry.include).length
            : repo.package_count;
          const effectiveRg = isDisabled
            ? { ...repo, package_count: headerPackageCount }
            : repo;

          // Match-scoped filter expansion: only expand this group if it contains matching items
          const filterQ = filterText.trim().toLowerCase();
          const groupHasMatch = filterQ.length > 0 && part.items.some((item) => {
            if (item.type !== "package") return false;
            const e = item.data.entry;
            return `${e.name} ${e.arch} ${e.version} ${e.source_repo}`.toLowerCase().includes(filterQ);
          });
          // Match-scoped routine expansion: only expand routine summary if it contains matching packages
          const routineHasMatch = filterQ.length > 0 && part.routine.some((item) => {
            if (item.type !== "package") return false;
            const e = item.data.entry;
            return `${e.name} ${e.arch} ${e.version} ${e.source_repo}`.toLowerCase().includes(filterQ);
          });

          // Determine display props for header
          const infoCount = !hasNeedsReview && hasInfo ? part.informational.length : undefined;
          const summaryText = allRoutine && !isDisabled ? "No action needed" : undefined;

          // Disabled repos always start collapsed
          const defaultExpanded = isDisabled ? false : hasNeedsReview || hasInfo;

          const headerIdx = flatItemIds.indexOf(`repo-header:${part.sectionId}`);
          return (
            <RepoGroup
              key={part.sectionId}
              repo={effectiveRg}
              defaultExpanded={defaultExpanded}
              forceExpanded={groupHasMatch}
              infoCount={infoCount}
              summaryText={summaryText}
              revealItemId={revealItemId}
              itemIds={part.items.map(getItemId)}
              tabIndex={headerIdx === focusedIndex ? 0 : -1}
              onRepoToggle={handleRepoToggle}
              onKeyDown={handleRowKeyDown}
            >
              {/* needs_review items: rendered as full DecisionItem rows */}
              {part.needsReview.map((item) => {
                runningRowIndex++;
                const id = getItemId(item);
                const flatIdx = flatItemIds.indexOf(id);
                return (
                  <DecisionItem
                    key={id}
                    item={item}
                    level="needs_review"
                    rowIndex={runningRowIndex}
                    isViewed={viewedIds.has(id)}
                    isPending={mutation.isPending}
                    tabIndex={flatIdx === focusedIndex ? 0 : -1}
                    onToggleInclude={isDisabled ? undefined : handleToggle}
                    onMarkViewed={markAsViewed}
                    onKeyDown={handleRowKeyDown}
                  />
                );
              })}
              {/* informational items: rendered as full DecisionItem rows */}
              {part.informational.map((item) => {
                runningRowIndex++;
                const id = getItemId(item);
                const flatIdx = flatItemIds.indexOf(id);
                return (
                  <DecisionItem
                    key={id}
                    item={item}
                    level="informational"
                    rowIndex={runningRowIndex}
                    isViewed={viewedIds.has(id)}
                    isPending={mutation.isPending}
                    tabIndex={flatIdx === focusedIndex ? 0 : -1}
                    onToggleInclude={isDisabled ? undefined : handleToggle}
                    onMarkViewed={markAsViewed}
                    onKeyDown={handleRowKeyDown}
                  />
                );
              })}
              {/* routine items: collapsed as RoutineSummary */}
              {part.routine.length > 0 && (
                <RoutineSummary
                  items={part.routine}
                  forceExpanded={routineHasMatch}
                  revealItemId={revealItemId}
                  onToggleInclude={isDisabled ? undefined : handleToggle}
                  onMarkViewed={markAsViewed}
                  viewedIds={viewedIds}
                  isPending={mutation.isPending}
                  onKeyDown={handleRowKeyDown}
                  startRowIndex={runningRowIndex + 1}
                  flatItemIds={flatItemIds}
                  focusedIndex={focusedIndex}
                />
              )}
            </RepoGroup>
          );
        });
      })()}

      {/* Attention-first grouping when no repoGroups (configs section) */}
      {repoGroups.length === 0 && (() => {
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
                  <BaselineSummary count={baselineItems.length} items={baselineItems} revealItemId={revealItemId} filterActive={forceExpanded} />
                )}
                {configManagedItems.length > 0 && (
                  <ConfigManagedSummary count={configManagedItems.length} items={configManagedItems} revealItemId={revealItemId} filterActive={forceExpanded} />
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
