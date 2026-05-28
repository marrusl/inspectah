# Threshold Action Bar Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a threshold action bar to the fleet report's Overview section that offers bulk include/exclude when the prevalence threshold classification disagrees with toggle state.

**Architecture:** The action bar appears below the threshold dropdown on Overview when the user changes the threshold and a mismatch exists between classification zones and toggle states. It uses a dedicated `applyThresholdSuggestion()` bulk-flip function (not `makeDecision()` in a loop) with first-touch `priorValues` semantics. The bar is DOM-only (no new Go backend changes) and is removed on navigation away from Overview.

**Tech Stack:** Vanilla JS (ES5 compat, `var` not `let`/`const`), inline in report.html. Playwright E2E tests in TypeScript.

**Spec:** `/Users/mrussell/Work/bootc-migration/inspectah/docs/specs/proposed/2026-05-07-threshold-action-bar-design.md`

---

### Task 1: Harmonize makeDecision() to first-touch priorValues

**Files:**
- Modify: `cmd/inspectah/internal/renderer/static/report.html:4267`

This is a prerequisite for the action bar. The current `makeDecision()` overwrites `App.priorValues[key]` unconditionally; it must only seed when undefined so undo always returns to the original pre-first-touch state.

- [ ] **Step 1: Write the failing E2E test**

Create `tests/e2e-go/tests/fleet-threshold-action-bar.spec.ts` with the test harness and the first test. This test proves the priorValues guard works: toggle an item, then toggle it again — priorValues should hold the *original* value, not the intermediate one.

```ts
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

    // Find the first toggle-card switch
    const toggle = page.locator('#section-packages button[role="switch"]').first();
    await expect(toggle).toBeVisible();

    // Read the item key from the card's data-key
    const card = page.locator('#section-packages [data-key]').first();
    const key = await card.getAttribute('data-key');
    expect(key).toBeTruthy();

    // Capture original include state
    const originalInclude = await page.evaluate(
      (k) => (window as any).App.snapshot.rpm.packages_added.find(
        (p: any) => 'pkg-' + p.name + '-' + p.arch === k
      )?.include !== false,
      key
    );

    // First toggle — sets priorValues[key] to originalInclude
    await toggle.click();

    const priorAfterFirst = await page.evaluate(
      (k) => (window as any).App.priorValues[k],
      key
    );
    expect(priorAfterFirst).toBe(originalInclude);

    // Second toggle — priorValues[key] must NOT change (first-touch preserved)
    // Re-locate toggle after re-render
    const toggle2 = page.locator(`[data-key="${key}"] button[role="switch"]`);
    if (await toggle2.count() > 0) {
      await toggle2.click();

      const priorAfterSecond = await page.evaluate(
        (k) => (window as any).App.priorValues[k],
        key
      );
      expect(priorAfterSecond).toBe(originalInclude);
    }
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && npx playwright test tests/e2e-go/tests/fleet-threshold-action-bar.spec.ts --grep "makeDecision preserves first-touch" 2>&1 | tail -20`

Expected: FAIL — the second toggle overwrites priorValues with the intermediate value because `makeDecision()` lacks the guard.

- [ ] **Step 3: Implement the first-touch guard**

In `cmd/inspectah/internal/renderer/static/report.html`, change line 4267 from:

```js
  App.priorValues[key] = getSnapshotInclude(key);
```

to:

```js
  if (App.priorValues[key] === undefined) {
    App.priorValues[key] = getSnapshotInclude(key);
  }
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && npx playwright test tests/e2e-go/tests/fleet-threshold-action-bar.spec.ts --grep "makeDecision preserves first-touch" 2>&1 | tail -20`

Expected: PASS

