import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { SortHeader } from "./SortHeader";
import { ExcludedZone } from "./ExcludedZone";
import { RepoConflictPopover } from "./fleet/RepoConflictPopover";
import type { RepoGroupInfo, RepoTier } from "../api/types";

// --- Types ---

export interface PackageListPackage {
  name: string;
  source_repo: string;
  include: boolean;
  prevalence?: { count: number; total: number };
  repo_conflict?: { repo: string; host_count: number }[];
}

export interface PackageListProps {
  mode: "single" | "fleet";
  packages: PackageListPackage[];
  repoGroups: RepoGroupInfo[];
  onToggle: (packageName: string) => void;
  onRepoToggle: (sectionId: string) => void;
  onDismissedCountChange?: (count: number) => void;
  /** When toggled from false to true, clears all dismissed conflicts. */
  onRestoreDismissed?: boolean;
}

// --- Tier ordering for sort ---

const TIER_ORDER: Record<RepoTier, number> = {
  distro: 0,
  official_optional: 1,
  third_party: 2,
};

// --- Repo text styling per tier ---

const repoStyles: Record<string, React.CSSProperties> = {
  distro: {
    color: "var(--pf-t--global--text--color--subtle)",
  },
  official_optional: {
    color: "var(--pf-t--global--color--status--success--default)",
    textDecoration: "underline",
    textDecorationStyle: "dotted" as const,
    textUnderlineOffset: "3px",
  },
  third_party: {
    color: "var(--pf-t--global--color--status--warning--default)",
    textDecoration: "underline",
    textDecorationStyle: "solid" as const,
    textUnderlineOffset: "3px",
  },
};

// --- Component ---

export function PackageList({
  mode,
  packages,
  repoGroups,
  onToggle,
  onRepoToggle: _onRepoToggle,
  onDismissedCountChange,
  onRestoreDismissed,
}: PackageListProps) {
  // Sort state: defaults differ by mode
  const [activeColumn, setActiveColumn] = useState<"left" | "right">(
    mode === "fleet" ? "right" : "left",
  );
  const [direction, setDirection] = useState<"asc" | "desc">("asc");

  // Dismissed conflicts state (owned by PackageList)
  const [dismissedConflicts, setDismissedConflicts] = useState<Set<string>>(
    () => new Set(),
  );

  // Latched: once any non-distro repo is disabled, stays true for the session
  const [hasEverToggled, setHasEverToggled] = useState(false);
  useEffect(() => {
    if (repoGroups.some((r) => !r.is_distro && !r.enabled)) {
      setHasEverToggled(true);
    }
  }, [repoGroups]);

  // Report dismissed count to parent
  useEffect(() => {
    onDismissedCountChange?.(dismissedConflicts.size);
  }, [dismissedConflicts.size, onDismissedCountChange]);

  // Handle restore dismissed from parent
  useEffect(() => {
    if (onRestoreDismissed) {
      setDismissedConflicts(new Set());
    }
  }, [onRestoreDismissed]);

  // Build repo tier lookup
  const repoTierMap = useMemo(() => {
    const map = new Map<string, RepoTier>();
    for (const rg of repoGroups) {
      map.set(rg.section_id, rg.tier);
    }
    return map;
  }, [repoGroups]);

  // Build disabled repo set
  const disabledRepos = useMemo(() => {
    const set = new Set<string>();
    for (const rg of repoGroups) {
      if (!rg.enabled) set.add(rg.section_id);
    }
    return set;
  }, [repoGroups]);

  // Split packages: included vs excluded
  const { includedPackages, excludedPackages } = useMemo(() => {
    const included: PackageListPackage[] = [];
    const excluded: { name: string; repo: string }[] = [];
    for (const pkg of packages) {
      if (disabledRepos.has(pkg.source_repo)) {
        excluded.push({ name: pkg.name, repo: pkg.source_repo });
      } else {
        included.push(pkg);
      }
    }
    return { includedPackages: included, excludedPackages: excluded };
  }, [packages, disabledRepos]);

  // Sort logic
  const sortedPackages = useMemo(() => {
    const sorted = [...includedPackages];

    if (mode === "single") {
      if (activeColumn === "left") {
        // Alphabetical by name
        sorted.sort((a, b) => {
          const cmp = a.name.localeCompare(b.name);
          return direction === "asc" ? cmp : -cmp;
        });
      } else {
        // Tier-first sort: distro < official_optional < third_party, then alpha within tier
        sorted.sort((a, b) => {
          const tierA = TIER_ORDER[repoTierMap.get(a.source_repo) ?? "distro"];
          const tierB = TIER_ORDER[repoTierMap.get(b.source_repo) ?? "distro"];
          if (tierA !== tierB) {
            return direction === "asc" ? tierA - tierB : tierB - tierA;
          }
          return a.name.localeCompare(b.name);
        });
      }
    } else {
      // Fleet mode
      if (activeColumn === "right" || activeColumn === "left") {
        // Prevalence ascending (rarest first) is the default for fleet
        sorted.sort((a, b) => {
          const prevA = a.prevalence?.count ?? 0;
          const prevB = b.prevalence?.count ?? 0;

          if (activeColumn === "right") {
            // Sort by prevalence
            if (prevA !== prevB) {
              return direction === "asc" ? prevA - prevB : prevB - prevA;
            }
            // Within same prevalence: conflicts first (if not dismissed)
            const conflictA = hasUndismissedConflict(a, dismissedConflicts);
            const conflictB = hasUndismissedConflict(b, dismissedConflicts);
            if (conflictA !== conflictB) {
              return conflictA ? -1 : 1;
            }
            return a.name.localeCompare(b.name);
          } else {
            // Left column = alphabetical
            const cmp = a.name.localeCompare(b.name);
            return direction === "asc" ? cmp : -cmp;
          }
        });
      }
    }

    return sorted;
  }, [includedPackages, mode, activeColumn, direction, repoTierMap, dismissedConflicts]);

  // Sort handler
  const handleSort = useCallback(
    (column: "left" | "right") => {
      if (column === activeColumn) {
        setDirection((d) => (d === "asc" ? "desc" : "asc"));
      } else {
        setActiveColumn(column);
        setDirection("asc");
      }
    },
    [activeColumn],
  );

  // Dismiss handler for conflict popovers (Task 10 will use this)
  const handleDismiss = useCallback((key: string) => {
    setDismissedConflicts((prev) => {
      const next = new Set(prev);
      next.add(key);
      return next;
    });
  }, []);

  // Labels by mode
  const rightLabel = mode === "single" ? "Repo" : "Prevalence";

  return (
    <div data-testid="package-list">
      <SortHeader
        leftLabel="Packages"
        rightLabel={rightLabel}
        activeColumn={activeColumn}
        direction={direction}
        onSort={handleSort}
      />

      <div role="list">
        {sortedPackages.map((pkg) => (
          <PackageRow
            key={pkg.name}
            pkg={pkg}
            mode={mode}
            tier={repoTierMap.get(pkg.source_repo) ?? "distro"}
            dismissed={dismissedConflicts.has(pkg.name)}
            onToggle={onToggle}
            onDismiss={handleDismiss}
          />
        ))}
      </div>

      <ExcludedZone
        packages={excludedPackages}
        hasEverToggled={hasEverToggled}
      />
    </div>
  );
}

