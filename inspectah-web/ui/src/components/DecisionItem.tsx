import { useState, useCallback, useRef } from "react";
import { Label } from "@patternfly/react-core";
import { AngleRightIcon, AngleDownIcon } from "@patternfly/react-icons";
import type {
  RefinedPackage,
  RefinedConfig,
  AttentionLevel,
  RefinementOp,
  VersionChangeEntry,
} from "../api/types";
import {
  attentionLabelColor,
  formatReasonText,
  highestAttention,
} from "./attentionUtils";
import { PackageDetail } from "./PackageDetail";
import { ConfigDetail } from "./ConfigDetail";

export type DecisionItemKind =
  | { type: "package"; data: RefinedPackage }
  | { type: "config"; data: RefinedConfig };

export interface DecisionItemProps {
  item: DecisionItemKind;
  level: AttentionLevel;
  rowIndex: number;
  isViewed: boolean;
  isPending: boolean;
  tabIndex?: number;
  leafDepTree?: Record<string, string[]>;
  versionChanges?: VersionChangeEntry[];
  onToggleInclude?: (op: RefinementOp) => void;
  onMarkViewed: (id: string) => void;
  onKeyDown?: (e: React.KeyboardEvent) => void;
}

export function itemId(item: DecisionItemKind): string {
  if (item.type === "package") {
    return `packages:${item.data.entry.name}.${item.data.entry.arch}`;
  }
  return `configs:${item.data.entry.path}`;
}

function itemName(item: DecisionItemKind): string {
  if (item.type === "package") {
    return `${item.data.entry.name}.${item.data.entry.arch}`;
  }
  return item.data.entry.path;
}

function isIncluded(item: DecisionItemKind): boolean {
  return item.data.entry.include;
}

function buildToggleOp(item: DecisionItemKind): RefinementOp {
  if (item.type === "package") {
    const { name, arch } = item.data.entry;
    return item.data.entry.include
      ? { op: "ExcludePackage", target: { name, arch } }
      : { op: "IncludePackage", target: { name, arch } };
  }
  const { path } = item.data.entry;
  return item.data.entry.include
    ? { op: "ExcludeConfig", target: { path } }
    : { op: "IncludeConfig", target: { path } };
}

const LEVEL_BORDER: Record<string, string> = {
  needs_review: "3px solid var(--pf-t--global--color--status--danger--default)",
  informational: "3px solid var(--pf-t--global--color--status--info--default)",
  routine: "none",
};

export function DecisionItem({
  item,
  level,
  rowIndex,
  isViewed,
  isPending,
  tabIndex = 0,
  leafDepTree,
  versionChanges,
  onToggleInclude,
  onMarkViewed,
  onKeyDown: onKeyDownProp,
}: DecisionItemProps) {
  const [isExpanded, setIsExpanded] = useState(false);
  const id = itemId(item);
  const name = itemName(item);
  const included = isIncluded(item);
  const hasBeenToggled = useRef(false);
  const isNeedsReview = level === "needs_review";
  const showUnviewedDot = isNeedsReview && !isViewed;

  const matchingVc = item.type === "package" && versionChanges
    ? versionChanges.find(
        (vc) => vc.name === item.data.entry.name && vc.arch === item.data.entry.arch,
      ) ?? null
    : null;

  const handleToggle = useCallback(() => {
    if (!onToggleInclude) return;
    const op = buildToggleOp(item);
    onToggleInclude(op);
    onMarkViewed(id);
    hasBeenToggled.current = true;
  }, [item, onToggleInclude, onMarkViewed, id]);

  const handleExpand = useCallback(() => {
    const willExpand = !isExpanded;
    setIsExpanded(willExpand);
    if (willExpand && !hasBeenToggled.current) {
      onMarkViewed(id);
    }
  }, [isExpanded, onMarkViewed, id]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.target !== e.currentTarget) return;
      if (e.key === " " || e.key === "x") {
        e.preventDefault();
        handleToggle();
      } else if (e.key === "Enter") {
        e.preventDefault();
        handleExpand();
      } else if (onKeyDownProp) {
        onKeyDownProp(e);
      }
    },
    [handleToggle, handleExpand, onKeyDownProp],
  );

  const topAttention = item.data.attention.length > 0
    ? highestAttention(item.data.attention)
    : level;
  const topReason = item.data.attention[0];

  const borderLeft = LEVEL_BORDER[level] ?? LEVEL_BORDER.routine;

  const badgeText = level === "informational"
    ? (topReason?.reason === "package_provenance_unavailable"
      ? "Baseline Unavailable"
      : topReason?.reason === "package_user_added" && item.type === "package"
        ? (item.data as RefinedPackage).entry.source_repo || "Unknown"
        : topReason
          ? formatReasonText(topReason.reason, topReason.detail)
          : null)
    : level === "needs_review" && topReason
      ? formatReasonText(topReason.reason, topReason.detail)
      : null;

  return (
    <div
      role="row"
      aria-rowindex={rowIndex}
      aria-label={name}
      tabIndex={tabIndex}
      onKeyDown={handleKeyDown}
      data-testid={`decision-item-${id}`}
      data-expanded={isExpanded ? "true" : "false"}
      className="inspectah-decision-row"
      style={{ borderLeft }}
    >
      <div className="inspectah-decision-row__main">
        {onToggleInclude && (
          <div role="gridcell" className="inspectah-decision-row__toggle">
            <input
              type="checkbox"
              role="checkbox"
              id={`switch-${id}`}
              checked={included}
              onChange={handleToggle}
              disabled={isPending}
              aria-label={`Toggle ${name}`}
            />
          </div>
        )}
        <div role="gridcell" className="inspectah-decision-row__name">
          {showUnviewedDot && (
            <span
              role="img"
              data-testid="unviewed-dot"
              className="inspectah-decision-row__dot"
              aria-label="Not yet reviewed"
            />
          )}
          <span style={{ fontWeight: showUnviewedDot ? 600 : 400 }}>{name}</span>
        </div>
        {badgeText && (
          <div role="gridcell" className="inspectah-decision-row__badge">
            <Label color={attentionLabelColor(topAttention)}>
              {badgeText}
            </Label>
          </div>
        )}
        <div role="gridcell" className="inspectah-decision-row__expand">
          <button
            onClick={handleExpand}
            aria-expanded={isExpanded}
            aria-label={`${isExpanded ? "Collapse" : "Expand"} ${name}`}
            tabIndex={-1}
            className="inspectah-decision-row__expand-btn"
          >
            {isExpanded ? <AngleDownIcon /> : <AngleRightIcon />}
          </button>
        </div>
      </div>
      {isExpanded && (
        <div role="gridcell" className="inspectah-decision-row__detail">
          {item.type === "package" ? (
            <PackageDetail pkg={item.data as RefinedPackage} leafDepTree={leafDepTree} versionChange={matchingVc} />
          ) : (
            <ConfigDetail config={item.data as RefinedConfig} />
          )}
        </div>
      )}
    </div>
  );
}
