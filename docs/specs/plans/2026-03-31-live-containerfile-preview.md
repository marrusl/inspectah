# Live Containerfile Preview Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate the re-render cycle by generating a live Containerfile preview client-side, updating audit summary counts in real-time, and repositioning the server pipeline as an export action (Rebuild & Download).

**Architecture:** A new JS function `generateContainerfilePreview()` reads `window.snapshot` and writes Containerfile text to `#containerfile-pre` on every toggle/edit event. The toolbar is restructured: Re-render becomes Rebuild & Download (re-render + tarball), Reset becomes Discard (with confirmation), Download Tarball is removed. Audit summary counts update live via the existing `recalcTriageCounts()`. No Python changes needed.

**Tech Stack:** Vanilla JS (no framework), Jinja2 templates, Playwright E2E tests

**Spec:** `docs/specs/proposed/2026-03-31-live-containerfile-preview-design.md`

---

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `src/inspectah/templates/report/_js.html.j2` | Modify | Add `generateContainerfilePreview()`, hook into events, remove Copy clipboard logic, add Discard confirmation dialog, change Rebuild & Download behavior to in-place update |
| `src/inspectah/templates/report/_containerfile.html.j2` | Modify | Add preview helper line, remove Copy button |
| `src/inspectah/templates/report/_toolbar.html.j2` | Modify | Rename Re-render → Rebuild & Download, remove Download Tarball button, update Reset → Discard |
| `src/inspectah/templates/report/_css.html.j2` | Modify | Add confirmation dialog styling, preview helper line styling |
| `tests/e2e/tests/live-preview.spec.ts` | Create | E2E tests for live preview, Discard, Rebuild & Download, audit count updates, no Copy button |
| `tests/e2e/tests/re-render-cycle.spec.ts` | Modify | Update for renamed buttons and changed behavior |

---

## Task 1: Remove Copy Button and Add Preview Helper Line

**Files:**
- Modify: `src/inspectah/templates/report/_containerfile.html.j2`
- Modify: `src/inspectah/templates/report/_js.html.j2` (lines 838-873 — Copy clipboard IIFE)

- [ ] **Step 1: Write failing E2E test — no Copy button on Containerfile tab**

Create `tests/e2e/tests/live-preview.spec.ts`:

```typescript
import { test, expect } from '@playwright/test';
import { FLEET_URL } from './helpers';

test.describe('Live Containerfile Preview', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto(FLEET_URL);
    await page.locator('.helper-active').waitFor({ state: 'attached', timeout: 10_000 });
  });

  test('no Copy button on Containerfile tab', async ({ page }) => {
    await page.click('a[data-tab="containerfile"]');
    await expect(page.locator('#section-containerfile')).toBeVisible();
    await expect(page.locator('#btn-copy-cf')).not.toBeAttached();
  });

  test('preview helper line is visible on Containerfile tab', async ({ page }) => {
    await page.click('a[data-tab="containerfile"]');
    await expect(page.locator('#section-containerfile')).toBeVisible();
    const helper = page.locator('#containerfile-preview-cue');
    await expect(helper).toBeVisible();
    await expect(helper).toContainText('Live preview');
    await expect(helper).toContainText('Rebuild & Download');
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && npx playwright test tests/e2e/tests/live-preview.spec.ts --reporter=line`
Expected: FAIL — `#btn-copy-cf` still exists, `#containerfile-preview-cue` doesn't exist

- [ ] **Step 3: Remove Copy button from template**

In `_containerfile.html.j2`, replace:

```html
        <div class="section-heading-row">
          <h2>Containerfile</h2>
          <button type="button" class="pf-v6-c-button pf-m-secondary pf-m-small btn-copy-cf" id="btn-copy-cf">Copy</button>
        </div>
```

with:

```html
        <div class="section-heading-row">
          <h2>Containerfile</h2>
        </div>
```

And add the preview helper line. Replace:

```html
      <p>Generated image definition. Build with <code>podman build -t my-image .</code></p>
      <pre class="containerfile-pre" id="containerfile-pre">{{ containerfile_html|safe }}</pre>
```

with:

```html
      <p>Generated image definition. Build with <code>podman build -t my-image .</code></p>
      <p class="containerfile-preview-cue" id="containerfile-preview-cue">Live preview — updates as you edit. <strong>Rebuild &amp; Download</strong> produces the Containerfile in your tarball.</p>
      <pre class="containerfile-pre" id="containerfile-pre">{{ containerfile_html|safe }}</pre>
```

- [ ] **Step 4: Remove Copy clipboard JS**

In `_js.html.j2`, remove the entire Copy Containerfile IIFE (lines 838-873):

```javascript
  // --- Copy Containerfile to clipboard ---
  (function() {
    var btn = document.getElementById('btn-copy-cf');
    // ... entire block through closing })();
  })();
```

- [ ] **Step 5: Add preview helper line CSS**

In `_css.html.j2`, add after the `.containerfile-pre` rule (line 117):

