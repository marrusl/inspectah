/**
 * Rebuild cycle tests for the refine report UI.
 * Validates the re-render flow: toggle items -> rebuild -> updated output.
 */
import { test, expect } from '@playwright/test';
import { waitForBoot, navigateToSection, isRefineMode, findToggleInSection } from './helpers';

test.describe('Rebuild cycle', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await waitForBoot(page);
  });

  test('rebuild bar is visible in refine mode', async ({ page }) => {
    const refine = await isRefineMode(page);
    expect(refine).toBe(true);

    const rebuildBar = page.locator('#rebuild-bar');
    await expect(rebuildBar).toBeVisible();
  });

  test('rebuild button exists and is enabled', async ({ page }) => {
    const rebuildBtn = page.locator('#rebuild-btn');
    await expect(rebuildBtn).toBeVisible();
    await expect(rebuildBtn).toBeEnabled();
    await expect(rebuildBtn).toHaveText('Rebuild');
  });

  test('rebuild triggers API call and updates UI', async ({ page }) => {
    // Find any toggle in config or runtime to create a change
    for (const sectionId of ['config', 'runtime', 'packages']) {
      await navigateToSection(page, sectionId);
      const toggle = await findToggleInSection(page, sectionId);
      if (toggle) {
        await toggle.click();
        break;
      }
    }

    // Click rebuild
    const rebuildBtn = page.locator('#rebuild-btn');
    const statusEl = page.locator('#rebuild-status');

    // Listen for the API call
    const renderPromise = page.waitForResponse(
      (resp) => resp.url().includes('/api/render') && resp.status() === 200,
      { timeout: 15_000 }
    );

    await rebuildBtn.click();

    // Button should show "Rebuilding..." state
    await expect(rebuildBtn).toHaveText('Rebuilding...');

    // Wait for the render response
    const renderResp = await renderPromise;
    expect(renderResp.ok()).toBeTruthy();

    // Button should return to "Rebuild" after completion
    await expect(rebuildBtn).toHaveText('Rebuild', { timeout: 10_000 });

    // Status should indicate completion
    await expect(statusEl).toContainText(/complete|downloading/i, { timeout: 5_000 });
  });

  test('rebuild response contains expected fields', async ({ page }) => {
    // Find any toggle to create a change
    for (const sectionId of ['config', 'runtime', 'packages']) {
      await navigateToSection(page, sectionId);
      const toggle = await findToggleInSection(page, sectionId);
      if (toggle) {
        await toggle.click();
        break;
      }
    }

    const renderPromise = page.waitForResponse(
      (resp) => resp.url().includes('/api/render') && resp.status() === 200,
      { timeout: 15_000 }
    );

    await page.locator('#rebuild-btn').click();
    const resp = await renderPromise;
    const body = await resp.json();

    expect(body.html).toBeDefined();
    expect(body.snapshot).toBeDefined();
    expect(body.containerfile).toBeDefined();
    expect(body.triage_manifest).toBeDefined();
    expect(body.render_id).toBeDefined();
    expect(body.revision).toBeGreaterThanOrEqual(1);
  });
});
