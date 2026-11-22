import { useEffect, useMemo, useRef, useState } from "react";
import {
  Alert,
  AlertActionCloseButton,
  Button,
  Label,
  Switch,
} from "@patternfly/react-core";
import type { RepoGroupInfo, RepoOwnedEntry } from "../api/types";
import {
  CARRIAGE_ONLY_NOTE,
  REPO_ROW_TESTID_PREFIX,
  basename,
  carriageOnlyBadge,
  hasRepoSwitch,
  ownedStateWord,
  repoCountText,
  repoNoControlText,
  repoOwnedExplanation,
  repoSwitchChecked,
  type RepoFailure,
} from "./repoControl";

export interface RepoBarProps {
  repos: RepoGroupInfo[];
  onToggle: (sectionId: string, include: boolean) => void;
  /** Repos (by `section_id`) whose toggle request is in flight. */
  pendingKeys?: ReadonlySet<string>;
  conflictCount?: number;
  dismissedCount?: number;
  onRestoreDismissed?: () => void;
  /**
   * The view's `repo_owned`: what the export does with each file and key a
   * repo owns. Absent on a fleet, whose view has none; the owns line then
   * names the files without a state.
   */
  repoOwned?: RepoOwnedEntry[];
  /** The last failed request of a repo, by `section_id`. */
  failures?: ReadonlyMap<string, RepoFailure>;
  /** Clears a repo's failure, when the operator dismisses it. */
  onDismissFailure?: (sectionId: string) => void;
}

const tierColors: Record<string, string> = {
  distro: "var(--pf-t--global--text--color--subtle)",
  official_optional: "var(--pf-t--global--color--status--success--default)",
  third_party: "var(--pf-t--global--text--color--status--warning--default)",
};

const NO_FAILURES: ReadonlyMap<string, RepoFailure> = new Map();

function displayName(repo: RepoGroupInfo): string {
  return repo.section_id === "@commandline"
    ? "Local / Manual installs"
    : repo.section_id;
}

/**
 * The files and keys a repo owns, one per item, by basename, with the
 * full destination and the export's state as its accessible name (spec
 * § 7.2). Nothing is said of a file that simply ships.
 */
function OwnsLine({
  repo,
  owned,
}: {
  repo: RepoGroupInfo;
  owned: ReadonlyMap<string, RepoOwnedEntry> | null;
}) {
  if (repo.owns.length === 0) return null;
  return (
    <ul
      className="inspectah-repo-bar__owns"
      aria-label={`Files and keys of repo ${repo.section_id}`}
    >
      {repo.owns.map((destination) => {
        const entry = owned?.get(destination);
        const word = entry ? ownedStateWord(entry) : "";
        const ships = entry ? entry.export.state === "ships" : true;
        return (
          <li
            key={destination}
            aria-label={
              entry ? `${destination}: ${word || "ships"}` : destination
            }
            className={
              ships
                ? "inspectah-repo-bar__owned"
                : "inspectah-repo-bar__owned inspectah-repo-bar__owned--left-out"
            }
          >
            <span className="inspectah-repo-bar__owned-name">
              {basename(destination)}
            </span>
            {word && (
              <span className="inspectah-repo-bar__owned-state">{word}</span>
            )}
          </li>
        );
      })}
    </ul>
  );
}

