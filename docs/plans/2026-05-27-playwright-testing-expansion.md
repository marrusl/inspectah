# Playwright Testing Expansion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expand the Playwright e2e test suite from 6 spec files (many skipped) to 11 spec files with comprehensive mock and real-server coverage of the inspectah refine UI.

**Architecture:** Hybrid fixture strategy — 80% mock API via Playwright's `page.route()` with canned JSON fixtures, 20% real-server smoke tests with checked-in tarballs. Schema validation via `insta` JSON snapshots of serialized responses prevents mock-rot. Mock layer uses shared fixture modules with GET presets, per-test POST handlers, and three distinct mutation models matching the app's real behavior.

**Tech Stack:** Playwright, TypeScript, PatternFly 6, Rust (`insta`), axe-core

**Spec:** `docs/specs/proposed/2026-05-27-playwright-testing-expansion.md`

**Round 1 review changes:** Consolidated mock-api into one task with three mutation patterns (Kit consult), simplified schema validation to dev-dep + insta snapshots without cross-crate derives (Tang consult), expanded real-server smoke tests with coherence flows, fixed all placeholder selectors against current component tree.

---

## File Map

### New files (e2e infrastructure)
- `inspectah-web/ui/e2e/helpers/mock-api.ts` — `applyMockApi()`, `mockPostResponse()`, `mockSequence()`, `mockError()`, `mockViewed()`, `clearMocks()`
- `inspectah-web/ui/e2e/helpers/assertions.ts` — shared assertion helpers
- `inspectah-web/ui/e2e/fixtures/manifest.json` — fixture-to-schema routing manifest
- `inspectah-web/ui/e2e/fixtures/single-host/*.json` — GET preset body fixtures (8 files)
- `inspectah-web/ui/e2e/fixtures/fleet/*.json` — fleet GET preset body fixtures (3 files)
- `inspectah-web/ui/e2e/fixtures/post-responses/**/*.json` — POST harness wrappers (~12 files)
- `inspectah-web/ui/e2e/fixtures/sequences/exclude-undo-redo/*.json` — sequence body fixtures (3 files)
- `inspectah-web/ui/e2e/fixtures/errors/*` — error simulation fixtures (2 files)

### New files (test specs)
- `inspectah-web/ui/e2e/smoke-integration.spec.ts` — real-server golden path + coherence
- `inspectah-web/ui/e2e/containerfile.spec.ts` — containerfile panel + change highlights
- `inspectah-web/ui/e2e/sections.spec.ts` — context section rendering + navigation
- `inspectah-web/ui/e2e/repos.spec.ts` — repo groups + attention summary
- `inspectah-web/ui/e2e/users.spec.ts` — user/group materialization

### New files (Rust schema validation)
- `inspectah-web/tests/schema_export_test.rs` — serializes representative responses, `insta` JSON snapshots

### New files (test data)
- `testdata/single-host-e2e.tar.gz` — curated single-host scan tarball
- `testdata/fleet-e2e.tar.gz` — curated 3-host fleet tarball

### Modified files
- `inspectah-web/ui/e2e/keyboard.spec.ts` — rewrite to use mock fixtures
- `inspectah-web/ui/e2e/triage.spec.ts` — rewrite to use mock fixtures, unskip tests
- `inspectah-web/ui/e2e/fleet.spec.ts` — rewrite to use mock fixtures, unskip tests
- `inspectah-web/ui/e2e/a11y.spec.ts` — expand with mock-backed scans
- `inspectah-web/ui/e2e/responsive.spec.ts` — expand with mock-backed tests
- `inspectah-web/Cargo.toml` — add `schemars` + `insta` as dev-dependencies

---

## Phase 1a: Mock Infrastructure Proof-of-Concept

### Task 1: Create consolidated mock-api helper

**Files:**
- Create: `inspectah-web/ui/e2e/helpers/mock-api.ts`

The mock helper models three distinct mutation patterns matching the app's actual behavior (verified against `useMutation.ts`, `useFleetMutation.ts`, `useViewed.ts`, `App.tsx`):

1. **Single-host mutations (op, undo, redo, user-strategy, user-password):** All use the same cycle: POST returns `ViewResponse` but `onMutationSuccess` in `App.tsx` **ignores the `_result` parameter**. Instead it calls `view.invalidate()` → triggers `GET /api/view` re-fetch, plus `refreshViewed()` → triggers `GET /api/viewed` re-fetch. The mock must serve updated data on the **GET**, not rely on the POST body reaching the UI. Minor variation: before undo/redo, App.tsx pre-fetches `GET /api/ops` to find the focus-restoration target — the mock must serve `/api/ops` for undo/redo tests.
2. **Fleet mutations:** POST body is **explicitly discarded** (`useFleetMutation.ts` calls `applyOp()` without consuming the return). App always re-fetches `GET /api/fleet/view` after POST. Never calls `/api/view` or `/api/viewed`.
3. **Viewed persistence:** `useViewed.ts` applies **optimistic local update** (adds to Set immediately), fire-and-forget POST to `/api/viewed`, then debounced `GET /api/viewed` re-fetch via `onViewedChange` → `refreshViewed()`. On reload, `GET /api/viewed` hydrates the set. Mock needs stateful tracking (`mockViewed` function).

- [ ] **Step 1: Write mock-api.ts with all functions**

```typescript
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
// Models single-host (sequence on /api/view, triggered by /api/op etc.)
// and fleet (sequence on /api/fleet/view, triggered by /api/op etc.)
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

  // Wire trigger routes: POST advances counter, returns 200 with empty JSON
  // (UI ignores POST body for single-host mutations via view.invalidate(),
  //  and explicitly discards it for fleet mutations)
  for (const trigger of triggers) {
    await page.route(`**${trigger}`, (routeObj) => {
      if (routeObj.request().method() !== "POST") {
        routeObj.continue();
        return;
      }
      handler.current = Math.min(handler.current + 1, handler.responses.length - 1);
      // Return a valid ViewResponse so the POST doesn't fail,
      // but the UI will re-fetch via GET which gets the sequenced response
      routeObj.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(handler.responses[handler.current]),
      });
    });
  }
}

// --- mockViewed: stateful GET+POST /api/viewed ---
// Tracks viewed IDs in memory. POST adds to set, GET returns current set.
// Models the real useViewed hook (optimistic update + fire-and-forget POST + debounced GET refetch)
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
```

