import { useEffect, useRef, useState } from "react";
import {
  Alert,
  AlertActionCloseButton,
  Button,
  ExpandableSection,
  Label,
  Modal,
  ModalBody,
  ModalFooter,
  ModalHeader,
} from "@patternfly/react-core";
import { KeyIcon } from "@patternfly/react-icons";
import type {
  MachineStateReason,
  MachineStateRowDto,
} from "../api/types";

/** DOM id of the block, the target of the Export dialog's review link. */
export const MACHINE_STATE_GROUP_ID = "machine-state-group";

/** Prefix of a machine-state row's GlobalSearch and reveal item id. */
export const MACHINE_STATE_ITEM_PREFIX = "machine-state:";

/** Prefix of a row's test id; the rest is the row's path, verbatim. */
export const MACHINE_STATE_ROW_TESTID_PREFIX = "machine-state-row-";

/**
 * The row's item id for GlobalSearch, reveal and links from other
 * sections, and its pending-guard and failure key: `machine-state:<path>`.
 * Paths are unique within `machine_state` and disjoint from `config_files`.
 */
export function machineStateItemId(path: string): string {
  return `${MACHINE_STATE_ITEM_PREFIX}${path}`;
}

/** Why a request on a row failed, shown on the row. */
export interface MachineStateFailure {
  /**
   * `not_carried`: the server refused the op as not carriable (422
   * `not carried:`), a statement about the file, not a fault.
   * `nothing_to_change`: the row already held the decision (422
   * `nothing to change:`); the owner refetches the view.
   * `failed`: anything else; the row keeps the server's current state.
   */
  kind: "not_carried" | "nothing_to_change" | "failed";
  text: string;
}

const REASON_LABELS: Record<MachineStateReason, string> = {
  host_credential: "Host credential",
  host_identity: "Host identity",
  generated: "Generated",
};

/** The reason label's text. An unknown reason reads as identity. */
export function reasonLabel(reason: MachineStateReason): string {
  return REASON_LABELS[reason] ?? REASON_LABELS.host_identity;
}

/** Shown for a notice kind or `by` this client does not know. */
const UNKNOWN_NOTICE = "See the audit report";

const AUTOMATIC_LINES: Record<string, string> = {
  first_boot: "Recreated at first boot",
  provisioning: "Set at provisioning by the installer, cloud-init or DHCP",
  carried_setting: "Rewritten from a setting the image carries",
};

/**
 * The NVMe host id member's line in place of its `automatic` notice, which
 * says first boot recreates the files. That is false on RHEL 9: the
 * rhel-bootc 9 base image ships one hostnqn and hostid that nothing
 * regenerates. Mirrors `NVME_HOST_ID_NOTICE` in
 * `crates/pipeline/src/render/machine_state.rs`, the audit report's text,
 * until the member's own notice is corrected.
 */
export const NVME_HOST_ID_LINE =
  "The RHEL 9 bootc base image ships its own /etc/nvme/hostnqn and hostid, " +
  "so every machine built from it shares one NVMe host identity until you " +
  "delete both files in the Containerfile or write new ones at provisioning. " +
  "RHEL 10 derives it from each machine's firmware UUID";

/** A row's notice line before any override: what replaces the file. */
function replacementLine(row: MachineStateRowDto): string {
  const notice = row.notice;
  switch (notice.kind) {
    case "owed":
      return `At provisioning: ${notice.command}`;
    case "automatic":
      if (row.member === "nvme_host_id") return NVME_HOST_ID_LINE;
      return AUTOMATIC_LINES[notice.by] ?? UNKNOWN_NOTICE;
    case "none":
      return "Not needed in the image";
    default:
      return UNKNOWN_NOTICE;
  }
}

/**
 * The row's notice line. An included row owes nothing, so its line is
 * the consequence of the override instead.
 */
export function noticeLine(row: MachineStateRowDto): string {
  if (row.include) {
    return row.override === "warn"
      ? "In the image: every machine built from it shares this identity"
      : "In the image, copied from this host";
  }
  return replacementLine(row);
}

