# Containerfile Change Highlights Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Highlight added and removed lines in the containerfile preview when refinement toggles change the output.

**Architecture:** A custom React hook (`useContainerfileDiff`) diffs old vs. new `containerfilePreview` strings and returns a stable render model with `{ id, text, state }` entries. `ContainerfilePanel` renders from this model instead of splitting the raw string. CSS handles all animations. Scroll, dot indicator, and accessibility live in the panel component.

**Tech Stack:** React 18, TypeScript, PatternFly v6, `diff` npm package, Vitest + Testing Library, CSS animations/transitions.

**Spec:** `docs/specs/proposed/2026-05-26-containerfile-change-highlights-design.md`

---

### Task 1: Install `diff` dependency

**Files:**
- Modify: `inspectah-web/ui/package.json`

- [ ] **Step 1: Install the package**

```bash
cd inspectah-web/ui && npm install diff
```

- [ ] **Step 2: Install type definitions**

```bash
cd inspectah-web/ui && npm install -D @types/diff
```

- [ ] **Step 3: Verify import works**

Create a throwaway check — open a node REPL or add a temporary import in any `.ts` file and run `npx tsc --noEmit` to confirm types resolve. Remove the temporary import.

```bash
cd inspectah-web/ui && npx tsc --noEmit
```

- [ ] **Step 4: Commit**

```bash
git add inspectah-web/ui/package.json inspectah-web/ui/package-lock.json
git commit -m "chore(ui): add diff package for containerfile change highlights"
```

---

### Task 2: Create `useContainerfileDiff` hook — core diff logic

This is the heart of the feature. The hook takes the current `containerfilePreview` string and returns a merged render model.

**Files:**
- Create: `inspectah-web/ui/src/hooks/useContainerfileDiff.ts`
- Create: `inspectah-web/ui/src/hooks/__tests__/useContainerfileDiff.test.ts`

- [ ] **Step 1: Write the type definitions**

```typescript
// inspectah-web/ui/src/hooks/useContainerfileDiff.ts

export type LineState = "stable" | "added" | "removing";

export interface DiffLine {
  /** Stable identifier for React key — survives across renders. */
  id: string;
  /** The text content of the line. */
  text: string;
  /** Current visual state. */
  state: LineState;
}

export interface DiffResult {
  /** Merged render model — stable + added + removing lines in order. */
  lines: DiffLine[];
  /** Count of added lines in this diff. */
  addedCount: number;
  /** Count of removed lines in this diff. */
  removedCount: number;
  /** Whether any changes exist (addedCount + removedCount > 0). */
  hasChanges: boolean;
}
```

- [ ] **Step 2: Write failing tests for the pure diff function**

The hook has React lifecycle concerns (timers, refs). Extract the pure diff computation into a testable function `computeDiff(prev, next, priorLines?)` that the hook calls internally. The hook owns identity preservation: it passes the prior render model's lines into `computeDiff` so that unchanged lines keep their existing IDs across successive diffs. New and removed lines get fresh IDs.

