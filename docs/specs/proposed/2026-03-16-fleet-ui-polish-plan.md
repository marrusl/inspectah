# Fleet UI Polish Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix three fleet-mode UI issues: table column widths, compare button disabled state, and editor variant accordion.

**Architecture:** Template-only and JS-only changes. No Python or schema modifications. Parts B and C are independent quick wins; Part A is a significant JS rework of `buildTree()` in the editor.

**Tech Stack:** Jinja2 templates, vanilla JS, PF6 CSS classes

**Spec:** `docs/specs/proposed/2026-03-16-fleet-ui-polish-design.md`

---

## Chunk 1: Part B — Table Column Width Pass

Add `pf-m-fit-content` to `<th>` elements for columns with short/fixed-width content. Only specific columns get this — name/path/description/content columns stay at default width.

### Task 1: Users & Groups table columns

**Files:**
- Modify: `src/yoinkc/templates/report/_users_groups.html.j2:23` (Users thead)
- Modify: `src/yoinkc/templates/report/_users_groups.html.j2:75` (Groups thead)

- [ ] **Step 1: Update Users table header (line 23)**

The current header is a single line with all `<th>` tags. Add `pf-m-fit-content` to: include toggle, UID/GID, Type, Strategy. Leave User, Shell, Home, Notes at default width.

Before:
```html
<thead><tr><th class="pf-v6-c-table__check" scope="col"></th><th scope="col">User</th><th scope="col">UID/GID</th><th scope="col">Shell</th><th scope="col">Home</th><th scope="col">Type</th><th scope="col">Strategy</th><th scope="col">Notes</th></tr></thead>
```

After:
```html
<thead><tr><th class="pf-v6-c-table__check pf-m-fit-content" scope="col"></th><th scope="col">User</th><th class="pf-m-fit-content" scope="col">UID/GID</th><th scope="col">Shell</th><th scope="col">Home</th><th class="pf-m-fit-content" scope="col">Type</th><th class="pf-m-fit-content" scope="col">Strategy</th><th scope="col">Notes</th></tr></thead>
```

- [ ] **Step 2: Update Groups table header (line 75)**

Before:
```html
<thead><tr><th class="pf-v6-c-table__check" scope="col"></th><th scope="col">Group</th><th scope="col">GID</th><th scope="col">Strategy</th><th scope="col">Members</th></tr></thead>
```

After:
```html
<thead><tr><th class="pf-v6-c-table__check pf-m-fit-content" scope="col"></th><th scope="col">Group</th><th class="pf-m-fit-content" scope="col">GID</th><th class="pf-m-fit-content" scope="col">Strategy</th><th scope="col">Members</th></tr></thead>
```

- [ ] **Step 3: Visual verification**

Run yoinkc against a driftify profile and open the report. Verify the Strategy column in Groups is no longer stretching to fill the row. Verify Members column has room.

- [ ] **Step 4: Commit**

```bash
git add src/yoinkc/templates/report/_users_groups.html.j2
git commit -m "fix: add pf-m-fit-content to users/groups table columns"
```

### Task 2: Config table columns

**Files:**
- Modify: `src/yoinkc/templates/report/_config.html.j2:5` (thead)

- [ ] **Step 1: Update config table header**

Add `pf-m-fit-content` to: include toggle, Kind, Category, rpm-Va flags. Leave Path, Diff, Fleet at default.

Find the `<thead>` line and add `pf-m-fit-content` to the appropriate `<th>` tags. The exact columns are: the `pf-v6-c-table__check` th, Kind, Category, and rpm-Va flags (if present as a separate column).

- [ ] **Step 2: Visual verification**

Open a fleet report with config files. Verify Kind and Category columns shrink to content width.

- [ ] **Step 3: Commit**

```bash
git add src/yoinkc/templates/report/_config.html.j2
git commit -m "fix: add pf-m-fit-content to config table columns"
```

