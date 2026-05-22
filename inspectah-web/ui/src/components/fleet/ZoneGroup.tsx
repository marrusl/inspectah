import { useState } from "react";
import { ExpandableSection, Badge } from "@patternfly/react-core";

export interface ZoneGroupProps {
  zone: "consensus" | "near_consensus" | "divergent";
  count: number;
  defaultExpanded: boolean;
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
  children,
}: ZoneGroupProps) {
  const [isExpanded, setIsExpanded] = useState(defaultExpanded);
  const label = ZONE_LABELS[zone];

  return (
    <div className={`fleet-zone-group fleet-zone-group--${zone}`} data-testid={`zone-${zone}`}>
      <ExpandableSection
        isExpanded={isExpanded}
        onToggle={(_event, expanded) => setIsExpanded(expanded)}
        toggleContent={
          <span className="fleet-zone-group__header">
            <span className="fleet-zone-group__label">{label}</span>{" "}
            <Badge isRead>{count}</Badge>
          </span>
        }
      >
        {children}
      </ExpandableSection>
    </div>
  );
}
