import { describe, it, expect, vi } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { usePendingGuard } from "../usePendingGuard";

/** A request the test settles by hand. */
function deferred() {
  let resolve: () => void = () => {};
  let reject: (err: Error) => void = () => {};
  const promise = new Promise<void>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("usePendingGuard", () => {
  it("runs one request per key until it settles", async () => {
    const { result } = renderHook(() => usePendingGuard());
    const first = deferred();
    const request = vi.fn(() => first.promise);

    act(() => {
      result.current.runGuarded("a", request);
      result.current.runGuarded("a", request);
    });

    expect(request).toHaveBeenCalledTimes(1);
    expect(result.current.pendingKeys.has("a")).toBe(true);

    await act(async () => {
      first.resolve();
      await first.promise;
    });

    expect(result.current.pendingKeys.has("a")).toBe(false);
    act(() => result.current.runGuarded("a", request));
    expect(request).toHaveBeenCalledTimes(2);
  });

  it("does not hold other keys", () => {
    const { result } = renderHook(() => usePendingGuard());
    const request = vi.fn(() => new Promise<void>(() => {}));

    act(() => {
      result.current.runGuarded("a", request);
      result.current.runGuarded("b", request);
    });

    expect(request).toHaveBeenCalledTimes(2);
    expect([...result.current.pendingKeys].sort()).toEqual(["a", "b"]);
  });

  it("releases the key when the request fails", async () => {
    const { result } = renderHook(() => usePendingGuard());
    const first = deferred();
    const failed = first.promise.catch(() => {});

    act(() => result.current.runGuarded("a", () => failed));
    await act(async () => {
      first.reject(new Error("connection reset"));
      await failed;
    });

    expect(result.current.pendingKeys.has("a")).toBe(false);
  });

  it("releases the key when the request factory throws", () => {
    const { result } = renderHook(() => usePendingGuard());
    const failure = new Error("could not build the request");

    act(() => {
      expect(() =>
        result.current.runGuarded("a", () => {
          throw failure;
        }),
      ).toThrow(failure);
    });

    expect(result.current.pendingKeys.has("a")).toBe(false);
    const request = vi.fn(() => new Promise<void>(() => {}));
    act(() => result.current.runGuarded("a", request));
    expect(request).toHaveBeenCalledTimes(1);
  });
});