### Task 3: Packages table columns

**Files:**
- Modify: `src/yoinkc/templates/report/_packages.html.j2` (multiple thead lines)

- [ ] **Step 1: Update dep tree table header**

Add `pf-m-fit-content` to: include toggle, Version, Dep count, Arch. Leave Leaf Package, Dependencies list, Details, Fleet at default.

- [ ] **Step 2: Update repos table header**

Add `pf-m-fit-content` to: include toggle, Source. Leave Path, Details, Fleet at default.

- [ ] **Step 3: Visual verification and commit**

```bash
git add src/yoinkc/templates/report/_packages.html.j2
git commit -m "fix: add pf-m-fit-content to packages table columns"
```

### Task 4: Non-RPM software table columns

**Files:**
- Modify: `src/yoinkc/templates/report/_non_rpm.html.j2` (multiple thead lines)

- [ ] **Step 1: Update ELF binaries table header**

Add `pf-m-fit-content` to: include toggle, Language, Linking. Leave Path, Shared Libraries at default.

- [ ] **Step 2: Update pip/npm packages table header**

Add `pf-m-fit-content` to: include toggle, Version. Leave Package, Path at default.

- [ ] **Step 3: Visual verification and commit**

```bash
git add src/yoinkc/templates/report/_non_rpm.html.j2
git commit -m "fix: add pf-m-fit-content to non-rpm table columns"
```

### Task 5: Network table columns

**Files:**
- Modify: `src/yoinkc/templates/report/_network.html.j2` (multiple thead lines)

- [ ] **Step 1: Update iptables table header**

Add `pf-m-fit-content` to: include toggle, IPv, Table, Chain. Leave Args at default.

- [ ] **Step 2: Update firewall zones table header**

Add `pf-m-fit-content` to: include toggle. Leave Zone, Services, Ports, Rich Rules, Fleet at default.

- [ ] **Step 3: Visual verification and commit**

```bash
git add src/yoinkc/templates/report/_network.html.j2
git commit -m "fix: add pf-m-fit-content to network table columns"
```

### Task 6: Remaining tables (containers, services, scheduled jobs, kernel/boot, selinux, storage)

**Files:**
- Modify: `src/yoinkc/templates/report/_containers.html.j2`
- Modify: `src/yoinkc/templates/report/_services.html.j2`
- Modify: `src/yoinkc/templates/report/_scheduled_jobs.html.j2`
- Modify: `src/yoinkc/templates/report/_kernel_boot.html.j2`
- Modify: `src/yoinkc/templates/report/_selinux.html.j2`
- Modify: `src/yoinkc/templates/report/_storage.html.j2`

- [ ] **Step 1: _containers.html.j2**

Add `pf-m-fit-content` to: include toggle column. Leave Unit, Image, Path, Content, Fleet at default.

- [ ] **Step 2: _services.html.j2**

Service state changes table: add `pf-m-fit-content` to: include toggle, Current, Default, Action. Leave Unit, Fleet at default.

Drop-in overrides table: add `pf-m-fit-content` to: include toggle. Leave Parent unit, Drop-in path, Content, Fleet at default.

- [ ] **Step 3: _scheduled_jobs.html.j2**

Cron-converted timers: add `pf-m-fit-content` to: include toggle. Leave Name, Cron Expression, Source File, Fleet at default.

Cron jobs: add `pf-m-fit-content` to: include toggle, Source, Action. Leave Path, Fleet at default.

At jobs: add `pf-m-fit-content` to: User. Leave File, Command at default.

- [ ] **Step 4: _kernel_boot.html.j2**

Sysctl overrides: add `pf-m-fit-content` to: include toggle, Runtime, Default. Leave Key, Source at default.

- [ ] **Step 5: _selinux.html.j2**

Booleans: add `pf-m-fit-content` to: Current, Default. Leave Boolean, Description at default.

- [ ] **Step 6: _storage.html.j2**

