import { useState, useEffect, useCallback, useRef } from "react";
import type { RefinedView } from "../api/types";
import { fetchView } from "../api/client";

export interface UseViewResult {
  data: RefinedView | null;
  loading: boolean;
  error: Error | null;
  refetch: () => void;
  /** Bump to trigger a refetch from mutation hooks. */
  invalidate: () => void;
}

export function useView(): UseViewResult {
  const [data, setData] = useState<RefinedView | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<Error | null>(null);
  const generationRef = useRef(0);
  const [tick, setTick] = useState(0);

  const refetch = useCallback(() => {
    setTick((t) => t + 1);
  }, []);

  const invalidate = useCallback(() => {
    setTick((t) => t + 1);
  }, []);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);

    fetchView()
      .then((view) => {
        if (cancelled) return;
        generationRef.current = view.generation;
        setData(view);
        setError(null);
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setError(err instanceof Error ? err : new Error(String(err)));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [tick]);

  return { data, loading, error, refetch, invalidate };
}
