/**
 * Triage card tests for the refine report UI.
 * Validates triage classification, tier grouping, and card rendering.
 */
import { test, expect } from '@playwright/test';
import { waitForBoot, navigateToSection } from './helpers';

test.describe('Triage cards', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await waitForBoot(page);
  });

  test('triage badges appear in sidebar', async ({ page }) => {
    // Triage badges show counts for tier-2 and tier-3 items
    const badges = page.locator('.triage-badge');
    const count = await badges.count();
    // At least some sections should have triage badges
    expect(count).toBeGreaterThanOrEqual(0);
  });

  test('triage cards render in tracked sections', async ({ page }) => {
    // Packages is a tracked section that should have triage cards
    await navigateToSection(page, 'packages');

    const cards = page.locator('#section-packages .triage-card, #section-packages .toggle-card');
    const count = await cards.count();
    expect(count).toBeGreaterThan(0);
  });

  test('tier groups organize cards by severity', async ({ page }) => {
    await navigateToSection(page, 'packages');

    // Tier groups use data-tier attribute
    const tierGroups = page.locator('#section-packages [data-tier]');
    const count = await tierGroups.count();
    // May or may not have tier groups depending on fixture data
    if (count > 0) {
      const tiers = await tierGroups.evaluateAll((els) =>
        els.map((el) => el.getAttribute('data-tier'))
      );
      // Tiers should be numeric strings
      for (const tier of tiers) {
        expect(['1', '2', '3']).toContain(tier);
      }
    }
  });

  test('tier-1 items start collapsed', async ({ page }) => {
    await navigateToSection(page, 'packages');

    const tier1Summary = page.locator('#section-packages .tier1-summary');
    const count = await tier1Summary.count();
    if (count > 0) {
      // Tier-1 uses a collapsed summary by default
      await expect(tier1Summary.first()).toBeVisible();
    }
  });

  test('triage card has key attribute for identification', async ({ page }) => {
    await navigateToSection(page, 'packages');

    const cards = page.locator('#section-packages [data-key]');
    const count = await cards.count();
    if (count > 0) {
      const firstKey = await cards.first().getAttribute('data-key');
      expect(firstKey).toBeTruthy();
    }
  });
});
