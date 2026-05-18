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
import { UserCard } from "./UserCard";
import { UserArtifactPreview } from "./UserArtifactPreview";

export interface UsersGroupsSectionProps {
  users: UserDecision[];
  /** SSH authorized_keys refs from the snapshot section data. */
  sshAuthorizedKeysRefs: Array<{ user: string; path: string }>;
  /** Sudoers rules from the snapshot section data. */
  sudoersRules: string[];
  /** Subuid entries from the snapshot section data. */
  subuidEntries: string[];
  sessionIsSensitive: boolean;
  onViewUpdate: (view: ViewResponse) => void;
  onMutationError: (err: Error) => void;
}

export function UsersGroupsSection({
  users,
  sshAuthorizedKeysRefs,
  sudoersRules,
  subuidEntries,
  sessionIsSensitive,
  onViewUpdate,
  onMutationError,
}: UsersGroupsSectionProps) {
  const [isPending, setIsPending] = useState(false);
  const [previewOpen, setPreviewOpen] = useState(false);

  // Build lookup maps for per-user badge data
  const sshRefsByUser = useMemo(() => {
    const map = new Map<string, Array<{ user: string; path: string }>>();
    for (const ref of sshAuthorizedKeysRefs) {
      const existing = map.get(ref.user) ?? [];
      existing.push(ref);
      map.set(ref.user, existing);
    }
    return map;
  }, [sshAuthorizedKeysRefs]);

  const sudoUsers = useMemo(() => {
    const set = new Set<string>();
    for (const rule of sudoersRules) {
      // Simple heuristic: first word of a sudoers rule is often the username
      const match = rule.match(/^(\S+)\s/);
      if (match && !match[1].startsWith("#") && !match[1].startsWith("%")) {
        set.add(match[1]);
      }
    }
    return set;
  }, [sudoersRules]);

  const subuidUsers = useMemo(() => {
    const set = new Set<string>();
    for (const entry of subuidEntries) {
      const parts = entry.split(":");
      if (parts.length >= 1 && parts[0]) {
        set.add(parts[0]);
      }
    }
    return set;
  }, [subuidEntries]);

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
          sshRefs={sshRefsByUser.get(user.name) ?? []}
          hasSubuid={subuidUsers.has(user.name)}
          hasSudo={sudoUsers.has(user.name)}
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
