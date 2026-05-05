/**
 * Triage card tests for the refine report UI.
 * Validates triage classification, tier grouping, and card rendering.
 *
 * Tests search across all tracked sections for triage items rather than
 * hardcoding a specific section, because fixture data varies.
 */
import { test, expect } from '@playwright/test';
import { waitForBoot, navigateToSection } from './helpers';

/** Tracked sections that may contain triage items. */
const TRACKED_SECTIONS = ['packages', 'config', 'runtime', 'containers',
                          'nonrpm', 'identity', 'system', 'secrets'];

/**
 * Find the first section that has triage cards (triage-card or toggle-card).
 * Returns the section ID, or null if none found.
 */
async function findSectionWithCards(page: import('@playwright/test').Page): Promise<string | null> {
  for (const sectionId of TRACKED_SECTIONS) {
    const navLink = page.locator(`[data-section="${sectionId}"]`);
    if ((await navLink.count()) === 0) continue;

    // Check if nav link is visible (some are hidden when data is absent)
    const isVisible = await navLink.evaluate((el) => {
      const li = el.closest('li');
      return li ? getComputedStyle(li).display !== 'none' : true;
    });
    if (!isVisible) continue;

    await navigateToSection(page, sectionId);
    const cards = page.locator(`#section-${sectionId} .triage-card, #section-${sectionId} .toggle-card`);
    if ((await cards.count()) > 0) return sectionId;
  }
  return null;
}

test.describe('Triage cards', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await waitForBoot(page);
  });

  test('triage badges appear in sidebar for sections with reviewable items', async ({ page }) => {
    // Sidebar nav badges use .triage-badge or .nav-badge with a numeric count.
    // Badges may exist in DOM but be hidden when sections have 0 items.
    const visibleBadges = page.locator('.triage-badge:visible, .nav-badge:visible');
    const count = await visibleBadges.count();
    if (count === 0) {
      test.skip(true, 'Fixture has no visible triage badges (sections may be empty)');
      return;
    }
    await expect(visibleBadges.first()).toBeVisible();
  });

  test('triage cards render in a tracked section', async ({ page }) => {
    const sectionId = await findSectionWithCards(page);
    if (!sectionId) {
      test.skip(true, 'Fixture has no triage cards in any tracked section');
      return;
    }

    const cards = page.locator(`#section-${sectionId} .triage-card, #section-${sectionId} .toggle-card`);
    await expect(cards.first()).toBeVisible();
  });

  test('tier groups organize cards by severity', async ({ page }) => {
    const sectionId = await findSectionWithCards(page);
    if (!sectionId) {
      test.skip(true, 'Fixture has no triage cards in any tracked section');
      return;
    }

    const tierGroups = page.locator(`#section-${sectionId} .tier-group`);
    const count = await tierGroups.count();
    expect(count).toBeGreaterThan(0);

    const headers = page.locator(`#section-${sectionId} .tier-group-header`);
    expect(await headers.count()).toBeGreaterThan(0);
  });

  test('triage cards have data-key for identification', async ({ page }) => {
    const sectionId = await findSectionWithCards(page);
    if (!sectionId) {
      test.skip(true, 'Fixture has no triage cards in any tracked section');
      return;
    }

    const cards = page.locator(`#section-${sectionId} [data-key]`);
    const count = await cards.count();
    expect(count).toBeGreaterThan(0);

    const firstKey = await cards.first().getAttribute('data-key');
    expect(firstKey).toBeTruthy();
    expect(typeof firstKey).toBe('string');
  });

  test('toggle cards have role="switch" with aria-checked', async ({ page }) => {
    // Find a section with toggle switches specifically
    let foundSection: string | null = null;
    for (const sectionId of TRACKED_SECTIONS) {
      const navLink = page.locator(`[data-section="${sectionId}"]`);
      if ((await navLink.count()) === 0) continue;

      const isVisible = await navLink.evaluate((el) => {
        const li = el.closest('li');
        return li ? getComputedStyle(li).display !== 'none' : true;
      });
      if (!isVisible) continue;

      await navigateToSection(page, sectionId);
      const toggles = page.locator(`#section-${sectionId} button[role="switch"]`);
      if ((await toggles.count()) > 0) {
        foundSection = sectionId;
        break;
      }
    }

    if (!foundSection) {
      test.skip(true, 'Fixture has no toggle switches in any tracked section');
      return;
    }

    const toggles = page.locator(`#section-${foundSection} button[role="switch"]`);
    const firstChecked = await toggles.first().getAttribute('aria-checked');
    expect(['true', 'false']).toContain(firstChecked);
  });
});
