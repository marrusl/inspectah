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
import type { ContextSection } from "../api/types";
import type { HealthResponse } from "../api/types";

/** Section IDs that represent decision sections (packages, configs). */
const DECISION_SECTIONS = [
  { id: "packages", label: "Packages" },
  { id: "configs", label: "Config Files" },
];

/** Section IDs from the snapshot context endpoint (read-only context). */
const CONTEXT_SECTIONS = [
  { id: "services", label: "Services" },
  { id: "containers", label: "Containers" },
  { id: "users_groups", label: "Users & Groups" },
  { id: "network", label: "Network" },
  { id: "storage", label: "Storage" },
  { id: "scheduled_tasks", label: "Scheduled Tasks" },
  { id: "non_rpm_software", label: "Non-RPM Software" },
  { id: "kernel_boot", label: "Kernel & Boot" },
  { id: "selinux", label: "SELinux" },
];

export interface SidebarProps {
  activeSection: string;
  onSelect: (sectionId: string) => void;
  stats: RefineStats | null;
  sections: ContextSection[] | null;
  health: HealthResponse | null;
  /** When true, renders as a fixed overlay with backdrop. */
  overlay?: boolean;
  /** Called to close the overlay (Escape, backdrop click). */
  onClose?: () => void;
  /** Optional search component rendered at the top of the sidebar. */
  searchSlot?: ReactNode;
}

function sectionCount(
  sections: ContextSection[] | null,
  id: string,
): string | undefined {
  if (!sections) return "...";
  const sec = sections.find((s) => s.id === id);
  return sec ? String(sec.items.length) : "0";
}

function decisionCount(
  stats: RefineStats | null,
  id: string,
): string | undefined {
  if (!stats) return "...";
  if (id === "packages") return String(stats.total_packages);
  if (id === "configs") return String(stats.total_configs);
  return "0";
}

export function Sidebar({
  activeSection,
  onSelect,
  stats,
  sections,
  health,
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
      {searchSlot}
      <Nav aria-label="Sections">
        <NavGroup title="Decisions">
          {DECISION_SECTIONS.map((sec) => (
            <NavItem
              key={sec.id}
              itemId={sec.id}
              isActive={activeSection === sec.id}
              aria-current={activeSection === sec.id ? "page" : undefined}
              onClick={() => onSelect(sec.id)}
            >
              {sec.label}{" "}
              <Badge isRead>{decisionCount(stats, sec.id)}</Badge>
            </NavItem>
          ))}
        </NavGroup>
        <NavGroup title="Context">
          {CONTEXT_SECTIONS.map((sec) => (
            <NavItem
              key={sec.id}
              itemId={sec.id}
              isActive={activeSection === sec.id}
              aria-current={activeSection === sec.id ? "page" : undefined}
              onClick={() => onSelect(sec.id)}
            >
              {sec.label}{" "}
              <Badge isRead>{sectionCount(sections, sec.id)}</Badge>
            </NavItem>
          ))}
        </NavGroup>
      </Nav>
      <div className="inspectah-sidebar__host">
        {health ? (
          <Content component="small">
            {health.host.hostname} &mdash; {health.host.os_name}{" "}
            {health.host.os_version}
          </Content>
        ) : (
          <Skeleton width="80%" />
        )}
      </div>
    </nav>
  );

  if (overlay) {
    return (
      <div className="inspectah-sidebar-backdrop" onClick={handleBackdropClick} data-testid="sidebar-backdrop">
        <div onClick={(e) => e.stopPropagation()}>
          {sidebarContent}
        </div>
      </div>
    );
  }

  return sidebarContent;
}
