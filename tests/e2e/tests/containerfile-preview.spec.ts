import { test, expect } from '@playwright/test';
import { ARCHITECT_URL } from './helpers';

test.describe('Containerfile Preview', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto(ARCHITECT_URL);
    await expect(page.locator('.layer-card').first()).toBeVisible();
  });

  test('preview button exists on every layer card', async ({ page }) => {
    const layerCards = page.locator('.layer-card');
    const count = await layerCards.count();
    expect(count).toBeGreaterThanOrEqual(4);

    for (let i = 0; i < count; i++) {
      const previewBtn = layerCards.nth(i).locator('.layer-preview-btn');
      await expect(previewBtn).toBeVisible();
      await expect(previewBtn).toContainText('View');
    }
  });

  test('clicking preview on base opens modal with FROM line', async ({ page }) => {
    // Click the preview button on the base layer card
    const basePreviewBtn = page.locator('.layer-card[data-layer="base"] .layer-preview-btn');
    await basePreviewBtn.click();

    // Modal should be visible
    const modal = page.locator('#containerfile-modal');
    await expect(modal).toBeVisible();

    // Modal title should contain "base"
    const modalTitle = page.locator('#cf-modal-title');
    await expect(modalTitle).toContainText('base');

    // Modal body should contain a FROM line with the base image
    const modalBody = page.locator('#cf-modal-body');
    await expect(modalBody).toContainText('FROM');
    await expect(modalBody).toContainText('centos-bootc');
  });

  test('base Containerfile preview shows dnf install for shared packages', async ({ page }) => {
    const basePreviewBtn = page.locator('.layer-card[data-layer="base"] .layer-preview-btn');
    await basePreviewBtn.click();

    const modalBody = page.locator('#cf-modal-body');
    await expect(modalBody).toContainText('dnf install');

    // Should contain some of the shared packages (bare names, not NVRAs)
    await expect(modalBody).toContainText('curl');
    await expect(modalBody).toContainText('jq');
    await expect(modalBody).toContainText('rsync');
  });

  test('derived layer Containerfile shows FROM localhost/base:latest', async ({ page }) => {
    const webPreviewBtn = page.locator('.layer-card[data-layer="web-servers-merged"] .layer-preview-btn');
    await webPreviewBtn.click();

    const modalBody = page.locator('#cf-modal-body');
    await expect(modalBody).toContainText('FROM localhost/base:latest');
  });

  test('derived layer Containerfile shows dnf install for fleet-specific packages', async ({ page }) => {
    const webPreviewBtn = page.locator('.layer-card[data-layer="web-servers-merged"] .layer-preview-btn');
    await webPreviewBtn.click();

    const modalBody = page.locator('#cf-modal-body');
    await expect(modalBody).toContainText('dnf install');
    await expect(modalBody).toContainText('httpd');
    await expect(modalBody).toContainText('php');
  });

  test('closing modal with X button hides it', async ({ page }) => {
    // Open the modal
    const basePreviewBtn = page.locator('.layer-card[data-layer="base"] .layer-preview-btn');
    await basePreviewBtn.click();

    const modal = page.locator('#containerfile-modal');
    await expect(modal).toBeVisible();

    // Click close button
    const closeBtn = page.locator('.cf-modal-close');
    await closeBtn.click();

    // Modal should be hidden
    await expect(modal).not.toBeVisible();
  });

  test('Escape key closes the modal', async ({ page }) => {
    const basePreviewBtn = page.locator('.layer-card[data-layer="base"] .layer-preview-btn');
    await basePreviewBtn.click();

    const modal = page.locator('#containerfile-modal');
    await expect(modal).toBeVisible();

    await page.keyboard.press('Escape');
    await expect(modal).not.toBeVisible();
  });
});
