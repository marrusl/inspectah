# Config Variant Visual Hierarchy

**Date:** 2026-03-31
**Status:** Proposed
**Author:** Kit (via brainstorm with Mark, Ember assessment)
**Extends:** `2026-03-22-variant-auto-selection-design.md` — this spec adds Tier 2 (auto-selected badge) and chevron affordance on top of the Mar 22 spec. Tie styling (gold), auto-selection logic, and tie detection are unchanged. The Mar 22 spec remains the source of truth for variant selection behavior.

## Goal

Add a three-tier visual hierarchy to config file variant groups in the refine report, so users can instantly distinguish ties (must resolve), auto-selected winners (worth reviewing), and clean files (no variation). Add chevron affordance to make expand/collapse discoverable.

## Definitions

**Tie:** A variant group where two or more variants share the highest `fleet.count` AND no variant has `include=true`. Operationally: all items in the group have `include=false`. This is the definition from the Mar 22 auto-selection spec, unchanged.

**Auto-selected:** A variant group where one variant has strictly higher `fleet.count` than all others, and that variant has `include=true`. The tool picked the winner automatically.

**Clean (no variation):** A config file with only one entry for that path — identical across all hosts, or only present on one host. No variant group exists.

## Apr 8 Scope

**In scope (this sprint):**

- Config section only (`_config.html.j2`, `_css.html.j2`, `_js.html.j2`)
- Chevron affordance on config variant group headers
- Auto-selected badge on config variant group headers
- Pre-expand ties on page load
- E2E test coverage for the three tiers

**Deferred (post-Apr 8):**

- Editor tree variant display (`_editor.html.j2`, `_editor_js.html.j2`) — follow-on, same pattern
- Services and containers variant groups — follow-on if they use the same variant group pattern
- Summary-line noise mitigation for large fleets (>10 auto-selected groups)

This narrowing reduces regression risk and screenshot timing concerns for the demo.

## Design

### Tier 1: Tied (must resolve)

**Visual:** Gold left border + tinted background (per Mar 22 spec). If the current implementation uses red/danger colors for ties, revert to gold (`#cc8800`).

**Badge text:** "⚠ N variants · tied — compare & choose"

**Color:** Gold/warning — `#cc8800` as established in the Mar 22 auto-selection spec. Ties are decision points, not errors. Gold says "your input needed" without implying something is broken. If the current implementation uses red/danger styling for ties, revert to gold to match the original spec intent.

**Expand state:** Pre-expanded on page load. JS runs on `DOMContentLoaded` and expands all tied variant groups. This happens only on initial page load, NOT after prevalence slider changes (the slider is a preview — ties are recomputed on re-render).

### Tier 2: Auto-selected (worth reviewing)

**Visual:** No row border or background tint. Just the badge on the group header row.

**Badge:** Light blue tinted pill — `"N variants · auto-selected"`. Informational, not actionable.

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

**Expand state:** Collapsed by default.

### Tier 3: No variation (clean)

**Visual:** No badge, no indicator. A clean file is a config file entry that has no sibling entries with the same path — it's a single row in the config table, not a variant group. No change from current behavior.

### Chevron Affordance

**Implementation:** CSS-rotated Unicode chevron via `::before` pseudo-element on the variant group toggle element. This is the single chosen approach — no SVG, no `.chevron` class reuse.

```css
.fleet-variant-toggle::before {
  content: "▶";
  display: inline-block;
  font-size: 0.6rem;
  margin-right: 0.4rem;
  transition: transform 150ms ease;
  color: var(--pf-t--global--text--color--subtle, #8a8d90);
}

.fleet-variant-toggle.expanded::before {
  transform: rotate(90deg);
}
```

The toggle element already has `cursor: pointer`. The chevron makes the affordance visible. Applied only to config variant groups in this sprint.

**Keyboard and semantics (minimal):** The toggle stays keyboard-operable with the existing pattern (focusable; Enter/Space toggles). `aria-expanded` must match the visible state (e.g. `true` when the group is expanded / `.fleet-variant-children` is shown). The tie badge “⚠” is decorative alongside the full label text — meaning must not depend on the glyph alone.

### Renderer Verification

**Claim: no Python/renderer changes needed.** This must be verified during implementation by confirming:

1. The template context includes variant group state (which variant has `include=true`) for both initial render and post-re-render
2. The tie detection logic in the renderer (`_build_context` in `html_report.py`) correctly identifies tied vs auto-selected groups
3. After prevalence threshold changes and re-render, the tier assignments are still correct

If any of these fail, the implementer must flag it as BLOCKED rather than working around it in JS.

## Testing

**Selector contract:** Class names referenced in the tables below (`.fleet-variant-toggle`, `.expanded`, `.variant-auto-badge`, `.variant-tie-badge`, `.fleet-variant-children`, etc.) are the **required DOM hooks** for this sprint’s markup and E2E assertions — not optional examples. If implementation renames them, update this spec and `variant-selection.spec.ts` in the same change so acceptance criteria stay traceable.

### E2E Tests (add to existing `variant-selection.spec.ts`)

| Test | Assertion |
| ------ | --------- |
| Tied groups are pre-expanded on load | `.fleet-variant-toggle.expanded` exists for tied groups, `.fleet-variant-children` visible |
| Auto-selected groups show blue badge | `.variant-auto-badge` visible on groups with a winner, text contains "auto-selected" |
| Clean files have no badge | Config rows without variant groups have no `.variant-auto-badge` or `.variant-tie-badge` |
| Chevron reflects expand/collapse | Click toggle → toggle gains/loses `.expanded` and `.fleet-variant-children` visibility toggles accordingly (assert class + children visibility; do not assert `::before` transform — that is an implementation detail of the chevron) |
| Auto badge readable in light mode | Default light theme: `.variant-auto-badge` text vs. pill background must meet WCAG 2.x **normal** text contrast (minimum 4.5∶1), e.g. by sampling computed foreground/background or validating the PF info tokens / rgba fill used in implementation |
| 2-way tie shows expanded with Compare | Pre-expanded, Compare buttons visible |
| 3-way tie shows expanded with Display | Pre-expanded, Display buttons visible |

### Python Tests (verify renderer)

| Test | Assertion |
| ------ | --------- |
| Tied group renders tie badge | HTML output contains `variant-tie-badge` for equal-count groups |
| Auto-selected group renders auto badge | HTML output contains `variant-auto-badge` for winner groups |
| Single-variant file has no badge | HTML output has no variant badge for non-grouped files |
