# Reset to Original Inspection — Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a "Reset" button to the report toolbar that reverts all include/exclude and strategy changes to the page-load state.

**Architecture:** Deep-copy the snapshot JSON at page load into `originalSnapshot`. The reset function walks DOM elements, restores values from the copy, re-runs cascade logic, and updates the toolbar. The button lives in the existing toolbar with PF6 danger-link styling.

**Tech Stack:** Jinja2 template (HTML/JS), pytest for Python-side tests.

**Spec:** `docs/specs/2026-03-13-reset-to-original-inspection-design.md`

---

## File Structure

| File | Responsibility |
|------|---------------|
| `src/yoinkc/templates/report.html.j2` | Template: button HTML, `originalSnapshot` variable, `resetToOriginal()` function, repo cascade extraction |
| `tests/test_renderer_outputs.py` | Python tests: verify button and `originalSnapshot` in rendered HTML |

No new files. No Python module changes.

---

## Chunk 1: Tests and Button HTML

### Task 1: Add Python tests for reset button presence

**Files:**
- Modify: `tests/test_renderer_outputs.py` — add tests to `TestHtmlReport`

- [ ] **Step 1: Write the failing tests**

Add two tests to the `TestHtmlReport` class:

```python
def test_reset_button_present(self, outputs_with_baseline):
    """Reset button should be in the toolbar, disabled by default."""
    html = self._html(outputs_with_baseline)
    assert 'id="btn-reset"' in html
    assert "disabled" in html.split('id="btn-reset"')[1].split(">")[0]

def test_original_snapshot_embedded(self, outputs_with_baseline):
    """Page JS should deep-copy the snapshot for reset support."""
    html = self._html(outputs_with_baseline)
    assert "var originalSnapshot = JSON.parse(JSON.stringify(snapshot));" in html
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd yoinkc && python -m pytest tests/test_renderer_outputs.py::TestHtmlReport::test_reset_button_present tests/test_renderer_outputs.py::TestHtmlReport::test_original_snapshot_embedded -v`

Expected: FAIL — button and `originalSnapshot` don't exist yet.

- [ ] **Step 3: Add the reset button to the toolbar HTML**

In `src/yoinkc/templates/report.html.j2`, find the toolbar (around line 1601):

```html
<div id="exclude-toolbar" class="pf-v6-c-toolbar">
  <span class="toolbar-status" id="toolbar-status-text"></span>
```

Add the reset button immediately after the status span, before the existing buttons:

```html
  <button id="btn-reset" class="pf-v6-c-button pf-m-link pf-m-danger" type="button" disabled title="Reset all selections to their initial state">Reset</button>
```

- [ ] **Step 4: Add `originalSnapshot` deep copy**

In the `<script>` block, immediately after `var snapshot = {{ snapshot_json|safe }};` (around line 1624), add:

```js
var originalSnapshot = JSON.parse(JSON.stringify(snapshot));
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd yoinkc && python -m pytest tests/test_renderer_outputs.py::TestHtmlReport::test_reset_button_present tests/test_renderer_outputs.py::TestHtmlReport::test_original_snapshot_embedded -v`

Expected: PASS

- [ ] **Step 6: Run full test suite**

Run: `cd yoinkc && python -m pytest -x -q`

Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add tests/test_renderer_outputs.py src/yoinkc/templates/report.html.j2
git commit -m "feat(templates): Add reset button and originalSnapshot for reset-to-initial support

Assisted-by: Cursor (<model>)"
```

---

## Chunk 2: Extract Repo Cascade and Implement resetToOriginal()

### Task 2: Extract repo cascade into a callable function

**Files:**
- Modify: `src/yoinkc/templates/report.html.j2` — JS section

- [ ] **Step 1: Extract the repo cascade body**

Find the repo cascade handler (around line 1944, inside the `.repo-cb` change event listener). Extract the body into a standalone function `applyRepoCascade(repoCb)` that takes a repo checkbox element and applies the cascade based on its `.checked` state. The event handler becomes:

```js
document.querySelectorAll('#pkg-repo-table .repo-cb').forEach(function(cb) {
    cb.addEventListener('change', function() {
        applyRepoCascade(this);
        updateToolbar();
        setDirty(!isSnapshotClean());
    });
});
```

The `applyRepoCascade(repoCb)` function contains the existing cascade logic from the handler body. Replace `this` with `repoCb` throughout. The function should:

1. Get the row: `var tr = repoCb.closest('tr[data-snap-index]');`
2. Get the repo index, repo section IDs from the snapshot
3. Iterate leaf packages, set checked/disabled/include state based on `repoCb.checked`
4. Call `recomputeAutoDeps()` and `updatePkgBanner()`

Do **not** include `updateToolbar()` or `setDirty()` inside `applyRepoCascade()` — those are called by the event handler and by `resetToOriginal()` separately.

- [ ] **Step 2: Run full test suite to verify no regression**

Run: `cd yoinkc && python -m pytest -x -q`

Expected: All tests pass. (This is a pure refactor — behavior unchanged.)

- [ ] **Step 3: Commit**

```bash
git add src/yoinkc/templates/report.html.j2
git commit -m "refactor(templates): Extract repo cascade into applyRepoCascade() function

