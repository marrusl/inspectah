import type { UserDecision } from "../api/types";

/** The users whose record one Undo or Redo step changed. */
export interface HistoryChange {
  users: readonly string[];
}

/**
 * Names the users whose record differs between two views, including users
 * only one of them has. Both views come from the same server encoder, so
 * comparing the encoded records is a field-by-field comparison.
 */
export function changedUsers(
  before: readonly UserDecision[] | undefined,
  after: readonly UserDecision[] | undefined,
): string[] {
  const encoded = (users: readonly UserDecision[] | undefined) =>
    new Map((users ?? []).map((u) => [u.name, JSON.stringify(u)]));
  const was = encoded(before);
  const now = encoded(after);
  const names = new Set([...was.keys(), ...now.keys()]);
  return [...names].filter((name) => was.get(name) !== now.get(name));
}
