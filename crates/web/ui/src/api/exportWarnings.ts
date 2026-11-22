/**
 * Parser for the `x-export-warnings` header on `POST /api/tarball`.
 *
 * The contract is in process-docs/skills/refine-export-warnings-header.md:
 * a JSON array of `{kind, text}` objects, `kind` either "failure" or
 * "notice". The tarball is already in hand when this runs, so nothing here
 * may throw: an unreadable header becomes a failure entry the operator can
 * see, never a broken download and never silence.
 */

export type ExportWarningKind = "failure" | "notice";

export interface ExportWarning {
  kind: ExportWarningKind;
  /** Display text. Render as plain text, never as HTML or markdown. */
  text: string;
}

/** Shown in place of a header, or entries, the client could not read. */
export const UNREADABLE_WARNINGS_TEXT =
  "The server sent export warnings this page could not read. " +
  "Check the audit report and the Containerfile in the export.";

/**
 * The most entries a well-formed header can hold: `MAX_HEADER_WARNINGS`
 * (20, in crates/web/src/handlers.rs) plus the server's "...and N more"
 * tail. A longer list is not from that server, so the client keeps the
 * first 20 and replaces the rest with one failure saying how many were
 * dropped, rather than rendering an unbounded list.
 */
export const MAX_CLIENT_WARNINGS = 21;

const UNREADABLE: ExportWarning = {
  kind: "failure",
  text: UNREADABLE_WARNINGS_TEXT,
};

export function parseExportWarnings(raw: string | null): ExportWarning[] {
  // Every successful export sends the header, so absence means an older
  // server with nothing to report, not a lost message.
  if (raw === null) return [];

  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    return [UNREADABLE];
  }
  if (!Array.isArray(value)) return [UNREADABLE];

  const warnings: ExportWarning[] = [];
  let unreadable = false;
  for (const entry of value) {
    if (
      typeof entry !== "object" ||
      entry === null ||
      typeof (entry as { text?: unknown }).text !== "string"
    ) {
      unreadable = true;
      continue;
    }
    const { kind, text } = entry as { kind?: unknown; text: string };
    // An unknown kind is treated as the more severe one: a newer server's
    // severity this client does not know must not be shown as routine.
    warnings.push({ kind: kind === "notice" ? "notice" : "failure", text });
  }
  if (unreadable) warnings.push(UNREADABLE);
  if (warnings.length > MAX_CLIENT_WARNINGS) {
    const kept = warnings.slice(0, MAX_CLIENT_WARNINGS - 1);
    const dropped = warnings.length - kept.length;
    kept.push({ kind: "failure", text: `...and ${dropped} more not shown` });
    return kept;
  }
  return warnings;
}

/**
 * Drop the header entries the operator already saw before download.
 *
 * `shown` holds the view's `not_carried_notice` and its
 * `machine_state_notices`, which the server builds with the same functions
 * and lists as the header's entries, so an exact match on `kind` and
 * `text` is the whole test. Never match on a text pattern: a header notice
 * that differs from every one shown (the view and the export drifted) has
 * not been seen and must stay in the list.
 */
export function withoutShownNotice(
  warnings: ExportWarning[],
  shown: readonly ExportWarning[],
): ExportWarning[] {
  if (shown.length === 0) return warnings;
  return warnings.filter(
    (w) => !shown.some((s) => w.kind === s.kind && w.text === s.text),
  );
}
