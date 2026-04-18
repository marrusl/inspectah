# Report Search & Bulk Controls Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add per-card search/filter and bulk Include All / Exclude All controls to the inspectah interactive HTML report.

**Architecture:** Pure client-side. A new Jinja2 macro renders a toolbar (search + bulk buttons) inside each filterable card. JavaScript handles filtering via `data-search-text` attributes, batched mutations for bulk operations, and a state machine for repo-grouped packages. No server-side changes.

**Tech Stack:** Jinja2 templates, vanilla JavaScript (embedded in `_js.html.j2`), PatternFly 6 CSS classes.

**Spec:** `docs/specs/proposed/2026-04-07-report-search-bulk-controls-design.md` (v3)

---

## File Map

| File | Action | Responsibility |
|------|--------|----------------|
| `src/inspectah/templates/report/_macros.html.j2` | Modify | New `card_toolbar()` macro |
| `src/inspectah/templates/report/_packages.html.j2` | Modify | `data-search-text`, `data-group`, toolbar insertion |
| `src/inspectah/templates/report/_services.html.j2` | Modify | `data-search-text`, toolbar insertion (2 cards) |
| `src/inspectah/templates/report/_config.html.j2` | Modify | `data-search-text`, toolbar insertion |
| `src/inspectah/templates/report/_network.html.j2` | Modify | `data-search-text`, toolbar insertion (firewall direct rules only) |
| `src/inspectah/templates/report/_containers.html.j2` | Modify | `data-search-text`, toolbar insertion (2 cards) |
| `src/inspectah/templates/report/_non_rpm.html.j2` | Modify | `data-search-text`, toolbar insertion (2 cards) |
| `src/inspectah/templates/report/_kernel_boot.html.j2` | Modify | `data-search-text`, toolbar insertion (sysctl only) |
| `src/inspectah/templates/report/_users_groups.html.j2` | Modify | `data-search-text`, toolbar insertion (2 tables) |
| `src/inspectah/templates/report/_js.html.j2` | Modify | `initCardSearch()`, `syncToolbar()`, `batchToggleItems()`, `updateGroupState()`, integration hooks |
| `src/inspectah/templates/report/_toolbar.html.j2` | Modify | CSS for card toolbar row |

---

### Task 1: Add `data-search-text` attributes to all filterable card templates

This is the foundation. Every filterable row/div gets a `data-search-text` attribute set by the Jinja2 renderer to the item's primary identifier. No JavaScript yet — just wiring up the search targets.

**Files:**
- Modify: `src/inspectah/templates/report/_packages.html.j2`
- Modify: `src/inspectah/templates/report/_services.html.j2`
- Modify: `src/inspectah/templates/report/_config.html.j2`
- Modify: `src/inspectah/templates/report/_network.html.j2`
- Modify: `src/inspectah/templates/report/_containers.html.j2`
- Modify: `src/inspectah/templates/report/_non_rpm.html.j2`
- Modify: `src/inspectah/templates/report/_kernel_boot.html.j2`
- Modify: `src/inspectah/templates/report/_users_groups.html.j2`

- [ ] **Step 1: Add `data-search-text` to package rows in `_packages.html.j2`**

Find every `<tr` that has `data-snap-section` and `data-snap-list` for packages. Add `data-search-text="{{ pkg.name }}"` (where `pkg` is the loop variable for the package item). This includes both the leaf packages table and the auto-dependency packages table.

Also add `data-group="{{ repo_name }}"` to repo group header rows and their child package rows, where `repo_name` is the repository group key. This enables group-aware filtering.

- [ ] **Step 2: Add `data-search-text` to service rows in `_services.html.j2`**

For the enabled/disabled units table: add `data-search-text="{{ s.unit }}"` on each `<tr data-snap-section="services" data-snap-list="state_changes">`.

For the drop-in overrides table (both fleet-variant parent rows and single-item rows): add `data-search-text="{{ d.path }}"` on each `<tr data-snap-section="services" data-snap-list="drop_ins">`.

- [ ] **Step 3: Add `data-search-text` to config rows in `_config.html.j2`**

For fleet-variant parent rows: add `data-search-text="{{ primary.item.path }}"` on the `.fleet-variant-group` `<tr>`.
For single-item rows: add `data-search-text="{{ item.path }}"` on each `<tr data-snap-section="config_files">`.

- [ ] **Step 4: Add `data-search-text` to network firewall direct rules in `_network.html.j2`**

Add `data-search-text="{{ rule.args }}"` on each `<tr data-snap-section="network" data-snap-list="firewall_direct_rules">`. Only this card — other network cards are read-only.

- [ ] **Step 5: Add `data-search-text` to container rows in `_containers.html.j2`**

For quadlet units (fleet-variant parent rows): add `data-search-text="{{ primary.item.path }}"`.
For quadlet units (single-item rows): add `data-search-text="{{ u.path }}"`.
For compose file divs: add `data-search-text="{{ c.path }}"` on each `<div data-snap-section="containers" data-snap-list="compose_files">`.

- [ ] **Step 6: Add `data-search-text` to non-RPM rows in `_non_rpm.html.j2`**

For compiled binaries: add `data-search-text="{{ b.path }}"` on binary table rows.
For system pip packages: add `data-search-text="{{ p.name }}"` on pip package rows.

- [ ] **Step 7: Add `data-search-text` to sysctl rows in `_kernel_boot.html.j2`**

Add `data-search-text="{{ s.key }}"` on each `<tr data-snap-section="kernel_boot" data-snap-list="sysctl_overrides">`. Only the sysctl card — other kernel_boot cards are read-only.

- [ ] **Step 8: Add `data-search-text` to user/group rows in `_users_groups.html.j2`**

For users table: add `data-search-text="{{ u.name }}"` on user rows.
For groups table: add `data-search-text="{{ g.name }}"` on group rows.

