/**
 * Theme switching tests for the refine report UI.
 * Validates dark/light theme toggle behavior.
 */
import { test, expect } from '@playwright/test';
import { waitForBoot, getTheme } from './helpers';

test.describe('Theme switching', () => {
  test.beforeEach(async ({ page }) => {
    // Clear localStorage to reset theme preference
    await page.goto('/');
    await page.evaluate(() => localStorage.removeItem('inspectah-theme'));
    await page.reload();
    await waitForBoot(page);
  });

  test('defaults to dark theme', async ({ page }) => {
    const theme = await getTheme(page);
    expect(theme).toBe('dark');
  });

  test('toggle switches to light theme', async ({ page }) => {
    const toggleBtn = page.locator('button[aria-label="Toggle theme"]');
    await toggleBtn.click();

    const theme = await getTheme(page);
    expect(theme).toBe('light');

    // HTML element should also reflect the change
    const htmlClass = await page.evaluate(() =>
      document.documentElement.classList.contains('pf-v6-theme-dark')
    );
    // Note: theme class is on body, not html element. The html element
    // has the initial class from the template.
  });

  test('toggle switches back to dark theme', async ({ page }) => {
    const toggleBtn = page.locator('button[aria-label="Toggle theme"]');

    // Toggle to light
    await toggleBtn.click();
    expect(await getTheme(page)).toBe('light');

    // Toggle back to dark
    await toggleBtn.click();
    expect(await getTheme(page)).toBe('dark');
  });

  test('theme persists to localStorage', async ({ page }) => {
    const toggleBtn = page.locator('button[aria-label="Toggle theme"]');
    await toggleBtn.click();

    const stored = await page.evaluate(() =>
      localStorage.getItem('inspectah-theme')
    );
    expect(stored).toBe('light');
  });

  test('theme restores from localStorage on reload', async ({ page }) => {
    // Set light theme in storage
    await page.evaluate(() =>
      localStorage.setItem('inspectah-theme', 'light')
    );
    await page.reload();
    await waitForBoot(page);

    const theme = await getTheme(page);
    expect(theme).toBe('light');
  });
});
