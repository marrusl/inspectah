/**
 * Shared helpers for inspectah Go port e2e tests.
 */
import { Page, expect } from '@playwright/test';

/** Wait for the report SPA to fully boot (sidebar + sections rendered). */
export async function waitForBoot(page: Page): Promise<void> {
  // The boot() function renders the sidebar and navigates to 'overview'.
  // Wait for the sidebar nav to have links and the overview heading to appear.
  await page.waitForSelector('#sidebar .pf-v6-c-nav__link', { timeout: 10_000 });
  await page.waitForSelector('#heading-overview', { timeout: 10_000 });
}

/** Navigate to a section via sidebar link click. */
export async function navigateToSection(page: Page, sectionId: string): Promise<void> {
  await page.click(`[data-section="${sectionId}"]`);
  await page.waitForSelector(`#heading-${sectionId}`, { timeout: 5_000 });
}

/**
 * Get all sidebar section IDs currently rendered.
 * Returns the data-section attribute values.
 */
export async function getSidebarSections(page: Page): Promise<string[]> {
  return page.$$eval('[data-section]', (els) =>
    els.map((el) => el.getAttribute('data-section') || '')
  );
}

/** Check if the page is in refine mode (rebuild bar visible). */
export async function isRefineMode(page: Page): Promise<boolean> {
  const bar = page.locator('#rebuild-bar');
  // The rebuild bar exists in the DOM always but may be hidden in static mode
  const display = await bar.evaluate((el) => getComputedStyle(el).display);
  return display !== 'none';
}

/** Get the current theme (dark or light). */
export async function getTheme(page: Page): Promise<'dark' | 'light'> {
  const isDark = await page.evaluate(() =>
    document.body.classList.contains('pf-v6-theme-dark')
  );
  return isDark ? 'dark' : 'light';
}

/**
 * Find the first interactive toggle in a section.
 * Tries accordion-toggle first (grouped items), then item-toggle (flat items).
 * Returns null if no toggles exist.
 */
export async function findToggleInSection(
  page: Page,
  sectionId: string
): Promise<ReturnType<Page['locator']> | null> {
  const accordion = page.locator(`#section-${sectionId} .accordion-toggle`).first();
  if ((await accordion.count()) > 0) return accordion;

  const item = page.locator(`#section-${sectionId} .item-toggle`).first();
  if ((await item.count()) > 0) return item;

  return null;
}
