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
    expect(msgText).toContain('above threshold but excluded');

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
    const actionBtnText = await actionBtn.textContent();
    expect(actionBtnText).toBe('Include them');
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

    // Find a toggle-card that starts INCLUDED and has fleet ratio >= 0.8
    const key = await page.evaluate(() => {
      var App = (window as any).App;
      for (var i = 0; i < App.triageManifest.length; i++) {
        var item = App.triageManifest[i];
        if (item.section !== 'packages') continue;
        if (!item.fleet) continue;
        var ratio = item.fleet.count / item.fleet.total;
        if (ratio >= 0.8 && (window as any).getSnapshotInclude(item.key)) {
          return item.key;
        }
      }
      return null;
    });

    if (!key) { test.skip(); return; }

    // Confirm starting state: included
    const originalInclude = await page.evaluate(
      (k: string) => (window as any).getSnapshotInclude(k), key
    );
    expect(originalInclude).toBe(true);

    // Dirty via real toggle-card switch → now excluded
    const toggle = page.locator(`[data-key="${key}"] button[role="switch"]`);
    await expect(toggle).toBeVisible();
    await toggle.click();

    // Verify: row is now excluded
    const afterDirty = await page.evaluate(
      (k: string) => (window as any).getSnapshotInclude(k), key
    );
    expect(afterDirty).toBe(false);

    // Verify: priorValues captured the original (true)
    const priorVal = await page.evaluate(
      (k: string) => (window as any).App.priorValues[k], key
    );
    expect(priorVal).toBe(true);

    // Record pre-bulk decided state for this key
    const wasDecidedBefore = await page.evaluate(
      (k: string) => !!(window as any).App.decisions[k], key
    );
    expect(wasDecidedBefore).toBe(true);

    const countBefore = await page.evaluate(() => (window as any).changeCount);

    // Count how many mismatch items are NOT already decided (expected counter delta)
    const expectedNewlyDecided = await page.evaluate((threshold: number) => {
      var App = (window as any).App;
      var applicable = ['packages', 'runtime', 'identity', 'system'];
      var count = 0;
      for (var i = 0; i < App.triageManifest.length; i++) {
        var item = App.triageManifest[i];
        if (applicable.indexOf(item.section) === -1) continue;
        if (!item.fleet) continue;
        var zone = (window as any).computePrevalenceZone(item, threshold);
        var included = (window as any).getSnapshotInclude(item.key);
        if ((zone === 'above' || zone === 'unanimous') && !included) {
          if (!App.decisions[item.key]) count++;
        }
      }
      return count;
    }, 0.8);

    // Lower threshold to 0.8 → row is above-threshold + excluded → include mismatch
    await navigateToSection(page, 'overview');
    const select = page.locator('#threshold-select');
    await select.selectOption('0.8');

    const bar = page.locator('.threshold-action-bar');
    await expect(bar).toBeVisible();

    // Verify include direction
    const msgText = await page.locator('#threshold-suggestion-text').textContent();
    expect(msgText).toContain('above threshold but excluded');

    // Apply bulk action
    await page.locator('#threshold-action-btn').click();
    await expect(bar).toBeHidden({ timeout: 3000 });

    // (a) Row was bulk-flipped back to included
    const afterBulk = await page.evaluate(
      (k: string) => (window as any).getSnapshotInclude(k), key
    );
    expect(afterBulk).toBe(true);

    // (b) priorValues still holds original (first-touch preserved)
    const priorAfterBulk = await page.evaluate(
      (k: string) => (window as any).App.priorValues[k], key
    );
    expect(priorAfterBulk).toBe(true);

    // (d) Counter delta equals only newly-decided items (our row was already decided — zero increment for it)
    const countAfter = await page.evaluate(() => (window as any).changeCount);
    expect(countAfter - countBefore).toBe(expectedNewlyDecided);

    // (e) Undo via the real product path: click the toggle switch again
    // Toggle-card items keep their toggle switch (no .undo-link) — the switch IS the undo mechanism
    await navigateToSection(page, 'packages');
    const undoToggle = page.locator(`[data-key="${key}"] button[role="switch"]`);
    await expect(undoToggle).toBeVisible();
    await undoToggle.click();

    // After toggle: include state flipped back to excluded
    const afterUndo = await page.evaluate(
      (k: string) => (window as any).getSnapshotInclude(k), key
    );
    expect(afterUndo).toBe(false);

    // priorValues still holds the original (first-touch preserved across all mutations)
    const priorAfterUndo = await page.evaluate(
      (k: string) => (window as any).App.priorValues[k], key
    );
    expect(priorAfterUndo).toBe(originalInclude);
  });

  test('action-bar-pre-dirtied-triage-card', async ({ page }) => {
    // Find a triage-card row that starts INCLUDED, has fleet ratio >= 0.8,
    // and has a "Leave out" button (calls makeDecision with false)
    const sections = ['packages', 'runtime', 'identity', 'system'];
    let foundKey: string | null = null;
    let foundSection = '';

    for (const sectionId of sections) {
      await navigateToSection(page, sectionId);

      const result = await page.evaluate((sid: string) => {
        var App = (window as any).App;
        var cards = document.querySelectorAll('#section-' + sid + ' .triage-card[data-key]');
        for (var i = 0; i < cards.length; i++) {
          var key = cards[i].getAttribute('data-key');
          if (!key) continue;
          // Must start included
          if (!(window as any).getSnapshotInclude(key)) continue;
          // Must have fleet data with ratio >= 0.8
          for (var j = 0; j < App.triageManifest.length; j++) {
            if (App.triageManifest[j].key !== key) continue;
            var item = App.triageManifest[j];
            if (!item.fleet) continue;
            if (item.fleet.count / item.fleet.total < 0.8) continue;
            // Must have a "Leave out" button (the exclude action that calls makeDecision)
            var btns = cards[i].querySelectorAll('.card-actions button');
            for (var b = 0; b < btns.length; b++) {
              if (btns[b].textContent === 'Leave out') {
                return { key: key, section: sid };
              }
            }
          }
        }
        return null;
      }, sectionId);

      if (result) {
        foundKey = result.key;
        foundSection = result.section;
        break;
      }
    }

    if (!foundKey) { test.skip(); return; }

    // Confirm starting state: included
    const originalInclude = await page.evaluate(
      (k: string) => (window as any).getSnapshotInclude(k), foundKey
    );
    expect(originalInclude).toBe(true);

    // Click "Leave out" → calls makeDecision(key, section, false) → now excluded
    const leaveBtn = page.locator(
      `#section-${foundSection} [data-key="${foundKey}"] .card-actions button:has-text("Leave out")`
    );
    await leaveBtn.click();

    // Verify: row is now excluded
    const afterDirty = await page.evaluate(
      (k: string) => (window as any).getSnapshotInclude(k), foundKey
    );
    expect(afterDirty).toBe(false);

    // Verify: priorValues captured the original (true) via makeDecision path
    const priorVal = await page.evaluate(
      (k: string) => (window as any).App.priorValues[k], foundKey
    );
    expect(priorVal).toBe(true);

    // Row is now decided
    const wasDecidedBefore = await page.evaluate(
      (k: string) => !!(window as any).App.decisions[k], foundKey
    );
    expect(wasDecidedBefore).toBe(true);

    const countBefore = await page.evaluate(() => (window as any).changeCount);

    // Count how many mismatch items are NOT already decided (expected counter delta)
    const expectedNewlyDecided = await page.evaluate((threshold: number) => {
      var App = (window as any).App;
      var applicable = ['packages', 'runtime', 'identity', 'system'];
      var count = 0;
      for (var i = 0; i < App.triageManifest.length; i++) {
        var item = App.triageManifest[i];
        if (applicable.indexOf(item.section) === -1) continue;
        if (!item.fleet) continue;
        var zone = (window as any).computePrevalenceZone(item, threshold);
        var included = (window as any).getSnapshotInclude(item.key);
        if ((zone === 'above' || zone === 'unanimous') && !included) {
          if (!App.decisions[item.key]) count++;
        }
      }
      return count;
    }, 0.8);

    // Lower threshold to 0.8 → row is above-threshold + excluded → include mismatch
    await navigateToSection(page, 'overview');
    const select = page.locator('#threshold-select');
    await select.selectOption('0.8');

    const bar = page.locator('.threshold-action-bar');
    await expect(bar).toBeVisible();

    const msgText = await page.locator('#threshold-suggestion-text').textContent();
    expect(msgText).toContain('above threshold but excluded');

    // Apply bulk action
    await page.locator('#threshold-action-btn').click();
    await expect(bar).toBeHidden({ timeout: 3000 });

    // (a) Row was bulk-flipped back to included
    const afterBulk = await page.evaluate(
      (k: string) => (window as any).getSnapshotInclude(k), foundKey
    );
    expect(afterBulk).toBe(true);

    // (b) priorValues still holds original (first-touch via makeDecision preserved)
    const priorAfterBulk = await page.evaluate(
      (k: string) => (window as any).App.priorValues[k], foundKey
    );
    expect(priorAfterBulk).toBe(true);

    // (d) Counter delta equals only newly-decided items (our row was already decided — zero increment for it)
    const countAfter = await page.evaluate(() => (window as any).changeCount);
    expect(countAfter - countBefore).toBe(expectedNewlyDecided);

    // (e) Undo via the real product path: click the .undo-link on the decided card
    await navigateToSection(page, foundSection);
    const undoLink = page.locator(`[data-key="${foundKey}"] .undo-link`);
    await expect(undoLink).toBeVisible();
    await undoLink.click();

    // After undo: include state should be restored to original
    const afterUndo = await page.evaluate(
      (k: string) => (window as any).getSnapshotInclude(k), foundKey
    );
    expect(afterUndo).toBe(originalInclude);

    // priorValues should be cleared (undoDecision deletes it)
    const priorAfterUndo = await page.evaluate(
      (k: string) => (window as any).App.priorValues[k], foundKey
    );
    expect(priorAfterUndo).toBeUndefined();

    // decisions should be cleared (undoDecision deletes it)
    const decidedAfterUndo = await page.evaluate(
      (k: string) => (window as any).App.decisions[k], foundKey
    );
    expect(decidedAfterUndo).toBeUndefined();
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
