import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen } from "@testing-library/react";
// Rendered with a real pending guard, as its owner supplies one in the app.
import { ServiceSectionWithGuard as ServiceSection } from "../../test-utils/withPendingGuard";
import type {
  ServiceDecisionDto,
  DropInDecisionDto,
  MountFindingDto,
  TriageTag,
} from "../../api/types";

const mockFetch = vi.fn();
beforeEach(() => {
  mockFetch.mockReset();
  vi.stubGlobal("fetch", mockFetch);
});
afterEach(() => {
  vi.restoreAllMocks();
});

const TRIAGE: TriageTag = {
  triage: { mode: "single_host", site: null },
  primary_reason: "service_drop_in_present",
  annotations: [],
};

const ORPHAN_PATH = "/etc/systemd/system/sshd.service.d/override.conf";

function makeService(
  overrides: Partial<ServiceDecisionDto> = {},
): ServiceDecisionDto {
  return {
    unit: "httpd.service",
    kind: "service",
    fragment: "vendor",
    triage: TRIAGE,
    include: true,
    locked: false,
    current_state: "enabled",
    required_config: [],
    data_paths: [],
    ...overrides,
  };
}

function makeDropin(
  overrides: Partial<DropInDecisionDto> = {},
): DropInDecisionDto {
  return {
    unit: "sshd.service",
    path: ORPHAN_PATH,
    triage: TRIAGE,
    include: true,
    locked: false,
    config_paths: [overrides.path ?? ORPHAN_PATH],
    ...overrides,
  };
}

const defaultProps = {
  services: [] as ServiceDecisionDto[],
  dropins: [] as DropInDecisionDto[],
  mountFindings: [] as MountFindingDto[],
  manualLinks: [],
  undeterminedInstalls: [],
  onViewUpdate: vi.fn(),
  onMutationError: vi.fn(),
};

// A unit whose enable state matches its preset produces no state_changes
// entry, so its drop-in has no parent service row to nest under. That is
// the common case, not an edge case: without this group the drop-in — the
// row that owns a shadowed config's decision — renders nowhere at all.
describe("ServiceSection drop-ins without a parent service row", () => {
  it("renders a drop-in whose unit has no state change", () => {
    render(
      <ServiceSection
        {...defaultProps}
        services={[makeService({ unit: "httpd.service" })]}
        dropins={[makeDropin()]}
      />,
    );
    expect(screen.getByTestId(`dropin-item-${ORPHAN_PATH}`)).toBeInTheDocument();
    expect(
      screen.getByTestId("dropin-orphan-group-sshd.service"),
    ).toBeInTheDocument();
  });

  it("states that the group's overrides have no unit-state row", () => {
    render(<ServiceSection {...defaultProps} dropins={[makeDropin()]} />);
    const group = screen.getByTestId("dropin-orphans");
    expect(group).toHaveTextContent("These overrides have no unit-state decision row.");
    // A record's drop-in lands here too, so the note claims nothing about presets.
    expect(group.textContent).not.toMatch(/preset/i);
  });

  it("renders orphan drop-ins when there are no services at all", () => {
    render(<ServiceSection {...defaultProps} dropins={[makeDropin()]} />);
    expect(screen.getByTestId(`dropin-item-${ORPHAN_PATH}`)).toBeInTheDocument();
    expect(
      screen.queryByText("No unit differs from its preset, and no drop-in was found."),
    ).toBeNull();
  });

  it("keeps the empty state when there are neither services nor drop-ins", () => {
    render(<ServiceSection {...defaultProps} />);
    expect(
      screen.getByText("No unit differs from its preset, and no drop-in was found."),
    ).toBeInTheDocument();
  });

  it("does not duplicate a drop-in that already nests under its service", () => {
    const nested = makeDropin({ unit: "httpd.service" });
    render(
      <ServiceSection
        {...defaultProps}
        services={[makeService({ unit: "httpd.service" })]}
        dropins={[nested]}
      />,
    );
    expect(screen.getAllByTestId(`dropin-item-${ORPHAN_PATH}`)).toHaveLength(1);
    expect(screen.queryByTestId("dropin-orphans")).toBeNull();
  });

  it("offers a live toggle on an orphan drop-in", () => {
    render(<ServiceSection {...defaultProps} dropins={[makeDropin()]} />);
    const toggle = screen.getByRole("checkbox", {
      name: `Toggle ${ORPHAN_PATH}`,
    });
    expect(toggle).toBeEnabled();
    expect(toggle).toBeChecked();
  });
});
