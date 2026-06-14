import { EmptyState, EmptyStateBody } from "@patternfly/react-core";
import { CubesIcon } from "@patternfly/react-icons";
import type { ReferenceSection } from "../api/types";
import { ContextItem } from "./ContextItem";

export interface ContextListProps {
  section: ReferenceSection;
}

export function ContextList({ section }: ContextListProps) {
  const subsections = (section.subsections ?? []).filter(
    (sub) => sub.items.length > 0,
  );
  const hasAnyItems = section.items.length > 0 || subsections.length > 0;

  if (!hasAnyItems) {
    return (
      <EmptyState
        titleText={`No ${section.display_name} data in this snapshot`}
        icon={CubesIcon}
        headingLevel="h3"
      >
        <EmptyStateBody>
          This section contains no items from the scanned host.
        </EmptyStateBody>
      </EmptyState>
    );
  }

  return (
    <>
      {section.items.length > 0 && (
        <div role="list" aria-label={`${section.display_name} context items`}>
          {section.items.map((item) => (
            <ContextItem key={item.id} item={item} />
          ))}
        </div>
      )}

      {subsections.map((sub) => (
        <section
          key={sub.id}
          className="inspectah-context-subsection"
          aria-labelledby={`subsection-${sub.id}`}
        >
          <h4
            id={`subsection-${sub.id}`}
            className="inspectah-context-subsection__label"
          >
            {sub.display_name}
          </h4>
          <div role="list" aria-label={`${sub.display_name} context items`}>
            {sub.items.map((item) => (
              <ContextItem key={item.id} item={item} />
            ))}
          </div>
        </section>
      ))}
    </>
  );
}
