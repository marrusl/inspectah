import { useState, useMemo, useCallback, useEffect } from "react";
import {
  Skeleton,
  EmptyState,
  EmptyStateBody,
  Button,
  Alert,
} from "@patternfly/react-core";
import type { ViewResponse, RefinedPackage, RefinedConfig, RefinementOp, ContextSection } from "../api/types";
import { applyOp } from "../api/client";
import { DecisionList } from "./DecisionList";
import type { DecisionItemKind } from "./DecisionItem";
import { ContextList } from "./ContextList";
import { UsersGroupsSection } from "./UsersGroupsSection";
import { ServiceSection } from "./ServiceSection";
import { SectionSearch } from "./SectionSearch";
import { RepoBar } from "./RepoBar";
import { PackageList } from "./PackageList";
import type { PackageListPackage } from "./PackageList";
import { CubesIcon } from "@patternfly/react-icons";


export interface MainContentProps {
  activeSection: string;
  loading: boolean;
  viewData: ViewResponse | null;
  sections: ContextSection[] | null;
  onViewUpdate: (view: ViewResponse) => void;
  onMutationError: (err: Error) => void;
  sectionSearchOpen: boolean;
  onSectionSearchClose: () => void;
  /** Called when a viewed POST succeeds, so App can refresh its viewed count. */
  onViewedChange?: () => void;
  /** Incremented when global search navigates, to clear section filter even for same-section nav. */
  filterClearCounter?: number;
  /** When set, auto-expands any collapsed summary containing this item ID. */
  revealItemId?: string;
}

function toConfigItems(configs: RefinedConfig[]): DecisionItemKind[] {
  return configs.map((cfg) => ({ type: "config" as const, data: cfg }));
}

/** Convert RefinedPackage[] to the flat PackageListPackage[] expected by PackageList. */
function toPackageListPackages(packages: RefinedPackage[]): PackageListPackage[] {
  return packages.map((pkg) => ({
    name: `${pkg.entry.name}.${pkg.entry.arch}`,
    source_repo: pkg.entry.source_repo,
    include: pkg.entry.include,
  }));
}

