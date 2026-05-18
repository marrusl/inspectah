{% raw %}
# Report Search & Bulk Controls Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [[...]

**Goal:** Add per-card search/filter and bulk Include All / Exclude All controls to the inspectah interactive HTML report.

**Architecture:** Pure client-side. A new Jinja2 macro renders a toolbar (search + bulk buttons) inside each filterable card. JavaScript handles filtering via `data-search-text` attributes, batched[...]

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

---

### Task 1: Add `data-search-text` attributes to all filterable card templates

This is the foundation. Every filterable row/div gets a `data-search-text` attribute set by the Jinja2 renderer to the item's primary identifier. No JavaScript yet — just wiring up the search ta[...]

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

Find every `<tr` that has `data-snap-section` and `data-snap-list` for packages. Add `data-search-text="{{ pkg.name }}"` (where `pkg` is the loop variable for the package item). This includes both[...]

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

Build the toolbar component that will be inserted into each filterable card. This task covers the HTML macro and the CSS styling. No JavaScript wiring yet — the buttons and search input render [...]

**Files:**
- Modify: `src/inspectah/templates/report/_macros.html.j2`
- Modify: `src/inspectah/templates/report/_toolbar.html.j2` (CSS)

- [ ] **Step 1: Add `card_toolbar` macro to `_macros.html.j2`**

```jinja2
{# ── Filterable card toolbar (search + bulk controls) ──────────────────── #}
{% raw %}
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
{% endraw %}
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
  color: var(--pf-t--global--color--status--warning--Default);
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

[...]

{% endraw %}