Assisted-by: Cursor (<model>)"
```

### Task 3: Implement resetToOriginal() and wire the button

**Files:**
- Modify: `src/yoinkc/templates/report.html.j2` — JS section

- [ ] **Step 1: Add resetToOriginal() function**

Add after the `applyRepoCascade()` function:

```js
function resetToOriginal() {
    // Walk all elements with snap data attributes
    document.querySelectorAll('[data-snap-section]').forEach(function(el) {
        var section = el.getAttribute('data-snap-section');
        var list = el.getAttribute('data-snap-list');
        var idx = parseInt(el.getAttribute('data-snap-index'), 10);
        if (!section || !list || isNaN(idx) || idx < 0) return;

        // Read original values
        var origSection = originalSnapshot[section];
        if (!origSection) return;
        var origList = origSection[list];
        if (!origList || !origList[idx]) return;
        var origItem = origList[idx];

        // Restore include in live snapshot
        var liveSection = snapshot[section];
        if (liveSection && liveSection[list] && liveSection[list][idx] !== undefined) {
            if (origItem.include !== undefined) {
                liveSection[list][idx].include = origItem.include;
            }
            if (origItem.strategy !== undefined) {
                liveSection[list][idx].strategy = origItem.strategy;
            }
        }

        // Sync DOM: checkbox
        var cb = el.querySelector('.include-cb');
        if (cb) {
            var shouldCheck = origItem.include !== undefined ? origItem.include : true;
            cb.checked = shouldCheck;
            // Reset disabled on leaf-cb only (repo cascade may have disabled them)
            if (cb.classList.contains('leaf-cb')) {
                cb.disabled = false;
            }
        }

        // Sync DOM: strategy select
        var sel = el.querySelector('.strategy-select');
        if (sel && origItem.strategy !== undefined) {
            sel.value = origItem.strategy;
        }

        // Sync DOM: excluded class
        var shouldExclude = origItem.include !== undefined ? !origItem.include : false;
        el.classList.toggle('excluded', shouldExclude);
    });

    // Re-run repo cascade for any repos that are unchecked in original state
    document.querySelectorAll('#pkg-repo-table .repo-cb').forEach(function(cb) {
        applyRepoCascade(cb);
    });

    // Re-run package cascade
    recomputeAutoDeps();
    updatePkgBanner();

    // Update toolbar and dirty state
    updateToolbar();
    setDirty(false);

    showToast('Selections reset to initial state');
}
```

Note: The `[data-snap-section]` selector matches both `<tr>` and `<div>` elements (compose files use `<div>`, not `<tr>`). This is intentional — do not filter to `tr` only.

- [ ] **Step 2: Wire the button and update toolbar**

Near the other button refs in the outer scope (around line 1711, alongside `rerenderBtn`, `tarballBtn`, etc.), add:

```js
var resetBtn = document.getElementById('btn-reset');
```

Add the click handler (anywhere after `resetToOriginal` is defined):

```js
if (resetBtn) {
    resetBtn.addEventListener('click', function() {
        if (confirm('Reset all selections to their initial state? This cannot be undone.')) {
            resetToOriginal();
        }
    });
}
```

Inside `updateToolbar()`, after the existing `changed` logic, add:

```js
if (resetBtn) resetBtn.disabled = !changed;
```

This ensures the reset button is disabled when no changes have been made. After a reset, `setDirty(false)` is called explicitly (rather than `setDirty(!isSnapshotClean())`) because the state is definitionally clean — it was just restored to the baseline.

Also update the JS comment block (around line 1615) to include `btn-reset` in the button state documentation.

- [ ] **Step 4: Run full test suite**

Run: `cd yoinkc && python -m pytest -x -q`

Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/yoinkc/templates/report.html.j2
git commit -m "feat(templates): Implement resetToOriginal() with confirmation and toolbar integration

Assisted-by: Cursor (<model>)"
```

---

## Manual Verification Checklist

After implementation, open a rendered report in a browser (via `yoinkc-refine` or by extracting from a tarball) and verify:

- [ ] Reset button visible in toolbar, disabled initially
- [ ] Uncheck a package checkbox → Reset button becomes enabled
- [ ] Click Reset → confirmation dialog appears
- [ ] Cancel → no changes
- [ ] Confirm → all checkboxes restored, toolbar shows "No pending changes"
- [ ] Change a strategy dropdown → Reset button becomes enabled
- [ ] Confirm reset → strategy restored to original value
- [ ] Uncheck a repo → leaf packages disabled → Reset → repo and packages restored
- [ ] Toast "Selections reset to initial state" appears after reset
