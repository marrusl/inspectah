import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SysctlSection } from "../SysctlSection";
import type {
  SysctlDecisionDto,
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

function makeTriage(
  bucket: "baseline" | "site" | "investigate",
  annotations: TriageTag["annotations"] = [],
): TriageTag {
  const reasonMap: Record<string, string> = {
    baseline: "sysctl_baseline_match",
    site: "sysctl_file_backed_override",
    investigate: "sysctl_no_baseline",
  };
  return {
    triage: { mode: "single_host" as const, [bucket]: null },
    primary_reason: reasonMap[bucket] as TriageTag["primary_reason"],
    annotations,
  };
}

function makeSysctl(
  overrides: Partial<SysctlDecisionDto> = {},
): SysctlDecisionDto {
  return {
    key: "net.ipv4.ip_forward",
    runtime: "1",
    default: "0",
    source: "/etc/sysctl.d/99-custom.conf",
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
  sysctls: [] as SysctlDecisionDto[],
  onViewUpdate: vi.fn(),
  onMutationError: vi.fn(),
};

// --- Tests ---

describe("SysctlSection", () => {
  it("renders empty state when no sysctls", () => {
    render(<SysctlSection {...defaultProps} />);
    expect(screen.getByTestId("sysctl-section")).toBeInTheDocument();
    expect(screen.getByText(/no sysctl overrides/i)).toBeInTheDocument();
  });

  it("renders sysctl rows with key and value", () => {
    const sysctls = [
      makeSysctl({ key: "net.ipv4.ip_forward", runtime: "1" }),
      makeSysctl({ key: "vm.swappiness", runtime: "10" }),
    ];
    render(<SysctlSection {...defaultProps} sysctls={sysctls} />);

    expect(
      screen.getByTestId("sysctl-item-net.ipv4.ip_forward"),
    ).toBeInTheDocument();
    expect(screen.getByTestId("sysctl-item-vm.swappiness")).toBeInTheDocument();
    expect(screen.getByText("net.ipv4.ip_forward")).toBeInTheDocument();
    expect(screen.getByText("= 1")).toBeInTheDocument();
    expect(screen.getByText("vm.swappiness")).toBeInTheDocument();
    expect(screen.getByText("= 10")).toBeInTheDocument();
  });

  it("shows source file in parentheses", () => {
    const sysctls = [
      makeSysctl({
        key: "net.ipv4.ip_forward",
        source: "/etc/sysctl.d/99-custom.conf",
      }),
    ];
    render(<SysctlSection {...defaultProps} sysctls={sysctls} />);
    expect(
      screen.getByText("(/etc/sysctl.d/99-custom.conf)"),
    ).toBeInTheDocument();
  });

  it("shows runtime-only indicator for runtime_only_observation annotation", () => {
    const sysctls = [
      makeSysctl({
        key: "net.core.rmem_max",
        triage: makeTriage("site", ["runtime_only_observation"]),
      }),
    ];
    render(<SysctlSection {...defaultProps} sysctls={sysctls} />);
    expect(
      screen.getByTestId("sysctl-runtime-only-net.core.rmem_max"),
    ).toBeInTheDocument();
    expect(screen.getByText("Runtime only")).toBeInTheDocument();
  });

  it("does not show runtime-only indicator when annotation is absent", () => {
    const sysctls = [makeSysctl({ key: "net.ipv4.ip_forward" })];
    render(<SysctlSection {...defaultProps} sysctls={sysctls} />);
    expect(
      screen.queryByTestId("sysctl-runtime-only-net.ipv4.ip_forward"),
    ).toBeNull();
  });

  it("shows triage badge for non-routine sysctls", () => {
    const sysctls = [
      makeSysctl({
        key: "unknown.sysctl",
        triage: makeTriage("investigate"),
      }),
    ];
    render(<SysctlSection {...defaultProps} sysctls={sysctls} />);
    expect(screen.getByText("No Baseline")).toBeInTheDocument();
  });

  it("toggling a sysctl sends SetInclude op with Sysctl ItemId", async () => {
    const onViewUpdate = vi.fn();
    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: () => Promise.resolve(MOCK_VIEW),
    });

    const sysctls = [makeSysctl({ key: "net.ipv4.ip_forward", include: true })];
    render(
      <SysctlSection
        {...defaultProps}
        sysctls={sysctls}
        onViewUpdate={onViewUpdate}
      />,
    );

    const row = screen.getByTestId("sysctl-item-net.ipv4.ip_forward");
    const checkbox = within(row).getByRole("checkbox");
    await userEvent.click(checkbox);

    expect(mockFetch).toHaveBeenCalledTimes(1);
    const callBody = JSON.parse(mockFetch.mock.calls[0][1].body);
    expect(callBody.op).toBe("SetInclude");
    expect(callBody.target.item_id).toEqual({
      kind: "Sysctl",
      key: { key: "net.ipv4.ip_forward" },
    });
    expect(callBody.target.include).toBe(false);
  });
});
