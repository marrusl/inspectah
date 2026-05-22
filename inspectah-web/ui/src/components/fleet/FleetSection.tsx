import { useState, useEffect } from "react";
import type { FleetSection, FleetItem, ItemId } from "../../api/types";
import type { UseVariantAckResult } from "../../hooks/useVariantAck";
import { ZoneGroup } from "./ZoneGroup";
import { FleetItemRow, itemDisplayName } from "./FleetItemRow";

export interface NavTarget {
  sectionId: string;
  itemId: ItemId;
}

export interface FleetSectionContentProps {
  section: FleetSection | undefined;
  filterText: string;
  isDecisionSection: boolean;
  onToggle: (itemId: ItemId, include: boolean) => void;
  ack: UseVariantAckResult;
  onExpandVariant?: (itemId: ItemId) => void;
  pendingNavTarget?: NavTarget | null;
  onNavTargetConsumed?: () => void;
}

function filterItems(items: FleetItem[], filterText: string): FleetItem[] {
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
  section: FleetSection,
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

export function FleetSectionContent({
  section,
  filterText,
  isDecisionSection,
  onToggle,
  ack,
  onExpandVariant,
  pendingNavTarget,
  onNavTargetConsumed,
}: FleetSectionContentProps) {
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

    // Auto-expand the variant view for the target item
    if (onExpandVariant) {
      const allItems = section.items ?? [
        ...(section.zones?.consensus.items ?? []),
        ...(section.zones?.near_consensus.items ?? []),
        ...(section.zones?.divergent.items ?? []),
      ];
      const targetItem = allItems.find((i) => itemIdKey(i.item_id) === targetKey);
      if (targetItem?.variants) {
        onExpandVariant(pendingNavTarget.itemId);
      }
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

  const rowProps = { isDecisionSection, onToggle, ack, onExpandVariant };

  // Flat mode: section has items directly (fleet-of-2 or no zones)
  if (!section.zones) {
    const filtered = filterItems(section.items ?? [], filterText);
    return (
      <div className="fleet-section" data-testid="fleet-section">
        {filtered.map((item) => (
          <FleetItemRow key={itemIdKey(item.item_id)} item={item} {...rowProps} />
        ))}
      </div>
    );
  }

  // Zone mode: group items by consensus zone
  const zones = section.zones;
  const consensusFiltered = filterItems(zones.consensus.items, filterText);
  const nearConsensusFiltered = filterItems(zones.near_consensus.items, filterText);
  const divergentFiltered = filterItems(zones.divergent.items, filterText);

  // Count zones with unfiltered items for header suppression
  const populatedZones = [
    zones.consensus.items.length > 0 ? 1 : 0,
    zones.near_consensus.items.length > 0 ? 1 : 0,
    zones.divergent.items.length > 0 ? 1 : 0,
  ].reduce((a, b) => a + b, 0);

  const suppressHeaders = populatedZones <= 1;

  const renderItems = (items: FleetItem[]) =>
    items.map((item) => (
      <FleetItemRow key={itemIdKey(item.item_id)} item={item} {...rowProps} />
    ));

  if (suppressHeaders) {
    return (
      <div className="fleet-section" data-testid="fleet-section">
        {renderItems(consensusFiltered)}
        {renderItems(nearConsensusFiltered)}
        {renderItems(divergentFiltered)}
      </div>
    );
  }

  return (
    <div className="fleet-section" data-testid="fleet-section">
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
    </div>
  );
}
