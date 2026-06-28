import { useState, useMemo, useCallback, useEffect, useRef } from "react";
import {
  Skeleton,
  EmptyState,
  EmptyStateBody,
  Button,
  Alert,
  AlertGroup,
  AlertActionCloseButton,
  AlertVariant,
} from "@patternfly/react-core";
import type {
  ViewResponse,
  RefinedPackage,
  RefinedConfig,
  RefinementOp,
  ReferenceSection,
} from "../api/types";
import { applyOp, ungroupGroup } from "../api/client";
import { DecisionList } from "./DecisionList";
import type { DecisionItemKind } from "./DecisionItem";
import { ContextList } from "./ContextList";
import { VersionChangesTable } from "./VersionChangesTable";
import { UsersGroupsSection } from "./UsersGroupsSection";
import { ServiceSection } from "./ServiceSection";
import { ContainerSection } from "./ContainerSection";
import { SystemTuningSection } from "./SystemTuningSection";
import { SectionSearch } from "./SectionSearch";
import { RepoBar } from "./RepoBar";
import { PackageList } from "./PackageList";
import type { PackageListPackage } from "./PackageList";
import { LanguagePackageList } from "./LanguagePackageList";
import { UnmanagedFileList } from "./UnmanagedFileList";
import { Content } from "@patternfly/react-core";

/** Maps section IDs to human-readable heading text (mirrors Sidebar labels). */
const SECTION_LABELS: Record<string, string> = {
  packages: "Packages",
  configs: "Config Files",
  services: "Services",
  containers: "Containers",
  system_tuning: "System Tuning",
  version_changes: "Version Changes",
  compose: "Compose",
  network: "Network",
  storage: "Storage",
  scheduled_tasks: "Scheduled Tasks",
  non_rpm_software: "Non-RPM Software",
  kernel_boot: "Kernel & Boot",
  selinux: "Security & Access Control",
  language_packages: "Language Packages",
  unmanaged_files: "Unmanaged Files",
};

export interface MainContentProps {
  activeSection: string;
  loading: boolean;
  viewData: ViewResponse | null;
  sections: ReferenceSection[] | null;
  onViewUpdate: (view: ViewResponse) => void;
  onMutationError: (err: Error) => void;
  sectionSearchOpen: boolean;
  onSectionSearchClose: () => void;
  /** Incremented when global search navigates, to clear section filter even for same-section nav. */
  filterClearCounter?: number;
  /** When set, auto-expands any collapsed summary containing this item ID. */
  revealItemId?: string;
  /** Called with test ID when an action needs focus restoration after undo. */
  onSetUndoFocusTarget?: (testId: string | null) => void;
  /** Toggle a language package environment include/exclude. */
  onToggleLangEnv?: (ecosystem: string, path: string) => void;
  /** Toggle a single unmanaged file include/exclude. */
  onToggleUnmanagedFile?: (path: string) => void;
  /** Toggle all files in an unmanaged directory group. */
  onToggleUnmanagedGroup?: (directory: string, include: boolean) => void;
  /** Bulk-exclude all unmanaged files. */
  onUnmanagedIncludeNone?: () => void;
  /** Reset all unmanaged files to included. */
  onUnmanagedResetAll?: () => void;
  /** True while an optimistic mutation is in flight. */
  isPending?: boolean;
}

interface ToastEntry {
  id: number;
  message: string;
  variant: AlertVariant;
}

function toConfigItems(configs: RefinedConfig[]): DecisionItemKind[] {
  return configs.map((cfg) => ({ type: "config" as const, data: cfg }));
}

