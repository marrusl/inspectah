import { useState, useRef, useCallback } from "react";
import { diffLines } from "diff";

export type LineState = "stable" | "added" | "removing";

export interface DiffLine {
  id: string;
  text: string;
  state: LineState;
}

export interface DiffResult {
  lines: DiffLine[];
  addedCount: number;
  removedCount: number;
  hasChanges: boolean;
}

let _idCounter = 0;

function makeId(): string {
  return `cf-${++_idCounter}`;
}

export function _resetIdCounter(): void {
  _idCounter = 0;
}

/** Split a diff chunk value into individual lines, dropping the trailing empty string from newline termination. */
function splitValue(value: string): string[] {
  const parts = value.split("\n");
  if (parts.length > 0 && parts[parts.length - 1] === "") {
    parts.pop();
  }
  return parts;
}

/**
 * Build a per-text FIFO map of IDs from prior surviving lines.
 * Excluding "removing" lines ensures only lines that were visible
 * in the previous render contribute IDs.
 */
function buildPriorIdMap(priorLines: DiffLine[]): Map<string, string[]> {
  const map = new Map<string, string[]>();
  for (const line of priorLines) {
    if (line.state === "removing") continue;
    const queue = map.get(line.text);
    if (queue) {
      queue.push(line.id);
    } else {
      map.set(line.text, [line.id]);
    }
  }
  return map;
}

/** Matches section headers like `# === Packages (23) ===` or `# === Services ===`. */
const SECTION_HEADER_RE = /^# === (.+?)(?:\s+\(\d+\))? ===$/;

/**
 * Extract the section name from a header line, ignoring any count.
 * Returns null if the line is not a section header.
 */
export function parseSectionHeader(line: string): string | null {
  const m = line.match(SECTION_HEADER_RE);
  return m ? m[1] : null;
}

/**
 * Suppress highlights on section headers whose only change is a count difference.
 * A removed `# === Packages (23) ===` paired with an added `# === Packages (22) ===`
 * means the user toggled a package, not the header — both lines become stable
 * (keeping the added version's text, pruning the removing version).
 *
 * Genuinely new or removed headers (no matching pair) keep their highlight.
 */
function suppressHeaderCountChanges(lines: DiffLine[]): {
  lines: DiffLine[];
  suppressed: number;
} {
  // Collect removing and added headers by section name
  const removingHeaders = new Map<string, number[]>(); // section name → indices
  const addedHeaders = new Map<string, number[]>();

  for (let i = 0; i < lines.length; i++) {
    const section = parseSectionHeader(lines[i].text);
    if (!section) continue;
    if (lines[i].state === "removing") {
      const arr = removingHeaders.get(section) ?? [];
      arr.push(i);
      removingHeaders.set(section, arr);
    } else if (lines[i].state === "added") {
      const arr = addedHeaders.get(section) ?? [];
      arr.push(i);
      addedHeaders.set(section, arr);
    }
  }

  // Match pairs: same section name with both a removing and an added header
  const indicesToRemove = new Set<number>(); // removing lines to prune
  const indicesToStabilize = new Set<number>(); // added lines to mark stable
  let suppressed = 0;

  for (const [section, removingIndices] of removingHeaders) {
    const addedIndices = addedHeaders.get(section);
    if (!addedIndices || addedIndices.length === 0) continue;

    // Pair them up (one-to-one)
    const pairs = Math.min(removingIndices.length, addedIndices.length);
    for (let i = 0; i < pairs; i++) {
      indicesToRemove.add(removingIndices[i]);
      indicesToStabilize.add(addedIndices[i]);
      suppressed += 2; // one removing + one added suppressed
    }
  }

  if (suppressed === 0) return { lines, suppressed: 0 };

  const result: DiffLine[] = [];
  for (let i = 0; i < lines.length; i++) {
    if (indicesToRemove.has(i)) continue; // drop the old-count header
    if (indicesToStabilize.has(i)) {
      result.push({ ...lines[i], state: "stable" }); // keep new-count text, mark stable
    } else {
      result.push(lines[i]);
    }
  }

  return { lines: result, suppressed };
}

