# Summary Tab Overhaul Design

**Date:** 2026-03-16
**Status:** Proposed

## Problem

The summary tab has three issues:

1. **Non-native styling** — readiness panel uses custom `.readiness-*`
   CSS classes with manual color-coded borders and spacing instead of
   PF6 card/status components.
2. **Poor layout** — content is sparse with too much whitespace, doesn't
   use horizontal space well.
3. **Missing fleet context** — no overview of fleet refinement state
   (include/exclude counts, variant breakdown by category).

## Scope

**Files:**
- Modify: `src/yoinkc/templates/report/_summary.html.j2` — full rewrite
- Modify: `src/yoinkc/templates/report/_css.html.j2` — remove custom
  summary classes
- Modify: `src/yoinkc/renderers/html_report.py` — add `variant_summary`
  to template context

### Out of scope

- Triage calculation logic (`_triage.py` unchanged)
- Sidebar navigation
- Fleet banner (`_banner.html.j2` stays separate)
- Schema or snapshot structure

## Design

### Two-Column Dashboard Layout

Replace the current single-column layout with a two-column CSS grid.
Use PF6 spacing tokens for gaps. Collapses to single column on narrow
viewports via `@media` query or PF6 grid breakpoints.

**Left column:**
1. System Information card
2. Migration Readiness cards (stacked)

**Right column:**
1. Breakdown card
2. Fleet Overview card (fleet mode only)

**Bottom (full width):**
1. Next Steps card

### System Information

`pf-v6-c-card` containing a `pf-v6-c-description-list` with:
- Hostname
- OS name
- Inspection timestamp
- Host root (if non-default)

Already uses PF6 description list — just needs the card wrapper added.

### Migration Readiness

Three `pf-v6-c-card` items stacked vertically. Each card shows:
- Status label (Automatic / Needs Review / Manual)
- Count (large, bold)
- Left border accent via PF6 status modifier or inline border using
  PF6 status color tokens:
  - Automatic: `--pf-t--global--color--status--success--default`
  - Needs Review: `--pf-t--global--color--status--warning--default`
  - Manual: `--pf-t--global--color--status--danger--default`

Cards with count 0 are hidden (existing behavior preserved).

### Breakdown

`pf-v6-c-card` containing a compact list of triage detail items.
Each row shows:
- Category name as a clickable link (navigates to the relevant tab
  via `show('tab_id')`)
- Count, color-coded by triage status using PF6 status color tokens

Data source: existing `triage_detail` context variable (list of
`{label, count, tab, status}` dicts from `compute_triage_detail()`).

### Fleet Overview (fleet mode only)

Rendered only when `fleet_meta` is present (`{% if fleet_meta %}`).

`pf-v6-c-card` with blue border accent
(`--pf-t--global--color--status--info--default`).

**Header:** "Fleet Overview" with subtitle showing host count and
prevalence threshold (e.g., "3 hosts · 60% threshold").

**Body:**
- Include/exclude counts as compact description list
- "Categories with variants:" followed by a list of categories that
  have multi-variant files. Each row shows:
  - Category name as clickable link (navigates to tab via `show()`)
  - "N files · M variants" count

**Python change** — add `variant_summary` to template context in
`html_report.py`:

```python
variant_summary = []
for label, groups, tab in [
    ("Config files", config_variant_groups, "config"),
    ("Drop-ins", dropin_variant_groups, "drop_ins"),
    ("Quadlet units", quadlet_variant_groups, "containers"),
]:
    multi = {path: vs for path, vs in groups.items() if len(vs) > 1}
    if multi:
        variant_summary.append({
            "label": label,
            "tab": tab,
            "files": len(multi),
            "variants": sum(len(v) for v in multi.values()),
        })
```

Only categories with multi-variant files appear in the list. If no
variants exist, the "Categories with variants" section is hidden.

### Next Steps

Full-width `pf-v6-c-card` at the bottom. Three ordered steps:

1. **Review** — "Check the Audit report tab for warnings and review
   any FIXME comments in the generated Containerfile."
2. **Refine** (optional) — "Use `yoinkc-refine` to interactively
   toggle package and config inclusions, then re-render the
   Containerfile."
   - Single host: `./yoinkc-refine hostname-*.tar.gz`
   - Fleet: `./yoinkc-refine fleet-*.tar.gz`
   - Template uses `{% if fleet_meta %}` to show the appropriate
     command.
3. **Build** — "Build your bootc image from the generated tarball."
   - Single host: `./yoinkc-build hostname-*.tar.gz my-image:latest`
   - Fleet: `./yoinkc-build fleet-*.tar.gz my-image:latest`
   - Same conditional for the command.

Rendered as an ordered list inside the card body. Code snippets use
`<code>` inline elements.

### CSS Cleanup

Remove from `_css.html.j2`:
- `.readiness-panel`
- `.readiness-col`
- `.readiness-auto`
- `.readiness-fixme`
- `.readiness-manual`
- `.readiness-header`
- `.readiness-count`
- `.readiness-link`
- `.summary-hero`
- `.summary-next`

Add minimal CSS for the two-column grid layout using PF6 spacing
tokens. The grid definition and responsive breakpoint are the only
new custom CSS needed.

## Testing

- **Layout:** Verify two-column layout on wide viewport, single-column
  on narrow.
- **System info:** Hostname, OS, timestamp display correctly.
- **Readiness cards:** Counts match triage calculation. Cards with 0
  count are hidden. Color accents match status.
- **Breakdown:** All triage detail items listed with correct counts and
  colors. Links navigate to correct tabs.
- **Fleet overview (fleet mode):** Host count, threshold, include/exclude
  counts correct. Variant list shows only categories with multi-variant
  files. Links navigate to correct tabs.
- **Fleet overview (non-fleet):** Card is not rendered.
- **Next steps:** Correct commands shown for single-host vs fleet.
  Code snippets render in monospace.
- **Existing Python tests:** Run `pytest tests/ -x -q` — triage tests
  should pass unchanged. May need to add a test for `variant_summary`
  context variable.
