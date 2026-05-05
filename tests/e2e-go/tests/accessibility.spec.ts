/**
 * Accessibility tests for the refine report UI.
 * Validates ARIA attributes, skip links, keyboard navigation, and
 * live region announcements.
 */
import { test, expect } from '@playwright/test';
import { waitForBoot, navigateToSection } from './helpers';

test.describe('ARIA landmarks and attributes', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await waitForBoot(page);
  });

  test('skip-to-content link exists and targets main-content', async ({ page }) => {
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

  test('toggle buttons use role="switch" with aria-checked', async ({ page }) => {
    await navigateToSection(page, 'config');

    // Scope to the active section to avoid matching hidden sections
    const toggles = page.locator('#section-config .item-toggle');
    const count = await toggles.count();
    expect(count).toBeGreaterThan(0);

    // Every toggle should have a valid aria-checked attribute
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

test.describe('Keyboard navigation', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await waitForBoot(page);
  });

  test('skip link becomes visible on focus and targets main-content', async ({ page }) => {
    // The skip link is positioned off-screen (left: -9999px) until focused.
    const skipLink = page.locator('.pf-v6-c-skip-to-content__link');

    // Before focus: should be off-screen
    const initialLeft = await skipLink.evaluate((el) => getComputedStyle(el).left);
    expect(parseInt(initialLeft)).toBeLessThan(0);

    // Focus the skip link
    await skipLink.focus();

    // On focus, the skip link should move into view (left: 0 via CSS :focus)
    const focusedLeft = await skipLink.evaluate((el) => getComputedStyle(el).left);
    expect(focusedLeft).toBe('0px');

    // The link's href targets #main-content
    await expect(skipLink).toHaveAttribute('href', '#main-content');
  });

  test('sidebar navigation updates aria-current on click', async ({ page }) => {
    await navigateToSection(page, 'packages');

    // The clicked link should have aria-current="page"
    const packagesLink = page.locator('[data-section="packages"]');
    await expect(packagesLink).toHaveAttribute('aria-current', 'page');

    // Other links should NOT have aria-current="page"
    const overviewLink = page.locator('[data-section="overview"]');
    const overviewCurrent = await overviewLink.getAttribute('aria-current');
    expect(overviewCurrent).not.toBe('page');
  });

  test('item-toggle activates with Enter key', async ({ page }) => {
    await navigateToSection(page, 'config');

    // Scope to config section for the toggle
    const toggle = page.locator('#section-config .item-toggle').first();
    // Scroll it into view and wait for it to be actionable
    await toggle.scrollIntoViewIfNeeded();
    await expect(toggle).toBeVisible();

    const initialState = await toggle.getAttribute('aria-checked');

    await toggle.focus();
    await page.keyboard.press('Enter');

    const newState = await toggle.getAttribute('aria-checked');
    expect(newState).not.toBe(initialState);
  });

  test('item-toggle activates with Space key', async ({ page }) => {
    await navigateToSection(page, 'config');

    const toggle = page.locator('#section-config .item-toggle').first();
    await toggle.scrollIntoViewIfNeeded();
    await expect(toggle).toBeVisible();

    const initialState = await toggle.getAttribute('aria-checked');

    await toggle.focus();
    await page.keyboard.press('Space');

    const newState = await toggle.getAttribute('aria-checked');
    expect(newState).not.toBe(initialState);
  });

  test('editor tab bar supports keyboard model', async ({ page }) => {
    await navigateToSection(page, 'editor');

    const tabs = page.getByRole('tab');
    const count = await tabs.count();
    expect(count).toBeGreaterThanOrEqual(1);

    // Focus the first tab
    await tabs.first().focus();
    await expect(tabs.first()).toBeFocused();

    // Active tab should have tabindex="0", others tabindex="-1"
    await expect(tabs.first()).toHaveAttribute('tabindex', '0');
    if (count >= 2) {
      await expect(tabs.nth(1)).toHaveAttribute('tabindex', '-1');
    }
  });
});

test.describe('Live region announcements', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await waitForBoot(page);
  });

  test('rebuild status announces outcome via role="status"', async ({ page }) => {
    // Toggle an item in config section
    await navigateToSection(page, 'config');
    const toggle = page.locator('#section-config .item-toggle').first();
    await toggle.scrollIntoViewIfNeeded();
    await toggle.click();

    // Trigger rebuild
    const rebuildBtn = page.locator('#rebuild-btn');
    const statusEl = page.locator('#rebuild-status');

    const renderPromise = page.waitForResponse(
      (resp) => resp.url().includes('/api/render'),
      { timeout: 15_000 }
    );

    await rebuildBtn.click();
    await renderPromise;

    // Wait for button to return to normal state
    await expect(rebuildBtn).toHaveText('Rebuild', { timeout: 10_000 });

    // Status should have been updated (success or failure message)
    // The role="status" attribute makes this a live region for screen readers
    await expect(statusEl).not.toHaveText('', { timeout: 5_000 });
  });

  test('autosave live region is initially empty', async ({ page }) => {
    const liveRegion = page.locator('#autosave-live');
    await expect(liveRegion).toHaveText('');
  });
});
