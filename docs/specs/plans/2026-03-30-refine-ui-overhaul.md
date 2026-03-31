# Refine UI Overhaul Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring the refine report UI to architect's quality level — cohesive PF6 design tokens, redesigned summary tab with live-updating prevalence control, upgraded components, and visual polish.

**Architecture:** Template-only changes across 16+ Jinja2 files in `src/yoinkc/templates/report/`. No backend changes. CSS token migration is the foundation; summary tab redesign is the centerpiece; component swaps and polish follow. All prevalence live-update logic uses the existing `snapshot` object and `countAtThreshold()` pattern.

**Tech Stack:** Jinja2 templates, PatternFly 6 CSS/components, vanilla JS, CSS custom properties

**Spec:** `docs/specs/proposed/2026-03-30-refine-ui-overhaul-design.md`

---

### Task 1: CSS Token Sweep + Hex Audit

**Files:**
- Modify: `src/yoinkc/templates/report/_css.html.j2` (all 472 lines)
- Modify: Any template file with inline `<style>` blocks

This is the foundation — every subsequent task builds on clean, theme-aware tokens.

- [ ] **Step 1: Audit current token usage**

Run from the repo root:

```bash
cd ~/Work/bootc-migration/yoinkc
grep -c 'pf-v6-global' src/yoinkc/templates/report/_css.html.j2
grep -c 'pf-t--global' src/yoinkc/templates/report/_css.html.j2
grep -oE '#[0-9a-fA-F]{3,6}' src/yoinkc/templates/report/_css.html.j2 | sort | uniq -c | sort -rn
```

Record the counts as a baseline.

- [ ] **Step 2: Replace semantic `--pf-v6-global--` tokens with `--pf-t--global--` equivalents**

In `_css.html.j2`, find-and-replace these semantic token prefixes. **Do NOT replace `--pf-v6-global--palette--*`** — those need case-by-case handling.

Key mappings:
```
--pf-v6-global--spacer--sm    → --pf-t--global--spacer--sm
--pf-v6-global--spacer--md    → --pf-t--global--spacer--md
--pf-v6-global--spacer--lg    → --pf-t--global--spacer--lg
--pf-v6-global--FontSize--sm  → --pf-t--global--font--size--sm
--pf-v6-global--FontSize--md  → --pf-t--global--font--size--md
```

Verify each replacement compiles (the fallback value after the comma stays unchanged).

- [ ] **Step 3: Replace hardcoded hex values with PF6 tokens**

Work through each unique hex value in `_css.html.j2`. For each occurrence, determine context (text vs background vs border) and apply the correct token:

| Hex | Text context | Background context | Border context |
|-----|-------------|-------------------|----------------|
| `#fff` | `--pf-t--global--text--color--on-brand--default` | `--pf-t--global--background--color--primary--default` | — |
| `#151515` | `--pf-t--global--text--color--regular` | validate per-occurrence | — |
| `#f0f0f0` | — | `--pf-t--global--background--color--secondary--default` | — |
| `#e0e0e0` | — | `--pf-t--global--background--color--secondary--default` | `--pf-t--global--border--color--default` |
| `#d2d2d2` | — | — | `--pf-t--global--border--color--default` |
| `#8a8d90` | `--pf-t--global--text--color--subtle` | — | — |

- [ ] **Step 4: Create theme-aware custom properties for diff colors**

Add to the top of `_css.html.j2`:

```css
:root {
  --yoinkc-diff-add-bg: rgba(77, 171, 247, 0.12);
  --yoinkc-diff-remove-bg: rgba(201, 25, 11, 0.12);
  --yoinkc-diff-add-border: #4dabf7;
  --yoinkc-diff-remove-border: #c9190b;
  --yoinkc-content-bottom-padding: 5rem;
  --yoinkc-content-max-height: calc(100vh - 200px);
}

html:not(.pf-v6-theme-dark) {
  --yoinkc-diff-add-bg: rgba(43, 154, 243, 0.08);
  --yoinkc-diff-remove-bg: rgba(201, 25, 11, 0.08);
}
```

