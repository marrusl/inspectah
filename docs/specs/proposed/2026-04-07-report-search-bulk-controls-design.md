# inspectah Report: Search & Bulk Controls (v3)

**Date:** 2026-04-07 (v3 — targeted fixes from round-2 review)
**Scope:** Per-card search/filter + bulk Include All / Exclude All controls
**Target:** inspectah HTML report (`src/inspectah/templates/report/`)

---

## Summary

Two complementary features for the inspectah interactive HTML report:

1. **Search/filter** — a text input on each filterable card that filters items by a dedicated search attribute, hiding non-matching rows while preserving repo grouping in the packages card.
2. **Bulk controls** — Include All / Exclude All buttons that act on visible items when a filter is active, or all items when unfiltered.

Both features are pure client-side JavaScript, work in standalone mode (no refine server), and integrate with the existing dirty-state tracking. Bulk operations in the packages card require a new batched mutation helper as a prerequisite refactoring step.

---

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Toolbar scope | Per filterable card, not per section tab | Section tabs contain mixed editable and read-only cards; toolbar must attach to a single item container with a defined scope |
| Bulk scope when filtered | Adaptive — acts on visible items only, label shows count | Enables the filter→bulk-select workflow |
| Search target | `data-search-text` attribute, not textContent | Precise, testable, excludes badges/labels/controls |
| Search + repo grouping | Preserve groups, filter within, explicit state machine | Maintains repo context; state machine resolves conflicts between search, manual collapse, and excluded stubs |
| Excluded group display | Collapse to one-line stub with expand affordance | Preserves awareness without wasting space |
| Match highlighting | Deferred to v2 | Filter behavior is the high-value feature; highlighting introduces innerHTML mutation risk for marginal gain |
| Warning visibility | Persistent indicator when filter hides warning-bearing items | Filtered/collapsed content must not read as "safe" |
| Implementation approach | Pure client-side JavaScript | Works in standalone mode, no new API surface |

---

## Filterable Card Inventory

The toolbar attaches to specific cards within section tabs, not to the `section()` macro. Each entry below is one toolbar instance.

| Section Tab | Filterable Card | Item Container | `data-search-text` Source | Notes |
|-------------|----------------|----------------|--------------------------|-------|
| RPM Packages | Packages (leaf + auto-dep) | Table rows (repo-grouped) | Package name | Repo grouping + dep cascade |
| Services | Enabled/disabled units | Table rows | Unit name | |
| Services | Drop-in overrides | Table rows (fleet variants) | Drop-in path | Fleet variant parent/child |
| Config Files | Config files | Table rows (fleet variants) | File path | Fleet variant parent/child |
| Network | Firewall direct rules | Table rows | Rule args | |
| Containers | Quadlet units | Table rows (fleet variants) | Unit path | Fleet variant parent/child |
| Containers | Compose files | Div blocks | File path | |
| Non-RPM Software | Compiled binaries | Table rows | Binary path | |
| Non-RPM Software | System pip packages | Table rows | Package name | |
| Kernel/Boot | Sysctl overrides | Table rows | Sysctl key | Only toggleable card in kernel_boot |
| Users/Groups | Users | Table rows (strategy select) | Username | |
| Users/Groups | Groups | Table rows (strategy select) | Group name | |

**Cards that do NOT get toolbars (read-only or no include toggles):** Summary, warnings (has its own search), output files, audit, version changes, dep tree, running containers, network connections, firewall zones, IP routes/rules, resolv provenance, storage/fstab, kernel command line, module/dracut config (combined read-only card), tuned profiles, locale/timezone, alternatives, all SELinux cards (overview, custom modules, booleans, port labels — none have include toggles), python venvs, git repos, env files.

**Template verification notes:**
- `_kernel_boot.html.j2`: Only sysctl overrides (`card-kb-sysctl`) has include toggles. Modules load.d, modprobe.d, and dracut.conf are rendered together in one read-only card (`card-kb-modconf`) with no toggles.
- `_selinux.html.j2`: No cards have include toggles. Custom modules renders as a plain `<ul>`. Booleans and port labels are read-only tables. Audit rules, fcontext rules, and PAM configs are counted in the overview but not rendered as card content.

---

## Component: Filterable Card Toolbar

### Structure

