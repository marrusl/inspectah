import { useMemo } from "react";
import { PageSection, Content, Skeleton } from "@patternfly/react-core";
import type { RefinedView, RefinedPackage, RefinedConfig } from "../api/types";
import { DecisionList } from "./DecisionList";
import type { DecisionItemKind } from "./DecisionItem";

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
  onViewUpdate: (view: RefinedView) => void;
  onMutationError: (err: Error) => void;
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
  onViewUpdate,
  onMutationError,
}: MainContentProps) {
  const label = SECTION_LABELS[activeSection] ?? activeSection;

  const packageItems = useMemo(
    () => (viewData ? toPackageItems(viewData.packages) : []),
    [viewData],
  );
  const configItems = useMemo(
    () => (viewData ? toConfigItems(viewData.config_files) : []),
    [viewData],
  );

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
        <DecisionList
          items={packageItems}
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
        <DecisionList
          items={configItems}
          sectionLabel="Config Files"
          onViewUpdate={onViewUpdate}
          onMutationError={onMutationError}
        />
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
