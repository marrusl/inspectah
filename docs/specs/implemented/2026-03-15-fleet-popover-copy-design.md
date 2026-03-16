# Fleet Popover: Copy Host List

**Date:** 2026-03-15
**Status:** Implemented

## Problem

The fleet prevalence popover shows a list of hosts that have a particular item, but users can't copy the list. Sysadmins need to paste host lists into Ansible commands (comma-separated), shell loops (space-separated), or inventory files (one per line). Currently they must manually select and copy text from the popover.

## Decision

Add a split "Copy" button to the fleet popover with a format dropdown. Three copy formats: one per line (default), comma-separated, space-separated. Last-used format remembered within the session.

## Popover Layout

Current popover is a plain `<ul><li>` host list in a dynamically created `<div>`. New layout adds a header row:

```
┌─────────────────────────┐
│ 3 of 5 hosts    [Copy▾] │
│─────────────────────────│
│ host-01                 │
│ host-02                 │
│ host-03                 │
└─────────────────────────┘
```

- **Header**: "N of M hosts" count (from existing `data-count`/`data-total` attributes) + split Copy button
- **Host list**: unchanged `<ul><li>` structure
- **Split button**: main "Copy" area + dropdown arrow toggle

## Split Button Behavior

**Main "Copy" button**: copies all hosts in the active format. Button text changes to "Copied!" for 1.5 seconds (matches existing Containerfile copy pattern).

**Dropdown arrow**: toggles a format menu below the button:

```
┌────────────────────┐
│ ✓ One per line     │
│   Comma-separated  │
│   Space-separated  │
└────────────────────┘
```

- Blue checkmark (PF6 brand color `--pf-t--global--icon--color--brand--default`) on the active format
- Non-active items indented to align with checkmark width
- Clicking a format: copies immediately in that format, updates the active format, closes the dropdown, shows "Copied!" feedback
- Dropdown closes on click-outside (scoped: clicking inside the popover but outside the dropdown closes only the dropdown, not the popover)

## Copy Formats

| Format | Variable | Output |
|--------|----------|--------|
| One per line | `newline` | `host-01\nhost-02\nhost-03` |
| Comma-separated | `comma` | `host-01,host-02,host-03` |
| Space-separated | `space` | `host-01 host-02 host-03` |

No trailing delimiter. No quoting. Hostnames are passed through as-is from `FleetPrevalence.hosts` (short or fully-qualified depending on what `YOINKC_HOSTNAME` provided).

## Format Memory

Active format stored in a JS module-level variable:

```javascript
var copyFormat = 'newline';
```

Persists across popover interactions within the same page session. Resets on page reload. Not stored in `localStorage`.

## Clipboard

Reuse the existing clipboard pattern from the Containerfile copy button (`_js.html.j2` lines 551-586):
- Primary: `navigator.clipboard.writeText(text)`
- Fallback: `document.execCommand('copy')` for older browsers
- Feedback: button text swap "Copy" → "Copied!" for 1.5s via `setTimeout`

## Affected Files

| File | Change |
|------|--------|
| `src/yoinkc/templates/report/_js.html.j2` | Modify popover creation (~lines 611-651): add header, split button, dropdown, format state, clipboard call |
| `src/yoinkc/templates/report/_css.html.j2` | Add styles: split button, dropdown menu, active item checkmark, "Copied!" state |

No template changes. No schema changes. No Python changes. Purely JS + CSS in the report.

## Event Propagation

The popover is a child of `.fleet-bar`. The bar's click handler destroys and recreates the popover on every click. Without care, clicking the Copy button or dropdown items will bubble up to the bar handler, destroying the popover mid-action.

**All interactive elements inside the popover must call `e.stopPropagation()`**: the Copy button, the dropdown arrow, and each dropdown menu item. This prevents the bar click handler from tearing down the popover. The existing fleet label click handler already uses this pattern.

**Dropdown vs. popover close scoping**: the document-level click-outside handler closes the popover when `!e.target.closest('.fleet-bar')`. Dropdown close needs its own check: if click is inside the popover but outside the dropdown menu, close only the dropdown. If click is outside the popover entirely, the existing handler closes the popover (which removes the dropdown with it).

## What Does NOT Change

- **Popover lifecycle**: still dynamically created, singleton (`activePopover`), click-outside-to-close
- **Fleet bar click behavior**: click bar → popover appears (same as today)
- **Host data source**: `data-hosts` attribute on `.fleet-bar` elements (comma-joined string, parsed in JS)
- **Popover positioning**: still appended as child of the bar with `position: relative`

## Alternatives Considered

1. **PF6 Dropdown Component (`pf-v6-c-dropdown`)** — the "proper" PF6 split button + menu. Rejected: the popover is dynamically created and destroyed in JS. PF6 components expect static DOM with toggle visibility. Would require refactoring the entire popover lifecycle.

2. **PF6 Menu Component (`pf-v6-c-menu`)** — use PF6 menu inside the custom popover. Rejected: same DOM lifecycle conflict as above, and still needs a custom split button trigger.

3. **Selectable text block (`user-select: all`)** — no button, user clicks to select all then Cmd+C. Rejected: requires two steps, no format choice, less discoverable.

4. **Single "Copy" button (no format choice)** — always newline. Rejected: doesn't serve Ansible (comma) or shell (space) workflows without manual transformation.

## Future Consideration

If the popover is ever refactored to use **static DOM** (Jinja2-rendered, one per fleet bar, hidden by default and toggled on click), the split button and dropdown could be replaced with proper PF6 `pf-v6-c-dropdown` components. This would give full PF6 semantics, ARIA attributes, and keyboard navigation for free. The tradeoff is page weight (one hidden popover per fleet bar). Worth evaluating during a "PF6 purity pass" on the report.
