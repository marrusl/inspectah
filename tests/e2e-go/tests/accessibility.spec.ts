/**
 * Accessibility tests for the refine report UI.
 * Validates ARIA attributes, skip links, and keyboard navigation basics.
 */
import { test, expect } from '@playwright/test';
import { waitForBoot, navigateToSection } from './helpers';

test.describe('Accessibility', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await waitForBoot(page);
  });

  test('skip-to-content link exists', async ({ page }) => {
    const skipLink = page.locator('.pf-v6-c-skip-to-content__link');
    await expect(skipLink).toBeAttached();
    await expect(skipLink).toHaveAttribute('href', '#main-content');
  });

  test('masthead has banner role', async ({ page }) => {
    const masthead = page.locator('.pf-v6-c-masthead');
    await expect(masthead).toHaveAttribute('role', 'banner');
  });

  test('sidebar has navigation role', async ({ page }) => {
    const sidebar = page.locator('#sidebar');
    await expect(sidebar).toHaveAttribute('role', 'navigation');
  });

  test('rebuild bar has region role and label', async ({ page }) => {
    const rebuildBar = page.locator('#rebuild-bar');
    await expect(rebuildBar).toHaveAttribute('role', 'region');
    await expect(rebuildBar).toHaveAttribute('aria-label', 'Rebuild controls');
  });

  test('rebuild button has descriptive aria-label', async ({ page }) => {
    const rebuildBtn = page.locator('#rebuild-btn');
    await expect(rebuildBtn).toHaveAttribute(
      'aria-label',
      'Rebuild Containerfile from current decisions'
    );
  });

  test('rebuild status has status role', async ({ page }) => {
    const status = page.locator('#rebuild-status');
    await expect(status).toHaveAttribute('role', 'status');
  });

  test('autosave live region exists for screen readers', async ({ page }) => {
    const liveRegion = page.locator('#autosave-live');
    await expect(liveRegion).toBeAttached();
    await expect(liveRegion).toHaveAttribute('aria-live', 'polite');
  });

  test('hamburger button has aria-controls and aria-expanded', async ({ page }) => {
    const hamburger = page.locator('#hamburger-btn');
    await expect(hamburger).toHaveAttribute('aria-controls', 'sidebar');
    await expect(hamburger).toHaveAttribute('aria-expanded', 'false');
  });

  test('toggle buttons use aria-checked', async ({ page }) => {
    // item-toggle buttons are in config/runtime sections, not packages
    await navigateToSection(page, 'config');

    const toggles = page.locator('.item-toggle');
    const count = await toggles.count();
    if (count === 0) {
      test.skip();
      return;
    }

    // Every toggle should have an aria-checked attribute
    for (let i = 0; i < Math.min(count, 5); i++) {
      const ariaChecked = await toggles.nth(i).getAttribute('aria-checked');
      expect(['true', 'false']).toContain(ariaChecked);
    }
  });

  test('section headings have tabindex for focus management', async ({ page }) => {
    const heading = page.locator('#heading-overview');
    const tabindex = await heading.getAttribute('tabindex');
    expect(tabindex).toBe('-1');
  });

  test('preview panel has accessible label', async ({ page }) => {
    const preview = page.locator('aside.preview-panel');
    await expect(preview).toHaveAttribute('aria-label', 'Containerfile preview');
  });
});
