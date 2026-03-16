# Fleet UI Polish — Cursor Prompts

## Prompt 1: Table Column Width Pass (Chunk 1, Tasks 1–6)

**Cursor: new chat**
**Model: sonnet**

~~~
**Context: Read AGENTS.md First**

Before proceeding, read `/Users/mrussell/Work/bootc-migration/AGENTS.md` to understand:
- Communication style and code quality expectations
- Commit message format and attribution requirements
- This workspace contains TWO separate git repositories: `yoinkc/` and `driftify/`
- The workspace root is NOT a repository — do not run git init or create repos here
- You are acting as Engineering: implement changes but do not commit until review is requested

Project you're working on: yoinkc

---

## Task

Add `pf-m-fit-content` to `<th>` elements in all report tables for columns that contain short/fixed-width content (toggles, dropdowns, numeric values, labels). This is a blanket pass — every table gets reviewed.

## Context

PF6 tables without `pf-m-fit-content` let all columns share space equally. Columns with short content (checkboxes, dropdowns, status labels) stretch unnecessarily, pushing content columns off-screen. Most visible in the Groups table where the Strategy dropdown consumes the entire row.

Read the spec for the full column inventory: `docs/specs/proposed/2026-03-16-fleet-ui-polish-design.md` (Part B) and the plan: `docs/specs/proposed/2026-03-16-fleet-ui-polish-plan.md` (Chunk 1, Tasks 1–6).

## Changes

For each template listed below, add `pf-m-fit-content` to the specified `<th>` tags. Do NOT add it to name/path/description/content/members columns — only to short/fixed-width columns.

**`_users_groups.html.j2`:**
- Users table: include toggle (`pf-v6-c-table__check`), UID/GID, Type, Strategy
- Groups table: include toggle, GID, Strategy

**`_config.html.j2`:**
- include toggle, Kind, Category, rpm-Va flags

**`_packages.html.j2`:**
- Dep tree: include toggle, Version, Dep count, Arch
- Repos: include toggle, Source

**`_non_rpm.html.j2`:**
- ELF binaries: include toggle, Language, Linking
- pip/npm: include toggle, Version

**`_network.html.j2`:**
- iptables: include toggle, IPv, Table, Chain
- Firewall zones: include toggle

**`_containers.html.j2`:**
- include toggle

**`_services.html.j2`:**
- State changes: include toggle, Current, Default, Action
- Drop-ins: include toggle

**`_scheduled_jobs.html.j2`:**
- Cron-converted: include toggle
- Cron jobs: include toggle, Source, Action
- At jobs: User

**`_kernel_boot.html.j2`:**
- Sysctl overrides: include toggle, Runtime, Default

**`_selinux.html.j2`:**
- Booleans: Current, Default

**`_storage.html.j2`:**
- Type

## Pattern

For `<th>` tags without existing classes:
```html
<!-- Before -->
<th scope="col">Strategy</th>
<!-- After -->
<th class="pf-m-fit-content" scope="col">Strategy</th>
```

For `<th>` tags with existing `pf-v6-c-table__check`:
```html
<!-- Before -->
<th class="pf-v6-c-table__check" scope="col"></th>
<!-- After -->
<th class="pf-v6-c-table__check pf-m-fit-content" scope="col"></th>
```

## Commit Structure

Two commits:
1. `fix: add pf-m-fit-content to users/groups and config table columns` — the most visible fixes
2. `fix: add pf-m-fit-content to remaining report table columns` — everything else

## Acceptance Criteria

- Every `<th>` for short/fixed-width columns has `pf-m-fit-content`
- No `<th>` for name/path/description/content/members columns has it
- All existing tests pass: `python -m pytest tests/ -x -q`

Do not commit yet. When asked, review your own changes in this chat before committing. Do not use subagents for review.
~~~

---

## Prompt 2: Compare Button Guard (Chunk 2, Task 7)

**Cursor: continue current chat**
**Model: sonnet**

~~~
## Task

Disable fleet compare buttons when no variant in the group is selected. Currently they silently fail (JS returns early with no feedback).

Read the full plan: `docs/specs/proposed/2026-03-16-fleet-ui-polish-plan.md` (Chunk 2, Task 7).

## Changes

### 1. Add `updateCompareButtons(group)` to `_js.html.j2`

Add near the existing compare button click handler (~line 860):

```javascript
function updateCompareButtons(group) {
  var rows = document.querySelectorAll('[data-variant-group="' + group + '"]');
  var hasSelected = false;
  rows.forEach(function(row) {
    var section = row.getAttribute('data-snap-section');
    var list = row.getAttribute('data-snap-list');
    var idx = parseInt(row.getAttribute('data-snap-index'), 10);
    var arr = resolveSnapshotRef(section, list);
    if (arr && arr[idx] && arr[idx].include) hasSelected = true;
  });
  rows.forEach(function(row) {
    var btn = row.querySelector('.variant-compare-btn');
    if (!btn) return;
    if (hasSelected) {
      btn.removeAttribute('disabled');
      btn.classList.remove('pf-m-disabled');
    } else {
      btn.setAttribute('disabled', '');
      btn.classList.add('pf-m-disabled');
    }
  });
}
```

### 2. Call from variant toggle handler

In the existing `.include-toggle` change handler (around line 234–256), after the loop that updates sibling variants' `include` state, add `updateCompareButtons(group)`.

