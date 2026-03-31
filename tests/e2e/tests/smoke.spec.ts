import { test, expect } from '@playwright/test';
import { FLEET_URL, SINGLE_URL, ARCHITECT_URL } from './helpers';

test.describe('Smoke Tests', () => {
  test('fleet refine loads report', async ({ page }) => {
    await page.goto(FLEET_URL);
    const brand = page.locator('.pf-v6-c-masthead__brand');
    await expect(brand).toContainText('yoinkc');
    const dashboard = page.locator('.summary-dashboard');
    await expect(dashboard).toBeVisible();
  });

  test('single-host refine loads report', async ({ page }) => {
    await page.goto(SINGLE_URL);
    const brand = page.locator('.pf-v6-c-masthead__brand');
    await expect(brand).toContainText('yoinkc');
    const dashboard = page.locator('.summary-dashboard');
    await expect(dashboard).toBeVisible();
  });

  test('architect loads UI', async ({ page }) => {
    await page.goto(ARCHITECT_URL);
    // Architect has its own UI — verify it loads something meaningful
    await expect(page).toHaveTitle(/yoinkc|architect/i);
  });

  test('fleet report has 4 summary cards', async ({ page }) => {
    await page.goto(FLEET_URL);
    const cards = page.locator('.summary-card');
    await expect(cards).toHaveCount(4);
  });

  test('single-host has 3 cards and no prevalence slider', async ({ page }) => {
    await page.goto(SINGLE_URL);
    const cards = page.locator('.summary-card');
    await expect(cards).toHaveCount(3);
    const slider = page.locator('.prevalence-slider');
    await expect(slider).toHaveCount(0);
  });
});