Replace hardcoded `rgba()` values in `.diff-line-add`, `.diff-line-remove`, `.diff-legend-selected`, `.diff-legend-comparison` with the new custom properties.

Replace `padding-bottom: 5rem` with `var(--yoinkc-content-bottom-padding)`.
Replace `calc(100vh - 200px)` with `var(--yoinkc-content-max-height)`.

- [ ] **Step 5: Fix fleet popover and fleet bar hardcoded colors**

In `_css.html.j2`, find the fleet popover styles (`.fleet-popover`, `.fleet-popover-*`) and fleet bar track (`.fleet-bar-track`). Replace all hardcoded hex with PF6 tokens:

```css
/* Before */
.fleet-bar-track { background: #e0e0e0; }

/* After */
.fleet-bar-track { background: var(--pf-t--global--background--color--secondary--default, #e0e0e0); }
```

Apply same pattern to all fleet popover colors (`#fff`, `#d2d2d2`, `#8a8d90`, `#151515`, `#f0f0f0`).

- [ ] **Step 6: Verify and commit**

```bash
uv run --extra dev pytest -q
```

Expected: 1028 passed. Visually verify: open a refine report, toggle dark/light theme, check fleet popover and diff colors in both modes.

```bash
git add src/yoinkc/templates/report/_css.html.j2
git commit -m "refactor(refine): Migrate CSS to PF6 design tokens and theme-aware properties

Replace --pf-v6-global-- with --pf-t--global-- semantic tokens.
Replace hardcoded hex values with PF6 token equivalents.
Add theme-aware custom properties for diff colors and magic numbers.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 2: Inline Style Extraction

**Files:**
- Modify: `src/yoinkc/templates/report/_css.html.j2` (add new classes)
- Modify: `src/yoinkc/templates/report/_sidebar.html.j2`
- Modify: `src/yoinkc/templates/report/_summary.html.j2`
- Modify: `src/yoinkc/templates/report/_banner.html.j2`
- Modify: Other templates as discovered

**Exclusion:** Do NOT extract computed/dynamic inline styles (e.g., `style="width: X%"` for fleet-bar widths in `_macros.html.j2`, or JS-set progress bar widths).

- [ ] **Step 1: Audit inline styles in priority templates**

```bash
grep -n 'style="' src/yoinkc/templates/report/_sidebar.html.j2
grep -n 'style="' src/yoinkc/templates/report/_summary.html.j2
grep -n 'style="' src/yoinkc/templates/report/_banner.html.j2
```

Record each inline style, its purpose, and proposed class name.

- [ ] **Step 2: Create named CSS classes in `_css.html.j2`**

For each inline style found, add a semantic CSS class. Examples:

```css
/* Sidebar layout */
.sidebar-body-layout {
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.sidebar-nav-container {
  flex: 1;
  overflow-y: auto;
}

/* Summary grid */
.summary-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: var(--pf-t--global--spacer--md, 1rem);
}

/* Banner stats */
.banner-stat-row {
  display: flex;
  align-items: center;
  gap: var(--pf-t--global--spacer--sm, 0.5rem);
}
```

Name classes semantically based on purpose, not visual appearance.

- [ ] **Step 3: Replace inline styles with class references in templates**

In each template, replace `style="..."` with the corresponding `class="..."`.

```html
<!-- Before -->
<div style="display:flex; flex-direction:column; overflow:hidden;">

<!-- After -->
<div class="sidebar-body-layout">
```

- [ ] **Step 4: Verify and commit**

```bash
uv run --extra dev pytest -q
```

Visually verify sidebar, summary, and banner render identically.

```bash
git add src/yoinkc/templates/report/_css.html.j2 src/yoinkc/templates/report/_sidebar.html.j2 src/yoinkc/templates/report/_summary.html.j2 src/yoinkc/templates/report/_banner.html.j2
git commit -m "refactor(refine): Extract inline styles to named CSS classes

Move inline style attributes from sidebar, summary, and banner templates
to semantic CSS classes. Dynamic/computed inline styles excluded.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 3: Summary Tab Redesign

