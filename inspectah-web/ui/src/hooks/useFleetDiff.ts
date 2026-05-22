import { useState, useCallback, useRef } from "react";
import type { FleetDiffResponse, ItemId } from "../api/types";
import { fetchFleetDiff } from "../api/fleet-client";

export interface UseFleetDiffResult {
  /** Fetch diff (returns cached if available). */
  fetchDiff: (itemId: ItemId, base: string, target: string) => Promise<void>;
  /** Current diff result (null before first fetch). */
  diff: FleetDiffResponse | null;
  /** Loading state. */
  isLoading: boolean;
  /** Error message (null if no error). */
  error: string | null;
  /** Clear current diff (when closing drawer). */
  clearDiff: () => void;
}

export function useFleetDiff(): UseFleetDiffResult {
  const [diff, setDiff] = useState<FleetDiffResponse | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const cacheRef = useRef<Map<string, FleetDiffResponse>>(new Map());

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
        const result = await fetchFleetDiff({ item_id: itemId, base, target });
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
