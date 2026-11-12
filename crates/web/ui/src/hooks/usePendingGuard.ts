import { useCallback, useRef, useState } from "react";

export interface PendingGuard {
  /** Keys whose request is in flight, for disabling their controls. */
  pendingKeys: ReadonlySet<string>;
  /**
   * Starts `request` unless one for `key` is already in flight. The key is
   * held until the request settles, on success and on failure alike, so the
   * caller's own `.then`/`.catch` belong inside `request`.
   */
  runGuarded: (key: string, request: () => Promise<unknown>) => void;
}

/**
 * Per-item guard for requests sent outside `useMutation`. The ref is the
 * guard: it is checked and set before the request leaves, so a second click
 * that lands before React re-renders still sees it. The state copy is what
 * disables the control.
 */
export function usePendingGuard(): PendingGuard {
  const inFlight = useRef(new Set<string>());
  const [pendingKeys, setPendingKeys] = useState<ReadonlySet<string>>(
    () => new Set(),
  );

  const runGuarded = useCallback(
    (key: string, request: () => Promise<unknown>) => {
      if (inFlight.current.has(key)) return;
      inFlight.current.add(key);
      setPendingKeys(new Set(inFlight.current));
      request().finally(() => {
        inFlight.current.delete(key);
        setPendingKeys(new Set(inFlight.current));
      });
    },
    [],
  );

  return { pendingKeys, runGuarded };
}
