import { useCallback } from "react";
import { Switch } from "@patternfly/react-core";
import { AngleRightIcon, AngleDownIcon } from "@patternfly/react-icons";
import type { RepoProvenance } from "../api/types";

export interface RepoGroupHeaderProps {
  sectionId: string;
  provenance: RepoProvenance;
  isDistro: boolean;
  packageCount: number;
  enabled: boolean;
  isExpanded?: boolean;
  /** Number of informational packages — shown in collapsed header */
  infoCount?: number;
  /** Summary text like "No action needed" for all-routine repos */
  summaryText?: string;
  /** Roving tabindex value — 0 when this header is the focused item, -1 otherwise */
  tabIndex?: number;
  onToggle?: (sectionId: string, enabled: boolean) => void;
  onExpandToggle?: () => void;
  onKeyDown?: (e: React.KeyboardEvent<HTMLDivElement>) => void;
}

/** Only verified non-distro repos are toggleable. */
const showToggle = (isDistro: boolean, provenance: RepoProvenance): boolean =>
  !isDistro && provenance === "verified";

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
  sectionId,
  provenance,
  isDistro,
  packageCount,
  enabled,
  isExpanded = false,
  infoCount,
  summaryText,
  tabIndex = 0,
  onToggle,
  onExpandToggle,
  onKeyDown: onKeyDownProp,
}: RepoGroupHeaderProps) {
  const canToggle = showToggle(isDistro, provenance);
  const label = classificationLabel(isDistro);
  const contentId = `repo-group-content-${sectionId}`;

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

  const disabledStyle = !enabled
    ? { textDecoration: "line-through" as const, opacity: "0.6" }
    : {};

  return (
    <div
      data-testid={`repo-group-${sectionId}`}
      role="row"
      aria-expanded={isExpanded}
      aria-controls={contentId}
      tabIndex={tabIndex}
      onKeyDown={handleKeyDown}
      className={`inspectah-repo-group-header${!enabled ? " inspectah-repo-group-header--disabled" : ""}`}
    >
      <span
        className="inspectah-repo-group-header__chevron"
        onClick={handleChevronClick}
        role="presentation"
      >
        {isExpanded ? <AngleDownIcon /> : <AngleRightIcon />}
      </span>
      <span
        className="inspectah-repo-group-header__label"
        style={disabledStyle}
      >
        {sectionId}
      </span>
      {label && (
        <span className="inspectah-repo-group-header__classification">
          {label}
        </span>
      )}
      <span className="inspectah-repo-group-header__count">
        {!enabled
          ? `${packageCount} ${packageCount === 1 ? "package" : "packages"} excluded`
          : `${packageCount} ${packageCount === 1 ? "package" : "packages"}`}
      </span>
      {enabled && infoCount != null && infoCount > 0 && (
        <span className="inspectah-repo-group-header__info-count">
          {infoCount} informational
        </span>
      )}
      {enabled && summaryText && (
        <span className="inspectah-repo-group-header__summary">
          {summaryText}
        </span>
      )}
      {canToggle && (
        <span
          className="inspectah-repo-group-header__toggle"
          onClick={(e) => e.stopPropagation()}
        >
          <Switch
            id={`repo-toggle-${sectionId}`}
            label={enabled ? "Enabled" : "Disabled"}
            isChecked={enabled}
            onChange={() => onToggle?.(sectionId, !enabled)}
            aria-label={`Toggle ${sectionId} repo`}
          />
        </span>
      )}
    </div>
  );
}
