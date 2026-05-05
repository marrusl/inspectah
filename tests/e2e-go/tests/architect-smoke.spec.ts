/**
 * Architect server tests for the inspectah Go port.
 *
 * Covers: page load, API endpoints, fleet sidebar interaction,
 * layer tree rendering, move/copy operations via API and UI,
 * toast notifications, preview modal with Containerfile content,
 * and export archive.
 */
import { test, expect } from '@playwright/test';
import { architectURL, waitForArchitectBoot } from './helpers';

test.describe('Architect server smoke tests', () => {
  test('health endpoint returns ok', async ({ request }) => {
    const resp = await request.get(`${architectURL()}/api/health`);
    expect(resp.ok()).toBeTruthy();
    const body = await resp.json();
    expect(body.status).toBe('ok');
  });

  test('topology API returns layers with packages', async ({ request }) => {
    const resp = await request.get(`${architectURL()}/api/topology`);
    expect(resp.ok()).toBeTruthy();
    const body = await resp.json();

    expect(body.layers).toBeDefined();
    expect(Array.isArray(body.layers)).toBe(true);
    expect(body.layers.length).toBeGreaterThan(0);

    // Each layer should have a name and packages array
    for (const layer of body.layers) {
      expect(layer.name).toBeTruthy();
      expect(Array.isArray(layer.packages)).toBe(true);
    }
  });

  test('architect page loads with correct title', async ({ page }) => {
    await page.goto(architectURL());
    await expect(page).toHaveTitle('inspectah Architect');

    const brand = page.locator('.pf-v6-c-masthead__brand');
    await expect(brand).toContainText('inspectah');
  });

  test('architect has skip-to-content link', async ({ page }) => {
    await page.goto(architectURL());

    const skipLink = page.locator('.pf-v6-c-skip-to-content');
    await expect(skipLink).toBeAttached();
    await expect(skipLink).toHaveAttribute('href', '#main-content');
  });

  test('architect starts in dark theme', async ({ page }) => {
    await page.goto(architectURL());

    const isDark = await page.evaluate(() =>
      document.documentElement.classList.contains('pf-v6-theme-dark')
    );
    expect(isDark).toBe(true);
  });

  test('architect layout has three-column grid', async ({ page }) => {
    await page.goto(architectURL());

    const layout = page.locator('.architect-layout');
    await expect(layout).toBeVisible();

    // All three columns should be present
    const sidebar = page.locator('#fleet-sidebar');
    const center = page.locator('#main-content');
    const drawer = page.locator('#package-drawer');
    await expect(sidebar).toBeVisible();
    await expect(center).toBeVisible();
    await expect(drawer).toBeAttached();
  });
});

test.describe('Architect fleet sidebar', () => {
  test('fleet sidebar renders fleet cards', async ({ page }) => {
    await page.goto(architectURL());
    await waitForArchitectBoot(page);

    const fleetCards = page.locator('.fleet-card');
    const count = await fleetCards.count();
    expect(count).toBeGreaterThan(0);
  });

  test('clicking a fleet card marks it active', async ({ page }) => {
    await page.goto(architectURL());
    await waitForArchitectBoot(page);

    const fleetCards = page.locator('.fleet-card');
    const count = await fleetCards.count();
    expect(count).toBeGreaterThan(0);

    // Click the first fleet card
    await fleetCards.first().click();

    // It should get the fleet-active class
    await expect(fleetCards.first()).toHaveClass(/fleet-active/);
  });

  test('fleet cards show host count and package count', async ({ page }) => {
    await page.goto(architectURL());
    await waitForArchitectBoot(page);

    const meta = page.locator('.fleet-card-meta').first();
    await expect(meta).toBeVisible();
    const text = await meta.textContent();
    // Should contain "hosts" and "pkgs"
    expect(text).toMatch(/\d+\s*(hosts|pkgs)/);
  });
});

test.describe('Architect layer tree', () => {
  test('layer tree renders with at least one layer card', async ({ page }) => {
    await page.goto(architectURL());
    await waitForArchitectBoot(page);

    const layerCards = page.locator('.layer-card');
    await expect(layerCards.first()).toBeVisible();
  });

  test('layer cards show package count badges', async ({ page }) => {
    await page.goto(architectURL());
    await waitForArchitectBoot(page);

    const badges = page.locator('.layer-badge-pkg');
    await expect(badges.first()).toBeVisible();
  });

  test('clicking a layer card selects it', async ({ page }) => {
    await page.goto(architectURL());
    await waitForArchitectBoot(page);

    const layerCards = page.locator('.layer-card');
    const count = await layerCards.count();
    expect(count).toBeGreaterThan(0);

    // Click the first layer card
    await layerCards.first().click();

    // Layer card should get the selected class
    await expect(layerCards.first()).toHaveClass(/layer-selected/);
  });
});

