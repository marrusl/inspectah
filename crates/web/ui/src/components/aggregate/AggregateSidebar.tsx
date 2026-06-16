import { Nav, NavGroup, NavItem, Badge } from "@patternfly/react-core";
import type { FleetSection } from "../../api/types";
import type { UseVariantAckResult } from "../../hooks/useVariantAck";

export interface AggregateSidebarProps {
  sections: FleetSection[];
  activeSection: string;
  onSelect: (sectionId: string) => void;
  ackState: UseVariantAckResult;
  searchSlot?: React.ReactNode;
}

function sectionItemCount(section: FleetSection): number {
  if (section.zones) {
    return (
      section.zones.consensus.count +
      section.zones.near_consensus.count +
      section.zones.divergent.count
    );
  }
  return section.items?.length ?? 0;
}

function ackLabel(
  section: FleetSection,
  ack: UseVariantAckResult,
): string | null {
  if (!section.is_decision_section) return null;
  if (ack.totalCount === 0) return null;
  const confirmed = ack.totalCount - ack.unackedCount;
  return `${confirmed}/${ack.totalCount} confirmed`;
}

export function AggregateSidebar({
  sections,
  activeSection,
  onSelect,
  ackState,
  searchSlot,
}: AggregateSidebarProps) {
  const reviewSections = sections.filter((s) => s.is_decision_section);
  const referenceSections = sections.filter((s) => !s.is_decision_section);

  const renderItem = (section: FleetSection) => {
    const ack = ackLabel(section, ackState);
    return (
      <NavItem
        key={section.id}
        itemId={section.id}
        isActive={activeSection === section.id}
        aria-current={activeSection === section.id ? "page" : undefined}
        onClick={() => onSelect(section.id)}
      >
        {section.display_name} <Badge isRead>{sectionItemCount(section)}</Badge>
        {ack && (
          <span
            className="aggregate-sidebar__ack-progress"
            data-testid={`ack-progress-${section.id}`}
          >
            {ack}
          </span>
        )}
      </NavItem>
    );
  };

  return (
    <nav
      className="inspectah-sidebar"
      aria-label="Aggregate section navigation"
      data-testid="aggregate-sidebar"
    >
      {searchSlot}
      <Nav aria-label="Aggregate sections">
        <NavGroup title="Review">{reviewSections.map(renderItem)}</NavGroup>
        {referenceSections.length > 0 && (
          <NavGroup title="Reference">
            {referenceSections.map(renderItem)}
          </NavGroup>
        )}
      </Nav>
    </nav>
  );
}
