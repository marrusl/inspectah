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
} from "@patternfly/react-core";
import type { RefineStats, ViewResponse } from "../api/types";
import { exportTarball, fetchView } from "../api/client";
import { ApiError } from "../api/types";

export interface ExportDialogProps {
  isOpen: boolean;
  onClose: () => void;
  stats: RefineStats | null;
  generation: number;
  sessionIsSensitive: boolean;
  onViewUpdate: (view: ViewResponse) => void;
}

export function ExportDialog({
  isOpen,
  onClose,
  stats,
  generation,
  sessionIsSensitive,
  onViewUpdate,
}: ExportDialogProps) {
  const [exporting, setExporting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [stale, setStale] = useState(false);
  const [sensitiveAck, setSensitiveAck] = useState(false);

  const excludedPackages = stats?.sections.find(s => s.kind === "package")?.excluded ?? 0;
  const excludedConfigs = stats?.sections.find(s => s.kind === "config")?.excluded ?? 0;

  const handleExport = useCallback(async () => {
    setExporting(true);
    setError(null);
    setStale(false);

    try {
      const blob = await exportTarball(generation, sensitiveAck);
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = "inspectah-refine-output.tar.gz";
      a.click();
      URL.revokeObjectURL(url);
      onClose();
    } catch (err) {
      if (err instanceof ApiError && err.status === 409) {
        setStale(true);
        try {
          const view = await fetchView();
          onViewUpdate(view);
        } catch {
          // If re-fetch also fails, stale alert is still visible
        }
        onClose();
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
  }, [generation, sensitiveAck, onClose, onViewUpdate]);

  const handleClose = useCallback(() => {
    setError(null);
    setStale(false);
    setSensitiveAck(false);
    onClose();
  }, [onClose]);

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
            {excludedPackages} packages excluded, {excludedConfigs} configs
            excluded
          </p>
          <p>
            <strong>Generation:</strong> {generation}
          </p>
        </Content>

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

        {error && (
          <Alert variant="danger" isInline title="Export failed">
            {error}
          </Alert>
        )}

        {stale && (
          <Alert variant="warning" isInline title="Stale state">
            The view has changed since it was last loaded. Re-fetching current
            state.
          </Alert>
        )}
      </ModalBody>
      <ModalFooter>
        <Button
          variant="primary"
          onClick={handleExport}
          isLoading={exporting}
          isDisabled={exporting || (sessionIsSensitive && !sensitiveAck)}
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
