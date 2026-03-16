# Fleet Refine Lifecycle Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable full refine mode on fleet snapshots — prevalence slider, variant radio groups, client-side triage recalculation, and reset integration.

**Architecture:** Entirely client-side. No Python/server changes needed. yoinkc already detects fleet metadata and passes it to templates. All new logic lives in Jinja2 templates, JavaScript, and CSS. The slider recalculates `include` fields on the in-memory `snapshot` object; the existing re-render flow sends the modified snapshot to yoinkc for Containerfile generation.

**Tech Stack:** Jinja2 templates, vanilla JavaScript (ES5 — no modules/bundling), PF6 CSS classes, existing yoinkc refine infrastructure

**Spec:** `docs/specs/proposed/2026-03-16-fleet-refine-lifecycle-design.md`

---

## File Structure

| File | Responsibility | Changes |
|------|---------------|---------|
| `src/yoinkc/templates/report/_toolbar.html.j2` | Refine action bar | Add slider HTML (fleet-conditional) |
| `src/yoinkc/templates/report/_css.html.j2` | All report styling | Add slider + Apply button styles |
| `src/yoinkc/templates/report/_js.html.j2` | Main report JS | Add slider logic, triage recalc, variant radio groups, update reset |
| `src/yoinkc/templates/report/_config.html.j2` | Config file rendering | Add `data-variant-group` to variant rows |
| `src/yoinkc/templates/report/_containers.html.j2` | Container rendering | Add `data-variant-group` to quadlet variant rows |
| `src/yoinkc/templates/report/_services.html.j2` | Service rendering | Add `data-variant-group` to drop-in variant rows |
| `src/yoinkc/templates/report/_sidebar.html.j2` | Sidebar navigation | Add triage count badges to nav items |

No Python changes. All work is in the template layer.

---

## Chunk 1: Slider UI & Preview

### Task 1: Slider HTML in Toolbar

**Files:**
- Modify: `src/yoinkc/templates/report/_toolbar.html.j2`
- Modify: `src/yoinkc/templates/report/_css.html.j2`

- [ ] **Step 1: Add slider HTML to toolbar**

In `_toolbar.html.j2`, add a fleet-conditional slider group after the Reset button but before the status text. Wrap in `{% if fleet_meta and refine_mode %}`:

```html
{% if fleet_meta and refine_mode %}
<div class="toolbar-divider"></div>
<div class="fleet-slider-group">
  <label class="fleet-slider-label" for="prevalence-slider">Prevalence:</label>
  <input type="range" id="prevalence-slider" class="fleet-slider"
         min="1" max="100"
         value="{{ fleet_meta.min_prevalence }}">
  <span id="prevalence-value" class="fleet-slider-value">{{ fleet_meta.min_prevalence }}%</span>
  <span id="prevalence-preview" class="fleet-slider-preview"></span>
  <button id="btn-apply-prevalence" class="pf-v6-c-button pf-m-secondary fleet-slider-apply"
          style="display: none;">Apply</button>
</div>
{% endif %}
```

- [ ] **Step 2: Add slider CSS**

In `_css.html.j2`, add styling for the slider group. Place near the existing `#exclude-toolbar` styles (~line 130):

```css
.toolbar-divider { border-left: 1px solid var(--pf-t--global--border--color--default); height: 24px; }
.fleet-slider-group { display: flex; align-items: center; gap: 0.5rem; }
.fleet-slider-label { font-weight: 600; font-size: 0.8rem; white-space: nowrap; }
.fleet-slider { width: 120px; cursor: pointer; }
.fleet-slider-value { font-weight: 600; min-width: 2.5rem; text-align: right; }
.fleet-slider-preview { font-size: 0.75rem; color: var(--pf-t--global--text--color--subtle); white-space: nowrap; }
.fleet-slider-apply { font-size: 0.75rem !important; padding: 0.2rem 0.6rem !important; }
```

- [ ] **Step 3: Verify toolbar renders slider in fleet refine mode**

Run the existing test suite to ensure no regressions:
```bash
pytest tests/ -x -q
```

Then manually verify: generate a fleet snapshot, run `yoinkc-refine` on it, and confirm the slider appears in the toolbar.

- [ ] **Step 4: Commit**

```bash
git add src/yoinkc/templates/report/_toolbar.html.j2 src/yoinkc/templates/report/_css.html.j2
git commit -m "feat(fleet): add prevalence slider to refine toolbar

Slider appears in the sticky toolbar when fleet_meta and refine_mode
are both set. Range 1-100, initial value from fleet min_prevalence.
Apply button hidden until value changes.

Assisted-by: Claude Code"
```

