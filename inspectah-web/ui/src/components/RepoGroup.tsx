import { useState, useCallback, useEffect } from "react";
import type { RepoGroupInfo } from "../api/types";
import { RepoGroupHeader } from "./RepoGroupHeader";
import { itemId as getItemId } from "./DecisionItem";
import type { DecisionItemKind } from "./DecisionItem";

export interface RepoGroupProps {
  repo: RepoGroupInfo;
  defaultExpanded: boolean;
  /** Override: force-expand when search filter matches items in this group */
  forceExpanded?: boolean;
  /** Number of informational packages — shown in collapsed header */
  infoCount?: number;
  /** Summary text for collapsed header (e.g., "No action needed") */
  summaryText?: string;
  /** When set, auto-expands if this item ID belongs to this group */
  revealItemId?: string;
  /** Item IDs in this group, for revealItemId matching */
  itemIds?: string[];
  /** Roving tabindex value for the header row */
  tabIndex?: number;
  onRepoToggle?: (sectionId: string, enabled: boolean) => void;
  onKeyDown?: (e: React.KeyboardEvent<HTMLDivElement>) => void;
  children: React.ReactNode;
}

export function RepoGroup({
  repo,
  defaultExpanded,
  forceExpanded = false,
  infoCount,
  summaryText,
  revealItemId,
  itemIds,
  tabIndex,
  onRepoToggle,
  onKeyDown,
  children,
}: RepoGroupProps) {
  const [isExpanded, setIsExpanded] = useState(defaultExpanded);

  // Auto-expand when revealItemId matches an item in this group
  useEffect(() => {
    if (!revealItemId || !itemIds) return;
    if (itemIds.includes(revealItemId) && !isExpanded) {
      setIsExpanded(true);
    }
  }, [revealItemId, itemIds, isExpanded]);

  const effectiveExpanded = forceExpanded || isExpanded;
  const contentId = `repo-group-content-${repo.section_id}`;

  const handleExpandToggle = useCallback(() => {
    setIsExpanded((prev) => !prev);
  }, []);

  return (
    <div data-testid={`repo-group-wrapper-${repo.section_id}`}>
      <RepoGroupHeader
        sectionId={repo.section_id}
        provenance={repo.provenance}
        isDistro={repo.is_distro}
        packageCount={repo.package_count}
        enabled={repo.enabled}
        isExpanded={effectiveExpanded}
        infoCount={infoCount}
        summaryText={summaryText}
        tabIndex={tabIndex}
        onToggle={onRepoToggle}
        onExpandToggle={handleExpandToggle}
        onKeyDown={onKeyDown}
      />
      {effectiveExpanded && (
        <div
          id={contentId}
          role="rowgroup"
          aria-label={`${repo.section_id} packages`}
        >
          {children}
        </div>
      )}
    </div>
  );
}
