import { useState } from "react";
import { Button } from "@patternfly/react-core";
import { UploadIcon } from "@patternfly/react-icons";
import type { RpmUploadRowState } from "../api/types";

export interface ExcludedPackage {
  name: string;
  repo: string;
  include: boolean;
}

export interface ExcludedZoneProps {
  packages: ExcludedPackage[];
  hasEverToggled: boolean;
  onToggle: (name: string) => void;
  /** Per-package RPM upload row state, mirroring PackageRow's upload gate. */
  rpmRowStates?: Record<string, RpmUploadRowState>;
  /** Callback when the upload icon is clicked on a blocked row. */
  onUploadClick?: (packageName: string) => void;
}

const COLLAPSE_THRESHOLD = 50;

export function ExcludedZone({
  packages,
  hasEverToggled,
  onToggle,
  rpmRowStates,
  onUploadClick,
}: ExcludedZoneProps) {
  const [expanded, setExpanded] = useState(false);

  if (!hasEverToggled) {
    return null;
  }

  if (packages.length === 0) {
    return (
      <div data-testid="excluded-zone">
        <div
          aria-live="polite"
          style={{
            fontSize: "var(--pf-t--global--font--size--body--sm)",
            color: "var(--pf-t--global--text--color--subtle)",
            padding: "var(--pf-t--global--spacer--sm) 0",
            fontWeight: 600,
          }}
        >
          From excluded repos &middot; 0 packages
        </div>
        <p
          style={{
            padding: "0 0 var(--pf-t--global--spacer--md)",
            color: "var(--pf-t--global--text--color--subtle)",
            fontStyle: "italic",
            margin: 0,
          }}
        >
          No packages from excluded repos
        </p>
      </div>
    );
  }

  const collapsed = packages.length >= COLLAPSE_THRESHOLD && !expanded;
  const packageWord = packages.length === 1 ? "package" : "packages";

  return (
    <div data-testid="excluded-zone">
      <div
        aria-live="polite"
        style={{
          fontSize: "var(--pf-t--global--font--size--body--sm)",
          color: "var(--pf-t--global--text--color--subtle)",
          padding: "var(--pf-t--global--spacer--sm) 0",
          fontWeight: 600,
        }}
      >
        From excluded repos &middot; {packages.length} {packageWord}
      </div>

      {packages.length >= COLLAPSE_THRESHOLD && (
        <Button
          variant="link"
          isInline
          aria-expanded={expanded}
          aria-controls="excluded-zone-content"
          onClick={() => setExpanded((prev) => !prev)}
          style={{ fontSize: "var(--pf-t--global--font--size--body--sm)" }}
        >
          {expanded
            ? `Hide ${packages.length} excluded`
            : `Show ${packages.length} excluded`}
        </Button>
      )}
      <div
        id="excluded-zone-content"
        hidden={collapsed}
        style={{
          display: collapsed ? "none" : "flex",
          flexWrap: "wrap",
          gap: "var(--pf-t--global--spacer--xs) var(--pf-t--global--spacer--sm)",
        }}
      >
        {packages.map((pkg) => {
          const isBlocked = rpmRowStates?.[pkg.name] === "needs_upload";
          // A <label> implicitly associates with a labelable descendant.
          // That's the point for the checkbox branch (clicking the name
          // toggles it), but `button` is also labelable, so wrapping the
          // blocked branch's Upload button in the same <label> would make
          // clicking the package name activate the file picker. Use a plain
          // <span> container there instead.
          const Container = isBlocked ? "span" : "label";
          return (
            <Container
              key={`${pkg.repo}/${pkg.name}`}
              style={{
                display: "inline-flex",
                alignItems: "center",
                gap: "var(--pf-t--global--spacer--xs)",
              }}
            >
              {isBlocked ? (
                <Button
                  variant="plain"
                  aria-label={`Upload RPM for ${pkg.name}`}
                  onClick={(e) => {
                    e.stopPropagation();
                    onUploadClick?.(pkg.name);
                  }}
                  icon={<UploadIcon />}
                />
              ) : (
                <input
                  type="checkbox"
                  role="checkbox"
                  checked={pkg.include}
                  aria-label={pkg.name}
                  onChange={() => onToggle(pkg.name)}
                />
              )}
              <span
                style={{
                  textDecoration: pkg.include ? "none" : "line-through",
                  color: "var(--pf-t--global--text--color--subtle)",
                  fontSize: "var(--pf-t--global--font--size--body--sm)",
                }}
              >
                {pkg.name}
              </span>
            </Container>
          );
        })}
      </div>
    </div>
  );
}
