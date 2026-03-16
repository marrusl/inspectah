# Toggle Switches: Replace Include/Exclude Checkboxes

**Date:** 2026-03-15
**Status:** Proposed

## Problem

The HTML report uses PF6 checkboxes (`pf-v6-c-check`) for include/exclude controls in refine mode. When unchecked, rows get strikethrough text + opacity 0.45. This visual treatment is harsh and the checkbox component doesn't match the toggle semantics — users are toggling items on/off, not selecting from a list.

## Decision

Replace all `pf-v6-c-check` checkboxes with `pf-v6-c-switch` toggle switches. Change excluded-item styling from strikethrough + opacity 0.45 to opacity 0.6 (no strikethrough).

## Scope

### Visibility

- Toggle switches are **hidden by default** (`display: none`), shown only when refine mode activates — same lifecycle as current checkboxes.
- Excluded items rendered during live editing appear at **opacity 0.6** with toggle off. After re-render, excluded items are omitted from the HTML entirely (renderer skips `include: false` items, unchanged behavior).

### Placement

- Left side, same position as current checkboxes (first column for tables, leading element for cards).
- Standalone — no label text next to the toggle. The row content serves as the label.

### Approach

Minimal swap — direct 1:1 replacement of PF6 component markup across all affected files. No new macros, no JS-driven rendering.

## Affected Files

### Templates (10 files)

Each template's checkbox markup changes from:

```html
<span class="pf-v6-c-check pf-m-standalone include-cb-wrap">
  <input type="checkbox" class="pf-v6-c-check__input include-cb"
         {{ 'checked' if item.include else '' }}/>
</span>
```

To:

```html
<label class="pf-v6-c-switch include-toggle-wrap">
  <input type="checkbox" class="pf-v6-c-switch__input include-toggle"
         {{ 'checked' if item.include else '' }}/>
  <span class="pf-v6-c-switch__toggle"></span>
</label>
```

Note: `pf-m-standalone` is dropped — it is a `pf-v6-c-check` modifier that does not exist on `pf-v6-c-switch`.

**Secondary classes must be preserved.** Some checkboxes carry additional classes used by JS:
- `_packages.html.j2`: inputs have `include-cb leaf-cb` or `include-cb repo-cb`. These become `include-toggle leaf-cb` and `include-toggle repo-cb`. The `leaf-cb` and `repo-cb` classes are referenced by 7+ JS selectors and must not be dropped.
- `_packages.html.j2`: default repo checkboxes have `disabled title="Default distribution repository — cannot be excluded"`. This `disabled` attribute carries forward to the switch input. PF6 switch disabled styling (muted track/knob) applies automatically.

Affected templates under `src/yoinkc/templates/report/`:

1. `_containers.html.j2`
2. `_non_rpm.html.j2`
3. `_users_groups.html.j2`
4. `_services.html.j2`
5. `_config.html.j2`
6. `_kernel_boot.html.j2`
7. `_network.html.j2`
8. `_packages.html.j2`
9. `_scheduled_jobs.html.j2`
10. `_toolbar.html.j2` (comment references to JS-targeted classes only)

### CSS (`_css.html.j2`)

From:

```css
tr.excluded td { text-decoration: line-through; opacity: 0.45; }
tr.excluded td:first-child { text-decoration: none; opacity: 1; }
div.excluded { text-decoration: line-through; opacity: 0.45; }
.pf-v6-c-check.pf-m-standalone.include-cb-wrap { display: none; }
```

To:

```css
tr.excluded td { opacity: 0.6; }
tr.excluded td:first-child { opacity: 1; }
div.excluded { opacity: 0.6; }
.pf-v6-c-switch.include-toggle-wrap { display: none; }
```

### JavaScript (`_js.html.j2`)

- Rename all `.include-cb` → `.include-toggle` and `.include-cb-wrap` → `.include-toggle-wrap` across all selectors. There are 6+ occurrences of `.include-cb` in `_js.html.j2` (change event listener, refine-mode activation, toolbar counting, reset button, and others). All must be renamed — grep to confirm none are missed.
- Event handler logic unchanged — `change` event, snapshot mutation, `classList.toggle('excluded')`, `updateToolbar()`, `setDirty()`
- Refine-mode activation sets `display: ''` on `.include-toggle-wrap` elements. PF6 switch uses `display: inline-grid` (same as `pf-v6-c-check.pf-m-standalone`), so the existing `el.style.display = 'inline-grid'` activation is already correct — no change needed.

## What Does NOT Change

- **Renderer behavior:** items with `include: false` are still omitted from re-rendered HTML. No change to the Python renderer.
- **Snapshot schema:** the `include` boolean field is unchanged.
- **Containerfile generation:** still reads `include` and skips excluded items.
- **Event handler logic:** same flow (toggle → mutate snapshot → toggle `.excluded` class → update toolbar → set dirty).
- **Refine-mode lifecycle:** toggles hidden by default, shown on activation, hidden again after re-render.

## PF6 Component Details

The `pf-v6-c-switch` component is already available in the bundled `patternfly.css` (v6.4.0). It provides:

- Checked state: blue track (`--pf-t--global--color--brand--default`), white knob
- Unchecked state: grey track, subtle knob
- Disabled state: muted colors (used for default distribution repo switches that cannot be excluded)
- Focus state: blue outline
- Accessible: keyboard-operable, ARIA semantics built in

No custom CSS needed for the switch component itself.

## Implementation Notes

- **Table cell sizing:** PF6 switch is wider than a checkbox (~36px vs ~16px). Test that `pf-v6-c-table__check` cells accommodate the switch without layout issues. If cramped, a small `min-width` on the check cell may be needed.
- **Config template markup:** `_config.html.j2` uses multi-line checkbox markup (2-3 lines) unlike the single-line format in most templates. Implementers should handle both formats.
- **Toolbar comments:** `_toolbar.html.j2` has comments referencing `.include-cb` / `.include-cb-wrap` as JS-targeted classes. Update these comments too.

## Alternatives Considered

1. **Jinja2 macro** — extract switch markup into a shared macro. Rejected: marginal DRY benefit for ~3 lines of markup, adds indirection.
2. **JS-driven rendering** — inject switch HTML at runtime. Rejected: fights the Jinja2-first architecture, harder to debug.
3. **Visible in all views** — show disabled toggles in read-only mode. Rejected: re-render removes excluded items for a clean view, so toggles in read-only would be misleading (everything visible would be included, making the toggle state redundant).
