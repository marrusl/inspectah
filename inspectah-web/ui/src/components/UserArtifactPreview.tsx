import { useState, useEffect, useCallback } from "react";
import {
  Modal,
  ModalVariant,
  ModalHeader,
  ModalBody,
  ModalFooter,
  Button,
  Spinner,
  Alert,
} from "@patternfly/react-core";
import type { UserPreviewResponse } from "../api/types";
import { fetchUserPreview } from "../api/client";

export interface UserArtifactPreviewProps {
  isOpen: boolean;
  onClose: () => void;
}

type TabId = "kickstart" | "blueprint_toml";

export function UserArtifactPreview({
  isOpen,
  onClose,
}: UserArtifactPreviewProps) {
  const [activeTab, setActiveTab] = useState<TabId>("kickstart");
  const [data, setData] = useState<UserPreviewResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [revealed, setRevealed] = useState(false);

  const loadPreview = useCallback(
    (reveal: boolean) => {
      setLoading(true);
      setError(null);
      fetchUserPreview(reveal)
        .then((resp) => {
          setData(resp);
          setRevealed(reveal);
        })
        .catch((err) =>
          setError(err instanceof Error ? err.message : String(err)),
        )
        .finally(() => setLoading(false));
    },
    [],
  );

  useEffect(() => {
    if (!isOpen) return;
    setRevealed(false);
    loadPreview(false);
  }, [isOpen, loadPreview]);

  const handleClose = useCallback(() => {
    setData(null);
    setError(null);
    setRevealed(false);
    onClose();
  }, [onClose]);

  const handleRevealToggle = useCallback(() => {
    loadPreview(!revealed);
  }, [revealed, loadPreview]);

  const content =
    activeTab === "kickstart"
      ? data?.kickstart ?? ""
      : data?.blueprint_toml ?? "";

  return (
    <Modal
      variant={ModalVariant.large}
      isOpen={isOpen}
      onClose={handleClose}
      aria-label="User artifact preview"
    >
      <ModalHeader title="User Artifact Preview" />
      <ModalBody>
        {/* Tab buttons */}
        <div
          style={{
            display: "flex",
            gap: "var(--pf-t--global--spacer--sm)",
            marginBottom: "var(--pf-t--global--spacer--md)",
            borderBottom:
              "1px solid var(--pf-t--global--border--color--default)",
            paddingBottom: "var(--pf-t--global--spacer--xs)",
          }}
        >
          <button
            onClick={() => setActiveTab("kickstart")}
            style={{
              background: "none",
              border: "none",
              cursor: "pointer",
              padding: "4px 8px",
              fontWeight: activeTab === "kickstart" ? 700 : 400,
              borderBottom:
                activeTab === "kickstart"
                  ? "2px solid var(--pf-t--global--color--brand--default)"
                  : "2px solid transparent",
            }}
          >
            Kickstart
          </button>
          <button
            onClick={() => setActiveTab("blueprint_toml")}
            style={{
              background: "none",
              border: "none",
              cursor: "pointer",
              padding: "4px 8px",
              fontWeight: activeTab === "blueprint_toml" ? 700 : 400,
              borderBottom:
                activeTab === "blueprint_toml"
                  ? "2px solid var(--pf-t--global--color--brand--default)"
                  : "2px solid transparent",
            }}
          >
            Blueprint TOML
          </button>
        </div>

        {/* Sensitive redaction banner */}
        {data?.sensitive && !loading && (
          <Alert
            variant={revealed ? "warning" : "info"}
            isInline
            title={
              revealed
                ? "Sensitive values are visible."
                : "Sensitive values are redacted."
            }
            style={{ marginBottom: "var(--pf-t--global--spacer--sm)" }}
            actionLinks={
              <Button
                variant="link"
                isInline
                onClick={handleRevealToggle}
                isDisabled={loading}
              >
                {revealed ? "Redact values" : "Click to reveal"}
              </Button>
            }
          />
        )}

        {loading && (
          <div style={{ textAlign: "center", padding: "var(--pf-t--global--spacer--lg)" }}>
            <Spinner size="lg" aria-label="Loading preview" />
          </div>
        )}

        {error && (
          <Alert variant="danger" isInline title="Failed to load preview">
            {error}
          </Alert>
        )}

        {!loading && !error && data && (
          <pre
            style={{
              fontFamily: "monospace",
              fontSize: "var(--pf-t--global--font--size--sm)",
              whiteSpace: "pre-wrap",
              wordBreak: "break-word",
              maxHeight: 400,
              overflowY: "auto",
              padding: "var(--pf-t--global--spacer--sm)",
              background:
                "var(--pf-t--global--background--color--secondary--default)",
              borderRadius: "var(--pf-t--global--border--radius--small)",
            }}
          >
            {content || "(empty)"}
          </pre>
        )}
      </ModalBody>
      <ModalFooter>
        <Button variant="link" onClick={handleClose}>
          Close
        </Button>
      </ModalFooter>
    </Modal>
  );
}
