import { useCallback } from "react";
import { Switch } from "@patternfly/react-core";
import { AngleRightIcon, AngleDownIcon } from "@patternfly/react-icons";
import type { RepoGroupInfo } from "../api/types";
import {
  CARRIAGE_ONLY_NOTE,
  carriageOnlyBadge,
  hasRepoSwitch,
  repoCountText,
  repoNoControlText,
  repoSwitchChecked,
} from "./repoControl";

export interface RepoGroupHeaderProps {
  /** The repo row; its control, decision and counts drive the switch
   *  (spec § 5.5), as they do in `RepoBar`. */
  repo: RepoGroupInfo;
  isExpanded?: boolean;
  /** Number of informational packages — shown in collapsed header */
  infoCount?: number;
  /** Summary text like "No action needed" for all-routine repos */
  summaryText?: string;
  /** Roving tabindex value — 0 when this header is the focused item, -1 otherwise */
  tabIndex?: number;
  onToggle?: (sectionId: string, include: boolean) => void;
  onExpandToggle?: () => void;
  onKeyDown?: (e: React.KeyboardEvent<HTMLDivElement>) => void;
}

/**
 * Source classification label:
 * - Distro repos: no label
 * - ALL non-distro repos: "Third-party" (regardless of provenance)
 */
function classificationLabel(isDistro: boolean): string | null {
  if (isDistro) return null;
  return "Third-party";
}

export function RepoGroupHeader({
  repo,
  isExpanded = false,
  infoCount,
  summaryText,
  tabIndex = 0,
  onToggle,
  onExpandToggle,
  onKeyDown: onKeyDownProp,
}: RepoGroupHeaderProps) {
  const {
    section_id: sectionId,
    provenance,
    is_distro: isDistro,
    package_count: packageCount,
    kept,
  } = repo;
  const canToggle = hasRepoSwitch(repo);
  // A row with no switch says why; a carriage-only row keeps its badge
  // (`always included`, `disabled on host`) beside its switch.
  const stateText = canToggle ? carriageOnlyBadge(repo) : repoNoControlText(repo);
  const showStateText = stateText !== null;
  const isChecked = repoSwitchChecked(repo);
  const countText = repoCountText(repo);
  const label = classificationLabel(isDistro);
  const contentId = `repo-group-content-${sectionId}`;
  const stateTextId = `repo-state-${sectionId}`;
  const displayName =
    provenance === "unknown" && sectionId === "@commandline"
      ? "Local / Manual installs"
      : sectionId;

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLDivElement>) => {
      if (onKeyDownProp) {
        onKeyDownProp(e);
        if (e.defaultPrevented) return;
      }
      if (e.key === "Enter") {
        e.preventDefault();
        onExpandToggle?.();
      }
      if (e.key === " ") {
        e.preventDefault();
      }
    },
    [onKeyDownProp, onExpandToggle],
  );

  const handleChevronClick = useCallback(
    (e: React.MouseEvent) => {
      e.stopPropagation();
      onExpandToggle?.();
    },
    [onExpandToggle],
  );

  return (
    <div
      data-testid={`repo-group-${sectionId}`}
      role="row"
      aria-expanded={isExpanded}
      aria-controls={contentId}
      aria-describedby={showStateText ? stateTextId : undefined}
      tabIndex={tabIndex}
      onKeyDown={handleKeyDown}
      className="inspectah-repo-group-header"
    >
      <span
        className="inspectah-repo-group-header__chevron"
        onClick={handleChevronClick}
        role="presentation"
      >
        {isExpanded ? <AngleDownIcon /> : <AngleRightIcon />}
      </span>
      <span className="inspectah-repo-group-header__label">
        {displayName}
      </span>
      {label && (
        <span className="inspectah-repo-group-header__classification">
          {label}
        </span>
      )}
      <span className="inspectah-repo-group-header__count">
        {`${packageCount} ${packageCount === 1 ? "package" : "packages"}`}
      </span>
      {kept && infoCount != null && infoCount > 0 && (
        <span className="inspectah-repo-group-header__info-count">
          {infoCount} informational
        </span>
      )}
      {kept && summaryText && (
        <span className="inspectah-repo-group-header__summary">
          {summaryText}
        </span>
      )}
      {canToggle && provenance === "incomplete" && (
        <span className="inspectah-repo-group-header__incomplete">
          repo definition incomplete
        </span>
      )}
      {canToggle && (
        <span
          className="inspectah-repo-group-header__toggle"
          onClick={(e) => e.stopPropagation()}
        >
          <Switch
            id={`repo-toggle-${sectionId}`}
            label={countText}
            isChecked={isChecked}
            onChange={() => onToggle?.(sectionId, !isChecked)}
            // The section id first, as in RepoBar, so two repos with equal
            // counts announce differently.
            aria-label={
              repo.control === "carriage_only"
                ? `${sectionId}: ${countText}; ${CARRIAGE_ONLY_NOTE}, packages unchanged`
                : `${sectionId}: ${countText}`
            }
          />
        </span>
      )}
      {showStateText && (
        <span
          id={stateTextId}
          className="inspectah-repo-group-header__state-text"
        >
          {stateText}
        </span>
      )}
    </div>
  );
}
