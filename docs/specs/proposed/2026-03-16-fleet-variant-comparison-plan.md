# Fleet Variant Comparison Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a Compare modal to fleet variant rows that shows an inline diff between the selected and comparison variants, with a one-click "Switch to this variant" button.

**Architecture:** Line-based diff algorithm in vanilla JS. Compare modal follows existing custom modal pattern (position:fixed overlay, not PF6 modal-box). Compare button added to non-selected variant child rows in section tab templates. All client-side — no Python changes.

**Tech Stack:** Vanilla JavaScript (ES5), Jinja2 templates, inline CSS with PF6 CSS variables

**Spec:** `docs/specs/proposed/2026-03-16-fleet-variant-comparison-design.md`

---

## File Structure

| File | Responsibility | Changes |
|------|---------------|---------|
| `src/yoinkc/templates/report/_js.html.j2` | Main report JS | Add diff algorithm, compare modal logic |
| `src/yoinkc/templates/report/_css.html.j2` | Report styling | Add diff line styles |
| `src/yoinkc/templates/report/_config.html.j2` | Config file rendering | Add Compare button to variant child rows |
| `src/yoinkc/templates/report/_containers.html.j2` | Container rendering | Add Compare button to quadlet variant child rows |
| `src/yoinkc/templates/report/_services.html.j2` | Service rendering | Add Compare button to drop-in variant child rows |

No new files. No Python changes.

---

## Chunk 1: Diff Algorithm & Compare Modal

### Task 1: Line-Based Diff Algorithm

**Files:**
- Modify: `src/yoinkc/templates/report/_js.html.j2`

- [ ] **Step 1: Add `lineDiff()` function**

In `_js.html.j2`, inside a `{% if fleet_meta %}` block, add the diff algorithm. This function takes two strings and returns an array of `{type, text}` objects where type is `'same'`, `'add'`, or `'remove'`:

```javascript
{% if fleet_meta %}
function lineDiff(oldText, newText) {
  var oldLines = (oldText || '').split('\n');
  var newLines = (newText || '').split('\n');
  var m = oldLines.length, n = newLines.length;

  // Defensive limit — skip diff for very large files
  if (m > 5000 || n > 5000) return null;

  // LCS table
  var dp = [];
  for (var i = 0; i <= m; i++) {
    dp[i] = [];
    for (var j = 0; j <= n; j++) {
      if (i === 0 || j === 0) dp[i][j] = 0;
      else if (oldLines[i - 1] === newLines[j - 1]) dp[i][j] = dp[i - 1][j - 1] + 1;
      else dp[i][j] = Math.max(dp[i - 1][j], dp[i][j - 1]);
    }
  }

  // Backtrack to produce diff
  var result = [];
  var i = m, j = n;
  while (i > 0 || j > 0) {
    if (i > 0 && j > 0 && oldLines[i - 1] === newLines[j - 1]) {
      result.push({ type: 'same', text: oldLines[i - 1] });
      i--; j--;
    } else if (j > 0 && (i === 0 || dp[i][j - 1] >= dp[i - 1][j])) {
      result.push({ type: 'add', text: newLines[j - 1] });
      j--;
    } else {
      result.push({ type: 'remove', text: oldLines[i - 1] });
      i--;
    }
  }
  result.reverse();
  return result;
}
{% endif %}
```

Returns `null` if either file exceeds 5000 lines.

- [ ] **Step 2: Verify no regressions**

Run: `pytest tests/ -x -q`
Expected: All tests pass (JS-only change, no server impact).

- [ ] **Step 3: Commit**

```bash
git add src/yoinkc/templates/report/_js.html.j2
git commit -m "feat(fleet): add line-based diff algorithm for variant comparison

LCS-based line diff in vanilla JS. Returns array of {type, text}
objects (same/add/remove). 5000-line defensive limit.

Assisted-by: Claude Code"
```

---

### Task 2: Compare Modal HTML & CSS

**Files:**
- Modify: `src/yoinkc/templates/report/_js.html.j2`
- Modify: `src/yoinkc/templates/report/_css.html.j2`

- [ ] **Step 1: Add compare modal DOM element**

In `_js.html.j2`, inside the `{% if fleet_meta %}` block, add a function that creates and shows the compare modal. The modal is created dynamically (not a static template element) since it needs to be populated with diff content each time:

```javascript
function showCompareModal(path, selectedItem, comparisonItem) {
  // Remove any existing modal
  var existing = document.getElementById('variant-compare-modal');
  if (existing) existing.remove();

  // Guard: if neither item has content, show a message
  if (!selectedItem.content && !comparisonItem.content) {
    alert('No content available for comparison.');
    return;
  }
  var diff = lineDiff(selectedItem.content || '', comparisonItem.content || '');

  // Build modal HTML
  var modal = document.createElement('div');
  modal.id = 'variant-compare-modal';
  modal.setAttribute('role', 'dialog');
  modal.setAttribute('aria-modal', 'true');
  modal.setAttribute('aria-label', 'Compare variants');
  modal.style.cssText = 'position:fixed;top:0;left:0;right:0;bottom:0;z-index:9999;';

  // Backdrop
  var backdrop = document.createElement('div');
  backdrop.style.cssText = 'position:absolute;top:0;left:0;right:0;bottom:0;background:rgba(0,0,0,0.5);';
  backdrop.addEventListener('click', closeCompareModal);
  modal.appendChild(backdrop);

  // Dialog box
  var box = document.createElement('div');
  box.style.cssText = 'position:absolute;top:50%;left:50%;transform:translate(-50%,-50%);'
    + 'background:var(--pf-t--global--background--color--primary--default);'
    + 'border-radius:8px;width:680px;max-height:80vh;display:flex;flex-direction:column;'
    + 'box-shadow:0 4px 24px rgba(0,0,0,0.3);';

  // Header
  var header = document.createElement('div');
  header.style.cssText = 'padding:16px 20px;border-bottom:1px solid var(--pf-t--global--border--color--default);display:flex;justify-content:space-between;align-items:center;';
  header.innerHTML = '<h2 class="pf-v6-c-title pf-m-lg" style="margin:0;">Compare variants</h2>'
    + '<button class="pf-v6-c-button pf-m-plain" aria-label="Close">&times;</button>';
  header.querySelector('button').addEventListener('click', closeCompareModal);
  box.appendChild(header);

  // Subtitle with host info
  var subtitle = document.createElement('div');
  subtitle.style.cssText = 'padding:8px 20px;font-size:0.8rem;color:var(--pf-t--global--text--color--subtle);border-bottom:1px solid var(--pf-t--global--border--color--default);';
  var selHosts = selectedItem.fleet ? selectedItem.fleet.hosts.join(', ') : '';
  var selCount = selectedItem.fleet ? selectedItem.fleet.count + '/' + selectedItem.fleet.total : '';
  var cmpHosts = comparisonItem.fleet ? comparisonItem.fleet.hosts.join(', ') : '';
  var cmpCount = comparisonItem.fleet ? comparisonItem.fleet.count + '/' + comparisonItem.fleet.total : '';
  subtitle.innerHTML = '<code style="font-size:0.8rem;">' + escapeHtml(path) + '</code><br>'
    + '<span class="diff-legend-selected">&#9632;</span> Selected (' + selCount + ' hosts: ' + escapeHtml(selHosts) + ')<br>'
    + '<span class="diff-legend-comparison">&#9632;</span> This variant (' + cmpCount + ' hosts: ' + escapeHtml(cmpHosts) + ')';
  box.appendChild(subtitle);

  // Diff body
  var body = document.createElement('div');
  body.style.cssText = 'padding:0;overflow-y:auto;flex:1;font-family:monospace;font-size:0.75rem;line-height:1.6;';

  if (diff === null) {
    body.innerHTML = '<div style="padding:20px;text-align:center;color:var(--pf-t--global--text--color--subtle);">File too large to diff (over 5000 lines)</div>';
  } else {
    var html = '';
    diff.forEach(function(line) {
      var cls = line.type === 'add' ? 'diff-line-add' : line.type === 'remove' ? 'diff-line-remove' : 'diff-line-same';
      var prefix = line.type === 'add' ? '+' : line.type === 'remove' ? '-' : ' ';
      html += '<div class="' + cls + '">' + prefix + ' ' + escapeHtml(line.text) + '</div>';
    });
    body.innerHTML = html;
  }
  box.appendChild(body);

  // Footer
  var footer = document.createElement('div');
  footer.style.cssText = 'padding:12px 20px;border-top:1px solid var(--pf-t--global--border--color--default);display:flex;justify-content:flex-end;gap:8px;';
  footer.innerHTML = '<button class="pf-v6-c-button pf-m-secondary" data-action="close">Close</button>'
    + '<button class="pf-v6-c-button pf-m-primary" data-action="switch">Switch to this variant</button>';
  footer.querySelector('[data-action="close"]').addEventListener('click', closeCompareModal);
  box.appendChild(footer);

  modal.appendChild(box);
  document.body.appendChild(modal);

  // Store comparison context for Switch button
  modal._comparisonContext = { path: path, selectedItem: selectedItem, comparisonItem: comparisonItem };

  // Switch handler
  footer.querySelector('[data-action="switch"]').addEventListener('click', function() {
    switchVariant(modal._comparisonContext);
    closeCompareModal();
  });
}

function closeCompareModal() {
  var modal = document.getElementById('variant-compare-modal');
  if (modal) modal.remove();
}

function escapeHtml(str) {
  var div = document.createElement('div');
  div.textContent = str;
  return div.innerHTML;
}
```

