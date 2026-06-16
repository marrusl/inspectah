import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, act, waitFor } from "@testing-library/react";
import { useAggregateMutation } from "../useAggregateMutation";
import type { AggregateViewResponse, RefinementOp } from "../../api/types";

const MOCK_AGGREGATE_VIEW: AggregateViewResponse = {
  generation: 1,
  can_undo: true,
  can_redo: false,
  containerfile_preview: "FROM ubi9\n",
  session_is_sensitive: false,
  summary: {
    host_count: 3,
    actionable_variant_items: [],
    informational_variant_count: 0,
  },
  sections: [],
  repo_groups: [],
  repo_conflict_count: 0,
};

const MOCK_AGGREGATE_VIEW_2: AggregateViewResponse = {
  ...MOCK_AGGREGATE_VIEW,
  generation: 2,
};

beforeEach(() => {
  vi.restoreAllMocks();
});

afterEach(() => {
  vi.restoreAllMocks();
});

/** Build a fetch mock that routes by URL. */
function mockFetch(routes: Record<string, () => Promise<Response>>) {
  return vi.spyOn(globalThis, "fetch").mockImplementation((input) => {
    const url = typeof input === "string" ? input : input.toString();
    for (const [pattern, handler] of Object.entries(routes)) {
      if (url.includes(pattern)) return handler();
    }
    return Promise.resolve({
      ok: false,
      status: 404,
      json: () => Promise.resolve({ error: "not found" }),
    } as unknown as Response);
  });
}