- [ ] **Step 9: Verify by generating a report**

Run: `inspectah inspect --from-snapshot tests/fixtures/<snapshot>.json -o /tmp/test-report`
Open the generated HTML and verify `data-search-text` attributes appear on the expected rows via browser dev tools.

- [ ] **Step 10: Commit**

```bash
git add src/inspectah/templates/report/
git commit -m "feat(report): add data-search-text attributes to all filterable cards"
```

---

### Task 2: Create `card_toolbar` Jinja2 macro and CSS

Build the toolbar component that will be inserted into each filterable card. This task covers the HTML macro and the CSS styling. No JavaScript wiring yet — the buttons and search input render but don't function.

**Files:**
- Modify: `src/inspectah/templates/report/_macros.html.j2`
- Modify: `src/inspectah/templates/report/_toolbar.html.j2` (CSS)

- [ ] **Step 1: Add `card_toolbar` macro to `_macros.html.j2`**

Add after the existing `card_toggle` macro:

```jinja2
{# ── Filterable card toolbar (search + bulk controls) ──────────────────── #}
{% macro card_toolbar(card_id, item_count, card_label) -%}
<div class="card-toolbar" data-card-id="{{ card_id }}" data-total-count="{{ item_count }}">
  <div class="card-toolbar-left">
    <input type="text"
           class="pf-v6-c-form-control card-search-input"
           role="searchbox"
           aria-label="Search {{ card_label }}"
           aria-controls="{{ card_id }}"
           placeholder="Search {{ card_label | lower }}..."
           data-card-id="{{ card_id }}" />
    <span class="card-toolbar-filter-count" aria-live="polite" style="display:none;"></span>
  </div>
  <div class="card-toolbar-right">
    <span class="card-toolbar-included-count"></span>
    <span class="card-toolbar-warning-indicator" role="status" style="display:none;"></span>
    <button type="button"
            class="pf-v6-c-button pf-m-link pf-m-small card-toolbar-include-btn"
            data-card-id="{{ card_id }}"
            aria-label="Include all {{ item_count }} {{ card_label | lower }}">
      Include All {{ item_count }}
    </button>
    <button type="button"
            class="pf-v6-c-button pf-m-link pf-m-small card-toolbar-exclude-btn"
            data-card-id="{{ card_id }}"
            aria-label="Exclude all {{ item_count }} {{ card_label | lower }}">
      Exclude All {{ item_count }}
    </button>
  </div>
</div>
{%- endmacro %}
```

- [ ] **Step 2: Add CSS for the card toolbar**

In `_toolbar.html.j2` (which contains the top toolbar CSS), add styles for the card toolbar:

```css
.card-toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 6px 16px;
  gap: 12px;
  border-bottom: 1px solid var(--pf-t--global--border--color--default);
  background: var(--pf-t--global--background--color--secondary--default);
}
.card-toolbar-left {
  display: flex;
  align-items: center;
  gap: 8px;
  flex: 1;
}
.card-toolbar-right {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}
.card-search-input {
  max-width: 220px;
  font-size: var(--pf-t--global--font--size--sm);
}
.card-search-input:not(:placeholder-shown) {
  border-color: var(--pf-t--global--color--brand--default);
}
.card-toolbar-filter-count {
  font-size: var(--pf-t--global--font--size--xs);
  color: var(--pf-t--global--color--brand--default);
}
.card-toolbar-included-count {
  font-size: var(--pf-t--global--font--size--xs);
  color: var(--pf-t--global--text--color--subtle);
}
.card-toolbar-warning-indicator {
  font-size: var(--pf-t--global--font--size--xs);
  color: var(--pf-t--global--color--status--warning--default);
}
.card-toolbar-include-btn {
  color: var(--pf-t--global--color--brand--default);
}
.card-toolbar-exclude-btn {
  color: var(--pf-t--global--color--status--danger--default);
}
.card-toolbar-include-btn:disabled,
.card-toolbar-exclude-btn:disabled {
  opacity: 0.4;
  cursor: default;
}
```

- [ ] **Step 3: Commit**

```bash
git add src/inspectah/templates/report/_macros.html.j2 src/inspectah/templates/report/_toolbar.html.j2
git commit -m "feat(report): add card_toolbar macro and CSS"
```

---

### Task 3: Insert `card_toolbar()` into all filterable cards

Wire the macro into each template that has filterable content. Each card gets one `card_toolbar()` call placed between its card header and item list.

**Files:**
- Modify: all 8 filterable card templates (see Task 1 file list)

- [ ] **Step 1: Insert toolbar in `_packages.html.j2`**

Inside the packages card (the `<div class="pf-v6-c-card">` that contains the leaf packages table), add after the card header and before the table:

```jinja2
{{ card_toolbar("card-pkg-leaves", leaf_packages_sorted|length, "Packages") }}
```

The `card_id` must match the ID of the item container (the `<tbody>` or wrapper element that holds the package rows).

- [ ] **Step 2: Insert toolbar in `_services.html.j2` (2 cards)**

For enabled/disabled units card:
```jinja2
{{ card_toolbar("card-svc-units", state_changes|length, "Services") }}
```

For drop-in overrides card:
```jinja2
{{ card_toolbar("card-svc-dropins", dropins_count, "Drop-in overrides") }}
```

Where `dropins_count` is the count of drop-in groups (not individual variants).

- [ ] **Step 3: Insert toolbar in `_config.html.j2`**

```jinja2
{{ card_toolbar("card-config-files", config_groups_count, "Config files") }}
```

Where `config_groups_count` counts variant groups for fleet reports, individual items for single-host reports.

- [ ] **Step 4: Insert toolbar in `_network.html.j2` (firewall direct rules only)**

```jinja2
{{ card_toolbar("card-net-direct-rules", snapshot.network.firewall_direct_rules|length, "Firewall rules") }}
```

