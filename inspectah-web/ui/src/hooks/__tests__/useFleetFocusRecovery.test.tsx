import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, act } from "@testing-library/react";
import { useState } from "react";
import { useFleetFocusRecovery } from "../useFleetFocusRecovery";

// Test harness that exposes generation bumping
function TestHarness({ initialGeneration }: { initialGeneration: number }) {
  const [gen, setGen] = useState(initialGeneration);
  useFleetFocusRecovery(gen);

  return (
    <div>
      <div data-item-id="item-a" tabIndex={0} data-testid="item-a">
        Item A
      </div>
      <div data-item-id="item-b" tabIndex={0} data-testid="item-b">
        Item B
      </div>
      <button data-testid="bump" onClick={() => setGen((g) => g + 1)}>
        Bump
      </button>
    </div>
  );
}

describe("useFleetFocusRecovery", () => {
  let rafCallbacks: (() => void)[];

  beforeEach(() => {
    rafCallbacks = [];
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
      rafCallbacks.push(cb);
      return rafCallbacks.length;
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  function flushRaf() {
    const cbs = [...rafCallbacks];
    rafCallbacks = [];
    cbs.forEach((cb) => cb());
  }

  it("restores focus to previously focused item after generation bump", () => {
    const { getByTestId } = render(<TestHarness initialGeneration={1} />);

    const itemA = getByTestId("item-a");
    act(() => { itemA.focus(); });
    expect(document.activeElement).toBe(itemA);

    // Bump generation
    act(() => { getByTestId("bump").click(); });
    act(() => { flushRaf(); });

    expect(document.activeElement).toBe(itemA);
  });

  it("falls back to first item when focused item is removed", () => {
    // Render with a removable item
    function RemovableHarness() {
      const [gen, setGen] = useState(1);
      const [showC, setShowC] = useState(true);
      useFleetFocusRecovery(gen);

      return (
        <div>
          <div data-item-id="item-a" tabIndex={0} data-testid="item-a">
            Item A
          </div>
          {showC && (
            <div data-item-id="item-c" tabIndex={0} data-testid="item-c">
              Item C
            </div>
          )}
          <button
            data-testid="remove-and-bump"
            onClick={() => {
              setShowC(false);
              setGen((g) => g + 1);
            }}
          >
            Remove & Bump
          </button>
        </div>
      );
    }

    const { getByTestId, queryByTestId } = render(<RemovableHarness />);

    const itemC = getByTestId("item-c");
    act(() => { itemC.focus(); });
    expect(document.activeElement).toBe(itemC);

    // Remove item-c and bump generation
    act(() => { getByTestId("remove-and-bump").click(); });
    expect(queryByTestId("item-c")).toBeNull();

    act(() => { flushRaf(); });

    // Should fall back to item-a (the first remaining item)
    expect(document.activeElement).toBe(getByTestId("item-a"));
  });

  it("does nothing when generation is null", () => {
    function NullGenHarness() {
      useFleetFocusRecovery(null);
      return (
        <div data-item-id="item-a" tabIndex={0} data-testid="item-a">
          Item A
        </div>
      );
    }

    const { getByTestId } = render(<NullGenHarness />);
    const itemA = getByTestId("item-a");
    act(() => { itemA.focus(); });

    // No rAF should have been queued for null generation
    expect(rafCallbacks).toHaveLength(0);
  });
});
