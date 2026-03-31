import { test, expect } from '@playwright/test';
import { FLEET_URL } from './helpers';

test.describe('Variant Selection', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto(FLEET_URL);
    // Wait for the helper script to detect the live server and enable toggles.
    // The helper calls /api/health and on success adds class 'helper-active'
    // to the toolbar and sets include-toggle-wrap elements to display: inline-grid.
    await page.locator('.helper-active').waitFor({ state: 'attached', timeout: 10_000 });
    // Navigate to Config tab
    await page.click('a[data-tab="config"]');
    await expect(page.locator('#section-config')).toBeVisible();
  });

  test('2-way variant group shows "2 variants" toggle', async ({ page }) => {
    // /etc/app.conf is a 2-way variant group
    const appConfGroup = page.locator('tr.fleet-variant-group', {
      has: page.locator('code', { hasText: '/etc/app.conf' }),
    });
    await expect(appConfGroup).toBeVisible();

    const toggle = appConfGroup.locator('.fleet-variant-toggle');
    await expect(toggle).toBeVisible();
    await expect(toggle).toContainText('2 variants');
  });

  test('3-way variant group shows "3 variants" toggle', async ({ page }) => {
    // /etc/httpd/conf/httpd.conf is a 3-way variant group
    const httpdGroup = page.locator('tr.fleet-variant-group', {
      has: page.locator('code', { hasText: '/etc/httpd/conf/httpd.conf' }),
    });
    await expect(httpdGroup).toBeVisible();

    const toggle = httpdGroup.locator('.fleet-variant-toggle');
    await expect(toggle).toContainText('3 variants');
  });

  test('variant children rows exist with correct data-variant-group', async ({ page }) => {
    // app.conf has 2 variant child rows (attached but hidden until expanded)
    const appConfVariants = page.locator('tr[data-variant-group="/etc/app.conf"]');
    await expect(appConfVariants).toHaveCount(2);

    // httpd.conf has 3 variant child rows
    const httpdVariants = page.locator('tr[data-variant-group="/etc/httpd/conf/httpd.conf"]');
    await expect(httpdVariants).toHaveCount(3);
  });

  test('expanding variant group shows children with "selected" labels', async ({ page }) => {
    // Click the "2 variants" toggle to expand the app.conf variant children
    const appConfGroup = page.locator('tr.fleet-variant-group', {
      has: page.locator('code', { hasText: '/etc/app.conf' }),
    });
    const variantToggle = appConfGroup.locator('.fleet-variant-toggle');
    await variantToggle.click();

    // The children row should now be visible (display: table-row)
    const childrenRow = page.locator('tr.fleet-variant-children').first();
    await expect(childrenRow).toBeVisible();

    // Variant children exist
    const appConfVariants = page.locator('tr[data-variant-group="/etc/app.conf"]');
    const count = await appConfVariants.count();
    expect(count).toBe(2);

    // Only the auto-selected variant (the one with include=true) shows
    // a "selected" label. The other variant shows a "Compare" button.
    // Verify exactly one variant has the "selected" label.
    const selectedLabels = page.locator(
      'tr[data-variant-group="/etc/app.conf"] .variant-selected-label'
    );
    await expect(selectedLabels).toHaveCount(1);
    await expect(selectedLabels.first()).toContainText('selected');

    // The non-selected variant should have a "Compare" button
    const compareButtons = page.locator(
      'tr[data-variant-group="/etc/app.conf"] .variant-compare-btn'
    );
    await expect(compareButtons).toHaveCount(1);
  });

  test('unchecking a variant excludes it and activates dirty state', async ({ page }) => {
    // Expand the app.conf variant group first
    const appConfGroup = page.locator('tr.fleet-variant-group', {
      has: page.locator('code', { hasText: '/etc/app.conf' }),
    });
    await appConfGroup.locator('.fleet-variant-toggle').click();

    // Wait for the children to be visible
    const childrenRow = page.locator('tr.fleet-variant-children').first();
    await expect(childrenRow).toBeVisible();

    // Uncheck variant 2 of app.conf (data-snap-index="1").
    // The PF switch component has a <span class="pf-v6-c-switch__toggle">
    // overlay that intercepts pointer events, so click the toggle span directly.
    const variant2 = page.locator(
      'tr[data-variant-group="/etc/app.conf"][data-snap-index="1"]'
    );
    const toggleSpan = variant2.locator('.pf-v6-c-switch__toggle');
    await toggleSpan.click();

    // Verify the checkbox is now unchecked
    const checkbox = variant2.locator('.include-toggle');
    await expect(checkbox).not.toBeChecked();

    // The row should get the 'excluded' class
    await expect(variant2).toHaveClass(/excluded/);

    // The toolbar should show dirty state (re-render button enabled)
    const rerender = page.locator('#btn-re-render');
    await expect(rerender).toBeEnabled();
  });

  test('unchecking all variants in 3-way group excludes all rows', async ({ page }) => {
    // Expand the httpd.conf variant group
    const httpdGroup = page.locator('tr.fleet-variant-group', {
      has: page.locator('code', { hasText: '/etc/httpd/conf/httpd.conf' }),
    });
    await httpdGroup.locator('.fleet-variant-toggle').click();

    // Wait for the children to be visible
    const httpdChildren = page.locator('tr.fleet-variant-children').nth(1);
    await expect(httpdChildren).toBeVisible();

    const httpdVariants = page.locator(
      'tr[data-variant-group="/etc/httpd/conf/httpd.conf"]'
    );
    const count = await httpdVariants.count();
    expect(count).toBe(3);

    // httpd.conf is a tied variant group — all 3 start unchecked (excluded).
    // The variant toggle uses radio-group behavior: checking one unchecks the rest.
    // To test the "exclude all" state, first check variant 1 (which selects it
    // and keeps others excluded via radio), then uncheck variant 1 to return
    // all to the excluded state.
    const variant1Toggle = httpdVariants.nth(0).locator('.pf-v6-c-switch__toggle');

    // Click variant 1 to check it (radio selects it, others remain excluded)
    await variant1Toggle.click();
    await expect(httpdVariants.nth(0).locator('.include-toggle')).toBeChecked();

    // Click variant 1 again to uncheck it (now all are excluded)
    await variant1Toggle.click();
    await expect(httpdVariants.nth(0).locator('.include-toggle')).not.toBeChecked();

    // All 3 variant rows should be excluded
    for (let i = 0; i < count; i++) {
      await expect(httpdVariants.nth(i)).toHaveClass(/excluded/);
      const checkbox = httpdVariants.nth(i).locator('.include-toggle');
      await expect(checkbox).not.toBeChecked();
    }
  });

  test('selecting a variant persists through re-render', async ({ page }) => {
    // Expand the app.conf variant group
    const appConfGroup = page.locator('tr.fleet-variant-group', {
      has: page.locator('code', { hasText: '/etc/app.conf' }),
    });
    await appConfGroup.locator('.fleet-variant-toggle').click();
    const childrenRow = page.locator('tr.fleet-variant-children').first();
    await expect(childrenRow).toBeVisible();

    // Uncheck variant 2 of app.conf by clicking the PF switch toggle
    const variant2 = page.locator(
      'tr[data-variant-group="/etc/app.conf"][data-snap-index="1"]'
    );
    const toggleSpan = variant2.locator('.pf-v6-c-switch__toggle');
    await toggleSpan.click();

    // Verify the change took effect: variant 2 checkbox should be unchecked
    const checkbox = variant2.locator('.include-toggle');
    await expect(checkbox).not.toBeChecked();

    // The re-render button should be enabled now (dirty state)
    const rerender = page.locator('#btn-re-render');
    await expect(rerender).toBeEnabled();

    // Click re-render and wait for the page to reload
    await Promise.all([
      page.waitForNavigation({ waitUntil: 'networkidle' }),
      rerender.click(),
    ]);

    // Wait for helper to reactivate after re-render
    await page.locator('.helper-active').waitFor({ state: 'attached', timeout: 10_000 });

    // Navigate back to config tab after re-render
    await page.click('a[data-tab="config"]');
    await expect(page.locator('#section-config')).toBeVisible();

    // Expand the variant group again (re-render resets expansion state)
    const appConfGroupAfter = page.locator('tr.fleet-variant-group', {
      has: page.locator('code', { hasText: '/etc/app.conf' }),
    });
    await appConfGroupAfter.locator('.fleet-variant-toggle').click();
    const childrenRowAfter = page.locator('tr.fleet-variant-children').first();
    await expect(childrenRowAfter).toBeVisible();

    // Verify variant 2 is still unchecked after re-render
    const variant2After = page.locator(
      'tr[data-variant-group="/etc/app.conf"][data-snap-index="1"]'
    );
    const checkboxAfter = variant2After.locator('.include-toggle');
    await expect(checkboxAfter).not.toBeChecked();
  });

  test('non-variant config file has no variant toggle', async ({ page }) => {
    // /etc/nginx/nginx.conf is a regular row, not a variant group
    const nginxRow = page.locator(
      'tr[data-snap-section="config"][data-snap-index="5"]'
    );
    await expect(nginxRow).toBeVisible();
    await expect(nginxRow.locator('code')).toContainText('/etc/nginx/nginx.conf');

    // It should NOT have a fleet-variant-toggle
    const toggle = nginxRow.locator('.fleet-variant-toggle');
    await expect(toggle).toHaveCount(0);
  });
});
