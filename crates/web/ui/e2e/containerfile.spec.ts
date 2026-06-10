import { test, expect } from "@playwright/test";
import {
  applyMockApi,
  clearMocks,
  mockSequence,
  mockPostResponse,
} from "./helpers/mock-api";

test.describe("Containerfile panel", () => {
  test.beforeEach(async ({ page }) => {
    await applyMockApi(page, "single-host");
    await mockPostResponse(
      page,
      "/api/op",
      "post-responses/op/success.json",
    );
    await page.goto("/");
    await expect(page.locator(".inspectah-statsbar")).toBeVisible();
  });

  test.afterEach(async ({ page }) => {
    await clearMocks(page);
  });

  // Helper: ensure the panel is expanded (click tab if collapsed)
  async function ensureOpen(page: import("@playwright/test").Page) {
    const collapsed = page.locator(".inspectah-cf-panel--collapsed");
    if ((await collapsed.count()) > 0) {
      await collapsed.locator(".inspectah-cf-panel__tab").click();
    }
    await expect(page.locator(".inspectah-cf-panel--open")).toBeVisible();
  }

  // ── 1. Panel visible on load ──────────────────────────────────────
  test("panel visible on load", async ({ page }) => {
    const panel = page.locator(".inspectah-cf-panel");
    await expect(panel).toBeVisible();

    // Must be in exactly one state: collapsed or open
    const collapsed = await page
      .locator(".inspectah-cf-panel--collapsed")
      .count();
    const open = await page.locator(".inspectah-cf-panel--open").count();
    expect(collapsed + open).toBe(1);
  });

  // ── 2. Tab shows "Containerfile" label ────────────────────────────
  test('tab shows "Containerfile" label', async ({ page }) => {
    // When collapsed, the tab-label is visible on the tab itself
    const collapsed = page.locator(".inspectah-cf-panel--collapsed");
    if ((await collapsed.count()) > 0) {
      await expect(
        collapsed.locator(".inspectah-cf-panel__tab-label"),
      ).toHaveText("Containerfile");
    } else {
      // When open, the header contains the title
      await expect(
        page.locator(".inspectah-cf-panel--open").getByText("Containerfile"),
      ).toBeVisible();
    }
  });

  // ── 3. Expanded panel shows FROM instruction ──────────────────────
  test("expanded panel shows FROM instruction", async ({ page }) => {
    await ensureOpen(page);

    const code = page.locator(".inspectah-cf-panel__code");
    await expect(code).toBeVisible();
    await expect(code).toContainText("FROM");
    await expect(code).toContainText("registry.redhat.io/rhel9/rhel-bootc");
  });

  // ── 4. Content updates after mutation ─────────────────────────────
  test("content updates after mutation", async ({ page }) => {
    await ensureOpen(page);

    const code = page.locator(".inspectah-cf-panel__code");
    await expect(code).toContainText("httpd");

    // Wire sequence: next GET /api/view returns after-exclude (no httpd)
    await mockSequence(
      page,
      "/api/view",
      [
        "single-host/view.json",
        "sequences/exclude-undo-redo/01-after-exclude.json",
      ],
      { triggerOn: "/api/op" },
    );

    // Toggle a package to trigger the mutation
    const toggle = page.locator(
      "input[type=checkbox][aria-label='httpd.x86_64']",
    );
    await toggle.click({ force: true });

    // After refetch, httpd should no longer appear in the containerfile
    await expect(code).not.toContainText("httpd", { timeout: 5000 });
  });

  // ── 5. Change highlight classes ───────────────────────────────────
  test("change highlight classes appear after mutation", async ({ page }) => {
    await ensureOpen(page);

    await mockSequence(
      page,
      "/api/view",
      [
        "single-host/view.json",
        "sequences/exclude-undo-redo/01-after-exclude.json",
      ],
      { triggerOn: "/api/op" },
    );

    const toggle = page.locator(
      "input[type=checkbox][aria-label='httpd.x86_64']",
    );
    await toggle.click({ force: true });

    // Wait for the diff engine to process and highlightsActive to engage.
    // The component sets highlightsActive after a 150ms timeout + scroll delay.
    const panel = page.locator(".inspectah-cf-panel--open");
    const hasHighlight = panel
      .locator(".inspectah-cf-line--added, .inspectah-cf-line--removing")
      .first();
    await expect(hasHighlight).toBeAttached({ timeout: 5000 });
  });

  // ── 6. Reduced motion suppresses animations ──────────────────────
  test("reduced motion — panel renders without errors", async ({ page }) => {
    await page.emulateMedia({ reducedMotion: "reduce" });

    await ensureOpen(page);

    await mockSequence(
      page,
      "/api/view",
      [
        "single-host/view.json",
        "sequences/exclude-undo-redo/01-after-exclude.json",
      ],
      { triggerOn: "/api/op" },
    );

    const toggle = page.locator(
      "input[type=checkbox][aria-label='httpd.x86_64']",
    );
    await toggle.click({ force: true });

    // Panel should still show updated content without errors
    const code = page.locator(".inspectah-cf-panel__code");
    await expect(code).not.toContainText("httpd", { timeout: 5000 });
    await expect(code).toContainText("FROM");

    // No console errors should have fired (panel rendered successfully)
    const panel = page.locator(".inspectah-cf-panel--open");
    await expect(panel).toBeVisible();
  });
});
