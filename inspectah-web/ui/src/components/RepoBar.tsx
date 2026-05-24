import { Button, Label } from "@patternfly/react-core";
import type { RepoGroupInfo } from "../api/types";

export interface RepoBarProps {
  repos: RepoGroupInfo[];
  onToggle: (sectionId: string) => void;
  conflictCount?: number;
  dismissedCount?: number;
  onRestoreDismissed?: () => void;
}

const tierStyles: Record<string, React.CSSProperties> = {
  official_optional: {
    color: "var(--pf-t--global--color--status--success--default)",
    textDecoration: "underline",
    textDecorationStyle: "dotted" as const,
    textUnderlineOffset: "3px",
  },
  third_party: {
    color: "var(--pf-t--global--color--status--warning--default)",
    textDecoration: "underline",
    textDecorationStyle: "solid" as const,
    textUnderlineOffset: "3px",
  },
};

const pillBase: React.CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  padding: "2px var(--pf-t--global--spacer--sm)",
  borderRadius: "var(--pf-t--global--border--radius--pill)",
  border: "1px solid var(--pf-t--global--border--color--default)",
  background: "transparent",
  cursor: "pointer",
  fontSize: "var(--pf-t--global--font--size--body--sm)",
  lineHeight: 1.4,
  gap: "var(--pf-t--global--spacer--xs)",
};

export function RepoBar({
  repos,
  onToggle,
  conflictCount,
  dismissedCount,
  onRestoreDismissed,
}: RepoBarProps) {
  const distroRepos = repos.filter((r) => r.is_distro);
  const toggleableRepos = repos.filter((r) => !r.is_distro);

  return (
    <div
      data-testid="repo-bar"
      style={{
        display: "flex",
        flexDirection: "column",
        gap: "var(--pf-t--global--spacer--xs)",
        padding: "var(--pf-t--global--spacer--sm) 0",
      }}
    >
      {/* Row 1: Distro repos — static text */}
      {distroRepos.length > 0 && (
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: "var(--pf-t--global--spacer--sm)",
            fontSize: "var(--pf-t--global--font--size--body--sm)",
            color: "var(--pf-t--global--text--color--subtle)",
          }}
        >
          {distroRepos.map((repo, i) => (
            <span key={repo.section_id}>
              {i > 0 && " · "}
              {repo.section_id} ({repo.package_count})
            </span>
          ))}
        </div>
      )}

      {/* Row 2: Toggleable repos + optional conflict/dismiss controls */}
      {(toggleableRepos.length > 0 || conflictCount != null) && (
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: "var(--pf-t--global--spacer--sm)",
            flexWrap: "wrap",
          }}
        >
          {toggleableRepos.map((repo) => (
            <button
              key={repo.section_id}
              role="switch"
              aria-checked={repo.enabled}
              aria-label={`${repo.section_id} (${repo.package_count})`}
              onClick={() => onToggle(repo.section_id)}
              style={{
                ...pillBase,
                ...(tierStyles[repo.tier] ?? {}),
              }}
            >
              {repo.section_id} ({repo.package_count})
            </button>
          ))}

          {conflictCount != null && conflictCount > 0 && (
            <Label color="orange" isCompact aria-live="polite">
              {conflictCount} conflicts
            </Label>
          )}

          {dismissedCount != null && dismissedCount > 0 && onRestoreDismissed && (
            <Button
              variant="link"
              isInline
              onClick={onRestoreDismissed}
              style={{ fontSize: "var(--pf-t--global--font--size--body--sm)" }}
            >
              Show {dismissedCount} dismissed
            </Button>
          )}
        </div>
      )}
    </div>
  );
}
