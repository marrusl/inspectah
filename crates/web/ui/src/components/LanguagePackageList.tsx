import { useCallback } from "react";
import { Badge, Label, Content } from "@patternfly/react-core";
import type { LanguagePackageEnv } from "../api/types";

/** Confidence-to-PatternFly Label color mapping per Plan 1 gate. */
const CONFIDENCE_COLOR: Record<string, "green" | "orange" | "grey"> = {
  high: "green",
  medium: "orange",
  low: "grey",
};

/** Human-readable manifest basis labels. */
const MANIFEST_LABELS: Record<string, string> = {
  "requirements.txt": "from requirements.txt",
  "dist-info": "from dist-info",
  "package-lock.json": "from package-lock.json",
  "Gemfile.lock": "from Gemfile.lock",
};

export interface LanguagePackageListProps {
  environments: LanguagePackageEnv[];
  /** Toggle callback. Receives { ecosystem, path } matching ItemId::LanguageEnv. */
  onToggle: (ecosystem: string, path: string) => void;
  isPending: boolean;
  /** Item to scroll into view (from global search). Format: "ecosystem:path". */
  revealItemId?: string;
}

export function LanguagePackageList({
  environments,
  onToggle,
  isPending,
  revealItemId,
}: LanguagePackageListProps) {
  const handleToggle = useCallback(
    (ecosystem: string, path: string) => {
      if (!isPending) onToggle(ecosystem, path);
    },
    [onToggle, isPending],
  );

  if (environments.length === 0) {
    return (
      <Content component="p" style={{ color: "var(--pf-t--global--text--color--subtle)" }}>
        None detected.
      </Content>
    );
  }

  return (
    <div
      className="inspectah-lang-pkg-list"
      role="list"
      aria-label="Language package environments"
      data-testid="language-package-list"
    >
      {environments.map((env) => {
        const itemKey = `${env.ecosystem}:${env.path}`;
        return (
          <div
            key={itemKey}
            role="listitem"
            tabIndex={-1}
            data-testid={`lang-env-row-${itemKey}`}
            className="inspectah-lang-pkg-row"
            data-revealed={revealItemId === itemKey ? "true" : undefined}
          >
            <div className="inspectah-lang-pkg-row__main">
              <div className="inspectah-lang-pkg-row__toggle">
                <input
                  type="checkbox"
                  role="checkbox"
                  checked={env.include}
                  disabled={isPending}
                  aria-label={`Toggle ${env.ecosystem} environment at ${env.path}`}
                  onChange={() => handleToggle(env.ecosystem, env.path)}
                />
              </div>
              <div className="inspectah-lang-pkg-row__info">
                <div className="inspectah-lang-pkg-row__header">
                  <Label
                    className="inspectah-lang-pkg-row__ecosystem"
                    isCompact
                  >
                    {env.ecosystem}
                  </Label>
                  <span className="inspectah-lang-pkg-row__path">
                    {env.path}
                  </span>
                </div>
                <div className="inspectah-lang-pkg-row__meta">
                  <Badge isRead>
                    {env.packages.length} package
                    {env.packages.length !== 1 ? "s" : ""}
                  </Badge>
                  <Label
                    color={CONFIDENCE_COLOR[env.confidence] ?? "grey"}
                    isCompact
                  >
                    {env.confidence}
                  </Label>
                  <span className="inspectah-lang-pkg-row__basis">
                    {MANIFEST_LABELS[env.manifest_basis] ?? env.manifest_basis}
                  </span>
                </div>
              </div>
            </div>
          </div>
        );
      })}
    </div>
  );
}
