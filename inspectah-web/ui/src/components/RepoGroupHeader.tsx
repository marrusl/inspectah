import { useCallback } from "react";
import { Switch, Label } from "@patternfly/react-core";
import type { RepoProvenance } from "../api/types";

export interface RepoGroupHeaderProps {
  sectionId: string;
  provenance: RepoProvenance;
  isDistro: boolean;
  packageCount: number;
  enabled: boolean;
  onToggle?: (sectionId: string, enabled: boolean) => void;
}

function badgeLabel(isDistro: boolean, provenance: RepoProvenance): string {
  if (isDistro) return "Distro";
  if (provenance === "verified") return "Third-party";
  if (provenance === "incomplete") return "Unverified";
  return "Unknown";
}

function badgeAbbrev(isDistro: boolean, provenance: RepoProvenance): string {
  if (isDistro) return "D";
  if (provenance === "verified") return "3P";
  if (provenance === "incomplete") return "U";
  return "?";
}

function badgeColor(isDistro: boolean, provenance: RepoProvenance): "blue" | "orange" | "grey" | "green" {
  if (isDistro) return "green";
  if (provenance === "verified") return "blue";
  if (provenance === "incomplete") return "orange";
  return "grey";
}

const showToggle = (isDistro: boolean, provenance: RepoProvenance): boolean =>
  !isDistro && provenance === "verified";

export function RepoGroupHeader({
  sectionId,
  provenance,
  isDistro,
  packageCount,
  enabled,
  onToggle,
}: RepoGroupHeaderProps) {
  const fullLabel = badgeLabel(isDistro, provenance);
  const abbrevLabel = badgeAbbrev(isDistro, provenance);
  const color = badgeColor(isDistro, provenance);
  const canToggle = showToggle(isDistro, provenance);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLDivElement>) => {
      if (canToggle && (e.key === "Enter" || e.key === " ")) {
        e.preventDefault();
        onToggle?.(sectionId, !enabled);
      }
    },
    [canToggle, onToggle, sectionId, enabled],
  );

  return (
    <div
      data-testid={`repo-group-${sectionId}`}
      role="heading"
      aria-level={3}
      tabIndex={0}
      onKeyDown={handleKeyDown}
      className="inspectah-repo-group-header"
    >
      <span className="inspectah-repo-group-header__label">
        {sectionId}
      </span>
      {/* Full badge: visible at >= 768px, hidden at narrow */}
      <span className="inspectah-repo-group-header__badge-full">
        <Label color={color} aria-label={fullLabel}>{fullLabel}</Label>
      </span>
      {/* Abbreviated badge: hidden at >= 768px, visible at narrow */}
      <span className="inspectah-repo-group-header__badge-abbrev">
        <Label color={color} aria-label={fullLabel}>{abbrevLabel}</Label>
      </span>
      <span className="inspectah-repo-group-header__count">
        {packageCount} {packageCount === 1 ? "package" : "packages"}
      </span>
      {canToggle && (
        <span className="inspectah-repo-group-header__toggle">
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
