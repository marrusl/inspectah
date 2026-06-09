import { useState, useCallback } from "react";
import { Label } from "@patternfly/react-core";
import type {
  TunedDecisionDto,
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

export interface TunedSectionProps {
  tuned: TunedDecisionDto[];
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

export function TunedSection({
  tuned,
  onViewUpdate,
  onMutationError,
}: TunedSectionProps) {
  const [pendingIds, setPendingIds] = useState<Set<string>>(new Set());

  const handleToggle = useCallback(
    (profile: string, currentInclude: boolean) => {
      const id = `tuned:${profile}`;
      setPendingIds((prev) => new Set(prev).add(id));
      const op: RefinementOp = {
        op: "SetInclude",
        target: {
          item_id: { kind: "TunedSelection", key: { profile } },
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

  if (tuned.length === 0) {
    return (
      <div className="inspectah-service-section" data-testid="tuned-section">
        <p style={{ padding: "var(--pf-t--global--spacer--md)", opacity: 0.6 }}>
          No tuned profile selections detected.
        </p>
      </div>
    );
  }

  // Sections with <3 items default expanded (already rendered as flat list)
  return (
    <div
      className="inspectah-service-section"
      role="grid"
      aria-label="Tuned Profiles"
      data-testid="tuned-section"
    >
      {tuned.map((t, idx) => {
        const bucket = extractTriageBucket(t.triage);
        const borderLeft = LEVEL_BORDER[bucket] ?? "none";
        const badge = badgeTextForTriage(t.triage);
        const level = triageBucketToAttention(t.triage);
        const isPending = pendingIds.has(`tuned:${t.active_profile}`);

        return (
          <div
            key={t.active_profile}
            role="row"
            aria-rowindex={idx + 1}
            aria-label={t.active_profile}
            tabIndex={idx === 0 ? 0 : -1}
            data-testid={`tuned-item-${t.active_profile}`}
            className="inspectah-decision-row"
            style={{ borderLeft }}
            onKeyDown={(e) => {
              if (e.target !== e.currentTarget) return;
              if (e.key === " " || e.key === "x") {
                e.preventDefault();
                handleToggle(t.active_profile, t.include);
              }
            }}
          >
            <div className="inspectah-decision-row__main">
              <div role="gridcell" className="inspectah-decision-row__toggle">
                <input
                  type="checkbox"
                  role="checkbox"
                  id={`switch-tuned-${t.active_profile}`}
                  checked={t.include}
                  onChange={() => handleToggle(t.active_profile, t.include)}
                  disabled={isPending}
                  aria-label={`Toggle ${t.active_profile}`}
                  style={{ minWidth: 20, minHeight: 20 }}
                />
              </div>
              <div role="gridcell" className="inspectah-decision-row__name">
                <span>{t.active_profile}</span>
                {t.custom_profiles.length > 0 && (
                  <span
                    style={{
                      fontSize: "var(--pf-t--global--font--size--xs)",
                      opacity: 0.6,
                      marginLeft: "var(--pf-t--global--spacer--xs)",
                    }}
                  >
                    Custom: {t.custom_profiles.join(", ")}
                  </span>
                )}
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
