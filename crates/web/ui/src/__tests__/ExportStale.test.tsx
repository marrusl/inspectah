/// <reference types="vite/client" />
/**
 * A stale export, through the owner of the displayed view. Another tab
 * changes the session after the Export dialog opens, so the first export
 * gets a 409. The dialog must not offer Export again until App shows the
 * current generation, and the next export must post that generation.
 *
 * Mounts `App` against a recorded single-host layout. `fetch` is the only
 * thing replaced: it plays a server whose generation moves on at the first
 * export, and it holds every later view request until the test lets it go.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import App from "../App";

const FIXTURES = import.meta.glob<{ default: unknown }>(
  "./fixtures/system-tuning/glob-only.*.json",
  { eager: true },
);

function fixture(name: string): Record<string, unknown> {
  const entry = FIXTURES[`./fixtures/system-tuning/glob-only.${name}.json`];
  if (!entry) throw new Error(`no recorded fixture glob-only.${name}.json`);
  return entry.default as Record<string, unknown>;
}

const TARBALL_URL = "/api/tarball";
const VIEW_URL = "/api/view";
/** Enough rounds for a refresh chain to show itself; the fix needs one. */
const MAX_RELEASE_ROUNDS = 5;

let generation = 0;
let exportsPosted: { generation: number; status: number }[] = [];
let heldViews: (() => void)[] = [];

function serve() {
  const recorded = fixture("view");
  generation = recorded.generation as number;
  const routes: Record<string, unknown> = {
    "/api/health": fixture("health"),
    "/api/groups": fixture("groups"),
    "/api/snapshot/sections": [],
    "/api/viewed": { ids: [] },
    "/api/ops": [],
  };
  let changedElsewhere = false;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      if (url === TARBALL_URL) {
        if (!changedElsewhere) {
          // Another tab changes the session before this export lands.
          changedElsewhere = true;
          generation += 1;
        }
        const posted = JSON.parse(String(init?.body)).generation as number;
        const status = posted === generation ? 200 : 409;
        exportsPosted.push({ generation: posted, status });
        if (status === 409) {
          return {
            ok: false,
            status,
            json: async () => ({ error: "stale generation" }),
          };
        }
        return {
          ok: true,
          status,
          headers: new Headers({ "x-export-warnings": "[]" }),
          blob: async () => new Blob(["tarball"]),
        };
      }
      if (url === VIEW_URL) {
        const view = { ...recorded, generation };
        if (changedElsewhere) {
          await new Promise<void>((resolve) => heldViews.push(resolve));
        }
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
  exportsPosted = [];
  heldViews = [];
  URL.createObjectURL = vi.fn(() => "blob:http://localhost/fake-url");
  URL.revokeObjectURL = vi.fn();
  serve();
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("stale export through App", () => {
  it("keeps Export off until App shows the new generation, then exports it", async () => {
    render(<App />);
    const stale = generation;
    fireEvent.click(await screen.findByRole("button", { name: "Export" }));
    const dialog = await screen.findByRole("dialog", {
      name: "Export tarball",
    });
    // While a request is out the button is named "Loading... Export".
    const exportButton = () =>
      within(dialog).getByRole("button", { name: /Export$/ });

    await act(async () => fireEvent.click(exportButton()));
    expect(exportsPosted).toEqual([{ generation: stale, status: 409 }]);
    expect(within(dialog).getByText("Stale state")).toBeVisible();

    // Let the held view requests land one at a time, and try Export before
    // each: while a refresh is outstanding it must not post anything.
    for (
      let round = 0;
      round < MAX_RELEASE_ROUNDS && heldViews.length > 0;
      round++
    ) {
      await act(async () => fireEvent.click(exportButton()));
      await act(async () => heldViews.shift()!());
    }
    expect(heldViews).toEqual([]);
    expect(exportsPosted).toEqual([{ generation: stale, status: 409 }]);

    // The dialog now shows the generation App displays, and says so.
    expect(dialog).toHaveTextContent(`Generation: ${stale + 1}`);
    expect(within(dialog).getByText(/export again/i)).toBeVisible();
    expect(exportButton()).toBeEnabled();

    await act(async () => fireEvent.click(exportButton()));
    expect(exportsPosted).toEqual([
      { generation: stale, status: 409 },
      { generation: stale + 1, status: 200 },
    ]);
    expect(URL.createObjectURL).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});
