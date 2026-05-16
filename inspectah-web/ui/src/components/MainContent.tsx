import { PageSection, Content, Skeleton } from "@patternfly/react-core";

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
}

export function MainContent({ activeSection, loading }: MainContentProps) {
  const label = SECTION_LABELS[activeSection] ?? activeSection;

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

  return (
    <PageSection>
      <Content>
        <h2>{label}</h2>
        <p>Not yet implemented.</p>
      </Content>
    </PageSection>
  );
}
