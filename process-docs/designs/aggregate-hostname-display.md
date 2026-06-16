# Aggregate Hostname Display — Design Options

**Date:** 2026-05-26
**Context:** The aggregate health endpoint returns `hostnames: string[]` in
`AggregateHealthInfo`. The StatsBar currently shows host *count* but not the
actual names. This design proposes tasteful ways to surface them.

## Constraints

- 3 to 100+ hosts
- Short names (`web-01`) or long FQDNs (`prod-web-east-01.datacenter.example.com`)
- Supporting info — must not compete with the main refine workflow
- Light and dark themes (PatternFly v6 `pf-v6-theme-dark`)
- Must feel native to the existing PatternFly UI

## Where hostnames already appear

Hostnames show up *per-item* in the variant view (`VariantView.tsx`) as a
comma-separated list inside each variant option. That works at the item
level. What's missing is a *aggregate-level* roster — "which hosts are in this
merge?"

Two natural anchor points:

1. **StatsBar** — where the host count already lives (`3 hosts · 1,247
   items · ...`). The count is clickable/expandable to show the roster.
2. **AggregateSidebar** — above the section nav, in the same spot single-host
   mode uses for the hostname block (`.inspectah-sidebar__host`).

---

## Option A: Inline Popover from StatsBar

The host count in the StatsBar becomes a clickable link that opens a
PatternFly `Popover` with the full hostname list.

```
 StatsBar
 ┌──────────────────────────────────────────────────────────────────────┐
 │  3 hosts · 1,247 items · All reviewed          [Undo] [Redo] [Export]│
 │  ^^^^^^^^                                                            │
 │  (clickable)                                                         │
 └──────────────────────────────────────────────────────────────────────┘

 Click "3 hosts" → popover appears below:

 ┌─────────────────────────────────┐
 │ Aggregate Hosts                     │
 │ ─────────────────────────────── │
 │  web-01                         │
 │  web-02                         │
 │  web-03                         │
 └─────────────────────────────────┘

 With 50+ hosts and long FQDNs:

 ┌─────────────────────────────────────────┐
 │ Aggregate Hosts (54)                        │
 │ ─────────────────────────────────────── │
 │  prod-web-east-01.datacenter.exampl...  │
 │  prod-web-east-02.datacenter.exampl...  │
 │  prod-web-east-03.datacenter.exampl...  │
 │  prod-web-west-01.datacenter.exampl...  │
 │  ...                                    │
 │  (scrollable, max-height ~300px)        │
 └─────────────────────────────────────────┘
```

**Implementation:**
- Wrap the host count text in StatsBar with a PF `Popover` or `Button
  variant="link"` that opens a popover.
- Popover body: a `<ul>` with monospace font, sorted alphabetically.
- `max-height: 300px; overflow-y: auto` for large aggregates.
- Long FQDNs truncated with `text-overflow: ellipsis`, full name on
  hover via `title` attribute.

**PatternFly components:** `Popover`, `Button variant="link"`, `Content`.

**Tradeoffs:**
- (+) Zero footprint when closed — doesn't affect layout at all
- (+) Discoverable — the count is already visible, making it clickable
  is a natural affordance
- (+) Works at any aggregate size — scroll handles 100+ hosts
- (-) Popover can feel transient; no persistent view
- (-) Long FQDNs still get truncated even inside the popover (though
  you can widen it or wrap)

---

## Option B: Expandable Section in AggregateSidebar

A collapsible hostname roster sits at the top of the sidebar, above the
section navigation. Collapsed by default; shows the count as a heading.

```
 Sidebar (collapsed — default)
 ┌──────────────────────────┐
 │ [>] Aggregate Hosts (3)      │
 │ ─────────────────────────│
 │  Review                  │
 │    Packages         [42] │
 │    Config Files     [18] │
 │    ...                   │
 └──────────────────────────┘

 Sidebar (expanded)
 ┌──────────────────────────┐
 │ [v] Aggregate Hosts (3)      │
 │   web-01                 │
 │   web-02                 │
 │   web-03                 │
 │ ─────────────────────────│
 │  Review                  │
 │    Packages         [42] │
 │    Config Files     [18] │
 │    ...                   │
 └──────────────────────────┘

 With 50+ hosts (expanded):
 ┌──────────────────────────┐
 │ [v] Aggregate Hosts (54)     │
 │   prod-web-east-01.da... │
 │   prod-web-east-02.da... │
 │   prod-web-east-03.da... │
 │   prod-web-west-01.da... │
 │   ... (scrollable area)  │
 │ ─────────────────────────│
 │  Review                  │
 │    Packages         [42] │
 └──────────────────────────┘
```

**Implementation:**
- Use PF `ExpandableSection` with `isIndented` in `AggregateSidebar.tsx`.
- The toggle text shows the count: "Aggregate Hosts (N)".
- Inner list: monospace, sorted, with a `max-height` (~200px) and
  scroll when expanded to prevent the nav from being pushed off screen.
