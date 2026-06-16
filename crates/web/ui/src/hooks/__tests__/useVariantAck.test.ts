import { describe, it, expect, beforeEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useVariantAck } from "../useVariantAck";
import type { ItemId } from "../../api/types";

// Provide a localStorage stub when jsdom doesn't expose one
function createStorageStub(): Storage {
  let store: Record<string, string> = {};
  return {
    getItem: (key: string) => store[key] ?? null,
    setItem: (key: string, value: string) => {
      store[key] = value;
    },
    removeItem: (key: string) => {
      delete store[key];
    },
    clear: () => {
      store = {};
    },
    get length() {
      return Object.keys(store).length;
    },
    key: (index: number) => Object.keys(store)[index] ?? null,
  };
}

if (typeof globalThis.localStorage === "undefined") {
  Object.defineProperty(globalThis, "localStorage", {
    value: createStorageStub(),
    writable: true,
  });
}

const AGGREGATE_LABEL = "test-aggregate";
const MERGED_AT = "2026-05-21T12:00:00Z";
const STORAGE_KEY = `aggregate-ack:${AGGREGATE_LABEL}:${MERGED_AT}`;

const ITEM_A: ItemId = { kind: "Config", key: { path: "/etc/foo.conf" } };
const ITEM_B: ItemId = { kind: "Config", key: { path: "/etc/bar.conf" } };
const ITEM_C: ItemId = {
  kind: "Package",
  key: { name: "vim", arch: "x86_64" },
};

beforeEach(() => {
  localStorage.clear();
});

describe("useVariantAck", () => {
  it("starts with all items unreviewed", () => {
    const { result } = renderHook(() =>
      useVariantAck(AGGREGATE_LABEL, MERGED_AT, [ITEM_A, ITEM_B]),
    );

    expect(result.current.getStatus(ITEM_A)).toBe("unreviewed");
    expect(result.current.getStatus(ITEM_B)).toBe("unreviewed");
    expect(result.current.isAcked(ITEM_A)).toBe(false);
    expect(result.current.isAcked(ITEM_B)).toBe(false);
    expect(result.current.unackedCount).toBe(2);
    expect(result.current.totalCount).toBe(2);
  });

  it("confirm() marks item as confirmed", () => {
    const { result } = renderHook(() =>
      useVariantAck(AGGREGATE_LABEL, MERGED_AT, [ITEM_A, ITEM_B]),
    );

    act(() => {
      result.current.confirm(ITEM_A);
    });

    expect(result.current.getStatus(ITEM_A)).toBe("confirmed");
    expect(result.current.getStatus(ITEM_B)).toBe("unreviewed");
  });

  it("markChanged() marks item as changed", () => {
    const { result } = renderHook(() =>
      useVariantAck(AGGREGATE_LABEL, MERGED_AT, [ITEM_A]),
    );

    act(() => {
      result.current.markChanged(ITEM_A);
    });

    expect(result.current.getStatus(ITEM_A)).toBe("changed");
  });

  it("isAcked returns true for confirmed and changed items", () => {
    const { result } = renderHook(() =>
      useVariantAck(AGGREGATE_LABEL, MERGED_AT, [ITEM_A, ITEM_B, ITEM_C]),
    );

    act(() => {
      result.current.confirm(ITEM_A);
      result.current.markChanged(ITEM_B);
    });

    expect(result.current.isAcked(ITEM_A)).toBe(true);
    expect(result.current.isAcked(ITEM_B)).toBe(true);
    expect(result.current.isAcked(ITEM_C)).toBe(false);
  });

  it("unackedCount decrements when item is acked", () => {
    const { result } = renderHook(() =>
      useVariantAck(AGGREGATE_LABEL, MERGED_AT, [ITEM_A, ITEM_B, ITEM_C]),
    );

    expect(result.current.unackedCount).toBe(3);

    act(() => {
      result.current.confirm(ITEM_A);
    });
    expect(result.current.unackedCount).toBe(2);

    act(() => {
      result.current.markChanged(ITEM_B);
    });
    expect(result.current.unackedCount).toBe(1);
  });

  it("persists state to localStorage", () => {
    const { result } = renderHook(() =>
      useVariantAck(AGGREGATE_LABEL, MERGED_AT, [ITEM_A, ITEM_B]),
    );

    act(() => {
      result.current.confirm(ITEM_A);
      result.current.markChanged(ITEM_B);
    });

    const stored = JSON.parse(localStorage.getItem(STORAGE_KEY)!);
    expect(stored[JSON.stringify(ITEM_A)]).toBe("confirmed");
    expect(stored[JSON.stringify(ITEM_B)]).toBe("changed");
  });

  it("restores state from localStorage on mount", () => {
    const seed: Record<string, string> = {
      [JSON.stringify(ITEM_A)]: "confirmed",
      [JSON.stringify(ITEM_B)]: "changed",
    };
    localStorage.setItem(STORAGE_KEY, JSON.stringify(seed));

    const { result } = renderHook(() =>
      useVariantAck(AGGREGATE_LABEL, MERGED_AT, [ITEM_A, ITEM_B]),
    );

    expect(result.current.getStatus(ITEM_A)).toBe("confirmed");
    expect(result.current.getStatus(ITEM_B)).toBe("changed");
    expect(result.current.unackedCount).toBe(0);
  });

  it("scopes storage key to aggregate label and mergedAt", () => {
    const { result: r1 } = renderHook(() =>
      useVariantAck("aggregate-a", "2026-01-01", [ITEM_A]),
    );
    const { result: r2 } = renderHook(() =>
      useVariantAck("aggregate-b", "2026-01-01", [ITEM_A]),
    );

    act(() => {
      r1.current.confirm(ITEM_A);
    });

    expect(r1.current.isAcked(ITEM_A)).toBe(true);
    expect(r2.current.isAcked(ITEM_A)).toBe(false);

    expect(localStorage.getItem("aggregate-ack:aggregate-a:2026-01-01")).toBeTruthy();
    expect(localStorage.getItem("aggregate-ack:aggregate-b:2026-01-01")).toBeNull();
  });

  it("ignores localStorage items not in actionableIds", () => {
    const seed: Record<string, string> = {
      [JSON.stringify(ITEM_A)]: "confirmed",
      [JSON.stringify(ITEM_B)]: "changed",
      [JSON.stringify(ITEM_C)]: "confirmed",
    };
    localStorage.setItem(STORAGE_KEY, JSON.stringify(seed));

    // Only ITEM_A and ITEM_B are actionable; ITEM_C should be ignored
    const { result } = renderHook(() =>
      useVariantAck(AGGREGATE_LABEL, MERGED_AT, [ITEM_A, ITEM_B]),
    );

    expect(result.current.getStatus(ITEM_A)).toBe("confirmed");
    expect(result.current.getStatus(ITEM_B)).toBe("changed");
    expect(result.current.totalCount).toBe(2);
    expect(result.current.unackedCount).toBe(0);
  });
});
