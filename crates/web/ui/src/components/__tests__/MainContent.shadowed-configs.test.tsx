import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MainContent } from "../MainContent";
import type {
  ConfigShadow,
  DropInDecisionDto,
  RefinedConfig,
  SysctlDecisionDto,
  TriageTag,
  ViewResponse,
} from "../../api/types";
import { mockStats } from "../../test-utils/mockStats";

const TRIAGE: TriageTag = {
  triage: { mode: "single_host", site: null },
  primary_reason: "config_modified",
  annotations: [],
};

const DROPIN_PATH = "/etc/systemd/system/sshd.service.d/override.conf";
const SYSCTL_PATH = "/etc/sysctl.d/90-network.conf";

function shadowedConfig(path: string, shadow: ConfigShadow): RefinedConfig {
  return {
    entry: {
      path,
      kind: "unowned",
      category: "other",
      content: "",
      rpm_va_flags: null,
      package: null,
      diff_against_rpm: null,
      disposition: { kind: "inventory" },
      tie: false,
      tie_winner: false,
      aggregate: null,
      shadowed_by: shadow,
    },
    triage: TRIAGE,
  };
}

function view(overrides: Partial<ViewResponse> = {}): ViewResponse {
  return {
    packages: [],
    config_files: [],
    containerfile_preview: "",
    stats: mockStats(),
    generation: 0,
    repo_groups: [],
    version_changes: [],
    service_states: [],
    service_dropins: [],
    quadlets: [],
    flatpaks: [],
    sysctls: [],
    tuned: [],
    users_groups_decisions: [],
    package_groups: [],
    session_is_sensitive: false,
    ...overrides,
  };
}

const DROPIN: DropInDecisionDto = {
  unit: "sshd.service",
  path: DROPIN_PATH,
  triage: TRIAGE,
  include: false,
};

const SYSCTL: SysctlDecisionDto = {
  key: "net.ipv4.ip_forward",
  runtime: "1",
  default: "0",
  source: SYSCTL_PATH,
  triage: TRIAGE,
  include: true,
};

const props = {
  activeSection: "configs",
  loading: false,
  sections: null,
  onViewUpdate: vi.fn(),
  onMutationError: vi.fn(),
  sectionSearchOpen: false,
  onSectionSearchClose: vi.fn(),
};

// The whole chain: viewData -> buildShadowOwners -> DecisionList -> the row.
// A break anywhere in the prop drilling silently restores the bare badge.
describe("shadowed config rows in the configs section", () => {
  beforeEach(() => {
    vi.stubGlobal(
      "fetch",
      vi.fn(() =>
        Promise.resolve({ ok: true, json: () => Promise.resolve({ ids: [] }) }),
      ),
    );
  });
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("names the Service Drop-ins owner and its state instead of 'Inventory'", () => {
    render(
      <MainContent
        {...props}
        viewData={view({
          config_files: [shadowedConfig(DROPIN_PATH, "services")],
          service_dropins: [DROPIN],
        })}
      />,
    );
    expect(screen.queryByTestId("inventory-badge")).toBeNull();
    expect(screen.getByTestId("shadow-owner-badge")).toHaveTextContent(
      "Controlled in Service Drop-ins",
    );
    expect(screen.getByTestId("shadow-owner-state")).toHaveTextContent(
      "Owner excluded",
    );
  });

  it("routes to the owning drop-in row", async () => {
    const onNavigateToItem = vi.fn();
    render(
      <MainContent
        {...props}
        viewData={view({
          config_files: [shadowedConfig(DROPIN_PATH, "services")],
          service_dropins: [DROPIN],
        })}
        onNavigateToItem={onNavigateToItem}
      />,
    );
    await userEvent.click(screen.getByTestId("shadow-owner-link"));
    expect(onNavigateToItem).toHaveBeenCalledWith("services", DROPIN_PATH);
  });

  it("names the sysctl owner and routes to the first absorbed key", async () => {
    const onNavigateToItem = vi.fn();
    render(
      <MainContent
        {...props}
        viewData={view({
          config_files: [shadowedConfig(SYSCTL_PATH, "sysctl")],
          sysctls: [SYSCTL],
        })}
        onNavigateToItem={onNavigateToItem}
      />,
    );
    expect(screen.getByTestId("shadow-owner-badge")).toHaveTextContent(
      "Merged into sysctl settings",
    );
    expect(screen.getByTestId("shadow-owner-state")).toHaveTextContent(
      "1 of 1 keys included",
    );
    await userEvent.click(screen.getByTestId("shadow-owner-link"));
    expect(onNavigateToItem).toHaveBeenCalledWith(
      "kernel_boot",
      "net.ipv4.ip_forward",
    );
  });

  it("leaves an unshadowed config row untouched", () => {
    const plain = shadowedConfig("/etc/hosts", "services");
    plain.entry.disposition = { kind: "actionable", include: true };
    delete plain.entry.shadowed_by;
    render(
      <MainContent {...props} viewData={view({ config_files: [plain] })} />,
    );
    expect(screen.queryByTestId("shadow-owner-badge")).toBeNull();
    expect(screen.getByRole("checkbox")).toBeInTheDocument();
  });
});
