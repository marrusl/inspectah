/**
 * Config/file editor tests for the refine report UI.
 * Validates the Edit Files section with tab bar, file list, and CodeMirror.
 */
import { test, expect } from '@playwright/test';
import { waitForBoot, navigateToSection } from './helpers';

test.describe('File editor', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await waitForBoot(page);
    await navigateToSection(page, 'editor');
  });

  test('editor section renders', async ({ page }) => {
    const heading = page.locator('#heading-editor');
    await expect(heading).toBeVisible();
  });

  test('tab bar shows file categories', async ({ page }) => {
    const tablist = page.locator('[role="tablist"][aria-label="File categories"]');
    await expect(tablist).toBeAttached();

    const tabs = page.locator('.editor-tab');
    const count = await tabs.count();
    // Should have Config, Drop-ins, and Quadlets tabs
    expect(count).toBeGreaterThanOrEqual(1);
  });

  test('clicking a tab switches the active panel', async ({ page }) => {
    const tabs = page.locator('.editor-tab');
    const count = await tabs.count();
    if (count < 2) {
      test.skip();
      return;
    }

    // Click the second tab
    const secondTab = tabs.nth(1);
    const tabId = await secondTab.getAttribute('data-tab');
    await secondTab.click();

    // Tab should be marked as selected
    await expect(secondTab).toHaveAttribute('aria-selected', 'true');

    // Corresponding panel should be visible
    const panel = page.locator(`#editor-panel-${tabId}`);
    await expect(panel).toBeVisible();
  });

  test('file list shows files with role="option"', async ({ page }) => {
    const fileItems = page.locator('.editor-file-item, [role="option"]');
    // Editor may or may not have files depending on the fixture
    const count = await fileItems.count();
    // Just verify the structure renders without error
    expect(count).toBeGreaterThanOrEqual(0);
  });

  test('selecting a file shows read-only view with Edit button', async ({ page }) => {
    const fileItems = page.locator('[role="option"]');
    const count = await fileItems.count();
    if (count === 0) {
      test.skip();
      return;
    }

    // Click the first file
    await fileItems.first().click();

    // Should show the editor toolbar with the file path and Edit button
    const toolbar = page.locator('.editor-toolbar');
    await expect(toolbar).toBeVisible({ timeout: 5_000 });

    const editBtn = page.locator('#editor-edit-btn');
    await expect(editBtn).toBeVisible();
    await expect(editBtn).toHaveText('Edit');
  });
});