---

### Task 2: Slider Drag Preview

**Files:**
- Modify: `src/yoinkc/templates/report/_js.html.j2`

- [ ] **Step 1: Add slider preview logic**

In `_js.html.j2`, inside the `{% if fleet_meta and refine_mode %}` block (near line 590), add all fleet refine JS. All fleet functions share a single scope (one IIFE containing the slider, apply, preview, and helper functions):

```javascript
{% if fleet_meta and refine_mode %}
(function() {
  var slider = document.getElementById('prevalence-slider');
  var valueLabel = document.getElementById('prevalence-value');
  var preview = document.getElementById('prevalence-preview');
  var applyBtn = document.getElementById('btn-apply-prevalence');
  if (!slider) return;

  // Store current threshold on the slider element for cross-scope access (reset)
  slider.setAttribute('data-current-threshold', '{{ fleet_meta.min_prevalence }}');

  function prevalenceInclude(count, total, minPrev) {
    return (count * 100) >= (minPrev * total);
  }

  function countAtThreshold(threshold) {
    // Count from snapshot data, not DOM, to avoid double-counting
    // from variant group headers that share snap_index with first child
    var included = 0, excluded = 0;
    var sections = ['rpm', 'config', 'services', 'network', 'selinux',
                    'non_rpm_software', 'containers', 'scheduled_tasks'];
    sections.forEach(function(secName) {
      var sec = snapshot[secName];
      if (!sec) return;
      Object.keys(sec).forEach(function(listName) {
        var items = sec[listName];
        if (!Array.isArray(items)) return;
        items.forEach(function(item) {
          if (!item.fleet) return;
          if (prevalenceInclude(item.fleet.count, item.fleet.total, threshold)) {
            included++;
          } else {
            excluded++;
          }
        });
      });
    });
    return { included: included, excluded: excluded };
  }

  slider.addEventListener('input', function() {
    var val = parseInt(this.value, 10);
    var currentThreshold = parseInt(slider.getAttribute('data-current-threshold'), 10);
    valueLabel.textContent = val + '%';
    var counts = countAtThreshold(val);
    preview.textContent = '(' + counts.included + ' included, ' + counts.excluded + ' excluded)';
    applyBtn.style.display = (val !== currentThreshold) ? '' : 'none';
  });

  applyBtn.addEventListener('click', function() {
    var val = parseInt(slider.value, 10);
    applyPrevalenceThreshold(val);
    slider.setAttribute('data-current-threshold', val);
    applyBtn.style.display = 'none';
    preview.textContent = '';
  });

  // applyPrevalenceThreshold is defined here — see Task 4
  // It must be inside this IIFE to access prevalenceInclude

})();
{% endif %}
```

**Key design decisions:**
- `countAtThreshold()` iterates snapshot data (not DOM) to avoid double-counting from variant group headers
- `data-current-threshold` attribute on slider enables cross-scope access from `resetToOriginal()`
- All fleet refine functions live inside one IIFE; `applyPrevalenceThreshold()` (Task 4) is added inside this same IIFE

- [ ] **Step 2: Verify slider preview works**

Manual verification: open a fleet report in refine mode, drag the slider, and confirm:
- Value label updates during drag
- Preview count shows included/excluded counts
- Apply button appears when value differs from initial

- [ ] **Step 3: Commit**

```bash
git add src/yoinkc/templates/report/_js.html.j2
git commit -m "feat(fleet): add slider drag preview with live counts

Value label and preview count update in real-time during drag.
Apply button appears when slider value differs from current
threshold.

Assisted-by: Claude Code"
```

---

## Chunk 2: Apply Logic, Triage Recalc & Variant Radio Groups

### Task 3: Variant Radio Group Attributes

**Files:**
- Modify: `src/yoinkc/templates/report/_config.html.j2`
- Modify: `src/yoinkc/templates/report/_containers.html.j2`
- Modify: `src/yoinkc/templates/report/_services.html.j2`

- [ ] **Step 1: Add `data-variant-group` to config variant rows**

In `_config.html.j2`, find the variant group rendering (inside the `{% if fleet_meta and config_variant_groups %}` block). On each variant's inner `<tr>` that has `data-snap-section="config"`, add a `data-variant-group` attribute with the file path as the group key:

```html
data-variant-group="{{ path }}"
```

Add this attribute only to child variant rows inside the nested `<table>` within `fleet-variant-children`, NOT to the parent group header row. The parent group header row and first child variant share the same `data-snap-index` — putting the attribute on both would cause double-toggling. The radio behavior operates on child rows only.

- [ ] **Step 2: Add `data-variant-group` to quadlet variant rows**

In `_containers.html.j2`, find the quadlet variant group rendering. Add the same `data-variant-group="{{ path }}"` attribute to variant rows.

- [ ] **Step 3: Add `data-variant-group` to drop-in variant rows**

In `_services.html.j2`, find the drop-in variant group rendering. Add `data-variant-group="{{ path }}"` to variant rows.

- [ ] **Step 4: Verify attributes render correctly**

Run existing tests:
```bash
pytest tests/ -x -q
```

Inspect rendered HTML of a fleet report to confirm `data-variant-group` attributes are present on variant rows.

- [ ] **Step 5: Commit**

```bash
git add src/yoinkc/templates/report/_config.html.j2 src/yoinkc/templates/report/_containers.html.j2 src/yoinkc/templates/report/_services.html.j2
git commit -m "feat(fleet): add data-variant-group attribute to variant rows

Config files, quadlet units, and drop-in variant rows now carry
a data-variant-group attribute keyed by file path. Enables JS
radio-button behavior within variant groups.

Assisted-by: Claude Code"
```

---

### Task 4: Variant Radio Toggle & Apply Threshold

**Files:**
- Modify: `src/yoinkc/templates/report/_js.html.j2`

- [ ] **Step 1: Add variant radio-group toggle logic**

In `_js.html.j2`, modify the existing include toggle handler (~line 200). After the existing `arr[idx].include = this.checked` line, add variant radio logic:

```javascript
// Variant radio-group: deselect siblings
var variantGroup = tr.getAttribute('data-variant-group');
if (variantGroup && this.checked) {
  document.querySelectorAll('[data-variant-group="' + variantGroup + '"]').forEach(function(sibling) {
    if (sibling === tr) return;
    var sibSection = sibling.getAttribute('data-snap-section');
    var sibList = sibling.getAttribute('data-snap-list');
    var sibIdx = parseInt(sibling.getAttribute('data-snap-index'), 10);
    if (!sibSection || !sibList || isNaN(sibIdx)) return;
    var sibArr = resolveSnapshotRef(sibSection, sibList);
    if (sibArr && sibArr[sibIdx] !== undefined) {
      sibArr[sibIdx].include = false;
    }
    var sibCb = sibling.querySelector('.include-toggle');
    if (sibCb) sibCb.checked = false;
    sibling.classList.add('excluded');
  });
}
```

This ensures that within a variant group, enabling one variant auto-excludes all others.

- [ ] **Step 2: Add `applyPrevalenceThreshold()` function**

In `_js.html.j2`, inside the fleet refine IIFE from Task 2 (before the closing `})();`), add the threshold application function. It must be inside the same IIFE to access `prevalenceInclude()`:

```javascript
function applyPrevalenceThreshold(threshold) {
  // Phase 1: Recalculate all include values based on threshold
  document.querySelectorAll('[data-snap-section]').forEach(function(el) {
    var section = el.getAttribute('data-snap-section');
    var list = el.getAttribute('data-snap-list');
    var idx = parseInt(el.getAttribute('data-snap-index'), 10);
    if (!section || !list || isNaN(idx) || idx < 0) return;
    var arr = resolveSnapshotRef(section, list);
    if (!arr || !arr[idx] || !arr[idx].fleet) return;
    var f = arr[idx].fleet;
    var shouldInclude = prevalenceInclude(f.count, f.total, threshold);
    arr[idx].include = shouldInclude;
    var cb = el.querySelector('.include-toggle');
    if (cb) cb.checked = shouldInclude;
    el.classList.toggle('excluded', !shouldInclude);
  });

  // Phase 2: Apply variant radio constraint — only most prevalent variant per group
  var groups = {};
  document.querySelectorAll('[data-variant-group]').forEach(function(el) {
    var group = el.getAttribute('data-variant-group');
    if (!groups[group]) groups[group] = [];
    var section = el.getAttribute('data-snap-section');
    var list = el.getAttribute('data-snap-list');
    var idx = parseInt(el.getAttribute('data-snap-index'), 10);
    var arr = resolveSnapshotRef(section, list);
    if (!arr || !arr[idx]) return;
    groups[group].push({ el: el, item: arr[idx], section: section, list: list, idx: idx });
  });

  Object.keys(groups).forEach(function(group) {
    var variants = groups[group];
    // Sort by prevalence descending
    variants.sort(function(a, b) {
      var ac = a.item.fleet ? a.item.fleet.count : 0;
      var bc = b.item.fleet ? b.item.fleet.count : 0;
      return bc - ac;
    });
    var foundIncluded = false;
    variants.forEach(function(v) {
      if (v.item.include && !foundIncluded) {
        foundIncluded = true;  // Keep the most prevalent included variant
      } else if (v.item.include) {
        // Deselect less prevalent variants
        v.item.include = false;
        var cb = v.el.querySelector('.include-toggle');
        if (cb) cb.checked = false;
        v.el.classList.add('excluded');
      }
    });
  });

  // Phase 3: Update UI
  updateToolbar();
  setDirty(!isSnapshotClean());
  recalcTriageCounts();
}
```

