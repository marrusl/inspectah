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

  useEffect(() => {
    if (!isOpen) return;
    setLoading(true);
    setError(null);
    fetchUserPreview()
      .then((resp) => setData(resp))
      .catch((err) =>
        setError(err instanceof Error ? err.message : String(err)),
      )
      .finally(() => setLoading(false));
  }, [isOpen]);

  const handleClose = useCallback(() => {
    setData(null);
    setError(null);
    onClose();
  }, [onClose]);

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