export function RepoBar({
  repos,
  onToggle,
  pendingKeys,
  conflictCount,
  dismissedCount,
  onRestoreDismissed,
  repoOwned,
  failures = NO_FAILURES,
  onDismissFailure,
}: RepoBarProps) {
  const distroRepos = repos.filter((r) => r.is_distro);
  // Non-distro repos with no switch: nothing the op could move.
  const noControlRepos = repos.filter(
    (r) => !r.is_distro && !hasRepoSwitch(r),
  );
  const switchRepos = repos.filter((r) => !r.is_distro && hasRepoSwitch(r));

  const owned = useMemo(
    () =>
      repoOwned
        ? new Map(repoOwned.map((e) => [e.destination, e] as const))
        : null,
    [repoOwned],
  );
  // Files and keys no row lists: owned by no repo, or only by repos with
  // no row. They still ship or not, so the section still says which.
  const unlisted = useMemo(() => {
    if (!repoOwned) return [];
    const listed = new Set(repos.flatMap((r) => r.owns));
    return repoOwned.filter((e) => !listed.has(e.destination));
  }, [repoOwned, repos]);

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

      {[...distroRepos, ...noControlRepos, ...switchRepos].map((repo) => {
        const failure = failures.get(repo.section_id);
        const hasSwitch = hasRepoSwitch(repo);
        const badge = hasSwitch
          ? carriageOnlyBadge(repo)
          : repoNoControlText(repo);
        const isChecked = repoSwitchChecked(repo);
        const countText = repoCountText(repo);
        const carriageOnly = repo.control === "carriage_only";
        return (
          <div
            key={repo.section_id}
            className="inspectah-repo-bar__repo"
            data-testid={`${REPO_ROW_TESTID_PREFIX}${repo.section_id}`}
            tabIndex={-1}
          >
            <div className="inspectah-repo-bar__row">
              <div className="inspectah-repo-bar__name">
                <span
                  style={{
                    color: hasSwitch
                      ? (tierColors[repo.tier] ?? tierColors.distro)
                      : tierColors.distro,
                  }}
                >
                  {displayName(repo)}
                </span>
                {(!hasSwitch || repo.is_distro) && (
                  <span className="inspectah-repo-bar__count">
                    {repo.package_count}
                  </span>
                )}
              </div>
              {hasSwitch && repo.provenance === "incomplete" && (
                <span className="inspectah-repo-bar__incomplete">
                  repo definition incomplete
                </span>
              )}
              {badge && (
                // No aria-describedby on a row with no switch: the row is a
                // bare div with no role, so the attribute would be inert.
                <span className="inspectah-repo-bar__always">{badge}</span>
              )}
              {carriageOnly && (
                <span className="inspectah-repo-bar__carriage-note">
                  {CARRIAGE_ONLY_NOTE}
                </span>
              )}
              {hasSwitch && (
                <Switch
                  id={`repo-toggle-${repo.section_id}`}
                  label={countText}
                  // aria-label overrides the visible repo name, so the id
                  // has to be in here too: two repos with equal counts
                  // would otherwise announce identically. A carriage-only
                  // switch says it leaves the packages alone.
                  aria-label={
                    carriageOnly
                      ? `${repo.section_id}: ${countText}; ${CARRIAGE_ONLY_NOTE}, packages unchanged`
                      : `${repo.section_id}: ${countText}`
                  }
                  isChecked={isChecked}
                  isDisabled={pendingKeys?.has(repo.section_id)}
                  onChange={() => onToggle(repo.section_id, !isChecked)}
                  isReversed
                />
              )}
            </div>
            <OwnsLine repo={repo} owned={owned} />
            <div aria-live="polite" className="inspectah-repo-bar__failure">
              {failure && (
                <Alert
                  variant={failure.kind === "failed" ? "warning" : "info"}
                  isInline
                  isPlain
                  title={failure.text}
                  data-testid={`repo-failure-${repo.section_id}`}
                  actionClose={
                    onDismissFailure ? (
                      <AlertActionCloseButton
                        aria-label={`Dismiss the message for repo ${repo.section_id}`}
                        onClose={() => onDismissFailure(repo.section_id)}
                      />
                    ) : undefined
                  }
                />
              )}
            </div>
          </div>
        );
      })}

      {unlisted.length > 0 && (
        <div
          className="inspectah-repo-bar__unlisted"
          data-testid="repo-bar-unlisted"
        >
          <div className="inspectah-repo-bar__unlisted-title" id="repo-bar-unlisted-title">
            Files and keys no repo switch decides
          </div>
          <ul aria-labelledby="repo-bar-unlisted-title">
            {unlisted.map((entry) => {
              const word = ownedStateWord(entry);
              return (
                <li
                  key={entry.destination}
                  className="inspectah-repo-bar__unlisted-item"
                >
                  <span className="inspectah-repo-bar__owned-name">
                    {entry.destination}
                  </span>
                  {word && (
                    <span className="inspectah-repo-bar__owned-state">
                      {word}
                    </span>
                  )}
                  <p className="inspectah-repo-bar__unlisted-why">
                    {repoOwnedExplanation(entry)}
                  </p>
                </li>
              );
            })}
          </ul>
        </div>
      )}

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