A new Jinja2 macro `card_toolbar(card_id, item_count)` rendered inside each filterable card, between the card header and item list. This is NOT inside the `section()` macro — it is placed explicitly in each card that opts in.

Each `card_id` uniquely identifies the item container the toolbar controls. The `aria-controls` attribute on the search input points at the item container element by this ID.

### Layout

```
[⌕ Search input...] [N of M included] [Include All N] [Exclude All N]
 ← left-aligned →                      ← right-aligned, flex-shrink: 0 →
```

### States

**Unfiltered:**
- Search input: empty, placeholder "Search [card label]..."
- Included count: "37 of 47 included"
- Buttons: "Include All 47" / "Exclude All 47"

**Filtered:**
- Search input: has value, border highlights (accent color)
- Filter count appears next to input: "8 of 47 shown"
- Included count: always visible-scope — "6 of 8 visible included"
- Buttons: "Include 8 Matching" / "Exclude 8 Matching" (border highlights to reinforce scoped behavior)

**Included count invariant:** The toolbar's included count always reflects the visible scope. When no filter is active, visible = all, so the count is "N of M included" (full card). When a filter is active, the count is "N of M visible included" (filtered items only). Hidden cascade side effects from bulk operations are communicated via the summary notification, not the included count. This is one invariant used everywhere.

**Warning indicator:** When filtered results hide any rows that carry warnings or redaction indicators, the toolbar shows a persistent badge: "N hidden items have warnings or redactions". This badge is visible regardless of filter state and cannot be dismissed by filtering.

### Button States

| State | Include button | Exclude button |
|-------|---------------|----------------|
| All items included | Disabled (dimmed) | Active |
| All items excluded | Active | Disabled (dimmed) |
| Mixed | Active | Active |
| Filtered — all visible included | Disabled | Active |
| Filtered — all visible excluded | Active | Disabled |

### Keyboard

- `Escape` in search input clears the filter and restores all rows
- Tab order: search → include → exclude → first item in list

### Accessibility

- Search input: `role="searchbox"`, `aria-label="Search [card label]"`, `aria-controls="[item-container-id]"`
- Bulk buttons: `aria-label` matches visual label (dynamic)
- Filter result count: `aria-live="polite"` region — screen reader announces filter results without interrupting
- Warning indicator: `role="status"` — screen reader announces when warnings or redaction indicators are hidden
- Disabled buttons: `aria-disabled="true"` with tooltip explaining why
- Tab order follows natural reading order

---

## Search & Filter Behavior

### Match Logic

- **Target:** The `data-search-text` attribute on each filterable row/div. Set by the Jinja2 renderer to the item's primary identifier (package name, file path, unit name, etc.). Search does NOT match against textContent, badges, control labels, or any other DOM content.
- **Query normalization:** Leading/trailing whitespace trimmed. Internal whitespace preserved. Case-insensitive substring match. A whitespace-only query is treated as empty (no filter).
- **Event:** Fires on `input` event. No debounce.
- **Mechanism:** Non-matching rows get `display: none`. DOM stays intact — clearing search restores all rows instantly.
- **Match highlighting:** Deferred to v2. The filter itself is the high-value feature. Highlighting introduces innerHTML mutation complexity and trust boundary concerns (see Slate review) for marginal UX gain.
- **Zero-match state:** When no items match the query, all item rows are hidden. The toolbar shows "0 of N shown". Both bulk buttons are disabled. For repo-grouped cards, all group headers show "no matches" stub.

### Repo Group Interaction (Packages Card)

- Repo group headers update to show filter impact: "AppStream (3 of 47 matching)"
- Groups with zero matches display as a single italic line: "EPEL — no matches"
- Original count always preserved in the label for context
- Requires `data-group="repo-name"` attribute on group headers and their child rows
- See **Repo Group State Machine** section below for full state precedence

### Fleet Variant Rows

Cards with fleet variant parent/child rows (services drop-ins, quadlet units, config files): search matches against the parent row's `data-search-text`. When a parent matches, all its variant children are shown. When a parent doesn't match, parent and all children are hidden. Variant children are not independently searchable.

### Non-Grouped Cards

Flat list filtering — same `data-search-text` match, same `display: none`. No group headers to manage.

---

