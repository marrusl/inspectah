import { useCallback, useId, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Button, Label } from "@patternfly/react-core";
import type {
  DeclinedView,
  ServiceDecisionDto,
  DropInDecisionDto,
  MountFindingDto,
  ManualLinkView,
  ManualLinkCause,
  UndeterminedInstallView,
  DefaultTargetView,
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
import {
  GROUP_ORDER,
  GROUP_TITLES,
  OUTCOME_LABELS,
  RELATION_LABELS,
  UNDETERMINED_CAUSE_LABEL,
  branchHeadOf,
  causeLabel,
  filterBarDrawn,
  followUpFlagText,
  groupUnits,
  keyFields,
  liveRegionText,
  outcomeCounts,
  pinsInView,
  showGroups,
  shownOutcomeRowCount,
  type FilterValue,
  type GroupId,
  type OccurrenceKey,
  type ReportedRow,
  type RowKey,
  type UnitBranch,
} from "./unitOutcome";
import { UnitGroup } from "./UnitGroup";
import { UnitOutcomeFilter } from "./UnitOutcomeFilter";

export interface ServiceSectionProps {
  services: ServiceDecisionDto[];
  dropins: DropInDecisionDto[];
  /**
   * The administrator's mount, automount and swap units. Rows with no
   * toggle: they are not decisions.
   */
  mountFindings: MountFindingDto[];
  /** The links inspectah reports and does not carry. Rows with no toggle. */
  manualLinks: ManualLinkView[];
  /** The units whose `[Install]` cannot be relied on. Rows with no toggle. */
  undeterminedInstalls: UndeterminedInstallView[];
  /** The default target and its toggle, when the host records one. */
  defaultTarget?: DefaultTargetView;
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

/**
 * A link row's identity: its relation, parent and unit, JSON-encoded. A
 * unit name can hold `-` (`a.service-b.service`), so joining the three
 * with a separator could give two links one key or test id.
 */
function linkIdentity(link: ManualLinkView): string {
  return JSON.stringify([link.relation, link.parent, link.unit]);
}

/** Guard key for the default target, whose op has no item_id key. */
const DEFAULT_TARGET_KEY = JSON.stringify(["default_target"]);

/**
 * The name of a link's cause. `undetermined` is served as an object that
 * carries its reason; every other cause is a bare string.
 */
function causeName(cause: ManualLinkCause): string {
  return typeof cause === "string" ? cause : "undetermined";
}

/** § 7 string 1: the section's accessible name. */
const SECTION_NAME = "Systemd units";

/** § 7 string 6, under the Reported, not carried header. */
const REPORTED_DESCRIPTION =
  "inspectah reports these and does not carry them. No toggle: the lines under each row are the Containerfile's comments for it.";

/** § 7 string 11, under the default target row. */
const DEFAULT_TARGET_EXPLANATION =
  "The target the source host boots to. Included, the image boots to it too.";

/** § 7 string 12: no row but the default target. */
const EMPTY_WITH_TARGET = "No other unit differs from its preset, and no drop-in was found.";

/** § 7 string 13: no row at all. */
const EMPTY_SECTION = "No unit differs from its preset, and no drop-in was found.";

/** § 7 string 23, a drop-in's badge while its parent is excluded. */
const UNIT_EXCLUDED = "Unit excluded";

/** The triage reason a unit row no longer shows as a badge (§ 6.6 item 3). */
const NON_DEFAULT_STATE_REASON = "service_non_default_state";

/** The outcome label's colour, by `declined.renders` (§ 6.6 item 2). */
const OUTCOME_COLORS: Record<DeclinedView["renders"], "orange" | "blue" | "grey"> = {
  fixme: "orange",
  advisory: "blue",
  omitted: "grey",
};

/** Subtle text for the preset beside a row's state (§ 6.6 item 1). */
const SUBTLE_TEXT_COLOR = "var(--pf-t--global--text--color--subtle)";

/** § 7 string 14: a filter value that shows no row, and its link button. */
const FILTER_EMPTY = "No units match this filter.";
const SHOW_ALL_LABEL = "Show all";

/** No section query yet (§ 6.12 arrives with section search). */
const NO_QUERY = "";

/** No pinned branch. */
const NO_PINS: ReadonlySet<OccurrenceKey> = new Set();

/** Hands a toggleable row's element to the section, by occurrence key. */
type RowRef = (el: HTMLDivElement | null) => void;

/** Every group open. */
const NONE_COLLAPSED = Object.fromEntries(GROUP_ORDER.map((id) => [id, false])) as Record<
  GroupId,
  boolean
>;

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

/** A declined row's outcome label in its badge cell (§ 6.6 item 2, § 7 string 15). */
function OutcomeLabel({ declined }: { declined: DeclinedView }) {
  return (
    <div role="gridcell" className="inspectah-decision-row__badge">
      <Label color={OUTCOME_COLORS[declined.renders]} variant="outline" isCompact>
        {OUTCOME_LABELS[declined.renders]}
      </Label>
    </div>
  );
}

/** A reported row's cause label in its badge cell (§ 6.6 item 12, § 7 string 18). */
function CauseLabel({ text }: { text: string }) {
  return (
    <div role="gridcell" className="inspectah-decision-row__badge">
      <Label color="grey" variant="outline" isCompact>
        {text}
      </Label>
    </div>
  );
}

/**
 * A row's DOM id. The first record with a row key keeps `base`; a later
 * one (a true duplicate, § 6.12) appends its own generated id, so its
 * `aria-describedby` names its own notes and not the first row's.
 */
function ownId(base: string, ordinal: number, generated: string): string {
  return ordinal === 0 ? base : `${base}${generated}`;
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
  /** The row key (§ 6.11), carried in `data-unit-key`. */
  unitKey: RowKey;
  /** The record's ordinal among the records with its row key (§ 6.11). */
  ordinal: number;
  /** The parent service row is excluded, so this drop-in cannot apply. */
  parentExcluded: boolean;
  /** The parent service row is locked, which locks its drop-ins too. */
  parentLocked: boolean;
  /** The drop-in is one of the parent unit's install drop-ins. */
  isInstallDropIn?: boolean;
  isPending: boolean;
  onToggle: (path: string, currentInclude: boolean) => void;
  rowRef: RowRef;
}

function DropInRow({
  dropin: di,
  unitKey,
  ordinal,
  parentExcluded,
  parentLocked,
  isInstallDropIn = false,
  isPending,
  onToggle,
  rowRef,
}: DropInRowProps) {
  const diBucket = extractTriageBucket(di.triage);
  const diBadge = badgeTextForTriage(di.triage);
  const diLevel = triageBucketToAttention(di.triage);
  const diLocked = di.locked === true || parentLocked;
  const disabled = parentExcluded || isPending || diLocked;
  const isShadow = di.shadow_type === "full_shadow";
  // The checkbox points at the rationale only when the rationale renders.
  const hasRationale = isShadow && !!di.shadow_rationale;
  // The ids `aria-describedby` names. A path can hold a space, and that
  // attribute is a list split on whitespace, so the ids are generated and
  // never built from the path; the test ids keep the path.
  const noteId = useId();
  const lockedReasonId = `${noteId}-locked-reason`;
  const rationaleId = `${noteId}-shadow-rationale`;
  const withheldId = `${noteId}-withheld`;

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
      ref={rowRef}
      role="row"
      aria-label={di.path}
      aria-describedby={diLocked ? lockedReasonId : undefined}
      tabIndex={diLocked ? 0 : -1}
      data-testid={`dropin-item-${di.path}`}
      data-unit-key={unitKey}
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
            id={ownId(`switch-dropin-${di.path}`, ordinal, noteId)}
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
              hasRationale && rationaleId,
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
              {UNIT_EXCLUDED}
            </Label>
          </div>
        )}
        {!parentExcluded && !diLocked && diBadge && (
          <div role="gridcell" className="inspectah-decision-row__badge">
            <Label color={attentionLabelColor(diLevel)}>{diBadge}</Label>
          </div>
        )}
        {isInstallDropIn && (
          <div
            role="gridcell"
            className="inspectah-decision-row__badge"
            data-testid="install-dropin-badge"
          >
            <Label color="blue" isCompact>
              [Install]
            </Label>
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
            id={lockedReasonId}
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
          id={rationaleId}
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
          testId={`withheld-dropin-${di.path}`}
          className="inspectah-unit-note--withheld"
          text={di.withheld.text}
        />
      )}
    </div>
  );
}