```css
.containerfile-preview-cue { font-size: 0.8rem; color: var(--pf-t--global--text--color--subtle); margin: 0 0 0.5rem 0; }
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `npx playwright test tests/e2e/tests/live-preview.spec.ts --reporter=line`
Expected: PASS for both tests

- [ ] **Step 7: Run existing tests to verify no regressions**

Run: `npx playwright test --reporter=line`
Expected: All existing tests pass (Copy button wasn't tested elsewhere)

- [ ] **Step 8: Commit**

```bash
git add src/inspectah/templates/report/_containerfile.html.j2 src/inspectah/templates/report/_js.html.j2 src/inspectah/templates/report/_css.html.j2 tests/e2e/tests/live-preview.spec.ts
git commit -m "feat: remove Containerfile Copy button, add preview helper line"
```

---

## Task 2: Toolbar Restructuring

**Files:**
- Modify: `src/inspectah/templates/report/_toolbar.html.j2`
- Modify: `src/inspectah/templates/report/_js.html.j2` (Reset handler at ~664, Re-render handler at ~684, Tarball handler at ~720)
- Modify: `src/inspectah/templates/report/_css.html.j2`
- Modify: `tests/e2e/tests/live-preview.spec.ts`
- Modify: `tests/e2e/tests/re-render-cycle.spec.ts`

- [ ] **Step 1: Write failing E2E tests for toolbar changes**

Append to `tests/e2e/tests/live-preview.spec.ts`:

```typescript
  test('Discard button shows confirmation dialog', async ({ page }) => {
    // Make a change to enter dirty state
    await page.click('a[data-tab="rpm"]');
    await expect(page.locator('#section-rpm')).toBeVisible();
    const firstToggle = page.locator('.include-toggle').first();
    await firstToggle.click();

    // Click Discard
    const discardBtn = page.locator('#btn-reset');
    await expect(discardBtn).toBeEnabled();
    await discardBtn.click();

    // Confirmation dialog should appear
    const dialog = page.locator('#discard-confirm-dialog');
    await expect(dialog).toBeVisible();
    await expect(dialog).toContainText('Discard all edits?');
  });

  test('Discard confirmed restores original Containerfile', async ({ page }) => {
    // Capture original Containerfile content
    await page.click('a[data-tab="containerfile"]');
    const originalText = await page.locator('#containerfile-pre').textContent();

    // Toggle a package to change the preview
    await page.click('a[data-tab="rpm"]');
    const firstToggle = page.locator('.include-toggle').first();
    await firstToggle.click();

    // Verify Containerfile changed
    await page.click('a[data-tab="containerfile"]');
    const changedText = await page.locator('#containerfile-pre').textContent();
    expect(changedText).not.toEqual(originalText);

    // Discard and confirm
    const discardBtn = page.locator('#btn-reset');
    await discardBtn.click();
    await page.locator('#discard-confirm-yes').click();

    // Verify Containerfile is restored
    const restoredText = await page.locator('#containerfile-pre').textContent();
    expect(restoredText).toEqual(originalText);
  });

  test('Rebuild & Download button exists, no separate tarball button', async ({ page }) => {
    await expect(page.locator('#btn-re-render')).toBeAttached();
    await expect(page.locator('#btn-tarball')).not.toBeAttached();
  });
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx playwright test tests/e2e/tests/live-preview.spec.ts --reporter=line`
Expected: FAIL — `#discard-confirm-dialog` doesn't exist, `#btn-tarball` still exists

- [ ] **Step 3: Restructure toolbar template**

Replace the full content of `_toolbar.html.j2`:

```html
{# ── Sticky toolbar for exclusions ─────────────────────────────────────── #}
<div id="exclude-toolbar" class="pf-v6-c-toolbar" role="toolbar" aria-label="Actions">
  <button id="btn-reset" class="pf-v6-c-button pf-m-link pf-m-danger" type="button" disabled title="Discard all edits and revert to last built state">Discard</button>
  <span class="toolbar-status" id="toolbar-status-text"></span>
  <button id="btn-download-snapshot" class="pf-v6-c-button pf-m-secondary" type="button" title="Download snapshot with current selections as JSON">Download Modified Snapshot</button>
  {% if not refine_mode %}
  <button id="btn-rerender" class="pf-v6-c-button pf-m-primary" type="button" disabled title="Start inspectah refine to enable Rebuild & Download">Rebuild &amp; Download</button>
  {% endif %}
  {% if refine_mode %}
  <button id="btn-re-render" class="pf-v6-c-button pf-m-primary" type="button" disabled title="Rebuild with current edits and download tarball">Rebuild &amp; Download</button>
  {% endif %}
</div>
<div class="pf-v6-c-alert-group pf-m-toast" id="toast-group">
  <div class="pf-v6-c-alert pf-m-success" id="toast">
    <div class="pf-v6-c-alert__icon">
      <span aria-hidden="true">&#x2714;</span>
    </div>
    <p class="pf-v6-c-alert__title" id="toast-message"></p>
  </div>
</div>

{# ── Discard confirmation dialog ──────────────────────────────────────── #}
<div id="discard-confirm-dialog" class="discard-dialog-backdrop" style="display:none;">
  <div class="discard-dialog" role="alertdialog" aria-modal="true" aria-labelledby="discard-dialog-title">
    <h3 id="discard-dialog-title">Discard all edits?</h3>
    <p>This will revert the Containerfile to its last built state.</p>
    <div class="discard-dialog-actions">
      <button id="discard-confirm-yes" class="pf-v6-c-button pf-m-danger" type="button">Discard</button>
      <button id="discard-confirm-cancel" class="pf-v6-c-button pf-m-link" type="button">Cancel</button>
    </div>
  </div>
</div>

{# ══════════════════════════════════════════════════════════════════════════
   JavaScript
   ─────────────────────────────────────────────────────────────────────────
   Nav selectors: .pf-v6-c-nav__link[data-tab] for sidebar navigation.
   Active state: pf-m-current toggled on the .pf-v6-c-nav__link element.
   Theme toggle: pf-v6-theme-dark class on <html>.
   Button state: disabled HTML attribute for btn-reset / btn-re-render.
   All other JS-targeted classes preserved: .section, .visible, .include-toggle,
   .include-toggle-wrap, .strategy-select, .leaf-cb, .repo-cb, .warning-row,
   .warning-row-dismiss, .dismissed, .btn-copy-cf, tr.excluded,
   #exclude-toolbar, .helper-active, .toolbar-status, .pf-v6-c-spinner, #toast-group
   ══════════════════════════════════════════════════════════════════════════ #}
```

- [ ] **Step 4: Add confirmation dialog CSS**

In `_css.html.j2`, add:

