import {
  usePrevalenceDisplay,
  formatPrevalence,
} from "../hooks/usePrevalenceDisplay";

export interface PrevalenceBadgeProps {
  count: number;
  total: number;
  /** Optional suffix after the formatted value (e.g., "hosts"). */
  suffix?: string;
}

function prevalenceLevel(count: number, total: number): string {
  if (total === 0) return "full";
  const ratio = count / total;
  if (ratio >= 1) return "full";
  if (ratio >= 0.6) return "partial";
  return "low";
}

/**
 * Clickable prevalence badge that toggles between fraction and percentage
 * display. Uses global PrevalenceDisplayContext so one click changes all
 * badges across the UI.
 */
export function PrevalenceBadge({
  count,
  total,
  suffix,
}: PrevalenceBadgeProps) {
  const { mode, cycle } = usePrevalenceDisplay();
  const label = formatPrevalence(count, total, mode);

  return (
    <button
      type="button"
      className={`prevalence-badge prevalence-badge--${prevalenceLevel(count, total)}`}
      onClick={(e) => {
        e.stopPropagation();
        cycle();
      }}
      title="Click to toggle display format"
    >
      {label}
      {suffix ? ` ${suffix}` : ""}
    </button>
  );
}
