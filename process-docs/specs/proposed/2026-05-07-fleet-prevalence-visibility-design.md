# Fleet Prevalence Visibility — Phase 1

**Date:** 2026-05-07
**Status:** Proposed (revision 6 — fixes stale updateBadge precedence bullet)
**Scope:** Fleet refine UI — prevalence data surfacing, threshold presets, reason text, sort order
**Phase:** 1 of N (prevalence visibility only; variant comparison, editor drawer, and editor consolidation are future phases)

---

## Summary

Fleet refine mode shows aggregated data from multiple hosts but doesn't surface *why* items are preselected or how prevalent they are across the fleet. This spec adds prevalence visibility throughout the refine UI so users understand fleet consensus at a glance and can adjust the classification threshold interactively.

**What this spec covers:**
1. Prevalence badges on toggle cards (applicable sections only)
2. Threshold presets dropdown on Overview
3. Reason text in expanded toggle card details
4. Section header review counts
5. Prevalence-aware sort order within sections
6. Merge default change (`--min-prevalence` default 100 → 0)

**What this spec does NOT cover (future phases):**
- Config variant comparison and selection UI (requires stable per-variant identity)
- Inline editing drawer
- Editor tab consolidation
- Diff rendering
- Tied-variant gold badges, variant counts in headers, variant-specific reason text

---

## State Contract

### Threshold is presentation-only

The prevalence threshold controls **sort order and badge styling only**. It does NOT modify `include`, `default_include`, or any build-relevant state in the snapshot.

**Concrete invariants:**
1. Changing the threshold preset never calls `updateSnapshotInclude()`.
2. Changing the threshold preset never increments the change counter.
3. Changing the threshold preset never triggers a rebuild prompt or sets rebuild-needed state.
4. Changing the threshold preset never modifies `App.decisions` or `App.priorValues`.
5. The threshold affects only `renderTriageSection()`, `updateBadge()`, and the Overview summary card counts for applicable sections.

### Relationship to the existing tier hierarchy

The triage tier system (Flagged / Review / Included / Auto-included) remains the **primary grouping** for all sections. Prevalence is a **secondary lens within each tier group**, not a replacement for the tier system.

Concretely:
- Items are first grouped by their triage tier (unchanged from current behavior).
- Within each tier group, items are sub-sorted by prevalence zone (below-threshold items first, above-threshold items second, unanimous items last).
- The prevalence badge is visual annotation on each row — it does not move items between tiers.
- Section header "X to review" counts are **additive information** about how many items in the section are below the prevalence threshold. They appear alongside the existing tier structure, not as a replacement.

The word "Review" in "X to review" refers to prevalence review (items below the threshold that the user should look at), not the triage "Review" tier. If this naming collision causes confusion in implementation, the header text can use "X below threshold" instead — the spec is flexible on the exact wording as long as the meaning is clear.

### Merge baseline change

The current Go port defaults `--min-prevalence` to 100, causing items below 100% prevalence to arrive with `include=false`. This conflicts with the presentation-only threshold model, where the threshold should only affect visual grouping — not build state.

**Change:** Default `--min-prevalence` from 100 to 0 in `cmd/inspectah/internal/cli/passthrough.go`. This means:
- All items arrive with `include=true` regardless of prevalence
- The `--min-prevalence` CLI flag still works for users who want pre-exclusion at merge time
- The presentation threshold (client-side dropdown) handles visual classification
- No items are silently excluded — the user sees everything and makes explicit decisions

This is the only backend change in Phase 1.

### Saved fleet snapshots and non-zero merge thresholds

Fleet snapshots created before Phase 1 (or with an explicit `--min-prevalence` value > 0) may have items with `include=false` baked in from the merge. Phase 1 does NOT retroactively alter this state.

**Behavior with pre-existing `include=false` items:**
- Items with `include=false` render with their toggle off, as they do today. The user can toggle them back on manually.
- Prevalence badges still render based on the `fleet` data (which is present regardless of `include` state).
- The presentation threshold still sub-sorts items by prevalence zone within each tier.
- The "X to review" header count reflects items below the presentation threshold, regardless of their `include` state. An item that is both `include=false` (from the merge) and below the presentation threshold is counted once — it doesn't double-count.

**The key principle:** Phase 1 is additive. It adds prevalence badges, sort order, and a threshold dropdown on top of whatever state the snapshot has. It never modifies existing state. A fleet snapshot from last week opens and works exactly as it does today, plus it now shows prevalence badges and supports threshold-based sorting.

---

## Applicable Surfaces

Phase 1 prevalence badges apply to sections that use the toggle-card pattern AND have stable per-item identity (no same-path variant collisions):

