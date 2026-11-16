import { Badge, ToggleGroup, ToggleGroupItem } from "@patternfly/react-core";
import { filterBarPresentation, type FilterValue, type OutcomeCounts } from "./unitOutcome";

export interface UnitOutcomeFilterProps {
  counts: OutcomeCounts;
  value: FilterValue;
  onChange: (value: FilterValue) => void;
  /** The live region's text: empty until the operator changes the value. */
  announcement: string;
}

/** § 7 string 7: the bar's label and the toggle group's accessible name. */
const SHOW_LABEL = "Show";

/** § 7 string 9, under the bar while a row needs follow-up. */
const FOLLOW_UP_HINT =
  "Needs follow-up: the Containerfile writes a comment for the row and no command.";

/**
 * The units outcome filter (§ 6.4, § 6.13): a toggle group of four values,
 * each with its count, the hint line, and a polite live region that is in
 * the DOM from the first render so the first change is announced.
 */
export function UnitOutcomeFilter({ counts, value, onChange, announcement }: UnitOutcomeFilterProps) {
  const bar = filterBarPresentation(counts, value);
  return (
    <div className="inspectah-unit-filter" style={{ marginBottom: "var(--pf-t--global--spacer--md)" }}>
      <div
        style={{
          display: "flex",
          flexWrap: "wrap",
          alignItems: "center",
          gap: "var(--pf-t--global--spacer--sm)",
        }}
      >
        <span className="inspectah-unit-filter__label">{SHOW_LABEL}</span>
        <ToggleGroup aria-label={SHOW_LABEL} isCompact>
          {bar.items.map((item) => (
            <ToggleGroupItem
              key={item.value}
              text={
                <>
                  {item.label} <Badge isRead>{item.count}</Badge>
                </>
              }
              isSelected={item.selected}
              isDisabled={item.disabled}
              onChange={() => {
                if (!item.selected) onChange(item.value);
              }}
            />
          ))}
        </ToggleGroup>
      </div>
      {bar.hint && (
        <p
          className="inspectah-unit-filter__hint"
          style={{
            marginTop: "var(--pf-t--global--spacer--xs)",
            color: "var(--pf-t--global--text--color--subtle)",
          }}
        >
          {FOLLOW_UP_HINT}
        </p>
      )}
      <div className="pf-v6-screen-reader" aria-live="polite">
        {announcement}
      </div>
    </div>
  );
}
