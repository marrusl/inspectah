# Refine UI Overhaul — Design Spec

**Date:** 2026-03-30
**Status:** Approved (with revisions)
**Owner:** Mark Russell
**Reviewer:** Mark Russell
**Estimated effort:** ~11 hours
**Deadline:** April 5 (stop coding), April 8 (talk day)

## Overview

The refine report UI needs a visual and structural overhaul to reach the quality
bar set by the architect tool. This spec covers a summary tab redesign with
interactive prevalence control, migration of hand-rolled components to PF6
equivalents, a full CSS token sweep, inline style extraction, and polish items.
The goal is a cohesive, theme-aware report that demos cleanly on April 8.

## Design Principles

**Sibling relationship to architect.** Refine and architect are siblings from the
same family — they share a PF6 base, typography scale, color palette, theme
toggle, badge system, and transition timing. Differences emerge from content
density needs (refine has 29 template files vs architect's 3), not from
intentional divergence.

**Shared elements:**
- Masthead typography: `1.125rem`, weight `700`, `0.03em` letter-spacing
- Dark/light theme toggle mechanism
- PF6 design tokens (`--pf-t--global--*`)
- Badge system: `pf-v6-c-badge` with status colors
- Transition timing: `150ms ease`
- Left-border accents on cards

**Refine-specific:**
- Higher information density (tables, toggles, fleet popovers)
- Interactive prevalence control
- Section-based navigation with triage badges
- Embedded file editor (out of scope for this overhaul)

## Changes

### 1. Summary Tab Redesign

The summary tab is the landing page and currently the biggest visual gap versus
architect. Replace the flat description list and stacked colored cards with a
dashboard grid layout.

**Layout: 2×2 card grid (fleet mode)**

| Position     | Card                    | Content                                                    |
|-------------|-------------------------|------------------------------------------------------------|
| Top-left    | **System**              | OS name/version, host count, base image. Read-only.        |
| Top-right   | **Prevalence Control**  | Interactive slider with glow/border affordance. Fleet only. |
| Bottom-left | **Migration Scope**     | Total items with category breakdown ("59 of 90" style).    |
| Bottom-right| **Needs Attention**     | Items requiring review or manual intervention.             |

**Prevalence control card behavior:**
- Slider with subtle glow or border change to signal interactivity
- Dragging updates Migration Scope and Needs Attention cards in real time
- Display text: "X included · Y below threshold (still visible)"
- All recalculation is client-side JS against snapshot data — no server round-trip
- Card numbers animate on update (counter transition)

**Live summary metric approach:**
- Summary cards use **client-side approximation** by reusing (or extending) the
  existing `countAtThreshold()` function, which iterates the embedded `snapshot`
  object and checks `item.fleet.count` / `item.fleet.total` via
  `prevalenceInclude()` to determine which items meet the current threshold
- No `data-prevalence` DOM attributes are involved — all prevalence data lives
  in the `snapshot` object already available to JS at render time
- This is an approximation — it won't match `src/inspectah/renderers/_triage.py` exactly (misses
  Containerfile FIXME counts, for example)
- Cards should signal this is a preview (e.g., counts update smoothly but the
  Re-render button is the "commit" action)
- **Backlog item:** Full client-side reimplementation of `src/inspectah/renderers/_triage.py`
  logic for exact parity. Low priority, post-talk.

**Single-host mode:**
- No prevalence control card — irrelevant without fleet data
- Grid layout: System (top-left), Migration Scope (top-right), Needs Attention
  (full-width bottom). No prevalence card.

**Section Priority list (full width, below grid):**
- Sections ranked by: manual count (highest first), then fixme/review count,
  then auto count. Sections with zero attention items show "✓ auto" in green.
- Each row shows section name + red/yellow/green status badge
- Clickable — navigates to the corresponding section tab

### 2. Prevalence Control Migration

**From:** Bottom toolbar slider (always visible, takes up fixed space)
**To:** Summary tab prevalence card (contextually placed, live-updating)

**Section tab headers:** A small read-only PF6 badge (`pf-v6-c-badge`) in each
section's card header, fleet-only (conditional on `fleet_meta`), displaying
"Prevalence: 80%" (or whatever the current setting is). Badge is clickable and
navigates back to the Summary tab where the slider lives. Hook point is the
`section` macro in `_macros.html.j2`.

**Toolbar simplification:** Remove the prevalence slider and its associated
controls from `_toolbar.html.j2`. All other toolbar elements (Reset, Re-render,
Download Tarball, Download Modified Snapshot, status text) are unchanged.

