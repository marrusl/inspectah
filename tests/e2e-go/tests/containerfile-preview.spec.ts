/**
 * Containerfile preview pane tests for the refine report UI.
 *
 * The Go port's preview panel is a static <aside class="preview-panel">
 * with a <pre id="containerfile-preview"> inside. On narrow viewports
 * the panel is hidden via CSS media query. Tests use a wide viewport.
 */
import { test, expect } from '@playwright/test';
import { waitForBoot } from './helpers';

test.describe('Containerfile preview', () => {
  test.use({ viewport: { width: 1600, height: 900 } });

  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await waitForBoot(page);
  });

  test('preview panel exists in DOM', async ({ page }) => {
    const previewPanel = page.locator('aside.preview-panel');
    await expect(previewPanel).toBeAttached();
  });

  test('preview panel has accessible label', async ({ page }) => {
    const previewPanel = page.locator('aside.preview-panel');
    await expect(previewPanel).toHaveAttribute('aria-label', 'Containerfile preview');
  });

  test('preview contains Containerfile content with FROM directive', async ({ page }) => {
    const codeBlock = page.locator('#containerfile-preview code');
    await expect(codeBlock).toBeAttached();
    const text = await codeBlock.textContent();
    expect(text).toContain('FROM');
  });

  test('preview header shows title', async ({ page }) => {
    const header = page.locator('aside.preview-panel .preview-header h2');
    await expect(header).toHaveText('Containerfile Preview');
  });

  test('copy button exists in preview header', async ({ page }) => {
    const copyBtn = page.locator('aside.preview-panel button[aria-label="Copy Containerfile to clipboard"]');
    await expect(copyBtn).toBeAttached();
    await expect(copyBtn).toHaveText('Copy');
  });

  test('changes badge is initially hidden', async ({ page }) => {
    const badge = page.locator('#changes-badge');
    await expect(badge).toBeAttached();
    await expect(badge).toBeHidden();
  });
});
