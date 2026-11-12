/**
 * Controls that send their request directly rather than through App's
 * useMutation each hold a per-item guard while the request is in flight.
 * The Not carried Include has its own cases in
 * MainContent.not-carried.test.tsx.
 *
 * The container and tuned rows already disabled their checkbox while a
 * request was pending, but Space on the row calls the same handler and a
 * disabled checkbox does not stop it, so those cases press Space twice.
 */
import { describe, it, expect, vi, beforeEach } from "vitest";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { MainContent } from "../MainContent";
import { ContainerSection } from "../ContainerSection";
import { TunedSection } from "../TunedSection";
import type {
  FlatpakDecisionDto,
  GroupInfo,
  QuadletDecisionDto,
  RefinedPackage,
  RepoGroupInfo,
  TunedDecisionDto,
  ViewResponse,
} from "../../api/types";
import * as client from "../../api/client";

vi.mock("../../api/client", () => ({
  applyOp: vi.fn(),
  ungroupGroup: vi.fn(),
}));

const VENDOR: RepoGroupInfo = {
  section_id: "vendor",
  provenance: "verified",
  is_distro: false,
  tier: "third_party",
  package_count: 1,
  effective_member_count: 1,
  effective_included: 1,
  effective_unlisted: 0,
  enabled: true,
};

const GROUP: GroupInfo = {
  name: "web-server",
  member_count: 1,
  added_count: 1,
  locked_count: 0,
  optional_spillover_count: 0,
  effective_member_count: 1,
  effective_included: 1,
  render_state: "renderable",
  degradation_reason: null,
  members: [
    { name: "httpd", locked: false, overlap_groups: [], in_base_image: false },
  ],
};

const PACKAGE = {
  entry: {
    name: "vendor-agent",
    epoch: "0",
    version: "1.0",
    release: "1",
    arch: "x86_64",
    state: "added",
    disposition: { kind: "actionable", include: true },
    source_repo: "vendor",
    aggregate: null,
  },
  triage: {
    triage: { mode: "single_host", baseline: null },
    primary_reason: "package_baseline_match",
    annotations: [],
  },
} as RefinedPackage;

const VIEW = {
  packages: [PACKAGE],
  config_files: [],
  repo_groups: [VENDOR],
  package_groups: [GROUP],
  generation: 1,
  stats: {
    sections: [],
    needs_review_count: 0,
    ops_applied: 0,
    can_undo: false,
    can_redo: false,
    baseline_available: false,
  },
  version_changes: [],
  service_states: [],
  service_dropins: [],
  quadlets: [],
  flatpaks: [],
  sysctl: { state: "unavailable", reason: "not collected" },
  tuned: [],
  users_groups_decisions: [],
  session_is_sensitive: false,
} as unknown as ViewResponse;

const QUADLET: QuadletDecisionDto = {
  path: "/etc/containers/systemd/myapp.container",
  name: "myapp.container",
  image: "quay.io/org/myapp:latest",
  triage: {
    triage: { mode: "single_host", baseline: null },
    primary_reason: "quadlet_user_deployed",
    annotations: [],
  },
  include: true,
};

const FLATPAK: FlatpakDecisionDto = {
  app_id: "org.mozilla.firefox",
  remote: "flathub",
  branch: "stable",
  triage: {
    triage: { mode: "single_host", baseline: null },
    primary_reason: "flatpak_provisioned_on_first_boot",
    annotations: [],
  },
  include: true,
  lifecycle: "first_boot",
};

const TUNED: TunedDecisionDto = {
  active_profile: "throughput-performance",
  custom_profiles: [],
  triage: {
    triage: { mode: "single_host", site: null },
    primary_reason: "tuned_non_default_profile",
    annotations: [],
  } as TunedDecisionDto["triage"],
  include: true,
};

function renderMainContent(onMutationError: (err: Error) => void) {
  render(
    <MainContent
      activeSection="packages"
      loading={false}
      viewData={VIEW}
      sections={null}
      onViewUpdate={vi.fn()}
      onMutationError={onMutationError}
      sectionSearchOpen={false}
      onSectionSearchClose={vi.fn()}
    />,
  );
}

interface GuardCase {
  name: string;
  request: "applyOp" | "ungroupGroup";
  renderWith: (onMutationError: (err: Error) => void) => void;
  control: () => HTMLElement;
  /** What the operator does, when it is not a click on the control. */
  trigger?: () => void;
}

const pressSpace = (testId: string) => () =>
  fireEvent.keyDown(screen.getByTestId(testId), { key: " " });