interface MountFindingRowProps {
  finding: MountFindingDto;
  unitKey: RowKey;
  ordinal: number;
  onNavigateToConfig?: (path: string) => void;
}

/**
 * An administrator's mount, automount or swap unit: what the host had and
 * the export's advisory lines, as given. No toggle and no keyboard action:
 * the export decides nothing here. The unit file is decided on its config
 * row, which `unit_file_path` names.
 */
function MountFindingRow({ finding: f, unitKey, ordinal, onNavigateToConfig }: MountFindingRowProps) {
  const advisoriesId = ownId(`mount-finding-advisories-${f.unit}`, ordinal, useId());
  const path = f.unit_file_path;
  return (
    <div
      role="row"
      aria-label={f.unit}
      aria-describedby={describedBy(f.advisories.length > 0 && advisoriesId)}
      tabIndex={-1}
      data-testid={`mount-finding-${f.unit}`}
      data-unit-key={unitKey}
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

/**
 * Paths the export does not stage, which keep a row's advisory out of the
 * Containerfile. The row's `# Omitted:` line says why.
 */
function NotAppliedPaths({
  id,
  testId,
  paths,
}: {
  id: string;
  testId: string;
  paths: string[];
}) {
  return (
    <div id={id} data-testid={testId} className="inspectah-unit-paths">
      <span className="inspectah-unit-paths__label">Not in the export:</span>
      {paths.map((p) => (
        <code key={p} data-testid="not-applied-path">
          {p}
        </code>
      ))}
    </div>
  );
}

/**
 * Paths a template instance names, under a label: the configuration its
 * enable needs, or the data the image does not carry.
 */
function InstancePaths({
  id,
  testId,
  label,
  pathTestId,
  paths,
}: {
  id: string;
  testId: string;
  label: string;
  pathTestId: string;
  paths: string[];
}) {
  return (
    <div id={id} data-testid={testId} className="inspectah-unit-paths">
      <span className="inspectah-unit-paths__label">{label}</span>
      {/* The spaces are not drawn in the flex row; they keep the paths
          apart in the text a screen reader reads from aria-describedby. */}
      {/* The path is a safe key: the collector and the fleet merge both
          dedup these lists. */}
      {paths.map((p) => (
        <span key={p}>
          {" "}
          <code data-testid={pathTestId}>{p}</code>
        </span>
      ))}
    </div>
  );
}

/** The server's advisory lines for a row, as given, one per line. */
function AdvisoryLines({
  id,
  testId,
  lines,
}: {
  id: string;
  testId: string;
  lines: string[];
}) {
  return (
    <div id={id} className="inspectah-mount-finding__advisories">
      {lines.map((line, i) => (
        <div key={i} data-testid={testId}>
          {line}
        </div>
      ))}
    </div>
  );
}

/**
 * A link inspectah reports and does not carry, an `Also=` name included:
 * its relation, its parent and the export's lines, as given. No toggle
 * and no keyboard action: the export decides nothing here.
 */
function ManualLinkRow({ link, unitKey }: { link: ManualLinkView; unitKey: RowKey }) {
  const baseId = useId();
  const advisoriesId = `${baseId}-advisories`;
  const notAppliedId = `${baseId}-not-applied`;
  const notApplied = link.not_applied ?? [];
  return (
    <div
      role="row"
      aria-label={link.unit}
      aria-describedby={describedBy(
        link.advisories.length > 0 && advisoriesId,
        notApplied.length > 0 && notAppliedId,
      )}
      tabIndex={-1}
      data-testid={`manual-link-${linkIdentity(link)}`}
      data-unit-key={unitKey}
      data-cause={causeName(link.cause)}
      className="inspectah-decision-row inspectah-manual-link"
    >
      <div className="inspectah-decision-row__main">
        <div role="gridcell" className="inspectah-decision-row__name">
          <span>{link.unit}</span>
          <span className="inspectah-unit-kind" data-testid="manual-link-relation">
            {RELATION_LABELS[link.relation]}
          </span>
          <code className="inspectah-manual-link__parent" data-testid="manual-link-parent">
            {link.parent}
          </code>
        </div>
        <CauseLabel text={causeLabel(link.cause)} />
      </div>
      {link.advisories.length > 0 && (
        <AdvisoryLines
          id={advisoriesId}
          testId="manual-link-advisory"
          lines={link.advisories}
        />
      )}
      {notApplied.length > 0 && (
        <NotAppliedPaths
          id={notAppliedId}
          testId="manual-link-not-applied"
          paths={notApplied}
        />
      )}
    </div>
  );
}

/**
 * A unit whose `[Install]` cannot be relied on: its reason, its install
 * files and the export's block, as given. No toggle and no keyboard
 * action.
 */
function UndeterminedInstallRow({
  record: r,
  unitKey,
}: {
  record: UndeterminedInstallView;
  unitKey: RowKey;
}) {
  const baseId = useId();
  const advisoriesId = `${baseId}-advisories`;
  const notAppliedId = `${baseId}-not-applied`;
  const notApplied = r.not_applied ?? [];
  return (
    <div
      role="row"
      aria-label={r.unit}
      aria-describedby={describedBy(
        r.advisories.length > 0 && advisoriesId,
        notApplied.length > 0 && notAppliedId,
      )}
      tabIndex={-1}
      data-testid={`undetermined-install-${r.unit}`}
      data-unit-key={unitKey}
      data-reason={r.reason}
      className="inspectah-decision-row inspectah-undetermined-install"
    >
      <div className="inspectah-decision-row__main">
        <div role="gridcell" className="inspectah-decision-row__name">
          <span>{r.unit}</span>
          <span className="inspectah-unit-kind" data-testid="undetermined-install-reason">
            {r.reason.replace(/_/g, " ")}
          </span>
          {r.install_files.map((f) => (
            <code key={f} className="inspectah-unit-paths__path" data-testid="install-file">
              {f}
            </code>
          ))}
        </div>
        <CauseLabel text={UNDETERMINED_CAUSE_LABEL} />
      </div>
      {r.advisories.length > 0 && (
        <AdvisoryLines
          id={advisoriesId}
          testId="undetermined-install-advisory"
          lines={r.advisories}
        />
      )}
      {notApplied.length > 0 && (
        <NotAppliedPaths
          id={notAppliedId}
          testId="undetermined-install-not-applied"
          paths={notApplied}
        />
      )}
    </div>
  );
}

interface DefaultTargetRowProps {
  target: DefaultTargetView;
  unitKey: RowKey;
  isPending: boolean;
  onToggle: (currentInclude: boolean) => void;
  rowRef: RowRef;
}

/**
 * The default target the export sets with `systemctl set-default`, and
 * its toggle. `declined` is the export's reason when it writes no line.
 */
function DefaultTargetRow({ target: t, unitKey, isPending, onToggle, rowRef }: DefaultTargetRowProps) {
  const declinedId = "declined-default-target";
  return (
    <div
      ref={rowRef}
      role="row"
      aria-label={t.target}
      tabIndex={0}
      data-testid="default-target"
      data-unit-key={unitKey}
      data-locked={t.locked ? "true" : undefined}
      className={`inspectah-decision-row${t.locked ? " inspectah-decision-row--locked" : ""}`}
      onKeyDown={(e) => {
        if (e.target !== e.currentTarget) return;
        if (t.locked || isPending) return;
        if (e.key === " " || e.key === "x") {
          e.preventDefault();
          onToggle(t.include);
        }
      }}
    >
      <div className="inspectah-decision-row__main">
        <div role="gridcell" className="inspectah-decision-row__toggle">
          <input
            type="checkbox"
            role="checkbox"
            id="switch-default-target"
            checked={t.include}
            onChange={() => onToggle(t.include)}
            disabled={isPending || t.locked}
            aria-label={t.locked ? `${t.target} (locked)` : `Toggle ${t.target}`}
            aria-describedby={describedBy(t.declined && declinedId)}
            style={{ minWidth: 20, minHeight: 20 }}
          />
        </div>
        <div role="gridcell" className="inspectah-decision-row__name">
          <span data-testid="default-target-name">{t.target}</span>
        </div>
        {t.declined && <OutcomeLabel declined={t.declined} />}
      </div>
      <div className="inspectah-decision-row__helper-text inspectah-unit-note">
        {DEFAULT_TARGET_EXPLANATION}
      </div>
      {t.declined && (
        <RowNote
          id={declinedId}
          testId={declinedId}
          className={`inspectah-unit-declined inspectah-unit-declined--${t.declined.renders}`}
          text={t.declined.text}
        />
      )}
    </div>
  );
}

interface UnitBranchViewProps {
  branch: UnitBranch;
  /** The kind chip, shown only where the group mixes kinds (§ 6.6 item 10). */
  showKind: boolean;
  /** Holds the section's one tab stop (today's rule: the first unit row, when there is no target). */
  isTabStop: boolean;
  pendingKeys: ReadonlySet<string>;
  /** The toggled row's occurrence key comes first: it decides the pin (§ 6.4). */
  onToggleService: (toggled: OccurrenceKey, unit: string, currentInclude: boolean) => void;
  onToggleDropin: (toggled: OccurrenceKey, path: string, currentInclude: boolean) => void;
  rowRef: (key: OccurrenceKey) => RowRef;
}

/** A unit row and the drop-ins nested under it (§ 6.2, § 6.12). */
function UnitBranchView({
  branch,
  showKind,
  isTabStop,
  pendingKeys,
  onToggleService,
  onToggleDropin,
  rowRef,
}: UnitBranchViewProps) {
  const svc = branch.head.record.data;
  const bucket = extractTriageBucket(svc.triage);
  const badge =
    svc.triage.primary_reason === NON_DEFAULT_STATE_REASON ? null : badgeTextForTriage(svc.triage);
  const level = triageBucketToAttention(svc.triage);
  const isPending = pendingKeys.has(serviceKey(svc.unit));
  const svcLocked = svc.locked === true;
  const borderLeft = LEVEL_BORDER[bucket] ?? "none";
  // Test ids stay the unit's; DOM ids are the row's own (see ownId).
  const generatedId = useId();
  const rowId = (base: string) => ownId(base, branch.head.ordinal, generatedId);
  const lockedReasonTestId = `locked-reason-service-${svc.unit}`;
  // A locked row shows its reason in the badge; an unlocked one, such
  // as a runtime updater excluded by default, under the row.
  const attentionTestId = `attention-service-${svc.unit}`;
  // One reason per row (§ 6.6 item 4): a declined row shows its declined
  // text and not its attention_reason.
  const attention = svcLocked || svc.declined ? null : (svc.attention_reason ?? null);
  const declinedTestId = `declined-service-${svc.unit}`;
  // What an instance needs (§ 11.1): its template, the configuration
  // its enable needs and the data the image does not carry. Any
  // advisory about them is already in attention_reason.
  const instanceOfTestId = `instance-of-${svc.unit}`;
  const requiredConfigTestId = `required-config-${svc.unit}`;
  const dataPathsTestId = `data-paths-${svc.unit}`;
  const lockedReasonId = rowId(lockedReasonTestId);
  const attentionId = rowId(attentionTestId);
  const declinedId = rowId(declinedTestId);
  const instanceOfId = rowId(instanceOfTestId);
  const requiredConfigId = rowId(requiredConfigTestId);
  const dataPathsId = rowId(dataPathsTestId);

  return (
    <div data-testid={`service-group-${svc.unit}`}>
      {/* Parent service row */}
      <div
        ref={rowRef(branch.head.occurrenceKey)}
        role="row"
        aria-label={svc.unit}
        tabIndex={isTabStop ? 0 : -1}
        data-testid={`service-item-${svc.unit}`}
        data-unit-key={branch.head.rowKey}
        data-locked={svcLocked ? "true" : undefined}
        className={`inspectah-decision-row${svcLocked ? " inspectah-decision-row--locked" : ""}`}
        style={{ borderLeft }}
        onKeyDown={(e) => {
          if (e.target !== e.currentTarget) return;
          if (svcLocked) return;
          if (e.key === " " || e.key === "x") {
            e.preventDefault();
            onToggleService(branch.head.occurrenceKey, svc.unit, svc.include);
          }
        }}
      >
        <div className="inspectah-decision-row__main">
          <div role="gridcell" className="inspectah-decision-row__toggle">
            <input
              type="checkbox"
              role="checkbox"
              id={rowId(`switch-service-${svc.unit}`)}
              checked={svc.include}
              onChange={() => onToggleService(branch.head.occurrenceKey, svc.unit, svc.include)}
              disabled={isPending || svcLocked}
              aria-label={
                svcLocked
                  ? `${svc.unit} (locked: ${svc.attention_reason ?? "cannot toggle"})`
                  : `Toggle ${svc.unit}`
              }
              aria-describedby={describedBy(
                svcLocked && lockedReasonId,
                attention && attentionId,
                svc.declined && declinedId,
                svc.instance_of && instanceOfId,
                svc.required_config.length > 0 && requiredConfigId,
                svc.data_paths.length > 0 && dataPathsId,
              )}
              style={{ minWidth: 20, minHeight: 20 }}
            />
          </div>
          <div role="gridcell" className="inspectah-decision-row__name">
            <span>{svc.unit}</span>
            {showKind && (
              <span
                className="inspectah-unit-kind"
                data-testid={`service-kind-${svc.unit}`}
              >
                {svc.kind}
              </span>
            )}
            {/* The package, or with none the fragment word (§ 6.6 item 9). */}
            <span
              style={{
                fontSize: "var(--pf-t--global--font--size--xs)",
                opacity: 0.6,
                marginLeft: "var(--pf-t--global--spacer--xs)",
              }}
            >
              ({svc.owning_package ?? svc.fragment})
            </span>
            {svc.instance_of && (
              <span
                id={instanceOfId}
                data-testid={instanceOfTestId}
                className="inspectah-unit-paths"
              >
                <span className="inspectah-unit-paths__label">Instance of</span>{" "}
                <code>{svc.instance_of}</code>
              </span>
            )}
            <span
              data-testid={`service-state-${svc.unit}`}
              style={{ marginLeft: "var(--pf-t--global--spacer--sm)" }}
            >
              {svc.current_state}
              {svc.default_state && (
                <>
                  {" "}
                  <span style={{ color: SUBTLE_TEXT_COLOR }}>preset: {svc.default_state}</span>
                </>
              )}
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
              id={lockedReasonId}
              data-testid={`locked-badge-service-${svc.unit}`}
            >
              <Label color="grey" isCompact>
                {svc.attention_reason ?? "LOCKED"}
              </Label>
            </div>
          )}
          {/* After the triage badge, as § 6.10's sketch draws it. */}
          {svc.declined && <OutcomeLabel declined={svc.declined} />}
        </div>
        {attention && (
          <RowNote id={attentionId} testId={attentionTestId} text={attention} />
        )}
        {svc.declined && (
          <RowNote
            id={declinedId}
            testId={declinedTestId}
            className={`inspectah-unit-declined inspectah-unit-declined--${svc.declined.renders}`}
            text={svc.declined.text}
          />
        )}
        {svc.also && svc.also.length > 0 && (
          <div className="inspectah-unit-paths inspectah-unit-also">
            <span className="inspectah-unit-paths__label">Also=</span>
            {svc.also.map((m) => (
              <span key={m.unit} data-testid="also-member">
                <code data-testid="also-member-unit">{m.unit}</code>{" "}
                <span className="inspectah-unit-state" data-testid="also-member-links">
                  {m.links}
                </span>
              </span>
            ))}
          </div>
        )}
        {svc.required_config.length > 0 && (
          <InstancePaths
            id={requiredConfigId}
            testId={requiredConfigTestId}
            label="Needs configuration:"
            pathTestId="required-config-path"
            paths={svc.required_config}
          />
        )}
        {svc.data_paths.length > 0 && (
          <InstancePaths
            id={dataPathsId}
            testId={dataPathsTestId}
            label="Keeps data in:"
            pathTestId="data-path"
            paths={svc.data_paths}
          />
        )}
      </div>

      {/* Drop-in children */}
      {branch.dropins.map((di) => (
        <DropInRow
          key={di.occurrenceKey}
          dropin={di.record.data}
          unitKey={di.rowKey}
          ordinal={di.ordinal}
          parentExcluded={!svc.include}
          parentLocked={svcLocked}
          isInstallDropIn={svc.install_drop_ins?.includes(di.record.data.path) ?? false}
          isPending={pendingKeys.has(dropinKey(di.record.data.path))}
          onToggle={(path, include) => onToggleDropin(di.occurrenceKey, path, include)}
          rowRef={rowRef(di.occurrenceKey)}
        />
      ))}
    </div>
  );
}

