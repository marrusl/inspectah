# Triage UX Fixes: Leaf Packages, Version Changes, SELinux Separation

**Status:** Proposed (revision 2)
**Date:** 2026-05-03
**Context:** Follow-up fixes discovered during live testing of the single-machine triage redesign.
**Revision notes:** Addresses round 1 review blockers: leaf-default state source, fleet gate, SELinux module truthfulness, browser interaction contracts, proof strategy.

## Problem

Three UX issues surfaced when running the triage redesign against a real Fedora 43 scan:

1. **Package noise.** The classifier creates triage items for all 510 `PackagesAdded` entries, but only 57 are leaf packages (user-installed). The other 453 are auto-pulled dependencies. Users see 510 "needs decision" cards when only 57 need attention.

2. **Missing version changes.** The Python renderer surfaced package version upgrades and downgrades. The Go port's triage UI does not. The scan data (`VersionChanges`) has 153 entries in a typical scan.

3. **SELinux drowning identity.** 365 SELinux booleans classified under the `identity` section alongside 7 users and groups. The section is unusable.

## Fix 1: Leaf-Only Packages with Dependency Drill-Down

### Leaf default state: source of truth

**Problem:** The scan sets `Include: false` on all `PackagesAdded` entries when `LeafPackages` is computed (Containerfile optimization — only leaf names go in `dnf install`). But leaf packages should render as "included by default" in the triage UI.

**Rejected approach (rev 1):** Mutating `pkg.Include` during classification. This breaks the snapshot/manifest/rerender contract because `SnapshotJSON` and `TRIAGE_MANIFEST` are produced separately, the browser reads state from the snapshot, and refine rerender recomputes the manifest independently.

**Approach (rev 2): Normalize at extraction time, before any rendering.**

When the refine server extracts a tarball, it runs a one-time normalization step on the snapshot BEFORE the first render:

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

**Where it runs:** In `RunRefine` (refine/server.go), after extracting the tarball and reading the snapshot, before writing the original sidecar and before the initial re-render. The original snapshot bytes are saved as the sidecar BEFORE normalization runs, so `DefaultInclude` reflects the scan's original intent.

For static HTML report generation (`RenderHTMLReport` without refine), the same normalization runs at the entry point before serialization and classification.

**Why this is safe:**
- Mutation happens once, explicitly, at the pipeline entry — not in the classifier.
- The snapshot JSON, triage manifest, and browser state all read the same normalized `Include` values.
- The original sidecar preserves the scan's raw values for `DefaultInclude` comparison.
- User changes persist to the normalized snapshot — re-renders don't clobber them because normalization only runs once at extraction.
- `isItemDecided` works correctly: on resume, if the user excluded a leaf, `current (false) !== default (true)` → decided → accordion shows "off". If the user never acted, `current (true) === default (true)` → undecided → accordion shows "on".

### Classifier change

When `!isFleet && snap.Rpm.LeafPackages != nil`, `classifyPackages` creates triage items **only for leaf packages**. Auto-dependency packages are excluded from the manifest entirely.

**Leaf detection:** Build a `leafSet map[string]bool` from `*snap.Rpm.LeafPackages`. In the `PackagesAdded` loop, skip any package whose name is not in `leafSet`.

**Fleet gate (blocker 2 fix):** The leaf-only filter is gated on `!isFleet`, not on `LeafPackages != nil` alone. Merged fleet snapshots carry `LeafPackages`, `AutoPackages`, and `LeafDepTree` from the merge engine. Without the fleet gate, leaf-only behavior would leak into fleet mode. The canonical single-machine predicate is `!isFleetSnapshot(snap)` — the same gate used by all other single-machine grouping in the classifier.

**Dependency data:** Each leaf's `TriageItem` gets a new field:

```go
Deps []string `json:"deps,omitempty"`
```

Populated from `snap.Rpm.LeafDepTree[pkg.Name]`. `LeafDepTree` is `map[string]interface{}` where each value is a `[]interface{}` of dependency name strings. The classifier normalizes this to `[]string` with explicit type assertion and nil/empty handling:

```go
func extractDeps(depTree map[string]interface{}, leafName string) []string {
    if depTree == nil {
        return nil
    }
    raw, ok := depTree[leafName]
    if !ok {
        return nil
    }
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

**Fallback:** When `LeafPackages` is nil (older scan format, no-baseline scans) OR when `isFleet` is true, all `PackagesAdded` render as before. The leaf-only filter and dep drill-down are single-machine features only.

### JS change — dependency drill-down

Each leaf package row in the expanded accordion table gets a dependency disclosure.

**DOM pattern:** The existing accordion table has rows `<tr>` with cells for checkbox, name, and meta. The dependency disclosure adds:

- In the **name cell** (`<td>`): after the package name text, a dep count badge (`<span class="dep-badge">12 deps</span>`) and a chevron button (`<button class="dep-chevron" aria-expanded="false" aria-controls="deps-{key}">`).
- When expanded, a **new `<tr class="dep-row">`** is inserted immediately after the leaf's row. This row spans all columns (`colspan`) and contains a `<ul aria-label="Dependencies for {name}">` with `<li>` elements for each dependency name. The `<ul>` preserves native list semantics (not `role="group"`).

**Interaction:**
- **Click/Enter/Space on chevron:** Toggles the dep row. Chevron `aria-expanded` toggles. Only one leaf's deps are expanded at a time within the accordion — expanding a second collapses the first. Tracked via `App.expandedDep` (string key of currently expanded leaf, or null).
- **Tab order:** Checkbox → package name (not focusable) → chevron button → (next row's checkbox). The chevron is a `<button>` element, naturally in tab order. Dep list items are NOT in the tab order — they are informational.
- **Focus after chevron toggle:** Focus stays on the chevron button. Since `renderTriageSection` does a full re-render, the `restoreFocus` helper is called with `targetType: 'dep-chevron'` and `targetId: item.key` to find the chevron in the new DOM.
- **Focus after accordion collapse/expand:** Dep expansion state resets when the parent accordion re-renders (expand/collapse clears `App.expandedDep` for that accordion). This is acceptable — dep disclosure is a convenience feature.
- **Screen reader:** Chevron label: `"Show 12 dependencies for vim-enhanced"` (when collapsed) / `"Hide dependencies for vim-enhanced"` (when expanded). Dep list announced as a list with item count.

**Zero deps:** If `item.deps` is null/empty/undefined, no chevron or badge is rendered.

**Static mode:** The chevron button is omitted (no interactive element). Instead, dep names are rendered inline as a comma-separated list in a `<span class="dep-list-static">` after the package name, styled muted. The dep count badge is also shown. This makes dependency information available in read-only reports, not just refine mode.

### Containerfile renderer interaction

No change needed. The renderer already uses `LeafPackages` when available and the TODO comment logic for unreachable packages checks `PackagesAdded` state independently.

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

**Tier 1** because these are informational — the base image already has the correct version. No user action needed.

**Display-only** — no toggle, no checkboxes. Renders as a `buildDisplayAccordion`.

**Single-machine only.** Fleet mode skips version changes.

### JS rendering and section accounting

The existing `buildDisplayAccordion` component renders the version changes accordion. However, the section footer and progress accounting need explicit handling:

**Decision accounting:** Version change items are display-only, so `isItemDecided` returns true when they are acknowledged (via `getSnapshotAcknowledged`). But version changes have no acknowledge button — they are passive informational surfaces. To prevent them from blocking section progress:

- The section footer's `X / Y decided` count **excludes** display-only grouped items that have no acknowledge path. The footer loop adds a filter: if `item.display_only && item.group` → skip from total and decided counts.
- The progress bar and sidebar dot calculations exclude these items for the same reason.
- This means the Packages section can reach "all decided" without the user interacting with version change accordions at all — which is correct, because version changes require no action.

## Fix 3: SELinux → System Section

### Classifier change

Move SELinux boolean, module, and port classification from `classifyIdentity` to `classifySystemItems`.

**In `classifyIdentity`:** Remove the three SELinux loops (sebools, semodules, seports). The function retains only users and groups.

**In `classifySystemItems`:** Add a new SELinux block with truthful surface types:

- `sebool-*` → `Group: "sub:selinux"`, tier 2, reason "SELinux boolean changed from default." **Output-affecting** — the renderer emits `setsebool` commands. Normal toggle behavior.
- `seport-*` → `Group: "sub:selinux"`, tier 2, reason "Custom SELinux port label." **Output-affecting** — the renderer emits `semanage port` commands. Normal toggle behavior.
- `semod-*` → `Group: "sub:selinux"`, tier 3, `CardType: "notification"`, reason "Custom SELinux policy module. The renderer cannot yet generate semodule installation commands — manual Containerfile steps required." **Not output-affecting as a normal toggle** — the current renderer emits FIXME/comment stubs (`# COPY ... # RUN semodule -i ...`), not executable commands. `getSnapshotInclude` is a no-op for `semod-*`. Treating these as notification cards is truthful to the current renderer contract.

This avoids the honesty problem from rev 1 where all SELinux items were claimed as output-affecting.

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
- **Leaf-only filtering:** When `!isFleet && LeafPackages != nil`, only leaf packages appear in manifest. When `isFleet && LeafPackages != nil`, ALL packages appear (fleet gate). When `LeafPackages == nil`, ALL packages appear (fallback).
- **Deps normalization:** `extractDeps` correctly handles: valid `LeafDepTree` entry → `[]string`; missing key → nil; wrong type → nil; empty array → nil; mixed types in array → strings only.
- **Version changes:** Items classified in `packages` section, tier 1, display-only, grouped by direction. Fleet mode → nil.
- **SELinux section:** `sebool-*` and `seport-*` in `system` section, output-affecting. `semod-*` in `system` section, `CardType: "notification"`. None in `identity` section.

### Rendered output tests (Go)
- **Golden HTML:** Rendered manifest contains `deps` array on leaf items. Version change items present as display-only grouped. SELinux items in `system` section.
- **Leaf normalization:** After `normalizeLeafDefaults`, leaf packages have `Include: true` in the snapshot. Non-leaf packages remain `Include: false`. Fleet snapshots are untouched.

### Browser contract tests (manual verification checklist)
Go tests cannot execute client-side JS or inspect DOM. The following are verified by manual browser testing:

- [ ] Leaf package rows show dep count badge and chevron
- [ ] Chevron click expands dep sub-list, second chevron collapses first
- [ ] Zero-dep leaves have no chevron or badge
- [ ] Static mode: dep names shown inline, no chevron
- [ ] Version changes accordion renders in Packages section
- [ ] Version changes do not affect section progress bar
- [ ] SELinux accordion appears in System & Security
- [ ] `semod-*` renders as notification card, not toggle
- [ ] Keyboard: Tab reaches chevron, Enter/Space toggles deps
- [ ] Focus returns to chevron after dep toggle re-render

## Out of Scope

- Crypto policy in the security section (future).
- Dep tree visualization (graph). Flat list is sufficient.
- Version lock suggestions from version changes (display-only, no output).
- Full renderer support for `semod-*` (current FIXME stubs are acknowledged as a limitation).