/** The muted line under a row whose content the scan did not capture. */
export function metadataLine(row: MachineStateRowDto): string | null {
  if (row.content_captured) return null;
  const mode = row.mode === null ? "mode unknown" : `mode ${row.mode}`;
  const size =
    row.size === null
      ? "size unknown"
      : `${row.size} ${row.size === 1 ? "byte" : "bytes"}`;
  return `${mode} · ${size} · contents not captured`;
}

/** What section search matches a row on: path, reason label, notice line. */
export function machineStateSearchText(row: MachineStateRowDto): string {
  return `${row.path} ${reasonLabel(row.reason)} ${noticeLine(row)}`.toLowerCase();
}

/** The rows whose search text holds `query`; every row when it is blank. */
export function filterMachineStateRows(
  rows: MachineStateRowDto[],
  query: string,
): MachineStateRowDto[] {
  const q = query.trim().toLowerCase();
  if (!q) return rows;
  return rows.filter((row) => machineStateSearchText(row).includes(q));
}

function plural(n: number, one: string, many: string): string {
  return `${n} ${n === 1 ? one : many}`;
}

/** The toggle text, which carries the important facts while collapsed. */
export function machineStateToggleText(
  rows: MachineStateRowDto[],
  stepCount: number,
): string {
  const parts = [`Machine state (${rows.length})`];
  const credentials = rows.filter((r) => r.reason === "host_credential").length;
  if (credentials > 0) {
    parts.push(plural(credentials, "host credential", "host credentials"));
  }
  if (stepCount > 0) {
    parts.push(plural(stepCount, "provisioning step", "provisioning steps"));
  }
  const included = rows.filter((r) => r.include).length;
  if (included > 0) parts.push(`${included} included by you`);
  return parts.join(" · ");
}

/** The three sub-lists, in risk order. */
const SUBLISTS: {
  reason: MachineStateReason;
  heading: string;
  help: string;
}[] = [
  {
    reason: "host_credential",
    heading: "Host credentials",
    help: "Keys and certificates bound to this host. Their contents are left out of the configuration.",
  },
  {
    reason: "host_identity",
    heading: "Host identity",
    help: "Values that name this host.",
  },
  {
    reason: "generated",
    heading: "Generated",
    help: "Written by a tool from settings the image already carries.",
  },
];

/** The sub-list a row belongs to; an unknown reason joins identity. */
function sublistOf(row: MachineStateRowDto): MachineStateReason {
  return row.reason === "host_credential" || row.reason === "generated"
    ? row.reason
    : "host_identity";
}

/** The `warn` dialog's body: the row's alternative, from its notice. */
function warnAlternative(row: MachineStateRowDto): string {
  if (row.notice.kind === "owed") {
    return `Leave it out and run this at provisioning instead: ${row.notice.command}.`;
  }
  const line = replacementLine(row);
  return `Leave it out and it is ${line.charAt(0).toLowerCase()}${line.slice(1)}.`;
}

/** The `warn` dialog's title and lead sentence, by the row's reason. */
function warnWording(row: MachineStateRowDto): { title: string; lead: string } {
  if (row.reason === "generated") {
    return {
      title: "Include this generated file in the image?",
      lead: "Every machine built from this image will get this host's copy instead of one written for it.",
    };
  }
  return {
    title: "Include this host's identity in the image?",
    lead: "Every machine built from this image will share this identity.",
  };
}

const CONFIRM_CANCEL_ID = "machine-state-confirm-cancel";

const NO_PENDING: ReadonlySet<string> = new Set();
const NO_FAILURES: ReadonlyMap<string, MachineStateFailure> = new Map();

