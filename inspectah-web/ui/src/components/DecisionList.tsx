import { useCallback, useMemo, useState, useRef, useEffect } from "react";
import {
  Alert,
  AlertGroup,
  AlertActionCloseButton,
  AlertVariant,
  EmptyState,
  EmptyStateBody,
} from "@patternfly/react-core";
import type {
  AttentionLevel,
  RefinementOp,
  RefinedView,
} from "../api/types";
import { fetchView } from "../api/client";
import { ApiError } from "../api/types";
import { useMutation } from "../hooks/useMutation";
import { useViewed } from "../hooks/useViewed";
import { AttentionGroup } from "./AttentionGroup";
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

export interface DecisionListProps {
  items: DecisionItemKind[];
  sectionLabel: string;
  /** Active filter text — when non-empty, groups with matching items are force-expanded. */
  filterText?: string;
  onViewUpdate: (view: RefinedView) => void;
  onMutationError: (err: Error) => void;
  /** Called (debounced) after a viewed POST succeeds, so App can refresh its viewed count. */
  onViewedChange?: () => void;
}

export function DecisionList({
  items,
  sectionLabel,
  filterText = "",
  onViewUpdate,
  onMutationError,
  onViewedChange,
}: DecisionListProps) {
  const toastIdRef = useRef(0);
  const [toasts, setToasts] = useState<ToastEntry[]>([]);

  const handleSuccess = useCallback(
    (view: RefinedView) => {
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

  const grouped = groupByAttention(items);
  const levels: AttentionLevel[] = [
    "needs_review",
    "informational",
    "routine",
  ];

  // Build flat ordered list of item IDs for roving tabindex
  const flatItemIds = useMemo(() => {
    const ids: string[] = [];
    for (const level of levels) {
      const groupItems = grouped[level];
      for (const item of groupItems) {
        ids.push(getItemId(item));
      }
    }
    return ids;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [items]);

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
