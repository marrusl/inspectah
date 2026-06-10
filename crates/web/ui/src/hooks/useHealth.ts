import { useState, useEffect, useRef, useCallback } from "react";
import type { HealthResponse } from "../api/types";
import { fetchHealth } from "../api/client";

export interface UseHealthResult {
  data: HealthResponse | null;
  loading: boolean;
  error: Error | null;
  refetch: () => void;
}

export function useHealth(): UseHealthResult {
  const [data, setData] = useState<HealthResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<Error | null>(null);
  const [tick, setTick] = useState(0);
  const initialFetch = useRef(false);

  const refetch = useCallback(() => {
    setTick((t) => t + 1);
  }, []);

  useEffect(() => {
    // Skip only the duplicate strict-mode mount, not manual refetches
    if (tick === 0 && initialFetch.current) return;
    initialFetch.current = true;

    setLoading(true);
    fetchHealth()
      .then((health) => {
        setData(health);
        setError(null);
      })
      .catch((err: unknown) => {
        setError(err instanceof Error ? err : new Error(String(err)));
      })
      .finally(() => {
        setLoading(false);
      });
  }, [tick]);

  return { data, loading, error, refetch };
}
