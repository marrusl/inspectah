import type { RefineStats } from "../api/types";

/** Build a RefineStats fixture with sensible defaults. */
export function mockStats(overrides?: Partial<RefineStats>): RefineStats {
  return {
    sections: [
      { kind: "package", total: 8, included: 5, excluded: 3 },
      { kind: "config", total: 4, included: 3, excluded: 1 },
    ],
    needs_review_count: 0,
    ops_applied: 0,
    can_undo: false,
    can_redo: false,
    baseline_available: false,
    ...overrides,
  };
}
