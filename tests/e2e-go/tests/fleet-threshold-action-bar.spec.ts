/**
 * E2E tests for the fleet threshold action bar feature.
 *
 * This spec covers:
 * - makeDecision() priorValues first-touch preservation
 * - Bulk flip all/none/defaults behavior
 * - Undo interaction with bulk flips
 */
import { test, expect } from '@playwright/test';
import { waitForBoot, navigateToSection, isRefineMode, resetServer } from './helpers';

test.describe('fleet threshold action bar', () => {
  test.beforeAll(async () => { await resetServer(); });
  test.afterAll(async () => { await resetServer(); });

  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await waitForBoot(page);
    const refine = await isRefineMode(page);
    expect(refine).toBe(true);
  });

  test('makeDecision preserves first-touch priorValues', async ({ page }) => {
    await navigateToSection(page, 'packages');

    // Find any item to test with (use first toggle-card)
    const card = page.locator('#section-packages .toggle-card').first();
    await expect(card).toBeVisible();
    const key = await card.getAttribute('data-key');
    expect(key).toBeTruthy();

    // Capture original include state
    const originalInclude = await page.evaluate(
      (k) => (window as any).getSnapshotInclude(k),
      key
    );

    // Call makeDecision() directly twice to test the guard
    // First call: should set priorValues[key] to originalInclude
    await page.evaluate(
      ({k, sectionId}) => (window as any).makeDecision(k, sectionId, false),
      {k: key, sectionId: 'packages'}
    );

    const priorAfterFirst = await page.evaluate(
      (k) => (window as any).App.priorValues[k],
      key
    );
    expect(priorAfterFirst).toBe(originalInclude);

    // Second call: should NOT overwrite priorValues[key] (first-touch preserved)
    // At this point, getSnapshotInclude(key) returns false (the intermediate value)
    // WITHOUT the guard, this would overwrite priorValues[key] = false
    await page.evaluate(
      ({k, sectionId}) => (window as any).makeDecision(k, sectionId, true),
      {k: key, sectionId: 'packages'}
    );

    const priorAfterSecond = await page.evaluate(
      (k) => (window as any).App.priorValues[k],
      key
    );

    // This is the key assertion - priorValues should still be the ORIGINAL value,
    // not the intermediate false value from the first call
    expect(priorAfterSecond).toBe(originalInclude);
  });
});
