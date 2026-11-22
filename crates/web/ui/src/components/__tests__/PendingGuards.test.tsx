/// <reference types="vite/client" />
/**
 * Controls that send their request directly rather than through App's
 * useMutation each hold a per-item guard while the request is in flight.
 * The Not carried Include has its own cases in
 * MainContent.not-carried.test.tsx.
 *
 * The container and tuned rows already disabled their checkbox while a
 * request was pending, but Space on the row calls the same handler and a
 * disabled checkbox does not stop it, so those cases press Space twice.
 *
 * Set password hashes in the browser before it sends anything, so its
 * guard has to hold from the click, not from the request.
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
import type { MainContentProps } from "../MainContent";
import type {
  DropInDecisionDto,
  FlatpakDecisionDto,
  GroupInfo,
  LanguagePackageEnv,
  MachineStateRowDto,
  QuadletDecisionDto,
  RefinedPackage,
  RepoGroupInfo,
  ServiceDecisionDto,
  TunedDecisionDto,
  UserDecision,
  ViewResponse,
} from "../../api/types";
import * as client from "../../api/client";
import { emptyView } from "../../test-utils/emptyView";
import * as crypt from "../../utils/crypt";

vi.mock("../../api/client", () => ({
  applyOp: vi.fn(),
  ungroupGroup: vi.fn(),
  setUserStrategy: vi.fn(),
  setUserPassword: vi.fn(),
}));

vi.mock("../../utils/crypt", () => ({
  sha512Crypt: vi.fn(),
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
  control: "full",
  kept: true,
  carriage_count: 0,
  carriage_included: 0,
  owns: [],
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

const VIEW: ViewResponse = emptyView({
  packages: [PACKAGE],
  repo_groups: [VENDOR],
  package_groups: [GROUP],
});

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
  locked: false,
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
  locked: false,
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
  locked: false,
};

const SERVICE: ServiceDecisionDto = {
  unit: "httpd.service",
  kind: "service",
  fragment: "vendor",
  triage: {
    triage: { mode: "single_host", site: null },
    primary_reason: "service_non_default_state",
    annotations: [],
  } as ServiceDecisionDto["triage"],
  include: true,
  locked: false,
  current_state: "enabled",
  required_config: [],
  data_paths: [],
};

const DROPIN: DropInDecisionDto = {
  unit: "httpd.service",
  path: "/etc/systemd/system/httpd.service.d/override.conf",
  config_paths: ["/etc/systemd/system/httpd.service.d/override.conf"],
  triage: SERVICE.triage,
  include: true,
  locked: false,
};

const USER: UserDecision = {
  name: "alice",
  uid: 1000,
  gid: 1000,
  shell: "/bin/bash",
  home: "/home/alice",
  include: true,
  classification: "interactive",
  containerfile_strategy: "skip",
  password_choice: "new",
};

/**
 * Renders one section through MainContent, which owns the section guards
 * in the app, so these cases exercise the production owner.
 */
function renderSection(
  activeSection: string,
  view: Partial<ViewResponse>,
  onMutationError: (err: Error) => void,
) {
  render(
    <MainContent
      activeSection={activeSection}
      loading={false}
      viewData={{ ...VIEW, ...view }}
      sections={null}
      onViewUpdate={vi.fn()}
      onMutationError={onMutationError}
      sectionSearchOpen={false}
      onSectionSearchClose={vi.fn()}
    />,
  );
}

/**
 * Scheduled Tasks with one generated timer row, whose toggle MainContent
 * guards by the generated name.
 */
function renderScheduled(onMutationError: (err: Error) => void) {
  render(
    <MainContent
      activeSection="scheduled_tasks"
      loading={false}
      viewData={{
        ...VIEW,
        generated_timers: [
          { name: "cron-backup", include: true, locked: false },
        ],
      }}
      sections={[
        {
          id: "scheduled_tasks",
          display_name: "Scheduled Tasks",
          items: [
            {
              id: "generated:0:cron-backup",
              title: "cron-backup",
              subtitle: "0 2 * * *",
              detail: null,
              searchable_text: "cron-backup generated-timer",
              generated_timer: "cron-backup",
            },
          ],
        },
      ]}
      onViewUpdate={vi.fn()}
      onMutationError={onMutationError}
      sectionSearchOpen={false}
      onSectionSearchClose={vi.fn()}
    />,
  );
}

