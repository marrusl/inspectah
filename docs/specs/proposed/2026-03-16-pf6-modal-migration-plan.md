# PF6 Modal Migration Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate all 4 report modals from custom inline styles to PF6 `pf-v6-c-modal-box` component.

**Architecture:** Direct class swap — replace inline styles with PF6 classes (`pf-v6-c-backdrop`, `pf-v6-l-bullseye`, `pf-v6-c-modal-box` and sub-elements). Compare modal gets extracted from dynamic JS DOM creation to a pre-rendered Jinja2 partial. No new abstractions.

**Tech Stack:** Jinja2 templates, PF6 CSS (already loaded), vanilla JS

**Spec:** `docs/specs/proposed/2026-03-16-pf6-modal-migration-design.md`

---

## File Map

- **Modify:** `src/yoinkc/templates/report/_editor.html.j2` — unsaved changes + delete file modals (lines 41-74)
- **Modify:** `src/yoinkc/templates/report/_new_file_modal.html.j2` — new file modal
- **Create:** `src/yoinkc/templates/report/_compare_modal.html.j2` — compare variants modal shell
- **Modify:** `src/yoinkc/templates/report/_file_browser.html.j2` — add include for compare modal
- **Modify:** `src/yoinkc/templates/report/_js.html.j2` — rewrite `showCompareModal()` / `closeCompareModal()`

No test file changes — this is a pure markup/style migration with no logic changes. Existing Python tests are unaffected.

---

## Task 1: Migrate Unsaved Changes + Delete File modals

**Files:**
- Modify: `src/yoinkc/templates/report/_editor.html.j2:41-74`

- [ ] **Step 1: Replace unsaved changes modal (lines 41-57)**

Replace the current inline-styled markup:

```html
{# --- Unsaved changes modal --- #}
<div id="unsaved-changes-modal" role="dialog" aria-modal="true" aria-label="Unsaved changes" style="display:none; position:fixed; top:0; left:0; right:0; bottom:0; z-index:9999;">
  <div style="position:absolute; top:0; left:0; right:0; bottom:0; background:rgba(0,0,0,0.5);"></div>
  <div style="position:absolute; top:50%; left:50%; transform:translate(-50%,-50%); background:var(--pf-t--global--background--color--primary--default); border-radius:8px; width:480px; box-shadow:0 4px 24px rgba(0,0,0,0.3);">
    <div style="padding:20px 24px; border-bottom:1px solid var(--pf-t--global--border--color--default);">
      <h2 class="pf-v6-c-title pf-m-xl" style="margin:0;">Unsaved changes</h2>
    </div>
    <div style="padding:24px;">
      <p id="unsaved-changes-message"></p>
    </div>
    <div style="padding:16px 24px; border-top:1px solid var(--pf-t--global--border--color--default); display:flex; justify-content:flex-end; gap:8px;">
      <button class="pf-v6-c-button pf-m-link" id="unsaved-btn-cancel" type="button">Cancel</button>
      <button class="pf-v6-c-button pf-m-secondary" id="unsaved-btn-discard" type="button">Discard</button>
      <button class="pf-v6-c-button pf-m-primary" id="unsaved-btn-save" type="button">Save</button>
    </div>
  </div>
</div>
```

With PF6 structure:

```html
{# --- Unsaved changes modal --- #}
<div class="pf-v6-c-backdrop" id="unsaved-changes-modal" style="display:none;">
  <div class="pf-v6-l-bullseye">
    <div class="pf-v6-c-modal-box pf-m-sm" role="dialog" aria-modal="true" aria-label="Unsaved changes">
      <div class="pf-v6-c-modal-box__header">
        <h2 class="pf-v6-c-modal-box__title">Unsaved changes</h2>
      </div>
      <div class="pf-v6-c-modal-box__body">
        <p id="unsaved-changes-message"></p>
      </div>
      <div class="pf-v6-c-modal-box__footer">
        <button class="pf-v6-c-button pf-m-link" id="unsaved-btn-cancel" type="button">Cancel</button>
        <button class="pf-v6-c-button pf-m-secondary" id="unsaved-btn-discard" type="button">Discard</button>
        <button class="pf-v6-c-button pf-m-primary" id="unsaved-btn-save" type="button">Save</button>
      </div>
    </div>
  </div>
</div>
```

