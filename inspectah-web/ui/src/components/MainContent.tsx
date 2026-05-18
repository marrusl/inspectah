import { useState, useMemo, useCallback, useEffect } from "react";
import {
  PageSection,
  Content,
  Skeleton,
  EmptyState,
  EmptyStateBody,
  Button,
  Alert,
} from "@patternfly/react-core";
import type { ViewResponse, RefinedPackage, RefinedConfig, ContextSection } from "../api/types";
import { DecisionList } from "./DecisionList";
import type { DecisionItemKind } from "./DecisionItem";
import { ContextList } from "./ContextList";
import { UsersGroupsSection } from "./UsersGroupsSection";
import { SectionSearch } from "./SectionSearch";
import { AttentionSummary } from "./AttentionSummary";
import { highestAttention } from "./attentionUtils";
import { CubesIcon } from "@patternfly/react-icons";

/** Section ID to human-readable label. */
const SECTION_LABELS: Record<string, string> = {
  packages: "Packages",
  configs: "Config Files",
  services: "Services",
  version_changes: "Version Changes",
  containers: "Containers",
  users_groups: "Users & Groups",
  network: "Network",
  storage: "Storage",
  scheduled_tasks: "Scheduled Tasks",
  non_rpm_software: "Non-RPM Software",
  kernel_boot: "Kernel & Boot",
  selinux: "Security & Access Control",
};

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

function toPackageItems(packages: RefinedPackage[]): DecisionItemKind[] {
  return packages.map((pkg) => ({ type: "package" as const, data: pkg }));
}