function renderUsers(onMutationError: (err: Error) => void) {
  renderSection(
    "users_groups",
    { users_groups_decisions: [USER] },
    onMutationError,
  );
}

/** Opens alice's password form and fills it in, ready to submit. */
function renderPasswordForm(onMutationError: (err: Error) => void) {
  renderUsers(onMutationError);
  fireEvent.click(screen.getByRole("button", { name: "Expand alice details" }));
  fireEvent.click(screen.getByRole("button", { name: /Password options/ }));
  for (const label of ["Password", "Confirm password"]) {
    fireEvent.change(screen.getByLabelText(label), {
      target: { value: "hunter2" },
    });
  }
}

function renderMainContent(onMutationError: (err: Error) => void) {
  renderSection("packages", {}, onMutationError);
}

/**
 * Configuration with the recorded machine-state host's `plain` hostname
 * row (`machine-state.view.json`, recorded by
 * `crates/web/tests/system_tuning_fixtures_test.rs`), its block expanded:
 * MainContent guards the row's include by `machine-state:<path>`.
 */
const MACHINE_STATE_VIEW = Object.values(
  import.meta.glob<{ default: ViewResponse }>(
    "../../__tests__/fixtures/system-tuning/machine-state.view.json",
    { eager: true },
  ),
)[0].default;

function recordedRow(path: string): MachineStateRowDto {
  const row = MACHINE_STATE_VIEW.machine_state.find((r) => r.path === path);
  if (!row) throw new Error(`no recorded machine-state row ${path}`);
  return row;
}

const HOSTNAME_ROW = recordedRow("/etc/hostname");

function renderMachineState(onMutationError: (err: Error) => void) {
  renderSection("configs", { machine_state: [HOSTNAME_ROW] }, onMutationError);
  fireEvent.click(screen.getByRole("button", { name: /^Machine state/ }));
}

interface GuardCase {
  name: string;
  request: "applyOp" | "ungroupGroup" | "setUserStrategy" | "setUserPassword";
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
      renderSection("containers", { quadlets: [QUADLET] }, onMutationError),
    control: () =>
      screen.getByRole("checkbox", { name: "Toggle myapp.container" }),
    trigger: pressSpace("quadlet-item-/etc/containers/systemd/myapp.container"),
  },
  {
    name: "flatpak toggle",
    request: "applyOp",
    renderWith: (onMutationError) =>
      renderSection("containers", { flatpaks: [FLATPAK] }, onMutationError),
    control: () =>
      screen.getByRole("checkbox", { name: "Toggle org.mozilla.firefox" }),
    trigger: pressSpace("flatpak-item-org.mozilla.firefox"),
  },
  {
    name: "tuned profile toggle",
    request: "applyOp",
    renderWith: (onMutationError) =>
      renderSection("system_tuning", { tuned: [TUNED] }, onMutationError),
    control: () =>
      screen.getByRole("checkbox", { name: "Toggle throughput-performance" }),
    trigger: pressSpace("tuned-item-throughput-performance"),
  },
  {
    name: "service toggle",
    request: "applyOp",
    renderWith: (onMutationError) =>
      renderSection(
        "units",
        { unit_states: [SERVICE], unit_dropins: [DROPIN] },
        onMutationError,
      ),
    control: () =>
      screen.getByRole("checkbox", { name: "Toggle httpd.service" }),
    trigger: pressSpace("service-item-httpd.service"),
  },
  {
    name: "service drop-in toggle",
    request: "applyOp",
    renderWith: (onMutationError) =>
      renderSection(
        "units",
        { unit_states: [SERVICE], unit_dropins: [DROPIN] },
        onMutationError,
      ),
    control: () =>
      screen.getByRole("checkbox", { name: `Toggle ${DROPIN.path}` }),
    trigger: pressSpace(`dropin-item-${DROPIN.path}`),
  },
  {
    name: "generated timer toggle",
    request: "applyOp",
    renderWith: renderScheduled,
    control: () => screen.getByRole("checkbox", { name: "Toggle cron-backup" }),
  },
  {
    name: "machine-state include",
    request: "applyOp",
    renderWith: renderMachineState,
    control: () =>
      screen.getByRole("button", { name: "Include /etc/hostname" }),
  },
  {
    name: "user include checkbox",
    request: "setUserStrategy",
    renderWith: renderUsers,
    control: () => screen.getByRole("checkbox", { name: "Include alice" }),
  },
  {
    name: "user set password button",
    request: "setUserPassword",
    renderWith: renderPasswordForm,
    control: () => screen.getByRole("button", { name: "Set password" }),
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
  vi.mocked(crypt.sha512Crypt).mockResolvedValue("$6$salt$hash");
});