- [ ] **Step 5: Insert toolbar in `_containers.html.j2` (2 cards)**

For quadlet units:
```jinja2
{{ card_toolbar("card-ctr-quadlets", quadlet_groups_count, "Quadlet units") }}
```

For compose files:
```jinja2
{{ card_toolbar("card-ctr-compose", snapshot.containers.compose_files|length, "Compose files") }}
```

- [ ] **Step 6: Insert toolbar in `_non_rpm.html.j2` (2 cards)**

For compiled binaries:
```jinja2
{{ card_toolbar("card-nonrpm-binaries", binaries|length, "Binaries") }}
```

For system pip packages:
```jinja2
{{ card_toolbar("card-nonrpm-pip", pip_packages|length, "Pip packages") }}
```

- [ ] **Step 7: Insert toolbar in `_kernel_boot.html.j2` (sysctl only)**

```jinja2
{{ card_toolbar("card-kb-sysctl-toolbar", snapshot.kernel_boot.sysctl_overrides|length, "Sysctl overrides") }}
```

- [ ] **Step 8: Insert toolbar in `_users_groups.html.j2` (2 tables)**

For users:
```jinja2
{{ card_toolbar("card-ug-users", users_list|length, "Users") }}
```

For groups:
```jinja2
{{ card_toolbar("card-ug-groups", groups_list|length, "Groups") }}
```

- [ ] **Step 9: Verify toolbars render**

Generate a report and open it. Verify each filterable card shows the toolbar row with search input and bulk buttons. Buttons don't work yet — that's expected.

- [ ] **Step 10: Commit**

```bash
git add src/inspectah/templates/report/
git commit -m "feat(report): insert card_toolbar into all filterable cards"
```

---

### Task 4: Implement `initCardSearch()` and `syncToolbar()` for flat cards

Build the core search and toolbar sync logic. Start with flat (non-grouped, non-variant) cards: sysctl overrides, firewall rules, binaries, pip packages, users, groups.

**Files:**
- Modify: `src/inspectah/templates/report/_js.html.j2`

- [ ] **Step 1: Add `initCardSearch()` function**

Add near the end of the `_js.html.j2` script block, before the closing `</script>`:

```javascript
// --- Card Search & Filter ---
function initCardSearch(cardId) {
  var toolbar = document.querySelector('.card-toolbar[data-card-id="' + cardId + '"]');
  if (!toolbar) return;
  var searchInput = toolbar.querySelector('.card-search-input');
  if (!searchInput) return;

  searchInput.addEventListener('input', function() {
    var query = (this.value || '').trim().toLowerCase();
    if (!query) {
      // Empty query — show all rows
      clearCardFilter(cardId);
      return;
    }
    var container = document.getElementById(cardId);
    if (!container) return;

    var rows = container.querySelectorAll('[data-search-text]');
    var matchCount = 0;
    rows.forEach(function(row) {
      var text = (row.getAttribute('data-search-text') || '').toLowerCase();
      var matches = text.indexOf(query) >= 0;
      row.style.display = matches ? '' : 'none';
      if (matches) matchCount++;
    });

    // Update filter count
    var filterCount = toolbar.querySelector('.card-toolbar-filter-count');
    var total = parseInt(toolbar.getAttribute('data-total-count'), 10);
    if (filterCount) {
      filterCount.textContent = matchCount + ' of ' + total + ' shown';
      filterCount.style.display = '';
    }

    syncToolbar(cardId);
  });

  // Escape clears filter
  searchInput.addEventListener('keydown', function(e) {
    if (e.key === 'Escape') {
      this.value = '';
      clearCardFilter(cardId);
    }
  });
}

function clearCardFilter(cardId) {
  var toolbar = document.querySelector('.card-toolbar[data-card-id="' + cardId + '"]');
  if (!toolbar) return;
  var container = document.getElementById(cardId);
  if (container) {
    container.querySelectorAll('[data-search-text]').forEach(function(row) {
      row.style.display = '';
    });
  }
  var filterCount = toolbar.querySelector('.card-toolbar-filter-count');
  if (filterCount) {
    filterCount.style.display = 'none';
    filterCount.textContent = '';
  }
  syncToolbar(cardId);
}
```

- [ ] **Step 2: Add `syncToolbar()` function**

```javascript
function syncToolbar(cardId) {
  var toolbar = document.querySelector('.card-toolbar[data-card-id="' + cardId + '"]');
  if (!toolbar) return;
  var container = document.getElementById(cardId);
  if (!container) return;
  var searchInput = toolbar.querySelector('.card-search-input');
  var isFiltered = searchInput && searchInput.value.trim() !== '';

  // Count visible items and how many are included
  var visibleRows = container.querySelectorAll('[data-search-text]');
  var visibleCount = 0;
  var includedCount = 0;
  visibleRows.forEach(function(row) {
    if (row.style.display === 'none') return;
    visibleCount++;
    var cb = row.querySelector('.include-toggle');
    if (cb && cb.checked) includedCount++;
  });

  var totalCount = parseInt(toolbar.getAttribute('data-total-count'), 10);

  // Update included count text
  var countEl = toolbar.querySelector('.card-toolbar-included-count');
  if (countEl) {
    if (isFiltered) {
      countEl.textContent = includedCount + ' of ' + visibleCount + ' visible included';
    } else {
      countEl.textContent = includedCount + ' of ' + totalCount + ' included';
    }
  }

  // Update button labels and states
  var includeBtn = toolbar.querySelector('.card-toolbar-include-btn');
  var excludeBtn = toolbar.querySelector('.card-toolbar-exclude-btn');
  var scope = isFiltered ? visibleCount : totalCount;
  var label = isFiltered ? 'Matching' : 'All ' + scope;

  if (includeBtn) {
    includeBtn.textContent = 'Include ' + (isFiltered ? scope + ' ' : '') + label;
    includeBtn.disabled = (includedCount === visibleCount) || visibleCount === 0;
    includeBtn.setAttribute('aria-disabled', includeBtn.disabled ? 'true' : 'false');
    includeBtn.setAttribute('aria-label',
      (includeBtn.disabled ? 'All visible items already included' : 'Include ' + scope + ' items'));
  }
  if (excludeBtn) {
    excludeBtn.textContent = 'Exclude ' + (isFiltered ? scope + ' ' : '') + label;
    excludeBtn.disabled = (includedCount === 0) || visibleCount === 0;
    excludeBtn.setAttribute('aria-disabled', excludeBtn.disabled ? 'true' : 'false');
    excludeBtn.setAttribute('aria-label',
      (excludeBtn.disabled ? 'All visible items already excluded' : 'Exclude ' + scope + ' items'));
  }
}
```

