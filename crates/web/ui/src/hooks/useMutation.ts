import { useState, useCallback, useRef } from "react";
import type { RefinementOp, ViewResponse } from "../api/types";
import { applyOp, undo as apiUndo, redo as apiRedo } from "../api/client";

export interface UseMutationResult {
  mutate: (op: RefinementOp) => void;
  undo: () => void;
  redo: () => void;
  isPending: boolean;
}

type QueueEntry =
  { kind: "op"; op: RefinementOp } | { kind: "undo" } | { kind: "redo" };

export function useMutation(
  onSuccess: (view: ViewResponse) => void,
  onError: (err: Error) => void,
): UseMutationResult {
  const [isPending, setIsPending] = useState(false);
  const queueRef = useRef<QueueEntry[]>([]);
  const processingRef = useRef(false);

  const processQueue = useCallback(async () => {
    if (processingRef.current) return;
    processingRef.current = true;
    setIsPending(true);

    while (queueRef.current.length > 0) {
      const entry = queueRef.current[0];
      try {
        let result: ViewResponse;
        if (entry.kind === "op") {
          result = await applyOp(entry.op);
        } else if (entry.kind === "undo") {
          result = await apiUndo();
        } else {
          result = await apiRedo();
        }
        queueRef.current.shift();
        onSuccess(result);
      } catch (err: unknown) {
        // Clear queue on error, revert all pending
        queueRef.current = [];
        onError(err instanceof Error ? err : new Error(String(err)));
        break;
      }
    }

    processingRef.current = false;
    setIsPending(false);
  }, [onSuccess, onError]);

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

  return { mutate, undo, redo, isPending };
}
