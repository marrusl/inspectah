import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MainContent } from "../MainContent";
import type {
  ConfigShadow,
  DropInDecisionDto,
  RefinedConfig,
  TriageTag,
  ViewResponse,
} from "../../api/types";
import { mockStats } from "../../test-utils/mockStats";
import { rowKey } from "../unitOutcome";

const TRIAGE: TriageTag = {
  triage: { mode: "single_host", site: null },
  primary_reason: "config_modified",
  annotations: [],
};

const DROPIN_PATH = "/etc/systemd/system/sshd.service.d/override.conf";

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
    repo_owned: [],
    version_changes: [],
    unit_states: [],
    unit_dropins: [],
    mount_findings: [],
    manual_links: [],
    undetermined_installs: [],
    quadlets: [],
    flatpaks: [],
    declined_generated: [],
    generated_timers: [],
    sysctl: { state: "unavailable", reason: "not collected" },
    tuned: [],
    users_groups_decisions: [],
    package_groups: [],
    session_is_sensitive: false,
    not_carried_packages: [],
    not_carried_notice: null,
    machine_state: [],
    machine_state_steps: [],
    machine_state_notices: [],
    ...overrides,
  };
}

const DROPIN: DropInDecisionDto = {
  unit: "sshd.service",
  path: DROPIN_PATH,
  config_paths: [DROPIN_PATH],
  triage: TRIAGE,
  include: false,
  locked: false,
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

  it("names the Systemd units owner and its state instead of 'Inventory'", () => {
    render(
      <MainContent
        {...props}
        viewData={view({
          config_files: [shadowedConfig(DROPIN_PATH, "units")],
          unit_dropins: [DROPIN],
        })}
      />,
    );
    expect(screen.queryByTestId("inventory-badge")).toBeNull();
    expect(screen.getByTestId("shadow-owner-badge")).toHaveTextContent(
      "Controlled in Systemd units",
    );
    expect(document.body.textContent).not.toContain("Controlled in Service Drop-ins");
    expect(screen.getByTestId("shadow-owner-state")).toHaveTextContent(
      "Owner excluded",
    );
    const row = screen.getByTestId("shadow-owner-badge").closest('[data-testid^="decision-item-"]');
    expect(row).not.toBeNull();
    expect(
      within(row as HTMLElement).getByTestId(/^shadow-owner-explanation-/),
    ).toHaveTextContent(
      "The Systemd units row for this same path carries the file into the image. Decide it there.",
    );
    expect(row!.textContent).not.toContain("Service Drop-ins");
  });

  it("routes to the owning drop-in row", async () => {
    const onNavigateToItem = vi.fn();
    render(
      <MainContent
        {...props}
        viewData={view({
          config_files: [shadowedConfig(DROPIN_PATH, "units")],
          unit_dropins: [DROPIN],
        })}
        onNavigateToItem={onNavigateToItem}
      />,
    );
    await userEvent.click(screen.getByTestId("shadow-owner-link"));
    expect(onNavigateToItem).toHaveBeenCalledWith(
      "units",
      rowKey({ kind: "dropin", data: DROPIN }),
    );
  });

  it("leaves an unshadowed config row untouched", () => {
    const plain = shadowedConfig("/etc/hosts", "units");
    plain.entry.disposition = { kind: "actionable", include: true };
    delete plain.entry.shadowed_by;
    render(
      <MainContent {...props} viewData={view({ config_files: [plain] })} />,
    );
    expect(screen.queryByTestId("shadow-owner-badge")).toBeNull();
    expect(screen.getByRole("checkbox")).toBeInTheDocument();
  });
});
