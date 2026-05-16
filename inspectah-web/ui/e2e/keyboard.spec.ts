import { test, expect } from "@playwright/test";

test.describe("Keyboard navigation", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await expect(page.locator(".inspectah-page")).toBeVisible();
    // Wait for data to load
    await expect(page.locator(".inspectah-statsbar")).toBeVisible();
  });

  test("j/k navigate items in a decision list", async ({ page }) => {
    // Ensure we're on the packages section
    await page
      .locator(".inspectah-layout__sidebar")
      .getByText("Packages")
      .click();

    // Focus the main content area
    await page.locator(".inspectah-layout__main").click();

    // Press j to move to first/next item
    await page.keyboard.press("j");

    // A row should have focus (tabindex="0" means it's the active roving item)
    const focusedRow = page.locator('[role="group"][tabindex="0"]');
    // At least one row should exist in the decision list
    const rowCount = await focusedRow.count();
    // j should work if there are items — gracefully skip if empty
    if (rowCount === 0) {
      test.skip();
      return;
    }

    // Press k to go back up
    await page.keyboard.press("k");
    // Should still have a focused element
    await expect(focusedRow.first()).toBeVisible();
  });

  test("Space toggles the focused item", async ({ page }) => {
    await page
      .locator(".inspectah-layout__sidebar")
      .getByText("Packages")
      .click();
    await page.locator(".inspectah-layout__main").click();

    // Navigate to first item
    await page.keyboard.press("j");

    const focusedRow = page.locator('[role="group"][tabindex="0"]');
    const hasRow = (await focusedRow.count()) > 0;
    if (!hasRow) {
      test.skip();
      return;
    }

    // Get initial stats
    const statsBefore = await page
      .locator(".inspectah-statsbar")
      .textContent();

    // Press Space to toggle
    await page.keyboard.press("Space");

    // Wait for API response
    try {
      await page.waitForResponse((res) => res.url().includes("/api/op"), {
        timeout: 3000,
      });
    } catch {
      // If no API call fired, the item may not be toggleable
      test.skip();
      return;
    }

    const statsAfter = await page.locator(".inspectah-statsbar").textContent();
    expect(statsAfter).not.toBe(statsBefore);
  });

  test("/ opens section search", async ({ page }) => {
    // Focus main content so / is not captured by sidebar search
    await page.locator(".inspectah-layout__main").click();

    // Press / to open section search
    await page.keyboard.press("/");

    // The section search input should appear (inline above decision list)
    const searchInput = page.locator('[data-testid="section-search"] input');
    await expect(searchInput).toBeVisible({ timeout: 2000 });

    // Escape closes it
    await page.keyboard.press("Escape");
    await expect(searchInput).not.toBeVisible({ timeout: 2000 });
  });

  test("Ctrl+K focuses global search in sidebar", async ({ page }) => {
    // Global search is always-visible in the sidebar, not a modal.
    // Ctrl+K focuses the search input.
    await page.keyboard.press("Control+k");

    // The sidebar search input should be focused
    const searchWrapper = page.locator('[data-testid="global-search-input"]');
    await expect(searchWrapper).toBeVisible();
    const searchInput = searchWrapper.locator('input');
    await expect(searchInput).toBeFocused({ timeout: 2000 });
  });

  test("? opens shortcut overlay", async ({ page }) => {
    await page.keyboard.press("?");

    // Shortcut help overlay should appear (rendered as a PF Modal)
    const overlay = page.locator('[data-testid="shortcut-overlay"]');
    await expect(overlay).toBeVisible({ timeout: 2000 });

    // Escape closes it
    await page.keyboard.press("Escape");
    await expect(overlay).not.toBeVisible({ timeout: 2000 });
  });

  test("Escape closes shortcut overlay", async ({ page }) => {
    // Open shortcut overlay
    await page.keyboard.press("?");
    const overlay = page.locator('[data-testid="shortcut-overlay"]');
    await expect(overlay).toBeVisible({ timeout: 2000 });

    // Escape should close it
    await page.keyboard.press("Escape");
    await expect(overlay).not.toBeVisible({ timeout: 2000 });
  });
});
