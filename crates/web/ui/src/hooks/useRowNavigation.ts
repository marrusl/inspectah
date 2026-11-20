import { useCallback } from "react";

/**
 * The row index a list navigation key moves to, or `null` for any other
 * key: j / ArrowDown next, k / ArrowUp previous (both wrap), g first,
 * G last. These are the Navigation keys the `?` overlay lists.
 */
export function rowNavigationTarget(
  key: string,
  current: number,
  total: number,
): number | null {
  if (total === 0) return null;
  if (key === "ArrowDown" || key === "j") return (current + 1) % total;
  if (key === "ArrowUp" || key === "k") return (current - 1 + total) % total;
  if (key === "g") return 0;
  if (key === "G") return total - 1;
  return null;
}

/**
 * Whether a keydown carries Ctrl, Meta or Alt. Row keys leave such presses
 * to the browser or the app (Ctrl+K is global search). Shift does not
 * count: `G` needs it.
 */
export function hasRowKeyModifier(e: React.KeyboardEvent): boolean {
  return e.ctrlKey || e.metaKey || e.altKey;
}

/**
 * Whether a keydown toggles a row: x or Space, with none of the Ctrl, Meta
 * or Alt modifiers the navigation keys also leave alone, so Ctrl+X and the
 * like never change a decision.
 */
export function isRowToggleKey(e: React.KeyboardEvent): boolean {
  return (e.key === " " || e.key === "x") && !hasRowKeyModifier(e);
}

/**
 * A keydown handler for a list container whose rows are the elements
 * matching `rowSelector`, in document order. A navigation key pressed on a
 * row focuses the row it names. Keys pressed on a control inside a row, or
 * with Ctrl, Meta or Alt held (Ctrl+K is global search), are left alone.
 */
export function useRowNavigation(rowSelector: string) {
  return useCallback(
    (e: React.KeyboardEvent<HTMLElement>) => {
      if (e.defaultPrevented || hasRowKeyModifier(e)) return;
      const rows = Array.from(
        e.currentTarget.querySelectorAll<HTMLElement>(rowSelector),
      );
      const current = rows.indexOf(e.target as HTMLElement);
      if (current === -1) return;
      const next = rowNavigationTarget(e.key, current, rows.length);
      if (next === null) return;
      e.preventDefault();
      rows[next].focus();
    },
    [rowSelector],
  );
}
