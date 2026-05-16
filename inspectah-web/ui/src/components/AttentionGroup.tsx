import { useState } from "react";
import { ExpandableSection } from "@patternfly/react-core";
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
      <ExpandableSection
        toggleText={toggleText}
        isExpanded={effectiveExpanded}
        onToggle={(_e, expanded) => setIsExpanded(expanded)}
        aria-label={`${label} items`}
      >
        {children}
      </ExpandableSection>
    </div>
  );
}