- [ ] **Step 3: Verify variant radio behavior**

Manual verification:
- Open fleet report in refine mode
- Find a config file with multiple variants
- Toggle on a non-primary variant — confirm the primary auto-deselects
- Move slider to a lower threshold, click Apply — confirm only most prevalent variant per path is included

- [ ] **Step 4: Commit**

```bash
git add src/yoinkc/templates/report/_js.html.j2
git commit -m "feat(fleet): variant radio groups and threshold apply

Toggle within variant group auto-deselects siblings (radio behavior).
applyPrevalenceThreshold() recalculates all includes, then enforces
radio constraint keeping only the most prevalent variant per path.

Assisted-by: Claude Code"
```

---

### Task 5: Client-Side Triage Recalculation

**Files:**
- Modify: `src/yoinkc/templates/report/_sidebar.html.j2`
- Modify: `src/yoinkc/templates/report/_js.html.j2`

**Context:** The sidebar currently shows only navigation links with no item
counts. Triage counts are computed server-side by `_triage.py` and rendered
in the main content area, not the sidebar. To support client-side
recalculation after slider/toggle changes, we need to:
1. Add count badges to sidebar nav items
2. Build a JS function that recomputes counts from the in-memory snapshot

- [ ] **Step 1: Add count badges to sidebar nav items**

In `_sidebar.html.j2`, find each sidebar navigation link (e.g., RPM Packages,
Config Files, etc.). Add a `<span>` badge after each label with a
`data-triage-section` attribute matching the snapshot section name. Example:

```html
<a href="#rpm">RPM Packages <span class="triage-count" data-triage-section="rpm">{{ counts.packages_added }}</span></a>
```

The exact element structure depends on how the sidebar currently renders nav
items. The implementer should inspect `_sidebar.html.j2` and adapt. Each nav
item that corresponds to a snapshot section gets a `triage-count` badge
initialized with the server-rendered count.

Add minimal badge styling in `_css.html.j2`:
```css
.triage-count { font-size: 0.75rem; opacity: 0.7; margin-left: 0.25rem; }
.triage-count::before { content: "("; }
.triage-count::after { content: ")"; }
```

- [ ] **Step 2: Add `recalcTriageCounts()` function**

In `_js.html.j2`, add a function that recomputes counts from the in-memory
snapshot and updates the sidebar badges. Counts are based on items with
`.include !== false` (items without an `.include` field are always counted).

```javascript
function recalcTriageCounts() {
  // Map sidebar section names to their snapshot list fields
  var sectionLists = {
    'rpm': ['packages_added'],
    'config': ['files'],
    'services': ['state_changes', 'drop_ins'],
    'network': ['firewall_zones'],
    'selinux': ['port_labels'],
    'non_rpm_software': ['items'],
    'containers': ['quadlet_units', 'compose_files'],
    'scheduled_tasks': ['generated_timer_units', 'cron_jobs'],
  };

  Object.keys(sectionLists).forEach(function(sectionName) {
    var sec = snapshot[sectionName];
    if (!sec) return;
    var count = 0;
    sectionLists[sectionName].forEach(function(listName) {
      var items = sec[listName];
      if (!items) return;
      items.forEach(function(item) {
        if (item.include !== false) count++;
      });
    });
    var el = document.querySelector('.triage-count[data-triage-section="' + sectionName + '"]');
    if (el) el.textContent = count;
  });
}
```