- [ ] **Step 2: Replace delete file modal (lines 59-74)**

Replace the current inline-styled markup:

```html
{# --- Delete file confirmation modal --- #}
<div id="delete-file-modal" role="dialog" aria-modal="true" aria-label="Delete file" style="display:none; position:fixed; top:0; left:0; right:0; bottom:0; z-index:9999;">
  <div style="position:absolute; top:0; left:0; right:0; bottom:0; background:rgba(0,0,0,0.5);"></div>
  <div style="position:absolute; top:50%; left:50%; transform:translate(-50%,-50%); background:var(--pf-t--global--background--color--primary--default); border-radius:8px; width:480px; box-shadow:0 4px 24px rgba(0,0,0,0.3);">
    <div style="padding:20px 24px; border-bottom:1px solid var(--pf-t--global--border--color--default);">
      <h2 class="pf-v6-c-title pf-m-xl" style="margin:0;">Delete file</h2>
    </div>
    <div style="padding:24px;">
      <p id="delete-file-message"></p>
    </div>
    <div style="padding:16px 24px; border-top:1px solid var(--pf-t--global--border--color--default); display:flex; justify-content:flex-end; gap:8px;">
      <button class="pf-v6-c-button pf-m-secondary" id="delete-btn-cancel" type="button">Cancel</button>
      <button class="pf-v6-c-button pf-m-danger" id="delete-btn-confirm" type="button">Delete</button>
    </div>
  </div>
</div>
```

With PF6 structure:

```html
{# --- Delete file confirmation modal --- #}
<div class="pf-v6-c-backdrop" id="delete-file-modal" style="display:none;">
  <div class="pf-v6-l-bullseye">
    <div class="pf-v6-c-modal-box pf-m-sm" role="dialog" aria-modal="true" aria-label="Delete file">
      <div class="pf-v6-c-modal-box__header">
        <h2 class="pf-v6-c-modal-box__title">Delete file</h2>
      </div>
      <div class="pf-v6-c-modal-box__body">
        <p id="delete-file-message"></p>
      </div>
      <div class="pf-v6-c-modal-box__footer">
        <button class="pf-v6-c-button pf-m-secondary" id="delete-btn-cancel" type="button">Cancel</button>
        <button class="pf-v6-c-button pf-m-danger" id="delete-btn-confirm" type="button">Delete</button>
      </div>
    </div>
  </div>
</div>
```

- [ ] **Step 3: Verify JS compatibility**

The `showUnsavedModal()` function in `_editor_js.html.j2` does:
```js
var modal = document.getElementById('unsaved-changes-modal');
modal.style.display = '';
```
And cleanup does `modal.style.display = 'none'`. Toggling to `display:''` removes the inline override, letting the browser default (`block`) take effect. The `pf-v6-c-backdrop` CSS provides `position:fixed` and full coverage; `pf-v6-l-bullseye` inside provides `display:flex` centering. All 4 modals use the same `display:''` / `display:'none'` toggle pattern.

Verify no other style references exist.

Run: `grep -n "unsaved-changes-modal\|delete-file-modal" src/yoinkc/templates/report/_editor_js.html.j2`

Expected: only `getElementById` and `style.display` references — no inline style assumptions.

- [ ] **Step 4: Commit**

```
git add src/yoinkc/templates/report/_editor.html.j2
git commit -m "refactor: migrate unsaved/delete modals to PF6 modal-box

Replace inline-styled modal markup with pf-v6-c-backdrop +
pf-v6-l-bullseye + pf-v6-c-modal-box component structure.
No JS changes — display toggle works unchanged."
```

