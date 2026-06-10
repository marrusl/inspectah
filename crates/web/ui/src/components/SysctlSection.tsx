import { useState, useCallback } from "react";
import { Label } from "@patternfly/react-core";
import type {
  SysctlDecisionDto,
  ViewResponse,
  RefinementOp,
  TriageTag,
  TriageAnnotation,
} from "../api/types";
import { applyOp } from "../api/client";
import {
  attentionLabelColor,
  formatTriageReason,
  triageBucketToAttention,
  extractTriageBucket,
} from "./attentionUtils";

export interface SysctlSectionProps {
  sysctls: SysctlDecisionDto[];
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

function isRuntimeOnly(annotations: TriageAnnotation[]): boolean {
  return annotations.some((a) => a === "runtime_only_observation");
}

export function SysctlSection({
  sysctls,
  onViewUpdate,
  onMutationError,
}: SysctlSectionProps) {
  const [pendingIds, setPendingIds] = useState<Set<string>>(new Set());

  const handleToggle = useCallback(
    (key: string, currentInclude: boolean) => {
      const id = `sysctl:${key}`;
      setPendingIds((prev) => new Set(prev).add(id));
      const op: RefinementOp = {
        op: "SetInclude",
        target: {
          item_id: { kind: "Sysctl", key: { key } },
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

  if (sysctls.length === 0) {
    return (
      <div className="inspectah-service-section" data-testid="sysctl-section">
        <p style={{ padding: "var(--pf-t--global--spacer--md)", opacity: 0.6 }}>
          No sysctl overrides detected.
        </p>
      </div>
    );
  }

  return (
    <div
      className="inspectah-service-section"
      role="grid"
      aria-label="Sysctls"
      data-testid="sysctl-section"
    >
      {sysctls.map((s, idx) => {
        const bucket = extractTriageBucket(s.triage);
        const borderLeft = LEVEL_BORDER[bucket] ?? "none";
        const badge = badgeTextForTriage(s.triage);
        const level = triageBucketToAttention(s.triage);
        const isPending = pendingIds.has(`sysctl:${s.key}`);
        const runtimeOnly = isRuntimeOnly(s.triage.annotations ?? []);

        return (
          <div
            key={s.key}
            role="row"
            aria-rowindex={idx + 1}
            aria-label={s.key}
            tabIndex={idx === 0 ? 0 : -1}
            data-testid={`sysctl-item-${s.key}`}
            className="inspectah-decision-row"
            style={{ borderLeft }}
            onKeyDown={(e) => {
              if (e.target !== e.currentTarget) return;
              if (e.key === " " || e.key === "x") {
                e.preventDefault();
                handleToggle(s.key, s.include);
              }
            }}
          >
            <div className="inspectah-decision-row__main">
              <div role="gridcell" className="inspectah-decision-row__toggle">
                <input
                  type="checkbox"
                  role="checkbox"
                  id={`switch-sysctl-${s.key}`}
                  checked={s.include}
                  onChange={() => handleToggle(s.key, s.include)}
                  disabled={isPending}
                  aria-label={`Toggle ${s.key}`}
                  style={{ minWidth: 20, minHeight: 20 }}
                />
              </div>
              <div role="gridcell" className="inspectah-decision-row__name">
                <span>{s.key}</span>
                <span
                  style={{
                    fontSize: "var(--pf-t--global--font--size--xs)",
                    opacity: 0.6,
                    marginLeft: "var(--pf-t--global--spacer--xs)",
                  }}
                >
                  = {s.runtime}
                </span>
                {s.source && (
                  <span
                    style={{
                      fontSize: "var(--pf-t--global--font--size--xs)",
                      opacity: 0.5,
                      marginLeft: "var(--pf-t--global--spacer--xs)",
                    }}
                  >
                    ({s.source})
                  </span>
                )}
              </div>
              {runtimeOnly && (
                <div role="gridcell" className="inspectah-decision-row__badge">
                  <Label
                    color="yellow"
                    isCompact
                    data-testid={`sysctl-runtime-only-${s.key}`}
                  >
                    Runtime only
                  </Label>
                </div>
              )}
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
