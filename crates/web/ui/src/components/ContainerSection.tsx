import { useState, useCallback } from "react";
import { Label } from "@patternfly/react-core";
import type {
  QuadletDecisionDto,
  FlatpakDecisionDto,
  ViewResponse,
  RefinementOp,
  TriageTag,
} from "../api/types";
import { applyOp } from "../api/client";
import {
  attentionLabelColor,
  formatTriageReason,
  triageBucketToAttention,
  extractTriageBucket,
} from "./attentionUtils";

export interface ContainerSectionProps {
  quadlets: QuadletDecisionDto[];
  flatpaks: FlatpakDecisionDto[];
  onViewUpdate: (view: ViewResponse) => void;
  onMutationError: (err: Error) => void;
}

const LEVEL_BORDER: Record<string, string> = {
  investigate: "3px solid var(--pf-t--global--color--status--danger--default)",
  site: "3px solid var(--pf-t--global--color--status--info--default)",
  divergent: "3px solid var(--pf-t--global--color--status--warning--default)",
  baseline: "none",
  partial: "3px solid var(--pf-t--global--color--status--custom--default)",
  universal: "none",
};

function badgeTextForTriage(triage: TriageTag): string | null {
  const level = triageBucketToAttention(triage);
  if (level === "routine") return null;
  return formatTriageReason(triage.primary_reason);
}

