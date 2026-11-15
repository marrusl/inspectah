import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
// Rendered with a real pending guard, as its owner supplies one in the app.
import { ServiceSectionWithGuard as ServiceSection } from "../../test-utils/withPendingGuard";
import type {
  ServiceDecisionDto,
  DropInDecisionDto,
  MountFindingDto,
  ViewResponse,
  TriageTag,
} from "../../api/types";
import { mockStats } from "../../test-utils/mockStats";

// --- Mock fetch globally ---
const mockFetch = vi.fn();
beforeEach(() => {
  mockFetch.mockReset();
  vi.stubGlobal("fetch", mockFetch);
});
afterEach(() => {
  vi.restoreAllMocks();
});

// --- Test data helpers ---

function makeTriage(bucket: "baseline" | "site" | "investigate"): TriageTag {
  const reasonMap: Record<string, string> = {
    baseline: "service_baseline_match",
    site: "service_non_default_state",
    investigate: "service_unknown_origin",
  };
  return {
    triage: { mode: "single_host" as const, [bucket]: null },
    primary_reason: reasonMap[bucket] as TriageTag["primary_reason"],
    annotations: [],
  };
}

function makeService(
  overrides: Partial<ServiceDecisionDto> = {},
): ServiceDecisionDto {
  return {
    unit: "httpd.service",
    kind: "service",
    fragment: "vendor",
    triage: makeTriage("site"),
    include: true,
    locked: false,
    current_state: "enabled",
    ...overrides,
  };
}

function makeDropin(
  overrides: Partial<DropInDecisionDto> = {},
): DropInDecisionDto {
  return {
    unit: "httpd.service",
    path: "/etc/systemd/system/httpd.service.d/override.conf",
    triage: makeTriage("site"),
    include: true,
    locked: false,
    ...overrides,
  };
}

const MOCK_STATS = mockStats({
  sections: [
    { kind: "package", total: 0, included: 0, excluded: 0 },
    { kind: "config", total: 0, included: 0, excluded: 0 },
  ],
});

const MOCK_VIEW: ViewResponse = {
  packages: [],
  config_files: [],
  containerfile_preview: "",
  stats: MOCK_STATS,
  generation: 1,
  repo_groups: [],
  version_changes: [],
  unit_states: [],
  unit_dropins: [],
  mount_findings: [],
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
};

const defaultProps = {
  services: [] as ServiceDecisionDto[],
  dropins: [] as DropInDecisionDto[],
  mountFindings: [] as MountFindingDto[],
  onViewUpdate: vi.fn(),
  onMutationError: vi.fn(),
};

// --- Tests ---

describe("ServiceSection", () => {
  it("renders empty state when no services", () => {
    render(<ServiceSection {...defaultProps} />);
    expect(screen.getByTestId("service-section")).toBeInTheDocument();
    expect(screen.getByText(/no service state changes/i)).toBeInTheDocument();
  });

  it("renders service rows with unit names", () => {
    const services = [
      makeService({ unit: "httpd.service" }),
      makeService({ unit: "sshd.service" }),
    ];
    render(<ServiceSection {...defaultProps} services={services} />);

    expect(
      screen.getByTestId("service-item-httpd.service"),
    ).toBeInTheDocument();
    expect(screen.getByTestId("service-item-sshd.service")).toBeInTheDocument();
  });

  it("renders drop-in children indented under their parent", () => {
    const services = [makeService({ unit: "httpd.service" })];
    const dropins = [
      makeDropin({
        unit: "httpd.service",
        path: "/etc/systemd/system/httpd.service.d/override.conf",
      }),
    ];
    render(
      <ServiceSection
        {...defaultProps}
        services={services}
        dropins={dropins}
      />,
    );

    const parentGroup = screen.getByTestId("service-group-httpd.service");
    expect(
      within(parentGroup).getByTestId(
        "dropin-item-/etc/systemd/system/httpd.service.d/override.conf",
      ),
    ).toBeInTheDocument();
  });

  it("shows owning_package in parentheses when present", () => {
    const services = [
      makeService({ unit: "httpd.service", owning_package: "httpd" }),
    ];
    render(<ServiceSection {...defaultProps} services={services} />);
    expect(screen.getByText("(httpd)")).toBeInTheDocument();
  });

  it("shows triage badge for non-routine services", () => {
    const services = [
      makeService({
        unit: "custom.service",
        triage: makeTriage("investigate"),
      }),
    ];
    render(<ServiceSection {...defaultProps} services={services} />);
    expect(screen.getByText("Unknown Origin")).toBeInTheDocument();
  });

  it("does not show badge for baseline (routine) services", () => {
    const services = [
      makeService({
        unit: "baseline.service",
        triage: makeTriage("baseline"),
      }),
    ];
    render(<ServiceSection {...defaultProps} services={services} />);
    const row = screen.getByTestId("service-item-baseline.service");
    // No Label badge should be rendered
    expect(within(row).queryByRole("gridcell", { name: /badge/i })).toBeNull();
  });

  it("shows current state and preset when default_state is present", () => {
    const services = [
      makeService({
        unit: "example.service",
        current_state: "disabled",
        default_state: "enable",
      }),
    ];
    render(<ServiceSection {...defaultProps} services={services} />);
    const stateLabel = screen.getByTestId("service-state-example.service");
    expect(stateLabel).toBeInTheDocument();
    expect(stateLabel).toHaveTextContent("disabled (preset: enable)");
  });

  it("shows current state without preset when default_state is absent", () => {
    const services = [
      makeService({
        unit: "example.service",
        current_state: "enabled",
        // no default_state
      }),
    ];
    render(<ServiceSection {...defaultProps} services={services} />);
    const stateLabel = screen.getByTestId("service-state-example.service");
    expect(stateLabel).toHaveTextContent("enabled");
    expect(stateLabel).not.toHaveTextContent("preset");
  });
});

