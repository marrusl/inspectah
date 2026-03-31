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
    // Click the "2 variants" toggle to expand the nginx.conf variant children
    // (nginx.conf is a clear winner with include=True on the majority variant)
    const nginxGroup = page.locator('tr.fleet-variant-group', {
      has: page.locator('code', { hasText: '/etc/nginx/nginx.conf' }),
    });
    const variantToggle = nginxGroup.locator('.fleet-variant-toggle');
    await variantToggle.click();

    // The children row should now be visible (display: table-row)
    const nginxChildren = page.locator('tr.fleet-variant-children', {
      has: page.locator('tr[data-variant-group="/etc/nginx/nginx.conf"]'),
    });
    await expect(nginxChildren).toBeVisible();

    // Variant children exist
    const nginxVariants = page.locator('tr[data-variant-group="/etc/nginx/nginx.conf"]');
    const count = await nginxVariants.count();
    expect(count).toBe(2);

    // Only the auto-selected variant (the one with include=true) shows
    // a "selected" label. The other variant shows a "Compare" button.
    // Verify exactly one variant has the "selected" label.
    const selectedLabels = page.locator(
      'tr[data-variant-group="/etc/nginx/nginx.conf"] .variant-selected-label'
    );
    await expect(selectedLabels).toHaveCount(1);
    await expect(selectedLabels.first()).toContainText('selected');

    // The non-selected variant should have a "Compare" button
    const compareButtons = page.locator(
      'tr[data-variant-group="/etc/nginx/nginx.conf"] .variant-compare-btn'
    );
    await expect(compareButtons).toHaveCount(1);
  });

  test('unchecking a variant excludes it and activates dirty state', async ({ page }) => {
    // Expand the nginx.conf variant group first (clear winner, variant 1 checked)
    const nginxGroup = page.locator('tr.fleet-variant-group', {
      has: page.locator('code', { hasText: '/etc/nginx/nginx.conf' }),
    });
    await nginxGroup.locator('.fleet-variant-toggle').click();

    // Wait for the children to be visible
    const nginxChildren = page.locator('tr.fleet-variant-children', {
      has: page.locator('tr[data-variant-group="/etc/nginx/nginx.conf"]'),
    });
    await expect(nginxChildren).toBeVisible();

    // Uncheck the selected variant of nginx.conf (data-snap-index="5").
    // The PF switch component has a <span class="pf-v6-c-switch__toggle">
    // overlay that intercepts pointer events, so click the toggle span directly.
    const variant1 = page.locator(
      'tr[data-variant-group="/etc/nginx/nginx.conf"][data-snap-index="5"]'
    );
    const toggleSpan = variant1.locator('.pf-v6-c-switch__toggle');
    await toggleSpan.click();

    // Verify the checkbox is now unchecked
    const checkbox = variant1.locator('.include-toggle');
    await expect(checkbox).not.toBeChecked();

    // The row should get the 'excluded' class
    await expect(variant1).toHaveClass(/excluded/);

    // The toolbar should show dirty state (re-render button enabled)
    const rerender = page.locator('#btn-re-render');
    await expect(rerender).toBeEnabled();
  });

  test('unchecking all variants in 3-way group excludes all rows', async ({ page }) => {
    // httpd.conf is a tied variant group — pre-expanded on page load
    const httpdGroup = page.locator('tr.fleet-variant-group', {
      has: page.locator('code', { hasText: '/etc/httpd/conf/httpd.conf' }),
    });

    // The children should already be visible (tied groups are pre-expanded)
    const httpdChildren = page.locator('tr.fleet-variant-children', {
      has: page.locator('tr[data-variant-group="/etc/httpd/conf/httpd.conf"]'),
    });
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
    // httpd.conf is a tied group (all include=false) — pre-expanded on load.
    // Select variant 1 to resolve the tie, then verify it persists.
    const httpdChildren = page.locator('tr.fleet-variant-children', {
      has: page.locator('tr[data-variant-group="/etc/httpd/conf/httpd.conf"]'),
    });
    await expect(httpdChildren).toBeVisible();

    // Check variant 1 of httpd.conf (data-snap-index="2")
    const variant1 = page.locator(
      'tr[data-variant-group="/etc/httpd/conf/httpd.conf"][data-snap-index="2"]'
    );
    const toggleSpan = variant1.locator('.pf-v6-c-switch__toggle');
    await toggleSpan.click();

    // Verify the change took effect: variant 1 checkbox should be checked
    const checkbox = variant1.locator('.include-toggle');
    await expect(checkbox).toBeChecked();

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

    // After re-render with a selected variant, httpd.conf is no longer tied.
    // Expand the variant group to check the children.
    const httpdGroupAfter = page.locator('tr.fleet-variant-group', {
      has: page.locator('code', { hasText: '/etc/httpd/conf/httpd.conf' }),
    });
    await httpdGroupAfter.locator('.fleet-variant-toggle').click();
    const childrenRowAfter = page.locator('tr.fleet-variant-children', {
      has: page.locator('tr[data-variant-group="/etc/httpd/conf/httpd.conf"]'),
    });
    await expect(childrenRowAfter).toBeVisible();

    // Verify variant 1 is still checked after re-render
    const variant1After = page.locator(
      'tr[data-variant-group="/etc/httpd/conf/httpd.conf"][data-snap-index="2"]'
    );
    const checkboxAfter = variant1After.locator('.include-toggle');
    await expect(checkboxAfter).toBeChecked();
  });

  test('non-variant config file has no variant toggle', async ({ page }) => {
    // /etc/motd is a regular row, not a variant group
    const motdRow = page.locator(
      'tr[data-snap-section="config"][data-snap-index="7"]'
    );
    await expect(motdRow).toBeVisible();
    await expect(motdRow.locator('code')).toContainText('/etc/motd');

    // It should NOT have a fleet-variant-toggle
    const toggle = motdRow.locator('.fleet-variant-toggle');
    await expect(toggle).toHaveCount(0);
  });

  // ── Three-tier variant hierarchy tests ─────────────────────────────────

  test('tied groups are pre-expanded on page load', async ({ page }) => {
    // /etc/app.conf is a 2-way tie (both include=false, equal fleet counts)
    const appConfGroup = page.locator('tr.fleet-variant-group.variant-tied', {
      has: page.locator('code', { hasText: '/etc/app.conf' }),
    });
    await expect(appConfGroup).toBeVisible();

    // The toggle should already have the .expanded class (pre-expanded by JS)
    const toggle = appConfGroup.locator('.fleet-variant-toggle');
    await expect(toggle).toHaveClass(/expanded/);

    // The children row should be visible without clicking
    const childrenRow = page.locator('tr.fleet-variant-children', {
      has: page.locator('tr[data-variant-group="/etc/app.conf"]'),
    });
    await expect(childrenRow).toBeVisible();
  });

  test('auto-selected groups show blue badge', async ({ page }) => {
    // /etc/nginx/nginx.conf has a clear winner (one include=true) — auto-selected
    const nginxGroup = page.locator('tr.fleet-variant-group', {
      has: page.locator('code', { hasText: '/etc/nginx/nginx.conf' }),
    });
    await expect(nginxGroup).toBeVisible();

    const badge = nginxGroup.locator('.variant-auto-badge');
    await expect(badge).toBeVisible();
    await expect(badge).toContainText('auto-selected');
  });

  test('clean files have no variant badges', async ({ page }) => {
    // /etc/motd is a non-variant row — should have no auto or tie badge
    const motdRow = page.locator(
      'tr[data-snap-section="config"][data-snap-index="7"]'
    );
    await expect(motdRow).toBeVisible();

    await expect(motdRow.locator('.variant-auto-badge')).toHaveCount(0);
    await expect(motdRow.locator('.variant-tie-badge')).toHaveCount(0);
  });

  test('chevron expand and collapse toggle', async ({ page }) => {
    // nginx.conf is a clear winner — starts collapsed (not tied)
    const nginxGroup = page.locator('tr.fleet-variant-group', {
      has: page.locator('code', { hasText: '/etc/nginx/nginx.conf' }),
    });
    const toggle = nginxGroup.locator('.fleet-variant-toggle');

    // Initially collapsed (no .expanded class)
    await expect(toggle).not.toHaveClass(/expanded/);

    // Click to expand
    await toggle.click();
    await expect(toggle).toHaveClass(/expanded/);

    const childrenRow = page.locator('tr.fleet-variant-children', {
      has: page.locator('tr[data-variant-group="/etc/nginx/nginx.conf"]'),
    });
    await expect(childrenRow).toBeVisible();

    // Click again to collapse
    await toggle.click();
    await expect(toggle).not.toHaveClass(/expanded/);
  });

  test('auto badge readable in light mode', async ({ page }) => {
    // Switch to light theme by removing pf-v6-theme-dark from <html>
    await page.evaluate(() => {
      document.documentElement.classList.remove('pf-v6-theme-dark');
    });

    const nginxGroup = page.locator('tr.fleet-variant-group', {
      has: page.locator('code', { hasText: '/etc/nginx/nginx.conf' }),
    });
    const badge = nginxGroup.locator('.variant-auto-badge');
    await expect(badge).toBeVisible();

    // Verify the badge has distinct foreground vs background color
    const styles = await badge.evaluate((el) => {
      const cs = window.getComputedStyle(el);
      return { color: cs.color, background: cs.backgroundColor };
    });
    expect(styles.color).not.toBe(styles.background);
  });
});
