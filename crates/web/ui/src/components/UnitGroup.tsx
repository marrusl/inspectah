import { Badge, Label } from "@patternfly/react-core";
import { AngleRightIcon, AngleDownIcon } from "@patternfly/react-icons";
import type { GroupId } from "./unitOutcome";

export interface UnitGroupProps {
  groupId: GroupId;
  title: string;
  /** Shown rows (§ 6.3 item 3); no badge when undefined. */
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
 * The button's accessible name, "Services 11, 5 need follow-up" (§ 6.3):
 * the badge and the label render as adjacent text with no separator.
 */
function headerName(title: string, count?: number, followUpText?: string | null): string {
  let name = count === undefined ? title : `${title} ${count}`;
  if (followUpText) name += `, ${followUpText}`;
  return name;
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
        aria-label={headerName(title, count, followUpText)}
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
        {count !== undefined && <Badge isRead>{count}</Badge>}
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
