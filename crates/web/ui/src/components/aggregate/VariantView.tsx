import { useState, useRef } from "react";
import { Button } from "@patternfly/react-core";
import type { AggregateItem, ItemId } from "../../api/types";
import type { UseVariantAckResult } from "../../hooks/useVariantAck";
import type { UseAggregateDiffResult } from "../../hooks/useAggregateDiff";
import { DiffDrawer } from "./DiffDrawer";

export interface VariantViewProps {
  item: AggregateItem;
  ack: UseVariantAckResult;
  onSelectVariant: (itemId: ItemId, hash: string) => void;
  diffHook: UseAggregateDiffResult;
}

export function VariantView({
  item,
  ack,
  onSelectVariant,
  diffHook,
}: VariantViewProps) {
  const [showDiff, setShowDiff] = useState(false);
  const [diffTargetHash, setDiffTargetHash] = useState<string | null>(null);
  const viewRef = useRef<HTMLDivElement>(null);

  // Guard: return null for items without variants (after hooks)
  if (!item.variants) {
    return null;
  }

  const variants = item.variants;
  const selectedHash = variants.selected;

  const handleSelect = (hash: string) => {
    if (hash !== selectedHash) {
      onSelectVariant(item.item_id, hash);
    }
  };

  const isReviewed = ack.isAcked(item.item_id);

  const handleConfirm = () => {
    if (isReviewed) {
      ack.unconfirm(item.item_id);
    } else {
      ack.confirm(item.item_id);
    }
  };

  const handleDiffVsSelected = (targetHash: string) => {
    setDiffTargetHash(targetHash);
    diffHook.fetchDiff(item.item_id, selectedHash, targetHash);
    setShowDiff(true);
  };

  const handleCloseDiff = () => {
    setShowDiff(false);
    setDiffTargetHash(null);
    diffHook.clearDiff();
  };

  const handleRetry = () => {
    if (diffTargetHash) {
      diffHook.fetchDiff(item.item_id, selectedHash, diffTargetHash);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    // Don't handle keys when focus is in a text input
    const tag = (e.target as HTMLElement).tagName?.toLowerCase();
    if (tag === "input" || tag === "textarea" || tag === "select") return;

    if (e.key === "Escape" && showDiff) {
      e.preventDefault();
      handleCloseDiff();
      return;
    }
  };

  // Build operand descriptions for the diff drawer header
  const selectedOption = variants.options.find((o) => o.hash === selectedHash);
  const targetOption = diffTargetHash
    ? variants.options.find((o) => o.hash === diffTargetHash)
    : null;

  return (
    <div
      ref={viewRef}
      className="variant-view"
      data-testid="variant-view"
      onKeyDown={handleKeyDown}
      tabIndex={-1}
    >
      <div
        className="variant-view__options"
        role="radiogroup"
        aria-label="Variant options"
      >
        {variants.options.map((option) => {
          const hostLabel =
            option.host_count === 1 ? "1 host" : `${option.host_count} hosts`;
          const isSelected = option.hash === selectedHash;

          return (
            <label key={option.hash} className="variant-view__option">
              <input
                type="radio"
                name={`variant-${JSON.stringify(item.item_id)}`}
                value={option.hash}
                checked={isSelected}
                onChange={() => handleSelect(option.hash)}
              />
              <span className="variant-view__option-info">
                <span className="variant-view__option-hash">
                  {option.hash.substring(0, 8)}
                </span>
                <span className="variant-view__option-hosts">{hostLabel}:</span>
                <span className="variant-view__option-hostnames">
                  {option.hosts.join(", ")}
                </span>
              </span>
              {isSelected && (
                <span
                  className="variant-view__selected-indicator"
                  data-testid="variant-selected-indicator"
                >
                  Selected
                </span>
              )}
              {!isSelected && variants.options.length >= 2 && (
                <Button
                  variant="link"
                  isInline
                  className="variant-view__diff-link"
                  onClick={(e) => {
                    e.preventDefault();
                    handleDiffVsSelected(option.hash);
                  }}
                >
                  Diff vs selected
                </Button>
              )}
            </label>
          );
        })}
      </div>

      <div className="variant-view__actions">
        {isReviewed ? (
          <button
            type="button"
            className="variant-view__reviewed-indicator"
            onClick={handleConfirm}
            data-testid="variant-reviewed-indicator"
            aria-label="Undo review"
          >
            Reviewed
          </button>
        ) : (
          <Button variant="primary" onClick={handleConfirm}>
            Mark as reviewed
          </Button>
        )}
      </div>

      {showDiff && (
        <DiffDrawer
          diff={diffHook.diff}
          isLoading={diffHook.isLoading}
          error={diffHook.error}
          onRetry={handleRetry}
          onClose={handleCloseDiff}
          targetLabel={
            targetOption
              ? `${targetOption.hash.substring(0, 8)} (${targetOption.hosts.join(", ")})`
              : undefined
          }
          baseLabel={
            selectedOption
              ? `${selectedOption.hash.substring(0, 8)} (${selectedOption.hosts.join(", ")}) [selected]`
              : undefined
          }
        />
      )}
    </div>
  );
}
