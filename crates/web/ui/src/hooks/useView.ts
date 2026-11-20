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
   * another request, unless the view on screen has a newer generation.
   * Fetches still in flight are judged by generation when they answer, so
   * one carrying a newer generation still replaces this view.
   */
  publish: (view: ViewResponse) => void;
}

export function useView(): UseViewResult {
  const [data, setData] = useState<ViewResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<Error | null>(null);
  // Server generation of the view on screen; null until one is shown.
  const shownGenerationRef = useRef<number | null>(null);
  // Bumped by each fetch and each publish. A fetch sets `loading` and
  // `error` only while it is still the latest of either, and at an equal
  // generation a view replaces the one on screen only from the latest.
  const latestRef = useRef(0);
  const mountedRef = useRef(true);
  const [tick, setTick] = useState(0);

  // Generations order views, whichever way they arrive: an older one never
  // replaces a newer one. Equal generations can still differ, since some
  // changes (an RPM upload) do not move the generation, so a tie goes to
  // the latest fetch or publish.
  const show = useCallback((view: ViewResponse, isLatest: boolean) => {
    const shown = shownGenerationRef.current;
    if (
      shown !== null &&
      (view.generation < shown || (view.generation === shown && !isLatest))
    ) {
      return false;
    }
    shownGenerationRef.current = view.generation;
    setData(view);
    setError(null);
    return true;
  }, []);

  const refetch = useCallback(() => {
    setTick((t) => t + 1);
  }, []);

  const invalidate = useCallback(() => {
    setTick((t) => t + 1);
  }, []);

  const publish = useCallback(
    (view: ViewResponse) => {
      if (!show(view, true)) return;
      latestRef.current += 1;
      setLoading(false);
    },
    [show],
  );

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  useEffect(() => {
    const request = ++latestRef.current;
    const superseded = () => request !== latestRef.current;
    setLoading(true);

    fetchView()
      .then((view) => {
        if (mountedRef.current) show(view, !superseded());
      })
      .catch((err: unknown) => {
        if (!mountedRef.current || superseded()) return;
        setError(err instanceof Error ? err : new Error(String(err)));
      })
      .finally(() => {
        if (mountedRef.current && !superseded()) setLoading(false);
      });
  }, [tick, show]);

  return { data, loading, error, refetch, invalidate, publish };
}
