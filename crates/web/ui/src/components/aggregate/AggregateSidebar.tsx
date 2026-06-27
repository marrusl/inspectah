import { Nav, NavGroup, NavItem, Badge } from "@patternfly/react-core";
import type { AggregateSection } from "../../api/types";
import type { UseVariantAckResult } from "../../hooks/useVariantAck";

export interface AggregateSidebarProps {
  sections: AggregateSection[];
  activeSection: string;
  onSelect: (sectionId: string) => void;
  ackState: UseVariantAckResult;
  searchSlot?: React.ReactNode;
}

function sectionItemCount(section: AggregateSection): number {
  if (section.zones) {
    return (
      section.zones.consensus.count +
      section.zones.near_consensus.count +
      section.zones.divergent.count
    );
  }
  return section.items?.length ?? 0;
}

export function AggregateSidebar({
  sections,
  activeSection,
  onSelect,
  ackState: _ackState,
  searchSlot,
}: AggregateSidebarProps) {
  const reviewSections = sections.filter((s) => s.is_decision_section);
  const referenceSections = sections.filter((s) => !s.is_decision_section);

  const renderItem = (section: AggregateSection) => {
    return (
      <NavItem
        key={section.id}
        itemId={section.id}
        isActive={activeSection === section.id}
        aria-current={activeSection === section.id ? "page" : undefined}
        onClick={() => onSelect(section.id)}
      >
        {section.display_name} <Badge isRead>{sectionItemCount(section)}</Badge>
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
