import { useState, useCallback } from "react";
import {
  Modal,
  ModalVariant,
  ModalHeader,
  ModalBody,
  ModalFooter,
  Button,
  Content,
  Alert,
  AlertActionLink,
} from "@patternfly/react-core";
import type {
  MachineStateRowDto,
  NotCarriedRow,
  ProvisioningStepDto,
  RefineStats,
  ViewResponse,
} from "../api/types";
import { exportTarball, fetchView } from "../api/client";
import { ApiError } from "../api/types";
import { withoutShownNotice } from "../api/exportWarnings";
import type { ExportWarning } from "../api/exportWarnings";

const NO_ROWS: NotCarriedRow[] = [];
const NO_MACHINE_STATE: MachineStateRowDto[] = [];
const NO_STEPS: ProvisioningStepDto[] = [];
const NO_NOTICES: ExportWarning[] = [];

export interface ExportDialogProps {
  isOpen: boolean;
  onClose: () => void;
  stats: RefineStats | null;
  generation: number;
  sessionIsSensitive: boolean;
  onViewUpdate: (view: ViewResponse) => void;
  /** Number of uploaded RPMs that did not match any package. */
  unmatchedUploadCount?: number;
  /** The view's `not_carried_packages`; the notice counts the unit owners. */
  notCarriedPackages?: NotCarriedRow[];
  /**
   * The view's `not_carried_notice`. Non-null shows the pre-download
   * notice, and the post-download list skips the header entry equal to it.
   */
  notCarriedNotice?: ExportWarning | null;
  /** Shows the Not carried group. Without it the notice has no link. */
  onReviewNotCarried?: () => void;
  /** The view's `machine_state`; non-empty shows the machine-state notice. */
  machineStateRows?: MachineStateRowDto[];
  /** The view's `machine_state_steps`, listed in their order. */
  machineStateSteps?: ProvisioningStepDto[];
  /**
   * The view's `machine_state_notices`: the post-download list skips the
   * header entries equal to them, since the dialog showed what they say.
   */
  machineStateNotices?: ExportWarning[];
  /** Shows the machine-state block. Without it the notice has no link. */
  onReviewMachineState?: () => void;
}

