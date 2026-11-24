import { useState, useMemo, useCallback, useEffect, useRef } from "react";
import type { ReactNode } from "react";
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
  MachineStateRowDto,
} from "../api/types";
import { ApiError } from "../api/types";
import { isIncluded, isToggleable } from "../api/disposition";
import { applyOp, fetchView, ungroupGroup } from "../api/client";
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
import { PackageList, PACKAGE_ROW_SELECTOR } from "./PackageList";
import type { PackageListPackage } from "./PackageList";
import { LanguagePackageList } from "./LanguagePackageList";
import { UnmanagedFileList } from "./UnmanagedFileList";
import { SystemTuningSection } from "./SystemTuningSection";
import { NotCarriedGroup, notCarriedRowKey } from "./NotCarriedGroup";
import {
  MachineStateGroup,
  filterMachineStateRows,
  machineStateItemId,
} from "./MachineStateGroup";
import type { MachineStateFailure } from "./MachineStateGroup";
import { TmpfilesOwnerNotice } from "./TmpfilesOwnerNotice";
import { buildShadowOwners } from "./shadowOwner";
import { repoFailure, type RepoFailure } from "./repoControl";
import { Content } from "@patternfly/react-core";

/** Maps section IDs to human-readable heading text (mirrors Sidebar labels). */
const SECTION_LABELS: Record<string, string> = {
  packages: "Packages",
  config: "Configuration Files",
  configs: "Config Files",
  units: "Systemd units",
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
   * Set to a new object after every successful Undo or Redo, naming the
   * users whose record that step changed, possibly none. Their kept
   * failures were about the record the step replaced, so they are
   * cleared; other users keep theirs. Every generated-timer failure ends
   * on any step.
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
  /** Expand and scroll to the machine-state block (the Export dialog's link). */
  revealMachineState?: boolean;
  /** Called once the machine-state block has handled a reveal. */
  onMachineStateRevealed?: () => void;
}

/** The prefix of a 422 the session sends for a file it will not carry. */
const NOT_CARRIED_PREFIX = "not carried:";

/**
 * The prefix of a 422 the session sends for a decision the row already
 * holds: the view the request came from was stale.
 */
const NOTHING_TO_CHANGE_PREFIX = "nothing to change:";

/** The server's reason after a 422 `prefix`, without its final period. */
function refusalReason(err: ApiError, prefix: string): string {
  return err.message.slice(prefix.length).trim().replace(/\.$/, "");
}

/**
 * What a machine-state row shows when its request failed. A 422
 * `not carried:` or `nothing to change:` is the server stating a fact
 * about the file, so it is worded as one; anything else says the change
 * did not happen.
 */