export function ContainerSection({
  quadlets,
  flatpaks,
  onViewUpdate,
  onMutationError,
}: ContainerSectionProps) {
  const [pendingIds, setPendingIds] = useState<Set<string>>(new Set());
  const [expandedPaths, setExpandedPaths] = useState<Set<string>>(new Set());

  const handleToggleQuadlet = useCallback(
    (path: string, currentInclude: boolean) => {
      const id = `quadlet:${path}`;
      setPendingIds((prev) => new Set(prev).add(id));
      const op: RefinementOp = {
        op: "SetInclude",
        target: {
          item_id: { kind: "Quadlet", key: { path } },
          include: !currentInclude,
        },
      };
      applyOp(op)
        .then((updatedView) => {
          setPendingIds((prev) => {
            const next = new Set(prev);
            next.delete(id);
            return next;
          });
          onViewUpdate(updatedView);
        })
        .catch((err) => {
          setPendingIds((prev) => {
            const next = new Set(prev);
            next.delete(id);
            return next;
          });
          onMutationError(err instanceof Error ? err : new Error(String(err)));
        });
    },
    [onViewUpdate, onMutationError],
  );

  const handleToggleFlatpak = useCallback(
    (
      appId: string,
      remote: string,
      branch: string,
      currentInclude: boolean,
    ) => {
      const id = `flatpak:${appId}`;
      setPendingIds((prev) => new Set(prev).add(id));
      const op: RefinementOp = {
        op: "SetInclude",
        target: {
          item_id: { kind: "Flatpak", key: { app_id: appId, remote, branch } },
          include: !currentInclude,
        },
      };
      applyOp(op)
        .then((updatedView) => {
          setPendingIds((prev) => {
            const next = new Set(prev);
            next.delete(id);
            return next;
          });
          onViewUpdate(updatedView);
        })
        .catch((err) => {
          setPendingIds((prev) => {
            const next = new Set(prev);
            next.delete(id);
            return next;
          });
          onMutationError(err instanceof Error ? err : new Error(String(err)));
        });
    },
    [onViewUpdate, onMutationError],
  );

  const totalItems = quadlets.length + flatpaks.length;

  if (totalItems === 0) {
    return (
      <div
        className="inspectah-service-section"
        data-testid="container-section"
      >
        <p style={{ padding: "var(--pf-t--global--spacer--md)", opacity: 0.6 }}>
          No quadlet or flatpak items detected.
        </p>
      </div>
    );
  }

  let rowIndex = 0;

  return (
    <div
      className="inspectah-service-section"
      role="grid"
      aria-label="Containers"
      data-testid="container-section"
    >
      {quadlets.map((q) => {
        const idx = rowIndex++;
        const bucket = extractTriageBucket(q.triage);
        const borderLeft = LEVEL_BORDER[bucket] ?? "none";
        const badge = badgeTextForTriage(q.triage);
        const level = triageBucketToAttention(q.triage);
        const isPending = pendingIds.has(`quadlet:${q.path}`);

        return (
          <div
            key={q.path}
            role="row"
            aria-rowindex={idx + 1}
            aria-label={q.name}
            tabIndex={idx === 0 ? 0 : -1}
            data-testid={`quadlet-item-${q.path}`}
            className="inspectah-decision-row"
            style={{ borderLeft }}
            onKeyDown={(e) => {
              if (e.target !== e.currentTarget) return;
              if (e.key === " " || e.key === "x") {
                e.preventDefault();
                handleToggleQuadlet(q.path, q.include);
              }
            }}
          >
            <div className="inspectah-decision-row__main">
              <div role="gridcell" className="inspectah-decision-row__toggle">
                <input
                  type="checkbox"
                  role="checkbox"
                  id={`switch-quadlet-${q.path}`}
                  checked={q.include}
                  onChange={() => handleToggleQuadlet(q.path, q.include)}
                  disabled={isPending}
                  aria-label={`Toggle ${q.name}`}
                  style={{ minWidth: 20, minHeight: 20 }}
                />
              </div>
              <div role="gridcell" className="inspectah-decision-row__name">
                <span>{q.name}</span>
                <span
                  style={{
                    fontSize: "var(--pf-t--global--font--size--xs)",
                    opacity: 0.6,
                    marginLeft: "var(--pf-t--global--spacer--xs)",
                  }}
                >
                  {q.image}
                </span>
              </div>
              <div role="gridcell" className="inspectah-decision-row__badge">
                <Label color="grey" isCompact>
                  Quadlet
                </Label>
                <span
                  style={{
                    fontSize: "var(--pf-t--global--font--size--xs)",
                    opacity: 0.6,
                    marginLeft: "var(--pf-t--global--spacer--xs)",
                  }}
                >
                  Image content
                </span>
              </div>
              {badge && (
                <div role="gridcell" className="inspectah-decision-row__badge">
                  <Label color={attentionLabelColor(level)}>{badge}</Label>
                </div>
              )}
              {q.content && (
                <div role="gridcell" style={{ marginLeft: "auto" }}>
                  <button
                    type="button"
                    aria-label={
                      expandedPaths.has(q.path)
                        ? "Hide unit file"
                        : "Show unit file"
                    }
                    onClick={(e) => {
                      e.stopPropagation();
                      setExpandedPaths((prev) => {
                        const next = new Set(prev);
                        if (next.has(q.path)) {
                          next.delete(q.path);
                        } else {
                          next.add(q.path);
                        }
                        return next;
                      });
                    }}
                    style={{
                      background: "none",
                      border: "none",
                      cursor: "pointer",
                      fontSize: "var(--pf-t--global--font--size--xs)",
                      opacity: 0.7,
                      padding: "2px 6px",
                    }}
                  >
                    {expandedPaths.has(q.path) ? "▼ Unit" : "▶ Unit"}
                  </button>
                </div>
              )}
            </div>
            {q.content && expandedPaths.has(q.path) && (
              <pre
                data-testid="quadlet-content"
                style={{
                  maxHeight: "300px",
                  overflow: "auto",
                  whiteSpace: "pre-wrap",
                  fontSize: "var(--pf-t--global--font--size--xs)",
                  padding: "var(--pf-t--global--spacer--sm)",
                  margin: 0,
                  background: "var(--pf-t--global--background--color--secondary--default)",
                  borderTop: "1px solid var(--pf-t--global--border--color--default)",
                }}
              >
                {q.content}
              </pre>
            )}
          </div>
        );
      })}

      {flatpaks.map((f) => {
        const idx = rowIndex++;
        const bucket = extractTriageBucket(f.triage);
        const borderLeft = LEVEL_BORDER[bucket] ?? "none";
        const badge = badgeTextForTriage(f.triage);
        const level = triageBucketToAttention(f.triage);
        const isPending = pendingIds.has(`flatpak:${f.app_id}`);

        return (
          <div
            key={`${f.app_id}:${f.remote}:${f.branch}`}
            role="row"
            aria-rowindex={idx + 1}
            aria-label={f.app_id}
            tabIndex={idx === 0 ? 0 : -1}
            data-testid={`flatpak-item-${f.app_id}`}
            className="inspectah-decision-row"
            style={{ borderLeft }}
            onKeyDown={(e) => {
              if (e.target !== e.currentTarget) return;
              if (e.key === " " || e.key === "x") {
                e.preventDefault();
                handleToggleFlatpak(f.app_id, f.remote, f.branch, f.include);
              }
            }}
          >
            <div className="inspectah-decision-row__main">
              <div role="gridcell" className="inspectah-decision-row__toggle">
                <input
                  type="checkbox"
                  role="checkbox"
                  id={`switch-flatpak-${f.app_id}`}
                  checked={f.include}
                  onChange={() =>
                    handleToggleFlatpak(f.app_id, f.remote, f.branch, f.include)
                  }
                  disabled={isPending}
                  aria-label={`Toggle ${f.app_id}`}
                  style={{ minWidth: 20, minHeight: 20 }}
                />
              </div>
              <div role="gridcell" className="inspectah-decision-row__name">
                <span>{f.app_id}</span>
                <span
                  style={{
                    fontSize: "var(--pf-t--global--font--size--xs)",
                    opacity: 0.6,
                    marginLeft: "var(--pf-t--global--spacer--xs)",
                  }}
                >
                  {f.remote}/{f.branch}
                </span>
              </div>
              <div role="gridcell" className="inspectah-decision-row__badge">
                <Label color="grey" isCompact>
                  Flatpak
                </Label>
                <span
                  style={{
                    fontSize: "var(--pf-t--global--font--size--xs)",
                    opacity: 0.6,
                    marginLeft: "var(--pf-t--global--spacer--xs)",
                  }}
                >
                  First boot
                </span>
              </div>
              {badge && (
                <div role="gridcell" className="inspectah-decision-row__badge">
                  <Label color={attentionLabelColor(level)}>{badge}</Label>
                </div>
              )}
            </div>
          </div>
        );
      })}
    </div>
  );
}