- [ ] **Step 3: Initialize all card toolbars on page load**

Add at the end of the existing DOMContentLoaded or inline init block:

```javascript
// Initialize card search toolbars
document.querySelectorAll('.card-toolbar').forEach(function(toolbar) {
  var cardId = toolbar.getAttribute('data-card-id');
  if (cardId) {
    initCardSearch(cardId);
    syncToolbar(cardId);
  }
});
```

- [ ] **Step 4: Verify flat card search works**

Generate a report with a driftify-standard snapshot. Open via `inspectah refine`. Test:
- Type in the sysctl search box → rows filter
- Clear with Escape → all rows return
- Included count updates correctly
- Button labels update ("Include 3 Matching" when filtered)

- [ ] **Step 5: Commit**

```bash
git add src/inspectah/templates/report/_js.html.j2
git commit -m "feat(report): implement initCardSearch and syncToolbar for flat cards"
```

---

### Task 5: Extend search for repo-grouped cards (packages)

The packages card groups items by repo with collapsible headers. Search needs to filter within groups, update group headers, and hide empty groups.

**Files:**
- Modify: `src/inspectah/templates/report/_js.html.j2`

- [ ] **Step 1: Add repo-group-aware filtering to `initCardSearch()`**

Extend `initCardSearch()` to detect whether the card has `[data-group]` rows. If so, use group-aware filtering instead of flat filtering:

```javascript
// Inside initCardSearch, replace the rows.forEach block with:
var hasGroups = container.querySelector('[data-group]') !== null;
if (hasGroups) {
  filterGroupedCard(container, toolbar, query);
} else {
  filterFlatCard(container, toolbar, query);
}
```

- [ ] **Step 2: Add `filterGroupedCard()` function**

```javascript
function filterGroupedCard(container, toolbar, query) {
  var groups = {};
  // Collect group info
  container.querySelectorAll('[data-group]').forEach(function(row) {
    var groupName = row.getAttribute('data-group');
    if (!groups[groupName]) groups[groupName] = { header: null, items: [], matchCount: 0, totalCount: 0 };
    if (row.classList.contains('repo-group-row')) {
      groups[groupName].header = row;
    } else if (row.hasAttribute('data-search-text')) {
      groups[groupName].items.push(row);
      groups[groupName].totalCount++;
    }
  });

  var totalMatches = 0;
  Object.keys(groups).forEach(function(groupName) {
    var g = groups[groupName];
    var matchCount = 0;
    g.items.forEach(function(row) {
      var text = (row.getAttribute('data-search-text') || '').toLowerCase();
      var matches = text.indexOf(query) >= 0;
      row.style.display = matches ? '' : 'none';
      if (matches) matchCount++;
    });
    g.matchCount = matchCount;
    totalMatches += matchCount;

    // Update group header
    if (g.header) {
      if (matchCount === 0) {
        // Hide normal header, show "no matches" stub
        g.header.style.display = 'none';
        showGroupNoMatchStub(g.header, groupName);
      } else {
        g.header.style.display = '';
        hideGroupNoMatchStub(g.header);
        updateGroupHeaderCount(g.header, matchCount, g.totalCount);
      }
    }
    // Hide variant children row if it exists
    var childrenRow = g.header ? g.header.nextElementSibling : null;
    if (childrenRow && childrenRow.classList.contains('fleet-variant-children') && matchCount === 0) {
      childrenRow.style.display = 'none';
    }
  });

  // Update filter count
  var filterCount = toolbar.querySelector('.card-toolbar-filter-count');
  var total = parseInt(toolbar.getAttribute('data-total-count'), 10);
  if (filterCount) {
    filterCount.textContent = totalMatches + ' of ' + total + ' shown';
    filterCount.style.display = '';
  }
}
```

- [ ] **Step 3: Add group header helper functions**

```javascript
function showGroupNoMatchStub(header, groupName) {
  var stubId = 'no-match-stub-' + groupName.replace(/[^a-zA-Z0-9]/g, '-');
  if (document.getElementById(stubId)) return; // already shown
  var stub = document.createElement('tr');
  stub.id = stubId;
  stub.className = 'group-no-match-stub';
  stub.innerHTML = '<td colspan="99" style="font-style:italic;color:var(--pf-t--global--text--color--subtle);padding:6px 16px;">' +
    groupName + ' \u2014 no matches</td>';
  header.parentNode.insertBefore(stub, header.nextSibling);
}

function hideGroupNoMatchStub(header) {
  var next = header.nextElementSibling;
  if (next && next.classList.contains('group-no-match-stub')) {
    next.remove();
  }
}

function updateGroupHeaderCount(header, matchCount, totalCount) {
  var label = header.querySelector('.repo-group-label, .table-group-header');
  if (!label) return;
  // Store original text on first call
  if (!label.hasAttribute('data-original-text')) {
    label.setAttribute('data-original-text', label.textContent);
  }
  if (matchCount < totalCount) {
    label.textContent = label.getAttribute('data-original-text') + ' (' + matchCount + ' of ' + totalCount + ' matching)';
  } else {
    label.textContent = label.getAttribute('data-original-text');
  }
}
```

