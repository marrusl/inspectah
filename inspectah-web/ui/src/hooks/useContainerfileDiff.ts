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

  const priorIdMap = priorLines ? buildPriorIdMap(priorLines) : new Map<string, string[]>();

  const changes = diffLines(prev, next);
  const lines: DiffLine[] = [];
  let addedCount = 0;
  let removedCount = 0;

  for (const change of changes) {
    const texts = splitValue(change.value);

    if (change.removed) {
      // Lines removed: mark as "removing", consume prior ID slot
      for (const text of texts) {
        const queue = priorIdMap.get(text);
        const id = queue?.shift() ?? makeId();
        lines.push({ id, text, state: "removing" });
        removedCount++;
      }
    } else if (change.added) {
      // Lines added: always fresh IDs
      for (const text of texts) {
        lines.push({ id: makeId(), text, state: "added" });
        addedCount++;
      }
    } else {
      // Unchanged lines: reuse prior ID if available
      for (const text of texts) {
        const queue = priorIdMap.get(text);
        const id = queue?.shift() ?? makeId();
        lines.push({ id, text, state: "stable" });
      }
    }
  }

  return {
    lines,
    addedCount,
    removedCount,
    hasChanges: addedCount > 0 || removedCount > 0,
  };
}
