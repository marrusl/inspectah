import { test, expect } from '@playwright/test';
import { ARCHITECT_URL } from './helpers';

test.describe('Impact Badges and Tooltips', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto(ARCHITECT_URL);
    await expect(page.locator('.layer-card').first()).toBeVisible();
  });

  test('impact badge on move-up button has title with fan-out info', async ({ page }) => {
    // Select a derived layer to see impact badges
    const webLayer = page.locator('.layer-card[data-layer="web-servers-merged"]');
    await webLayer.click();
    await expect(page.locator('.drawer-header-title')).toHaveText('web-servers-merged');

    // Impact badges appear next to move-up buttons
    const impactBadge = page.locator('.impact-badge').first();
    await expect(impactBadge).toBeVisible();

    // Badge should have a title attribute with descriptive text about the move
    const title = await impactBadge.getAttribute('title');
    expect(title).toBeTruthy();
    expect(title).toContain('Moving');
    expect(title).toContain('image');
    expect(title).toContain('Turbulence');
  });

  test('impact badge shows turbulence level class', async ({ page }) => {
    const webLayer = page.locator('.layer-card[data-layer="web-servers-merged"]');
    await webLayer.click();

    const impactBadge = page.locator('.impact-badge').first();
    await expect(impactBadge).toBeVisible();

    // Badge should have one of the turbulence level classes
    const classList = await impactBadge.getAttribute('class');
    const hasLevel = classList!.includes('impact-low') ||
                     classList!.includes('impact-med') ||
                     classList!.includes('impact-high');
    expect(hasLevel).toBe(true);
  });

  test('impact badge text shows image count and turbulence values', async ({ page }) => {
    const webLayer = page.locator('.layer-card[data-layer="web-servers-merged"]');
    await webLayer.click();

    const impactBadge = page.locator('.impact-badge').first();
    const text = await impactBadge.textContent();

    // Badge text format: "N imgs · X.X→Y.Y"
    expect(text).toContain('img');
    expect(text).toMatch(/\d/); // contains at least one number
  });

  test('layer badge for turbulence has title with summary', async ({ page }) => {
    // Turbulence badge on a layer card should have a descriptive title
    const baseTurbBadge = page.locator('.layer-card[data-layer="base"] .layer-badge-turbulence');
    await expect(baseTurbBadge).toBeVisible();

    const title = await baseTurbBadge.getAttribute('title');
    expect(title).toBeTruthy();
    expect(title).toContain('impact');
  });

  test('layer badge for fan-out has title with rebuild info', async ({ page }) => {
    // The base layer should have a fan-out badge (it has derived children)
    const baseFanoutBadge = page.locator('.layer-card[data-layer="base"] .layer-badge-fanout');
    await expect(baseFanoutBadge).toBeVisible();

    const title = await baseFanoutBadge.getAttribute('title');
    expect(title).toBeTruthy();
    expect(title).toContain('derived layers');
  });

  test('hosts badge has title on layer cards', async ({ page }) => {
    // Base layer serves all hosts
    const baseHostsBadge = page.locator('.layer-card[data-layer="base"] .layer-badge-hosts');
    await expect(baseHostsBadge).toBeVisible();

    const title = await baseHostsBadge.getAttribute('title');
    expect(title).toBeTruthy();
    expect(title).toContain('hosts');
  });

  test('copy-to menu items have impact badges with titles', async ({ page }) => {
    // Select a derived layer
    const webLayer = page.locator('.layer-card[data-layer="web-servers-merged"]');
    await webLayer.click();

    // Open the copy-to dropdown
    const copyBtn = page.locator('.move-btn').first();
    await copyBtn.click();

    const openMenu = page.locator('.move-menu.open');
    await expect(openMenu).toBeVisible();

    // Menu items should have impact badges
    const menuImpactBadge = openMenu.locator('.impact-badge').first();
    await expect(menuImpactBadge).toBeVisible();

    const title = await menuImpactBadge.getAttribute('title');
    expect(title).toBeTruthy();
    expect(title).toContain('Copying');
    expect(title).toContain('Turbulence');
  });
});
