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

  test('Discard button shows confirmation dialog', async ({ page }) => {
    await page.click('a[data-tab="rpm"]');
    await expect(page.locator('#section-rpm')).toBeVisible();
    const firstToggle = page.locator('.include-toggle').first();
    await firstToggle.click();

    const discardBtn = page.locator('#btn-reset');
    await expect(discardBtn).toBeEnabled();
    await discardBtn.click();

    const dialog = page.locator('#discard-confirm-dialog');
    await expect(dialog).toBeVisible();
    await expect(dialog).toContainText('Discard all edits?');
  });

  test('Discard confirmed restores original Containerfile', async ({ page }) => {
    await page.click('a[data-tab="containerfile"]');
    const originalText = await page.locator('#containerfile-pre').textContent();

    await page.click('a[data-tab="rpm"]');
    const firstToggle = page.locator('.include-toggle').first();
    await firstToggle.click();

    await page.click('a[data-tab="containerfile"]');
    const changedText = await page.locator('#containerfile-pre').textContent();
    expect(changedText).not.toEqual(originalText);

    const discardBtn = page.locator('#btn-reset');
    await discardBtn.click();
    await page.locator('#discard-confirm-yes').click();

    const restoredText = await page.locator('#containerfile-pre').textContent();
    expect(restoredText).toEqual(originalText);
  });

  test('Rebuild & Download button exists, no separate tarball button', async ({ page }) => {
    await expect(page.locator('#btn-re-render')).toBeAttached();
    await expect(page.locator('#btn-tarball')).not.toBeAttached();
  });
});
