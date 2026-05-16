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
}: SidebarProps) {
  return (
    <nav className="inspectah-sidebar" aria-label="Section navigation">
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
}
