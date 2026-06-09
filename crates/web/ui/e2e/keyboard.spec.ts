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
    await page.locator(".inspectah-layout__sidebar").getByText("Config Files").click();
    // Expand the summary group to reveal individual items
    await page.getByText("Routine").click();
    // Click the first visible decision item to give the DecisionList focus
    const firstItem = page.locator('[data-testid^="decision-item-"]').first();
    await expect(firstItem).toBeVisible();
    await firstItem.click();
    // Now j/k should navigate within the focused DecisionList
    await page.keyboard.press("j");
    const focusedRow = page.locator('[data-testid^="decision-item-"][tabindex="0"]');
    await expect(focusedRow.first()).toBeVisible();
    await page.keyboard.press("k");
    await expect(focusedRow.first()).toBeVisible();
  });

  test("/ opens section search", async ({ page }) => {
    await page.locator(".inspectah-layout__sidebar").getByText("Config Files").click();
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
