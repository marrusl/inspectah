import { useState } from "react";
import { AngleRightIcon, AngleDownIcon } from "@patternfly/react-icons";
import type { AttentionLevel } from "../api/types";

/** PatternFly token-based border colors per attention level. */
const BORDER_COLORS: Record<AttentionLevel, string> = {
  needs_review: "var(--pf-t--global--color--status--danger--default)",
  informational: "var(--pf-t--global--color--status--info--default)",
  routine: "var(--pf-t--global--color--status--success--default)",
};

const LEVEL_LABELS: Record<AttentionLevel, string> = {
  needs_review: "Needs Review",
  informational: "Informational",
  routine: "Routine",
};

/** Levels that default to expanded. */
const EXPANDED_BY_DEFAULT: Set<AttentionLevel> = new Set([
  "needs_review",
  "informational",
]);

/** Threshold below which sections are always expanded regardless of level. */
const ALWAYS_EXPAND_THRESHOLD = 3;

export interface TriageBucketGroupProps {
  level: AttentionLevel;
  count: number;
  /** When true, the group is forced open by a filter regardless of user toggle state. */
  forceExpanded?: boolean;
  /** aria-label for the grid element, set by parent. */
  gridLabel?: string;
  children: React.ReactNode;
}

export function TriageBucketGroup({
  level,
  count,
  forceExpanded,
  gridLabel,
  children,
}: TriageBucketGroupProps) {
  // Collapsible section behavior:
  //   investigate + divergent/site (needs_review + informational): expanded by default
  //   partial + universal/baseline (routine): collapsed by default
  //   Sections with <3 items: always expanded
  //   Empty sections: show header with "(0)", disabled
  const smallSection = count < ALWAYS_EXPAND_THRESHOLD;
  const defaultExpanded = EXPANDED_BY_DEFAULT.has(level) || smallSection;
  const [isExpanded, setIsExpanded] = useState(defaultExpanded);
  const isEmpty = count === 0;

  const label = LEVEL_LABELS[level];
  const toggleText = `${label} (${count})`;

  // forceExpanded overrides user toggle; small sections always expand; user toggle controls otherwise
  const effectiveExpanded = isEmpty
    ? false
    : forceExpanded || smallSection || isExpanded;

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
        onClick={() => !isEmpty && setIsExpanded((prev) => !prev)}
        aria-expanded={effectiveExpanded}
        aria-disabled={isEmpty || undefined}
        style={{
          background: "none",
          border: "none",
          cursor: isEmpty ? "default" : "pointer",
          padding: "var(--pf-t--global--spacer--xs) 0",
          marginBottom: "var(--pf-t--global--spacer--xs)",
          fontWeight: 600,
          fontSize: "var(--pf-t--global--font--size--md)",
          color: isEmpty
            ? "var(--pf-t--global--text--color--disabled)"
            : "var(--pf-t--global--link--color--default)",
          display: "flex",
          alignItems: "center",
          gap: "var(--pf-t--global--spacer--xs)",
          opacity: isEmpty ? 0.6 : 1,
        }}
      >
        {effectiveExpanded ? <AngleDownIcon /> : <AngleRightIcon />}
        {toggleText}
      </button>
      <div
        role="grid"
        aria-label={gridLabel ?? label}
        aria-rowcount={count}
        hidden={!effectiveExpanded}
      >
        <div role="rowgroup">{children}</div>
      </div>
    </div>
  );
}
