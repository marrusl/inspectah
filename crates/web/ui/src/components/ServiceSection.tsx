import { useCallback, useMemo } from "react";
import { Label } from "@patternfly/react-core";
import type {
  ServiceDecisionDto,
  DropInDecisionDto,
  MountFindingDto,
  ViewResponse,
  RefinementOp,
  TriageTag,
} from "../api/types";
import { applyOp } from "../api/client";
import type { PendingGuard } from "../hooks/usePendingGuard";
import {
  attentionLabelColor,
  formatTriageReason,
  triageBucketToAttention,
  extractTriageBucket,
} from "./attentionUtils";

export interface ServiceSectionProps {
  services: ServiceDecisionDto[];
  dropins: DropInDecisionDto[];
  /**
   * The administrator's mount, automount and swap units. Rows with no
   * toggle: they are not decisions.
   */
  mountFindings: MountFindingDto[];
  /** Opens the config row at a path (a finding's unit file). */
  onNavigateToConfig?: (path: string) => void;
  onViewUpdate: (view: ViewResponse) => void;
  onMutationError: (err: Error) => void;
  /**
   * Requests in flight, by `serviceKey` and `dropinKey`. The owner must
   * outlive this section, which unmounts on a section switch.
   */
  pendingGuard: PendingGuard;
}

/** Guard key for a service: the whole of its op's item_id key. */
function serviceKey(unit: string): string {
  return JSON.stringify(["service", unit]);
}