/** Convert RefinedPackage[] to the flat PackageListPackage[] expected by PackageList. */
function toPackageListPackages(
  packages: RefinedPackage[],
): PackageListPackage[] {
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
  filterClearCounter = 0,
  revealItemId,
  onSetUndoFocusTarget,
  onToggleLangEnv,
  onToggleUnmanagedFile,
  onToggleUnmanagedGroup,
  onUnmanagedIncludeNone,
  onUnmanagedResetAll,
  isPending = false,
}: MainContentProps) {
  const [filterText, setFilterText] = useState("");
  const toastIdRef = useRef(0);
  const [toasts, setToasts] = useState<ToastEntry[]>([]);
  const [pendingFocusTarget, setPendingFocusTarget] = useState<string | null>(null);

  // Clear stale filter when switching sections or when global search navigates within same section
  useEffect(() => {
    setFilterText("");
  }, [activeSection, filterClearCounter]);

  // Deferred post-ungroup focus: waits for the re-render with new package rows
  // before attempting querySelector, fixing the timing race where the DOM hasn't
  // updated yet after the API round-trip.
  useEffect(() => {
    if (!pendingFocusTarget) return;
    // Use exact-match selectors for each known RPM arch to avoid prefix collisions.
    // Example: "python3" should match "python3.x86_64" but NOT "python3.11.x86_64".
    const RPM_ARCHES = ['x86_64', 'noarch', 'i686', 'aarch64', 's390x', 'ppc64le', 'src'];
    let focused = false;
    for (const arch of RPM_ARCHES) {
      const selector = `[data-testid="package-row-${CSS.escape(pendingFocusTarget)}.${arch}"]`;
      const el = document.querySelector<HTMLElement>(selector);
      if (el) {
        el.focus();
        focused = true;
        break;
      }
    }
    if (focused) {
      setPendingFocusTarget(null);
    }
  }, [pendingFocusTarget, viewData]);

  const dismissToast = useCallback((id: number) => {
    setToasts((prev) => prev.filter((t) => t.id !== id));
  }, []);

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

  // Language packages filtered by SectionSearch query (path substring, case-insensitive)
  const langPkgs = viewData?.language_packages ?? [];
  const filteredLangPkgs = useMemo(() => {
    if (!filterText.trim()) return langPkgs;
    const q = filterText.toLowerCase();
    return langPkgs.filter((env) => env.path.toLowerCase().includes(q));
  }, [langPkgs, filterText]);

  // Unmanaged file groups filtered by SectionSearch query (item path substring, case-insensitive).
  // Groups with zero matching items are excluded; groups with matches are narrowed to matched items.
  const unmanagedGroups = viewData?.unmanaged_files ?? [];
  const filteredUnmanagedGroups = useMemo(() => {
    if (!filterText.trim()) return unmanagedGroups;
    const q = filterText.toLowerCase();
    return unmanagedGroups
      .map((group) => ({
        ...group,
        items: group.items.filter((item) =>
          item.path.toLowerCase().includes(q),
        ),
      }))
      .filter((group) => group.items.length > 0);
  }, [unmanagedGroups, filterText]);

  const filteredConfigItems = useMemo(() => {
    if (!filterText.trim()) return configItems;
    const q = filterText.toLowerCase();
    return configItems.filter((item) => {
      if (item.type !== "config") return false;
      const e = item.data.entry;
      const text =
        `${e.path} ${e.kind} ${e.category} ${e.package ?? ""}`.toLowerCase();
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
          item_id: {
            kind: "Package",
            key: { name: pkg.entry.name, arch: pkg.entry.arch },
          },
          include: !pkg.entry.include,
        },
      };
      applyOp(op)
        .then((updatedView) => onViewUpdate(updatedView))
        .catch((err) =>
          onMutationError(err instanceof Error ? err : new Error(String(err))),
        );
    },
    [viewData, onViewUpdate, onMutationError],
  );

  // Repo toggle: build SetInclude op for repos
  const handleRepoToggle = useCallback(
    (sectionId: string) => {
      const repo = viewData?.repo_groups.find(
        (r) => r.section_id === sectionId,
      );
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
        .catch((err) =>
          onMutationError(err instanceof Error ? err : new Error(String(err))),
        );
    },
    [viewData, onViewUpdate, onMutationError],
  );

  // Group toggle: SetInclude with ItemId::Group
  const handleGroupToggle = useCallback(
    (groupName: string, include: boolean) => {
      const op: RefinementOp = {
        op: "SetInclude",
        target: {
          item_id: { kind: "Group", key: { name: groupName } },
          include,
        },
      };
      applyOp(op)
        .then((updatedView) => onViewUpdate(updatedView))
        .catch((err) =>
          onMutationError(err instanceof Error ? err : new Error(String(err))),
        );
    },
    [onViewUpdate, onMutationError],
  );

  // Group ungroup: UngroupGroup directive
  const handleGroupUngroup = useCallback(
    (groupName: string) => {
      // Find the group to get added count and first new member for focus restoration
      const group = viewData?.package_groups?.find((g) => g.name === groupName);
      const addedCount = group?.added_count ?? 0;
      // Get first non-base-image member for focus target after ungroup.
      // GroupMemberInfo carries bare names (e.g. "httpd") but rendered package
      // rows use canonical "name.arch" (e.g. "package-row-httpd.x86_64").
      // Use a prefix-match selector to bridge the gap.
      const firstNewMember = group?.members?.find((m) => !m.in_base_image);
      const firstMemberName = firstNewMember?.name ?? null;

      // Set focus target for undo: the group row
      onSetUndoFocusTarget?.(`group-row-${groupName}`);

      ungroupGroup(groupName)
        .then((updatedView) => {
          onViewUpdate(updatedView);
          // Show success toast
          const id = ++toastIdRef.current;
          const message =
            addedCount === 0
              ? "Group ungrouped (all packages from base). Ctrl+Z to undo."
              : `Group ungrouped into ${addedCount} package${addedCount !== 1 ? "s" : ""}. Ctrl+Z to undo.`;
          setToasts((prev) => [
            ...prev,
            { id, message, variant: AlertVariant.success },
          ]);
          // Auto-dismiss after 5 seconds
          setTimeout(() => {
            setToasts((prev) => prev.filter((t) => t.id !== id));
          }, 5000);

          // Defer focus to post-render via useEffect — the DOM doesn't have
          // the individual package rows yet until React re-renders with the
          // updated viewData from the API response.
          if (firstMemberName) {
            setPendingFocusTarget(firstMemberName);
          }
        })
        .catch((err) =>
          onMutationError(err instanceof Error ? err : new Error(String(err))),
        );
    },
    [viewData, onViewUpdate, onMutationError, onSetUndoFocusTarget],
  );

  const handleArrowDown = useCallback(() => {
    // Focus the first decision item in the list
    const firstItem = document.querySelector(
      "[data-testid^='decision-item-']",
    ) as HTMLElement | null;
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

    // Render verification banner (baseline is always present in schema v19)
    const digestPrefix = baselineSummary?.image_digest.substring(0, 12) ?? "";
    const banner = baselineSummary ? (
      <Alert
        variant="info"
        isInline
        title={`Baseline compared against ${baselineSummary.image_ref} (${digestPrefix}…) — ${baselineSummary.baseline_count} in base image, ${baselineSummary.user_added_count} user-installed, ${baselineSummary.review_count} require review`}
        style={{ marginBottom: "var(--pf-t--global--spacer--md)" }}
      />
    ) : null;

    return (
      <>
        <Content>
          <h2>{SECTION_LABELS.packages}</h2>
        </Content>
        {banner}
        {sectionSearchOpen && (
          <SectionSearch
            value={filterText}
            onChange={setFilterText}
            onClose={handleSearchClose}
            onArrowDown={handleArrowDown}
            resultCount={0}
          />
        )}
        <RepoBar
          repos={viewData?.repo_groups ?? []}
          onToggle={handleRepoToggle}
        />
        <PackageList
          mode="single"
          packages={packageListPackages}
          repoGroups={viewData?.repo_groups ?? []}
          packageGroups={viewData?.package_groups}
          packageProvenances={viewData?.package_provenances}
          searchQuery={filterText}
          onToggle={handlePackageToggle}
          onRepoToggle={handleRepoToggle}
          onGroupToggle={handleGroupToggle}
          onGroupUngroup={handleGroupUngroup}
        />
        {toasts.length > 0 && (
          <AlertGroup isToast isLiveRegion>
            {toasts.map((toast) => (
              <Alert
                key={toast.id}
                variant={toast.variant}
                title={toast.message}
                actionClose={
                  <AlertActionCloseButton onClose={() => dismissToast(toast.id)} />
                }
              />
            ))}
          </AlertGroup>
        )}
      </>
    );
  }

  if (activeSection === "configs") {
    const hasFilter = filterText.trim().length > 0;
    const noResults = hasFilter && filteredConfigItems.length === 0;
    return (
      <>
        <Content>
          <h2>{SECTION_LABELS.configs}</h2>
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
      <>
        <Content>
          <h2>{SECTION_LABELS.services}</h2>
        </Content>
        <ServiceSection
          services={viewData?.service_states ?? []}
          dropins={viewData?.service_dropins ?? []}
          onViewUpdate={onViewUpdate}
          onMutationError={onMutationError}
        />
      </>
    );
  }

  // Containers decision section — quadlets and flatpaks as toggleable items.
  if (activeSection === "containers") {
    return (
      <>
        <Content>
          <h2>{SECTION_LABELS.containers}</h2>
        </Content>
        <ContainerSection
          quadlets={viewData?.quadlets ?? []}
          flatpaks={viewData?.flatpaks ?? []}
          onViewUpdate={onViewUpdate}
          onMutationError={onMutationError}
        />
      </>
    );
  }

  // System Tuning decision section — sysctls and tuned profiles combined.
  if (activeSection === "system_tuning") {
    return (
      <>
        <Content>
          <h2>{SECTION_LABELS.system_tuning}</h2>
        </Content>
        <SystemTuningSection
          sysctls={viewData?.sysctls ?? []}
          tuned={viewData?.tuned ?? []}
          onViewUpdate={onViewUpdate}
          onMutationError={onMutationError}
        />
      </>
    );
  }

  // Language Packages decision section
  if (activeSection === "language_packages") {
    return (
      <div data-testid="section-language_packages">
        <Content>
          <h2>{SECTION_LABELS.language_packages}</h2>
        </Content>
        {sectionSearchOpen && (
          <SectionSearch
            value={filterText}
            onChange={setFilterText}
            onClose={handleSearchClose}
            onArrowDown={handleArrowDown}
            resultCount={filteredLangPkgs.length}
          />
        )}
        {filterText.trim() && filteredLangPkgs.length === 0 ? (
          <EmptyState titleText="No items match your search" headingLevel="h3">
            <EmptyStateBody>
              <Button variant="link" onClick={() => setFilterText("")}>
                Clear filter
              </Button>
            </EmptyStateBody>
          </EmptyState>
        ) : (
          <LanguagePackageList
            environments={filteredLangPkgs}
            onToggle={(ecosystem, path) => {
              onToggleLangEnv?.(ecosystem, path);
            }}
            isPending={isPending}
            revealItemId={revealItemId}
          />
        )}
      </div>
    );
  }

  // Unmanaged Files decision section
  if (activeSection === "unmanaged_files") {
    return (
      <div data-testid="section-unmanaged_files">
        <Content>
          <h2>{SECTION_LABELS.unmanaged_files}</h2>
        </Content>
        {sectionSearchOpen && (
          <SectionSearch
            value={filterText}
            onChange={setFilterText}
            onClose={handleSearchClose}
            onArrowDown={handleArrowDown}
            resultCount={filteredUnmanagedGroups.reduce(
              (sum, g) => sum + g.items.length,
              0,
            )}
          />
        )}
        {filterText.trim() && filteredUnmanagedGroups.length === 0 ? (
          <EmptyState titleText="No items match your search" headingLevel="h3">
            <EmptyStateBody>
              <Button variant="link" onClick={() => setFilterText("")}>
                Clear filter
              </Button>
            </EmptyStateBody>
          </EmptyState>
        ) : (
          <UnmanagedFileList
            groups={filteredUnmanagedGroups}
            onToggleItem={(path) => {
              onToggleUnmanagedFile?.(path);
            }}
            onToggleGroup={(directory, include) => {
              onToggleUnmanagedGroup?.(directory, include);
            }}
            isPending={isPending}
            onIncludeNone={onUnmanagedIncludeNone}
            onResetAll={onUnmanagedResetAll}
            revealItemId={revealItemId}
          />
        )}
      </div>
    );
  }

  // Context sections: compose, network, storage,
  // scheduled_tasks, non_rpm_software, kernel_boot, selinux
  const contextSectionIds = [
    "version_changes",
    "compose",
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
    const emptyReason = section.empty_reason ?? undefined;

    return (
      <>
        <Content>
          <h2>{SECTION_LABELS.version_changes}</h2>
        </Content>
        <VersionChangesTable
          entries={viewData?.version_changes ?? []}
          emptyReason={emptyReason}
          revealItemId={revealItemId}
        />
      </>
    );
  }

  if (contextSectionIds.includes(activeSection)) {
    // "compose" sidebar entry maps to the "containers" backend context section
    const lookupId = activeSection === "compose" ? "containers" : activeSection;
    const section = sections?.find((s) => s.id === lookupId);
    if (!section) {
      return <p>Section data not available.</p>;
    }

    const heading = SECTION_LABELS[activeSection] ?? activeSection;
    return (
      <>
        <Content>
          <h2>{heading}</h2>
        </Content>
        <ContextList key={lookupId} section={section} />
      </>
    );
  }

  return <p>Not yet implemented.</p>;
}
