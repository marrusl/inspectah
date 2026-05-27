// e2e/helpers/mock-api.ts
import { Page } from "@playwright/test";
import * as fs from "fs";
import * as path from "path";

export type Preset = "single-host" | "fleet-3" | "empty";
export type RouteOverrides = Record<string, string>;
export type ErrorKind = "500" | "timeout" | "malformed";

const FIXTURES_DIR = path.join(__dirname, "..", "fixtures");

// Lookup table: fixture filename → API route (not convention-based)
const PRESET_ROUTE_MAP: Record<string, string> = {
  "health.json": "/api/health",
  "view.json": "/api/view",
  "ops-empty.json": "/api/ops",
  "changes-empty.json": "/api/changes",
  "viewed-empty.json": "/api/viewed",
  "sections.json": "/api/snapshot/sections",
  "user-preview.json": "/api/user-preview",
  "fleet-view.json": "/api/fleet/view",
};

const PRESET_DIR_MAP: Record<Preset, string> = {
  "single-host": "single-host",
  "fleet-3": "fleet",
  "empty": "empty",
};

// --- Sequence state ---
interface SequenceHandler {
  responses: object[];
  current: number;
  triggers: string[];
}
let sequenceHandlers = new Map<string, SequenceHandler>();

// --- Viewed state (stateful mock for GET+POST /api/viewed) ---
let viewedState: Set<string> | null = null;

function loadFixture(fixturePath: string): string {
  const full = fixturePath.startsWith("/")
    ? fixturePath
    : path.join(FIXTURES_DIR, fixturePath);
  return fs.readFileSync(full, "utf-8");
}

// --- clearMocks: removes all route handlers AND resets sequence/viewed state ---
export async function clearMocks(page: Page): Promise<void> {
  await page.unrouteAll({ behavior: "wait" });
  sequenceHandlers.clear();
  viewedState = null;
}

// --- applyMockApi: wire GET presets, calls clearMocks first ---
export async function applyMockApi(
  page: Page,
  preset: Preset,
  overrides?: RouteOverrides,
): Promise<void> {
  await clearMocks(page);

  const dir = path.join(FIXTURES_DIR, PRESET_DIR_MAP[preset]);

  for (const [filename, route] of Object.entries(PRESET_ROUTE_MAP)) {
    const fixturePath = overrides?.[route] ?? path.join(dir, filename);

    if (typeof fixturePath !== "string" || !fs.existsSync(fixturePath)) continue;

    const body = fs.readFileSync(fixturePath, "utf-8");
    const json = JSON.parse(body);

    await page.route(`**${route}`, (routeObj) => {
      if (routeObj.request().method() !== "GET") {
        routeObj.continue();
        return;
      }

      // Check sequence state first
      const seq = sequenceHandlers.get(route);
      if (seq) {
        const idx = Math.min(seq.current, seq.responses.length - 1);
        routeObj.fulfill({
          status: 200,
          contentType: "application/json",
          body: JSON.stringify(seq.responses[idx]),
        });
        return;
      }

      routeObj.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(json),
      });
    });
  }
}

// --- mockPostResponse: per-test POST handler ---
// Reads { "_status": N, ...body } from fixture, strips _status
// Runtime assertion if JSON fixture missing _status
// Binary fixtures (.tar.gz) hardcode 200
export async function mockPostResponse(
  page: Page,
  route: string,
  fixturePath: string,
): Promise<void> {
  const fullPath = path.join(FIXTURES_DIR, fixturePath);
  const isBinary = fullPath.endsWith(".tar.gz");

  await page.route(`**${route}`, (routeObj) => {
    if (routeObj.request().method() !== "POST") {
      routeObj.continue();
      return;
    }

    if (isBinary) {
      const buffer = fs.readFileSync(fullPath);
      routeObj.fulfill({
        status: 200,
        contentType: "application/gzip",
        headers: { "content-disposition": 'attachment; filename="export.tar.gz"' },
        body: buffer,
      });
      return;
    }

    const raw = fs.readFileSync(fullPath, "utf-8");
    const parsed = JSON.parse(raw);
    if (!("_status" in parsed)) {
      throw new Error(`Fixture ${fixturePath} missing _status field`);
    }
    const { _status, ...body } = parsed;
    routeObj.fulfill({
      status: _status,
      contentType: _status === 204 ? "text/plain" : "application/json",
      body: _status === 204 ? "" : JSON.stringify(body),
    });
  });
}

// --- mockSequence: stateful multi-step workflows ---
// triggerOn: POST route(s) that advance the sequence counter
// When triggered: next GET to the sequenced route returns the next response
export async function mockSequence(
  page: Page,
  route: string,
  responses: string[],
  opts: { triggerOn: string | string[] },
): Promise<void> {
  const triggers = Array.isArray(opts.triggerOn) ? opts.triggerOn : [opts.triggerOn];
  const responseData = responses.map((fp) => JSON.parse(loadFixture(fp)));

  const handler: SequenceHandler = { responses: responseData, current: 0, triggers };
  sequenceHandlers.set(route, handler);

  for (const trigger of triggers) {
    await page.route(`**${trigger}`, (routeObj) => {
      if (routeObj.request().method() !== "POST") {
        routeObj.continue();
        return;
      }
      handler.current = Math.min(handler.current + 1, handler.responses.length - 1);
      routeObj.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(handler.responses[handler.current]),
      });
    });
  }
}

// --- mockViewed: stateful GET+POST /api/viewed ---
export async function mockViewed(
  page: Page,
  initialIds: string[] = [],
): Promise<void> {
  viewedState = new Set(initialIds);

  await page.route("**/api/viewed", async (routeObj) => {
    if (routeObj.request().method() === "GET") {
      routeObj.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ ids: [...(viewedState ?? [])] }),
      });
    } else if (routeObj.request().method() === "POST") {
      try {
        const postBody = routeObj.request().postDataJSON();
        if (postBody?.id) viewedState?.add(postBody.id);
      } catch { /* fire-and-forget, matching real behavior */ }
      routeObj.fulfill({ status: 204, body: "" });
    } else {
      routeObj.continue();
    }
  });
}

// --- mockError: simulate server failures ---
export async function mockError(
  page: Page,
  route: string,
  kind: ErrorKind,
  opts?: { timeoutMs?: number },
): Promise<void> {
  await page.route(`**${route}`, async (routeObj) => {
    switch (kind) {
      case "500":
        routeObj.fulfill({
          status: 500,
          contentType: "application/json",
          body: JSON.stringify({ error: "internal server error" }),
        });
        break;
      case "timeout": {
        const ms = opts?.timeoutMs ?? 1000;
        await new Promise((r) => setTimeout(r, ms));
        routeObj.abort("timedout");
        break;
      }
      case "malformed":
        routeObj.fulfill({
          status: 200,
          contentType: "application/json",
          body: "this is not valid json {{{",
        });
        break;
    }
  });
}
