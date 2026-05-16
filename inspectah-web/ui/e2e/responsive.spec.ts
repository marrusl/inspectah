import { test, expect } from "@playwright/test";

test.describe("Responsive layout", () => {
  test("at 1024px viewport, sidebar collapses to hamburger", async ({
    page,
  }) => {
    // Breakpoint is max-width: 1023px, so use 800px to be safely below it
    await page.setViewportSize({ width: 800, height: 768 });
    await page.goto("/");
    await expect(page.locator(".inspectah-page")).toBeVisible();

    // Desktop sidebar should NOT be visible (hidden by CSS @media query)
    const desktopSidebar = page.locator(".inspectah-layout__sidebar");
    await expect(desktopSidebar).toBeHidden();

    // Hamburger button should be visible (use semantic selector)
    const hamburger = page.getByRole("button", { name: "Open navigation" });
    await expect(hamburger).toBeVisible();

    // Click hamburger to open overlay sidebar
    await hamburger.click();

    // Wait for overlay sidebar to render (it's conditionally rendered in React)
    await page.waitForSelector(".inspectah-sidebar--overlay", { timeout: 2000 });

    // Overlay sidebar should be visible
    const overlaySidebar = page.locator(".inspectah-sidebar--overlay");
    await expect(overlaySidebar).toBeVisible();

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

    // Hamburger should NOT be in the DOM (isMobile is false, so it's undefined)
    const hamburger = page.getByRole("button", { name: "Open navigation" });
    await expect(hamburger).toBeHidden();
  });

  test("at 1280px, Containerfile panel auto-collapses", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.goto("/");
    await expect(page.locator(".inspectah-page")).toBeVisible();

    // At exactly 1280px, the viewport is at the breakpoint boundary.
    // The panel initializes collapsed due to initialPanelOpen() logic.
    // Verify the panel starts collapsed and can be toggled.
    const cfPanel = page.locator(".inspectah-cf-panel");
    await expect(cfPanel).toBeVisible();

    // Panel should have collapsed class initially at 1280px
    await expect(cfPanel).toHaveClass(/inspectah-cf-panel--collapsed/);

    // Toggle panel with Ctrl+E to expand
    await page.keyboard.press("Control+e");
    await expect(cfPanel).toHaveClass(/inspectah-cf-panel--open/);

    // Toggle again to collapse
    await page.keyboard.press("Control+e");
    await expect(cfPanel).toHaveClass(/inspectah-cf-panel--collapsed/);
  });

  test("overlay sidebar closes on Escape", async ({ page }) => {
    // Use 800px to be safely below the 1023px breakpoint
    await page.setViewportSize({ width: 800, height: 768 });
    await page.goto("/");

    const hamburger = page.getByRole("button", { name: "Open navigation" });
    await expect(hamburger).toBeVisible();

    // Open overlay
    await hamburger.click();
    await page.waitForSelector(".inspectah-sidebar--overlay", { timeout: 2000 });
    const overlaySidebar = page.locator(".inspectah-sidebar--overlay");
    await expect(overlaySidebar).toBeVisible();

    // Escape should close it (sidebar is conditionally unmounted when closed)
    await page.keyboard.press("Escape");
    await expect(overlaySidebar).toBeHidden({ timeout: 2000 });
  });
});
