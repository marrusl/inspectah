import { useState, useEffect, useRef, useCallback } from "react";
import type { ContextSection } from "../api/types";
import { fetchSections } from "../api/client";

export interface UseSectionsResult {
  data: ContextSection[] | null;
  loading: boolean;
  error: Error | null;
  refetch: () => void;
}

export function useSections(): UseSectionsResult {
  const [data, setData] = useState<ContextSection[] | null>(null);
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
    fetchSections()
      .then((sections) => {
        setData(sections);
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
