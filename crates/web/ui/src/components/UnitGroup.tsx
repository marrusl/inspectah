import { Badge, Label } from "@patternfly/react-core";
import { AngleRightIcon, AngleDownIcon } from "@patternfly/react-icons";
import { groupHeader, type GroupId } from "./unitOutcome";

export interface UnitGroupProps {
  groupId: GroupId;
  title: string;
  /** Shown rows (§ 6.3 item 3); no badge when undefined or 0. */
  count?: number;
  /** § 7 string 10, or null for no flag. */
  followUpText?: string | null;
  /** A line under the header, outside the grid. */
  description?: string;
  expanded: boolean;
  onToggle: () => void;
  children: React.ReactNode;
}

/**
 * A collapsible group of the units section (§ 6.3, § 6.13): a header
 * button that controls a grid labelled by the title alone. The button
 * follows `TriageBucketGroup`'s pattern and adds `aria-controls`. A
 * collapsed grid keeps its rows in the DOM under `hidden` (§ 6.2).
 * Group ids are fixed words, so the DOM ids hold no host data.
 */
export function UnitGroup({
  groupId,
  title,
  count,
  followUpText,
  description,
  expanded,
  onToggle,
  children,
}: UnitGroupProps) {
  const header = groupHeader(title, count, followUpText);
  const titleId = `unit-group-title-${groupId}`;
  const gridId = `unit-group-grid-${groupId}`;
  return (
    <div
      className="inspectah-unit-group"
      style={{ marginBottom: "var(--pf-t--global--spacer--md)" }}
    >
      <button
        type="button"
        onClick={onToggle}
        aria-expanded={expanded}
        aria-controls={gridId}
        aria-label={header.name}
        style={{
          background: "none",
          border: "none",
          cursor: "pointer",
          padding: "var(--pf-t--global--spacer--xs) 0",
          marginBottom: "var(--pf-t--global--spacer--xs)",
          fontWeight: 600,
          fontSize: "var(--pf-t--global--font--size--md)",
          color: "var(--pf-t--global--link--color--default)",
          display: "flex",
          alignItems: "center",
          gap: "var(--pf-t--global--spacer--xs)",
        }}
      >
        {expanded ? <AngleDownIcon /> : <AngleRightIcon />}
        <span id={titleId}>{title}</span>
        {header.badge !== null && <Badge isRead>{header.badge}</Badge>}
        {followUpText && (
          <Label color="orange" variant="outline" isCompact>
            {followUpText}
          </Label>
        )}
      </button>
      {description && (
        <p className="inspectah-unit-group__description">{description}</p>
      )}
      <div role="grid" id={gridId} aria-labelledby={titleId} hidden={!expanded}>
        <div role="rowgroup">{children}</div>
      </div>
    </div>
  );
}
