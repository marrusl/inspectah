import { useState, useEffect, useRef } from "react";
import type { ContextSection } from "../api/types";
import { fetchSections } from "../api/client";

export interface UseSectionsResult {
  data: ContextSection[] | null;
  loading: boolean;
  error: Error | null;
}

export function useSections(): UseSectionsResult {
  const [data, setData] = useState<ContextSection[] | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<Error | null>(null);
  const fetched = useRef(false);

  useEffect(() => {
    if (fetched.current) return;
    fetched.current = true;

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
  }, []);

  return { data, loading, error };
}
