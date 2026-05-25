import { useCallback, useRef } from "react";

export interface SortHeaderProps {
  leftLabel: string;
  rightLabel: string;
  activeColumn: "left" | "right";
  direction: "asc" | "desc";
  onSort: (column: "left" | "right") => void;
}

function chevron(direction: "asc" | "desc"): string {
  return direction === "asc" ? "▲" : "▼";
}

function ariaSortValue(
  column: "left" | "right",
  activeColumn: "left" | "right",
  direction: "asc" | "desc",
): "ascending" | "descending" | "none" {
  if (column !== activeColumn) return "none";
  return direction === "asc" ? "ascending" : "descending";
}

export function SortHeader({
  leftLabel,
  rightLabel,
  activeColumn,
  direction,
  onSort,
}: SortHeaderProps) {
  const leftRef = useRef<HTMLButtonElement>(null);
  const rightRef = useRef<HTMLButtonElement>(null);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLButtonElement>, column: "left" | "right") => {
      if (e.key === "ArrowRight" || e.key === "ArrowLeft") {
        e.preventDefault();
        const target = column === "left" ? rightRef.current : leftRef.current;
        target?.focus();
      }
    },
    [],
  );

  const leftSortDir = ariaSortValue("left", activeColumn, direction);
  const rightSortDir = ariaSortValue("right", activeColumn, direction);

  function sortLabel(label: string, sortDir: "ascending" | "descending" | "none"): string {
    if (sortDir === "none") return `Sort by ${label}`;
    return `Sort by ${label}, currently ${sortDir === "ascending" ? "ascending" : "descending"}`;
  }

  return (
    <div role="grid" aria-label="Sort controls" className="inspectah-sort-header">
      <div role="row">
        <div role="columnheader" aria-sort={leftSortDir}>
          <button
            ref={leftRef}
            className="inspectah-sort-header__column"
            aria-label={sortLabel(leftLabel, leftSortDir)}
            aria-sort={leftSortDir}
            onClick={() => onSort("left")}
            onKeyDown={(e) => handleKeyDown(e, "left")}
          >
            {leftLabel}
            {activeColumn === "left" && (
              <span className="inspectah-sort-header__chevron" aria-hidden="true">
                {chevron(direction)}
              </span>
            )}
          </button>
        </div>
        <div role="columnheader" aria-sort={rightSortDir}>
          <button
            ref={rightRef}
            className="inspectah-sort-header__column"
            aria-label={sortLabel(rightLabel, rightSortDir)}
            aria-sort={rightSortDir}
            onClick={() => onSort("right")}
            onKeyDown={(e) => handleKeyDown(e, "right")}
          >
            {rightLabel}
            {activeColumn === "right" && (
              <span className="inspectah-sort-header__chevron" aria-hidden="true">
                {chevron(direction)}
              </span>
            )}
          </button>
        </div>
      </div>
    </div>
  );
}
