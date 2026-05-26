import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, waitFor, act } from "@testing-library/react";
import { useView } from "../useView";
import type { ViewResponse } from "../../api/types";

const MOCK_VIEW: ViewResponse = {
  packages: [],
  config_files: [],
  containerfile_preview: "FROM ubi9\n",
  stats: {
    sections: [
      { kind: "package", total: 10, included: 8, excluded: 2 },
      { kind: "config", total: 5, included: 3, excluded: 2 },
    ],
    needs_review_count: 3,
    ops_applied: 1,
    can_undo: true,
    can_redo: false,
    baseline_available: false,
  },
  generation: 1,
  repo_groups: [],
  version_changes: [],
  service_states: [],
  service_dropins: [],
  quadlets: [],
  flatpaks: [],
  sysctls: [],
  tuned: [],
  users_groups_decisions: [],
  session_is_sensitive: false,
};

beforeEach(() => {
  vi.restoreAllMocks();
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("useView", () => {
  it("returns loading initially", () => {
    vi.spyOn(globalThis, "fetch").mockImplementation(
      () => new Promise(() => {}), // never resolves
    );
    const { result } = renderHook(() => useView());
    expect(result.current.loading).toBe(true);
    expect(result.current.data).toBeNull();
    expect(result.current.error).toBeNull();
  });

  it("returns data on success", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: true,
      json: () => Promise.resolve(MOCK_VIEW),
    } as Response);

    const { result } = renderHook(() => useView());
    await waitFor(() => expect(result.current.loading).toBe(false));

    expect(result.current.data).toEqual(MOCK_VIEW);
    expect(result.current.error).toBeNull();
  });

  it("returns error on failure", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: false,
      status: 500,
      json: () => Promise.resolve({ error: "server error" }),
    } as unknown as Response);

    const { result } = renderHook(() => useView());
    await waitFor(() => expect(result.current.loading).toBe(false));

    expect(result.current.data).toBeNull();
    expect(result.current.error).toBeTruthy();
  });

  it("refetch triggers a new fetch", async () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: true,
      json: () => Promise.resolve(MOCK_VIEW),
    } as Response);

    const { result } = renderHook(() => useView());
    await waitFor(() => expect(result.current.loading).toBe(false));

    expect(fetchSpy).toHaveBeenCalledTimes(1);

    act(() => {
      result.current.refetch();
    });

    await waitFor(() => expect(fetchSpy).toHaveBeenCalledTimes(2));
  });
});
