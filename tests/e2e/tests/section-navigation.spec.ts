import { test, expect } from '@playwright/test';
import { FLEET_URL } from './helpers';

test.describe('Section Navigation', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto(FLEET_URL);
  });

  test('priority list row click navigates to correct section', async ({ page }) => {
    // The summary page should be visible by default
    await expect(page.locator('#section-summary')).toBeVisible();

    // Click the first priority row — it should navigate to its target section
    const firstRow = page.locator('.summary-priority-row').first();
    await expect(firstRow).toBeVisible();
    const targetTab = await firstRow.getAttribute('data-nav-tab');
    expect(targetTab).toBeTruthy();

    await firstRow.click();

    // The target section should become visible
    const targetSection = page.locator(`#section-${targetTab}`);
    await expect(targetSection).toBeVisible();

    // The summary section should no longer be visible
    await expect(page.locator('#section-summary')).not.toBeVisible();

    // The sidebar nav link for this tab should be active
    const navLink = page.locator(`.pf-v6-c-nav__link[data-tab="${targetTab}"]`);
    await expect(navLink).toHaveClass(/pf-m-current/);
  });

  test('sidebar nav links navigate to sections', async ({ page }) => {
    // Navigate to packages via sidebar
    const packagesLink = page.locator('.pf-v6-c-nav__link[data-tab="packages"]');
    await packagesLink.click();
    await expect(page.locator('#section-packages')).toBeVisible();
    await expect(page.locator('#section-summary')).not.toBeVisible();

    // Navigate to config via sidebar
    const configLink = page.locator('.pf-v6-c-nav__link[data-tab="config"]');
    await configLink.click();
    await expect(page.locator('#section-config')).toBeVisible();
    await expect(page.locator('#section-packages')).not.toBeVisible();

    // Navigate to services via sidebar
    const servicesLink = page.locator('.pf-v6-c-nav__link[data-tab="services"]');
    await servicesLink.click();
    await expect(page.locator('#section-services')).toBeVisible();
    await expect(page.locator('#section-config')).not.toBeVisible();

    // Navigate back to summary
    const summaryLink = page.locator('.pf-v6-c-nav__link[data-tab="summary"]');
    await summaryLink.click();
    await expect(page.locator('#section-summary')).toBeVisible();
    await expect(page.locator('#section-services')).not.toBeVisible();
  });

  test('sidebar nav shows active state with pf-m-current', async ({ page }) => {
    // On initial load, summary link should be active
    const summaryLink = page.locator('.pf-v6-c-nav__link[data-tab="summary"]');
    await expect(summaryLink).toHaveClass(/pf-m-current/);

    // All other nav links should NOT be active
    const otherLinks = page.locator('.pf-v6-c-nav__link[data-tab]:not([data-tab="summary"])');
    const count = await otherLinks.count();
    for (let i = 0; i < count; i++) {
      await expect(otherLinks.nth(i)).not.toHaveClass(/pf-m-current/);
    }

    // Navigate to packages — it should become active, summary should not
    const packagesLink = page.locator('.pf-v6-c-nav__link[data-tab="packages"]');
    await packagesLink.click();
    await expect(packagesLink).toHaveClass(/pf-m-current/);
    await expect(summaryLink).not.toHaveClass(/pf-m-current/);

    // Navigate to containerfile — verify active state transfers
    const cfLink = page.locator('.pf-v6-c-nav__link[data-tab="containerfile"]');
    await cfLink.click();
    await expect(cfLink).toHaveClass(/pf-m-current/);
    await expect(packagesLink).not.toHaveClass(/pf-m-current/);
  });
});
