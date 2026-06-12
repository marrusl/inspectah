import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, act, waitFor } from "@testing-library/react";
import { useMutation } from "../useMutation";
import type { ViewResponse, RefinementOp } from "../../api/types";

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
  package_groups: [],
  session_is_sensitive: false,
};

beforeEach(() => {
  vi.restoreAllMocks();
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("useMutation", () => {
  it("calls applyOp and triggers onSuccess", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: true,
      json: () => Promise.resolve(MOCK_VIEW),
    } as Response);

    const onSuccess = vi.fn();
    const onError = vi.fn();
    const { result } = renderHook(() => useMutation(onSuccess, onError));

    const op: RefinementOp = {
      op: "ExcludePackage",
      target: { name: "httpd", arch: "x86_64" },
    };

    act(() => {
      result.current.mutate(op);
    });

    await waitFor(() => expect(onSuccess).toHaveBeenCalledWith(MOCK_VIEW));
    expect(onError).not.toHaveBeenCalled();
  });

  it("calls undo endpoint", async () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: true,
      json: () => Promise.resolve(MOCK_VIEW),
    } as Response);

    const onSuccess = vi.fn();
    const onError = vi.fn();
    const { result } = renderHook(() => useMutation(onSuccess, onError));

    act(() => {
      result.current.undo();
    });

    await waitFor(() => expect(onSuccess).toHaveBeenCalled());
    expect(fetchSpy).toHaveBeenCalledWith("/api/undo", expect.any(Object));
  });

  it("calls redo endpoint", async () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: true,
      json: () => Promise.resolve(MOCK_VIEW),
    } as Response);

    const onSuccess = vi.fn();
    const onError = vi.fn();
    const { result } = renderHook(() => useMutation(onSuccess, onError));

    act(() => {
      result.current.redo();
    });

    await waitFor(() => expect(onSuccess).toHaveBeenCalled());
    expect(fetchSpy).toHaveBeenCalledWith("/api/redo", expect.any(Object));
  });

  it("clears queue and calls onError on failure", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: false,
      status: 500,
      json: () => Promise.resolve({ error: "fail" }),
    } as unknown as Response);

    const onSuccess = vi.fn();
    const onError = vi.fn();
    const { result } = renderHook(() => useMutation(onSuccess, onError));

    act(() => {
      result.current.mutate({
        op: "ExcludePackage",
        target: { name: "httpd", arch: "x86_64" },
      });
    });

    await waitFor(() => expect(onError).toHaveBeenCalled());
    expect(onSuccess).not.toHaveBeenCalled();
  });

  it("queues mutations sequentially", async () => {
    let callCount = 0;
    vi.spyOn(globalThis, "fetch").mockImplementation(async () => {
      callCount++;
      return {
        ok: true,
        json: () => Promise.resolve({ ...MOCK_VIEW, generation: callCount }),
      } as Response;
    });

    const onSuccess = vi.fn();
    const onError = vi.fn();
    const { result } = renderHook(() => useMutation(onSuccess, onError));

    act(() => {
      result.current.mutate({
        op: "ExcludePackage",
        target: { name: "a", arch: "x86_64" },
      });
      result.current.mutate({
        op: "ExcludePackage",
        target: { name: "b", arch: "x86_64" },
      });
    });

    await waitFor(() => expect(onSuccess).toHaveBeenCalledTimes(2));
  });
});