## Bulk Controls Behavior

### Adaptive Scope

- **No filter active:** Acts on every item in the card.
- **Filter active:** Acts on visible (matching) items only.
- Button labels dynamically reflect scope and count.

### Prerequisite: Batched Mutation Helper

The current codebase does not expose a clean per-item toggle primitive. Package toggle behavior is spread across the generic `.include-toggle` handler, `applyRepoCascade()`, `recomputeAutoDeps()`, `updatePkgBanner()`, dirty-state, triage recount, and containerfile preview refresh. Bulk operations cannot simply loop over these — that would spam repeated recalculations.

**Implementation must first extract:**

1. **`batchToggleItems(cardId, items, include)`** — sets all target checkboxes to the desired state without triggering per-item side effects.
2. **One cascade recompute pass** after the batch (packages only): `recomputeAutoDeps()` runs once, not per-item.
3. **One UI refresh pass** after cascade: dirty-state update, triage recount, containerfile preview, toolbar sync — each runs once.
4. **One summary notification** after the operation: "Excluded 8 packages (+3 dependencies)" or "Included 12 packages" — a single message, not per-item toasts.

This batched path is a prerequisite refactoring step. It applies to both filtered and unfiltered bulk operations.

### Postconditions for Filtered Bulk Exclude (Packages)

When the user clicks "Exclude 8 Matching" while a search filter is active:

1. The 8 visible matching packages are directly excluded.
2. Dependency cascade runs once. It may exclude additional hidden (non-matching) packages if they become orphaned.
3. The toolbar's included count recalculates for the visible scope only (per the included count invariant). If all 8 visible items are now excluded, the count shows "0 of 8 visible included." Hidden cascade effects are not reflected in this count.
4. The summary notification explicitly reports all changes: "Excluded 8 packages (+N dependencies)". This is the only place hidden cascade changes are surfaced. They are never silent.
5. Button labels update based on the visible scope: "Include 8 Matching" reflects the 8 visible items regardless of hidden cascade state.

### Bulk Include (Packages)

- Does NOT auto-include dependencies. Same as individual include behavior.
- User explicitly chooses what to add back.
- No cascade side effects on include.

### Fleet Variant Cards (Config, Services Drop-ins, Quadlet Units)

**Current DOM/state model:** Both parent and child variant rows have `.include-toggle` checkboxes. The parent row is an alias for the first child — they share the same snapshot index (comment in `_js.html.j2`: "Parent fleet rows do not carry `data-variant-group` because they point at the same snapshot item as the first child row"). Child rows carry `data-variant-group` and the radio constraint fires on child toggles: checking one child unchecks its siblings. The parent's checked state always mirrors child[0].

**Bulk semantics — operating on the variant group level:**

Bulk operates at the group level. A "group" is one variant group (e.g., all variants of `/etc/httpd/conf/httpd.conf`). The user-facing unit is the group, not individual variants.

- **Bulk Exclude:** For each targeted group, uncheck all child variant toggles. The parent (alias for child[0]) unchecks automatically. The group is fully excluded. This is straightforward — no constraint conflicts.
- **Bulk Include:** For each targeted group that is currently fully excluded (no variant checked), check the primary child variant (child[0] / highest-prevalence). The radio constraint is inherently satisfied because only one child is checked. Groups that already have a selected variant are left unchanged — bulk include does not override the user's variant choice.
- **Radio constraint:** Bulk never checks multiple children in the same group. Bulk exclude unchecks all children. Bulk include checks exactly one child (the primary) per fully-excluded group.
- **Parent row synchronization:** The parent row's checkbox UI does not auto-sync when child toggles change via the generic `.include-toggle` handler. After bulk variant mutations, implementation must explicitly sync the parent row's checkbox and `.excluded` class to match child[0]'s state — either by calling the existing `applyVariantSelection()` helper or by adding a parent-sync step to the batched mutation path.
- **Item counts reflect groups, not individual variants.** "12 of 15 included" means 12 groups have at least one variant checked. A group with 3 variants where 1 is selected counts as 1 included item.
- **When filtered:** Same logic scoped to visible groups. Search matches against the parent row's `data-search-text` (the file path / unit path). Matching a parent shows all its variant children.

### Non-Package, Non-Variant Cards