Add `pf-m-fit-content` to: Type. Leave Device, Mount at default.

- [ ] **Step 7: Visual spot-check across all updated tables and commit**

```bash
git add src/yoinkc/templates/report/_containers.html.j2 \
       src/yoinkc/templates/report/_services.html.j2 \
       src/yoinkc/templates/report/_scheduled_jobs.html.j2 \
       src/yoinkc/templates/report/_kernel_boot.html.j2 \
       src/yoinkc/templates/report/_selinux.html.j2 \
       src/yoinkc/templates/report/_storage.html.j2
git commit -m "fix: add pf-m-fit-content to remaining table columns

Covers containers, services, scheduled jobs, kernel/boot, selinux, storage."
```

---

## Chunk 2: Part C — Compare Button Guard

### Task 7: Disable compare buttons when no variant is selected

**Files:**
- Modify: `src/yoinkc/templates/report/_config.html.j2` (compare button markup)
- Modify: `src/yoinkc/templates/report/_services.html.j2` (compare button markup for drop-ins)
- Modify: `src/yoinkc/templates/report/_containers.html.j2` (compare button markup for quadlets)
- Modify: `src/yoinkc/templates/report/_js.html.j2` (variant change handler + new updateCompareButtons function)

- [ ] **Step 1: Add updateCompareButtons() function to _js.html.j2**

Add a new function near the compare button click handler (~line 860). This function checks all variant groups and enables/disables compare buttons based on whether any variant in each group has `include === true`.

```javascript
function updateCompareButtons(group) {
  /* Find all rows in this variant group */
  var rows = document.querySelectorAll('[data-variant-group="' + group + '"]');
  var hasSelected = false;
  rows.forEach(function(row) {
    var section = row.getAttribute('data-snap-section');
    var list = row.getAttribute('data-snap-list');
    var idx = parseInt(row.getAttribute('data-snap-index'), 10);
    var arr = resolveSnapshotRef(section, list);
    if (arr && arr[idx] && arr[idx].include) hasSelected = true;
  });
  /* Enable or disable compare buttons in this group */
  rows.forEach(function(row) {
    var btn = row.querySelector('.variant-compare-btn');
    if (!btn) return;
    if (hasSelected) {
      btn.removeAttribute('disabled');
      btn.classList.remove('pf-m-disabled');
    } else {
      btn.setAttribute('disabled', '');
      btn.classList.add('pf-m-disabled');
    }
  });
}
```

- [ ] **Step 2: Call updateCompareButtons() from the radio/toggle variant change handler**

In the existing `.include-toggle` change handler (around line 234–256 in `_js.html.j2`), after the loop that updates sibling variants' `include` state, add:

```javascript
updateCompareButtons(group);
```

Where `group` is the `data-variant-group` value already extracted in that handler.

- [ ] **Step 3: Call updateCompareButtons() from reset handler**

Find the reset handler. After it restores original snapshot state, iterate all unique variant groups and call `updateCompareButtons(group)` for each.

- [ ] **Step 4: Update templates to render disabled state initially**

All three variant-supporting templates (`_config.html.j2`, `_services.html.j2`, `_containers.html.j2`) render compare buttons inside a `{% for v in variants %}` loop. Each template follows the same pattern — the button appears inside `{% if not v.item.include %}`.

In each template, before the variant loop, compute whether any variant is selected:

```jinja2
{%- set group_has_selected = variants | selectattr('item.include', 'equalto', true) | list | length > 0 %}
```

Then update the compare button markup (same change in all three templates):

```html
<button class="pf-v6-c-button pf-m-link pf-m-small variant-compare-btn{% if not group_has_selected %} pf-m-disabled{% endif %}"{% if not group_has_selected %} disabled{% endif %}>Compare</button>
```

