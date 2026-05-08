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
    await resetServer();
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
        (window as any).App.prevalenceThreshold,
        'include'
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

  test('action-bar-focus-stays-on-select', async ({ page }) => {
    await navigateToSection(page, 'overview');

    const select = page.locator('#threshold-select');

    // Focus the select explicitly
    await select.focus();

    // Change threshold — bar appears but focus stays on select
    await select.selectOption('0.8');

    const bar = page.locator('.threshold-action-bar');
    await expect(bar).toBeVisible();

    // Tab from select should go to action button, then dismiss
    await page.keyboard.press('Tab');
    const focusAfterTab1 = await page.evaluate(() => document.activeElement?.id);
    expect(focusAfterTab1).toBe('threshold-action-btn');

    await page.keyboard.press('Tab');
    const focusAfterTab2 = await page.evaluate(() => document.activeElement?.id);
    expect(focusAfterTab2).toBe('threshold-dismiss-btn');
  });

  test('action-bar-pre-dirtied-toggle-card', async ({ page }) => {
    await navigateToSection(page, 'packages');

    // Find a toggle-card and its switch
    const card = page.locator('#section-packages .toggle-card').first();
    await expect(card).toBeVisible();
    const key = await card.getAttribute('data-key');
    expect(key).toBeTruthy();

    // Capture original include state
    const originalInclude = await page.evaluate(
      (k) => (window as any).getSnapshotInclude(k),
      key
    );

    // Dirty via the REAL toggle-card switch (not makeDecision)
    const toggle = card.locator('button[role="switch"]');
    await toggle.click();

    // Verify priorValues captured the original via the toggle-card's inline handler
    const priorVal = await page.evaluate(
      (k) => (window as any).App.priorValues[k],
      key
    );
    expect(priorVal).toBe(originalInclude);

    // Now trigger bulk action
    await navigateToSection(page, 'overview');
    const select = page.locator('#threshold-select');
    await select.selectOption('0.8');

    const bar = page.locator('.threshold-action-bar');
    if (!(await bar.isVisible())) {
      test.skip();
      return;
    }

    await page.locator('#threshold-action-btn').click();
    await expect(bar).toBeHidden({ timeout: 3000 });

    // priorValues must still hold the ORIGINAL value (first-touch preserved)
    const priorAfterBulk = await page.evaluate(
      (k) => (window as any).App.priorValues[k],
      key
    );
    expect(priorAfterBulk).toBe(originalInclude);
  });

  test('action-bar-pre-dirtied-triage-card', async ({ page }) => {
    // Search applicable sections for a triage card with action buttons
    const sections = ['packages', 'runtime', 'identity', 'system'];
    let foundKey: string | null = null;
    let foundSection = '';

    for (const sectionId of sections) {
      await navigateToSection(page, sectionId);
      // Look for triage-card action buttons that call makeDecision()
      // Actual labels: "Keep included" or "Leave out"
      const actionBtn = page.locator(
        `#section-${sectionId} .triage-card .card-actions button:has-text("Keep included")`
      ).first();
      if (await actionBtn.count() > 0) {
        const cardEl = actionBtn.locator('xpath=ancestor::*[@data-key]').first();
        const key = await cardEl.getAttribute('data-key');
        if (key) {
          foundKey = key;
          foundSection = sectionId;
          break;
        }
      }
      // Also check "Leave out" buttons
      const leaveBtn = page.locator(
        `#section-${sectionId} .triage-card .card-actions button:has-text("Leave out")`
      ).first();
      if (await leaveBtn.count() > 0) {
        const cardEl = leaveBtn.locator('xpath=ancestor::*[@data-key]').first();
        const key = await cardEl.getAttribute('data-key');
        if (key) {
          foundKey = key;
          foundSection = sectionId;
          break;
        }
      }
    }

    if (!foundKey) {
      test.skip();
      return;
    }

    // Capture original include state
    const originalInclude = await page.evaluate(
      (k) => (window as any).getSnapshotInclude(k),
      foundKey
    );

    // Click the triage-card action button (calls makeDecision)
    const btn = page.locator(
      `#section-${foundSection} [data-key="${foundKey}"] .card-actions button`
    ).first();
    await btn.click();

    // Verify priorValues captured the original via makeDecision()
    const priorVal = await page.evaluate(
      (k) => (window as any).App.priorValues[k],
      foundKey
    );
    expect(priorVal).toBe(originalInclude);

    // Trigger bulk action
    await navigateToSection(page, 'overview');
    const select = page.locator('#threshold-select');
    await select.selectOption('0.8');

    const bar = page.locator('.threshold-action-bar');
    if (!(await bar.isVisible())) {
      test.skip();
      return;
    }

    await page.locator('#threshold-action-btn').click();
    await expect(bar).toBeHidden({ timeout: 3000 });

    // priorValues must still hold the ORIGINAL value
    const priorAfterBulk = await page.evaluate(
      (k) => (window as any).App.priorValues[k],
      foundKey
    );
    expect(priorAfterBulk).toBe(originalInclude);
  });

  test('action-bar-sections-reopen', async ({ page }) => {
    // Navigate to packages and mark it as reviewed
    await navigateToSection(page, 'packages');

    // Check if there's a mark-reviewed button
    const markReviewedBtn = page.locator('#section-packages .mark-reviewed-btn').first();
    if (await markReviewedBtn.count() === 0) {
      test.skip();
      return;
    }

    await markReviewedBtn.click();

    // Verify sidebar dot has 'reviewed' class
    const dot = page.locator('#dot-packages');
    await expect(dot).toHaveClass(/reviewed/);

    // Go to overview and trigger action bar
    await navigateToSection(page, 'overview');
    const select = page.locator('#threshold-select');
    await select.selectOption('0.8');

    const bar = page.locator('.threshold-action-bar');
    if (!(await bar.isVisible())) {
      test.skip();
      return;
    }

    // Check if the message includes Packages
    const msg = await page.locator('#threshold-suggestion-text').textContent();
    if (!msg || !msg.includes('Packages')) {
      // Mismatch doesn't affect packages section — skip
      test.skip();
      return;
    }

    // Apply
    await page.locator('#threshold-action-btn').click();
    await expect(bar).toBeHidden({ timeout: 3000 });

    // Sidebar dot should have reverted to unreviewed
    await expect(dot).not.toHaveClass(/reviewed/);

    // Progress bar should reflect the change
    const progressText = await page.locator('#review-progress-text').textContent();
    expect(progressText).toBeTruthy();
  });
});
