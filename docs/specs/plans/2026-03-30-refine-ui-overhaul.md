# Refine UI Overhaul Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring the refine report UI to architect's quality level — cohesive PF6 design tokens, redesigned summary tab with live-updating prevalence control, upgraded components, and visual polish.

**Architecture:** Template-only changes across 16+ Jinja2 files in `src/yoinkc/templates/report/`. No renderer changes needed — `triage_detail` already provides per-section breakdown data in the existing context. CSS token migration is the foundation; summary tab redesign is the centerpiece; component swaps and polish follow. All prevalence live-update logic uses the existing `snapshot` object and a hoisted `countAtThreshold()` function.

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

/* Summary grid — already exists in _summary.html.j2 */
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
- Modify: `src/yoinkc/templates/report/_js.html.j2` (hoist `countAtThreshold`, live prevalence update logic)

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

/* Counter animation for card values */
.summary-card-value .counter-animate {
  display: inline-block;
  transition: transform 200ms ease, opacity 200ms ease;
}
.summary-card-value .counter-animate.updating {
  transform: translateY(-4px);
  opacity: 0.3;
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

- [ ] **Step 2a: Rewrite `_summary.html.j2`**

Replace the entire template content. This template uses variables that actually exist in the renderer context (verified in `html_report.py` `_build_context()`):

- `os_desc` — OS description string
- `meta` — the `snapshot.meta` dict (has `hostname`, `timestamp`, `host_root`)
- `fleet_meta` — `FleetMeta` object (has `total_hosts`, `min_prevalence`) or None
- `counts` — dict with keys: `packages_added`, `config_files`, `services_enabled`, `services_disabled`, `n_included`, `n_excluded`, `containers`, `non_rpm`, etc.
- `triage` — dict with keys: `automatic`, `fixme`, `manual`
- `triage_detail` — list of dicts, each: `{label, count, tab, status}` where status is `"automatic"`, `"fixme"`, or `"manual"`
- `variant_summary` — list of dicts for fleet variant info

```jinja2
{% from "report/_macros.html.j2" import section with context -%}
{# ── Summary ────────────────────────────────────────────────────────────── #}
{% call section("summary", "Summary", visible=true) %}

<div class="summary-dashboard{% if not fleet_meta %} summary-dashboard-single{% endif %}">
  {# ── System card ───────────────────────────────────────────────────── #}
  <div class="summary-card summary-card-system">
    <div class="summary-card-label">System</div>
    <div class="summary-card-value">{{ os_desc }}</div>
    <div class="summary-card-detail">
      {% if fleet_meta %}{{ fleet_meta.total_hosts }} host{{ 's' if fleet_meta.total_hosts != 1 else '' }} · {{ fleet_meta.min_prevalence }}% prevalence threshold{% endif %}
      {% if not fleet_meta %}{{ meta.hostname|default('—') }}{% endif %}
    </div>
  </div>

  {% if fleet_meta %}
  {# ── Prevalence control card (fleet only) ──────────────────────────── #}
  <div class="summary-card summary-card-prevalence">
    <div class="summary-card-label">Fleet Prevalence</div>
    <input type="range" id="summary-prevalence-slider" min="1" max="100"
           value="{{ fleet_meta.min_prevalence }}"
           data-current-threshold="{{ fleet_meta.min_prevalence }}">
    <div class="summary-card-detail" id="summary-prevalence-detail">
      <span id="summary-prevalence-value">{{ fleet_meta.min_prevalence }}%</span> ·
      <span id="summary-prevalence-counts">{{ counts.n_included }} included · {{ counts.n_excluded }} below threshold</span>
    </div>
  </div>
  {% endif %}

  {# ── Migration scope card ──────────────────────────────────────────── #}
  {# IMPLEMENTER NOTE — mixed data sources by design:
     - Total (included items) comes from countAtThreshold() — updates live with slider
     - Review/manual breakdown comes from server-rendered triage counts — only changes on Re-render
     These are intentionally from different sources. The total reflects the
     slider's "what if" preview, while review/manual are structural properties
     of the Containerfile (FIXMEs, warnings) that don't change with prevalence.
     The "Approximate preview" note on the Needs Attention card signals this to
     the user. Do NOT try to unify these into a single data source — the slider
     cannot know the true review/manual split without a full server re-render. #}
  <div class="summary-card summary-card-scope">
    <div class="summary-card-label">Migration Scope</div>
    <div class="summary-card-value">
      <span id="summary-scope-total" class="counter-animate">{{ triage.automatic + triage.fixme + triage.manual }}</span> items
    </div>
    <div class="summary-card-detail">
      <span id="summary-scope-auto" class="counter-animate">{{ triage.automatic }}</span> automatic ·
      <span id="summary-scope-fixme" class="counter-animate">{{ triage.fixme }}</span> review ·
      <span id="summary-scope-manual" class="counter-animate">{{ triage.manual }}</span> manual
    </div>
  </div>

  {# ── Needs attention card ──────────────────────────────────────────── #}
  <div class="summary-card summary-card-attention{% if not fleet_meta %} summary-card-attention-full{% endif %}">
    <div class="summary-card-label">Needs Attention</div>
    <div class="summary-card-value">
      <span id="summary-attention-count" class="counter-animate">{{ triage.fixme + triage.manual }}</span>
    </div>
    <div class="summary-card-detail">
      <span id="summary-review-count" class="counter-animate">{{ triage.fixme }}</span> review ·
      <span id="summary-manual-count" class="counter-animate">{{ triage.manual }}</span> manual
    </div>
    <div class="summary-card-detail" style="font-style:italic; opacity:0.4;">
      Approximate preview — Re-render for exact counts
    </div>
  </div>
</div>

{# ── Section Priority list (full width) ──────────────────────────────── #}
{# NOTE: triage_detail arrives in pipeline order (rpm → config → services…),
   but the design spec requires priority order: manual first, then review/fixme,
   then auto. Jinja renders the fallback rows in pipeline order; the JS below
   re-sorts them client-side so no-JS users still see all rows (just unsorted). #}
<div class="summary-priority-list">
  <div class="summary-priority-heading">Sections by Priority</div>
  <div id="summary-priority-rows">
    {# Rendered from triage_detail in pipeline order — JS re-sorts by priority #}
    {% for item in triage_detail %}
    <div class="summary-priority-row" data-nav-tab="{{ item.tab }}">
      <span>{{ item.label }}</span>
      <span class="summary-priority-status-{{ 'manual' if item.status == 'manual' else ('review' if item.status == 'fixme' else 'auto') }}">
        {% if item.status == 'manual' %}{{ item.count }} manual
        {% elif item.status == 'fixme' %}{{ item.count }} review
        {% else %}&check; {{ item.count }} auto
        {% endif %}
      </span>
    </div>
    {% endfor %}
  </div>
</div>

{# ── Next Steps (full width) ───────────────────────────────────────────── #}
<div class="pf-v6-c-card mt-lg">
  <div class="pf-v6-c-card__header">
    <div class="pf-v6-c-card__title"><h3>Next Steps</h3></div>
  </div>
  <div class="pf-v6-c-card__body">
    <ol>
      <li><strong>Review</strong> — Check the Audit report tab for warnings and review any FIXME comments in the generated Containerfile.</li>
      <li><strong>Refine</strong> (optional) — Use <code>./run-yoinkc.sh refine {% if fleet_meta %}fleet-*.tar.gz{% else %}hostname-*.tar.gz{% endif %}</code> to interactively toggle package and config inclusions, then re-render the Containerfile.</li>
      <li><strong>Build</strong> — Build your bootc image: <code>./yoinkc-build {% if fleet_meta %}fleet-*.tar.gz{% else %}hostname-*.tar.gz{% endif %} my-image:latest</code></li>
    </ol>
  </div>
</div>
{% endcall %}
```

**Key differences from existing template:**
- Uses `os_desc` (real variable, line 534 of `html_report.py`) not `meta.os_pretty_name` (does not exist)
- Uses `counts.n_included`, `counts.n_excluded` (real, lines 529-530) not fabricated count variables
- Uses `triage.automatic`, `triage.fixme`, `triage.manual` (real, from `compute_triage()`)
- Uses `triage_detail` (real, from `compute_triage_detail()`) for the priority list — each item has `.label`, `.count`, `.tab`, `.status`
- Does NOT use `counts.config_modified`, `counts.config_unmanaged`, `counts.services_changed` — these do not exist. Uses `triage.automatic` etc. as summary numbers instead.
- Preserves the Next Steps card below the priority list

- [ ] **Step 2b: (Approved backend exception) No additional renderer change needed**

The `triage_detail` variable already exists in the renderer context (line 725 of `html_report.py`). It contains per-section breakdown data with `label`, `count`, `tab`, and `status` fields — exactly what the priority list needs. The priority list is rendered server-side via the Jinja loop above, with JS sorting for dynamic reordering.

If dynamic JS-driven sorting is needed, the `triage_detail` data can be embedded as JSON:

```javascript
var triageDetail = {{ triage_detail | tojson }};
```

This uses existing context — no renderer modification required.

- [ ] **Step 3: Hoist `countAtThreshold()` and add live prevalence JS**

**Critical fix: `countAtThreshold()` scope.** Currently this function is defined inside the `{% if fleet_meta and refine_mode %}` IIFE block at line ~1501 of `_js.html.j2`. It is inside a closure and inaccessible to any code outside that IIFE.

**The fix:** Hoist `countAtThreshold()` and its helper `prevalenceInclude()` out of the IIFE and into the `{% if fleet_meta %}` section (which wraps both fleet_meta-only and refine_mode code). Place them at module scope so both the summary slider code and the existing prevalence IIFE can use them.

In `_js.html.j2`, **before** the `{% if fleet_meta and refine_mode %}` block (around line 1498), add:

```javascript
{% if fleet_meta %}
// Hoisted prevalence helpers — used by summary slider and toolbar slider
function prevalenceInclude(count, total, minPrev) {
  return (count * 100) >= (minPrev * total);
}

function countAtThreshold(threshold) {
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
{% endif %}
```

Then **remove** the duplicate `prevalenceInclude()` and `countAtThreshold()` definitions from inside the `{% if fleet_meta and refine_mode %}` IIFE (lines ~1512-1539 of the current file). The IIFE's existing code that calls these functions will now resolve them from the outer scope.

**Prevalence control on ALL fleet reports:** The summary slider and its JS must be rendered whenever `fleet_meta` is set, NOT only when `refine_mode` is true. The `{% if fleet_meta %}` guard (not `{% if fleet_meta and refine_mode %}`) ensures the prevalence card and its JS appear on all fleet reports. The slider provides a read-only preview of what different thresholds would mean; Re-render (only available in refine mode) commits the change.

Next, add the summary slider JS. Place this after the hoisted helpers, still inside the `{% if fleet_meta %}` block:

```javascript
// Summary tab prevalence — live card updates (all fleet reports)
(function() {
  var summarySlider = document.getElementById('summary-prevalence-slider');
  if (!summarySlider) return;

  var valueEl = document.getElementById('summary-prevalence-value');
  var countsEl = document.getElementById('summary-prevalence-counts');
  var scopeTotalEl = document.getElementById('summary-scope-total');
  var scopeAutoEl = document.getElementById('summary-scope-auto');
  var attentionEl = document.getElementById('summary-attention-count');
  var reviewEl = document.getElementById('summary-review-count');
  var manualEl = document.getElementById('summary-manual-count');

  // Static triage values from server render (fixme and manual don't change
  // with prevalence — they're Containerfile FIXMEs and warnings/redactions)
  var staticFixme = {{ triage.fixme }};
  var staticManual = {{ triage.manual }};

  function animateCounter(el, newValue) {
    if (!el) return;
    var newText = String(newValue);
    if (el.textContent === newText) return;
    el.classList.add('updating');
    setTimeout(function() {
      el.textContent = newText;
      el.classList.remove('updating');
    }, 100);
  }

  function updateSummaryCards(threshold) {
    var result = countAtThreshold(threshold);
    if (valueEl) valueEl.textContent = threshold + '%';
    if (countsEl) countsEl.textContent = result.included + ' included \u00b7 ' + result.excluded + ' below threshold';

    // Mixed data sources (intentional):
    //   Total = live from countAtThreshold() — changes with slider
    //   Review/manual = static from server triage — only changes on Re-render
    // The "Approximate preview" note on the card signals this to the user.
    // Do not try to compute review/manual client-side — that requires a full
    // Containerfile re-render to determine which items produce FIXMEs.
    var approxTotal = result.included + staticFixme + staticManual;
    var approxAuto = result.included;
    var attentionCount = staticFixme + staticManual;

    animateCounter(scopeTotalEl, approxTotal);
    if (scopeAutoEl) animateCounter(scopeAutoEl, approxAuto);
    animateCounter(attentionEl, attentionCount);
    if (reviewEl) animateCounter(reviewEl, staticFixme);
    if (manualEl) animateCounter(manualEl, staticManual);
  }

  summarySlider.addEventListener('input', function() {
    updateSummaryCards(parseInt(this.value, 10));
  });

  // IMPORTANT: Call on initial load to overwrite the Jinja-rendered counts
  // (which use counts.n_included / counts.n_excluded — RPM + config only)
  // with countAtThreshold() output (which covers all 8 section types).
  // Without this, the initial numbers come from a different counting system
  // than the slider-updated numbers, and the first slider move would change
  // the meaning of the counts, not just the values.
  updateSummaryCards(parseInt(summarySlider.value, 10));
})();
```

Also add JS for the priority list click-to-navigate:

```javascript
// Section priority list — sort by priority: manual first, then fixme/review, then auto
(function() {
  var container = document.getElementById('summary-priority-rows');
  if (!container) return;

  // Re-sort Jinja-rendered rows by priority (Jinja renders in pipeline order as a no-JS fallback)
  // Design spec: rank by manual count desc, then fixme/review count desc, then auto count desc
  var rows = Array.prototype.slice.call(container.children);
  var bucket = { manual: 0, fixme: 1, review: 1, auto: 2 };
  rows.sort(function(a, b) {
    var aStatus = a.querySelector('[class*="status-"]');
    var bStatus = b.querySelector('[class*="status-"]');
    var aMatch = aStatus && aStatus.className.match(/status-(\w+)/);
    var bMatch = bStatus && bStatus.className.match(/status-(\w+)/);
    var aBucket = aMatch ? (bucket[aMatch[1]] || 2) : 2;
    var bBucket = bMatch ? (bucket[bMatch[1]] || 2) : 2;
    if (aBucket !== bBucket) return aBucket - bBucket;
    // Within same bucket, sort by count descending
    var aCount = parseInt((aStatus && aStatus.textContent.match(/(\d+)/)) ? RegExp.$1 : '0', 10);
    var bCount = parseInt((bStatus && bStatus.textContent.match(/(\d+)/)) ? RegExp.$1 : '0', 10);
    return bCount - aCount;
  });
  rows.forEach(function(row) { container.appendChild(row); });

  // Click to navigate
  container.addEventListener('click', function(e) {
    var row = e.target.closest('.summary-priority-row');
    if (!row) return;
    var tabId = row.getAttribute('data-nav-tab');
    if (tabId && typeof show === 'function') {
      show(tabId);
    }
  });
})();
```

**Navigation selector note:** The priority list uses `data-nav-tab` on the row itself and calls the global `show()` function directly (which is exposed on `window` at line 78 of `_js.html.j2`). This is simpler and more correct than trying to find and click a nav link — `show()` handles history state, section visibility, and active link styling already. The `show()` function takes tab IDs matching `data-tab` attributes on `.pf-v6-c-nav__link` elements (e.g., `"summary"`, `"packages"`, `"config"`, `"services"`).

- [ ] **Step 4: Verify and commit**

```bash
uv run --extra dev pytest -q
```

Visually verify: Summary tab shows 4-card grid (fleet) or 3-card grid (single-host). Prevalence slider updates card numbers live with counter animation. Section priority list is clickable and navigates correctly. Next Steps card is preserved below the priority list. Toggle dark/light theme.

```bash
git add src/yoinkc/templates/report/_summary.html.j2 src/yoinkc/templates/report/_css.html.j2 src/yoinkc/templates/report/_js.html.j2
git commit -m "feat(refine): Redesign summary tab with dashboard grid and live prevalence

Replace flat description list with 4-card dashboard grid: system, prevalence
control, migration scope, and needs attention. Prevalence slider updates
card counts in real time via client-side approximation with counter
animation. Section priority list uses triage_detail with click-to-navigate.
Next Steps card preserved. Hoist countAtThreshold() to module scope.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 4: Prevalence Control Migration

**Files:**
- Modify: `src/yoinkc/templates/report/_toolbar.html.j2` (remove slider, keep other controls)
- Modify: `src/yoinkc/templates/report/_macros.html.j2` (add threshold badge to section headers)
- Modify: `src/yoinkc/templates/report/_js.html.j2` (migrate ALL prevalence-slider references, sync summary slider with re-render)
- Modify: `src/yoinkc/templates/report/_editor_js.html.j2` (update prevalence-slider reference in re-render handler)

**This task fully migrates the prevalence pipeline.** Every reference to `#prevalence-slider`, `#btn-apply-prevalence`, `#btn-cancel-prevalence`, `#prevalence-value`, and `#prevalence-preview` must be updated or removed.

- [ ] **Step 1: Remove prevalence slider from toolbar**

In `_toolbar.html.j2`, remove the `{% if fleet_meta and refine_mode %}` block containing the `fleet-slider-group` div (lines 4-18). This removes:
- `<input id="prevalence-slider">` (the toolbar slider)
- `<span id="prevalence-value">` (the value label)
- `<span id="prevalence-preview">` (the preview text)
- `<button id="btn-apply-prevalence">` (the Apply button)
- `<button id="btn-cancel-prevalence">` (the Cancel button)

Keep all other toolbar elements: Reset, Re-render buttons, Download Tarball, Download Modified Snapshot, status text.

After removal, the toolbar structure should be:

```jinja2
<div id="exclude-toolbar" class="pf-v6-c-toolbar" role="toolbar" aria-label="Actions">
  <button id="btn-reset" class="pf-v6-c-button pf-m-link pf-m-danger" type="button" disabled title="Reset all selections to their initial state">Reset</button>
  <span class="toolbar-status" id="toolbar-status-text"></span>
  <button id="btn-download-snapshot" class="pf-v6-c-button pf-m-secondary" type="button" title="Download snapshot with current selections as JSON">Download Modified Snapshot</button>
  {% if not refine_mode %}
  <button id="btn-rerender" class="pf-v6-c-button pf-m-primary" type="button" disabled title="Start yoinkc refine to enable live re-render">Re-render</button>
  {% endif %}
  {% if refine_mode %}
  <button id="btn-re-render" class="pf-v6-c-button pf-m-primary" type="button" disabled title="Re-render with editor changes">Re-render</button>
  {% endif %}
  <button id="btn-tarball" class="pf-v6-c-button pf-m-secondary" type="button" disabled title="Start yoinkc refine to enable tarball download">Download Tarball</button>
</div>
<div id="toast" class="pf-v6-c-alert pf-m-danger toast"></div>
```

- [ ] **Step 2: Migrate ALL `#prevalence-slider` references in `_js.html.j2`**

There are four reference sites for `#prevalence-slider` in `_js.html.j2` that must be migrated to `#summary-prevalence-slider`:

**2a. The `resetToOriginal()` function (line ~607-620):**

Current code reads `document.getElementById('prevalence-slider')` and resets it. Change to:

```javascript
// Reset prevalence slider to original value
var slider = document.getElementById('summary-prevalence-slider');
if (slider && originalSnapshot.meta && originalSnapshot.meta.fleet) {
  var origThreshold = originalSnapshot.meta.fleet.min_prevalence;
  slider.value = origThreshold;
  slider.setAttribute('data-current-threshold', origThreshold);
  var valueLabel = document.getElementById('summary-prevalence-value');
  if (valueLabel) valueLabel.textContent = origThreshold + '%';
  var countsLabel = document.getElementById('summary-prevalence-counts');
  if (countsLabel) {
    var result = countAtThreshold(origThreshold);
    countsLabel.textContent = result.included + ' included \u00b7 ' + result.excluded + ' below threshold';
  }
}
```

Note: The `#btn-apply-prevalence`, `#btn-cancel-prevalence`, and `#prevalence-preview` references in the old reset code are removed entirely — the summary slider has no Apply/Cancel buttons. The slider updates cards as a live preview; Re-render is the commit action.

**2b. The Re-render handler (line ~671-673):**

Current code reads `document.getElementById('prevalence-slider')` to get the threshold before sending the re-render request. Change to:

```javascript
var sliderEl = document.getElementById('summary-prevalence-slider');
if (snapshot.meta && snapshot.meta.fleet && sliderEl) {
  var newThreshold = parseInt(sliderEl.value, 10);
  // CRITICAL: Update include flags BEFORE posting — the server re-renders
  // from the posted snapshot as-is, so stale include flags produce a report
  // with the new threshold label but old inclusions.
  if (typeof applyPrevalenceThreshold === 'function') {
    applyPrevalenceThreshold(newThreshold);
  }
  snapshot.meta.fleet.min_prevalence = newThreshold;
}
```

**Why this ordering matters:** `applyPrevalenceThreshold(value)` walks every `[data-snap-section]` element, recalculates `item.include` based on the new threshold via `prevalenceInclude()`, and handles variant radio constraints. Without this call, the snapshot posted to `/api/re-render` carries `min_prevalence = newValue` but all the per-item `include` flags still reflect the *old* threshold — the server faithfully renders from those stale flags, producing a report where the threshold label changed but the actual included items did not.

**2c. The `{% if fleet_meta and refine_mode %}` IIFE (line ~1501-1671):**

This entire IIFE currently manages the toolbar slider with Apply/Cancel buttons. **Rewrite it** to be a thin wrapper that only handles the `applyPrevalenceThreshold()` function (which toggles DOM elements based on threshold). The slider input/Apply/Cancel event handlers are removed since those buttons no longer exist.

The IIFE should become:

```javascript
{% if fleet_meta and refine_mode %}
(function() {
  // Wire summary slider to apply prevalence on Re-render
  var summarySlider = document.getElementById('summary-prevalence-slider');
  if (!summarySlider) return;

  // applyPrevalenceThreshold() — called when re-render commits the threshold
  // This function updates DOM include/exclude state for all fleet items.
  // The implementation is unchanged from the existing code (lines 1575-1669).
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

    // Phase 2: Apply variant radio constraint (unchanged from current code)
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
      variants.sort(function(a, b) {
        var ac = a.item.fleet ? a.item.fleet.count : 0;
        var bc = b.item.fleet ? b.item.fleet.count : 0;
        return bc - ac;
      });
      var topCount = variants[0].item.fleet ? variants[0].item.fleet.count : 0;
      var isTied = variants.length >= 2 &&
        (variants[1].item.fleet ? variants[1].item.fleet.count : 0) === topCount;

      if (isTied) {
        variants.forEach(function(v) {
          v.item.include = false;
          var cb = v.el.querySelector('.include-toggle');
          if (cb) cb.checked = false;
          v.el.classList.add('excluded');
        });
      } else {
        var foundIncluded = false;
        variants.forEach(function(v) {
          if (v.item.include && !foundIncluded) {
            foundIncluded = true;
          } else if (v.item.include) {
            v.item.include = false;
            var cb = v.el.querySelector('.include-toggle');
            if (cb) cb.checked = false;
            v.el.classList.add('excluded');
          }
        });
        if (!foundIncluded) {
          variants[0].item.include = true;
          var winCb = variants[0].el.querySelector('.include-toggle');
          if (winCb) winCb.checked = true;
          variants[0].el.classList.remove('excluded');
        }
      }
    });

    Object.keys(groups).forEach(function(group) {
      syncParentGroupRows(group);
    });

    // Phase 3: Update UI
    updateToolbar();
    setDirty(!isSnapshotClean());
    recalcTriageCounts();
    if (typeof updateCompareButtons === 'function') {
      var seen = {};
      document.querySelectorAll('[data-variant-group]').forEach(function(el) {
        seen[el.getAttribute('data-variant-group')] = true;
      });
      Object.keys(seen).forEach(function(g) { updateCompareButtons(g); });
    }
    if (typeof editorRefreshTree === 'function') editorRefreshTree();
  }

  // Expose for the re-render handler
  window.applyPrevalenceThreshold = applyPrevalenceThreshold;
})();
{% endif %}
```

Note: The `applyPrevalenceThreshold` function now uses the hoisted `prevalenceInclude()` from Task 3 Step 3 — no change needed inside the function body.

- [ ] **Step 3: Update `_editor_js.html.j2` prevalence-slider reference**

> **Design spec exception:** `_editor_js.html.j2` is listed as excluded in the design spec (theme-breaking colors only), but the prevalence slider ID migration is a necessary functional change — the editor's re-render handler references the slider ID and must be updated to avoid a runtime error. Without this change, clicking Re-render in the editor tab would silently fail to read the prevalence threshold because `document.getElementById('prevalence-slider')` returns `null` after the toolbar slider is removed.

In `_editor_js.html.j2`, the re-render click handler (line ~862-865) reads `document.getElementById('prevalence-slider')`. Change to:

```javascript
var sliderEl = document.getElementById('summary-prevalence-slider');
if (snapshot.meta && snapshot.meta.fleet && sliderEl) {
  var newThreshold = parseInt(sliderEl.value, 10);
  // Update include flags before posting (same critical fix as main re-render handler)
  if (typeof applyPrevalenceThreshold === 'function') {
    applyPrevalenceThreshold(newThreshold);
  }
  snapshot.meta.fleet.min_prevalence = newThreshold;
}
```

- [ ] **Step 4: Add prevalence badge to section macro**

In `_macros.html.j2`, inside the `section` macro's card header, add a fleet-only prevalence badge. This badge appears on ALL fleet reports (not just refine_mode) since it shows the threshold that was used for the current view.

Modify the `section` macro to add badge support:

```jinja2
{% macro section(id, title, visible=false, triageable=false) -%}
<div id="section-{{ id }}" class="section{% if visible %} visible{% endif %}">
  <div class="pf-v6-c-card">
    <div class="pf-v6-c-card__header">
      <div class="pf-v6-c-card__title"><h2>{{ title }}</h2></div>
      {% if fleet_meta and id != 'summary' %}
      <span class="pf-v6-c-badge pf-m-read prevalence-badge"
            title="Click to adjust prevalence threshold"
            style="cursor: pointer; margin-left: auto;">
        Prevalence: <span class="prevalence-badge-value">{{ fleet_meta.min_prevalence }}%</span>
      </span>
      {% endif %}
    </div>
    <div class="pf-v6-c-card__body">
    {%- if triageable %}
    <div style="display: flex; justify-content: flex-end; margin-bottom: var(--pf-t--global--spacer--sm);">
      <button class="pf-v6-c-button pf-m-secondary pf-m-small" onclick="toggleReviewed('{{ id }}', this)" data-tab-id="{{ id }}">Mark as reviewed</button>
    </div>
    {%- endif %}
    {{- caller() }}
    </div>
  </div>
</div>
{%- endmacro %}
```

Note: The badge is excluded from the summary section (`id != 'summary'`) since the summary tab has the actual slider control.

- [ ] **Step 5: Wire prevalence badge click and sync**

In `_js.html.j2`, add click handler for prevalence badges (navigates to summary tab) and sync badge values when the summary slider changes. Place inside `{% if fleet_meta %}`:

```javascript
// Prevalence badge click → navigate to summary tab
document.addEventListener('click', function(e) {
  var badge = e.target.closest('.prevalence-badge');
  if (badge && typeof show === 'function') {
    show('summary');
  }
});

// Update all prevalence badges when summary slider changes
var summarySliderForBadges = document.getElementById('summary-prevalence-slider');
if (summarySliderForBadges) {
  summarySliderForBadges.addEventListener('input', function() {
    var val = this.value + '%';
    document.querySelectorAll('.prevalence-badge-value').forEach(function(badge) {
      badge.textContent = val;
    });
  });
}
```

- [ ] **Step 6: Clean up toolbar CSS**

In `_css.html.j2`, remove or mark as unused any CSS classes related to the toolbar prevalence slider: `.fleet-slider-group`, `.fleet-slider-label`, `.fleet-slider`, `.fleet-slider-value`, `.fleet-slider-preview`, `.fleet-slider-apply`, `.fleet-slider-cancel`.

- [ ] **Step 7: Migration completeness checklist**

Before committing, verify ALL references are migrated:

```bash
# These should return ZERO results:
grep -n 'prevalence-slider' src/yoinkc/templates/report/_toolbar.html.j2
grep -n 'btn-apply-prevalence' src/yoinkc/templates/report/_js.html.j2
grep -n 'btn-cancel-prevalence' src/yoinkc/templates/report/_js.html.j2
grep -n 'prevalence-preview' src/yoinkc/templates/report/_js.html.j2
grep -n 'prevalence-value"' src/yoinkc/templates/report/_js.html.j2

# These should show ONLY the summary slider:
grep -n 'prevalence-slider' src/yoinkc/templates/report/_js.html.j2
grep -n 'prevalence-slider' src/yoinkc/templates/report/_editor_js.html.j2
grep -n 'prevalence-slider' src/yoinkc/templates/report/_summary.html.j2
```

- [ ] **Step 8: Verify and commit**

```bash
uv run --extra dev pytest -q
```

Visually verify: Toolbar no longer shows prevalence slider. Section headers show "Prevalence: 80%" badge in fleet mode (not on summary tab itself). Badge click navigates to Summary. Badge value updates when Summary slider moves. Re-render still works correctly (reads from summary slider). Reset button restores summary slider to original value.

```bash
git add src/yoinkc/templates/report/_toolbar.html.j2 src/yoinkc/templates/report/_macros.html.j2 src/yoinkc/templates/report/_js.html.j2 src/yoinkc/templates/report/_editor_js.html.j2 src/yoinkc/templates/report/_css.html.j2
git commit -m "feat(refine): Move prevalence control from toolbar to summary tab

Remove prevalence slider and Apply/Cancel buttons from bottom toolbar.
Summary slider is now the single prevalence control, on all fleet reports.
Migrate all #prevalence-slider references in _js.html.j2, _editor_js.html.j2.
Add read-only prevalence badge to section headers (fleet mode, clickable).
Drop Apply/Cancel — slider updates summary cards live, Re-render commits.

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

Update the JS in `_js.html.j2` that sets `rerenderBtn.innerHTML = '<span class="spinner"></span>Re-rendering...'` (line ~669) to use the PF6 spinner markup.

Remove the custom `.spinner` CSS (keyframe animation and sizing).

- [ ] **Step 2: Replace toast with PF6 alert group**

Find the custom `.toast` CSS and HTML. Replace with PF6 toast pattern.

**Important:** The refine report does NOT load Font Awesome, so do NOT use `fas fa-*` icon classes. Use a Unicode check mark instead.

```html
<!-- Before (in _toolbar.html.j2) -->
<div id="toast" class="pf-v6-c-alert pf-m-danger toast"></div>

<!-- After -->
<div class="pf-v6-c-alert-group pf-m-toast" id="toast-group">
  <div class="pf-v6-c-alert pf-m-success" id="toast">
    <div class="pf-v6-c-alert__icon">
      <span aria-hidden="true">&#x2714;</span>
    </div>
    <p class="pf-v6-c-alert__title" id="toast-message"></p>
  </div>
</div>
```

**Update `showToast()` JS in `_js.html.j2`** to write to the new `#toast-message` element and toggle visibility on the `#toast-group` container:

```javascript
// Before (current code, line ~232 of _js.html.j2):
function showToast(msg, ms) {
  if (!toast) return;
  toast.textContent = msg;
  toast.classList.add('visible');
  setTimeout(function(){ toast.classList.remove('visible'); }, ms || 4000);
}

// After:
var toastGroup = document.getElementById('toast-group');
var toastMessage = document.getElementById('toast-message');

function showToast(msg, ms) {
  if (!toastGroup || !toastMessage) return;
  toastMessage.textContent = msg;
  toastGroup.classList.add('visible');
  setTimeout(function(){ toastGroup.classList.remove('visible'); }, ms || 4000);
}
```

Also update the `toast` variable declaration (line ~167) — it currently does `var toast = document.getElementById('toast')` which is used for both display and error messages. With the new markup, error toasts should set `pf-m-danger` on the `#toast` alert div:

```javascript
// For error toasts (e.g., re-render failure), swap the alert modifier:
function showToast(msg, ms, isError) {
  if (!toastGroup || !toastMessage) return;
  var alertEl = document.getElementById('toast');
  if (alertEl) {
    alertEl.classList.toggle('pf-m-success', !isError);
    alertEl.classList.toggle('pf-m-danger', !!isError);
  }
  toastMessage.textContent = msg;
  toastGroup.classList.add('visible');
  setTimeout(function(){ toastGroup.classList.remove('visible'); }, ms || 4000);
}
```

Update the re-render error catch to pass the error flag: `showToast('Re-render failed: ' + err.message, 5000, true)`.

Remove custom `.toast` CSS.

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

Update JS that modifies triage counts to use new badge classes. The `recalcTriageCounts()` function in `_js.html.j2` (line ~749) queries `.triage-count[data-triage-section="..."]` — update to `.triage-badge[data-triage-section="..."]` or keep both selectors for backwards compatibility.

- [ ] **Step 4: Add warning dismiss animation**

In `_css.html.j2`, add a dismiss transition:

```css
.warning-row.dismissing {
  opacity: 0;
  max-height: 0;
  margin: 0;
  padding: 0;
  overflow: hidden;
  transition: opacity 150ms ease-out, max-height 150ms ease-out, margin 150ms ease-out, padding 150ms ease-out;
}
```

In `_js.html.j2`, the warning dismiss handler (line ~116) currently does `this.closest('.warning-row').classList.add('dismissed')`. Change to animate first:

```javascript
var row = this.closest('.warning-row');
row.style.maxHeight = row.scrollHeight + 'px';
void row.offsetHeight; // force reflow
row.classList.add('dismissing');
setTimeout(function() {
  row.classList.add('dismissed');
  row.classList.remove('dismissing');
  row.style.maxHeight = '';
  updateWarningBadge();
}, 150);
```

Move `updateWarningBadge()` inside the timeout so the count updates after the animation.

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
.fleet-bar.active {
  outline: 2px solid var(--pf-t--global--color--brand--default, #0066cc);
  outline-offset: 1px;
  z-index: 1;
}
```

In `_js.html.j2`, toggle the `.active` class when a fleet popover is opened/closed. In the fleet bar click handler (line ~1346):

```javascript
// When opening popover
this.classList.add('active');

// When closing popover (in the document click handler, line ~1476)
if (activePopover) {
  activePopover.closest('.fleet-bar').classList.remove('active');
  // ... existing cleanup
}
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
- [x] **All Jinja variables verified against `html_report.py`:** `os_desc`, `counts.config_files`, `triage.automatic/fixme/manual`, `triage_detail`, `fleet_meta`, `meta`, `counts.n_included/n_excluded` — all exist in `_build_context()`
- [x] **No fabricated variables:** `meta.os_pretty_name`, `meta.target_image`, `counts.config_modified`, `counts.config_unmanaged`, `counts.services_changed` are NOT used (they don't exist). No renderer changes needed — `triage_detail` already exists in context.
- [x] **`countAtThreshold()` scope fixed:** Hoisted to module scope under `{% if fleet_meta %}` so it's accessible from both the summary slider IIFE and the prevalence IIFE
- [x] **Prevalence pipeline fully migrated:**
  - `_toolbar.html.j2`: slider + Apply/Cancel removed
  - `_js.html.j2` `resetToOriginal()`: `#prevalence-slider` → `#summary-prevalence-slider`, Apply/Cancel refs removed
  - `_js.html.j2` re-render handler: `#prevalence-slider` → `#summary-prevalence-slider`
  - `_js.html.j2` IIFE: rewritten to remove slider input/Apply/Cancel handlers, uses hoisted helpers
  - `_editor_js.html.j2` re-render handler: `#prevalence-slider` → `#summary-prevalence-slider`
  - No orphaned references to `#prevalence-slider`, `#btn-apply-prevalence`, `#btn-cancel-prevalence`
- [x] **Navigation uses correct selectors:** `show('tabId')` via the global `show()` function (exposed on `window`), which operates on `.pf-v6-c-nav__link[data-tab]` elements. Does NOT use nonexistent `[data-nav="..."]` selectors.
- [x] **Needs Attention card uses simple approach:** Shows static `triage.fixme + triage.manual` values. Fixme (Containerfile FIXMEs) and manual (warnings + redactions) don't change with prevalence — only automatic items change. The card notes it's an approximation and directs to Re-render for exact counts.
- [x] **Migration Scope card mixed semantics documented:** Total from `countAtThreshold()` (live), review/manual from server triage (static). Intentional — comments in both template and JS explain rationale so implementer doesn't try to unify.
- [x] **Section priority list sorted by priority:** JS re-sorts Jinja-rendered rows (manual first, fixme/review, auto last). No-JS fallback shows pipeline order.
- [x] **Toast migration does not use Font Awesome:** Unicode check mark (`&#x2714;`) instead of `fas fa-check-circle`. `showToast()` JS updated to write to `#toast-message` and toggle `#toast-group`.
- [x] **`_editor_js.html.j2` exception documented:** Prevalence slider ID migration is a necessary functional change despite the file being listed as design-spec-excluded.
- [x] **Next Steps card preserved:** Kept below the Section Priority list as a workflow guide
- [x] **Counter animation included:** CSS `counter-animate` class with JS `animateCounter()` function for smooth number updates
- [x] **Prevalence control on ALL fleet reports:** Guarded by `{% if fleet_meta %}` not `{% if fleet_meta and refine_mode %}`, showing the slider as a read-only preview on non-refine fleet reports too
- [x] **Toolbar:** Only prevalence controls removed, all other buttons preserved
- [x] **No team member names or model names in file content** (only in commit `Assisted-by` lines)
- [x] **Post-talk items not included:** Kill-Re-render, editor polish, triage parity, fleet split button all deferred

## Notes for Implementer

- **Test after every task.** Run `uv run --extra dev pytest -q` (expect 1028 passed) and visually verify in both dark and light themes.
- **Generate test data** with driftify multi-fleet fixtures before starting. You need both fleet-mode and single-host reports to verify all layouts.
- **Verify the migration checklist** (Task 4, Step 7) — `grep` for orphaned `prevalence-slider` references. This is the highest-risk area for subtle bugs.
- **The `triage_detail` variable** is already in the context (line 725 of `html_report.py`). Each entry has `{label, count, tab, status}` where status is `"automatic"`, `"fixme"`, or `"manual"`. The `tab` values match sidebar `data-tab` attributes (e.g., `"packages"`, `"config"`, `"services"`, `"containerfile"`, `"warnings"`, `"secrets"`, `"users_groups"`, `"scheduled_tasks"`, `"network"`).
- **PF6 CDN:** The refine report includes PF6 via CDN link in `report.html.j2`. Verify the version matches the components referenced (v6 class names).
