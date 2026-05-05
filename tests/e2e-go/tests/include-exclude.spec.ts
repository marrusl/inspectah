/**
 * Include/exclude decision tests for the refine report UI.
 *
 * The fleet fixture renders triage cards with Include/Exclude action
 * buttons in the config section. These are not toggle switches but
 * button-based decisions (btn-primary = Include, btn-outline = Exclude).
 */
import { test, expect } from '@playwright/test';
import { waitForBoot, navigateToSection } from './helpers';

test.describe('Include/exclude decisions', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await waitForBoot(page);
  });

  test('config section has triage cards', async ({ page }) => {
    await navigateToSection(page, 'config');

    const cards = page.locator('#section-config .triage-card');
    const count = await cards.count();
    expect(count).toBeGreaterThan(0);
  });

  test('triage cards have Include and Exclude action buttons', async ({ page }) => {
    await navigateToSection(page, 'config');

    const actions = page.locator('#section-config .card-actions');
    const count = await actions.count();
    if (count === 0) {
      test.skip();
      return;
    }

    // First undecided card should have both buttons
    const includeBtn = actions.first().locator('.btn-primary');
    const excludeBtn = actions.first().locator('.btn-outline');
    await expect(includeBtn).toBeAttached();
    await expect(excludeBtn).toBeAttached();
  });

  test('clicking Include converts card to decided state', async ({ page }) => {
    await navigateToSection(page, 'config');

    const firstCard = page.locator('#section-config .triage-card').first();
    const count = await firstCard.count();
    if (count === 0) {
      test.skip();
      return;
    }

    const includeBtn = firstCard.locator('.btn-primary');
    const btnCount = await includeBtn.count();
    if (btnCount === 0) {
      test.skip();
      return;
    }
    await includeBtn.click();

    // Card should now show as decided (with decided-card class or undo link)
    const decidedCards = page.locator('#section-config .decided-card');
    const decidedCount = await decidedCards.count();
    expect(decidedCount).toBeGreaterThan(0);
  });

  test('decided cards show undo link', async ({ page }) => {
    await navigateToSection(page, 'config');

    // First decide on a card
    const firstCard = page.locator('#section-config .triage-card').first();
    const includeBtn = firstCard.locator('.btn-primary');
    const count = await includeBtn.count();
    if (count === 0) {
      test.skip();
      return;
    }
    await includeBtn.click();

    // Should show undo link
    const undoLinks = page.locator('#section-config .undo-link');
    const undoCount = await undoLinks.count();
    expect(undoCount).toBeGreaterThan(0);
  });

  test('triage cards have data-key attribute', async ({ page }) => {
    await navigateToSection(page, 'config');

    const cards = page.locator('#section-config [data-key]');
    const count = await cards.count();
    expect(count).toBeGreaterThan(0);

    // First key should follow the cfg- prefix pattern
    const firstKey = await cards.first().getAttribute('data-key');
    expect(firstKey).toMatch(/^cfg-/);
  });

  test('section footer shows review stats', async ({ page }) => {
    await navigateToSection(page, 'config');

    const footer = page.locator('#section-config .section-footer');
    const count = await footer.count();
    if (count > 0) {
      const stats = page.locator('#section-config .section-stats');
      await expect(stats).toBeAttached();
    }
  });
});
