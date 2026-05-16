import { DataList, EmptyState, EmptyStateBody } from "@patternfly/react-core";
import { CubesIcon } from "@patternfly/react-icons";
import type { ContextSection } from "../api/types";
import { ContextItem } from "./ContextItem";

export interface ContextListProps {
  section: ContextSection;
}

export function ContextList({ section }: ContextListProps) {
  if (section.items.length === 0) {
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
  );
}