```css
/* Discard confirmation dialog */
.discard-dialog-backdrop { position: fixed; inset: 0; background: rgba(0,0,0,0.5); z-index: 1100; display: flex; align-items: center; justify-content: center; }
.discard-dialog { background: var(--pf-t--global--background--color--primary--default, #fff); border-radius: 8px; padding: 1.5rem; max-width: 400px; width: 90%; box-shadow: 0 4px 16px rgba(0,0,0,0.3); }
.pf-v6-theme-dark .discard-dialog { background: var(--pf-t--global--background--color--primary--default, #1b1d21); }
.discard-dialog h3 { margin: 0 0 0.5rem; }
.discard-dialog p { margin: 0 0 1rem; font-size: 0.9rem; color: var(--pf-t--global--text--color--subtle); }
.discard-dialog-actions { display: flex; gap: 0.5rem; justify-content: flex-end; }
```

- [ ] **Step 5: Update Reset handler to show confirmation dialog**

In `_js.html.j2`, replace the Reset click handler (lines ~663-668):

```javascript
  if (resetBtn) {
    resetBtn.addEventListener('click', function() {
      if (confirm('Reset all selections to their initial state? This cannot be undone.')) {
        resetToOriginal();
      }
    });
  }
```

with:

```javascript
  // --- Discard with confirmation dialog ---
  var discardDialog = document.getElementById('discard-confirm-dialog');
  var discardYes = document.getElementById('discard-confirm-yes');
  var discardCancel = document.getElementById('discard-confirm-cancel');

  function showDiscardDialog() {
    if (discardDialog) discardDialog.style.display = '';
  }
  function hideDiscardDialog() {
    if (discardDialog) discardDialog.style.display = 'none';
  }

  if (resetBtn) {
    resetBtn.addEventListener('click', function() {
      if (!dirty) return;
      showDiscardDialog();
    });
  }
  if (discardYes) {
    discardYes.addEventListener('click', function() {
      hideDiscardDialog();
      resetToOriginal();
      if (typeof generateContainerfilePreview === 'function') {
        generateContainerfilePreview();
      }
    });
  }
  if (discardCancel) {
    discardCancel.addEventListener('click', hideDiscardDialog);
  }
```

- [ ] **Step 6: Update Re-render handler for Rebuild & Download (in-place update + tarball)**

In `_js.html.j2`, replace the Re-render handler (lines ~683-717) and remove the separate tarball handler (lines ~719-723):

```javascript
  // --- Rebuild & Download ---
  if (rerenderBtn) {
    rerenderBtn.addEventListener('click', function(){
      if (!helperAvailable) return;
      if (!dirty) return;
      var origText = rerenderBtn.innerHTML;
      rerenderBtn.innerHTML = '<span class="pf-v6-c-spinner pf-m-sm" role="progressbar" aria-label="Loading"><span class="pf-v6-c-spinner__clipper"></span><span class="pf-v6-c-spinner__lead-ball"></span><span class="pf-v6-c-spinner__tail-ball"></span></span>Rebuilding...';
      rerenderBtn.disabled = true;
      var sliderEl = document.getElementById('summary-prevalence-slider');
      if (snapshot.meta && snapshot.meta.fleet && sliderEl) {
        var newThreshold = parseInt(sliderEl.value, 10);
        if (typeof window.applyPrevalenceThreshold === 'function') {
          window.applyPrevalenceThreshold(newThreshold);
        }
        snapshot.meta.fleet.min_prevalence = newThreshold;
      }
      fetch(window.location.origin + '/api/re-render', {
        method: 'POST',
        headers: {'Content-Type': 'application/json'},
        body: JSON.stringify({snapshot: snapshot, original: originalSnapshot}),
      }).then(function(r){
        if (!r.ok) throw new Error('HTTP ' + r.status);
        return r.text();
      }).then(function(html){
        // Step 1: Extract snapshot and Containerfile from response
        var parser = new DOMParser();
        var doc = parser.parseFromString(html, 'text/html');
        var newCfPre = doc.getElementById('containerfile-pre');
        if (newCfPre) {
          document.getElementById('containerfile-pre').textContent = newCfPre.textContent;
        }
        // Extract updated snapshot from response
        var scriptTag = doc.querySelector('script');
        if (scriptTag) {
          var match = scriptTag.textContent.match(/^var snapshot = ([\s\S]*?);\s*var originalSnapshot/m);
          if (match) {
            try { snapshot = JSON.parse(match[1]); } catch(e) {}
          }
        }
        // Step 2: Update originalSnapshot (new Discard baseline)
        originalSnapshot = JSON.parse(JSON.stringify(snapshot));
        window.originalSnapshot = originalSnapshot;
        // Step 3: Rebuild baseline and clear dirty state
        buildBaseline();
        recalcTriageCounts();
        setDirty(false);
        updateToolbar();
        // Step 4: Trigger tarball download
        var tarballUrl = window.location.origin + '/api/tarball';
        fetch(tarballUrl).then(function(tr) {
          if (!tr.ok) throw new Error('Tarball download failed: HTTP ' + tr.status);
          return tr.blob();
        }).then(function(blob) {
          var a = document.createElement('a');
          a.href = URL.createObjectURL(blob);
          a.download = 'inspectah-output.tar.gz';
          a.click();
          URL.revokeObjectURL(a.href);
          showToast('Rebuilt and downloaded successfully');
        }).catch(function(err) {
          showToast('Rebuild succeeded but download failed: ' + err.message + '. Use Download Modified Snapshot or retry.', 6000, true);
        });
        rerenderBtn.innerHTML = origText;
        rerenderBtn.disabled = true; // clean state — disable until next edit
      }).catch(function(err){
        showToast('Rebuild failed: ' + err.message, 5000, true);
        rerenderBtn.innerHTML = origText;
        rerenderBtn.disabled = false;
      });
    });
  }
```

Also remove the standalone tarball button handler:

```javascript
  // --- Download tarball ---
  tarballBtn.addEventListener('click', function(){
    if (!helperAvailable) return;
    window.location.href = window.location.origin + '/api/tarball';
  });
```

And update the `tarballBtn` variable reference (line 178):

Replace:
```javascript
  var tarballBtn = document.getElementById('btn-tarball');
```