export function machineStateFailure(err: unknown): MachineStateFailure {
  if (err instanceof ApiError && err.status === 422) {
    if (err.message.startsWith(NOT_CARRIED_PREFIX)) {
      const why = refusalReason(err, NOT_CARRIED_PREFIX);
      return {
        kind: "not_carried",
        text: `inspectah does not carry this file: ${why}. Nothing was changed.`,
      };
    }
    if (err.message.startsWith(NOTHING_TO_CHANGE_PREFIX)) {
      const why = refusalReason(err, NOTHING_TO_CHANGE_PREFIX);
      return {
        kind: "nothing_to_change",
        text: `Nothing to change: ${why}.`,
      };
    }
  }
  const message = err instanceof Error ? err.message : String(err);
  return { kind: "failed", text: `The change was not saved: ${message}` };
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
  revealMachineState = false,
  onMachineStateRevealed,
}: MainContentProps) {
  const [filterText, setFilterText] = useState("");
  const toastIdRef = useRef(0);
  const [toasts, setToasts] = useState<ToastEntry[]>([]);
  const [pendingFocusTarget, setPendingFocusTarget] = useState<string | null>(
    null,
  );

  // Clear stale filter when switching sections or when global search
  // navigates within same section. Adjusted during render, so the section
  // a navigation reveals a row in renders with the query already cleared
  // and plans its reveal against that (units: spec § 6.11).
  const [filterClearedFor, setFilterClearedFor] = useState({ activeSection, filterClearCounter });
  if (
    filterClearedFor.activeSection !== activeSection ||
    filterClearedFor.filterClearCounter !== filterClearCounter
  ) {
    setFilterClearedFor({ activeSection, filterClearCounter });
    setFilterText("");
  }

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

  // Version Changes in the packages section, collapsed until opened.
  const [versionChangesExpanded, setVersionChangesExpanded] = useState(false);

  // The units section's result count and its first-row focus (§ 6.12):
  // the section computes both, since only it knows its pins and groups.
  const [unitsResultCount, setUnitsResultCount] = useState(0);
  const focusFirstUnitRow = useRef<(() => void) | null>(null);
  const registerFocusFirstUnitRow = useCallback((focus: (() => void) | null) => {
    focusFirstUnitRow.current = focus;
  }, []);
  const clearFilterText = useCallback(() => setFilterText(""), []);
  // The units section's search box, which "Clear filter" hands its focus
  // to (§ 6.13); registered by the box while it is open.
  const focusUnitsSearch = useRef<(() => void) | null>(null);
  const registerFocusUnitsSearch = useCallback((focus: (() => void) | null) => {
    focusUnitsSearch.current = focus;
  }, []);
  const handleFocusUnitsSearch = useCallback(() => focusUnitsSearch.current?.(), []);

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

  // Machine-state rows, by path: a class path has no config row, and a
  // link to it reveals its machine-state row instead.
  const machineStatePaths = useMemo(
    () => new Set((viewData?.machine_state ?? []).map((r) => r.path)),
    [viewData],
  );

  // The units section links a required_config path to its config row, or
  // to its machine-state row.
  const configPaths = useMemo(
    () =>
      new Set([
        ...(viewData ? viewData.config_files.map((c) => c.entry.path) : []),
        ...machineStatePaths,
      ]),
    [viewData, machineStatePaths],
  );

  // The item id a link to a Configuration path reveals, and the hash the
  // units section's link writes.
  const configItemId = useCallback(
    (configPath: string) =>
      machineStatePaths.has(configPath)
        ? machineStateItemId(configPath)
        : `configs:${configPath}`,
    [machineStatePaths],
  );
  const navigateToConfig = useCallback(
    (configPath: string) =>
      onNavigateToItem?.("config", configItemId(configPath)),
    [onNavigateToItem, configItemId],
  );

  // A config entry whose path is also carried by Service Drop-ins is
  // Inventory: the session refuses SetInclude on it. Resolving the owner
  // here is what turns a dead "Inventory" badge into a pointer at the
  // control that replaced it.
  const shadowOwners = useMemo(
    () =>
      viewData
        ? buildShadowOwners(
            viewData.config_files,
            viewData.unit_dropins,
            viewData.repo_owned,
          )
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
  // Machine-state include and exclude, keyed `machine-state:<path>` for
  // both, with their failures kept here for the same reason as the users'
  // (process-docs/skills/web-ui-direct-request-guard.md). A reset or any
  // Undo or Redo can replace a row's state, so either ends every failure;
  // a row that leaves the view ends its own.
  const machineStateGuard = usePendingGuard();
  const machineStateFailures = useItemFailures<MachineStateFailure>();
  const {
    begin: beginMachineStateAttempt,
    clear: clearMachineStateFailure,
    clearAll: clearMachineStateFailures,
    retain: retainMachineStateFailures,
  } = machineStateFailures;
  useEffect(() => {
    clearMachineStateFailures();
  }, [resetCounter, clearMachineStateFailures]);
  useEffect(() => {
    if (historyChange) clearMachineStateFailures();
  }, [historyChange, clearMachineStateFailures]);
  useEffect(() => {
    if (!viewData) return;
    retainMachineStateFailures(
      new Set((viewData.machine_state ?? []).map((r) => machineStateItemId(r.path))),
    );
  }, [viewData, retainMachineStateFailures]);
  // Repo switch failures, by section id, kept here with `repoGuard` for
  // the same reason as the machine-state ones: a reset or any Undo or Redo
  // can replace a row's state, so either ends every failure, and a repo
  // that leaves the view ends its own.
  const repoFailures = useItemFailures<RepoFailure>();
  const {
    begin: beginRepoAttempt,
    clear: clearRepoFailure,
    clearAll: clearRepoFailures,
    retain: retainRepoFailures,
  } = repoFailures;
  useEffect(() => {
    clearRepoFailures();
  }, [resetCounter, clearRepoFailures]);
  useEffect(() => {
    if (historyChange) clearRepoFailures();
  }, [historyChange, clearRepoFailures]);
  useEffect(() => {
    if (!viewData) return;
    retainRepoFailures(
      new Set((viewData.repo_groups ?? []).map((r) => r.section_id)),
    );
  }, [viewData, retainRepoFailures]);
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

  // Machine-state include or exclude: the ordinary config op, with the
  // row's item_id passed back verbatim. A row without one is never sent.
  const runMachineState = machineStateGuard.runGuarded;
  const handleMachineStateSetInclude = useCallback(
    (row: MachineStateRowDto, include: boolean) => {
      const itemId = row.item_id;
      if (itemId === undefined) return;
      const key = machineStateItemId(row.path);
      runMachineState(key, () => {
        const recordFailure = beginMachineStateAttempt(key);
        return applyOp({
          op: "SetInclude",
          target: { item_id: itemId, include },
        })
          .then((updatedView) => onViewUpdate(updatedView))
          .catch((err: unknown) => {
            const failure = machineStateFailure(err);
            recordFailure(failure);
            onMutationError(
              err instanceof Error ? err : new Error(String(err)),
            );
            // The row already held the decision, so this view is stale:
            // fetch the server's, which the row then shows.
            if (failure.kind === "nothing_to_change") {
              return fetchView()
                .then((freshView) => onViewUpdate(freshView))
                .catch((refetchErr: unknown) =>
                  onMutationError(
                    refetchErr instanceof Error
                      ? refetchErr
                      : new Error(String(refetchErr)),
                  ),
                );
            }
          });
      });
    },
    [runMachineState, beginMachineStateAttempt, onViewUpdate, onMutationError],
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
      runRepo(sectionId, () => {
        const recordFailure = beginRepoAttempt(sectionId);
        return applyOp(op)
          .then((updatedView) => onViewUpdate(updatedView))
          .catch((err: unknown) => {
            const failure = repoFailure(err);
            recordFailure(failure);
            onMutationError(
              err instanceof Error ? err : new Error(String(err)),
            );
            // The switch posts only a direction the server accepts, so a
            // `nothing to change:` means this view was stale: fetch the
            // server's, which the row then shows.
            if (failure.kind === "nothing_to_change") {
              return fetchView()
                .then((freshView) => onViewUpdate(freshView))
                .catch((refetchErr: unknown) =>
                  onMutationError(
                    refetchErr instanceof Error
                      ? refetchErr
                      : new Error(String(refetchErr)),
                  ),
                );
            }
          });
      });
    },
    [viewData, runRepo, beginRepoAttempt, onViewUpdate, onMutationError],
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
    // The units section registers its own first shown row while mounted.
    if (focusFirstUnitRow.current) {
      focusFirstUnitRow.current();
      return;
    }

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

  // Packages: ArrowDown from the search box goes to the first shown row,
  // since typing in the box leaves focus there.
  const focusFirstPackageRow = useCallback(() => {
    document
      .querySelector<HTMLElement>('[data-testid="package-list"]')
      ?.querySelector<HTMLElement>(PACKAGE_ROW_SELECTOR)
      ?.focus();
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
            onArrowDown={focusFirstPackageRow}
            resultCount={0}
          />
        )}
        <RepoBar
          repos={viewData?.repo_groups ?? []}
          onToggle={handleRepoToggle}
          pendingKeys={repoGuard.pendingKeys}
          repoOwned={viewData?.repo_owned ?? []}
          failures={repoFailures.failures}
          onDismissFailure={clearRepoFailure}
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
            isExpanded={versionChangesExpanded}
            onToggle={(_event, expanded) => setVersionChangesExpanded(expanded)}
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
    const machineStateRows = viewData?.machine_state ?? [];
    const machineStateMatches = hasFilter
      ? filterMachineStateRows(machineStateRows, filterText).length
      : machineStateRows.length;
    // "All excluded" is a statement about decisions the user made, so only
    // actionable entries can produce it. Advisories are not excluded — they
    // were never candidates — and a host whose only config findings are
    // advisories must still see them. Machine-state rows are not in
    // `config_files`, so they never count toward it.
    const anyConfigIncluded = configItems.some((item) =>
      isIncluded(item.data.entry.disposition),
    );
    const anyAdvisoryOrInventory = configItems.some(
      (item) => !isToggleable(item.data.entry.disposition),
    );
    const allConfigsExcluded =
      configItems.length > 0 && !anyConfigIncluded && !anyAdvisoryOrInventory;
    // The decision list's rows on screen: none while the all-excluded
    // state stands in for the list.
    const listedConfigCount = allConfigsExcluded ? 0 : filteredConfigItems.length;
    // The block's matches count with the decision list's, and "no
    // results" means neither has one.
    const noResults =
      hasFilter && listedConfigCount === 0 && machineStateMatches === 0;
    // One layout for every state: the heading, the search and the block
    // stay where they are, and an empty state replaces only the decision
    // list. A different tree for "all excluded" would drop the search
    // while its query still filters the block, and remount the block,
    // collapsing it.
    let decisionArea: ReactNode = null;
    if (allConfigsExcluded) {
      decisionArea = (
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
    } else if (noResults) {
      decisionArea = (
        <EmptyState titleText="No items match your search" headingLevel="h3">
          <EmptyStateBody>
            <Button variant="link" onClick={() => setFilterText("")}>
              Clear filter
            </Button>
          </EmptyStateBody>
        </EmptyState>
      );
    } else if (filteredConfigItems.length > 0 || machineStateMatches === 0) {
      // With only block rows to show, the list's own "No items" empty
      // state would contradict the block under it.
      decisionArea = (
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
            resultCount={
              listedConfigCount + (hasFilter ? machineStateMatches : 0)
            }
          />
        )}
        <TmpfilesOwnerNotice
          rows={viewData?.tmpfiles_missing_owners}
          remedy={viewData?.tmpfiles_owner_remedy}
        />
        {decisionArea}
        {/* At the foot of the section whatever the list area shows. */}
        <MachineStateGroup
          rows={machineStateRows}
          stepCount={viewData?.machine_state_steps.length ?? 0}
          filterText={filterText}
          onSetInclude={handleMachineStateSetInclude}
          pendingKeys={machineStateGuard.pendingKeys}
          failures={machineStateFailures.failures}
          onDismissFailure={clearMachineStateFailure}
          revealItemId={revealItemId}
          reveal={revealMachineState}
          onRevealed={onMachineStateRevealed}
        />
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
    const manualLinks = viewData ? viewData.manual_links : [];
    const undeterminedInstalls = viewData ? viewData.undetermined_installs : [];
    // No shadow-override count: a full shadow has its own label on its row (§ 6.14).
    return (
      <>
        <Content>
          <h2>{SECTION_LABELS.units}</h2>
        </Content>
        {sectionSearchOpen && (
          <SectionSearch
            value={filterText}
            onChange={setFilterText}
            onClose={handleSearchClose}
            onArrowDown={handleArrowDown}
            resultCount={unitsResultCount}
            registerFocus={registerFocusUnitsSearch}
          />
        )}
        <ServiceSection
          services={services}
          dropins={dropins}
          mountFindings={mountFindings}
          manualLinks={manualLinks}
          undeterminedInstalls={undeterminedInstalls}
          defaultTarget={viewData?.default_target}
          onNavigateToConfig={navigateToConfig}
          revealItemId={revealItemId}
          onNavigateToItem={onNavigateToItem}
          filterText={filterText}
          onResultCount={setUnitsResultCount}
          onClearQuery={clearFilterText}
          onFocusSearch={handleFocusUnitsSearch}
          registerFocusFirstRow={registerFocusFirstUnitRow}
          configPaths={configPaths}
          configItemId={configItemId}
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
          onNavigateToConfig={navigateToConfig}
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
