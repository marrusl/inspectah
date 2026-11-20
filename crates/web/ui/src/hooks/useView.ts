import { useState, useEffect, useCallback, useRef } from "react";
import type { ViewResponse } from "../api/types";
import { fetchView } from "../api/client";

export interface UseViewResult {
  data: ViewResponse | null;
  loading: boolean;
  error: Error | null;
  refetch: () => void;
  /** Bump to trigger a refetch from mutation hooks. */
  invalidate: () => void;
  /**
   * Show a view the caller already fetched, in the same render, without
   * another request. A fetch still in flight is dropped so it cannot
   * replace this view with one it read earlier.
   */
  publish: (view: ViewResponse) => void;
}

export function useView(): UseViewResult {
  const [data, setData] = useState<ViewResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<Error | null>(null);
  const generationRef = useRef(0);
  // Bumped by each fetch and each publish; a fetch applies its result only
  // while it is still the latest of either.
  const latestRef = useRef(0);
  const [tick, setTick] = useState(0);

  const refetch = useCallback(() => {
    setTick((t) => t + 1);
  }, []);

  const invalidate = useCallback(() => {
    setTick((t) => t + 1);
  }, []);

  const publish = useCallback((view: ViewResponse) => {
    latestRef.current += 1;
    generationRef.current = view.generation;
    setData(view);
    setError(null);
    setLoading(false);
  }, []);

  useEffect(() => {
    let cancelled = false;
    const request = ++latestRef.current;
    const superseded = () => cancelled || request !== latestRef.current;
    setLoading(true);

    fetchView()
      .then((view) => {
        if (superseded()) return;
        generationRef.current = view.generation;
        setData(view);
        setError(null);
      })
      .catch((err: unknown) => {
        if (superseded()) return;
        setError(err instanceof Error ? err : new Error(String(err)));
      })
      .finally(() => {
        if (!superseded()) setLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [tick]);

  return { data, loading, error, refetch, invalidate, publish };
}
