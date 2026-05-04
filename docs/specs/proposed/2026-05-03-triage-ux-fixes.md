# Triage UX Fixes: Leaf Packages, Version Changes, SELinux Separation

**Status:** Proposed
**Date:** 2026-05-03
**Context:** Follow-up fixes discovered during live testing of the single-machine triage redesign.

## Problem

Three UX issues surfaced when running the triage redesign against a real Fedora 43 scan:

1. **Package noise.** The classifier creates triage items for all 510 `PackagesAdded` entries, but only 57 are leaf packages (user-installed). The other 453 are auto-pulled dependencies. Users see 510 "needs decision" cards when only 57 need attention.

2. **Missing version changes.** The Python renderer surfaced package version upgrades and downgrades. The Go port's triage UI does not. The scan data (`VersionChanges`) has 153 entries in a typical scan.

3. **SELinux drowning identity.** 365 SELinux booleans classified under the `identity` section alongside 7 users and groups. The section is unusable.

## Fix 1: Leaf-Only Packages with Dependency Drill-Down

### Classifier change

When `snap.Rpm.LeafPackages` is non-nil, `classifyPackages` creates triage items **only for leaf packages**. Auto-dependency packages are excluded from the manifest entirely.

**Leaf detection:** Build a `leafSet map[string]bool` from `*snap.Rpm.LeafPackages`. In the `PackagesAdded` loop, skip any package whose name is not in `leafSet`.

**Dependency data:** Each leaf's `TriageItem` gets a new field:

```go
Deps []string `json:"deps,omitempty"`
```

Populated from `snap.Rpm.LeafDepTree[pkg.Name]`. The dep tree is `map[string]interface{}` where each key is a leaf name and the value is a list of dependency names.

**Fallback:** When `LeafPackages` is nil (older scan format, no-baseline scans), all `PackagesAdded` render as before. No behavior change for fleet mode (fleet doesn't use LeafPackages for triage).

**Include state:** When `LeafPackages` is populated, the classifier sets `DefaultInclude: true` on leaf package triage items, regardless of the snapshot's `pkg.Include` value. The scan sets `Include: false` on all `PackagesAdded` entries as a Containerfile optimization hint (only leaf names go in `dnf install`), but this should not be exposed as triage state — the user installed these packages and expects them included by default.

The accordion toggle reads `getSnapshotInclude(key)` which returns the snapshot's `pkg.include` value. To make toggles show "on" by default, the classifier must also set `pkg.Include = true` on leaf packages in the snapshot before rendering. This is a snapshot mutation during classification — acceptable because it only fires when `LeafPackages` is populated (single-machine mode) and only affects packages the user explicitly installed.

### JS change — dependency drill-down

Each leaf package row in an accordion gets:

- A **dep count badge** (e.g., `12 deps`) — static text, right-aligned.
- A **chevron** next to the badge — the interactive trigger.

**On click/Enter/Space:** The leaf row expands downward to show a sub-list of dependency names. Read-only (no checkboxes), muted styling (smaller font, left indent, no hover highlight). The chevron rotates to indicate open state.

**One-at-a-time:** Only one leaf's deps are expanded within an accordion at a time. Opening a second collapses the first.

**Keyboard:** Chevron is focusable via Tab. Enter/Space toggles. Arrow keys continue between leaf rows (deps are not in the tab order).

**Screen reader:** Chevron has `aria-expanded`, `aria-controls` pointing to the dep list ID, label `"Show N dependencies for <package>"`. Sub-list is `<ul role="group" aria-label="Dependencies">`.

**Zero deps:** If a leaf has no dependencies in `LeafDepTree`, hide the chevron and badge entirely.

**Static mode:** Chevron is omitted (no interactive element). The dep badge is still visible as informational text.

### Containerfile renderer interaction

No change needed. The renderer already uses `LeafPackages` when available — it installs only leaf packages via `dnf install`. The TODO comment logic for `local_install`/`no_repo` packages (from Fix 1 of the review round) continues to work because it checks `PackagesAdded` state regardless of leaf filtering.

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

**Single-machine only.** Fleet mode skips version changes (fleet snapshots aggregate across hosts where version deltas are different).

### JS rendering

The existing `buildDisplayAccordion` component handles this naturally. The accordion will render as "version-upgrades" with "N items — informational" subtitle. If downgrades exist, a separate "version-downgrades" accordion appears.

The `renderTriageSection` routing already sends `display_only` grouped items to `buildDisplayAccordion` — no JS change needed for basic rendering.

## Fix 3: SELinux → System Section

### Classifier change

Move SELinux boolean, module, and port classification from `classifyIdentity` to `classifySystemItems`.

**In `classifyIdentity`:** Remove the three SELinux loops (sebools, semodules, seports). The function retains only users and groups.

**In `classifySystemItems`:** Add a new SELinux block that creates items with:
- `sebool-*` → `Group: "sub:selinux"`, tier 2, reason "SELinux boolean changed from default."
- `semodule-*` → `Group: "sub:selinux"`, tier 3, reason "Custom SELinux policy module."
- `seport-*` → `Group: "sub:selinux"`, tier 2, reason "Custom SELinux port label."

All SELinux items are output-affecting (they generate `setsebool`/`semodule`/`semanage` lines in the Containerfile).

### Section label

The `MIGRATION_SECTIONS` array in `report.html` maps section IDs to labels. Update:
- `system` → "System & Security"

The `identity` section label stays as-is ("Identity") — SELinux is no longer there.

## Schema Change

One new field on `TriageItem`:

```go
Deps []string `json:"deps,omitempty"`
```

No other schema changes needed. `VersionChange` struct already exists.

## Test Strategy

- **Classifier tests:** Leaf-only filtering (verify 57 items when LeafPackages populated, 510 when nil). Version changes classified correctly. SELinux items appear in `system` section, not `identity`.
- **Golden HTML tests:** Rendered manifest contains dep data for leaf items. Version changes appear as display-only grouped items.
- **Containerfile tests:** Existing TODO/unreachable tests remain valid (they test PackagesAdded state, not leaf filtering).
- **Browser verification:** Dep chevron expands/collapses. Version changes accordion renders. SELinux accordion appears in System & Security.

## Out of Scope

- Crypto policy in the security section (future — Mark flagged for eventual placement).
- Dep tree visualization (tree view, graph). Flat list of dep names is sufficient.
- Version lock suggestions from version changes (display-only, no output).
- Renaming the identity section to "Identity & Security" — SELinux moved out, so the section is just identity now.
