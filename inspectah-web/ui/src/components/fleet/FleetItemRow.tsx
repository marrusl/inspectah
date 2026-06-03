import { Label, Switch } from "@patternfly/react-core";
import type { FleetItem, ItemId } from "../../api/types";
import type { UseVariantAckResult } from "../../hooks/useVariantAck";

export interface FleetItemRowProps {
  item: FleetItem;
  isDecisionSection: boolean;
  onToggle: (itemId: ItemId, include: boolean) => void;
  ack: UseVariantAckResult;
  onExpandVariant?: (itemId: ItemId) => void;
  /** Whether this row's inline variant view is expanded. */
  isExpanded?: boolean;
}

export function attentionDisplayLabel(level: string): string {
  switch (level) {
    case "needs_review":
      return "Needs review";
    case "informational":
      return "Info";
    case "routine":
      return "Routine";
    default:
      return level.replace(/_/g, " ");
  }
}

export function itemDisplayName(itemId: ItemId): string {
  switch (itemId.kind) {
    case "Package":
      return `${itemId.key.name}.${itemId.key.arch}`;
    case "Config":
      return itemId.key.path;
    case "Repo":
      return itemId.key.path;
    case "ModuleStream":
      return itemId.key.module_stream;
    case "VersionLock":
      return itemId.key.name_arch;
    case "Service":
      return itemId.key.unit;
    case "DropIn":
      return itemId.key.path;
    case "Quadlet":
      return itemId.key.path;
    case "Compose":
      return itemId.key.path;
    case "Flatpak": {
      const parts = [itemId.key.app_id];
      if (itemId.key.remote) parts.push(itemId.key.remote);
      if (itemId.key.branch) parts.push(itemId.key.branch);
      return parts.join(" / ");
    }
    case "NMConnection":
      return itemId.key.path;
    case "FirewallZone":
      return itemId.key.path;
    case "KernelModule":
      return itemId.key.name;
    case "Sysctl":
      return itemId.key.key;
    case "TunedSelection":
      return itemId.key.profile;
    case "CronJob":
      return itemId.key.path;
    case "SystemdTimer":
      return itemId.key.name;
    case "AtJob":
      return itemId.key.file;
    case "GeneratedTimer":
      return itemId.key.name;
    case "SelinuxPort":
      return itemId.key.protocol_port;
    case "Fstab":
      return itemId.key.mount_point;
    case "NonRpm":
      return itemId.key.name;
  }
}

function prevalenceLevel(count: number, total: number): string {
  if (total === 0) return "full";
  const ratio = count / total;
  if (ratio >= 1) return "full";
  if (ratio >= 0.6) return "partial";
  return "low";
}

export function FleetItemRow({
  item,
  isDecisionSection,
  onToggle,
  ack: _ack,
  onExpandVariant,
  isExpanded = false,
}: FleetItemRowProps) {
  const name = itemDisplayName(item.item_id);
  const { count, total } = item.prevalence;
  const hasVariants = item.variants != null && item.variants.count > 1;
  const locked = item.locked === true;

  const handleToggle = () => {
    if (locked) return;
    onToggle(item.item_id, !item.include);
  };

  const handleVariantClick = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (isDecisionSection) onExpandVariant?.(item.item_id);
  };

  const handleRowClick = () => {
    if (isDecisionSection) onExpandVariant?.(item.item_id);
  };

  return (
    <div
      className={`fleet-item-row${locked ? " fleet-item-row--locked" : ""}`}
      data-testid="fleet-item-row"
      data-item-id={JSON.stringify(item.item_id)}
      data-locked={locked ? "true" : undefined}
      onClick={handleRowClick}
      role="row"
      tabIndex={0}
    >
      {isDecisionSection && (
        <div
          className="fleet-item-row__toggle"
          onClick={(e) => e.stopPropagation()}
        >
          <Switch
            id={`fleet-switch-${name}`}
            isChecked={item.include}
            onChange={handleToggle}
            isDisabled={locked}
            aria-label={locked ? `${name} (locked)` : `Toggle ${name}`}
          />
        </div>
      )}

      <div className="fleet-item-row__name">{name}</div>

      {locked && (
        <Label
          color="grey"
          isCompact
          data-testid={`locked-badge-fleet-${name}`}
          className="fleet-item-row__locked-badge"
        >
          {item.attention_reason ?? "LOCKED"}
        </Label>
      )}

      <span
        className={`fleet-item-row__prevalence fleet-item-row__prevalence--${prevalenceLevel(count, total)}`}
      >
        {count}/{total} hosts
      </span>

      {hasVariants && isDecisionSection && (
        <button
          className="fleet-item-row__variants"
          onClick={handleVariantClick}
          type="button"
          aria-expanded={isExpanded}
        >
          {item.variants!.count} variants{" "}
          <span
            className={`fleet-item-row__variants-chevron${isExpanded ? " fleet-item-row__variants-chevron--expanded" : ""}`}
            aria-hidden="true"
          >
            &#9656;
          </span>
        </button>
      )}
      {hasVariants && !isDecisionSection && (
        <span className="fleet-item-row__variants fleet-item-row__variants--readonly">
          {item.variants!.count} variants
        </span>
      )}
    </div>
  );
}