- [ ] **Step 2: Add diff line CSS**

In `_css.html.j2`, add styles for the diff lines and legend colors:

```css
.diff-line-same { padding: 0 16px; color: var(--pf-t--global--text--color--subtle); }
.diff-line-add { padding: 0 16px; background: rgba(77, 171, 247, 0.12); color: var(--pf-t--global--text--color--regular); }
.diff-line-remove { padding: 0 16px; background: rgba(201, 25, 11, 0.12); color: var(--pf-t--global--text--color--regular); }
.diff-legend-selected { color: #c9190b; margin-right: 4px; }
.diff-legend-comparison { color: #4dabf7; margin-right: 4px; }
```

Blue for additions (comparison variant) — intentionally matches fleet prevalence color coding rather than conventional green.

- [ ] **Step 3: Verify no regressions**

Run: `pytest tests/ -x -q`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/yoinkc/templates/report/_js.html.j2 src/yoinkc/templates/report/_css.html.j2
git commit -m "feat(fleet): add compare modal for variant diff display

Dynamic modal with inline diff, host info subtitle, Close and
Switch buttons. Follows existing custom modal pattern.

Assisted-by: Claude Code"
```

---

## Chunk 2: Compare Buttons, Switch Logic & Verification

### Task 3: Switch Variant Logic

**Files:**
- Modify: `src/yoinkc/templates/report/_js.html.j2`

- [ ] **Step 1: Add `switchVariant()` function**

In `_js.html.j2`, inside the `{% if fleet_meta %}` block, add the function that performs the variant switch. This reuses Spec 2's radio toggle behavior:

```javascript
function switchVariant(ctx) {
  var group = ctx.path;

  // Find all variant rows in this group
  var rows = document.querySelectorAll('[data-variant-group="' + group + '"]');
  rows.forEach(function(row) {
    var section = row.getAttribute('data-snap-section');
    var list = row.getAttribute('data-snap-list');
    var idx = parseInt(row.getAttribute('data-snap-index'), 10);
    if (!section || !list || isNaN(idx)) return;
    var arr = resolveSnapshotRef(section, list);
    if (!arr || !arr[idx]) return;

    var isTarget = (arr[idx] === ctx.comparisonItem);
    arr[idx].include = isTarget;

    var cb = row.querySelector('.include-toggle');
    if (cb) cb.checked = isTarget;
    row.classList.toggle('excluded', !isTarget);

    // Update Compare button vs "selected" label
    var actionCell = row.querySelector('.variant-action-cell');
    if (actionCell) {
      if (isTarget) {
        actionCell.innerHTML = '<span style="font-size:0.75rem;color:var(--pf-t--global--text--color--subtle);">selected</span>';
      } else {
        actionCell.innerHTML = '<button class="pf-v6-c-button pf-m-link pf-m-small variant-compare-btn">Compare</button>';
      }
    }
  });

  // Update parent group header row's toggle.
  // Depends on Spec 2 convention: parent header row and its first child
  // share the same data-snap-index, so updating the snapshot item above
  // already covers the parent's data. We just need to sync the DOM.
  var parentRows = document.querySelectorAll('.fleet-variant-group');
  parentRows.forEach(function(parentRow) {
    var parentIdx = parseInt(parentRow.getAttribute('data-snap-index'), 10);
    var section = parentRow.getAttribute('data-snap-section');
    var list = parentRow.getAttribute('data-snap-list');
    if (!section || !list || isNaN(parentIdx)) return;
    var arr = resolveSnapshotRef(section, list);
    if (!arr || !arr[parentIdx]) return;

    // Check if this parent's snap_index matches any variant in the group
    var childRows = document.querySelectorAll('[data-variant-group="' + group + '"]');
    var matchesGroup = false;
    childRows.forEach(function(child) {
      if (parseInt(child.getAttribute('data-snap-index'), 10) === parentIdx) matchesGroup = true;
    });
    if (!matchesGroup) return;

    var parentCb = parentRow.querySelector('.include-toggle');
    if (parentCb) parentCb.checked = arr[parentIdx].include;
    parentRow.classList.toggle('excluded', !arr[parentIdx].include);
  });

  updateToolbar();
  setDirty(!isSnapshotClean());
  if (typeof recalcTriageCounts === 'function') recalcTriageCounts();
}
```

- [ ] **Step 2: Verify no regressions**

Run: `pytest tests/ -x -q`
Expected: All tests pass.

- [ ] **Step 3: Commit**

```bash
git add src/yoinkc/templates/report/_js.html.j2
git commit -m "feat(fleet): add switchVariant logic for compare modal

Switches radio selection within variant group, updates row
styling, swaps Compare button and selected label, syncs parent
group header row.

