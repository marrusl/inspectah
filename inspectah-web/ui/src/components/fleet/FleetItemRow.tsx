import { Switch, Badge } from "@patternfly/react-core";
import type { FleetItem, ItemId } from "../../api/types";
import type { UseVariantAckResult } from "../../hooks/useVariantAck";

export interface FleetItemRowProps {
  item: FleetItem;
  isDecisionSection: boolean;
  onToggle: (itemId: ItemId, include: boolean) => void;
  ack: UseVariantAckResult;
  onExpandVariant?: (itemId: ItemId) => void;
}

export function itemDisplayName(itemId: ItemId): string {
  switch (itemId.kind) {
    case "Config":
      return itemId.key.path;
    case "Package":
      return itemId.key.name_arch;
  }
}

export function FleetItemRow({
  item,
  isDecisionSection,
  onToggle,
  ack,
  onExpandVariant,
}: FleetItemRowProps) {
  const name = itemDisplayName(item.item_id);
  const { count, total } = item.prevalence;
  const hasVariants = item.variants != null && item.variants.count > 1;
  const showAttention = item.attention.level !== "none";

  const handleToggle = () => {
    onToggle(item.item_id, !item.include);
  };

  const handleVariantClick = (e: React.MouseEvent) => {
    e.stopPropagation();
    onExpandVariant?.(item.item_id);
  };

  const handleRowClick = () => {
    onExpandVariant?.(item.item_id);
  };

  return (
    <div
      className="fleet-item-row"
      data-testid="fleet-item-row"
      data-item-id={JSON.stringify(item.item_id)}
      onClick={handleRowClick}
      role="row"
    >
      {isDecisionSection && (
        <div className="fleet-item-row__toggle" onClick={(e) => e.stopPropagation()}>
          <Switch
            id={`fleet-switch-${name}`}
            isChecked={item.include}
            onChange={handleToggle}
            aria-label={`Toggle ${name}`}
          />
        </div>
      )}

      <div className="fleet-item-row__name">{name}</div>

      <Badge isRead className="fleet-item-row__prevalence">
        {count}/{total} hosts
      </Badge>

      {hasVariants && (
        <button
          className="fleet-item-row__variants"
          onClick={handleVariantClick}
          type="button"
        >
          {item.variants!.count} variants
        </button>
      )}

      {showAttention && (
        <span
          className={`fleet-item-row__attention fleet-item-row__attention--${item.attention.level}`}
          data-testid="attention-badge"
        >
          {item.attention.level}
        </span>
      )}
    </div>
  );
}
