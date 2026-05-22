import type { FleetSection, FleetItem, ItemId } from "../../api/types";
import type { UseVariantAckResult } from "../../hooks/useVariantAck";
import { ZoneGroup } from "./ZoneGroup";
import { FleetItemRow, itemDisplayName } from "./FleetItemRow";

export interface FleetSectionContentProps {
  section: FleetSection | undefined;
  filterText: string;
  isDecisionSection: boolean;
  onToggle: (itemId: ItemId, include: boolean) => void;
  ack: UseVariantAckResult;
  onExpandVariant?: (itemId: ItemId) => void;
}

function filterItems(items: FleetItem[], filterText: string): FleetItem[] {
  if (!filterText) return items;
  const lower = filterText.toLowerCase();
  return items.filter((item) =>
    itemDisplayName(item.item_id).toLowerCase().includes(lower),
  );
}

export function FleetSectionContent({
  section,
  filterText,
  isDecisionSection,
  onToggle,
  ack,
  onExpandVariant,
}: FleetSectionContentProps) {
  if (!section) return null;

  const rowProps = { isDecisionSection, onToggle, ack, onExpandVariant };

  // Flat mode: section has items directly (fleet-of-2 or no zones)
  if (!section.zones) {
    const filtered = filterItems(section.items ?? [], filterText);
    return (
      <div className="fleet-section" data-testid="fleet-section">
        {filtered.map((item) => (
          <FleetItemRow key={JSON.stringify(item.item_id)} item={item} {...rowProps} />
        ))}
      </div>
    );
  }

  // Zone mode: group items by consensus zone
  const zones = section.zones;
  const consensusFiltered = filterItems(zones.consensus.items, filterText);
  const nearConsensusFiltered = filterItems(zones.near_consensus.items, filterText);
  const divergentFiltered = filterItems(zones.divergent.items, filterText);

  // Count zones that have items (for header suppression)
  const populatedZones = [
    consensusFiltered.length > 0 ? 1 : 0,
    nearConsensusFiltered.length > 0 ? 1 : 0,
    divergentFiltered.length > 0 ? 1 : 0,
  ].reduce((a, b) => a + b, 0);

  const suppressHeaders = populatedZones <= 1;

  const renderItems = (items: FleetItem[]) =>
    items.map((item) => (
      <FleetItemRow key={JSON.stringify(item.item_id)} item={item} {...rowProps} />
    ));

  if (suppressHeaders) {
    // Render items flat without zone wrappers
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
        <ZoneGroup zone="consensus" count={zones.consensus.count} defaultExpanded={false}>
          {renderItems(consensusFiltered)}
        </ZoneGroup>
      )}
      {nearConsensusFiltered.length > 0 && (
        <ZoneGroup zone="near_consensus" count={zones.near_consensus.count} defaultExpanded={true}>
          {renderItems(nearConsensusFiltered)}
        </ZoneGroup>
      )}
      {divergentFiltered.length > 0 && (
        <ZoneGroup zone="divergent" count={zones.divergent.count} defaultExpanded={true}>
          {renderItems(divergentFiltered)}
        </ZoneGroup>
      )}
    </div>
  );
}
