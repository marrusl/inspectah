import { useState, useEffect, useRef, useCallback, type ReactNode } from "react";
import {
  Nav,
  NavExpandable,
  NavItem,
  Badge,
  Content,
  Skeleton,
} from "@patternfly/react-core";
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
}: SidebarProps) {
  const sidebarRef = useRef<HTMLElement>(null);
  const [expandedGroups, setExpandedGroups] = useState<
    Record<string, boolean>
  >(getInitialExpanded);

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
      setExpandedGroups((prev) => {
        const next = { ...prev, [slug]: toggledItem.isExpanded };
        try {
          localStorage.setItem(EXPANDED_STORAGE_KEY, JSON.stringify(next));
        } catch {
          /* ignore */
        }
        return next;
      });
    },
    [],
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
      if (group.sections.length === 1) {
        // Singleton group — render as a plain NavItem (no chevron)
        const sec = group.sections[0];
        return (
          <NavItem
            key={sec.id}
            itemId={sec.id}
            isActive={activeSection === sec.id}
            aria-current={activeSection === sec.id ? "page" : undefined}
            onClick={() => onSelect(sec.id)}
          >
            {sec.label}{" "}
            <Badge isRead>{badge(sec.id, sec.is_triage)}</Badge>
          </NavItem>
        );
      }

      // Multi-section group — NavExpandable with child NavItems
      const groupIsActive = group.sections.some(
        (s) => activeSection === s.id,
      );
      return (
        <NavExpandable
          key={group.slug}
          title={group.label}
          groupId={group.slug}
          isExpanded={expandedGroups[group.slug] ?? true}
          isActive={groupIsActive}
          id={group.slug}
        >
          {group.sections.map((sec) => (
            <NavItem
              key={sec.id}
              itemId={sec.id}
              isActive={activeSection === sec.id}
              aria-current={activeSection === sec.id ? "page" : undefined}
              onClick={() => onSelect(sec.id)}
            >
              {sec.label}{" "}
              <Badge isRead>{badge(sec.id, sec.is_triage)}</Badge>
            </NavItem>
          ))}
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
    </nav>
  );

  if (overlay) {
    return (
      <div
        className="inspectah-sidebar-backdrop"
        onClick={handleBackdropClick}
        data-testid="sidebar-backdrop"
      >
        <div onClick={(e) => e.stopPropagation()}>{sidebarContent}</div>
      </div>
    );
  }

  return sidebarContent;
}
