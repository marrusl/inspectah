import { useState, useCallback } from "react";
import {
  DataListItem,
  DataListItemRow,
  DataListItemCells,
  DataListCell,
  DataListContent,
} from "@patternfly/react-core";
import { AngleRightIcon, AngleDownIcon } from "@patternfly/react-icons";
import type { ContextItem as ContextItemType } from "../api/types";

export interface ContextItemProps {
  item: ContextItemType;
}

export function ContextItem({ item }: ContextItemProps) {
  const [isExpanded, setIsExpanded] = useState(false);
  const hasDetail = item.detail !== null;

  const handleToggle = useCallback(() => {
    setIsExpanded((prev) => !prev);
  }, []);

  return (
    <DataListItem aria-labelledby={`context-item-${item.id}`}>
      <DataListItemRow>
        <DataListItemCells
          dataListCells={[
            <DataListCell key="primary" width={5}>
              <div id={`context-item-${item.id}`}>
                <strong>{item.title}</strong>
                {item.subtitle && (
                  <div style={{ color: "var(--pf-t--global--color--200)", fontSize: "var(--pf-t--global--font--size--sm)" }}>
                    {item.subtitle}
                  </div>
                )}
              </div>
            </DataListCell>,
            hasDetail && (
              <DataListCell key="expand" width={1}>
                <button
                  onClick={handleToggle}
                  aria-label={isExpanded ? "Collapse detail" : "Expand detail"}
                  aria-expanded={isExpanded}
                  style={{
                    background: "none",
                    border: "none",
                    cursor: "pointer",
                    padding: "4px",
                    display: "flex",
                    alignItems: "center",
                  }}
                >
                  {isExpanded ? <AngleDownIcon /> : <AngleRightIcon />}
                </button>
              </DataListCell>
            ),
          ].filter(Boolean)}
        />
      </DataListItemRow>
      {hasDetail && isExpanded && (
        <DataListContent
          aria-label="Detail content"
          isHidden={!isExpanded}
        >
          <pre
            style={{
              whiteSpace: "pre-wrap",
              fontFamily: "var(--pf-t--global--font--family--mono)",
              fontSize: "var(--pf-t--global--font--size--sm)",
              color: "var(--pf-t--global--color--200)",
              margin: 0,
              padding: "var(--pf-t--global--spacer--md)",
              backgroundColor: "var(--pf-t--global--color--nonstatus--gray--100)",
            }}
          >
            {item.detail}
          </pre>
        </DataListContent>
      )}
    </DataListItem>
  );
}