export function ExportDialog({
  isOpen,
  onClose,
  stats,
  generation,
  sessionIsSensitive,
  onViewUpdate,
  unmatchedUploadCount = 0,
  notCarriedPackages = NO_ROWS,
  notCarriedNotice = null,
  onReviewNotCarried,
  machineStateRows = NO_MACHINE_STATE,
  machineStateSteps = NO_STEPS,
  machineStateNotices = NO_NOTICES,
  onReviewMachineState,
}: ExportDialogProps) {
  const [exporting, setExporting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Set by a 409, then by how loading the current view after it went.
  const [stale, setStale] = useState<
    "refreshing" | "refreshed" | "refresh-failed" | null
  >(null);
  const [sensitiveAck, setSensitiveAck] = useState(false);
  const [unmatchedAck, setUnmatchedAck] = useState(false);
  // Non-null once a download finished with warnings: the dialog then stays
  // open showing them until the operator closes it.
  const [downloadWarnings, setDownloadWarnings] = useState<
    ExportWarning[] | null
  >(null);

  const excludedPackages =
    stats?.sections.find((s) => s.kind === "package")?.excluded ?? 0;
  const excludedConfigs =
    stats?.sections.find((s) => s.kind === "config")?.excluded ?? 0;
  // Advisories are not exclusions, so they are no longer inside the two
  // counts above — but they are still absent from the image, and the
  // dialog's job is to say what export will do. Summed across sections so
  // the count stays right if a section other than config ever reports one.
  const advisoryFindings = (stats?.sections ?? []).reduce(
    (sum, s) => sum + (s.advisory ?? 0),
    0,
  );

  const handleExport = useCallback(async () => {
    setExporting(true);
    setError(null);
    setStale(null);

    try {
      const { blob, warnings } = await exportTarball(generation, sensitiveAck);
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = "inspectah-refine-output.tar.gz";
      a.click();
      URL.revokeObjectURL(url);
      // The notices were on screen before download; do not repeat them.
      const shown =
        notCarriedNotice === null
          ? machineStateNotices
          : [notCarriedNotice, ...machineStateNotices];
      const unseen = withoutShownNotice(warnings, shown);
      if (unseen.length > 0) {
        setDownloadWarnings(unseen);
      } else {
        onClose();
      }
    } catch (err) {
      if (err instanceof ApiError && err.status === 409) {
        // Stay open: the stale alert explains the failed export. The owner
        // shows the re-fetched view before `exporting` clears (both land in
        // one render), so Export comes back at the current generation.
        setStale("refreshing");
        try {
          const view = await fetchView();
          onViewUpdate(view);
          setStale("refreshed");
        } catch {
          setStale("refresh-failed");
        }
      } else if (err instanceof ApiError && err.status === 428) {
        // Server requires sensitive acknowledgment
        setError(
          "This export contains sensitive data (password hashes). " +
            "Check the acknowledgment box and try again.",
        );
      } else {
        setError(err instanceof Error ? err.message : String(err));
      }
    } finally {
      setExporting(false);
    }
  }, [
    generation,
    sensitiveAck,
    notCarriedNotice,
    machineStateNotices,
    onClose,
    onViewUpdate,
  ]);

  const handleClose = useCallback(() => {
    setError(null);
    setStale(null);
    setSensitiveAck(false);
    setUnmatchedAck(false);
    setDownloadWarnings(null);
    onClose();
  }, [onClose]);

  const handleReviewNotCarried = useCallback(() => {
    handleClose();
    onReviewNotCarried?.();
  }, [handleClose, onReviewNotCarried]);

  const handleReviewMachineState = useCallback(() => {
    handleClose();
    onReviewMachineState?.();
  }, [handleClose, onReviewMachineState]);

  // The server's flag decides ownership; the rows with it set are exactly
  // the packages the notice counts.
  const unitOwners = notCarriedPackages.filter((r) => r.owns_enabled_unit);

  // Machine state: the rows left out, and the `warn` rows the operator
  // included, which every machine built from the image will share.
  const machineStateLeftOut = machineStateRows.filter((r) => !r.include).length;
  const identityIncluded = machineStateRows.filter(
    (r) => r.override === "warn" && r.include,
  );
  const machineStateTitle = [
    `${machineStateLeftOut} machine-state ${machineStateLeftOut === 1 ? "file" : "files"} left out`,
    ...(machineStateSteps.length > 0
      ? [
          `${machineStateSteps.length} provisioning ${machineStateSteps.length === 1 ? "step" : "steps"}`,
        ]
      : []),
  ].join(" · ");

  if (downloadWarnings !== null) {
    return (
      <Modal
        variant={ModalVariant.small}
        isOpen={isOpen}
        onClose={handleClose}
        aria-label="Export warnings"
      >
        <ModalHeader title="Downloaded, with warnings" />
        <ModalBody>
          {downloadWarnings.map((w, i) => (
            // A failure is something asked for that is not in the export,
            // but the export itself succeeded, so `warning` rather than
            // `danger`, which this dialog keeps for "Export failed".
            <Alert
              key={i}
              variant={w.kind === "failure" ? "warning" : "info"}
              isInline
              title={w.text}
              data-testid={`export-warning-${i}`}
              style={
                i > 0
                  ? { marginTop: "var(--pf-t--global--spacer--sm)" }
                  : undefined
              }
            />
          ))}
        </ModalBody>
        <ModalFooter>
          <Button variant="primary" onClick={handleClose}>
            Close
          </Button>
        </ModalFooter>
      </Modal>
    );
  }

  return (
    <Modal
      variant={ModalVariant.small}
      isOpen={isOpen}
      onClose={handleClose}
      aria-label="Export tarball"
    >
      <ModalHeader title="Export Tarball" />
      <ModalBody>
        <Content>
          <p>
            {excludedPackages}{" "}
            {excludedPackages === 1 ? "package" : "packages"} excluded,{" "}
            {excludedConfigs} {excludedConfigs === 1 ? "config" : "configs"}{" "}
            excluded
          </p>
          {advisoryFindings > 0 && (
            <p>
              Advisory findings ({advisoryFindings}) are reported in the audit
              report; their files are not copied into the image.
            </p>
          )}
          <p>
            <strong>Generation:</strong> {generation}
          </p>
        </Content>

        {notCarriedNotice !== null && (
          // Info, compact and collapsed: a stock host lists unit owners, so
          // this shows on nearly every export.
          <Alert
            variant="info"
            isInline
            isExpandable
            toggleAriaLabel="Show not-carried details"
            data-testid="not-carried-notice"
            title={
              unitOwners.length === 0
                ? notCarriedNotice.text
                : unitOwners.length === 1
                  ? "1 package that owns an enabled unit will not be carried"
                  : `${unitOwners.length} packages that own enabled units will not be carried`
            }
            actionLinks={
              onReviewNotCarried ? (
                <AlertActionLink onClick={handleReviewNotCarried}>
                  Review not-carried packages
                </AlertActionLink>
              ) : undefined
            }
            style={{ marginBottom: "var(--pf-t--global--spacer--sm)" }}
          >
            <Content>
              <p>
                These units are enabled on the source host, but their packages
                are left out of the image:
              </p>
              <ul>
                {unitOwners.map((r) => (
                  <li key={`${r.item_id.key.name}.${r.item_id.key.arch}`}>
                    <strong>{r.display_name}</strong>{" "}
                    {r.enabled_units.map((u) => (
                      <code key={u} style={{ marginRight: "0.5em" }}>
                        {u}
                      </code>
                    ))}
                  </li>
                ))}
              </ul>
              <p>
                The export lists them in the audit report's Not carried table
                and the Containerfile's <code># Not carried</code> block.
              </p>
            </Content>
          </Alert>
        )}

        {machineStateRows.length > 0 && (
          // Built as the not-carried notice is: info, collapsed, with a
          // link to the block. The steps are the server's, in its order.
          <Alert
            variant="info"
            isInline
            isExpandable
            toggleAriaLabel="Show machine-state details"
            data-testid="machine-state-notice"
            title={machineStateTitle}
            actionLinks={
              onReviewMachineState ? (
                <AlertActionLink onClick={handleReviewMachineState}>
                  Review machine state
                </AlertActionLink>
              ) : undefined
            }
            style={{ marginBottom: "var(--pf-t--global--spacer--sm)" }}
          >
            <Content>
              {machineStateSteps.length > 0 && (
                <>
                  <p>At provisioning:</p>
                  <ul>
                    {machineStateSteps.map((s) => (
                      <li key={s.step} data-testid="machine-state-step">
                        <span>{s.command}</span>
                        <br />
                        {s.paths.map((p) => (
                          <code key={p} style={{ marginRight: "0.5em" }}>
                            {p}
                          </code>
                        ))}
                      </li>
                    ))}
                  </ul>
                </>
              )}
              <p>The other files are recreated without any step.</p>
            </Content>
          </Alert>
        )}

        {identityIncluded.length > 0 && (
          <Alert
            variant="warning"
            isInline
            data-testid="machine-state-included-warning"
            title={
              identityIncluded.length === 1
                ? "1 host identity file is included in the image"
                : `${identityIncluded.length} host identity files are included in the image`
            }
            style={{ marginBottom: "var(--pf-t--global--spacer--sm)" }}
          >
            <Content>
              <ul>
                {identityIncluded.map((r) => (
                  <li key={r.path}>
                    <code>{r.path}</code>
                  </li>
                ))}
              </ul>
            </Content>
          </Alert>
        )}

        <Alert variant="info" isInline isPlain title="Reference sections">
          Reference sections (network, storage, scheduled tasks, etc.) are
          included in the export as-is and cannot be toggled.
        </Alert>

        {sessionIsSensitive && (
          <>
            <Alert
              variant="warning"
              isInline
              title="Sensitive data detected"
              style={{ marginTop: "var(--pf-t--global--spacer--sm)" }}
            >
              This export contains password hashes or other sensitive material.
              The tarball should be handled with appropriate care.
            </Alert>
            <label
              style={{
                display: "flex",
                alignItems: "center",
                gap: "var(--pf-t--global--spacer--xs)",
                marginTop: "var(--pf-t--global--spacer--sm)",
                cursor: "pointer",
              }}
            >
              <input
                type="checkbox"
                checked={sensitiveAck}
                onChange={(e) => setSensitiveAck(e.target.checked)}
              />
              I acknowledge this export contains sensitive data
            </label>
          </>
        )}

        {unmatchedUploadCount > 0 && (
          <>
            <Alert
              variant="warning"
              isInline
              title="Unmatched uploads"
              data-testid="unmatched-upload-warning"
              style={{ marginTop: "var(--pf-t--global--spacer--sm)" }}
            >
              {unmatchedUploadCount} uploaded{" "}
              {unmatchedUploadCount === 1 ? "RPM has" : "RPMs have"} no matching
              package and will not be included in the export.
            </Alert>
            <label
              style={{
                display: "flex",
                alignItems: "center",
                gap: "var(--pf-t--global--spacer--xs)",
                marginTop: "var(--pf-t--global--spacer--sm)",
                cursor: "pointer",
              }}
            >
              <input
                type="checkbox"
                checked={unmatchedAck}
                onChange={(e) => setUnmatchedAck(e.target.checked)}
                data-testid="unmatched-ack-checkbox"
              />
              Continue with export anyway
            </label>
          </>
        )}

        {error && (
          <Alert variant="danger" isInline title="Export failed">
            {error}
          </Alert>
        )}

        {stale !== null && (
          <Alert variant="warning" isInline title="Stale state">
            The session changed since this view loaded.{" "}
            {stale === "refreshing"
              ? "Loading the current state."
              : stale === "refreshed"
                ? "The view now shows the current state; review it, then " +
                  "export again."
                : "The current state could not be loaded. Reload the page, " +
                  "then export again."}
          </Alert>
        )}
      </ModalBody>
      <ModalFooter>
        <Button
          variant="primary"
          onClick={handleExport}
          isLoading={exporting}
          isDisabled={
            exporting ||
            (sessionIsSensitive && !sensitiveAck) ||
            (unmatchedUploadCount > 0 && !unmatchedAck)
          }
        >
          Export
        </Button>
        <Button variant="link" onClick={handleClose} isDisabled={exporting}>
          Cancel
        </Button>
      </ModalFooter>
    </Modal>
  );
}
