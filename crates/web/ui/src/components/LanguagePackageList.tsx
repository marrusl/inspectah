import { useCallback, useEffect, useRef, useState } from "react";
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
  /** When non-empty, auto-expands environments with matching packages and highlights matches. */
  searchQuery?: string;
}

export function LanguagePackageList({
  environments,
  onToggle,
  isPending,
  revealItemId,
  onSetPackagePin,
  onSetBulkPackagePin,
  searchQuery,
}: LanguagePackageListProps) {
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [announcement, setAnnouncement] = useState("");
  const expandBtnRefs = useRef<Map<string, HTMLButtonElement>>(new Map());
  const envRowRefs = useRef<Map<string, HTMLDivElement>>(new Map());

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

  /** Auto-expand environments whose packages match searchQuery. */
  useEffect(() => {
    if (!searchQuery?.trim()) return;
    const q = searchQuery.toLowerCase();
    const keysToExpand: string[] = [];
    for (const env of environments) {
      if (env.method !== EXPANDABLE_METHOD) continue;
      if (env.packages.some((p) => p.name.toLowerCase().includes(q))) {
        keysToExpand.push(`${env.ecosystem}:${env.path}`);
      }
    }
    if (keysToExpand.length > 0) {
      setExpanded((prev) => {
        const next = new Set(prev);
        for (const k of keysToExpand) next.add(k);
        return next;
      });
    }
  }, [searchQuery, environments]);

  /** Scroll first matching package into view after search-driven expansion. */
  useEffect(() => {
    if (!searchQuery?.trim()) return;
    // Wait for expansion state change to render the sublist DOM
    requestAnimationFrame(() => {
      const match = document.querySelector(
        '[data-search-match="true"]',
      ) as HTMLElement | null;
      if (match) {
        match.scrollIntoView({ behavior: "smooth", block: "nearest" });
        match.focus();
      }
    });
  }, [searchQuery, environments]);

  /** Keyboard handler for expanded sublist: ArrowUp/Down navigate, Escape collapses. */
  const handleSublistKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLDivElement>, itemKey: string) => {
      const sublist = e.currentTarget;
      const items = Array.from(
        sublist.querySelectorAll<HTMLElement>('[role="listitem"]'),
      );
      const active = document.activeElement?.closest(
        '[role="listitem"]',
      ) as HTMLElement | null;
      const currentIndex = active ? items.indexOf(active) : -1;

      if (e.key === "ArrowDown") {
        e.preventDefault();
        const next = Math.min(currentIndex + 1, items.length - 1);
        items[next]?.focus();
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        const prev = Math.max(currentIndex - 1, 0);
        items[prev]?.focus();
      } else if (e.key === "Escape") {
        e.preventDefault();
        toggleExpand(itemKey);
        envRowRefs.current.get(itemKey)?.focus();
      }
    },
    [toggleExpand],
  );

  /** Wrap bulk pin to fire aria-live announcement. */
  const handleBulkPin = useCallback(
    (
      ecosystem: string,
      envPath: string,
      pinned: boolean,
      pkgCount: number,
    ) => {
      onSetBulkPackagePin?.(ecosystem, envPath, pinned);
      const action = pinned ? "Pinned" : "Unpinned";
      setAnnouncement(
        `${action} ${pkgCount} packages in ${ecosystem} globals`,
      );
    },
    [onSetBulkPackagePin],
  );

  if (environments.length === 0) {
    return (
      <Content component="p" style={{ color: "var(--pf-t--global--text--color--subtle)" }}>
        None detected.
      </Content>
    );
  }

  const queryLower = searchQuery?.toLowerCase() ?? "";

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
            tabIndex={isExpandable ? 0 : -1}
            aria-expanded={isExpandable ? isExpanded : undefined}
            ref={(el) => {
              if (el) envRowRefs.current.set(itemKey, el);
            }}
            data-testid={`lang-env-row-${itemKey}`}
            className="inspectah-lang-pkg-row"
            data-revealed={revealItemId === itemKey ? "true" : undefined}
            onKeyDown={
              isExpandable
                ? (e: React.KeyboardEvent) => {
                    if (
                      e.key === "Enter" &&
                      e.target === e.currentTarget
                    ) {
                      e.preventDefault();
                      toggleExpand(itemKey);
                    }
                  }
                : undefined
            }
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
                      ref={(el) => {
                        if (el) expandBtnRefs.current.set(itemKey, el);
                      }}
                      className="inspectah-lang-pkg-row__expand-btn"
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
                onKeyDown={(e) => handleSublistKeyDown(e, itemKey)}
              >
                {onSetBulkPackagePin && env.packages.length >= 1 && (
                  <div className="inspectah-lang-pkg-sublist__bulk">
                    <button
                      type="button"
                      className="inspectah-lang-pkg-sublist__bulk-btn"
                      disabled={isPending}
                      aria-label={`${env.packages.every((p) => p.pinned) ? "Unpin" : "Pin"} all packages in ${env.ecosystem} globals ${env.path}`}
                      onClick={() => {
                        const allPinned = env.packages.every(
                          (p) => p.pinned,
                        );
                        handleBulkPin(
                          env.ecosystem,
                          env.path,
                          !allPinned,
                          env.packages.length,
                        );
                      }}
                      data-testid={`lang-env-bulk-pin-${itemKey}`}
                    >
                      {env.packages.every((p) => p.pinned)
                        ? "Unpin all"
                        : "Pin all"}
                    </button>
                  </div>
                )}
                {env.packages.map((pkg) => {
                  const isSearchMatch =
                    queryLower !== "" &&
                    pkg.name.toLowerCase().includes(queryLower);
                  return (
                    <div
                      key={pkg.name}
                      role="listitem"
                      tabIndex={0}
                      className={`inspectah-lang-pkg-sublist__item${isSearchMatch ? " inspectah-lang-pkg-sublist__item--search-match" : ""}`}
                      data-testid={`lang-pkg-${pkg.name}`}
                      data-search-match={
                        isSearchMatch ? "true" : undefined
                      }
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
                          aria-label={`Pin ${pkg.name} to version ${pkg.detected_version}`}
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
                  );
                })}
              </div>
            )}
          </div>
        );
      })}
      <div
        aria-live="polite"
        aria-atomic="true"
        style={{
          position: "absolute",
          width: "1px",
          height: "1px",
          padding: 0,
          margin: "-1px",
          overflow: "hidden",
          clip: "rect(0, 0, 0, 0)",
          whiteSpace: "nowrap",
          border: 0,
        }}
        data-testid="lang-pkg-announcement"
      >
        {announcement}
      </div>
    </div>
  );
}