describe("ServiceSection parent-child cascade", () => {
  it("excluding parent disables drop-in checkboxes visually", () => {
    const services = [makeService({ unit: "httpd.service", include: false })];
    const dropins = [
      makeDropin({
        unit: "httpd.service",
        path: "/etc/systemd/system/httpd.service.d/override.conf",
        include: true,
      }),
    ];
    render(
      <ServiceSection
        {...defaultProps}
        services={services}
        dropins={dropins}
      />,
    );

    const dropinRow = screen.getByTestId(
      "dropin-item-/etc/systemd/system/httpd.service.d/override.conf",
    );
    const checkbox = within(dropinRow).getByRole("checkbox");
    expect(checkbox).toBeDisabled();
  });

  it("shows 'Service excluded' badge on drop-ins when parent is excluded", () => {
    const services = [makeService({ unit: "httpd.service", include: false })];
    const dropins = [
      makeDropin({
        unit: "httpd.service",
        path: "/etc/systemd/system/httpd.service.d/override.conf",
      }),
    ];
    render(
      <ServiceSection
        {...defaultProps}
        services={services}
        dropins={dropins}
      />,
    );

    expect(
      screen.getByTestId(
        "dropin-excluded-badge-/etc/systemd/system/httpd.service.d/override.conf",
      ),
    ).toBeInTheDocument();
    expect(screen.getByText("Service excluded")).toBeInTheDocument();
  });

  it("drop-in checkboxes are enabled when parent is included", () => {
    const services = [makeService({ unit: "httpd.service", include: true })];
    const dropins = [
      makeDropin({
        unit: "httpd.service",
        path: "/etc/systemd/system/httpd.service.d/override.conf",
        include: true,
      }),
    ];
    render(
      <ServiceSection
        {...defaultProps}
        services={services}
        dropins={dropins}
      />,
    );

    const dropinRow = screen.getByTestId(
      "dropin-item-/etc/systemd/system/httpd.service.d/override.conf",
    );
    const checkbox = within(dropinRow).getByRole("checkbox");
    expect(checkbox).not.toBeDisabled();
  });

  it("excluding individual drop-in while parent stays included", async () => {
    const onViewUpdate = vi.fn();
    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: () => Promise.resolve(MOCK_VIEW),
    });

    const services = [makeService({ unit: "httpd.service", include: true })];
    const dropins = [
      makeDropin({
        unit: "httpd.service",
        path: "/etc/systemd/system/httpd.service.d/override.conf",
        include: true,
      }),
    ];
    render(
      <ServiceSection
        {...defaultProps}
        services={services}
        dropins={dropins}
        onViewUpdate={onViewUpdate}
      />,
    );

    const dropinRow = screen.getByTestId(
      "dropin-item-/etc/systemd/system/httpd.service.d/override.conf",
    );
    const checkbox = within(dropinRow).getByRole("checkbox");
    await userEvent.click(checkbox);

    expect(mockFetch).toHaveBeenCalledWith(
      "/api/op",
      expect.objectContaining({
        method: "POST",
      }),
    );
  });

  it("reduced opacity on drop-in rows when parent excluded", () => {
    const services = [makeService({ unit: "httpd.service", include: false })];
    const dropins = [
      makeDropin({
        unit: "httpd.service",
        path: "/etc/systemd/system/httpd.service.d/override.conf",
      }),
    ];
    render(
      <ServiceSection
        {...defaultProps}
        services={services}
        dropins={dropins}
      />,
    );

    const dropinRow = screen.getByTestId(
      "dropin-item-/etc/systemd/system/httpd.service.d/override.conf",
    );
    expect(dropinRow.style.opacity).toBe("0.55");
  });

  it("Space on disabled drop-in is a no-op", async () => {
    const services = [makeService({ unit: "httpd.service", include: false })];
    const dropins = [
      makeDropin({
        unit: "httpd.service",
        path: "/etc/systemd/system/httpd.service.d/override.conf",
      }),
    ];
    render(
      <ServiceSection
        {...defaultProps}
        services={services}
        dropins={dropins}
      />,
    );

    const dropinRow = screen.getByTestId(
      "dropin-item-/etc/systemd/system/httpd.service.d/override.conf",
    );
    dropinRow.focus();
    await userEvent.keyboard(" ");

    // No fetch should have been called
    expect(mockFetch).not.toHaveBeenCalled();
  });
});