- [ ] **Step 5: Run existing fleet threshold test to verify no regression**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && npx playwright test tests/e2e-go/tests/fleet-threshold-no-dirty-state.spec.ts 2>&1 | tail -20`

Expected: PASS — the guard only prevents overwrite; first-time behavior unchanged.

- [ ] **Step 6: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/renderer/static/report.html tests/e2e-go/tests/fleet-threshold-action-bar.spec.ts
git commit -m "fix(report): harmonize makeDecision() to first-touch priorValues

The existing makeDecision() unconditionally overwrites App.priorValues[key],
which breaks per-item undo when multiple mutations hit the same key. Add an
'if undefined' guard so the first touch is preserved as the restore point.

Prerequisite for the threshold action bar bulk-flip contract.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 2: Action bar CSS + mismatch computation + bar rendering

**Files:**
- Modify: `cmd/inspectah/internal/renderer/static/report.html` (CSS around line 1976, JS around line 2700, renderOverview around line 2751)

- [ ] **Step 1: Write the failing E2E tests**

Append to `tests/e2e-go/tests/fleet-threshold-action-bar.spec.ts`, inside the existing `test.describe` block:

```ts
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

    const select = page.locator('#threshold-select');

    // At Unanimous (default), all items are at or above threshold
    // and their toggles match — no mismatch expected
    // (depends on fixture data; if unanimous items are all included, no bar)
    const bar = page.locator('.threshold-action-bar');

    // Change to "Any presence" (0) — everything is above threshold
    await select.selectOption('0');
    // If all items are already included, no mismatch
    // Allow brief render time
    await page.waitForTimeout(100);
    // Bar should not appear since include states match classification
    const barCount = await bar.count();
    if (barCount > 0) {
      // If bar appeared, it means there are excluded items above threshold 0
      // which is valid — the test should check no bar ONLY when mismatch is 0
      // For this fixture, verify by checking the computed mismatch count
      const mismatchCount = await page.evaluate(() => {
        const App = (window as any).App;
        let count = 0;
        const applicable = ['packages', 'runtime', 'identity', 'system'];
        for (const item of App.triageManifest) {
          if (!applicable.includes(item.section)) continue;
          if (!item.fleet) continue;
          const zone = item.fleet.count === item.fleet.total ? 'unanimous'
            : (item.fleet.count / item.fleet.total) >= 0 ? 'above' : 'below';
          const included = App.snapshot.rpm?.packages_added?.find(
            (p: any) => 'pkg-' + p.name + '-' + p.arch === item.key
          )?.include !== false;
          // At threshold 0, zone is never "below", so only exclude-direction mismatches
          // would be below-threshold + included, which can't happen at threshold 0
        }
        return count;
      });
    }

    // Simpler assertion: change threshold back to 1.0 (Unanimous)
    // At default, there should be no mismatch
    await select.selectOption('1');
    await page.waitForTimeout(100);
    await expect(bar).toBeHidden();
  });
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && npx playwright test tests/e2e-go/tests/fleet-threshold-action-bar.spec.ts --grep "action-bar-appears|action-bar-zero" 2>&1 | tail -20`

Expected: FAIL — `.threshold-action-bar` element does not exist.

- [ ] **Step 3: Add CSS for the action bar**

In `cmd/inspectah/internal/renderer/static/report.html`, after the `.threshold-select` CSS block (around line 1976), add:

```css
    .threshold-action-bar {
      display: flex;
      align-items: center;
      gap: 0.75rem;
      padding: 0.5rem 0.75rem;
      margin-top: 0.5rem;
      margin-bottom: 0.5rem;
      border-radius: 6px;
      background: var(--pf-t--global--color--status--info--default, #2b9af3);
      color: #fff;
      font-size: 0.85rem;
      line-height: 1.4;
    }
    .threshold-action-bar button {
      padding: 0.25rem 0.75rem;
      border: 1px solid rgba(255,255,255,0.4);
      border-radius: 4px;
      background: rgba(255,255,255,0.15);
      color: #fff;
      font-size: 0.8rem;
      cursor: pointer;
      white-space: nowrap;
    }
    .threshold-action-bar button:hover {
      background: rgba(255,255,255,0.3);
    }
    .threshold-action-bar button:disabled {
      opacity: 0.5;
      cursor: default;
    }
    .threshold-action-bar button.dismiss-btn {
      background: transparent;
      border-color: rgba(255,255,255,0.3);
    }
```

- [ ] **Step 4: Add helper functions**

In `cmd/inspectah/internal/renderer/static/report.html`, after the `invalidateApplicableSections()` function (around line 2709), add:

```js
function getSectionLabel(sectionId) {
  for (var i = 0; i < MIGRATION_SECTIONS.length; i++) {
    if (MIGRATION_SECTIONS[i].id === sectionId) return MIGRATION_SECTIONS[i].label;
  }
  return sectionId;
}

function computeThresholdMismatch(threshold) {
  var result = {count: 0, direction: '', perSection: {}};
  var includeCount = 0;
  var excludeCount = 0;
  var applicableSections = ['packages', 'runtime', 'identity', 'system'];

  for (var i = 0; i < App.triageManifest.length; i++) {
    var item = App.triageManifest[i];
    if (applicableSections.indexOf(item.section) === -1) continue;
    if (!item.fleet) continue;

    var zone = computePrevalenceZone(item, threshold);
    var included = getSnapshotInclude(item.key);

    if ((zone === 'above' || zone === 'unanimous') && !included) {
      includeCount++;
      if (!result.perSection[item.section]) result.perSection[item.section] = 0;
      result.perSection[item.section]++;
    } else if (zone === 'below' && included) {
      excludeCount++;
      if (!result.perSection[item.section]) result.perSection[item.section] = 0;
      result.perSection[item.section]++;
    }
  }

  if (includeCount > 0 && excludeCount === 0) {
    result.count = includeCount;
    result.direction = 'include';
  } else if (excludeCount > 0 && includeCount === 0) {
    result.count = excludeCount;
    result.direction = 'exclude';
  } else if (includeCount > 0 && excludeCount > 0) {
    // Mixed — pick the larger direction
    if (includeCount >= excludeCount) {
      result.count = includeCount;
      result.direction = 'include';
    } else {
      result.count = excludeCount;
      result.direction = 'exclude';
    }
  }

  return result;
}

function formatMismatchMessage(mismatch) {
  var sections = [];
  for (var sectionId in mismatch.perSection) {
    sections.push(getSectionLabel(sectionId) + ' (' + mismatch.perSection[sectionId] + ')');
  }
  var sectionList = sections.join(', ');
  if (mismatch.direction === 'include') {
    return mismatch.count + ' item' + (mismatch.count !== 1 ? 's' : '') +
      ' above threshold but excluded: ' + sectionList;
  } else {
    return mismatch.count + ' item' + (mismatch.count !== 1 ? 's' : '') +
      ' below threshold but included: ' + sectionList;
  }
}

function renderThresholdActionBar(container, mismatch) {
  // Remove existing bar if present
  var existing = document.getElementById('threshold-action-bar');
  if (existing) existing.remove();

  if (mismatch.count === 0) {
    // Clear any lingering announcement
    var liveRegion = document.getElementById('threshold-suggestion-text');
    if (liveRegion) liveRegion.textContent = '';
    return;
  }

  var bar = document.createElement('div');
  bar.className = 'threshold-action-bar';
  bar.id = 'threshold-action-bar';

  var msgSpan = document.createElement('span');
  msgSpan.setAttribute('aria-live', 'polite');
  msgSpan.id = 'threshold-suggestion-text';
  msgSpan.textContent = formatMismatchMessage(mismatch);
  bar.appendChild(msgSpan);

  var actionLabel = mismatch.direction === 'include' ? 'Include them' : 'Exclude them';
  var actionBtn = document.createElement('button');
  actionBtn.type = 'button';
  actionBtn.textContent = actionLabel;
  actionBtn.id = 'threshold-action-btn';
  actionBtn.onclick = function() {
    applyThresholdSuggestion(mismatch.direction, App.prevalenceThreshold);
    dismissThresholdActionBar('Applied: ' + actionLabel.toLowerCase().replace('them', mismatch.count + ' items'));
  };
  bar.appendChild(actionBtn);

  var dismissBtn = document.createElement('button');
  dismissBtn.type = 'button';
  dismissBtn.textContent = 'Dismiss';
  dismissBtn.className = 'dismiss-btn';
  dismissBtn.id = 'threshold-dismiss-btn';
  dismissBtn.onclick = function() {
    dismissThresholdActionBar('Suggestion dismissed');
  };
  bar.appendChild(dismissBtn);

  // Insert after the threshold dropdown div
  var thresholdDiv = container.querySelector('#threshold-select');
  if (thresholdDiv && thresholdDiv.parentElement) {
    thresholdDiv.parentElement.insertAdjacentElement('afterend', bar);
  } else {
    container.appendChild(bar);
  }
}

function dismissThresholdActionBar(announcement) {
  var bar = document.getElementById('threshold-action-bar');
  if (!bar) return;

  // Disable buttons during dwell
  var buttons = bar.querySelectorAll('button');
  for (var i = 0; i < buttons.length; i++) {
    buttons[i].disabled = true;
  }

  // Announce the result
  var msgSpan = document.getElementById('threshold-suggestion-text');
  if (msgSpan) {
    msgSpan.textContent = announcement;
  }

  // Brief pause for SR to read announcement, then remove
  setTimeout(function() {
    var barEl = document.getElementById('threshold-action-bar');
    if (barEl) barEl.remove();
    // Return focus to the threshold select
    var select = document.getElementById('threshold-select');
    if (select) select.focus();
  }, 1000);
}
```

- [ ] **Step 5: Wire threshold change handler to show action bar**

In `cmd/inspectah/internal/renderer/static/report.html`, modify the `thresholdSelect.onchange` handler (around line 2751). Change:

```js
    thresholdSelect.onchange = function() {
      App.prevalenceThreshold = parseFloat(this.value);
      invalidateApplicableSections();
      updateAllBadges();
      updateSidebarThreshold();
    };
```

to:

```js
    thresholdSelect.onchange = function() {
      App.prevalenceThreshold = parseFloat(this.value);
      invalidateApplicableSections();
      updateAllBadges();
      updateSidebarThreshold();
      // Compute mismatch and show/update action bar
      var mismatch = computeThresholdMismatch(App.prevalenceThreshold);
      renderThresholdActionBar(container, mismatch);
    };
```

- [ ] **Step 6: Stub applyThresholdSuggestion (needed for bar to render without errors)**

After the `dismissThresholdActionBar` function, add a stub that will be fully implemented in Task 3:

```js
function applyThresholdSuggestion(direction, threshold) {
  // Stub — full implementation in Task 3
}
```

- [ ] **Step 7: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && npx playwright test tests/e2e-go/tests/fleet-threshold-action-bar.spec.ts --grep "action-bar-appears|action-bar-zero" 2>&1 | tail -20`

Expected: PASS

- [ ] **Step 8: Run existing fleet threshold test for regression check**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && npx playwright test tests/e2e-go/tests/fleet-threshold-no-dirty-state.spec.ts 2>&1 | tail -20`

Expected: PASS — the threshold change handler still doesn't mutate state; the bar only appears, doesn't act.

- [ ] **Step 9: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/renderer/static/report.html tests/e2e-go/tests/fleet-threshold-action-bar.spec.ts
git commit -m "feat(report): add threshold action bar DOM and mismatch computation

Render a threshold action bar below the dropdown when the user changes
the prevalence threshold and a mismatch exists between classification
zones and toggle states. The bar shows per-section counts and offers
Include/Exclude and Dismiss buttons (apply is stubbed for next commit).

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 3: Implement applyThresholdSuggestion() bulk flip

**Files:**
- Modify: `cmd/inspectah/internal/renderer/static/report.html` (replace the stub)
- Modify: `tests/e2e-go/tests/fleet-threshold-action-bar.spec.ts`

- [ ] **Step 1: Write the failing E2E tests**

Append to `tests/e2e-go/tests/fleet-threshold-action-bar.spec.ts`, inside the `test.describe` block:

```ts
  test('action-bar-include-flips-toggles', async ({ page }) => {
    await navigateToSection(page, 'overview');

    const select = page.locator('#threshold-select');
    const changesBadge = page.locator('#changes-badge');
    await expect(changesBadge).toBeHidden();

    // Capture snapshot before
    const snapshotBefore = await page.evaluate(() =>
      JSON.stringify((window as any).App.snapshot)
    );

    // Lower threshold to trigger include-direction mismatches
    await select.selectOption('0.8');

    const bar = page.locator('.threshold-action-bar');
    await expect(bar).toBeVisible();

    // Click Include button
    const actionBtn = page.locator('#threshold-action-btn');
    await actionBtn.click();

    // Wait for bar to disappear (confirmation dwell)
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

    // First lower threshold to 0 (everything above), then raise to create exclude mismatches
    // Lower to Any presence first
    await select.selectOption('0');
    // Now raise to Unanimous — items below Unanimous that are included get exclude offer
    await select.selectOption('1');

    const bar = page.locator('.threshold-action-bar');
    // Check if bar appeared with exclude direction
    const barVisible = await bar.isVisible();
    if (!barVisible) {
      // If no mismatches at this threshold with current fixture data, skip
      test.skip();
      return;
    }

    const msgText = await page.locator('#threshold-suggestion-text').textContent();
    expect(msgText).toContain('below threshold but included');

    // Click Exclude button
    const actionBtn = page.locator('#threshold-action-btn');
    await actionBtn.click();

    // Wait for bar to disappear
    await expect(bar).toBeHidden({ timeout: 3000 });

    // Changes badge should be visible
    await expect(changesBadge).toBeVisible();

    // Focus should return to the select
    const focusedId = await page.evaluate(() => document.activeElement?.id);
    expect(focusedId).toBe('threshold-select');
  });
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && npx playwright test tests/e2e-go/tests/fleet-threshold-action-bar.spec.ts --grep "action-bar-include-flips|action-bar-exclude-flips" 2>&1 | tail -20`

Expected: FAIL — the stub `applyThresholdSuggestion()` does nothing; snapshot unchanged, changes badge stays hidden.

- [ ] **Step 3: Replace the stub with the full implementation**

In `cmd/inspectah/internal/renderer/static/report.html`, replace the `applyThresholdSuggestion` stub with:

```js
function applyThresholdSuggestion(direction, threshold) {
  if (App.mode === 'static') return;

  var affectedSections = {};
  var newDecisionCount = 0;
  var applicableSections = ['packages', 'runtime', 'identity', 'system'];

  for (var i = 0; i < App.triageManifest.length; i++) {
    var item = App.triageManifest[i];
    if (applicableSections.indexOf(item.section) === -1) continue;
    if (!item.fleet) continue;

    var zone = computePrevalenceZone(item, threshold);
    var included = getSnapshotInclude(item.key);
    var shouldFlip = false;

    if (direction === 'include' && (zone === 'above' || zone === 'unanimous') && !included) {
      shouldFlip = true;
    } else if (direction === 'exclude' && zone === 'below' && included) {
      shouldFlip = true;
    }

    if (shouldFlip) {
      // First-touch priorValues: only seed when undefined
      if (App.priorValues[item.key] === undefined) {
        App.priorValues[item.key] = included;
      }
      // Only count newly-decided items
      if (!App.decisions[item.key]) {
        newDecisionCount++;
      }
      App.decisions[item.key] = true;
      updateSnapshotInclude(item.key, direction === 'include');
      affectedSections[item.section] = true;
    }
  }

  // Increment change counter once by newly-decided count
  for (var n = 0; n < newDecisionCount; n++) {
    incrementChangeCounter();
  }

  // Re-render affected sections and reset review states
  for (var sectionId in affectedSections) {
    var container = document.getElementById('section-' + sectionId);
    if (container) container.innerHTML = '';
    reopenReviewedSection(sectionId);
    updateSidebarDot(sectionId);
  }

  updateAllBadges();
  updateProgressBar();
  scheduleAutosave();
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && npx playwright test tests/e2e-go/tests/fleet-threshold-action-bar.spec.ts --grep "action-bar-include-flips|action-bar-exclude-flips" 2>&1 | tail -20`

Expected: PASS

- [ ] **Step 5: Run all action bar tests so far**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && npx playwright test tests/e2e-go/tests/fleet-threshold-action-bar.spec.ts 2>&1 | tail -20`

Expected: All tests PASS.

- [ ] **Step 6: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/renderer/static/report.html tests/e2e-go/tests/fleet-threshold-action-bar.spec.ts
git commit -m "feat(report): implement applyThresholdSuggestion() bulk flip

Dedicated bulk path that flips include state for mismatched items.
Uses first-touch priorValues semantics, only increments change counter
for newly-decided items, reopens affected reviewed sections, and
schedules a single autosave.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 4: Dismiss, replace, and navigation lifecycle

**Files:**
- Modify: `cmd/inspectah/internal/renderer/static/report.html:2421` (navigateTo function)
- Modify: `tests/e2e-go/tests/fleet-threshold-action-bar.spec.ts`

- [ ] **Step 1: Write the failing E2E tests**

Append to `tests/e2e-go/tests/fleet-threshold-action-bar.spec.ts`:

```ts
  test('action-bar-dismiss-no-state-change', async ({ page }) => {
    await navigateToSection(page, 'overview');

    const select = page.locator('#threshold-select');
    const changesBadge = page.locator('#changes-badge');
    await expect(changesBadge).toBeHidden();

    // Capture snapshot before
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

    // Changes badge stays hidden — no state mutation
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
    // Messages should differ (different counts at different thresholds)
    // At minimum, bar is still visible and updated
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

    // Bar should be gone — does not reappear without new threshold change
    await expect(bar).toBeHidden();
  });
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && npx playwright test tests/e2e-go/tests/fleet-threshold-action-bar.spec.ts --grep "action-bar-dismiss|action-bar-replaces|action-bar-navigation" 2>&1 | tail -20`

Expected: `action-bar-navigation-dismisses` FAIL — `navigateTo()` doesn't remove the bar. The dismiss and replace tests may already pass from the existing implementation, but navigation is missing.

- [ ] **Step 3: Add navigation cleanup to navigateTo()**

In `cmd/inspectah/internal/renderer/static/report.html`, in the `navigateTo()` function (around line 2421), add the action bar cleanup at the top of the function, after the editor dirty-state guard (after line ~2431, before `App.activeSection = sectionId`):

```js
  // Remove threshold action bar when leaving overview
  var actionBar = document.getElementById('threshold-action-bar');
  if (actionBar) actionBar.remove();
```

The full insertion point is after the editor mode check and before `App.activeSection = sectionId;`. The lines around that area look like:

```js
  if (editorState.mode.startsWith('editing')) exitEditModeClean();

  // Remove threshold action bar when leaving overview
  var actionBar = document.getElementById('threshold-action-bar');
  if (actionBar) actionBar.remove();

  App.activeSection = sectionId;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && npx playwright test tests/e2e-go/tests/fleet-threshold-action-bar.spec.ts --grep "action-bar-dismiss|action-bar-replaces|action-bar-navigation" 2>&1 | tail -20`

Expected: PASS

- [ ] **Step 5: Run all tests**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && npx playwright test tests/e2e-go/tests/fleet-threshold-action-bar.spec.ts 2>&1 | tail -20`

Expected: All PASS.

- [ ] **Step 6: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/renderer/static/report.html tests/e2e-go/tests/fleet-threshold-action-bar.spec.ts
git commit -m "feat(report): add dismiss, replace, and navigation lifecycle

Dismiss button removes bar without state change. Threshold re-change
replaces bar content in place. Navigation away from Overview removes
the bar from the DOM; it does not reappear on return without a new
threshold change.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 5: Focus and accessibility contract

**Files:**
- Modify: `cmd/inspectah/internal/renderer/static/report.html` (renderThresholdActionBar and dismissThresholdActionBar already handle most of this; this task adds the remaining focus test)
- Modify: `tests/e2e-go/tests/fleet-threshold-action-bar.spec.ts`

- [ ] **Step 1: Write the failing E2E test**

Append to `tests/e2e-go/tests/fleet-threshold-action-bar.spec.ts`:

```ts
  test('action-bar-focus-stays-on-select', async ({ page }) => {
    await navigateToSection(page, 'overview');

    const select = page.locator('#threshold-select');

    // Focus the select explicitly
    await select.focus();

    // Change threshold — bar appears but focus stays on select
    await select.selectOption('0.8');

    const bar = page.locator('.threshold-action-bar');
    await expect(bar).toBeVisible();

    // Verify focus is on the select, NOT the action button
    const focusedId = await page.evaluate(() => document.activeElement?.id);
    expect(focusedId).toBe('threshold-select');

    // Tab from select should go to action button, then dismiss
    await page.keyboard.press('Tab');
    const focusAfterTab1 = await page.evaluate(() => document.activeElement?.id);
    expect(focusAfterTab1).toBe('threshold-action-btn');

    await page.keyboard.press('Tab');
    const focusAfterTab2 = await page.evaluate(() => document.activeElement?.id);
    expect(focusAfterTab2).toBe('threshold-dismiss-btn');
  });
```

- [ ] **Step 2: Run the test**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && npx playwright test tests/e2e-go/tests/fleet-threshold-action-bar.spec.ts --grep "action-bar-focus-stays" 2>&1 | tail -20`

Expected: This should PASS with the existing implementation since:
- The `thresholdSelect.onchange` doesn't move focus (browser keeps focus on `<select>` after option change)
- The action bar renders without calling `.focus()` on any element
- Tab order follows DOM order: select → action button → dismiss button

If PASS, no additional implementation needed. If FAIL, investigate and fix the focus management.

- [ ] **Step 3: Verify the confirmation dwell works**

The `dismissThresholdActionBar()` function already disables buttons during the dwell and returns focus to the select after the timeout. Verify this is working by running:

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && npx playwright test tests/e2e-go/tests/fleet-threshold-action-bar.spec.ts --grep "action-bar-include-flips" 2>&1 | tail -20`

Expected: PASS — the include test already verifies focus returns to select after the dwell.

- [ ] **Step 4: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add tests/e2e-go/tests/fleet-threshold-action-bar.spec.ts
git commit -m "test(e2e): add focus and tab-order assertion for action bar

Verifies focus stays on threshold-select after bar appears, and tab
order moves through action button then dismiss button.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 6: Pre-dirtied row and section-reopen E2E tests

**Files:**
- Modify: `tests/e2e-go/tests/fleet-threshold-action-bar.spec.ts`

These tests validate the state contract for edge cases: items that were already manually toggled before the bulk action, and sections that were marked as reviewed being reopened after bulk changes.

- [ ] **Step 1: Write the pre-dirtied toggle-card test**

Append to `tests/e2e-go/tests/fleet-threshold-action-bar.spec.ts`:

```ts
  test('action-bar-pre-dirtied-toggle-card', async ({ page }) => {
    await navigateToSection(page, 'packages');

    // Find the first toggle-card switch and its key
    const card = page.locator('#section-packages [data-key]').first();
    const key = await card.getAttribute('data-key');
    expect(key).toBeTruthy();

    // Capture original include state
    const originalInclude = await page.evaluate(
      (k) => {
        const App = (window as any).App;
        // Walk snapshot to find include value
        for (const section of ['rpm.packages_added', 'runtime.services', 'identity.users', 'system.system_changes']) {
          const parts = section.split('.');
          let arr = App.snapshot;
          for (const p of parts) { arr = arr?.[p]; }
          if (!arr) continue;
          for (const item of arr) {
            const itemKey = parts[0] === 'rpm' ? 'pkg-' + item.name + '-' + item.arch : item.key;
            if (itemKey === k) return item.include !== false;
          }
        }
        return null;
      },
      key
    );

    // Dirty the item via inline toggle click
    const toggle = page.locator(`[data-key="${key}"] button[role="switch"]`).first();
    if (await toggle.count() > 0) {
      await toggle.click();
    }

    // Verify priorValues captured the original
    const priorVal = await page.evaluate(
      (k) => (window as any).App.priorValues[k],
      key
    );
    expect(priorVal).toBe(originalInclude);

    // Note the current change count
    const countBefore = await page.evaluate(() => {
      const badge = document.getElementById('changes-badge');
      return badge ? badge.textContent : '';
    });

    // Now go to overview and trigger action bar
    await navigateToSection(page, 'overview');
    const select = page.locator('#threshold-select');
    await select.selectOption('0.8');

    const bar = page.locator('.threshold-action-bar');
    if (await bar.isVisible()) {
      // Apply the bulk action
      await page.locator('#threshold-action-btn').click();
      await expect(bar).toBeHidden({ timeout: 3000 });

      // priorValues for the pre-dirtied key must still hold the ORIGINAL value
      const priorAfterBulk = await page.evaluate(
        (k) => (window as any).App.priorValues[k],
        key
      );
      expect(priorAfterBulk).toBe(originalInclude);
    }
  });

  test('action-bar-pre-dirtied-triage-card', async ({ page }) => {
    await navigateToSection(page, 'packages');

    // Find a tier-3 flagged item (triage card with decision buttons)
    const triageCard = page.locator('#section-packages .triage-card').first();
    if (await triageCard.count() === 0) {
      // No triage cards in packages, try runtime
      await navigateToSection(page, 'runtime');
    }

    // Find any card with action buttons (Keep/Leave out) — these call makeDecision()
    const keepBtn = page.locator('.card-actions button:has-text("Keep")').first();
    if (await keepBtn.count() === 0) {
      test.skip();
      return;
    }

    // Get the key from the parent card
    const cardEl = page.locator('.card-actions button:has-text("Keep")').first()
      .locator('xpath=ancestor::*[@data-key]');
    const key = await cardEl.getAttribute('data-key');
    expect(key).toBeTruthy();

    // Capture original include state
    const originalInclude = await page.evaluate(
      (k) => {
        const App = (window as any).App;
        for (const item of App.triageManifest) {
          if (item.key === k) {
            return (window as any).getSnapshotInclude(k);
          }
        }
        return null;
      },
      key
    );

    // Click Keep — this calls makeDecision(key, section, true)
    await keepBtn.click();

    // Verify priorValues captured the original
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
    if (await bar.isVisible()) {
      await page.locator('#threshold-action-btn').click();
      await expect(bar).toBeHidden({ timeout: 3000 });

      // priorValues for the pre-dirtied key must still hold the ORIGINAL value
      const priorAfterBulk = await page.evaluate(
        (k) => (window as any).App.priorValues[k],
        key
      );
      expect(priorAfterBulk).toBe(originalInclude);
    }
  });

  test('action-bar-sections-reopen', async ({ page }) => {
    // Navigate to packages and mark it as reviewed
    await navigateToSection(page, 'packages');
    const markReviewedBtn = page.locator('#section-packages .mark-reviewed-btn').first();
    if (await markReviewedBtn.count() > 0) {
      await markReviewedBtn.click();
    } else {
      // Section auto-reviews if no items — skip test
      test.skip();
      return;
    }

    // Verify sidebar dot is green (reviewed)
    const dot = page.locator('#dot-packages');
    await expect(dot).toHaveClass(/reviewed/);

    // Go to overview and trigger action bar that affects packages
    await navigateToSection(page, 'overview');
    const select = page.locator('#threshold-select');
    await select.selectOption('0.8');

    const bar = page.locator('.threshold-action-bar');
    if (!(await bar.isVisible())) {
      test.skip();
      return;
    }

    // Verify the message includes Packages
    const msg = await page.locator('#threshold-suggestion-text').textContent();
    if (!msg?.includes('Packages')) {
      test.skip();
      return;
    }

    // Apply
    await page.locator('#threshold-action-btn').click();
    await expect(bar).toBeHidden({ timeout: 3000 });

    // Sidebar dot should have reverted to unreviewed (no 'reviewed' class)
    await expect(dot).not.toHaveClass(/reviewed/);

    // Progress bar should have decremented
    const progressText = await page.locator('#review-progress-text').textContent();
    expect(progressText).toBeTruthy();
  });
```

- [ ] **Step 2: Run the tests**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && npx playwright test tests/e2e-go/tests/fleet-threshold-action-bar.spec.ts --grep "pre-dirtied|sections-reopen" 2>&1 | tail -20`

Expected: PASS — the implementation from Task 1 (priorValues guard) and Task 3 (applyThresholdSuggestion with first-touch semantics and reopenReviewedSection) should handle all these cases.

If any FAIL, investigate the assertion and fix the implementation.

- [ ] **Step 3: Run the full test suite**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && npx playwright test tests/e2e-go/tests/fleet-threshold-action-bar.spec.ts 2>&1 | tail -30`

Expected: All 11 tests PASS:
1. makeDecision preserves first-touch priorValues
2. action-bar-appears-on-threshold-lower
3. action-bar-zero-mismatch-no-bar
4. action-bar-include-flips-toggles
5. action-bar-exclude-flips-toggles
6. action-bar-dismiss-no-state-change
7. action-bar-replaces-on-rechange
8. action-bar-navigation-dismisses
9. action-bar-focus-stays-on-select
10. action-bar-pre-dirtied-toggle-card
11. action-bar-pre-dirtied-triage-card
12. action-bar-sections-reopen

- [ ] **Step 4: Run the existing fleet threshold test**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && npx playwright test tests/e2e-go/tests/fleet-threshold-no-dirty-state.spec.ts 2>&1 | tail -20`

Expected: PASS — threshold changes alone still produce zero side effects.

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add tests/e2e-go/tests/fleet-threshold-action-bar.spec.ts
git commit -m "test(e2e): add pre-dirtied row and section-reopen proofs

Validates that priorValues holds the original pre-first-touch state for
both toggle-card and triage-card mutation paths after a bulk action, and
that reviewed sections reopen when affected by the action bar.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Post-Implementation Checklist

After all tasks are complete, verify:

- [ ] `npx playwright test tests/e2e-go/tests/fleet-threshold-action-bar.spec.ts` — all pass
- [ ] `npx playwright test tests/e2e-go/tests/fleet-threshold-no-dirty-state.spec.ts` — regression-free
- [ ] `npx playwright test tests/e2e-go/tests/` — full E2E suite passes
- [ ] Manual smoke test: open a fleet snapshot in refine mode, change threshold, observe bar, click Include, verify toggles flipped
- [ ] Manual smoke test: verify bar disappears on navigation away from Overview
- [ ] Manual a11y smoke test: use keyboard Tab through select → action button → dismiss button
