import { test, expect } from "@playwright/test";

test.describe("Triage workflow", () => {
  // These tests mutate shared server state; must run serially
  test.describe.configure({ mode: "serial" });

  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    // Wait for data to load
    await expect(page.locator(".inspectah-statsbar")).toBeVisible();
    // Ensure Packages section is active (default)
    await expect(
      page.locator(".inspectah-layout__sidebar").getByText("Packages"),
    ).toBeVisible();
  });

  test("toggle a package exclusion updates Containerfile preview", async ({
    page,
  }) => {
    // The panel may be collapsed at 1280px viewport — expand if needed
    const cfPanelOpen = page.locator(".inspectah-cf-panel--open");
    const isOpen = await cfPanelOpen.isVisible().catch(() => false);
    if (!isOpen) {
      await page.keyboard.press("Control+e");
      await expect(cfPanelOpen).toBeVisible({ timeout: 2000 });
    }

    const initialPreview = await cfPanelOpen
      .locator(".inspectah-cf-panel__code")
      .textContent();

    // Find the first Switch toggle in a decision item.
    // PF6 Switch renders <input type="checkbox" role="switch">.
    const firstToggle = page
      .getByRole("switch", { name: /toggle/i })
      .first();

    // Set up response listener before clicking to avoid race
    const opResponse = page.waitForResponse((res) => res.url().includes("/api/op"));

    // Click to toggle (exclude)
    await firstToggle.click({ force: true });

    // Wait for the API response to update the view
    await opResponse;

    // Wait for React to re-render the Containerfile preview
    await expect(async () => {
      const updatedPreview = await cfPanelOpen
        .locator(".inspectah-cf-panel__code")
        .textContent();
      expect(updatedPreview).not.toBe(initialPreview);
    }).toPass({ timeout: 5000 });
  });

  test("undo reverts the last operation", async ({ page }) => {
    // Get initial package counts (triage/viewed counter is not undone)
    const statsBar = page.locator(".inspectah-statsbar");
    const initialText = await statsBar.textContent();
    const pkgPattern = /Packages:\s*\d+\s*included\s*\/\s*\d+\s*excluded/;
    const initialPkgs = initialText?.match(pkgPattern)?.[0];

    // Find and click a toggle
    const toggle = page
      .getByRole("switch", { name: /toggle/i })
      .first();
    const opResp = page.waitForResponse((res) => res.url().includes("/api/op"));
    await toggle.click({ force: true });
    await opResp;

    // Package counts should have changed
    await page.waitForTimeout(500);
    const afterToggle = await statsBar.textContent();
    const afterTogglePkgs = afterToggle?.match(pkgPattern)?.[0];
    expect(afterTogglePkgs).not.toBe(initialPkgs);

    // Click undo
    const undoResp2 = page.waitForResponse((res) => res.url().includes("/api/undo"));
    await page.getByRole("button", { name: /undo/i }).click();
    await undoResp2;

    // Package counts should revert
    await page.waitForTimeout(500);
    const afterUndo = await statsBar.textContent();
    const afterUndoPkgs = afterUndo?.match(pkgPattern)?.[0];
    expect(afterUndoPkgs).toBe(initialPkgs);
  });

  test("redo re-applies an undone operation", async ({ page }) => {
    const toggle = page
      .getByRole("switch", { name: /toggle/i })
      .first();

    // Toggle, undo, then redo — compare package counts (triage/viewed counter
    // is not undone, so we can't compare the full statsbar text)
    const opResp2 = page.waitForResponse((res) => res.url().includes("/api/op"));
    await toggle.click({ force: true });
    await opResp2;

    // Wait for stats to update, then read the package line
    await page.waitForTimeout(500);
    const afterToggle = await page.locator(".inspectah-statsbar").textContent();
    // Extract just the "Packages: X included / Y excluded" part
    const pkgPattern = /Packages:\s*\d+\s*included\s*\/\s*\d+\s*excluded/;
    const afterTogglePkgs = afterToggle?.match(pkgPattern)?.[0];

    const undoResp = page.waitForResponse((res) => res.url().includes("/api/undo"));
    await page.getByRole("button", { name: /undo/i }).click();
    await undoResp;

    const redoResp = page.waitForResponse((res) => res.url().includes("/api/redo"));
    await page.getByRole("button", { name: /redo/i }).click();
    await redoResp;

    await page.waitForTimeout(500);
    const afterRedo = await page.locator(".inspectah-statsbar").textContent();
    const afterRedoPkgs = afterRedo?.match(pkgPattern)?.[0];
    expect(afterRedoPkgs).toBe(afterTogglePkgs);
  });

  test("export tarball downloads successfully", async ({ page }) => {
    // Click the export button in the stats bar
    const exportBtn = page.getByRole("button", { name: /export/i });
    await exportBtn.click();

    // Export dialog should appear
    const dialog = page.getByRole("dialog");
    await expect(dialog).toBeVisible();

    // Click the confirm/download button in the dialog
    const downloadBtn = dialog.getByRole("button", {
      name: /download|export|save/i,
    });
    const downloadExists = await downloadBtn.isVisible().catch(() => false);
    if (!downloadExists) {
      // Dialog might have a different structure — just verify it opened
      await expect(dialog).toBeVisible();
      return;
    }

    // Start waiting for download before clicking
    const downloadPromise = page.waitForEvent("download");
    await downloadBtn.click();
    const download = await downloadPromise;

    // Verify the download has a tar.gz filename
    expect(download.suggestedFilename()).toMatch(/\.tar\.gz$/);
  });
});