export function computeDiff(
  prev: string | null,
  next: string | null,
  priorLines?: DiffLine[],
): DiffResult {
  // Both null: empty result
  if (next === null) {
    return { lines: [], addedCount: 0, removedCount: 0, hasChanges: false };
  }

  // Baseline: prev is null, return all lines as stable with fresh IDs
  if (prev === null) {
    const lines = splitValue(next).map((text) => ({
      id: makeId(),
      text,
      state: "stable" as const,
    }));
    return { lines, addedCount: 0, removedCount: 0, hasChanges: false };
  }

  const priorIdMap = priorLines
    ? buildPriorIdMap(priorLines)
    : new Map<string, string[]>();

  const changes = diffLines(prev, next);
  const rawLines: DiffLine[] = [];
  let addedCount = 0;
  let removedCount = 0;

  for (const change of changes) {
    const texts = splitValue(change.value);

    if (change.removed) {
      // Lines removed: mark as "removing", consume prior ID slot
      for (const text of texts) {
        const queue = priorIdMap.get(text);
        const id = queue?.shift() ?? makeId();
        rawLines.push({ id, text, state: "removing" });
        removedCount++;
      }
    } else if (change.added) {
      // Lines added: always fresh IDs
      for (const text of texts) {
        rawLines.push({ id: makeId(), text, state: "added" });
        addedCount++;
      }
    } else {
      // Unchanged lines: reuse prior ID if available
      for (const text of texts) {
        const queue = priorIdMap.get(text);
        const id = queue?.shift() ?? makeId();
        rawLines.push({ id, text, state: "stable" });
      }
    }
  }

  // Suppress noise from section headers that only changed their count
  const { lines, suppressed } = suppressHeaderCountChanges(rawLines);
  if (suppressed > 0) {
    // Each suppressed pair removes one "removing" and one "added" from counts
    const pairs = suppressed / 2;
    addedCount = Math.max(0, addedCount - pairs);
    removedCount = Math.max(0, removedCount - pairs);
  }

  return {
    lines,
    addedCount,
    removedCount,
    hasChanges: addedCount > 0 || removedCount > 0,
  };
}

export interface UseContainerfileDiffReturn {
  diffResult: DiffResult;
  hasPendingChanges: boolean;
  pruneRemovingLine: (id: string) => void;
  clearHighlight: (id: string) => void;
}

const EMPTY_DIFF: DiffResult = {
  lines: [],
  addedCount: 0,
  removedCount: 0,
  hasChanges: false,
};

export function useContainerfileDiff(
  content: string | null,
  isOpen: boolean,
): UseContainerfileDiffReturn {
  // Re-render trigger for mutation callbacks. Value is irrelevant.
  const [, forceRender] = useState(0);

  // The render model lives in a ref so it can be read and written
  // synchronously during the render phase AND from callbacks.
  const modelRef = useRef<DiffResult>(EMPTY_DIFF);
  const prevContentRef = useRef<string | null | undefined>(undefined);
  const lastOpenContentRef = useRef<string | null>(null);
  const wasOpenRef = useRef(isOpen);

  // Determine what changed since last render
  const contentChanged = content !== prevContentRef.current;
  const justOpened = isOpen && !wasOpenRef.current;
  const justClosed = !isOpen && wasOpenRef.current;

  if (contentChanged || justOpened || justClosed) {
    const isFirstContent = prevContentRef.current === undefined;

    if (isFirstContent) {
      // Baseline establishment: first non-null content, all stable
      modelRef.current = computeDiff(null, content);
      prevContentRef.current = content;
      lastOpenContentRef.current = content;
      wasOpenRef.current = isOpen;
    } else if (justClosed) {
      // Panel collapsed: snapshot baseline, don't re-diff
      lastOpenContentRef.current = prevContentRef.current as string | null;
      prevContentRef.current = content;
      wasOpenRef.current = false;
    } else if (!isOpen) {
      // Still collapsed, content changed: track but don't diff.
      // If lastOpenContent is still null (panel was never open), treat
      // the first real content as the baseline — not a pending change.
      if (lastOpenContentRef.current === null && content !== null) {
        lastOpenContentRef.current = content;
        modelRef.current = computeDiff(null, content);
      }
      prevContentRef.current = content;
    } else if (justOpened) {
      // Panel re-expanded: diff current against last-open baseline
      modelRef.current = computeDiff(
        lastOpenContentRef.current,
        content,
        modelRef.current.lines,
      );
      prevContentRef.current = content;
      lastOpenContentRef.current = content;
      wasOpenRef.current = true;
    } else {
      // Panel open, content changed: normal diff
      modelRef.current = computeDiff(
        prevContentRef.current as string | null,
        content,
        modelRef.current.lines,
      );
      prevContentRef.current = content;
    }
  }

  const hasPendingChanges = !isOpen && content !== lastOpenContentRef.current;

  const pruneRemovingLine = useCallback((id: string) => {
    const prev = modelRef.current;
    const filtered = prev.lines.filter((l) => l.id !== id);
    const newRemovedCount = Math.max(0, prev.removedCount - 1);
    modelRef.current = {
      lines: filtered,
      addedCount: prev.addedCount,
      removedCount: newRemovedCount,
      hasChanges: prev.addedCount > 0 || newRemovedCount > 0,
    };
    forceRender((n) => n + 1);
  }, []);

  const clearHighlight = useCallback((id: string) => {
    const prev = modelRef.current;
    const updated = prev.lines.map((l) =>
      l.id === id ? { ...l, state: "stable" as const } : l,
    );
    const newAddedCount = Math.max(0, prev.addedCount - 1);
    modelRef.current = {
      lines: updated,
      addedCount: newAddedCount,
      removedCount: prev.removedCount,
      hasChanges: newAddedCount > 0 || prev.removedCount > 0,
    };
    forceRender((n) => n + 1);
  }, []);

  return {
    diffResult: modelRef.current,
    hasPendingChanges,
    pruneRemovingLine,
    clearHighlight,
  };
}