### 3. Call from reset handler

After reset restores original snapshot state, iterate all unique `data-variant-group` values and call `updateCompareButtons(group)` for each.

### 4. Render disabled state initially in all three variant templates

All three templates that render compare buttons need the same change: `_config.html.j2`, `_services.html.j2`, `_containers.html.j2`.

Before the variant `{% for %}` loop, compute:
```jinja2
{%- set group_has_selected = variants | selectattr('item.include', 'equalto', true) | list | length > 0 %}
```

Then update the compare button markup:
```html
<button class="pf-v6-c-button pf-m-link pf-m-small variant-compare-btn{% if not group_has_selected %} pf-m-disabled{% endif %}"{% if not group_has_selected %} disabled{% endif %}>Compare</button>
```

## Acceptance Criteria

- Compare buttons are visually greyed out and unclickable when no variant is selected
- Selecting a variant enables compare buttons on sibling rows
- Reset re-disables compare buttons if no variant remains selected
- Existing `if (!selectedItem) return;` in the click handler stays as a safety net
- All existing tests pass: `python -m pytest tests/ -x -q`

Do not commit yet. When asked, review your own changes in this chat before committing. Do not use subagents for review.
~~~

---

## Prompt 3: Editor Variant Accordion (Chunk 3, Task 8)

**Cursor: new chat**
**Model: opus-high**

~~~
**Context: Read AGENTS.md First**

Before proceeding, read `/Users/mrussell/Work/bootc-migration/AGENTS.md` to understand:
- Communication style and code quality expectations
- Commit message format and attribution requirements
- This workspace contains TWO separate git repositories: `yoinkc/` and `driftify/`
- The workspace root is NOT a repository — do not run git init or create repos here
- You are acting as Engineering: implement changes but do not commit until review is requested

Project you're working on: yoinkc

---

## Task

Replace the PF6 tree-view editor file list with a custom variant accordion. This is the biggest piece of the fleet UI polish spec.

Read the full spec and plan:
- Spec: `docs/specs/proposed/2026-03-16-fleet-ui-polish-design.md` (Part A)
- Plan: `docs/specs/proposed/2026-03-16-fleet-ui-polish-plan.md` (Chunk 3, Task 8)

## Summary

Rewrite `buildTree()` in `_editor_js.html.j2` (~lines 23–220) to render:

**Multi-variant files** as accordion groups:
- Header: chevron (▼/►), file path (dimmed dir / bold filename), "N variants" badge
- Variant rows (indented): prevalence bar (blue majority / gold minority), host chips (or "N hosts" fallback), selected badge or hover-to-switch button + compare link button
- Row click = view variant (editable if selected, read-only if not)
- "Use this variant" button click = switch selection (`stopPropagation`, mutate snapshot, `setDirty()`, `updateCompareButtons()`, re-render tree)
- "Compare" button click = navigate to config tab and trigger compare modal for this variant

**Single-variant files** as flat entries (no accordion).

## Key Implementation Details

- Sort variants by `fleet.count` descending
- Prevalence: `Math.round((item.fleet.count / item.fleet.total) * 100)`, majority when > 50%
- Host chip overflow: measure total chip width vs available space (~row width minus 120px). If overflow, show "N hosts" text. Individual chips: `max-width: 80px`, `text-overflow: ellipsis`
- Non-fleet fallback: guard all `item.fleet` access with `(item.fleet && item.fleet.count) || 0`. Single-host reports render flat list only.
- All groups start expanded
- Cross-tab sync: lazy model — pill click mutates `snapshot[section][list][index].include`, config tab reads same object on switch

## Files to Modify

- `src/yoinkc/templates/report/_editor_js.html.j2` — rewrite `buildTree()`, add `toggleAccordion()`, add `switchVariantFromEditor()`, add `compareFromEditor()`
- `src/yoinkc/templates/report/_editor.html.j2` — add CSS for accordion in `<style>` block

## CSS

Add to `_editor.html.j2` `<style>` block. Full CSS is in the plan (Task 8, Step 5). Key classes: `.editor-variant-group`, `.editor-variant-header`, `.editor-variant-row`, `.prevalence-bar`, `.host-chip`, `.selected-badge`, `.switch-btn`, `.editor-compare-btn`, `.editor-single-file`.

**Design distinction:** "selected" is a **badge** (status indicator). "Use this variant" is a **button** (action, appears on hover). "Compare" is a **link button** (always visible on non-selected rows, disabled when no variant selected).

## Also Update

`doSelectFile()` — change the `.pf-m-current` highlight logic to work with new `.editor-variant-row` and `.editor-single-file` selectors instead of tree-view nodes.

## Acceptance Criteria

- Multi-variant files render as accordion groups with all visual elements
- Single-variant files render flat
- Row click loads variant in editor (editable if selected, read-only if not)
- "Use this variant" button appears on hover, switches selection on click
- "Compare" link button on non-selected rows navigates to config tab compare modal
- "selected" badge (not button) on selected variant row
- Host chips show when they fit, fall back to "N hosts" count
- Non-fleet (single-host) report renders flat file list with no variant UI
- All existing tests pass: `python -m pytest tests/ -x -q`

Do not commit yet. When asked, review your own changes in this chat before committing. Do not use subagents for review.
~~~
