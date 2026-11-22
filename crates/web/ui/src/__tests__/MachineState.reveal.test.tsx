/// <reference types="vite/client" />
/**
 * GlobalSearch and reveal of machine-state rows (spec § 8, Search and
 * reveal), through the whole application against the bodies the real
 * handlers return for the machine-state host (recorded by
 * `crates/web/tests/system_tuning_fixtures_test.rs`). `fetch` is the only
 * thing replaced.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import App from "../App";

const FIXTURES = import.meta.glob<{ default: unknown }>(
  "./fixtures/system-tuning/machine-state.*.json",
  { eager: true },
);

function fixture(name: string): unknown {
  const entry = FIXTURES[`./fixtures/system-tuning/machine-state.${name}.json`];
  if (!entry) throw new Error(`no recorded fixture machine-state.${name}.json`);
  return entry.default;
}

const KEYTAB = "/etc/krb5.keytab";
const HOSTNAME = "/etc/hostname";

beforeEach(() => {
  const routes: Record<string, unknown> = {
    "/api/health": fixture("health"),
    "/api/groups": fixture("groups"),
    "/api/view": fixture("view"),
    "/api/snapshot/sections": [],
    "/api/viewed": { ids: [] },
    "/api/ops": [],
  };
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) => {
      if (!(url in routes)) {
        return { ok: false, status: 404, json: async () => ({ error: url }) };
      }
      return { ok: true, status: 200, json: async () => routes[url] };
    }),
  );
  // jsdom has no layout; the reveal scrolls before it focuses.
  Element.prototype.scrollIntoView = vi.fn();
});
afterEach(() => {
  vi.unstubAllGlobals();
});

async function searchAndPick(query: string, title: string) {
  const user = userEvent.setup();
  const input = await screen.findByRole("textbox", {
    name: "Search all sections",
  });
  await user.type(input, query);
  const results = await screen.findByRole("listbox", {
    name: "Search results",
  });
  await user.click(within(results).getByText(title));
}

describe("machine-state rows in GlobalSearch", () => {
  it("reveals a never row: the block expands and the row takes focus", async () => {
    render(<App />);
    await searchAndPick("keytab", KEYTAB);

    const row = await screen.findByTestId(`machine-state-row-${KEYTAB}`);
    expect(
      screen.getByRole("button", { name: /^Machine state/ }),
    ).toHaveAttribute("aria-expanded", "true");
    await waitFor(() => expect(row).toHaveFocus());
  });

  it("reveals a plain row by focusing its button", async () => {
    render(<App />);
    await searchAndPick("hostname", HOSTNAME);

    const row = await screen.findByTestId(`machine-state-row-${HOSTNAME}`);
    await waitFor(() =>
      expect(
        within(row).getByRole("button", { name: `Include ${HOSTNAME}` }),
      ).toHaveFocus(),
    );
  });
});

describe("the Export dialog's machine-state link", () => {
  it("clears a Configuration query before it reveals the block", async () => {
    const user = userEvent.setup();
    render(<App />);
    const nav = await screen.findByRole("navigation", { name: "Sections" });
    await user.click(
      await within(nav).findByRole("link", { name: /^Configuration Files / }),
    );
    await screen.findByRole("heading", { level: 2, name: "Config Files" });

    // A query that matches the sysctl config and no machine-state row.
    await user.keyboard("/");
    const search = await screen.findByLabelText("Filter section items");
    await user.type(search, "sysctl");
    expect(screen.queryByTestId(`machine-state-row-${KEYTAB}`)).toBeNull();

    await user.click(screen.getByRole("button", { name: "Export" }));
    const dialog = await screen.findByRole("dialog");
    await user.click(
      within(dialog).getByRole("button", { name: "Review machine state" }),
    );

    // Configuration was already the active section; the query is cleared
    // all the same, so the block opens with every row.
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: /^Machine state/ }),
      ).toHaveAttribute("aria-expanded", "true"),
    );
    expect(screen.getByLabelText("Filter section items")).toHaveValue("");
    expect(
      await screen.findByTestId(`machine-state-row-${KEYTAB}`),
    ).toBeVisible();
    expect(screen.getByTestId(`machine-state-row-${HOSTNAME}`)).toBeVisible();
  });
});