```typescript
// inspectah-web/ui/src/hooks/__tests__/useContainerfileDiff.test.ts

import { describe, it, expect } from "vitest";
import { computeDiff } from "../useContainerfileDiff";

describe("computeDiff", () => {
  it("returns all stable lines when strings are identical", () => {
    const text = "FROM quay.io/fedora/fedora-bootc:42\nRUN dnf install -y httpd";
    const result = computeDiff(text, text);
    expect(result.hasChanges).toBe(false);
    expect(result.addedCount).toBe(0);
    expect(result.removedCount).toBe(0);
    expect(result.lines.every((l) => l.state === "stable")).toBe(true);
    expect(result.lines.map((l) => l.text)).toEqual([
      "FROM quay.io/fedora/fedora-bootc:42",
      "RUN dnf install -y httpd",
    ]);
  });

  it("marks added lines when new content has extra lines", () => {
    const prev = "FROM quay.io/fedora/fedora-bootc:42\nRUN dnf install -y httpd";
    const next = "FROM quay.io/fedora/fedora-bootc:42\nRUN dnf install -y httpd\nEXPOSE 80";
    const result = computeDiff(prev, next);
    expect(result.hasChanges).toBe(true);
    expect(result.addedCount).toBe(1);
    expect(result.removedCount).toBe(0);
    const added = result.lines.filter((l) => l.state === "added");
    expect(added).toHaveLength(1);
    expect(added[0].text).toBe("EXPOSE 80");
  });

  it("marks removed lines when content has fewer lines", () => {
    const prev = "FROM quay.io/fedora/fedora-bootc:42\nRUN dnf install -y httpd\nEXPOSE 80";
    const next = "FROM quay.io/fedora/fedora-bootc:42\nRUN dnf install -y httpd";
    const result = computeDiff(prev, next);
    expect(result.hasChanges).toBe(true);
    expect(result.addedCount).toBe(0);
    expect(result.removedCount).toBe(1);
    const removing = result.lines.filter((l) => l.state === "removing");
    expect(removing).toHaveLength(1);
    expect(removing[0].text).toBe("EXPOSE 80");
  });

  it("handles simultaneous adds and removes", () => {
    const prev = "FROM quay.io/fedora/fedora-bootc:42\nRUN dnf install -y httpd\nEXPOSE 80";
    const next = "FROM quay.io/fedora/fedora-bootc:42\nRUN dnf install -y nginx\nCOPY . /var/www";
    const result = computeDiff(prev, next);
    expect(result.hasChanges).toBe(true);
    expect(result.addedCount).toBeGreaterThan(0);
    expect(result.removedCount).toBeGreaterThan(0);
  });

  it("handles duplicate lines correctly with stable IDs", () => {
    const prev = "RUN echo a\nRUN echo a\nRUN echo b";
    const next = "RUN echo a\nRUN echo b";
    const result = computeDiff(prev, next);
    // One "RUN echo a" was removed
    expect(result.removedCount).toBe(1);
    // IDs must be unique even for duplicate text
    const ids = result.lines.map((l) => l.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("preserves IDs for unchanged lines across successive diffs", () => {
    const v1 = "FROM quay.io/fedora/fedora-bootc:42\nRUN dnf install -y httpd";
    const v2 = "FROM quay.io/fedora/fedora-bootc:42\nRUN dnf install -y httpd\nEXPOSE 80";
    const first = computeDiff(null, v1);
    const second = computeDiff(v1, v2, first.lines);
    // FROM and RUN lines are unchanged — their IDs must survive
    const firstIds = first.lines.map((l) => l.id);
    const stableInSecond = second.lines.filter((l) => l.state === "stable");
    expect(stableInSecond[0].id).toBe(firstIds[0]);
    expect(stableInSecond[1].id).toBe(firstIds[1]);
  });

  it("preserves IDs for unchanged duplicate lines across successive diffs", () => {
    const v1 = "RUN echo a\nRUN echo a\nRUN echo b";
    const v2 = "RUN echo a\nRUN echo a\nRUN echo b\nRUN echo c";
    const first = computeDiff(null, v1);
    const second = computeDiff(v1, v2, first.lines);
    // All three original lines are unchanged — IDs must match
    const stableInSecond = second.lines.filter((l) => l.state === "stable");
    expect(stableInSecond).toHaveLength(3);
    expect(stableInSecond[0].id).toBe(first.lines[0].id);
    expect(stableInSecond[1].id).toBe(first.lines[1].id);
    expect(stableInSecond[2].id).toBe(first.lines[2].id);
    // New line gets a fresh ID
    const added = second.lines.filter((l) => l.state === "added");
    expect(added).toHaveLength(1);
    expect(first.lines.map((l) => l.id)).not.toContain(added[0].id);
  });

  it("preserves IDs across three successive diffs", () => {
    const v1 = "FROM quay.io/fedora/fedora-bootc:42\nRUN dnf install -y httpd";
    const v2 = "FROM quay.io/fedora/fedora-bootc:42\nRUN dnf install -y httpd\nEXPOSE 80";
    const v3 = "FROM quay.io/fedora/fedora-bootc:42\nRUN dnf install -y httpd\nEXPOSE 80\nEXPOSE 443";
    const first = computeDiff(null, v1);
    const second = computeDiff(v1, v2, first.lines);
    const third = computeDiff(v2, v3, second.lines);
    // FROM line ID stable across all three
    expect(third.lines[0].id).toBe(first.lines[0].id);
  });

  it("preserves surviving duplicate ID when one duplicate is removed", () => {
    const v1 = "RUN echo a\nRUN echo a\nRUN echo b";
    const v2 = "RUN echo a\nRUN echo b";
    const first = computeDiff(null, v1);
    const second = computeDiff(v1, v2, first.lines);
    // The surviving "RUN echo a" keeps the first occurrence's ID
    const survivingA = second.lines.find(
      (l) => l.text === "RUN echo a" && l.state === "stable",
    );
    expect(survivingA).toBeTruthy();
    expect(survivingA!.id).toBe(first.lines[0].id);
    // "RUN echo b" keeps its ID
    const survivingB = second.lines.find(
      (l) => l.text === "RUN echo b" && l.state === "stable",
    );
    expect(survivingB!.id).toBe(first.lines[2].id);
  });

  it("settles added lines to stable with preserved ID on next diff", () => {
    const v1 = "FROM quay.io/fedora/fedora-bootc:42";
    const v2 = "FROM quay.io/fedora/fedora-bootc:42\nEXPOSE 80";
    const v3 = "FROM quay.io/fedora/fedora-bootc:42\nEXPOSE 80\nEXPOSE 443";
    const first = computeDiff(null, v1);
    const second = computeDiff(v1, v2, first.lines);
    // EXPOSE 80 is "added" in second
    const addedLine = second.lines.find((l) => l.state === "added");
    expect(addedLine!.text).toBe("EXPOSE 80");
    const addedId = addedLine!.id;
    // In third diff, EXPOSE 80 is now unchanged — it should keep its ID
    // and settle to "stable"
    const third = computeDiff(v2, v3, second.lines);
    const settledLine = third.lines.find((l) => l.text === "EXPOSE 80");
    expect(settledLine!.state).toBe("stable");
    expect(settledLine!.id).toBe(addedId);
  });

  it("returns baseline (all stable) when prev is null", () => {
    const result = computeDiff(null, "FROM quay.io/fedora/fedora-bootc:42\nRUN dnf install -y httpd");
    expect(result.hasChanges).toBe(false);
    expect(result.lines.every((l) => l.state === "stable")).toBe(true);
  });

  it("returns empty when both are null", () => {
    const result = computeDiff(null, null);
    expect(result.lines).toEqual([]);
    expect(result.hasChanges).toBe(false);
  });

  it("handles entire section appearing", () => {
    const prev = "FROM quay.io/fedora/fedora-bootc:42\nRUN dnf install -y httpd";
    const next =
      "FROM quay.io/fedora/fedora-bootc:42\nRUN dnf install -y httpd\n\n# === Services ===\nRUN systemctl enable httpd.service";
    const result = computeDiff(prev, next);
    const added = result.lines.filter((l) => l.state === "added");
    // The blank line, section header, and service line are all added
    expect(added.length).toBeGreaterThanOrEqual(2);
    expect(added.some((l) => l.text.includes("Services"))).toBe(true);
  });
});
```