**Files:**
- Modify: `src/yoinkc/templates/report/_summary.html.j2` (complete rewrite)
- Modify: `src/yoinkc/templates/report/_css.html.j2` (new summary card styles)
- Modify: `src/yoinkc/templates/report/_js.html.j2` (live prevalence update logic)

This is the largest task — the centerpiece of the overhaul.

- [ ] **Step 1: Add summary card CSS classes to `_css.html.j2`**

```css
/* Summary dashboard grid */
.summary-dashboard {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: var(--pf-t--global--spacer--md, 1rem);
  margin-bottom: var(--pf-t--global--spacer--md, 1rem);
}

.summary-card {
  padding: var(--pf-t--global--spacer--md, 1rem);
  border: 1px solid var(--pf-t--global--border--color--default, #444);
  border-radius: var(--pf-t--global--border--radius--small, 3px);
  background: var(--pf-t--global--background--color--primary--default, #1a1a1a);
}

.summary-card-label {
  font-size: 0.625rem;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  opacity: 0.5;
  margin-bottom: var(--pf-t--global--spacer--xs, 0.25rem);
}

.summary-card-value {
  font-size: 1.25rem;
  font-weight: 700;
}

.summary-card-detail {
  font-size: 0.6875rem;
  opacity: 0.6;
  margin-top: 0.125rem;
}

/* Prevalence control card — interactive affordance */
.summary-card-prevalence {
  border-color: rgba(43, 154, 243, 0.2);
  background: rgba(43, 154, 243, 0.04);
  box-shadow: 0 0 0 1px rgba(43, 154, 243, 0.1);
}

.summary-card-prevalence input[type="range"] {
  width: 100%;
  accent-color: var(--pf-t--global--color--brand--default, #0066cc);
  margin-top: var(--pf-t--global--spacer--xs, 0.25rem);
}

/* Left-border accent colors for summary cards */
.summary-card-system { border-left: 3px solid var(--pf-t--global--color--status--info--default, #2b9af3); }
.summary-card-scope { border-left: 3px solid var(--pf-t--global--color--status--success--default, #3e8635); }
.summary-card-attention { border-left: 3px solid var(--pf-t--global--color--status--warning--default, #f0ab00); }

/* Section priority list */
.summary-priority-list {
  margin-top: var(--pf-t--global--spacer--md, 1rem);
}

.summary-priority-heading {
  font-size: 0.625rem;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  opacity: 0.5;
  margin-bottom: var(--pf-t--global--spacer--sm, 0.5rem);
}

.summary-priority-row {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: var(--pf-t--global--spacer--xs, 0.25rem) var(--pf-t--global--spacer--sm, 0.5rem);
  border-radius: var(--pf-t--global--border--radius--small, 3px);
  cursor: pointer;
  transition: background 150ms ease;
}

.summary-priority-row:hover {
  background: var(--pf-t--global--background--color--secondary--hover, rgba(255,255,255,0.06));
}

.summary-priority-status-manual { color: var(--pf-t--global--color--status--danger--default, #c9190b); }
.summary-priority-status-review { color: var(--pf-t--global--color--status--warning--default, #f0ab00); }
.summary-priority-status-auto { color: var(--pf-t--global--color--status--success--default, #3e8635); }

/* Single-host: needs-attention spans full width */
.summary-dashboard-single {
  grid-template-columns: 1fr 1fr;
}
.summary-card-attention-full {
  grid-column: 1 / -1;
}

/* Light theme overrides */
html:not(.pf-v6-theme-dark) .summary-card {
  background: #fff;
  border-top-color: #d2d2d2;
  border-right-color: #d2d2d2;
  border-bottom-color: #d2d2d2;
}

html:not(.pf-v6-theme-dark) .summary-card-prevalence {
  background: rgba(43, 154, 243, 0.03);
}
```

- [ ] **Step 2: Rewrite `_summary.html.j2`**

Replace the entire template content with the dashboard grid layout:

```html
{# Summary dashboard — scope → readiness → triage #}

<div class="summary-dashboard{% if not fleet_meta %} summary-dashboard-single{% endif %}">
  {# System card #}
  <div class="summary-card summary-card-system">
    <div class="summary-card-label">System</div>
    <div class="summary-card-value">{{ meta.os_pretty_name }}</div>
    <div class="summary-card-detail">
      {% if fleet_meta %}{{ fleet_meta.total_hosts }} host{{ 's' if fleet_meta.total_hosts != 1 else '' }} · {% endif %}
      Base: {{ meta.target_image or 'auto-detected' }}
    </div>
  </div>

  {% if fleet_meta %}
  {# Prevalence control card (fleet only) #}
  <div class="summary-card summary-card-prevalence">
    <div class="summary-card-label">Fleet Prevalence ⚡</div>
    <input type="range" id="summary-prevalence-slider" min="1" max="100"
           value="{{ fleet_meta.min_prevalence }}"
           data-current-threshold="{{ fleet_meta.min_prevalence }}">
    <div class="summary-card-detail" id="summary-prevalence-detail">
      <span id="summary-prevalence-value">{{ fleet_meta.min_prevalence }}%</span> ·
      <span id="summary-prevalence-counts"></span>
    </div>
  </div>
  {% endif %}

  {# Migration scope card #}
  <div class="summary-card summary-card-scope">
    <div class="summary-card-label">Migration Scope</div>
    <div class="summary-card-value">
      <span id="summary-scope-included">{{ triage.automatic + triage.fixme + triage.manual }}</span>
      {% if fleet_meta %}<span class="summary-card-detail" style="font-size: 0.8rem;"> of <span id="summary-scope-total">{{ triage.automatic + triage.fixme + triage.manual }}</span></span>{% endif %}
    </div>
    <div class="summary-card-detail">
      {{ counts.packages_added }} pkgs · {{ counts.config_modified + counts.config_unmanaged }} configs · {{ counts.services_changed }} svcs
    </div>
  </div>

  {# Needs attention card #}
  <div class="summary-card summary-card-attention{% if not fleet_meta %} summary-card-attention-full{% endif %}">
    <div class="summary-card-label">Needs Attention</div>
    <div class="summary-card-value" id="summary-attention-count">{{ triage.fixme + triage.manual }}</div>
    <div class="summary-card-detail">
      <span id="summary-review-count">{{ triage.fixme }}</span> review ·
      <span id="summary-manual-count">{{ triage.manual }}</span> manual
    </div>
  </div>
</div>

{# Section priority list #}
<div class="summary-priority-list">
  <div class="summary-priority-heading">Sections by Priority</div>
  <div id="summary-priority-rows">
    {# Populated by JS based on triage data #}
  </div>
</div>
```

Note: The exact Jinja context variables (`triage.automatic`, `counts.packages_added`, etc.) are already available from the existing template — verify the variable names match. The section priority list rows will be populated by JS from triage data.

- [ ] **Step 3: Add live prevalence update JS to `_js.html.j2`**

Add a new function that wires the summary prevalence slider to live-update the summary cards. Place this near the existing prevalence slider code:

```javascript
// Summary tab prevalence — live card updates
(function() {
  var summarySlider = document.getElementById('summary-prevalence-slider');
  if (!summarySlider) return;  // single-host mode, no slider

  var valueEl = document.getElementById('summary-prevalence-value');
  var countsEl = document.getElementById('summary-prevalence-counts');
  var scopeEl = document.getElementById('summary-scope-included');
  var totalEl = document.getElementById('summary-scope-total');
  var attentionEl = document.getElementById('summary-attention-count');

  function updateSummaryCards(threshold) {
    var result = countAtThreshold(threshold);
    valueEl.textContent = threshold + '%';
    countsEl.textContent = result.included + ' included · ' + result.excluded + ' below threshold';
    scopeEl.textContent = result.included;
    if (totalEl) totalEl.textContent = result.included + result.excluded;
    // Approximate attention count — items included that are not automatic
    // This is a client-side approximation; Re-render gives exact numbers
    attentionEl.textContent = Math.max(0, result.included - {{ triage.automatic }});
  }

  summarySlider.addEventListener('input', function() {
    updateSummaryCards(parseInt(this.value, 10));
  });

  // Sync with toolbar slider if both exist
  var toolbarSlider = document.getElementById('prevalence-slider');
  if (toolbarSlider) {
    summarySlider.addEventListener('change', function() {
      toolbarSlider.value = this.value;
      toolbarSlider.dispatchEvent(new Event('input'));
    });
  }

  // Initial render
  updateSummaryCards(parseInt(summarySlider.value, 10));
})();
```

