# Triage UX Fixes: Leaf Packages, Version Changes, SELinux Separation

**Status:** Proposed (revision 3)
**Date:** 2026-05-03
**Context:** Follow-up fixes discovered during live testing of the single-machine triage redesign.
**Revision 2 notes:** Addresses round 1 blockers: leaf-default state source, fleet gate, SELinux module truthfulness, browser interaction contracts, proof strategy.
**Revision 3 notes:** Addresses round 2 blockers: sidecar timing (normalize BEFORE sidecar so baselines agree), `semod-*` notification copy/state/acknowledge persistence, version-change `updateBadge` exclusion, `LeafDepTree` dual-shape handling.

## Problem

Three UX issues surfaced when running the triage redesign against a real Fedora 43 scan:

1. **Package noise.** The classifier creates triage items for all 510 `PackagesAdded` entries, but only 57 are leaf packages (user-installed). The other 453 are auto-pulled dependencies. Users see 510 "needs decision" cards when only 57 need attention.

2. **Missing version changes.** The Python renderer surfaced package version upgrades and downgrades. The Go port's triage UI does not. The scan data (`VersionChanges`) has 153 entries in a typical scan.

3. **SELinux drowning identity.** 365 SELinux booleans classified under the `identity` section alongside 7 users and groups. The section is unusable.

## Fix 1: Leaf-Only Packages with Dependency Drill-Down

### Leaf default state: source of truth

**Problem:** The scan sets `Include: false` on all `PackagesAdded` entries when `LeafPackages` is computed (Containerfile optimization — only leaf names go in `dnf install`). But leaf packages should render as "included by default" in the triage UI.

**Rejected approach (rev 1):** Mutating `pkg.Include` during classification. This breaks the snapshot/manifest/rerender contract.

**Approach: Normalize at extraction time, before sidecar creation.**

When the refine server extracts a tarball, it runs a one-time normalization step on the snapshot. Critically, this happens BEFORE the sidecar is saved — so the sidecar and the working snapshot both start with `Include: true` for leaf packages. This makes all downstream comparisons agree.

```go
func normalizeLeafDefaults(snap *schema.InspectionSnapshot) {
    if snap.Rpm == nil || snap.Rpm.LeafPackages == nil || isFleetSnapshot(snap) {
        return
    }
    leafSet := make(map[string]bool)
    for _, name := range *snap.Rpm.LeafPackages {
        leafSet[name] = true
    }
    for i := range snap.Rpm.PackagesAdded {
        if leafSet[snap.Rpm.PackagesAdded[i].Name] {
            snap.Rpm.PackagesAdded[i].Include = true
        }
    }
}
```

**Pipeline sequence in `RunRefine`:**

1. Extract tarball → read snapshot bytes
2. Deserialize snapshot
3. Run `normalizeLeafDefaults(snap)` — leaf packages now have `Include: true`
4. Re-serialize normalized snapshot to bytes
5. Write sidecar from the NORMALIZED bytes (not the raw tarball bytes)
6. Write working snapshot
7. Proceed with initial re-render

**Why this ordering closes the round 1/2 blocker:**

The rev 2 approach saved the sidecar from raw tarball bytes (pre-normalization), then normalized. This meant `ClassifySnapshot(snap, original)` would compute `DefaultInclude` from the sidecar's `Include: false`, while the working snapshot had `Include: true`. Result: `current (true) !== default (false)` → all leaf packages read as "already decided" even though the user hadn't touched them.

By normalizing BEFORE saving the sidecar, both copies start with `Include: true` for leaves:
- `DefaultInclude = true` (from normalized sidecar)
- `getSnapshotInclude = true` (from normalized working snapshot)
- `isItemDecided`: `current (true) === default (true)` → NOT decided. Correct.
- User excludes a leaf → working snapshot `Include: false`, sidecar still `true` → `current (false) !== default (true)` → decided. Correct.
- Resume: working snapshot persisted with `false`, sidecar immutable at `true` → decided. Correct.

