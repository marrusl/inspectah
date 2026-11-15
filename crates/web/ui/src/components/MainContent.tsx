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
  ExpandableSection,
} from "@patternfly/react-core";
import type {
  ViewResponse,
  RefinedPackage,
  RefinedConfig,
  RefinementOp,
  ReferenceSection,
  NotCarriedRow,
  ContextItem,
  DeclinedGeneratedView,
  DeclinedView,
} from "../api/types";
import { isIncluded, isToggleable } from "../api/disposition";
import { applyOp, ungroupGroup } from "../api/client";
import { usePendingGuard } from "../hooks/usePendingGuard";
import { useItemFailures } from "../hooks/useItemFailures";
import { DecisionList } from "./DecisionList";
import type { DecisionItemKind } from "./DecisionItem";
import { ContextList } from "./ContextList";
import { VersionChangesTable } from "./VersionChangesTable";
import { UsersGroupsSection } from "./UsersGroupsSection";
import type { UserFailure } from "./UserCard";
import type { HistoryChange } from "./userHistory";
import { ServiceSection } from "./ServiceSection";
import { ContainerSection } from "./ContainerSection";
import { SectionSearch } from "./SectionSearch";
import { RepoBar } from "./RepoBar";
import { PackageList } from "./PackageList";
import type { PackageListPackage } from "./PackageList";
import { LanguagePackageList } from "./LanguagePackageList";
import { UnmanagedFileList } from "./UnmanagedFileList";
import { SystemTuningSection } from "./SystemTuningSection";
import { NotCarriedGroup, notCarriedRowKey } from "./NotCarriedGroup";
import { buildShadowOwners } from "./shadowOwner";
import { Content } from "@patternfly/react-core";

/** Maps section IDs to human-readable heading text (mirrors Sidebar labels). */
const SECTION_LABELS: Record<string, string> = {
  packages: "Packages",
  config: "Configuration Files",
  configs: "Config Files",
  units: "Services",
  containers: "Containers",
  compose: "Compose",
  network: "Network",
  storage: "Storage",
  scheduled_tasks: "Scheduled Tasks",
  non_rpm_software: "Non-RPM Software",
  kernel_boot: "Kernel & Boot",
  system_tuning: "System Tuning",
  selinux: "Security & Access Control",
  language_packages: "Language Packages",
  unmanaged_files: "Unmanaged Files",
};

/**
 * The declined generated unit a Scheduled Tasks row shows: the one whose
 * request came from the generated timer this row describes.
 */
