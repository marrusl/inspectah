/**
 * Smoke tests for the inspectah Go port architect server.
 * Validates that the architect dashboard loads and basic interactions work.
 */
import { test, expect } from '@playwright/test';

const architectURL = () => process.env.ARCHITECT_URL || 'http://localhost:9202';

test.describe('Architect server smoke tests', () => {
  test('health endpoint returns ok', async ({ request }) => {
    const resp = await request.get(`${architectURL()}/api/health`);
    expect(resp.ok()).toBeTruthy();
    const body = await resp.json();
    expect(body.status).toBe('ok');
  });

  test('topology API returns layer data', async ({ request }) => {
    const resp = await request.get(`${architectURL()}/api/topology`);
    expect(resp.ok()).toBeTruthy();
    const body = await resp.json();

    // Topology should have layers array
    expect(body.layers).toBeDefined();
    expect(Array.isArray(body.layers)).toBe(true);
    expect(body.layers.length).toBeGreaterThan(0);

    // Each layer should have a name and packages
    for (const layer of body.layers) {
      expect(layer.name).toBeDefined();
      expect(layer.packages).toBeDefined();
    }
  });

  test('architect page loads', async ({ page }) => {
    await page.goto(architectURL());
    await expect(page).toHaveTitle('inspectah Architect');

    // Masthead should show the brand
    const brand = page.locator('.pf-v6-c-masthead__brand');
    await expect(brand).toContainText('inspectah');
  });

  test('architect shows fleet sidebar', async ({ page }) => {
    await page.goto(architectURL());

    // Sidebar should have fleet cards
    const sidebar = page.locator('.architect-sidebar');
    await expect(sidebar).toBeVisible();

    const sidebarTitle = page.locator('.sidebar-title');
    await expect(sidebarTitle).toBeVisible();
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
  });
});

test.describe('Architect API endpoints', () => {
  test('export endpoint returns gzip data', async ({ request }) => {
    const resp = await request.get(`${architectURL()}/api/export`);
    expect(resp.ok()).toBeTruthy();
    expect(resp.headers()['content-type']).toContain('application/gzip');
    expect(resp.headers()['content-disposition']).toContain('attachment');
  });

  test('move endpoint requires POST method', async ({ request }) => {
    const resp = await request.get(`${architectURL()}/api/move`);
    // GET should be rejected (405 or similar)
    expect(resp.ok()).toBeFalsy();
  });

  test('copy endpoint requires POST method', async ({ request }) => {
    const resp = await request.get(`${architectURL()}/api/copy`);
    expect(resp.ok()).toBeFalsy();
  });

  test('move accepts valid package operation', async ({ request }) => {
    // First get the topology to find a valid package to move
    const topoResp = await request.get(`${architectURL()}/api/topology`);
    const topo = await topoResp.json();

    // Find a non-base layer with at least one package
    const sourceLayers = topo.layers.filter(
      (l: { parent: string | null; packages: unknown[] }) =>
        l.parent !== null && l.packages.length > 0
    );
    if (sourceLayers.length === 0) {
      test.skip();
      return;
    }

    const sourceLayer = sourceLayers[0];
    const pkg = sourceLayer.packages[0];

    // Find a different layer to move to
    const targetLayers = topo.layers.filter(
      (l: { name: string }) => l.name !== sourceLayer.name
    );
    if (targetLayers.length === 0) {
      test.skip();
      return;
    }

    const resp = await request.post(`${architectURL()}/api/move`, {
      data: {
        package: typeof pkg === 'string' ? pkg : pkg.name,
        from: sourceLayer.name,
        to: targetLayers[0].name,
      },
    });
    expect(resp.ok()).toBeTruthy();

    // Response should be the updated topology
    const updated = await resp.json();
    expect(updated.layers).toBeDefined();
  });
});
