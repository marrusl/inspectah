import { Button, Spinner } from "@patternfly/react-core";
import type { FleetDiffResponse, DiffHunk } from "../../api/types";

export interface DiffDrawerProps {
  diff: FleetDiffResponse | null;
  isLoading: boolean;
  error: string | null;
  onRetry: () => void;
  onClose: () => void;
  /** Label for the target operand, e.g. "e5f6g7h (web-4, web-5)" */
  targetLabel?: string;
  /** Label for the base (selected) operand, e.g. "a1b2c3d (web-1, web-2) [selected]" */
  baseLabel?: string;
}

function HunkView({ hunk }: { hunk: DiffHunk }) {
  const rangeHeader = `@@ -${hunk.base_range.start},${hunk.base_range.count} +${hunk.target_range.start},${hunk.target_range.count} @@`;

  return (
    <div className="diff-drawer__hunk">
      <div className="diff-drawer__range-header">{rangeHeader}</div>
      <pre className="diff-drawer__lines">
        {hunk.changes.map((change, i) => {
          let prefix = " ";
          let className = "diff-drawer__line";

          if (change.kind === "delete") {
            prefix = "-";
            className += " diff-drawer__line--delete";
          } else if (change.kind === "insert") {
            prefix = "+";
            className += " diff-drawer__line--insert";
          }

          return (
            <div key={i} className={className}>
              <span className="diff-drawer__line-prefix">{prefix}</span>
              <span className="diff-drawer__line-content">
                {change.content}
              </span>
            </div>
          );
        })}
      </pre>
    </div>
  );
}

export function DiffDrawer({
  diff,
  isLoading,
  error,
  onRetry,
  onClose,
  targetLabel,
  baseLabel,
}: DiffDrawerProps) {
  const title =
    targetLabel && baseLabel ? `Diff: ${targetLabel} vs ${baseLabel}` : "Diff";

  return (
    <div className="diff-drawer" data-testid="diff-drawer">
      <div className="diff-drawer__header">
        <span className="diff-drawer__title" data-testid="diff-drawer-title">
          {title}
        </span>
        <Button variant="plain" onClick={onClose} aria-label="Close">
          &times;
        </Button>
      </div>

      <div className="diff-drawer__body">
        {isLoading && (
          <div className="diff-drawer__loading" role="status">
            <Spinner size="lg" aria-label="Loading diff" />
          </div>
        )}

        {error && !isLoading && (
          <div className="diff-drawer__error">
            <p>{error}</p>
            <Button variant="secondary" onClick={onRetry}>
              Retry
            </Button>
          </div>
        )}

        {diff && !isLoading && !error && (
          <>
            <div className="diff-drawer__stats">
              <span className="diff-drawer__stats-insertions">
                +{diff.stats.insertions} insertion
                {diff.stats.insertions !== 1 ? "s" : ""}
              </span>
              {", "}
              <span className="diff-drawer__stats-deletions">
                -{diff.stats.deletions} deletion
                {diff.stats.deletions !== 1 ? "s" : ""}
              </span>
            </div>

            <div className="diff-drawer__hunks">
              {diff.hunks.map((hunk, i) => (
                <HunkView key={i} hunk={hunk} />
              ))}
            </div>
          </>
        )}
      </div>
    </div>
  );
}