Note: The list field names (e.g., `packages_added`, `state_changes`) must
match the actual snapshot schema field names. The implementer should verify
these against `src/yoinkc/schema.py` and adjust if any are incorrect.

- [ ] **Step 3: Wire recalcTriageCounts into existing toggle handler**

At the end of the existing toggle change handler (~line 217, after `setDirty(!isSnapshotClean())`), add:

```javascript
recalcTriageCounts();
```

This ensures triage counts update when individual items are toggled, not just when the slider is used.

- [ ] **Step 4: Verify triage counts update**

Manual verification: toggle an item's include, confirm sidebar count decreases. Move slider, click Apply, confirm counts update to match new threshold.

- [ ] **Step 5: Commit**

```bash
git add src/yoinkc/templates/report/_js.html.j2 src/yoinkc/templates/report/_sidebar.html.j2 src/yoinkc/templates/report/_css.html.j2
git commit -m "feat(fleet): client-side triage count recalculation

recalcTriageCounts() recomputes sidebar section counts from the
in-memory snapshot. Called after toggle changes and slider apply.

Assisted-by: Claude Code"
```

---

## Chunk 3: Reset Integration & Final Verification

### Task 6: Reset with Slider

**Files:**
- Modify: `src/yoinkc/templates/report/_js.html.j2`

- [ ] **Step 1: Update `resetToOriginal()` to reset slider**

In `_js.html.j2`, find the `resetToOriginal()` function (~line 364). At the end of the function, before the toast message, add slider reset logic:

```javascript
// Reset prevalence slider to original value
var slider = document.getElementById('prevalence-slider');
var valueLabel = document.getElementById('prevalence-value');
var applyBtn = document.getElementById('btn-apply-prevalence');
var preview = document.getElementById('prevalence-preview');
if (slider && snapshot.meta && snapshot.meta.fleet) {
  var origThreshold = originalSnapshot.meta.fleet.min_prevalence;
  slider.value = origThreshold;
  if (valueLabel) valueLabel.textContent = origThreshold + '%';
  if (applyBtn) applyBtn.style.display = 'none';
  if (preview) preview.textContent = '';
}
```

- [ ] **Step 2: Update `data-current-threshold` in reset**

The slider IIFE (Task 2) already uses `data-current-threshold` on the slider element for cross-scope access. In `resetToOriginal()`, also reset this attribute:

```javascript
if (slider) slider.setAttribute('data-current-threshold', origThreshold);
```

- [ ] **Step 3: Update reset confirmation to use PF6 modal**

The existing reset uses `confirm()`. Keep using `confirm()` for now — PF6 modal migration is cosmetic and out of scope.

- [ ] **Step 4: Wire `recalcTriageCounts()` into reset**

At the end of `resetToOriginal()`, after `setDirty(false)`, call:

```javascript
recalcTriageCounts();
```

- [ ] **Step 5: Verify reset restores slider**

Manual verification:
- Move slider to a new value, click Apply
- Toggle a few items manually
- Click Reset, confirm "Reset all selections?"
- Verify: slider returns to original value, all toggles restore, triage counts reset

- [ ] **Step 6: Commit**

```bash
git add src/yoinkc/templates/report/_js.html.j2
git commit -m "feat(fleet): reset button restores slider to original threshold

resetToOriginal() now resets the prevalence slider value and
currentThreshold to the original min_prevalence. Also calls
recalcTriageCounts() to update sidebar counts.

Assisted-by: Claude Code"
```

---

### Task 7: Final Verification

- [ ] **Step 1: Run full test suite**

```bash
pytest tests/ -v
```

Expected: All tests pass. No regressions.

- [ ] **Step 2: End-to-end manual verification**

Using a fleet snapshot with at least 3 hosts and content variants:

1. Run `yoinkc-refine fleet-output.tar.gz`
2. Verify slider appears in toolbar with correct initial value
3. Drag slider — preview counts update live
4. Click Apply at a lower threshold — more items become included, triage counts update
5. Find a config file with variants — toggle a non-primary variant, confirm primary auto-deselects
6. Click Re-render — Containerfile regenerates with new selections
7. Click Reset — everything restores to original state including slider
8. Verify non-fleet snapshots still work normally (no slider, no variant groups)

- [ ] **Step 3: Verify fleet detection**

Confirm that `yoinkc-refine` on a non-fleet tarball does NOT show the slider or variant group behavior. The `{% if fleet_meta and refine_mode %}` guard should prevent any fleet UI from appearing.