- [ ] **Step 5: Run existing tests**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
python -m pytest tests/ -x -q
```

All existing tests should pass — this is a template/JS-only change.

- [ ] **Step 6: Visual verification**

Open a fleet report where no variant hits the prevalence threshold (use the 3-host fleet test). Verify compare buttons are greyed out and unclickable. Select a variant via radio button, verify compare buttons on sibling variants become active. Click reset, verify they disable again.

- [ ] **Step 7: Commit**

```bash
git add src/yoinkc/templates/report/_config.html.j2 \
       src/yoinkc/templates/report/_services.html.j2 \
       src/yoinkc/templates/report/_containers.html.j2 \
       src/yoinkc/templates/report/_js.html.j2
git commit -m "fix: disable compare buttons when no fleet variant is selected

Add updateCompareButtons() function that enables/disables based on
whether any variant in the group has include===true. Called from
variant toggle handler and reset handler. Buttons render disabled
initially when no variant meets prevalence threshold."
```

---

## Chunk 3: Part A — Editor Variant Accordion

### Task 8: Rewrite buildTree() with accordion grouping

**Files:**
- Modify: `src/yoinkc/templates/report/_editor_js.html.j2:23-220` (buildTree function)

- [ ] **Step 1: Write the new buildTree() function**

Replace the existing `buildTree()` (lines ~23–220) with a new implementation. The function should:

1. Clear the `editor-tree-list` container
2. Iterate sections (`config/files`, `services/drop_ins`, `containers/quadlet_units`)
3. Group items by path (same logic as current code)
4. For single-variant items: render a flat clickable row (path only, no accordion)
5. For multi-variant items: render an accordion group:

**Accordion group HTML structure:**
```html
<div class="editor-variant-group" data-variant-path="/etc/ssh/sshd_config">
  <!-- Header -->
  <div class="editor-variant-header" onclick="toggleAccordion(this)">
    <span class="chevron">▼</span>
    <span class="path-dir">/etc/ssh/</span><span class="path-file">sshd_config</span>
    <span class="pf-v6-c-badge pf-m-read">2 variants</span>
  </div>
  <!-- Variant children -->
  <div class="editor-variant-children">
    <div class="editor-variant-row selected"
         data-section="config" data-list="files" data-index="0"
         onclick="selectFile('config','files',0,'/etc/ssh/sshd_config')">
      <div class="prevalence-bar"><div class="fill majority" style="width:67%"></div></div>
      <span class="host-chips"><!-- chips or count --></span>
      <span class="variant-pill selected-pill">selected</span>
    </div>
    <div class="editor-variant-row"
         data-section="config" data-list="files" data-index="1"
         onclick="selectFile('config','files',1,'/etc/ssh/sshd_config')">
      <div class="prevalence-bar"><div class="fill minority" style="width:33%"></div></div>
      <span class="host-chips"><!-- chips or count --></span>
      <span class="variant-pill switch-pill" onclick="event.stopPropagation(); switchVariantFromEditor(this)">use this variant</span>
    </div>
  </div>
