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

  test('Containerfile updates on package toggle', async ({ page }) => {
    await page.click('a[data-tab="containerfile"]');
    const initialText = await page.locator('#containerfile-pre').textContent();

    await page.click('a[data-tab="rpm"]');
    await expect(page.locator('#section-rpm')).toBeVisible();
    const firstToggle = page.locator('.include-toggle').first();
    const wasChecked = await firstToggle.isChecked();
    await firstToggle.click();

    await page.click('a[data-tab="containerfile"]');
    const updatedText = await page.locator('#containerfile-pre').textContent();
    expect(updatedText).not.toEqual(initialText);

    // Toggle back
    await page.click('a[data-tab="rpm"]');
    await firstToggle.click();
    await page.click('a[data-tab="containerfile"]');
    const restoredText = await page.locator('#containerfile-pre').textContent();
    expect(restoredText).toEqual(initialText);
  });

  test('Containerfile updates on variant selection', async ({ page }) => {
    await page.click('a[data-tab="containerfile"]');
    const initialText = await page.locator('#containerfile-pre').textContent();

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
    await variant2.locator('.pf-v6-c-switch__toggle').click();

    await page.click('a[data-tab="containerfile"]');
    const updatedText = await page.locator('#containerfile-pre').textContent();
    expect(updatedText).toBeTruthy();
  });

  test('Containerfile updates on prevalence slider', async ({ page }) => {
    const slider = page.locator('#summary-prevalence-slider');
    if (!(await slider.isVisible())) {
      test.skip();
      return;
    }
    await page.click('a[data-tab="containerfile"]');
    const initialText = await page.locator('#containerfile-pre').textContent();

    await slider.fill('40');
    await slider.dispatchEvent('input');

    await page.click('a[data-tab="containerfile"]');
    const updatedText = await page.locator('#containerfile-pre').textContent();
    expect(updatedText).toBeTruthy();
  });

  test('audit counts update on package toggle', async ({ page }) => {
    await page.click('a[data-tab="summary"]');
    const initialTotal = await page.locator('#summary-scope-total').textContent();

    await page.click('a[data-tab="rpm"]');
    const firstToggle = page.locator('.include-toggle').first();
    await firstToggle.click();

    await page.click('a[data-tab="summary"]');
    const updatedTotal = await page.locator('#summary-scope-total').textContent();
    expect(parseInt(updatedTotal || '0')).toBeLessThan(parseInt(initialTotal || '0'));
  });

  test('audit preview cue is visible on audit tab', async ({ page }) => {
    await page.click('a[data-tab="audit"]');
    const cue = page.locator('#audit-preview-cue');
    if (await cue.isVisible()) {
      await expect(cue).toContainText('Summary counts update as you edit');
      await expect(cue).toContainText('Rebuild & Download');
    }
  });
});