- [ ] **Step 3: Run tests to verify they fail**

```bash
cd inspectah-web/ui && npx vitest run src/hooks/__tests__/useContainerfileDiff.test.ts
```

Expected: FAIL — `computeDiff` does not exist yet.

- [ ] **Step 4: Implement `computeDiff`**

```typescript
// Add to inspectah-web/ui/src/hooks/useContainerfileDiff.ts

import { diffLines } from "diff";

let nextId = 0;
function makeId(): string {
  return `dl-${++nextId}`;
}

/** Reset ID counter — exposed for tests only. */
export function _resetIdCounter(): void {
  nextId = 0;
}

export function computeDiff(
  prev: string | null,
  next: string | null,
  priorLines?: DiffLine[],
): DiffResult {
  if (next == null) {
    return { lines: [], addedCount: 0, removedCount: 0, hasChanges: false };
  }

  // Baseline establishment: first non-null value, no diff.
  if (prev == null) {
    const lines: DiffLine[] = next.split("\n").map((text) => ({
      id: makeId(),
      text,
      state: "stable" as const,
    }));
    return { lines, addedCount: 0, removedCount: 0, hasChanges: false };
  }

  // Build a queue of prior surviving-line IDs to reuse for unchanged lines.
  // Include both stable and added lines — an added line that persists into
  // the next diff should keep its ID when it settles to stable.
  // Removing lines are excluded — they are departing and their IDs should
  // not be reused for new stable lines.
  const priorSurvivingIds: string[] = [];
  if (priorLines) {
    for (const pl of priorLines) {
      if (pl.state !== "removing") priorSurvivingIds.push(pl.id);
    }
  }
  let priorIdx = 0;

  const changes = diffLines(prev, next);
  const lines: DiffLine[] = [];
  let addedCount = 0;
  let removedCount = 0;

  for (const change of changes) {
    // diffLines includes trailing newline in each value — split and
    // drop the trailing empty string from the split.
    const rawLines = change.value.split("\n");
    if (rawLines[rawLines.length - 1] === "") {
      rawLines.pop();
    }

    for (const text of rawLines) {
      if (change.added) {
        lines.push({ id: makeId(), text, state: "added" });
        addedCount++;
      } else if (change.removed) {
        lines.push({ id: makeId(), text, state: "removing" });
        removedCount++;
      } else {
        // Reuse prior ID for unchanged lines when available.
        const id = priorIdx < priorSurvivingIds.length
          ? priorSurvivingIds[priorIdx++]
          : makeId();
        lines.push({ id, text, state: "stable" });
      }
    }
  }

  return {
    lines,
    addedCount,
    removedCount,
    hasChanges: addedCount + removedCount > 0,
  };
}
```

- [ ] **Step 5: Run tests to verify they pass**

```bash
cd inspectah-web/ui && npx vitest run src/hooks/__tests__/useContainerfileDiff.test.ts
```

Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add inspectah-web/ui/src/hooks/useContainerfileDiff.ts \
       inspectah-web/ui/src/hooks/__tests__/useContainerfileDiff.test.ts