</div>
```

Key implementation details:
- Sort variants by `fleet.count` descending (most prevalent first)
- Compute `percentage = Math.round((item.fleet.count / item.fleet.total) * 100)`
- Majority color when `percentage > 50`, minority otherwise
- Host chips: measure total chip width vs available space (row width minus ~120px for bar + pill). If chips overflow, show "N hosts" text instead.
- Individual chips get `max-width: 80px` and `text-overflow: ellipsis`
- All groups start expanded
- **Non-fleet fallback:** When `item.fleet` is absent (single-host mode), multi-variant grouping won't occur (there are no variants without fleet data). Single items render as flat entries without prevalence bars or host chips. Guard all fleet property access: use `(item.fleet && item.fleet.count) || 0` for sort, skip prevalence bar/chip rendering when `!item.fleet`. The existing `buildTree()` already uses this pattern.

- [ ] **Step 2: Add toggleAccordion() function**

```javascript
function toggleAccordion(header) {
  var children = header.nextElementSibling;
  var chevron = header.querySelector('.chevron');
  if (children.style.display === 'none') {
    children.style.display = '';
    chevron.textContent = '▼';
  } else {
    children.style.display = 'none';
    chevron.textContent = '►';
  }
}
```

- [ ] **Step 3: Add switchVariantFromEditor() function**

This function handles the "use this variant" pill click from the editor sidebar. It should:

1. Get `data-section`, `data-list`, `data-index` from the pill's parent row
2. Find all sibling variants (same path) using `findSiblingVariants()`
3. Set `include = false` on all siblings, `include = true` on this variant
4. Call `setDirty()`
5. Call `updateCompareButtons(group)` (from Part C)
6. Call `buildTree()` to re-render the file list with updated pills
7. Call `doSelectFile()` to reload the now-editable variant in the editor pane

```javascript
function switchVariantFromEditor(pill) {
  var row = pill.closest('.editor-variant-row');
  var section = row.getAttribute('data-section');
  var list = row.getAttribute('data-list');
  var idx = parseInt(row.getAttribute('data-index'), 10);
  var arr = resolveSnapshotRef(section, list);
  if (!arr || !arr[idx]) return;
  var item = arr[idx];
  var path = item.path;

  /* Deselect siblings */
  var siblings = findSiblingVariants(section, list, path);
  siblings.forEach(function(s) { s.item.include = false; });

  /* Select this variant */
  item.include = true;
  setDirty();

  /* Update compare buttons on config tab */
  var group = path; /* variant group key is the path */
  updateCompareButtons(group);

  /* Re-render tree and select this file */
  buildTree();
  doSelectFile(section, list, idx, path);
}
```

- [ ] **Step 4: Add CSS for the accordion**

Add styles to the `<style>` block in `_editor.html.j2` (this file contains the editor sidebar markup and existing editor styles):

```css
.editor-variant-group { margin-bottom: 2px; }

