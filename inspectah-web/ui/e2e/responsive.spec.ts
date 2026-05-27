import { test, expect } from "@playwright/test";
import { applyMockApi, clearMocks } from "./helpers/mock-api";

test.describe("Responsive layout", () => {
  test.beforeEach(async ({ page }) => {
    await applyMockApi(page, "single-host");
  });

  test.afterEach(async ({ page }) => {
    await clearMocks(page);
  });

  test("hamburger visible at 768px viewport", async ({ page }) => {
    await page.setViewportSize({ width: 768, height: 1024 });
    await page.goto("/");
    await expect(page.locator(".inspectah-page")).toBeVisible();

    const hamburger = page.getByRole("button", { name: "Open navigation" });
    await expect(hamburger).toBeVisible();
  });

  test("sidebar hidden at mobile viewport", async ({ page }) => {
    await page.setViewportSize({ width: 800, height: 768 });
    await page.goto("/");
    await expect(page.locator(".inspectah-page")).toBeVisible();

    // Desktop sidebar should NOT be visible
    const desktopSidebar = page.locator(".inspectah-layout__sidebar");
    await expect(desktopSidebar).toBeHidden();

    // Hamburger should be visible
    const hamburger = page.getByRole("button", { name: "Open navigation" });
    await expect(hamburger).toBeVisible();

    // Click hamburger to open overlay sidebar
    await hamburger.click();
    await page.waitForSelector(".inspectah-sidebar--overlay", {
      timeout: 2000,
    });
    const overlaySidebar = page.locator(".inspectah-sidebar--overlay");
    await expect(overlaySidebar).toBeVisible();

    // Sidebar should show sections
    await expect(overlaySidebar.getByText("Packages")).toBeVisible();
  });

  test("sidebar visible at desktop viewport", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.goto("/");
    await expect(page.locator(".inspectah-page")).toBeVisible();

    // Desktop sidebar should be visible
    const desktopSidebar = page.locator(".inspectah-layout__sidebar");
    await expect(desktopSidebar).toBeVisible();

    // Hamburger should NOT be visible
    const hamburger = page.getByRole("button", { name: "Open navigation" });
    await expect(hamburger).toBeHidden();
  });

  test("at 1280px, Containerfile panel stays open (full layout)", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.goto("/");
    await expect(page.locator(".inspectah-page")).toBeVisible();

    // At >= 1280px the panel should start open
    const cfPanel = page.locator(".inspectah-cf-panel");
    await expect(cfPanel).toBeVisible();
    await expect(cfPanel).toHaveClass(/inspectah-cf-panel--open/);

    // Toggle panel with Ctrl+E to collapse
    await page.keyboard.press("Control+e");
    await expect(cfPanel).toHaveClass(/inspectah-cf-panel--collapsed/);

    // Toggle again to re-open
    await page.keyboard.press("Control+e");
    await expect(cfPanel).toHaveClass(/inspectah-cf-panel--open/);
  });

  test("at 1279px, Containerfile panel auto-collapses", async ({ page }) => {
    await page.setViewportSize({ width: 1279, height: 800 });
    await page.goto("/");
    await expect(page.locator(".inspectah-page")).toBeVisible();

    // Below 1280px the panel initializes collapsed
    const cfPanel = page.locator(".inspectah-cf-panel");
    await expect(cfPanel).toBeVisible();
    await expect(cfPanel).toHaveClass(/inspectah-cf-panel--collapsed/);
  });

  test("overlay sidebar closes on Escape", async ({ page }) => {
    await page.setViewportSize({ width: 800, height: 768 });
    await page.goto("/");

    const hamburger = page.getByRole("button", { name: "Open navigation" });
    await expect(hamburger).toBeVisible();

    // Open overlay
    await hamburger.click();
    await page.waitForSelector(".inspectah-sidebar--overlay", {
      timeout: 2000,
    });
    const overlaySidebar = page.locator(".inspectah-sidebar--overlay");
    await expect(overlaySidebar).toBeVisible();

    // Escape should close it
    await page.keyboard.press("Escape");
    await expect(overlaySidebar).toBeHidden({ timeout: 2000 });
  });

  test("resize from desktop to mobile transitions layout", async ({
    page,
  }) => {
    // Start at desktop
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.goto("/");
    await expect(page.locator(".inspectah-layout__sidebar")).toBeVisible();

    // Resize to mobile
    await page.setViewportSize({ width: 800, height: 768 });
    await expect(page.locator(".inspectah-layout__sidebar")).toBeHidden();

    const hamburger = page.getByRole("button", { name: "Open navigation" });
    await expect(hamburger).toBeVisible();

    // Resize back to desktop
    await page.setViewportSize({ width: 1280, height: 800 });
    await expect(page.locator(".inspectah-layout__sidebar")).toBeVisible();
    await expect(hamburger).toBeHidden();
  });
});