| Section | ID | In scope | Notes |
|---|---|---|---|
| Packages | `packages` | **Yes** | Identity-stable: keyed by name.arch |
| Runtime | `runtime` | **Yes** | Identity-stable: keyed by unit name |
| Identity | `identity` | **Yes** | Identity-stable: keyed by username/group |
| System & Security | `system` | **Yes** | Identity-stable: keyed by setting name |
| Secrets | `secrets` | **Yes** | Identity-stable: keyed by path + kind |

### Excluded from Phase 1

| Section | ID | Reason |
|---|---|---|
| Configuration | `config` | Same-path variants can collide on the current path-based identity. Config items with multiple variants for the same path produce ambiguous prevalence badges. Deferred until per-variant identity is stable (Phase 2). |
| Containers | `containers` | Uses flat subsection rendering (`sectionId === 'containers'` special case), not tier grouping. |
| Non-RPM Software | `nonrpm` | Uses review-status cards (radio-group pattern), not toggle cards. Badge placement, header semantics, and sidebar behavior differ from toggle-card sections. Requires its own explicit prevalence contract rather than inheriting toggle-card rules. |
| Overview | `overview` | Hosts the threshold dropdown, not a triage surface. |
| Version Changes | `version-changes` | Informational display only. |
| Edit Files | `editor` | File editor, not a triage surface. |

---

## 1. Prevalence Badge on Toggle Cards

### Behavior

Every toggle card in an applicable section gets an inline prevalence badge when `item.fleet` exists on the triage manifest entry.

The badge sits between the item name/meta and the chevron, in the `toggle-card-meta` visual lane.

**Badge format:** `N/M hosts` by default. The format toggle is **report-global** — one click on any badge switches ALL badges between `N/M hosts` and percentage (`67%`). Stored in `App.prevalenceFormat` (`"count"` or `"percent"`).

**Badge styling:**

| Prevalence zone | Style |
|---|---|
| `unanimous` (N/N, 100%) | Muted text color (`#8b949e`), no special treatment |
| `above` (<100% but ≥ threshold) | Normal text, amber-tinted (`#d29922`) |
| `below` (< threshold, review needed) | Bold text, amber-tinted (`#d29922`, `font-weight: 600`) |

**Collapsed-row behavior:** The badge is visible in collapsed state. The host list and reason text are NOT visible until the card is expanded. The badge is the only fleet signal on a collapsed row.

**Single-machine mode:** No prevalence badge. The badge only renders when `item.fleet` is non-null.

### Expand Detail Enrichment

When a toggle card is expanded, the detail area shows the host list in the existing `detail-meta` slot:

```
Hosts: web-01, web-02 (missing: db-01)
```

The "missing" hosts are computed as the set difference between the full fleet host list (from `fleet_meta.host_title_map` keys) and `item.fleet.hosts`.

### Badge Click Behavior

Clicking the badge toggles the format for ALL badges (report-global toggle). **The badge click must NOT expand/collapse the card.** Implementation: `event.stopPropagation()` on the badge click handler prevents the event from reaching the card's expand handler.

### Accessibility

- **Badge element:** `<span>` with `role="button"`, `tabindex="0"`, `aria-label="Prevalence: 2 of 3 hosts. Activate to toggle percentage display."`.
- **Keyboard:** Enter or Space toggles the format (report-global). Tab order: toggle switch → item name → prevalence badge → chevron.
- **Screen reader:** Badge announces `"2 of 3 hosts"` (not `"2/3 hosts"`). After toggle, announces `"67 percent"`. Format change is announced via an `aria-live="polite"` region (a visually hidden span that updates with "Showing percentages" or "Showing host counts").
- **Badge click does NOT expand the card.** This is both a UX requirement (format toggle is a lightweight action; card expand is a navigation action) and an accessibility requirement (two different actions on the same visual row must be separately activatable).
- **Click target:** Minimum 24x24px touch target via padding.

### Data Source

The `FleetPrevalence` struct on each triage manifest item:

```json
{
  "fleet": {
    "count": 2,
    "total": 3,
    "hosts": ["web-01", "web-02"]
  }
}
```

No additional backend changes needed for the badge (the merge default change in State Contract covers the baseline).

---

## 2. Threshold Presets Dropdown

### Placement

Top of the Overview section, below the section heading, above the summary cards. A `<select>` element with a `<label>`:

```
Prevalence threshold: [Unanimous (100%) ▾]
Items below this threshold are flagged for review.
```

### Preset Options