Also add JS to populate the section priority list:

```javascript
// Section priority list — ranked by attention needed
(function() {
  var container = document.getElementById('summary-priority-rows');
  if (!container) return;

  var sections = [
    {% for sec in section_triage %}
    { name: '{{ sec.name }}', id: '{{ sec.id }}', manual: {{ sec.manual }}, review: {{ sec.review }}, auto: {{ sec.auto }} },
    {% endfor %}
  ];

  // Sort: manual desc, then review desc, then auto desc
  sections.sort(function(a, b) {
    if (b.manual !== a.manual) return b.manual - a.manual;
    if (b.review !== a.review) return b.review - a.review;
    return b.auto - a.auto;
  });

  var html = '';
  for (var i = 0; i < sections.length; i++) {
    var s = sections[i];
    var statusClass, statusText;
    if (s.manual > 0) {
      statusClass = 'summary-priority-status-manual';
      statusText = s.manual + ' manual';
    } else if (s.review > 0) {
      statusClass = 'summary-priority-status-review';
      statusText = s.review + ' review';
    } else {
      statusClass = 'summary-priority-status-auto';
      statusText = '\u2713 auto';
    }
    html += '<div class="summary-priority-row" data-nav-section="' + s.id + '">' +
      '<span>' + s.name + '</span>' +
      '<span class="' + statusClass + '">' + statusText + '</span>' +
      '</div>';
  }
  container.innerHTML = html;

  // Click to navigate
  container.addEventListener('click', function(e) {
    var row = e.target.closest('.summary-priority-row');
    if (row) {
      var sectionId = row.getAttribute('data-nav-section');
      var link = document.querySelector('[data-nav="' + sectionId + '"]');
      if (link) link.click();
    }
  });
})();
```

Note: The `section_triage` Jinja variable may need to be constructed in the template or passed from the renderer. Check whether this data is already available. If not, construct it from the existing `triage` and `counts` objects inline.

- [ ] **Step 4: Verify and commit**

```bash
uv run --extra dev pytest -q
```

Visually verify: Summary tab shows 4-card grid (fleet) or 3-card grid (single-host). Prevalence slider updates card numbers live. Section priority list is clickable. Toggle dark/light theme.

```bash
git add src/yoinkc/templates/report/_summary.html.j2 src/yoinkc/templates/report/_css.html.j2 src/yoinkc/templates/report/_js.html.j2
git commit -m "feat(refine): Redesign summary tab with dashboard grid and live prevalence

Replace flat description list with 4-card dashboard grid: system, prevalence
control, migration scope, and needs attention. Prevalence slider updates
card counts in real time via client-side approximation. Section priority
list ranked by attention needed with click-to-navigate.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 4: Prevalence Control Migration

**Files:**
- Modify: `src/yoinkc/templates/report/_toolbar.html.j2` (remove slider)
- Modify: `src/yoinkc/templates/report/_macros.html.j2` (add threshold badge)
- Modify: `src/yoinkc/templates/report/_js.html.j2` (sync summary slider with toolbar logic)

- [ ] **Step 1: Remove prevalence slider from toolbar**

In `_toolbar.html.j2`, remove the `{% if fleet_meta and refine_mode %}` block containing the `fleet-slider-group` div (lines 5-16 approximately). Keep all other toolbar elements: Reset, Re-render, Download Tarball, Download Modified Snapshot, status text.

- [ ] **Step 2: Add prevalence badge to section macro**

In `_macros.html.j2`, inside the `section` macro's card header, add a fleet-only prevalence badge:

```html
{% macro section(id, title, visible=false, triageable=false) -%}
<div id="section-{{ id }}" class="section pf-v6-c-card{% if visible %} visible{% endif %}">
  <div class="pf-v6-c-card__header">
    <div class="pf-v6-c-card__title"><h2>{{ title }}</h2></div>
    <div class="pf-v6-c-card__actions">
      {% if fleet_meta %}
      <span class="pf-v6-c-badge pf-m-read prevalence-badge" data-nav-section="summary"
            title="Click to adjust prevalence threshold"
            style="cursor: pointer;">
        Prevalence: <span class="prevalence-badge-value">{{ fleet_meta.min_prevalence }}%</span>
      </span>
      {% endif %}
      {% if triageable %}
      <button class="pf-v6-c-button pf-m-secondary pf-m-small mark-reviewed-btn"
              data-section="{{ id }}">Mark as reviewed</button>
      {% endif %}
    </div>
  </div>
  <div class="pf-v6-c-card__body">
    {{ caller() }}
  </div>