describe("useAggregateMutation", () => {
  it("calls applyOp then re-fetches aggregate view", async () => {
    mockFetch({
      "/api/op": () =>
        Promise.resolve({
          ok: true,
          json: () => Promise.resolve({}), // ignored
        } as Response),
      "/api/aggregate/view": () =>
        Promise.resolve({
          ok: true,
          json: () => Promise.resolve(MOCK_AGGREGATE_VIEW),
        } as Response),
    });

    const onViewUpdate = vi.fn();
    const onError = vi.fn();
    const { result } = renderHook(() =>
      useAggregateMutation(onViewUpdate, onError),
    );

    const op: RefinementOp = {
      op: "ExcludePackage",
      target: { name: "httpd", arch: "x86_64" },
    };

    act(() => {
      result.current.mutate(op);
    });

    await waitFor(() =>
      expect(onViewUpdate).toHaveBeenCalledWith(MOCK_AGGREGATE_VIEW),
    );
    expect(onError).not.toHaveBeenCalled();
    expect(result.current.lastConfirmedView.current).toBe(MOCK_AGGREGATE_VIEW);
  });

  it("clears queue on mutation failure", async () => {
    mockFetch({
      "/api/op": () =>
        Promise.resolve({
          ok: false,
          status: 500,
          json: () => Promise.resolve({ error: "server error" }),
        } as unknown as Response),
    });

    const onViewUpdate = vi.fn();
    const onError = vi.fn();
    const { result } = renderHook(() =>
      useAggregateMutation(onViewUpdate, onError),
    );

    act(() => {
      result.current.mutate({
        op: "ExcludePackage",
        target: { name: "httpd", arch: "x86_64" },
      });
    });

    await waitFor(() => expect(onError).toHaveBeenCalled());
    expect(onViewUpdate).not.toHaveBeenCalled();
    expect(result.current.isPending).toBe(false);
  });

  it("holds lastConfirmedView on refetch failure", async () => {
    let aggregateCallCount = 0;
    mockFetch({
      "/api/op": () =>
        Promise.resolve({
          ok: true,
          json: () => Promise.resolve({}),
        } as Response),
      "/api/aggregate/view": () => {
        aggregateCallCount++;
        if (aggregateCallCount === 1) {
          // First refetch fails
          return Promise.resolve({
            ok: false,
            status: 500,
            json: () => Promise.resolve({ error: "view unavailable" }),
          } as unknown as Response);
        }
        // Retry succeeds
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve(MOCK_AGGREGATE_VIEW),
        } as Response);
      },
    });

    const onViewUpdate = vi.fn();
    const onError = vi.fn();
    const { result } = renderHook(() =>
      useAggregateMutation(onViewUpdate, onError),
    );

    act(() => {
      result.current.mutate({
        op: "ExcludePackage",
        target: { name: "httpd", arch: "x86_64" },
      });
    });

    // Refetch fails — refetchError set, no onViewUpdate
    await waitFor(() => expect(result.current.refetchError).toBeTruthy());
    expect(onViewUpdate).not.toHaveBeenCalled();
    expect(onError).not.toHaveBeenCalled();

    // Retry succeeds
    await act(async () => {
      await result.current.retry();
    });

    expect(result.current.refetchError).toBeNull();
    expect(onViewUpdate).toHaveBeenCalledWith(MOCK_AGGREGATE_VIEW);
    expect(result.current.lastConfirmedView.current).toBe(MOCK_AGGREGATE_VIEW);
  });

  it("queues mutations sequentially", async () => {
    const callOrder: string[] = [];

    mockFetch({
      "/api/op": () => {
        callOrder.push("op");
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve({}),
        } as Response);
      },
      "/api/aggregate/view": () => {
        callOrder.push("aggregate");
        return Promise.resolve({
          ok: true,
          json: () =>
            Promise.resolve(
              callOrder.filter((c) => c === "aggregate").length === 1
                ? MOCK_AGGREGATE_VIEW
                : MOCK_AGGREGATE_VIEW_2,
            ),
        } as Response);
      },
    });

    const onViewUpdate = vi.fn();
    const onError = vi.fn();
    const { result } = renderHook(() =>
      useAggregateMutation(onViewUpdate, onError),
    );

    act(() => {
      result.current.mutate({
        op: "ExcludePackage",
        target: { name: "httpd", arch: "x86_64" },
      });
      result.current.mutate({
        op: "ExcludePackage",
        target: { name: "nginx", arch: "x86_64" },
      });
    });

    await waitFor(() => expect(onViewUpdate).toHaveBeenCalledTimes(2));

    // Each op is followed by a aggregate refetch: op, aggregate, op, aggregate
    expect(callOrder).toEqual(["op", "aggregate", "op", "aggregate"]);
  });

  it("calls undo endpoint then re-fetches aggregate view", async () => {
    const fetchSpy = mockFetch({
      "/api/undo": () =>
        Promise.resolve({
          ok: true,
          json: () => Promise.resolve({}),
        } as Response),
      "/api/aggregate/view": () =>
        Promise.resolve({
          ok: true,
          json: () => Promise.resolve(MOCK_AGGREGATE_VIEW),
        } as Response),
    });

    const onViewUpdate = vi.fn();
    const onError = vi.fn();
    const { result } = renderHook(() =>
      useAggregateMutation(onViewUpdate, onError),
    );

    act(() => {
      result.current.undo();
    });

    await waitFor(() =>
      expect(onViewUpdate).toHaveBeenCalledWith(MOCK_AGGREGATE_VIEW),
    );
    expect(fetchSpy).toHaveBeenCalledWith("/api/undo", expect.any(Object));
  });

  it("calls redo endpoint then re-fetches aggregate view", async () => {
    const fetchSpy = mockFetch({
      "/api/redo": () =>
        Promise.resolve({
          ok: true,
          json: () => Promise.resolve({}),
        } as Response),
      "/api/aggregate/view": () =>
        Promise.resolve({
          ok: true,
          json: () => Promise.resolve(MOCK_AGGREGATE_VIEW),
        } as Response),
    });

    const onViewUpdate = vi.fn();
    const onError = vi.fn();
    const { result } = renderHook(() =>
      useAggregateMutation(onViewUpdate, onError),
    );

    act(() => {
      result.current.redo();
    });

    await waitFor(() =>
      expect(onViewUpdate).toHaveBeenCalledWith(MOCK_AGGREGATE_VIEW),
    );
    expect(fetchSpy).toHaveBeenCalledWith("/api/redo", expect.any(Object));
  });
});
