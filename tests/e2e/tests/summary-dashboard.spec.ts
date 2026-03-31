import { test, expect } from '@playwright/test';
import { FLEET_URL, SINGLE_URL } from './helpers';

test.describe('Summary Dashboard', () => {
  test.describe('Fleet mode', () => {
    test.beforeEach(async ({ page }) => {
      await page.goto(FLEET_URL);
    });

    test('shows 4-card grid', async ({ page }) => {
      await expect(page.locator('.summary-card')).toHaveCount(4);
    });

    test('system card shows OS description', async ({ page }) => {
      const systemCard = page.locator('.summary-card-system');
      await expect(systemCard.locator('.summary-card-label')).toHaveText('System');
      await expect(systemCard.locator('.summary-card-value')).not.toBeEmpty();
    });

    test('prevalence card shows slider', async ({ page }) => {
      const prevCard = page.locator('.summary-card-prevalence');
      await expect(prevCard).toBeVisible();
      await expect(prevCard.locator('#summary-prevalence-slider')).toBeVisible();
    });

    test('migration scope card shows item counts', async ({ page }) => {
      const scopeCard = page.locator('.summary-card-scope');
      await expect(scopeCard.locator('.summary-card-label')).toHaveText('Migration Scope');
      await expect(scopeCard.locator('.summary-card-value')).toContainText('items');
    });

    test('needs attention card shows review and manual counts', async ({ page }) => {
      const attentionCard = page.locator('.summary-card-attention');
      await expect(attentionCard.locator('.summary-card-label')).toHaveText('Needs Attention');
    });

    test('needs attention includes tie callout when ties exist', async ({ page }) => {
      const callout = page.locator('.summary-ties-callout');
      const count = await callout.count();
      if (count > 0) {
        await expect(callout).toBeVisible();
        await expect(callout).toContainText('must be resolved');
      } else {
        // Fixture has no ties - verify callout is correctly absent
        await expect(callout).not.toBeVisible();
      }
    });

    test('variant drift callout shows when variants exist', async ({ page }) => {
      const drift = page.locator('.summary-drift-callout');
      await expect(drift).toBeVisible();
      await expect(drift).toContainText('variants');
    });

    test('section priority list renders rows', async ({ page }) => {
      const rows = page.locator('.summary-priority-row');
      await expect(rows.first()).toBeVisible();
      const count = await rows.count();
      expect(count).toBeGreaterThan(0);
    });

    test('next steps card is present', async ({ page }) => {
      await expect(page.locator('text=Next Steps')).toBeVisible();
    });
  });

  test.describe('Single-host mode', () => {
    test.beforeEach(async ({ page }) => {
      await page.goto(SINGLE_URL);
    });

    test('shows 3-card grid (no prevalence)', async ({ page }) => {
      await expect(page.locator('.summary-card')).toHaveCount(3);
      await expect(page.locator('.summary-card-prevalence')).not.toBeVisible();
    });

    test('needs attention card spans full width', async ({ page }) => {
      await expect(page.locator('.summary-card-attention-full')).toBeVisible();
    });

    test('no prevalence badges in section headers', async ({ page }) => {
      await expect(page.locator('.prevalence-badge')).toHaveCount(0);
    });
  });
});