No cascade logic, no variant groups. Bulk toggle iterates checkboxes directly via the batched helper. Same adaptive scope rules and button state logic. Summary notification: "Included/Excluded N items."

### Dirty State Integration

- The batched mutation marks the snapshot dirty once — same as if individual toggles had fired.
- Top toolbar's "Rebuild & Download" activates immediately.
- Card toolbar's included count updates after the batch completes.
- "Discard all" in the top toolbar reverts everything (including bulk changes) to baseline, clears all search inputs, and resets all card toolbars.

---

## Repo Group State Machine (Packages Card Only)

The packages card has three interacting collapse/expand mechanisms that require explicit precedence rules.

### Inputs

| Input | Source |
|-------|--------|
| `search_active` | Non-empty query in the card toolbar search input |
| `has_matches` | At least one item in this group matches the search query |
| `all_excluded` | Every item in this group has `include = false` |
| `manually_collapsed` | User clicked the existing `.repo-collapse-btn` |
| `user_expanded_stub` | User clicked the excluded-group stub's expand button |

### Precedence Table

Priority from highest to lowest. First matching row wins.

| # | search_active | has_matches | all_excluded | manually_collapsed | user_expanded_stub | → Rendered State |
|---|---|---|---|---|---|---|
| 1 | yes | yes | any | any | any | **Expanded, showing matches only.** Search overrides all other states. Non-matching rows hidden. Group header shows "N of M matching." |
| 2 | yes | no | any | any | any | **"No matches" stub.** Single italic line: "RepoName — no matches." |
| 3 | no | — | yes | any | no | **Excluded stub.** Single line: "▸ RepoName (N packages — all excluded) [Expand]". Warning badge if any excluded items have warnings. |
| 4 | no | — | yes | any | yes | **Expanded, all dimmed.** User manually expanded the stub. All items visible with excluded styling. |
| 5 | no | — | no | yes | — | **Manually collapsed.** Existing behavior, unchanged. Group header visible, items hidden. |
| 6 | no | — | no | no | — | **Normal.** Fully expanded, items visible. |

### Transitions

- **Entering search:** `user_expanded_stub` resets to `false`. Search takes full control.
- **Clearing search:** State reverts to whatever row 3-6 applies based on current `all_excluded` and `manually_collapsed` values. `user_expanded_stub` resets to `false`.
- **Bulk exclude makes group all-excluded:** Transitions to row 3 (stub). `user_expanded_stub` resets to `false`.
- **Including any item in a stubbed/expanded-stub group:** If group is no longer all-excluded, transitions to row 5 or 6 based on `manually_collapsed`. `user_expanded_stub` resets to `false`.
- **Clicking stub expand:** Sets `user_expanded_stub = true`, transitions to row 4.

### Warning Badge on Stubs

When a repo group is in the excluded stub state (row 3), if any item in the group carries a warning or redaction indicator, the stub shows a warning badge: "▸ EPEL (12 packages — all excluded) ⚠ 2 warnings [Expand]". This ensures warning signals are never hidden behind a collapsed stub.

---

## Implementation Architecture

### Template Changes

| File | Change |
|------|--------|
| `_macros.html.j2` | New `card_toolbar(card_id, item_count, card_label)` macro. Renders search input + bulk buttons + included count + warning indicator. |
| `_packages.html.j2` | Add `data-group="repo-name"` on group headers and child rows. Add `data-search-text` on package rows. Insert `card_toolbar()` call. |
| `_services.html.j2` | Add `data-search-text` on unit rows and drop-in rows. Insert `card_toolbar()` calls for each toggleable card. |
| `_config.html.j2` | Add `data-search-text` on config rows. Insert `card_toolbar()` call. |
| `_network.html.j2` | Add `data-search-text` on firewall direct rule rows. Insert `card_toolbar()` call for that card only. |
| `_containers.html.j2` | Add `data-search-text` on quadlet rows and compose divs. Insert `card_toolbar()` calls for each toggleable card. |
| `_non_rpm.html.j2` | Add `data-search-text` on binary rows and pip rows. Insert `card_toolbar()` calls for each toggleable card. |
| `_kernel_boot.html.j2` | Add `data-search-text` on sysctl override rows. Insert `card_toolbar()` call for sysctl card only. No changes to other cards (read-only). |
| `_selinux.html.j2` | No changes. No cards have include toggles in current templates. |
| `_users_groups.html.j2` | Add `data-search-text` on user/group rows. Insert `card_toolbar()` calls for each table. |

