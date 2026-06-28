import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useContainerfileDiff } from "../useContainerfileDiff";
import { _resetIdCounter } from "../useContainerfileDiff";

beforeEach(() => {
  _resetIdCounter();
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("useContainerfileDiff", () => {
  it("returns all stable lines on first non-null content", () => {
    const { result } = renderHook(() =>
      useContainerfileDiff(
        "FROM quay.io/fedora/fedora-bootc:42\nRUN dnf install -y httpd\n",
        true,
      ),
    );
    expect(result.current.diffResult.hasChanges).toBe(false);
    expect(result.current.diffResult.lines).toHaveLength(2);
    expect(
      result.current.diffResult.lines.every((l) => l.state === "stable"),
    ).toBe(true);
  });

  it("returns empty lines when content is null", () => {
    const { result } = renderHook(() => useContainerfileDiff(null, true));
    expect(result.current.diffResult.lines).toEqual([]);
  });

  it("detects added lines on content change", () => {
    const { result, rerender } = renderHook(
      ({ content }) => useContainerfileDiff(content, true),
      {
        initialProps: {
          content: "FROM quay.io/fedora/fedora-bootc:42\n" as string | null,
        },
      },
    );
    rerender({ content: "FROM quay.io/fedora/fedora-bootc:42\nEXPOSE 80\n" });
    expect(result.current.diffResult.hasChanges).toBe(true);
    expect(result.current.diffResult.addedCount).toBe(1);
  });

  it("does not diff when panel is collapsed — sets hasPendingChanges", () => {
    const { result, rerender } = renderHook(
      ({ content, isOpen }) => useContainerfileDiff(content, isOpen),
      {
        initialProps: {
          content: "FROM quay.io/fedora/fedora-bootc:42\n" as string | null,
          isOpen: true,
        },
      },
    );
    rerender({
      content: "FROM quay.io/fedora/fedora-bootc:42\n",
      isOpen: false,
    });
    rerender({
      content: "FROM quay.io/fedora/fedora-bootc:42\nEXPOSE 80\n",
      isOpen: false,
    });
    expect(result.current.hasPendingChanges).toBe(true);
    expect(result.current.diffResult.hasChanges).toBe(false);
  });

  it("diffs against last-seen baseline on expand", () => {
    const { result, rerender } = renderHook(
      ({ content, isOpen }) => useContainerfileDiff(content, isOpen),
      {
        initialProps: {
          content: "FROM quay.io/fedora/fedora-bootc:42\n" as string | null,
          isOpen: true,
        },
      },
    );
    rerender({
      content: "FROM quay.io/fedora/fedora-bootc:42\n",
      isOpen: false,
    });
    rerender({
      content: "FROM quay.io/fedora/fedora-bootc:42\nEXPOSE 80\n",
      isOpen: false,
    });
    rerender({
      content: "FROM quay.io/fedora/fedora-bootc:42\nEXPOSE 80\n",
      isOpen: true,
    });
    expect(result.current.diffResult.hasChanges).toBe(true);
    expect(result.current.diffResult.addedCount).toBe(1);
    expect(result.current.hasPendingChanges).toBe(false);
  });

  it("clears hasPendingChanges when content reverts to baseline while collapsed", () => {
    const { result, rerender } = renderHook(
      ({ content, isOpen }) => useContainerfileDiff(content, isOpen),
      {
        initialProps: {
          content: "FROM quay.io/fedora/fedora-bootc:42\n" as string | null,
          isOpen: true,
        },
      },
    );
    rerender({
      content: "FROM quay.io/fedora/fedora-bootc:42\n",
      isOpen: false,
    });
    rerender({
      content: "FROM quay.io/fedora/fedora-bootc:42\nEXPOSE 80\n",
      isOpen: false,
    });
    expect(result.current.hasPendingChanges).toBe(true);
    rerender({
      content: "FROM quay.io/fedora/fedora-bootc:42\n",
      isOpen: false,
    });
    expect(result.current.hasPendingChanges).toBe(false);
  });

  it("pruneRemovingLine removes a removing line and decrements removedCount", () => {
    const { result, rerender } = renderHook(
      ({ content }) => useContainerfileDiff(content, true),
      {
        initialProps: {
          content:
            "FROM quay.io/fedora/fedora-bootc:42\nRUN dnf install -y httpd\n" as
              string | null,
        },
      },
    );
    // Remove a line to produce a "removing" entry
    rerender({ content: "FROM quay.io/fedora/fedora-bootc:42\n" });
    expect(result.current.diffResult.removedCount).toBe(1);
    const removingLine = result.current.diffResult.lines.find(
      (l) => l.state === "removing",
    );
    expect(removingLine).toBeDefined();

    act(() => {
      result.current.pruneRemovingLine(removingLine!.id);
    });
    expect(
      result.current.diffResult.lines.find((l) => l.id === removingLine!.id),
    ).toBeUndefined();
    expect(result.current.diffResult.removedCount).toBe(0);
  });

  it("clearHighlight transitions an added line to stable and decrements addedCount", () => {
    const { result, rerender } = renderHook(
      ({ content }) => useContainerfileDiff(content, true),
      {
        initialProps: {
          content: "FROM quay.io/fedora/fedora-bootc:42\n" as string | null,
        },
      },
    );
    rerender({ content: "FROM quay.io/fedora/fedora-bootc:42\nEXPOSE 80\n" });
    expect(result.current.diffResult.addedCount).toBe(1);
    const addedLine = result.current.diffResult.lines.find(
      (l) => l.state === "added",
    );
    expect(addedLine).toBeDefined();

    act(() => {
      result.current.clearHighlight(addedLine!.id);
    });
    const clearedLine = result.current.diffResult.lines.find(
      (l) => l.id === addedLine!.id,
    );
    expect(clearedLine).toBeDefined();
    expect(clearedLine!.state).toBe("stable");
    expect(result.current.diffResult.addedCount).toBe(0);
  });

  it("updates hasChanges after all highlights are cleared", () => {
    const { result, rerender } = renderHook(
      ({ content }) => useContainerfileDiff(content, true),
      {
        initialProps: {
          content: "FROM quay.io/fedora/fedora-bootc:42\n" as string | null,
        },
      },
    );
    rerender({ content: "FROM quay.io/fedora/fedora-bootc:42\nEXPOSE 80\n" });
    expect(result.current.diffResult.hasChanges).toBe(true);

    const addedLine = result.current.diffResult.lines.find(
      (l) => l.state === "added",
    );
    act(() => {
      result.current.clearHighlight(addedLine!.id);
    });
    expect(result.current.diffResult.hasChanges).toBe(false);
  });
});