For static HTML report generation (`RenderHTMLReport` without refine), the same normalization runs at the entry point. No sidecar is involved — `OriginalSnapshot` is nil, and `DefaultInclude` comes directly from `pkg.Include` (which is now normalized to `true` for leaves).

### Classifier change

When `!isFleet && snap.Rpm.LeafPackages != nil`, `classifyPackages` creates triage items **only for leaf packages**. Auto-dependency packages are excluded from the manifest entirely.

**Leaf detection:** Build a `leafSet map[string]bool` from `*snap.Rpm.LeafPackages`. In the `PackagesAdded` loop, skip any package whose name is not in `leafSet`.

**Fleet gate:** The leaf-only filter is gated on `!isFleet`, not on `LeafPackages != nil` alone. Merged fleet snapshots carry `LeafPackages`, `AutoPackages`, and `LeafDepTree` from the merge engine. The canonical single-machine predicate is `!isFleetSnapshot(snap)`.

**Dependency data:** Each leaf's `TriageItem` gets a new field:

```go
Deps []string `json:"deps,omitempty"`
```

Populated from `snap.Rpm.LeafDepTree[pkg.Name]`. `LeafDepTree` is `map[string]interface{}` where values can be either `[]interface{}` (JSON-decoded from snapshot) or `[]string` (when the Go inspector builds it natively). The `extractDeps` helper handles both shapes:

```go
func extractDeps(depTree map[string]interface{}, leafName string) []string {
    if depTree == nil {
        return nil
    }
    raw, ok := depTree[leafName]
    if !ok || raw == nil {
        return nil
    }
    // Shape 1: []string (Go-native inspector path)
    if strSlice, ok := raw.([]string); ok {
        if len(strSlice) == 0 {
            return nil
        }
        return strSlice
    }
    // Shape 2: []interface{} (JSON-decoded path)
    arr, ok := raw.([]interface{})
    if !ok {
        return nil
    }
    deps := make([]string, 0, len(arr))
    for _, v := range arr {
        if s, ok := v.(string); ok {
            deps = append(deps, s)
        }
    }
    if len(deps) == 0 {
        return nil
    }
    return deps
}
```

**Fallback:** When `LeafPackages` is nil OR `isFleet` is true, all `PackagesAdded` render as before.

### JS change — dependency drill-down

Each leaf package row in the expanded accordion table gets a dependency disclosure.

**DOM pattern:** The existing accordion table has rows `<tr>` with cells for checkbox, name, and meta. The dependency disclosure adds:

- In the **name cell** (`<td>`): after the package name text, a dep count badge (`<span class="dep-badge">12 deps</span>`) and a chevron button (`<button class="dep-chevron" aria-expanded="false" aria-controls="deps-{key}">`).
- When expanded, a **new `<tr class="dep-row">`** is inserted immediately after the leaf's row. This row spans all columns (`colspan`) and contains a `<ul aria-label="Dependencies for {name}">` with `<li>` elements for each dependency name. The `<ul>` preserves native list semantics.

