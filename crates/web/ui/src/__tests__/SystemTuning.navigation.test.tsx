/// <reference types="vite/client" />
/**
 * System Tuning navigation through the application (spec § 8, case 22).
 *
 * Mounts `App` against the JSON the real handlers return for each layout
 * (recorded by `crates/web/tests/system_tuning_fixtures_test.rs` into
 * `fixtures/system-tuning/`). `fetch` is the only thing replaced, so the
 * API client, the hooks and the router all run as they do in a browser.
 * Each case clicks the System Tuning sidebar entry by its accessible name
 * and checks what the operator sees.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import App from "../App";

/** Every recorded handler body, keyed by file name. */
const FIXTURES = import.meta.glob<{ default: unknown }>(
  "./fixtures/system-tuning/*.json",
  { eager: true },
);

function fixture(name: string): unknown {
  const entry = FIXTURES[`./fixtures/system-tuning/${name}.json`];
  if (!entry) throw new Error(`no recorded fixture ${name}.json`);
  return entry.default;
}

/** The section's label, as `/api/groups` and the aggregate view send it. */
const SYSTEM_TUNING = "System Tuning";

/** Requests the app sent that would change the session. */
let mutatingRequests: string[] = [];

/**
 * Answer the client's requests from one recorded layout. Endpoints the
 * recorder does not capture, which no assertion here reads, answer empty.
 */
function serveLayout(layout: string) {
  const routes: Record<string, unknown> = {
    "/api/health": fixture(`${layout}.health`),
    "/api/groups": fixture(`${layout}.groups`),
    "/api/view": fixture(`${layout}.view`),
    "/api/snapshot/sections": [],
    "/api/viewed": { ids: [] },
    "/api/ops": [],
  };
  if (layout === "aggregate") {
    routes["/api/aggregate/view"] = fixture("aggregate.aggregate-view");
  }
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      const method = init?.method ?? "GET";
      if (method !== "GET" && url !== "/api/viewed") {
        mutatingRequests.push(`${method} ${url}`);
      }
      if (!(url in routes)) {
        return { ok: false, status: 404, json: async () => ({ error: url }) };
      }
      return { ok: true, status: 200, json: async () => routes[url] };
    }),
  );
}

/**
 * The sidebar link for `label`, found by role and accessible name, with
 * its badge text.
 */
async function sidebarEntry(navName: string, label: string, badge: string) {
  const nav = await screen.findByRole("navigation", { name: navName });
  const entry = await within(nav).findByRole("link", {
    name: new RegExp(`^${label} `),
  });
  expect(entry.querySelector(".pf-v6-c-badge")).toHaveTextContent(
    new RegExp(`^${badge}$`),
  );
  return entry;
}

/** Select a sidebar entry from the keyboard: focus it, press Enter. */
async function activateFromKeyboard(entry: HTMLElement) {
  entry.focus();
  expect(entry).toHaveFocus();
  await userEvent.keyboard("{Enter}");
}

/** A paragraph of the panel whose text is exactly `text`. */
function paragraph(panel: HTMLElement, text: string) {
  return within(panel).getByText(
    (_, el) => el?.tagName === "P" && el.textContent === text,
  );
}

/** The panel holds nothing an operator could use to change the session. */
function expectReadOnly(panel: HTMLElement) {
  expect(within(panel).queryAllByRole("checkbox")).toHaveLength(0);
  expect(within(panel).queryAllByRole("switch")).toHaveLength(0);
  expect(within(panel).queryAllByRole("button")).toHaveLength(0);
}

/** Open System Tuning in a single-host session and return the panel. */
async function openSingleHost(layout: string) {
  serveLayout(layout);
  render(<App />);
  // The badge counts tuned rows only, and no layout here has one.
  const entry = await sidebarEntry("Sections", SYSTEM_TUNING, "0");
  expect(within(entry).getByLabelText("0 decisions")).toBeInTheDocument();
  await activateFromKeyboard(entry);
  expect(
    await screen.findByRole("heading", { level: 2, name: SYSTEM_TUNING }),
  ).toBeInTheDocument();
  const panel = await screen.findByRole("region", { name: "Sysctl" });
  expectReadOnly(panel);
  return panel;
}

/** The settings table's body rows; `null` when there is no table. */
function settingsRows(panel: HTMLElement): HTMLElement[] | null {
  const table = within(panel).queryByRole("table", { name: "Sysctl settings" });
  if (!table) return null;
  // The first row is the header.
  return within(table).getAllByRole("row").slice(1);
}