</div>
{%- endmacro %}
```

- [ ] **Step 3: Wire prevalence badge click and sync**

In `_js.html.j2`, add click handler for prevalence badges and sync badge values when slider changes:

```javascript
// Prevalence badge click → navigate to summary
document.addEventListener('click', function(e) {
  var badge = e.target.closest('.prevalence-badge');
  if (badge) {
    var summaryLink = document.querySelector('[data-nav="summary"]');
    if (summaryLink) summaryLink.click();
  }
});

// Update all prevalence badges when summary slider changes
var summarySlider = document.getElementById('summary-prevalence-slider');
if (summarySlider) {
  summarySlider.addEventListener('input', function() {
    var val = this.value + '%';
    var badges = document.querySelectorAll('.prevalence-badge-value');
    for (var i = 0; i < badges.length; i++) {
      badges[i].textContent = val;
    }
  });
}
```

- [ ] **Step 4: Clean up toolbar CSS**

In `_css.html.j2`, remove or mark as unused any CSS classes related to the toolbar prevalence slider: `.fleet-slider-group`, `.fleet-slider-label`, `.fleet-slider`, `.fleet-slider-value`, `.fleet-slider-preview`, `.fleet-slider-apply`, `.fleet-slider-cancel`.

- [ ] **Step 5: Verify and commit**

```bash
uv run --extra dev pytest -q
```

Visually verify: Toolbar no longer shows prevalence slider. Section headers show "Prevalence: 80%" badge in fleet mode. Badge click navigates to Summary. Badge value updates when Summary slider moves.

```bash
git add src/yoinkc/templates/report/_toolbar.html.j2 src/yoinkc/templates/report/_macros.html.j2 src/yoinkc/templates/report/_js.html.j2 src/yoinkc/templates/report/_css.html.j2
git commit -m "feat(refine): Move prevalence control from toolbar to summary tab

Remove prevalence slider from bottom toolbar. Add read-only prevalence
badge to section headers (fleet mode only, clickable to navigate to
summary). Sync badge values with summary slider.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 5: Component Upgrades

**Files:**
- Modify: `src/yoinkc/templates/report/_css.html.j2` (remove custom component styles, add PF6)
- Modify: `src/yoinkc/templates/report/_js.html.j2` (update component selectors)
- Modify: `src/yoinkc/templates/report/_macros.html.j2` (spinner macro)
- Modify: `src/yoinkc/templates/report/_sidebar.html.j2` (badge upgrade)
- Modify: `src/yoinkc/templates/report/_warnings.html.j2` (dismiss animation)

Each sub-step is an independent component swap.

- [ ] **Step 1: Replace spinner with PF6 spinner**

Find the custom `.spinner` CSS class in `_css.html.j2` and the corresponding HTML usage. Replace with PF6:

```html
<!-- Before -->
<span class="spinner"></span>

<!-- After -->
<span class="pf-v6-c-spinner pf-m-sm" role="progressbar" aria-label="Loading">
  <span class="pf-v6-c-spinner__clipper"></span>
  <span class="pf-v6-c-spinner__lead-ball"></span>
  <span class="pf-v6-c-spinner__tail-ball"></span>
</span>
```

