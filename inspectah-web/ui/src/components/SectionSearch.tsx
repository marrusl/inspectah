import { useRef, useEffect, useCallback } from "react";
import { SearchInput } from "@patternfly/react-core";

export interface SectionSearchProps {
  /** Current filter text. */
  value: string;
  /** Called when the user types or clears the filter. */
  onChange: (value: string) => void;
  /** Called when the user presses Escape — parent should close the search. */
  onClose: () => void;
  /** Called when the user presses ArrowDown — parent should focus the first matching item. */
  onArrowDown: () => void;
  /** Count of matching items shown to the user. */
  resultCount: number;
}

/**
 * Inline search input displayed above a section's item list.
 * Opened by pressing `/` (handled by useKeyboard), closed by `Escape`.
 */
export function SectionSearch({
  value,
  onChange,
  onClose,
  onArrowDown,
  resultCount,
}: SectionSearchProps) {
  const inputRef = useRef<HTMLInputElement>(null);

  // Auto-focus on mount
  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onClose();
        return;
      }
      if (e.key === "ArrowDown") {
        e.preventDefault();
        onArrowDown();
        return;
      }
    },
    [onClose, onArrowDown],
  );

  return (
    <div
      data-testid="section-search"
      style={{ marginBottom: "var(--pf-t--global--spacer--sm)" }}
    >
      <SearchInput
        ref={inputRef}
        placeholder="Filter items..."
        value={value}
        onChange={(_e, val) => onChange(val)}
        onClear={() => onChange("")}
        onKeyDown={handleKeyDown}
        resultsCount={resultCount > 0 ? `${resultCount} match${resultCount === 1 ? "" : "es"}` : undefined}
        aria-label="Filter section items"
        data-testid="section-search-input"
      />
    </div>
  );
}
