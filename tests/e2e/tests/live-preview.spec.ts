import { test, expect } from '@playwright/test';
import { FLEET_URL } from './helpers';

test.describe('Live Containerfile Preview', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto(FLEET_URL);
    await page.locator('.helper-active').waitFor({ state: 'attached', timeout: 10_000 });
  });

  test('no Copy button on Containerfile tab', async ({ page }) => {
    await page.click('a[data-tab="containerfile"]');
    await expect(page.locator('#section-containerfile')).toBeVisible();
    await expect(page.locator('#btn-copy-cf')).not.toBeAttached();
  });

  test('preview helper line is visible on Containerfile tab', async ({ page }) => {
    await page.click('a[data-tab="containerfile"]');
    await expect(page.locator('#section-containerfile')).toBeVisible();
    const helper = page.locator('#containerfile-preview-cue');
    await expect(helper).toBeVisible();
    await expect(helper).toContainText('Live preview');
    await expect(helper).toContainText('Rebuild & Download');
  });
});
