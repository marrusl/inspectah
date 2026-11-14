import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen } from "@testing-library/react";
// Rendered with a real pending guard, as its owner supplies one in the app.
import { SystemTuningSectionWithGuard as SystemTuningSection } from "../../test-utils/withPendingGuard";
import type {
  SysctlFileDto,
  SysctlViewDto,
  TunedDecisionDto,
  TriageTag,
} from "../../api/types";

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

function makeTunedTriage(): TriageTag {
  return {
    triage: { mode: "single_host" as const, site: null },
    primary_reason: "tuned_non_default_profile" as TriageTag["primary_reason"],
    annotations: [],
  };
}

function makeTuned(
  overrides: Partial<TunedDecisionDto> = {},
): TunedDecisionDto {
  return {
    active_profile: "throughput-performance",
    custom_profiles: [],
    triage: makeTunedTriage(),
    include: true,
    locked: false,
    ...overrides,
  };
}

const FILE: SysctlFileDto = {
  path: "/etc/sysctl.d/50-x.conf",
  dir: "etc",
  body: { kind: "mask" },
  config_path: "/etc/sysctl.d/50-x.conf",
};

function hostView(files: SysctlFileDto[]): SysctlViewDto {
  return {
    state: "host",
    explanation: { state: "resolved", settings: [], drift: [] },
    vendor_replacements: [],
    not_carried: [],
    as_written: [],
    files,
    tuned_active: null,
  };
}

const defaultProps = {
  sysctl: hostView([]),
  tuned: [] as TunedDecisionDto[],
  onViewUpdate: vi.fn(),
  onMutationError: vi.fn(),
};

const EMPTY = "No system tuning detected.";

// --- Tests ---

describe("SystemTuningSection", () => {
  it("shows the empty state only with no tuned rows and no sysctl files", () => {
    render(<SystemTuningSection {...defaultProps} />);
    expect(screen.getByTestId("system-tuning-section")).toBeInTheDocument();
    expect(screen.getByText(EMPTY)).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Sysctl" })).toBeNull();
  });

  it("shows the sysctl panel for a host with files and no tuned rows", () => {
    render(<SystemTuningSection {...defaultProps} sysctl={hostView([FILE])} />);
    expect(screen.queryByText(EMPTY)).toBeNull();
    expect(screen.getByRole("region", { name: "Sysctl" })).toBeInTheDocument();
    expect(screen.queryByTestId("tuned-section")).toBeNull();
  });

  it("shows the tuned rows and the panel independently", () => {
    render(
      <SystemTuningSection
        {...defaultProps}
        sysctl={hostView([FILE])}
        tuned={[makeTuned()]}
      />,
    );
    expect(screen.getByText("Tuned Profiles")).toBeInTheDocument();
    expect(screen.getByTestId("tuned-section")).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Sysctl" })).toBeInTheDocument();
  });

  it("shows tuned rows beside a host with no sysctl files, without the empty state", () => {
    render(<SystemTuningSection {...defaultProps} tuned={[makeTuned()]} />);
    expect(screen.getByTestId("tuned-section")).toBeInTheDocument();
    expect(screen.queryByText(EMPTY)).toBeNull();
  });

  it.each<[string, SysctlViewDto]>([
    ["unavailable", { state: "unavailable", reason: "degraded: sysctl -a failed" }],
    ["aggregate", { state: "aggregate" }],
  ])("always renders the %s line, even with nothing else", (_name, sysctl) => {
    render(<SystemTuningSection {...defaultProps} sysctl={sysctl} />);
    expect(screen.queryByText(EMPTY)).toBeNull();
    expect(screen.getByRole("region", { name: "Sysctl" })).toBeInTheDocument();
  });
});
