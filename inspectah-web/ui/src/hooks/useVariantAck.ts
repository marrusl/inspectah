import { useState, useCallback, useMemo } from "react";
import type { ItemId } from "../api/types";

type AckStatus = "unreviewed" | "confirmed" | "changed";

export interface UseVariantAckResult {
  /** Check if a specific item is acked (confirmed or changed). */
  isAcked: (itemId: ItemId) => boolean;
  /** Get the ack status for a specific item. */
  getStatus: (itemId: ItemId) => AckStatus;
  /** Confirm an item (operator explicitly acknowledges current selection). */
  confirm: (itemId: ItemId) => void;
  /** Mark as changed (auto-called when variant selection changes). */
  markChanged: (itemId: ItemId) => void;
  /** Count of unacked items (for banner/toolbar display). */
  unackedCount: number;
  /** Total actionable items. */
  totalCount: number;
}

function itemKey(id: ItemId): string {
  return JSON.stringify(id);
}

function loadFromStorage(
  storageKey: string,
  actionableKeys: Set<string>,
): Map<string, AckStatus> {
  const map = new Map<string, AckStatus>();
  try {
    const raw = localStorage.getItem(storageKey);
    if (raw) {
      const parsed = JSON.parse(raw) as Record<string, string>;
      for (const [k, v] of Object.entries(parsed)) {
        if (actionableKeys.has(k) && (v === "confirmed" || v === "changed")) {
          map.set(k, v);
        }
      }
    }
  } catch {
    // Corrupt localStorage — start fresh
  }
  return map;
}

function persistToStorage(
  storageKey: string,
  ackMap: Map<string, AckStatus>,
): void {
  const obj: Record<string, string> = {};
  for (const [k, v] of ackMap) {
    obj[k] = v;
  }
  try {
    localStorage.setItem(storageKey, JSON.stringify(obj));
  } catch {
    // Storage full or unavailable — non-critical
  }
}

export function useVariantAck(
  fleetLabel: string,
  mergedAt: string,
  actionableIds: ItemId[],
): UseVariantAckResult {
  const storageKey = `fleet-ack:${fleetLabel}:${mergedAt}`;

  const actionableKeys = useMemo(
    () => new Set(actionableIds.map(itemKey)),
    [actionableIds],
  );

  const [ackMap, setAckMap] = useState<Map<string, AckStatus>>(() =>
    loadFromStorage(storageKey, actionableKeys),
  );

  const getStatus = useCallback(
    (id: ItemId): AckStatus => ackMap.get(itemKey(id)) ?? "unreviewed",
    [ackMap],
  );

  const isAcked = useCallback(
    (id: ItemId): boolean => {
      const status = ackMap.get(itemKey(id));
      return status === "confirmed" || status === "changed";
    },
    [ackMap],
  );

  const updateItem = useCallback(
    (id: ItemId, status: AckStatus) => {
      setAckMap((prev) => {
        const next = new Map(prev);
        next.set(itemKey(id), status);
        persistToStorage(storageKey, next);
        return next;
      });
    },
    [storageKey],
  );

  const confirm = useCallback(
    (id: ItemId) => updateItem(id, "confirmed"),
    [updateItem],
  );

  const markChanged = useCallback(
    (id: ItemId) => updateItem(id, "changed"),
    [updateItem],
  );

  const unackedCount = useMemo(() => {
    let count = 0;
    for (const key of actionableKeys) {
      const status = ackMap.get(key);
      if (status !== "confirmed" && status !== "changed") {
        count++;
      }
    }
    return count;
  }, [ackMap, actionableKeys]);

  const totalCount = actionableIds.length;

  return { isAcked, getStatus, confirm, markChanged, unackedCount, totalCount };
}
