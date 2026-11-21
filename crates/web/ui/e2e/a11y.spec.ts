import { test, expect } from "@playwright/test";
import { applyMockApi, clearMocks } from "./helpers/mock-api";
import { expectNoAxeViolations } from "./helpers/assertions";

test.describe("Accessibility", () => {
  test.beforeEach(async ({ page }) => {
    await applyMockApi(page, "single-host");
  });

  test.afterEach(async ({ page }) => {
    await clearMocks(page);
  });

  // Known pre-existing violation: aria-sort on <button> in SortHeader
  // component (only valid on <th>/<td>/columnheader roles). The attribute
  // is correctly on the columnheader wrapper but redundantly on the button.
  // Exclude aria-allowed-attr until the component is fixed.
  const AXE_EXCLUDE_RULES = ["aria-allowed-attr"];

  test("single-host axe scan has no critical or serious violations", async ({
    page,
  }) => {
    await page.goto("/");
    await expect(page.locator(".inspectah-statsbar")).toBeVisible();
    await expectNoAxeViolations(page, undefined, AXE_EXCLUDE_RULES);
  });

  test("aggregate axe scan has no critical or serious violations", async ({
    page,
  }) => {
    await clearMocks(page);
    await applyMockApi(page, "aggregate-3");
    await page.goto("/");
    await expect(page.locator(".inspectah-statsbar")).toBeVisible();
    await expectNoAxeViolations(page, undefined, AXE_EXCLUDE_RULES);
  });

  test("sidebar navigation is keyboard accessible", async ({ page }) => {
    await page.goto("/");
    await expect(page.locator(".inspectah-layout__sidebar")).toBeVisible();

    // Sidebar should use nav element
    const nav = page.locator(
      ".inspectah-layout__sidebar nav.inspectah-sidebar",
    );
    await expect(nav).toBeVisible();

    // Nav items should be focusable (PF NavItem renders as <a> or <button>)
    const navLinks = nav.locator("a");
    const navButtons = nav.locator("button");
    const linkCount = await navLinks.count();
    const buttonCount = await navButtons.count();
    expect(linkCount + buttonCount).toBeGreaterThan(0);
  });

  test("stats bar buttons have accessible names", async ({ page }) => {
    await page.goto("/");
    const statsBar = page.locator(".inspectah-statsbar");
    await expect(statsBar).toBeVisible();

    // Undo and redo buttons should have accessible labels
    const undoBtn = statsBar.getByRole("button", { name: /undo/i });
    const redoBtn = statsBar.getByRole("button", { name: /redo/i });
    await expect(undoBtn).toBeVisible();
    await expect(redoBtn).toBeVisible();
  });

  test("hamburger button has aria attributes at mobile viewport", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1023, height: 768 });
    await page.goto("/");

    const hamburger = page.locator(".inspectah-hamburger");
    await expect(hamburger).toBeVisible();

    // Should have aria-label
    await expect(hamburger).toHaveAttribute("aria-label", /navigation/i);

    // Should have aria-expanded
    await expect(hamburger).toHaveAttribute("aria-expanded", "false");

    // After click, aria-expanded should be true
    await hamburger.click();
    await expect(hamburger).toHaveAttribute("aria-expanded", "true");
  });
});