**Interaction:**
- **Click/Enter/Space on chevron:** Toggles the dep row. Chevron `aria-expanded` toggles. Only one leaf's deps are expanded at a time within the accordion — expanding a second collapses the first. Tracked via `App.expandedDep` (string key of currently expanded leaf, or null).
- **Tab order:** Checkbox → chevron button → (next row's checkbox). The chevron is a `<button>` element, naturally in tab order. Dep list items are NOT in the tab order.
- **Focus after chevron toggle:** `restoreFocus(sectionId, 'dep-chevron', item.key)` finds the chevron in the re-rendered DOM.
- **Focus after accordion collapse/expand:** Dep expansion state (`App.expandedDep`) resets when the parent accordion re-renders.
- **Screen reader:** Chevron label: `"Show 12 dependencies for vim-enhanced"` (collapsed) / `"Hide dependencies for vim-enhanced"` (expanded). Dep list announced as a list with item count.

**Zero deps:** If `item.deps` is null/empty/undefined, no chevron or badge is rendered.

**Static mode:** The chevron button is omitted. Dep names are rendered inline as a comma-separated list in a `<span class="dep-list-static">` after the package name, styled muted. The dep count badge is also shown.

### Containerfile renderer interaction

No change needed.

## Fix 2: Version Changes Accordion

### Classifier change

New function `classifyVersionChanges(snap, isFleet)` added to `classifyAll`. Creates display-only triage items in section `packages`.

```go
func classifyVersionChanges(snap *schema.InspectionSnapshot, isFleet bool) []TriageItem {
    if snap.Rpm == nil || len(snap.Rpm.VersionChanges) == 0 || isFleet {
        return nil
    }
    var items []TriageItem
    for _, vc := range snap.Rpm.VersionChanges {
        group := "sub:version-upgrades"
        if vc.Direction == schema.VersionChangeDowngrade {
            group = "sub:version-downgrades"
        }
        items = append(items, TriageItem{
            Section:     "packages",
            Key:         "verchg-" + vc.Name + "-" + vc.Arch,
            Tier:        1,
            Reason:      fmt.Sprintf("Package %s from %s to %s.", vc.Direction, vc.HostVersion, vc.BaseVersion),
            Name:        vc.Name,
            Meta:        vc.HostVersion + " → " + vc.BaseVersion,
            Group:       group,
            DisplayOnly: true,
        })
    }
    return items
}
```

**Tier 1**, display-only, single-machine only.

### JS rendering and section accounting

The existing `buildDisplayAccordion` component renders the version changes accordion. The section accounting needs exclusion at ALL three points:

**`isItemDecided`:** Already returns true for display-only items via `getSnapshotAcknowledged` (rev 2 fix). Version change items have no acknowledge path, so they would never become "decided" — blocking section progress indefinitely.

**Fix: Exclude passive display-only grouped items from ALL accounting paths.** A helper function identifies items that are passive (display-only, grouped, no acknowledge mechanism):

```javascript
function isPassiveItem(item) {
    return item.display_only && item.group &&
           item.key.indexOf('verchg-') === 0;
}
```

This function gates three accounting paths:

1. **Section footer** (`renderTriageSection`): The `X / Y decided` count skips items where `isPassiveItem(item)` is true.
2. **Sidebar badge** (`updateBadge`): The undecided counter skips `isPassiveItem` items. Without this, version change items would show as undecided tier-1 items forever, but since the badge only shows tier 2/3 counts, this is technically not a visible bug — however, excluding them is correct for consistency and future-proofing.
3. **Progress bar/sidebar dot** (`updateProgressBar`, `updateSidebarDot`): Calculations exclude `isPassiveItem` items from both total and decided counts.

This means the Packages section can reach "all decided" without interacting with version change accordions.

## Fix 3: SELinux → System Section

### Classifier change

Move SELinux boolean, module, and port classification from `classifyIdentity` to `classifySystemItems`.

**In `classifyIdentity`:** Remove the three SELinux loops (sebools, semodules, seports). The function retains only users and groups.

**In `classifySystemItems`:** Add a new SELinux block with truthful surface types:

- `sebool-*` → `Group: "sub:selinux"`, tier 2, reason "SELinux boolean changed from default." **Output-affecting** — the renderer emits `setsebool` commands. Normal toggle behavior.
- `seport-*` → `Group: "sub:selinux"`, tier 2, reason "Custom SELinux port label." **Output-affecting** — the renderer emits `semanage port` commands. Normal toggle behavior.
- `semod-*` → `Group: "sub:selinux"`, tier 3, `CardType: "notification"`. **Not output-affecting** — the current renderer emits FIXME/comment stubs, not executable commands.

### `semod-*` notification contract

The `semod-*` notification card uses SELinux-specific copy, not the package-oriented notification copy used for `local_install` packages:

- **Full card reason:** "Custom SELinux policy module. inspectah cannot yet generate semodule installation commands. To include this module in the image, add COPY and semodule -i steps to your Containerfile manually."
- **Collapsed card warning:** "custom module — manual Containerfile steps required"
- **Acknowledge button:** Same `acknowledgeNotification` path as other notification cards.

**Acknowledged persistence for `semod-*`:** The current JS has `semod-*` as a no-op in `getSnapshotInclude`/`updateSnapshotInclude` because custom modules are `[]string` with no per-item `include` field. The `getSnapshotAcknowledged`/`setSnapshotAcknowledged` helpers also lack a `semod-*` handler.

Fix: Add `semod-*` support to `getSnapshotAcknowledged`/`setSnapshotAcknowledged`. Since `selinux.custom_modules` is `[]string` (no typed struct with an `acknowledged` field), use the same session-only proxy as `group-*` identity items: `App.decisions[key]` tracks the acknowledged state. This means `semod-*` acknowledged state does NOT survive page reload — same limitation as identity groups, documented as a known constraint.

```javascript
// In getSnapshotAcknowledged:
if (key.indexOf('semod-') === 0) {
    return App.decisions[key] || false;
}

// In setSnapshotAcknowledged:
if (key.indexOf('semod-') === 0) {
    return; // Cannot persist — []string has no acknowledged field
}
```

### Section label

Update `MIGRATION_SECTIONS` in `report.html`:
- `system` → "System & Security"

The `identity` section label stays as-is ("Identity").

## Schema Change

One new field on `TriageItem`:

```go
Deps []string `json:"deps,omitempty"`
```

No other schema changes needed.

## Test Strategy

### Classifier tests (Go)
- **Leaf-only filtering:** `!isFleet && LeafPackages != nil` → only leaf packages. `isFleet && LeafPackages != nil` → ALL packages (fleet gate). `LeafPackages == nil` → ALL packages (fallback).
- **Deps normalization:** `extractDeps` handles: `[]interface{}` of strings → `[]string`; `[]string` (Go-native) → returned directly; nil value → nil; missing key → nil; wrong type → nil; empty array → nil; mixed types → strings only.
- **Version changes:** Classified in `packages` section, tier 1, display-only, grouped by direction. Fleet → nil.
- **SELinux section:** `sebool-*`/`seport-*` in `system`, output-affecting. `semod-*` in `system`, `CardType: "notification"`. None in `identity`.

### Rendered output tests (Go)
- **Golden HTML:** Manifest contains `deps` array on leaf items. Version changes present as display-only grouped. SELinux items in `system`.
- **Leaf normalization:** After `normalizeLeafDefaults`, leaf packages have `Include: true`. Non-leaf remain `Include: false`. Fleet untouched. Normalized sidecar and working snapshot agree on leaf `Include` values.

### Browser contract tests (manual verification checklist)
- [ ] Leaf package accordions show with toggles ON by default
- [ ] Leaf package rows show dep count badge and chevron
- [ ] Chevron click expands dep sub-list; second chevron collapses first
- [ ] Zero-dep leaves have no chevron or badge
- [ ] Static mode: dep names shown inline, no chevron
- [ ] Version changes accordion renders in Packages section
- [ ] Version changes do not affect section progress bar or sidebar badge
- [ ] SELinux accordion appears in System & Security
- [ ] `semod-*` renders as notification card with SELinux-specific copy, not package copy
- [ ] `semod-*` Acknowledge/undo works (session-only persistence)
- [ ] Keyboard: Tab reaches chevron, Enter/Space toggles deps
- [ ] Focus returns to chevron after dep toggle re-render

## Out of Scope

- Crypto policy in the security section (future).
- Dep tree visualization (graph). Flat list is sufficient.
- Version lock suggestions from version changes (display-only, no output).
- Full renderer support for `semod-*` (current FIXME stubs acknowledged as a limitation).
- Persistent acknowledged state for `semod-*` and `group-*` items (requires schema change to add per-item fields to `[]string` arrays — deferred).
