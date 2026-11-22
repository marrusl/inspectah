import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen } from "@testing-library/react";
import { MainContent } from "../MainContent";
import type {
  Disposition,
  RefinedConfig,
  TriageTag,
  ViewResponse,
} from "../../api/types";
import { mockStats } from "../../test-utils/mockStats";

const TRIAGE: TriageTag = {
  triage: { mode: "single_host", investigate: null },
  primary_reason: "config_unowned",
  annotations: [],
};

const ADVISORY_PATH = "/etc/init.d/legacy-app";
const RATIONALE = "sysvinit script — port to a systemd unit";
const PLAIN_PATH = "/etc/httpd/conf/httpd.conf";

const ADVISORY: Disposition = {
  kind: "advisory",
  advisory_type: "modernization",
  rationale: RATIONALE,
};

function makeConfig(path: string, disposition: Disposition): RefinedConfig {
  return {
    entry: {
      path,
      kind: "unowned",
      category: "other",
      content: "",
      rpm_va_flags: null,
      package: null,
      diff_against_rpm: null,
      disposition,
      tie: false,
      tie_winner: false,
      aggregate: null,
    },
    triage: TRIAGE,
  };
}

function view(config_files: RefinedConfig[]): ViewResponse {
  return {
    packages: [],
    config_files,
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
  };
}

const props = {
  activeSection: "configs",
  loading: false,
  sections: null,
  onViewUpdate: vi.fn(),
  onMutationError: vi.fn(),
  sectionSearchOpen: false,
  onSectionSearchClose: vi.fn(),
};

describe("config advisories in the refine web UI", () => {
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

  it("shows a config advisory with its rationale and no toggle", () => {
    render(
      <MainContent {...props} viewData={view([makeConfig(ADVISORY_PATH, ADVISORY)])} />,
    );
    const row = screen.getByTestId(`decision-item-configs:${ADVISORY_PATH}`);
    expect(row.getAttribute("data-disposition")).toBe("advisory");
    expect(screen.getByText(RATIONALE)).toBeInTheDocument();
    expect(screen.queryByRole("checkbox")).toBeNull();
  });

  it("does not claim everything is excluded when the only findings are advisories", () => {
    // `is_included()` is false for an advisory, so the old all-excluded check
    // counted a host whose sole config finding was a modernization advisory as
    // "the user excluded everything" and replaced the list with an empty state
    // — hiding the finding entirely.
    render(
      <MainContent {...props} viewData={view([makeConfig(ADVISORY_PATH, ADVISORY)])} />,
    );
    expect(screen.queryByTestId("config-all-excluded")).toBeNull();
    expect(
      screen.getByTestId(`decision-item-configs:${ADVISORY_PATH}`),
    ).toBeInTheDocument();
  });

  it("keeps the advisory visible when every actionable config is excluded", () => {
    render(
      <MainContent
        {...props}
        viewData={view([
          makeConfig(ADVISORY_PATH, ADVISORY),
          makeConfig(PLAIN_PATH, { kind: "actionable", include: false }),
        ])}
      />,
    );
    expect(screen.queryByTestId("config-all-excluded")).toBeNull();
    expect(screen.getByText(RATIONALE)).toBeInTheDocument();
  });

  it("still reports all-excluded when every config is an excluded decision", () => {
    render(
      <MainContent
        {...props}
        viewData={view([makeConfig(PLAIN_PATH, { kind: "actionable", include: false })])}
      />,
    );
    expect(screen.getByTestId("config-all-excluded")).toBeInTheDocument();
  });
});
