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
import type { RefineStats, RefinedView } from "../api/types";
import { exportTarball, fetchView } from "../api/client";
import { ApiError } from "../api/types";

export interface ExportDialogProps {
  isOpen: boolean;
  onClose: () => void;
  stats: RefineStats | null;
  generation: number;
  onViewUpdate: (view: RefinedView) => void;
}

export function ExportDialog({
  isOpen,
  onClose,
  stats,
  generation,
  onViewUpdate,
}: ExportDialogProps) {
  const [exporting, setExporting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [stale, setStale] = useState(false);

  const excludedPackages = stats?.excluded_packages ?? 0;
  const excludedConfigs = stats?.excluded_configs ?? 0;

  const handleExport = useCallback(async () => {
    setExporting(true);
    setError(null);
    setStale(false);

    try {
      const blob = await exportTarball(generation);
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
      } else {
        setError(err instanceof Error ? err.message : String(err));
      }
    } finally {
      setExporting(false);
    }
  }, [generation, onClose, onViewUpdate]);

  const handleClose = useCallback(() => {
    setError(null);
    setStale(false);
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
          isDisabled={exporting}
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
