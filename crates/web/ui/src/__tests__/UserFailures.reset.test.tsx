/// <reference types="vite/client" />
/**
 * MainContent keeps a failed set-password request by username so the
 * failure outlives the user card. A reset replaces every user record, so
 * App tells MainContent when one succeeds, and a request that was in
 * flight across the reset must not report against the new record.
 *
 * Mounts `App` against a recorded single-host layout with one user added.
 * `fetch` is the only thing replaced; the set-password request is held
 * open until the test fails it.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
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
const RESET_URL = "/api/reset";
const PASSWORD_FAILURE = "Failed to set password. Please try again.";

let failPassword: () => void = () => {};
let resets = 0;

function serve() {
  const recorded = fixture("view");
  const view = {
    ...recorded,
    // One applied op, so Reset to defaults is enabled.
    stats: { ...(recorded.stats as object), ops_applied: 1 },
    users_groups_decisions: [
      {
        name: "alice",
        uid: 1000,
        gid: 1000,
        shell: "/bin/bash",
        home: "/home/alice",
        include: true,
        classification: "interactive",
        containerfile_strategy: "skip",
        password_choice: "new",
      },
    ],
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
        await new Promise<void>((resolve) => {
          failPassword = resolve;
        });
        return {
          ok: false,
          status: 500,
          json: async () => ({ error: "connection reset" }),
        };
      }
      if (url === RESET_URL) {
        resets += 1;
        return { ok: true, status: 200, json: async () => view };
      }
      if (!(url in routes)) {
        return { ok: false, status: 404, json: async () => ({ error: url }) };
      }
      return { ok: true, status: 200, json: async () => routes[url] };
    }),
  );
}

beforeEach(() => {
  failPassword = () => {};
  resets = 0;
  vi.mocked(crypt.sha512Crypt).mockResolvedValue("$6$salt$hash");
  vi.spyOn(console, "error").mockImplementation(() => {});
  serve();
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("user failures across a reset", () => {
  it("drops the failure of a set-password request that was in flight across a reset", async () => {
    render(<App />);
    fireEvent.click(
      await screen.findByRole("link", { name: /^Users & Groups/ }),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "Expand alice details" }),
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

    fireEvent.click(screen.getByTestId("reset-defaults-btn"));
    await act(async () => screen.getByTestId("reset-confirm-yes").click());
    await waitFor(() => expect(resets).toBe(1));
    await waitFor(() =>
      expect(screen.getByTestId("reset-defaults-btn")).toBeEnabled(),
    );

    await act(async () => failPassword());

    await waitFor(() =>
      expect(
        screen.getByRole("checkbox", { name: "Include alice" }),
      ).toBeEnabled(),
    );
    expect(screen.queryByText(PASSWORD_FAILURE)).toBeNull();
  });
});
