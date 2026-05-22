import { useState } from "react";
import { Button } from "@patternfly/react-core";
import type { FleetItem, ItemId } from "../../api/types";
import type { UseVariantAckResult } from "../../hooks/useVariantAck";
import type { UseFleetDiffResult } from "../../hooks/useFleetDiff";
import { DiffDrawer } from "./DiffDrawer";

export interface VariantViewProps {
  item: FleetItem;
  ack: UseVariantAckResult;
  onSelectVariant: (itemId: ItemId, hash: string) => void;
  diffHook: UseFleetDiffResult;
}

export function VariantView({
  item,
  ack,
  onSelectVariant,
  diffHook,
}: VariantViewProps) {
  // Guard: return null for items without variants
  if (!item.variants) {
    return null;
  }

  const [showDiff, setShowDiff] = useState(false);
  const variants = item.variants;
  const selectedHash = variants.selected;

  const handleSelect = (hash: string) => {
    if (hash !== selectedHash) {
      onSelectVariant(item.item_id, hash);
      ack.markChanged(item.item_id);
    }
  };

  const handleConfirm = () => {
    ack.confirm(item.item_id);
  };

  const handleCompare = () => {
    // Diff between selected and first non-selected option
    const target = variants.options.find((o) => o.hash !== selectedHash);
    if (target) {
      diffHook.fetchDiff(item.item_id, selectedHash, target.hash);
      setShowDiff(true);
    }
  };

  const handleCloseDiff = () => {
    setShowDiff(false);
    diffHook.clearDiff();
  };

  const handleRetry = () => {
    const target = variants.options.find((o) => o.hash !== selectedHash);
    if (target) {
      diffHook.fetchDiff(item.item_id, selectedHash, target.hash);
    }
  };

  return (
    <div className="variant-view" data-testid="variant-view">
      <div className="variant-view__options" role="radiogroup" aria-label="Variant options">
        {variants.options.map((option) => {
          const hostLabel =
            option.host_count === 1
              ? "1 host"
              : `${option.host_count} hosts`;

          return (
            <label key={option.hash} className="variant-view__option">
              <input
                type="radio"
                name={`variant-${JSON.stringify(item.item_id)}`}
                value={option.hash}
                checked={option.hash === selectedHash}
                onChange={() => handleSelect(option.hash)}
              />
              <span className="variant-view__option-info">
                <span className="variant-view__option-hash">
                  {option.hash.substring(0, 8)}
                </span>
                <span className="variant-view__option-hosts">{hostLabel}</span>
                <span className="variant-view__option-hostnames">
                  {option.hosts.join(", ")}
                </span>
              </span>
              {option.hash === selectedHash && (
                <span className="variant-view__selected-indicator">selected</span>
              )}
            </label>
          );
        })}
      </div>

      <div className="variant-view__actions">
        <Button
          variant="secondary"
          onClick={handleCompare}
          isDisabled={variants.options.length < 2}
        >
          Compare
        </Button>
        <Button variant="primary" onClick={handleConfirm}>
          Confirm
        </Button>
      </div>

      {showDiff && (
        <DiffDrawer
          diff={diffHook.diff}
          isLoading={diffHook.isLoading}
          error={diffHook.error}
          onRetry={handleRetry}
          onClose={handleCloseDiff}
        />
      )}
    </div>
  );
}
