import { DataList, EmptyState, EmptyStateBody, Title } from "@patternfly/react-core";
import { CubesIcon } from "@patternfly/react-icons";
import type { ContextSection } from "../api/types";
import { ContextItem } from "./ContextItem";

export interface ContextListProps {
  section: ContextSection;
}

export function ContextList({ section }: ContextListProps) {
  const subsections = (section.subsections ?? []).filter((sub) => sub.items.length > 0);
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
        <DataList
          aria-label={`${section.display_name} context items`}
          style={{
            borderLeft: "3px solid var(--pf-t--global--border--color--default)",
            marginTop: "var(--pf-t--global--spacer--md)",
          }}
        >
          {section.items.map((item) => (
            <ContextItem key={item.id} item={item} />
          ))}
        </DataList>
      )}

      {subsections.map((sub) => (
        <div key={sub.id} style={{ marginTop: "var(--pf-t--global--spacer--lg)" }}>
          <Title headingLevel="h3" size="lg">
            {sub.display_name}
          </Title>
          <DataList
            aria-label={`${sub.display_name} context items`}
            style={{
              borderLeft: "3px solid var(--pf-t--global--border--color--default)",
              marginTop: "var(--pf-t--global--spacer--md)",
            }}
          >
            {sub.items.map((item) => (
              <ContextItem key={item.id} item={item} />
            ))}
          </DataList>
        </div>
      ))}
    </>
  );
}
