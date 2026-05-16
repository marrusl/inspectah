import { useCallback, useState, useRef, useEffect } from "react";
import {
  Alert,
  AlertGroup,
  AlertActionCloseButton,
  AlertVariant,
} from "@patternfly/react-core";
import type {
  RefinedPackage,
  RefinedConfig,
  AttentionLevel,
  RefinementOp,
  RefinedView,
} from "../api/types";
import { useMutation } from "../hooks/useMutation";
import { useViewed } from "../hooks/useViewed";
import { AttentionGroup } from "./AttentionGroup";
import { DecisionItem } from "./DecisionItem";
import type { DecisionItemKind } from "./DecisionItem";
import { highestAttention } from "./attentionUtils";

interface GroupedItems {
  needs_review: DecisionItemKind[];
  informational: DecisionItemKind[];
  routine: DecisionItemKind[];
}

function groupByAttention(items: DecisionItemKind[]): GroupedItems {
  const groups: GroupedItems = {
    needs_review: [],
    informational: [],
    routine: [],
  };
  for (const item of items) {
    const level =
      item.data.attention.length > 0
        ? highestAttention(item.data.attention)
        : "routine";
    groups[level].push(item);
  }
  return groups;
}

interface ToastEntry {
  id: number;
  message: string;
  variant: AlertVariant;
}

export interface DecisionListProps {
  items: DecisionItemKind[];
  sectionLabel: string;
  onViewUpdate: (view: RefinedView) => void;
  onMutationError: (err: Error) => void;
}

export function DecisionList({
  items,
  sectionLabel,
  onViewUpdate,
  onMutationError,
}: DecisionListProps) {
  const toastIdRef = useRef(0);
  const [toasts, setToasts] = useState<ToastEntry[]>([]);

  const handleSuccess = useCallback(
    (view: RefinedView) => {
      onViewUpdate(view);
    },
    [onViewUpdate],
  );

  const handleError = useCallback(
    (err: Error) => {
      const id = ++toastIdRef.current;
      const isNetwork =
        err.message.includes("fetch") ||
        err.message.includes("network") ||
        err.message.includes("Failed to fetch");
      const variant = isNetwork ? AlertVariant.danger : AlertVariant.warning;
      const message = isNetwork
        ? `Network error: ${err.message}`
        : `Error: ${err.message}`;
      setToasts((prev) => [...prev, { id, message, variant }]);

      // Auto-dismiss non-network errors after 3 seconds
      if (!isNetwork) {
        setTimeout(() => {
          setToasts((prev) => prev.filter((t) => t.id !== id));
        }, 3000);
      }

      onMutationError(err);
    },
    [onMutationError],
  );

  const mutation = useMutation(handleSuccess, handleError);
  const { viewedIds, markAsViewed } = useViewed();

  const dismissToast = useCallback((id: number) => {
    setToasts((prev) => prev.filter((t) => t.id !== id));
  }, []);

  const handleToggle = useCallback(
    (op: RefinementOp) => {
      mutation.mutate(op);
    },
    [mutation],
  );

  const grouped = groupByAttention(items);
  const levels: AttentionLevel[] = [
    "needs_review",
    "informational",
    "routine",
  ];

  // Clean up auto-dismiss timers
  const timerIds = useRef<number[]>([]);
  useEffect(() => {
    return () => {
      timerIds.current.forEach(clearTimeout);
    };
  }, []);

  return (
    <div
      role="grid"
      aria-label={`${sectionLabel} decisions`}
      data-testid={`decision-list-${sectionLabel.toLowerCase().replace(/\s+/g, "-")}`}
    >
      {toasts.length > 0 && (
        <AlertGroup isToast isLiveRegion>
          {toasts.map((toast) => (
            <Alert
              key={toast.id}
              variant={toast.variant}
              title={toast.message}
              actionClose={
                <AlertActionCloseButton
                  onClose={() => dismissToast(toast.id)}
                />
              }
            />
          ))}
        </AlertGroup>
      )}

      {levels.map((level) => {
        const groupItems = grouped[level];
        if (groupItems.length === 0) return null;
        return (
          <AttentionGroup key={level} level={level} count={groupItems.length}>
            {groupItems.map((item) => {
              const id =
                item.type === "package"
                  ? `pkg:${item.data.entry.name}:${(item.data as RefinedPackage).entry.arch}`
                  : `cfg:${(item.data as RefinedConfig).entry.path}`;
              return (
                <DecisionItem
                  key={id}
                  item={item}
                  level={level}
                  isViewed={viewedIds.has(id)}
                  isPending={mutation.isPending}
                  onToggleInclude={handleToggle}
                  onMarkViewed={markAsViewed}
                />
              );
            })}
          </AttentionGroup>
        );
      })}

      {items.length === 0 && (
        <div style={{ padding: "var(--pf-t--global--spacer--lg)", textAlign: "center", color: "var(--pf-t--global--color--200)" }}>
          No items in this section.
        </div>
      )}
    </div>
  );
}