describe.each(CASES)("pending guard: $name", (c) => {
  it("sends one request when triggered twice", async () => {
    const { mock } = holdRequests(c.request);
    c.renderWith(vi.fn());
    const control = c.control();

    const trigger = c.trigger ?? (() => control.click());

    await act(async () => {
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

    await act(async () => control.click());
    expect(control).toBeDisabled();

    fail(new Error("connection reset"));

    await waitFor(() => expect(control).toBeEnabled());
    expect(onMutationError).toHaveBeenCalledTimes(1);

    await act(async () => control.click());
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
    renderSection("containers", { flatpaks: [FLATPAK, other] }, vi.fn());
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

describe("pending guard: user set password", () => {
  it("holds the user from the click, through hashing, until the request settles", async () => {
    let finishHash: (hash: string) => void = () => {};
    vi.mocked(crypt.sha512Crypt).mockImplementation(
      () =>
        new Promise<string>((resolve) => {
          finishHash = resolve;
        }),
    );
    const { mock } = holdRequests("setUserPassword");
    renderPasswordForm(vi.fn());

    await act(async () =>
      screen.getByRole("button", { name: "Set password" }).click(),
    );

    // Still hashing: nothing sent, and the user's other control is held too.
    expect(mock).not.toHaveBeenCalled();
    expect(
      screen.getByRole("checkbox", { name: "Include alice" }),
    ).toBeDisabled();

    await act(async () => finishHash("$6$salt$hash"));

    expect(mock).toHaveBeenCalledTimes(1);
    expect(mock).toHaveBeenCalledWith("alice", "new", "$6$salt$hash");
    expect(
      screen.getByRole("checkbox", { name: "Include alice" }),
    ).toBeDisabled();
  });
});

const NPM_GLOBALS: LanguagePackageEnv = {
  ecosystem: "npm",
  path: "/usr/lib/node_modules",
  method: "npm global",
  packages: [{ name: "typescript", detected_version: "5.4.0", pinned: false }],
  confidence: "high",
  manifest_basis: "package-lock.json",
  include: true,
  has_c_extensions: false,
  system_site_packages: false,
};

/** A request the test holds open, and how to settle it. */
interface HeldRequest {
  sent: ReturnType<typeof vi.fn>;
  settle: () => void;
  props?: Partial<MainContentProps>;
}

function heldClientRequest(request: GuardCase["request"]): HeldRequest {
  const { mock, fail } = holdRequests(request);
  return { sent: mock, settle: () => fail(new Error("connection reset")) };
}

function heldPin(): HeldRequest {
  let settle: () => void = () => {};
  const sent = vi.fn(
    () =>
      new Promise<void>((resolve) => {
        settle = resolve;
      }),
  );
  return { sent, settle: () => settle(), props: { onSetPackagePin: sent } };
}

interface LifetimeCase {
  name: string;
  section: string;
  view: Partial<ViewResponse>;
  hold: () => HeldRequest;
  /** Brings the control on screen, for a section that starts collapsed. */
  reveal?: () => void;
  control: () => HTMLElement;
  trigger: () => void;
}

const LIFETIME_CASES: LifetimeCase[] = [
  {
    name: "quadlet toggle",
    section: "containers",
    view: { quadlets: [QUADLET] },
    hold: () => heldClientRequest("applyOp"),
    control: () =>
      screen.getByRole("checkbox", { name: "Toggle myapp.container" }),
    trigger: pressSpace("quadlet-item-/etc/containers/systemd/myapp.container"),
  },
  {
    name: "tuned profile toggle",
    section: "system_tuning",
    view: { tuned: [TUNED] },
    hold: () => heldClientRequest("applyOp"),
    control: () =>
      screen.getByRole("checkbox", { name: "Toggle throughput-performance" }),
    trigger: pressSpace("tuned-item-throughput-performance"),
  },
  {
    name: "service toggle",
    section: "units",
    view: { unit_states: [SERVICE], unit_dropins: [DROPIN] },
    hold: () => heldClientRequest("applyOp"),
    control: () =>
      screen.getByRole("checkbox", { name: "Toggle httpd.service" }),
    trigger: pressSpace("service-item-httpd.service"),
  },
  {
    name: "service drop-in toggle",
    section: "units",
    view: { unit_states: [SERVICE], unit_dropins: [DROPIN] },
    hold: () => heldClientRequest("applyOp"),
    control: () =>
      screen.getByRole("checkbox", { name: `Toggle ${DROPIN.path}` }),
    trigger: () =>
      screen.getByRole("checkbox", { name: `Toggle ${DROPIN.path}` }).click(),
  },
  {
    name: "user include checkbox",
    section: "users_groups",
    view: { users_groups_decisions: [USER] },
    hold: () => heldClientRequest("setUserStrategy"),
    control: () => screen.getByRole("checkbox", { name: "Include alice" }),
    trigger: () =>
      screen.getByRole("checkbox", { name: "Include alice" }).click(),
  },
  {
    name: "language package pin",
    section: "language_packages",
    view: { language_packages: [NPM_GLOBALS] },
    hold: heldPin,
    reveal: () =>
      fireEvent.click(
        screen.getByTestId("lang-env-expand-npm:/usr/lib/node_modules"),
      ),
    control: () => screen.getByTestId("lang-pkg-pin-typescript"),
    trigger: () =>
      fireEvent.keyDown(screen.getByTestId("lang-pkg-typescript"), {
        key: "Enter",
      }),
  },
];

/**
 * Switching sections unmounts the section's component. A guard that lived
 * there came back empty while its request was still in flight, so the same
 * request could go out again.
 */
describe.each(LIFETIME_CASES)(
  "pending guard survives a section switch: $name",
  (c) => {
    it("suppresses a repeat until the request settles", async () => {
      const held = c.hold();
      const props: MainContentProps = {
        activeSection: c.section,
        loading: false,
        viewData: { ...VIEW, ...c.view },
        sections: null,
        onViewUpdate: vi.fn(),
        onMutationError: vi.fn(),
        sectionSearchOpen: false,
        onSectionSearchClose: vi.fn(),
        ...held.props,
      };
      const { rerender } = render(<MainContent {...props} />);
      c.reveal?.();

      await act(async () => c.trigger());
      expect(held.sent).toHaveBeenCalledTimes(1);

      rerender(<MainContent {...props} activeSection="packages" />);
      expect(() => c.control()).toThrow();
      rerender(<MainContent {...props} />);
      c.reveal?.();

      expect(c.control()).toBeDisabled();
      await act(async () => c.trigger());
      expect(held.sent).toHaveBeenCalledTimes(1);

      await act(async () => held.settle());
      await waitFor(() => expect(c.control()).toBeEnabled());

      await act(async () => c.trigger());
      expect(held.sent).toHaveBeenCalledTimes(2);
    });
  },
);

const BOB: UserDecision = {
  ...USER,
  name: "bob",
  uid: 1001,
  gid: 1001,
  home: "/home/bob",
};

/**
 * The Users guard is keyed by username, so one user's request holds that
 * user's card and no other.
 */
describe("pending guard: users are held one at a time", () => {
  it("holds alice while bob stays usable and sends his own request", async () => {
    const { mock } = holdRequests("setUserStrategy");
    renderSection(
      "users_groups",
      { users_groups_decisions: [USER, BOB] },
      vi.fn(),
    );
    const alice = screen.getByRole("checkbox", { name: "Include alice" });
    const bob = screen.getByRole("checkbox", { name: "Include bob" });

    await act(async () => alice.click());
    expect(alice).toBeDisabled();
    expect(bob).toBeEnabled();

    await act(async () => bob.click());
    expect(mock).toHaveBeenCalledTimes(2);
    expect(mock).toHaveBeenNthCalledWith(1, "alice", expect.anything());
    expect(mock).toHaveBeenNthCalledWith(2, "bob", expect.anything());
    expect(alice).toBeDisabled();
    expect(bob).toBeDisabled();
  });
});

/**
 * Hashing runs inside the guard before any request exists. A hash that
 * fails, whether it throws or rejects, sends nothing, reports on the card
 * only (it is not a failed request), and must release the user.
 */
describe.each([
  {
    failure: "throws synchronously",
    breakHash: () =>
      vi.mocked(crypt.sha512Crypt).mockImplementation(() => {
        throw new Error("crypto unavailable");
      }),
  },
  {
    failure: "rejects",
    breakHash: () =>
      vi
        .mocked(crypt.sha512Crypt)
        .mockRejectedValue(new Error("crypto unavailable")),
  },
])("pending guard: set password when hashing $failure", ({ breakHash }) => {
  it("sends nothing, shows the card error, and allows a retry", async () => {
    breakHash();
    const send = vi.mocked(client.setUserPassword);
    send.mockResolvedValue(VIEW);
    const onMutationError = vi.fn();
    renderPasswordForm(onMutationError);
    const setPassword = screen.getByRole("button", { name: "Set password" });

    await act(async () => setPassword.click());

    expect(send).not.toHaveBeenCalled();
    expect(
      screen.getByText("Failed to set password. Please try again."),
    ).toBeInTheDocument();
    expect(onMutationError).not.toHaveBeenCalled();
    expect(setPassword).toBeEnabled();
    expect(
      screen.getByRole("checkbox", { name: "Include alice" }),
    ).toBeEnabled();

    vi.mocked(crypt.sha512Crypt).mockResolvedValue("$6$salt$hash");
    await act(async () => setPassword.click());

    expect(send).toHaveBeenCalledTimes(1);
    expect(send).toHaveBeenCalledWith("alice", "new", "$6$salt$hash");
  });
});

/**
 * The card unmounts on a section switch, so a set-password failure that
 * lands while the operator is elsewhere has to be kept by the owner, or
 * the card comes back with no sign that the password was never set.
 */
describe.each([
  {
    failure: "the hash rejects",
    arrange: () => {
      let fail: (err: Error) => void = () => {};
      vi.mocked(crypt.sha512Crypt).mockImplementation(
        () =>
          new Promise<string>((_resolve, reject) => {
            fail = reject;
          }),
      );
      return { fail: (err: Error) => fail(err), reportedAsRequest: false };
    },
  },
  {
    failure: "the request rejects",
    arrange: () => {
      const { fail } = holdRequests("setUserPassword");
      return { fail, reportedAsRequest: true };
    },
  },
])("set password failure while on another section: $failure", ({ arrange }) => {
  it("shows the failure on return, releases the user, and allows a retry", async () => {
    const { fail, reportedAsRequest } = arrange();
    const onMutationError = vi.fn();
    const props: MainContentProps = {
      activeSection: "users_groups",
      loading: false,
      viewData: { ...VIEW, users_groups_decisions: [USER] },
      sections: null,
      onViewUpdate: vi.fn(),
      onMutationError,
      sectionSearchOpen: false,
      onSectionSearchClose: vi.fn(),
    };
    const { rerender } = render(<MainContent {...props} />);
    fireEvent.click(
      screen.getByRole("button", { name: "Expand alice details" }),
    );
    fireEvent.click(screen.getByRole("button", { name: /Password options/ }));
    for (const label of ["Password", "Confirm password"]) {
      fireEvent.change(screen.getByLabelText(label), {
        target: { value: "hunter2" },
      });
    }
    await act(async () =>
      screen.getByRole("button", { name: "Set password" }).click(),
    );

    rerender(<MainContent {...props} activeSection="packages" />);
    expect(screen.queryByTestId("user-card-alice")).toBeNull();
    await act(async () => fail(new Error("connection reset")));
    rerender(<MainContent {...props} />);

    // Visible on return, without the operator having to open the card.
    expect(
      screen.getByText("Failed to set password. Please try again."),
    ).toBeVisible();
    expect(onMutationError).toHaveBeenCalledTimes(reportedAsRequest ? 1 : 0);
    await waitFor(() =>
      expect(
        screen.getByRole("checkbox", { name: "Include alice" }),
      ).toBeEnabled(),
    );

    vi.mocked(crypt.sha512Crypt).mockResolvedValue("$6$salt$hash");
    const send = vi.mocked(client.setUserPassword);
    send.mockReset();
    send.mockResolvedValue(VIEW);
    for (const label of ["Password", "Confirm password"]) {
      fireEvent.change(screen.getByLabelText(label), {
        target: { value: "hunter2" },
      });
    }
    // Editing the password clears the old failure, as before.
    expect(
      screen.queryByText("Failed to set password. Please try again."),
    ).toBeNull();
    const setPassword = screen.getByRole("button", { name: "Set password" });
    expect(setPassword).toBeEnabled();
    await act(async () => setPassword.click());

    expect(send).toHaveBeenCalledTimes(1);
    expect(send).toHaveBeenCalledWith("alice", "new", "$6$salt$hash");
  });
});
