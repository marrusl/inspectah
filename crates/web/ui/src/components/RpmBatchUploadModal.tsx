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
  ExpandableSection,
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

/**
 * Extract the canonical "name.arch" from an RPM filename.
 * RPM filenames follow the NEVRA pattern: name-version-release.arch.rpm
 */
function extractCanonicalKey(filename: string): string | null {
  const match = filename.match(/^(.+?)-\d.*\.(\w+)\.rpm$/);
  if (!match) return null;
  return `${match[1]}.${match[2]}`;
}

/**
 * Extract the bare package name from a canonical "name.arch" key.
 * Falls back to the full string when no recognised arch suffix is found.
 */
const KNOWN_ARCHES = new Set([
  "x86_64",
  "noarch",
  "i686",
  "aarch64",
  "s390x",
  "ppc64le",
  "src",
]);

function bareNameFromCanonical(canonicalKey: string): string {
  const dotIdx = canonicalKey.lastIndexOf(".");
  if (dotIdx === -1) return canonicalKey;
  const suffix = canonicalKey.slice(dotIdx + 1);
  return KNOWN_ARCHES.has(suffix)
    ? canonicalKey.slice(0, dotIdx)
    : canonicalKey;
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
  const [isChecklistExpanded, setIsChecklistExpanded] = useState(true);

  // Build a set of canonical keys for quick lookup, plus a map from bare name
  // to canonical keys to handle matching when canonical extraction fails.
  const canonicalSet = useMemo(
    () => new Set(needsUploadPackages),
    [needsUploadPackages],
  );
  const bareNameToCanonicals = useMemo(() => {
    const map = new Map<string, string[]>();
    for (const key of needsUploadPackages) {
      const bare = bareNameFromCanonical(key);
      const existing = map.get(bare);
      if (existing) {
        existing.push(key);
      } else {
        map.set(bare, [key]);
      }
    }
    return map;
  }, [needsUploadPackages]);

  const matchResult: MatchResult = useMemo(() => {
    const matched: MatchResult["matched"] = [];
    const unmatched: File[] = [];
    const conflictMap = new Map<string, File[]>();

    for (const file of files) {
      if (!file.name.endsWith(".rpm")) {
        unmatched.push(file);
        continue;
      }

      // Try canonical name.arch extraction first (disambiguates multilib)
      let key: string | null = null;
      const canonical = extractCanonicalKey(file.name);
      if (canonical && canonicalSet.has(canonical)) {
        key = canonical;
      } else {
        // Fall back to bare name match
        const bareName = extractPackageName(file.name);
        if (bareName) {
          const candidates = bareNameToCanonicals.get(bareName);
          if (candidates && candidates.length === 1) {
            key = candidates[0];
          }
          // If multiple candidates (multilib), canonical extraction should
          // have resolved it. If it didn't, leave unmatched.
        }
      }

      if (!key) {
        unmatched.push(file);
        continue;
      }

      const existing = conflictMap.get(key);
      if (existing) {
        existing.push(file);
      } else if (matched.some((m) => m.packageName === key)) {
        const prev = matched.find((m) => m.packageName === key)!;
        conflictMap.set(key, [prev.file, file]);
        matched.splice(matched.indexOf(prev), 1);
      } else {
        matched.push({ packageName: key, file });
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
  }, [files, canonicalSet, bareNameToCanonicals]);

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

  const matchedSet = useMemo(() => {
    const s = new Set<string>();
    for (const m of matchResult.matched) {
      s.add(m.packageName);
    }
    return s;
  }, [matchResult.matched]);

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
        <ExpandableSection
          toggleContent={
            <span className="inspectah-rpm-batch__checklist-toggle">
              Packages needing RPMs ({needsUploadPackages.length})
            </span>
          }
          isExpanded={isChecklistExpanded}
          onToggle={(_event, expanded) => setIsChecklistExpanded(expanded)}
          aria-label="Packages needing RPMs"
          className="inspectah-rpm-batch__checklist"
        >
          <Content
            component="p"
            className="inspectah-rpm-batch__checklist-summary"
          >
            {matchedSet.size} of {needsUploadPackages.length} packages matched
          </Content>
          <div
            className="inspectah-rpm-batch__checklist-labels"
            role="list"
            aria-label="Package checklist"
          >
            {needsUploadPackages.map((pkg) => (
              <span key={pkg} role="listitem">
                {matchedSet.has(pkg) ? (
                  <Label
                    color="green"
                    isCompact
                    icon={<CheckCircleIcon />}
                    className="inspectah-rpm-batch__checklist-label"
                  >
                    {pkg}
                  </Label>
                ) : (
                  <Label
                    color="grey"
                    isCompact
                    className="inspectah-rpm-batch__checklist-label"
                  >
                    {pkg}
                  </Label>
                )}
              </span>
            ))}
          </div>
        </ExpandableSection>

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
