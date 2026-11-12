import { useState, useCallback, useMemo } from "react";
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
import { usePendingGuard } from "../hooks/usePendingGuard";
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
  const [previewOpen, setPreviewOpen] = useState(false);
  // Requests in flight, by username: the strategy and the password both
  // change the one user record, so a user's card is held while either is.
  const { pendingKeys, runGuarded } = usePendingGuard();

  const handleStrategyChange = useCallback(
    (username: string, strategy: "skip" | "useradd") => {
      runGuarded(username, () =>
        setUserStrategy(username, strategy)
          .then(onViewUpdate)
          .catch(onMutationError),
      );
    },
    [runGuarded, onViewUpdate, onMutationError],
  );

  /**
   * Sends the password choice. For a new password, `hashPassword` runs
   * inside the guard, so a second click while it hashes is dropped too.
   * Returns the outcome (rejecting on failure, after `onMutationError` for a
   * failed request), or `undefined` when the user already has a request in
   * flight and this one was dropped.
   */
  const handlePasswordChange = useCallback(
    (
      username: string,
      choice: "none" | "preserve" | "new",
      hashPassword?: () => Promise<string>,
    ): Promise<void> | undefined => {
      let outcome: Promise<void> | undefined;
      runGuarded(username, () => {
        outcome = (async () => {
          const hash = hashPassword ? await hashPassword() : undefined;
          try {
            const view = await setUserPassword(username, choice, hash);
            onViewUpdate(view);
          } catch (err) {
            onMutationError(
              err instanceof Error ? err : new Error(String(err)),
            );
            throw err;
          }
        })();
        // The caller handles the rejection through `outcome`; the guard
        // only needs to know when it settles.
        return outcome.catch(() => {});
      });
      return outcome;
    },
    [runGuarded, onViewUpdate, onMutationError],
  );

  // Summarize which users are included via useradd. The full set of
  // groupadd/useradd commands (including supplementary groups) is visible
  // in the artifact preview — don't try to replicate renderer logic here.
  const useraddSummary = useMemo(() => {
    const included = users.filter(
      (u) => u.containerfile_strategy === "useradd",
    );
    if (included.length === 0) return null;
    return included.map((u) => u.name);
  }, [users]);

  if (users.length === 0) {
    return (
      <PageSection>
        <Content>
          <h2>Users &amp; Groups</h2>
        </Content>
        <EmptyState titleText="No non-system users detected" headingLevel="h3">
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

      <div
        style={{
          fontSize: "var(--pf-t--global--font--size--sm)",
          opacity: 0.6,
          marginBottom: "var(--pf-t--global--spacer--md)",
          fontStyle: "italic",
        }}
      >
        {useraddSummary && useraddSummary.length > 0
          ? `Users added via useradd: ${useraddSummary.join(", ")}`
          : "No users added to Containerfile"}
      </div>

      {users.map((user) => (
        <UserCard
          key={user.name}
          user={user}
          isPending={pendingKeys.has(user.name)}
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
