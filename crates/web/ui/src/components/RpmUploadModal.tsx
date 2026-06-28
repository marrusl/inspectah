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
  Content,
} from "@patternfly/react-core";
import {
  CheckCircleIcon,
  ExclamationCircleIcon,
} from "@patternfly/react-icons";

export interface RpmUploadModalProps {
  isOpen: boolean;
  packageName: string;
  packageArch: string;
  onUpload: (packageName: string, file: File) => void;
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
  }, []);

  const handleConfirm = useCallback(() => {
    if (file && validation?.valid) {
      onUpload(packageName, file);
      handleClear();
      onClose();
      // Return focus to trigger element
      triggerRef.current?.focus();
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
        {validation && (
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
      </ModalBody>
      <ModalFooter>
        <Button
          variant="primary"
          onClick={handleConfirm}
          isDisabled={!file || !validation?.valid}
          aria-label="Confirm upload"
        >
          Upload
        </Button>
        <Button variant="link" onClick={handleClose}>
          Cancel
        </Button>
      </ModalFooter>
    </Modal>
  );
}
