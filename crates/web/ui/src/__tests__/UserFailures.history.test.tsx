/// <reference types="vite/client" />
/**
 * Undo and Redo keep a user in the view but can replace the user's record.
 * App compares each user's record before and after a successful step and
 * tells MainContent which ones changed, so a failure about a replaced
 * record goes, and a failure on a user the step did not touch stays.
 *
 * Mounts `App` against a recorded single-host layout with two users added.
 * `fetch` is the only thing replaced; the set-password request fails.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import App from "../App";
import * as crypt from "../utils/crypt";

vi.mock("../utils/crypt", () => ({
  sha512Crypt: vi.fn(),
}));

const FIXTURES = import.meta.glob<{ default: unknown }>(
  "./fixtures/system-tuning/glob-only.*.json",
  { eager: true },
);

function fixture(name: string): Record<string, unknown> {
  const entry = FIXTURES[`./fixtures/system-tuning/glob-only.${name}.json`];
  if (!entry) throw new Error(`no recorded fixture glob-only.${name}.json`);
  return entry.default as Record<string, unknown>;
}

const PASSWORD_URL = "/api/user-password";
const UNDO_URL = "/api/undo";
const PASSWORD_FAILURE = "Failed to set password. Please try again.";

let undos = 0;
let views = 0;

const ALICE = {
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
const BOB = { ...ALICE, name: "bob", uid: 1001, gid: 1001, home: "/home/bob" };

/** Serves the view, and answers Undo with the users given. */
function serve(usersAfterUndo: object[]) {
  const recorded = fixture("view");
  const stats = { ...(recorded.stats as object), ops_applied: 1 };
  const view = {
    ...recorded,
    stats: { ...stats, can_undo: true },
    users_groups_decisions: [ALICE, BOB],
  };
  const undone = {
    ...recorded,
    stats: { ...stats, can_redo: true },
    users_groups_decisions: usersAfterUndo,
  };
  const routes: Record<string, unknown> = {
    "/api/health": fixture("health"),
    "/api/groups": fixture("groups"),
    "/api/view": view,
    "/api/snapshot/sections": [],
    "/api/viewed": { ids: [] },
    "/api/ops": [],
  };
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) => {
      if (url === PASSWORD_URL) {
        return {
          ok: false,
          status: 500,
          json: async () => ({ error: "connection reset" }),
        };
      }
      if (url === UNDO_URL) {
        undos += 1;
        return { ok: true, status: 200, json: async () => undone };
      }
      if (url === "/api/view") views += 1;
      if (!(url in routes)) {
        return { ok: false, status: 404, json: async () => ({ error: url }) };
      }
      return { ok: true, status: 200, json: async () => routes[url] };
    }),
  );
}

beforeEach(() => {
  undos = 0;
  views = 0;
  vi.mocked(crypt.sha512Crypt).mockResolvedValue("$6$salt$hash");
  vi.spyOn(console, "error").mockImplementation(() => {});
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

/** Fails a new password for alice, then undoes once. */
async function failPasswordThenUndo() {
  render(<App />);
  fireEvent.click(await screen.findByRole("link", { name: /^Users & Groups/ }));
  fireEvent.click(
    await screen.findByRole("button", { name: "Expand alice details" }),
  );
  fireEvent.click(
    screen.getAllByRole("button", { name: /Password options/ })[0],
  );
  for (const label of ["Password", "Confirm password"]) {
    fireEvent.change(screen.getByLabelText(label), {
      target: { value: "hunter2" },
    });
  }
  await act(async () =>
    screen.getByRole("button", { name: "Set password" }).click(),
  );
  expect(await screen.findByText(PASSWORD_FAILURE)).toBeVisible();

  await waitFor(() =>
    expect(screen.getByRole("button", { name: "Undo" })).toBeEnabled(),
  );
  const viewsBefore = views;
  await act(async () => screen.getByRole("button", { name: "Undo" }).click());
  await waitFor(() => expect(undos).toBe(1));
  // The successful Undo refetches the view; wait until that lands.
  await waitFor(() => expect(views).toBeGreaterThan(viewsBefore));
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "Undo" })).toBeEnabled(),
  );
}

describe("user failures across Undo", () => {
  it("clears the failure of a user whose record the Undo changed", async () => {
    serve([{ ...ALICE, password_choice: "none" }, BOB]);
    await failPasswordThenUndo();
    await waitFor(() =>
      expect(screen.queryByText(PASSWORD_FAILURE)).toBeNull(),
    );
  });

  it("keeps the failure of a user the Undo did not change", async () => {
    serve([ALICE, { ...BOB, containerfile_strategy: "useradd" }]);
    await failPasswordThenUndo();
    expect(screen.getByText(PASSWORD_FAILURE)).toBeVisible();
  });
  it("keeps the failure when the Undo changed no user at all", async () => {
    serve([ALICE, BOB]);
    await failPasswordThenUndo();
    expect(screen.getByText(PASSWORD_FAILURE)).toBeVisible();
  });
});