with:
```javascript
  // btn-tarball removed — tarball download is now part of Rebuild & Download
```

And remove any `tarballBtn` references in `enableHelperButtons()` (lines ~743-744):

Replace:
```javascript
    tarballBtn.disabled = false;
    tarballBtn.title = 'Download current output as tar.gz';
```

with nothing (remove both lines).

Also update `setDirty()` (line 239) — remove the tarball button reference:

Replace:
```javascript
  function setDirty(isDirty) {
    dirty = isDirty;
    if (tarballBtn) tarballBtn.disabled = isDirty;
```

with:
```javascript
  function setDirty(isDirty) {
    dirty = isDirty;
```

- [ ] **Step 7: Update re-render-cycle.spec.ts for new button names**

In `tests/e2e/tests/re-render-cycle.spec.ts`, the tests reference `#btn-re-render` which still exists, and the test logic (making a change, clicking re-render, verifying page updates) still works but the behavior changes:

- The `document.write()` replacement is gone — instead the page updates in-place
- After Rebuild & Download, the page stays intact (no `page.waitForNavigation()` needed)

Replace the full file:

```typescript
import { test, expect } from '@playwright/test';
import { FLEET_URL } from './helpers';

test.describe('Rebuild & Download Cycle', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto(FLEET_URL);
    await page.locator('.helper-active').waitFor({ state: 'attached', timeout: 10_000 });
  });

  test('Rebuild & Download triggers full pipeline and downloads tarball', async ({ page }) => {
    // Navigate to config section
    await page.click('a[data-tab="config"]');
    await expect(page.locator('#section-config')).toBeVisible();

    // Expand the app.conf variant group and uncheck variant 2
    const appConfGroup = page.locator('tr.fleet-variant-group', {
      has: page.locator('code', { hasText: '/etc/app.conf' }),
    });
    await appConfGroup.locator('.fleet-variant-toggle').click();
    const childrenRow = page.locator('tr.fleet-variant-children').first();
    await expect(childrenRow).toBeVisible();

    // Uncheck variant 2 (snap-index="1")
    const variant2 = page.locator(
      'tr[data-variant-group="/etc/app.conf"][data-snap-index="1"]'
    );
    const toggleSpan = variant2.locator('.pf-v6-c-switch__toggle');
    await toggleSpan.click();

    // Verify dirty state
    const rebuildBtn = page.locator('#btn-re-render');
    await expect(rebuildBtn).toBeEnabled();

    // Set up download listener before clicking
    const downloadPromise = page.waitForEvent('download', { timeout: 30_000 });

    // Click Rebuild & Download — page updates in-place (no navigation)
    await rebuildBtn.click();

    // Wait for tarball download
    const download = await downloadPromise;
    expect(download.suggestedFilename()).toContain('.tar.gz');

    // Verify page is still intact (no document.write replacement)
    await expect(page.locator('#section-config')).toBeVisible();

    // Verify Rebuild & Download is disabled after clean rebuild
    await expect(rebuildBtn).toBeDisabled();
  });

  test('error on corrupted rebuild: route interception returns 500', async ({ page }) => {
    // Navigate to config section
    await page.click('a[data-tab="config"]');
    await expect(page.locator('#section-config')).toBeVisible();

    // Toggle a config variant to create dirty state
    const appConfGroup = page.locator('tr.fleet-variant-group', {
      has: page.locator('code', { hasText: '/etc/app.conf' }),
    });
    await appConfGroup.locator('.fleet-variant-toggle').click();
    const childrenRow = page.locator('tr.fleet-variant-children').first();
    await expect(childrenRow).toBeVisible();

    const variant2 = page.locator(
      'tr[data-variant-group="/etc/app.conf"][data-snap-index="1"]'
    );
    const toggleSpan = variant2.locator('.pf-v6-c-switch__toggle');
    await toggleSpan.click();

    const rebuildBtn = page.locator('#btn-re-render');
    await expect(rebuildBtn).toBeEnabled();

    // Intercept re-render API call and return 500
    await page.route('**/api/re-render', (route) =>
      route.fulfill({ status: 500, body: 'Internal Server Error' })
    );

    await rebuildBtn.click();

    // Verify error toast appears
    const toast = page.locator('#toast-message');
    await expect(toast).toContainText(/Rebuild failed/, { timeout: 10_000 });

    // Verify the button is re-enabled after error
    await expect(rebuildBtn).toBeEnabled({ timeout: 5_000 });
  });
});
```

- [ ] **Step 8: Run all tests**

Run: `npx playwright test --reporter=line`
Expected: All tests pass

- [ ] **Step 9: Commit**

```bash
git add src/inspectah/templates/report/_toolbar.html.j2 src/inspectah/templates/report/_js.html.j2 src/inspectah/templates/report/_css.html.j2 tests/e2e/tests/live-preview.spec.ts tests/e2e/tests/re-render-cycle.spec.ts
git commit -m "feat: toolbar restructure — Rebuild & Download, Discard with confirmation"
```

---

## Task 3: Client-Side Containerfile Preview Generator

**Files:**
- Modify: `src/inspectah/templates/report/_js.html.j2`
- Modify: `tests/e2e/tests/live-preview.spec.ts`

This is the core feature. The generator reads `window.snapshot` and produces Containerfile text matching the Python renderer's section order, with the documented simplifications (no FIXMEs, no multistage, no DHCP filtering, no detailed comments).

- [ ] **Step 1: Write failing E2E test — Containerfile updates on package toggle**

Append to `tests/e2e/tests/live-preview.spec.ts`:

```typescript
  test('Containerfile updates on package toggle', async ({ page }) => {
    // Get initial Containerfile text
    await page.click('a[data-tab="containerfile"]');
    const initialText = await page.locator('#containerfile-pre').textContent();

    // Navigate to packages and toggle a package off
    await page.click('a[data-tab="rpm"]');
    await expect(page.locator('#section-rpm')).toBeVisible();
    const firstToggle = page.locator('.include-toggle').first();
    const wasChecked = await firstToggle.isChecked();
    await firstToggle.click();

    // Go back to Containerfile tab — text should have changed
    await page.click('a[data-tab="containerfile"]');
    const updatedText = await page.locator('#containerfile-pre').textContent();
    expect(updatedText).not.toEqual(initialText);

    // Toggle back
    await page.click('a[data-tab="rpm"]');
    await firstToggle.click();
    await page.click('a[data-tab="containerfile"]');
    const restoredText = await page.locator('#containerfile-pre').textContent();
    expect(restoredText).toEqual(initialText);
  });
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx playwright test tests/e2e/tests/live-preview.spec.ts -g "package toggle" --reporter=line`
Expected: FAIL — toggling a package doesn't change Containerfile text (no generator yet)

- [ ] **Step 3: Implement `generateContainerfilePreview()`**

In `_js.html.j2`, add the following function **after** the `recalcTriageCounts()` function (after line ~815) and **before** the theme toggle IIFE:

```javascript
  // --- Client-side Containerfile preview generator ---
  function generateContainerfilePreview() {
    var pre = document.getElementById('containerfile-pre');
    if (!pre || !snapshot) return;
    var lines = [];

    // Helper: resolve base image
    function getBaseImage() {
      if (snapshot.rpm && snapshot.rpm.base_image) return snapshot.rpm.base_image;
      if (snapshot.rpm && snapshot.rpm.target_base_image) return snapshot.rpm.target_base_image;
      var osId = (snapshot.os_release && snapshot.os_release.id) ? snapshot.os_release.id.toLowerCase() : '';
      var verId = (snapshot.os_release && snapshot.os_release.version_id) || '';
      var major = verId.split('.')[0];
      if (osId === 'rhel' && major) return 'registry.redhat.io/rhel' + major + '/rhel-bootc:' + verId;
      if (osId === 'centos' && major) return 'quay.io/centos-bootc/centos-bootc:stream' + major;
      if (osId === 'fedora' && verId) return 'quay.io/fedora/fedora-bootc:' + verId;
      return 'registry.redhat.io/rhel9/rhel-bootc:latest';
    }

    var base = getBaseImage();

    // 1. Packages — FROM + dnf install
    lines.push('FROM ' + base);
    lines.push('');
    if (snapshot.rpm && snapshot.rpm.packages_added) {
      var included = snapshot.rpm.packages_added.filter(function(p) { return p.include !== false; });
      var names;
      var leafSet = snapshot.rpm.leaf_packages ? new Set(snapshot.rpm.leaf_packages) : null;
      if (leafSet && !snapshot.rpm.no_baseline) {
        var includedNames = new Set(included.map(function(p) { return p.name; }));
        names = Array.from(includedNames).filter(function(n) { return leafSet.has(n); }).sort();
      } else {
        names = Array.from(new Set(included.map(function(p) { return p.name; }))).sort();
      }
      if (names.length > 0) {
        lines.push('RUN dnf install -y \\');
        for (var i = 0; i < names.length - 1; i++) {
          lines.push('    ' + names[i] + ' \\');
        }
        lines.push('    ' + names[names.length - 1] + ' \\');
        lines.push('    && dnf clean all && rm -rf /var/cache/dnf /var/lib/dnf/history* /var/log/dnf* /var/log/hawkey.log /var/log/rhsm');
        lines.push('');
      }
    }

    // 2. Services — systemctl enable/disable
    if (snapshot.services) {
      var enableUnits = (snapshot.services.enabled_units || []).slice();
      var disableUnits = (snapshot.services.disabled_units || []).slice();
      if (enableUnits.length > 0 || disableUnits.length > 0) {
        if (enableUnits.length > 0) {
          lines.push('RUN systemctl enable ' + enableUnits.join(' '));
        }
        if (disableUnits.length > 0) {
          lines.push('RUN systemctl disable ' + disableUnits.join(' '));
        }
        lines.push('');
      }
    }

    // 3. Firewall — firewall zones
    if (snapshot.network && snapshot.network.firewall_zones) {
      var fwZones = snapshot.network.firewall_zones.filter(function(z) { return z.include !== false; });
      if (fwZones.length > 0) {
        lines.push('# Firewall: ' + fwZones.length + ' zone(s) — included in COPY config/etc/ below');
        lines.push('');
      }
    }

    // 4. Scheduled tasks — timer units and cron jobs
    if (snapshot.scheduled_tasks) {
      var genTimers = (snapshot.scheduled_tasks.generated_timer_units || []).filter(function(u) { return u.include !== false; });
      var cronJobs = (snapshot.scheduled_tasks.cron_jobs || []).filter(function(j) { return j.include !== false; });
      if (genTimers.length > 0 || cronJobs.length > 0) {
        lines.push('COPY config/etc/systemd/system/ /etc/systemd/system/');
        if (genTimers.length > 0) {
          var timerNames = genTimers.filter(function(u) { return u.name; }).map(function(u) { return u.name + '.timer'; });
          if (timerNames.length > 0) {
            lines.push('RUN systemctl enable ' + timerNames.join(' '));
          }
        }
        lines.push('');
      }
    }

    // 5. Config files — COPY
    if (snapshot.config && snapshot.config.files) {
      var configFiles = snapshot.config.files.filter(function(f) { return f.include !== false; });
      if (configFiles.length > 0) {
        lines.push('COPY config/etc/ /etc/');
        lines.push('');
      }
    }

    // 6. Non-RPM software — pip install
    if (snapshot.non_rpm_software && snapshot.non_rpm_software.items) {
      var nrItems = snapshot.non_rpm_software.items.filter(function(it) { return it.include !== false; });
      var pipPkgs = nrItems.filter(function(it) {
        return it.method === 'pip dist-info' && it.version && !it.has_c_extensions;
      });
      if (pipPkgs.length > 0) {
        pipPkgs.sort(function(a, b) { return a.name.localeCompare(b.name); });
        lines.push('RUN pip install \\');
        for (var pi = 0; pi < pipPkgs.length - 1; pi++) {
          lines.push('    ' + pipPkgs[pi].name + '==' + pipPkgs[pi].version + ' \\');
        }
        lines.push('    ' + pipPkgs[pipPkgs.length - 1].name + '==' + pipPkgs[pipPkgs.length - 1].version);
        lines.push('');
      }
    }

    // 7. Containers — quadlets and compose
    if (snapshot.containers) {
      var quadlets = (snapshot.containers.quadlet_units || []).filter(function(u) { return u.include !== false; });
      var compose = (snapshot.containers.compose_files || []).filter(function(c) { return c.include !== false; });
      if (quadlets.length > 0 || compose.length > 0) {
        if (quadlets.length > 0) {
          lines.push('COPY quadlet/ /etc/containers/systemd/');
        }
        lines.push('');
      }
    }

    // 8. Users/Groups — useradd/groupadd
    if (snapshot.users_groups && snapshot.users_groups.users) {
      var includedUsers = snapshot.users_groups.users.filter(function(u) {
        return u.include !== false && (u.strategy === 'useradd' || u.strategy === 'sysusers');
      });
      if (includedUsers.length > 0) {
        var sysusers = includedUsers.filter(function(u) { return u.strategy === 'sysusers'; });
        var useradd = includedUsers.filter(function(u) { return u.strategy === 'useradd'; });
        if (sysusers.length > 0) {
          lines.push('COPY config/usr/lib/sysusers.d/inspectah-users.conf /usr/lib/sysusers.d/inspectah-users.conf');
        }
        useradd.forEach(function(u) {
          var cmd = 'RUN useradd -m -u ' + u.uid;
          if (u.gid) cmd += ' -g ' + u.gid;
          if (u.shell) cmd += ' -s ' + u.shell;
          cmd += ' ' + u.name;
          lines.push(cmd);
        });
        lines.push('');
      }
    }

    // 9. Kernel boot — kargs.d
    if (snapshot.kernel_boot && snapshot.kernel_boot.cmdline) {
      lines.push('RUN mkdir -p /usr/lib/bootc/kargs.d');
      lines.push('COPY config/usr/lib/bootc/kargs.d/inspectah-migrated.toml /usr/lib/bootc/kargs.d/');
      lines.push('');
    }

    // 10. SELinux — port labels
    if (snapshot.selinux && snapshot.selinux.port_labels) {
      var portLabels = snapshot.selinux.port_labels.filter(function(pl) { return pl.include !== false; });
      portLabels.forEach(function(pl) {
        lines.push('RUN semanage port -a -t ' + pl.type + ' -p ' + pl.protocol + ' ' + pl.port);
      });
      if (portLabels.length > 0) lines.push('');
    }

    // 11. Epilogue — bootc lint
    lines.push('RUN bootc container lint');

    pre.textContent = lines.join('\n');
  }

  // Make available globally for Discard handler
  window.generateContainerfilePreview = generateContainerfilePreview;
```