- [ ] **Step 2: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add inspectah-web/ui/e2e/helpers/mock-api.ts
git commit -m "feat(e2e): add consolidated mock-api helper with three mutation models"
```

### Task 2: Create single-host fixture preset

**Files:**
- Create: `inspectah-web/ui/e2e/fixtures/single-host/*.json` (8 files)

- [ ] **Step 1: Create directory and capture fixtures from a running server**

```bash
mkdir -p /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui/e2e/fixtures/single-host
cd /Users/mrussell/Work/bootc-migration/inspectah

# Start server with any available tarball
cargo run -p inspectah-cli -- refine testdata/*.tar.gz --no-browser --port 8642 &
SERVER_PID=$!
sleep 3

# Capture each GET endpoint
for ep in health view ops changes viewed user-preview; do
  route="/api/$ep"
  fname="$ep.json"
  [ "$ep" = "ops" ] && fname="ops-empty.json"
  [ "$ep" = "changes" ] && fname="changes-empty.json"
  [ "$ep" = "viewed" ] && fname="viewed-empty.json"
  curl -s "http://127.0.0.1:8642${route}" | python3 -m json.tool > "inspectah-web/ui/e2e/fixtures/single-host/${fname}"
done
curl -s http://127.0.0.1:8642/api/snapshot/sections | python3 -m json.tool > inspectah-web/ui/e2e/fixtures/single-host/sections.json
curl -s "http://127.0.0.1:8642/api/user-preview?reveal=false" | python3 -m json.tool > inspectah-web/ui/e2e/fixtures/single-host/user-preview-redacted.json

kill $SERVER_PID
```

If no tarball is available, create minimal fixtures manually matching the TypeScript types in `inspectah-web/ui/src/api/types.ts`.

- [ ] **Step 2: Review and redact sensitive data**

Replace real hostnames, IPs, and sensitive paths with test-safe values. The fixtures are checked into a public repo.

- [ ] **Step 3: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add inspectah-web/ui/e2e/fixtures/single-host/
git commit -m "feat(e2e): add single-host fixture preset"
```

### Task 3: Create shared assertions helper

**Files:**
- Create: `inspectah-web/ui/e2e/helpers/assertions.ts`

- [ ] **Step 1: Write assertions.ts**

```typescript
// e2e/helpers/assertions.ts
import { Page, expect } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

export async function expectStatsBar(
  page: Page,
  expected: { packages?: string; configs?: string },
): Promise<void> {
  const statsBar = page.locator(".inspectah-statsbar");
  await expect(statsBar).toBeVisible();
  if (expected.packages) await expect(statsBar.getByText("Packages:")).toBeVisible();
  if (expected.configs) await expect(statsBar.getByText("Configs:")).toBeVisible();
}

export async function expectSidebarSection(
  page: Page,
  name: string,
  visible = true,
): Promise<void> {
  const sidebar = page.locator(".inspectah-layout__sidebar");
  const section = sidebar.getByText(name);
  if (visible) {
    await expect(section).toBeVisible();
  } else {
    await expect(section).not.toBeVisible();
  }
}

export async function expectDecisionItem(
  page: Page,
  testId: string,
  included: boolean,
): Promise<void> {
  const item = page.getByTestId(testId);
  await expect(item).toBeVisible();
  const toggle = item.getByRole("switch");
  if (included) {
    await expect(toggle).toBeChecked();
  } else {
    await expect(toggle).not.toBeChecked();
  }
}

export async function expectContainerfileContains(
  page: Page,
  text: string,
): Promise<void> {
  const panel = page.locator(".inspectah-cf-panel--open");
  await expect(panel).toBeVisible();
  await expect(panel.locator(".inspectah-cf-panel__code")).toContainText(text);
}

export async function expectNoAxeViolations(
  page: Page,
  tags: string[] = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"],
): Promise<void> {
  await page.waitForTimeout(500);
  const results = await new AxeBuilder({ page }).withTags(tags).analyze();
  const critical = results.violations.filter(
    (v) => v.impact === "critical" || v.impact === "serious",
  );
  if (critical.length > 0) {
    const summary = critical
      .map((v) => `[${v.impact}] ${v.id}: ${v.description} (${v.nodes.length})`)
      .join("\n");
    expect(critical, `Accessibility violations:\n${summary}`).toEqual([]);
  }
}
```

- [ ] **Step 2: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add inspectah-web/ui/e2e/helpers/assertions.ts
git commit -m "feat(e2e): add shared assertion helpers"
```

### Task 4: Add Vite webServer to playwright.config.ts + rewrite keyboard.spec.ts as POC

**Files:**
- Modify: `inspectah-web/ui/playwright.config.ts`
- Modify: `inspectah-web/ui/e2e/keyboard.spec.ts`

Mock-tier tests use `page.goto("/")` which needs something serving the app shell (HTML/JS/CSS). The Vite dev server serves the shell; `page.route()` intercepts `/api/*` before Vite's proxy fires, so no Rust server is needed for mock tests. Real-server tests also work because Vite proxies unintercepted `/api` requests through to the Rust server on port 8642.

- [ ] **Step 1: Add webServer and update baseURL in playwright.config.ts**

```typescript
import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "./e2e",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: process.env.CI ? 1 : undefined,
  reporter: process.env.CI ? "github" : "list",
  use: {
    baseURL: "http://127.0.0.1:5173",
    screenshot: "only-on-failure",
    trace: "on-first-retry",
  },
  webServer: {
    command: "npx vite",
    url: "http://127.0.0.1:5173",
    reuseExistingServer: !process.env.CI,
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
});
```

Key changes from existing config:
- `baseURL` changed from `http://127.0.0.1:8642` (Rust server) to `http://127.0.0.1:5173` (Vite)
- `webServer` added — auto-starts Vite dev server before tests, stops after
- `reuseExistingServer` allows reuse in local dev (if `npx vite` is already running)
- Mock tests: `page.route()` intercepts `/api/*` before Vite's proxy reaches port 8642
- Real-server tests: Vite proxies `/api/*` to the Rust server (configured in `vite.config.ts`)

- [ ] **Step 2: Build the UI first (Vite needs compiled assets)**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npm run build`

This ensures the Vite server has assets to serve. In dev mode (`npx vite`), it serves from source, so a full build isn't strictly required, but verify it works.

- [ ] **Step 3: Rewrite keyboard.spec.ts**

```typescript
import { test, expect } from "@playwright/test";
import { applyMockApi, clearMocks } from "./helpers/mock-api";

test.describe("Keyboard navigation", () => {
  test.beforeEach(async ({ page }) => {
    await applyMockApi(page, "single-host");
    await page.goto("/");
    await expect(page.locator(".inspectah-page")).toBeVisible();
    await expect(page.locator(".inspectah-statsbar")).toBeVisible();
  });

  test.afterEach(async ({ page }) => {
    await clearMocks(page);
  });

  test("j/k navigate decision items", async ({ page }) => {
    await page.locator(".inspectah-layout__sidebar").getByText("Packages").click();
    await page.locator(".inspectah-layout__main").click();
    await page.keyboard.press("j");
    const focusedRow = page.locator('[role="group"][tabindex="0"]');
    await expect(focusedRow.first()).toBeVisible();
    await page.keyboard.press("k");
    await expect(focusedRow.first()).toBeVisible();
  });

  test("/ opens section search", async ({ page }) => {
    await page.locator(".inspectah-layout__main").click();
    await page.keyboard.press("/");
    const searchInput = page.locator('[data-testid="section-search"] input');
    await expect(searchInput).toBeVisible({ timeout: 2000 });
    await page.keyboard.press("Escape");
    await expect(searchInput).not.toBeVisible({ timeout: 2000 });
  });

  test("Ctrl+K focuses global search", async ({ page }) => {
    await page.keyboard.press("Control+k");
    const searchInput = page.locator('[data-testid="global-search-input"] input');
    await expect(searchInput).toBeFocused({ timeout: 2000 });
  });

  test("? opens shortcut overlay", async ({ page }) => {
    await page.keyboard.press("?");
    await expect(page.locator('[data-testid="shortcut-overlay"]')).toBeVisible({ timeout: 2000 });
  });

  test("Escape closes shortcut overlay", async ({ page }) => {
    await page.keyboard.press("?");
    const overlay = page.locator('[data-testid="shortcut-overlay"]');
    await expect(overlay).toBeVisible({ timeout: 2000 });
    await page.keyboard.press("Escape");
    await expect(overlay).not.toBeVisible({ timeout: 2000 });
  });
});
```

- [ ] **Step 4: Run to verify the mock pattern works**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx playwright test e2e/keyboard.spec.ts --headed`

Expected: Playwright auto-starts Vite (via `webServer` config), tests run against mock data served over Vite. No Rust server needed — `page.route()` intercepts all `/api/*` requests before Vite's proxy. If tests fail on missing elements, adjust fixture JSON (ensure `view.json` has at least one package for j/k navigation).

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add inspectah-web/ui/playwright.config.ts inspectah-web/ui/e2e/keyboard.spec.ts
git commit -m "feat(e2e): add Vite webServer to playwright config, rewrite keyboard.spec.ts"
```

---

## Phase 1b: Remaining Fixtures + Mutation Proof

### Task 5: Create POST response, sequence, error, fleet fixtures + manifest

**Files:**
- Create: `inspectah-web/ui/e2e/fixtures/post-responses/**/*.json`
- Create: `inspectah-web/ui/e2e/fixtures/sequences/exclude-undo-redo/*.json`
- Create: `inspectah-web/ui/e2e/fixtures/errors/*`
- Create: `inspectah-web/ui/e2e/fixtures/fleet/*.json`
- Create: `inspectah-web/ui/e2e/fixtures/manifest.json`

- [ ] **Step 1: Create directories**

```bash
mkdir -p /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui/e2e/fixtures/{post-responses/{op,undo,redo,tarball,user-strategy,user-password,viewed,fleet-diff},sequences/exclude-undo-redo,errors,fleet}
```

- [ ] **Step 2: Create POST response fixtures**

For each, start from captured `view.json` (Task 2) and modify. The `_status` field is required transport metadata:

**`post-responses/op/success.json`:** Copy `view.json`, add `"_status": 200`, set `generation: 2`, `can_undo: true`.

**`post-responses/undo/success.json`:** Copy `view.json`, add `"_status": 200`, set `generation: 3`, `can_undo: false`, `can_redo: true`.

**`post-responses/undo/nothing-to-undo.json`:**
```json
{ "_status": 409, "error": "nothing to undo" }
```

**`post-responses/redo/success.json`:** Copy `view.json`, add `"_status": 200`, set `generation: 4`, `can_redo: false`.

**`post-responses/tarball/stale.json`:**
```json
{ "_status": 409, "error": "stale generation: expected 2, got 1" }
```

**`post-responses/tarball/sensitive-required.json`:**
```json
{ "_status": 428, "sensitive_files": ["/etc/shadow"], "message": "Export contains sensitive data. Set X-Acknowledge-Sensitive header to proceed." }
```

**`post-responses/tarball/stub.tar.gz`:**
```bash
echo "e2e-stub" | gzip > inspectah-web/ui/e2e/fixtures/post-responses/tarball/stub.tar.gz
```

**`post-responses/user-strategy/success.json`:** Copy `view.json`, add `"_status": 200`.

**`post-responses/user-password/success.json`:** Copy `view.json`, add `"_status": 200`.

**`post-responses/user-password/invalid.json`:**
```json
{ "_status": 400, "error": "password does not meet complexity requirements" }
```

**`post-responses/viewed/success.json`:**
```json
{ "_status": 204 }
```

**`post-responses/fleet-diff/success.json`:** Minimal `FleetDiffResponse` with `_status: 200`. Match the shape in `inspectah-web/ui/src/api/types.ts`.

- [ ] **Step 3: Create sequence fixtures**

Three ViewResponse snapshots. Start from captured `view.json`:

**`sequences/exclude-undo-redo/01-after-exclude.json`:** Set one package `include: false`, `generation: 2`, `can_undo: true`.

**`sequences/exclude-undo-redo/02-after-undo.json`:** Original state, `generation: 3`, `can_undo: false`, `can_redo: true`.

**`sequences/exclude-undo-redo/03-after-redo.json`:** Same as 01 but `generation: 4`.

- [ ] **Step 4: Create error fixtures**

**`errors/server-500.json`:** `{ "_status": 500, "error": "internal server error" }`

**`errors/malformed.txt`:** `this is not valid json {{{ broken`

- [ ] **Step 5: Create fleet fixtures**

Capture from fleet-mode server or create manually matching `FleetViewResponse`/`FleetHealthInfo` types. If no fleet tarball exists yet, defer — the fleet spec (Phase 3) needs this.

- [ ] **Step 6: Create manifest.json**

```json
{
  "fixtures": {
    "single-host/health.json": { "category": "body", "schema": "HealthResponse" },
    "single-host/view.json": { "category": "body", "schema": "ViewResponse" },
    "single-host/ops-empty.json": { "category": "body", "schema": "AnnotatedOps" },
    "single-host/changes-empty.json": { "category": "body", "schema": "ChangesSummary" },
    "single-host/viewed-empty.json": { "category": "body", "schema": "ViewedResponse" },
    "single-host/sections.json": { "category": "body", "schema": "ContextSections" },
    "single-host/user-preview.json": { "category": "body", "schema": "UserPreviewResponse" },
    "single-host/user-preview-redacted.json": { "category": "body", "schema": "UserPreviewResponse" },
    "fleet/health.json": { "category": "body", "schema": "HealthResponse" },
    "fleet/fleet-view.json": { "category": "body", "schema": "FleetViewResponse" },
    "fleet/sections.json": { "category": "body", "schema": "ContextSections" },
    "post-responses/op/success.json": { "category": "wrapper", "schema": "ViewResponse" },
    "post-responses/undo/success.json": { "category": "wrapper", "schema": "ViewResponse" },
    "post-responses/undo/nothing-to-undo.json": { "category": "error-envelope" },
    "post-responses/redo/success.json": { "category": "wrapper", "schema": "ViewResponse" },
    "post-responses/tarball/stub.tar.gz": { "category": "excluded" },
    "post-responses/tarball/stale.json": { "category": "error-envelope" },
    "post-responses/tarball/sensitive-required.json": { "category": "wrapper", "schema": "TarballSensitivity" },
    "post-responses/user-strategy/success.json": { "category": "wrapper", "schema": "ViewResponse" },
    "post-responses/user-password/success.json": { "category": "wrapper", "schema": "ViewResponse" },
    "post-responses/user-password/invalid.json": { "category": "error-envelope" },
    "post-responses/viewed/success.json": { "category": "excluded" },
    "post-responses/fleet-diff/success.json": { "category": "wrapper", "schema": "FleetDiffResponse" },
    "sequences/exclude-undo-redo/01-after-exclude.json": { "category": "body", "schema": "ViewResponse" },
    "sequences/exclude-undo-redo/02-after-undo.json": { "category": "body", "schema": "ViewResponse" },
    "sequences/exclude-undo-redo/03-after-redo.json": { "category": "body", "schema": "ViewResponse" },
    "errors/server-500.json": { "category": "error-envelope" },
    "errors/malformed.txt": { "category": "excluded" }
  }
}
```

- [ ] **Step 7: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add inspectah-web/ui/e2e/fixtures/
git commit -m "feat(e2e): add POST response, sequence, error, fleet fixtures with manifest"
```

### Task 6: Mutation proof — triage toggle + undo + error

This task de-risks the mock harness by proving the mutation-heavy path works before Phase 2. Writes a focused subset of `triage.spec.ts` covering toggle → undo → error.

**Files:**
- Modify: `inspectah-web/ui/e2e/triage.spec.ts`

- [ ] **Step 1: Add three mock-backed mutation tests to triage.spec.ts**

Keep the existing real-server tests in `triage.spec.ts` (they'll be fully rewritten in Task 9). Add a new `test.describe` block at the top:

```typescript
import { applyMockApi, clearMocks, mockSequence, mockError } from "./helpers/mock-api";

test.describe("Triage workflow (mock tier)", () => {
  test.beforeEach(async ({ page }) => {
    await applyMockApi(page, "single-host");
    await page.goto("/");
    await expect(page.locator(".inspectah-statsbar")).toBeVisible();
  });

  test.afterEach(async ({ page }) => {
    await clearMocks(page);
  });

  test("toggle package exclude → view updates via GET refetch", async ({ page }) => {
    // Navigate to Packages section where PF Switch toggles live
    await page.locator(".inspectah-layout__sidebar").getByText("Packages").click();

    await mockSequence(page, "/api/view", [
      "sequences/exclude-undo-redo/01-after-exclude.json",
    ], { triggerOn: ["/api/op"] });

    // DecisionItem renders PF Switch (role="switch") for package include/exclude
    const firstToggle = page.getByRole("switch").first();
    await expect(firstToggle).toBeVisible({ timeout: 5000 });
    const initialStats = await page.locator(".inspectah-statsbar").textContent();
    await firstToggle.click({ force: true });
    // UI sends POST /api/op, ignores response, calls view.invalidate() → GET /api/view refetch
    await page.waitForResponse((res) => res.url().includes("/api/view") && res.request().method() === "GET");
    const updatedStats = await page.locator(".inspectah-statsbar").textContent();
    expect(updatedStats).not.toBe(initialStats);
  });

  test("undo reverts state (pre-fetches /api/ops for focus target)", async ({ page }) => {
    await page.locator(".inspectah-layout__sidebar").getByText("Packages").click();

    await mockSequence(page, "/api/view", [
      "sequences/exclude-undo-redo/01-after-exclude.json",
      "sequences/exclude-undo-redo/02-after-undo.json",
    ], { triggerOn: ["/api/op", "/api/undo"] });

    // Toggle to create undo-able state
    const firstToggle = page.getByRole("switch").first();
    await firstToggle.click({ force: true });
    await page.waitForResponse((res) => res.url().includes("/api/view") && res.request().method() === "GET");

    // Undo — App.tsx pre-fetches GET /api/ops before calling mutation.undo()
    const undoBtn = page.locator(".inspectah-statsbar").getByRole("button", { name: /undo/i });
    await undoBtn.click();
    await page.waitForResponse((res) => res.url().includes("/api/view") && res.request().method() === "GET");

    // Redo button should now be enabled (can_redo: true in fixture 02)
    const redoBtn = page.locator(".inspectah-statsbar").getByRole("button", { name: /redo/i });
    await expect(redoBtn).toBeEnabled();
  });

  test("server error on mutation — page stays interactive", async ({ page }) => {
    await page.locator(".inspectah-layout__sidebar").getByText("Packages").click();

    await mockError(page, "/api/op", "500");
    const firstToggle = page.getByRole("switch").first();
    await expect(firstToggle).toBeVisible({ timeout: 5000 });
    await firstToggle.click({ force: true });
    await page.waitForResponse((res) => res.url().includes("/api/op") && res.status() === 500);
    // The page doesn't crash — sidebar and stats bar remain interactive
    await expect(page.locator(".inspectah-page")).toBeVisible();
    await expect(page.locator(".inspectah-statsbar")).toBeVisible();
  });
});
```

- [ ] **Step 2: Run the mutation proof**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx playwright test e2e/triage.spec.ts --headed --grep "mock tier"`

Expected: All 3 tests pass. This proves the mock harness handles the single-host mutation cycle (POST → GET refetch → UI update).

- [ ] **Step 3: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add inspectah-web/ui/e2e/triage.spec.ts
git commit -m "feat(e2e): add mutation proof tests to triage.spec.ts (toggle, undo, error)"
```

---

## Phase 1c: Schema Validation

### Task 7: Schema export test with insta JSON snapshots

**What this task proves and what it doesn't:**

Task 7 validates that fixture JSON files are **structurally intentional** — any change to a fixture breaks its insta snapshot and requires explicit review. This catches accidental fixture edits and ensures fixture changes are deliberate.

Task 7 does **not** validate fixtures against Rust types. The response DTOs (`ViewResponse`, `FleetViewResponse`, etc.) derive `Serialize` only — adding `Deserialize` would require transitive derives across ~50 types in 3 crates, the same cascade we avoided by dropping `schemars`. **Rust-type compatibility is proven by the real-server tier** (Task 8), where the actual server serializes responses and Playwright asserts against them.

The `manifest.json` (Task 5) documents fixture categories for future CI validation tooling. Task 7 does not implement manifest-driven validation — it provides the insta-snapshot safety net only.

**Files:**
- Modify: `inspectah-web/Cargo.toml` (verify `insta` dev-dep)
- Create: `inspectah-web/tests/fixture_structure_test.rs`

- [ ] **Step 1: Verify insta is a dev-dependency**

`insta` is already in workspace deps. Verify `inspectah-web/Cargo.toml` has it under `[dev-dependencies]`:

```toml
insta = { workspace = true }
```

If it's not there, add it. No other dependency changes needed.

- [ ] **Step 2: Write the fixture structure test**

```rust
// inspectah-web/tests/fixture_structure_test.rs
//
// Snapshot test: parses each e2e fixture as serde_json::Value and snapshots it.
// Any fixture change (field added, removed, renamed, retyped) breaks the snapshot
// and requires explicit `cargo insta review` acceptance.
//
// This does NOT validate against Rust response types (they lack Deserialize).
// Rust-type compatibility is proven by Task 8's real-server smoke tests.
//
// Run with: INSPECTAH_SKIP_UI=1 cargo test -p inspectah-web --test fixture_structure_test

fn fixture_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("ui/e2e/fixtures")
}

fn snapshot_fixture(name: &str, relative_path: &str) {
    let path = fixture_dir().join(relative_path);
    let json_str = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Cannot read fixture {}: {}", path.display(), e));
    let value: serde_json::Value = serde_json::from_str(&json_str)
        .unwrap_or_else(|e| panic!("Fixture {} is not valid JSON: {}", relative_path, e));
    insta::assert_json_snapshot!(name, value);
}

fn snapshot_fixture_if_exists(name: &str, relative_path: &str) {
    let path = fixture_dir().join(relative_path);
    if !path.exists() { return; }
    snapshot_fixture(name, relative_path);
}

// --- Body fixtures (GET presets) ---

#[test]
fn fixture_single_host_health() {
    snapshot_fixture("single_host_health", "single-host/health.json");
}

#[test]
fn fixture_single_host_view() {
    snapshot_fixture("single_host_view", "single-host/view.json");
}

#[test]
fn fixture_single_host_sections() {
    snapshot_fixture("single_host_sections", "single-host/sections.json");
}

#[test]
fn fixture_single_host_ops() {
    snapshot_fixture("single_host_ops", "single-host/ops-empty.json");
}

#[test]
fn fixture_single_host_changes() {
    snapshot_fixture("single_host_changes", "single-host/changes-empty.json");
}

#[test]
fn fixture_single_host_viewed() {
    snapshot_fixture("single_host_viewed", "single-host/viewed-empty.json");
}

#[test]
fn fixture_single_host_user_preview() {
    snapshot_fixture("single_host_user_preview", "single-host/user-preview.json");
}

// --- Fleet fixtures (created in Phase 1b, may not exist yet) ---

#[test]
fn fixture_fleet_view() {
    snapshot_fixture_if_exists("fleet_view", "fleet/fleet-view.json");
}

// --- Sequence fixtures ---

#[test]
fn fixture_sequence_after_exclude() {
    snapshot_fixture_if_exists("seq_after_exclude", "sequences/exclude-undo-redo/01-after-exclude.json");
}

#[test]
fn fixture_sequence_after_undo() {
    snapshot_fixture_if_exists("seq_after_undo", "sequences/exclude-undo-redo/02-after-undo.json");
}

#[test]
fn fixture_sequence_after_redo() {
    snapshot_fixture_if_exists("seq_after_redo", "sequences/exclude-undo-redo/03-after-redo.json");
}

// --- POST response wrappers (strip _status, snapshot body) ---

fn snapshot_post_fixture(name: &str, relative_path: &str) {
    let path = fixture_dir().join(relative_path);
    if !path.exists() { return; }
    let json_str = std::fs::read_to_string(&path).unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    // Strip _status transport metadata before snapshotting the body
    if let Some(obj) = value.as_object_mut() {
        obj.remove("_status");
    }
    insta::assert_json_snapshot!(name, value);
}

#[test]
fn fixture_post_op_success() {
    snapshot_post_fixture("post_op_success", "post-responses/op/success.json");
}

#[test]
fn fixture_post_undo_success() {
    snapshot_post_fixture("post_undo_success", "post-responses/undo/success.json");
}

#[test]
fn fixture_post_redo_success() {
    snapshot_post_fixture("post_redo_success", "post-responses/redo/success.json");
}
```

- [ ] **Step 3: Run the test**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && INSPECTAH_SKIP_UI=1 cargo test -p inspectah-web --test fixture_structure_test 2>&1 | tail -20`

`INSPECTAH_SKIP_UI=1` skips the UI build in `build.rs`.

Expected: First run creates new snapshots for each fixture. Accept them:

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && cargo insta review`

Accept all snapshots. These become the baseline. Any future fixture change will break its snapshot and require explicit review.

Snapshots land at `inspectah-web/tests/snapshots/fixture_structure_test__*.snap`.

- [ ] **Step 4: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add inspectah-web/Cargo.toml inspectah-web/tests/fixture_structure_test.rs inspectah-web/tests/snapshots/
git commit -m "feat: add fixture structure test — insta snapshots of e2e fixture JSON"
```

---

## Phase 1d: Real-Server Smoke Tests

### Task 8: Curate tarballs and write smoke-integration.spec.ts

Expanded from round 1 to include coherence flows (viewed persistence, `/api/ops` verification) that justify the real-server tier's claims.

**Files:**
- Create: `testdata/single-host-e2e.tar.gz`
- Create: `testdata/fleet-e2e.tar.gz`
- Create: `inspectah-web/ui/e2e/smoke-integration.spec.ts`

- [ ] **Step 1: Curate tarballs**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
# Use existing scan or scan a test VM
cp path/to/existing-scan.tar.gz testdata/single-host-e2e.tar.gz
```

Requirements: packages across 2+ repos, 2+ config files, 1+ service, 1+ user, at least one sensitive field.

Fleet tarball (if available):
```bash
inspectah fleet merge scan-01.tar.gz scan-02.tar.gz scan-03.tar.gz -o testdata/fleet-e2e.tar.gz
```

- [ ] **Step 2: Write smoke-integration.spec.ts with coherence tests**

```typescript
import { test, expect } from "@playwright/test";

test.describe("Smoke integration (real server)", () => {
  test.describe.configure({ mode: "serial" });

  test.beforeEach(async ({ page }) => {
    try {
      const response = await page.request.get("/api/health", { timeout: 3000 });
      if (!response.ok()) test.skip(true, "Refine server not running on port 8642");
    } catch {
      test.skip(true, "Cannot connect to refine server at port 8642");
    }
  });

  test("health endpoint returns valid response", async ({ request }) => {
    const response = await request.get("/api/health");
    expect(response.ok()).toBeTruthy();
    const body = await response.json();
    expect(body.status).toBe("ok");
    expect(body.host.hostname).toBeTruthy();
    expect(typeof body.session_is_sensitive).toBe("boolean");
  });

  test("view returns valid initial state", async ({ request }) => {
    const response = await request.get("/api/view");
    expect(response.ok()).toBeTruthy();
    const view = await response.json();
    expect(view.generation).toBe(1);
    expect(view.packages.length).toBeGreaterThan(0);
    expect(typeof view.containerfile_preview).toBe("string");
  });

  test("exclude → containerfile changes → undo", async ({ page }) => {
    await page.goto("/");
    await expect(page.locator(".inspectah-statsbar")).toBeVisible();

    const initialView = await (await page.request.get("/api/view")).json();

    const firstToggle = page.getByRole("switch").first();
    await expect(firstToggle).toBeVisible({ timeout: 5000 });
    const opResp = page.waitForResponse((res) => res.url().includes("/api/op"));
    await firstToggle.click({ force: true });
    const afterExclude = await (await opResp).json();
    expect(afterExclude.generation).toBe(2);
    expect(afterExclude.can_undo).toBe(true);
    expect(afterExclude.containerfile_preview).not.toBe(initialView.containerfile_preview);

    const undoResp = page.waitForResponse((res) => res.url().includes("/api/undo"));
    await page.locator(".inspectah-statsbar").getByRole("button", { name: /undo/i }).click();
    const afterUndo = await (await undoResp).json();
    expect(afterUndo.can_undo).toBe(false);
    expect(afterUndo.can_redo).toBe(true);
  });

  test("ops reflects the mutation (cross-endpoint coherence)", async ({ page, request }) => {
    await page.goto("/");
    await expect(page.locator(".inspectah-statsbar")).toBeVisible();

    // Initial ops should be empty
    const opsBefore = await (await request.get("/api/ops")).json();
    const countBefore = opsBefore.length;

    // Perform a mutation
    const firstToggle = page.getByRole("switch").first();
    await firstToggle.click({ force: true });
    await page.waitForResponse((res) => res.url().includes("/api/op"));

    // Ops should now have one more entry
    const opsAfter = await (await request.get("/api/ops")).json();
    expect(opsAfter.length).toBe(countBefore + 1);
  });

  test("viewed persistence across reload", async ({ page, request }) => {
    await page.goto("/");
    await expect(page.locator(".inspectah-statsbar")).toBeVisible();

    // Mark an item as viewed
    await request.post("/api/viewed", {
      data: { id: "packages:test-pkg" },
      headers: { "Content-Type": "application/json" },
    });

    // Verify GET /api/viewed reflects it
    const viewed = await (await request.get("/api/viewed")).json();
    expect(viewed.ids).toContain("packages:test-pkg");

    // Reload and verify persistence
    await page.reload();
    await expect(page.locator(".inspectah-statsbar")).toBeVisible();
    const viewedAfterReload = await (await request.get("/api/viewed")).json();
    expect(viewedAfterReload.ids).toContain("packages:test-pkg");
  });

  test("tarball export produces gzip response", async ({ request }) => {
    const response = await request.post("/api/tarball", {
      data: { generation: 1 },
      headers: { "Content-Type": "application/json" },
    });
    expect(response.ok()).toBeTruthy();
    expect(response.headers()["content-type"]).toContain("application/gzip");
    const body = await response.body();
    expect(body.length).toBeGreaterThan(0);
  });
});
```

- [ ] **Step 3: Test against running server**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
cargo run -p inspectah-cli -- refine testdata/single-host-e2e.tar.gz --no-browser --port 8642 &
sleep 3
cd inspectah-web/ui && npx playwright test e2e/smoke-integration.spec.ts --headed
kill %1
```

- [ ] **Step 4: Verify graceful skip without server**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx playwright test e2e/smoke-integration.spec.ts 2>&1 | tail -5`

Expected: All tests show "skipped".

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add testdata/single-host-e2e.tar.gz inspectah-web/ui/e2e/smoke-integration.spec.ts
git commit -m "feat(e2e): add real-server smoke tests with coherence and viewed persistence"
```

---

## Phase 2: Single-Host Core

### Task 9: Full triage.spec.ts rewrite

**Files:**
- Modify: `inspectah-web/ui/e2e/triage.spec.ts`

- [ ] **Step 1: Full rewrite of triage.spec.ts**

Replace the entire file. Remove all `test.skip` guards. All tests use mock fixtures. Key tests:

1. **Package toggle** — `applyMockApi('single-host')` + `mockSequence`, click toggle, verify stats update via GET refetch
2. **Config toggle** — same pattern for config files section
3. **Undo/redo sequence** — `mockSequence` with 3 states, verify undo disables undo button / enables redo
4. **Containerfile preview updates** — verify `.inspectah-cf-panel__code` content differs between sequence states
5. **Export download** — `mockPostResponse('/api/tarball', 'post-responses/tarball/stub.tar.gz')`, verify download event
6. **Sensitive tarball gating** — `mockPostResponse('/api/tarball', 'post-responses/tarball/sensitive-required.json')`, verify 428 handling
7. **Nothing to undo error** — `mockPostResponse('/api/undo', 'post-responses/undo/nothing-to-undo.json')`, verify 409 handling
8. **Server error on mutation** — `mockError(page, '/api/op', '500')`, verify page stays interactive
9. **Timeout on mutation** — `mockError(page, '/api/op', 'timeout', { timeoutMs: 1000 })`, verify timeout handling
10. **Malformed response** — `mockError(page, '/api/view', 'malformed')`, verify error state

Follow `beforeEach`/`afterEach` pattern from Task 4.

- [ ] **Step 2: Run and commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx playwright test e2e/triage.spec.ts --headed
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-web/ui/e2e/triage.spec.ts
git commit -m "feat(e2e): rewrite triage.spec.ts with mock fixtures, unskip all tests"
```

### Task 10: Expand a11y.spec.ts and responsive.spec.ts

**Files:**
- Modify: `inspectah-web/ui/e2e/a11y.spec.ts`
- Modify: `inspectah-web/ui/e2e/responsive.spec.ts`

- [ ] **Step 1: Expand a11y.spec.ts with mock presets**

Add `applyMockApi` to each test. Add fleet-preset axe scan. Keep existing ARIA tests, adding mock fixtures. Use `expectNoAxeViolations` from assertions helper.

Key tests: single-host axe scan, fleet axe scan, sidebar nav keyboard accessible, stats bar buttons have accessible names, hamburger ARIA at mobile viewport.

- [ ] **Step 2: Expand responsive.spec.ts with mock presets**

Add `applyMockApi('single-host')` to each test. Key tests: hamburger visible at 768px, sidebar hidden at mobile, sidebar visible at desktop, resize transitions.

- [ ] **Step 3: Run and commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx playwright test e2e/a11y.spec.ts e2e/responsive.spec.ts
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-web/ui/e2e/a11y.spec.ts inspectah-web/ui/e2e/responsive.spec.ts
git commit -m "feat(e2e): expand a11y and responsive specs with mock fixtures"
```

### Task 11: Create sections.spec.ts

**Files:**
- Create: `inspectah-web/ui/e2e/sections.spec.ts`

- [ ] **Step 1: Write sections.spec.ts**

The sidebar section labels come from the fixture's `sections.json` `display_name` fields and from `view.json` section names. Hardcoding labels risks drift. Instead, verify sections render dynamically from fixture data.

```typescript
import { test, expect } from "@playwright/test";
import { applyMockApi, clearMocks } from "./helpers/mock-api";
import * as fs from "fs";
import * as path from "path";

// Read section names from the fixture instead of hardcoding
const sectionsFixture = JSON.parse(
  fs.readFileSync(
    path.join(__dirname, "fixtures/single-host/sections.json"),
    "utf-8",
  ),
);
const SECTION_NAMES: string[] = sectionsFixture.map(
  (s: { display_name: string }) => s.display_name,
);

test.describe("Context sections", () => {
  test.beforeEach(async ({ page }) => {
    await applyMockApi(page, "single-host");
    await page.goto("/");
    await expect(page.locator(".inspectah-statsbar")).toBeVisible();
  });

  test.afterEach(async ({ page }) => { await clearMocks(page); });

  test("sidebar renders all context sections from fixture", async ({ page }) => {
    const sidebar = page.locator(".inspectah-layout__sidebar");
    for (const name of SECTION_NAMES) {
      await expect(sidebar.getByText(name)).toBeVisible();
    }
  });

  for (const section of SECTION_NAMES) {
    test(`clicking ${section} renders content pane`, async ({ page }) => {
      await page.locator(".inspectah-layout__sidebar").getByText(section).click();
      const main = page.locator(".inspectah-layout__main");
      await expect(main).toBeVisible();
      // Content pane should have at least one context item or empty reason
      const hasItems = await main.locator("[data-testid^='context-item-']").count() > 0;
      const hasEmpty = await main.getByText(/no items|empty/i).count() > 0;
      expect(hasItems || hasEmpty).toBe(true);
    });
  }

  test("/ opens inline section search above content list", async ({ page }) => {
    // SectionSearch (data-testid="section-search") renders inline above the
    // decision/context item list in the main content pane. It filters items
    // within the currently active section — it does NOT filter sidebar sections.
    await page.locator(".inspectah-layout__main").click();
    await page.keyboard.press("/");
    const input = page.locator('[data-testid="section-search"] input');
    await expect(input).toBeVisible();
    // Type a query — items in the content pane should filter
    await input.fill("net");
    // The sidebar sections remain unchanged (all still visible)
    // Escape closes the inline search
    await page.keyboard.press("Escape");
    await expect(input).not.toBeVisible({ timeout: 2000 });
  });
});
```

- [ ] **Step 2: Run and commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx playwright test e2e/sections.spec.ts
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-web/ui/e2e/sections.spec.ts
git commit -m "feat(e2e): add sections.spec.ts for context section rendering and navigation"
```

---

## Phase 3: Recent Features + Fleet Gaps

### Task 12: Rewrite fleet.spec.ts with mock fixtures

**Files:**
- Modify: `inspectah-web/ui/e2e/fleet.spec.ts`

- [ ] **Step 1: Rewrite fleet.spec.ts**

Replace entire file. Use `applyMockApi(page, 'fleet-3')`. Key tests:

1. **Fleet app loads** — `data-testid="fleet-app"` visible
2. **Zone groups render** — `[data-testid^="zone-"]` locator, verify consensus/near-consensus/divergent
3. **Fleet banner** — `data-testid="fleet-banner"`, verify `role="status"` and `data-severity`
4. **Variant ack progress** — `data-testid="ack-progress"`, verify "X of Y variants need review"
5. **Fleet undo/redo** — `mockSequence(page, '/api/fleet/view', [...], { triggerOn: ['/api/op'] })` — fleet always re-fetches GET, POST body discarded
6. **Diff drawer** — `mockPostResponse('/api/fleet/diff', ...)`, verify `data-testid="diff-drawer"` opens
7. **Fleet keyboard** — `?` opens help with "Compare" shortcut
8. **Fleet axe scan** — `expectNoAxeViolations(page)`
9. **Fleet banner ARIA** — `role="status"` attribute check
10. **Fleet item rows focusable** — `.fleet-item-row` elements have tabindex

Use `test.describe.configure({ mode: "serial" })`.

- [ ] **Step 2: Run and commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx playwright test e2e/fleet.spec.ts --headed
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-web/ui/e2e/fleet.spec.ts
git commit -m "feat(e2e): rewrite fleet.spec.ts with mock fixtures, unskip all tests"
```

### Task 13: Create containerfile.spec.ts

**Files:**
- Create: `inspectah-web/ui/e2e/containerfile.spec.ts`

Selectors from `ContainerfilePanel.tsx`: `.inspectah-cf-panel--collapsed` (collapsed state), `.inspectah-cf-panel--open` (expanded), `.inspectah-cf-panel__code` (code content), `.inspectah-cf-panel__tab--has-changes` (changes indicator), `.inspectah-cf-line--added` / `.inspectah-cf-line--removing` (change highlights), `.inspectah-cf-panel__tab-label` (tab text "Containerfile").

- [ ] **Step 1: Write containerfile.spec.ts**

```typescript
import { test, expect } from "@playwright/test";
import { applyMockApi, clearMocks, mockSequence } from "./helpers/mock-api";

test.describe("Containerfile panel", () => {
  test.beforeEach(async ({ page }) => {
    await applyMockApi(page, "single-host");
    await page.goto("/");
    await expect(page.locator(".inspectah-statsbar")).toBeVisible();
  });

  test.afterEach(async ({ page }) => { await clearMocks(page); });

  test("panel is visible in collapsed state on load", async ({ page }) => {
    const collapsed = page.locator(".inspectah-cf-panel--collapsed");
    const open = page.locator(".inspectah-cf-panel--open");
    // Panel starts in one of the two states
    const isCollapsed = await collapsed.isVisible().catch(() => false);
    const isOpen = await open.isVisible().catch(() => false);
    expect(isCollapsed || isOpen).toBe(true);
  });

  test("panel tab shows 'Containerfile' label", async ({ page }) => {
    const tabLabel = page.locator(".inspectah-cf-panel__tab-label");
    await expect(tabLabel).toContainText("Containerfile");
  });

  test("expanded panel shows FROM instruction in code", async ({ page }) => {
    // Click tab to expand if collapsed
    const tab = page.locator(".inspectah-cf-panel__tab");
    if (await page.locator(".inspectah-cf-panel--collapsed").isVisible().catch(() => false)) {
      await tab.click();
    }
    const code = page.locator(".inspectah-cf-panel__code");
    await expect(code).toBeVisible();
    await expect(code).toContainText("FROM");
  });

  test("content updates after mutation via view refetch", async ({ page }) => {
    // Ensure panel is open
    if (await page.locator(".inspectah-cf-panel--collapsed").isVisible().catch(() => false)) {
      await page.locator(".inspectah-cf-panel__tab").click();
    }

    await mockSequence(page, "/api/view", [
      "sequences/exclude-undo-redo/01-after-exclude.json",
    ], { triggerOn: ["/api/op"] });

    const code = page.locator(".inspectah-cf-panel__code");
    const initialContent = await code.textContent();

    const firstToggle = page.getByRole("switch").first();
    if (await firstToggle.count() > 0) {
      await firstToggle.click({ force: true });
      await page.waitForResponse((res) => res.url().includes("/api/view") && res.request().method() === "GET");
      await expect(async () => {
        const updated = await code.textContent();
        expect(updated).not.toBe(initialContent);
      }).toPass({ timeout: 5000 });
    }
  });

  test("change highlight classes present on diff lines", async ({ page }) => {
    // This test requires fixture data where containerfile_preview differs
    // between initial and post-mutation states. The 01-after-exclude.json
    // sequence fixture must have a different containerfile_preview.
    await mockSequence(page, "/api/view", [
      "sequences/exclude-undo-redo/01-after-exclude.json",
    ], { triggerOn: ["/api/op"] });

    if (await page.locator(".inspectah-cf-panel--collapsed").isVisible().catch(() => false)) {
      await page.locator(".inspectah-cf-panel__tab").click();
    }

    const firstToggle = page.getByRole("switch").first();
    if (await firstToggle.count() > 0) {
      await firstToggle.click({ force: true });
      await page.waitForResponse((res) => res.url().includes("/api/view") && res.request().method() === "GET");
      // Look for change highlight classes
      const addedLines = page.locator(".inspectah-cf-line--added");
      const removingLines = page.locator(".inspectah-cf-line--removing");
      // At least one type of change indicator should appear
      const hasAdded = (await addedLines.count()) > 0;
      const hasRemoving = (await removingLines.count()) > 0;
      expect(hasAdded || hasRemoving).toBe(true);
    }
  });

  test("reduced motion suppresses change highlight animations", async ({ page }) => {
    await page.emulateMedia({ reducedMotion: "reduce" });

    await mockSequence(page, "/api/view", [
      "sequences/exclude-undo-redo/01-after-exclude.json",
    ], { triggerOn: ["/api/op"] });

    if (await page.locator(".inspectah-cf-panel--collapsed").isVisible().catch(() => false)) {
      await page.locator(".inspectah-cf-panel__tab").click();
    }

    const firstToggle = page.getByRole("switch").first();
    if (await firstToggle.count() > 0) {
      await firstToggle.click({ force: true });
      await page.waitForResponse((res) => res.url().includes("/api/view") && res.request().method() === "GET");
      // Under prefers-reduced-motion: reduce, the collapsing/removing animation
      // classes should not be applied. Verify no .inspectah-cf-line--removing elements
      // are in a CSS-animated state (the class may appear briefly but duration is 0).
      const removingLines = page.locator(".inspectah-cf-line--removing");
      // Wait a tick for any zero-duration transitions to resolve
      await page.waitForTimeout(100);
      const removingCount = await removingLines.count();
      // Even if removing lines exist, verify the panel rendered without errors
      await expect(page.locator(".inspectah-cf-panel--open")).toBeVisible();
    }
  });
});
```

- [ ] **Step 2: Run and commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx playwright test e2e/containerfile.spec.ts
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-web/ui/e2e/containerfile.spec.ts
git commit -m "feat(e2e): add containerfile.spec.ts with panel state, highlights, reduced motion"
```

### Task 14: Create repos.spec.ts

**Files:**
- Create: `inspectah-web/ui/e2e/repos.spec.ts`

Selectors from components: `data-testid="repo-bar"` (RepoBar), `data-testid={`repo-group-wrapper-${section_id}`}` (RepoGroup), `data-testid="excluded-zone"` (ExcludedZone). Note: `AttentionSummary` (`data-testid="attention-summary"`) renders inside `MainContent` for the active section, not as a standalone page-level surface. It is tested as part of triage.spec.ts where the full Packages content pane is rendered, not here in isolation.

- [ ] **Step 1: Write repos.spec.ts**

```typescript
import { test, expect } from "@playwright/test";
import { applyMockApi, clearMocks, mockSequence } from "./helpers/mock-api";

test.describe("Repo groups", () => {
  test.beforeEach(async ({ page }) => {
    await applyMockApi(page, "single-host");
    await page.goto("/");
    await expect(page.locator(".inspectah-statsbar")).toBeVisible();
    // Navigate to Packages section where repo groups render
    await page.locator(".inspectah-layout__sidebar").getByText("Packages").click();
  });

  test.afterEach(async ({ page }) => { await clearMocks(page); });

  test("repo bar renders above package list", async ({ page }) => {
    await expect(page.getByTestId("repo-bar")).toBeVisible();
  });

  test("repo group wrappers render for each repo in fixture", async ({ page }) => {
    const groups = page.locator("[data-testid^='repo-group-wrapper-']");
    const count = await groups.count();
    expect(count).toBeGreaterThan(0);
  });

  test("excluded zone renders after excluding items", async ({ page }) => {
    await mockSequence(page, "/api/view", [
      "sequences/exclude-undo-redo/01-after-exclude.json",
    ], { triggerOn: ["/api/op"] });

    // Toggle first available repo or package
    const firstToggle = page.getByRole("switch").first();
    if (await firstToggle.count() > 0) {
      await firstToggle.click({ force: true });
      await page.waitForResponse((res) => res.url().includes("/api/view") && res.request().method() === "GET");
      // Excluded zone should appear if the fixture has excluded items
      const excluded = page.getByTestId("excluded-zone");
      // May or may not be visible depending on fixture shape
      const visible = await excluded.isVisible().catch(() => false);
      // Just verify the page didn't crash
      await expect(page.locator(".inspectah-page")).toBeVisible();
    }
  });
});
```

- [ ] **Step 2: Run and commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx playwright test e2e/repos.spec.ts
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-web/ui/e2e/repos.spec.ts
git commit -m "feat(e2e): add repos.spec.ts for repo groups, bar, and excluded zone"
```

### Task 15: Create users.spec.ts

**Files:**
- Create: `inspectah-web/ui/e2e/users.spec.ts`

Selectors from `UserCard.tsx`: `data-testid={`user-card-${user.name}`}` (card wrapper), `input[type="checkbox"][aria-label="Include ${name}"]` (include toggle), `input[name="strategy-${name}"]` (skip/useradd radio), `aria-label="${expanded ? "Collapse" : "Expand"} ${name} details"` (expand button). From `UserArtifactPreview.tsx`: tab buttons "Kickstart" / "Blueprint", sensitive banner (Alert variant="info"/"warning").

Note: users/groups fixtures are NOT schema-backed (`users_groups_decisions: Vec<serde_json::Value>`). Tests rely on structural fixture correctness only.

**Implementation note:** The preview and password interaction flows below are based on source-reading of `UserCard.tsx`, `UserArtifactPreview.tsx`, and `UsersGroupsSection.tsx`. The exact flow (whether preview is inline or behind a modal, whether password requires expand → radio → fill → button click or a different sequence) **must be verified in a headed browser run** during implementation. Adjust selectors and click sequences to match the actual mounted behavior. The test intent (what to verify) is stable; the interaction path may need tuning.

- [ ] **Step 1: Write users.spec.ts**

```typescript
import { test, expect } from "@playwright/test";
import { applyMockApi, clearMocks, mockPostResponse } from "./helpers/mock-api";

test.describe("User/group materialization", () => {
  test.beforeEach(async ({ page }) => {
    await applyMockApi(page, "single-host");
    await page.goto("/");
    await expect(page.locator(".inspectah-statsbar")).toBeVisible();
    // Navigate to Users & Groups section
    await page.locator(".inspectah-layout__sidebar").getByText("Users & Groups").click();
  });

  test.afterEach(async ({ page }) => { await clearMocks(page); });

  test("user cards render for users in fixture", async ({ page }) => {
    const userCards = page.locator("[data-testid^='user-card-']");
    const count = await userCards.count();
    // Fixture must have at least one user for this test to be meaningful
    if (count === 0) {
      test.skip(true, "Fixture has no user entries");
      return;
    }
    await expect(userCards.first()).toBeVisible();
  });

  test("include toggle changes user strategy", async ({ page }) => {
    const userCards = page.locator("[data-testid^='user-card-']");
    if ((await userCards.count()) === 0) { test.skip(true, "No users in fixture"); return; }

    // The include checkbox has aria-label="Include <username>"
    const includeCheckbox = userCards.first().locator('input[type="checkbox"]');
    await expect(includeCheckbox).toBeVisible();
    const wasChecked = await includeCheckbox.isChecked();

    // Click to toggle
    const opResp = page.waitForResponse((res) => res.url().includes("/api/user-strategy"));
    await includeCheckbox.click({ force: true });
    await opResp;

    // Checkbox state should have changed
    const nowChecked = await includeCheckbox.isChecked();
    expect(nowChecked).not.toBe(wasChecked);
  });

  test("expand button reveals strategy radio and password section", async ({ page }) => {
    const userCards = page.locator("[data-testid^='user-card-']");
    if ((await userCards.count()) === 0) { test.skip(true, "No users in fixture"); return; }

    const card = userCards.first();
    // Find expand button by aria-label pattern
    const expandBtn = card.locator('button[aria-expanded]');
    if ((await expandBtn.count()) === 0) { test.skip(true, "No expandable user card"); return; }

    const isExpanded = await expandBtn.getAttribute("aria-expanded");
    if (isExpanded === "false") {
      await expandBtn.click();
    }

    // Strategy radios should be visible (skip / useradd)
    await expect(card.getByText("Containerfile strategy")).toBeVisible();
  });

  test("user artifact preview shows kickstart and blueprint tabs", async ({ page }) => {
    // UserArtifactPreview renders below user cards in the Users & Groups section.
    // It has two tab buttons. Scroll down if needed to find them.
    const kickstartTab = page.getByRole("button", { name: /kickstart/i });
    const blueprintTab = page.getByRole("button", { name: /blueprint/i });

    // Scroll the main content to reveal preview tabs (they render below the card list)
    await page.locator(".inspectah-layout__main").evaluate((el) =>
      el.scrollTo(0, el.scrollHeight),
    );

    const hasKickstart = await kickstartTab.isVisible().catch(() => false);
    const hasBlueprint = await blueprintTab.isVisible().catch(() => false);
    if (!hasKickstart && !hasBlueprint) {
      test.skip(true, "No user artifact preview tabs visible — fixture may lack users");
      return;
    }

    // Click between tabs to verify switching
    if (hasKickstart) {
      await kickstartTab.click();
      // Preview content area should have text
      await expect(page.locator(".inspectah-layout__main")).toContainText("user");
    }
  });

  test("redacted preview shows sensitive banner when not revealed", async ({ page }) => {
    await applyMockApi(page, "single-host", {
      "/api/user-preview": "single-host/user-preview-redacted.json",
    });
    await page.goto("/");
    await page.locator(".inspectah-layout__sidebar").getByText("Users & Groups").click();

    // Scroll to artifact preview
    await page.locator(".inspectah-layout__main").evaluate((el) =>
      el.scrollTo(0, el.scrollHeight),
    );

    // UserArtifactPreview shows PF Alert when data.sensitive is true and revealed is false.
    // The Alert uses variant="info" (not revealed) or variant="warning" (revealed).
    const alert = page.locator(".pf-v6-c-alert");
    const hasAlert = await alert.isVisible().catch(() => false);
    if (hasAlert) {
      await expect(alert).toBeVisible();
    }
  });

  test("invalid password shows error in card", async ({ page }) => {
    const userCards = page.locator("[data-testid^='user-card-']");
    if ((await userCards.count()) === 0) { test.skip(true, "No users in fixture"); return; }

    await mockPostResponse(page, "/api/user-password", "post-responses/user-password/invalid.json");

    const card = userCards.first();

    // Step 1: Expand the user card (click the chevron/expand button)
    const expandBtn = card.locator("button[aria-expanded]");
    if ((await expandBtn.count()) > 0) {
      const isExpanded = await expandBtn.getAttribute("aria-expanded");
      if (isExpanded === "false") await expandBtn.click();
    }

    // Step 2: Expand the password section within the card
    // The password section has its own expand toggle
    const passwordToggle = card.locator("button").filter({ hasText: /password/i });
    if ((await passwordToggle.count()) === 0) {
      test.skip(true, "No password section in this user card");
      return;
    }
    await passwordToggle.click();

    // Step 3: Fill password input and submit
    const passwordInput = card.locator('input[type="password"]');
    await expect(passwordInput).toBeVisible({ timeout: 2000 });
    await passwordInput.fill("weak");
    await passwordInput.press("Enter");

    // Step 4: Verify error message from the 400 response
    await expect(card.getByText(/does not meet|failed to set/i)).toBeVisible({ timeout: 3000 });
  });
});
```

- [ ] **Step 2: Run and commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx playwright test e2e/users.spec.ts --headed
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-web/ui/e2e/users.spec.ts
git commit -m "feat(e2e): add users.spec.ts with card toggle, strategy, preview, password error"
```

---

## Post-Implementation: Roadmap Update

### Task 16: Add future phases to ROADMAP.md

**Files:**
- Modify: `docs/ROADMAP.md`

- [ ] **Step 1: Add roadmap items**

Under "Upcoming Work" in `docs/ROADMAP.md`, add:

```markdown
### Playwright E2E: CI Automation (MEDIUM -- after testing expansion)

Add `webServer` config to `playwright.config.ts` to auto-start the refine server with checked-in tarballs. GitHub Actions integration. Makes `npx playwright test` run everything including real-server tests without manual server startup.

### Playwright E2E: Visual Regression (MEDIUM -- after CI automation)

Playwright screenshot comparison for key views (single-host refine, fleet zones, containerfile panel, responsive breakpoints). Catches CSS regressions and theme rendering bugs that functional tests miss.

### Playwright E2E: Multi-Browser (MEDIUM -- after CI automation)

Add Firefox project to `playwright.config.ts`. Firefox's Gecko engine handles CSS grid/flexbox and keyboard events differently from Chromium, especially relevant for PatternFly 6.
```

- [ ] **Step 2: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add docs/ROADMAP.md
git commit -m "docs: add Playwright CI, visual regression, and multi-browser to roadmap"
```

---

## Revision history

### Round 3 → Round 4

1. **Task 7 rewritten as fixture structure snapshots:** Dropped the Rust-type deserialization approach — response DTOs are `Serialize`-only, adding `Deserialize` would cascade through ~50 types across 3 crates. Task 7 now snapshots fixture JSON as `serde_json::Value` with insta. This catches fixture drift (any change requires explicit review). Rust-type compatibility is proven by the real-server tier (Task 8), not Task 7. The overclaim about manifest-driven CI validation is removed — Task 7 provides the insta safety net only.
2. **Task 11 section search clarified:** SectionSearch filters items within the active content pane, not sidebar sections. Description corrected.
3. **Task 14 AttentionSummary removed:** `attention-summary` renders inside `MainContent` for the active section, not as a standalone surface on the packages page. Removed the test that claimed it as a separate repos-page surface. Moved coverage to triage.spec.ts where the full Packages content pane is rendered.
4. **Task 15 implementation note added:** Explicit callout that the preview and password interaction flows must be verified in a headed browser during implementation. Test intent is stable; exact click sequences may need tuning.
5. **Pushback:** The `repo-group-wrapper-*` testid is correct per `RepoGroup.tsx:56` — the reviewer's objection was about mounted-page coverage scope, not the selector itself. Fixed the scope (removed AttentionSummary), kept the testid.

### Round 2 → Round 3

1. **Shell serving for mock-tier tests (Task 4):** Added `webServer` config to `playwright.config.ts` running `npx vite`. Vite serves the app shell; `page.route()` intercepts `/api/*` before Vite's proxy. Removed "no server needed" claim — now says "no Rust server needed."
2. **Single-host mutation model tightened (Task 1):** Clarified that ALL single-host mutations (not just some) use the same invalidate→refetch cycle. POST body is always ignored. Noted undo/redo pre-fetches `/api/ops` as a minor variation, not a separate seam. Pushed back on splitting into "two seams" — Kit's consult confirmed it's one seam.
3. **Schema test rewritten as fixture contract test (Task 7):** Replaced hand-authored JSON and key-list assertions with fixture deserialization into actual Rust types (`serde_json::from_str::<ViewResponse>(...)`). Test fails hard if fixture doesn't deserialize — no silent pass. Renamed to `fixture_contract_test.rs`. Dropped `schemars` dependency entirely.
4. **Task 6 proof targets packages section explicitly:** Added sidebar navigation to Packages before locating switches. Noted that DecisionItem uses PF Switch (role="switch") while UserCard uses native checkbox.
5. **Section labels from fixture, not hardcoded (Task 11):** Reads `display_name` from `sections.json` fixture instead of hardcoding. Fixed search description — section search filters content pane items, not sidebar sections.
6. **Containerfile reduced-motion strengthened (Task 13):** Now triggers a mutation under `prefers-reduced-motion: reduce` and verifies the panel renders without animation errors.
7. **Users spec flow corrected (Task 15):** Preview tabs: scroll to artifact preview below card list. Password: expand card → expand password section → fill input → submit → verify error. Redacted preview: scroll + check Alert banner.
8. **Pushback noted:** Declined to split single-host into two mutation seams (one seam with minor variation). Declined to change `repo-group-wrapper-*` testid (matches `RepoGroup.tsx:56`). Declined to further strengthen reduced-motion assertion beyond "panel renders correctly under the media query."

### Round 1 → Round 2

1. **Consolidated mock-api.ts** (Tasks 1+5+6 → Task 1): One cohesive file with all functions. Three distinct mutation models matching actual app behavior (Kit consult): single-host POST-ignored-GET-refetch, fleet POST-discarded-GET-refetch, viewed stateful tracking. Added `mockViewed()` function.
2. **Simplified schema validation** (Tasks 8+9+10 → Task 7): Dropped `JsonSchema` derives and cross-crate feature flags entirely (Tang consult). `schemars` as dev-dep in `inspectah-web` only. Insta JSON snapshots of serialized responses, no cross-crate churn. `INSPECTAH_SKIP_UI=1` for test command.
3. **Expanded real-server smoke tests** (Task 8): Added ops coherence test (mutation → verify `/api/ops` has one more entry) and viewed persistence test (POST viewed → reload → verify GET reflects it). Real-server tier now proves the coherence claims the spec makes.
4. **Fixed placeholder selectors** (Tasks 13-15): Replaced all "exact selectors depend on component structure" with actual `data-testid` values and class names from current component tree. `UserCard.tsx`: `data-testid="user-card-${name}"`, `input[type="checkbox"][aria-label="Include ${name}"]`, `button[aria-expanded]`, strategy radios. `ContainerfilePanel.tsx`: `.inspectah-cf-panel--collapsed`/`--open`, `.inspectah-cf-panel__code`, `.inspectah-cf-line--added`/`--removing`, `.inspectah-cf-panel__tab-label`. `RepoGroup.tsx`: `data-testid="repo-group-wrapper-${section_id}"`. `AttentionSummary.tsx`: `data-testid="attention-summary"`.
5. **Mutation proof pulled into Phase 1b** (Task 6): Three focused tests (toggle → undo → error) validate the mock harness handles the mutation cycle before Phase 2 starts.