/** Guard key for a drop-in: the whole of its op's item_id key. */
function dropinKey(path: string): string {
  return JSON.stringify(["dropin", path]);
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

/**
 * The ids an element's `aria-describedby` names, space-separated, or
 * undefined when none renders, so no id dangles.
 */
function describedBy(...ids: (string | false | null | undefined)[]): string | undefined {
  const present = ids.filter((id): id is string => !!id);
  return present.length > 0 ? present.join(" ") : undefined;
}

/**
 * A reason the server wrote, under its row. The text is shown as given:
 * never composed, rephrased or parsed here.
 */
function RowNote({
  id,
  testId,
  className,
  text,
}: {
  id: string;
  testId: string;
  className?: string;
  text: string;
}) {
  return (
    <div
      id={id}
      data-testid={testId}
      className={`inspectah-decision-row__helper-text inspectah-unit-note${className ? ` ${className}` : ""}`}
    >
      {text}
    </div>
  );
}

function badgeTextForTriage(triage: TriageTag): string | null {
  const level = triageBucketToAttention(triage);
  if (level === "routine") return null;
  return formatTriageReason(triage.primary_reason);
}

interface DropInRowProps {
  dropin: DropInDecisionDto;
  /** The parent service row is excluded, so this drop-in cannot apply. */
  parentExcluded: boolean;
  /** The parent service row is locked, which locks its drop-ins too. */
  parentLocked: boolean;
  isPending: boolean;
  onToggle: (path: string, currentInclude: boolean) => void;
}

function DropInRow({
  dropin: di,
  parentExcluded,
  parentLocked,
  isPending,
  onToggle,
}: DropInRowProps) {
  const diBucket = extractTriageBucket(di.triage);
  const diBadge = badgeTextForTriage(di.triage);
  const diLevel = triageBucketToAttention(di.triage);
  const diLocked = di.locked === true || parentLocked;
  const disabled = parentExcluded || isPending || diLocked;
  const isShadow = di.shadow_type === "full_shadow";
  // The checkbox points at the rationale only when the rationale renders.
  const hasRationale = isShadow && !!di.shadow_rationale;
  const withheldId = `withheld-dropin-${di.path}`;

  // Border-left priority: triage border wins if present, else shadow warning border
  let diBorder = LEVEL_BORDER[diBucket] ?? "none";
  let backgroundColor: string | undefined;
  if (isShadow) {
    if (diBorder !== "none") {
      // Triage border wins left edge, shadow gets background
      backgroundColor = "var(--pf-t--global--color--status--warning--default)";
    } else {
      // No triage border, shadow gets left border
      diBorder = "4px solid var(--pf-t--global--color--status--warning--default)";
    }
  }

  return (
    <div
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
        backgroundColor: backgroundColor
          ? `color-mix(in srgb, ${backgroundColor} 10%, transparent)`
          : undefined,
        paddingLeft: "calc(var(--pf-t--global--spacer--md) + 16px)",
        opacity: parentExcluded ? 0.55 : 1,
        transition: "opacity 200ms ease, background-color 150ms ease",
      }}
      onKeyDown={(e) => {
        if (e.target !== e.currentTarget) return;
        if (disabled) return;
        if (e.key === " " || e.key === "x") {
          e.preventDefault();
          onToggle(di.path, di.include);
        }
      }}
    >
      <div className="inspectah-service-dropin__connector" aria-hidden="true" />
      <div className="inspectah-decision-row__main">
        <div role="gridcell" className="inspectah-decision-row__toggle">
          <input
            type="checkbox"
            role="checkbox"
            id={`switch-dropin-${di.path}`}
            checked={di.include}
            onChange={() => onToggle(di.path, di.include)}
            disabled={disabled}
            aria-label={
              diLocked
                ? `${di.path} (locked: ${di.attention_reason ?? "cannot toggle"})`
                : `Toggle ${di.path}`
            }
            aria-disabled={parentExcluded}
            aria-describedby={describedBy(
              hasRationale && `shadow-rationale-dropin-${di.path}`,
              di.withheld && withheldId,
            )}
            style={{ minWidth: 20, minHeight: 20 }}
          />
        </div>
        <div role="gridcell" className="inspectah-decision-row__name">
          <span style={{ fontSize: "var(--pf-t--global--font--size--sm)" }}>
            {di.path}
          </span>
        </div>
        {parentExcluded && !diLocked && (
          <div role="gridcell" className="inspectah-decision-row__badge">
            <Label color="grey" data-testid={`dropin-excluded-badge-${di.path}`}>
              Service excluded
            </Label>
          </div>
        )}
        {!parentExcluded && !diLocked && diBadge && (
          <div role="gridcell" className="inspectah-decision-row__badge">
            <Label color={attentionLabelColor(diLevel)}>{diBadge}</Label>
          </div>
        )}
        {isShadow && (
          <div
            role="gridcell"
            className="inspectah-decision-row__badge"
            data-testid={`shadow-badge-dropin-${di.path}`}
          >
            <Label color="orange" isCompact>
              Shadow override
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
      </div>
      {/* Full-shadow rationale below the drop-in toggle row */}
      {hasRationale && (
        <div
          id={`shadow-rationale-dropin-${di.path}`}
          className="inspectah-decision-row__helper-text"
          data-testid={`shadow-rationale-dropin-${di.path}`}
          style={{
            paddingLeft: "calc(var(--pf-t--global--spacer--md) + 28px)",
            paddingBottom: "var(--pf-t--global--spacer--xs)",
            fontSize: "var(--pf-t--global--font--size--xs)",
            color: "var(--pf-t--global--text--color--subtle)",
            fontStyle: "italic",
          }}
        >
          {di.shadow_rationale}
        </div>
      )}
      {di.withheld && (
        <RowNote
          id={withheldId}
          testId={withheldId}
          className="inspectah-unit-note--withheld"
          text={di.withheld.text}
        />
      )}
    </div>
  );
}

interface MountFindingRowProps {
  finding: MountFindingDto;
  onNavigateToConfig?: (path: string) => void;
}

/**
 * An administrator's mount, automount or swap unit: what the host had and
 * the export's advisory lines, as given. No toggle and no keyboard action:
 * the export decides nothing here. The unit file is decided on its config
 * row, which `unit_file_path` names.
 */
function MountFindingRow({ finding: f, onNavigateToConfig }: MountFindingRowProps) {
  const advisoriesId = `mount-finding-advisories-${f.unit}`;
  const path = f.unit_file_path;
  return (
    <div
      role="row"
      aria-label={f.unit}
      aria-describedby={describedBy(f.advisories.length > 0 && advisoriesId)}
      tabIndex={-1}
      data-testid={`mount-finding-${f.unit}`}
      className="inspectah-decision-row inspectah-mount-finding"
    >
      <div className="inspectah-decision-row__main">
        <div role="gridcell" className="inspectah-decision-row__name">
          <span>{f.unit}</span>
          <span
            className="inspectah-unit-kind"
            data-testid={`mount-finding-kind-${f.unit}`}
          >
            {f.kind}
          </span>
          {f.state !== null && (
            <span
              className="inspectah-unit-state"
              data-testid={`mount-finding-state-${f.unit}`}
            >
              {f.state}
            </span>
          )}
          {f.mount_point !== null && (
            <code
              className="inspectah-mount-finding__where"
              data-testid={`mount-finding-where-${f.unit}`}
            >
              {f.mount_point}
            </code>
          )}
        </div>
        {path !== null && (
          <div role="gridcell" className="inspectah-decision-row__badge">
            {onNavigateToConfig ? (
              <a
                href={`#configs:${path}`}
                data-testid={`mount-finding-file-${f.unit}`}
                onClick={(e) => {
                  e.preventDefault();
                  onNavigateToConfig(path);
                }}
              >
                {path}
              </a>
            ) : (
              <code data-testid={`mount-finding-file-${f.unit}`}>{path}</code>
            )}
          </div>
        )}
      </div>
      {f.advisories.length > 0 && (
        <div id={advisoriesId} className="inspectah-mount-finding__advisories">
          {f.advisories.map((line, i) => (
            <div key={i} data-testid="mount-finding-advisory">
              {line}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

export function ServiceSection({
  services,
  dropins,
  mountFindings,
  onNavigateToConfig,
  onViewUpdate,
  onMutationError,
  pendingGuard,
}: ServiceSectionProps) {
  const { pendingKeys, runGuarded } = pendingGuard;

  const dropinsByUnit = useMemo(() => groupDropinsByUnit(dropins), [dropins]);

  const orphanDropins = useMemo(() => {
    const units = new Set(services.map((s) => s.unit));
    return dropins.filter((d) => !units.has(d.unit));
  }, [services, dropins]);

  const sendInclude = useCallback(
    (key: string, op: RefinementOp) => {
      runGuarded(key, () =>
        applyOp(op)
          .then(onViewUpdate)
          .catch((err) =>
            onMutationError(
              err instanceof Error ? err : new Error(String(err)),
            ),
          ),
      );
    },
    [runGuarded, onViewUpdate, onMutationError],
  );

  const handleToggleService = useCallback(
    (unit: string, currentInclude: boolean) => {
      sendInclude(serviceKey(unit), {
        op: "SetInclude",
        target: {
          item_id: { kind: "Unit", key: { unit } },
          include: !currentInclude,
        },
      });
    },
    [sendInclude],
  );

  const handleToggleDropin = useCallback(
    (path: string, currentInclude: boolean) => {
      sendInclude(dropinKey(path), {
        op: "SetInclude",
        target: {
          item_id: { kind: "DropIn", key: { path } },
          include: !currentInclude,
        },
      });
    },
    [sendInclude],
  );

  if (
    services.length === 0 &&
    dropins.length === 0 &&
    mountFindings.length === 0
  ) {
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
        const badge = badgeTextForTriage(svc.triage);
        const level = triageBucketToAttention(svc.triage);
        const isPending = pendingKeys.has(serviceKey(svc.unit));
        const svcLocked = svc.locked === true;
        const borderLeft = LEVEL_BORDER[bucket] ?? "none";
        // A locked row shows its reason in the badge; an unlocked one, such
        // as a runtime updater excluded by default, under the row.
        const attentionId = `attention-service-${svc.unit}`;
        // A scanned protected unit's attention_reason and its declined text
        // are the same served sentence: show it once, as the decline.
        const attention =
          svcLocked || svc.attention_reason === svc.declined?.text
            ? null
            : (svc.attention_reason ?? null);
        const declinedId = `declined-service-${svc.unit}`;

        return (
          <div key={svc.unit} data-testid={`service-group-${svc.unit}`}>
            {/* Parent service row */}
            <div
              role="row"
              aria-rowindex={idx + 1}
              aria-label={svc.unit}
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
                    aria-describedby={describedBy(
                      svcLocked && `locked-reason-service-${svc.unit}`,
                      attention && attentionId,
                      svc.declined && declinedId,
                    )}
                    style={{ minWidth: 20, minHeight: 20 }}
                  />
                </div>
                <div role="gridcell" className="inspectah-decision-row__name">
                  <span>{svc.unit}</span>
                  <span
                    className="inspectah-unit-kind"
                    data-testid={`service-kind-${svc.unit}`}
                  >
                    {svc.kind}
                  </span>
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
              {attention && (
                <RowNote id={attentionId} testId={attentionId} text={attention} />
              )}
              {svc.declined && (
                <RowNote
                  id={declinedId}
                  testId={declinedId}
                  className={`inspectah-unit-declined inspectah-unit-declined--${svc.declined.renders}`}
                  text={svc.declined.text}
                />
              )}
            </div>

            {/* Drop-in children */}
            {unitDropins.map((di) => (
              <DropInRow
                key={di.path}
                dropin={di}
                parentExcluded={!svc.include}
                parentLocked={svcLocked}
                isPending={pendingKeys.has(dropinKey(di.path))}
                onToggle={handleToggleDropin}
              />
            ))}
          </div>
        );
      })}

      {mountFindings.map((f) => (
        <MountFindingRow
          key={f.unit}
          finding={f}
          onNavigateToConfig={onNavigateToConfig}
        />
      ))}

      {/* Drop-ins whose unit produced no state change: a unit sitting at
          its preset state never reaches `state_changes`, so without this
          group the drop-in row — which owns the decision for a config
          path it shadows — would render nowhere. */}
      {orphanDropins.length > 0 && (
        <div data-testid="dropin-orphans">
          <h3 className="inspectah-service-section__orphan-heading">
            Other drop-in overrides
          </h3>
          <p className="inspectah-service-section__orphan-note">
            These units are at their preset state, so the drop-in is the whole
            decision.
          </p>
          {[...groupDropinsByUnit(orphanDropins).entries()].map(
            ([unit, unitDropins]) => (
              <div key={unit} data-testid={`dropin-orphan-group-${unit}`}>
                <div className="inspectah-service-section__orphan-unit">
                  {unit}
                </div>
                {unitDropins.map((di) => (
                  <DropInRow
                    key={di.path}
                    dropin={di}
                    parentExcluded={false}
                    parentLocked={false}
                    isPending={pendingKeys.has(dropinKey(di.path))}
                    onToggle={handleToggleDropin}
                  />
                ))}
              </div>
            ),
          )}
        </div>
      )}
    </div>
  );
}