### 3. Component Upgrades

| Component          | Before (current)                | After (target)                                   |
|--------------------|---------------------------------|--------------------------------------------------|
| Fleet popover      | Custom CSS popover              | `pf-v6-c-popover` (full rebuild)                 |
| Loading spinner    | `.spinner` custom CSS           | `pf-v6-c-spinner pf-m-sm`                        |
| Toast notification | `.toast` custom CSS             | `pf-v6-c-alert-group` toast pattern               |
| Sidebar badges     | Custom triage count styling     | `pf-v6-c-badge` with status colors               |
| Warning dismiss    | Instant `display:none`          | 150ms `ease-out` opacity+height fade transition   |
| Readiness cards    | Inline `style="..."` borders    | CSS classes: `.readiness-card-success`, `-warning`, `-danger`, `-info` |

### 4. CSS Cleanup & Token Migration

**Token sweep (single pass):**
All `--pf-v6-global--` references → `--pf-t--global--` design token equivalents.
Grep the entire `_css.html.j2` and any inline `<style>` blocks in templates.

**Note:** This sweep applies to **semantic tokens only.** `--pf-v6-global--palette--*`
references have no direct `--pf-t--` equivalents and need case-by-case mapping.
These are handled individually during the hex audit, not as part of the blanket
find-and-replace.

**Hardcoded hex audit — replace with PF6 tokens:**
| Hex value   | Typical usage            | Target token                                              |
|-------------|--------------------------|-----------------------------------------------------------|
| `#fff`      | Backgrounds, text        | `--pf-t--global--background--color--primary--default`     |
| `#151515`   | Dark text/backgrounds    | `--pf-t--global--text--color--regular` (see note below)   |
| `#f0f0f0`   | Light backgrounds        | `--pf-t--global--background--color--secondary--default`   |
| `#e0e0e0`   | Fleet bar track          | `--pf-t--global--background--color--secondary--default`   |
| `#d2d2d2`   | Borders, muted elements  | `--pf-t--global--border--color--default`                  |
| `#8a8d90`   | Muted text               | `--pf-t--global--text--color--subtle`                     |

**Note on `#151515`:** This hex value is used as both text color and background
color in different contexts. Each occurrence should be validated during the audit
rather than blindly mapped to a single token — text usages map to
`--pf-t--global--text--color--regular`, while background usages may need
`--pf-t--global--background--color--primary--default` (dark theme) or another
appropriate background token.

**Hardcoded `rgba()` values:**
- `.diff-line-add` and `.diff-line-remove` — replace with theme-aware custom
  properties (e.g., `--inspectah-diff-add-bg`, `--inspectah-diff-remove-bg`) that
  resolve differently in light vs dark mode. Current hardcoded values (`#c9190b`
  for remove, `#4dabf7` for add) must map to theme-aware tokens.
- Fleet bar segment colors — same treatment.

**Spacer token consistency:**
Standardize all spacing on `--pf-t--global--spacer--*` tokens. Grep for mixed
usage of `--pf-v6-global--spacer--*` and hardcoded `rem`/`px` spacing values.

**Fleet split button:** The current fleet split button is a candidate for PF6
split button component replacement. Not in this pass — noted for future work.

**Inline style extraction:**
Every `style="..."` attribute in template files becomes a named CSS class in
`_css.html.j2`. **Exception:** Computed/dynamic inline styles (e.g.,
`style="width: X%"` for fleet-bar widths in `_macros.html.j2`, and dynamic
sidebar progress bar width styles set by JS) are intentional and excluded from
extraction. Priority order:
1. `_sidebar.html.j2`
2. `_summary.html.j2`
3. `_banner.html.j2`
4. Remaining templates as encountered

**Magic number elimination:**
Replace hardcoded dimension values with CSS custom properties:
- `padding-bottom: 5rem` → `var(--inspectah-content-bottom-padding)`
- `calc(100vh - 200px)` → `var(--inspectah-content-max-height)`
- Other one-off values as discovered during the sweep

### 5. Polish

These are nice-to-have items that elevate quality if time permits, ranked by
priority (highest first):

1. **Masthead branding:** Update text to "inspectah Refine" with the shared
   typography spec (`1.125rem`, `700`, `0.03em`) — visible immediately on load
2. **Warning dismiss transition:** 150ms `ease-out` opacity+height fade instead
   of instant `display:none`
3. **Section headings:** Small, uppercase, letter-spaced treatment to match
   architect's section header style
4. **Fleet bar active state:** Visual indicator (subtle highlight or border) when
   the fleet popover is open for a given bar segment
