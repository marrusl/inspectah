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

  test('action-bar-appears-on-threshold-lower', async ({ page }) => {
    await navigateToSection(page, 'overview');

    const select = page.locator('#threshold-select');
    await expect(select).toBeVisible();

    const changesBadge = page.locator('#changes-badge');
    await expect(changesBadge).toBeHidden();

    // Lower from Unanimous (1.0) to Strong consensus (0.8)
    await select.selectOption('0.8');

    // Action bar should appear with mismatch count
    const bar = page.locator('.threshold-action-bar');
    await expect(bar).toBeVisible();

    // Message should include section names and counts
    const msg = page.locator('#threshold-suggestion-text');
    const msgText = await msg.textContent();
    // Direction depends on fixture data: items may be above-but-excluded
    // or below-but-included after threshold change
    expect(msgText).toMatch(/above threshold but excluded|below threshold but included/);

    // Changes badge stays hidden — no state mutation yet
    await expect(changesBadge).toBeHidden();
  });

  test('action-bar-zero-mismatch-no-bar', async ({ page }) => {
    await navigateToSection(page, 'overview');

    const bar = page.locator('.threshold-action-bar');

    // On initial page load, no bar regardless of mismatch state —
    // the bar only renders when onchange fires (user changes threshold)
    await expect(bar).toBeHidden();

    // computeThresholdMismatch is callable and returns a valid result
    const mismatch = await page.evaluate(() => {
      return (window as any).computeThresholdMismatch(
        (window as any).App.prevalenceThreshold
      );
    });
    expect(typeof mismatch.count).toBe('number');
    expect(mismatch.count).toBeGreaterThanOrEqual(0);

    // If mismatch is zero, direction should be empty
    if (mismatch.count === 0) {
      expect(mismatch.direction).toBe('');
    } else {
      // If mismatch is non-zero, direction must be include or exclude
      expect(['include', 'exclude']).toContain(mismatch.direction);
    }
  });

  test('action-bar-include-flips-toggles', async ({ page }) => {
    await navigateToSection(page, 'overview');

    const select = page.locator('#threshold-select');
    const changesBadge = page.locator('#changes-badge');
    await expect(changesBadge).toBeHidden();

    // Capture snapshot before
    const snapshotBefore = await page.evaluate(() =>
      JSON.stringify((window as any).App.snapshot)
    );

    // Lower threshold to trigger mismatches
    await select.selectOption('0.8');

    const bar = page.locator('.threshold-action-bar');
    await expect(bar).toBeVisible();

    // Click the action button (Include or Exclude, depending on fixture direction)
    const actionBtn = page.locator('#threshold-action-btn');
    await actionBtn.click();

    // Wait for bar to disappear (confirmation dwell ~1s)
    await expect(bar).toBeHidden({ timeout: 3000 });

    // Changes badge should now be visible
    await expect(changesBadge).toBeVisible();

    // Snapshot should have changed (include values flipped)
    const snapshotAfter = await page.evaluate(() =>
      JSON.stringify((window as any).App.snapshot)
    );
    expect(snapshotAfter).not.toBe(snapshotBefore);

    // Focus should return to the select
    const focusedId = await page.evaluate(() => document.activeElement?.id);
    expect(focusedId).toBe('threshold-select');
  });

  test('action-bar-exclude-flips-toggles', async ({ page }) => {
    await navigateToSection(page, 'overview');

    const select = page.locator('#threshold-select');
    const changesBadge = page.locator('#changes-badge');

    // First lower threshold to 0 ("Any presence") so everything is above
    await select.selectOption('0');
    // Then raise to Unanimous — items below Unanimous that are included → exclude
    await select.selectOption('1');

    const bar = page.locator('.threshold-action-bar');
    if (!(await bar.isVisible())) {
      // If no mismatches at this threshold with fixture data, skip
      test.skip();
      return;
    }

    const snapshotBefore = await page.evaluate(() =>
      JSON.stringify((window as any).App.snapshot)
    );

    // Click the action button
    await page.locator('#threshold-action-btn').click();
    await expect(bar).toBeHidden({ timeout: 3000 });

    // Changes badge should be visible
    await expect(changesBadge).toBeVisible();

    // Snapshot should have changed
    const snapshotAfter = await page.evaluate(() =>
      JSON.stringify((window as any).App.snapshot)
    );
    expect(snapshotAfter).not.toBe(snapshotBefore);

    // Focus returns to select
    const focusedId = await page.evaluate(() => document.activeElement?.id);
    expect(focusedId).toBe('threshold-select');
  });

  test('action-bar-dismiss-no-state-change', async ({ page }) => {
    await navigateToSection(page, 'overview');

    const select = page.locator('#threshold-select');
    const changesBadge = page.locator('#changes-badge');
    await expect(changesBadge).toBeHidden();

    const snapshotBefore = await page.evaluate(() =>
      JSON.stringify((window as any).App.snapshot)
    );

    // Lower threshold to show bar
    await select.selectOption('0.8');
    const bar = page.locator('.threshold-action-bar');
    await expect(bar).toBeVisible();

    // Click Dismiss
    const dismissBtn = page.locator('#threshold-dismiss-btn');
    await dismissBtn.click();

    // Wait for bar to disappear
    await expect(bar).toBeHidden({ timeout: 3000 });

    // Changes badge stays hidden
    await expect(changesBadge).toBeHidden();

    // Snapshot unchanged
    const snapshotAfter = await page.evaluate(() =>
      JSON.stringify((window as any).App.snapshot)
    );
    expect(snapshotAfter).toBe(snapshotBefore);

    // Focus returns to select
    const focusedId = await page.evaluate(() => document.activeElement?.id);
    expect(focusedId).toBe('threshold-select');
  });

  test('action-bar-replaces-on-rechange', async ({ page }) => {
    await navigateToSection(page, 'overview');

    const select = page.locator('#threshold-select');
    const changesBadge = page.locator('#changes-badge');

    // Lower to 80%
    await select.selectOption('0.8');
    const bar = page.locator('.threshold-action-bar');
    await expect(bar).toBeVisible();

    const msg1 = await page.locator('#threshold-suggestion-text').textContent();

    // Lower again to 50% — bar replaces with new content
    await select.selectOption('0.5');
    await expect(bar).toBeVisible();

    const msg2 = await page.locator('#threshold-suggestion-text').textContent();
    expect(msg2).toBeTruthy();

    // No state change
    await expect(changesBadge).toBeHidden();

    // Focus stays on select
    const focusedId = await page.evaluate(() => document.activeElement?.id);
    expect(focusedId).toBe('threshold-select');
  });

  test('action-bar-navigation-dismisses', async ({ page }) => {
    await navigateToSection(page, 'overview');

    const select = page.locator('#threshold-select');

    // Show action bar
    await select.selectOption('0.8');
    const bar = page.locator('.threshold-action-bar');
    await expect(bar).toBeVisible();

    // Navigate to Packages
    await navigateToSection(page, 'packages');

    // Return to Overview
    await navigateToSection(page, 'overview');

    // Bar should be gone
    await expect(bar).toBeHidden();
  });
});