function scheduledDeclined(
  declined: DeclinedGeneratedView[],
  item: ContextItem,
): DeclinedView | undefined {
  if (item.generated_timer === undefined) return undefined;
  return declined.find(
    (d) => d.row.row === "scheduled" && d.row.name === item.generated_timer,
  )?.declined;
}

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
  /**
   * Incremented after a successful reset. Every user record is replaced,
   * so failures kept for the old records are cleared.
   */
  resetCounter?: number;
  /**
   * Set to a new object after each successful Undo or Redo, naming the
   * users whose record that step changed. Their kept failures were about
   * the record the step replaced, so they are cleared; other users keep
   * theirs.
   */
  historyChange?: HistoryChange;
  /** When set, auto-expands any collapsed summary containing this item ID. */
  revealItemId?: string;
  /** Called with test ID when an action needs focus restoration after undo. */
  onSetUndoFocusTarget?: (testId: string | null) => void;
  /** Navigate to a specific item in another section (shadowed config owners). */
  onNavigateToItem?: (sectionId: string, itemId: string) => void;
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
  /** Whether the snapshot was collected with --include-unmanaged. */
  hasUnmanagedScan?: boolean;
  /** Per-package RPM upload row states for repo-less packages. */
  rpmRowStates?: Record<string, import("../api/types").RpmUploadRowState>;
  /** Called when user clicks the upload button on a repo-less package row. */
  onUploadClick?: (packageName: string) => void;
  /** Called when user removes a previously uploaded RPM. */
  onRemoveRpmUpload?: (packageName: string) => void;
  /** Callback to pin/unpin a single language package. */
  onSetPackagePin?: (ecosystem: string, envPath: string, pkg: string, pinned: boolean) => Promise<unknown> | void;
  /** Callback to bulk pin/unpin all packages in a language environment. */
  onSetBulkPackagePin?: (ecosystem: string, envPath: string, pinned: boolean) => Promise<unknown> | void;
  /** Expand and scroll to the Not carried group (the Export dialog's link). */
  revealNotCarried?: boolean;
  /** Called once the Not carried group has handled a reveal. */
  onNotCarriedRevealed?: () => void;
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
    include: isIncluded(pkg.entry.disposition),
    uploaded_version: pkg.entry.uploaded_version,
    installed_version: pkg.entry.uploaded_version ? pkg.entry.version : undefined,
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
  resetCounter = 0,
  historyChange,
  revealItemId,
  onSetUndoFocusTarget,
  onNavigateToItem,
  onToggleLangEnv,
  onToggleUnmanagedFile,
  onToggleUnmanagedGroup,
  onUnmanagedIncludeNone,
  onUnmanagedResetAll,
  isPending = false,
  rpmRowStates,
  onUploadClick,
  onRemoveRpmUpload,
  hasUnmanagedScan = false,
  onSetPackagePin,
  onSetBulkPackagePin,
  revealNotCarried = false,
  onNotCarriedRevealed,
}: MainContentProps) {
  const [filterText, setFilterText] = useState("");
  const toastIdRef = useRef(0);
  const [toasts, setToasts] = useState<ToastEntry[]>([]);
  const [pendingFocusTarget, setPendingFocusTarget] = useState<string | null>(
    null,
  );

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
    const RPM_ARCHES = [
      "x86_64",
      "noarch",
      "i686",
      "aarch64",
      "s390x",
      "ppc64le",
      "src",
    ];
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

  // A config entry whose path is also carried by Service Drop-ins is
  // Inventory: the session refuses SetInclude on it. Resolving the owner
  // here is what turns a dead "Inventory" badge into a pointer at the
  // control that replaced it.
  const shadowOwners = useMemo(
    () =>
      viewData
        ? buildShadowOwners(viewData.config_files, viewData.unit_dropins)
        : new Map(),
    [viewData],
  );

  // Language packages filtered by SectionSearch query:
  // matches environment path, package names, and ecosystem (case-insensitive)
  const langPkgs = viewData?.language_packages ?? [];
  const filteredLangPkgs = useMemo(() => {
    if (!filterText.trim()) return langPkgs;
    const q = filterText.toLowerCase();
    return langPkgs.filter(
      (env) =>
        env.path.toLowerCase().includes(q) ||
        env.ecosystem.toLowerCase().includes(q) ||
        env.packages.some((pkg) => pkg.name.toLowerCase().includes(q)),
    );
  }, [langPkgs, filterText]);

  // Unmanaged file groups filtered by SectionSearch query:
  // matches file path and file type (case-insensitive).
  // Groups with zero matching items are excluded; groups with matches
  // are narrowed to matched items and auto-expand.
  const unmanagedGroups = viewData?.unmanaged_files ?? [];
  const filteredUnmanagedGroups = useMemo(() => {
    if (!filterText.trim()) return unmanagedGroups;
    const q = filterText.toLowerCase();
    return unmanagedGroups
      .map((group) => ({
        ...group,
        items: group.items.filter(
          (item) =>
            item.path.toLowerCase().includes(q) ||
            item.provenance.file_type.toLowerCase().includes(q),
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

  // Requests in flight, by item: packages by "name.arch" (the package
  // toggle and the Not carried Include act on the same item), repos by
  // section id, groups by name. Each key is the whole of its op's item_id
  // key. "name.arch" is unambiguous because an RPM arch never contains a
  // dot, and it is the identifier the package rows already hand back.
  const packageGuard = usePendingGuard();
  const repoGuard = usePendingGuard();
  const groupGuard = usePendingGuard();
  // Guards for the sections that send their own requests. They live here,
  // not in the sections, because a section unmounts when the operator
  // switches away, and a guard that unmounted with it would come back
  // empty while its request was still in flight. MainContent itself stays
  // mounted for the whole session.
  const pinGuard = usePendingGuard();
  const containerGuard = usePendingGuard();
  const tunedGuard = usePendingGuard();
  const usersGuard = usePendingGuard();
  const serviceGuard = usePendingGuard();
  const scheduledGuard = usePendingGuard();
  // Failed Users requests, by username. Held here for the same reason as
  // the guards: the card that shows the failure unmounts on a section
  // switch, and a request that fails while the operator is away must
  // still be on the card when they come back. A failure ends with the
  // user record it was about: a reset replaces every record, an Undo or
  // Redo replaces the records it changed, and a user that leaves the view
  // is a new record if it comes back.
  const userFailures = useItemFailures<UserFailure>();
  const {
    clearAll: clearUserFailures,
    retain: retainUserFailures,
    end: endUserFailures,
  } = userFailures;
  useEffect(() => {
    clearUserFailures();
  }, [resetCounter, clearUserFailures]);
  useEffect(() => {
    if (historyChange) endUserFailures(historyChange.users);
  }, [historyChange, endUserFailures]);
  useEffect(() => {
    if (!viewData) return;
    retainUserFailures(
      new Set((viewData.users_groups_decisions ?? []).map((u) => u.name)),
    );
  }, [viewData, retainUserFailures]);
  // Failed generated-timer toggles, by generated name, kept here with
  // `scheduledGuard` so a failure that settles after a section switch is
  // still on the row when the operator returns. A reset ends every one, a
  // name that leaves the view ends its own, and an Undo or Redo ends all
  // of them, since either can replace the row's state.
  const generatedFailures = useItemFailures<string>();
  const {
    begin: beginGeneratedAttempt,
    clearAll: clearGeneratedFailures,
    retain: retainGeneratedFailures,
    end: endGeneratedFailures,
  } = generatedFailures;
  useEffect(() => {
    clearGeneratedFailures();
  }, [resetCounter, clearGeneratedFailures]);
  useEffect(() => {
    if (historyChange) {
      endGeneratedFailures(
        (viewData?.generated_timers ?? []).map((g) => g.name),
      );
    }
    // Keyed on the history step alone: a new view must not end them.
  }, [historyChange, endGeneratedFailures]);
  useEffect(() => {
    if (!viewData) return;
    retainGeneratedFailures(
      new Set(viewData.generated_timers.map((g) => g.name)),
    );
  }, [viewData, retainGeneratedFailures]);
  const runPackage = packageGuard.runGuarded;
  const runRepo = repoGuard.runGuarded;
  const runGroup = groupGuard.runGuarded;

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
          include: !isIncluded(pkg.entry.disposition),
        },
      };
      runPackage(nameArch, () =>
        applyOp(op)
          .then((updatedView) => onViewUpdate(updatedView))
          .catch((err) =>
            onMutationError(
              err instanceof Error ? err : new Error(String(err)),
            ),
          ),
      );
    },
    [viewData, runPackage, onViewUpdate, onMutationError],
  );

  // Not carried include: the ordinary package op, with the row's item_id
  // passed back verbatim (Refine projection contract).
  const handleNotCarriedInclude = useCallback(
    (row: NotCarriedRow) => {
      runPackage(notCarriedRowKey(row), () =>
        applyOp({
          op: "SetInclude",
          target: { item_id: row.item_id, include: true },
        })
          .then((updatedView) => onViewUpdate(updatedView))
          .catch((err) =>
            onMutationError(
              err instanceof Error ? err : new Error(String(err)),
            ),
          ),
      );
    },
    [runPackage, onViewUpdate, onMutationError],
  );

  // Repo toggle: build SetInclude op for repos
  const handleRepoToggle = useCallback(
    (sectionId: string, include: boolean) => {
      const repo = viewData?.repo_groups.find(
        (r) => r.section_id === sectionId,
      );
      if (!repo) return;
      const op: RefinementOp = {
        op: "SetInclude",
        target: {
          item_id: { kind: "Repo", key: { path: sectionId } },
          include,
        },
      };
      runRepo(sectionId, () =>
        applyOp(op)
          .then((updatedView) => onViewUpdate(updatedView))
          .catch((err) =>
            onMutationError(
              err instanceof Error ? err : new Error(String(err)),
            ),
          ),
      );
    },
    [viewData, runRepo, onViewUpdate, onMutationError],
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
      runGroup(groupName, () =>
        applyOp(op)
          .then((updatedView) => onViewUpdate(updatedView))
          .catch((err) =>
            onMutationError(
              err instanceof Error ? err : new Error(String(err)),
            ),
          ),
      );
    },
    [runGroup, onViewUpdate, onMutationError],
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

      runGroup(groupName, () =>
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
            onMutationError(
              err instanceof Error ? err : new Error(String(err)),
            ),
          ),
      );
    },
    [viewData, runGroup, onViewUpdate, onMutationError, onSetUndoFocusTarget],
  );

  const handleArrowDown = useCallback(() => {
    // Focus the first focusable item in the active section.
    // Tries decision items first, then group headers, then any element with tabindex=-1.
    const firstDecisionItem = document.querySelector(
      "[data-testid^='decision-item-']",
    ) as HTMLElement | null;
    if (firstDecisionItem) {
      firstDecisionItem.focus();
      return;
    }

    const firstGroup = document.querySelector(
      "[role='button'][aria-expanded]",
    ) as HTMLElement | null;
    if (firstGroup) {
      firstGroup.focus();
      return;
    }

    const firstFocusable = document.querySelector(
      "[tabindex='-1']",
    ) as HTMLElement | null;
    firstFocusable?.focus();
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
          pendingKeys={repoGuard.pendingKeys}
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
          pendingKeys={packageGuard.pendingKeys}
          pendingGroups={groupGuard.pendingKeys}
          rpmRowStates={rpmRowStates}
          onUploadClick={onUploadClick}
          onRemoveRpmUpload={onRemoveRpmUpload}
        />
        <NotCarriedGroup
          rows={viewData?.not_carried_packages ?? []}
          onInclude={handleNotCarriedInclude}
          isPending={isPending}
          pendingKeys={packageGuard.pendingKeys}
          reveal={revealNotCarried}
          onRevealed={onNotCarriedRevealed}
        />
        <div style={{ marginTop: "var(--pf-t--global--spacer--lg)" }}>
          <ExpandableSection
            toggleText="Version Changes"
            isExpanded={false}
            data-testid="version-changes-panel"
          >
            <VersionChangesTable
              entries={viewData?.version_changes ?? []}
              revealItemId={revealItemId}
            />
          </ExpandableSection>
        </div>
        {toasts.length > 0 && (
          <AlertGroup isToast isLiveRegion>
            {toasts.map((toast) => (
              <Alert
                key={toast.id}
                variant={toast.variant}
                title={toast.message}
                actionClose={
                  <AlertActionCloseButton
                    onClose={() => dismissToast(toast.id)}
                  />
                }
              />
            ))}
          </AlertGroup>
        )}
      </>
    );
  }

  if (activeSection === "configs" || activeSection === "config") {
    const hasFilter = filterText.trim().length > 0;
    const noResults = hasFilter && filteredConfigItems.length === 0;
    // "All excluded" is a statement about decisions the user made, so only
    // actionable entries can produce it. Advisories are not excluded — they
    // were never candidates — and a host whose only config findings are
    // advisories must still see them.
    const anyConfigIncluded = configItems.some((item) =>
      isIncluded(item.data.entry.disposition),
    );
    const anyAdvisoryOrInventory = configItems.some(
      (item) => !isToggleable(item.data.entry.disposition),
    );
    const allConfigsExcluded =
      configItems.length > 0 && !anyConfigIncluded && !anyAdvisoryOrInventory;
    if (allConfigsExcluded) {
      return (
        <EmptyState
          titleText="All configuration files excluded"
          headingLevel="h3"
          data-testid="config-all-excluded"
        >
          <EmptyStateBody>
            Use undo or re-include individual config files to restore them.
          </EmptyStateBody>
        </EmptyState>
      );
    }
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
            shadowOwners={shadowOwners}
            onViewUpdate={onViewUpdate}
            onMutationError={onMutationError}
            onNavigateToOwner={onNavigateToItem}
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
        pendingGuard={usersGuard}
        userFailures={userFailures}
      />
    );
  }

  // Services decision section — promoted from context to decision.
  if (activeSection === "units") {
    const services = viewData ? viewData.unit_states : [];
    const dropins = viewData ? viewData.unit_dropins : [];
    // No fallback for a view that lacks the field: the type requires it,
    // and an empty default would hide a server that stopped sending it.
    const mountFindings = viewData ? viewData.mount_findings : [];
    const shadowCount = dropins.filter(
      (d) => d.shadow_type === "full_shadow",
    ).length;
    const headerText =
      shadowCount > 0
        ? `${SECTION_LABELS.units} (${shadowCount} shadow override${shadowCount === 1 ? "" : "s"})`
        : SECTION_LABELS.units;

    return (
      <>
        <Content>
          <h2>{headerText}</h2>
        </Content>
        <ServiceSection
          services={services}
          dropins={dropins}
          mountFindings={mountFindings}
          onNavigateToConfig={(configPath) =>
            onNavigateToItem?.("config", `configs:${configPath}`)
          }
          onViewUpdate={onViewUpdate}
          onMutationError={onMutationError}
          pendingGuard={serviceGuard}
        />
      </>
    );
  }

  // System Tuning: the tuned decision rows and the read-only sysctl panel,
  // whose file paths open their Configuration Files rows.
  if (activeSection === "system_tuning") {
    if (!viewData) return <p>Section data not available.</p>;
    return (
      <>
        <Content>
          <h2>{SECTION_LABELS.system_tuning}</h2>
        </Content>
        <SystemTuningSection
          sysctl={viewData.sysctl}
          tuned={viewData.tuned}
          onViewUpdate={onViewUpdate}
          onMutationError={onMutationError}
          onNavigateToConfig={(configPath) =>
            onNavigateToItem?.("config", `configs:${configPath}`)
          }
          tunedGuard={tunedGuard}
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
          flatpakDeclined={
            viewData?.declined_generated.find((d) => d.row.row === "flatpak")
              ?.declined
          }
          onViewUpdate={onViewUpdate}
          onMutationError={onMutationError}
          pendingGuard={containerGuard}
        />
      </>
    );
  }


  // Language Packages decision section
  if (activeSection === "language_packages") {
    const hasFilter = filterText.trim().length > 0;
    const noResults = hasFilter && filteredLangPkgs.length === 0;
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
        {noResults ? (
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
            onSetPackagePin={onSetPackagePin}
            onSetBulkPackagePin={onSetBulkPackagePin}
            searchQuery={filterText}
            pinGuard={pinGuard}
          />
        )}
      </div>
    );
  }

  // Unmanaged Files decision section
  if (activeSection === "unmanaged_files") {
    const hasFilter = filterText.trim().length > 0;
    const noResults = hasFilter && filteredUnmanagedGroups.length === 0;
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
        {noResults ? (
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
            searchActive={hasFilter}
            hasUnmanagedScan={hasUnmanagedScan}
          />
        )}
      </div>
    );
  }

  // Context sections: compose, network, storage,
  // scheduled_tasks, non_rpm_software, kernel_boot, selinux
  const contextSectionIds = [
    "compose",
    "network",
    "storage",
    "scheduled_tasks",
    "non_rpm_software",
    "kernel_boot",
    "selinux",
  ];

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
        {section.has_ifcfg && section.ifcfg_note && (
          <Alert
            variant="info"
            isInline
            title="ifcfg Deprecation"
            style={{ marginBottom: "var(--pf-t--global--spacer--md)" }}
          >
            {section.ifcfg_note}
          </Alert>
        )}
        <ContextList
          key={lookupId}
          section={section}
          onNavigateToUnit={
            onNavigateToItem &&
            ((unit) => onNavigateToItem("units", unit))
          }
          declinedFor={
            lookupId === "scheduled_tasks"
              ? (item) =>
                  scheduledDeclined(viewData?.declined_generated ?? [], item)
              : undefined
          }
          controlFor={
            lookupId === "scheduled_tasks"
              ? (item) => {
                  const name = item.generated_timer;
                  const row = viewData?.generated_timers.find(
                    (g) => g.name === name,
                  );
                  if (name === undefined || !row) return undefined;
                  const key = JSON.stringify(["generated-timer", name]);
                  return {
                    include: row.include,
                    disabled:
                      row.locked || scheduledGuard.pendingKeys.has(key),
                    // Including is refused while a source cron job is
                    // excluded. Excluding is not: while any record of the
                    // name is still included, the toggle stays.
                    blockedReason:
                      !row.include && row.cron_job_excluded
                        ? `Excluded with its cron job ${row.cron_job_excluded}; include the cron job to carry this unit.`
                        : undefined,
                    failure: generatedFailures.failures.get(name),
                    onToggle: () => {
                      const include = !row.include;
                      const op: RefinementOp = {
                        op: "SetInclude",
                        target: {
                          item_id: { kind: "GeneratedTimer", key: { name } },
                          include,
                        },
                      };
                      scheduledGuard.runGuarded(key, () => {
                        const recordFailure = beginGeneratedAttempt(name);
                        return applyOp(op)
                          .then((updated) => onViewUpdate(updated))
                          .catch((err) => {
                            const error =
                              err instanceof Error ? err : new Error(String(err));
                            recordFailure(
                              `Could not ${include ? "include" : "exclude"} ${name}: ${error.message}`,
                            );
                            onMutationError(error);
                          });
                      });
                    },
                  };
                }
              : undefined
          }
        />
      </>
    );
  }

  return <p>Not yet implemented.</p>;
}