Remove the custom `.spinner` CSS (keyframe animation and sizing).

- [ ] **Step 2: Replace toast with PF6 alert group**

Find the custom `.toast` CSS and HTML. Replace with PF6 toast pattern:

```html
<!-- Before -->
<div class="toast" id="toast">...</div>

<!-- After -->
<div class="pf-v6-c-alert-group pf-m-toast" id="toast-group">
  <div class="pf-v6-c-alert pf-m-success" id="toast">
    <div class="pf-v6-c-alert__icon">
      <i class="fas fa-check-circle" aria-hidden="true"></i>
    </div>
    <p class="pf-v6-c-alert__title" id="toast-message"></p>
  </div>
</div>
```

Update JS that shows/hides the toast to use the new selectors. Remove custom `.toast` CSS.

- [ ] **Step 3: Upgrade sidebar triage badges**

In `_sidebar.html.j2`, replace the custom triage count styling with PF6 badges:

```html
<!-- Before -->
<span class="triage-count" data-triage-section="rpm">(47)</span>

<!-- After -->
<span class="pf-v6-c-badge pf-m-unread triage-badge" data-triage-section="rpm">47</span>
```

Add CSS for badge status coloring:

```css
.triage-badge-reviewed {
  background: var(--pf-t--global--color--status--success--default, #3e8635);
}
.triage-badge-attention {
  background: var(--pf-t--global--color--status--warning--default, #f0ab00);
}
```

Update JS that modifies triage counts to use new badge classes.

- [ ] **Step 4: Add warning dismiss animation**

In `_css.html.j2`, add a dismiss transition:

```css
.pf-v6-c-alert.dismissing {
  opacity: 0;
  max-height: 0;
  margin: 0;
  padding: 0;
  overflow: hidden;
  transition: opacity 150ms ease-out, max-height 150ms ease-out, margin 150ms ease-out, padding 150ms ease-out;
}
```

In `_warnings.html.j2` or the JS that handles dismiss, instead of `element.style.display = 'none'`:

```javascript
// Before
warningEl.style.display = 'none';

// After
warningEl.classList.add('dismissing');
setTimeout(function() { warningEl.style.display = 'none'; }, 150);
```

- [ ] **Step 5: Rebuild fleet popover as PF6 popover**

This is the largest component swap. Replace the custom fleet popover with PF6 `pf-v6-c-popover`:

```html
<div class="pf-v6-c-popover" role="dialog" aria-label="Fleet details">
  <div class="pf-v6-c-popover__arrow"></div>
  <div class="pf-v6-c-popover__content">
    <div class="pf-v6-c-popover__header">
      <h3 class="pf-v6-c-popover__title">Fleet Breakdown</h3>
    </div>
    <div class="pf-v6-c-popover__body">
      <!-- fleet detail content -->
    </div>
  </div>
</div>
```

Update the JS that creates/positions the popover to use PF6 classes and positioning patterns. Remove all custom fleet popover CSS.

- [ ] **Step 6: Verify and commit**

```bash
uv run --extra dev pytest -q
```

Visually verify each component: spinner appears during re-render, toast shows on actions, sidebar badges show correct counts with colors, warnings fade out on dismiss, fleet popover opens/closes correctly in both themes.

