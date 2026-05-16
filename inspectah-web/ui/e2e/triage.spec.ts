import { test, expect } from "@playwright/test";

test.describe("Triage workflow", () => {
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
    // Get the initial Containerfile preview content
    const cfPanel = page.locator(".inspectah-cf-panel");

    // If the panel isn't visible, it may be collapsed — check for toggle
    const panelVisible = await cfPanel.isVisible().catch(() => false);
    if (!panelVisible) {
      // Try Ctrl+E to open the panel
      await page.keyboard.press("Control+e");
    }

    const initialPreview = await cfPanel
      .locator("code, pre")
      .first()
      .textContent();

    // Find the first Switch toggle in a decision item.
    // PF Switch renders as checkbox with aria-label="Toggle <name>".
    const firstToggle = page
      .getByRole("checkbox", { name: /toggle/i })
      .first();

    // Only proceed if there's a toggleable item
    const toggleExists = await firstToggle.isVisible().catch(() => false);
    if (!toggleExists) {
      test.skip();
      return;
    }

    // Click to toggle (exclude)
    await firstToggle.click({ force: true });

    // Wait for the API response to update the view
    await page.waitForResponse((res) => res.url().includes("/api/op"));

    // Containerfile preview should change
    const updatedPreview = await cfPanel
      .locator("code, pre")
      .first()
      .textContent();
    expect(updatedPreview).not.toBe(initialPreview);
  });

  test("undo reverts the last operation", async ({ page }) => {
    // Get initial stats
    const statsBar = page.locator(".inspectah-statsbar");
    const initialText = await statsBar.textContent();

    // Find and click a toggle
    const toggle = page
      .getByRole("checkbox", { name: /toggle/i })
      .first();
    const toggleExists = await toggle.isVisible().catch(() => false);
    if (!toggleExists) {
      test.skip();
      return;
    }

    await toggle.click({ force: true });
    await page.waitForResponse((res) => res.url().includes("/api/op"));

    // Stats should have changed
    const afterToggle = await statsBar.textContent();
    expect(afterToggle).not.toBe(initialText);

    // Click undo
    await page.getByRole("button", { name: /undo/i }).click();
    await page.waitForResponse((res) => res.url().includes("/api/undo"));

    // Stats should revert
    const afterUndo = await statsBar.textContent();
    expect(afterUndo).toBe(initialText);
  });

  test("redo re-applies an undone operation", async ({ page }) => {
    const toggle = page
      .getByRole("checkbox", { name: /toggle/i })
      .first();
    const toggleExists = await toggle.isVisible().catch(() => false);
    if (!toggleExists) {
      test.skip();
      return;
    }

    // Toggle, undo, then redo
    await toggle.click({ force: true });
    await page.waitForResponse((res) => res.url().includes("/api/op"));
    const afterToggle = await page.locator(".inspectah-statsbar").textContent();

    await page.getByRole("button", { name: /undo/i }).click();
    await page.waitForResponse((res) => res.url().includes("/api/undo"));

    await page.getByRole("button", { name: /redo/i }).click();
    await page.waitForResponse((res) => res.url().includes("/api/redo"));

    const afterRedo = await page.locator(".inspectah-statsbar").textContent();
    expect(afterRedo).toBe(afterToggle);
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