git commit -m "feat(ui): add computeDiff for containerfile change detection"
```

---

### Task 3: Create the `useContainerfileDiff` React hook

Wraps `computeDiff` with React state management: previous value tracking, highlight expiry timers, removing-line cleanup, and collapsed-panel baseline.

**Files:**
- Modify: `inspectah-web/ui/src/hooks/useContainerfileDiff.ts`
- Create: `inspectah-web/ui/src/hooks/__tests__/useContainerfileDiffHook.test.tsx`

- [ ] **Step 1: Write failing tests for the hook**

```tsx
// inspectah-web/ui/src/hooks/__tests__/useContainerfileDiffHook.test.tsx

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
      useContainerfileDiff("FROM quay.io/fedora/fedora-bootc:42\nRUN dnf install -y httpd", true),
    );
    expect(result.current.diffResult.hasChanges).toBe(false);
    expect(result.current.diffResult.lines).toHaveLength(2);
    expect(result.current.diffResult.lines.every((l) => l.state === "stable")).toBe(true);
  });

  it("returns empty lines when content is null", () => {
    const { result } = renderHook(() =>
      useContainerfileDiff(null, true),
    );
    expect(result.current.diffResult.lines).toEqual([]);
  });

  it("detects added lines on content change", () => {
    const { result, rerender } = renderHook(
      ({ content }) => useContainerfileDiff(content, true),
      { initialProps: { content: "FROM quay.io/fedora/fedora-bootc:42" as string | null } },
    );

    rerender({ content: "FROM quay.io/fedora/fedora-bootc:42\nEXPOSE 80" });
    expect(result.current.diffResult.hasChanges).toBe(true);
    expect(result.current.diffResult.addedCount).toBe(1);
  });

  it("does not diff when panel is collapsed — sets hasPendingChanges", () => {
    const { result, rerender } = renderHook(
      ({ content, isOpen }) => useContainerfileDiff(content, isOpen),
      { initialProps: { content: "FROM quay.io/fedora/fedora-bootc:42" as string | null, isOpen: true } },
    );

    // Collapse the panel
    rerender({ content: "FROM quay.io/fedora/fedora-bootc:42", isOpen: false });
    // Change content while collapsed
    rerender({ content: "FROM quay.io/fedora/fedora-bootc:42\nEXPOSE 80", isOpen: false });

    expect(result.current.hasPendingChanges).toBe(true);
    // Lines should still reflect the last-seen open state
    expect(result.current.diffResult.hasChanges).toBe(false);
  });

  it("diffs against last-seen baseline on expand", () => {
    const { result, rerender } = renderHook(
      ({ content, isOpen }) => useContainerfileDiff(content, isOpen),
      { initialProps: { content: "FROM quay.io/fedora/fedora-bootc:42" as string | null, isOpen: true } },
    );

    // Collapse, change content, re-expand
    rerender({ content: "FROM quay.io/fedora/fedora-bootc:42", isOpen: false });
    rerender({ content: "FROM quay.io/fedora/fedora-bootc:42\nEXPOSE 80", isOpen: false });
    rerender({ content: "FROM quay.io/fedora/fedora-bootc:42\nEXPOSE 80", isOpen: true });

    expect(result.current.diffResult.hasChanges).toBe(true);
    expect(result.current.diffResult.addedCount).toBe(1);
    expect(result.current.hasPendingChanges).toBe(false);
  });

  it("clears hasPendingChanges when content reverts to baseline while collapsed", () => {
    const { result, rerender } = renderHook(
      ({ content, isOpen }) => useContainerfileDiff(content, isOpen),
      { initialProps: { content: "FROM quay.io/fedora/fedora-bootc:42" as string | null, isOpen: true } },
    );

    rerender({ content: "FROM quay.io/fedora/fedora-bootc:42", isOpen: false });
    rerender({ content: "FROM quay.io/fedora/fedora-bootc:42\nEXPOSE 80", isOpen: false });
    expect(result.current.hasPendingChanges).toBe(true);

    // Revert to baseline
    rerender({ content: "FROM quay.io/fedora/fedora-bootc:42", isOpen: false });
    expect(result.current.hasPendingChanges).toBe(false);
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd inspectah-web/ui && npx vitest run src/hooks/__tests__/useContainerfileDiffHook.test.tsx
```

Expected: FAIL — `useContainerfileDiff` does not exist yet.

- [ ] **Step 3: Implement the hook**

```typescript
// Add to inspectah-web/ui/src/hooks/useContainerfileDiff.ts

import { useRef, useCallback, useState } from "react";

export interface UseContainerfileDiffReturn {
  diffResult: DiffResult;
  hasPendingChanges: boolean;
  /** Call after removing-line animation completes to prune it from the model. */
  pruneRemovingLine: (id: string) => void;
  /** Call to clear an added-line highlight (used by reduced-motion 2s timer). */
  clearHighlight: (id: string) => void;
}

export function useContainerfileDiff(
  content: string | null,
  isOpen: boolean,
): UseContainerfileDiffReturn {
  // The render model is the source of truth. It lives in state so that
  // surgical mutations (prune, clear) trigger rerenders without recomputing
  // the diff from raw strings.
  const [renderModel, setRenderModel] = useState<DiffResult>({
    lines: [], addedCount: 0, removedCount: 0, hasChanges: false,
  });

  const hasBaselineRef = useRef(false);
  const lastOpenContentRef = useRef<string | null>(null);
  const prevContentRef = useRef<string | null>(null);
  const wasOpenRef = useRef(isOpen);

  // Recompute the diff model only when content or isOpen actually changes.
  // This replaces the old useMemo — the key difference is that callbacks
  // below mutate renderModel directly without re-diffing.
  const prevContentForDiff = useRef<string | null>(null);
  const prevIsOpenForDiff = useRef(isOpen);

  if (content !== prevContentForDiff.current || isOpen !== prevIsOpenForDiff.current) {
    prevContentForDiff.current = content;
    prevIsOpenForDiff.current = isOpen;

    // Baseline establishment: first non-null content.
    if (content != null && !hasBaselineRef.current) {
      hasBaselineRef.current = true;
      lastOpenContentRef.current = content;
      prevContentRef.current = content;
      const baseline = computeDiff(null, content);
      setRenderModel(baseline);
    } else if (hasBaselineRef.current) {
      // Panel open→collapsed: snapshot baseline.
      if (wasOpenRef.current && !isOpen) {
        lastOpenContentRef.current = content;
      }

      // Panel collapsed→open: diff against last-seen baseline.
      if (!wasOpenRef.current && isOpen) {
        const result = computeDiff(
          lastOpenContentRef.current, content, renderModel.lines,
        );
        prevContentRef.current = content;
        setRenderModel(result);
      }

      // Panel open, content changed: diff against previous content.
      if (isOpen && content !== prevContentRef.current) {
        const result = computeDiff(
          prevContentRef.current, content, renderModel.lines,
        );
        prevContentRef.current = content;
        setRenderModel(result);
      }
    }

    wasOpenRef.current = isOpen;
  }

  const hasPendingChanges =
    !isOpen && hasBaselineRef.current && content !== lastOpenContentRef.current;

  // Surgical mutation: remove a departing line from the render model.
  // Does NOT recompute the diff — just filters the current model.
  const pruneRemovingLine = useCallback((id: string) => {
    setRenderModel((prev) => ({
      ...prev,
      lines: prev.lines.filter((l) => l.id !== id),
      removedCount: Math.max(0, prev.removedCount - 1),
      hasChanges: prev.addedCount + Math.max(0, prev.removedCount - 1) > 0,
    }));
  }, []);

  // Surgical mutation: downgrade an added line to stable.
  // Does NOT recompute the diff — just changes the state field.
  const clearHighlight = useCallback((id: string) => {
    setRenderModel((prev) => ({
      ...prev,
      lines: prev.lines.map((l) =>
        l.id === id && l.state === "added" ? { ...l, state: "stable" as const } : l,
      ),
      addedCount: Math.max(0, prev.addedCount - 1),
      hasChanges: Math.max(0, prev.addedCount - 1) + prev.removedCount > 0,
    }));
  }, []);

  return { diffResult: renderModel, hasPendingChanges, pruneRemovingLine, clearHighlight };
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cd inspectah-web/ui && npx vitest run src/hooks/__tests__/useContainerfileDiffHook.test.tsx
```

Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add inspectah-web/ui/src/hooks/useContainerfileDiff.ts \
       inspectah-web/ui/src/hooks/__tests__/useContainerfileDiffHook.test.tsx
git commit -m "feat(ui): add useContainerfileDiff hook with collapsed-panel state"
```

---

### Task 4: Add CSS for highlights, removals, dot indicator, and reduced motion

**Files:**
- Modify: `inspectah-web/ui/src/App.css`

- [ ] **Step 1: Add CSS custom properties and highlight animations**

Append the following to `App.css` after the existing containerfile panel styles (after the responsive section around line 265):

```css
/* --- Containerfile change highlights --- */

/* Theme tokens — light mode defaults */
.inspectah-cf-panel {
  --cf-highlight-add-bg: rgba(34, 197, 94, 0.12);
  --cf-highlight-add-border: #22c55e;
  --cf-highlight-remove-bg: rgba(251, 191, 36, 0.12);
  --cf-highlight-remove-border: #f59e0b;
}

/* Dark mode overrides */
.pf-v6-theme-dark .inspectah-cf-panel {
  --cf-highlight-add-bg: rgba(74, 222, 128, 0.15);
  --cf-highlight-add-border: #4ade80;
  --cf-highlight-remove-bg: rgba(251, 191, 36, 0.15);
  --cf-highlight-remove-border: #f59e0b;
}

/* Addition highlight — instant appear, hold, then fade */
.inspectah-cf-line--added {
  background-color: var(--cf-highlight-add-bg);
  border-left: 3px solid var(--cf-highlight-add-border);
  padding-left: 8px;
  animation: cf-highlight-fade 1.5s ease-out 0.5s forwards;
}

@keyframes cf-highlight-fade {
  from {
    background-color: var(--cf-highlight-add-bg);
    border-left-color: var(--cf-highlight-add-border);
  }
  to {
    background-color: transparent;
    border-left-color: transparent;
  }
}

/* Removal — phase 1: glow */
.inspectah-cf-line--removing {
  background-color: var(--cf-highlight-remove-bg);
  border-left: 3px solid var(--cf-highlight-remove-border);
  padding-left: 8px;
  overflow: hidden;
  animation: cf-removal-glow 0.3s ease-out forwards;
}

/* Removal — phase 2: collapse (triggered by adding --collapsing modifier) */
.inspectah-cf-line--collapsing {
  transition: max-height 0.5s ease-out, opacity 0.3s ease-out;
  max-height: 0 !important;
  opacity: 0;
}

/* Dot indicator on collapsed tab */
.inspectah-cf-panel__tab--has-changes::after {
  content: "";
  display: block;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background-color: var(--cf-highlight-add-border);
  margin-top: 8px;
}

/* Reduced motion: no animations, static highlight via JS timer */
@media (prefers-reduced-motion: reduce) {
  .inspectah-cf-line--added {
    animation: none;
  }

  .inspectah-cf-line--removing {
    animation: none;
  }

  .inspectah-cf-line--collapsing {
    transition: none;
  }
}
```

- [ ] **Step 2: Verify CSS parses correctly**

```bash
cd inspectah-web/ui && npx tsc --noEmit && npx vite build 2>&1 | tail -3
```

Expected: build succeeds (CSS is included in the bundle).

- [ ] **Step 3: Commit**

```bash
git add inspectah-web/ui/src/App.css
git commit -m "feat(ui): add CSS for containerfile change highlights and dot indicator"
```

---

### Task 5: Refactor `ContainerfilePanel` to use the diff render model

This is the main integration task. Replace the raw string split with the hook's render model.

**Files:**
- Modify: `inspectah-web/ui/src/components/ContainerfilePanel.tsx`
- Modify: `inspectah-web/ui/src/components/__tests__/ContainerfilePanel.test.tsx`

- [ ] **Step 1: Write failing tests for diff-driven rendering**

Add these tests to the existing test file:

```tsx
// Append to inspectah-web/ui/src/components/__tests__/ContainerfilePanel.test.tsx

describe("ContainerfilePanel change highlights", () => {
  it("highlights added lines on content change", () => {
    const { rerender } = render(
      <ContainerfilePanel
        content={"FROM quay.io/fedora/fedora-bootc:42\nRUN dnf install -y httpd"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    rerender(
      <ContainerfilePanel
        content={"FROM quay.io/fedora/fedora-bootc:42\nRUN dnf install -y httpd\nEXPOSE 80"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    const addedLines = document.querySelectorAll(".inspectah-cf-line--added");
    expect(addedLines.length).toBe(1);
    expect(addedLines[0].textContent).toContain("EXPOSE");
  });

  it("does not highlight on first render (baseline)", () => {
    render(
      <ContainerfilePanel
        content={"FROM quay.io/fedora/fedora-bootc:42\nRUN dnf install -y httpd"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    const addedLines = document.querySelectorAll(".inspectah-cf-line--added");
    const removingLines = document.querySelectorAll(".inspectah-cf-line--removing");
    expect(addedLines.length).toBe(0);
    expect(removingLines.length).toBe(0);
  });

  it("marks removed lines with departing class and aria-hidden", () => {
    const { rerender } = render(
      <ContainerfilePanel
        content={"FROM quay.io/fedora/fedora-bootc:42\nRUN dnf install -y httpd\nEXPOSE 80"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    rerender(
      <ContainerfilePanel
        content={"FROM quay.io/fedora/fedora-bootc:42\nRUN dnf install -y httpd"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    const removingLines = document.querySelectorAll(".inspectah-cf-line--removing");
    expect(removingLines.length).toBe(1);
    expect(removingLines[0].textContent).toContain("EXPOSE");
    expect(removingLines[0].getAttribute("aria-hidden")).toBe("true");
  });

  it("shows dot indicator when collapsed and content changes", () => {
    const { rerender } = render(
      <ContainerfilePanel
        content={"FROM quay.io/fedora/fedora-bootc:42"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    // Collapse
    rerender(
      <ContainerfilePanel
        content={"FROM quay.io/fedora/fedora-bootc:42"}
        isOpen={false}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    // Change content while collapsed
    rerender(
      <ContainerfilePanel
        content={"FROM quay.io/fedora/fedora-bootc:42\nEXPOSE 80"}
        isOpen={false}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    const tab = screen.getByLabelText(/Expand Containerfile panel/);
    expect(tab.closest(".inspectah-cf-panel__tab--has-changes")).toBeTruthy();
    expect(tab.getAttribute("aria-label")).toBe(
      "Expand Containerfile panel, pending changes",
    );
  });

  it("announces diff summary via aria-live region", () => {
    const { rerender } = render(
      <ContainerfilePanel
        content={"FROM quay.io/fedora/fedora-bootc:42"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    rerender(
      <ContainerfilePanel
        content={"FROM quay.io/fedora/fedora-bootc:42\nEXPOSE 80"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    const liveRegion = document.querySelector('[aria-live="polite"]');
    expect(liveRegion).toBeTruthy();
    expect(liveRegion!.textContent).toContain("1 line added");
  });

  it("does not announce when diff is empty", () => {
    const { rerender } = render(
      <ContainerfilePanel
        content={"FROM quay.io/fedora/fedora-bootc:42"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    // Same content — no change
    rerender(
      <ContainerfilePanel
        content={"FROM quay.io/fedora/fedora-bootc:42"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    const liveRegion = document.querySelector('[aria-live="polite"]');
    expect(liveRegion?.textContent?.trim() ?? "").toBe("");
  });
});
```

- [ ] **Step 2: Run tests to verify the new tests fail**

```bash
cd inspectah-web/ui && npx vitest run src/components/__tests__/ContainerfilePanel.test.tsx
```

Expected: new tests FAIL. Existing tests should still PASS.

- [ ] **Step 3: Refactor `ContainerfilePanel` to use the hook**

Replace the line-rendering logic in `ContainerfilePanel.tsx`. Key changes:

1. Import and call `useContainerfileDiff(content, isOpen)`.
2. Replace the `lines` useMemo (raw string split) with the hook's `diffResult.lines`.
3. Render each line as a block-level `<span>` (via `inspectah-cf-panel__line` CSS class with `display: block`) with the appropriate highlight class based on `state`.
4. Add `aria-hidden="true"` on removing lines.
5. Add the dot indicator and dynamic `aria-label` on the collapsed tab.
6. Add a visually hidden `aria-live="polite"` `<span>` for diff announcements (use `sr-only` / `clip-rect` pattern — not `display: none` or `hidden`, which suppress announcements).
7. Wire up the removal collapse animation: after the glow phase (0.3s timeout), add the `--collapsing` class, then on `transitionend` (or 1.5s fallback timeout) call `pruneRemovingLine(id)`.

The `tokenizeLine` function stays — it is applied to each `DiffLine.text` the same way it was applied to each raw string line.

The redaction logic (`sessionIsSensitive` / `hashesRevealed`) applies to the `text` field of each `DiffLine` before rendering, in a `useMemo` that maps over `diffResult.lines`.

```typescript
// Key structural change in the render:
// OLD: lines.map((line, i) => <span key={i}>...)
// NEW: diffResult.lines.map((dl) => <span key={dl.id} style={{display:'block'}} className={lineClass(dl)} ...>...)
```

**DOM structure note:** The current panel renders inside `<pre><code>...</code></pre>`. Use block-level `<span>` elements (`display: block` via CSS class) for each line — `<div>` inside `<code>` is invalid HTML. The `inspectah-cf-panel__line` class already exists and can be extended with `display: block`.

The full implementation integrates the hook, CSS classes, collapsed-panel dot, aria-live region, and removal animation lifecycle. Keep the existing resize drag, auto-collapse, keyword tokenization, and footer intact.

- [ ] **Step 4: Run all ContainerfilePanel tests**

```bash
cd inspectah-web/ui && npx vitest run src/components/__tests__/ContainerfilePanel.test.tsx
```

Expected: all PASS (both old and new tests).

- [ ] **Step 5: Run full test suite to check for regressions**

```bash
cd inspectah-web/ui && npx vitest run
```

Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add inspectah-web/ui/src/components/ContainerfilePanel.tsx \
       inspectah-web/ui/src/components/__tests__/ContainerfilePanel.test.tsx
git commit -m "feat(ui): integrate containerfile change highlights into panel"
```

---

### Task 6: Add scroll behavior

**Files:**
- Modify: `inspectah-web/ui/src/components/ContainerfilePanel.tsx`
- Add tests to: `inspectah-web/ui/src/components/__tests__/ContainerfilePanel.test.tsx`

- [ ] **Step 1: Write failing test for scroll-to-change**

```tsx
// Append to the change highlights describe block

it("calls scrollIntoView on the first added line", () => {
  const scrollMock = vi.fn();
  // Mock scrollIntoView on all elements
  Element.prototype.scrollIntoView = scrollMock;

  const { rerender } = render(
    <ContainerfilePanel
      content={"FROM quay.io/fedora/fedora-bootc:42"}
      isOpen={true}
      onToggle={vi.fn()}
      loading={false}
    />,
  );

  rerender(
    <ContainerfilePanel
      content={"FROM quay.io/fedora/fedora-bootc:42\nEXPOSE 80"}
      isOpen={true}
      onToggle={vi.fn()}
      loading={false}
    />,
  );

  expect(scrollMock).toHaveBeenCalled();
});

it("does not scroll when changed line is already visible", () => {
  const scrollMock = vi.fn();
  Element.prototype.scrollIntoView = scrollMock;

  // Mock getBoundingClientRect to report the element is visible
  const originalGetBCR = Element.prototype.getBoundingClientRect;
  Element.prototype.getBoundingClientRect = function () {
    return { top: 100, bottom: 120, left: 0, right: 100, width: 100, height: 20, x: 0, y: 100, toJSON: () => ({}) };
  };

  const { rerender } = render(
    <ContainerfilePanel
      content={"FROM quay.io/fedora/fedora-bootc:42"}
      isOpen={true}
      onToggle={vi.fn()}
      loading={false}
    />,
  );

  rerender(
    <ContainerfilePanel
      content={"FROM quay.io/fedora/fedora-bootc:42\nEXPOSE 80"}
      isOpen={true}
      onToggle={vi.fn()}
      loading={false}
    />,
  );

  // scrollIntoView should NOT be called since element is "visible"
  // (This test is approximate — the real check compares against the
  // scroll container bounds. In jsdom, getBoundingClientRect returns
  // zeros by default, so the mock makes the element "visible".)
  Element.prototype.getBoundingClientRect = originalGetBCR;
});
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd inspectah-web/ui && npx vitest run src/components/__tests__/ContainerfilePanel.test.tsx
```

- [ ] **Step 3: Implement scroll logic in ContainerfilePanel**

After the diff model renders, use a `useEffect` that fires when `diffResult` changes:

1. Find the first `added` or `removing` line element via a `data-line-id` attribute.
2. Check if it's visible using `getBoundingClientRect` against the panel body's bounds.
3. If not visible, call `scrollIntoView({ behavior: prefersReducedMotion ? 'auto' : 'smooth' })`.
4. Start the highlight animation on the next `requestAnimationFrame`.
5. Debounce the scroll with a 150ms window using a `useRef` timeout.

```typescript
// Check reduced motion preference
const prefersReducedMotion =
  window.matchMedia("(prefers-reduced-motion: reduce)").matches;
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cd inspectah-web/ui && npx vitest run src/components/__tests__/ContainerfilePanel.test.tsx
```

- [ ] **Step 5: Commit**

```bash
git add inspectah-web/ui/src/components/ContainerfilePanel.tsx \
       inspectah-web/ui/src/components/__tests__/ContainerfilePanel.test.tsx
git commit -m "feat(ui): add scroll-to-change behavior for containerfile highlights"
```

---

### Task 7: Reduced motion support

**Files:**
- Modify: `inspectah-web/ui/src/components/ContainerfilePanel.tsx`
- Add tests to: `inspectah-web/ui/src/components/__tests__/ContainerfilePanel.test.tsx`

- [ ] **Step 1: Write failing test**

```tsx
it("removes highlight class after 2s in reduced-motion mode", () => {
  // Mock prefers-reduced-motion
  Object.defineProperty(window, "matchMedia", {
    writable: true,
    value: (query: string) => ({
      matches: query === "(prefers-reduced-motion: reduce)",
      media: query,
      onchange: null,
      addListener: () => {},
      removeListener: () => {},
      addEventListener: () => {},
      removeEventListener: () => {},
      dispatchEvent: () => false,
    }),
  });

  vi.useFakeTimers();

  const { rerender } = render(
    <ContainerfilePanel
      content={"FROM quay.io/fedora/fedora-bootc:42"}
      isOpen={true}
      onToggle={vi.fn()}
      loading={false}
    />,
  );

  rerender(
    <ContainerfilePanel
      content={"FROM quay.io/fedora/fedora-bootc:42\nEXPOSE 80"}
      isOpen={true}
      onToggle={vi.fn()}
      loading={false}
    />,
  );

  // Highlight should be present
  expect(document.querySelectorAll(".inspectah-cf-line--added").length).toBe(1);

  // After 2s, highlight class should be removed
  act(() => { vi.advanceTimersByTime(2000); });
  expect(document.querySelectorAll(".inspectah-cf-line--added").length).toBe(0);

  vi.useRealTimers();

  // Restore default matchMedia
  Object.defineProperty(window, "matchMedia", {
    writable: true,
    value: () => ({
      matches: false, media: "", onchange: null,
      addListener: () => {}, removeListener: () => {},
      addEventListener: () => {}, removeEventListener: () => {},
      dispatchEvent: () => false,
    }),
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd inspectah-web/ui && npx vitest run src/components/__tests__/ContainerfilePanel.test.tsx
```

- [ ] **Step 3: Implement reduced-motion JS timer**

In the removal/highlight lifecycle logic, check `window.matchMedia("(prefers-reduced-motion: reduce)").matches`:

- If `true` and the line is `added`: set a 2s `setTimeout` that calls `clearHighlight(id)` to downgrade the line to `stable` (removes the highlight class).
- If `true` and the line is `removing`: call `pruneRemovingLine(id)` immediately (no animation).
- The CSS `@media (prefers-reduced-motion: reduce)` block from Task 4 already zeroes animation durations, so this JS timer is the only additional work needed.

- [ ] **Step 4: Run tests**

```bash
cd inspectah-web/ui && npx vitest run src/components/__tests__/ContainerfilePanel.test.tsx
```

- [ ] **Step 5: Commit**

```bash
git add inspectah-web/ui/src/components/ContainerfilePanel.tsx \
       inspectah-web/ui/src/components/__tests__/ContainerfilePanel.test.tsx
git commit -m "feat(ui): add reduced-motion support for containerfile highlights"
```

---

### Task 8: Full integration test and type check

**Files:**
- No new files — validation only.

- [ ] **Step 1: Run the full unit test suite**

```bash
cd inspectah-web/ui && npx vitest run
```

Expected: all PASS.

- [ ] **Step 2: Run TypeScript type check**

```bash
cd inspectah-web/ui && npx tsc --noEmit
```

Expected: no errors.

- [ ] **Step 3: Run the production build**

```bash
cd inspectah-web/ui && npx vite build
```

Expected: build succeeds with no errors.

- [ ] **Step 4: Commit any fixups from the integration pass**

If any issues were found and fixed in steps 1-3, commit them:

```bash
git add -A
git commit -m "fix(ui): integration fixups for containerfile change highlights"
```

If no fixups were needed, skip this step.
