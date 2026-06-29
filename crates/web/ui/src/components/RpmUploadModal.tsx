import { useState, useCallback, useEffect, useRef } from "react";
import {
  Modal,
  ModalVariant,
  ModalHeader,
  ModalBody,
  ModalFooter,
  Button,
  FileUpload,
  HelperText,
  HelperTextItem,
  Alert,
  Content,
} from "@patternfly/react-core";
import {
  CheckCircleIcon,
  ExclamationCircleIcon,
} from "@patternfly/react-icons";
import type { UploadMatchResult } from "../hooks/useRpmUpload";

export interface RpmUploadModalProps {
  isOpen: boolean;
  packageName: string;
  packageArch: string;
  onUpload: (
    packageName: string,
    file: File,
  ) => Promise<UploadMatchResult | void>;
  onClose: () => void;
  /** Ref to the trigger element for focus return on close. */
  triggerRef: React.RefObject<HTMLElement | null>;
}

function validateRpmFile(
  packageName: string,
  arch: string,
  filename: string,
): { valid: boolean; error?: string } {
  if (!filename.endsWith(".rpm")) {
    return { valid: false, error: "File must be an .rpm package" };
  }
  const match = filename.match(/^(.+?)-\d/);
  const extractedName = match ? match[1] : null;
  if (!extractedName || extractedName !== packageName) {
    return {
      valid: false,
      error: `Expected package "${packageName}", filename suggests "${extractedName ?? "unknown"}"`,
    };
  }
  const validArch =
    filename.endsWith(`.${arch}.rpm`) || filename.endsWith(".noarch.rpm");
  if (!validArch) {
    return {
      valid: false,
      error: `Expected architecture "${arch}" or "noarch"`,
    };
  }
  return { valid: true };
}

export function RpmUploadModal({
  isOpen,
  packageName,
  packageArch,
  onUpload,
  onClose,
  triggerRef,
}: RpmUploadModalProps) {
  const [file, setFile] = useState<File | null>(null);
  const [filename, setFilename] = useState("");
  const [validation, setValidation] = useState<{
    valid: boolean;
    error?: string;
  } | null>(null);
  const [uploading, setUploading] = useState(false);
  const [matchResult, setMatchResult] = useState<UploadMatchResult | null>(
    null,
  );
  const uploadAreaRef = useRef<HTMLDivElement>(null);

  // Focus the upload area on open
  useEffect(() => {
    if (isOpen) {
      // Defer to let modal mount
      const timer = setTimeout(() => {
        uploadAreaRef.current?.focus();
      }, 50);
      return () => clearTimeout(timer);
    }
  }, [isOpen]);

  const handleFileChange = useCallback(
    (_event: unknown, selectedFile: File) => {
      setFile(selectedFile);
      setFilename(selectedFile.name);
      setValidation(
        validateRpmFile(packageName, packageArch, selectedFile.name),
      );
    },
    [packageName, packageArch],
  );

  const handleClear = useCallback(() => {
    setFile(null);
    setFilename("");
    setValidation(null);
    setMatchResult(null);
  }, []);

  const handleConfirm = useCallback(async () => {
    if (file && validation?.valid) {
      setUploading(true);
      try {
        const result = await onUpload(packageName, file);
        if (result) {
          setMatchResult(result);
        } else {
          // Legacy: onUpload returned void — close immediately
          handleClear();
          onClose();
          triggerRef.current?.focus();
        }
      } catch {
        setMatchResult(null);
      } finally {
        setUploading(false);
      }
    }
  }, [
    file,
    validation,
    packageName,
    onUpload,
    onClose,
    handleClear,
    triggerRef,
  ]);

  const handleClose = useCallback(() => {
    handleClear();
    setUploading(false);
    onClose();
    // Return focus to trigger element
    triggerRef.current?.focus();
  }, [onClose, handleClear, triggerRef]);

  if (!isOpen) return null;

  return (
    <Modal
      variant={ModalVariant.medium}
      isOpen={isOpen}
      onClose={handleClose}
      aria-label={`Upload RPM for ${packageName}`}
    >
      <ModalHeader title={`Upload RPM for ${packageName}`} />
      <ModalBody>
        <Content component="p">
          Expected filename pattern:{" "}
          <code>
            {packageName}-*-*.{packageArch}.rpm
          </code>
        </Content>
        <div ref={uploadAreaRef} tabIndex={-1}>
          <FileUpload
            id={`rpm-upload-${packageName}`}
            value={file ?? undefined}
            filename={filename}
            onFileInputChange={handleFileChange}
            onClearClick={handleClear}
            browseButtonText="Choose RPM"
            dropzoneProps={{
              accept: { "application/x-rpm": [".rpm"] },
            }}
            aria-label={`Upload RPM for ${packageName}`}
          />
        </div>
        {validation && !matchResult && (
          <HelperText>
            <HelperTextItem
              variant={validation.valid ? "success" : "error"}
              icon={
                validation.valid ? (
                  <CheckCircleIcon />
                ) : (
                  <ExclamationCircleIcon />
                )
              }
            >
              {validation.valid
                ? `${filename} matches ${packageName}`
                : validation.error}
            </HelperTextItem>
          </HelperText>
        )}
        {matchResult && matchResult.status === "matched" && (
          <HelperText data-testid="upload-match-result">
            <HelperTextItem variant="success" icon={<CheckCircleIcon />}>
              Matched to {matchResult.matched}
            </HelperTextItem>
          </HelperText>
        )}
        {matchResult && matchResult.status === "unmatched" && (
          <Alert
            variant="warning"
            isInline
            isPlain
            title="No matching package"
            data-testid="upload-unmatched-warning"
          >
            RPM uploaded but no matching package found. Check that the filename
            matches a package in the list.
          </Alert>
        )}
      </ModalBody>
      <ModalFooter>
        {matchResult ? (
          <Button variant="primary" onClick={handleClose}>
            Done
          </Button>
        ) : (
          <>
            <Button
              variant="primary"
              onClick={handleConfirm}
              isDisabled={!file || !validation?.valid || uploading}
              isLoading={uploading}
              aria-label="Confirm upload"
            >
              Upload
            </Button>
            <Button
              variant="link"
              onClick={handleClose}
              isDisabled={uploading}
            >
              Cancel
            </Button>
          </>
        )}
      </ModalFooter>
    </Modal>
  );
}
