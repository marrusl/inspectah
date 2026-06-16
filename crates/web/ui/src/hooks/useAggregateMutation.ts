import { useState, useCallback, useRef } from "react";
import type { RefinementOp, AggregateViewResponse } from "../api/types";
import { applyOp, undo as apiUndo, redo as apiRedo } from "../api/client";
import { fetchAggregateView } from "../api/aggregate-client";

export interface UseAggregateMutationResult {
  mutate: (op: RefinementOp) => void;
  undo: () => void;
  redo: () => void;
  isPending: boolean;
  refetchError: string | null;
  retry: () => Promise<void>;
  lastConfirmedView: React.RefObject<AggregateViewResponse | null>;
}

type QueueEntry =
  | { kind: "op"; op: RefinementOp }
  | { kind: "undo" }
  | { kind: "redo" };

export function useAggregateMutation(
  onViewUpdate: (view: AggregateViewResponse) => void,
  onError: (err: Error) => void,
): UseAggregateMutationResult {
  const [isPending, setIsPending] = useState(false);
  const [refetchError, setRefetchError] = useState<string | null>(null);
  const lastConfirmedView = useRef<AggregateViewResponse | null>(null);
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
        // Apply the mutation (ignore the ViewResponse — we refetch aggregate view)
        if (entry.kind === "op") {
          await applyOp(entry.op);
        } else if (entry.kind === "undo") {
          await apiUndo();
        } else {
          await apiRedo();
        }
        queueRef.current.shift();

        // Re-fetch aggregated view
        try {
          const aggregateView = await fetchAggregateView();
          lastConfirmedView.current = aggregateView;
          onViewUpdate(aggregateView);
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
      const aggregateView = await fetchAggregateView();
      lastConfirmedView.current = aggregateView;
      onViewUpdate(aggregateView);
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
