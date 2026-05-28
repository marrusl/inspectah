# Fleet Refine UX Brainstorm

**Status:** Pre-spec brainstorm -- structured thinking for iteration (Mark decisions captured 2026-05-07)  
**Date:** 2026-05-07  
**Author:** Fern (UX Specialist)  
**Scope:** Fleet refine UI -- prevalence surfacing, config variant comparison, editor model, heuristic simplification

---

## 1. Surfacing Prevalence Data

### Mark's idea

The fleet merge already computes `FleetPrevalence` (count/total/hosts) on every merged item, but the refine UI doesn't show it. Users need to see WHY something is preselected.

### Assessment: Strongly agree

This is the single highest-impact change on this list. The data exists in the snapshot (`item.fleet.count` / `item.fleet.total` / `item.fleet.hosts`). The rendering path already reads these fields for threshold computation. The gap is purely presentational.

### Recommended pattern: Inline badge on the toggle card row

Add a prevalence badge to the toggle-card row, between the item name and the expand chevron. The badge occupies the same visual lane as `toggle-card-meta` but is distinct from it:

```
[toggle] nginx.x86_64       3/3 hosts    ▸
[toggle] custom-tool.x86_64  1/3 hosts    ▸
```

**Badge format:** `N/M hosts` -- not a percentage. Absolute counts are meaningful to sysadmins managing a known fleet. "67%" means nothing when you have 3 servers. "2/3" means "which host is missing this?"

**Badge styling tiers:**

| Prevalence | Visual treatment | Rationale |
|---|---|---|
| N/N (unanimous) | Muted text, no special styling | Consensus = no action needed |
| >50% but <100% | Normal text weight, amber-tinted | Soft attention signal |
| <=50% | Normal text weight, amber-tinted, slightly bolder | Stronger attention signal |
| Tied variants | Gold badge with "tied" label (existing pattern from config variant hierarchy spec) | Must-resolve |

**Expand detail enrichment:** When the user expands a toggle card, show the host list in `detail-meta`:

```
Hosts: web-01, web-02 (missing: db-01)
```

This reuses the existing `toggle-card-detail` / `detail-meta` structure. No new DOM patterns required.

### What NOT to do

- Don't add a separate prevalence column. The toggle card is a single-row element, not a table. Adding columns breaks the card paradigm and creates alignment problems across sections.
- Don't show prevalence only on hover. The information is too important to hide behind a mouse gesture, and this audience works with keyboard navigation.

### Open questions