/** A reported row (§ 6.7): no toggle, its kind's row as before. */
function ReportedRowView({
  row,
  onNavigateToConfig,
}: {
  row: ReportedRow;
  onNavigateToConfig?: (path: string) => void;
}) {
  const { record } = row;
  switch (record.kind) {
    case "mount_finding":
      return (
        <MountFindingRow
          finding={record.data}
          unitKey={row.rowKey}
          ordinal={row.ordinal}
          onNavigateToConfig={onNavigateToConfig}
        />
      );
    case "manual_link":
      return <ManualLinkRow link={record.data} unitKey={row.rowKey} />;
    case "undetermined_install":
      return <UndeterminedInstallRow record={record.data} unitKey={row.rowKey} />;
  }
}

export function ServiceSection({
  services,
  dropins,
  mountFindings,
  manualLinks,
  undeterminedInstalls,
  defaultTarget,
  onNavigateToConfig,
  onViewUpdate,
  onMutationError,
  pendingGuard,
}: ServiceSectionProps) {
  const { pendingKeys, runGuarded } = pendingGuard;

  // Keys are assigned once per view response, then grouped (§ 6.11, § 6.2).
  const keyed = useMemo(
    () =>
      keyFields({
        unit_states: services,
        unit_dropins: dropins,
        default_target: defaultTarget,
        mount_findings: mountFindings,
        manual_links: manualLinks,
        undetermined_installs: undeterminedInstalls,
      }),
    [services, dropins, defaultTarget, mountFindings, manualLinks, undeterminedInstalls],
  );
  const groups = useMemo(() => groupUnits(keyed), [keyed]);
  const counts = useMemo(() => outcomeCounts(keyed), [keyed]);
  const barDrawn = filterBarDrawn(counts);

  // The filter is "All" on mount and lives only here: not in the URL or
  // localStorage (§ 6.4 Persistence).
  const [filter, setFilter] = useState<FilterValue>("all");
  const [announcement, setAnnouncement] = useState("");
  // The bar stopped being drawn: the filter returns to "All", and the live
  // region starts empty when the bar returns. Adjusted during render, so
  // no frame shows a filter with no bar.
  if (!barDrawn && (filter !== "all" || announcement !== "")) {
    setFilter("all");
    setAnnouncement("");
  }

  // Pinned branches, by the occurrence key of the row heading each (§ 6.4,
  // § 12). They go with the section on unmount. The filter's return to
  // "All" above clears none: it follows a toggle.
  const [pins, setPins] = useState<ReadonlySet<OccurrenceKey>>(NO_PINS);
  // Each view keeps only the pins whose occurrence key it still holds.
  const [pinsCheckedFor, setPinsCheckedFor] = useState(keyed);
  if (pinsCheckedFor !== keyed) {
    setPinsCheckedFor(keyed);
    setPins((prev) => pinsInView(prev, keyed));
  }
  /** Clears every pin: a filter change here; a query change or a reveal later (§ 6.11). */
  const clearPins = useCallback(() => setPins(NO_PINS), []);

  const shown = useMemo(
    () => showGroups(groups, { filter, query: NO_QUERY, pins }),
    [groups, filter, pins],
  );

  const changeFilter = useCallback(
    (value: FilterValue) => {
      setFilter(value);
      clearPins();
      const next = showGroups(groups, { filter: value, query: NO_QUERY, pins: NO_PINS });
      setAnnouncement(liveRegionText(value, shownOutcomeRowCount(next)));
    },
    [groups, clearPins],
  );

  // Toggleable rows by occurrence key, so focus returns to the occurrence
  // the operator toggled (§ 6.13), never to a twin with its row key.
  const rowElements = useRef(new Map<OccurrenceKey, HTMLDivElement>());
  const rowRef = useCallback(
    (key: OccurrenceKey): RowRef =>
      (el) => {
        if (el) rowElements.current.set(key, el);
        else rowElements.current.delete(key);
      },
    [],
  );
  const [focusRequest, setFocusRequest] = useState<{ key: OccurrenceKey } | null>(null);
  // After the op settles, the toggled row gets focus back if focus was lost
  // meanwhile (a browser blurs a control disabled while its request runs).
  // Focus the operator moved elsewhere stays where it is.
  useLayoutEffect(() => {
    if (!focusRequest) return;
    const active = document.activeElement;
    if (active === null || active === document.body) {
      rowElements.current.get(focusRequest.key)?.focus();
    }
  }, [focusRequest]);

  // Every group opens on mount; the state goes with the section (§ 6.2).
  const [collapsed, setCollapsed] = useState<Record<GroupId, boolean>>(NONE_COLLAPSED);
  const toggleGroup = useCallback(
    (id: GroupId) => setCollapsed((prev) => ({ ...prev, [id]: !prev[id] })),
    [],
  );

  // Every toggle the operator makes pins the branch that holds the row
  // (§ 6.4), before the op leaves, so a failed op leaves it shown too.
  const sendInclude = useCallback(
    (key: string, op: RefinementOp, toggled: OccurrenceKey) => {
      const head = branchHeadOf(groups, toggled);
      setPins((prev) => (prev.has(head) ? prev : new Set(prev).add(head)));
      runGuarded(key, () =>
        applyOp(op)
          .then(onViewUpdate)
          .catch((err) =>
            onMutationError(
              err instanceof Error ? err : new Error(String(err)),
            ),
          )
          .finally(() => setFocusRequest({ key: toggled })),
      );
    },
    [groups, runGuarded, onViewUpdate, onMutationError],
  );

  const handleToggleService = useCallback(
    (toggled: OccurrenceKey, unit: string, currentInclude: boolean) => {
      sendInclude(serviceKey(unit), {
        op: "SetInclude",
        target: {
          item_id: { kind: "Unit", key: { unit } },
          include: !currentInclude,
        },
      }, toggled);
    },
    [sendInclude],
  );

  const handleToggleDropin = useCallback(
    (toggled: OccurrenceKey, path: string, currentInclude: boolean) => {
      sendInclude(dropinKey(path), {
        op: "SetInclude",
        target: {
          item_id: { kind: "DropIn", key: { path } },
          include: !currentInclude,
        },
      }, toggled);
    },
    [sendInclude],
  );

  const handleToggleDefaultTarget = useCallback(
    (toggled: OccurrenceKey, currentInclude: boolean) => {
      sendInclude(DEFAULT_TARGET_KEY, {
        op: "SetInclude",
        target: {
          item_id: { kind: "DefaultTarget" },
          include: !currentInclude,
        },
      }, toggled);
    },
    [sendInclude],
  );

  const noOtherRow =
    services.length === 0 &&
    dropins.length === 0 &&
    mountFindings.length === 0 &&
    manualLinks.length === 0 &&
    undeterminedInstalls.length === 0;

  // Today's tab stop: the target, else the first shown unit row.
  const targetShown = shown.some((g) => g.target);
  const tabStop = targetShown
    ? undefined
    : shown.find((g) => g.branches.length > 0)?.branches[0].head.occurrenceKey;

  // § 6.9: a value that shows no row. Section search's own empty state
  // (row 5) will take precedence over this one.
  const filterEmpty = filter !== "all" && shown.length === 0;

  return (
    <div
      className="inspectah-service-section"
      role="region"
      aria-label={SECTION_NAME}
      data-testid="service-section"
    >
      {barDrawn && (
        <UnitOutcomeFilter
          counts={counts}
          value={filter}
          onChange={changeFilter}
          announcement={announcement}
        />
      )}

      {filterEmpty && (
        <p className="inspectah-unit-filter__empty" style={{ padding: "var(--pf-t--global--spacer--md)" }}>
          {FILTER_EMPTY}{" "}
          <Button variant="link" isInline onClick={() => changeFilter("all")}>
            {SHOW_ALL_LABEL}
          </Button>
        </p>
      )}

      {shown.map((group) => (
        <UnitGroup
          key={group.id}
          groupId={group.id}
          title={GROUP_TITLES[group.id]}
          count={group.id === "default_target" ? undefined : group.count}
          followUpText={group.followUp === null ? null : followUpFlagText(group.followUp)}
          description={group.id === "reported" ? REPORTED_DESCRIPTION : undefined}
          expanded={!collapsed[group.id]}
          onToggle={() => toggleGroup(group.id)}
        >
          {group.target && (
            <DefaultTargetRow
              key={group.target.occurrenceKey}
              target={group.target.record.data}
              unitKey={group.target.rowKey}
              isPending={pendingKeys.has(DEFAULT_TARGET_KEY)}
              onToggle={(include) => handleToggleDefaultTarget(group.target!.occurrenceKey, include)}
              rowRef={rowRef(group.target.occurrenceKey)}
            />
          )}

          {group.branches.map((branch) => (
            <UnitBranchView
              key={branch.head.occurrenceKey}
              branch={branch}
              showKind={group.id === "other_units"}
              isTabStop={branch.head.occurrenceKey === tabStop}
              pendingKeys={pendingKeys}
              onToggleService={handleToggleService}
              onToggleDropin={handleToggleDropin}
              rowRef={rowRef}
            />
          ))}

          {/* Drop-ins whose unit has no state row: a unit at its preset, or one
              with an undetermined or install-difference record, never reaches
              `state_changes`, so without this block the drop-in row — which
              owns the decision for a config path it shadows — would render
              nowhere. It closes the group of the parent's kind (§ 6.8). */}
          {group.orphans.length > 0 && (
            <div data-testid="dropin-orphans">
              <h3 className="inspectah-service-section__orphan-heading">
                Other drop-in overrides
              </h3>
              <p className="inspectah-service-section__orphan-note">
                These overrides have no unit-state decision row.
              </p>
              {group.orphans.map((block) => (
                <div key={block.unit} data-testid={`dropin-orphan-group-${block.unit}`}>
                  <div className="inspectah-service-section__orphan-unit">
                    {block.unit}
                  </div>
                  {block.dropins.map((di) => (
                    <DropInRow
                      key={di.occurrenceKey}
                      dropin={di.record.data}
                      unitKey={di.rowKey}
                      ordinal={di.ordinal}
                      parentExcluded={false}
                      parentLocked={false}
                      isPending={pendingKeys.has(dropinKey(di.record.data.path))}
                      onToggle={(path, include) => handleToggleDropin(di.occurrenceKey, path, include)}
                      rowRef={rowRef(di.occurrenceKey)}
                    />
                  ))}
                </div>
              ))}
            </div>
          )}

          {group.reported
            .flatMap((block) => block.rows)
            .map((row) => (
              <ReportedRowView
                key={row.occurrenceKey}
                row={row}
                onNavigateToConfig={onNavigateToConfig}
              />
            ))}
        </UnitGroup>
      ))}

      {noOtherRow && (
        <p style={{ padding: "var(--pf-t--global--spacer--md)", opacity: 0.6 }}>
          {defaultTarget ? EMPTY_WITH_TARGET : EMPTY_SECTION}
        </p>
      )}
    </div>
  );
}
