import { useEffect, useRef, useCallback } from "react";

/**
 * Tracks the last focused fleet item (by data-item-id) and restores
 * focus after view updates (e.g. refetch). If the previously focused
 * item no longer exists, falls back to the first item with data-item-id.
 *
 * @param generation - The view generation counter; focus recovery runs
 *   whenever this changes (indicating a refetch).
 */
export function useFleetFocusRecovery(generation: number | null): void {
  const lastFocusedItemIdRef = useRef<string | null>(null);

  // Track focus changes — record the data-item-id of the focused element
  const handleFocusIn = useCallback((e: FocusEvent) => {
    const target = e.target;
    if (!(target instanceof HTMLElement)) return;

    // Walk up the DOM to find an element with data-item-id
    const itemEl = target.closest("[data-item-id]");
    if (itemEl) {
      lastFocusedItemIdRef.current = itemEl.getAttribute("data-item-id");
    }
  }, []);

  // Listen for focusin events to track the last focused item
  useEffect(() => {
    document.addEventListener("focusin", handleFocusIn);
    return () => {
      document.removeEventListener("focusin", handleFocusIn);
    };
  }, [handleFocusIn]);

  // Restore focus after generation changes (view refetch)
  useEffect(() => {
    if (generation === null) return;
    const savedId = lastFocusedItemIdRef.current;
    if (!savedId) return;

    // Use requestAnimationFrame to let the DOM settle after the state update
    requestAnimationFrame(() => {
      const el = document.querySelector(`[data-item-id='${CSS.escape(savedId)}']`);
      if (el) {
        (el as HTMLElement).focus();
      } else {
        // Item no longer exists — focus the first available item
        const firstItem = document.querySelector("[data-item-id]");
        if (firstItem) (firstItem as HTMLElement).focus();
      }
    });
  }, [generation]);
}
