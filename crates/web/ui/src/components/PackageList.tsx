import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { SortHeader } from "./SortHeader";
import { ExcludedZone } from "./ExcludedZone";
import { GroupRow } from "./GroupRow";
import { RepoConflictPopover } from "./aggregate/RepoConflictPopover";
import {
  usePrevalenceDisplay,
  formatPrevalence,
} from "../hooks/usePrevalenceDisplay";
import type {
  GroupInfo,
  PackageProvenance,
  RepoGroupInfo,
  RepoTier,
} from "../api/types";

// --- Types ---

export interface PackageListPackage {
  name: string;
  source_repo: string;
  include: boolean;
  prevalence?: { count: number; total: number };
  repo_conflict?: { repo: string; host_count: number }[];
}

export interface PackageListProps {
  mode: "single" | "aggregate";
  packages: PackageListPackage[];
  repoGroups: RepoGroupInfo[];
  /** Groups from the view response (package_groups). */
  packageGroups?: GroupInfo[];
  /** Package provenance data for spillover badges. */
  packageProvenances?: Record<string, PackageProvenance>;
  /** Current section search text. Groups with matching members auto-expand. */
  searchQuery?: string;
  onToggle: (packageName: string) => void;
  onRepoToggle: (sectionId: string) => void;
  /** Called when a group's include toggle changes. */
  onGroupToggle?: (groupName: string, include: boolean) => void;
  /** Called when a group is ungrouped (dissolved into individual packages). */
  onGroupUngroup?: (groupName: string) => void;
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
  packageGroups,
  packageProvenances,
  searchQuery = "",
  onToggle,
  onRepoToggle: _onRepoToggle,
  onGroupToggle,
  onGroupUngroup,
  onDismissedCountChange,
  onRestoreDismissed,
}: PackageListProps) {
  // Sort state: defaults differ by mode
  const [activeColumn, setActiveColumn] = useState<"left" | "right">(
    mode === "aggregate" ? "right" : "left",
  );
  const [direction, setDirection] = useState<"asc" | "desc">("asc");

  // Track groups the user manually expanded (persists across search changes)
  const [userExpandedGroups, setUserExpandedGroups] = useState<Set<string>>(
    () => new Set(),
  );

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
        // Tier → repo → name: group by tier, then by repo within tier, then alpha
        sorted.sort((a, b) => {
          const tierA = TIER_ORDER[repoTierMap.get(a.source_repo) ?? "distro"];
          const tierB = TIER_ORDER[repoTierMap.get(b.source_repo) ?? "distro"];
          if (tierA !== tierB) {
            return direction === "asc" ? tierA - tierB : tierB - tierA;
          }
          const repoCmp = a.source_repo.localeCompare(b.source_repo);
          if (repoCmp !== 0) {
            return direction === "asc" ? repoCmp : -repoCmp;
          }
          return a.name.localeCompare(b.name);
        });
      }
    } else {
      // Aggregate mode
      if (activeColumn === "right" || activeColumn === "left") {
        // Prevalence ascending (rarest first) is the default for aggregate
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
  }, [
    includedPackages,
    mode,
    activeColumn,
    direction,
    repoTierMap,
    dismissedConflicts,
  ]);

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

  // All non-ungrouped groups are visible: renderable, excluded, and degraded
  const visibleGroups = useMemo(
    () =>
      (packageGroups ?? []).filter((g) => g.render_state !== "ungrouped"),
    [packageGroups],
  );

  // Build suppression set: renderable group members should not appear individually
  const renderableMemberSet = useMemo(() => {
    const set = new Set<string>();
    for (const group of visibleGroups) {
      if (group.render_state === "renderable") {
        for (const member of group.members) {
          set.add(member.name);
        }
      }
    }
    return set;
  }, [visibleGroups]);

  // Compute which groups should be auto-expanded by search
  const searchLower = searchQuery.trim().toLowerCase();
  const autoExpandedGroups = useMemo(() => {
    if (!searchLower) return new Set<string>();
    const set = new Set<string>();
    for (const group of visibleGroups) {
      // Only auto-expand if search matches a member name, NOT the group name
      const groupNameMatches = group.name.toLowerCase().includes(searchLower);
      if (groupNameMatches) continue;

      const hasMemberMatch = group.members.some((m) =>
        m.name.toLowerCase().includes(searchLower),
      );
      if (hasMemberMatch) {
        set.add(group.name);
      }
    }
    return set;
  }, [visibleGroups, searchLower]);

  // Handler for user manual expand/collapse
  const handleGroupExpandChange = useCallback(
    (groupName: string, expanded: boolean) => {
      setUserExpandedGroups((prev) => {
        const next = new Set(prev);
        if (expanded) {
          next.add(groupName);
        } else {
          next.delete(groupName);
        }
        return next;
      });
    },
    [],
  );

  // Remove renderable group members from the individual zone.
  // Package names use canonical "name.arch" format (e.g. "httpd.x86_64")
  // while GroupMemberInfo carries bare names (e.g. "httpd").  Extract the
  // bare portion before the last dot for suppression comparison.
  const individualPackages = useMemo(() => {
    return sortedPackages.filter((pkg) => {
      const bare = extractBareName(pkg.name);
      return !renderableMemberSet.has(bare);
    });
  }, [sortedPackages, renderableMemberSet]);

  // Filter individual packages by search
  const filteredIndividualPackages = useMemo(() => {
    if (!searchLower) return individualPackages;
    return individualPackages.filter((pkg) =>
      pkg.name.toLowerCase().includes(searchLower),
    );
  }, [individualPackages, searchLower]);

  // Summary counts - unique packages across renderable groups (deduplicates overlaps)
  const renderableOnly = useMemo(
    () => visibleGroups.filter((g) => g.render_state === "renderable"),
    [visibleGroups],
  );
  const { newCount, baseCount } = useMemo(() => {
    const newSet = new Set<string>();
    const baseSet = new Set<string>();
    for (const group of visibleGroups) {
      for (const member of group.members) {
        if (member.in_base_image) {
          baseSet.add(member.name);
        } else {
          newSet.add(member.name);
        }
      }
    }
    // Remove any package from baseSet that also appears in newSet (new takes precedence)
    for (const name of newSet) baseSet.delete(name);
    return { newCount: newSet.size, baseCount: baseSet.size };
  }, [visibleGroups]);

  const groupSummaryLabel = useMemo(() => {
    if (newCount === 0) return "all from base";
    if (baseCount === 0) return `${newCount} ${newCount === 1 ? "package" : "packages"}`;
    return `${newCount} new, ${baseCount} from base`;
  }, [newCount, baseCount]);
  const optionalSpilloverCount = useMemo(
    () =>
      renderableOnly.reduce(
        (sum, g) => sum + g.optional_spillover_count,
        0,
      ),
    [renderableOnly],
  );

  // Filter groups by search: show group if its name or any member matches
  const filteredGroups = useMemo(() => {
    if (!searchLower) return visibleGroups;
    return visibleGroups.filter((g) => {
      if (g.name.toLowerCase().includes(searchLower)) return true;
      return g.members.some((m) =>
        m.name.toLowerCase().includes(searchLower),
      );
    });
  }, [visibleGroups, searchLower]);

  // Determine which groups and packages to display
  const displayGroups = searchLower ? filteredGroups : visibleGroups;
  const displayPackages = searchLower ? filteredIndividualPackages : individualPackages;

  // Group toggle handler
  const handleGroupToggle = useCallback(
    (groupName: string, include: boolean) => {
      onGroupToggle?.(groupName, include);
    },
    [onGroupToggle],
  );

  // Group ungroup handler
  const handleGroupUngroup = useCallback(
    (groupName: string) => {
      onGroupUngroup?.(groupName);
    },
    [onGroupUngroup],
  );

  // Focus on first match when search changes
  useEffect(() => {
    if (!searchLower) return;

    // Check if search matches a group name (focus group row, don't expand)
    for (const group of displayGroups) {
      if (group.name.toLowerCase().includes(searchLower)) {
        const groupRow = document.querySelector(
          `[data-testid="group-row-${CSS.escape(group.name)}"]`
        ) as HTMLElement;
        if (groupRow) {
          groupRow.focus();
          groupRow.scrollIntoView?.({ block: "nearest", behavior: "smooth" });
          return;
        }
      }
    }

    // Check if search matches a member name (focus member row inside auto-expanded group)
    for (const group of displayGroups) {
      for (const member of group.members) {
        if (member.name.toLowerCase().includes(searchLower)) {
          const memberRow = document.querySelector(
            `[data-testid="group-member-${CSS.escape(member.name)}"]`
          ) as HTMLElement;
          if (memberRow) {
            memberRow.focus();
            memberRow.scrollIntoView?.({ block: "nearest", behavior: "smooth" });
            return;
          }
        }
      }
    }

    // If no group/member match, try individual packages
    for (const pkg of displayPackages) {
      if (pkg.name.toLowerCase().includes(searchLower)) {
        const row = document.querySelector(
          `[data-testid="package-row-${CSS.escape(pkg.name)}"]`
        ) as HTMLElement;
        if (row) {
          row.focus();
          row.scrollIntoView?.({ block: "nearest", behavior: "smooth" });
          return;
        }
      }
    }
  }, [searchLower, displayGroups, displayPackages]);

  // Labels by mode
  const rightLabel = mode === "single" ? "Repo" : "Prevalence";

  const hasGroups = visibleGroups.length > 0;
  const showGroupsZone = hasGroups && displayGroups.length > 0;

  return (
    <div data-testid="package-list">
      <div
        data-testid="package-list-summary"
        className="inspectah-package-list__summary"
      >
        {searchLower ? (
          // Filtered view during search
          <>
            {hasGroups && (
              <>
                {displayGroups.length}{" "}
                {displayGroups.length === 1 ? "group" : "groups"} ({groupSummaryLabel}) &middot;{" "}
              </>
            )}
            {displayPackages.length}{" "}
            {hasGroups ? "other " : ""}
            {displayPackages.length === 1 ? "package" : "packages"}
          </>
        ) : (
          // Full view when not searching
          <>
            {hasGroups && (
              <>
                {visibleGroups.length}{" "}
                {visibleGroups.length === 1 ? "group" : "groups"} ({groupSummaryLabel}) &middot;{" "}
              </>
            )}
            {displayPackages.length}{" "}
            {hasGroups ? "other " : ""}
            {displayPackages.length === 1 ? "package" : "packages"}
            {hasGroups && optionalSpilloverCount > 0 && (
              <>
                {" "}
                &middot; {optionalSpilloverCount} optional from groups
              </>
            )}
          </>
        )}
      </div>

      {showGroupsZone && (
        <div data-testid="groups-zone" role="list">
          {displayGroups.map((group) => (
            <GroupRow
              key={group.name}
              group={group}
              searchQuery={searchQuery}
              isIncluded={group.render_state !== "excluded"}
              forceExpanded={autoExpandedGroups.has(group.name)}
              defaultExpanded={userExpandedGroups.has(group.name)}
              onExpandChange={handleGroupExpandChange}
              onToggle={handleGroupToggle}
              onUngroup={handleGroupUngroup}
            />
          ))}
        </div>
      )}

      {hasGroups && (
        <div
          data-testid="zone-divider"
          className="inspectah-package-list__zone-divider"
        >
          Individual Packages
        </div>
      )}

      <SortHeader
        leftLabel="Packages"
        rightLabel={rightLabel}
        activeColumn={activeColumn}
        direction={direction}
        onSort={handleSort}
      />

      <div role="list" data-testid="individual-packages-zone">
        {displayPackages.map((pkg) => (
          <PackageRow
            key={pkg.name}
            pkg={pkg}
            mode={mode}
            tier={repoTierMap.get(pkg.source_repo) ?? "distro"}
            dismissed={dismissedConflicts.has(pkg.name)}
            packageProvenances={packageProvenances}
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
  mode: "single" | "aggregate";
  tier: RepoTier;
  dismissed: boolean;
  packageProvenances?: Record<string, PackageProvenance>;
  onToggle: (name: string) => void;
  onDismiss: (key: string) => void;
}

function prevalenceClass(count: number, total: number): string {
  if (total === 0) return "";
  const ratio = count / total;
  if (ratio >= 1) return "inspectah-package-row__prevalence--full";
  if (ratio >= 0.6) return "inspectah-package-row__prevalence--partial";
  return "inspectah-package-row__prevalence--low";
}

function PackageRow({
  pkg,
  mode,
  tier,
  dismissed,
  packageProvenances,
  onToggle,
  onDismiss,
}: PackageRowProps) {
  const { mode: prevalenceMode, cycle: cyclePrevalence } =
    usePrevalenceDisplay();
  const style = repoStyles[tier] ?? repoStyles.distro;
  const checkboxRef = useRef<HTMLInputElement>(null);

  // Look up provenance for this package.  The map is keyed by "name.arch"
  // but PackageListPackage carries only name, so do a direct lookup with a
  // dot-boundary guard to avoid "foo" matching "foobar.x86_64".
  const provenance = packageProvenances
    ? Object.entries(packageProvenances).find(
        ([key]) =>
          key === pkg.name ||
          (key.startsWith(pkg.name) && key[pkg.name.length] === "."),
      )?.[1]
    : undefined;

  // Generate badge text from provenance
  const provenanceBadge = (() => {
    if (!provenance) return null;
    switch (provenance.kind) {
      case "optional_spillover":
        return `optional from "${provenance.group_name}"`;
      case "ungrouped_member":
        return `ungrouped from "${provenance.group_name}"`;
      case "degraded_member":
        return `from "${provenance.group_name}" (rendered individually)`;
      default:
        return null;
    }
  })();

  return (
    <div
      role="listitem"
      tabIndex={-1}
      data-testid={`package-row-${pkg.name}`}
      className="inspectah-package-row"
    >
      <div data-testid="left-column" className="inspectah-package-row__left">
        <input
          ref={checkboxRef}
          type="checkbox"
          role="checkbox"
          checked={pkg.include}
          aria-label={pkg.name}
          onChange={() => onToggle(pkg.name)}
        />
        <div className="inspectah-package-row__name-container">
          <span className="inspectah-package-row__name">{pkg.name}</span>
          {provenanceBadge && (
            <span
              data-testid="provenance-badge"
              className="inspectah-package-row__provenance-badge"
            >
              {provenanceBadge}
            </span>
          )}
        </div>
        {mode === "aggregate" && (
          <>
            <span
              data-testid="repo-text"
              className="inspectah-package-row__repo"
              style={style}
            >
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

      <div data-testid="right-column" className="inspectah-package-row__right">
        {mode === "single" ? (
          <span data-testid="repo-text" style={style}>
            {pkg.source_repo}
          </span>
        ) : pkg.prevalence ? (
            <button
              type="button"
              className={`inspectah-package-row__prevalence-btn ${prevalenceClass(pkg.prevalence.count, pkg.prevalence.total)}`}
              onClick={(e) => {
                e.stopPropagation();
                cyclePrevalence();
              }}
              title="Click to toggle display format"
            >
              {formatPrevalence(
                pkg.prevalence.count,
                pkg.prevalence.total,
                prevalenceMode,
              )}
            </button>
        ) : (
            <span>—</span>
        )}
      </div>
    </div>
  );
}

// --- Helpers ---

/** Known RPM architecture suffixes used in canonical "name.arch" identifiers. */
const RPM_ARCHES = new Set([
  "x86_64",
  "noarch",
  "i686",
  "aarch64",
  "s390x",
  "ppc64le",
  "src",
]);

/**
 * Extract the bare package name from a canonical "name.arch" string.
 * Falls back to the full string when no recognised arch suffix is present,
 * so plain bare names pass through unchanged.
 */
function extractBareName(nameArch: string): string {
  const dotIdx = nameArch.lastIndexOf(".");
  if (dotIdx === -1) return nameArch;
  const suffix = nameArch.slice(dotIdx + 1);
  return RPM_ARCHES.has(suffix) ? nameArch.slice(0, dotIdx) : nameArch;
}

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