- [ ] **Step 4: Update `clearCardFilter()` to clean up group state**

```javascript
// Add to clearCardFilter() after restoring row visibility:
// Clean up group stubs and header counts
if (container) {
  container.querySelectorAll('.group-no-match-stub').forEach(function(stub) { stub.remove(); });
  container.querySelectorAll('[data-original-text]').forEach(function(el) {
    el.textContent = el.getAttribute('data-original-text');
  });
  container.querySelectorAll('.repo-group-row').forEach(function(row) { row.style.display = ''; });
}
```

- [ ] **Step 5: Verify grouped search works**

Generate a report, open via `inspectah refine`. In the packages section:
- Search "http" → only matching packages visible, group headers update to "N of M matching"
- Groups with no matches show "RepoName — no matches" italic stub
- Clear with Escape → everything restored, group headers show original text

- [ ] **Step 6: Commit**

```bash
git add src/inspectah/templates/report/_js.html.j2
git commit -m "feat(report): extend search to handle repo-grouped package cards"
```

---

### Task 6: Implement `batchToggleItems()` — batched mutation helper

This is the prerequisite refactoring for bulk operations. Extract a batched mutation path that sets multiple checkboxes without per-item side effects, runs one cascade pass, and shows one notification.

**Files:**
- Modify: `src/inspectah/templates/report/_js.html.j2`

- [ ] **Step 1: Add `batchToggleItems()` function**

```javascript
// --- Batched Mutation Helper ---
function batchToggleItems(cardId, items, include) {
  if (items.length === 0) return;
  var directCount = 0;

  // Phase 1: Set checkboxes and snapshot state without per-item side effects
  items.forEach(function(row) {
    var section = row.getAttribute('data-snap-section');
    var list = row.getAttribute('data-snap-list');
    var idx = parseInt(row.getAttribute('data-snap-index'), 10);
    if (!section || !list || isNaN(idx)) return;

    var arr = resolveSnapshotRef(section, list);
    if (arr && arr[idx] !== undefined) {
      if (arr[idx].include !== include) {
        arr[idx].include = include;
        directCount++;
      }
    }
    var cb = row.querySelector('.include-toggle');
    if (cb) cb.checked = include;
    row.classList.toggle('excluded', !include);
  });

  if (directCount === 0) return;

  // Phase 2: One cascade recompute (packages only)
  var cascadeCount = 0;
  if (typeof recomputeAutoDeps === 'function') {
    var beforeCount = countExcludedPackages();
    recomputeAutoDeps();
    var afterCount = countExcludedPackages();
    cascadeCount = Math.abs(afterCount - beforeCount) - directCount;
    if (cascadeCount < 0) cascadeCount = 0;
    // Sync checkboxes for any cascade-affected rows
    syncAllPackageCheckboxes();
  }

  // Phase 3: One UI refresh
  recalcTriageCounts();
  updatePkgBanner();
  setDirty(true);
  updateToolbar();
  syncToolbar(cardId);

  // Phase 4: One summary notification
  var verb = include ? 'Included' : 'Excluded';
  var msg = verb + ' ' + directCount + ' item' + (directCount !== 1 ? 's' : '');
  if (cascadeCount > 0) {
    msg += ' (+' + cascadeCount + ' dependencies)';
  }
  showToast(msg);
}
```

- [ ] **Step 2: Add helper functions for cascade counting**

```javascript
function countExcludedPackages() {
  var count = 0;
  if (snapshot.rpm && snapshot.rpm.packages_added) {
    snapshot.rpm.packages_added.forEach(function(pkg) {
      if (!pkg.include) count++;
    });
  }
  return count;
}

function syncAllPackageCheckboxes() {
  // Sync DOM checkboxes to match snapshot state after cascade
  document.querySelectorAll('[data-snap-section="rpm"] .include-toggle, [data-snap-section="packages"] .include-toggle').forEach(function(cb) {
    var row = cb.closest('[data-snap-section]');
    if (!row) return;
    var section = row.getAttribute('data-snap-section');
    var list = row.getAttribute('data-snap-list');
    var idx = parseInt(row.getAttribute('data-snap-index'), 10);
    if (!section || !list || isNaN(idx)) return;
    var arr = resolveSnapshotRef(section, list);
    if (arr && arr[idx] !== undefined) {
      cb.checked = arr[idx].include;
      row.classList.toggle('excluded', !arr[idx].include);
    }
  });
}
```

- [ ] **Step 3: Commit**

```bash
git add src/inspectah/templates/report/_js.html.j2
git commit -m "refactor(report): extract batchToggleItems for bulk operations"
```

---

### Task 7: Wire bulk controls — all card types

Connect the Include All / Exclude All buttons to `batchToggleItems()`. Handle three card types: flat cards, packages (with cascade), and fleet variant cards.

**Files:**
- Modify: `src/inspectah/templates/report/_js.html.j2`

- [ ] **Step 1: Add bulk button click handlers**

Add to the card toolbar initialization block:

```javascript
// Wire bulk buttons for each card toolbar
document.querySelectorAll('.card-toolbar').forEach(function(toolbar) {
  var cardId = toolbar.getAttribute('data-card-id');

  var includeBtn = toolbar.querySelector('.card-toolbar-include-btn');
  var excludeBtn = toolbar.querySelector('.card-toolbar-exclude-btn');

  if (includeBtn) {
    includeBtn.addEventListener('click', function() {
      if (this.disabled) return;
      bulkToggle(cardId, true);
    });
  }
  if (excludeBtn) {
    excludeBtn.addEventListener('click', function() {
      if (this.disabled) return;
      bulkToggle(cardId, false);
    });
  }
});
```

- [ ] **Step 2: Add `bulkToggle()` routing function**