---

## Task 2: Migrate New File modal

**Files:**
- Modify: `src/yoinkc/templates/report/_new_file_modal.html.j2`

- [ ] **Step 1: Replace outer wrapper and header**

Replace the outermost container, backdrop, box, and header (lines 1-9):

```html
{% if refine_mode %}
<div id="new-file-modal" role="dialog" aria-modal="true" aria-label="Create new file" style="display:none; position:fixed; top:0; left:0; right:0; bottom:0; z-index:9999;">
  <div style="position:absolute; top:0; left:0; right:0; bottom:0; background:rgba(0,0,0,0.5);" onclick="closeNewFileModal()"></div>
  <div style="position:absolute; top:50%; left:50%; transform:translate(-50%,-50%); background:var(--pf-t--global--background--color--primary--default); border-radius:8px; width:560px; box-shadow:0 4px 24px rgba(0,0,0,0.3);">

    <div style="padding:20px 24px; border-bottom:1px solid var(--pf-t--global--border--color--default); display:flex; justify-content:space-between; align-items:center;">
      <h2 class="pf-v6-c-title pf-m-xl" style="margin:0;">Create new file</h2>
      <button class="pf-v6-c-button pf-m-plain" onclick="closeNewFileModal()" aria-label="Close">&times;</button>
    </div>
```

With:

```html
{% if refine_mode %}
<div class="pf-v6-c-backdrop" id="new-file-modal" style="display:none;" onclick="closeNewFileModal()">
  <div class="pf-v6-l-bullseye">
    <div class="pf-v6-c-modal-box pf-m-md" role="dialog" aria-modal="true" aria-label="Create new file" onclick="event.stopPropagation()">
      <div class="pf-v6-c-modal-box__close">
        <button class="pf-v6-c-button pf-m-plain" onclick="closeNewFileModal()" aria-label="Close">&times;</button>
      </div>
      <div class="pf-v6-c-modal-box__header">
        <h2 class="pf-v6-c-modal-box__title">Create new file</h2>
      </div>
```

Note: `onclick="event.stopPropagation()"` on the modal-box prevents backdrop click-to-close from firing when clicking inside the modal. The backdrop's `onclick` handles dismiss.

- [ ] **Step 2: Wrap body content**

The form body currently starts with `<div style="padding:24px;">`. Replace this opening tag with:

```html
      <div class="pf-v6-c-modal-box__body">
```

The inner form content (radio buttons, inputs, path preview, error divs) stays unchanged.

- [ ] **Step 3: Replace footer and closing tags**

The footer currently uses inline styles. Find the footer div (has the Create and Cancel buttons) and replace its opening tag. Also fix the closing div nesting to match the new structure (3 closing divs for modal-box, bullseye, backdrop instead of 2 for box, container).

Replace the footer opening:
```html
    <div style="padding:16px 24px; border-top:1px solid var(--pf-t--global--border--color--default); display:flex; justify-content:flex-end; gap:8px;">
```

With:
```html
      <div class="pf-v6-c-modal-box__footer">
```

Ensure closing tags are: `</div>` (footer) `</div>` (body) `</div>` (modal-box) `</div>` (bullseye) `</div>` (backdrop).

- [ ] **Step 4: Verify JS compatibility**

Run: `grep -n "new-file-modal\|closeNewFileModal\|showNewFileModal" src/yoinkc/templates/report/_new_file_modal.html.j2 src/yoinkc/templates/report/_js.html.j2 src/yoinkc/templates/report/_editor_js.html.j2`

The JS functions use `getElementById('new-file-modal')` and toggle `style.display`. Confirm no references to removed inline styles.

- [ ] **Step 5: Commit**

```
git add src/yoinkc/templates/report/_new_file_modal.html.j2
git commit -m "refactor: migrate new-file modal to PF6 modal-box

Replace inline-styled modal with pf-v6-c-backdrop + bullseye +
modal-box. Backdrop click-to-close preserved via onclick +
stopPropagation on modal-box. Form content unchanged."
```

