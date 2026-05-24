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

  return (
    <div role="row" className="inspectah-sort-header">
      <button
        ref={leftRef}
        role="columnheader"
        aria-sort={ariaSortValue("left", activeColumn, direction)}
        className="inspectah-sort-header__column"
        onClick={() => onSort("left")}
        onKeyDown={(e) => handleKeyDown(e, "left")}
      >
        {leftLabel}
        {activeColumn === "left" && (
          <span className="inspectah-sort-header__chevron" aria-hidden="true">
            {" "}{chevron(direction)}
          </span>
        )}
      </button>
      <button
        ref={rightRef}
        role="columnheader"
        aria-sort={ariaSortValue("right", activeColumn, direction)}
        className="inspectah-sort-header__column"
        onClick={() => onSort("right")}
        onKeyDown={(e) => handleKeyDown(e, "right")}
      >
        {rightLabel}
        {activeColumn === "right" && (
          <span className="inspectah-sort-header__chevron" aria-hidden="true">
            {" "}{chevron(direction)}
          </span>
        )}
      </button>
    </div>
  );
}
