import { useState, useCallback, useRef } from "react";
import { Switch, Label } from "@patternfly/react-core";
import { AngleRightIcon, AngleDownIcon } from "@patternfly/react-icons";
import type {
  RefinedPackage,
  RefinedConfig,
  AttentionLevel,
  RefinementOp,
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

export function DecisionItem({
  item,
  level,
  rowIndex,
  isViewed,
  isPending,
  tabIndex = 0,
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
    // Expanding a non-toggled item marks viewed
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

  if (isNeedsReview) {
    // Full card layout for NeedsReview items
    return (
      <div
        role="row"
        aria-rowindex={rowIndex}
        aria-label={name}
        tabIndex={tabIndex}
        onKeyDown={handleKeyDown}
        data-testid={`decision-item-${id}`}
        data-expanded={isExpanded ? "true" : "false"}
        style={{
          borderLeft: `3px solid var(--pf-t--global--color--status--danger--default)`,
          padding: "var(--pf-t--global--spacer--sm) var(--pf-t--global--spacer--md)",
          marginBottom: "var(--pf-t--global--spacer--sm)",
          background: "var(--pf-t--global--background--color--secondary--default)",
          borderRadius: "var(--pf-t--global--border--radius--small)",
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: "var(--pf-t--global--spacer--sm)" }}>
          {onToggleInclude && (
            <div role="gridcell" style={{ flexShrink: 0 }}>
              <Switch
                id={`switch-${id}`}
                label={included ? "Include" : "Exclude"}
                isChecked={included}
                onChange={handleToggle}
                isDisabled={isPending}
                aria-label={`Toggle ${name}`}
              />
            </div>
          )}
          <div role="gridcell" style={{ flex: 1, minWidth: 0, display: "flex", alignItems: "center", gap: "var(--pf-t--global--spacer--sm)" }}>
            {showUnviewedDot && (
              <span
                role="img"
                data-testid="unviewed-dot"
                style={{
                  width: 8,
                  height: 8,
                  borderRadius: "50%",
                  background: "var(--pf-t--global--color--status--danger--default)",
                  flexShrink: 0,
                }}
                aria-label="Not yet reviewed"
              />
            )}
            <span style={{ fontWeight: showUnviewedDot ? 600 : 400 }}>{name}</span>
          </div>
          <div role="gridcell" style={{ flexShrink: 0 }}>
            {topReason && (
              <Label color={attentionLabelColor(topAttention)}>
                {formatReasonText(topReason.reason, topReason.detail)}
              </Label>
            )}
          </div>
          <div role="gridcell" style={{ flexShrink: 0 }}>
            <button
              onClick={handleExpand}
              aria-expanded={isExpanded}
              aria-label={`${isExpanded ? "Collapse" : "Expand"} ${name}`}
              tabIndex={-1}
              style={{
                background: "none",
                border: "none",
                cursor: "pointer",
                padding: "4px",
                display: "flex",
                alignItems: "center",
              }}
            >
              {isExpanded ? <AngleDownIcon /> : <AngleRightIcon />}
            </button>
          </div>
        </div>
        {isExpanded && (
          <div role="gridcell" style={{ marginTop: "var(--pf-t--global--spacer--sm)" }}>
            {item.type === "package" ? (
              <PackageDetail pkg={item.data as RefinedPackage} />
            ) : (
              <ConfigDetail config={item.data as RefinedConfig} />
            )}
          </div>
        )}
      </div>
    );
  }

  // Informational (Tier 2) — full card with info-level styling and provenance badge
  if (level === "informational") {
    const badgeText = topReason?.reason === "package_provenance_unavailable"
      ? "Baseline Unavailable"
      : topReason?.reason === "package_user_added" && item.type === "package"
        ? (item.data as RefinedPackage).entry.source_repo || "Unknown"
        : topReason
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
        style={{
          borderLeft: "3px solid var(--pf-t--global--color--status--info--default)",
          padding: "var(--pf-t--global--spacer--sm) var(--pf-t--global--spacer--md)",
          marginBottom: "var(--pf-t--global--spacer--sm)",
          background: "var(--pf-t--global--background--color--secondary--default)",
          borderRadius: "var(--pf-t--global--border--radius--small)",
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: "var(--pf-t--global--spacer--sm)" }}>
          {onToggleInclude && (
            <div role="gridcell" style={{ flexShrink: 0 }}>
              <Switch
                id={`switch-${id}`}
                label={included ? "Include" : "Exclude"}
                isChecked={included}
                onChange={handleToggle}
                isDisabled={isPending}
                aria-label={`Toggle ${name}`}
              />
            </div>
          )}
          <div role="gridcell" style={{ flex: 1, minWidth: 0 }}>
            <span>{name}</span>
          </div>
          <div role="gridcell" style={{ flexShrink: 0 }}>
            {badgeText && (
              <Label color="blue">
                {badgeText}
              </Label>
            )}
          </div>
          <div role="gridcell" style={{ flexShrink: 0 }}>
            <button
              onClick={handleExpand}
              aria-expanded={isExpanded}
              aria-label={`${isExpanded ? "Collapse" : "Expand"} ${name}`}
              tabIndex={-1}
              style={{
                background: "none",
                border: "none",
                cursor: "pointer",
                padding: "4px",
                display: "flex",
                alignItems: "center",
              }}
            >
              {isExpanded ? <AngleDownIcon /> : <AngleRightIcon />}
            </button>
          </div>
        </div>
        {isExpanded && (
          <div role="gridcell" style={{ marginTop: "var(--pf-t--global--spacer--sm)" }}>
            {item.type === "package" ? (
              <PackageDetail pkg={item.data as RefinedPackage} />
            ) : (
              <ConfigDetail config={item.data as RefinedConfig} />
            )}
          </div>
        )}
      </div>
    );
  }

  // Compact row for Routine items
  return (
    <div
      role="row"
      aria-rowindex={rowIndex}
      aria-label={name}
      tabIndex={tabIndex}
      onKeyDown={handleKeyDown}
      data-testid={`decision-item-${id}`}
      data-expanded={isExpanded ? "true" : "false"}
      style={{
        display: "flex",
        flexWrap: "wrap",
        alignItems: "center",
        gap: "var(--pf-t--global--spacer--sm)",
        padding: "var(--pf-t--global--spacer--xs) var(--pf-t--global--spacer--md)",
        borderBottom: "1px solid var(--pf-t--global--border--color--default)",
      }}
    >
      {onToggleInclude && (
        <div role="gridcell" style={{ flexShrink: 0 }}>
          <Switch
            id={`switch-${id}`}
            label={included ? "Include" : "Exclude"}
            isChecked={included}
            onChange={handleToggle}
            isDisabled={isPending}
            aria-label={`Toggle ${name}`}
          />
        </div>
      )}
      <div role="gridcell" style={{ flex: 1, minWidth: 0 }}>
        <span>{name}</span>
      </div>
      <div role="gridcell" style={{ flexShrink: 0 }}>
        <button
          onClick={handleExpand}
          aria-expanded={isExpanded}
          aria-label={`${isExpanded ? "Collapse" : "Expand"} ${name}`}
          tabIndex={-1}
          style={{
            background: "none",
            border: "none",
            cursor: "pointer",
            padding: "4px",
            display: "flex",
            alignItems: "center",
          }}
        >
          {isExpanded ? <AngleDownIcon /> : <AngleRightIcon />}
        </button>
      </div>
      {isExpanded && (
        <div role="gridcell" style={{ flexBasis: "100%", paddingTop: "var(--pf-t--global--spacer--xs)" }}>
          {item.type === "package" ? (
            <PackageDetail pkg={item.data as RefinedPackage} />
          ) : (
            <ConfigDetail config={item.data as RefinedConfig} />
          )}
        </div>
      )}
    </div>
  );
}