1. **Should prevalence badge be visible in collapsed state?** (Recommended: yes -- it's the primary fleet-mode signal.)
2. **Should host names be short names or FQDNs?** The `host_title_map` in fleet metadata already handles this, but confirm which form is preferred in the badge tooltip vs. the expanded detail.

---

## 2. The Prevalence Heuristic: "100% = auto-include, <100% = review"

### Mark's idea

Replace the user-set prevalence slider with inspectah's own heuristic: default everything to selected, but separate items with less than 100% agreement into a "review needed" bucket. Items on ALL hosts get auto-included; items on SOME hosts get flagged for review.

### Assessment: Agree with the direction, modify the implementation

The intuition is correct. The current slider at arbitrary thresholds (66%, 50%, etc.) is a power-user knob that most sysadmins won't touch meaningfully. The two extremes (0% = union, 100% = intersection) are the natural modes, and everything in between is guesswork.

**However, the binary split (100% vs. <100%) needs nuance for larger fleets.** At 3 hosts, "2/3" is genuinely ambiguous -- could be intentional or accidental. At 50 hosts, "49/50" is almost certainly a single laggy host, not a meaningful divergence. One host out of fifty should not force a manual review.

### Recommended heuristic: Three-zone model

| Zone | Condition | Default behavior | Visual signal |
|---|---|---|---|
| **Consensus** | N/N (all hosts) | Auto-include, no attention needed | Muted prevalence badge |
| **Near-consensus** | N/M where N >= ceil(M * 0.9) | Auto-include, but flagged as "review recommended" | Amber prevalence badge |
| **Divergent** | N/M where N < ceil(M * 0.9) | Auto-include (preserve union), but sorted into "Needs Review" grouping | Amber badge + sorted to top of section |

**Why auto-include divergent items instead of excluding them:**

Excluding items from a migration image is a destructive decision -- the sysadmin expects "my image works like my server." Including an extra package is recoverable; missing one is an outage. The heuristic should sort and surface, not exclude.

**The 90% threshold is configurable at merge time** (--near-consensus-threshold) but has a sensible default. Users who want the strict 100% interpretation can pass `--near-consensus-threshold 100`.

### What about the slider?

**Drop the slider.** Replace it with the three-zone model above, computed at merge time. The refine UI shows the zones as sort groups within each section, not as a separate control. The "Needs Attention" card on the summary tab (from the refine-ui-overhaul spec) counts items in the Divergent zone.

The slider was a proxy for "how strict should I be?" The zone model answers that question with inspectah's own judgment while keeping the user in control of individual items.

### The CLI interface

Keep `--min-prevalence` as a numeric flag for backward compatibility, but add `--heuristic auto` (default) which uses the three-zone model. When `--heuristic auto` is active, `--min-prevalence` is ignored.

### Open questions

1. **Is 90% the right near-consensus threshold?** It's arbitrary. For 3-host fleets, 90% rounds to ceil(2.7)=3, meaning the near-consensus zone is empty and everything that's not unanimous is divergent. This seems correct for small fleets. For 10-host fleets, ceil(9)=9, so 9/10 is near-consensus and 8/10 is divergent. Does this feel right?
2. **Should the zone thresholds be exposed in the UI at all?** Leaning no. The zones are inspectah's recommendation; users override per-item.
3. **What about items unique to a single host (1/50)?** Still auto-include, still sorted to "review," but could have a distinct visual treatment ("unique to host X") to communicate that this might be host-specific drift rather than intentional configuration.

---

## 3. Config File Variant Comparison and Selection

### Mark's idea

Users should be able to compare config file variants (e.g., "Host A has this nginx.conf, Host B has this different nginx.conf") and pick which one to use -- without navigating away.

### Assessment: Strongly agree -- this is the other high-impact item

The existing variant auto-selection spec (2026-03-22) and the config variant visual hierarchy spec (2026-03-31) already established the model: variants grouped by path, tie/winner flags, fleet counts per variant. The gap is in the comparison UI: users can see that variants exist but can't see what's different.

### Recommended pattern: Inline variant drawer

When a config file has multiple variants (same path, different content), the expanded toggle card should show a variant comparison panel. Not a modal, not a separate tab -- an inline expansion below the toggle card, inside the current section.

**Layout for 2 variants:**

```
/etc/nginx/nginx.conf
  [Variant A]  web-01, web-02 (2/3 hosts)  [selected]
  [Variant B]  db-01 (1/3 hosts)
  [Compare >>]
```

Clicking "Compare" expands to a unified diff view below:

```
--- Variant A (web-01, web-02)
+++ Variant B (db-01)
@@ -12,3 +12,3 @@
 worker_processes auto;
-worker_connections 1024;
+worker_connections 2048;
 keepalive_timeout 65;
```

**Why unified diff, not side-by-side:**

- The report is already width-constrained (especially with the preview panel open).
- Sysadmins read unified diffs daily (`diff -u`, `git diff`). This is their native format.
- Side-by-side requires horizontal scrolling for config files with long lines. Unified avoids this.
- For 3+ variants, side-by-side becomes unwieldy. Unified scales to N variants with a variant selector dropdown.

**Variant selection interaction:**

- Radio buttons next to each variant. Selecting a variant sets `include=true` on that variant and `include=false` on the others.
- The "selected" variant's content is what goes into the tarball and Containerfile.
- If no variant is selected (tie, user hasn't chosen), the radio group has nothing checked and the item shows a "choose a variant" prompt.

**For 3+ variants:**

- Show the full variant list with host attribution and radio buttons.
- Compare dropdown: "Compare A vs B", "Compare A vs C", "Compare B vs C". Default to comparing the auto-selected variant against the next most prevalent.
- Don't try to show a three-way diff -- it's confusing even for experienced users. Pairwise comparison is the right pattern.

### Interaction with the File Editor

When a variant is selected, the user can edit it. The "Edit" action on a variant should open the editor (see section 5 below) pre-loaded with that variant's content. This is where the editor integration becomes critical.

### Open questions

1. **Should the diff be computed client-side or at merge time?** Client-side is simpler (the content is already in the snapshot JSON). Server-side pre-computation would add to the snapshot size. Leaning client-side.
2. **What diff library for client-side?** A lightweight JS diff like `diff-match-patch` or a simpler line-diff implementation. Must be embeddable in the single-file HTML report (no CDN dependencies).
3. **Maximum variant count before the UI pattern breaks?** At 5+ variants, the dropdown gets noisy. Is there a real-world scenario where a single config file has 5+ distinct variants across a fleet? If so, we might need a "show all variants" overflow pattern.

---

## 4. Grouping Conflicting Config Files

### Mark's idea

When the same config file has different content across hosts, the fleet merge picks one but doesn't make the conflict visible. Users need to see the variants and understand what happened.

### Assessment: This is the same problem as #3, solved by the same mechanism

The variant comparison panel (section 3) directly addresses this. Config files with variants are "conflicting" -- they're the same path with different content. The visual hierarchy spec (2026-03-31) already defined the three-tier system:

- **Tier 1 (Tied):** Gold badge, "tied -- compare & choose"
- **Tier 2 (Auto-selected winner):** Amber badge, "2/3 hosts -- review"
- **Tier 3 (Unanimous):** No badge, consensus

What's new here is the inline comparison UI that makes the conflict actionable, not just visible.

**Additional recommendation: Conflict count in section header.**

The Config section header should show: `Config (39 files, 3 with variants)`. This gives immediate visibility into how much variant resolution work exists without scrolling through every item.

**Sorting:** Items with variants should sort to the top of the section, with tied items above auto-selected items. This is the progressive disclosure approach: put the items that need decisions first.

---

## 5. Inline Editing Modal vs. Separate File Editor Tab

### Mark's idea

Replace the separate "Edit Files" tab with an inline modal that pops up from the config/containers/drop-ins interface. Same revert/save semantics, but editing happens in context.

### Assessment: Agree with the direction, but "modal" is the wrong pattern -- use a drawer

The problem Mark identified is real: the File Editor tab duplicates the file listing from the Config tab. Users navigate to Config, find a file, realize they want to edit it, then switch to Edit Files and find the same file again. This is a double-navigation tax.

**But a modal (overlay covering the center of the screen) has problems:**

1. **Modals lose context.** The user can't see the toggle card or variant comparison behind the modal. They lose the "why am I editing this?" context.
2. **Modals are disruptive.** CodeMirror in a modal requires careful focus management. Escape typically closes modals, but in CodeMirror Escape exits editing -- these conflict.
3. **Modals don't resize well.** Config files can be long. A modal is constrained to viewport height; a user can't make it bigger.

### Recommended pattern: Slide-in drawer (right side)

The existing report layout has a collapsible preview panel on the right. The editor drawer would use the same slot -- when editing, the preview panel collapses and the editor drawer slides in from the right.

**Interaction flow:**

1. User is on the Config section, looking at a toggle card for `/etc/nginx/nginx.conf`.
2. User clicks "Edit" on the toggle card (or on a variant in the comparison panel).
3. The preview panel (right side) transitions to the editor drawer:
   - File path displayed at top
   - CodeMirror editor with syntax highlighting
   - Toolbar: Save / Revert / Close
   - Modified indicator (blue dot) on the file's toggle card
4. The user can still see the left-side section list and toggle cards. They maintain context.
5. "Close" collapses the drawer back to the preview panel.

**This pattern:**

- Keeps context visible (the section list stays on the left)
- Provides ample editing space (the right panel is already ~40-50% of viewport)
- Reuses existing layout infrastructure (the preview panel collapse/expand mechanism)
- Avoids Escape key conflicts (drawer close is a button, not Escape)
- Works for both config editing and variant selection+editing

**What happens to the Edit Files tab?**

Remove it. All editing flows through the drawer. The tab bar loses one entry, which is fine -- fewer tabs means less cognitive load.

**The PF6 resizable drawer spec (from 2026-03-22 variant auto-selection spec, Part C) already proposed this pattern.** This brainstorm validates and extends it.

### Tradeoffs vs. keeping the tab

| Drawer | Separate tab |
|---|---|
| In-context editing, no navigation tax | Full-screen editing space |
| Single file at a time | File browser for batch review |
| Preview panel unavailable during editing | Preview always available |
| Simpler mental model ("edit where you find") | Familiar "editor app" metaphor |

The tab's advantage is batch file review -- scrolling through multiple files quickly. The drawer optimizes for the more common case: editing a specific file the user already found through the refine flow.

**Mitigation for batch review:** Add a "Next file" / "Previous file" button to the drawer toolbar. This lets users step through files without closing the drawer. The file order follows the section's sort order.

### Open questions

1. **Should the drawer support opening multiple files as tabs within the drawer?** (Recommended: no, not initially. Start with single-file. Add tabs if users need batch review.)
2. **What happens when the user navigates to a different section while the drawer is open?** Options: (a) auto-close the drawer with unsaved-changes protection, (b) keep the drawer open showing the current file regardless of section. Leaning toward (a) with the existing dirty-state modal.
3. **Mobile/narrow viewport:** The drawer pattern breaks below ~900px. For narrow screens, fall back to a full-screen overlay (which is effectively the modal Mark suggested, but only for narrow viewports).

---

## 6. Making Preselection Reasoning Transparent

### Mark's idea

The current UI doesn't explain why something is preselected or not. Prevalence-based decisions should be transparent.

### Assessment: Agree -- this is about explanation text, not new UI elements

Every toggle card's expanded detail area (`toggle-card-detail`) should include a `detail-reason` line explaining the preselection:

**Reason text patterns:**

| Scenario | Reason text |
|---|---|
| Unanimous, included | "Present on all 3 hosts" |
| Near-consensus, included | "Present on 9/10 hosts (review recommended)" |
| Divergent, included | "Present on 2/10 hosts -- review recommended" |
| Variant auto-selected | "Selected: most prevalent variant (2/3 hosts). 1 other variant exists." |
| Tied, no selection | "Tied: 2 variants with equal prevalence (1/3 hosts each). Choose a variant." |
| Excluded by user | "Excluded by you" (or blank if never interacted with) |
| Single-machine mode | No fleet reason (existing heuristic reasons apply) |

These reason strings are generated in JS from the `fleet` object on each item. No backend changes needed.

**This is a low-effort, high-clarity change.** It directly addresses Mark's concern and requires only JS additions to `renderToggleCard()`.

---

## 7. Progressive Disclosure Strategy

Adding prevalence badges, variant comparison, inline diffs, and an editor drawer is a lot of new information. How to keep it manageable:

### Layer 1: Always visible (no interaction required)

- Prevalence badge (`N/M hosts`) on toggle card row
- Section header conflict count (`3 with variants`)
- Sort order (variants first, then divergent, then near-consensus, then unanimous)

### Layer 2: One click to reveal

- Expand toggle card to see host list, reason text, variant list
- "Compare" button to show unified diff
- "Edit" button to open editor drawer

### Layer 3: Explicit action

- Variant radio selection (choosing which content to use)
- Editing file content in the drawer
- Revert / Save actions

This three-layer model means the default view is dense but scannable. Every item shows its prevalence. Conflicts sort to the top. But the user doesn't see diffs, host lists, or editors until they engage with a specific item.

---

## 8. Phasing Recommendation

These changes have different dependency profiles and can be delivered incrementally:

### Phase 1: Prevalence visibility (low risk, high impact)

- Add prevalence badge to toggle cards
- Add reason text to toggle card detail
- Add conflict count to section headers
- Sort variants to top of sections
- **No backend changes.** All data is already in the snapshot JSON.
- **Estimated scope:** JS + CSS changes to `report.html` only.

### Phase 2: Heuristic simplification (backend + frontend)

- Implement three-zone model in `fleet/merge.go`
- Remove prevalence slider from summary tab
- Add zone indicators to toggle cards
- Update CLI flags (--heuristic, --near-consensus-threshold)
- **Backend changes:** `merge.go`, `types.go` (add zone field), CLI flag parsing.
- **Depends on Phase 1** for the rendering infrastructure.

### Phase 3: Variant comparison (frontend, medium complexity)

- Inline variant comparison panel in toggle card expansion
- Unified diff view (client-side diff computation)
- Variant radio selection
- **Depends on Phase 1** (variant sorting) but independent of Phase 2.
- **Complexity driver:** Choosing and embedding a diff library.

### Phase 4: Editor drawer (frontend, high complexity)

- Replace Edit Files tab with slide-in editor drawer
- "Edit" button on toggle cards and variant panels
- Unsaved-changes protection on drawer close
- File navigation within drawer (next/prev)
- **Independent of Phases 1-3** architecturally, but benefits from Phase 3's variant selection UI.
- **Complexity driver:** CodeMirror lifecycle management, drawer animation, keyboard focus trapping.
- **Risk:** This is the most disruptive change. The Edit Files tab is a known pattern. Removing it requires that the drawer fully replaces its functionality.

### Phase 5: Refinement (post-feedback)

- Multi-file tabs in editor drawer (if needed)
- Three-way variant comparison (if real-world data shows 3+ variants are common)
- Keyboard shortcuts for variant selection
- ARIA live region announcements for prevalence changes

---

## Summary of Reactions

| Mark's idea | Reaction | Key modification |
|---|---|---|
| Surface prevalence data | Strongly agree | Inline badge, not column. `N/M hosts` format. |
| Group conflicting config files | Agree | Solved by variant comparison panel + section header counts |
| Make preselection reasoning transparent | Agree | Reason text in toggle card detail, low-effort JS change |
| Drop prevalence slider, use 100% heuristic | Agree with modification | Three-zone model (consensus / near-consensus / divergent) instead of binary 100% split |
| Config variant comparison in-place | Strongly agree | Unified diff, pairwise comparison, variant radio selection |
| Inline modal instead of Editor tab | Agree with modification | Drawer, not modal. Right-side slide-in replaces preview panel. |

---

## Open Questions Requiring Mark's Input

1. **Near-consensus threshold:** Is 90% the right default, or should it be stricter (95%) or looser (80%)?
2. **Host name format:** Short hostnames or FQDNs in prevalence badges?
3. ~~**Diff library choice:** Embed a JS diff library or write a minimal line-diff?~~ **Resolved.** The Python version has a self-contained LCS-based `lineDiff()` function (no external library) that can be ported. Mark says don't feel bound by it, but it's a proven starting point.
4. **Batch file review:** Is stepping through files in the drawer (next/prev) sufficient, or does the loss of the file browser sidebar matter?
5. **Variant count ceiling:** Is there a practical limit to how many variants a single config file might have across a fleet? This affects whether pairwise comparison is sufficient or if we need a more scalable pattern.
6. ~~**Phase 1 timing:**~~ **Resolved.** Mark agrees with incremental phasing.

---

## Mark's Decisions (2026-05-07)

1. **Prevalence threshold:** Default to 100%. Items at 100% prevalence (strict intersection) are "Included" — no review needed. Items below 100% fall into the "Review" tier. The threshold is adjustable interactively in the UI — users can lower it and watch items move from "Review" to "Included" in real time. This replaces both the old CLI-only `-p` flag and Fern's proposed three-zone model with a single interactive control.
2. **Diff implementation:** The Python version's self-contained `lineDiff()` (LCS-based, no external deps, 5000-line cap) is a proven reference. Port or rewrite as needed — not bound to it.
3. **Phasing:** Agreed — prevalence visibility first (Phase 1), then variant comparison, then editor consolidation.
