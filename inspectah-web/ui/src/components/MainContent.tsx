import { useState, useMemo, useCallback } from "react";
import { PageSection, Content, Skeleton } from "@patternfly/react-core";
import type { RefinedView, RefinedPackage, RefinedConfig, ContextSection } from "../api/types";
import { DecisionList } from "./DecisionList";
import type { DecisionItemKind } from "./DecisionItem";
import { ContextList } from "./ContextList";
import { SectionSearch } from "./SectionSearch";

/** Section ID to human-readable label. */
const SECTION_LABELS: Record<string, string> = {
  packages: "Packages",
  configs: "Config Files",
  services: "Services",
  containers: "Containers",
  users_groups: "Users & Groups",
  network: "Network",
  storage: "Storage",
  scheduled_tasks: "Scheduled Tasks",
  non_rpm_software: "Non-RPM Software",
  kernel_boot: "Kernel & Boot",
  selinux: "SELinux",
};

export interface MainContentProps {
  activeSection: string;
  loading: boolean;
  viewData: RefinedView | null;
  sections: ContextSection[] | null;
  onViewUpdate: (view: RefinedView) => void;
  onMutationError: (err: Error) => void;
  sectionSearchOpen: boolean;
  onSectionSearchClose: () => void;
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
}: MainContentProps) {
  const label = SECTION_LABELS[activeSection] ?? activeSection;
  const [filterText, setFilterText] = useState("");

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
            resultCount={filteredPackageItems.length}
          />
        )}
        <DecisionList
          items={filteredPackageItems}
          sectionLabel="Packages"
          onViewUpdate={onViewUpdate}
          onMutationError={onMutationError}
        />
      </PageSection>
    );
  }

  if (activeSection === "configs") {
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
        <DecisionList
          items={filteredConfigItems}
          sectionLabel="Config Files"
          onViewUpdate={onViewUpdate}
          onMutationError={onMutationError}
        />
      </PageSection>
    );
  }

  // Context sections: services, containers, users_groups, network, storage,
  // scheduled_tasks, non_rpm_software, kernel_boot, selinux
  const contextSectionIds = [
    "services",
    "containers",
    "users_groups",
    "network",
    "storage",
    "scheduled_tasks",
    "non_rpm_software",
    "kernel_boot",
    "selinux",
  ];

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
