import { test, expect } from '@playwright/test';
import { ARCHITECT_URL } from './helpers';

test.describe('Package Move', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto(ARCHITECT_URL);
    await expect(page.locator('.layer-card').first()).toBeVisible();
  });

  test('derived layer packages have a move-up button', async ({ page }) => {
    // Select a derived layer to see its packages
    const webLayer = page.locator('.layer-card[data-layer="web-servers-merged"]');
    await webLayer.click();

    await expect(page.locator('.drawer-header-title')).toHaveText('web-servers-merged');

    // Move-up button should be present on derived layer packages
    const moveupBtn = page.locator('.moveup-btn').first();
    await expect(moveupBtn).toBeVisible();
    await expect(moveupBtn).toContainText('Move up');
  });

  test('base layer packages do not have move buttons', async ({ page }) => {
    // Select base layer
    const baseCard = page.locator('.layer-card[data-layer="base"]');
    await baseCard.click();

    await expect(page.locator('.drawer-header-title')).toHaveText('base');

    // Base layer packages should not have any move/copy buttons
    await expect(page.locator('.moveup-btn')).toHaveCount(0);
    await expect(page.locator('.move-btn')).toHaveCount(0);
  });

  test('moving a package updates layer package counts', async ({ page }) => {
    // Select web-servers-merged layer
    const webLayer = page.locator('.layer-card[data-layer="web-servers-merged"]');
    await webLayer.click();
    await expect(page.locator('.drawer-header-title')).toHaveText('web-servers-merged');

    // Get initial package count from the badge
    const webBadge = page.locator('.layer-card[data-layer="web-servers-merged"] .layer-badge-pkg');
    const initialWebText = await webBadge.textContent();
    const initialWebCount = parseInt(initialWebText!.match(/\d+/)![0], 10);

    const baseBadge = page.locator('.layer-card[data-layer="base"] .layer-badge-pkg');
    const initialBaseText = await baseBadge.textContent();
    const initialBaseCount = parseInt(initialBaseText!.match(/\d+/)![0], 10);

    // Click the first move-up button
    const moveupBtn = page.locator('.moveup-btn').first();
    await moveupBtn.click();

    // Wait for the toast to confirm the move
    const toast = page.locator('#toast');
    await expect(toast).toHaveClass(/visible/);
    await expect(toast).toContainText('Moved');

    // Package count on web-servers-merged should decrease by 1
    const newWebText = await webBadge.textContent();
    const newWebCount = parseInt(newWebText!.match(/\d+/)![0], 10);
    expect(newWebCount).toBe(initialWebCount - 1);

    // Package count on base should increase by 1
    const newBaseText = await baseBadge.textContent();
    const newBaseCount = parseInt(newBaseText!.match(/\d+/)![0], 10);
    expect(newBaseCount).toBe(initialBaseCount + 1);
  });

  test('derived layer packages have a copy-to dropdown', async ({ page }) => {
    // Select a derived layer
    const webLayer = page.locator('.layer-card[data-layer="web-servers-merged"]');
    await webLayer.click();

    // Copy-to dropdown trigger button should exist
    const copyBtn = page.locator('.move-btn').first();
    await expect(copyBtn).toBeVisible();
    await expect(copyBtn).toContainText('Copy to');

    // Click to open the dropdown menu
    await copyBtn.click();

    // Menu should open and show sibling layers
    const openMenu = page.locator('.move-menu.open');
    await expect(openMenu).toBeVisible();

    // Should show db-servers and app-servers as copy targets
    const menuItems = openMenu.locator('.move-menu-item');
    const count = await menuItems.count();
    expect(count).toBe(2); // 2 siblings
  });
});