export interface MachineStateGroupProps {
  /** `machine_state` from the view, in server order (sorted by path). */
  rows: MachineStateRowDto[];
  /** `machine_state_steps.length`. */
  stepCount: number;
  /** The section search query. While set, only matching rows show. */
  filterText?: string;
  /**
   * Sends `SetInclude` with the row's `item_id` verbatim. Called only for
   * a row with an `item_id`, and for `warn` only after the dialog.
   */
  onSetInclude: (row: MachineStateRowDto, include: boolean) => void;
  /** Row item ids whose request is in flight. */
  pendingKeys?: ReadonlySet<string>;
  /** Failures by row item id, held by the owner. */
  failures?: ReadonlyMap<string, MachineStateFailure>;
  /** Clears a row's failure, when the operator dismisses it. */
  onDismissFailure?: (itemId: string) => void;
  /** A GlobalSearch or link reveal; expands the block when it names a row. */
  revealItemId?: string;
  /** When true, expand, scroll into view and focus the toggle, once. */
  reveal?: boolean;
  /** Called after `reveal` was handled, so the caller can clear it. */
  onRevealed?: () => void;
}

/**
 * Files that belong to this host, not to the image (spec § 8, Refine
 * presentation). Every row says why it is out and what replaces it; the
 * control comes from `override` alone, never from the path, member or
 * reason. Rows never move: an override changes a row's control in place.
 */
