import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen } from "@testing-library/react";
import { SystemTuningSection } from "../SystemTuningSection";
import type {
  SysctlDecisionDto,
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

function makeSysctlTriage(): TriageTag {
  return {
    triage: { mode: "single_host" as const, site: null },
    primary_reason: "sysctl_file_backed_override" as TriageTag["primary_reason"],
    annotations: [],
  };
}

function makeTunedTriage(): TriageTag {
  return {
    triage: { mode: "single_host" as const, site: null },
    primary_reason: "tuned_non_default_profile" as TriageTag["primary_reason"],
    annotations: [],
  };
}

function makeSysctl(overrides: Partial<SysctlDecisionDto> = {}): SysctlDecisionDto {
  return {
    key: "net.ipv4.ip_forward",
    runtime: "1",
    default: "0",
    source: "/etc/sysctl.d/99-custom.conf",
    triage: makeSysctlTriage(),
    include: true,
    ...overrides,
  };
}

function makeTuned(overrides: Partial<TunedDecisionDto> = {}): TunedDecisionDto {
  return {
    active_profile: "throughput-performance",
    custom_profiles: [],
    triage: makeTunedTriage(),
    include: true,
    ...overrides,
  };
}

const defaultProps = {
  sysctls: [] as SysctlDecisionDto[],
  tuned: [] as TunedDecisionDto[],
  onViewUpdate: vi.fn(),
  onMutationError: vi.fn(),
};

// --- Tests ---

describe("SystemTuningSection", () => {
  it("renders empty state when no sysctls or tuned profiles", () => {
    render(<SystemTuningSection {...defaultProps} />);
    expect(screen.getByTestId("system-tuning-section")).toBeInTheDocument();
    expect(screen.getByText(/no system tuning overrides/i)).toBeInTheDocument();
  });

  it("renders both subsections when both have data", () => {
    render(
      <SystemTuningSection
        {...defaultProps}
        sysctls={[makeSysctl()]}
        tuned={[makeTuned()]}
      />,
    );
    expect(screen.getByTestId("system-tuning-section")).toBeInTheDocument();
    // Subheadings
    expect(screen.getByText("Sysctls")).toBeInTheDocument();
    expect(screen.getByText("Tuned Profiles")).toBeInTheDocument();
    // Items
    expect(screen.getByTestId("sysctl-section")).toBeInTheDocument();
    expect(screen.getByTestId("tuned-section")).toBeInTheDocument();
    // Divider present
    expect(screen.getByTestId("system-tuning-section").querySelector("hr")).toBeTruthy();
  });

  it("renders only sysctls subsection when tuned is empty", () => {
    render(
      <SystemTuningSection
        {...defaultProps}
        sysctls={[makeSysctl()]}
      />,
    );
    expect(screen.getByText("Sysctls")).toBeInTheDocument();
    expect(screen.queryByText("Tuned Profiles")).not.toBeInTheDocument();
    expect(screen.getByTestId("sysctl-section")).toBeInTheDocument();
    expect(screen.queryByTestId("tuned-section")).not.toBeInTheDocument();
    // No divider when only one subsection
    expect(screen.getByTestId("system-tuning-section").querySelector("hr")).toBeFalsy();
  });

  it("renders only tuned subsection when sysctls is empty", () => {
    render(
      <SystemTuningSection
        {...defaultProps}
        tuned={[makeTuned()]}
      />,
    );
    expect(screen.queryByText("Sysctls")).not.toBeInTheDocument();
    expect(screen.getByText("Tuned Profiles")).toBeInTheDocument();
    expect(screen.queryByTestId("sysctl-section")).not.toBeInTheDocument();
    expect(screen.getByTestId("tuned-section")).toBeInTheDocument();
  });
});
