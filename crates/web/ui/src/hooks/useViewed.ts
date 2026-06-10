import { useState, useEffect, useCallback, useRef } from "react";
import { fetchViewed, markViewed } from "../api/client";

export interface UseViewedResult {
  /** Set of item IDs that have been viewed. */
  viewedIds: Set<string>;
  /** Mark an item as viewed (fire-and-forget POST). */
  markAsViewed: (id: string) => void;
}

export function useViewed(onViewedChange?: () => void): UseViewedResult {
  const [viewedIds, setViewedIds] = useState<Set<string>>(new Set());
  const pendingRef = useRef<Set<string>>(new Set());
  const notifyTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    let cancelled = false;
    fetchViewed()
      .then(({ ids }) => {
        if (!cancelled) setViewedIds(new Set(ids));
      })
      .catch(() => {
        // Silently fail — viewed tracking is non-critical
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // Clean up debounce timer on unmount
  useEffect(() => {
    return () => {
      if (notifyTimerRef.current) clearTimeout(notifyTimerRef.current);
    };
  }, []);

  const markAsViewed = useCallback(
    (id: string) => {
      setViewedIds((prev) => {
        if (prev.has(id)) return prev;
        const next = new Set(prev);
        next.add(id);
        return next;
      });

      // Deduplicate in-flight POSTs
      if (pendingRef.current.has(id)) return;
      pendingRef.current.add(id);

      markViewed(id)
        .then(() => {
          // Notify App that viewed state changed (debounced)
          if (onViewedChange) {
            if (notifyTimerRef.current) clearTimeout(notifyTimerRef.current);
            notifyTimerRef.current = setTimeout(() => {
              onViewedChange();
              notifyTimerRef.current = null;
            }, 300);
          }
        })
        .catch(() => {
          // Non-critical — don't revert optimistic update
        })
        .finally(() => {
          pendingRef.current.delete(id);
        });
    },
    [onViewedChange],
  );

  return { viewedIds, markAsViewed };
}