```javascript
function bulkToggle(cardId, include) {
  var container = document.getElementById(cardId);
  if (!container) return;

  // Determine card type
  var hasVariantGroups = container.querySelector('[data-variant-group]') !== null;
  var isFiltered = isCardFiltered(cardId);

  if (hasVariantGroups) {
    bulkToggleVariantCard(cardId, container, include, isFiltered);
  } else {
    bulkToggleFlatCard(cardId, container, include, isFiltered);
  }
}

function isCardFiltered(cardId) {
  var toolbar = document.querySelector('.card-toolbar[data-card-id="' + cardId + '"]');
  if (!toolbar) return false;
  var input = toolbar.querySelector('.card-search-input');
  return input && input.value.trim() !== '';
}
```

- [ ] **Step 3: Add `bulkToggleFlatCard()`**

This handles flat cards and packages (packages get cascade via `batchToggleItems`):

```javascript
function bulkToggleFlatCard(cardId, container, include, isFiltered) {
  var rows = [];
  container.querySelectorAll('[data-search-text]').forEach(function(row) {
    if (isFiltered && row.style.display === 'none') return; // skip hidden rows
    var cb = row.querySelector('.include-toggle');
    if (cb && cb.checked !== include) {
      rows.push(row);
    }
  });
  batchToggleItems(cardId, rows, include);
}
```

- [ ] **Step 4: Add `bulkToggleVariantCard()`**

Fleet variant cards: bulk operates at the group level. Exclude unchecks all children. Include checks child[0] of fully-excluded groups only.

```javascript
function bulkToggleVariantCard(cardId, container, include, isFiltered) {
  // Collect variant groups
  var groups = {};
  container.querySelectorAll('[data-variant-group]').forEach(function(row) {
    var groupKey = row.getAttribute('data-variant-group');
    if (!groups[groupKey]) groups[groupKey] = [];
    groups[groupKey].push(row);
  });

  // Also collect non-variant rows (single-item, no variant group)
  var singleRows = [];
  container.querySelectorAll('[data-search-text]:not([data-variant-group])').forEach(function(row) {
    if (!row.classList.contains('fleet-variant-group') &&
        !row.classList.contains('fleet-variant-children')) {
      singleRows.push(row);
    }
  });

  var itemsToToggle = [];

  if (include) {
    // Include: for each fully-excluded group, check child[0] (primary variant)
    Object.keys(groups).forEach(function(key) {
      var children = groups[key];
      // Check if group is visible (filter)
      var parentRow = findVariantParentRow(children[0]);
      if (isFiltered && parentRow && parentRow.style.display === 'none') return;

      var anyChecked = children.some(function(r) {
        var cb = r.querySelector('.include-toggle');
        return cb && cb.checked;
      });
      if (!anyChecked && children.length > 0) {
        itemsToToggle.push(children[0]); // primary variant
      }
    });
    // Single rows: include if excluded
    singleRows.forEach(function(row) {
      if (isFiltered && row.style.display === 'none') return;
      var cb = row.querySelector('.include-toggle');
      if (cb && !cb.checked) itemsToToggle.push(row);
    });
  } else {
    // Exclude: uncheck all children in all groups
    Object.keys(groups).forEach(function(key) {
      var children = groups[key];
      var parentRow = findVariantParentRow(children[0]);
      if (isFiltered && parentRow && parentRow.style.display === 'none') return;

      children.forEach(function(row) {
        var cb = row.querySelector('.include-toggle');
        if (cb && cb.checked) itemsToToggle.push(row);
      });
    });
    // Single rows: exclude if included
    singleRows.forEach(function(row) {
      if (isFiltered && row.style.display === 'none') return;
      var cb = row.querySelector('.include-toggle');
      if (cb && cb.checked) itemsToToggle.push(row);
    });
  }

  batchToggleItems(cardId, itemsToToggle, include);

  // Sync parent rows after variant mutations
  syncVariantParentRows(container);
}

function findVariantParentRow(childRow) {
  var childrenWrapper = childRow.closest('.fleet-variant-children');
  if (childrenWrapper) return childrenWrapper.previousElementSibling;
  return null;
}

function syncVariantParentRows(container) {
  container.querySelectorAll('.fleet-variant-group').forEach(function(parentRow) {
    var childrenWrapper = parentRow.nextElementSibling;
    if (!childrenWrapper || !childrenWrapper.classList.contains('fleet-variant-children')) return;
    var firstChild = childrenWrapper.querySelector('[data-variant-group]');
    if (!firstChild) return;
    var firstCb = firstChild.querySelector('.include-toggle');
    var parentCb = parentRow.querySelector('.include-toggle');
    if (firstCb && parentCb) {
      parentCb.checked = firstCb.checked;
      parentRow.classList.toggle('excluded', !firstCb.checked);
    }
  });
}
```

- [ ] **Step 5: Verify bulk operations**

Test in the refine UI:
- Flat card: click "Exclude All" on sysctl overrides → all rows excluded, button disables
- Click "Include All" → all rows included
- Filter sysctl, then "Exclude 2 Matching" → only visible rows excluded
- Packages: "Exclude All" → all packages excluded with cascade notification
- Variant card (config files in fleet report): "Exclude All" → all groups excluded

- [ ] **Step 6: Commit**

```bash
git add src/inspectah/templates/report/_js.html.j2
git commit -m "feat(report): wire bulk Include/Exclude controls for all card types"
```

---

### Task 8: Implement `updateGroupState()` — repo group state machine

The packages card needs the state machine from the spec: search overrides manual collapse, excluded groups show stubs, stubs are expandable.

**Files:**
- Modify: `src/inspectah/templates/report/_js.html.j2`

- [ ] **Step 1: Add group state tracking**

```javascript
// Track per-group state for the excluded-stub feature
var groupStubState = {}; // { groupName: { userExpandedStub: false } }
```

- [ ] **Step 2: Add `updateGroupState()` function**

