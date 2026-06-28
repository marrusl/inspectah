import { useState, useCallback, useMemo } from "react";
import { Label } from "@patternfly/react-core";
import type {
  ServiceDecisionDto,
  DropInDecisionDto,
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

export interface ServiceSectionProps {
  services: ServiceDecisionDto[];
  dropins: DropInDecisionDto[];
  onViewUpdate: (view: ViewResponse) => void;
  onMutationError: (err: Error) => void;
}

/** Group drop-ins by their parent unit name. */
function groupDropinsByUnit(
  dropins: DropInDecisionDto[],
): Map<string, DropInDecisionDto[]> {
  const map = new Map<string, DropInDecisionDto[]>();
  for (const d of dropins) {
    const existing = map.get(d.unit);
    if (existing) {
      existing.push(d);
    } else {
      map.set(d.unit, [d]);
    }
  }
  return map;
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

export function ServiceSection({
  services,
  dropins,
  onViewUpdate,
  onMutationError,
}: ServiceSectionProps) {
  const [pendingIds, setPendingIds] = useState<Set<string>>(new Set());

  const dropinsByUnit = useMemo(() => groupDropinsByUnit(dropins), [dropins]);

  const handleToggleService = useCallback(
    (unit: string, currentInclude: boolean) => {
      const id = `service:${unit}`;
      setPendingIds((prev) => new Set(prev).add(id));
      const op: RefinementOp = {
        op: "SetInclude",
        target: {
          item_id: { kind: "Service", key: { unit } },
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

  const handleToggleDropin = useCallback(
    (path: string, currentInclude: boolean) => {
      const id = `dropin:${path}`;
      setPendingIds((prev) => new Set(prev).add(id));
      const op: RefinementOp = {
        op: "SetInclude",
        target: {
          item_id: { kind: "DropIn", key: { path } },
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

  if (services.length === 0) {
    return (
      <div className="inspectah-service-section" data-testid="service-section">
        <p style={{ padding: "var(--pf-t--global--spacer--md)", opacity: 0.6 }}>
          No service state changes detected.
        </p>
      </div>
    );
  }

  return (
    <div
      className="inspectah-service-section"
      role="grid"
      aria-label="Services"
      data-testid="service-section"
    >
      {services.map((svc, idx) => {
        const unitDropins = dropinsByUnit.get(svc.unit) ?? [];
        const bucket = extractTriageBucket(svc.triage);
        const borderLeft = LEVEL_BORDER[bucket] ?? "none";
        const badge = badgeTextForTriage(svc.triage);
        const level = triageBucketToAttention(svc.triage);
        const isPending = pendingIds.has(`service:${svc.unit}`);
        const svcLocked = svc.locked === true;

        return (
          <div key={svc.unit} data-testid={`service-group-${svc.unit}`}>
            {/* Parent service row */}
            <div
              role="row"
              aria-rowindex={idx + 1}
              aria-label={svc.unit}
              aria-describedby={
                svcLocked ? `locked-reason-service-${svc.unit}` : undefined
              }
              tabIndex={idx === 0 ? 0 : -1}
              data-testid={`service-item-${svc.unit}`}
              data-locked={svcLocked ? "true" : undefined}
              className={`inspectah-decision-row${svcLocked ? " inspectah-decision-row--locked" : ""}`}
              style={{ borderLeft }}
              onKeyDown={(e) => {
                if (e.target !== e.currentTarget) return;
                if (svcLocked) return;
                if (e.key === " " || e.key === "x") {
                  e.preventDefault();
                  handleToggleService(svc.unit, svc.include);
                }
              }}
            >
              <div className="inspectah-decision-row__main">
                <div role="gridcell" className="inspectah-decision-row__toggle">
                  <input
                    type="checkbox"
                    role="checkbox"
                    id={`switch-service-${svc.unit}`}
                    checked={svc.include}
                    onChange={() => handleToggleService(svc.unit, svc.include)}
                    disabled={isPending || svcLocked}
                    aria-label={
                      svcLocked
                        ? `${svc.unit} (locked: ${svc.attention_reason ?? "cannot toggle"})`
                        : `Toggle ${svc.unit}`
                    }
                    style={{ minWidth: 20, minHeight: 20 }}
                  />
                </div>
                <div role="gridcell" className="inspectah-decision-row__name">
                  <span>{svc.unit}</span>
                  {svc.owning_package && (
                    <span
                      style={{
                        fontSize: "var(--pf-t--global--font--size--xs)",
                        opacity: 0.6,
                        marginLeft: "var(--pf-t--global--spacer--xs)",
                      }}
                    >
                      ({svc.owning_package})
                    </span>
                  )}
                  <span
                    data-testid={`service-state-${svc.unit}`}
                    style={{
                      fontSize: "var(--pf-t--global--font--size--xs)",
                      opacity: 0.55,
                      marginLeft: "var(--pf-t--global--spacer--sm)",
                      fontStyle: "italic",
                    }}
                  >
                    {svc.current_state}
                    {svc.default_state && ` (preset: ${svc.default_state})`}
                  </span>
                </div>
                {badge && !svcLocked && (
                  <div
                    role="gridcell"
                    className="inspectah-decision-row__badge"
                  >
                    <Label color={attentionLabelColor(level)}>{badge}</Label>
                  </div>
                )}
                {svcLocked && (
                  <div
                    role="gridcell"
                    className="inspectah-decision-row__badge"
                    id={`locked-reason-service-${svc.unit}`}
                    data-testid={`locked-badge-service-${svc.unit}`}
                  >
                    <Label color="grey" isCompact>
                      {svc.attention_reason ?? "LOCKED"}
                    </Label>
                  </div>
                )}
              </div>
            </div>

            {/* Drop-in children */}
            {unitDropins.map((di) => {
              const parentExcluded = !svc.include;
              const diBucket = extractTriageBucket(di.triage);
              const diBorder = LEVEL_BORDER[diBucket] ?? "none";
              const diBadge = badgeTextForTriage(di.triage);
              const diLevel = triageBucketToAttention(di.triage);
              const diPending = pendingIds.has(`dropin:${di.path}`);
              const diLocked = di.locked === true || svcLocked;
              const disabled = parentExcluded || diPending || diLocked;

              return (
                <div
                  key={di.path}
                  role="row"
                  aria-label={di.path}
                  aria-describedby={
                    diLocked ? `locked-reason-dropin-${di.path}` : undefined
                  }
                  tabIndex={diLocked ? 0 : -1}
                  data-testid={`dropin-item-${di.path}`}
                  data-locked={diLocked ? "true" : undefined}
                  className={`inspectah-decision-row inspectah-service-dropin${diLocked ? " inspectah-decision-row--locked" : ""}`}
                  style={{
                    borderLeft: diBorder,
                    paddingLeft: "calc(var(--pf-t--global--spacer--md) + 16px)",
                    opacity: parentExcluded ? 0.55 : 1,
                    transition:
                      "opacity 200ms ease, background-color 150ms ease",
                  }}
                  onKeyDown={(e) => {
                    if (e.target !== e.currentTarget) return;
                    if (disabled) return;
                    if (e.key === " " || e.key === "x") {
                      e.preventDefault();
                      handleToggleDropin(di.path, di.include);
                    }
                  }}
                >
                  <div
                    className="inspectah-service-dropin__connector"
                    aria-hidden="true"
                  />
                  <div className="inspectah-decision-row__main">
                    <div
                      role="gridcell"
                      className="inspectah-decision-row__toggle"
                    >
                      <input
                        type="checkbox"
                        role="checkbox"
                        id={`switch-dropin-${di.path}`}
                        checked={di.include}
                        onChange={() => handleToggleDropin(di.path, di.include)}
                        disabled={disabled}
                        aria-label={
                          diLocked
                            ? `${di.path} (locked: ${di.attention_reason ?? "cannot toggle"})`
                            : `Toggle ${di.path}`
                        }
                        aria-disabled={parentExcluded}
                        style={{ minWidth: 20, minHeight: 20 }}
                      />
                    </div>
                    <div
                      role="gridcell"
                      className="inspectah-decision-row__name"
                    >
                      <span
                        style={{
                          fontSize: "var(--pf-t--global--font--size--sm)",
                        }}
                      >
                        {di.path}
                      </span>
                    </div>
                    {parentExcluded && !diLocked && (
                      <div
                        role="gridcell"
                        className="inspectah-decision-row__badge"
                      >
                        <Label
                          color="grey"
                          data-testid={`dropin-excluded-badge-${di.path}`}
                        >
                          Service excluded
                        </Label>
                      </div>
                    )}
                    {diLocked && (
                      <div
                        role="gridcell"
                        className="inspectah-decision-row__badge"
                        id={`locked-reason-dropin-${di.path}`}
                        data-testid={`locked-badge-dropin-${di.path}`}
                      >
                        <Label color="grey" isCompact>
                          {di.attention_reason ?? "LOCKED"}
                        </Label>
                      </div>
                    )}
                    {!parentExcluded && !diLocked && diBadge && (
                      <div
                        role="gridcell"
                        className="inspectah-decision-row__badge"
                      >
                        <Label color={attentionLabelColor(diLevel)}>
                          {diBadge}
                        </Label>
                      </div>
                    )}
                  </div>
                </div>
              );
            })}
          </div>
        );
      })}
    </div>
  );
}
