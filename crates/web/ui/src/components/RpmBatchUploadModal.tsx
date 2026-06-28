import { useState, useCallback, useMemo } from "react";
import {
  Modal,
  ModalVariant,
  ModalHeader,
  ModalBody,
  ModalFooter,
  Button,
  Alert,
  Content,
  List,
  ListItem,
  Label,
} from "@patternfly/react-core";
import {
  CheckCircleIcon,
  ExclamationCircleIcon,
  ExclamationTriangleIcon,
} from "@patternfly/react-icons";

/** Extract the package name prefix from an RPM filename. */
function extractPackageName(filename: string): string | null {
  const match = filename.match(/^(.+?)-\d/);
  return match ? match[1] : null;
}

interface MatchResult {
  matched: Array<{ packageName: string; file: File }>;
  unmatched: File[];
  conflicts: Array<{ packageName: string; files: File[] }>;
}

export interface RpmBatchUploadModalProps {
  isOpen: boolean;
  needsUploadPackages: string[];
  onBatchUpload: (matched: Array<{ packageName: string; file: File }>) => void;
  onClose: () => void;
}

export function RpmBatchUploadModal({
  isOpen,
  needsUploadPackages,
  onBatchUpload,
  onClose,
}: RpmBatchUploadModalProps) {
  const [files, setFiles] = useState<File[]>([]);
  const [isDragActive, setIsDragActive] = useState(false);
  const packageSet = useMemo(
    () => new Set(needsUploadPackages),
    [needsUploadPackages],
  );

  const matchResult: MatchResult = useMemo(() => {
    const matched: MatchResult["matched"] = [];
    const unmatched: File[] = [];
    const conflictMap = new Map<string, File[]>();

    for (const file of files) {
      if (!file.name.endsWith(".rpm")) {
        unmatched.push(file);
        continue;
      }
      const name = extractPackageName(file.name);
      if (!name || !packageSet.has(name)) {
        unmatched.push(file);
        continue;
      }

      const existing = conflictMap.get(name);
      if (existing) {
        existing.push(file);
      } else if (matched.some((m) => m.packageName === name)) {
        const prev = matched.find((m) => m.packageName === name)!;
        conflictMap.set(name, [prev.file, file]);
        matched.splice(matched.indexOf(prev), 1);
      } else {
        matched.push({ packageName: name, file });
      }
    }

    return {
      matched,
      unmatched,
      conflicts: Array.from(conflictMap.entries()).map(
        ([packageName, conflictFiles]) => ({
          packageName,
          files: conflictFiles,
        }),
      ),
    };
  }, [files, packageSet]);

  const handleDragEnter = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragActive(true);
  }, []);

  const handleDragLeave = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragActive(false);
  }, []);

  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
  }, []);

  const handleDrop = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragActive(false);

    const droppedFiles = Array.from(e.dataTransfer.files);
    setFiles((prev) => [...prev, ...droppedFiles]);
  }, []);

  const handleFileInputChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      if (e.target.files) {
        const selectedFiles = Array.from(e.target.files);
        setFiles((prev) => [...prev, ...selectedFiles]);
      }
    },
    [],
  );

  const handleRemoveFile = useCallback((removedFile: File) => {
    setFiles((prev) => prev.filter((f) => f !== removedFile));
  }, []);

  const handleConfirm = useCallback(() => {
    if (matchResult.matched.length > 0 && matchResult.conflicts.length === 0) {
      onBatchUpload(matchResult.matched);
      setFiles([]);
      onClose();
    }
  }, [matchResult, onBatchUpload, onClose]);

  const handleClose = useCallback(() => {
    setFiles([]);
    onClose();
  }, [onClose]);

  if (!isOpen) return null;

  const canConfirm =
    matchResult.matched.length > 0 && matchResult.conflicts.length === 0;

  return (
    <Modal
      variant={ModalVariant.large}
      isOpen={isOpen}
      onClose={handleClose}
      aria-label={`Upload RPMs for ${needsUploadPackages.length} packages`}
    >
      <ModalHeader
        title={`Upload RPMs (${needsUploadPackages.length} packages need RPMs)`}
      />
      <ModalBody>
        <div
          onDragEnter={handleDragEnter}
          onDragLeave={handleDragLeave}
          onDragOver={handleDragOver}
          onDrop={handleDrop}
          className={`inspectah-rpm-batch__dropzone ${isDragActive ? "inspectah-rpm-batch__dropzone--active" : ""}`}
        >
          <Content component="p">
            Drag and drop RPM files here or{" "}
            <label
              htmlFor="rpm-batch-file-input"
              className="inspectah-rpm-batch__browse-label"
            >
              browse
            </label>
          </Content>
          <input
            id="rpm-batch-file-input"
            type="file"
            multiple
            accept=".rpm"
            onChange={handleFileInputChange}
            className="inspectah-rpm-batch__file-input"
          />
          <Content component="small">Accepted file types: .rpm</Content>
        </div>

        {files.length > 0 && (
          <>
            <Content
              component="p"
              className="inspectah-rpm-batch__match-summary"
            >
              {matchResult.matched.length} of {files.length} RPMs matched
            </Content>

            {matchResult.matched.length > 0 && (
              <div className="inspectah-rpm-batch__section">
                <Content
                  component="h3"
                  className="inspectah-rpm-batch__section-title"
                >
                  Matched RPMs
                </Content>
                <List isPlain>
                  {matchResult.matched.map(({ packageName, file }) => (
                    <ListItem key={packageName}>
                      <Label color="green" isCompact icon={<CheckCircleIcon />}>
                        {packageName}
                      </Label>
                      <span className="inspectah-rpm-batch__file-name">
                        {file.name}
                      </span>
                      <Button
                        variant="plain"
                        onClick={() => handleRemoveFile(file)}
                        aria-label={`Remove ${file.name}`}
                        className="inspectah-rpm-batch__remove-btn"
                      >
                        ×
                      </Button>
                    </ListItem>
                  ))}
                </List>
              </div>
            )}

            {matchResult.unmatched.length > 0 && (
              <div className="inspectah-rpm-batch__section">
                <Content
                  component="h3"
                  className="inspectah-rpm-batch__section-title"
                >
                  Unmatched Files
                </Content>
                <List isPlain>
                  {matchResult.unmatched.map((file, idx) => (
                    <ListItem key={`unmatched-${idx}`}>
                      <Label
                        color="red"
                        isCompact
                        icon={<ExclamationCircleIcon />}
                      >
                        No match
                      </Label>
                      <span className="inspectah-rpm-batch__file-name">
                        {file.name}
                      </span>
                      <Button
                        variant="plain"
                        onClick={() => handleRemoveFile(file)}
                        aria-label={`Remove ${file.name}`}
                        className="inspectah-rpm-batch__remove-btn"
                      >
                        ×
                      </Button>
                    </ListItem>
                  ))}
                </List>
              </div>
            )}

            {matchResult.conflicts.length > 0 && (
              <Alert
                variant="warning"
                isInline
                title="Conflicting uploads"
                className="inspectah-rpm-batch__alert"
              >
                <Content component="small">
                  {matchResult.conflicts.map((c) => (
                    <div
                      key={c.packageName}
                      className="inspectah-rpm-batch__conflict"
                    >
                      <ExclamationTriangleIcon />{" "}
                      <strong>{c.packageName}</strong>: {c.files.length} files
                      match. Remove duplicates to resolve:{" "}
                      {c.files.map((f) => f.name).join(", ")}
                    </div>
                  ))}
                </Content>
              </Alert>
            )}
          </>
        )}
      </ModalBody>
      <ModalFooter>
        <Button
          variant="primary"
          onClick={handleConfirm}
          isDisabled={!canConfirm}
          aria-label="Confirm upload"
        >
          Upload{" "}
          {matchResult.matched.length > 0
            ? `(${matchResult.matched.length})`
            : ""}
        </Button>
        <Button variant="link" onClick={handleClose}>
          Cancel
        </Button>
      </ModalFooter>
    </Modal>
  );
}