```javascript
function updateGroupState(cardId) {
  var container = document.getElementById(cardId);
  if (!container) return;
  // Only applies to cards with data-group (packages)
  if (!container.querySelector('[data-group]')) return;

  var searchInput = document.querySelector('.card-toolbar[data-card-id="' + cardId + '"] .card-search-input');
  var searchActive = searchInput && searchInput.value.trim() !== '';

  var groups = {};
  container.querySelectorAll('[data-group]').forEach(function(row) {
    var groupName = row.getAttribute('data-group');
    if (!groups[groupName]) groups[groupName] = { header: null, items: [] };
    if (row.classList.contains('repo-group-row')) {
      groups[groupName].header = row;
    } else if (row.hasAttribute('data-snap-index')) {
      groups[groupName].items.push(row);
    }
  });

  Object.keys(groups).forEach(function(groupName) {
    var g = groups[groupName];
    if (!g.header) return;
    if (!groupStubState[groupName]) groupStubState[groupName] = { userExpandedStub: false };
    var state = groupStubState[groupName];

    var allExcluded = g.items.length > 0 && g.items.every(function(row) {
      var cb = row.querySelector('.include-toggle');
      return cb && !cb.checked;
    });
    var hasVisibleMatches = searchActive && g.items.some(function(row) {
      return row.style.display !== 'none';
    });
    var manuallyCollapsed = g.header.classList.contains('repo-collapsed');

    // Apply precedence table from spec
    if (searchActive && hasVisibleMatches) {
      // Row 1: Expanded, showing matches only (search already handles visibility)
      showGroupNormal(g.header);
      removeExcludedStub(groupName);
    } else if (searchActive && !hasVisibleMatches) {
      // Row 2: "No matches" stub (search already shows this)
      // No additional action needed — filterGroupedCard handles this
    } else if (!searchActive && allExcluded && !state.userExpandedStub) {
      // Row 3: Excluded stub
      showExcludedStub(g, groupName);
    } else if (!searchActive && allExcluded && state.userExpandedStub) {
      // Row 4: Expanded, all dimmed
      showGroupNormal(g.header);
      removeExcludedStub(groupName);
      g.items.forEach(function(row) { row.style.display = ''; });
    } else if (!searchActive && !allExcluded && manuallyCollapsed) {
      // Row 5: Manually collapsed (existing behavior)
      removeExcludedStub(groupName);
    } else {
      // Row 6: Normal
      showGroupNormal(g.header);
      removeExcludedStub(groupName);
      state.userExpandedStub = false;
    }
  });
}
```

- [ ] **Step 3: Add excluded stub rendering functions**

```javascript
function showExcludedStub(group, groupName) {
  group.header.style.display = 'none';
  group.items.forEach(function(row) { row.style.display = 'none'; });

  var stubId = 'excluded-stub-' + groupName.replace(/[^a-zA-Z0-9]/g, '-');
  if (document.getElementById(stubId)) return;

  // Count warnings/redactions in excluded items
  var warningCount = 0;
  group.items.forEach(function(row) {
    if (row.querySelector('.warning-indicator, .redaction-indicator, [data-has-warning]')) {
      warningCount++;
    }
  });

  var stub = document.createElement('tr');
  stub.id = stubId;
  stub.className = 'excluded-group-stub';
  stub.setAttribute('data-stub-group', groupName);
  var warningBadge = warningCount > 0
    ? ' <span class="stub-warning-badge">\u26A0 ' + warningCount + ' warning' + (warningCount !== 1 ? 's' : '') + '</span>'
    : '';
  stub.innerHTML = '<td colspan="99" style="padding:8px 16px;cursor:pointer;">' +
    '<span class="stub-chevron">\u25B8</span> ' + groupName +
    ' <span style="color:var(--pf-t--global--text--color--subtle);">(' + group.items.length + ' packages \u2014 all excluded)</span>' +
    warningBadge +
    ' <button class="pf-v6-c-button pf-m-link pf-m-small">Expand</button></td>';

  stub.addEventListener('click', function() {
    groupStubState[groupName].userExpandedStub = true;
    var cardId = group.header.closest('[id]').id;
    updateGroupState(cardId);
  });

  group.header.parentNode.insertBefore(stub, group.header);
}

function removeExcludedStub(groupName) {
  var stubId = 'excluded-stub-' + groupName.replace(/[^a-zA-Z0-9]/g, '-');
  var stub = document.getElementById(stubId);
  if (stub) stub.remove();
}

function showGroupNormal(header) {
  header.style.display = '';
}
```

- [ ] **Step 4: Wire `updateGroupState` into the `.include-toggle` handler**

Find the existing `.include-toggle` change handler. At the end, after existing processing, add:

```javascript
// Sync card toolbar and group state
var cardToolbar = tr.closest('[id]');
if (cardToolbar) {
  syncToolbar(cardToolbar.id);
  updateGroupState(cardToolbar.id);
}
```

- [ ] **Step 5: Reset group state on search clear and discard**

In `clearCardFilter()`, add:
```javascript
// Reset group stub state
Object.keys(groupStubState).forEach(function(key) {
  groupStubState[key].userExpandedStub = false;
});
updateGroupState(cardId);
```

In the existing `#btn-reset` handler, add:
```javascript
// Clear search and reset group state for all cards
document.querySelectorAll('.card-search-input').forEach(function(input) {
  input.value = '';
});
document.querySelectorAll('.card-toolbar').forEach(function(toolbar) {
  var cardId = toolbar.getAttribute('data-card-id');
  clearCardFilter(cardId);
  updateGroupState(cardId);
});
```

- [ ] **Step 6: Wire into re-render response**

In the existing re-render success handler (after `buildBaseline()` and `setDirty(false)`), add:

```javascript
// Sync card toolbars after re-render (search state persists, counts recalculate)
document.querySelectorAll('.card-toolbar').forEach(function(toolbar) {
  var cardId = toolbar.getAttribute('data-card-id');
  syncToolbar(cardId);
  updateGroupState(cardId);
});
```

- [ ] **Step 7: Verify state machine**

Test in the refine UI:
- Exclude all packages in a repo → stub appears with item count
- Click stub expand → all items visible, dimmed
- Include one item → transitions to normal, stub gone
- Search while in stub state → matches override stub
- Clear search → stub returns if still all excluded

- [ ] **Step 8: Commit**

```bash
git add src/inspectah/templates/report/_js.html.j2
git commit -m "feat(report): implement repo group state machine with excluded stubs"
```

---

### Task 9: Warning/redaction indicator

When filtered results hide rows with warnings or redaction indicators, the toolbar shows a persistent badge.

**Files:**
- Modify: `src/inspectah/templates/report/_js.html.j2`

- [ ] **Step 1: Add warning tracking to `syncToolbar()`**

At the end of `syncToolbar()`, add:

```javascript
// Update warning indicator
var warningEl = toolbar.querySelector('.card-toolbar-warning-indicator');
if (warningEl) {
  var hiddenWarnings = 0;
  container.querySelectorAll('[data-search-text]').forEach(function(row) {
    if (row.style.display !== 'none') return; // visible — no concern
    if (row.querySelector('.warning-indicator, .redaction-indicator, [data-has-warning], [data-has-redaction]')) {
      hiddenWarnings++;
    }
  });
  if (hiddenWarnings > 0 && isFiltered) {
    warningEl.textContent = hiddenWarnings + ' hidden item' + (hiddenWarnings !== 1 ? 's' : '') + ' have warnings or redactions';
    warningEl.style.display = '';
  } else {
    warningEl.style.display = 'none';
    warningEl.textContent = '';
  }
}
```

- [ ] **Step 2: Verify warning indicator**

Generate a report from a driftify snapshot that has secrets/warnings. Filter to hide some warning-bearing rows. Verify the toolbar shows the warning badge. Clear the filter — badge disappears.

- [ ] **Step 3: Commit**

```bash
git add src/inspectah/templates/report/_js.html.j2
git commit -m "feat(report): add hidden warning/redaction indicator to card toolbar"
```

---

### Task 10: Fleet variant search support

Ensure search on fleet-variant cards (config, services drop-ins, quadlets) matches against parent rows and shows/hides variant children accordingly.

**Files:**
- Modify: `src/inspectah/templates/report/_js.html.j2`

- [ ] **Step 1: Extend `filterFlatCard()` to handle variant parent/child rows**

```javascript
// In filterFlatCard, after hiding non-matching rows:
// Handle fleet variant parent/child visibility
container.querySelectorAll('.fleet-variant-group').forEach(function(parentRow) {
  if (!parentRow.hasAttribute('data-search-text')) return;
  var childrenRow = parentRow.nextElementSibling;
  if (childrenRow && childrenRow.classList.contains('fleet-variant-children')) {
    childrenRow.style.display = parentRow.style.display; // children follow parent
  }
});
```

- [ ] **Step 2: Update `syncToolbar()` count logic for variant cards**

Variant cards should count groups, not individual variants:

```javascript
// In syncToolbar, when counting visible items:
var hasVariantGroups = container.querySelector('[data-variant-group]') !== null;
if (hasVariantGroups) {
  // Count groups, not individual variants
  var groupsSeen = {};
  container.querySelectorAll('.fleet-variant-group, [data-search-text]:not(.fleet-variant-children [data-search-text])').forEach(function(row) {
    if (row.style.display === 'none') return;
    // ... count based on parent group inclusion
  });
}
```

Note: The exact counting logic will need to be tuned during implementation based on the actual DOM structure. The principle is: one group = one item in the count.

- [ ] **Step 3: Verify variant search**

In a fleet report, search in the config files card → parent rows filter, children follow. Counts reflect groups, not variants.

- [ ] **Step 4: Commit**

```bash
git add src/inspectah/templates/report/_js.html.j2
git commit -m "feat(report): support fleet variant parent/child search filtering"
```

---

## Verification

After all tasks are complete, run through the verification matrix from the spec (`docs/specs/proposed/2026-04-07-report-search-bulk-controls-design.md`, "Verification Matrix" section). Each scenario (S1-S7, B1-B10, G1-G9, W1-W3, R1-R3, A1-A5) should pass via manual testing in the refine UI.

**Test environments:**
1. Single-host report: `inspectah inspect --from-snapshot` with a driftify-standard snapshot
2. Fleet report: `inspectah fleet` output with 2-3 driftify profiles
3. Standalone mode: open the HTML directly without `inspectah refine`
4. Refine mode: serve via `inspectah refine` and test interactive operations

---

## Notes for Implementer

- **Template variable names will vary.** The exact Jinja2 loop variable names (`pkg`, `s`, `d`, `item`, etc.) differ per template. Read the template before adding `data-search-text` — use the variable that's in scope for the row.
- **Card IDs must be unique.** When inserting `card_toolbar()`, the `card_id` must match a real DOM element that wraps the filterable rows. If no wrapper exists, add one (a `<div id="card-xxx">` around the `<table>`).
- **`resolveSnapshotRef()`** is an existing JS function that navigates the snapshot object to find the right array. Use it for snapshot state access.
- **`recomputeAutoDeps()`**, `updatePkgBanner()`, `recalcTriageCounts()`, `setDirty()`, `updateToolbar()`, `showToast()` are all existing functions. Do not reimplement them.
- **CSS token names** use PatternFly 6 conventions (`--pf-t--global--*`). Check `_toolbar.html.j2` for existing patterns.
- **The `item_count` parameter** in `card_toolbar()` needs to reflect the correct count for each card. For fleet-variant cards, this is the group count, not the total variant count. Compute this in the template if the template variable doesn't already provide it.
