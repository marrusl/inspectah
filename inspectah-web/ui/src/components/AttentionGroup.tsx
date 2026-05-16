import { useState } from "react";
import { AngleRightIcon, AngleDownIcon } from "@patternfly/react-icons";
import type { AttentionLevel } from "../api/types";

/** PatternFly token-based border colors per attention level. */
const BORDER_COLORS: Record<AttentionLevel, string> = {
  needs_review: "var(--pf-t--global--color--status--danger--default)",
  informational: "var(--pf-t--global--color--status--warning--default)",
  routine: "var(--pf-t--global--color--status--success--default)",
};

const LEVEL_LABELS: Record<AttentionLevel, string> = {
  needs_review: "Needs Review",
  informational: "Informational",
  routine: "Routine",
};

export interface AttentionGroupProps {
  level: AttentionLevel;
  count: number;
  /** When true, the group is forced open by a filter regardless of user toggle state. */
  forceExpanded?: boolean;
  children: React.ReactNode;
}

export function AttentionGroup({ level, count, forceExpanded, children }: AttentionGroupProps) {
  const defaultExpanded = level === "needs_review";
  const [isExpanded, setIsExpanded] = useState(defaultExpanded);

  const label = LEVEL_LABELS[level];
  const toggleText = `${label} (${count})`;

  // forceExpanded overrides user toggle; user toggle still controls when filter is inactive
  const effectiveExpanded = forceExpanded || isExpanded;

  return (
    <div
      data-testid={`attention-group-${level}`}
      style={{
        borderLeft: `4px solid ${BORDER_COLORS[level]}`,
        paddingLeft: "var(--pf-t--global--spacer--md)",
        marginBottom: "var(--pf-t--global--spacer--md)",
      }}
    >
      <button
        type="button"
        onClick={() => setIsExpanded((prev) => !prev)}
        aria-expanded={effectiveExpanded}
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
        {effectiveExpanded ? <AngleDownIcon /> : <AngleRightIcon />}
        {toggleText}
      </button>
      <div hidden={!effectiveExpanded}>
        {children}
      </div>
    </div>
  );
}
