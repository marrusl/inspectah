# Threshold Action Bar (revision 3)

## Summary

- **Status:** Proposed (revision 4)
- **Scope:** Add a threshold action bar that offers bulk include/exclude when the prevalence threshold classification disagrees with toggle state. No scope expansion — applies to the existing 4 applicable sections (packages, runtime, identity, system).
- **Depends on:** Fleet Prevalence Visibility Phase 1 (implemented)
- **Deferred:** Config prevalence (needs variant-stable identity, Phase 2 dependency), container prevalence (needs per-subtype specification and subsection renderer work), non-RPM prevalence (different card pattern)

**Revision 2 changes from round 1 review:**
- Removed config and containers scope expansion (Thorn #1, Kit blocker, Thorn should-fix #3)
- Added explicit batch-state contract with `applyThresholdSuggestion()` helper (Thorn #2, Kit high)
- Fixed focus contract: focus stays on `<select>`, bar announced via `aria-live` (Fern #1, Kit high, Thorn should-fix #1)
- Added complete focus contract for all scenarios (Fern #1)
- Added section-level disclosure in action bar message (Fern #2)
- Replaced manual verification with required automated tests (Thorn should-fix #2, Kit medium)

**Revision 3 changes from round 2 review:**
- Fixed `priorValues` overwrite: preserve first-touch restore point, do not overwrite (Kit blocker, Thorn #1)
- Added reviewed-section reopening after bulk apply (Kit blocker, Thorn #1)
- Fixed dirty-count: only increment for items not already in `App.decisions` (Thorn #1)
- Named affected sections in bar message, not just count (Fern #1)
- Complete screen-reader announcement contract for appearance/replacement/dismiss/apply (Fern #2)
- Added pre-dirtied row E2E test case (Thorn proof note)
- Demoted static-mode E2E test to manual (no harness seam) (Thorn proof note)

**Revision 4 changes from round 3 review:**
- Harmonize `makeDecision()` to first-touch `priorValues` semantics — implementation must add `if (App.priorValues[key] === undefined)` guard (Kit finding, Thorn #1)
- Updated "Individual toggles after bulk action" to remove contradictory overwrite language
- Pre-dirtied-row E2E test now requires coverage of BOTH toggle-card and triage-card mutation paths (Thorn should-fix #1)
- Added non-blocking dwell-state note: buttons disabled during post-action confirmation pause (Fern note)

## 1. Threshold Action Bar

### What it is

A transient notification bar that appears on the Overview section when the user changes the prevalence threshold and a mismatch exists between the threshold classification and toggle state. It offers a one-click bulk operation to align toggle state with the classification.

### Applicable sections

Packages, runtime, identity, system — the same 4 sections that already have prevalence from Phase 1. No changes to `isApplicableForPrevalence()`.

### Placement

Below the threshold dropdown, above the stat cards grid, on the Overview section only.

### Two directions

| Scenario | Trigger | Message | Action |
|----------|---------|---------|--------|
| User lowers threshold | Items are above the new threshold but toggled off | "15 items above threshold but excluded: Packages (9), Runtime (4), Identity (2)" | [Include them] [Dismiss] |
| User raises threshold | Items are below the new threshold but toggled on | "8 items below threshold but included: Packages (5), System (3)" | [Exclude them] [Dismiss] |

The message names every affected section with its per-section count so the operator knows exactly what the bulk action will touch before clicking. Sections with zero mismatched items are omitted from the list.

### Counting rules

- Count spans all applicable sections: packages, runtime, identity, system.
- Only counts items where classification and toggle state disagree:
  - Above-threshold (zone is `"above"` or `"unanimous"`) AND `include === false` → offer to include
  - Below-threshold (zone is `"below"`) AND `include === true` → offer to exclude
- Items with no fleet data are ignored.
- If the mismatch count is zero, no action bar appears.

### Lifecycle

1. User changes the threshold dropdown.
2. Visual reclassification happens instantly: badges re-color, items move between tier groups, headers update review counts, sidebar badges update. This is the existing Phase 1 behavior — no change.
3. After reclassification, the action bar computes the mismatch count across all applicable sections.
4. If mismatch count > 0, the action bar appears below the dropdown.
5. If the user changes the threshold again before acting, the bar replaces itself with updated counts and direction.
6. **User clicks the action button:** Calls `applyThresholdSuggestion()` (see Batch State Contract below). The action bar disappears.
7. **User clicks Dismiss:** The action bar disappears. No state change.
8. **User navigates away from Overview:** The action bar is removed from the DOM. No state change. If the user returns to Overview, the bar does not reappear unless they change the threshold again.

### No manual-toggle protection

The action bar affects all items matching the threshold criteria, regardless of whether they were toggled manually or by a previous action bar click. The operation is simple and predictable: every item that mismatches the threshold classification gets flipped.

## 2. Batch State Contract

The action bar introduces a new bulk mutation pattern. This section defines exactly how it interacts with the existing client state model.

### The `applyThresholdSuggestion()` helper

A new function that performs the bulk flip. It does not reuse `makeDecision()` in a loop — it is a dedicated bulk path with its own state semantics.

**Input:** direction (`"include"` or `"exclude"`), current threshold value.

**Algorithm:**

```
1. Collect all triage manifest items in applicable sections
2. Track affected sections: affectedSections = new Set()
3. Track newly-decided count: newDecisionCount = 0
4. For each item with fleet data:
   a. Compute zone from fleet.count/fleet.total against threshold
   b. Read current include state from snapshot via getSnapshotInclude(key)
   c. If mismatch (zone says include but toggle is off, or vice versa):
      - If App.priorValues[key] is undefined, seed it: App.priorValues[key] = current include
        (Do NOT overwrite existing priorValues — the first touch is the real restore point)
      - If App.decisions[key] is not already true, increment newDecisionCount
      - Set App.decisions[key] = true
      - Flip the include state in the snapshot via updateSnapshotInclude(key, newValue)
      - Add item.section to affectedSections
5. Increment change counter ONCE by newDecisionCount
   (Items already in App.decisions were already counted — do not double-count)
6. For each section in affectedSections:
   a. Clear section container (invalidate for re-render)
   b. Reset App.reviewStates[sectionId] = 'unreviewed'
   c. Update sidebar dot via updateSidebarDot(sectionId)
7. Update sidebar badges via updateAllBadges()
8. Update progress bar via updateProgressBar()
9. Schedule autosave
```

**State bookkeeping rules:**

- `App.priorValues[key]`: Only seeded when undefined. If the item was already manually toggled (via either toggle-card or triage-card path), the existing prior value is preserved as the restore point. This is consistent across all mutation paths after the required `makeDecision()` harmonization (see above). Per-item undo always returns to the first-touch state, whether the first touch was a manual toggle, a triage-card decision, or a bulk action.
- `App.decisions[key]`: Set to `true` for all flipped items, regardless of prior state. This marks every affected item as "user has acted" for decided-state rendering.
- `App.groupPriorState`: Not modified. Accordion group restore is unaffected — group toggles and threshold bulk actions are independent operations. If a user later toggles an accordion group that contains bulk-flipped items, the accordion's own prior-state snapshot captures the current (post-bulk) state.
- **Change counter:** Incremented once by `newDecisionCount` — the number of items that were NOT already in `App.decisions`. Items already decided were already counted by their original toggle, so re-flipping them does not add to the counter. This keeps `#changes-badge` honest relative to the actual number of user actions.
- **Reviewed-section reopening:** Every affected section has its `App.reviewStates` reset to `'unreviewed'` and its sidebar dot updated. This ensures review progress and sidebar dots stay honest after a cross-section bulk mutation. The progress bar updates to reflect the reopened sections.
- **Autosave:** Scheduled once after all flips, not per item.
- **Section re-render:** Affected sections (not all applicable sections) are invalidated. They re-render on next navigation with the new toggle states.

### Required implementation fix: harmonize `makeDecision()` priorValues

The current `makeDecision()` in `report.html` overwrites `App.priorValues[key]` unconditionally. Toggle-card inline handlers only seed it when undefined. This creates a split contract where first-touch semantics hold on one path but not the other.

**This spec requires harmonizing `makeDecision()` to first-touch semantics** by adding an `if (App.priorValues[key] === undefined)` guard before seeding. This is a one-line change that makes the restore contract consistent across all per-item mutation paths:

```js
// Current (overwrites):
App.priorValues[key] = getSnapshotInclude(key);

// Required (first-touch):
if (App.priorValues[key] === undefined) {
    App.priorValues[key] = getSnapshotInclude(key);
}
```

After this change, per-item undo always returns to the original pre-first-touch state, whether the first touch was a manual toggle, a triage-card decision, or a bulk action. This is a prerequisite for the action bar, not an optional cleanup.

### Interaction with existing controls

**Individual toggles after bulk action:** Work normally. The user can toggle any item back. Both toggle-card and triage-card paths now use first-touch `priorValues` semantics (see above). Undo returns to the original state before any user action on that item.

**Accordion group toggles after bulk action:** Work normally. `toggleAccordionGroup()` captures current state (post-bulk) as its restore point.

**Rebuild:** No special handling. Rebuild reads snapshot include state, same as after manual toggles.

**Threshold dropdown remains presentation-only.** The dropdown itself never mutates state. `applyThresholdSuggestion()` is a separate function triggered by the action button, not by the dropdown change event.

## 3. Focus and Accessibility Contract

### Core rule: focus stays on `<select>`

The threshold dropdown retains focus after a threshold change. The action bar appears without stealing focus. This preserves the approved Phase 1 behavior and allows keyboard users to iterate through presets without interruption.

### Announcement architecture

The action bar contains a **message-only text node** inside an `aria-live="polite"` container. The interactive buttons are siblings of the live region, not children of it. This ensures screen readers announce the message text without reading button labels as part of the live-region update.

```
<div class="threshold-action-bar">
  <span aria-live="polite" id="threshold-suggestion-text">
    15 items above threshold but excluded: Packages (9), Runtime (4), Identity (2)
  </span>
  <button>[Include them]</button>
  <button>[Dismiss]</button>
</div>
```

### Screen-reader announcement contract

| Event | Announcement |
|-------|-------------|
| Bar appears (threshold change with mismatch) | Live region text updates → SR announces: "15 items above threshold but excluded: Packages (9), Runtime (4), Identity (2)" |
| Bar replaces (threshold re-change before action) | Live region text updates with new content → SR announces the new message |
| User clicks Include/Exclude | Live region text updates to: "Included 15 items" or "Excluded 8 items". Buttons are disabled during confirmation. Bar disappears after a brief pause (~1s) to allow the announcement to complete. |
| User clicks Dismiss | Live region text updates to: "Suggestion dismissed". Buttons are disabled during confirmation. Bar disappears after a brief pause. |
| Threshold change with zero mismatch | If bar was visible, live region text clears → bar disappears. If bar was not visible, no announcement. |
| Threshold dropdown value change | Native `<select>` announces its new value via standard browser behavior. The action bar announcement is additive — it follows the select announcement. |

### Complete focus contract

| Event | Focus behavior |
|-------|---------------|
| Threshold change with mismatch | Focus stays on `<select>`. Bar appears, message announced via `aria-live`. |
| Threshold change with zero mismatch | Focus stays on `<select>`. No bar appears. |
| User Tabs from `<select>` | Focus moves to action button (if bar visible), then Dismiss, then next control. |
| User clicks Include/Exclude | Focus moves to `<select>` (bar disappears after confirmation announcement). |
| User clicks Dismiss | Focus moves to `<select>` (bar disappears after dismissal announcement). |
| Bar replaces on threshold re-change | Focus stays on `<select>` (bar content updates in place, new message announced). |
| User navigates away from Overview | Bar removed from DOM. Focus moves to new section heading (existing `navigateTo` behavior). |

### Button accessibility

- Action button: `aria-label` includes count, section breakdown, and threshold name. Example: `"Include 15 items: Packages 9, Runtime 4, Identity 2, above Strong consensus threshold"`.
- Dismiss button: `aria-label="Dismiss threshold suggestion"`.
- Both buttons: keyboard operable (Enter/Space), visible focus ring.

### Static mode

Action bar does not appear in static (read-only) reports. Guarded by `App.mode !== 'static'`, consistent with all other decision controls.

## 4. What Does NOT Change

- `isApplicableForPrevalence()` — stays as packages, runtime, identity, system
- Threshold dropdown behavior — presentation-only, no state mutation
- Individual toggle behavior — `makeDecision()` works as before
- Accordion group behavior — `toggleAccordionGroup()` works as before
- Rebuild pipeline — reads toggle state, generates Containerfile
- Single-machine mode — no prevalence features, no action bar
- Phase 1 E2E test `fleet-threshold-no-dirty-state` — threshold changes without clicking the action button remain zero-side-effect (the test's `select.selectOption()` does not click the action button)

## 5. Verification

### Required automated tests

#### Renderer unit tests (`cmd/inspectah/internal/renderer/triage_test.go`)

No new renderer tests needed — the action bar is purely frontend. The Phase 1 Fleet propagation tests already cover the applicable sections.

#### Playwright E2E tests (`tests/e2e-go/tests/`)

Create `tests/e2e-go/tests/fleet-threshold-action-bar.spec.ts`:

| Test | Assertion |
|------|-----------|
| `action-bar-appears-on-threshold-lower` | Lower from Unanimous to 80%. Bar appears with correct count. `#changes-badge` stays hidden (no state change yet). |
| `action-bar-appears-on-threshold-raise` | From 50% threshold with included below-threshold items, raise to Unanimous. Bar appears with correct count and "Exclude" direction. |
| `action-bar-include-flips-toggles` | Lower threshold, click Include button. `#changes-badge` becomes visible with correct count. Snapshot include values change for matching items. Focus returns to `<select>`. |
| `action-bar-exclude-flips-toggles` | Raise threshold, click Exclude button. Same assertions as include but opposite direction. |
| `action-bar-dismiss-no-state-change` | Bar appears, click Dismiss. `#changes-badge` stays hidden. Snapshot unchanged. Focus returns to `<select>`. |
| `action-bar-replaces-on-rechange` | Lower to 80% (bar appears). Lower to 50% (bar replaces with new count). Focus stays on `<select>`. No state change. |
| `action-bar-zero-mismatch-no-bar` | Set threshold where all items match classification. No bar appears. |
| `action-bar-navigation-dismisses` | Bar appears on Overview. Navigate to Packages. Return to Overview. Bar is gone (does not reappear without new threshold change). |
| `action-bar-focus-stays-on-select` | Change threshold. Verify `document.activeElement` is `#threshold-select`, not the action button. |
| `action-bar-pre-dirtied-toggle-card` | Dirty a toggle-card item (inline toggle click). Change threshold so action bar includes that item. Apply. Verify: (1) `priorValues[key]` holds original pre-toggle value (not overwritten), (2) `#changes-badge` count increments only by newly-decided items, (3) undo returns to original state. |
| `action-bar-pre-dirtied-triage-card` | Dirty a triage-card item (via `makeDecision()` — e.g., a tier-3 flagged item's decision button). Change threshold so action bar includes that item. Apply. Verify same three assertions as above. This test exercises the `makeDecision()` path specifically, which is the path that required harmonization to first-touch semantics. |
| `action-bar-sections-reopen` | Mark a section as reviewed (sidebar dot green). Change threshold and apply the action bar so it flips items in that section. Verify the section's review state resets to unreviewed (dot reverts, progress bar decrements). |

All tests follow the existing harness conventions: `resetServer()` in `beforeAll`/`afterAll`, `waitForBoot()` + `isRefineMode()` in `beforeEach`.

### Manual verification (no automated harness seam)

| Check | Assertion |
|-------|-----------|
| Static mode: no bar | Open a fleet report as a static file (`file://` protocol). Change threshold. Verify no action bar appears. (No Playwright harness for static-mode refine in the current test infrastructure.) |

### Preserved Phase 1 invariants

The existing `fleet-threshold-no-dirty-state.spec.ts` continues to pass unchanged. That test changes the threshold via `select.selectOption()` without interacting with the action bar — proving that threshold changes alone remain zero-side-effect.

## 6. Deferred

- **Config prevalence:** Needs variant-stable identity (per-variant manifest keys, snapshot mutation lookups) before prevalence badges or action-bar flips are safe. Phase 2 variant comparison is the prerequisite.
- **Container prevalence:** Needs per-subtype specification. Container subtypes (quadlets, flatpaks, running containers, compose files) use different render paths, Fleet sources, and toggleability. Separate follow-up spec.
- **Non-RPM prevalence:** Different card pattern (review-status cards, not toggle cards). Separate follow-up.
