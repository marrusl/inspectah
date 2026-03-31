import { test, expect } from '@playwright/test';
import { FLEET_URL } from './helpers';

test.describe('Theme Switching', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto(FLEET_URL);
  });

  test('theme toggle button exists and is clickable', async ({ page }) => {
    const themeBtn = page.locator('#theme-toggle');
    await expect(themeBtn).toBeVisible();
    await expect(themeBtn).toBeEnabled();

    // Verify it has the expected aria-label
    await expect(themeBtn).toHaveAttribute('aria-label', 'Toggle theme');
  });

  test('toggling theme changes html class from dark to light', async ({ page }) => {
    // Default is pf-v6-theme-dark
    const html = page.locator('html');
    await expect(html).toHaveClass(/pf-v6-theme-dark/);

    // Click theme toggle to switch to light
    await page.click('#theme-toggle');

    // The dark class should be removed
    await expect(html).not.toHaveClass(/pf-v6-theme-dark/);

    // Click again to switch back to dark
    await page.click('#theme-toggle');
    await expect(html).toHaveClass(/pf-v6-theme-dark/);
  });

  test('badge contrast check in light mode', async ({ page }) => {
    // Navigate to packages section so sidebar triage badges are visible
    await page.click('a[data-tab="packages"]');
    await expect(page.locator('#section-packages')).toBeVisible();

    // Assert triage badge is visible in the sidebar
    const triageBadge = page.locator('.triage-badge').first();
    await expect(triageBadge).toBeVisible();

    // Switch to light mode
    await page.click('#theme-toggle');
    const html = page.locator('html');
    await expect(html).not.toHaveClass(/pf-v6-theme-dark/);

    // Verify the badge text color differs from its background color
    const styles = await triageBadge.evaluate((el) => {
      const computed = window.getComputedStyle(el);
      return {
        color: computed.color,
        backgroundColor: computed.backgroundColor,
      };
    });

    // Text color and background color must be different for readability
    expect(styles.color).not.toBe(styles.backgroundColor);
  });
});
