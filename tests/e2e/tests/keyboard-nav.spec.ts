import { test, expect } from '@playwright/test';
import { FLEET_URL } from './helpers';

test.describe('Keyboard Navigation', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto(FLEET_URL);
  });

  test('prevalence badge is focusable and Enter navigates to summary', async ({ page }) => {
    // Navigate to packages section so prevalence badges are visible
    await page.click('a[data-tab="packages"]');
    await expect(page.locator('#section-packages')).toBeVisible();

    // Assert prevalence badge is visible (scope to packages section
    // since hidden sections also contain prevalence badges)
    const badge = page.locator('#section-packages .prevalence-badge');
    await expect(badge).toBeVisible();

    // Verify the badge has tabindex="0" and role="button"
    await expect(badge).toHaveAttribute('tabindex', '0');
    await expect(badge).toHaveAttribute('role', 'button');

    // Focus the badge and press Enter
    await badge.focus();
    await page.keyboard.press('Enter');

    // Verify navigation to summary section
    const summarySection = page.locator('#section-summary');
    await expect(summarySection).toBeVisible();

    // Packages section should no longer be visible
    await expect(page.locator('#section-packages')).not.toBeVisible();
  });

  test('priority rows are focusable and Enter navigates to target section', async ({ page }) => {
    // Summary section should be visible by default
    await expect(page.locator('#section-summary')).toBeVisible();

    // Focus the first priority row
    const firstRow = page.locator('.summary-priority-row').first();
    await expect(firstRow).toBeVisible();

    // Verify the row has tabindex="0" and role="button"
    await expect(firstRow).toHaveAttribute('tabindex', '0');
    await expect(firstRow).toHaveAttribute('role', 'button');

    // Get the target tab before pressing Enter
    const targetTab = await firstRow.getAttribute('data-nav-tab');
    expect(targetTab).toBeTruthy();

    // Focus the row and press Enter
    await firstRow.focus();
    await page.keyboard.press('Enter');

    // The target section should become visible
    const targetSection = page.locator(`#section-${targetTab}`);
    await expect(targetSection).toBeVisible();

    // Summary section should no longer be visible
    await expect(page.locator('#section-summary')).not.toBeVisible();
  });
});