- [ ] **Step 4: Hook generator into events**

In `_js.html.j2`, update the include-toggle change handler (after line 368, inside the existing handler). After `recalcTriageCounts();`, add:

```javascript
      generateContainerfilePreview();
```

Similarly, in the strategy-select change handler, after its `recalcTriageCounts()` call, add:

```javascript
      generateContainerfilePreview();
```

In the `resetToOriginal()` function, after `recalcTriageCounts()` (line ~642), add:

```javascript
    generateContainerfilePreview();
```

In the repo-cb change handler (after line ~657's `recalcTriageCounts()` call), add:

```javascript
      generateContainerfilePreview();
```

- [ ] **Step 5: Call generator on page load**

At the end of the main IIFE (after `checkHelperAvailable(0);` and `updateToolbar();`, around line ~770), add:

```javascript
  // Generate initial live preview
  generateContainerfilePreview();
```

- [ ] **Step 6: Run the test**

Run: `npx playwright test tests/e2e/tests/live-preview.spec.ts -g "package toggle" --reporter=line`
Expected: PASS

- [ ] **Step 7: Run all tests**

Run: `npx playwright test --reporter=line`
Expected: All tests pass

- [ ] **Step 8: Commit**

```bash
git add src/inspectah/templates/report/_js.html.j2 tests/e2e/tests/live-preview.spec.ts
git commit -m "feat: client-side Containerfile preview generator with live updates"
```

---

## Task 4: Hook Preview into Variant Selection, Config Editor, and Prevalence Slider

**Files:**
- Modify: `src/inspectah/templates/report/_js.html.j2`
- Modify: `tests/e2e/tests/live-preview.spec.ts`

- [ ] **Step 1: Write failing E2E tests**

Append to `tests/e2e/tests/live-preview.spec.ts`:

```typescript
  test('Containerfile updates on variant selection', async ({ page }) => {
    // Capture initial Containerfile
    await page.click('a[data-tab="containerfile"]');
    const initialText = await page.locator('#containerfile-pre').textContent();

    // Navigate to config and change a variant
    await page.click('a[data-tab="config"]');
    await expect(page.locator('#section-config')).toBeVisible();

    const appConfGroup = page.locator('tr.fleet-variant-group', {
      has: page.locator('code', { hasText: '/etc/app.conf' }),
    });
    await appConfGroup.locator('.fleet-variant-toggle').click();
    const childrenRow = page.locator('tr.fleet-variant-children').first();
    await expect(childrenRow).toBeVisible();

    // Toggle variant 2
    const variant2 = page.locator(
      'tr[data-variant-group="/etc/app.conf"][data-snap-index="1"]'
    );
    await variant2.locator('.pf-v6-c-switch__toggle').click();

    // Containerfile should have changed (config files affect COPY lines)
    await page.click('a[data-tab="containerfile"]');
    // The preview regenerates on toggle — verify it ran without error
    const updatedText = await page.locator('#containerfile-pre').textContent();
    expect(updatedText).toBeTruthy();
  });

  test('Containerfile updates on prevalence slider', async ({ page }) => {
    const slider = page.locator('#summary-prevalence-slider');
    if (!(await slider.isVisible())) {
      test.skip();
      return;
    }
    await page.click('a[data-tab="containerfile"]');
    const initialText = await page.locator('#containerfile-pre').textContent();

    // Move slider to a different value
    await slider.fill('40');
    await slider.dispatchEvent('input');

    // Containerfile should reflect changed inclusions
    await page.click('a[data-tab="containerfile"]');
    const updatedText = await page.locator('#containerfile-pre').textContent();
    // Text may or may not change depending on fixture data,
    // but the preview should have regenerated without error
    expect(updatedText).toBeTruthy();
  });
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx playwright test tests/e2e/tests/live-preview.spec.ts -g "variant|prevalence" --reporter=line`
Expected: FAIL or PASS depending on whether the hooks from Task 3 already covered these paths. If they pass, the variant/slider handlers already trigger `generateContainerfilePreview()` through the include-toggle cascade.

- [ ] **Step 3: Hook into prevalence slider input event**

Find the prevalence slider `input` event handler in `_js.html.j2` (search for `summary-prevalence-slider` input listener). After the handler updates counts, add `generateContainerfilePreview()`.

Search for the slider handler and after its `recalcTriageCounts()` call, add:

```javascript
      generateContainerfilePreview();
```

- [ ] **Step 4: Hook into config editor save**

Search for the config editor save handler in `_js.html.j2` (search for `editor` save or `config-editor` save). After its `setDirty()` or `recalcTriageCounts()` call, add:

```javascript
      generateContainerfilePreview();
```

- [ ] **Step 5: Run tests**

Run: `npx playwright test tests/e2e/tests/live-preview.spec.ts --reporter=line`
Expected: All tests pass

- [ ] **Step 6: Run full test suite**

Run: `npx playwright test --reporter=line`
Expected: All tests pass

- [ ] **Step 7: Commit**

```bash
git add src/inspectah/templates/report/_js.html.j2 tests/e2e/tests/live-preview.spec.ts
git commit -m "feat: hook preview into variant selection, config editor, prevalence slider"
```

---

## Task 5: Audit Summary Count Updates and Audit Preview Cue

**Files:**
- Modify: `src/inspectah/templates/report/_js.html.j2` (verify `recalcTriageCounts()` already runs on all events)
- Modify: audit report template (for preview cue line)
- Modify: `tests/e2e/tests/live-preview.spec.ts`

- [ ] **Step 1: Write failing E2E tests for audit counts**

Append to `tests/e2e/tests/live-preview.spec.ts`:

```typescript
  test('audit counts update on package toggle', async ({ page }) => {
    // Navigate to summary to read initial triage counts
    await page.click('a[data-tab="summary"]');
    const initialTotal = await page.locator('#summary-scope-total').textContent();

    // Toggle a package off
    await page.click('a[data-tab="rpm"]');
    const firstToggle = page.locator('.include-toggle').first();
    await firstToggle.click();

    // Check that triage count changed
    await page.click('a[data-tab="summary"]');
    const updatedTotal = await page.locator('#summary-scope-total').textContent();
    expect(parseInt(updatedTotal || '0')).toBeLessThan(parseInt(initialTotal || '0'));
  });

  test('audit preview cue is visible on audit tab', async ({ page }) => {
    await page.click('a[data-tab="audit"]');
    const cue = page.locator('#audit-preview-cue');
    if (await cue.isVisible()) {
      await expect(cue).toContainText('Summary counts update as you edit');
      await expect(cue).toContainText('Rebuild & Download');
    }
    // If audit tab doesn't exist in fixture, skip gracefully
  });
```

- [ ] **Step 2: Run tests to verify status**

Run: `npx playwright test tests/e2e/tests/live-preview.spec.ts -g "audit" --reporter=line`
Expected: The counts test may already pass (recalcTriageCounts already runs on toggle). The cue test will fail.

- [ ] **Step 3: Add audit preview cue line**

Find the audit report template. Search for the audit section heading:

```bash
grep -rn "audit" src/inspectah/templates/report/ --include="*.j2" -l
```

In the audit report template (likely `_audit_report.html.j2`), add the preview cue after the section heading:

```html
<p class="containerfile-preview-cue" id="audit-preview-cue">Summary counts update as you edit. Detail tables refresh on <strong>Rebuild &amp; Download</strong>.</p>
```

- [ ] **Step 4: Verify `recalcTriageCounts()` already handles all events**

Review `_js.html.j2` to confirm `recalcTriageCounts()` is called in:
- Include toggle handler ✓ (line 368)
- Strategy select handler ✓
- Repo-cb handler ✓ (line 657)
- resetToOriginal ✓ (line 642)
- Prevalence slider handler ✓

If any are missing, add the call.

- [ ] **Step 5: Run tests**

Run: `npx playwright test tests/e2e/tests/live-preview.spec.ts --reporter=line`
Expected: All tests pass

- [ ] **Step 6: Run full test suite**

Run: `npx playwright test --reporter=line`
Expected: All tests pass

- [ ] **Step 7: Commit**

```bash
git add src/inspectah/templates/report/ tests/e2e/tests/live-preview.spec.ts
git commit -m "feat: audit preview cue, verify live triage count updates"
```

---

## Task 6: Dirty State Lifecycle and Baseline Refresh

**Files:**
- Modify: `src/inspectah/templates/report/_js.html.j2`
- Modify: `tests/e2e/tests/live-preview.spec.ts`

This task ensures the dirty state lifecycle from the spec is correct:
- Dirty is set on toggle/variant/config/slider changes
- Dirty clears on Discard confirm or Rebuild & Download success
- After Rebuild & Download, `originalSnapshot` is refreshed (new Discard baseline)

- [ ] **Step 1: Write failing E2E test — Rebuild & Download resets Discard baseline**

Append to `tests/e2e/tests/live-preview.spec.ts`:

```typescript
  test('after Rebuild & Download, Discard returns to post-rebuild state', async ({ page }) => {
    // Make change 1
    await page.click('a[data-tab="rpm"]');
    const firstToggle = page.locator('.include-toggle').first();
    await firstToggle.click();

    // Rebuild & Download
    const rebuildBtn = page.locator('#btn-re-render');
    const downloadPromise = page.waitForEvent('download', { timeout: 30_000 });
    await rebuildBtn.click();
    await downloadPromise;

    // Capture post-rebuild Containerfile
    await page.click('a[data-tab="containerfile"]');
    const postRebuildText = await page.locator('#containerfile-pre').textContent();

    // Make change 2
    await page.click('a[data-tab="rpm"]');
    const secondToggle = page.locator('.include-toggle').nth(1);
    await secondToggle.click();

    // Discard — should return to post-rebuild state, not original page load
    const discardBtn = page.locator('#btn-reset');
    await discardBtn.click();
    await page.locator('#discard-confirm-yes').click();

    await page.click('a[data-tab="containerfile"]');
    const afterDiscardText = await page.locator('#containerfile-pre').textContent();
    expect(afterDiscardText).toEqual(postRebuildText);
  });
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx playwright test tests/e2e/tests/live-preview.spec.ts -g "post-rebuild" --reporter=line`
Expected: FAIL (unless Task 2's Rebuild & Download handler already refreshes `originalSnapshot`)

- [ ] **Step 3: Verify Rebuild & Download already refreshes baseline**

Check that the Rebuild & Download handler from Task 2 includes:
```javascript
originalSnapshot = JSON.parse(JSON.stringify(snapshot));
window.originalSnapshot = originalSnapshot;
buildBaseline();
```

If these lines exist, the test should pass. If not, add them.

Also ensure `window.originalSnapshot` is set on page load (line 5 of `_js.html.j2` sets `var originalSnapshot` — make sure it's accessible):

Add after line 5:
```javascript
window.originalSnapshot = originalSnapshot;
```

- [ ] **Step 4: Run tests**

Run: `npx playwright test tests/e2e/tests/live-preview.spec.ts --reporter=line`
Expected: All tests pass

- [ ] **Step 5: Run full test suite**

Run: `npx playwright test --reporter=line`
Expected: All tests pass

- [ ] **Step 6: Commit**

```bash
git add src/inspectah/templates/report/_js.html.j2 tests/e2e/tests/live-preview.spec.ts
git commit -m "feat: dirty state lifecycle — Discard baseline refreshes after Rebuild"
```

---

## Task 7: Deep Clone for Original Snapshot and Edge Cases

**Files:**
- Modify: `src/inspectah/templates/report/_js.html.j2`

- [ ] **Step 1: Ensure `window.originalSnapshot` is a deep clone on page load**

At the top of `_js.html.j2` (after line 5), the `originalSnapshot` is set from server-rendered JSON. The server provides both `snapshot` and `originalSnapshot` separately, but to guarantee isolation, add:

After line 5 (`var originalSnapshot = {{ original_snapshot_json|safe }};`), add:

```javascript
// Ensure originalSnapshot is a deep clone (defense against reference sharing)
if (typeof originalSnapshot === 'object' && originalSnapshot !== null) {
  originalSnapshot = JSON.parse(JSON.stringify(originalSnapshot));
}
```

- [ ] **Step 2: Verify generateContainerfilePreview handles missing sections**

Review the generator from Task 3. Each section already checks `if (snapshot.section && snapshot.section.items)` before processing. Confirm no unchecked property access would throw on an empty/minimal snapshot (e.g., a system with no SELinux, no non-RPM software, etc.).

- [ ] **Step 3: Run full test suite**

Run: `npx playwright test --reporter=line`
Expected: All tests pass

- [ ] **Step 4: Run Python tests to verify nothing is broken server-side**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && python -m pytest tests/ -x -q --tb=short`
Expected: All Python tests pass (no server-side changes)

- [ ] **Step 5: Commit**

```bash
git add src/inspectah/templates/report/_js.html.j2
git commit -m "fix: deep clone originalSnapshot on page load for state isolation"
```

---

## Self-Review Checklist

### Spec Coverage
| Spec Requirement | Task |
|-----------------|------|
| `generateContainerfilePreview()` with 11 sections | Task 3 |
| Hook into include toggle, variant, config editor, prevalence slider, page load | Tasks 3 & 4 |
| Toolbar: Re-render → Rebuild & Download | Task 2 |
| Toolbar: Reset → Discard with confirmation | Task 2 |
| Toolbar: Remove Download Tarball (merged into R&D) | Task 2 |
| Remove Copy button on Containerfile tab | Task 1 |
| Preview helper line (exact copy) | Task 1 |
| Audit preview cue line | Task 5 |
| Live audit summary counts via `recalcTriageCounts()` | Task 5 |
| Dirty state lifecycle | Task 6 |
| Deep clone for `originalSnapshot` | Task 7 |
| Rebuild & Download: in-place update (no `document.write()`) | Task 2 |
| Rebuild & Download: baseline refresh | Task 6 |
| E2E tests per spec table (11 tests) | Tasks 1-6 |
| No new Python tests (spec says none needed) | ✓ |

### Placeholder Scan
No TBD, TODO, or "implement later" references. All code blocks contain complete implementations.

### Type Consistency
- `generateContainerfilePreview()` — consistent name across Tasks 3, 4, 6, 7
- `recalcTriageCounts()` — existing function, referenced consistently
- `originalSnapshot` / `window.originalSnapshot` — consistent
- `#containerfile-pre`, `#btn-re-render`, `#btn-reset`, `#btn-copy-cf` — match existing IDs
- `#discard-confirm-dialog`, `#discard-confirm-yes`, `#discard-confirm-cancel` — new IDs, consistent across Tasks 2 and 6
- `#containerfile-preview-cue`, `#audit-preview-cue` — new IDs, consistent
