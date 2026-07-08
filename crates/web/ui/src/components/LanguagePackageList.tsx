import { useCallback, useState } from "react";
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

/** Method value that triggers expandable package sublists. */
const EXPANDABLE_METHOD = "npm global";

export interface LanguagePackageListProps {
  environments: LanguagePackageEnv[];
  /** Toggle callback. Receives { ecosystem, path } matching ItemId::LanguageEnv. */
  onToggle: (ecosystem: string, path: string) => void;
  isPending: boolean;
  /** Item to scroll into view (from global search). Format: "ecosystem:path". */
  revealItemId?: string;
  /** Callback to pin/unpin a single package. */
  onSetPackagePin?: (
    ecosystem: string,
    envPath: string,
    pkg: string,
    pinned: boolean,
  ) => void;
  /** Callback to bulk pin/unpin all packages in an environment. */
  onSetBulkPackagePin?: (
    ecosystem: string,
    envPath: string,
    pinned: boolean,
  ) => void;
}

export function LanguagePackageList({
  environments,
  onToggle,
  isPending,
  revealItemId,
  onSetPackagePin,
  onSetBulkPackagePin,
}: LanguagePackageListProps) {
  const [expanded, setExpanded] = useState<Set<string>>(new Set());

  const handleToggle = useCallback(
    (ecosystem: string, path: string) => {
      if (!isPending) onToggle(ecosystem, path);
    },
    [onToggle, isPending],
  );

  const toggleExpand = useCallback((key: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(key)) {
        next.delete(key);
      } else {
        next.add(key);
      }
      return next;
    });
  }, []);

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
        const isExpandable = env.method === EXPANDABLE_METHOD;
        const isExpanded = expanded.has(itemKey);

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
                  {isExpandable && (
                    <button
                      type="button"
                      className="inspectah-lang-pkg-row__expand-btn"
                      aria-expanded={isExpanded}
                      aria-label={`${isExpanded ? "Collapse" : "Expand"} package list for ${env.path}`}
                      onClick={() => toggleExpand(itemKey)}
                      data-testid={`lang-env-expand-${itemKey}`}
                    >
                      <span
                        className="inspectah-lang-pkg-row__chevron"
                        data-expanded={isExpanded ? "true" : undefined}
                      >
                        &#9654;
                      </span>
                    </button>
                  )}
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
                  {env.has_c_extensions && (
                    <span role="status" aria-label="This environment contains C extensions">
                      <Label color="orange" isCompact>C extensions</Label>
                    </span>
                  )}
                  {env.system_site_packages && (
                    <span role="status" aria-label="This environment uses system site-packages">
                      <Label color="blue" isCompact>system site-packages</Label>
                    </span>
                  )}
                </div>
              </div>
            </div>
            {isExpandable && isExpanded && (
              <div
                className="inspectah-lang-pkg-sublist"
                role="list"
                aria-label={`Packages in ${env.path}`}
                data-testid={`lang-env-sublist-${itemKey}`}
              >
                {onSetBulkPackagePin && env.packages.length > 1 && (
                  <div className="inspectah-lang-pkg-sublist__bulk">
                    <button
                      type="button"
                      className="inspectah-lang-pkg-sublist__bulk-btn"
                      disabled={isPending}
                      onClick={() =>
                        onSetBulkPackagePin(
                          env.ecosystem,
                          env.path,
                          !env.packages.every((p) => p.pinned),
                        )
                      }
                      data-testid={`lang-env-bulk-pin-${itemKey}`}
                    >
                      {env.packages.every((p) => p.pinned)
                        ? "Unpin all"
                        : "Pin all"}
                    </button>
                  </div>
                )}
                {env.packages.map((pkg) => (
                  <div
                    key={pkg.name}
                    role="listitem"
                    className="inspectah-lang-pkg-sublist__item"
                    data-testid={`lang-pkg-${pkg.name}`}
                  >
                    <span className="inspectah-lang-pkg-sublist__name">
                      {pkg.name}
                    </span>
                    <span className="inspectah-lang-pkg-sublist__version">
                      {pkg.detected_version}
                    </span>
                    {onSetPackagePin && (
                      <input
                        type="checkbox"
                        checked={pkg.pinned}
                        disabled={isPending}
                        aria-label={`Pin ${pkg.name}`}
                        onChange={() =>
                          onSetPackagePin(
                            env.ecosystem,
                            env.path,
                            pkg.name,
                            !pkg.pinned,
                          )
                        }
                        data-testid={`lang-pkg-pin-${pkg.name}`}
                      />
                    )}
                  </div>
                ))}
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}
