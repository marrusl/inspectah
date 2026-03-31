# Config Variant Visual Hierarchy

**Date:** 2026-03-31
**Status:** Proposed
**Author:** Kit (via brainstorm with Mark, Ember assessment)

## Goal

Add a three-tier visual hierarchy to config file variant groups in the refine report, so users can instantly distinguish ties (must resolve), auto-selected winners (worth reviewing), and clean files (no variation).

## Context

Fleet mode surfaces config file variants — the same path with different content across hosts. Currently:
- Ties are highlighted with warning styling and "must fix" callouts (shipped in UI overhaul)
- Auto-selected winners look identical to non-variant files — users don't know variation exists unless they expand the group
- Clean files (no variation) have no indicator

This leaves a trust gap: the tool silently picks a winner and the user doesn't know it happened.

## Design

### Three Tiers

**Tier 1: Tied (must resolve)**
- Red/warning left border + tinted background (already implemented)
- Bold badge: "⚠ N variants · tied — choose one"
- **Pre-expanded on page load** — children visible immediately
- Compare buttons (2-way) or Display buttons (3+)

**Tier 2: Auto-selected (worth reviewing)**
- No row border or background tint — visually quiet
- Light blue tinted badge: "N variants · auto-selected"
- Collapsed by default (user expands to review)
- Compare buttons available on non-selected variants

**Tier 3: No variation (clean)**
- No indicator, no badge
- Collapsed by default
- No change from current behavior

### Chevron Affordance

All expandable variant groups get a directional chevron:
- ▶ when collapsed
- ▼ when expanded

Use the existing `.chevron` CSS class (already defined but not rendered in HTML). For consistency with the file browser, use the PF6 tree-view SVG chevron (`_TOGGLE_ICON_SVG` constant already in `html_report.py`), or a CSS-rotated Unicode chevron if simpler.

The chevron makes the expand/collapse interaction discoverable — currently users only find it by accident.

### Scope

**Files to modify:**
- `src/yoinkc/templates/report/_config.html.j2` — add chevron, auto-selected badge, tie pre-expand
- `src/yoinkc/templates/report/_css.html.j2` — auto-selected badge styling, chevron rotation
- `src/yoinkc/templates/report/_js.html.j2` — pre-expand ties on page load, chevron toggle
- `src/yoinkc/templates/report/_editor.html.j2` — same chevron treatment in editor tree
- `src/yoinkc/templates/report/_editor_js.html.j2` — editor tree chevron behavior

**Also apply to services and containers** if they have variant groups (same pattern).

### What NOT to change

- Tie styling — already implemented and working
- Summary dashboard — existing variant drift callout is sufficient
- No new Python/renderer changes needed — all variant state data is already in the template context

### Auto-selected Badge CSS

```css
.variant-auto-badge {
  background: rgba(43, 154, 243, 0.12);
  color: var(--pf-t--global--color--status--info--default, #2b9af3);
  padding: 0.15rem 0.5rem;
  border-radius: 3px;
  font-size: 0.75rem;
}

html:not(.pf-v6-theme-dark) .variant-auto-badge {
  background: rgba(43, 154, 243, 0.08);
  color: #0066cc;
}
```

### Pre-expand Ties

On page load, JS finds all variant groups where all variants have `include=false` (no winner selected = tie) and expands them by triggering the toggle. Non-tied groups stay collapsed.

### Testing

- E2E tests should verify: ties are expanded on load, auto-selected groups show blue badge, clean files have no badge
- The existing variant-selection E2E tests cover tie/Compare/Display behavior — this adds visual tier assertions
