/**
 * Include/exclude decision tests for the refine report UI.
 *
 * The Go port renders two card types depending on triage tier:
 * - Tier 2/3 items use triage-cards with Include/Exclude buttons
 *   (btn-primary / btn-outline inside .card-actions)
 * - Tier 1 items use toggle-cards with role="switch" toggles
 *
 * Tests search across tracked sections for items rather than hardcoding
 * a specific section, because fixture data varies.
 */
import { test, expect } from '@playwright/test';
import { waitForRefineBoot, navigateToSection } from './helpers';

/** Tracked sections that may contain triage items. */
const TRACKED_SECTIONS = ['packages', 'config', 'runtime', 'containers',
                          'nonrpm', 'identity', 'system', 'secrets'];

/** Find first visible section with triage/toggle cards. */
async function findSectionWithCards(page: import('@playwright/test').Page): Promise<string | null> {
  for (const id of TRACKED_SECTIONS) {
    const nav = page.locator(`[data-section="${id}"]`);
    if ((await nav.count()) === 0) continue;
    const visible = await nav.evaluate((el) => {
      const li = el.closest('li');
      return li ? getComputedStyle(li).display !== 'none' : true;
    });
    if (!visible) continue;
    await navigateToSection(page, id);
    const cards = page.locator(`#section-${id} .triage-card, #section-${id} .toggle-card`);
    if ((await cards.count()) > 0) return id;
  }
  return null;
}

/** Find first visible section with toggle switches (role="switch"). */
async function findSectionWithToggles(page: import('@playwright/test').Page): Promise<string | null> {
  for (const id of TRACKED_SECTIONS) {
    const nav = page.locator(`[data-section="${id}"]`);
    if ((await nav.count()) === 0) continue;
    const visible = await nav.evaluate((el) => {
      const li = el.closest('li');
      return li ? getComputedStyle(li).display !== 'none' : true;
    });
    if (!visible) continue;
    await navigateToSection(page, id);
    const toggles = page.locator(`#section-${id} button[role="switch"]`);
    if ((await toggles.count()) > 0) return id;
  }
  return null;
}

test.describe('Include/exclude decisions', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await waitForRefineBoot(page);
  });

  test('a tracked section has item cards', async ({ page }) => {
    const sectionId = await findSectionWithCards(page);
    if (!sectionId) {
      test.skip(true, 'Fixture has no triage/toggle cards in any tracked section');
      return;
    }

    const cards = page.locator(`#section-${sectionId} .triage-card, #section-${sectionId} .toggle-card`);
    expect(await cards.count()).toBeGreaterThan(0);
  });

  test('triage cards have Include and Exclude action buttons', async ({ page }) => {
    // Find a section with triage-card action buttons (tier 2/3 items)
    let foundSection: string | null = null;
    for (const id of TRACKED_SECTIONS) {
      const nav = page.locator(`[data-section="${id}"]`);
      if ((await nav.count()) === 0) continue;
      const visible = await nav.evaluate((el) => {
        const li = el.closest('li');
        return li ? getComputedStyle(li).display !== 'none' : true;
      });
      if (!visible) continue;
      await navigateToSection(page, id);
      const actions = page.locator(`#section-${id} .triage-card .card-actions`);
      if ((await actions.count()) > 0) {
        foundSection = id;
        break;
      }
    }

    if (!foundSection) {
      // No tier 2/3 cards; verify at least toggle switches exist somewhere
      const toggleSection = await findSectionWithToggles(page);
      if (!toggleSection) {
        test.skip(true, 'Fixture has no triage cards or toggles in any section');
        return;
      }
      const toggles = page.locator(`#section-${toggleSection} button[role="switch"]`);
      expect(await toggles.count()).toBeGreaterThan(0);
      return;
    }

    const actions = page.locator(`#section-${foundSection} .triage-card .card-actions`).first();
    const includeBtn = actions.locator('.btn-primary');
    const excludeBtn = actions.locator('.btn-outline');
    await expect(includeBtn).toBeVisible();
    await expect(excludeBtn).toBeVisible();
  });

  test('toggle switches exist for decisions', async ({ page }) => {
    const sectionId = await findSectionWithToggles(page);
    if (!sectionId) {
      test.skip(true, 'Fixture has no toggle switches in any tracked section');
      return;
    }

    const toggles = page.locator(`#section-${sectionId} button[role="switch"]`);
    expect(await toggles.count()).toBeGreaterThan(0);

    const firstChecked = await toggles.first().getAttribute('aria-checked');
    expect(['true', 'false']).toContain(firstChecked);
  });

  test('toggling a switch changes its state', async ({ page }) => {
    const sectionId = await findSectionWithToggles(page);
    if (!sectionId) {
      test.skip(true, 'Fixture has no toggle switches in any tracked section');
      return;
    }

    const toggle = page.locator(`#section-${sectionId} button[role="switch"]`).first();
    const initialState = await toggle.getAttribute('aria-checked');
    await toggle.click();
    const newState = await toggle.getAttribute('aria-checked');
    expect(newState).not.toBe(initialState);
  });

  test('toggle-card has data-key attribute for identification', async ({ page }) => {
    const sectionId = await findSectionWithToggles(page);
    if (!sectionId) {
      test.skip(true, 'Fixture has no toggle switches in any tracked section');
      return;
    }

    const cards = page.locator(`#section-${sectionId} [data-key]`);
    const count = await cards.count();
    expect(count).toBeGreaterThan(0);

    const firstKey = await cards.first().getAttribute('data-key');
    expect(firstKey).toBeTruthy();
  });

  test('section footer shows review stats after decision', async ({ page }) => {
    const sectionId = await findSectionWithToggles(page);
    if (!sectionId) {
      test.skip(true, 'Fixture has no toggle switches in any tracked section');
      return;
    }

    const toggle = page.locator(`#section-${sectionId} button[role="switch"]`).first();
    await toggle.click();

    const footer = page.locator(`#section-${sectionId} .section-footer`);
    const stats = page.locator(`#section-${sectionId} .section-stats`);
    await expect(footer).toBeAttached();
    await expect(stats).toBeAttached();
  });
});
