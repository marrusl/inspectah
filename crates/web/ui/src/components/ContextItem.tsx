import { useState, useCallback } from "react";
import { AngleRightIcon, AngleDownIcon } from "@patternfly/react-icons";
import type {
  ContextItem as ContextItemType,
  DeclinedView,
} from "../api/types";

export interface ContextItemProps {
  item: ContextItemType;
  /** Opens the unit row a scheduled timer row names. */
  onNavigateToUnit?: (unit: string) => void;
  /**
   * Why the export does not write the unit this row generates. The text
   * is the server's, shown as given.
   */
  declined?: DeclinedView;
}

export function ContextItem({
  item,
  onNavigateToUnit,
  declined,
}: ContextItemProps) {
  const [isExpanded, setIsExpanded] = useState(false);
  const hasDetail = item.detail != null && item.detail.trim().length > 0;
  const unit = item.unit;

  const handleToggle = useCallback(() => {
    setIsExpanded((prev) => !prev);
  }, []);

  return (
    <div
      role="listitem"
      data-testid={`context-item-${item.id}`}
      className="inspectah-context-row"
      tabIndex={-1}
    >
      <div
        className="inspectah-context-row__main"
        onClick={hasDetail ? handleToggle : undefined}
        style={hasDetail ? { cursor: "pointer" } : undefined}
      >
        <div
          id={`context-item-${item.id}`}
          className="inspectah-context-row__name"
        >
          <span>{item.title}</span>
          {item.subtitle && (
            <span className="inspectah-context-row__subtitle">
              {item.subtitle}
            </span>
          )}
        </div>
        {unit != null && (
          <div className="inspectah-context-row__unit">
            {item.unit_row && onNavigateToUnit ? (
              <a
                href={`#units:${unit}`}
                data-testid={`context-item-unit-${item.id}`}
                onClick={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  onNavigateToUnit(unit);
                }}
              >
                {unit}
              </a>
            ) : (
              <code data-testid={`context-item-unit-${item.id}`}>{unit}</code>
            )}
          </div>
        )}
        {hasDetail && (
          <button
            onClick={(e) => {
              e.stopPropagation();
              handleToggle();
            }}
            aria-label={isExpanded ? "Collapse detail" : "Expand detail"}
            aria-expanded={isExpanded}
            className="inspectah-decision-row__expand-btn"
          >
            {isExpanded ? <AngleDownIcon /> : <AngleRightIcon />}
          </button>
        )}
      </div>
      {declined && (
        <div
          id={`declined-generated-${item.id}`}
          data-testid={`declined-generated-${item.id}`}
          className={`inspectah-decision-row__helper-text inspectah-unit-note inspectah-unit-declined inspectah-unit-declined--${declined.renders}`}
        >
          {declined.text}
        </div>
      )}
      {hasDetail && isExpanded && (
        <pre className="inspectah-context-row__detail">{item.detail}</pre>
      )}
    </div>
  );
}