| Label | Threshold value | Meaning |
|---|---|---|
| Unanimous (100%) | `1.0` | On every host — default |
| Strong consensus (≥80%) | `0.8` | On nearly all hosts |
| Majority (≥50%) | `0.5` | On most hosts |
| Any presence (≥1) | `0.0` | On at least one host |

**Default:** Unanimous (`1.0`). Items at 100% prevalence are above threshold (no prevalence attention needed). Items below 100% are below threshold and sub-sort to the top of their tier group with a prevalence badge.

### Interaction

When the user changes the selection:

1. `App.prevalenceThreshold` updates to the new value
2. Overview summary card counts re-render (for applicable sections only)
3. Sidebar badges update via `updateBadge()` for applicable sections
4. Section content re-renders on next navigation via `renderTriageSection()`
5. No server round-trip
6. No `include` / `default_include` state changes (see State Contract)

### Threshold Visibility Outside Overview

The active threshold is shown as a static text indicator below the review progress bar in the sidebar:

```
Threshold: Unanimous (100%)
```

This is a `<span>` (not interactive — the dropdown lives on Overview only). It updates when the dropdown changes.

### Mode Availability

The dropdown and sidebar indicator appear in both static and refine modes.

### Accessibility

- **`<select>` element:** Standard HTML `<select>` with `<label for="...">` association. No custom widget.
- **Sidebar indicator:** `aria-live="polite"` region. When the threshold changes, the region content updates and screen readers announce the new value.
- **Focus:** After changing the threshold, focus remains on the `<select>`.

---

## 3. Reason Text in Toggle Card Detail

### Behavior

When a user expands a toggle card in fleet mode (applicable sections only), the detail area shows a reason line in the existing `detail-reason` slot.

### Reason Text Patterns (Phase 1)

| Scenario | Text |
|---|---|
| 100% prevalence | `Present on all 3 hosts` |
| Sub-threshold | `Present on 2/3 hosts — review recommended` |
| Single host only | `Present on 1/3 hosts (web-staging-01 only) — review recommended` |
| Above threshold but <100% | `Present on 2/3 hosts` (no "review recommended" since it's above the user's threshold) |
| Not fleet mode | No fleet reason shown (existing heuristic reasons apply) |

**Deferred to Phase 2 (variant comparison):**
- Variant-specific reason text ("Selected: most prevalent variant", "Tied: 2 variants")

### Rules

- **Data over rationale.** Show `2/3 hosts`, not explanatory prose.
- **Single-host items name the host inline.**
- **Reason text is generated in JS** from the `fleet` object. No backend changes.
- **Reason text is threshold-aware.** The "review recommended" suffix only appears when the item is below the current threshold. Changing the threshold updates reason text on next expand/render.

---

## 4. Section Header Review Counts

### Behavior

In fleet mode, section headers for applicable surfaces show a review count.

### Format

```
Packages (94 items, 7 to review)
Runtime (18 items, 2 to review)
Identity (12 items)
```

### Rules

- `X to review` = items in this section with prevalence below the current threshold
- Counts update when the user changes the threshold preset
- If all items are at or above threshold: just show the item count — `Packages (94 items)`
- Single-machine mode: no change to current headers
- **Excluded sections** (config, containers, nonrpm) show their current header format — no fleet enrichment in Phase 1

### Sidebar Badges

The sidebar navigation badges for applicable sections reflect the review count, reusing the existing tier badge visual pattern. The badge shows the count of items below threshold. Badge is hidden when the count is zero.

**Badge coexistence:** In fleet mode, the sidebar badge shows the prevalence "below threshold" count as additive information alongside the existing tier attention signals. The existing tier-3/tier-2 badge logic remains unchanged — prevalence counts are a secondary indicator, not a replacement. If a section has both tier-3 flagged items and below-threshold prevalence items, both signals are relevant. Implementation can show whichever count is higher, or combine them — the spec is flexible on the exact display as long as both signals are preserved. Single-machine mode keeps the existing badge behavior unchanged.

---

## 5. Sort Order

### Behavior

Within each applicable section, fleet mode adds prevalence-aware sorting on top of the existing tier structure.

### Sort Priority (within each tier group)

1. **Below threshold** (review needed) — sorted by prevalence ascending (lowest prevalence first, most divergent items at top)
2. **Above threshold** (included) — sorted by prevalence descending (unanimous items last since they need no attention)

### Interaction with Threshold Presets

When the user lowers the threshold (e.g., Unanimous → Majority), items move from the "below" group to the "above" group and re-sort. The re-render is instant (client-side).

### Single-Machine Mode

No change to current sort behavior.

---

## Architecture

### Approach: Triage-manifest driven with client-side interactivity

