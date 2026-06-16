import { useState, useEffect } from "react";
import { ExpandableSection, Badge } from "@patternfly/react-core";

export interface ZoneGroupProps {
  zone: "consensus" | "near_consensus" | "divergent";
  count: number;
  defaultExpanded: boolean;
  /** When true, forces the zone open regardless of internal state. */
  forceExpanded?: boolean;
  children: React.ReactNode;
}

const ZONE_LABELS: Record<ZoneGroupProps["zone"], string> = {
  consensus: "Consensus",
  near_consensus: "Near Consensus",
  divergent: "Divergent",
};

export function ZoneGroup({
  zone,
  count,
  defaultExpanded,
  forceExpanded,
  children,
}: ZoneGroupProps) {
  const [isExpanded, setIsExpanded] = useState(defaultExpanded);
  const label = ZONE_LABELS[zone];

  useEffect(() => {
    if (forceExpanded) setIsExpanded(true);
  }, [forceExpanded]);

  return (
    <div
      className={`aggregate-zone-group aggregate-zone-group--${zone}`}
      data-testid={`zone-${zone}`}
    >
      <ExpandableSection
        isExpanded={isExpanded}
        onToggle={(_event, expanded) => setIsExpanded(expanded)}
        toggleContent={
          <span className="aggregate-zone-group__header">
            <span className="aggregate-zone-group__label">{label}</span>{" "}
            <Badge isRead>{count}</Badge>
          </span>
        }
      >
        {children}
      </ExpandableSection>
    </div>
  );
}
