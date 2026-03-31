import { test, expect } from '@playwright/test';
import { FLEET_URL } from './helpers';

test.describe('Re-render Cycle', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto(FLEET_URL);
    // Wait for the helper script to detect the live server and enable toggles
    await page.locator('.helper-active').waitFor({ state: 'attached', timeout: 10_000 });
  });

  test('full re-render cycle: toggle variant, re-render, verify dashboard loads', async ({ page }) => {
    // Navigate to config section
    await page.click('a[data-tab="config"]');
    await expect(page.locator('#section-config')).toBeVisible();

    // Expand the app.conf variant group and uncheck variant 2
    const appConfGroup = page.locator('tr.fleet-variant-group', {
      has: page.locator('code', { hasText: '/etc/app.conf' }),
    });
    await appConfGroup.locator('.fleet-variant-toggle').click();
    const childrenRow = page.locator('tr.fleet-variant-children').first();
    await expect(childrenRow).toBeVisible();

    // Uncheck variant 2 (snap-index="1") by clicking PF switch toggle
    const variant2 = page.locator(
      'tr[data-variant-group="/etc/app.conf"][data-snap-index="1"]'
    );
    const toggleSpan = variant2.locator('.pf-v6-c-switch__toggle');
    await toggleSpan.click();

    // Verify dirty state: Re-render button is enabled
    const rerender = page.locator('#btn-re-render');
    await expect(rerender).toBeEnabled();

    // Click Re-render and wait for page reload
    await Promise.all([
      page.waitForNavigation({ waitUntil: 'networkidle' }),
      rerender.click(),
    ]);

    // Wait for helper to reactivate after re-render
    await page.locator('.helper-active').waitFor({ state: 'attached', timeout: 10_000 });

    // Navigate to summary tab and verify the dashboard loads after re-render
    await page.click('a[data-tab="summary"]');
    const dashboard = page.locator('.summary-dashboard');
    await expect(dashboard).toBeVisible();

    // Verify variant selection persisted: navigate to config, expand, check state
    await page.click('a[data-tab="config"]');
    await expect(page.locator('#section-config')).toBeVisible();

    const appConfGroupAfter = page.locator('tr.fleet-variant-group', {
      has: page.locator('code', { hasText: '/etc/app.conf' }),
    });
    await appConfGroupAfter.locator('.fleet-variant-toggle').click();
    const childrenRowAfter = page.locator('tr.fleet-variant-children').first();
    await expect(childrenRowAfter).toBeVisible();

    const variant2After = page.locator(
      'tr[data-variant-group="/etc/app.conf"][data-snap-index="1"]'
    );
    const checkboxAfter = variant2After.locator('.include-toggle');
    await expect(checkboxAfter).not.toBeChecked();
  });

  test('error on corrupted re-render: route interception returns 500', async ({ page }) => {
    // Navigate to config section
    await page.click('a[data-tab="config"]');
    await expect(page.locator('#section-config')).toBeVisible();

    // Toggle a config variant to create dirty state
    const appConfGroup = page.locator('tr.fleet-variant-group', {
      has: page.locator('code', { hasText: '/etc/app.conf' }),
    });
    await appConfGroup.locator('.fleet-variant-toggle').click();
    const childrenRow = page.locator('tr.fleet-variant-children').first();
    await expect(childrenRow).toBeVisible();

    // Uncheck variant 2 to enter dirty state
    const variant2 = page.locator(
      'tr[data-variant-group="/etc/app.conf"][data-snap-index="1"]'
    );
    const toggleSpan = variant2.locator('.pf-v6-c-switch__toggle');
    await toggleSpan.click();

    // Verify Re-render is enabled (dirty state exists)
    const rerender = page.locator('#btn-re-render');
    await expect(rerender).toBeEnabled();

    // Intercept the re-render API call and return a 500 error
    await page.route('**/api/re-render', (route) =>
      route.fulfill({ status: 500, body: 'Internal Server Error' })
    );

    // Click Re-render
    await rerender.click();

    // The editor re-render error handler calls showEditorError() which
    // sets the #editor-error-message text content. The banner element
    // has pf-m-danger class. Verify the error message was populated.
    const errorMsg = page.locator('#editor-error-message');
    await expect(errorMsg).toHaveText(/HTTP 500/, { timeout: 10_000 });

    // Verify the error banner has pf-m-danger class (it always does)
    const errorBanner = page.locator('#editor-error-banner');
    await expect(errorBanner).toHaveClass(/pf-m-danger/);

    // Verify the Re-render button is re-enabled after error
    await expect(rerender).toBeEnabled({ timeout: 5_000 });
  });
});
