import { useEffect, useRef, useState } from "react";
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

/** Non-distro repos with verified or incomplete provenance and at least one
 *  effective (actionable, unlocked) member are toggleable. Everything else
 *  -- disabled, stale, unknown, or a verified/incomplete repo with nothing
 *  left to decide -- gets state text instead (§ 6.3). */
const canToggleRepo = (repo: RepoGroupInfo): boolean =>
  !repo.is_distro &&
  (repo.provenance === "verified" || repo.provenance === "incomplete") &&
  repo.effective_member_count > 0;

/** § 6.3 state text for the badge cell when no control is shown. */
function noControlStateText(repo: RepoGroupInfo): string {
  switch (repo.provenance) {
    case "disabled":
      return "disabled on host";
    case "stale":
      return "repo id not found in any .repo file";
    case "unknown":
      return "installed outside any repo";
    default:
      // verified or incomplete, effective_member_count === 0
      return "nothing to decide: all members are locked";
  }
}

export function RepoBar({
  repos,
  onToggle,
  conflictCount,
  dismissedCount,
  onRestoreDismissed,
}: RepoBarProps) {
  const distroRepos = repos.filter((r) => r.is_distro);
  const toggleableRepos = repos.filter(canToggleRepo);
  // Non-distro repos the server refuses to toggle: disabled, stale, unknown
  // provenance, or a verified/incomplete repo with nothing left to decide.
  const nonToggleableRepos = repos.filter(
    (r) => !r.is_distro && !canToggleRepo(r),
  );

  const visibleConflicts = (conflictCount ?? 0) - (dismissedCount ?? 0);

  // Screen reader announcement for dismiss/restore actions
  const [announcement, setAnnouncement] = useState("");
  const prevDismissedRef = useRef(dismissedCount ?? 0);

  useEffect(() => {
    const prev = prevDismissedRef.current;
    const curr = dismissedCount ?? 0;
    prevDismissedRef.current = curr;

    if (curr > prev) {
      const delta = curr - prev;
      setAnnouncement(
        `${delta} ${delta === 1 ? "conflict" : "conflicts"} dismissed`,
      );
    } else if (curr < prev) {
      setAnnouncement("All conflicts restored");
    }
  }, [dismissedCount]);

  return (
    <div className="inspectah-repo-bar" data-testid="repo-bar">
      <div className="inspectah-repo-bar__label">Repositories</div>

      {distroRepos.map((repo) => (
        <div key={repo.section_id} className="inspectah-repo-bar__row">
          <div className="inspectah-repo-bar__name">
            <span style={{ color: tierColors.distro }}>{repo.section_id}</span>
            <span className="inspectah-repo-bar__count">
              {repo.package_count}
            </span>
          </div>
          <span className="inspectah-repo-bar__always">always included</span>
        </div>
      ))}

      {nonToggleableRepos.map((repo) => (
        // No aria-describedby here: this row is a bare div with no role or
        // tabindex (maps to `generic`), so the attribute would be inert --
        // not exposed in the accessibility tree. The association lands in
        // phase 1 when these rows join PackageList's list/listitem tree.
        <div key={repo.section_id} className="inspectah-repo-bar__row">
          <div className="inspectah-repo-bar__name">
            <span style={{ color: tierColors.distro }}>
              {repo.section_id === "@commandline"
                ? "Local / Manual installs"
                : repo.section_id}
            </span>
            <span className="inspectah-repo-bar__count">
              {repo.package_count}
            </span>
          </div>
          <span className="inspectah-repo-bar__always">
            {noControlStateText(repo)}
          </span>
        </div>
      ))}

      {toggleableRepos.map((repo) => (
        <div key={repo.section_id} className="inspectah-repo-bar__row">
          <div className="inspectah-repo-bar__name">
            <span style={{ color: tierColors[repo.tier] ?? tierColors.distro }}>
              {repo.section_id}
            </span>
            <span className="inspectah-repo-bar__count">
              {repo.package_count}
            </span>
          </div>
          {repo.provenance === "incomplete" && (
            <span className="inspectah-repo-bar__incomplete">
              repo definition incomplete
            </span>
          )}
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
              {visibleConflicts}{" "}
              {visibleConflicts === 1 ? "conflict" : "conflicts"}
            </Label>
          )}
          {dismissedCount != null &&
            dismissedCount > 0 &&
            onRestoreDismissed && (
              <Button
                variant="link"
                isInline
                onClick={onRestoreDismissed}
                style={{
                  fontSize: "var(--pf-t--global--font--size--body--sm)",
                }}
              >
                Show {dismissedCount} dismissed
              </Button>
            )}
        </div>
      )}
      <span
        className="inspectah-sr-only"
        aria-live="assertive"
        aria-atomic="true"
        data-testid="repo-bar-announcement"
      >
        {announcement}
      </span>
    </div>
  );
}
