import { test, expect } from '@playwright/test';
import { FLEET_URL } from './helpers';

test.describe('Rebuild & Download Cycle', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto(FLEET_URL);
    await page.locator('.helper-active').waitFor({ state: 'attached', timeout: 10_000 });
  });

  test('Rebuild & Download triggers full pipeline and downloads tarball', async ({ page }) => {
    await page.click('a[data-tab="config"]');
    await expect(page.locator('#section-config')).toBeVisible();

    const appConfGroup = page.locator('tr.fleet-variant-group', {
      has: page.locator('code', { hasText: '/etc/app.conf' }),
    });
    await appConfGroup.locator('.fleet-variant-toggle').click();
    const childrenRow = page.locator('tr.fleet-variant-children').first();
    await expect(childrenRow).toBeVisible();

    const variant2 = page.locator(
      'tr[data-variant-group="/etc/app.conf"][data-snap-index="1"]'
    );
    const toggleSpan = variant2.locator('.pf-v6-c-switch__toggle');
    await toggleSpan.click();

    const rebuildBtn = page.locator('#btn-re-render');
    await expect(rebuildBtn).toBeEnabled();

    const downloadPromise = page.waitForEvent('download', { timeout: 30_000 });
    await rebuildBtn.click();
    const download = await downloadPromise;
    expect(download.suggestedFilename()).toContain('.tar.gz');

    await expect(page.locator('#section-config')).toBeVisible();
    await expect(rebuildBtn).toBeDisabled();
  });

  test('error on corrupted rebuild: route interception returns 500', async ({ page }) => {
    await page.click('a[data-tab="config"]');
    await expect(page.locator('#section-config')).toBeVisible();

    const appConfGroup = page.locator('tr.fleet-variant-group', {
      has: page.locator('code', { hasText: '/etc/app.conf' }),
    });
    await appConfGroup.locator('.fleet-variant-toggle').click();
    const childrenRow = page.locator('tr.fleet-variant-children').first();
    await expect(childrenRow).toBeVisible();

    const variant2 = page.locator(
      'tr[data-variant-group="/etc/app.conf"][data-snap-index="1"]'
    );
    const toggleSpan = variant2.locator('.pf-v6-c-switch__toggle');
    await toggleSpan.click();

    const rebuildBtn = page.locator('#btn-re-render');
    await expect(rebuildBtn).toBeEnabled();

    await page.route('**/api/re-render', (route) =>
      route.fulfill({ status: 500, body: 'Internal Server Error' })
    );

    await rebuildBtn.click();

    const toast = page.locator('#toast-message');
    await expect(toast).toContainText(/Rebuild failed/, { timeout: 10_000 });
    await expect(rebuildBtn).toBeEnabled({ timeout: 5_000 });
  });
});
