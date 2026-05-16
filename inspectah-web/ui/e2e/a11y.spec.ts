import { test, expect } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

test.describe("Accessibility", () => {
  test("main page has no critical or serious axe violations", async ({
    page,
  }) => {
    await page.goto("/");
    // Wait for the app to fully load with data
    await expect(page.locator(".inspectah-statsbar")).toBeVisible();
    // Give dynamic content a moment to render
    await page.waitForTimeout(500);

    const results = await new AxeBuilder({ page })
      .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"])
      .analyze();

    const critical = results.violations.filter(
      (v) => v.impact === "critical" || v.impact === "serious",
    );

    if (critical.length > 0) {
      const summary = critical
        .map(
          (v) =>
            `[${v.impact}] ${v.id}: ${v.description} (${v.nodes.length} instance(s))`,
        )
        .join("\n");
      expect(critical, `Accessibility violations found:\n${summary}`).toEqual(
        [],
      );
    }
  });

  test("sidebar navigation is keyboard accessible", async ({ page }) => {
    await page.goto("/");
    await expect(page.locator(".inspectah-layout__sidebar")).toBeVisible();

    // Sidebar should use nav element
    const nav = page.locator(".inspectah-layout__sidebar nav");
    await expect(nav).toBeVisible();

    // Nav items should be focusable (PF NavItem renders as <a> or <button>)
    const navLinks = nav.getByRole("link");
    const navButtons = nav.getByRole("button");
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
    await page.setViewportSize({ width: 1024, height: 768 });
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
