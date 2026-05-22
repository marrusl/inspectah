import type {
  FleetSummary,
  ActionableVariantItem,
  ItemId,
} from "../../api/types";
import type { UseVariantAckResult } from "../../hooks/useVariantAck";
import { itemDisplayName } from "./FleetItemRow";

export interface FleetBannerProps {
  summary: FleetSummary;
  ackState: UseVariantAckResult;
  onNavigate: (sectionId: string, itemId: ItemId) => void;
  /** When set, only show items for this section; summarize others. */
  activeSection?: string;
}

/** Convert a snake_case section_id to a readable tag label. */
function sectionTag(sectionId: string): string {
  const labels: Record<string, string> = {
    config_files: "Config",
    packages: "Packages",
    repos: "Repos",
    users_groups: "Users",
  };
  return labels[sectionId] ?? sectionId;
}

type Severity = "success" | "warning" | "danger";

function getSeverity(ackState: UseVariantAckResult): Severity {
  if (ackState.unackedCount === 0) return "success";
  if (ackState.unackedCount < ackState.totalCount) return "warning";
  return "danger";
}

export function FleetBanner({
  summary,
  ackState,
  onNavigate,
  activeSection,
}: FleetBannerProps) {
  const { actionable_variant_items, informational_variant_count } = summary;

  if (actionable_variant_items.length === 0) return null;

  const severity = getSeverity(ackState);

  // Filter items to the active section when specified
  const sectionItems = activeSection
    ? actionable_variant_items.filter((item) => item.section_id === activeSection)
    : actionable_variant_items;

  const otherSectionItems = activeSection
    ? actionable_variant_items.filter((item) => item.section_id !== activeSection)
    : [];

  // Count unacked items within the visible set
  const unackedSectionItems =
    severity === "success"
      ? []
      : sectionItems.filter((item) => !ackState.isAcked(item.item_id));

  // Build cross-section summary for other sections
  const otherSectionSummary = otherSectionItems.reduce<Record<string, number>>(
    (acc, item) => {
      if (!ackState.isAcked(item.item_id)) {
        const tag = sectionTag(item.section_id);
        acc[tag] = (acc[tag] ?? 0) + 1;
      }
      return acc;
    },
    {},
  );

  const headline =
    severity === "success"
      ? `All ${ackState.totalCount} variants reviewed`
      : `${ackState.unackedCount} config items have variants requiring review`;

  return (
    <div
      className={`fleet-banner fleet-banner--${severity}`}
      data-testid="fleet-banner"
      data-severity={severity}
      role="status"
    >
      <div className="fleet-banner__headline">
        {headline}
      </div>

      {unackedSectionItems.length > 0 && (
        <ul className="fleet-banner__items">
          {unackedSectionItems.map((item) => (
            <BannerItem
              key={JSON.stringify(item.item_id)}
              item={item}
              onNavigate={onNavigate}
            />
          ))}
        </ul>
      )}

      {Object.keys(otherSectionSummary).length > 0 && (
        <div className="fleet-banner__cross-section">
          Also:{" "}
          {Object.entries(otherSectionSummary)
            .map(([tag, count]) => `${count} ${tag.toLowerCase()} variant${count !== 1 ? "s" : ""}`)
            .join(", ")}
        </div>
      )}

      {informational_variant_count > 0 && (
        <div className="fleet-banner__info">
          {informational_variant_count} additional items in other sections have
          variants (read-only)
        </div>
      )}
    </div>
  );
}

function BannerItem({
  item,
  onNavigate,
}: {
  item: ActionableVariantItem;
  onNavigate: (sectionId: string, itemId: ItemId) => void;
}) {
  const name = itemDisplayName(item.item_id);
  const tag = sectionTag(item.section_id);

  return (
    <li className="fleet-banner__item">
      <span className="fleet-banner__item-tag">
        [{tag}]
      </span>
      <button
        type="button"
        className="fleet-banner__item-link"
        onClick={() => onNavigate(item.section_id, item.item_id)}
        aria-label={`Navigate to ${name}`}
      >
        {name}
      </button>
      <span className="fleet-banner__item-count">
        {" — "}{item.variant_count} variants
      </span>
    </li>
  );
}
