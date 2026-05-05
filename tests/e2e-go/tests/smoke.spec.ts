/**
 * Smoke tests for the inspectah Go port refine server.
 * Validates that the server starts, serves HTML, and the SPA boots.
 */
import { test, expect } from '@playwright/test';
import { waitForBoot, getSidebarSections } from './helpers';

test.describe('Refine server smoke tests', () => {
  test('health endpoint returns ok', async ({ request }) => {
    const resp = await request.get('/api/health');
    expect(resp.ok()).toBeTruthy();
    const body = await resp.json();
    expect(body.status).toBe('ok');
    expect(body.re_render).toBe(true);
  });

  test('report page loads and SPA boots', async ({ page }) => {
    await page.goto('/');
    await waitForBoot(page);

    // Title should be set
    await expect(page).toHaveTitle('inspectah Report');

    // Masthead should show the report heading
    const masthead = page.getByRole('heading', { name: 'inspectah Report', level: 1 });
    await expect(masthead).toBeVisible();
  });

  test('sidebar renders core migration sections', async ({ page }) => {
    await page.goto('/');
    await waitForBoot(page);

    const sections = await getSidebarSections(page);

    // These sections are always present regardless of fixture data.
    // version-changes and nonrpm are conditionally rendered based on
    // snapshot content (version_changes data, non_rpm_software items).
    const alwaysPresent = [
      'overview', 'packages', 'config', 'runtime',
      'containers', 'identity', 'system', 'secrets', 'editor',
    ];
    for (const s of alwaysPresent) {
      expect(sections).toContain(s);
    }

    // Verify ordering: overview is first, editor is last
    expect(sections[0]).toBe('overview');
    expect(sections[sections.length - 1]).toBe('editor');
  });

  test('overview section is active by default', async ({ page }) => {
    await page.goto('/');
    await waitForBoot(page);

    // The overview section heading should be visible
    const heading = page.locator('#heading-overview');
    await expect(heading).toBeVisible();
  });

  test('snapshot API returns valid JSON', async ({ request }) => {
    const resp = await request.get('/api/snapshot');
    expect(resp.ok()).toBeTruthy();
    const body = await resp.json();
    expect(body.snapshot).toBeDefined();
    expect(body.revision).toBeGreaterThanOrEqual(1);
  });

  test('body starts in dark theme', async ({ page }) => {
    await page.goto('/');
    await waitForBoot(page);

    const isDark = await page.evaluate(() =>
      document.body.classList.contains('pf-v6-theme-dark')
    );
    expect(isDark).toBe(true);
  });
});
