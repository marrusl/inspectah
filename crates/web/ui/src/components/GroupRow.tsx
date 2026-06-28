import { useState, useCallback, useMemo, useRef } from "react";
import { Button } from "@patternfly/react-core";
import {
  AngleRightIcon,
  AngleDownIcon,
  LockIcon,
} from "@patternfly/react-icons";
import type { GroupInfo } from "../api/types";

export interface GroupRowProps {
  group: GroupInfo;
  onToggle: (groupName: string, include: boolean) => void;
  onUngroup: (groupName: string) => void;
  searchQuery?: string;
  isIncluded?: boolean;
  /** When true, group is forced open by search (overrides local state). */
  forceExpanded?: boolean;
  /** Initial expanded state (used to restore user expansion across remounts). */
  defaultExpanded?: boolean;
  /** Notifies parent when the user manually expands or collapses. */
  onExpandChange?: (groupName: string, expanded: boolean) => void;
  /** Announcement text for the aria-live region (toast feedback). */
  announcement?: string;
}

export function GroupRow({
  group,
  onToggle: _onToggle,
  onUngroup,
  searchQuery,
  isIncluded: _isIncluded = true,
  forceExpanded = false,
  defaultExpanded = false,
  onExpandChange,
  announcement,
}: GroupRowProps) {
  const [localExpanded, setLocalExpanded] = useState(defaultExpanded);
  const [showAll, setShowAll] = useState(false);
  const rowRef = useRef<HTMLDivElement>(null);

  // Group is visually expanded if user expanded it OR search forced it open
  const expanded = localExpanded || forceExpanded;

  const isDegraded = group.render_state === "degraded";
  const isExcluded = group.render_state === "excluded";
  const hasOptionalSpillover = group.optional_spillover_count > 0;

  const handleExpandToggle = useCallback(() => {
    const next = !localExpanded;
    setLocalExpanded(next);
    onExpandChange?.(group.name, next);
  }, [localExpanded, onExpandChange, group.name]);

  const handleUngroup = useCallback(
    (e: React.MouseEvent) => {
      e.stopPropagation();
      onUngroup(group.name);
    },
    [onUngroup, group.name],
  );

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Enter" && e.target === rowRef.current) {
        handleExpandToggle();
      }
    },
    [handleExpandToggle],
  );

  const sortedMembers = useMemo(
    () => [...group.members].sort((a, b) => a.name.localeCompare(b.name)),
    [group.members],
  );

  // Determine if search matches the group name itself
  const q = searchQuery?.trim().toLowerCase() ?? "";
  const groupNameMatches = q.length > 0 && group.name.toLowerCase().includes(q);

  const visibleMembers = showAll ? sortedMembers : sortedMembers.slice(0, 5);
  const hasMore = sortedMembers.length > 5;

  const pkgLabel = useMemo(() => {
    if (group.added_count === 0) {
      return `${group.member_count} ${group.member_count === 1 ? "package" : "packages"} (all from base)`;
    }
    if (group.added_count === group.member_count) {
      return group.member_count === 1
        ? "1 package"
        : `${group.member_count} packages`;
    }
    const baseCount = group.member_count - group.added_count;
    return `${group.added_count} new, ${baseCount} from base`;
  }, [group.member_count, group.added_count]);

  const rowClassName = [
    "inspectah-group-row",
    isDegraded ? "inspectah-group-row--degraded" : "",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <div
      ref={rowRef}
      data-testid={`group-row-${group.name}`}
      className={rowClassName}
      role="group"
      aria-label={`${group.name}, ${pkgLabel}`}
      tabIndex={-1}
      onKeyDown={handleKeyDown}
      {...(groupNameMatches ? { "data-search-match": "true" } : {})}
    >
      <div className="inspectah-group-row__header">
        <button
          className="inspectah-group-row__chevron"
          onClick={handleExpandToggle}
          aria-label={expanded ? "Collapse group" : "Expand group"}
          aria-expanded={expanded}
        >
          {expanded ? <AngleDownIcon /> : <AngleRightIcon />}
        </button>

        <span className="inspectah-group-row__name">{group.name}</span>

        <span className="inspectah-group-row__count">{pkgLabel}</span>

        {group.locked_count > 0 && (
          <span className="inspectah-group-row__locked-count">
            {group.locked_count} locked
          </span>
        )}

        {isDegraded && (
          <span className="inspectah-group-row__subtitle">
            rendered individually
          </span>
        )}

        {isExcluded && hasOptionalSpillover && (
          <span className="inspectah-group-row__subtitle">
            {group.optional_spillover_count} optional still included
          </span>
        )}

        <span className="inspectah-group-row__actions">
          <Button
            variant="link"
            size="sm"
            onClick={handleUngroup}
            aria-label={`Ungroup ${group.name}`}
            isDisabled={isDegraded}
          >
            ungroup
          </Button>
        </span>

        {/* Group-level toggle removed — groups are managed via ungroup
            (dissolves into individual packages) or per-member toggles.
            The toggle was confusing in degraded states where it appeared
            but did nothing. */}
      </div>

      {expanded && (
        <div className="inspectah-group-row__members" role="list">
          {visibleMembers.map((member) => {
            const memberMatches =
              q.length > 0 && member.name.toLowerCase().includes(q);
            return (
              <div
                key={member.name}
                data-testid={`group-member-${member.name}`}
                className="inspectah-group-row__member"
                role="listitem"
                tabIndex={-1}
                {...(memberMatches ? { "data-search-match": "true" } : {})}
              >
                <span
                  className={`inspectah-group-row__member-name${member.in_base_image ? " inspectah-group-row__member--from-base" : ""}`}
                  aria-label={
                    member.in_base_image
                      ? `${member.name} (from base image, no action needed)`
                      : undefined
                  }
                >
                  {member.name}
                  {member.in_base_image && (
                    <span className="inspectah-group-row__from-base-label">
                      {" "}
                      (from base)
                    </span>
                  )}
                </span>
                {member.locked && (
                  <span className="inspectah-group-row__member-locked">
                    <LockIcon /> locked
                  </span>
                )}
              </div>
            );
          })}
          {hasMore && !showAll && (
            <button
              className="inspectah-group-row__show-all"
              onClick={(e) => {
                e.stopPropagation();
                setShowAll(true);
              }}
            >
              Show all {sortedMembers.length} members
            </button>
          )}
          {hasMore && showAll && (
            <button
              className="inspectah-group-row__show-all"
              onClick={(e) => {
                e.stopPropagation();
                setShowAll(false);
              }}
            >
              Show less
            </button>
          )}
        </div>
      )}

      <div aria-live="polite" className="inspectah-sr-only">
        {announcement}
      </div>
    </div>
  );
}