// --- Internal: PackageRow ---

interface PackageRowProps {
  pkg: PackageListPackage;
  mode: "single" | "fleet";
  tier: RepoTier;
  dismissed: boolean;
  onToggle: (name: string) => void;
  onDismiss: (key: string) => void;
}

function PackageRow({ pkg, mode, tier, dismissed, onToggle, onDismiss }: PackageRowProps) {
  const style = repoStyles[tier] ?? repoStyles.distro;
  const checkboxRef = useRef<HTMLInputElement>(null);

  return (
    <div
      role="listitem"
      data-testid={`package-row-${pkg.name}`}
      style={{
        display: "flex",
        alignItems: "center",
        padding: "var(--pf-t--global--spacer--xs) 0",
        gap: "var(--pf-t--global--spacer--sm)",
      }}
    >
      <div
        data-testid="left-column"
        style={{
          flex: 1,
          display: "flex",
          alignItems: "center",
          gap: "var(--pf-t--global--spacer--sm)",
        }}
      >
        <input
          ref={checkboxRef}
          type="checkbox"
          role="checkbox"
          checked={pkg.include}
          aria-label={pkg.name}
          onChange={() => onToggle(pkg.name)}
        />
        <span>{pkg.name}</span>
        {mode === "fleet" && (
          <>
            <span data-testid="repo-text" style={style}>
              {pkg.source_repo}
            </span>
            {pkg.repo_conflict && pkg.repo_conflict.length > 0 && (
              <RepoConflictPopover
                packageName={pkg.name}
                identityKey={pkg.name}
                entries={pkg.repo_conflict}
                isDismissed={dismissed}
                onDismiss={onDismiss}
                focusTargetRef={checkboxRef}
              />
            )}
          </>
        )}
      </div>

      <div
        data-testid="right-column"
        style={{
          minWidth: 80,
          textAlign: "right",
        }}
      >
        {mode === "single" ? (
          <span data-testid="repo-text" style={style}>
            {pkg.source_repo}
          </span>
        ) : (
          <span>
            {pkg.prevalence
              ? `${pkg.prevalence.count}/${pkg.prevalence.total}`
              : "—"}
          </span>
        )}
      </div>
    </div>
  );
}

// --- Helpers ---

function hasUndismissedConflict(
  pkg: PackageListPackage,
  dismissed: Set<string>,
): boolean {
  return (
    pkg.repo_conflict != null &&
    pkg.repo_conflict.length > 0 &&
    !dismissed.has(pkg.name)
  );
}