export function MachineStateGroup({
  rows,
  stepCount,
  filterText = "",
  onSetInclude,
  pendingKeys = NO_PENDING,
  failures = NO_FAILURES,
  onDismissFailure,
  revealItemId,
  reveal = false,
  onRevealed,
}: MachineStateGroupProps) {
  const [expanded, setExpanded] = useState(false);
  const sectionRef = useRef<HTMLElement>(null);
  const [confirmRow, setConfirmRow] = useState<MachineStateRowDto | null>(
    null,
  );
  // The row whose button takes focus once it can: after the dialog closes,
  // and after a request settles (a disabled button cannot take focus).
  const [focusAfter, setFocusAfter] = useState<string | null>(null);
  // Rows the operator changed here, by item id, with the value asked for;
  // announced once the view shows it.
  const requested = useRef(new Map<string, boolean>());
  const [announcement, setAnnouncement] = useState("");

  const searching = filterText.trim().length > 0;

  // A reveal of one of our rows expands the block in the same render, so
  // the App's reveal finds the row visible.
  const revealedRow =
    revealItemId !== undefined &&
    rows.some((r) => machineStateItemId(r.path) === revealItemId)
      ? revealItemId
      : undefined;
  const [lastReveal, setLastReveal] = useState<string | undefined>(undefined);
  if (revealedRow !== lastReveal) {
    setLastReveal(revealedRow);
    if (revealedRow !== undefined && !expanded) setExpanded(true);
  }
  // A search expands the block; clearing it leaves it expanded.
  const [wasSearching, setWasSearching] = useState(false);
  if (searching !== wasSearching) {
    setWasSearching(searching);
    if (searching && !expanded) setExpanded(true);
  }

  useEffect(() => {
    if (!reveal) return;
    setExpanded(true);
    onRevealed?.();
    // Two frames, as NotCarriedGroup: the section change that brought the
    // Configuration view up moves focus in the next frame.
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        const section = sectionRef.current;
        if (!section) return;
        section.scrollIntoView({ behavior: "smooth", block: "start" });
        section.querySelector("button")?.focus({ preventScroll: true });
      }),
    );
  }, [reveal, onRevealed]);

  useEffect(() => {
    if (focusAfter === null || confirmRow !== null) return;
    if (pendingKeys.has(focusAfter)) return;
    const li = rowElement(sectionRef.current, focusAfter);
    const button = li?.querySelector<HTMLButtonElement>(
      ".inspectah-machine-state__action button",
    );
    if (button && !button.disabled) button.focus();
    setFocusAfter(null);
  }, [focusAfter, confirmRow, pendingKeys, rows]);

  useEffect(() => {
    for (const row of rows) {
      const key = machineStateItemId(row.path);
      const asked = requested.current.get(key);
      if (asked === undefined || asked !== row.include) continue;
      requested.current.delete(key);
      setAnnouncement(
        row.include
          ? `${row.path} included in the image`
          : `${row.path} left out of the image`,
      );
    }
  }, [rows]);

  // A failed request asked for nothing that will arrive.
  useEffect(() => {
    for (const key of failures.keys()) requested.current.delete(key);
  }, [failures]);

  if (rows.length === 0) return null;

  const shown = filterMachineStateRows(rows, filterText);

  const send = (row: MachineStateRowDto, include: boolean) => {
    const key = machineStateItemId(row.path);
    requested.current.set(key, include);
    setFocusAfter(key);
    onSetInclude(row, include);
  };

  const handleAction = (row: MachineStateRowDto) => {
    if (row.include) {
      send(row, false);
    } else if (row.override === "warn") {
      setConfirmRow(row);
    } else {
      send(row, true);
    }
  };

  const closeDialog = () => {
    if (confirmRow) setFocusAfter(machineStateItemId(confirmRow.path));
    setConfirmRow(null);
  };

  return (
    <section
      id={MACHINE_STATE_GROUP_ID}
      ref={sectionRef}
      data-testid="machine-state-group"
      className="inspectah-machine-state"
    >
      <ExpandableSection
        toggleText={machineStateToggleText(rows, stepCount)}
        isExpanded={expanded}
        onToggle={(_e, isExpanded) => setExpanded(isExpanded)}
      >
        <p className="inspectah-machine-state__help">
          Files that belong to this host, not to the image. inspectah leaves
          them out; provisioning or first boot recreates them.
        </p>
        {SUBLISTS.map(({ reason, heading, help }) => {
          const members = shown.filter((r) => sublistOf(r) === reason);
          if (members.length === 0) return null;
          return (
            <div
              key={reason}
              className="inspectah-machine-state__sublist"
              data-testid={`machine-state-sublist-${reason}`}
            >
              <h3 className="inspectah-machine-state__heading">{heading}</h3>
              <p className="inspectah-machine-state__help">{help}</p>
              <ul className="inspectah-machine-state__list">
                {members.map((row) => (
                  <MachineStateRow
                    key={row.path}
                    row={row}
                    isPending={pendingKeys.has(machineStateItemId(row.path))}
                    failure={failures.get(machineStateItemId(row.path))}
                    onAction={handleAction}
                    onDismissFailure={onDismissFailure}
                  />
                ))}
              </ul>
            </div>
          );
        })}
      </ExpandableSection>
      <div className="pf-v6-screen-reader" aria-live="polite" role="status">
        {announcement}
      </div>
      {confirmRow && (
        <Modal
          isOpen
          variant="small"
          onClose={closeDialog}
          aria-labelledby="machine-state-confirm-title"
          elementToFocus={`#${CONFIRM_CANCEL_ID}`}
          data-testid="machine-state-confirm"
        >
          <ModalHeader
            title={warnWording(confirmRow).title}
            labelId="machine-state-confirm-title"
          />
          <ModalBody>
            <p>{warnWording(confirmRow).lead}</p>
            <p>
              <code className="inspectah-machine-state__path">
                {confirmRow.path}
              </code>
            </p>
            <p>{warnAlternative(confirmRow)}</p>
          </ModalBody>
          <ModalFooter>
            <Button
              variant="primary"
              isDanger
              onClick={() => {
                const row = confirmRow;
                setConfirmRow(null);
                send(row, true);
              }}
            >
              Include in image
            </Button>
            <Button id={CONFIRM_CANCEL_ID} variant="link" onClick={closeDialog}>
              Cancel
            </Button>
          </ModalFooter>
        </Modal>
      )}
    </section>
  );
}

/**
 * Scrolls the row with `itemId` into view and focuses its button, or the
 * row itself when it has none (a `never` row). False when the row is not
 * in the document, so the caller can try again once it renders.
 */
