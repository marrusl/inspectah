/**
 * Proves the presentation-only threshold invariant:
 * changing the threshold dropdown must NOT mutate include state,
 * increment the change counter, or lose focus.
 *
 * Follows the same harness pattern as rebuild-cycle.spec.ts:
 * resetServer() for state isolation, waitForBoot() + isRefineMode()
 * for readiness.
 */
import { test, expect } from '@playwright/test';
import { waitForBoot, navigateToSection, isRefineMode, resetServer } from './helpers';

test.describe('fleet threshold is presentation-only', () => {
  test.beforeAll(async () => { await resetServer(); });
  test.afterAll(async () => { await resetServer(); });

  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await waitForBoot(page);
    const refine = await isRefineMode(page);
    expect(refine).toBe(true);
  });

  test('threshold change does not dirty state', async ({ page }) => {
    await navigateToSection(page, 'overview');

    const select = page.locator('#threshold-select');
    await expect(select).toBeVisible();

    // #changes-badge is the real dirty-state signal
    const changesBadge = page.locator('#changes-badge');
    await expect(changesBadge).toBeHidden();

    // Capture snapshot byte-for-byte before threshold change
    const snapshotBefore = await page.evaluate(() => {
      return JSON.stringify((window as any).App.snapshot);
    });

    // Change threshold to "Majority (50%)"
    await select.selectOption('0.5');

    // Assert: changes badge stays hidden (no dirty state)
    await expect(changesBadge).toBeHidden();

    // Assert: snapshot is byte-for-byte unchanged (no include mutations)
    const snapshotAfter = await page.evaluate(() => {
      return JSON.stringify((window as any).App.snapshot);
    });
    expect(snapshotAfter).toBe(snapshotBefore);

    // Assert: focus stays on the threshold select
    const focusedId = await page.evaluate(() => document.activeElement?.id);
    expect(focusedId).toBe('threshold-select');

    // Cycle through remaining presets — same invariant holds
    for (const value of ['0.8', '0', '1']) {
      await select.selectOption(value);
      await expect(changesBadge).toBeHidden();

      const snap = await page.evaluate(() => {
        return JSON.stringify((window as any).App.snapshot);
      });
      expect(snap).toBe(snapshotBefore);
    }
  });

  test('threshold change updates presentation on navigation', async ({ page }) => {
    await navigateToSection(page, 'overview');

    const select = page.locator('#threshold-select');
    await expect(select).toBeVisible();

    // Change to "Any presence" (0) — no items should be below threshold
    await select.selectOption('0');

    // Navigate to packages — section was invalidated, re-renders fresh
    await navigateToSection(page, 'packages');
    const heading = page.locator('#heading-packages');
    await expect(heading).toBeVisible();

    // At threshold 0, no items are "below", so no "to review" suffix
    const headingText = await heading.textContent();
    expect(headingText).not.toContain('to review');

    // Navigate back to Overview — dropdown should still show "Any presence"
    await navigateToSection(page, 'overview');
    const selectedValue = await page.locator('#threshold-select').inputValue();
    expect(selectedValue).toBe('0');
  });
});
