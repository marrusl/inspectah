import { useState, useCallback, useRef } from "react";
import type { AggregateDiffResponse, ItemId } from "../api/types";
import { fetchAggregateDiff } from "../api/aggregate-client";

export interface UseAggregateDiffResult {
  /** Fetch diff (returns cached if available). */
  fetchDiff: (itemId: ItemId, base: string, target: string) => Promise<void>;
  /** Current diff result (null before first fetch). */
  diff: AggregateDiffResponse | null;
  /** Loading state. */
  isLoading: boolean;
  /** Error message (null if no error). */
  error: string | null;
  /** Clear current diff (when closing drawer). */
  clearDiff: () => void;
}

export function useAggregateDiff(): UseAggregateDiffResult {
  const [diff, setDiff] = useState<AggregateDiffResponse | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const cacheRef = useRef<Map<string, AggregateDiffResponse>>(new Map());

  const fetchDiff = useCallback(
    async (itemId: ItemId, base: string, target: string): Promise<void> => {
      const cacheKey = `${JSON.stringify(itemId)}:${base}:${target}`;
      const cached = cacheRef.current.get(cacheKey);
      if (cached) {
        setDiff(cached);
        setError(null);
        return;
      }

      setIsLoading(true);
      try {
        const result = await fetchAggregateDiff({ item_id: itemId, base, target });
        cacheRef.current.set(cacheKey, result);
        setDiff(result);
        setError(null);
      } catch (err: unknown) {
        const message =
          err instanceof Error ? err.message : "Failed to fetch diff";
        setError(message);
      } finally {
        setIsLoading(false);
      }
    },
    [],
  );

  const clearDiff = useCallback(() => {
    setDiff(null);
  }, []);

  return { fetchDiff, diff, isLoading, error, clearDiff };
}
