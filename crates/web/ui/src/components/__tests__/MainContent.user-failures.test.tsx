/**
 * A failed Users request is kept by MainContent, by username, so it
 * outlives the user card, which unmounts on a section switch. What it must
 * not outlive is the user record it was about: a reset replaces every
 * record, and a user that leaves the view and comes back is a new record.
 * A failure from before either of those is no longer true of the card.
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
import type { UserDecision, ViewResponse } from "../../api/types";
import * as client from "../../api/client";
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

const ALICE: UserDecision = {
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

const BOB: UserDecision = {
  ...ALICE,
  name: "bob",
  uid: 1001,
  gid: 1001,
  home: "/home/bob",
};

const PASSWORD_FAILURE = "Failed to set password. Please try again.";

function view(users: UserDecision[]): ViewResponse {
  return {
    packages: [],
    config_files: [],
    repo_groups: [],
    package_groups: [],
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
    users_groups_decisions: users,
    session_is_sensitive: false,
  } as unknown as ViewResponse;
}

function baseProps(): MainContentProps {
  return {
    activeSection: "users_groups",
    loading: false,
    viewData: view([ALICE, BOB]),
    sections: null,
    onViewUpdate: vi.fn(),
    onMutationError: vi.fn(),
    sectionSearchOpen: false,
    onSectionSearchClose: vi.fn(),
  };
}

/** Holds each call to `mock` open; settles the latest one on demand. */
function hold(mock: {
  mockImplementation: (impl: () => Promise<ViewResponse>) => unknown;
}) {
  let fail: (err: Error) => void = () => {};
  let succeed: () => void = () => {};
  mock.mockImplementation(
    () =>
      new Promise<ViewResponse>((resolve, reject) => {
        fail = reject;
        succeed = () => resolve(view([ALICE, BOB]));
      }),
  );
  return {
    fail: () => fail(new Error("connection reset")),
    succeed: () => succeed(),
  };
}

const includeAlice = () =>
  screen.getByRole("checkbox", { name: "Include alice" });

/** Opens alice's card and password options if they are closed. */
function openPasswordForm() {
  const expand = screen.queryByRole("button", { name: "Expand alice details" });
  if (expand) fireEvent.click(expand);
  if (!screen.queryByLabelText("Password")) {
    fireEvent.click(
      screen.getAllByRole("button", { name: /Password options/ })[0],
    );
  }
}

/** Fills in and submits a new password for alice. */
async function submitNewPassword() {
  openPasswordForm();
  for (const label of ["Password", "Confirm password"]) {
    fireEvent.change(screen.getByLabelText(label), {
      target: { value: "hunter2" },
    });
  }
  await act(async () =>
    screen.getByRole("button", { name: "Set password" }).click(),
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(crypt.sha512Crypt).mockResolvedValue("$6$salt$hash");
});

describe("user failures end with the user record", () => {
  it("keeps a failure across a new view that still has the user", async () => {
    const held = hold(vi.mocked(client.setUserPassword));
    const props = baseProps();
    const { rerender } = render(<MainContent {...props} />);
    await submitNewPassword();
    await act(async () => held.fail());
    expect(screen.getByText(PASSWORD_FAILURE)).toBeVisible();

    rerender(<MainContent {...props} viewData={view([ALICE, BOB])} />);
    rerender(<MainContent {...props} activeSection="packages" />);
    rerender(<MainContent {...props} viewData={view([ALICE, BOB])} />);

    expect(screen.getByText(PASSWORD_FAILURE)).toBeVisible();
  });

  it("clears a shown failure on reset", async () => {
    const held = hold(vi.mocked(client.setUserPassword));
    const props = baseProps();
    const { rerender } = render(<MainContent {...props} />);
    await submitNewPassword();
    await act(async () => held.fail());
    expect(screen.getByText(PASSWORD_FAILURE)).toBeVisible();

    rerender(<MainContent {...props} resetCounter={1} />);

    expect(screen.queryByText(PASSWORD_FAILURE)).toBeNull();
  });

  it("drops a failure from a request that was in flight across a reset", async () => {
    const held = hold(vi.mocked(client.setUserPassword));
    const props = baseProps();
    const { rerender } = render(<MainContent {...props} />);
    await submitNewPassword();

    rerender(<MainContent {...props} resetCounter={1} />);
    await act(async () => held.fail());

    expect(screen.queryByText(PASSWORD_FAILURE)).toBeNull();
    await waitFor(() => expect(includeAlice()).toBeEnabled());
  });

  it("does not show an old failure on a user that left the view and came back", async () => {
    const held = hold(vi.mocked(client.setUserPassword));
    const props = baseProps();
    const { rerender } = render(<MainContent {...props} />);
    await submitNewPassword();
    await act(async () => held.fail());
    expect(screen.getByText(PASSWORD_FAILURE)).toBeVisible();

    rerender(<MainContent {...props} viewData={view([BOB])} />);
    expect(screen.queryByTestId("user-card-alice")).toBeNull();
    rerender(<MainContent {...props} viewData={view([ALICE, BOB])} />);

    expect(screen.getByTestId("user-card-alice")).toBeInTheDocument();
    expect(screen.queryByText(PASSWORD_FAILURE)).toBeNull();
  });

  it("drops a failure from a request sent before the user left and came back", async () => {
    const held = hold(vi.mocked(client.setUserPassword));
    const props = baseProps();
    const { rerender } = render(<MainContent {...props} />);
    await submitNewPassword();

    rerender(<MainContent {...props} viewData={view([BOB])} />);
    rerender(<MainContent {...props} viewData={view([ALICE, BOB])} />);
    await act(async () => held.fail());

    expect(screen.queryByText(PASSWORD_FAILURE)).toBeNull();
  });
});

