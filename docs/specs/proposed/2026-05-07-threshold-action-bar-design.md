# Threshold Action Bar (revision 2)

## Summary

- **Status:** Proposed (revision 2)
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
| User lowers threshold | Items are above the new threshold but toggled off | "15 items across 3 sections are above your threshold but excluded." | [Include them] [Dismiss] |
| User raises threshold | Items are below the new threshold but toggled on | "8 items across 2 sections are below your threshold but included." | [Exclude them] [Dismiss] |

The message includes a section count ("across N sections") so the operator understands the cross-section scope of the mutation before clicking.

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
2. For each item with fleet data:
   a. Compute zone from fleet.count/fleet.total against threshold
   b. Read current include state from snapshot
   c. If mismatch (zone says include but toggle is off, or vice versa):
      - Record App.priorValues[key] = current include state
      - Record App.decisions[key] = true
      - Flip the include state in the snapshot
      - Add key to flipped-keys list
3. Increment change counter ONCE by flipped-keys.length
4. Invalidate all applicable sections (clear containers for re-render)
5. Update sidebar badges
6. Schedule autosave
```

**State bookkeeping rules:**

- `App.priorValues[key]`: Set for each flipped item to its pre-flip include state. This enables per-item undo via the existing toggle mechanism. If a prior value already exists (from an earlier manual toggle or previous bulk action), it is overwritten — the bulk action becomes the new restore point.
- `App.decisions[key]`: Set to `true` for each flipped item. This marks the item as "user has acted on this" for the decided-state logic (sidebar dots, review progress).
- `App.groupPriorState`: Not modified. Accordion group restore is unaffected — group toggles and threshold bulk actions are independent operations. If a user later toggles an accordion group that contains bulk-flipped items, the accordion's own prior-state snapshot captures the current (post-bulk) state, not the pre-bulk state.
- **Change counter:** Incremented once by the total count of flipped items, not once per item. This matches the user's mental model — one action, one count update. The `#changes-badge` shows "15 changes pending."
- **Autosave:** Scheduled once after all flips, not per item.
- **Section re-render:** All applicable sections are invalidated (containers cleared). They re-render on next navigation with the new toggle states.

### Interaction with existing controls

**Individual toggles after bulk action:** Work normally. The user can toggle any item back. `makeDecision()` overwrites `App.priorValues[key]` and `App.decisions[key]` for that item.

**Accordion group toggles after bulk action:** Work normally. `toggleAccordionGroup()` captures current state (post-bulk) as its restore point.

**Rebuild:** No special handling. Rebuild reads snapshot include state, same as after manual toggles.

**Threshold dropdown remains presentation-only.** The dropdown itself never mutates state. `applyThresholdSuggestion()` is a separate function triggered by the action button, not by the dropdown change event.

## 3. Focus and Accessibility Contract

### Core rule: focus stays on `<select>`

The threshold dropdown retains focus after a threshold change. The action bar appears without stealing focus. This preserves the approved Phase 1 behavior and allows keyboard users to iterate through presets without interruption.

### Announcement strategy

The action bar uses `aria-live="polite"` on its container. When it appears, screen readers announce the message text (e.g., "15 items across 3 sections are above your threshold but excluded"). The user can then Tab to the action button.

### Complete focus contract

| Event | Focus behavior |
|-------|---------------|
| Threshold change with mismatch | Focus stays on `<select>`. Bar appears and is announced via `aria-live`. |
| Threshold change with zero mismatch | Focus stays on `<select>`. No bar appears. |
| User Tabs from `<select>` | Focus moves to action button (if bar visible), then Dismiss, then next control. |
| User clicks Include/Exclude | Focus moves to `<select>` (bar disappears, return focus to the control that spawned it). |
| User clicks Dismiss | Focus moves to `<select>`. |
| Bar replaces on threshold re-change | Focus stays on `<select>` (bar content updates in place). |
| User navigates away from Overview | Bar removed from DOM. Focus moves to new section heading (existing `navigateTo` behavior). |

### Button accessibility

- Action button: `aria-label` includes count, sections, and threshold name. Example: `"Include 15 items across 3 sections above Strong consensus threshold"`.
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
| `action-bar-absent-static-mode` | In static mode, changing threshold shows no action bar. |
| `action-bar-navigation-dismisses` | Bar appears on Overview. Navigate to Packages. Return to Overview. Bar is gone (does not reappear without new threshold change). |
| `action-bar-focus-stays-on-select` | Change threshold. Verify `document.activeElement` is `#threshold-select`, not the action button. |

All tests follow the existing harness conventions: `resetServer()` in `beforeAll`/`afterAll`, `waitForBoot()` + `isRefineMode()` in `beforeEach`.

### Preserved Phase 1 invariants

The existing `fleet-threshold-no-dirty-state.spec.ts` continues to pass unchanged. That test changes the threshold via `select.selectOption()` without interacting with the action bar — proving that threshold changes alone remain zero-side-effect.

## 6. Deferred

- **Config prevalence:** Needs variant-stable identity (per-variant manifest keys, snapshot mutation lookups) before prevalence badges or action-bar flips are safe. Phase 2 variant comparison is the prerequisite.
- **Container prevalence:** Needs per-subtype specification. Container subtypes (quadlets, flatpaks, running containers, compose files) use different render paths, Fleet sources, and toggleability. Separate follow-up spec.
- **Non-RPM prevalence:** Different card pattern (review-status cards, not toggle cards). Separate follow-up.