### JavaScript Additions (`_js.html.j2`)

| Function | Purpose |
|----------|---------|
| `initCardSearch(cardId)` | Binds input listener on the card's search input. Matches `data-search-text` against trimmed, lowercased query. Hides non-matching rows. Updates group headers (packages). Updates bulk button labels, included count, warning indicator. |
| `batchToggleItems(cardId, items, include)` | Prerequisite refactoring. Sets target checkboxes without per-item side effects. Runs one cascade pass (packages). Runs one UI refresh. Shows one summary notification. |
| `updateGroupState(cardId)` | Evaluates the state machine for each repo group. Applies the correct rendered state based on precedence table. Packages card only. |
| `syncToolbar(cardId)` | Recalculates included count (visible scope if filtered, all if not). Updates button labels, enables/disables. Updates warning indicator. Called after search, bulk toggle, individual toggle, discard-all. |

### Integration Points

| Existing Code | Addition |
|---------------|----------|
| `.include-toggle` change handler | Call `syncToolbar(cardId)` and `updateGroupState(cardId)` after processing |
| `#btn-reset` (Discard all) handler | Clear all search inputs, restore all rows to visible, call `syncToolbar` and `updateGroupState` for all cards, recalculate from baseline |
| Re-render response handler | See **Re-render Contract** below |

### Re-render Contract

The current re-render flow does NOT replace the report DOM. It POSTs the snapshot to `/api/re-render`, receives updated snapshot JSON + Containerfile text, updates in-memory `snapshot` and `originalSnapshot` variables, refreshes the Containerfile preview, rebuilds the dirty-state baseline, and triggers tarball download. The DOM (checkboxes, rows, cards) remains unchanged.

This means for search/filter/bulk:
- **Search state persists across re-render.** The DOM didn't change, so any active filter remains visually applied. This is correct — the user's view intent should survive a re-render.
- **Toolbar counts recalculate** after re-render by calling `syncToolbar` for all cards. Since the DOM reflects the post-re-render state (checkboxes match the new snapshot), counts will be accurate.
- **Group state recalculates** after re-render by calling `updateGroupState`. The excluded/collapsed state derives from current checkbox state.
- **Dirty state resets** (existing behavior via `buildBaseline()` + `setDirty(false)`). Bulk changes that were dirty before re-render are now part of the new baseline.
- **"Discard all" after re-render** reverts to the post-re-render baseline, not to the pre-re-render state. This is existing behavior.

### No Changes Needed

- Refine server — no new endpoints
- Snapshot schema — no changes
- Standalone mode — everything works as-is

### Security Constraints

- Search query is treated as literal text. Never used to build selectors, HTML strings, or regex without escaping.
- No `innerHTML` mutation for search/filter behavior. Visibility is controlled exclusively via `display` style and class toggling.
- `data-search-text` is set server-side by the Jinja2 renderer. Client-side code reads it but never writes it.
- This feature does not cache or reinsert HTML fragments during search, filter, bulk, reset, or re-render flows. This is categorical — no exceptions, no conditional paths.

---

## Verification Matrix

Observable acceptance criteria that implementation must satisfy.

### Search

| # | Scenario | Expected |
|---|----------|----------|
| S1 | Empty query | All items visible. No filter indicators. Buttons show full count. |
| S2 | Whitespace-only query | Treated as empty. Same as S1. |
| S3 | Case-insensitive match | Query "HTTP" matches item with `data-search-text="httpd"`. |
| S4 | Zero matches | All items hidden. "0 of N shown". Both bulk buttons disabled. Repo groups show "no matches" stubs. |
| S5 | Partial match | Query "http" matches "httpd", "libhttpparser", "mod_http2". |
| S6 | Escape key | Clears search, restores all items, resets toolbar to unfiltered state. |
| S7 | Search in repo-grouped card | Groups with matches show updated count "N of M matching". Groups with no matches show "no matches" stub. |

### Bulk Controls

