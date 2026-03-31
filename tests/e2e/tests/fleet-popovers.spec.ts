import { test, expect } from '@playwright/test';
import { FLEET_URL } from './helpers';

test.describe('Fleet Bar Popovers', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto(FLEET_URL);
    // Navigate to packages section where fleet bars appear
    await page.click('a[data-tab="packages"]');
    await expect(page.locator('#section-packages')).toBeVisible();
  });

  test('clicking fleet bar opens PF6 popover', async ({ page }) => {
    // Find the first fleet bar in the packages section
    const fleetBar = page.locator('#section-packages .fleet-bar').first();
    await expect(fleetBar).toBeVisible();

    // Click the fleet bar to open popover
    await fleetBar.click();

    // The popover should appear inside the fleet bar
    const popover = page.locator('.pf-v6-c-popover.fleet-popover');
    await expect(popover).toBeVisible();
  });

  test('fleet bar gets active outline on click', async ({ page }) => {
    const fleetBar = page.locator('#section-packages .fleet-bar').first();
    await expect(fleetBar).toBeVisible();

    // Before clicking, bar should NOT have active class
    await expect(fleetBar).not.toHaveClass(/active/);

    // Click to open popover
    await fleetBar.click();

    // Bar should now have the 'active' class
    await expect(fleetBar).toHaveClass(/\bactive\b/);
  });

  test('popover shows host breakdown', async ({ page }) => {
    const fleetBar = page.locator('#section-packages .fleet-bar').first();
    await fleetBar.click();

    // The popover body should contain host information (not be empty)
    const popoverBody = page.locator('.pf-v6-c-popover.fleet-popover .pf-v6-c-popover__body');
    await expect(popoverBody).toBeVisible();
    await expect(popoverBody).not.toBeEmpty();

    // The popover should show a "Fleet Breakdown" title
    const popoverTitle = page.locator('.pf-v6-c-popover.fleet-popover .pf-v6-c-popover__title');
    await expect(popoverTitle).toContainText('Fleet Breakdown');

    // The popover should show the host count
    const countLabel = page.locator('.pf-v6-c-popover.fleet-popover .fleet-popover-count');
    await expect(countLabel).toContainText('hosts');
  });

  test('clicking outside closes popover and removes active state', async ({ page }) => {
    const fleetBar = page.locator('#section-packages .fleet-bar').first();
    await fleetBar.click();

    // Verify popover is open
    const popover = page.locator('.pf-v6-c-popover.fleet-popover');
    await expect(popover).toBeVisible();
    await expect(fleetBar).toHaveClass(/\bactive\b/);

    // Click outside the fleet bar (on the main content area)
    await page.locator('#main-content').click({ position: { x: 10, y: 10 } });

    // Popover should be removed from the DOM
    await expect(popover).toHaveCount(0);

    // Fleet bar should no longer have active class
    await expect(fleetBar).not.toHaveClass(/active/);
  });
});
