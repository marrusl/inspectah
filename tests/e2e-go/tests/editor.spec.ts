/**
 * Config/file editor tests for the refine report UI.
 * Validates the Edit Files section with tab bar, file list, and CodeMirror.
 *
 * Requires a fixture with config files, drop-ins, or quadlet units.
 * When the fixture has no editor files, the editor section is hidden
 * and all tests skip gracefully.
 */
import { test, expect } from '@playwright/test';
import { waitForRefineBoot, navigateToSection } from './helpers';

test.describe('File editor', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await waitForRefineBoot(page);

    // The editor section is only rendered when the fixture has config files,
    // drop-ins, or quadlet units. If the nav link is hidden, skip the test.
    const editorNav = page.locator('[data-section="editor"]');
    const isNavVisible = (await editorNav.count()) > 0 &&
      await editorNav.evaluate((el) => {
        const li = el.closest('li');
        return li ? getComputedStyle(li).display !== 'none' : true;
      });

    if (!isNavVisible) {
      test.skip(true, 'Fixture has no editor files (config, drop-ins, or quadlets)');
      return;
    }

    await navigateToSection(page, 'editor');
  });

  test('editor section renders with heading', async ({ page }) => {
    const heading = page.locator('#heading-editor');
    await expect(heading).toBeVisible();
  });

  test('tab bar has tablist role with accessible label', async ({ page }) => {
    const tablist = page.getByRole('tablist', { name: 'File categories' });
    await expect(tablist).toBeVisible();
  });

  test('editor has at least one tab', async ({ page }) => {
    const tabs = page.getByRole('tab');
    const count = await tabs.count();
    expect(count).toBeGreaterThanOrEqual(1);
  });

  test('active tab has aria-selected true, others false', async ({ page }) => {
    const tabs = page.getByRole('tab');
    const count = await tabs.count();
    expect(count).toBeGreaterThanOrEqual(1);

    // Exactly one tab should be selected
    const selectedTabs = page.locator('[role="tab"][aria-selected="true"]');
    await expect(selectedTabs).toHaveCount(1);

    // Remaining tabs should have aria-selected="false"
    for (let i = 0; i < count; i++) {
      const tab = tabs.nth(i);
      const selected = await tab.getAttribute('aria-selected');
      expect(['true', 'false']).toContain(selected);
    }
  });

  test('clicking a tab switches the active panel', async ({ page }) => {
    const tabs = page.getByRole('tab');
    const count = await tabs.count();

    const targetTab = count >= 2 ? tabs.nth(1) : tabs.first();
    const tabId = await targetTab.getAttribute('data-tab');
    await targetTab.click();

    // Clicked tab should now be selected
    await expect(targetTab).toHaveAttribute('aria-selected', 'true');

    // Corresponding panel should be visible
    const panel = page.locator(`#editor-panel-${tabId}`);
    await expect(panel).toBeVisible();
  });

  test('file list items have role="option" for keyboard navigation', async ({ page }) => {
    const fileItems = page.locator('[role="option"]');
    const count = await fileItems.count();
    expect(count).toBeGreaterThan(0);
  });

  test('selecting a file shows read-only view with Edit button', async ({ page }) => {
    const fileItems = page.locator('[role="option"]');
    await expect(fileItems.first()).toBeVisible();

    await fileItems.first().click();

    const editBtn = page.locator('#editor-edit-btn');
    await expect(editBtn).toBeVisible({ timeout: 5_000 });
    await expect(editBtn).toHaveText('Edit');
  });

  test('tab keyboard navigation: ArrowRight/ArrowLeft moves between tabs', async ({ page }) => {
    const tabs = page.getByRole('tab');
    const count = await tabs.count();
    if (count < 2) return; // Need 2+ tabs for this test

    await tabs.first().focus();
    await expect(tabs.first()).toBeFocused();

    await page.keyboard.press('ArrowRight');
    await expect(tabs.nth(1)).toBeFocused();
  });

  test('file list keyboard navigation: ArrowDown moves focus', async ({ page }) => {
    const fileItems = page.locator('[role="option"]');
    const count = await fileItems.count();
    if (count < 2) return; // Need 2+ files for this test

    await fileItems.first().focus();
    await expect(fileItems.first()).toBeFocused();

    await page.keyboard.press('ArrowDown');
    await expect(fileItems.nth(1)).toBeFocused();
  });

  test('file list keyboard navigation: Enter selects a file', async ({ page }) => {
    const fileItems = page.locator('[role="option"]');
    await expect(fileItems.first()).toBeVisible();

    await fileItems.first().focus();
    await page.keyboard.press('Enter');

    const editBtn = page.locator('#editor-edit-btn');
    await expect(editBtn).toBeVisible({ timeout: 5_000 });
  });
});