| # | Scenario | Expected |
|---|----------|----------|
| B1 | Unfiltered bulk include (all already included) | Include button disabled. No operation. |
| B2 | Unfiltered bulk exclude | All items excluded. Exclude button disabled. Include button active with full count. |
| B3 | Filtered bulk exclude | Only visible matching items directly excluded. Cascade may exclude additional hidden items. Summary shows direct + cascaded counts. |
| B4 | Filtered bulk exclude cascade side effects | Hidden auto-deps orphaned by visible leaf exclusion are excluded. Toolbar included count reflects visible scope only (per invariant). Notification: "Excluded N packages (+M dependencies)" surfaces hidden side effects. |
| B5 | Filtered bulk include | Only visible matching items included. No cascade (include never auto-includes deps). |
| B6 | Bulk on non-package, non-variant card | No cascade, no variant logic. Direct toggle only. Summary: "Included/Excluded N items." |
| B7 | Bulk exclude on fleet variant card | All child variant toggles unchecked in all targeted groups. Parent rows (alias for child[0]) uncheck automatically. Groups fully excluded. |
| B8 | Bulk include on variant card (all groups excluded) | Child[0] (primary variant) checked for each group. Radio constraint satisfied — one child per group. Parent reflects child[0]. |
| B9 | Bulk include on variant card (some groups already have selection) | Only fully-excluded groups get child[0] checked. Groups with an existing user-selected variant are unchanged. |
| B10 | Item count on variant card | Counts groups with at least one variant checked, not total variants. "12 of 15 included" = 12 groups active. |

### Repo Group State Machine

| # | Scenario | Expected |
|---|----------|----------|
| G1 | All items excluded, no search | Excluded stub visible. Chevron ▸. Warning badge if applicable. |
| G2 | Click stub expand | All items visible, dimmed. Chevron ▾. User can re-include items. |
| G3 | Include one item from expanded stub | Group transitions to normal state. Stub removed. |
| G4 | Search matches inside excluded stub | Stub expands to show matches. |
| G5 | Search clears after G4 | Reverts to excluded stub (row 3). |
| G6 | Search active, group has no matches | "RepoName — no matches" stub. |
| G7 | Manually collapsed group, no search | Existing collapse behavior unchanged. |
| G8 | Search active overrides manual collapse | Matches shown regardless of manual collapse state. |
| G9 | Bulk exclude makes group all-excluded | Transitions to stub. `user_expanded_stub` resets. |

### Warning Visibility

| # | Scenario | Expected |
|---|----------|----------|
| W1 | Filter hides items with warnings or redaction indicators | Toolbar shows "N hidden items have warnings or redactions". |
| W2 | Excluded stub contains items with warnings or redaction indicators | Stub shows warning badge. |
| W3 | All items visible (no filter, no stubs) | No warning indicator in toolbar (all warnings visible inline). |

### Reset and Re-render

| # | Scenario | Expected |
|---|----------|----------|
| R1 | Discard all while filtered | All toggles revert to baseline. Search inputs cleared. All rows restored to visible. All toolbars reset to unfiltered state. Group states recalculated from baseline. |
| R2 | Re-render after dirty changes | DOM unchanged (existing behavior). Snapshot and baseline updated. Search state persists (user's filter intent survives). Toolbar counts recalculate from current DOM. Group states recalculate. Dirty state resets. |
| R3 | Discard all after re-render | Reverts to post-re-render baseline, not pre-re-render state (existing behavior). Search cleared, toolbars reset. |

### Keyboard and Accessibility

| # | Scenario | Expected |
|---|----------|----------|
| A1 | Tab through toolbar | Focus order: search → include → exclude → first item. |
| A2 | Escape in search | Filter cleared, all items restored. |
| A3 | Disabled button focus | Focusable but inert. `aria-disabled="true"`. Tooltip explains why. |
| A4 | Filter count announcement | `aria-live="polite"` region announces "N of M shown" on filter change. |
| A5 | Warning indicator | `role="status"` announces when hidden warnings exist. |

---

## Out of Scope

- Match highlighting (deferred to v2 — filter behavior ships first)
- Filter chips / classification filters (leaf / auto-dep / unclassified)
- Search within collapsed dependency trees
- Debounce on search input
- Virtual scrolling
- Server-side search or bulk operations
- Any new API endpoints or refine server changes
