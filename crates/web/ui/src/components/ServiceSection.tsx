import { useCallback, useId, useMemo } from "react";
import { Button, EmptyState, EmptyStateBody, Label } from "@patternfly/react-core";
import type {
  ServiceDecisionDto,
  DropInDecisionDto,
  MountFindingDto,
  ManualLinkView,
  UndeterminedInstallView,
  DefaultTargetView,
  ViewResponse,
} from "../api/types";
import type { PendingGuard } from "../hooks/usePendingGuard";
import { useUnitsSectionState, type RowRef } from "../hooks/useUnitsSectionState";
import { extractTriageBucket } from "./attentionUtils";
import {
  GROUP_DESCRIPTIONS,
  GROUP_TITLES,
  QUIET_HOST_SENTENCES,
  causeName,
  defaultTargetPresentation,
  dropinRowPresentation,
  followUpFlagText,
  reportedRowLabels,
  rowKeyIgnored,
  unitRowKeySet,
  unitRowPresentation,
  type GroupId,
  type OccurrenceKey,
  type ReportedRow,
  type RowKey,
  type RowLabel,
  type RowPath,
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
  /**
   * The item a navigation is revealing (§ 6.11): a row key, or a bare unit
   * name for a unit row. The section shows its row before `App` looks it up.
   */
  revealItemId?: string;
  /** Navigates to an item, as `App`'s `handleNavigateFromSearch` does. */
  onNavigateToItem?: (sectionId: string, itemId: string) => void;
  /** The section query (§ 6.12), owned by `MainContent`; empty for none. */
  filterText?: string;
  /** Receives `shownRowCount` for the search box's result count (§ 6.12). */
  onResultCount?: (n: number) => void;
  /**
   * Clears the section query: reveal step 1 (§ 6.11) and the search empty
   * state's "Clear filter" button. The owner of the text clears it.
   */
  onClearQuery?: () => void;
  /** Focuses the section search box, where "Clear filter" hands its focus (§ 6.13). */
  onFocusSearch?: () => void;
  /**
   * Receives the function that focuses the first shown row, which
   * `MainContent`'s `handleArrowDown` calls from the search box (§ 6.12),
   * and `null` on unmount.
   */
  registerFocusFirstRow?: (focusFirstRow: (() => void) | null) => void;
  /**
   * The paths `config_files` holds: a `required_config` path among them
   * links to its config row (§ 6.6 item 7).
   */
  configPaths?: ReadonlySet<string>;
  /**
   * The item id a navigation to a Configuration path reveals, which a
   * `required_config` link also writes as its hash, so the two name the
   * same target: `configs:<path>` for a config row, the machine-state
   * row's id for a class path.
   */
  configItemId?: (path: string) => string;
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

/** § 7 string 1: the section's accessible name. */
const SECTION_NAME = "Systemd units";

/** § 7 string 11, under the default target row. */
const DEFAULT_TARGET_EXPLANATION =
  "The target the source host boots to. Included, the image boots to it too.";

/** § 7 string 23, a drop-in's badge while its parent is excluded. */
const UNIT_EXCLUDED = "Unit excluded";

/**
 * The class that dims a dimmed row's toggle and name cells. The row keeps
 * full opacity, so its focus outline and badges keep theirs (§ 6.13).
 */
const DIMMED_CELL = "inspectah-unit-dimmed";

/** A row cell's class, with the dimming class when the row is dimmed. */
function cellClass(base: string, dimmed: boolean): string {
  return dimmed ? `${base} ${DIMMED_CELL}` : base;
}

/**
 * Subtle text for the preset beside a row's state (§ 6.6 item 1), an
 * Also= member's links and a mount finding's state.
 */
const SUBTLE_TEXT_COLOR = "var(--pf-t--global--text--color--subtle)";

/** § 7 string 14: a filter value that shows no row, and its link button. */
const FILTER_EMPTY = "No units match this filter.";
const SHOW_ALL_LABEL = "Show all";

/** § 6.9 row 5: section search with no match, as in the other sections. */
const SEARCH_EMPTY_TITLE = "No items match your search";
const CLEAR_QUERY_LABEL = "Clear filter";

/** No config row to link to. */
const NO_CONFIG_PATHS: ReadonlySet<string> = new Set();

/** A config row's item id, the one `DecisionItem` gives it. */
function configRowItemId(path: string): string {
  return `configs:${path}`;
}

/** The section id an `Also=` member link navigates to (§ 6.11). */
const UNITS_SECTION_ID = "units";

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

/** A row's `tabIndex`: 0 on the row holding the section's one tab stop (§ 6.13), else -1. */
function tabIndexFor(key: OccurrenceKey, tabStop: OccurrenceKey | null): number {
  return key === tabStop ? 0 : -1;
}

/** A declined row's outcome label in its badge cell (§ 6.6 item 2, § 7 string 15). */
function OutcomeLabel({ label }: { label: RowLabel }) {
  return (
    <div role="gridcell" className="inspectah-decision-row__badge">
      <Label color={label.color} variant="outline" isCompact>
        {label.text}
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

interface DropInRowProps {
  dropin: DropInDecisionDto;
  /** The row key (§ 6.11), carried in `data-unit-key`. */
  unitKey: RowKey;
  /** The record's ordinal among the records with its row key (§ 6.11). */
  ordinal: number;
  /** Its unit row, or `null` for an orphan drop-in (§ 6.8). */
  parent: ServiceDecisionDto | null;
  isPending: boolean;
  onToggle: (path: string, currentInclude: boolean) => void;
  rowRef: RowRef;
  /** 0 on the row holding the section's one tab stop (§ 6.13), else -1. */
  tabIndex: number;
}

function DropInRow({
  dropin: di,
  unitKey,
  ordinal,
  parent,
  isPending,
  onToggle,
  rowRef,
  tabIndex,
}: DropInRowProps) {
  const p = dropinRowPresentation(di, parent);
  const diBucket = extractTriageBucket(di.triage);
  const diLocked = p.locked;
  const parentExcluded = p.parentExcluded;
  const disabled = !p.toggleable || isPending;
  const isShadow = p.fullShadow;
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
      tabIndex={tabIndex}
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
        transition: "background-color 150ms ease",
      }}
      onKeyDown={(e) => {
        if (e.target !== e.currentTarget || rowKeyIgnored(e)) return;
        if (disabled) return;
        if (e.key === " " || e.key === "x") {
          e.preventDefault();
          onToggle(di.path, di.include);
        }
      }}
    >
      <div className="inspectah-service-dropin__connector" aria-hidden="true" />
      <div className="inspectah-decision-row__main">
        <div role="gridcell" className={cellClass("inspectah-decision-row__toggle", p.dimmed)}>
          <input
            type="checkbox"
            role="checkbox"
            id={ownId(`switch-dropin-${di.path}`, ordinal, noteId)}
            checked={di.include}
            onChange={() => onToggle(di.path, di.include)}
            disabled={disabled}
            aria-label={p.toggleLabel}
            aria-disabled={parentExcluded}
            aria-describedby={describedBy(
              p.shadowRationale !== null && rationaleId,
              di.withheld && withheldId,
            )}
            style={{ minWidth: 20, minHeight: 20 }}
          />
        </div>
        <div role="gridcell" className={cellClass("inspectah-decision-row__name", p.dimmed)}>
          <span style={{ fontSize: "var(--pf-t--global--font--size--sm)" }}>
            {di.path}
          </span>
        </div>
        {p.unitExcluded && (
          <div role="gridcell" className="inspectah-decision-row__badge">
            <Label color="grey" data-testid={`dropin-excluded-badge-${di.path}`}>
              {UNIT_EXCLUDED}
            </Label>
          </div>
        )}
        {p.triageBadge && (
          <div role="gridcell" className="inspectah-decision-row__badge">
            <Label color={p.triageBadge.color}>{p.triageBadge.text}</Label>
          </div>
        )}
        {p.installDropIn && (
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
              {p.lockedLabel}
            </Label>
          </div>
        )}
      </div>
      {/* Full-shadow rationale below the drop-in toggle row */}
      {p.shadowRationale !== null && (
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
          {p.shadowRationale}
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
  rowRef: RowRef;
  tabIndex: number;
}

/**
 * An administrator's mount, automount or swap unit: what the host had and
 * the export's advisory lines, as given. No toggle and no keyboard action:
 * the export decides nothing here. The unit file is decided on its config
 * row, which `unit_file_path` names.
 */
function MountFindingRow({
  finding: f,
  unitKey,
  ordinal,
  onNavigateToConfig,
  rowRef,
  tabIndex,
}: MountFindingRowProps) {
  const advisoriesId = ownId(`mount-finding-advisories-${f.unit}`, ordinal, useId());
  const labels = reportedRowLabels({ kind: "mount_finding", data: f });
  const path = f.unit_file_path;
  return (
    <div
      ref={rowRef}
      role="row"
      aria-label={f.unit}
      aria-describedby={describedBy(f.advisories.length > 0 && advisoriesId)}
      tabIndex={tabIndex}
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
            {labels.detail}
          </span>
          {f.state !== null && (
            <span
              className="inspectah-unit-state"
              data-testid={`mount-finding-state-${f.unit}`}
              style={{ color: SUBTLE_TEXT_COLOR }}
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
  onNavigateToConfig,
  configItemId = configRowItemId,
}: {
  id: string;
  testId: string;
  label: string;
  pathTestId: string;
  /** Each path, and whether it links to its config row (§ 6.6 item 7). */
  paths: RowPath[];
  onNavigateToConfig?: (path: string) => void;
  /** The item id a link's hash names, as `ServiceSectionProps.configItemId`. */
  configItemId?: (path: string) => string;
}) {
  return (
    <div id={id} role="gridcell" data-testid={testId} className="inspectah-unit-paths">
      <span className="inspectah-unit-paths__label">{label}</span>
      {/* The spaces are not drawn in the flex row; they keep the paths
          apart in the text a screen reader reads from aria-describedby. */}
      {/* The path is a safe key: the collector and the fleet merge both
          dedup these lists. */}
      {paths.map(({ path: p, linked }) => (
        <span key={p}>
          {" "}
          {onNavigateToConfig && linked ? (
            <a
              href={`#${configItemId(p)}`}
              data-testid={pathTestId}
              onClick={(e) => {
                e.preventDefault();
                onNavigateToConfig(p);
              }}
            >
              {p}
            </a>
          ) : (
            <code data-testid={pathTestId}>{p}</code>
          )}
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
function ManualLinkRow({
  link,
  unitKey,
  rowRef,
  tabIndex,
}: {
  link: ManualLinkView;
  unitKey: RowKey;
  rowRef: RowRef;
  tabIndex: number;
}) {
  const baseId = useId();
  const advisoriesId = `${baseId}-advisories`;
  const notAppliedId = `${baseId}-not-applied`;
  const notApplied = link.not_applied ?? [];
  const labels = reportedRowLabels({ kind: "manual_link", data: link });
  return (
    <div
      ref={rowRef}
      role="row"
      aria-label={link.unit}
      aria-describedby={describedBy(
        link.advisories.length > 0 && advisoriesId,
        notApplied.length > 0 && notAppliedId,
      )}
      tabIndex={tabIndex}
      data-testid={`manual-link-${linkIdentity(link)}`}
      data-unit-key={unitKey}
      data-cause={causeName(link.cause)}
      className="inspectah-decision-row inspectah-manual-link"
    >
      <div className="inspectah-decision-row__main">
        <div role="gridcell" className="inspectah-decision-row__name">
          <span>{link.unit}</span>
          <span className="inspectah-unit-kind" data-testid="manual-link-relation">
            {labels.detail}
          </span>
          <code className="inspectah-manual-link__parent" data-testid="manual-link-parent">
            {link.parent}
          </code>
        </div>
        {labels.cause !== null && <CauseLabel text={labels.cause} />}
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
  rowRef,
  tabIndex,
}: {
  record: UndeterminedInstallView;
  unitKey: RowKey;
  rowRef: RowRef;
  tabIndex: number;
}) {
  const baseId = useId();
  const advisoriesId = `${baseId}-advisories`;
  const notAppliedId = `${baseId}-not-applied`;
  const notApplied = r.not_applied ?? [];
  const labels = reportedRowLabels({ kind: "undetermined_install", data: r });
  return (
    <div
      ref={rowRef}
      role="row"
      aria-label={r.unit}
      aria-describedby={describedBy(
        r.advisories.length > 0 && advisoriesId,
        notApplied.length > 0 && notAppliedId,
      )}
      tabIndex={tabIndex}
      data-testid={`undetermined-install-${r.unit}`}
      data-unit-key={unitKey}
      data-reason={r.reason}
      className="inspectah-decision-row inspectah-undetermined-install"
    >
      <div className="inspectah-decision-row__main">
        <div role="gridcell" className="inspectah-decision-row__name">
          <span>{r.unit}</span>
          <span className="inspectah-unit-kind" data-testid="undetermined-install-reason">
            {labels.detail}
          </span>
          {r.install_files.map((f) => (
            <code key={f} className="inspectah-unit-paths__path" data-testid="install-file">
              {f}
            </code>
          ))}
        </div>
        {labels.cause !== null && <CauseLabel text={labels.cause} />}
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
  tabIndex: number;
}

/**
 * The default target the export sets with `systemctl set-default`, and
 * its toggle. `declined` is the export's reason when it writes no line.
 */
function DefaultTargetRow({ target: t, unitKey, isPending, onToggle, rowRef, tabIndex }: DefaultTargetRowProps) {
  const declinedId = "declined-default-target";
  const p = defaultTargetPresentation(t);
  return (
    <div
      ref={rowRef}
      role="row"
      aria-label={t.target}
      tabIndex={tabIndex}
      data-testid="default-target"
      data-unit-key={unitKey}
      data-locked={p.locked ? "true" : undefined}
      className={`inspectah-decision-row${p.locked ? " inspectah-decision-row--locked" : ""}`}
      onKeyDown={(e) => {
        if (e.target !== e.currentTarget || rowKeyIgnored(e)) return;
        if (!p.toggleable || isPending) return;
        if (e.key === " " || e.key === "x") {
          e.preventDefault();
          onToggle(t.include);
        }
      }}
    >
      <div className="inspectah-decision-row__main">
        <div role="gridcell" className={cellClass("inspectah-decision-row__toggle", p.locked)}>
          <input
            type="checkbox"
            role="checkbox"
            id="switch-default-target"
            checked={t.include}
            onChange={() => onToggle(t.include)}
            disabled={isPending || !p.toggleable}
            aria-label={p.toggleLabel}
            aria-describedby={describedBy(t.declined && declinedId)}
            style={{ minWidth: 20, minHeight: 20 }}
          />
        </div>
        <div role="gridcell" className={cellClass("inspectah-decision-row__name", p.locked)}>
          <span data-testid="default-target-name">{t.target}</span>
        </div>
        {p.outcome && <OutcomeLabel label={p.outcome} />}
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
  /** The group the branch is drawn in, which decides the kind chip (§ 6.6 item 10). */
  groupId: GroupId;
  /** The occurrence key of the row holding the section's one tab stop (§ 6.13). */
  tabStop: OccurrenceKey | null;
  pendingKeys: ReadonlySet<string>;
  /** The toggled row's occurrence key comes first: it decides the pin (§ 6.4). */
  onToggleService: (toggled: OccurrenceKey, unit: string, currentInclude: boolean) => void;
  onToggleDropin: (toggled: OccurrenceKey, path: string, currentInclude: boolean) => void;
  rowRef: (key: OccurrenceKey) => RowRef;
  /** `unitRowKeySet` of the view, which an `Also=` member links to (§ 6.6 item 5). */
  unitRowKeys: ReadonlySet<RowKey>;
  onNavigateToItem?: (sectionId: string, itemId: string) => void;
  configPaths: ReadonlySet<string>;
  onNavigateToConfig?: (path: string) => void;
  configItemId?: (path: string) => string;
}

/** A unit row and the drop-ins nested under it (§ 6.2, § 6.12). */
function UnitBranchView({
  branch,
  groupId,
  tabStop,
  pendingKeys,
  onToggleService,
  onToggleDropin,
  rowRef,
  unitRowKeys,
  onNavigateToItem,
  configPaths,
  onNavigateToConfig,
  configItemId,
}: UnitBranchViewProps) {
  const svc = branch.head.record.data;
  const p = unitRowPresentation(svc, { groupId, unitRowKeys, configPaths });
  const bucket = extractTriageBucket(svc.triage);
  const isPending = pendingKeys.has(serviceKey(svc.unit));
  const svcLocked = p.locked;
  const borderLeft = LEVEL_BORDER[bucket] ?? "none";
  // Test ids stay the unit's; DOM ids are the row's own (see ownId).
  const generatedId = useId();
  const rowId = (base: string) => ownId(base, branch.head.ordinal, generatedId);
  const lockedReasonTestId = `locked-reason-service-${svc.unit}`;
  // A locked row shows its reason in the badge; an unlocked one, such
  // as a runtime updater excluded by default, under the row.
  const attentionTestId = `attention-service-${svc.unit}`;
  const attention = p.attentionNote;
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
        tabIndex={tabIndexFor(branch.head.occurrenceKey, tabStop)}
        data-testid={`service-item-${svc.unit}`}
        data-unit-key={branch.head.rowKey}
        data-locked={svcLocked ? "true" : undefined}
        className={`inspectah-decision-row${svcLocked ? " inspectah-decision-row--locked" : ""}`}
        style={{ borderLeft }}
        onKeyDown={(e) => {
          if (e.target !== e.currentTarget || rowKeyIgnored(e)) return;
          if (!p.toggleable) return;
          if (e.key === " " || e.key === "x") {
            e.preventDefault();
            onToggleService(branch.head.occurrenceKey, svc.unit, svc.include);
          }
        }}
      >
        <div className="inspectah-decision-row__main">
          <div role="gridcell" className={cellClass("inspectah-decision-row__toggle", svcLocked)}>
            <input
              type="checkbox"
              role="checkbox"
              id={rowId(`switch-service-${svc.unit}`)}
              checked={svc.include}
              onChange={() => onToggleService(branch.head.occurrenceKey, svc.unit, svc.include)}
              disabled={isPending || !p.toggleable}
              aria-label={p.toggleLabel}
              aria-describedby={describedBy(
                svcLocked && lockedReasonId,
                attention && attentionId,
                svc.declined && declinedId,
                svc.instance_of && instanceOfId,
                p.requiredConfig.length > 0 && requiredConfigId,
                p.dataPaths.length > 0 && dataPathsId,
              )}
              style={{ minWidth: 20, minHeight: 20 }}
            />
          </div>
          <div role="gridcell" className={cellClass("inspectah-decision-row__name", svcLocked)}>
            <span>{svc.unit}</span>
            {p.kindChip !== null && (
              <span
                className="inspectah-unit-kind"
                data-testid={`service-kind-${svc.unit}`}
              >
                {p.kindChip}
              </span>
            )}
            {/* The package, or with none the fragment word (§ 6.6 item 9). */}
            <span
              className="inspectah-unit-package"
              style={{
                fontSize: "var(--pf-t--global--font--size--xs)",
                marginLeft: "var(--pf-t--global--spacer--xs)",
              }}
            >
              ({p.packageWord})
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
          {p.triageBadge && (
            <div
              role="gridcell"
              className="inspectah-decision-row__badge"
            >
              <Label color={p.triageBadge.color}>{p.triageBadge.text}</Label>
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
                {p.lockedLabel}
              </Label>
            </div>
          )}
          {/* After the triage badge, as § 6.10's sketch draws it. */}
          {p.outcome && <OutcomeLabel label={p.outcome} />}
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
        {p.alsoMembers.length > 0 && (
          <div role="gridcell" className="inspectah-unit-paths inspectah-unit-also">
            <span className="inspectah-unit-paths__label">Also=</span>
            {p.alsoMembers.map((m) => {
              return (
                <span key={m.unit} data-testid="also-member">
                  {onNavigateToItem && m.linked ? (
                    <a
                      href={`#${UNITS_SECTION_ID}:${m.unit}`}
                      data-testid="also-member-unit"
                      onClick={(e) => {
                        e.preventDefault();
                        onNavigateToItem(UNITS_SECTION_ID, m.unit);
                      }}
                    >
                      {m.unit}
                    </a>
                  ) : (
                    <code data-testid="also-member-unit">{m.unit}</code>
                  )}{" "}
                  <span
                    className="inspectah-unit-state"
                    data-testid="also-member-links"
                    style={{ color: SUBTLE_TEXT_COLOR }}
                  >
                    {m.links}
                  </span>
                </span>
              );
            })}
          </div>
        )}
        {p.requiredConfig.length > 0 && (
          <InstancePaths
            id={requiredConfigId}
            testId={requiredConfigTestId}
            label="Needs configuration:"
            pathTestId="required-config-path"
            paths={p.requiredConfig}
            onNavigateToConfig={onNavigateToConfig}
            configItemId={configItemId}
          />
        )}
        {p.dataPaths.length > 0 && (
          <InstancePaths
            id={dataPathsId}
            testId={dataPathsTestId}
            label="Keeps data in:"
            pathTestId="data-path"
            paths={p.dataPaths}
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
          parent={svc}
          isPending={pendingKeys.has(dropinKey(di.record.data.path))}
          onToggle={(path, include) => onToggleDropin(di.occurrenceKey, path, include)}
          rowRef={rowRef(di.occurrenceKey)}
          tabIndex={tabIndexFor(di.occurrenceKey, tabStop)}
        />
      ))}
    </div>
  );
}

/** A reported row (§ 6.7): no toggle, its kind's row as before. */
function ReportedRowView({
  row,
  onNavigateToConfig,
  rowRef,
  tabIndex,
}: {
  row: ReportedRow;
  onNavigateToConfig?: (path: string) => void;
  rowRef: RowRef;
  tabIndex: number;
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
          rowRef={rowRef}
          tabIndex={tabIndex}
        />
      );
    case "manual_link":
      return <ManualLinkRow link={record.data} unitKey={row.rowKey} rowRef={rowRef} tabIndex={tabIndex} />;
    case "undetermined_install":
      return (
        <UndeterminedInstallRow record={record.data} unitKey={row.rowKey} rowRef={rowRef} tabIndex={tabIndex} />
      );
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
  revealItemId,
  onNavigateToItem,
  filterText,
  onResultCount,
  onClearQuery,
  onFocusSearch,
  registerFocusFirstRow,
  configPaths = NO_CONFIG_PATHS,
  configItemId,
  onViewUpdate,
  onMutationError,
  pendingGuard,
}: ServiceSectionProps) {
  const { pendingKeys, runGuarded } = pendingGuard;

  const {
    keyed,
    counts,
    barDrawn,
    filter,
    changeFilter,
    announcement,
    shown,
    emptyState,
    collapsed,
    toggleGroup,
    tabStop,
    rowRef,
    handleFocus,
    handleBlur,
    handleKeyDown,
    sendInclude,
    clearQueryFromEmptyState,
    showAllFromEmptyState,
  } = useUnitsSectionState({
    fields: {
      unit_states: services,
      unit_dropins: dropins,
      default_target: defaultTarget,
      mount_findings: mountFindings,
      manual_links: manualLinks,
      undetermined_installs: undeterminedInstalls,
    },
    filterText,
    revealItemId,
    onResultCount,
    onClearQuery,
    onFocusSearch,
    registerFocusFirstRow,
    runGuarded,
    onViewUpdate,
    onMutationError,
  });
  const unitRowKeys = useMemo(() => unitRowKeySet(keyed), [keyed]);

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

  return (
    <div
      className="inspectah-service-section inspectah-units-section"
      role="region"
      aria-label={SECTION_NAME}
      data-testid="service-section"
      onFocus={handleFocus}
      onBlur={handleBlur}
      onKeyDown={handleKeyDown}
    >
      {barDrawn && (
        <UnitOutcomeFilter
          counts={counts}
          value={filter}
          onChange={changeFilter}
          announcement={announcement}
        />
      )}

      {emptyState.notice === "search" && (
        <EmptyState titleText={SEARCH_EMPTY_TITLE} headingLevel="h3">
          <EmptyStateBody>
            <Button
              variant="link"
              onClick={(e) => clearQueryFromEmptyState(e.currentTarget.contains(document.activeElement))}
            >
              {CLEAR_QUERY_LABEL}
            </Button>
          </EmptyStateBody>
        </EmptyState>
      )}

      {emptyState.notice === "filter" && (
        <p className="inspectah-unit-filter__empty" style={{ padding: "var(--pf-t--global--spacer--md)" }}>
          {FILTER_EMPTY}{" "}
          <Button
            variant="link"
            isInline
            onClick={(e) => showAllFromEmptyState(e.currentTarget.contains(document.activeElement))}
          >
            {SHOW_ALL_LABEL}
          </Button>
        </p>
      )}

      {shown.map((group) => (
        <UnitGroup
          key={group.id}
          groupId={group.id}
          title={GROUP_TITLES[group.id]}
          count={group.count}
          followUpText={group.followUp === null ? null : followUpFlagText(group.followUp)}
          description={GROUP_DESCRIPTIONS[group.id]}
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
              tabIndex={tabIndexFor(group.target.occurrenceKey, tabStop)}
            />
          )}

          {group.branches.map((branch) => (
            <UnitBranchView
              key={branch.head.occurrenceKey}
              branch={branch}
              groupId={group.id}
              tabStop={tabStop}
              pendingKeys={pendingKeys}
              onToggleService={handleToggleService}
              onToggleDropin={handleToggleDropin}
              rowRef={rowRef}
              unitRowKeys={unitRowKeys}
              onNavigateToItem={onNavigateToItem}
              configPaths={configPaths}
              onNavigateToConfig={onNavigateToConfig}
              configItemId={configItemId}
            />
          ))}

          {/* Drop-ins whose unit has no state row: a unit at its preset, or one
              with an undetermined or install-difference record, never reaches
              `state_changes`, so without this block the drop-in row — which
              owns the decision for a config path it shadows — would render
              nowhere. It closes the group of the parent's kind (§ 6.8). */}
          {group.orphans.length > 0 && (
            <div data-testid="dropin-orphans">
              {/* Not a heading: the block sits in the group's rowgroup, which
                  may own rows only, and a generic element is transparent to it. */}
              <div className="inspectah-service-section__orphan-heading">
                Other drop-in overrides
              </div>
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
                      parent={null}
                      isPending={pendingKeys.has(dropinKey(di.record.data.path))}
                      onToggle={(path, include) => handleToggleDropin(di.occurrenceKey, path, include)}
                      rowRef={rowRef(di.occurrenceKey)}
                      tabIndex={tabIndexFor(di.occurrenceKey, tabStop)}
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
                rowRef={rowRef(row.occurrenceKey)}
                tabIndex={tabIndexFor(row.occurrenceKey, tabStop)}
              />
            ))}
        </UnitGroup>
      ))}

      {emptyState.quietHost !== null && (
        <p style={{ padding: "var(--pf-t--global--spacer--md)", opacity: 0.6 }}>
          {QUIET_HOST_SENTENCES[emptyState.quietHost]}
        </p>
      )}
    </div>
  );
}
