import { test, expect } from '@playwright/test';
import { ARCHITECT_URL } from './helpers';

test.describe('Layer Decomposition', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto(ARCHITECT_URL);
    // Wait for the layer tree to render
    await expect(page.locator('.layer-card').first()).toBeVisible();
  });

  test('base layer renders with "base" name', async ({ page }) => {
    const baseCard = page.locator('.layer-card[data-layer="base"]');
    await expect(baseCard).toBeVisible();
    await expect(baseCard.locator('.layer-card-name')).toHaveText('base');
  });

  test('at least 4 layer cards render (base + 3 derived)', async ({ page }) => {
    const layers = page.locator('.layer-card');
    const count = await layers.count();
    expect(count).toBeGreaterThanOrEqual(4);
  });

  test('derived layers exist for each fleet', async ({ page }) => {
    for (const fleet of ['web-servers-merged', 'db-servers-merged', 'app-servers-merged']) {
      const card = page.locator(`.layer-card[data-layer="${fleet}"]`);
      await expect(card).toBeVisible();
    }
  });

  test('derived layers have layer-derived class', async ({ page }) => {
    const derived = page.locator('.layer-card.layer-derived');
    const count = await derived.count();
    expect(count).toBe(3);
  });

  test('base layer contains shared packages in drawer', async ({ page }) => {
    // Base layer is selected by default, so drawer should show its packages
    const baseCard = page.locator('.layer-card[data-layer="base"]');
    await baseCard.click();

    // Verify drawer header shows "base"
    const drawerTitle = page.locator('.drawer-header-title');
    await expect(drawerTitle).toHaveText('base');

    // Check for expected shared packages
    const sharedPackages = [
      'bash-completion', 'bind-utils', 'curl', 'jq', 'lsof',
      'net-tools', 'rsync', 'strace', 'tcpdump', 'unzip',
    ];

    for (const pkg of sharedPackages) {
      const pkgRow = page.locator('.pkg-name', { hasText: pkg });
      await expect(pkgRow.first()).toBeVisible();
    }
  });

  test('base layer package count badge shows 10 pkgs', async ({ page }) => {
    const baseCard = page.locator('.layer-card[data-layer="base"]');
    const pkgBadge = baseCard.locator('.layer-badge-pkg');
    await expect(pkgBadge).toHaveText('10 pkgs');
  });

  test('each derived layer has a package count badge', async ({ page }) => {
    for (const fleet of ['web-servers-merged', 'db-servers-merged', 'app-servers-merged']) {
      const card = page.locator(`.layer-card[data-layer="${fleet}"]`);
      const pkgBadge = card.locator('.layer-badge-pkg');
      await expect(pkgBadge).toContainText('pkg');
    }
  });

  test('toolbar summary shows fleet, host, and layer counts', async ({ page }) => {
    const summary = page.locator('#toolbar-summary');
    await expect(summary).toContainText('fleet');
    await expect(summary).toContainText('host');
    await expect(summary).toContainText('layer');
  });

  test('fleet sidebar lists all 3 fleets', async ({ page }) => {
    const fleetCards = page.locator('.fleet-card');
    await expect(fleetCards).toHaveCount(3);

    for (const fleet of ['web-servers-merged', 'db-servers-merged', 'app-servers-merged']) {
      const card = page.locator(`.fleet-card[data-fleet="${fleet}"]`);
      await expect(card).toBeVisible();
    }
  });

  test('clicking a fleet card selects its layer', async ({ page }) => {
    const webFleet = page.locator('.fleet-card[data-fleet="web-servers-merged"]');
    await webFleet.click();

    // Drawer should now show "web-servers-merged"
    const drawerTitle = page.locator('.drawer-header-title');
    await expect(drawerTitle).toHaveText('web-servers-merged');

    // The layer card should be selected
    const layerCard = page.locator('.layer-card[data-layer="web-servers-merged"]');
    await expect(layerCard).toHaveClass(/layer-selected/);
  });
});
