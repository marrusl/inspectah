import type {
  FleetSummary,
  ActionableVariantItem,
  ItemId,
} from "../../api/types";
import type { UseVariantAckResult } from "../../hooks/useVariantAck";
import { itemDisplayName } from "./AggregateItemRow";

export interface AggregateBannerProps {
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

function getSeverity(unacked: number, total: number): Severity {
  if (total === 0 || unacked === 0) return "success";
  if (unacked < total) return "warning";
  return "danger";
}

export function AggregateBanner({
  summary,
  ackState,
  onNavigate,
  activeSection,
}: AggregateBannerProps) {
  const { actionable_variant_items, informational_variant_count } = summary;

  if (actionable_variant_items.length === 0) return null;

  // Filter items to the active section when specified
  const sectionItems = activeSection
    ? actionable_variant_items.filter(
        (item) => item.section_id === activeSection,
      )
    : actionable_variant_items;

  const otherSectionItems = activeSection
    ? actionable_variant_items.filter(
        (item) => item.section_id !== activeSection,
      )
    : [];

  // Count unacked items within the visible set
  const unackedSectionItems = sectionItems.filter(
    (item) => !ackState.isAcked(item.item_id),
  );

  // Severity is scoped to the active section so color matches what the user sees
  const severity = activeSection
    ? getSeverity(unackedSectionItems.length, sectionItems.length)
    : getSeverity(ackState.unackedCount, ackState.totalCount);

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

  const sectionLabel = activeSection
    ? sectionTag(activeSection).toLowerCase()
    : "items";
  const unackedInSection = unackedSectionItems.length;

  const headline =
    severity === "success"
      ? `All ${ackState.totalCount} variants reviewed`
      : activeSection
        ? `${unackedInSection} ${sectionLabel} ${unackedInSection === 1 ? "item has" : "items have"} variants requiring review`
        : `${ackState.unackedCount} items have variants requiring review`;

  return (
    <div
      className={`aggregate-banner aggregate-banner--${severity}`}
      data-testid="aggregate-banner"
      data-severity={severity}
      role="status"
    >
      <div className="aggregate-banner__headline">{headline}</div>

      {unackedSectionItems.length > 0 && (
        <ul className="aggregate-banner__items">
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
        <div className="aggregate-banner__cross-section">
          Also:{" "}
          {Object.entries(otherSectionSummary)
            .map(
              ([tag, count]) =>
                `${count} ${tag.toLowerCase()} variant${count !== 1 ? "s" : ""}`,
            )
            .join(", ")}
        </div>
      )}

      {informational_variant_count > 0 && (
        <div className="aggregate-banner__info">
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
    <li className="aggregate-banner__item">
      <span className="aggregate-banner__item-tag">[{tag}]</span>
      <button
        type="button"
        className="aggregate-banner__item-link"
        onClick={() => onNavigate(item.section_id, item.item_id)}
        aria-label={`Navigate to ${name}`}
      >
        {name}
      </button>
      <span className="aggregate-banner__item-count">
        {" — "}
        {item.variant_count} variants
      </span>
    </li>
  );
}
