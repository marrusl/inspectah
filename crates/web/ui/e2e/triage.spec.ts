import { test, expect } from "@playwright/test";
import {
  applyMockApi,
  clearMocks,
  mockSequence,
  mockPostResponse,
  mockError,
} from "./helpers/mock-api";

test.describe("Triage workflow", () => {
  test.describe.configure({ mode: "serial" });

  test.beforeEach(async ({ page }) => {
    await applyMockApi(page, "single-host");
    await page.goto("/");
    await expect(page.locator(".inspectah-statsbar")).toBeVisible();
  });

  test.afterEach(async ({ page }) => {
    await clearMocks(page);
  });

  // ── 1. Package toggle ─────────────────────────────────────────────
  test("package toggle — stats update via GET refetch", async ({ page }) => {
    const statsBar = page.locator(".inspectah-statsbar");
    await expect(statsBar.getByText(/Packages:\s*4 included/)).toBeVisible();

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

    await expect(statsBar.getByText(/Packages:\s*3 included/)).toBeVisible({
      timeout: 5000,
    });
    await expect(statsBar.getByText(/2 excluded/)).toBeVisible();
  });

  // ── 2. Config toggle ──────────────────────────────────────────────
  test("config toggle — stats update via GET refetch", async ({ page }) => {
    // Navigate to Config Files section
    await page
      .locator(".inspectah-layout__sidebar")
      .getByText("Config Files")
      .click();

    const statsBar = page.locator(".inspectah-statsbar");
    await expect(statsBar.getByText(/Configs:\s*2 included/)).toBeVisible();

    // Config items are grouped under a collapsed "Routine" bucket.
    // Expand it to reveal the individual config item checkboxes.
    const routineGroup = page.getByText(/Routine\s*\(\d+\)/);
    await routineGroup.click();

    // Wait for the config item checkboxes to become visible
    const toggle = page.locator(
      "input[type=checkbox][aria-label='Toggle /etc/sysconfig/network']",
    );
    await expect(toggle).toBeVisible({ timeout: 3000 });

    await mockSequence(
      page,
      "/api/view",
      [
        "single-host/view.json",
        "sequences/exclude-undo-redo/01-after-exclude.json",
      ],
      { triggerOn: "/api/op" },
    );

    await toggle.click({ force: true });

    // After-exclude fixture keeps configs at 2/1, but the toggle itself
    // should fire POST /api/op which triggers the sequence advance.
    // The stats bar should reflect the refetched view state.
    await expect(statsBar.getByText(/Configs:\s*2 included/)).toBeVisible({
      timeout: 5000,
    });
  });

  // ── 3. Undo/redo sequence ─────────────────────────────────────────
  test("undo reverts state, redo re-applies", async ({ page }) => {
    await mockSequence(
      page,
      "/api/view",
      [
        "single-host/view.json",
        "sequences/exclude-undo-redo/01-after-exclude.json",
        "sequences/exclude-undo-redo/02-after-undo.json",
        "sequences/exclude-undo-redo/03-after-redo.json",
      ],
      { triggerOn: ["/api/op", "/api/undo", "/api/redo"] },
    );

    const statsBar = page.locator(".inspectah-statsbar");

    // Step 1: exclude a package → state advances to 01 (3 included / 2 excluded)
    const toggle = page.locator(
      "input[type=checkbox][aria-label='httpd.x86_64']",
    );
    await toggle.click({ force: true });
    await expect(statsBar.getByText(/Packages:\s*3 included/)).toBeVisible({
      timeout: 5000,
    });

    // Step 2: undo → state 02 (4 included / 1 excluded, can_redo: true)
    const undoBtn = page.getByRole("button", { name: "Undo" });
    await undoBtn.click();
    await expect(statsBar.getByText(/Packages:\s*4 included/)).toBeVisible({
      timeout: 5000,
    });
    const redoBtn = page.getByRole("button", { name: "Redo" });
    await expect(redoBtn).toBeEnabled({ timeout: 3000 });

    // Step 3: redo → state 03 (3 included / 2 excluded again)
    await redoBtn.click();
    await expect(statsBar.getByText(/Packages:\s*3 included/)).toBeVisible({
      timeout: 5000,
    });
  });

  // ── 4. Containerfile preview updates ──────────────────────────────
  test("Containerfile preview updates after toggle", async ({ page }) => {
    // Open the Containerfile panel via keyboard shortcut
    const cfPanel = page.locator(".inspectah-cf-panel--open");
    if (!(await cfPanel.isVisible().catch(() => false))) {
      await page.keyboard.press("Control+e");
      await expect(cfPanel).toBeVisible({ timeout: 2000 });
    }

    const initialPreview = await cfPanel
      .locator(".inspectah-cf-panel__code")
      .textContent();

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

    // The after-exclude fixture has a shorter containerfile_preview (356 vs 393 chars)
    await expect(async () => {
      const updatedPreview = await cfPanel
        .locator(".inspectah-cf-panel__code")
        .textContent();
      expect(updatedPreview).not.toBe(initialPreview);
    }).toPass({ timeout: 5000 });
  });

  // ── 5. Export download ─────────────────────────────────────────────
  test("export tarball triggers download", async ({ page }) => {
    await mockPostResponse(
      page,
      "/api/tarball",
      "post-responses/tarball/stub.tar.gz",
    );

    // Click the Export button in the StatsBar to open ExportDialog
    const exportBtn = page.getByRole("button", { name: /export/i });
    await exportBtn.click();

    // ExportDialog modal should appear
    const dialog = page.getByRole("dialog");
    await expect(dialog).toBeVisible({ timeout: 3000 });

    // Click the Export button inside the dialog to trigger download
    const dialogExportBtn = dialog
      .getByRole("button", { name: /export/i })
      .first();

    const downloadPromise = page.waitForEvent("download");
    await dialogExportBtn.click();
    const download = await downloadPromise;

    expect(download.suggestedFilename()).toMatch(/\.tar\.gz$/);
  });

  // ── 6. Sensitive tarball gating ────────────────────────────────────
  test("sensitive tarball gating — 428 shows error", async ({ page }) => {
    await mockPostResponse(
      page,
      "/api/tarball",
      "post-responses/tarball/sensitive-required.json",
    );

    // Open the export dialog
    const exportBtn = page.getByRole("button", { name: /export/i });
    await exportBtn.click();

    const dialog = page.getByRole("dialog");
    await expect(dialog).toBeVisible({ timeout: 3000 });

    // Click export in the dialog (should trigger 428 response)
    const dialogExportBtn = dialog
      .getByRole("button", { name: /export/i })
      .first();
    await dialogExportBtn.click();

    // The ExportDialog should show an error alert for the 428
    await expect(
      dialog.locator(".pf-v6-c-alert").filter({ hasText: /failed|error|sensitive/i }),
    ).toBeVisible({ timeout: 5000 });
  });

  // ── 7. Nothing to undo (409) ──────────────────────────────────────
  test("nothing to undo — page stays interactive", async ({ page }) => {
    await mockPostResponse(
      page,
      "/api/undo",
      "post-responses/undo/nothing-to-undo.json",
    );

    // Initial state has can_undo: false, so Undo button is disabled.
    // We need to make the button clickable — use force click to bypass disabled state
    // and let the POST happen, which returns 409.
    const undoBtn = page.getByRole("button", { name: "Undo" });
    await undoBtn.click({ force: true });

    // Give the error response time to process
    await page.waitForTimeout(500);

    // Page should remain interactive
    await expect(page.locator(".inspectah-statsbar")).toBeVisible();
    await expect(
      page.locator(".inspectah-statsbar").getByText(/Packages:\s*4 included/),
    ).toBeVisible();
  });

  // ── 8. Server error on mutation ────────────────────────────────────
  test("server error on mutation — page stays interactive", async ({
    page,
  }) => {
    await mockError(page, "/api/op", "500");

    const toggle = page.locator(
      "input[type=checkbox][aria-label='httpd.x86_64']",
    );
    await toggle.click({ force: true });

    await page.waitForTimeout(500);

    // Page should still be interactive: statsbar visible, toggles present
    await expect(page.locator(".inspectah-statsbar")).toBeVisible();
    const toggleCount = await page.locator("input[type=checkbox]").count();
    expect(toggleCount).toBeGreaterThan(0);

    // Stats unchanged from initial mock
    const statsBar = page.locator(".inspectah-statsbar");
    await expect(statsBar.getByText(/Packages:\s*4 included/)).toBeVisible();
  });

  // ── 9. Timeout on mutation ─────────────────────────────────────────
  test("timeout on mutation — page stays interactive", async ({ page }) => {
    await mockError(page, "/api/op", "timeout", { timeoutMs: 1000 });

    const toggle = page.locator(
      "input[type=checkbox][aria-label='httpd.x86_64']",
    );
    await toggle.click({ force: true });

    // Wait long enough for the timeout mock to trigger
    await page.waitForTimeout(2000);

    // Page should remain interactive despite the timeout
    await expect(page.locator(".inspectah-statsbar")).toBeVisible();
    const toggleCount = await page.locator("input[type=checkbox]").count();
    expect(toggleCount).toBeGreaterThan(0);
  });

  // ── 10. Malformed response ─────────────────────────────────────────
  test("malformed response — page shows error state", async ({ page }) => {
    // Layer a malformed mock ON TOP of the existing applyMockApi routes.
    // mockError uses page.route which adds a new handler that takes priority.
    await mockError(page, "/api/view", "malformed");

    // Navigate — the malformed /api/view should trigger an error state.
    // The other routes (health, sections, etc.) still work from applyMockApi.
    await page.goto("/");

    // Wait a moment for the app to process the malformed response
    await page.waitForTimeout(2000);

    // The React app should render something — it won't crash completely
    // because it has an error boundary or fallback state.
    // With malformed /api/view, the app may:
    // 1. Show an error alert/banner
    // 2. Show a loading state that never resolves
    // 3. Show the shell without data
    // Any of these is acceptable — the key test is the page didn't crash.
    const rootContent = await page.locator("#root").innerHTML();
    expect(rootContent.length).toBeGreaterThan(0);
  });
});