describe("ServiceSection toggle operations", () => {
  it("toggling a service sends SetInclude op", async () => {
    const onViewUpdate = vi.fn();
    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: () => Promise.resolve(MOCK_VIEW),
    });

    const services = [makeService({ unit: "httpd.service", include: true })];
    render(
      <ServiceSection
        {...defaultProps}
        services={services}
        onViewUpdate={onViewUpdate}
      />,
    );

    const row = screen.getByTestId("service-item-httpd.service");
    const checkbox = within(row).getByRole("checkbox");
    await userEvent.click(checkbox);

    expect(mockFetch).toHaveBeenCalledTimes(1);
    const callBody = JSON.parse(mockFetch.mock.calls[0][1].body);
    expect(callBody.op).toBe("SetInclude");
    expect(callBody.target.item_id).toEqual({
      kind: "Unit",
      key: { unit: "httpd.service" },
    });
    expect(callBody.target.include).toBe(false);
  });

  it("toggling a drop-in sends SetInclude op with DropIn kind", async () => {
    const onViewUpdate = vi.fn();
    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: () => Promise.resolve(MOCK_VIEW),
    });

    const services = [makeService({ unit: "httpd.service", include: true })];
    const dropins = [
      makeDropin({
        unit: "httpd.service",
        path: "/etc/systemd/system/httpd.service.d/override.conf",
        include: true,
      }),
    ];
    render(
      <ServiceSection
        {...defaultProps}
        services={services}
        dropins={dropins}
        onViewUpdate={onViewUpdate}
      />,
    );

    const dropinRow = screen.getByTestId(
      "dropin-item-/etc/systemd/system/httpd.service.d/override.conf",
    );
    const checkbox = within(dropinRow).getByRole("checkbox");
    await userEvent.click(checkbox);

    expect(mockFetch).toHaveBeenCalledTimes(1);
    const callBody = JSON.parse(mockFetch.mock.calls[0][1].body);
    expect(callBody.op).toBe("SetInclude");
    expect(callBody.target.item_id).toEqual({
      kind: "DropIn",
      key: { path: "/etc/systemd/system/httpd.service.d/override.conf" },
    });
    expect(callBody.target.include).toBe(false);
  });
});