Assisted-by: Claude Code"
```

---

### Task 4: Compare Buttons on Variant Rows

**Files:**
- Modify: `src/yoinkc/templates/report/_config.html.j2`
- Modify: `src/yoinkc/templates/report/_containers.html.j2`
- Modify: `src/yoinkc/templates/report/_services.html.j2`
- Modify: `src/yoinkc/templates/report/_js.html.j2`

- [ ] **Step 1: Add Compare button cell to config variant child rows**

In `_config.html.j2`, inside the variant child rows (the `<tr>` elements within `fleet-variant-children`), add a new `<td>` cell with class `variant-action-cell`. Show "selected" for the included variant, "Compare" button for others:

```html
<td class="variant-action-cell">
  {% if v.item.include %}
    <span style="font-size:0.75rem;color:var(--pf-t--global--text--color--subtle);">selected</span>
  {% else %}
    <button class="pf-v6-c-button pf-m-link pf-m-small variant-compare-btn">Compare</button>
  {% endif %}
</td>
```

Add a matching `<th>` in the variant children table header if one exists.

- [ ] **Step 2: Add Compare button cell to quadlet variant child rows**

Same pattern in `_containers.html.j2` for quadlet variant children.

- [ ] **Step 3: Add Compare button cell to drop-in variant child rows**

Same pattern in `_services.html.j2` for drop-in variant children.

- [ ] **Step 4: Add Compare button click handler**

In `_js.html.j2`, inside the `{% if fleet_meta %}` block, add an event delegation handler for Compare button clicks:

```javascript
document.addEventListener('click', function(e) {
  var btn = e.target.closest('.variant-compare-btn');
  if (!btn) return;

  var row = btn.closest('[data-variant-group]');
  if (!row) return;

  var group = row.getAttribute('data-variant-group');
  var section = row.getAttribute('data-snap-section');
  var list = row.getAttribute('data-snap-list');
  var idx = parseInt(row.getAttribute('data-snap-index'), 10);
  if (!section || !list || isNaN(idx)) return;

  var arr = resolveSnapshotRef(section, list);
  if (!arr || !arr[idx]) return;
  var comparisonItem = arr[idx];

  // Find the selected variant in the same group
  var selectedItem = null;
  document.querySelectorAll('[data-variant-group="' + group + '"]').forEach(function(sibling) {
    var sibIdx = parseInt(sibling.getAttribute('data-snap-index'), 10);
    var sibArr = resolveSnapshotRef(sibling.getAttribute('data-snap-section'), sibling.getAttribute('data-snap-list'));
    if (sibArr && sibArr[sibIdx] && sibArr[sibIdx].include) {
      selectedItem = sibArr[sibIdx];
    }
  });

  if (!selectedItem) {
    // Should be unreachable — radio groups always have one selected
    return;
  }

  showCompareModal(group, selectedItem, comparisonItem);
});
```

- [ ] **Step 5: Verify buttons render and modal opens**

Manual verification:
- Open a fleet report with content variants
- Non-selected variant rows show "Compare" button
- Selected variant row shows "selected" text
- Clicking Compare opens the modal with correct diff
- Clicking "Switch to this variant" toggles the radio, swaps button labels
- Clicking Close or backdrop dismisses without changes

- [ ] **Step 6: Run full test suite**

Run: `pytest tests/ -x -q`
Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/yoinkc/templates/report/_config.html.j2 src/yoinkc/templates/report/_containers.html.j2 src/yoinkc/templates/report/_services.html.j2 src/yoinkc/templates/report/_js.html.j2
git commit -m "feat(fleet): add Compare button to variant rows with modal diff

Non-selected variant rows show Compare button. Click opens modal
with inline diff against selected variant. Switch button toggles
radio and swaps button labels. Buttons on config, quadlet, and
drop-in variant rows.

Assisted-by: Claude Code"
```

---

### Task 5: Final Verification

- [ ] **Step 1: Run full test suite**

Run: `pytest tests/ -v`
Expected: All tests pass.

- [ ] **Step 2: End-to-end manual verification**

Using a fleet snapshot with content variants:

1. Open fleet report in refine mode via `yoinkc-refine`
2. Find a config file with multiple variants
3. Click Compare on a non-selected variant — modal opens with diff
4. Verify diff content: red lines for selected-only, blue for comparison-only, gray for shared
5. Verify subtitle shows correct host lists and prevalence for both variants
6. Click "Switch to this variant" — modal closes, radio toggles, selected/Compare labels swap
7. Click Compare on the now-non-selected variant (the previously selected one) — diff shows correctly with roles reversed
8. Click Close — modal dismisses, no changes
9. Click backdrop — modal dismisses, no changes
10. Verify non-fleet snapshots show no Compare buttons
11. Test with large file variant (if available) — verify 5000-line limit message
