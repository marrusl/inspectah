import { useState, useEffect, useCallback, useRef } from "react";
import { fetchViewed, markViewed } from "../api/client";

export interface UseViewedResult {
  /** Set of item IDs that have been viewed. */
  viewedIds: Set<string>;
  /** Mark an item as viewed (fire-and-forget POST). */
  markAsViewed: (id: string) => void;
}

export function useViewed(): UseViewedResult {
  const [viewedIds, setViewedIds] = useState<Set<string>>(new Set());
  const pendingRef = useRef<Set<string>>(new Set());

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

  const markAsViewed = useCallback((id: string) => {
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
      .catch(() => {
        // Non-critical — don't revert optimistic update
      })
      .finally(() => {
        pendingRef.current.delete(id);
      });
  }, []);

  return { viewedIds, markAsViewed };
}
