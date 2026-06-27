import { Nav, NavGroup, NavItem, Badge } from "@patternfly/react-core";
import type { AggregateSection, AggregateItem } from "../../api/types";
import type { UseVariantAckResult } from "../../hooks/useVariantAck";

export interface AggregateSidebarProps {
  sections: AggregateSection[];
  activeSection: string;
  onSelect: (sectionId: string) => void;
  ackState: UseVariantAckResult;
  searchSlot?: React.ReactNode;
}

/** Collect all items from a section (flat or zoned). */
function allItems(section: AggregateSection): AggregateItem[] {
  if (section.zones) {
    return [
      ...section.zones.consensus.items,
      ...section.zones.near_consensus.items,
      ...section.zones.divergent.items,
    ];
  }
  return section.items ?? [];
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

/** Format badge text: "N included / M" for decision sections, "N" for reference. */
function badgeText(section: AggregateSection): string {
  const total = sectionItemCount(section);
  if (!section.is_decision_section) return String(total);
  const included = allItems(section).filter((item) => item.include).length;
  return `${included} / ${total}`;
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
        {section.display_name} <Badge isRead>{badgeText(section)}</Badge>
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
