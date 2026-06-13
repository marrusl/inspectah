import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ServiceSection } from "../ServiceSection";
import type {
  ServiceDecisionDto,
  DropInDecisionDto,
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
    triage: makeTriage("site"),
    include: true,
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
  service_states: [],
  service_dropins: [],
  quadlets: [],
  flatpaks: [],
  sysctls: [],
  tuned: [],
  users_groups_decisions: [],
  package_groups: [],
  session_is_sensitive: false,
};

const defaultProps = {
  services: [] as ServiceDecisionDto[],
  dropins: [] as DropInDecisionDto[],
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

  it("shows preset label when default_state is present", () => {
    const services = [
      makeService({
        unit: "example.service",
        default_state: "disable",
      }),
    ];
    render(<ServiceSection {...defaultProps} services={services} />);
    const presetLabel = screen.getByTestId("default-state-example.service");
    expect(presetLabel).toBeInTheDocument();
    expect(presetLabel).toHaveTextContent("preset: disable");
  });

  it("does not show preset label when default_state is absent", () => {
    const services = [
      makeService({
        unit: "example.service",
        // no default_state
      }),
    ];
    render(<ServiceSection {...defaultProps} services={services} />);
    expect(
      screen.queryByTestId("default-state-example.service"),
    ).not.toBeInTheDocument();
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
      kind: "Service",
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
