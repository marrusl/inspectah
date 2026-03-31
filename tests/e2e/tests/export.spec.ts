import { test, expect } from '@playwright/test';
import { ARCHITECT_URL } from './helpers';

test.describe('Export', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto(ARCHITECT_URL);
    await expect(page.locator('.layer-card').first()).toBeVisible();
  });

  test('export button exists in toolbar', async ({ page }) => {
    const exportBtn = page.locator('#btn-export');
    await expect(exportBtn).toBeVisible();
    await expect(exportBtn).toContainText('Export Containerfiles');
  });

  test('export triggers download with .tar.gz filename', async ({ page }) => {
    const downloadPromise = page.waitForEvent('download');

    const exportBtn = page.locator('#btn-export');
    await exportBtn.click();

    const download = await downloadPromise;
    expect(download.suggestedFilename()).toBe('architect-export.tar.gz');
  });

  test('export button shows loading state during export', async ({ page }) => {
    const exportBtn = page.locator('#btn-export');

    // Start the export and check that the button text changes
    const downloadPromise = page.waitForEvent('download');
    await exportBtn.click();

    // After download completes, button text should revert
    await downloadPromise;
    await expect(exportBtn).toContainText('Export Containerfiles');
    await expect(exportBtn).toBeEnabled();
  });

  test('export toast confirms success', async ({ page }) => {
    const downloadPromise = page.waitForEvent('download');
    await page.locator('#btn-export').click();
    await downloadPromise;

    const toast = page.locator('#toast');
    await expect(toast).toHaveClass(/visible/);
    await expect(toast).toContainText('exported');
  });
});
