import { Button, Label, Switch } from "@patternfly/react-core";
import type { RepoGroupInfo } from "../api/types";

export interface RepoBarProps {
  repos: RepoGroupInfo[];
  onToggle: (sectionId: string) => void;
  conflictCount?: number;
  dismissedCount?: number;
  onRestoreDismissed?: () => void;
}

const tierColors: Record<string, string> = {
  distro: "var(--pf-t--global--text--color--subtle)",
  official_optional: "var(--pf-t--global--color--status--success--default)",
  third_party: "var(--pf-t--global--text--color--status--warning--default)",
};

export function RepoBar({
  repos,
  onToggle,
  conflictCount,
  dismissedCount,
  onRestoreDismissed,
}: RepoBarProps) {
  const distroRepos = repos.filter((r) => r.is_distro);
  // Repos with unknown provenance (e.g. @commandline / locally installed) are
  // not toggleable — they represent packages installed outside any repo.
  const nonToggleableRepos = repos.filter((r) => !r.is_distro && r.provenance === "unknown" && r.package_count > 0);
  const toggleableRepos = repos.filter((r) => !r.is_distro && r.provenance !== "unknown");

  const visibleConflicts = (conflictCount ?? 0) - (dismissedCount ?? 0);

  return (
    <div className="inspectah-repo-bar" data-testid="repo-bar">
      <div className="inspectah-repo-bar__label">Repositories</div>

      {distroRepos.map((repo) => (
        <div key={repo.section_id} className="inspectah-repo-bar__row">
          <div className="inspectah-repo-bar__name">
            <span style={{ color: tierColors.distro }}>{repo.section_id}</span>
            <span className="inspectah-repo-bar__count">{repo.package_count}</span>
          </div>
          <span className="inspectah-repo-bar__always">always included</span>
        </div>
      ))}

      {nonToggleableRepos.map((repo) => (
        <div key={repo.section_id} className="inspectah-repo-bar__row">
          <div className="inspectah-repo-bar__name">
            <span style={{ color: tierColors.distro }}>
              {repo.section_id === "@commandline" ? "Local / Manual installs" : repo.section_id}
            </span>
            <span className="inspectah-repo-bar__count">{repo.package_count}</span>
          </div>
          <span className="inspectah-repo-bar__always">
            {repo.section_id === "@commandline" ? "not included" : "always included"}
          </span>
        </div>
      ))}

      {toggleableRepos.map((repo) => (
        <div key={repo.section_id} className="inspectah-repo-bar__row">
          <div className="inspectah-repo-bar__name">
            <span style={{ color: tierColors[repo.tier] ?? tierColors.distro }}>
              {repo.section_id}
            </span>
            <span className="inspectah-repo-bar__count">{repo.package_count}</span>
          </div>
          <Switch
            id={`repo-toggle-${repo.section_id}`}
            aria-label={`${repo.section_id} (${repo.package_count})`}
            isChecked={repo.enabled}
            onChange={() => onToggle(repo.section_id)}
            isReversed
          />
        </div>
      ))}

      {(visibleConflicts > 0 || (dismissedCount ?? 0) > 0) && (
        <div className="inspectah-repo-bar__controls">
          {visibleConflicts > 0 && (
            <Label color="orange" isCompact aria-live="polite">
              {visibleConflicts} {visibleConflicts === 1 ? "conflict" : "conflicts"}
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
