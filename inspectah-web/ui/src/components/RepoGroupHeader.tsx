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
  const label = badgeLabel(isDistro, provenance);
  const color = badgeColor(isDistro, provenance);
  const canToggle = showToggle(isDistro, provenance);

  return (
    <div
      data-testid={`repo-group-${sectionId}`}
      style={{
        display: "flex",
        alignItems: "center",
        gap: "var(--pf-t--global--spacer--sm)",
        padding: "var(--pf-t--global--spacer--xs) 0",
        marginTop: "var(--pf-t--global--spacer--sm)",
        marginBottom: "var(--pf-t--global--spacer--xs)",
        borderBottom: "1px solid var(--pf-t--global--border--color--default)",
      }}
    >
      <span
        style={{
          fontWeight: 600,
          fontSize: "var(--pf-t--global--font--size--body--default)",
        }}
      >
        {sectionId}
      </span>
      <Label color={color}>{label}</Label>
      <span
        style={{
          color: "var(--pf-t--global--text--color--subtle)",
          fontSize: "var(--pf-t--global--font--size--body--sm)",
        }}
      >
        {packageCount} {packageCount === 1 ? "package" : "packages"}
      </span>
      {canToggle && (
        <Switch
          id={`repo-toggle-${sectionId}`}
          label={enabled ? "Enabled" : "Disabled"}
          isChecked={enabled}
          onChange={() => onToggle?.(sectionId, !enabled)}
          aria-label={`Toggle ${sectionId} repo`}
        />
      )}
    </div>
  );
}
