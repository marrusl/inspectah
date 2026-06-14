import { useEffect, useRef, useCallback, type ReactNode } from "react";
import {
  Nav,
  NavGroup,
  NavItem,
  Badge,
  Content,
  Skeleton,
} from "@patternfly/react-core";
import type { RefineStats } from "../api/types";
import type { ReferenceSection } from "../api/types";
import type { HealthResponse } from "../api/types";
import type { ViewResponse } from "../api/types";

/** Section IDs that represent review sections (packages, configs, users, services, containers). */
const REVIEW_SECTIONS = [
  { id: "packages", label: "Packages" },
  { id: "configs", label: "Config Files" },
  { id: "users_groups", label: "Users & Groups" },
  { id: "services", label: "Services" },
  { id: "containers", label: "Containers" },
  { id: "system_tuning", label: "System Tuning" },
];

/** Section IDs from the snapshot context endpoint (read-only reference). */
const REFERENCE_SECTIONS = [
  { id: "version_changes", label: "Version Changes" },
  { id: "compose", label: "Compose" },
  { id: "network", label: "Network" },
  { id: "storage", label: "Storage" },
  { id: "scheduled_tasks", label: "Scheduled Tasks" },
  { id: "non_rpm_software", label: "Non-RPM Software" },
  { id: "kernel_boot", label: "Kernel & Boot" },
  { id: "selinux", label: "Security & Access Control" },
];

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
  if (id === "system_tuning") {
    if (!viewData) return "...";
    return String(
      (viewData.sysctls?.length ?? 0) + (viewData.tuned?.length ?? 0),
    );
  }
  if (!stats) return "...";
  const section = stats.sections?.find((s: { kind: string }) => {
    if (id === "packages") return s.kind === "package";
    if (id === "configs") return s.kind === "config";
    return false;
  });
  if (section) return String(section.total);
  return "0";
}

export function Sidebar({
  activeSection,
  onSelect,
  stats,
  sections,
  health,
  viewData,
  userDecisionCount,
  overlay = false,
  onClose,
  searchSlot,
}: SidebarProps) {
  const sidebarRef = useRef<HTMLElement>(null);

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
      <Nav aria-label="Sections">
        <NavGroup title="Review">
          {REVIEW_SECTIONS.map((sec) => (
            <NavItem
              key={sec.id}
              itemId={sec.id}
              isActive={activeSection === sec.id}
              aria-current={activeSection === sec.id ? "page" : undefined}
              onClick={() => onSelect(sec.id)}
            >
              {sec.label}{" "}
              <Badge isRead>
                {decisionCount(stats, sec.id, userDecisionCount, viewData)}
              </Badge>
            </NavItem>
          ))}
        </NavGroup>
        <NavGroup title="Reference">
          {REFERENCE_SECTIONS.map((sec) => (
            <NavItem
              key={sec.id}
              itemId={sec.id}
              isActive={activeSection === sec.id}
              aria-current={activeSection === sec.id ? "page" : undefined}
              onClick={() => onSelect(sec.id)}
            >
              {sec.label} <Badge isRead>{sectionCount(sections, sec.id)}</Badge>
            </NavItem>
          ))}
        </NavGroup>
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
