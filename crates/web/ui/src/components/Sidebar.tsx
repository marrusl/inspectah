import { useState, useEffect, useRef, useCallback, type ReactNode } from "react";
import {
  Nav,
  NavExpandable,
  NavItem,
  Badge,
  Content,
  Skeleton,
  Dropdown,
  DropdownItem,
  DropdownList,
  MenuToggle,
  Modal,
  ModalBody,
  ModalFooter,
  ModalHeader,
  Button,
} from "@patternfly/react-core";
import { EllipsisVIcon } from "@patternfly/react-icons";
import type { RefineStats } from "../api/types";
import type { ReferenceSection } from "../api/types";
import type { HealthResponse } from "../api/types";
import type { ViewResponse } from "../api/types";
import type { SectionGroupMeta } from "../api/types";

/** localStorage key for persisting group expand/collapse state. */
const EXPANDED_STORAGE_KEY = "inspectah-sidebar-expanded";

export interface SidebarProps {
  activeSection: string;
  onSelect: (sectionId: string) => void;
  stats: RefineStats | null;
  sections: ReferenceSection[] | null;
  health: HealthResponse | null;
  /** View data for counting decision items (services). */
  viewData?: ViewResponse | null;
  /** Number of user decisions (for the Users & Groups badge). */
  userDecisionCount?: number;
  /** Whether the snapshot was collected with --include-unmanaged. */
  hasUnmanagedScan?: boolean;
  /** Section groups from /api/groups. */
  groups?: SectionGroupMeta[] | null;
  /** When true, renders as a fixed overlay with backdrop. */
  overlay?: boolean;
  /** Called to close the overlay (Escape, backdrop click). */
  onClose?: () => void;
  /** Optional search component rendered at the top of the sidebar. */
  searchSlot?: ReactNode;
  /** Callback for batch-toggling all actionable items in a group. */
  onBatchToggle?: (groupSlug: string, include: boolean) => Promise<void>;
}

function sectionCount(
  sections: ReferenceSection[] | null,
  id: string,
): string | undefined {
  if (!sections) return "...";
  // "compose" sidebar entry maps to the "containers" context section from the backend
  const lookupId = id === "compose" ? "containers" : id;
  const sec = sections.find((s) => s.id === lookupId);
  if (!sec) return "0";
  const topLevel = sec.items.length;
  if (topLevel > 0) return String(topLevel);
  const subTotal = (sec.subsections ?? []).reduce(
    (sum, sub) => sum + sub.items.length,
    0,
  );
  return String(subTotal);
}

function decisionCount(
  stats: RefineStats | null,
  id: string,
  userDecisionCount?: number,
  viewData?: ViewResponse | null,
): string | undefined {
  if (id === "users_groups") {
    return userDecisionCount != null ? String(userDecisionCount) : "...";
  }
  if (id === "services") {
    if (!viewData) return "...";
    return String(viewData.service_states?.length ?? 0);
  }
  if (id === "containers") {
    if (!viewData) return "...";
    return String(
      (viewData.quadlets?.length ?? 0) + (viewData.flatpaks?.length ?? 0),
    );
  }
  if (id === "language_packages") {
    if (!viewData) return "...";
    return String(viewData.language_packages?.length ?? 0);
  }
  if (id === "unmanaged_files") {
    if (!viewData) return "...";
    return String(viewData.unmanaged_files?.length ?? 0);
  }
  if (!stats) return "...";
  const section = stats.sections?.find((s: { kind: string }) => {
    if (id === "packages") return s.kind === "package";
    if (id === "config" || id === "configs") return s.kind === "config";
    return false;
  });
  if (section) return String(section.total);
  return "0";
}

function getInitialExpanded(): Record<string, boolean> {
  try {
    const stored = localStorage.getItem(EXPANDED_STORAGE_KEY);
    if (stored) return JSON.parse(stored);
  } catch {
    /* ignore malformed localStorage */
  }
  return {};
}

