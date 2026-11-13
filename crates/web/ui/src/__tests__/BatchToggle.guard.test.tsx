/// <reference types="vite/client" />
/**
 * The sidebar's batch toggle holds a pending guard while its request is in
 * flight. On a narrow screen the sidebar is an overlay that App mounts only
 * while it is open, so closing it must not drop the guard.
 *
 * Mounts `App` against a recorded single-host layout. `fetch` is the only
 * thing replaced; the batch-toggle request is held open until the test
 * settles it.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import App from "../App";

const FIXTURES = import.meta.glob<{ default: unknown }>(
  "./fixtures/system-tuning/glob-only.*.json",
  { eager: true },
);

function fixture(name: string): unknown {
  const entry = FIXTURES[`./fixtures/system-tuning/glob-only.${name}.json`];
  if (!entry) throw new Error(`no recorded fixture glob-only.${name}.json`);
  return entry.default;
}

const NARROW = "(max-width: 1023px)";
const BATCH_URL = "/api/batch-toggle/packages-group";

let batchRequests = 0;
let settleBatch: () => void = () => {};
const originalMatchMedia = window.matchMedia;

function serve() {
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
      if (url === BATCH_URL) {
        batchRequests += 1;
        await new Promise<void>((resolve) => {
          settleBatch = resolve;
        });
        return { ok: true, status: 200, json: async () => routes["/api/view"] };
      }
      if (!(url in routes)) {
        return { ok: false, status: 404, json: async () => ({ error: url }) };
      }
      return { ok: true, status: 200, json: async () => routes[url] };
    }),
  );
}

/** Report a narrow viewport, so App shows the sidebar as an overlay. */
function useNarrowViewport() {
  Object.defineProperty(window, "matchMedia", {
    writable: true,
    value: (query: string) => ({
      matches: query === NARROW,
      media: query,
      onchange: null,
      addListener: () => {},
      removeListener: () => {},
      addEventListener: () => {},
      removeEventListener: () => {},
      dispatchEvent: () => false,
    }),
  });
}

function overlay(): HTMLElement {
  const el = document.getElementById("inspectah-sidebar-overlay");
  if (!el) throw new Error("sidebar overlay is not open");
  return el;
}

/** Ctrl+Shift+A on the Packages entry: include every package. */
function includeAllPackages() {
  const entry = within(overlay()).getByRole("link", { name: /^Packages / });
  entry.focus();
  fireEvent.keyDown(entry, { key: "a", ctrlKey: true, shiftKey: true });
}

const kebab = () => within(overlay()).getByTestId("kebab-packages-group");

beforeEach(() => {
  batchRequests = 0;
  settleBatch = () => {};
  serve();
  useNarrowViewport();
});

afterEach(() => {
  vi.unstubAllGlobals();
  Object.defineProperty(window, "matchMedia", {
    writable: true,
    value: originalMatchMedia,
  });
});

describe("batch toggle guard across the sidebar overlay", () => {
  it("suppresses a repeat after the overlay closes and reopens, until the request settles", async () => {
    render(<App />);
    fireEvent.click(
      await screen.findByRole("button", { name: "Open navigation" }),
    );

    await act(async () => includeAllPackages());
    expect(batchRequests).toBe(1);

    fireEvent.click(screen.getByRole("button", { name: "Close navigation" }));
    expect(document.getElementById("inspectah-sidebar-overlay")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Open navigation" }));

    expect(kebab()).toBeDisabled();
    await act(async () => includeAllPackages());
    expect(batchRequests).toBe(1);

    await act(async () => settleBatch());
    await waitFor(() => expect(kebab()).toBeEnabled());

    await act(async () => includeAllPackages());
    expect(batchRequests).toBe(2);
  });
});