/** A fresh in-memory store per test; jsdom here has no localStorage. */
function stubLocalStorage() {
  const store = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => void store.set(key, value),
    removeItem: (key: string) => void store.delete(key),
    clear: () => store.clear(),
    key: () => null,
    get length() {
      return store.size;
    },
  });
}

beforeEach(() => {
  mutatingRequests = [];
  stubLocalStorage();
});

afterEach(() => {
  vi.unstubAllGlobals();
  expect(mutatingRequests).toEqual([]);
});

describe("System Tuning navigation, single host", () => {
  it("shows a host with sysctl files and no tuned selection", async () => {
    const panel = await openSingleHost("files-no-tuned");
    paragraph(
      panel,
      "These values are trusted host configuration. The files that set them are carried as they were, not rewritten.",
    );
    const rows = settingsRows(panel)!;
    expect(rows.map((r) => within(r).getAllByRole("cell")[0].textContent)).toEqual([
      "net.ipv4.ip_forward",
      "vm.swappiness",
    ]);
    paragraph(
      panel,
      "vm.swappiness runs at 60; the files request 10 (/etc/sysctl.d/90-tuning.conf:1).",
    );
    expect(screen.queryByText("No system tuning detected.")).toBeNull();
  });

  it("opens a sysctl file's Configuration Files row from the panel", async () => {
    const panel = await openSingleHost("files-no-tuned");
    const files = within(panel).getByRole("table", { name: "Sysctl files" });
    await userEvent.click(
      within(files).getByRole("link", { name: "/etc/sysctl.d/90-tuning.conf" }),
    );
    expect(
      await screen.findByRole("heading", { level: 2, name: "Config Files" }),
    ).toBeInTheDocument();
    // The app reveals and focuses the row it was sent to.
    const row = await screen.findByTestId(
      "decision-item-configs:/etc/sysctl.d/90-tuning.conf",
    );
    await waitFor(() => expect(row).toHaveFocus());
  });

  it("shows a mask-only host with an empty settings table", async () => {
    const panel = await openSingleHost("mask-only");
    expect(settingsRows(panel)).toEqual([]);
    paragraph(
      panel,
      "/etc/sysctl.d/50-x.conf disables /usr/lib/sysctl.d/50-x.conf entirely, on the target as well, so keys the target's version of that vendor file sets will not apply from it. Compare it with the target image's copy.",
    );
  });

  it("shows a glob-only host with an empty settings table", async () => {
    const panel = await openSingleHost("glob-only");
    expect(settingsRows(panel)).toEqual([]);
    paragraph(
      panel,
      "/etc/sysctl.d/50-rp.conf:1: glob net.ipv4.conf.*.rp_filter set to 2, not expanded.",
    );
  });

  it("shows the unavailable line when sysctl -a failed", async () => {
    const panel = await openSingleHost("sysctl-a-fails");
    paragraph(
      panel,
      "Sysctl explanation unavailable: degraded: sysctl: sysctl -a failed with exit code 1. Sysctl files are still carried as Configuration Files.",
    );
    expect(settingsRows(panel)).toBeNull();
  });

  it("shows the unknown text naming the outside link", async () => {
    const panel = await openSingleHost("outside-link-overlap");
    paragraph(
      panel,
      "Values and drift are not stated for this host, because inspectah does not know every input below as the host had it. One unknown line can set, override or cancel any key.",
    );
    paragraph(
      panel,
      "/etc/sysctl.d/90-site.conf links to /usr/share/site/tuning.conf, outside /etc and the sysctl directories, and inspectah did not read it.",
    );
    expect(settingsRows(panel)).toBeNull();
    expect(within(panel).queryByText(/swappiness/)).toBeNull();
  });
});

describe("System Tuning navigation, aggregate", () => {
  it("shows the aggregate line and a 0 / 0 badge", async () => {
    serveLayout("aggregate");
    render(<App />);
    const entry = await sidebarEntry("Aggregate sections", SYSTEM_TUNING, "0 / 0");
    await activateFromKeyboard(entry);
    const panel = await screen.findByRole("region", { name: "Sysctl" });
    expectReadOnly(panel);
    paragraph(
      panel,
      "Sysctl explanation is per host and is not available across a fleet yet; each host's own scan has it. Sysctl files are carried as Configuration Files, with a variant per differing content.",
    );
    await waitFor(() => expect(entry).toHaveAttribute("aria-current", "page"));
  });
});
