import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useFleetDiff } from "../useFleetDiff";
import type { FleetDiffResponse, ItemId } from "../../api/types";

const MOCK_DIFF: FleetDiffResponse = {
  base_hash: "aaa111",
  target_hash: "bbb222",
  base_hosts: ["host-a"],
  target_hosts: ["host-b"],
  hunks: [
    {
      base_range: { start: 1, count: 1 },
      target_range: { start: 1, count: 1 },
      changes: [
        { kind: "delete", content: "old-line" },
        { kind: "insert", content: "new-line" },
      ],
    },
  ],
  stats: { total_changes: 2, insertions: 1, deletions: 1 },
};

const CONFIG_ITEM: ItemId = { kind: "Config", key: { path: "/etc/foo.conf" } };
const PKG_ITEM: ItemId = { kind: "Package", key: { name: "httpd", arch: "x86_64" } };

beforeEach(() => {
  vi.restoreAllMocks();
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("useFleetDiff", () => {
  it("fetches diff from API on cache miss", async () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: true,
      json: () => Promise.resolve(MOCK_DIFF),
    } as Response);

    const { result } = renderHook(() => useFleetDiff());

    await act(async () => {
      await result.current.fetchDiff(CONFIG_ITEM, "aaa111", "bbb222");
    });

    expect(fetchSpy).toHaveBeenCalledTimes(1);
    expect(fetchSpy).toHaveBeenCalledWith("/api/fleet/diff", expect.objectContaining({
      method: "POST",
    }));
    expect(result.current.diff).toEqual(MOCK_DIFF);
    expect(result.current.error).toBeNull();
    expect(result.current.isLoading).toBe(false);
  });

  it("returns cached diff without API call on cache hit", async () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: true,
      json: () => Promise.resolve(MOCK_DIFF),
    } as Response);

    const { result } = renderHook(() => useFleetDiff());

    // First call — cache miss
    await act(async () => {
      await result.current.fetchDiff(CONFIG_ITEM, "aaa111", "bbb222");
    });
    expect(fetchSpy).toHaveBeenCalledTimes(1);

    // Second call — cache hit
    await act(async () => {
      await result.current.fetchDiff(CONFIG_ITEM, "aaa111", "bbb222");
    });
    expect(fetchSpy).toHaveBeenCalledTimes(1); // no additional fetch
    expect(result.current.diff).toEqual(MOCK_DIFF);
  });

  it("sets isLoading during fetch", async () => {
    let resolveFetch: (value: Response) => void;
    vi.spyOn(globalThis, "fetch").mockImplementation(
      () => new Promise((resolve) => { resolveFetch = resolve; }),
    );

    const { result } = renderHook(() => useFleetDiff());

    let fetchPromise: Promise<void>;
    act(() => {
      fetchPromise = result.current.fetchDiff(CONFIG_ITEM, "aaa111", "bbb222");
    });

    // Loading should be true while fetch is in-flight
    expect(result.current.isLoading).toBe(true);

    await act(async () => {
      resolveFetch!({
        ok: true,
        json: () => Promise.resolve(MOCK_DIFF),
      } as Response);
      await fetchPromise!;
    });

    expect(result.current.isLoading).toBe(false);
  });

  it("sets error on fetch failure", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: false,
      status: 500,
      json: () => Promise.resolve({ error: "internal error" }),
    } as unknown as Response);

    const { result } = renderHook(() => useFleetDiff());

    await act(async () => {
      await result.current.fetchDiff(CONFIG_ITEM, "aaa111", "bbb222");
    });

    expect(result.current.error).toBe("internal error");
    expect(result.current.diff).toBeNull();
    expect(result.current.isLoading).toBe(false);
  });

  it("clearDiff resets diff but preserves cache", async () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: true,
      json: () => Promise.resolve(MOCK_DIFF),
    } as Response);

    const { result } = renderHook(() => useFleetDiff());

    // Fetch and populate diff
    await act(async () => {
      await result.current.fetchDiff(CONFIG_ITEM, "aaa111", "bbb222");
    });
    expect(result.current.diff).toEqual(MOCK_DIFF);

    // Clear diff
    act(() => {
      result.current.clearDiff();
    });
    expect(result.current.diff).toBeNull();

    // Re-fetch same params — should hit cache (no new fetch call)
    await act(async () => {
      await result.current.fetchDiff(CONFIG_ITEM, "aaa111", "bbb222");
    });
    expect(fetchSpy).toHaveBeenCalledTimes(1); // still only the original call
    expect(result.current.diff).toEqual(MOCK_DIFF);
  });

  it("different item/hash combos get separate cache entries", async () => {
    const secondDiff: FleetDiffResponse = {
      ...MOCK_DIFF,
      base_hash: "ccc333",
      target_hash: "ddd444",
    };

    let callCount = 0;
    const fetchSpy = vi.spyOn(globalThis, "fetch").mockImplementation(() => {
      callCount++;
      const data = callCount === 1 ? MOCK_DIFF : secondDiff;
      return Promise.resolve({
        ok: true,
        json: () => Promise.resolve(data),
      } as Response);
    });

    const { result } = renderHook(() => useFleetDiff());

    // First item
    await act(async () => {
      await result.current.fetchDiff(CONFIG_ITEM, "aaa111", "bbb222");
    });
    expect(result.current.diff).toEqual(MOCK_DIFF);

    // Different item — should trigger new fetch
    await act(async () => {
      await result.current.fetchDiff(PKG_ITEM, "ccc333", "ddd444");
    });
    expect(fetchSpy).toHaveBeenCalledTimes(2);
    expect(result.current.diff).toEqual(secondDiff);

    // Back to first item — cache hit
    await act(async () => {
      await result.current.fetchDiff(CONFIG_ITEM, "aaa111", "bbb222");
    });
    expect(fetchSpy).toHaveBeenCalledTimes(2); // no additional call
    expect(result.current.diff).toEqual(MOCK_DIFF);
  });
});