5. **Package table group headers:** Add left-border accent (4px, status-colored)
   matching the card accent pattern

## Exclusions

- **File browser and editor** (`_file_browser.html.j2`, `_editor.html.j2`,
  `_editor_js.html.j2`): Only fix theme-breaking colors (hardcoded hex values
  that look wrong in dark mode). No structural changes, no layout rework. The
  editor is a future candidate for dedicated polish work.
- **Functional changes** to analysis logic, data processing, or report generation.
  This is purely a UI/presentation overhaul.

## Post-Talk Stretch Goals

Items explicitly deferred past April 8:

- **Kill the Re-render button entirely.** Make all prevalence and toggle changes
  update live via client-side JS, eliminating the need for any server round-trip
  to regenerate the report. This is the natural end-state of the prevalence
  migration.
- **Editor polish pass.** Dedicated spec for CodeMirror integration, drawer
  behavior, and theme consistency in the file editor.
- **Full client-side triage parity.** Reimplement `src/inspectah/renderers/_triage.py` logic in JS so
  prevalence slider updates are exact, not approximations. Eliminates the need
  for Re-render entirely.
- **Fleet split button migration.** Replace custom fleet split button with PF6
  split button component.

## Implementation Order

Suggested sequence to maximize visual impact early and reduce rework:

| Order | Task                          | Est. Hours | Rationale                                      |
|-------|-------------------------------|------------|-------------------------------------------------|
| 1     | CSS token sweep + hex audit   | 1.5        | Foundation — everything else builds on clean tokens |
| 2     | Inline style extraction       | 2.0        | Unblocks template work with clean separation    |
| 3     | Summary tab redesign          | 4.0        | Biggest visual impact, includes prevalence card |
| 4     | Prevalence migration          | 1.0        | Depends on summary tab being done               |
| 5     | Component upgrades            | 1.5        | Independent swaps, can be done in any order      |
| 6     | Polish items                  | 1.0        | Last — only if time remains before April 5      |

## Files Affected

**Primary targets (heavy changes):**
- `src/inspectah/templates/report/_css.html.j2` — token sweep, hex audit, new classes
- `src/inspectah/templates/report/_summary.html.j2` — complete redesign
- `src/inspectah/templates/report/_js.html.j2` — prevalence slider logic, live update JS
- `src/inspectah/templates/report/_toolbar.html.j2` — remove prevalence slider
- `src/inspectah/templates/report/_sidebar.html.j2` — badge upgrades, inline style extraction

**Secondary targets (moderate changes):**
- `src/inspectah/templates/report/_banner.html.j2` — inline style extraction
- `src/inspectah/templates/report/_warnings.html.j2` — dismiss animation
- `src/inspectah/templates/report/_macros.html.j2` — badge/spinner macro updates
- `src/inspectah/templates/report.html.j2` — masthead branding

**Light touch (token fixes only):**
- `src/inspectah/templates/report/_packages.html.j2`
- `src/inspectah/templates/report/_services.html.j2`
- `src/inspectah/templates/report/_config.html.j2`
- `src/inspectah/templates/report/_containers.html.j2`
- `src/inspectah/templates/report/_module_streams.html.j2`
- `src/inspectah/templates/report/_version_locks.html.j2`
- `src/inspectah/templates/report/_audit_report.html.j2`
- `src/inspectah/templates/report/_compare_modal.html.j2`
- `src/inspectah/templates/report/_new_file_modal.html.j2`
- `src/inspectah/templates/report/_file_browser.html.j2` — theme-breaking colors only
- `src/inspectah/templates/report/_editor.html.j2` — theme-breaking colors only

## Testing

**Visual verification (every change):**
- Generate a refine report in fleet mode (multi-host) and single-host mode
- Toggle dark/light theme — verify no hardcoded colors bleed through
- Check all section tabs render correctly after token migration
- Verify prevalence slider updates cards in real time without page reload

**Specific checks:**
- Summary tab: 4-card grid in fleet mode, 3-card grid in single-host mode
- Prevalence badge in section tab headers shows correct threshold value
- Fleet popover opens/closes cleanly with PF6 component
- Spinner and toast replacements render identically to PF6 docs
- Warning dismiss has visible fade-out (not instant disappear)
- No `style="..."` attributes remain in priority template files (except
  computed/dynamic values like fleet-bar widths in `_macros.html.j2`)

**Regression:**
- Run existing test suite (`pytest`) to catch any template rendering errors
- Verify Download button still produces correct output
- Verify Re-render button still triggers full regeneration
- Check file editor still functions (should be untouched structurally)
