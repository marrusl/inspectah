import { useState, useCallback, useRef } from "react";
import { Label } from "@patternfly/react-core";
import { AngleRightIcon, AngleDownIcon } from "@patternfly/react-icons";
import type {
  RefinedPackage,
  RefinedConfig,
  AttentionLevel,
  RefinementOp,
  VersionChangeEntry,
  ItemId,
  TriageTag,
} from "../api/types";
import {
  attentionLabelColor,
  extractTriageBucket,
  formatReasonText,
  formatTriageBucket,
  formatTriageReason,
  triageBucketLabelColor,
  triageBucketToAttention,
} from "./attentionUtils";
import { PackageDetail } from "./PackageDetail";
import { ConfigDetail } from "./ConfigDetail";

export type DecisionItemKind =
  | { type: "package"; data: RefinedPackage }
  | { type: "config"; data: RefinedConfig };

export interface DecisionItemProps {
  item: DecisionItemKind;
  /** @deprecated Use triageTag instead. Kept for backward compat during migration. */
  level?: AttentionLevel;
  triageTag?: TriageTag;
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

function buildItemId(item: DecisionItemKind): ItemId {
  if (item.type === "package") {
    return {
      kind: "Package",
      key: { name: item.data.entry.name, arch: item.data.entry.arch },
    };
  }
  return { kind: "Config", key: { path: item.data.entry.path } };
}

function buildToggleOp(item: DecisionItemKind): RefinementOp {
  const itemId = buildItemId(item);
  return {
    op: "SetInclude",
    target: { item_id: itemId, include: !item.data.entry.include },
  };
}

const LEVEL_BORDER: Record<string, string> = {
  needs_review: "3px solid var(--pf-t--global--color--status--danger--default)",
  informational: "3px solid var(--pf-t--global--color--status--info--default)",
  routine: "none",
  investigate: "3px solid var(--pf-t--global--color--status--danger--default)",
  divergent: "3px solid var(--pf-t--global--color--status--warning--default)",
  site: "none",
  baseline: "none",
  partial: "3px solid var(--pf-t--global--color--status--custom--default)",
  universal: "none",
};

export function DecisionItem({
  item,
  level,
  triageTag,
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

  // Derive effective attention level: triageTag takes priority over legacy level prop
  const effectiveLevel: AttentionLevel = triageTag
    ? triageBucketToAttention(triageTag)
    : (level ?? "routine");

  const isNeedsReview = effectiveLevel === "needs_review";
  const showUnviewedDot = isNeedsReview && !isViewed;

  const matchingVc =
    item.type === "package" && versionChanges
      ? (versionChanges.find(
          (vc) =>
            vc.name === item.data.entry.name &&
            vc.arch === item.data.entry.arch,
        ) ?? null)
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

  // Derive border and badge from triage or legacy attention
  const topAttention = item.data.triage
    ? triageBucketToAttention(item.data.triage)
    : effectiveLevel;
  const topReason = item.data.attention?.[0];

  // Use triage bucket for border when available, otherwise legacy level
  const triageBucketKey = triageTag
    ? triageTag.triage.mode === "single_host"
      ? (Object.keys(triageTag.triage).find((k) => k !== "mode") ?? "baseline")
      : triageTag.triage.bucket
    : null;
  const borderLeft = triageBucketKey
    ? (LEVEL_BORDER[triageBucketKey] ?? "none")
    : (LEVEL_BORDER[effectiveLevel] ?? LEVEL_BORDER.routine);

  // Triage bucket badge for package rows (repo-first exception: badge, not bucket grouping)
  const bucketBadge = (() => {
    if (item.type !== "package" || !triageTag) return null;
    const bucket = extractTriageBucket(triageTag);
    // Skip badge for baseline — it's the default, no signal value
    if (bucket === "baseline" || bucket === "universal") return null;
    return {
      text: formatTriageBucket(bucket),
      color: triageBucketLabelColor(bucket),
    };
  })();

  // Badge text from triage primary_reason or legacy attention
  const badgeText = (() => {
    if (triageTag) {
      const reason = triageTag.primary_reason;
      if (typeof reason === "object" && "custom" in reason)
        return reason.custom;
      if (reason === "package_provenance_unavailable")
        return "Baseline Unavailable";
      if (reason === "package_user_added" && item.type === "package") {
        return (item.data as RefinedPackage).entry.source_repo || "Unknown";
      }
      if (effectiveLevel === "routine") return null;
      return formatTriageReason(reason);
    }
    // Legacy path
    if (effectiveLevel === "informational") {
      if (topReason?.reason === "package_provenance_unavailable")
        return "Baseline Unavailable";
      if (
        topReason?.reason === "package_user_added" &&
        item.type === "package"
      ) {
        return (item.data as RefinedPackage).entry.source_repo || "Unknown";
      }
      return topReason
        ? formatReasonText(topReason.reason, topReason.detail)
        : null;
    }
    if (effectiveLevel === "needs_review" && topReason) {
      return formatReasonText(topReason.reason, topReason.detail);
    }
    return null;
  })();

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
      <div
        className="inspectah-decision-row__main"
        onClick={handleExpand}
        style={{ cursor: "pointer" }}
      >
        {onToggleInclude && (
          <div role="gridcell" className="inspectah-decision-row__toggle">
            <input
              type="checkbox"
              role="checkbox"
              id={`switch-${id}`}
              checked={included}
              onChange={handleToggle}
              onClick={(e) => e.stopPropagation()}
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
          <span style={{ fontWeight: showUnviewedDot ? 600 : 400 }}>
            {name}
          </span>
        </div>
        {bucketBadge && (
          <div
            role="gridcell"
            className="inspectah-decision-row__badge"
            data-testid="triage-bucket-badge"
          >
            <Label color={bucketBadge.color} isCompact>
              {bucketBadge.text}
            </Label>
          </div>
        )}
        {badgeText && (
          <div role="gridcell" className="inspectah-decision-row__badge">
            <Label color={attentionLabelColor(topAttention)}>{badgeText}</Label>
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
            <PackageDetail
              pkg={item.data as RefinedPackage}
              leafDepTree={leafDepTree}
              versionChange={matchingVc}
            />
          ) : (
            <ConfigDetail config={item.data as RefinedConfig} />
          )}
        </div>
      )}
    </div>
  );
}