function toConfigItems(configs: RefinedConfig[]): DecisionItemKind[] {
  return configs.map((cfg) => ({ type: "config" as const, data: cfg }));
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
  const label = SECTION_LABELS[activeSection] ?? activeSection;
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

  const packageItems = useMemo(
    () => (viewData ? toPackageItems(viewData.packages) : []),
    [viewData],
  );
  const configItems = useMemo(
    () => (viewData ? toConfigItems(viewData.config_files) : []),
    [viewData],
  );

  // Filter decision items by search text
  const filteredPackageItems = useMemo(() => {
    if (!filterText.trim()) return packageItems;
    const q = filterText.toLowerCase();
    return packageItems.filter((item) => {
      if (item.type !== "package") return false;
      const e = item.data.entry;
      const text = `${e.name} ${e.arch} ${e.version} ${e.source_repo}`.toLowerCase();
      return text.includes(q);
    });
  }, [packageItems, filterText]);

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

  const handleArrowDown = useCallback(() => {
    // Focus the first decision item in the list
    const firstItem = document.querySelector("[data-testid^='decision-item-']") as HTMLElement | null;
    firstItem?.focus();
  }, []);

  if (loading) {
    return (
      <PageSection>
        <Skeleton screenreaderText="Loading content" width="40%" />
        <br />
        <Skeleton width="100%" />
        <Skeleton width="100%" />
        <Skeleton width="80%" />
      </PageSection>
    );
  }

  if (activeSection === "packages") {
    const hasFilter = filterText.trim().length > 0;
    const noResults = hasFilter && filteredPackageItems.length === 0;
    const baselineSummary = viewData?.baseline_summary;

    // Compute attention counts for AttentionSummary
    const needsReviewPkgs = packageItems.filter(
      (item) => item.data.attention.length > 0 &&
        highestAttention(item.data.attention) === "needs_review",
    );
    const infoPkgs = packageItems.filter(
      (item) => item.data.attention.length > 0 &&
        highestAttention(item.data.attention) === "informational",
    );
    const needsReviewRepos = new Set(
      needsReviewPkgs
        .filter((item) => item.type === "package")
        .map((item) => (item.data as any).entry.source_repo),
    );
    const infoRepos = new Set(
      infoPkgs
        .filter((item) => item.type === "package")
        .map((item) => (item.data as any).entry.source_repo),
    );

    // Render verification banner
    let banner: JSX.Element | null = null;
    if (baselineSummary) {
      // Verified mode: baseline comparison active
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
      // Degraded mode: baseline unavailable
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
      <PageSection>
        <Content>
          <h2>{label}</h2>
        </Content>
        {banner}
        <AttentionSummary
          needsReviewCount={needsReviewPkgs.length}
          needsReviewRepoCount={needsReviewRepos.size}
          infoCount={infoPkgs.length}
          infoRepoCount={infoRepos.size}
        />
        {sectionSearchOpen && (
          <SectionSearch
            value={filterText}
            onChange={setFilterText}
            onClose={handleSearchClose}
            onArrowDown={handleArrowDown}
            resultCount={filteredPackageItems.length}
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
            items={filteredPackageItems}
            sectionLabel="Packages"
            filterText={filterText}
            repoGroups={viewData?.repo_groups ?? []}
            revealItemId={revealItemId}
            leafDepTree={viewData?.leaf_dep_tree}
            versionChanges={viewData?.version_changes}
            onViewUpdate={onViewUpdate}
            onMutationError={onMutationError}
            onViewedChange={onViewedChange}
          />
        )}
      </PageSection>
    );
  }

  if (activeSection === "configs") {
    const hasFilter = filterText.trim().length > 0;
    const noResults = hasFilter && filteredConfigItems.length === 0;
    return (
      <PageSection>
        <Content>
          <h2>{label}</h2>
        </Content>
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
      </PageSection>
    );
  }

  // Users & Groups decision section
  if (activeSection === "users_groups") {
    // Extract section-level data from the context sections endpoint
    const ugSection = sections?.find((s) => s.id === "users_groups");
    // The actual user decisions come from viewData
    const userDecisions = viewData?.users_groups_decisions ?? [];

    // Parse section-level SSH refs, sudoers, subuid from context section items
    // These are available from the snapshot context, not the view endpoint
    const sshRefs: Array<{ user: string; path: string }> = [];
    const sudoersRules: string[] = [];
    const subuidEntries: string[] = [];

    if (ugSection) {
      for (const item of ugSection.items) {
        if (item.id.startsWith("ssh:")) {
          const parts = item.title.split(" ");
          sshRefs.push({ user: parts[0] ?? "", path: item.subtitle ?? "" });
        }
      }
    }

    return (
      <UsersGroupsSection
        users={userDecisions}
        sshAuthorizedKeysRefs={sshRefs}
        sudoersRules={sudoersRules}
        subuidEntries={subuidEntries}
        sessionIsSensitive={viewData?.session_is_sensitive ?? false}
        onViewUpdate={onViewUpdate}
        onMutationError={onMutationError}
      />
    );
  }

  // Context sections: services, containers, network, storage,
  // scheduled_tasks, non_rpm_software, kernel_boot, selinux
  const contextSectionIds = [
    "services",
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
      return (
        <PageSection>
          <Content><h2>{label}</h2></Content>
          <p>Section data not available.</p>
        </PageSection>
      );
    }

    if (section.items.length === 0 && section.empty_reason) {
      const copyMap: Record<string, string> = {
        no_baseline: "Version comparison requires a baseline. Run with --baseline to enable.",
        zero_drift: "All packages match the target baseline versions.",
        data_unavailable: "Version change data is not available for this snapshot.",
      };
      const copy = copyMap[section.empty_reason] ?? copyMap.data_unavailable;
      return (
        <PageSection>
          <Content><h2>{label}</h2></Content>
          <EmptyState titleText={copy} icon={CubesIcon} headingLevel="h3" />
        </PageSection>
      );
    }

    return (
      <PageSection>
        <Content><h2>{label}</h2></Content>
        <ContextList section={section} />
      </PageSection>
    );
  }

  if (contextSectionIds.includes(activeSection)) {
    const section = sections?.find((s) => s.id === activeSection);
    if (!section) {
      return (
        <PageSection>
          <Content>
            <h2>{label}</h2>
            <p>Section data not available.</p>
          </Content>
        </PageSection>
      );
    }

    return (
      <PageSection>
        <Content>
          <h2>{label}</h2>
        </Content>
        <ContextList section={section} />
      </PageSection>
    );
  }

  return (
    <PageSection>
      <Content>
        <h2>{label}</h2>
        <p>Not yet implemented.</p>
      </Content>
    </PageSection>
  );
}
