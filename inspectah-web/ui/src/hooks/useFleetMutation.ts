import { useState, useCallback, useRef } from "react";
import type { RefinementOp, FleetViewResponse } from "../api/types";
import { applyOp, undo as apiUndo, redo as apiRedo } from "../api/client";
import { fetchFleetView } from "../api/fleet-client";

export interface UseFleetMutationResult {
  mutate: (op: RefinementOp) => void;
  undo: () => void;
  redo: () => void;
  isPending: boolean;
  refetchError: string | null;
  retry: () => Promise<void>;
  lastConfirmedView: React.RefObject<FleetViewResponse | null>;
}

type QueueEntry =
  | { kind: "op"; op: RefinementOp }
  | { kind: "undo" }
  | { kind: "redo" };

export function useFleetMutation(
  onViewUpdate: (view: FleetViewResponse) => void,
  onError: (err: Error) => void,
): UseFleetMutationResult {
  const [isPending, setIsPending] = useState(false);
  const [refetchError, setRefetchError] = useState<string | null>(null);
  const lastConfirmedView = useRef<FleetViewResponse | null>(null);
  const queueRef = useRef<QueueEntry[]>([]);
  const processingRef = useRef(false);

  const processQueue = useCallback(async () => {
    if (processingRef.current) return;
    processingRef.current = true;
    setIsPending(true);
    setRefetchError(null);

    while (queueRef.current.length > 0) {
      const entry = queueRef.current[0];
      try {
        // Apply the mutation (ignore the ViewResponse — we refetch fleet view)
        if (entry.kind === "op") {
          await applyOp(entry.op);
        } else if (entry.kind === "undo") {
          await apiUndo();
        } else {
          await apiRedo();
        }
        queueRef.current.shift();

        // Re-fetch fleet-aggregated view
        try {
          const fleetView = await fetchFleetView();
          lastConfirmedView.current = fleetView;
          onViewUpdate(fleetView);
        } catch (refetchErr: unknown) {
          setRefetchError(
            refetchErr instanceof Error
              ? refetchErr.message
              : "View update failed",
          );
          queueRef.current = [];
          break;
        }
      } catch (err: unknown) {
        queueRef.current = [];
        onError(err instanceof Error ? err : new Error(String(err)));
        break;
      }
    }

    processingRef.current = false;
    setIsPending(false);
  }, [onViewUpdate, onError]);

  const enqueue = useCallback(
    (entry: QueueEntry) => {
      queueRef.current.push(entry);
      processQueue();
    },
    [processQueue],
  );

  const mutate = useCallback(
    (op: RefinementOp) => enqueue({ kind: "op", op }),
    [enqueue],
  );

  const undo = useCallback(() => enqueue({ kind: "undo" }), [enqueue]);

  const redo = useCallback(() => enqueue({ kind: "redo" }), [enqueue]);

  const retry = useCallback(async () => {
    setRefetchError(null);
    try {
      const fleetView = await fetchFleetView();
      lastConfirmedView.current = fleetView;
      onViewUpdate(fleetView);
    } catch (err: unknown) {
      setRefetchError(err instanceof Error ? err.message : "Retry failed");
    }
  }, [onViewUpdate]);

  return {
    mutate,
    undo,
    redo,
    isPending,
    refetchError,
    retry,
    lastConfirmedView,
  };
}