The triage manifest gets a `PrevalenceZone` field computed at render time from `fleet.Count / fleet.Total` against the default 100% threshold. JS reads this field for initial badge styling and prevalence sub-sort order within each tier group. Triage tier assignment is unchanged — `PrevalenceZone` is orthogonal to tiers.

**`prevalence_zone` values:**
- `"unanimous"` — `count == total` (100%)
- `"above"` — `count/total >= threshold` but not 100%
- `"below"` — `count/total < threshold` (review needed)
- `""` (empty) — not a fleet item (single-machine mode), or section excluded from Phase 1

When the user changes the threshold preset, JS recomputes `prevalence_zone` client-side from the raw `fleet` data on each item and re-renders affected sections.

### Identity Constraints

Phase 1 prevalence badges only appear on sections with stable per-item identity (see Applicable Surfaces). Config, containers, and nonrpm are excluded because their identity models don't safely support per-row prevalence badges without variant-aware keying (config) or require a different card pattern (nonrpm, containers).

### Changes Required

**Backend (Go):**

`cmd/inspectah/internal/cli/passthrough.go`:
- Change `--min-prevalence` default from `100` to `0`

`cmd/inspectah/internal/renderer/triage.go`:
- Add `PrevalenceZone string` field to `TriageItem` struct
- The existing `Fleet *schema.FleetPrevalence` field on `TriageItem` is the data source — it is already populated by the triage builder for fleet snapshots and is `nil` for single-machine snapshots
- Compute zone from `Fleet.Count / Fleet.Total` against 100% threshold at manifest build time
- Populate on all manifest entries that have non-nil `Fleet` data
- Entries for excluded sections (config, containers, nonrpm) get `prevalence_zone: ""`

**Frontend (report.html JS):**
- `App.prevalenceThreshold` — state variable, default `1.0`
- `App.prevalenceFormat` — state variable, `"count"` (default) or `"percent"`, report-global
- `computePrevalenceZone(item, threshold)` — pure function for client-side recomputation
- `isApplicableForPrevalence(sectionId)` — returns false for config, containers, nonrpm, overview, version-changes, editor
- Threshold preset `<select>` with `<label>` on Overview section
- Sidebar threshold indicator `<span>` with `aria-live="polite"`
- Prevalence badge in `buildToggleCard()` — `<span role="button" tabindex="0">` with `event.stopPropagation()`, click toggles `App.prevalenceFormat` and re-renders all visible badges
- Hidden `aria-live="polite"` region for format toggle announcements
- Reason text generation from `fleet` object in toggle card detail
- Section header count enrichment in section heading builder (applicable sections only)
- `updateBadge()` enrichment for fleet review counts (applicable sections, additive alongside existing tier badges per the sidebar coexistence contract above)
- Prevalence-aware comparator in tier-group sort logic (applicable sections only)

**Frontend (report.html CSS):**
- `.prevalence-badge` — base style (font-size: 0.75rem, padding, cursor: pointer, min touch target 24x24px)
- `.prevalence-badge.zone-unanimous` — color: `#8b949e`
- `.prevalence-badge.zone-above` — color: `#d29922`
- `.prevalence-badge.zone-below` — color: `#d29922`, font-weight: 600
- Dark theme overrides for badge colors
- `.sidebar-threshold` — indicator style (font-size: 0.8rem, opacity: 0.7)

### What Does NOT Change

- Fleet merge logic (`fleet/merge.go`) — merge algorithm unchanged; only CLI default changes
- Snapshot schema (`FleetPrevalence` struct already complete)
- Refine server (`refine/server.go`) — no new endpoints
- Single-machine mode — no behavioral changes
- Existing tier structure (Flagged / Review / Included / Auto-included)
- `include` / `default_include` state — threshold is presentation-only
- Config section renderer — excluded from Phase 1
- Containers section renderer — excluded from Phase 1
- Non-RPM section renderer — excluded from Phase 1

---

## Deferred: Variant Identity

The Go port currently keys several content-bearing items by path/name identity. Same-path config variants can collide in the triage manifest, making per-row prevalence badges ambiguous. This affects:

- Config-section prevalence badges
- Tied-variant gold badges
- Variant counts in section headers ("3 with variants")
- Variant-specific reason text

All of these require a stable per-variant identity seam in `triage.go`. This is a prerequisite for the Phase 2 variant comparison spec and is explicitly out of scope for Phase 1.

## Deferred: Non-RPM Prevalence

Non-RPM items use review-status cards with a radio-group interaction pattern, not toggle cards. Prevalence badges for nonrpm require:

- Defining where the badge sits in the review-status card layout (different from toggle-card anatomy)
- Defining how the header count interacts with the existing `countBadge` pattern (nonrpm already has its own badge semantics)
- Defining sidebar badge precedence for a non-tracked section