export function revealMachineStateRow(itemId: string): boolean {
  const li = rowElement(document.body, itemId);
  if (!li) return false;
  li.scrollIntoView({ behavior: "smooth", block: "nearest" });
  const button = li.querySelector<HTMLElement>(
    ".inspectah-machine-state__action button",
  );
  (button ?? li).focus();
  return true;
}

/**
 * The `<li>` of the row with `itemId` under `root`, compared by test id as
 * a string, never through a CSS selector built from a host path.
 */
function rowElement(root: HTMLElement | null, itemId: string): HTMLElement | null {
  if (!root) return null;
  const testId = `${MACHINE_STATE_ROW_TESTID_PREFIX}${itemId.slice(MACHINE_STATE_ITEM_PREFIX.length)}`;
  for (const el of root.querySelectorAll<HTMLElement>("[data-testid]")) {
    if (el.getAttribute("data-testid") === testId) return el;
  }
  return null;
}

interface MachineStateRowProps {
  row: MachineStateRowDto;
  isPending: boolean;
  failure: MachineStateFailure | undefined;
  onAction: (row: MachineStateRowDto) => void;
  onDismissFailure?: (itemId: string) => void;
}

function MachineStateRow({
  row,
  isPending,
  failure,
  onAction,
  onDismissFailure,
}: MachineStateRowProps) {
  const itemId = machineStateItemId(row.path);
  const labelId = `${itemId}-label`;
  const noticeId = `${itemId}-notice`;
  const meta = metadataLine(row);
  const canChange = row.override !== "never" && row.item_id !== undefined;
  // `warn` opens the dialog first, which the ellipsis says.
  const [verb, ellipsis] = row.include
    ? ["Exclude", ""]
    : row.override === "warn"
      ? ["Include", "…"]
      : ["Include", ""];

  return (
    <li
      data-testid={`${MACHINE_STATE_ROW_TESTID_PREFIX}${row.path}`}
      className="inspectah-machine-state__row"
      tabIndex={-1}
    >
      <div className="inspectah-machine-state__main">
        <code className="inspectah-machine-state__path">{row.path}</code>
        <span id={labelId}>
          {row.reason === "host_credential" ? (
            <Label color="orange" isCompact icon={<KeyIcon />}>
              {reasonLabel(row.reason)}
            </Label>
          ) : (
            <Label color="grey" isCompact>
              {reasonLabel(row.reason)}
            </Label>
          )}
        </span>
      </div>
      <div className="inspectah-machine-state__action">
        {!canChange ? (
          <span className="inspectah-machine-state__outcome">Not carried</span>
        ) : (
          <>
            {row.include && (
              <Label color="green" isCompact>
                Included by you
              </Label>
            )}
            <Button
              variant="secondary"
              size="sm"
              aria-label={`${verb} ${row.path}${ellipsis}`}
              aria-describedby={`${labelId} ${noticeId}`}
              isDisabled={isPending}
              onClick={() => onAction(row)}
            >
              {verb}
              {ellipsis}
            </Button>
          </>
        )}
      </div>
      <p
        id={noticeId}
        className={
          row.content_captured
            ? "inspectah-unit-note inspectah-machine-state__note"
            : "inspectah-unit-note inspectah-unit-note--withheld inspectah-machine-state__note"
        }
      >
        {noticeLine(row)}
      </p>
      {meta !== null && (
        <p className="inspectah-machine-state__meta">{meta}</p>
      )}
      <div aria-live="polite" className="inspectah-machine-state__failure">
        {failure && (
          <Alert
            variant={failure.kind === "failed" ? "warning" : "info"}
            isInline
            isPlain
            title={failure.text}
            data-testid={`machine-state-failure-${row.path}`}
            actionClose={
              onDismissFailure ? (
                <AlertActionCloseButton
                  aria-label={`Dismiss the message for ${row.path}`}
                  onClose={() => onDismissFailure(itemId)}
                />
              ) : undefined
            }
          />
        )}
      </div>
    </li>
  );
}