/**
 * Undo and Redo keep the user in the view but can replace their record.
 * A failure about the record that step replaced is no longer true of the
 * card; a failure on a user the step did not touch still is.
 */
describe("user failures across Undo and Redo", () => {
  it("clears a shown failure when the step changes that user", async () => {
    const held = hold(vi.mocked(client.setUserPassword));
    const props = baseProps();
    const { rerender } = render(<MainContent {...props} />);
    await submitNewPassword();
    await act(async () => held.fail());
    expect(screen.getByText(PASSWORD_FAILURE)).toBeVisible();

    rerender(<MainContent {...props} historyChange={{ users: ["alice"] }} />);

    expect(screen.queryByText(PASSWORD_FAILURE)).toBeNull();
  });

  it("keeps a failure when the step changes only another user", async () => {
    const held = hold(vi.mocked(client.setUserPassword));
    const props = baseProps();
    const { rerender } = render(<MainContent {...props} />);
    await submitNewPassword();
    await act(async () => held.fail());

    rerender(<MainContent {...props} historyChange={{ users: ["bob"] }} />);

    expect(screen.getByText(PASSWORD_FAILURE)).toBeVisible();
  });

  it("drops a failure from a request in flight across a step that changes the user, and records the next one", async () => {
    const held = hold(vi.mocked(client.setUserPassword));
    const props = baseProps();
    const { rerender } = render(<MainContent {...props} />);
    await submitNewPassword();

    rerender(<MainContent {...props} historyChange={{ users: ["alice"] }} />);
    await act(async () => held.fail());

    expect(screen.queryByText(PASSWORD_FAILURE)).toBeNull();
    await waitFor(() => expect(includeAlice()).toBeEnabled());

    await submitNewPassword();
    await act(async () => held.fail());
    expect(screen.getByText(PASSWORD_FAILURE)).toBeVisible();
  });
});

/**
 * A failure opens the card and its password form so the operator sees it.
 * Closing either is the operator acknowledging it, so the failure must not
 * open them again when the card mounts after a section switch.
 */
describe.each([
  {
    dismissal: "collapsing the card",
    dismiss: () =>
      fireEvent.click(
        screen.getByRole("button", { name: "Collapse alice details" }),
      ),
  },
  {
    dismissal: "collapsing the password options",
    dismiss: () =>
      fireEvent.click(
        screen.getAllByRole("button", { name: /Password options/ })[0],
      ),
  },
])("a failure dismissed by $dismissal", ({ dismiss }) => {
  it("stays dismissed across a rerender and a section switch, and a new failure opens the form again", async () => {
    const held = hold(vi.mocked(client.setUserPassword));
    const props = baseProps();
    const { rerender } = render(<MainContent {...props} />);
    await submitNewPassword();
    await act(async () => held.fail());
    expect(screen.getByText(PASSWORD_FAILURE)).toBeVisible();

    dismiss();
    expect(screen.queryByLabelText("Password")).toBeNull();

    rerender(<MainContent {...props} viewData={view([ALICE, BOB])} />);
    expect(screen.queryByLabelText("Password")).toBeNull();
    rerender(<MainContent {...props} activeSection="packages" />);
    rerender(<MainContent {...props} />);

    expect(screen.queryByLabelText("Password")).toBeNull();
    expect(screen.queryByText(PASSWORD_FAILURE)).toBeNull();
    openPasswordForm();
    expect(screen.queryByText(PASSWORD_FAILURE)).toBeNull();

    await submitNewPassword();
    dismiss();
    expect(screen.queryByLabelText("Password")).toBeNull();
    await act(async () => held.fail());

    expect(screen.getByText(PASSWORD_FAILURE)).toBeVisible();
  });
});

