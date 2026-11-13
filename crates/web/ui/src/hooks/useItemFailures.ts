import { useCallback, useRef, useState } from "react";

export interface ItemFailures<F> {
  /** The current failure of each item that has one. */
  failures: ReadonlyMap<string, F>;
  /**
   * Starts an attempt on `key`: clears its failure and returns the function
   * that records the attempt's own failure. That function records nothing
   * once the item's lifetime has ended, by a reset or by the item leaving
   * the view, so a request that settles late cannot put a failure on a
   * record it never changed.
   */
  begin: (key: string) => (failure: F) => void;
  /** Clears the failure on `key`, for an operator who has acknowledged it. */
  clear: (key: string) => void;
  /** Ends every item's lifetime and clears every failure. */
  clearAll: () => void;
  /** Ends the lifetime of every item not in `present` and drops its failure. */
  retain: (present: ReadonlySet<string>) => void;
}

/**
 * Failures of direct requests, by item, for an owner that outlives the
 * controls that send them. Each item has a lifetime, a number taken from
 * a counter when the first attempt on it begins and never reused: a reset
 * or the item leaving the view ends it, so an attempt from before cannot
 * match a later one.
 */
export function useItemFailures<F>(): ItemFailures<F> {
  const [failures, setFailures] = useState<ReadonlyMap<string, F>>(
    () => new Map(),
  );
  const lifetimes = useRef(new Map<string, number>());
  const lastLifetime = useRef(0);

  const set = useCallback((key: string, failure: F | undefined) => {
    setFailures((prev) => {
      if (failure === undefined && !prev.has(key)) return prev;
      const next = new Map(prev);
      if (failure === undefined) next.delete(key);
      else next.set(key, failure);
      return next;
    });
  }, []);

  const begin = useCallback(
    (key: string) => {
      let lifetime = lifetimes.current.get(key);
      if (lifetime === undefined) {
        lifetime = ++lastLifetime.current;
        lifetimes.current.set(key, lifetime);
      }
      set(key, undefined);
      return (failure: F) => {
        if (lifetimes.current.get(key) === lifetime) set(key, failure);
      };
    },
    [set],
  );

  const clear = useCallback((key: string) => set(key, undefined), [set]);

  const clearAll = useCallback(() => {
    lifetimes.current.clear();
    setFailures((prev) => (prev.size === 0 ? prev : new Map()));
  }, []);

  const retain = useCallback((present: ReadonlySet<string>) => {
    for (const key of lifetimes.current.keys()) {
      if (!present.has(key)) lifetimes.current.delete(key);
    }
    setFailures((prev) => {
      const gone = [...prev.keys()].filter((key) => !present.has(key));
      if (gone.length === 0) return prev;
      const next = new Map(prev);
      for (const key of gone) next.delete(key);
      return next;
    });
  }, []);

  return { failures, begin, clear, clearAll, retain };
}
