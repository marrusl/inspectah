/**
 * These components take their pending guard from an owner that outlives
 * them (App or MainContent), so the guard survives the component
 * unmounting. Each wrapper here stands in for that owner, so a test can
 * render the component on its own with a real guard.
 */
import { usePendingGuard } from "../hooks/usePendingGuard";
import { Sidebar, type SidebarProps } from "../components/Sidebar";
import {
  LanguagePackageList,
  type LanguagePackageListProps,
} from "../components/LanguagePackageList";
import {
  ContainerSection,
  type ContainerSectionProps,
} from "../components/ContainerSection";
import {
  TunedSection,
  type TunedSectionProps,
} from "../components/TunedSection";
import {
  ServiceSection,
  type ServiceSectionProps,
} from "../components/ServiceSection";
import {
  SystemTuningSection,
  type SystemTuningSectionProps,
} from "../components/SystemTuningSection";

export function SidebarWithGuard(props: Omit<SidebarProps, "batchGuard">) {
  const guard = usePendingGuard();
  return <Sidebar {...props} batchGuard={guard} />;
}

export function LanguagePackageListWithGuard(
  props: Omit<LanguagePackageListProps, "pinGuard">,
) {
  const guard = usePendingGuard();
  return <LanguagePackageList {...props} pinGuard={guard} />;
}

export function ContainerSectionWithGuard(
  props: Omit<ContainerSectionProps, "pendingGuard">,
) {
  const guard = usePendingGuard();
  return <ContainerSection {...props} pendingGuard={guard} />;
}

export function TunedSectionWithGuard(
  props: Omit<TunedSectionProps, "pendingGuard">,
) {
  const guard = usePendingGuard();
  return <TunedSection {...props} pendingGuard={guard} />;
}

export function SystemTuningSectionWithGuard(
  props: Omit<SystemTuningSectionProps, "tunedGuard">,
) {
  const guard = usePendingGuard();
  return <SystemTuningSection {...props} tunedGuard={guard} />;
}

export function ServiceSectionWithGuard(
  props: Omit<ServiceSectionProps, "pendingGuard">,
) {
  const guard = usePendingGuard();
  return <ServiceSection {...props} pendingGuard={guard} />;
}