/**
 * A failed strategy change or a failed "No password" or "Keep existing"
 * choice used to reach only `onMutationError`, which logs. The card shows
 * it where it is visible with the card closed, since none of these come
 * from the new-password form.
 */
describe.each([
  {
    action: "strategy change",
    mock: () => vi.mocked(client.setUserStrategy),
    prepare: () => {},
    trigger: () => includeAlice().click(),
    message: "Failed to change the Containerfile strategy. Please try again.",
  },
  {
    action: "password option",
    mock: () => vi.mocked(client.setUserPassword),
    prepare: openPasswordForm,
    trigger: () => screen.getByRole("radio", { name: "No password" }).click(),
    message: "Failed to change the password option. Please try again.",
  },
])("a failed $action", ({ mock, prepare, trigger, message }) => {
  it.each([
    { where: "while the card is mounted", away: false },
    { where: "while on another section", away: true },
  ])(
    "shows on the card $where, and a successful retry clears it",
    async ({ away }) => {
      const held = hold(mock());
      const props = baseProps();
      const { rerender } = render(<MainContent {...props} />);

      prepare();
      await act(async () => trigger());
      if (away) rerender(<MainContent {...props} activeSection="packages" />);
      await act(async () => held.fail());
      if (away) rerender(<MainContent {...props} />);

      expect(screen.getByText(message)).toBeVisible();
      expect(props.onMutationError).toHaveBeenCalledTimes(1);
      await waitFor(() => expect(includeAlice()).toBeEnabled());

      prepare();
      await act(async () => trigger());
      expect(screen.queryByText(message)).toBeNull();
      await act(async () => held.succeed());

      await waitFor(() => expect(includeAlice()).toBeEnabled());
      expect(screen.queryByText(message)).toBeNull();
      expect(mock()).toHaveBeenCalledTimes(2);
    },
  );
});

/**
 * A failure can land while the operator's focus is elsewhere, so each one
 * is in a polite live region that is in the page before the failure is,
 * which is what makes a screen reader announce the change. One region
 * only, so the failure is announced once.
 */
describe("user failures are announced", () => {
  /** The live regions and alerts the element sits in. */
  function announcers(el: HTMLElement): Element[] {
    const found: Element[] = [];
    for (let at: Element | null = el; at; at = at.parentElement) {
      const role = at.getAttribute("role");
      if (at.hasAttribute("aria-live") || role === "alert" || role === "status")
        found.push(at);
    }
    return found;
  }

  it("announces a new-password failure in the form's live region", async () => {
    const held = hold(vi.mocked(client.setUserPassword));
    render(<MainContent {...baseProps()} />);
    await submitNewPassword();
    const region = screen.getByTestId("user-password-error-alice");
    expect(region).toHaveAttribute("aria-live", "polite");
    expect(region).toHaveTextContent("");

    await act(async () => held.fail());

    const text = screen.getByText(PASSWORD_FAILURE);
    expect(region).toContainElement(text);
    expect(announcers(text)).toEqual([region]);
  });

  it("announces a strategy failure in the card's live region", async () => {
    const message =
      "Failed to change the Containerfile strategy. Please try again.";
    const held = hold(vi.mocked(client.setUserStrategy));
    render(<MainContent {...baseProps()} />);
    const region = screen.getByTestId("user-failure-alice");
    expect(region).toHaveAttribute("aria-live", "polite");
    expect(region).toHaveTextContent("");

    await act(async () => includeAlice().click());
    await act(async () => held.fail());

    const text = screen.getByText(message);
    expect(region).toContainElement(text);
    expect(announcers(text)).toEqual([region]);
  });
});
