# Threshold Action Bar + Prevalence Scope Expansion

## Summary

- **Status:** Proposed
- **Scope:** Expand prevalence features to config and container sections; add a threshold action bar that offers bulk include/exclude when the threshold classification disagrees with toggle state.
- **Depends on:** Fleet Prevalence Visibility Phase 1 (implemented)
- **Deferred:** Non-RPM prevalence (different card pattern, separate follow-up)

## 1. Prevalence Scope Expansion

### What changes

Phase 1 prevalence features (badges, threshold reclassification, review counts, sort, tier-group movement) currently apply to packages, runtime, identity, and system. This spec adds config and containers to the applicable set.

### Backend

Add `Fleet` propagation to two classify functions in `cmd/inspectah/internal/renderer/triage.go`:

- `classifyConfigFiles`: Add `Fleet: f.Fleet` to each `ConfigFileEntry` TriageItem.
- `classifyContainerItems`: Add `Fleet: <source>.Fleet` for typed structs and `Fleet: extractFleetFromMap(<map>)` for map-based items, following the same pattern as `classifySystemItems`.

### Frontend

Update `isApplicableForPrevalence()` in `cmd/inspectah/internal/renderer/static/report.html`:

```js
function isApplicableForPrevalence(sectionId) {
  return sectionId === 'packages' || sectionId === 'runtime' ||
         sectionId === 'identity' || sectionId === 'system' ||
         sectionId === 'config' || sectionId === 'containers';
}
```

No new UI components. The existing prevalence infrastructure (badges, threshold dropdown, review counts, sort, tier-group reclassification) handles config and container items once they are marked applicable.

### Config variant note

Config files with content variants (same path, different content across hosts) may have per-variant prevalence counts that don't sum to the fleet total. The badge shows the variant-level count (e.g., "2/3 hosts"). Phase 2 variant comparison handles the selection UX — this spec does not change variant behavior.

## 2. Threshold Action Bar

### What it is

A transient notification bar that appears on the Overview section when the user changes the prevalence threshold and a mismatch exists between the threshold classification and toggle state. It offers a one-click bulk operation to align toggle state with the classification.

### Placement

Below the threshold dropdown, above the stat cards grid, on the Overview section only.

### Two directions

| Scenario | Trigger | Message | Action |
|----------|---------|---------|--------|
| User lowers threshold | Items are above the new threshold but toggled off | "15 items are above your threshold but excluded." | [Include them] [Dismiss] |
| User raises threshold | Items are below the new threshold but toggled on | "8 items are below your threshold but included." | [Exclude them] [Dismiss] |

### Counting rules

- Count spans all applicable sections: packages, runtime, identity, system, config, containers.
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
6. **User clicks the action button:** All matching items flip their `include` state. The change counter increments by the number of flipped items. All affected sections re-render. The action bar disappears.
7. **User clicks Dismiss or navigates away from Overview:** The action bar disappears. No state change.

### No manual-toggle protection

The action bar affects all items matching the threshold criteria, regardless of whether they were toggled manually or by a previous action bar click. The operation is simple and predictable: every item that mismatches the threshold classification gets flipped.

### Interaction with existing controls

**Change counter:** The action bar uses the same state-mutation path as individual toggles. Each flipped item increments the change counter. "Include 15 items" → badge shows "15 changes pending."

**Rebuild:** No special handling. The action bar flips toggles. Rebuild reads toggle state and materializes the Containerfile. Same pipeline as manual decisions.

**Threshold dropdown remains presentation-only.** The dropdown itself never mutates state. The action bar is a separate control that offers a bulk operation informed by the threshold classification. Changing the threshold without clicking the action button has zero side effects beyond visual reclassification.

**Saved snapshots:** The action bar works on any fleet snapshot regardless of creation date. It reads the current toggle state and current threshold to compute the mismatch count.

**Static mode:** The action bar does not appear in static (read-only) reports, consistent with all other decision controls.

### Accessibility

- Action bar container: `role="alert"` so screen readers announce it on appearance.
- Action button: descriptive `aria-label` including count and threshold name (e.g., "Include 15 items above Strong consensus threshold").
- Dismiss button: `aria-label="Dismiss threshold suggestion"`.
- When the bar appears, focus moves to the action button.
- Both buttons are keyboard operable (Enter/Space).

## 3. What Does NOT Change

- Threshold dropdown behavior (presentation-only, no state mutation).
- Individual toggle behavior (manual toggles work as before).
- Rebuild pipeline (reads toggle state, generates Containerfile).
- Single-machine mode (no prevalence features, no action bar).
- Non-RPM section (excluded from prevalence, different card pattern, separate follow-up).
- Phase 2 variant comparison (config variant selection is a future feature).

## 4. Verification

### Backend tests

| Test | Assertion |
|------|-----------|
| `TestClassifySnapshot_ConfigFleetPropagation` | Config items with Fleet data have it propagated to TriageItem |
| `TestClassifySnapshot_ContainerFleetPropagation` | Container items with Fleet data have it propagated to TriageItem |

### Frontend verification

| Check | Assertion |
|-------|-----------|
| Action bar appears on threshold lower | Lowering from Unanimous to 80% with excluded above-threshold items shows the bar with correct count |
| Action bar appears on threshold raise | Raising from 50% to Unanimous with included below-threshold items shows the bar with correct count |
| Include action flips toggles | Clicking "Include them" sets matching items to `include=true`, increments change counter |
| Exclude action flips toggles | Clicking "Exclude them" sets matching items to `include=false`, increments change counter |
| Dismiss has no effect | Clicking Dismiss hides bar, no state change, change counter unchanged |
| Bar replaces on threshold re-change | Changing threshold again before acting replaces the bar with new counts |
| Zero mismatch = no bar | When all items already match the classification, no bar appears |
| Config and containers included | Prevalence badges and action bar counts include config and container items |
| Static mode = no bar | Action bar does not appear in static reports |
| Navigation dismisses | Navigating away from Overview hides the bar |