- FQDNs truncated via CSS with `title` for hover.
- Collapsed by default so it doesn't eat sidebar space.

**PatternFly components:** `ExpandableSection`, `Badge`.

**Tradeoffs:**
- (+) Persistent location — always visible in the sidebar whether
  collapsed or expanded
- (+) Consistent with how single-host mode shows the hostname in the
  sidebar (`.inspectah-sidebar__host`)
- (+) No extra click target in the StatsBar — keeps the toolbar clean
- (-) Sidebar is only 240px wide, so long FQDNs get aggressively
  truncated
- (-) When expanded with 50+ hosts, even with max-height the sidebar
  feels heavy; the scroll-within-a-scroll (sidebar already scrolls)
  can be awkward
- (-) Sidebar is hidden below 1024px viewport width (responsive
  breakpoint), so hostnames become inaccessible on narrow screens

---

## Option C: Compact Label Strip Below StatsBar

A thin, secondary bar appears below the StatsBar showing hostnames as
compact labels. Shows the first N hosts inline, with a "+X more" toggle
to expand.

```
 StatsBar
 ┌──────────────────────────────────────────────────────────────────────┐
 │  3 hosts · 1,247 items · All reviewed          [Undo] [Redo] [Export]│
 └──────────────────────────────────────────────────────────────────────┘
 ┌──────────────────────────────────────────────────────────────────────┐
 │  Hosts: [web-01] [web-02] [web-03]                                   │
 └──────────────────────────────────────────────────────────────────────┘

 With many hosts (collapsed — default for >8):

 ┌──────────────────────────────────────────────────────────────────────┐
 │  Hosts: [web-01] [web-02] [web-03] [web-04] ... +46 more            │
 └──────────────────────────────────────────────────────────────────────┘

 Expanded:

 ┌──────────────────────────────────────────────────────────────────────┐
 │  Hosts: [web-01] [web-02] [web-03] [web-04] [web-05] [web-06]       │
 │  [web-07] [web-08] [prod-web-east-01.datacente...] [prod-web-ea...] │
 │  [prod-web-east-03.dat...] ... (wraps naturally)           [Collapse]│
 └──────────────────────────────────────────────────────────────────────┘
```

**Implementation:**
- New thin bar component rendered between StatsBar and the
  `inspectah-layout` div in `AggregateApp`.
- Uses PF `Label` (compact, `isCompact` prop) for each hostname.
- Labels use `color="grey"` to stay visually subdued.
- Show up to 8 labels inline; if more, add a "+N more" link/button
  that expands to show all (with `flex-wrap: wrap`).
- FQDNs truncated via `max-width` on each label with `title` hover.
- Sits in the existing `toolbarExtra` slot or a new dedicated slot.

**PatternFly components:** `Label isCompact color="grey"`, `Button
variant="link"`.

**Tradeoffs:**
- (+) Always visible without clicking — good for small aggregates
- (+) PF Label components look native and handle theming automatically
- (+) Full width available, so FQDNs get more room than sidebar
- (-) Adds permanent vertical space to the UI, even when you don't
  care about hostnames
- (-) At 100 hosts the expanded view is very tall — could push the
  actual content far down the page
- (-) The label strip can feel noisy for large aggregates even when
  collapsed, because the labels have visual weight (borders,
  backgrounds)

---

## Recommendation

**Option A (Popover)** is the strongest fit for this UI. Reasons:

1. **Zero-cost default.** The host count is already there. Making it
   clickable adds discoverability without any layout cost.
2. **Scales cleanly.** A scrollable popover handles 3 hosts and 100+
   hosts equally well. No awkward truncation pressure.
3. **Consistent with PF patterns.** PatternFly popovers are used
   extensively in OpenShift console for exactly this kind of
   "details on demand" pattern.
4. **Theme-safe.** PF Popover inherits theme tokens automatically.
5. **No responsive breakpoint issues.** Unlike sidebar (Option B),
   the StatsBar is always visible at all viewport widths.

Option B is a reasonable alternative if there's a preference for
persistent visibility, but the 240px sidebar width makes it a tight
fit for FQDNs.

Option C is the weakest — it trades vertical space for information
that most users check once and then ignore.

---

## Implementation Notes (for whichever option is chosen)

- **Sort order:** Alphabetical. Natural sort (`web-1`, `web-2`, ...
  `web-10`) if hostnames contain numeric suffixes.
- **Monospace font:** Use `--pf-t--global--font--family--mono` for
  hostname text to maintain the technical-data feel.
- **Copy affordance:** Consider a "Copy all" button in the popover or
  expanded view — useful for ops workflows.
- **Data source:** `AggregateHealthInfo.hostnames` from the health
  endpoint, already available as `aggregate.hostnames` in
  `AggregateApp.tsx` via the `aggregate` prop.
- **Component location:** `StatsBar.tsx` for Option A,
  `AggregateSidebar.tsx` for Option B, `AggregateApp.tsx` for Option C.
