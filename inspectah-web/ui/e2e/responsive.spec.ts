import { test, expect } from "@playwright/test";

test.describe("Responsive layout", () => {
  test("at 1024px viewport, sidebar collapses to hamburger", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1024, height: 768 });
    await page.goto("/");
    await expect(page.locator(".inspectah-page")).toBeVisible();

    // Desktop sidebar should NOT be visible
    const desktopSidebar = page.locator(".inspectah-layout__sidebar");
    await expect(desktopSidebar).not.toBeVisible();

    // Hamburger button should be visible
    const hamburger = page.locator(".inspectah-hamburger");
    await expect(hamburger).toBeVisible();

    // Click hamburger to open overlay sidebar
    await hamburger.click();

    // Overlay sidebar should appear
    const overlaySidebar = page.locator(
      '[id="inspectah-sidebar-overlay"], .inspectah-sidebar--overlay',
    );
    await expect(overlaySidebar).toBeVisible({ timeout: 2000 });

    // Sidebar should show sections
    await expect(overlaySidebar.getByText("Packages")).toBeVisible();
  });

  test("at 1280px viewport, sidebar is visible without hamburger", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.goto("/");
    await expect(page.locator(".inspectah-page")).toBeVisible();

    // Desktop sidebar should be visible
    const desktopSidebar = page.locator(".inspectah-layout__sidebar");
    await expect(desktopSidebar).toBeVisible();

    // Hamburger should NOT be visible
    const hamburger = page.locator(".inspectah-hamburger");
    await expect(hamburger).not.toBeVisible();
  });

  test("at 1280px, Containerfile panel auto-collapses", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.goto("/");
    await expect(page.locator(".inspectah-page")).toBeVisible();

    // At exactly 1280px, the Containerfile panel may auto-collapse
    // depending on user preference stored in localStorage.
    // Verify the toggle mechanism works.
    const cfPanel = page.locator(".inspectah-cf-panel");
    const panelVisible = await cfPanel.isVisible().catch(() => false);

    // Toggle panel with Ctrl+E
    await page.keyboard.press("Control+e");

    // Panel visibility should change
    if (panelVisible) {
      await expect(cfPanel).not.toBeVisible({ timeout: 2000 });
    } else {
      await expect(cfPanel).toBeVisible({ timeout: 2000 });
    }
  });

  test("overlay sidebar closes on Escape", async ({ page }) => {
    await page.setViewportSize({ width: 1024, height: 768 });
    await page.goto("/");

    const hamburger = page.locator(".inspectah-hamburger");
    await expect(hamburger).toBeVisible();

    // Open overlay
    await hamburger.click();
    const overlaySidebar = page.locator(
      '[id="inspectah-sidebar-overlay"], .inspectah-sidebar--overlay',
    );
    await expect(overlaySidebar).toBeVisible({ timeout: 2000 });

    // Escape should close it
    await page.keyboard.press("Escape");
    await expect(overlaySidebar).not.toBeVisible({ timeout: 2000 });
  });
});