describe("ServiceSection full shadow on the drop-in row", () => {
  const SHADOW_PATH = "/etc/systemd/system/httpd.service";
  const PLAIN_PATH = "/etc/systemd/system/httpd.service.d/override.conf";

  function shadowDropin(
    overrides: Partial<DropInDecisionDto> = {},
  ): DropInDecisionDto {
    return makeDropin({
      path: SHADOW_PATH,
      shadow_type: "full_shadow",
      shadow_rationale: "This service is critical for bootc",
      ...overrides,
    });
  }

  it("renders the badge and rationale on a full shadow and neither on a sibling drop-in", () => {
    render(
      <ServiceSection
        {...defaultProps}
        services={[makeService()]}
        dropins={[
          shadowDropin(),
          makeDropin({ path: PLAIN_PATH, shadow_type: "drop_in" }),
        ]}
      />,
    );

    const shadowRow = screen.getByTestId(`dropin-item-${SHADOW_PATH}`);
    expect(within(shadowRow).getByText("Shadow override")).toBeInTheDocument();
    expect(
      within(shadowRow).getByTestId(`shadow-rationale-dropin-${SHADOW_PATH}`),
    ).toHaveTextContent("This service is critical for bootc");

    const plainRow = screen.getByTestId(`dropin-item-${PLAIN_PATH}`);
    expect(within(plainRow).queryByText("Shadow override")).toBeNull();
    expect(
      within(plainRow).queryByTestId(`shadow-rationale-dropin-${PLAIN_PATH}`),
    ).toBeNull();
  });

  it("shows a full shadow on a unit at its preset, which has no state row", () => {
    render(
      <ServiceSection
        {...defaultProps}
        services={[]}
        dropins={[shadowDropin({ unit: "sshd.service" })]}
      />,
    );

    const orphans = screen.getByTestId("dropin-orphans");
    const row = within(orphans).getByTestId(`dropin-item-${SHADOW_PATH}`);
    expect(
      within(row).getByTestId(`shadow-badge-dropin-${SHADOW_PATH}`),
    ).toHaveTextContent("Shadow override");
    expect(screen.queryByTestId("service-item-sshd.service")).toBeNull();
  });

  it("renders amber border-left for a full shadow", () => {
    render(
      <ServiceSection
        {...defaultProps}
        dropins={[shadowDropin({ triage: makeTriage("baseline") })]}
      />,
    );

    const row = screen.getByTestId(`dropin-item-${SHADOW_PATH}`);
    expect(row.style.borderLeft).toContain(
      "var(--pf-t--global--color--status--warning--default)",
    );
  });

  it("uses background color when the triage border conflicts with the shadow border", () => {
    render(
      <ServiceSection
        {...defaultProps}
        dropins={[shadowDropin({ triage: makeTriage("investigate") })]}
      />,
    );

    const row = screen.getByTestId(`dropin-item-${SHADOW_PATH}`);
    expect(row.style.borderLeft).toContain(
      "var(--pf-t--global--color--status--danger--default)",
    );
    expect(row.style.backgroundColor).toContain(
      "var(--pf-t--global--color--status--warning--default)",
    );
  });

  it("renders the rationale with its id and wires the checkbox to it", () => {
    render(<ServiceSection {...defaultProps} dropins={[shadowDropin()]} />);

    const row = screen.getByTestId(`dropin-item-${SHADOW_PATH}`);
    const rationale = within(row).getByTestId(
      `shadow-rationale-dropin-${SHADOW_PATH}`,
    );
    expect(rationale.id).not.toBe("");
    expect(within(row).getByRole("checkbox")).toHaveAttribute(
      "aria-describedby",
      rationale.id,
    );
  });

  it.each([
    ["null", null],
    ["absent", undefined],
  ])(
    "leaves the checkbox undescribed when a full shadow's rationale is %s",
    (_label, rationale) => {
      render(
        <ServiceSection
          {...defaultProps}
          dropins={[shadowDropin({ shadow_rationale: rationale })]}
        />,
      );

      const row = screen.getByTestId(`dropin-item-${SHADOW_PATH}`);
      // Still a full shadow: the badge renders.
      expect(
        within(row).getByTestId(`shadow-badge-dropin-${SHADOW_PATH}`),
      ).toHaveTextContent("Shadow override");
      expect(
        within(row).queryByTestId(`shadow-rationale-dropin-${SHADOW_PATH}`),
      ).toBeNull();
      expect(within(row).getByRole("checkbox")).not.toHaveAttribute(
        "aria-describedby",
      );
    },
  );

  it("badge order is triage then shadow when not locked", () => {
    render(
      <ServiceSection
        {...defaultProps}
        dropins={[shadowDropin({ triage: makeTriage("investigate") })]}
      />,
    );

    const row = screen.getByTestId(`dropin-item-${SHADOW_PATH}`);
    const badgeTexts = within(row)
      .getAllByRole("gridcell")
      .map((b) => b.textContent);
    const triageIdx = badgeTexts.indexOf("Unknown Origin");
    const shadowIdx = badgeTexts.indexOf("Shadow override");
    expect(triageIdx).toBeGreaterThanOrEqual(0);
    expect(triageIdx).toBeLessThan(shadowIdx);
  });

  it("shows shadow before locked when the drop-in is locked (no triage badge)", () => {
    render(
      <ServiceSection
        {...defaultProps}
        dropins={[
          shadowDropin({
            locked: true,
            attention_reason: "Cannot change",
            triage: makeTriage("investigate"),
          }),
        ]}
      />,
    );

    const row = screen.getByTestId(`dropin-item-${SHADOW_PATH}`);
    expect(within(row).queryByText("Unknown Origin")).toBeNull();
    const badgeTexts = within(row)
      .getAllByRole("gridcell")
      .map((b) => b.textContent);
    const shadowIdx = badgeTexts.indexOf("Shadow override");
    const lockedIdx = badgeTexts.indexOf("Cannot change");
    expect(shadowIdx).toBeGreaterThanOrEqual(0);
    expect(shadowIdx).toBeLessThan(lockedIdx);
  });
});