```bash
git add src/yoinkc/templates/report/_css.html.j2 src/yoinkc/templates/report/_js.html.j2 src/yoinkc/templates/report/_macros.html.j2 src/yoinkc/templates/report/_sidebar.html.j2 src/yoinkc/templates/report/_warnings.html.j2
git commit -m "feat(refine): Upgrade hand-rolled components to PF6 equivalents

Replace custom spinner, toast, and fleet popover with PF6 components.
Upgrade sidebar triage counts to PF6 badges with status colors.
Add warning dismiss fade-out transition.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 6: Polish

**Files:**
- Modify: `src/yoinkc/templates/report.html.j2` (masthead branding)
- Modify: `src/yoinkc/templates/report/_css.html.j2` (typography, active states, accents)
- Modify: `src/yoinkc/templates/report/_js.html.j2` (fleet bar active state)

Ranked by priority — implement in order, stop if time runs out.

- [ ] **Step 1: Masthead branding (highest priority)**

In `report.html.j2`, change the masthead brand text and add typography:

```html
<span class="pf-v6-c-masthead__brand">yoinkc Refine</span>
```

In `_css.html.j2`, add masthead typography to match architect:

```css
.pf-v6-c-masthead__brand {
  font-size: 1.125rem;
  font-weight: 700;
  letter-spacing: 0.03em;
}
```

- [ ] **Step 2: Warning dismiss transition**

Already handled in Task 5, Step 4. Skip if done.

- [ ] **Step 3: Section heading consistency**

In `_css.html.j2`, style section subheadings to match architect:

```css
.summary-priority-heading,
.pf-v6-c-card__title h3 {
  font-size: 0.6875rem;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--pf-t--global--text--color--subtle, #8a8d90);
}
```

Note: Be selective — the `h2` headings in section card titles should remain as-is. Only apply to `h3` subheadings and section labels.

- [ ] **Step 4: Fleet bar active state**

In `_css.html.j2`, add:

```css
.fleet-bar-segment.active {
  outline: 2px solid var(--pf-t--global--color--brand--default, #0066cc);
  outline-offset: 1px;
  z-index: 1;
}
```

In `_js.html.j2`, toggle the `.active` class when a fleet popover is opened/closed:

```javascript
// When opening popover for a fleet bar segment
segment.classList.add('active');

// When closing popover
document.querySelectorAll('.fleet-bar-segment.active').forEach(function(el) {
  el.classList.remove('active');
});
```

- [ ] **Step 5: Package table group header accents**

In `_css.html.j2`, add a left-border accent to table group headers:

```css
.table-group-header td {
  border-left: 3px solid var(--pf-t--global--color--status--info--default, #2b9af3);
}
```

Use different accent colors for different group types if the group type is distinguishable via a class.

- [ ] **Step 6: Final verification and commit**

```bash
uv run --extra dev pytest -q
```

Full visual check: masthead says "yoinkc Refine" with correct typography, section headings are consistently styled, fleet bars highlight when popover is open, package groups have accent borders. Toggle dark/light theme. Check single-host and fleet modes.

```bash
git add src/yoinkc/templates/report.html.j2 src/yoinkc/templates/report/_css.html.j2 src/yoinkc/templates/report/_js.html.j2
git commit -m "feat(refine): Polish — masthead branding, section headings, fleet bar states

Update masthead to 'yoinkc Refine' with shared typography spec.
Add uppercase letter-spaced section subheadings. Add fleet bar
active state on popover open. Add group header border accents.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Self-Review Checklist

- [x] **Spec coverage:** All 5 spec sections (summary redesign, prevalence migration, component upgrades, CSS cleanup, polish) have corresponding tasks
- [x] **Exclusions honored:** Editor/file browser explicitly excluded. Dynamic inline styles excluded from extraction.
- [x] **Token sweep scoped correctly:** Palette vars noted as case-by-case, not blanket replace
- [x] **Live summary metrics:** Uses existing `countAtThreshold()` + `snapshot` object, not nonexistent `data-prevalence` scheme
- [x] **Toolbar:** Only prevalence controls removed, all other buttons preserved
- [x] **No AI team member names or model names in file content** (only in commit `Assisted-by` lines)
- [x] **Post-talk items not included:** Kill-Re-render, editor polish, triage parity, fleet split button all deferred

## Notes for Implementer

- **Test after every task.** Run `uv run --extra dev pytest -q` (expect 1028 passed) and visually verify in both dark and light themes.
- **Generate test data** with driftify multi-fleet fixtures before starting. You need both fleet-mode and single-host reports to verify all layouts.
- **The `section_triage` variable** in Task 3 may need to be constructed. Check whether the renderer passes per-section triage data to the template. If not, construct it from the `snapshot` object inline in the template or add a simple Jinja loop.
- **PF6 CDN:** The refine report includes PF6 via CDN link in `report.html.j2`. Verify the version matches the components referenced (v6 class names).
