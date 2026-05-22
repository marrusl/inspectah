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

const severityColors: Record<Severity, { bg: string; border: string }> = {
  success: {
    bg: "var(--pf-t--global--color--status--success--default, #3e8635)",
    border: "var(--pf-t--global--color--status--success--default, #3e8635)",
  },
  warning: {
    bg: "var(--pf-t--global--color--status--warning--default, #f0ab00)",
    border: "var(--pf-t--global--color--status--warning--default, #f0ab00)",
  },
  danger: {
    bg: "var(--pf-t--global--color--status--danger--default, #c9190b)",
    border: "var(--pf-t--global--color--status--danger--default, #c9190b)",
  },
};

export function FleetBanner({
  summary,
  ackState,
  onNavigate,
}: FleetBannerProps) {
  const { actionable_variant_items, informational_variant_count } = summary;

  if (actionable_variant_items.length === 0) return null;

  const severity = getSeverity(ackState);
  const colors = severityColors[severity];

  const headline =
    severity === "success"
      ? `All ${ackState.totalCount} variants reviewed`
      : `${ackState.unackedCount} config items have variants requiring review`;

  const unackedItems =
    severity === "success"
      ? []
      : actionable_variant_items.filter(
          (item) => !ackState.isAcked(item.item_id),
        );

  return (
    <div
      data-testid="fleet-banner"
      data-severity={severity}
      role="status"
      style={{
        border: `1px solid ${colors.border}`,
        borderLeft: `4px solid ${colors.border}`,
        borderRadius: "4px",
        padding: "12px 16px",
        marginBottom: "16px",
        backgroundColor:
          severity === "success"
            ? "var(--pf-t--global--color--status--success--100, #f3faf2)"
            : severity === "warning"
              ? "var(--pf-t--global--color--status--warning--100, #fef6e7)"
              : "var(--pf-t--global--color--status--danger--100, #fce8e6)",
      }}
    >
      <div style={{ fontWeight: 600, marginBottom: unackedItems.length > 0 ? "8px" : 0 }}>
        {headline}
      </div>

      {unackedItems.length > 0 && (
        <ul style={{ margin: 0, padding: 0, listStyle: "none" }}>
          {unackedItems.map((item) => (
            <BannerItem
              key={JSON.stringify(item.item_id)}
              item={item}
              onNavigate={onNavigate}
            />
          ))}
        </ul>
      )}

      {informational_variant_count > 0 && (
        <div
          style={{
            marginTop: "8px",
            fontSize: "0.875rem",
            opacity: 0.8,
          }}
        >
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
    <li style={{ padding: "2px 0" }}>
      <span
        style={{
          display: "inline-block",
          fontSize: "0.75rem",
          fontWeight: 600,
          color: "var(--pf-t--global--text--color--subtle, #6a6e73)",
          marginRight: "6px",
        }}
      >
        [{tag}]
      </span>
      <button
        type="button"
        onClick={() => onNavigate(item.section_id, item.item_id)}
        aria-label={`Navigate to ${name}`}
        style={{
          background: "none",
          border: "none",
          padding: 0,
          cursor: "pointer",
          textDecoration: "underline",
          color: "inherit",
          fontSize: "inherit",
        }}
      >
        {name}
      </button>
      <span
        style={{
          marginLeft: "8px",
          fontSize: "0.8125rem",
          color: "var(--pf-t--global--text--color--subtle, #6a6e73)",
        }}
      >
        &mdash; {item.variant_count} variants
      </span>
    </li>
  );
}