This is a small, self-contained follow-up that can land after Phase 1 without the variant identity dependency. It should be specced separately.

---

## Verification Contract

### Renderer Tests (`cmd/inspectah/internal/renderer/`)

| Test | Assertion |
|---|---|
| `TestTriageManifest_PrevalenceZone_Unanimous` | Manifest entry with `fleet.count == fleet.total` gets `prevalence_zone: "unanimous"` |
| `TestTriageManifest_PrevalenceZone_Below` | Manifest entry with `fleet.count < fleet.total` gets `prevalence_zone: "below"` at 100% threshold |
| `TestTriageManifest_PrevalenceZone_NoFleet` | Manifest entry without fleet data gets `prevalence_zone: ""` |
| `TestTriageManifest_PrevalenceZone_ExcludedSection` | Manifest entries for config/containers/nonrpm sections get `prevalence_zone: ""` regardless of fleet data |

### CLI Tests (`cmd/inspectah/internal/cli/`)

| Test | Assertion |
|---|---|
| `TestFleetCommand_DefaultPrevalence` | `--min-prevalence` flag defaults to 0 (not 100) |

### Refine Tests (`cmd/inspectah/internal/refine/`)

| Test | Assertion |
|---|---|
| `TestRebuild_ThresholdDoesNotDirtyState` | Verify that no code path exists where threshold state changes call `updateSnapshotInclude`, increment the change counter, or set rebuild-needed flags. (This is a code-level assertion that the threshold is presentation-only, not just a UI test.) |

### Browser / E2E Tests (`tests/e2e-go/tests/`)

| Test | Assertion |
|---|---|
| `fleet-prevalence-badge-visible` | Fleet report toggle cards in applicable sections show `N/M hosts` badge. Badge text matches `fleet.count`/`fleet.total` from snapshot. |
| `fleet-prevalence-badge-absent-config` | Config section toggle cards do NOT show prevalence badges. |
| `fleet-prevalence-badge-absent-single` | Single-machine report shows no prevalence badges anywhere. |
| `fleet-badge-format-toggle` | Clicking any badge toggles ALL badges between `N/M hosts` and percentage format. |
| `fleet-badge-no-expand` | Clicking the badge does NOT expand/collapse the card (stopPropagation). |
| `fleet-badge-keyboard` | Badge is reachable via Tab. Enter/Space toggles format. Focus remains on badge after toggle. |
| `fleet-threshold-dropdown` | Overview section contains threshold `<select>` with 4 options. Default is "Unanimous (100%)". |
| `fleet-threshold-recount` | Changing threshold from "Unanimous" to "Majority" updates sidebar badge counts for applicable sections. |
| `fleet-threshold-resort` | After threshold change, items re-sort within tier groups: below-threshold items appear before above-threshold items. |
| `fleet-threshold-no-dirty` | After changing threshold, change counter remains at 0, rebuild badge does not appear, and no `include` values in the snapshot have changed. |
| `fleet-threshold-focus` | After changing the threshold `<select>`, focus remains on the `<select>` element. |
| `fleet-threshold-sidebar-indicator` | Sidebar shows active threshold label. Changes when dropdown changes. |
| `fleet-threshold-aria-live` | Changing threshold triggers `aria-live` region update with new threshold name. |
| `fleet-reason-text` | Expanded toggle card in fleet mode (applicable section) shows reason text with prevalence data. |
| `fleet-reason-text-threshold-aware` | Reason text "review recommended" suffix appears only when item is below current threshold. |
| `fleet-badge-format-aria-live` | Toggling badge format triggers `aria-live` announcement ("Showing percentages" / "Showing host counts"). |

### What Tests Do NOT Cover (deferred)

- Config section prevalence (excluded from Phase 1)
- Containers section prevalence (excluded from Phase 1)
- Non-RPM section prevalence (excluded from Phase 1)
- Variant-specific badge behavior (tied, winner)
- Variant comparison or selection interactions
- Editor drawer interactions

---

## Non-Goals (Future Phases)

- **Config section prevalence** — requires stable per-variant identity
- **Containers section prevalence** — requires adapting subsection renderer
- **Non-RPM prevalence** — requires its own card-pattern contract (small follow-up)
- **Config variant comparison UI** — requires per-variant identity
- **Editor drawer** — slide-over drawer replacing preview panel
- **Editor tab consolidation** — removing or de-emphasizing Edit Files tab
- **Tied-variant badges and counts** — requires variant identity seam
- **Backend heuristic engine** — Go triage engine unchanged; threshold is client-side