const CASES: GuardCase[] = [
  {
    name: "package include/exclude checkbox",
    request: "applyOp",
    renderWith: renderMainContent,
    control: () =>
      screen.getByRole("checkbox", { name: "vendor-agent.x86_64" }),
  },
  {
    name: "repo switch",
    request: "applyOp",
    renderWith: renderMainContent,
    control: () => screen.getByRole("switch", { name: /^vendor:/ }),
  },
  {
    name: "group ungroup button",
    request: "ungroupGroup",
    renderWith: renderMainContent,
    control: () => screen.getByRole("button", { name: "Ungroup web-server" }),
  },
  {
    name: "quadlet toggle",
    request: "applyOp",
    renderWith: (onMutationError) =>
      render(
        <ContainerSection
          quadlets={[QUADLET]}
          flatpaks={[]}
          onViewUpdate={vi.fn()}
          onMutationError={onMutationError}
        />,
      ),
    control: () =>
      screen.getByRole("checkbox", { name: "Toggle myapp.container" }),
    trigger: pressSpace("quadlet-item-/etc/containers/systemd/myapp.container"),
  },
  {
    name: "flatpak toggle",
    request: "applyOp",
    renderWith: (onMutationError) =>
      render(
        <ContainerSection
          quadlets={[]}
          flatpaks={[FLATPAK]}
          onViewUpdate={vi.fn()}
          onMutationError={onMutationError}
        />,
      ),
    control: () =>
      screen.getByRole("checkbox", { name: "Toggle org.mozilla.firefox" }),
    trigger: pressSpace("flatpak-item-org.mozilla.firefox"),
  },
  {
    name: "tuned profile toggle",
    request: "applyOp",
    renderWith: (onMutationError) =>
      render(
        <TunedSection
          tuned={[TUNED]}
          onViewUpdate={vi.fn()}
          onMutationError={onMutationError}
        />,
      ),
    control: () =>
      screen.getByRole("checkbox", { name: "Toggle throughput-performance" }),
    trigger: pressSpace("tuned-item-throughput-performance"),
  },
];

/** Holds every request open; `fail` rejects the most recent one. */
function holdRequests(request: GuardCase["request"]) {
  let fail: (err: Error) => void = () => {};
  const mock = vi.mocked(client[request]);
  mock.mockImplementation(
    () =>
      new Promise<ViewResponse>((_resolve, reject) => {
        fail = reject;
      }),
  );
  return { mock, fail: (err: Error) => fail(err) };
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe.each(CASES)("pending guard: $name", (c) => {
  it("sends one request when triggered twice", () => {
    const { mock } = holdRequests(c.request);
    c.renderWith(vi.fn());
    const control = c.control();

    const trigger = c.trigger ?? (() => control.click());

    act(() => {
      trigger();
      trigger();
    });

    expect(mock).toHaveBeenCalledTimes(1);
    expect(control).toBeDisabled();
  });

  it("re-enables the control after the request fails", async () => {
    const { mock, fail } = holdRequests(c.request);
    const onMutationError = vi.fn();
    c.renderWith(onMutationError);
    const control = c.control();

    act(() => control.click());
    expect(control).toBeDisabled();

    fail(new Error("connection reset"));

    await waitFor(() => expect(control).toBeEnabled());
    expect(onMutationError).toHaveBeenCalledTimes(1);

    act(() => control.click());
    expect(mock).toHaveBeenCalledTimes(2);
  });
});

/**
 * The flatpak op identifies an item by (app_id, remote, branch). Rows that
 * share an app_id are distinct items and must not share a guard.
 */
describe.each([
  { differ: "remote", other: { ...FLATPAK, remote: "fedora" } },
  { differ: "branch", other: { ...FLATPAK, branch: "beta" } },
])("pending guard: flatpaks differing only by $differ", ({ other }) => {
  it("guards each row by its own identity", async () => {
    const { mock } = holdRequests("applyOp");
    render(
      <ContainerSection
        quadlets={[]}
        flatpaks={[FLATPAK, other]}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );
    const rows = screen.getAllByTestId("flatpak-item-org.mozilla.firefox");
    const boxes = screen.getAllByRole("checkbox", {
      name: "Toggle org.mozilla.firefox",
    });

    act(() => {
      fireEvent.keyDown(rows[0], { key: " " });
    });
    expect(boxes[0]).toBeDisabled();
    expect(boxes[1]).toBeEnabled();

    act(() => {
      fireEvent.keyDown(rows[1], { key: " " });
      fireEvent.keyDown(rows[0], { key: " " });
    });

    expect(mock).toHaveBeenCalledTimes(2);
    const sentTo = (f: FlatpakDecisionDto) =>
      expect.objectContaining({
        target: expect.objectContaining({
          item_id: {
            kind: "Flatpak",
            key: { app_id: f.app_id, remote: f.remote, branch: f.branch },
          },
        }),
      });
    expect(mock).toHaveBeenNthCalledWith(1, sentTo(FLATPAK));
    expect(mock).toHaveBeenNthCalledWith(2, sentTo(other));
    expect(boxes[1]).toBeDisabled();
  });
});
