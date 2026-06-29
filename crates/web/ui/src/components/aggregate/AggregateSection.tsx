import { useState, useEffect } from "react";
import type { AggregateSection, AggregateItem, ItemId } from "../../api/types";
import type { UseVariantAckResult } from "../../hooks/useVariantAck";
import type { UseAggregateDiffResult } from "../../hooks/useAggregateDiff";
import { ZoneGroup } from "./ZoneGroup";
import { AggregateItemRow, itemDisplayName } from "./AggregateItemRow";
import { VariantView } from "./VariantView";
import { ItemDetailPane } from "./ItemDetailPane";

export interface NavTarget {
  sectionId: string;
  itemId: ItemId;
}

export interface AggregateSectionContentProps {
  section: AggregateSection | undefined;
  filterText: string;
  isDecisionSection: boolean;
  onToggle: (itemId: ItemId, include: boolean) => void;
  ack: UseVariantAckResult;
  onExpandVariant?: (itemId: ItemId) => void;
  /** Force-open variant view (idempotent, used by portal navigation). */
  onForceExpandVariant?: (itemId: ItemId) => void;
  pendingNavTarget?: NavTarget | null;
  onNavTargetConsumed?: () => void;
  /** Currently expanded item for inline variant view. */
  expandedItemId?: ItemId | null;
  /** Callback when a variant option is selected. */
  onSelectVariant?: (itemId: ItemId, hash: string) => void;
  /** Diff hook for variant comparison. */
  diffHook?: UseAggregateDiffResult;
}

function filterItems(
  items: AggregateItem[],
  filterText: string,
): AggregateItem[] {
  if (!filterText) return items;
  const lower = filterText.toLowerCase();
  return items.filter((item) =>
    itemDisplayName(item.item_id).toLowerCase().includes(lower),
  );
}

function itemIdKey(id: ItemId): string {
  return JSON.stringify(id);
}

function findItemZone(
  section: AggregateSection,
  targetId: string,
): "consensus" | "near_consensus" | "divergent" | null {
  if (!section.zones) return null;
  for (const item of section.zones.consensus.items) {
    if (itemIdKey(item.item_id) === targetId) return "consensus";
  }
  for (const item of section.zones.near_consensus.items) {
    if (itemIdKey(item.item_id) === targetId) return "near_consensus";
  }
  for (const item of section.zones.divergent.items) {
    if (itemIdKey(item.item_id) === targetId) return "divergent";
  }
  return null;
}

