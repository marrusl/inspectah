import { test, expect } from '@playwright/test';
import { FLEET_URL } from './helpers';

test.describe('Prevalence Slider', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto(FLEET_URL);
  });

  test('slider exists with initial value', async ({ page }) => {
    const slider = page.locator('#summary-prevalence-slider');
    await expect(slider).toBeVisible();
    const value = await slider.inputValue();
    expect(parseInt(value)).toBeGreaterThan(0);
  });

  test('dragging slider updates card counts', async ({ page }) => {
    const slider = page.locator('#summary-prevalence-slider');
    const prevValue = page.locator('#summary-prevalence-value');

    const initialValue = await prevValue.textContent();

    // Move slider to different threshold
    await slider.fill('75');
    await slider.dispatchEvent('input');

    // Wait for UI update
    await page.waitForTimeout(100);
    const updatedValue = await prevValue.textContent();
    expect(updatedValue).toBe('75%');
    expect(updatedValue).not.toEqual(initialValue);
  });

  test('preview-state border appears when slider deviates', async ({ page }) => {
    const slider = page.locator('#summary-prevalence-slider');
    const scopeCard = page.locator('.summary-card-scope');

    await expect(scopeCard).not.toHaveClass(/preview-state/);

    const currentThreshold = await slider.getAttribute('data-current-threshold');
    const newValue = parseInt(currentThreshold || '50') + 20;
    await slider.fill(String(Math.min(newValue, 100)));
    await slider.dispatchEvent('input');

    await expect(scopeCard).toHaveClass(/preview-state/);
  });

  test('returning slider to original value removes preview-state', async ({ page }) => {
    const slider = page.locator('#summary-prevalence-slider');
    const scopeCard = page.locator('.summary-card-scope');
    const originalValue = await slider.getAttribute('data-current-threshold');

    await slider.fill('100');
    await slider.dispatchEvent('input');
    await expect(scopeCard).toHaveClass(/preview-state/);

    await slider.fill(originalValue || '50');
    await slider.dispatchEvent('input');
    await expect(scopeCard).not.toHaveClass(/preview-state/);
  });

  test('prevalence badges in section headers sync with slider', async ({ page }) => {
    const slider = page.locator('#summary-prevalence-slider');
    const badge = page.locator('.prevalence-badge-value').first();

    const initialBadge = await badge.textContent();

    await slider.fill('90');
    await slider.dispatchEvent('input');

    const updatedBadge = await badge.textContent();
    expect(updatedBadge).toBe('90%');
    expect(updatedBadge).not.toEqual(initialBadge);
  });

  test('slider change enables Re-render button', async ({ page }) => {
    const slider = page.locator('#summary-prevalence-slider');
    const rerender = page.locator('#btn-re-render');

    await slider.fill('100');
    await slider.dispatchEvent('input');

    await expect(rerender).toBeEnabled();
  });
});
