import { useState, useEffect } from "react";
import { AngleRightIcon, AngleDownIcon } from "@patternfly/react-icons";
import type { RefinementOp } from "../api/types";
import { DecisionItem, itemId as getItemId } from "./DecisionItem";
import type { DecisionItemKind } from "./DecisionItem";
import { highestAttention } from "./attentionUtils";

export interface RoutineSummaryProps {
  items: DecisionItemKind[];
  /** Override: force-expand when search filter matches */
  forceExpanded?: boolean;
  /** When set, auto-expands if this item ID is in the list */
  revealItemId?: string;
  /** Callback for include/exclude toggle on expanded items */
  onToggleInclude: (op: RefinementOp) => void;
  /** Callback for marking items as viewed */
  onMarkViewed: (id: string) => void;
  /** Set of already-viewed item IDs */
  viewedIds: Set<string>;
  /** Whether a mutation is in flight */
  isPending: boolean;
  /** Callback for roving tabindex key handling */
  onKeyDown?: (e: React.KeyboardEvent) => void;
  /** Starting row index for tabIndex computation */
  startRowIndex?: number;
  /** Flat item IDs array for roving tabindex */
  flatItemIds?: string[];
  /** Current focused index in the flat roving sequence */
  focusedIndex?: number;
}

export function RoutineSummary({
  items,
  forceExpanded = false,
  revealItemId,
  onToggleInclude,
  onMarkViewed,
  viewedIds,
  isPending,
  onKeyDown,
  startRowIndex = 0,
  flatItemIds = [],
  focusedIndex = -1,
}: RoutineSummaryProps) {
  const [isExpanded, setIsExpanded] = useState(false);

  // Auto-expand when revealItemId matches an item
  useEffect(() => {
    if (!revealItemId) return;
    const match = items.some((item) => getItemId(item) === revealItemId);
    if (match && !isExpanded) {
      setIsExpanded(true);
    }
  }, [revealItemId, items, isExpanded]);

  // Auto-expand when filter is active
  useEffect(() => {
    if (forceExpanded && !isExpanded) {
      setIsExpanded(true);
    }
  }, [forceExpanded]); // eslint-disable-line react-hooks/exhaustive-deps

  const effectiveExpanded = forceExpanded || isExpanded;

  return (
    <div data-testid="routine-summary" style={{ marginBottom: "var(--pf-t--global--spacer--sm)" }}>
      <button
        type="button"
        onClick={() => setIsExpanded((prev) => !prev)}
        aria-expanded={effectiveExpanded}
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
        {effectiveExpanded ? <AngleDownIcon /> : <AngleRightIcon />}
        + {items.length} routine
      </button>
      {effectiveExpanded &&
        items.map((item, idx) => {
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
              rowIndex={startRowIndex + idx}
              isViewed={viewedIds.has(id)}
              isPending={isPending}
              tabIndex={flatIdx === focusedIndex ? 0 : -1}
              onToggleInclude={onToggleInclude}
              onMarkViewed={onMarkViewed}
              onKeyDown={onKeyDown}
            />
          );
        })}
    </div>
  );
}