export function Sidebar({
  activeSection,
  onSelect,
  stats,
  sections,
  health,
  viewData,
  userDecisionCount,
  groups,
  hasUnmanagedScan = false,
  overlay = false,
  onClose,
  searchSlot,
  onBatchToggle,
}: SidebarProps) {
  const sidebarRef = useRef<HTMLElement>(null);
  const [expandedGroups, setExpandedGroups] = useState<
    Record<string, boolean>
  >(getInitialExpanded);
  const [clearedState, setClearedState] = useState<Record<string, boolean>>({});

  // --- Batch-toggle state ---
  const [openMenuSlug, setOpenMenuSlug] = useState<string | null>(null);
  const [batchPending, setBatchPending] = useState<Record<string, boolean>>({});
  const [pkgConfirmSlug, setPkgConfirmSlug] = useState<string | null>(null);
  const [batchAnnouncement, setBatchAnnouncement] = useState("");

  // Auto-expand the group containing the active section when navigating.
  // This ensures keyboard (1-8) and search navigation reveal the target
  // section even if the user previously collapsed its parent group.
  useEffect(() => {
    if (!groups) return;
    const parentGroup = groups.find(
      (g) =>
        g.sections.length > 1 &&
        g.sections.some((s) => s.id === activeSection),
    );
    if (!parentGroup) return;

    setExpandedGroups((prev) => {
      // Default is expanded (true), so only act when explicitly collapsed
      if (prev[parentGroup.slug] ?? true) return prev;
      const next = { ...prev, [parentGroup.slug]: true };
      try {
        localStorage.setItem(EXPANDED_STORAGE_KEY, JSON.stringify(next));
      } catch {
        /* ignore */
      }
      return next;
    });
  }, [activeSection, groups]);

  // Focus trap and Escape handler for overlay mode
  useEffect(() => {
    if (!overlay) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        onClose?.();
        return;
      }

      // Focus trap: cycle through focusable elements
      if (e.key === "Tab") {
        const sidebar = sidebarRef.current;
        if (!sidebar) return;
        const focusable = sidebar.querySelectorAll<HTMLElement>(
          'a, button, [tabindex]:not([tabindex="-1"])',
        );
        if (focusable.length === 0) return;
        const first = focusable[0];
        const last = focusable[focusable.length - 1];
        if (e.shiftKey && document.activeElement === first) {
          e.preventDefault();
          last.focus();
        } else if (!e.shiftKey && document.activeElement === last) {
          e.preventDefault();
          first.focus();
        }
      }
    };

    document.addEventListener("keydown", handleKeyDown);

    // Focus first nav link on open
    const sidebar = sidebarRef.current;
    if (sidebar) {
      const firstLink = sidebar.querySelector<HTMLElement>("a, button");
      firstLink?.focus();
    }

    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [overlay, onClose]);

  const handleBackdropClick = useCallback(() => {
    onClose?.();
  }, [onClose]);

  const handleNavToggle = useCallback(
    (
      _event: React.MouseEvent<HTMLButtonElement>,
      toggledItem: { groupId: number | string; isExpanded: boolean },
    ) => {
      const slug = String(toggledItem.groupId);
      const isCollapsing = !toggledItem.isExpanded;

      setExpandedGroups((prev) => {
        const next = { ...prev, [slug]: toggledItem.isExpanded };
        try {
          localStorage.setItem(EXPANDED_STORAGE_KEY, JSON.stringify(next));
        } catch {
          /* ignore */
        }
        return next;
      });

      // Focus restoration: when collapsing a group that contains the
      // active section, move focus to the group heading button so the
      // user doesn't lose their place. aria-current stays on the
      // (now hidden) child NavItem and content keeps showing.
      if (isCollapsing && groups) {
        const group = groups.find((g) => g.slug === slug);
        if (group?.sections.some((s) => s.id === activeSection)) {
          requestAnimationFrame(() => {
            const groupEl = document.getElementById(slug);
            const headingBtn = groupEl?.querySelector("button");
            (headingBtn as HTMLElement | null)?.focus();
          });
        }
      }
    },
    [activeSection, groups],
  );

  // Clear batch-toggle announcement after screen readers have time to read it
  useEffect(() => {
    if (!batchAnnouncement) return;
    const timer = setTimeout(() => setBatchAnnouncement(""), 5000);
    return () => clearTimeout(timer);
  }, [batchAnnouncement]);

  /** Execute a batch toggle and announce the result. */
  const executeBatchToggle = useCallback(
    async (groupSlug: string, include: boolean, groupLabel: string) => {
      if (!onBatchToggle) return;
      setBatchPending((prev) => ({ ...prev, [groupSlug]: true }));
      try {
        await onBatchToggle(groupSlug, include);
        const verb = include ? "included in" : "excluded from";
        setBatchAnnouncement(`All items ${verb} ${groupLabel}`);
      } catch {
        setBatchAnnouncement(`Failed to update ${groupLabel}`);
      } finally {
        setBatchPending((prev) => ({ ...prev, [groupSlug]: false }));
      }
    },
    [onBatchToggle],
  );

  /** Handle a batch-toggle menu action, gating Packages exclude behind confirmation. */
  const handleBatchAction = useCallback(
    (groupSlug: string, include: boolean, groupLabel: string) => {
      setOpenMenuSlug(null);
      if (groupSlug === "packages-group" && !include) {
        setPkgConfirmSlug(groupSlug);
        return;
      }
      executeBatchToggle(groupSlug, include, groupLabel);
    },
    [executeBatchToggle],
  );

  /** Confirm Packages exclude from the safety dialog. */
  const handlePkgConfirm = useCallback(() => {
    const slug = pkgConfirmSlug;
    if (!slug) return;
    const group = groups?.find((g) => g.slug === slug);
    setPkgConfirmSlug(null);
    executeBatchToggle(slug, false, group?.label ?? slug);
  }, [pkgConfirmSlug, groups, executeBatchToggle]);

  /** ArrowDown on a group heading focuses the first child when expanded.
   *  This completes the heading row Tab order contract:
   *  group label -> kebab menu (if present) -> no more stops.
   *  ArrowDown moves into children; ArrowUp on first child returns to heading.
   *
   *  Ctrl+Shift+A / Ctrl+Shift+X: batch include/exclude when a triage group
   *  heading or singleton triage NavItem has focus. No-op on reference-only
   *  groups. */
  const handleSidebarKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      const target = e.target as HTMLElement;

      // --- Ctrl+Shift+A/X: batch toggle focused group ---
      if ((e.ctrlKey || e.metaKey) && e.shiftKey && !e.altKey) {
        const key = e.key.toLowerCase();
        if (key === "a" || key === "x") {
          const wrapper = target.closest<HTMLElement>(
            "[data-group-slug][data-group-actionable]",
          );
          if (wrapper) {
            // Only fire from group heading buttons (multi-section groups)
            // or from within singleton group wrappers. Child NavItem links
            // inside a multi-section group must not trigger batch actions.
            const isMultiSection = !wrapper.classList.contains(
              "inspectah-sidebar__group-wrapper--singleton",
            );
            if (isMultiSection && !target.matches("button[aria-expanded]")) {
              return;
            }
            e.preventDefault();
            const slug = wrapper.getAttribute("data-group-slug")!;
            const label = wrapper.getAttribute("data-group-label")!;
            handleBatchAction(slug, key === "a", label);
          }
        }
      }

      if (e.key === "ArrowDown" && target.tagName === "BUTTON") {
        // Only act on NavExpandable toggle buttons (identified by aria-expanded)
        if (target.getAttribute("aria-expanded") === "true") {
          const parentLi = target.closest("li");
          if (!parentLi) return;
          const firstChildLink = parentLi.querySelector(
            "section a, ul a",
          ) as HTMLElement | null;
          if (firstChildLink) {
            e.preventDefault();
            firstChildLink.focus();
          }
        }
      }

      if (e.key === "ArrowUp") {
        // From first child back to group heading
        const link = target.closest("a");
        if (!link) return;
        const section = link.closest("section");
        if (!section) return;
        const allLinks = section.querySelectorAll("a");
        if (allLinks.length > 0 && allLinks[0] === link) {
          const parentLi = section.closest("li");
          const headingBtn = parentLi?.querySelector(
            ":scope > button",
          ) as HTMLElement | null;
          if (headingBtn) {
            e.preventDefault();
            headingBtn.focus();
          }
        }
      }
    },
    [handleBatchAction],
  );

  /** Badge text for a given section. */
  const badge = useCallback(
    (sectionId: string, isTriage: boolean): string | undefined => {
      if (isTriage) {
        return decisionCount(stats, sectionId, userDecisionCount, viewData);
      }
      return sectionCount(sections, sectionId);
    },
    [stats, sections, userDecisionCount, viewData],
  );

  /** Track cleared state (count === "0") for triage sections with aria-live announcements. */
  useEffect(() => {
    if (!groups) return;
    const newClearedState: Record<string, boolean> = {};
    groups.forEach((group) => {
      group.sections.forEach((sec) => {
        if (sec.is_triage) {
          const count = decisionCount(stats, sec.id, userDecisionCount, viewData);
          const isCleared = count === "0";
          newClearedState[sec.id] = isCleared;
        }
      });
    });
    setClearedState(newClearedState);
  }, [groups, stats, userDecisionCount, viewData]);

  const renderNavContent = () => {
    if (!groups) {
      return (
        <>
          <Skeleton width="80%" />
          <Skeleton width="60%" />
          <Skeleton width="70%" />
        </>
      );
    }

    return groups.map((group) => {
      // Identity batch-toggle deferred — handler only covers groups, not individual users
      const hasKebab = group.has_actionable_sections && !!onBatchToggle && group.slug !== "identity";
      const isPending = batchPending[group.slug] ?? false;
      const isMenuOpen = openMenuSlug === group.slug;

      /** Render the kebab action menu for a triage group. */
      const kebabMenu = hasKebab ? (
        <span
          className="inspectah-sidebar__kebab"
          onClick={(e) => e.stopPropagation()}
          onKeyDown={(e) => e.stopPropagation()}
        >
          <Dropdown
            isOpen={isMenuOpen}
            onSelect={(_event, value) => {
              setOpenMenuSlug(null);
              if (value === "include") {
                handleBatchAction(group.slug, true, group.label);
              } else if (value === "exclude") {
                handleBatchAction(group.slug, false, group.label);
              }
            }}
            onOpenChange={(open) =>
              setOpenMenuSlug(open ? group.slug : null)
            }
            popperProps={{ position: "right" }}
            toggle={(toggleRef) => (
              <MenuToggle
                ref={toggleRef}
                variant="plain"
                onClick={(e) => {
                  e.stopPropagation();
                  e.preventDefault();
                  setOpenMenuSlug(isMenuOpen ? null : group.slug);
                }}
                isExpanded={isMenuOpen}
                isDisabled={isPending}
                aria-label={`Actions for ${group.label}`}
                data-testid={`kebab-${group.slug}`}
                className="inspectah-sidebar__kebab-toggle"
              >
                <EllipsisVIcon />
              </MenuToggle>
            )}
          >
            <DropdownList>
              <DropdownItem
                key="include"
                value="include"
                isDisabled={isPending}
                data-testid={`include-all-${group.slug}`}
              >
                Include all
              </DropdownItem>
              <DropdownItem
                key="exclude"
                value="exclude"
                isDisabled={isPending}
                data-testid={`exclude-all-${group.slug}`}
              >
                Exclude all
              </DropdownItem>
            </DropdownList>
          </Dropdown>
        </span>
      ) : null;

      if (group.sections.length === 1) {
        // Singleton group — render as a plain NavItem (no chevron)
        const sec = group.sections[0];
        const badgeText = badge(sec.id, sec.is_triage);
        const isCleared = sec.is_triage && clearedState[sec.id];

        const navItem = (
          <NavItem
            key={hasKebab ? undefined : sec.id}
            itemId={sec.id}
            isActive={activeSection === sec.id}
            aria-current={activeSection === sec.id ? "page" : undefined}
            onClick={() => onSelect(sec.id)}
          >
            {sec.label}{" "}
            {sec.is_triage ? (
              <Badge aria-label={`${badgeText} decisions`}>{badgeText}</Badge>
            ) : (
              <Badge isRead aria-label={`${badgeText} items`}>{badgeText}</Badge>
            )}
            {isCleared && (
              <span className="pf-v6-screen-reader" aria-live="polite">
                {sec.label}: 0 decisions remaining
              </span>
            )}
          </NavItem>
        );

        if (hasKebab) {
          return (
            <div
              key={sec.id}
              className="inspectah-sidebar__group-wrapper inspectah-sidebar__group-wrapper--singleton"
              data-group-slug={group.slug}
              data-group-actionable="true"
              data-group-label={group.label}
            >
              {navItem}
              {kebabMenu}
            </div>
          );
        }

        return navItem;
      }

      // Multi-section group — NavExpandable with child NavItems.
      // Batch-toggle menus are anchored per-section (not at the group
      // heading) so that "Include all / Exclude all" for config files
      // appears next to "Configuration Files", not "System Configuration".
      const groupIsActive = group.sections.some(
        (s) => activeSection === s.id,
      );

      /** Build a per-section kebab for a triage section inside a multi-section group. */
      const sectionKebab = (sec: { id: string; label: string; is_triage: boolean }) => {
        if (!sec.is_triage || !onBatchToggle) return null;
        const secMenuOpen = openMenuSlug === sec.id;
        const secPending = batchPending[sec.id] ?? false;
        return (
          <span
            className="inspectah-sidebar__kebab"
            onClick={(e) => e.stopPropagation()}
            onKeyDown={(e) => e.stopPropagation()}
          >
            <Dropdown
              isOpen={secMenuOpen}
              onSelect={(_event, value) => {
                setOpenMenuSlug(null);
                if (value === "include") {
                  handleBatchAction(sec.id, true, sec.label);
                } else if (value === "exclude") {
                  handleBatchAction(sec.id, false, sec.label);
                }
              }}
              onOpenChange={(open) =>
                setOpenMenuSlug(open ? sec.id : null)
              }
              popperProps={{ position: "right" }}
              toggle={(toggleRef) => (
                <MenuToggle
                  ref={toggleRef}
                  variant="plain"
                  onClick={(e) => {
                    e.stopPropagation();
                    e.preventDefault();
                    setOpenMenuSlug(secMenuOpen ? null : sec.id);
                  }}
                  isExpanded={secMenuOpen}
                  isDisabled={secPending}
                  aria-label={`Actions for ${sec.label}`}
                  data-testid={`kebab-${sec.id}`}
                  className="inspectah-sidebar__kebab-toggle"
                >
                  <EllipsisVIcon />
                </MenuToggle>
              )}
            >
              <DropdownList>
                <DropdownItem
                  key="include"
                  value="include"
                  isDisabled={secPending}
                  data-testid={`include-all-${sec.id}`}
                >
                  Include all
                </DropdownItem>
                <DropdownItem
                  key="exclude"
                  value="exclude"
                  isDisabled={secPending}
                  data-testid={`exclude-all-${sec.id}`}
                >
                  Exclude all
                </DropdownItem>
              </DropdownList>
            </Dropdown>
          </span>
        );
      };

      return (
        <NavExpandable
          key={group.slug}
          title={group.label}
          groupId={group.slug}
          isExpanded={expandedGroups[group.slug] ?? true}
          isActive={groupIsActive}
          id={group.slug}
        >
          {group.sections.map((sec) => {
            const badgeText = badge(sec.id, sec.is_triage);
            const isCleared = sec.is_triage && clearedState[sec.id];
            const secKebab = hasKebab ? sectionKebab(sec) : null;

            const navItem = (
              <NavItem
                key={secKebab ? undefined : sec.id}
                itemId={sec.id}
                isActive={activeSection === sec.id}
                aria-current={activeSection === sec.id ? "page" : undefined}
                onClick={() => onSelect(sec.id)}
              >
                {sec.label}{" "}
                {sec.is_triage ? (
                  <Badge aria-label={`${badgeText} decisions`}>{badgeText}</Badge>
                ) : (
                  <Badge isRead aria-label={`${badgeText} items`}>{badgeText}</Badge>
                )}
                {isCleared && (
                  <span className="pf-v6-screen-reader" aria-live="polite">
                    {sec.label}: 0 decisions remaining
                  </span>
                )}
              </NavItem>
            );

            if (secKebab) {
              return (
                <div
                  key={sec.id}
                  className="inspectah-sidebar__group-wrapper inspectah-sidebar__group-wrapper--singleton"
                  data-group-slug={sec.id}
                  data-group-actionable="true"
                  data-group-label={sec.label}
                >
                  {navItem}
                  {secKebab}
                </div>
              );
            }

            return navItem;
          })}
          {group.slug === "software" && !hasUnmanagedScan && (
            <Content
              component="small"
              className="inspectah-sidebar__hint"
              data-testid="unmanaged-hint"
            >
              Unmanaged files not scanned. Re-run with{" "}
              <code>--include-unmanaged</code> to review.
            </Content>
          )}
        </NavExpandable>
      );
    });
  };

  const sidebarContent = (
    <nav
      className={`inspectah-sidebar${overlay ? " inspectah-sidebar--overlay" : ""}`}
      aria-label="Section navigation"
      id={overlay ? "inspectah-sidebar-overlay" : undefined}
      ref={sidebarRef}
      onKeyDown={handleSidebarKeyDown}
    >
      <div className="inspectah-sidebar__host">
        {health ? (
          <>
            {health.host.hostname && (
              <Content component="p">
                <strong>{health.host.hostname}</strong>
              </Content>
            )}
            <Content component="small">
              {health.host.os_name} {health.host.os_version}
            </Content>
          </>
        ) : (
          <Skeleton width="80%" />
        )}
      </div>
      {searchSlot}
      <Nav aria-label="Sections" onToggle={handleNavToggle}>
        {renderNavContent()}
      </Nav>
      <div
        aria-live="assertive"
        className="pf-v6-screen-reader"
        data-testid="batch-toggle-announce"
      >
        {batchAnnouncement}
      </div>
    </nav>
  );

  const pkgConfirmModal = pkgConfirmSlug ? (
    <Modal
      isOpen
      onClose={() => setPkgConfirmSlug(null)}
      aria-label="Confirm package exclusion"
      variant="small"
      data-testid="packages-confirm-modal"
    >
      <ModalHeader title="Exclude all packages?" />
      <ModalBody>
        This will exclude all packages, producing an image missing critical
        runtime dependencies. Continue?
      </ModalBody>
      <ModalFooter>
        <Button
          variant="danger"
          onClick={handlePkgConfirm}
          data-testid="packages-confirm-yes"
        >
          Exclude all
        </Button>
        <Button variant="link" onClick={() => setPkgConfirmSlug(null)}>
          Cancel
        </Button>
      </ModalFooter>
    </Modal>
  ) : null;

  if (overlay) {
    return (
      <>
        <div
          className="inspectah-sidebar-backdrop"
          onClick={handleBackdropClick}
          data-testid="sidebar-backdrop"
        >
          <div onClick={(e) => e.stopPropagation()}>{sidebarContent}</div>
        </div>
        {pkgConfirmModal}
      </>
    );
  }

  return (
    <>
      {sidebarContent}
      {pkgConfirmModal}
    </>
  );
}