export function MainContent({
  activeSection,
  loading,
  viewData,
  sections,
  onViewUpdate,
  onMutationError,
  sectionSearchOpen,
  onSectionSearchClose,
  onViewedChange,
  filterClearCounter = 0,
  revealItemId,
}: MainContentProps) {
  const [filterText, setFilterText] = useState("");

  // Clear stale filter when switching sections or when global search navigates within same section
  useEffect(() => {
    setFilterText("");
  }, [activeSection, filterClearCounter]);

  // Reset filter when section changes or search closes
  const handleSearchClose = useCallback(() => {
    setFilterText("");
    onSectionSearchClose();
  }, [onSectionSearchClose]);

  // Convert packages to the flat format expected by PackageList
  const packageListPackages = useMemo(
    () => (viewData ? toPackageListPackages(viewData.packages) : []),
    [viewData],
  );

  const configItems = useMemo(
    () => (viewData ? toConfigItems(viewData.config_files) : []),
    [viewData],
  );

  const filteredConfigItems = useMemo(() => {
    if (!filterText.trim()) return configItems;
    const q = filterText.toLowerCase();
    return configItems.filter((item) => {
      if (item.type !== "config") return false;
      const e = item.data.entry;
      const text = `${e.path} ${e.kind} ${e.category} ${e.package ?? ""}`.toLowerCase();
      return text.includes(q);
    });
  }, [configItems, filterText]);

  // Package toggle: build SetInclude op from "name.arch" string
  const handlePackageToggle = useCallback(
    (nameArch: string) => {
      const pkg = viewData?.packages.find(
        (p) => `${p.entry.name}.${p.entry.arch}` === nameArch,
      );
      if (!pkg) return;
      const op: RefinementOp = {
        op: "SetInclude",
        target: {
          item_id: { kind: "Package", key: { name: pkg.entry.name, arch: pkg.entry.arch } },
          include: !pkg.entry.include,
        },
      };
      applyOp(op)
        .then((updatedView) => onViewUpdate(updatedView))
        .catch((err) => onMutationError(err instanceof Error ? err : new Error(String(err))));
    },
    [viewData, onViewUpdate, onMutationError],
  );

  // Repo toggle: build SetInclude op for repos
  const handleRepoToggle = useCallback(
    (sectionId: string) => {
      const repo = viewData?.repo_groups.find((r) => r.section_id === sectionId);
      if (!repo) return;
      const op: RefinementOp = {
        op: "SetInclude",
        target: {
          item_id: { kind: "Repo", key: { path: sectionId } },
          include: !repo.enabled,
        },
      };
      applyOp(op)
        .then((updatedView) => onViewUpdate(updatedView))
        .catch((err) => onMutationError(err instanceof Error ? err : new Error(String(err))));
    },
    [viewData, onViewUpdate, onMutationError],
  );

  const handleArrowDown = useCallback(() => {
    // Focus the first decision item in the list
    const firstItem = document.querySelector("[data-testid^='decision-item-']") as HTMLElement | null;
    firstItem?.focus();
  }, []);

  if (loading) {
    return (
      <>
        <Skeleton screenreaderText="Loading content" width="40%" />
        <br />
        <Skeleton width="100%" />
        <Skeleton width="100%" />
        <Skeleton width="80%" />
      </>
    );
  }

  if (activeSection === "packages") {
    const baselineSummary = viewData?.baseline_summary;

    // Render verification banner
    let banner: JSX.Element | null = null;
    if (baselineSummary) {
      const digestPrefix = baselineSummary.image_digest.substring(0, 12);
      banner = (
        <Alert
          variant="info"
          isInline
          title={`Baseline compared against ${baselineSummary.image_ref} (${digestPrefix}…) — ${baselineSummary.baseline_count} in base image, ${baselineSummary.user_added_count} user-installed, ${baselineSummary.review_count} require review`}
          style={{ marginBottom: "var(--pf-t--global--spacer--md)" }}
        />
      );
    } else {
      banner = (
        <Alert
          variant="warning"
          isInline
          title="Baseline unavailable — all added packages shown as NeedsReview"
          style={{ marginBottom: "var(--pf-t--global--spacer--md)" }}
        />
      );
    }

    return (
      <>
        {banner}
        <RepoBar
          repos={viewData?.repo_groups ?? []}
          onToggle={handleRepoToggle}
        />
        <PackageList
          mode="single"
          packages={packageListPackages}
          repoGroups={viewData?.repo_groups ?? []}
          onToggle={handlePackageToggle}
          onRepoToggle={handleRepoToggle}
        />
      </>
    );
  }

  if (activeSection === "configs") {
    const hasFilter = filterText.trim().length > 0;
    const noResults = hasFilter && filteredConfigItems.length === 0;
    return (
      <>
        {sectionSearchOpen && (
          <SectionSearch
            value={filterText}
            onChange={setFilterText}
            onClose={handleSearchClose}
            onArrowDown={handleArrowDown}
            resultCount={filteredConfigItems.length}
          />
        )}
        {noResults ? (
          <EmptyState titleText="No items match your search" headingLevel="h3">
            <EmptyStateBody>
              <Button variant="link" onClick={() => setFilterText("")}>
                Clear filter
              </Button>
            </EmptyStateBody>
          </EmptyState>
        ) : (
          <DecisionList
            items={filteredConfigItems}
            sectionLabel="Config Files"
            filterText={filterText}
            revealItemId={revealItemId}
            onViewUpdate={onViewUpdate}
            onMutationError={onMutationError}
            onViewedChange={onViewedChange}
          />
        )}
      </>
    );
  }

  // Users & Groups decision section — all trust-cue data (has_sudo, has_subuid,
  // ssh_key_count, classification_rationale, etc.) is enriched directly on each
  // UserDecision by the collector, delivered via ViewResponse.users_groups_decisions.
  if (activeSection === "users_groups") {
    const userDecisions = viewData?.users_groups_decisions ?? [];

    return (
      <UsersGroupsSection
        users={userDecisions}
        sessionIsSensitive={viewData?.session_is_sensitive ?? false}
        onViewUpdate={onViewUpdate}
        onMutationError={onMutationError}
      />
    );
  }

  // Services decision section — promoted from context to decision.
  if (activeSection === "services") {
    return (
      <ServiceSection
        services={viewData?.service_states ?? []}
        dropins={viewData?.service_dropins ?? []}
        onViewUpdate={onViewUpdate}
        onMutationError={onMutationError}
      />
    );
  }

  // Context sections: containers, network, storage,
  // scheduled_tasks, non_rpm_software, kernel_boot, selinux
  const contextSectionIds = [
    "version_changes",
    "containers",
    "network",
    "storage",
    "scheduled_tasks",
    "non_rpm_software",
    "kernel_boot",
    "selinux",
  ];

  if (activeSection === "version_changes") {
    const section = sections?.find((s) => s.id === "version_changes");
    if (!section) {
      return <p>Section data not available.</p>;
    }

    if (section.items.length === 0 && section.empty_reason) {
      const copyMap: Record<string, string> = {
        no_baseline: "Version comparison requires a baseline. Run with --baseline to enable.",
        zero_drift: "All packages match the target baseline versions.",
        data_unavailable: "Version change data is not available for this snapshot.",
      };
      const copy = copyMap[section.empty_reason] ?? copyMap.data_unavailable;
      return <EmptyState titleText={copy} icon={CubesIcon} headingLevel="h3" />;
    }

    return <ContextList section={section} />;
  }

  if (contextSectionIds.includes(activeSection)) {
    const section = sections?.find((s) => s.id === activeSection);
    if (!section) {
      return <p>Section data not available.</p>;
    }

    return <ContextList section={section} />;
  }

  return <p>Not yet implemented.</p>;
}