.editor-variant-header {
  display: flex; align-items: center; gap: 6px;
  padding: 7px 10px; background: rgba(255,255,255,0.08);
  border-radius: 4px; cursor: pointer; font-size: 12px;
}
.editor-variant-header .chevron { color: #888; font-size: 10px; width: 12px; }
.editor-variant-header .path-dir { color: #888; }
.editor-variant-header .path-file { color: #ccc; font-weight: 600; }

.editor-variant-row {
  display: flex; align-items: center; gap: 6px;
  padding: 6px 10px; margin: 2px 0 2px 20px;
  border-radius: 4px; cursor: pointer; font-size: 12px;
}
.editor-variant-row.selected {
  background: rgba(0, 102, 204, 0.15);
  border-left: 3px solid #0066cc;
}
.editor-variant-row:hover { background: rgba(255,255,255,0.05); }
.editor-variant-row.selected:hover { background: rgba(0, 102, 204, 0.2); }

.prevalence-bar {
  width: 36px; height: 5px; flex-shrink: 0;
  background: #333; border-radius: 3px; overflow: hidden;
}
.prevalence-bar .fill { height: 100%; border-radius: 3px; }
.prevalence-bar .fill.majority { background: #0066cc; }
.prevalence-bar .fill.minority { background: #cc8800; }

.host-chip {
  background: rgba(100,160,220,0.2); color: #88ccff;
  padding: 1px 5px; border-radius: 3px; font-size: 10px;
  max-width: 80px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
}
.editor-variant-row:not(.selected) .host-chip {
  background: rgba(200,170,100,0.15); color: #ccaa66;
}
.host-count { font-size: 11px; color: #ccaa66; }

.variant-pill {
  margin-left: auto; padding: 1px 6px;
  border-radius: 10px; font-size: 10px; white-space: nowrap;
}
.selected-pill { background: #1a7a3a; color: #fff; }
.switch-pill {
  background: #0066cc; color: #fff; cursor: pointer;
  opacity: 0; transition: opacity 0.15s;
}
.editor-variant-row:hover .switch-pill { opacity: 1; }

/* Single-variant flat entry */
.editor-single-file {
  padding: 7px 10px 7px 24px;
  cursor: pointer; font-size: 12px; color: #ccc;
  border-radius: 4px;
}
.editor-single-file:hover { background: rgba(255,255,255,0.05); }
.editor-single-file.pf-m-current { background: rgba(0,102,204,0.15); }
```

- [ ] **Step 5: Update doSelectFile() to highlight the active row**

The current `doSelectFile()` sets `.pf-m-current` on tree-view nodes. Update it to work with the new accordion structure:

```javascript
/* Clear previous selection highlight */
document.querySelectorAll('.editor-variant-row.pf-m-current, .editor-single-file.pf-m-current')
  .forEach(function(el) { el.classList.remove('pf-m-current'); });

/* Highlight current row */
var currentRow = document.querySelector(
  '.editor-variant-row[data-section="' + section + '"][data-list="' + list + '"][data-index="' + index + '"], ' +
  '.editor-single-file[data-section="' + section + '"][data-list="' + list + '"][data-index="' + index + '"]'
);
if (currentRow) currentRow.classList.add('pf-m-current');
```

- [ ] **Step 6: Run existing tests**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
python -m pytest tests/ -x -q
```

- [ ] **Step 7: Visual verification**

Open a fleet report in refine mode. Verify:
- Multi-variant files show accordion with chevron, path, badge
- Variant rows show prevalence bars with correct colors
- Host chips display (or "N hosts" fallback)
- "selected" pill on the selected variant
- "use this variant" pill appears on hover
- Clicking a variant row loads it in editor (editable if selected, read-only if not)
- Clicking "use this variant" pill switches selection
- Single-variant files render flat
- Accordion expand/collapse works

Also open a **non-fleet** (single-host) report in refine mode and verify the editor file list renders as a flat list with no variant grouping, prevalence bars, or host chips. This confirms the non-fleet fallback works.

- [ ] **Step 8: Commit**

```bash
git add src/yoinkc/templates/report/_editor_js.html.j2 \
       src/yoinkc/templates/report/_editor.html.j2
git commit -m "feat: replace editor tree-view with variant accordion

Custom accordion grouping for multi-variant fleet files. Each group
shows prevalence bars, host chips (with overflow fallback), selected
pill, and hover-to-switch pill. Single-variant files render flat.

Supersedes PF6 tree-view approach from 2026-03-16 editor variants spec."
```

### Task 9: Integration test — end-to-end fleet workflow

**Prerequisite:** Tasks 1–8 must all be complete before running integration tests. This task validates all three parts together.

- [ ] **Step 1: Run the fleet test script**

```bash
cd /Users/mrussell/Work/bootc-migration/driftify
bash run-fleet-test.sh
```

This runs 3 driftify profiles, aggregates them at 66% threshold, and produces a fleet tarball.

- [ ] **Step 2: Open the fleet report in refine mode and verify all three parts**

1. **Part B:** Check tables across multiple tabs — Strategy columns should be compact, include toggles shouldn't stretch
2. **Part C:** Find a variant group where no variant is selected (if any exist at 66%). Verify compare buttons are disabled. Select a variant, verify compare buttons enable.
3. **Part A:** Switch to editor tab. Verify accordion grouping, prevalence bars, host chips, pill interactions.

- [ ] **Step 3: Test cross-tab sync**

1. In editor, click "use this variant" on a non-selected variant
2. Switch to config tab — verify the radio button reflects the new selection
3. In config tab, select a different variant via radio button
4. Switch to editor tab — verify the accordion shows the updated selection

- [ ] **Step 4: Test reset**

Click reset button. Verify:
- Editor accordion reverts to original selection state
- Compare buttons re-disable if no variant is selected post-reset
- Config tab radio buttons reflect reset state
