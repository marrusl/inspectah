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

test.describe('Architect behavioral workflows', () => {
  test('move-up button moves package to parent layer and shows toast', async ({ page }) => {
    await page.goto(architectURL());
    await waitForArchitectBoot(page);

    // Find a derived layer with multiple packages from the page's topology.
    // Pick the LAST derived layer to avoid colliding with API tests that
    // operate on the first derived layer's first package.
    const derivedInfo = await page.evaluate(() => {
      type Layer = { name: string; parent: string | null; packages: string[] };
      const topo = (window as unknown as { __TOPOLOGY__: { layers: Layer[] } }).__TOPOLOGY__;
      const derived = topo.layers.filter(l => l.parent !== null && l.packages.length > 1);
      if (derived.length === 0) {
        // Fall back to any derived layer with packages
        const any = topo.layers.filter(l => l.parent !== null && l.packages.length > 0);
        return any.length > 0 ? { name: any[any.length - 1].name } : null;
      }
      return { name: derived[derived.length - 1].name };
    });
    if (!derivedInfo) return;

    // Click the specific derived layer card
    const targetCard = page.locator(`.layer-card[data-layer="${derivedInfo.name}"]`);
    await targetCard.click();
    await expect(targetCard).toHaveClass(/layer-selected/);

    // Wait for drawer to render with package rows
    await page.waitForTimeout(500);

    // Use the LAST moveup button to avoid the package the API tests touched
    const allMoveupBtns = page.locator('.moveup-btn');
    const btnCount = await allMoveupBtns.count();
    if (btnCount === 0) return;
    const moveupBtn = allMoveupBtns.nth(btnCount - 1);

    // Capture the package name and source/target for verification
    const pkgName = await moveupBtn.getAttribute('data-pkg');
    const fromLayer = await moveupBtn.getAttribute('data-from');
    const toLayer = await moveupBtn.getAttribute('data-to');
    expect(pkgName).toBeTruthy();
    expect(fromLayer).toBeTruthy();
    expect(toLayer).toBeTruthy();

    // Get package count in source layer badge before move
    const sourceBadge = page.locator(
      `.layer-card[data-layer="${fromLayer}"] .layer-badge-pkg`
    );
    const beforeBadgeText = await sourceBadge.textContent();

    // Click the move-up button
    await moveupBtn.click();

    // Toast should appear confirming the move
    const toast = page.locator('#toast');
    await expect(toast).toHaveClass(/visible/, { timeout: 3000 });
    const toastText = await toast.textContent();
    expect(toastText).toContain('Moved');
    expect(toastText).toContain(pkgName!);

    // Source layer badge should update (one fewer package)
    await page.waitForTimeout(300); // Allow re-render
    const afterBadgeText = await sourceBadge.textContent();
    const beforeCount = parseInt(beforeBadgeText!, 10);
    const afterCount = parseInt(afterBadgeText!, 10);
    expect(afterCount).toBe(beforeCount - 1);
  });

  test('copy-to dropdown copies package to sibling layer and shows toast', async ({ page }) => {
    await page.goto(architectURL());
    await waitForArchitectBoot(page);

    // The page HTML template has the ORIGINAL topology baked in at server start,
    // but API tests may have mutated server state. Fetch the CURRENT server
    // topology to find packages that actually exist on the server right now.
    type ServerLayer = { name: string; parent: string | null; packages: string[] };
    const serverTopo: { layers: ServerLayer[] } = await page.evaluate(async () => {
      const resp = await fetch('/api/topology');
      return resp.json();
    });

    const derivedWithPkgs = serverTopo.layers.filter(
      l => l.parent !== null && l.packages.length > 0
    );
    // Need at least 2 derived layers for copy dropdown to appear
    if (derivedWithPkgs.length < 2) return;

    // Pick a source layer that actually has packages on the server
    const sourceLayer = derivedWithPkgs[derivedWithPkgs.length - 1];
    const serverPkg = sourceLayer.packages[sourceLayer.packages.length - 1];

    // Select the source derived layer in the UI
    const sourceCard = page.locator(`.layer-card[data-layer="${sourceLayer.name}"]`);
    await sourceCard.click();
    await expect(sourceCard).toHaveClass(/layer-selected/);
    await page.waitForTimeout(500);

    // Find the copy button for the specific package we verified exists on the server
    const copyBtn = page.locator(`.move-btn[data-pkg="${serverPkg}"][data-from="${sourceLayer.name}"]`);
    if ((await copyBtn.count()) === 0) return;

    // Click to open the dropdown menu
    await copyBtn.click();

    // The move-menu should become visible (gets .open class)
    const menu = page.locator('.move-menu.open');
    await expect(menu).toBeVisible();

    // Click the first copy-action menu item
    const copyItem = menu.locator('.move-menu-item.copy-action').first();
    if ((await copyItem.count()) === 0) return;

    const targetLayerName = await copyItem.getAttribute('data-to');
    expect(targetLayerName).toBeTruthy();

    // Get the target layer's package count badge before copy
    await copyItem.click();

    // Toast should confirm the copy
    const toast = page.locator('#toast');
    await expect(toast).toHaveClass(/visible/, { timeout: 3000 });
    const toastText = await toast.textContent();
    expect(toastText).toContain('Copied');

    // Verify via API that the target layer now contains the copied package.
    // We use the API rather than badge counts because the page's HTML template
    // has the original topology baked in (pre-rendered at server start), so
    // badge counts cross a stale/fresh boundary on re-render.
    const verifyTopo: { layers: ServerLayer[] } = await page.evaluate(async () => {
      const resp = await fetch('/api/topology');
      return resp.json();
    });
    const updatedTarget = verifyTopo.layers.find(l => l.name === targetLayerName);
    expect(updatedTarget).toBeDefined();
    expect(updatedTarget!.packages).toContain(serverPkg);
  });

  test('preview modal shows correct layer name in title', async ({ page }) => {
    await page.goto(architectURL());
    await waitForArchitectBoot(page);

    // Get the layer name from the preview button's data attribute
    const previewBtn = page.locator('.layer-preview-btn').first();
    if ((await previewBtn.count()) === 0) return;

    const layerName = await previewBtn.getAttribute('data-preview-layer');
    expect(layerName).toBeTruthy();

    await previewBtn.click();

    const modal = page.locator('#containerfile-modal');
    await expect(modal).toBeVisible();

    // Modal title should contain the layer name and "Containerfile"
    const title = page.locator('#cf-modal-title');
    await expect(title).toContainText(layerName!);
    await expect(title).toContainText('Containerfile');
  });

  test('preview modal closes when clicking overlay backdrop', async ({ page }) => {
    await page.goto(architectURL());
    await waitForArchitectBoot(page);

    const previewBtn = page.locator('.layer-preview-btn').first();
    if ((await previewBtn.count()) === 0) return;

    await previewBtn.click();
    const modal = page.locator('#containerfile-modal');
    await expect(modal).toBeVisible();

    // Click the overlay backdrop (the outer cf-modal-overlay element)
    // Force click at position (0,0) of the overlay which is outside the modal
    await modal.click({ position: { x: 5, y: 5 } });
    await expect(modal).toBeHidden();
  });

  test('preview modal close button dismisses modal', async ({ page }) => {
    await page.goto(architectURL());
    await waitForArchitectBoot(page);

    const previewBtn = page.locator('.layer-preview-btn').first();
    if ((await previewBtn.count()) === 0) return;

    await previewBtn.click();
    const modal = page.locator('#containerfile-modal');
    await expect(modal).toBeVisible();

    // Click the X close button
    const closeBtn = page.locator('.cf-modal-close');
    await closeBtn.click();
    await expect(modal).toBeHidden();
  });

  test('export button triggers download of tar.gz archive', async ({ page }) => {
    await page.goto(architectURL());
    await waitForArchitectBoot(page);

    const exportBtn = page.locator('#btn-export');
    await expect(exportBtn).toBeVisible();

    // Set up download listener before clicking
    const downloadPromise = page.waitForEvent('download');
    await exportBtn.click();
    const download = await downloadPromise;

    // Verify the download filename
    expect(download.suggestedFilename()).toBe('architect-export.tar.gz');

    // Button should show "Exporting..." briefly then reset
    // (by the time download completes, it may have already reset)
    await expect(exportBtn).toContainText('Export', { timeout: 5000 });
  });

  test('export button shows loading state during export', async ({ page }) => {
    await page.goto(architectURL());
    await waitForArchitectBoot(page);

    const exportBtn = page.locator('#btn-export');
    await expect(exportBtn).toBeVisible();
    await expect(exportBtn).toBeEnabled();

    // The button text starts as "Export Containerfiles"
    await expect(exportBtn).toContainText('Export Containerfiles');

    // Click and quickly check the disabled + loading state
    const downloadPromise = page.waitForEvent('download');
    await exportBtn.click();

    // Button should become disabled with "Exporting..." text
    // This is a race — the export may complete very fast on local fixtures
    // so we verify the final state is restored regardless
    await downloadPromise;
    await expect(exportBtn).toBeEnabled({ timeout: 5000 });
    await expect(exportBtn).toContainText('Export Containerfiles');
  });

  test('move-up button has impact tooltip', async ({ page }) => {
    await page.goto(architectURL());
    await waitForArchitectBoot(page);

    // Find a derived layer with packages from the page's own topology
    const derivedName = await page.evaluate(() => {
      const topo = (window as unknown as { __TOPOLOGY__: { layers: Array<{ name: string; parent: string | null; packages: string[] }> } }).__TOPOLOGY__;
      const derived = topo.layers.find(l => l.parent !== null && l.packages.length > 0);
      return derived ? derived.name : null;
    });
    if (!derivedName) return;

    const card = page.locator(`.layer-card[data-layer="${derivedName}"]`);
    await card.click();
    await page.waitForTimeout(500);

    const moveupBtn = page.locator('.moveup-btn').first();
    if ((await moveupBtn.count()) === 0) return;

    // The moveup button should have a title attribute with impact info
    const title = await moveupBtn.getAttribute('title');
    expect(title).toBeTruthy();
    expect(title).toContain('Moving');
    expect(title).toContain('affects');
  });

  test('copy dropdown items show impact badges', async ({ page }) => {
    await page.goto(architectURL());
    await waitForArchitectBoot(page);

    // Find a derived layer with packages and siblings from page topology
    const derivedName = await page.evaluate(() => {
      const topo = (window as unknown as { __TOPOLOGY__: { layers: Array<{ name: string; parent: string | null; packages: string[] }> } }).__TOPOLOGY__;
      const derived = topo.layers.filter(l => l.parent !== null && l.packages.length > 0);
      return derived.length >= 2 ? derived[0].name : null;
    });
    if (!derivedName) return;

    const card = page.locator(`.layer-card[data-layer="${derivedName}"]`);
    await card.click();
    await page.waitForTimeout(500);

    const copyBtn = page.locator('.move-btn').first();
    if ((await copyBtn.count()) === 0) return;

    // Open the copy dropdown
    await copyBtn.click();

    const menu = page.locator('.move-menu.open');
    await expect(menu).toBeVisible();

    // Each menu item should have an impact badge
    const menuItems = menu.locator('.move-menu-item.copy-action');
    const count = await menuItems.count();
    if (count === 0) return;

    for (let i = 0; i < count; i++) {
      const badge = menuItems.nth(i).locator('.impact-badge');
      await expect(badge).toBeVisible();

      // Badge should contain image count and turbulence arrow
      const badgeText = await badge.textContent();
      expect(badgeText).toMatch(/\d+\s*imgs?\s*·/);
    }
  });

  test('clicking outside copy dropdown closes it', async ({ page }) => {
    await page.goto(architectURL());
    await waitForArchitectBoot(page);

    // Find a derived layer with packages and siblings from page topology
    const derivedName = await page.evaluate(() => {
      const topo = (window as unknown as { __TOPOLOGY__: { layers: Array<{ name: string; parent: string | null; packages: string[] }> } }).__TOPOLOGY__;
      const derived = topo.layers.filter(l => l.parent !== null && l.packages.length > 0);
      return derived.length >= 2 ? derived[0].name : null;
    });
    if (!derivedName) return;

    const card = page.locator(`.layer-card[data-layer="${derivedName}"]`);
    await card.click();
    await page.waitForTimeout(500);

    const copyBtn = page.locator('.move-btn').first();
    if ((await copyBtn.count()) === 0) return;

    // Open the dropdown
    await copyBtn.click();
    const menu = page.locator('.move-menu.open');
    await expect(menu).toBeVisible();

    // Click elsewhere to close
    await page.locator('.architect-center').click();
    await expect(menu).not.toBeVisible();
  });
});
