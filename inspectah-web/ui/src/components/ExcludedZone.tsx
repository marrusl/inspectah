import { useState } from "react";
import { Button } from "@patternfly/react-core";

export interface ExcludedPackage {
  name: string;
  repo: string;
}

export interface ExcludedZoneProps {
  packages: ExcludedPackage[];
  hasEverToggled: boolean;
}

const COLLAPSE_THRESHOLD = 50;

export function ExcludedZone({ packages, hasEverToggled }: ExcludedZoneProps) {
  const [expanded, setExpanded] = useState(false);

  if (!hasEverToggled) {
    return null;
  }

  if (packages.length === 0) {
    return (
      <div
        style={{
          padding: "var(--pf-t--global--spacer--md)",
          color: "var(--pf-t--global--text--color--subtle)",
          fontStyle: "italic",
        }}
      >
        No excluded packages
      </div>
    );
  }

  const collapsed = packages.length >= COLLAPSE_THRESHOLD && !expanded;

  return (
    <div
      data-testid="excluded-zone"
      style={{ opacity: 0.55 }}
    >
      <div
        aria-live="polite"
        style={{
          fontSize: "var(--pf-t--global--font--size--body--sm)",
          color: "var(--pf-t--global--text--color--subtle)",
          padding: "var(--pf-t--global--spacer--sm) 0",
          fontWeight: 600,
        }}
      >
        Excluded &middot; {packages.length} packages
      </div>

      {collapsed ? (
        <Button
          variant="link"
          isInline
          aria-expanded={false}
          aria-controls="excluded-zone-content"
          onClick={() => setExpanded(true)}
          style={{ fontSize: "var(--pf-t--global--font--size--body--sm)" }}
        >
          Show {packages.length} excluded
        </Button>
      ) : null}
      <div
        id="excluded-zone-content"
        hidden={collapsed}
        style={{
          display: collapsed ? "none" : "flex",
          flexWrap: "wrap",
          gap: "var(--pf-t--global--spacer--xs) var(--pf-t--global--spacer--sm)",
        }}
      >
        {packages.map((pkg) => (
          <span
            key={`${pkg.repo}/${pkg.name}`}
            style={{
              textDecoration: "line-through",
              color: "var(--pf-t--global--text--color--subtle)",
              fontSize: "var(--pf-t--global--font--size--body--sm)",
            }}
          >
            {pkg.name}
          </span>
        ))}
      </div>
    </div>
  );
}