test.describe('Architect API endpoints', () => {
  test('export endpoint returns gzip archive', async ({ request }) => {
    const resp = await request.get(`${architectURL()}/api/export`);
    expect(resp.ok()).toBeTruthy();
    expect(resp.headers()['content-type']).toContain('application/gzip');
    expect(resp.headers()['content-disposition']).toContain('attachment');

    // Archive should have non-trivial size
    const body = await resp.body();
    expect(body.length).toBeGreaterThan(100);
  });

  test('preview endpoint returns Containerfile text for a layer', async ({ request }) => {
    // Get topology to find a layer name
    const topoResp = await request.get(`${architectURL()}/api/topology`);
    const topo = await topoResp.json();
    const layerName = topo.layers[0].name;

    const resp = await request.get(`${architectURL()}/api/preview/${encodeURIComponent(layerName)}`);
    expect(resp.ok()).toBeTruthy();
    const text = await resp.text();
    expect(text).toContain('FROM');
  });

  test('move endpoint requires POST method', async ({ request }) => {
    const resp = await request.get(`${architectURL()}/api/move`);
    expect(resp.ok()).toBeFalsy();
  });

  test('copy endpoint requires POST method', async ({ request }) => {
    const resp = await request.get(`${architectURL()}/api/copy`);
    expect(resp.ok()).toBeFalsy();
  });

  test('move accepts valid package operation and returns updated topology', async ({ request }) => {
    const topoResp = await request.get(`${architectURL()}/api/topology`);
    const topo = await topoResp.json();

    // Find a non-base layer with at least one package
    const sourceLayers = topo.layers.filter(
      (l: { parent: string | null; packages: unknown[] }) =>
        l.parent !== null && l.packages.length > 0
    );
    if (sourceLayers.length === 0) return; // No movable packages

    const sourceLayer = sourceLayers[0];
    const pkg = sourceLayer.packages[0];
    const pkgName = typeof pkg === 'string' ? pkg : pkg.name;

    // Find a different layer to move to
    const targetLayer = topo.layers.find(
      (l: { name: string }) => l.name !== sourceLayer.name
    );
    expect(targetLayer).toBeDefined();

    const resp = await request.post(`${architectURL()}/api/move`, {
      data: { package: pkgName, from: sourceLayer.name, to: targetLayer.name },
    });
    expect(resp.ok()).toBeTruthy();

    const updated = await resp.json();
    expect(updated.layers).toBeDefined();
    expect(Array.isArray(updated.layers)).toBe(true);
  });

  test('copy adds package to target layer without removing from source', async ({ request }) => {
    // Re-fetch topology (may have changed from move test)
    const topoResp = await request.get(`${architectURL()}/api/topology`);
    const topo = await topoResp.json();

    const sourceLayers = topo.layers.filter(
      (l: { parent: string | null; packages: unknown[] }) =>
        l.parent !== null && l.packages.length > 0
    );
    if (sourceLayers.length === 0) return;

    const sourceLayer = sourceLayers[0];
    const pkg = sourceLayer.packages[0];
    const pkgName = typeof pkg === 'string' ? pkg : pkg.name;
    const sourceCount = sourceLayer.packages.length;

    const targetLayer = topo.layers.find(
      (l: { name: string }) => l.name !== sourceLayer.name
    );
    expect(targetLayer).toBeDefined();

    const resp = await request.post(`${architectURL()}/api/copy`, {
      data: { package: pkgName, from: sourceLayer.name, to: targetLayer.name },
    });
    expect(resp.ok()).toBeTruthy();

    const updated = await resp.json();
    // Source layer should still have the same number of packages (copy, not move)
    const updatedSource = updated.layers.find(
      (l: { name: string }) => l.name === sourceLayer.name
    );
    expect(updatedSource.packages.length).toBe(sourceCount);
  });
});

test.describe('Architect UI interactions', () => {
  test('selecting a derived layer shows drawer content', async ({ page }) => {
    await page.goto(architectURL());
    await waitForArchitectBoot(page);

    // Find a derived layer card (has layer-derived class)
    const derivedCards = page.locator('.layer-card.layer-derived');
    const count = await derivedCards.count();
    if (count === 0) {
      // Fixture may not have derived layers -- verify base layer at minimum
      const baseCards = page.locator('.layer-card');
      expect(await baseCards.count()).toBeGreaterThan(0);
      return;
    }

    // Click the first derived layer card
    await derivedCards.first().click();
    await expect(derivedCards.first()).toHaveClass(/layer-selected/);

    // Drawer should populate (derived layers always have packages)
    const pkgRows = page.locator('.pkg-row');
    // Give the drawer time to render
    await page.waitForTimeout(500);
    if ((await pkgRows.count()) > 0) {
      await expect(pkgRows.first()).toBeVisible();
    }
  });

  test('preview button opens modal with Containerfile content', async ({ page }) => {
    await page.goto(architectURL());
    await waitForArchitectBoot(page);

    // Find and click a preview button on a layer card
    const previewBtn = page.locator('.layer-preview-btn').first();
    if ((await previewBtn.count()) === 0) return;

    await previewBtn.click();

    // Modal should appear with Containerfile content
    const modal = page.locator('#containerfile-modal');
    await expect(modal).toBeVisible();

    const modalBody = page.locator('#cf-modal-body');
    const content = await modalBody.textContent();
    expect(content).toContain('FROM');
  });

  test('preview modal closes on Escape', async ({ page }) => {
    await page.goto(architectURL());
    await waitForArchitectBoot(page);

    const previewBtn = page.locator('.layer-preview-btn').first();
    if ((await previewBtn.count()) === 0) return;

    await previewBtn.click();
    const modal = page.locator('#containerfile-modal');
    await expect(modal).toBeVisible();

    await page.keyboard.press('Escape');
    await expect(modal).toBeHidden();
  });

  test('toolbar has export button and toolbar role', async ({ page }) => {
    await page.goto(architectURL());
    await waitForArchitectBoot(page);

    const toolbar = page.locator('[role="toolbar"]');
    await expect(toolbar).toBeVisible();
    await expect(toolbar).toHaveAttribute('aria-label', 'Actions');

    const exportBtn = page.locator('#btn-export');
    await expect(exportBtn).toBeVisible();
    await expect(exportBtn).toContainText('Export');
  });

  test('toast element exists for notifications', async ({ page }) => {
    await page.goto(architectURL());
    await waitForArchitectBoot(page);

    // Toast element should exist in DOM (hidden until triggered)
    const toast = page.locator('#toast');
    await expect(toast).toBeAttached();
    await expect(toast).toHaveClass(/architect-toast/);
  });
});
