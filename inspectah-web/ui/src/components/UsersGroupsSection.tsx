import { useState, useCallback } from "react";
import {
  PageSection,
  Content,
  EmptyState,
  EmptyStateBody,
  Button,
  Alert,
} from "@patternfly/react-core";
import type { UserDecision, ViewResponse } from "../api/types";
import { setUserStrategy, setUserPassword } from "../api/client";
import { UserCard } from "./UserCard";
import { UserArtifactPreview } from "./UserArtifactPreview";

export interface UsersGroupsSectionProps {
  users: UserDecision[];
  sessionIsSensitive: boolean;
  onViewUpdate: (view: ViewResponse) => void;
  onMutationError: (err: Error) => void;
}

export function UsersGroupsSection({
  users,
  sessionIsSensitive,
  onViewUpdate,
  onMutationError,
}: UsersGroupsSectionProps) {
  const [isPending, setIsPending] = useState(false);
  const [previewOpen, setPreviewOpen] = useState(false);

  const handleStrategyChange = useCallback(
    (username: string, strategy: "skip" | "useradd") => {
      setIsPending(true);
      setUserStrategy(username, strategy)
        .then(onViewUpdate)
        .catch(onMutationError)
        .finally(() => setIsPending(false));
    },
    [onViewUpdate, onMutationError],
  );

  const handlePasswordChange = useCallback(
    (username: string, choice: "none" | "preserve" | "new", hash?: string) => {
      setIsPending(true);
      setUserPassword(username, choice, hash)
        .then(onViewUpdate)
        .catch(onMutationError)
        .finally(() => setIsPending(false));
    },
    [onViewUpdate, onMutationError],
  );

  if (users.length === 0) {
    return (
      <PageSection>
        <Content>
          <h2>Users &amp; Groups</h2>
        </Content>
        <EmptyState
          titleText="No non-system users detected"
          headingLevel="h3"
        >
          <EmptyStateBody>
            This system has no non-system users (UID 1000&ndash;59999) that
            require migration decisions.
          </EmptyStateBody>
        </EmptyState>
      </PageSection>
    );
  }

  return (
    <PageSection>
      <Content>
        <h2>Users &amp; Groups</h2>
      </Content>

      <Alert
        variant="info"
        isInline
        title="User migration produces Kickstart and Blueprint TOML artifacts for the output tarball."
        style={{ marginBottom: "var(--pf-t--global--spacer--md)" }}
      />

      {sessionIsSensitive && (
        <Alert
          variant="warning"
          isInline
          title="This session contains sensitive data (password hashes). Export will require explicit acknowledgment."
          style={{ marginBottom: "var(--pf-t--global--spacer--md)" }}
        />
      )}

      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: "var(--pf-t--global--spacer--sm)",
          marginBottom: "var(--pf-t--global--spacer--md)",
        }}
      >
        <span
          style={{
            fontSize: "var(--pf-t--global--font--size--sm)",
            opacity: 0.7,
          }}
        >
          {users.length} user{users.length !== 1 ? "s" : ""} detected
        </span>
        <Button
          variant="secondary"
          size="sm"
          onClick={() => setPreviewOpen(true)}
        >
          Preview Artifacts
        </Button>
      </div>

      {users.map((user) => (
        <UserCard
          key={user.name}
          user={user}
          isPending={isPending}
          onStrategyChange={handleStrategyChange}
          onPasswordChange={handlePasswordChange}
        />
      ))}

      <UserArtifactPreview
        isOpen={previewOpen}
        onClose={() => setPreviewOpen(false)}
      />
    </PageSection>
  );
}
