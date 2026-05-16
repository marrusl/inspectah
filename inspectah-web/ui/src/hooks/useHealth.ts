import { useState, useEffect, useRef } from "react";
import type { HealthResponse } from "../api/types";
import { fetchHealth } from "../api/client";

export interface UseHealthResult {
  data: HealthResponse | null;
  loading: boolean;
  error: Error | null;
}

export function useHealth(): UseHealthResult {
  const [data, setData] = useState<HealthResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<Error | null>(null);
  const fetched = useRef(false);

  useEffect(() => {
    if (fetched.current) return;
    fetched.current = true;

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
  }, []);

  return { data, loading, error };
}
