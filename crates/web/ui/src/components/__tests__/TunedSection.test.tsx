import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { TunedSection } from "../TunedSection";
import type {
  TunedDecisionDto,
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
    baseline: "tuned_baseline_match",
    site: "tuned_non_default_profile",
    investigate: "tuned_unusual_state",
  };
  return {
    triage: { mode: "single_host" as const, [bucket]: null },
    primary_reason: reasonMap[bucket] as TriageTag["primary_reason"],
    annotations: [],
  };
}

function makeTuned(
  overrides: Partial<TunedDecisionDto> = {},
): TunedDecisionDto {
  return {
    active_profile: "throughput-performance",
    custom_profiles: [],
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
  tuned: [] as TunedDecisionDto[],
  onViewUpdate: vi.fn(),
  onMutationError: vi.fn(),
};

// --- Tests ---

describe("TunedSection", () => {
  it("renders empty state when no tuned profiles", () => {
    render(<TunedSection {...defaultProps} />);
    expect(screen.getByTestId("tuned-section")).toBeInTheDocument();
    expect(
      screen.getByText(/no tuned profile selections/i),
    ).toBeInTheDocument();
  });

  it("renders tuned row with profile name", () => {
    const tuned = [makeTuned({ active_profile: "throughput-performance" })];
    render(<TunedSection {...defaultProps} tuned={tuned} />);

    expect(
      screen.getByTestId("tuned-item-throughput-performance"),
    ).toBeInTheDocument();
    expect(screen.getByText("throughput-performance")).toBeInTheDocument();
  });

  it("shows custom profiles list when present", () => {
    const tuned = [
      makeTuned({
        active_profile: "my-custom",
        custom_profiles: ["my-custom", "my-other"],
      }),
    ];
    render(<TunedSection {...defaultProps} tuned={tuned} />);
    expect(screen.getByText("Custom: my-custom, my-other")).toBeInTheDocument();
  });

  it("does not show custom profiles when list is empty", () => {
    const tuned = [
      makeTuned({ active_profile: "balanced", custom_profiles: [] }),
    ];
    render(<TunedSection {...defaultProps} tuned={tuned} />);
    expect(screen.queryByText(/Custom:/)).toBeNull();
  });

  it("toggling a tuned profile sends SetInclude op with TunedSelection ItemId", async () => {
    const onViewUpdate = vi.fn();
    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: () => Promise.resolve(MOCK_VIEW),
    });

    const tuned = [
      makeTuned({ active_profile: "throughput-performance", include: true }),
    ];
    render(
      <TunedSection
        {...defaultProps}
        tuned={tuned}
        onViewUpdate={onViewUpdate}
      />,
    );

    const row = screen.getByTestId("tuned-item-throughput-performance");
    const checkbox = within(row).getByRole("checkbox");
    await userEvent.click(checkbox);

    expect(mockFetch).toHaveBeenCalledTimes(1);
    const callBody = JSON.parse(mockFetch.mock.calls[0][1].body);
    expect(callBody.op).toBe("SetInclude");
    expect(callBody.target.item_id).toEqual({
      kind: "TunedSelection",
      key: { profile: "throughput-performance" },
    });
    expect(callBody.target.include).toBe(false);
  });

  it("sections with fewer than 3 items are rendered flat (default expanded)", () => {
    const tuned = [makeTuned({ active_profile: "balanced" })];
    render(<TunedSection {...defaultProps} tuned={tuned} />);

    // All items are visible without any expand/collapse interaction
    expect(screen.getByText("balanced")).toBeInTheDocument();
    // The grid role is present (flat list, no collapsible wrapper)
    expect(screen.getByRole("grid")).toBeInTheDocument();
  });
});