export function AggregateSectionContent({
  section,
  filterText,
  isDecisionSection,
  onToggle,
  ack,
  onExpandVariant,
  onForceExpandVariant,
  pendingNavTarget,
  onNavTargetConsumed,
  expandedItemId,
  onSelectVariant,
  diffHook,
}: AggregateSectionContentProps) {
  const [forceExpandZone, setForceExpandZone] = useState<string | null>(null);
  const [revealCounter, setRevealCounter] = useState(0);

  // When a nav target arrives for this section, determine which zone
  // contains it and force that zone open. Also expand the item's
  // variant view if it has variants.
  useEffect(() => {
    if (!pendingNavTarget || !section) return;
    if (pendingNavTarget.sectionId !== section.id) return;

    const targetKey = itemIdKey(pendingNavTarget.itemId);

    // Force-expand the zone containing the target item
    const zone = findItemZone(section, targetKey);
    if (zone) {
      setForceExpandZone(zone);
    }

    // Auto-expand the detail/variant view for the target item (force-open, not toggle).
    // Only for decision section items — context items don't get editable variants.
    const expandFn = onForceExpandVariant ?? onExpandVariant;
    if (expandFn && section.is_decision_section) {
      expandFn(pendingNavTarget.itemId);
    }

    setRevealCounter((c) => c + 1);
  }, [pendingNavTarget, section, onExpandVariant]);

  // After forcing zone open and React re-renders, scroll + highlight + focus
  useEffect(() => {
    if (!pendingNavTarget || revealCounter === 0) return;

    const targetKey = itemIdKey(pendingNavTarget.itemId);

    requestAnimationFrame(() => {
      const selector = `[data-item-id='${targetKey.replace(/'/g, "\\'")}']`;
      const el = document.querySelector(selector) as HTMLElement | null;
      if (el) {
        el.scrollIntoView({ behavior: "smooth", block: "center" });
        el.classList.add("inspectah-highlight");
        el.focus();
        setTimeout(() => el.classList.remove("inspectah-highlight"), 1500);
      }
      setForceExpandZone(null);
      onNavTargetConsumed?.();
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [revealCounter]);

  if (!section) return null;

  const rowProps = {
    isDecisionSection,
    onToggle,
    ack,
    onExpandVariant,
    sectionId: section.id,
  };

  const isItemExpanded = (item: AggregateItem) =>
    expandedItemId != null &&
    JSON.stringify(item.item_id) === JSON.stringify(expandedItemId);

  // Flat mode: section has items directly (aggregate-of-2 or no zones)
  if (!section.zones) {
    const filtered = filterItems(section.items ?? [], filterText);
    return (
      <div className="aggregate-section" data-testid="aggregate-section">
        {filtered.map((item) => {
          const key = itemIdKey(item.item_id);
          const expanded = isItemExpanded(item);
          const hasVariants = item.variants != null && item.variants.count > 1;
          return (
            <div key={key}>
              <AggregateItemRow
                item={item}
                {...rowProps}
                isExpanded={expanded}
              />
              {expanded &&
                hasVariants &&
                isDecisionSection &&
                onSelectVariant &&
                diffHook && (
                  <VariantView
                    item={item}
                    ack={ack}
                    onSelectVariant={onSelectVariant}
                    diffHook={diffHook}
                    sectionId={section.id}
                  />
                )}
              {expanded && !hasVariants && isDecisionSection && (
                <ItemDetailPane item={item} sectionId={section.id} />
              )}
            </div>
          );
        })}
      </div>
    );
  }

  // Zone mode: group items by consensus zone
  const zones = section.zones;
  const consensusFiltered = filterItems(zones.consensus.items, filterText);
  const nearConsensusFiltered = filterItems(
    zones.near_consensus.items,
    filterText,
  );
  const divergentFiltered = filterItems(zones.divergent.items, filterText);

  // Count zones with unfiltered items for header suppression
  const populatedZones = [
    zones.consensus.items.length > 0 ? 1 : 0,
    zones.near_consensus.items.length > 0 ? 1 : 0,
    zones.divergent.items.length > 0 ? 1 : 0,
  ].reduce((a, b) => a + b, 0);

  const suppressHeaders = populatedZones <= 1;

  const renderItems = (items: AggregateItem[]) =>
    items.map((item) => {
      const key = itemIdKey(item.item_id);
      const expanded = isItemExpanded(item);
      const hasVariants = item.variants != null && item.variants.count > 1;
      return (
        <div key={key}>
          <AggregateItemRow item={item} {...rowProps} isExpanded={expanded} />
          {expanded &&
            hasVariants &&
            isDecisionSection &&
            onSelectVariant &&
            diffHook && (
              <VariantView
                item={item}
                ack={ack}
                onSelectVariant={onSelectVariant}
                diffHook={diffHook}
                sectionId={section.id}
              />
            )}
          {expanded && !hasVariants && isDecisionSection && (
            <ItemDetailPane item={item} sectionId={section.id} />
          )}
        </div>
      );
    });

  if (suppressHeaders) {
    return (
      <div className="aggregate-section" data-testid="aggregate-section">
        {renderItems(divergentFiltered)}
        {renderItems(nearConsensusFiltered)}
        {renderItems(consensusFiltered)}
      </div>
    );
  }

  return (
    <div className="aggregate-section" data-testid="aggregate-section">
      {divergentFiltered.length > 0 && (
        <ZoneGroup
          zone="divergent"
          count={zones.divergent.count}
          defaultExpanded={true}
          forceExpanded={forceExpandZone === "divergent"}
        >
          {renderItems(divergentFiltered)}
        </ZoneGroup>
      )}
      {nearConsensusFiltered.length > 0 && (
        <ZoneGroup
          zone="near_consensus"
          count={zones.near_consensus.count}
          defaultExpanded={true}
          forceExpanded={forceExpandZone === "near_consensus"}
        >
          {renderItems(nearConsensusFiltered)}
        </ZoneGroup>
      )}
      {consensusFiltered.length > 0 && (
        <ZoneGroup
          zone="consensus"
          count={zones.consensus.count}
          defaultExpanded={false}
          forceExpanded={forceExpandZone === "consensus"}
        >
          {renderItems(consensusFiltered)}
        </ZoneGroup>
      )}
    </div>
  );
}