---

## Task 3: Extract and migrate Compare Variants modal

**Files:**
- Create: `src/yoinkc/templates/report/_compare_modal.html.j2`
- Modify: `src/yoinkc/templates/report/_file_browser.html.j2` (add include)
- Modify: `src/yoinkc/templates/report/_js.html.j2` (rewrite show/close functions)

- [ ] **Step 1: Create the compare modal Jinja2 partial**

Create `src/yoinkc/templates/report/_compare_modal.html.j2`:

```html
{# --- Compare variants modal (fleet mode) --- #}
{% if snapshot.meta and snapshot.meta.fleet %}
<div class="pf-v6-c-backdrop" id="variant-compare-modal" style="display:none;" onclick="closeCompareModal()">
  <div class="pf-v6-l-bullseye">
    <div class="pf-v6-c-modal-box pf-m-md" role="dialog" aria-modal="true" aria-label="Compare variants" onclick="event.stopPropagation()">
      <div class="pf-v6-c-modal-box__close">
        <button class="pf-v6-c-button pf-m-plain" onclick="closeCompareModal()" aria-label="Close">&times;</button>
      </div>
      <div class="pf-v6-c-modal-box__header">
        <h2 class="pf-v6-c-modal-box__title" id="compare-modal-title">Compare variants</h2>
      </div>
      <div class="pf-v6-c-modal-box__body" id="compare-modal-body">
      </div>
      <div class="pf-v6-c-modal-box__footer" id="compare-modal-footer">
      </div>
    </div>
  </div>
</div>
{% endif %}
```

The body and footer are empty shells — JS populates them on open.

- [ ] **Step 2: Add include in file browser**

In `src/yoinkc/templates/report/_file_browser.html.j2`, after the existing includes for `_editor.html.j2` and `_new_file_modal.html.j2`, add:

```jinja2
{% include "report/_compare_modal.html.j2" %}
```

- [ ] **Step 3: Rewrite `closeCompareModal()` in `_js.html.j2`**

Find the current function (around line 709-712):
```js
function closeCompareModal() {
  var modal = document.getElementById('variant-compare-modal');
  if (modal) modal.remove();
}
```

Replace with:
```js
function closeCompareModal() {
  var modal = document.getElementById('variant-compare-modal');
  if (modal) {
    modal.style.display = 'none';
    var body = document.getElementById('compare-modal-body');
    var footer = document.getElementById('compare-modal-footer');
    if (body) body.innerHTML = '';
    if (footer) footer.innerHTML = '';
  }
}
```

- [ ] **Step 4: Rewrite `showCompareModal()` in `_js.html.j2`**

The current function (around lines 714-800+) creates the entire modal DOM from scratch. Replace it with a function that populates the pre-rendered shell.

Find the current function starting at `function showCompareModal(path, selectedItem, comparisonItem) {` and ending before the next top-level function.

Replace with a version that:
1. Removes the `existing.remove()` cleanup (no longer needed)
2. Keeps the `lineDiff()` call and diff HTML generation logic
3. Keeps the subtitle HTML generation (host info + legend)
4. Instead of `createElement` / `appendChild`, uses `getElementById` to populate:
   - `compare-modal-title` — `innerHTML` with the title
   - `compare-modal-body` — `innerHTML` with subtitle + diff content
   - `compare-modal-footer` — `innerHTML` with Close and Switch buttons
5. Shows the modal with `modal.style.display = ''`
6. Attaches event listeners to the dynamically-created footer buttons

The key structure of the replacement:

```js
function showCompareModal(path, selectedItem, comparisonItem) {
  var modal = document.getElementById('variant-compare-modal');
  if (!modal) return;

  var hasContent = !!(selectedItem.content || comparisonItem.content);
  var diff = hasContent ? lineDiff(selectedItem.content || '', comparisonItem.content || '') : null;

  // Title
  var titleEl = document.getElementById('compare-modal-title');
  titleEl.textContent = 'Compare variants';

  // Build subtitle HTML (preserve existing logic for host info + legend)
  var subtitleHtml = '...';  // Keep the existing subtitle-building code

  // Build body HTML (preserve existing diff rendering logic)
  var bodyHtml = subtitleHtml;
  if (diff) {
    bodyHtml += '<pre style="margin:0;font-size:var(--pf-t--global--font--size--sm);overflow-x:auto;">';
    // ... existing diff line rendering loop ...
    bodyHtml += '</pre>';
  } else {
    bodyHtml += '<p style="...">No file content to compare...</p>';
  }

  document.getElementById('compare-modal-body').innerHTML = bodyHtml;

  // Build footer HTML
  var footerHtml = '<button class="pf-v6-c-button pf-m-secondary" data-action="close">Close</button>';
  footerHtml += '<button class="pf-v6-c-button pf-m-primary" data-action="switch">Switch to this variant</button>';
  var footerEl = document.getElementById('compare-modal-footer');
  footerEl.innerHTML = footerHtml;

  // Attach event listeners
  footerEl.querySelector('[data-action="close"]').addEventListener('click', closeCompareModal);
  footerEl.querySelector('[data-action="switch"]').addEventListener('click', function() {
    switchVariant(path, selectedItem, comparisonItem);
    closeCompareModal();
  });

  modal.style.display = '';
}
```

**Important:** Preserve ALL existing logic for:
- The `lineDiff()` call and diff line rendering (classes `diff-line-add`, `diff-line-remove`, `diff-line-same`)
- The subtitle with host counts, selected/comparison labels, and legend spans
- The "Switch to this variant" button behavior (calls `switchVariant` then closes)
- The "no content" fallback message
- The `_comparisonContext` global for tracking state

- [ ] **Step 5: Verify no orphaned code**

Run: `grep -n "createElement\|appendChild" src/yoinkc/templates/report/_js.html.j2 | grep -i modal`

Expected: no results (all DOM creation code removed from the compare modal section).

- [ ] **Step 6: Commit**

```
git add src/yoinkc/templates/report/_compare_modal.html.j2 \
       src/yoinkc/templates/report/_file_browser.html.j2 \
       src/yoinkc/templates/report/_js.html.j2
git commit -m "refactor: extract and migrate compare modal to PF6

Extract compare-variants modal from dynamic JS DOM creation to
pre-rendered Jinja2 partial with PF6 modal-box structure. JS
populates body/footer on open, hides on close."
```

---

## Task 4: Visual verification

- [ ] **Step 1: Generate a test report**

Run yoinkc against a driftify profile to produce an HTML report. Use an existing snapshot or run:

```bash
cd /Users/mrussell/Work/bootc-migration
./driftify/driftify.py --profile web-server
```

Then run yoinkc to generate the report.

- [ ] **Step 2: Verify each modal**

Open the generated HTML report in a browser and verify:

1. **Unsaved changes modal** — open editor, make a change, click a different file. Modal should appear centered with PF6 styling. Save/Discard/Cancel buttons work.
2. **Delete file modal** — in editor, click Delete on a file. Modal appears centered. Cancel and Delete buttons work.
3. **New file modal** — click "+ New File". Modal appears centered with close button. Radio buttons switch fields. Create and Cancel work. Backdrop click dismisses.
4. **Compare variants modal** — (requires fleet report) click Compare on a variant row. Modal appears with diff. Close and Switch buttons work. Backdrop click dismisses.

- [ ] **Step 3: Run existing tests**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
python -m pytest tests/ -x -q
```

Expected: all tests pass (no test changes needed — tests don't assert on modal HTML structure).

- [ ] **Step 4: Commit any fixes if needed**

If visual verification reveals issues, fix and commit before marking complete.
