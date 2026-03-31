import { test, expect } from '@playwright/test';
import { FLEET_URL } from './helpers';

test.describe('Include/Exclude Toggles', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto(FLEET_URL);
    // Wait for the helper script to detect the live server and enable toggles.
    // The helper calls /api/health and on success adds class 'helper-active'
    // to the toolbar and sets include-toggle-wrap elements to display: inline-grid.
    await page.locator('.helper-active').waitFor({ state: 'attached', timeout: 10_000 });
  });

  test('toggling a package checkbox activates Re-render button', async ({ page }) => {
    // Navigate to the packages section
    await page.click('a[data-tab="packages"]');
    await expect(page.locator('#section-packages')).toBeVisible();

    // Find the first visible include toggle (PF Switch) in the packages section
    const firstToggleWrap = page.locator('#section-packages .include-toggle-wrap').first();
    await expect(firstToggleWrap).toBeVisible();

    // Get the checkbox state before toggling
    const checkbox = firstToggleWrap.locator('.include-toggle');
    const wasChecked = await checkbox.isChecked();

    // Click the PF Switch toggle span (not the raw checkbox)
    const toggleSpan = firstToggleWrap.locator('.pf-v6-c-switch__toggle');
    await toggleSpan.click();

    // Verify the checkbox state changed
    if (wasChecked) {
      await expect(checkbox).not.toBeChecked();
    } else {
      await expect(checkbox).toBeChecked();
    }

    // The Re-render button should now be enabled (dirty state)
    const rerender = page.locator('#btn-re-render');
    await expect(rerender).toBeEnabled();
  });

  test('Re-render completes full cycle after toggle change', async ({ page }) => {
    // Navigate to the config section — config variant toggles are proven to
    // persist through re-render (see variant-selection.spec.ts).
    await page.click('a[data-tab="config"]');
    await expect(page.locator('#section-config')).toBeVisible();

    // Expand the app.conf variant group and uncheck variant 2
    const appConfGroup = page.locator('tr.fleet-variant-group', {
      has: page.locator('code', { hasText: '/etc/app.conf' }),
    });
    await appConfGroup.locator('.fleet-variant-toggle').click();
    const childrenRow = page.locator('tr.fleet-variant-children').first();
    await expect(childrenRow).toBeVisible();

    // Uncheck variant 2 (snap-index="1")
    const variant2 = page.locator(
      'tr[data-variant-group="/etc/app.conf"][data-snap-index="1"]'
    );
    const toggleSpan = variant2.locator('.pf-v6-c-switch__toggle');
    await toggleSpan.click();

    // Verify the checkbox unchecked and Re-render is enabled
    const checkbox = variant2.locator('.include-toggle');
    await expect(checkbox).not.toBeChecked();
    await expect(variant2).toHaveClass(/excluded/);

    const rerender = page.locator('#btn-re-render');
    await expect(rerender).toBeEnabled();

    // Click Re-render and wait for page reload
    await Promise.all([
      page.waitForNavigation({ waitUntil: 'networkidle' }),
      rerender.click(),
    ]);

    // Wait for helper to reactivate after re-render
    await page.locator('.helper-active').waitFor({ state: 'attached', timeout: 10_000 });

    // Navigate back to config tab and verify the variant is still excluded
    await page.click('a[data-tab="config"]');
    await expect(page.locator('#section-config')).toBeVisible();

    // Expand variant group again (re-render resets expansion)
    const appConfGroupAfter = page.locator('tr.fleet-variant-group', {
      has: page.locator('code', { hasText: '/etc/app.conf' }),
    });
    await appConfGroupAfter.locator('.fleet-variant-toggle').click();
    const childrenRowAfter = page.locator('tr.fleet-variant-children').first();
    await expect(childrenRowAfter).toBeVisible();

    // Verify variant 2 is still unchecked after re-render
    const variant2After = page.locator(
      'tr[data-variant-group="/etc/app.conf"][data-snap-index="1"]'
    );
    const checkboxAfter = variant2After.locator('.include-toggle');
    await expect(checkboxAfter).not.toBeChecked();
    await expect(variant2After).toHaveClass(/excluded/);
  });
});
