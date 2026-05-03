# Single-Machine Triage Redesign

*Revision 2 — addresses round 1 review feedback from Collins, Fern, and Thorn.*

## Summary

Redesign the single-machine refine mode triage experience around an "assume blessed" mental model. Everything different from the base image is auto-included by default. The admin verifies and opts out of exceptions rather than clicking through hundreds of individual decision cards.

**Problem:** On a single-machine scan, the Packages section produces 300-400+ tier-2 decision cards — one per non-base-image package. Each requires an individual Include/Leave out click. A Fedora workstation with 435 packages is unworkable to triage.

**Solution:** Keep the existing 3-tier classification system (it's semantically correct). Add a display grouping layer that collapses tier-2 items into accordion cards grouped by natural unit (repository for packages, kind for configs, subsystem for system items). Items requiring genuine attention stay as individual cards.

**Scope:** Single-machine refine mode only. Fleet mode is unaffected.

### Surface ownership contract

This spec respects the output-contract boundaries established in the approved HTML report redesign. Every triage surface falls into exactly one of two categories:

1. **Output-affecting** — toggling this item changes the Containerfile, kickstart, or other generated artifacts. Uses Include/Exclude language and toggle switches.
2. **Display-only** — the item is surfaced for admin awareness but does not affect generated output in v1. Uses Acknowledge/Skip language. No toggle switches.

The category is determined by whether the current renderer (`containerfile.go`, `kickstart.go`) has an actionable output path for the item type. If it does not, the surface is display-only regardless of how it is grouped.

## Architecture

### Approach: Keep tiers, add grouping layer

The Go tier classifier (`triage.go`) continues to assign tier 1/2/3 based on the same signals. New fields on `TriageItem` tell the JS how to render each item. Items with a non-empty `Group` render as accordion members; items with an empty `Group` render as individual cards. The display logic lives in `report.html`; the data model changes are additive.

### Single-machine detection

The classifier needs a stable predicate for single-machine vs fleet mode. The canonical rule: a snapshot is single-machine if and only if `snap.FleetMetadata == nil`. `FleetMetadata` is populated by the fleet merge engine and contains prevalence data, source snapshot IDs, and merge parameters. Its absence means the snapshot was produced by a direct host scan, not an aggregation.

When single-machine: the classifier populates `Group` fields and sets `CardType` where applicable.
When fleet: `Group` stays empty and the current card-per-item behavior is preserved.

### Card types

Four card types in the redesigned triage:

1. **Output-affecting accordion** — grouped items with a toggle switch. For surfaces where Include/Exclude changes the generated Containerfile. Collapsed shows group name, item count, and a toggle. Expand shows a drill-down table with per-item checkboxes.

2. **Display-only accordion** — grouped items for admin review. For surfaces with no v1 output path. Collapsed shows group name and item count. No toggle switch. Expand shows a read-only drill-down table. Items use Acknowledge/Skip language if individual actions exist.

3. **Decision card** — individual items needing explicit Include/Leave out or Acknowledge/Skip choice. Same structural design as current tier-2/tier-3 cards, with language matching the surface ownership.

4. **Notification card** — items the tool cannot act on (no-repo packages, non-RPM binaries). Single "Acknowledge" button. Collapses to a one-line summary showing item name + warning text + undo. The Containerfile renderer emits a TODO comment.

## Section-by-Section Design

### Packages

**Tier 3 — Flagged:**
- Locally-installed packages (no repo): **Notification cards.** "No repository source available. inspectah cannot reconstruct installation steps for this package. You will need to provide a reproducible build-time source (vendor RPM, download script, or equivalent) and add the installation to your Containerfile manually." Action: "Acknowledge" button. Containerfile output: TODO comment (see Notification Card Specification).
- Kernel modules from non-standard sources: **Decision cards** (Include/Leave out).

**Tier 2 — Grouped by repository (output-affecting accordions):**
- Each repository gets an **output-affecting accordion card** showing: repo name, package count, and a toggle switch (on by default).
- **BaseOS repo:** Always included, no toggle switch. Shows "always included" label. Rationale: BaseOS packages are core system libraries; accidental exclusion risks breaking the system.
- **Standard repos (AppStream, CRB, etc.):** Toggle switch, auto-included. Admin can expand to de-select individual packages via checkboxes.
- **Third-party repos (EPEL, vendor repos):** Toggle switch, auto-included. Header carries a "third-party" badge (yellow pill) to distinguish from standard repos.
- **Module streams:** **Individual decision cards.** Stream choice affects the entire dependency tree and warrants explicit confirmation. Reasoning: "Module stream package. Stream choice affects dependency tree. Verify compatibility."
- **`dnf-makecache`, `packagekit`:** If present as services, classified as tier 3 in the Runtime section with image-mode incompatibility warning. Extensible — new incompatible services can be added to the classifier list.

**Tier 1 — Auto-included (base image match):**
- Unchanged from current behavior. Collapsed count summary: "312 packages match the base image and are included automatically." Expandable to review or override.

### Config

**RPM-owned-default / baseline match:** **Output-affecting accordion** grouped as "Unchanged configs." Auto-included. These match the base image content — safe to collapse.

**RPM-owned-modified:** **Individual decision cards (output-affecting).** Each card includes 3-way merge guidance: "This file diverges from the RPM default. Including it means you own it through future image upgrades — upstream default changes will not automatically apply. Consider whether a systemd drop-in or runtime config management is more appropriate." Rationale: bootc's `/etc` 3-way merge operates at file level. If the admin bakes a modified config into the image, it becomes the image default. On upgrade, if the admin hasn't further modified it locally, the new upstream default replaces it silently. If they have modified it locally, the local version wins wholesale and upstream changes are lost. This is a per-file ownership decision the admin must make consciously.

**Systemd drop-ins:** **Output-affecting accordion** grouped as "Systemd drop-ins." Auto-included. Drop-ins are the correct image-mode pattern — they layer on top of upstream defaults cleanly.

**Custom/untracked files:** **Individual decision cards (output-affecting).** Users need to uncheck extraneous files specifically.

### Runtime

**Services in default state:** **Output-affecting accordion** grouped as "Services (default state)." Auto-included. These match base image defaults — no migration consequence.

**Services with state changes:** **Output-affecting accordion** grouped as "Services (state changed)." Auto-included. Each row shows: unit name, default state → current state, owning package. Admin can de-select individual services.

**Image-mode incompatible services:** (`dnf-makecache`, `packagekit`, and future additions) **Individual decision cards**, tier 3, with warning: "This service assumes package management at runtime, which is unavailable in image mode. Consider disabling or removing it from the image." Extensible list in the classifier.

**Cron jobs:** **Output-affecting accordion** grouped as "Cron jobs." Auto-included.

**Systemd timers:** **Output-affecting accordion** grouped as "Systemd timers." Auto-included.

### Containers

**Quadlet units:** **Output-affecting accordion** grouped as "Quadlet units." Auto-included. Quadlets are the target deployment pattern for image mode — they're declarative and portable.

**Running containers without quadlet backing:** **Individual display-only cards** using Acknowledge/Skip. These are live runtime state, not image content. The renderer does not produce Containerfile output for running containers. Warning: "Running container without quadlet backing. This is runtime state — it will not be reproduced in the image. Consider converting to a Quadlet unit for image-mode compatibility."

**Non-RPM binaries:** **Individual notification cards.** "inspectah cannot determine the provenance or installation method for this binary. To include it in the image, provide a reproducible build-time source and add it to your Containerfile." Containerfile output: TODO comment.

### Identity

**No accordion grouping.** All items get **individual cards.** User migration strategies are not implemented in this version but are planned.

- **System users (UID < 1000):** Tier 1 decision cards (output-affecting). These come from the base image via RPM packages. Still surfaced individually because the admin should verify they're not custom system users masquerading in the system UID range.
- **User-created accounts (UID >= 1000):** Tier 2 decision cards (output-affecting). Must be provisioned differently in image mode — through deploy-time provisioning (Ignition, cloud-init), external identity management, or deliberate image build-time creation.
- **Groups:** Tier 2 **display-only cards** using Acknowledge/Skip. The renderer does not produce group-specific Containerfile output in v1.
- **SELinux booleans:** Tier 2 decision cards (output-affecting). Straightforward Containerfile additions.
- **Custom SELinux modules:** Tier 3 decision cards (output-affecting). Can conflict with image updates, hard to debug.
- **SELinux port labels:** Tier 2 decision cards (output-affecting).

### System

**Sysctl overrides:** **Output-affecting accordion**, auto-included. Drill-down: parameter name, runtime value.

**Kernel modules:** **Output-affecting accordion**, auto-included. Drill-down: module name, used-by.

**Network connections:** **Display-only accordion.** No toggle switch. The renderer does not produce per-connection Containerfile output — NM connection files are included via directory-level `COPY config/etc/`. Grouped for density reduction; individual items use Acknowledge/Skip if the admin wants to record their review. Drill-down: connection name, type.

**Firewall zones:** **Output-affecting accordion**, auto-included. Drill-down: zone name.

**Fstab entries:** **Display-only.** The renderer produces advisory comments only, not actionable Containerfile lines for mount points. Entries touching `/`, `/boot`, `/var`, `/sysroot`, `/usr`, or `/etc`, or using unstable device paths: **individual display-only cards** using Acknowledge/Skip, with guidance noting the mount interacts with bootc's filesystem model and should be handled at deploy time. Remaining entries: **display-only accordion** for informational review.

### Secrets

**Unchanged.** All secrets items are tier 3 with individual cards. The existing behavior is correct for secrets — every finding needs explicit review.

## Output-Affecting Accordion Specification

### Grouped selection state model

An output-affecting accordion has three states derived from its member items' `include` values:

| State | Condition | Toggle visual | Collapsed summary |
|-------|-----------|--------------|-------------------|
| **All included** | Every item has `include=true` | ON (green) | "N packages" |
| **Partially excluded** | At least one item has `include=false`, but not all | ON (green) | "N of M included" |
| **All excluded** | Every item has `include=false` | OFF (gray, dimmed card) | "N packages, excluded" |

The toggle switch reflects the group state, not a separate boolean. There is no independent "group on/off" state — the toggle is a derived view of the member items.

### Toggle behavior

**Toggle OFF (from all-included or partially-excluded):** Sets `include=false` on every item in the group. The prior per-item `include` values are saved in `App.groupPriorState[groupKey]` (in-memory JS map, keyed by group name).

**Toggle ON (from all-excluded):** Restores prior per-item `include` values from `App.groupPriorState[groupKey]`. If no prior state exists (first interaction), sets all items to `include=true`. This preserves row-level exceptions across an off/on cycle — toggling a group off and back on does not destroy per-item work.

**Row controls while group is off:** Disabled. Per-item checkboxes are grayed and non-interactive when the group toggle is OFF. The admin must re-enable the group toggle first, then adjust individual items. Rationale: allowing row edits while the group is off creates ambiguous state (is this item "excluded because the group is off" or "excluded individually"?).

### Expand/collapse

- Collapsed by default in refine mode
- Click header (outside toggle) to expand/collapse
- Chevron indicates state: ▶ collapsed, ▼ expanded
- Expand/collapse state is in-memory (`App.tierExpanded`), not persisted

### Keyboard and accessibility

- Accordion header: focusable, Enter/Space toggles expand/collapse
- Toggle switch: separate tab stop within header, Enter/Space toggles, `role="switch"` with `aria-checked`
- Screen reader announcement for collapsed states:
  - All included: "EPEL 9, third-party repository, 23 packages, included, collapsed"
  - Partially excluded: "EPEL 9, third-party repository, 20 of 23 included, collapsed"
  - All excluded: "EPEL 9, third-party repository, 23 packages, excluded, collapsed"
- Expanded table: standard table navigation, checkboxes focusable via Tab
- BaseOS: no toggle tab stop (no switch element exists)

## Display-Only Accordion Specification

Same visual structure as output-affecting accordion, with these differences:

- **No toggle switch.** The header shows only the group name, item count, and expand chevron.
- **No per-item checkboxes.** Drill-down table is read-only.
- **Optional per-item Acknowledge/Skip.** If individual review is meaningful (e.g., network connections), rows can have Acknowledge/Skip buttons. Acknowledged items collapse to a one-line summary within the table.
- **Collapsed summary:** "N connections" / "N entries" — no included/excluded language.
- Screen reader: "Network connections, 4 connections, informational, collapsed"

## Notification Card Specification

### Expanded state

```
┌─────────────────────────────────────────────────────────┐
│ ┌─ No repository source available.                    ┐ │
│ │  inspectah cannot reconstruct installation steps.   │ │
│                                                         │
│ custom-monitoring-agent                                 │
│ 1.4.2-1 | x86_64 | (no repo)                          │
│                                                         │
│ Provide a reproducible build-time source and add the    │
│ installation to your Containerfile manually.            │
│                                                         │
│ [Acknowledge]                                           │
└─────────────────────────────────────────────────────────┘
```

- Warning banner: red background, red text
- "Acknowledge" button: solid blue (`#4493f8`), same styling as "Include in image"
- Clicking "Acknowledge" collapses the card and persists `acknowledged=true`

### Collapsed state (after acknowledging)

```
┌─────────────────────────────────────────────────────────┐
│ custom-monitoring-agent   no repo — manual install req. │
│                                                  [undo] │
└─────────────────────────────────────────────────────────┘
```

- Package name (bold) + red warning text on the same line
- Undo button to expand back to full card (sets `acknowledged=false`)

### Persistence

Notification card acknowledged state is persisted via a per-item `acknowledged` field in the snapshot. This field is independent of `include` — notification items do not have an include/exclude semantic. The Containerfile renderer emits a TODO comment for notification-class items regardless of acknowledged state; the field exists solely to track review progress.

**Wire mapping:**
- `PackageEntry`: Add `Acknowledged bool` field. Set to `true` when admin clicks Acknowledge, `false` on Undo.
- `NonRpmSoftware.Items`: Add `Acknowledged bool` field. Same behavior.
- `TriageItem`: Add `Acknowledged bool` field, populated from the snapshot item.
- Autosave: `PUT /api/snapshot` includes the updated `acknowledged` values in the snapshot JSON.
- Resume: `GET /api/snapshot` returns the snapshot with `acknowledged` values. The JS renders acknowledged notification cards in their collapsed state.

### Containerfile output

```dockerfile
# TODO: custom-monitoring-agent (1.4.2-1, x86_64)
# No repository source available. Provide a reproducible build-time
# source (vendor RPM, download script, or equivalent) and add the
# installation steps above this comment.
```

### Keyboard and accessibility

- "Acknowledge" button: focusable, Enter/Space activates
- After acknowledging, focus moves to next undecided card in the section
- Undo button: focusable, Enter/Space expands back to full card
- Screen reader: `aria-label="custom-monitoring-agent: No repository source, manual follow-up required"`

## Data Model Changes

### TriageItem additions

```go
type TriageItem struct {
    // ... existing fields ...
    Group        string `json:"group,omitempty"`
    CardType     string `json:"card_type,omitempty"`     // "decision" (default), "notification"
    DisplayOnly  bool   `json:"display_only,omitempty"`  // true for surfaces with no v1 output path
    Acknowledged bool   `json:"acknowledged,omitempty"`  // persisted review state for notification cards
}
```

`Group`: Populated by the classifier for items that should be grouped. Examples:
- Packages: `"repo:appstream"`, `"repo:epel-9"`, `"repo:grafana-oss"`
- Config: `"kind:unchanged"`, `"kind:drop-in"`
- Runtime: `"sub:services-default"`, `"sub:services-changed"`, `"sub:cron"`, `"sub:timers"`
- System: `"sub:sysctl"`, `"sub:kmod"`, `"sub:network"`, `"sub:firewall"`

`CardType`: Defaults to `"decision"`. Set to `"notification"` for no-repo packages and non-RPM binaries.

`DisplayOnly`: Set to `true` for surfaces where the renderer has no actionable output path. The JS uses this to choose output-affecting vs display-only accordion rendering, and Acknowledge/Skip vs Include/Exclude language on individual cards.

`Acknowledged`: For notification cards, persisted via the snapshot's per-item `acknowledged` field. For display-only cards using Acknowledge/Skip, persisted the same way. Not used for output-affecting decision cards (those use `include` vs `default_include`).

Items with a non-empty `Group` render as accordion members. Items with an empty `Group` render as individual cards. The accordion type (output-affecting vs display-only) is determined by the `DisplayOnly` field of the group's items.

### Schema additions

```go
type PackageEntry struct {
    // ... existing fields ...
    Acknowledged bool `json:"acknowledged,omitempty"`
}
```

Same `Acknowledged` field added to `NonRpmSoftware` item entries, and to any untyped map items (`users_groups.groups[]`, `network.connections[]`, `storage.fstab_entries[]`) via the `mapAcknowledged()` helper (parallel to existing `mapInclude()`).

### Classifier changes (triage.go)

- `classifyPackages`: Add `Group` field with `"repo:" + pkg.SourceRepo`. Set `CardType: "notification"` and read `Acknowledged` from snapshot for locally-installed packages (no repo). BaseOS repo: JS suppresses toggle for `"repo:baseos"` groups.
- `classifyConfigFiles`: Group unchanged configs as `"kind:unchanged"`, drop-ins as `"kind:drop-in"`. Modified and custom files get no group (individual cards).
- `classifyRuntime`: Group services by state (default vs. changed), cron and timers by type. Add `dnf-makecache` and `packagekit` to a known-incompatible list → tier 3 with specific warning.
- `classifyContainerItems`: Group quadlets as `"sub:quadlet"`. Running containers without quadlet: individual cards, `DisplayOnly: true`. Non-RPM binaries: individual cards, `CardType: "notification"`, read `Acknowledged` from snapshot.
- `classifyIdentity`: No groups. Groups (GID): `DisplayOnly: true`. All others: individual output-affecting cards.
- `classifySystemItems`: Group sysctl, kmod, firewall by subsystem. Network connections: group as `"sub:network"`, `DisplayOnly: true`. Fstab entries: all `DisplayOnly: true`; entries touching `/`, `/boot`, `/var`, `/sysroot`, `/usr`, `/etc`, or using unstable device paths get no group (individual display-only cards); remaining entries group as `"sub:fstab"`.
- Single-machine predicate: `snap.FleetMetadata == nil`.

### JS changes (report.html)

- `renderTriageSection`: After `groupByTier`, within each tier, group items by `Group` field. Check `DisplayOnly` to choose accordion type. Render ungrouped items as individual cards.
- New `buildOutputAccordion(groupName, items, options)`: Toggle switch, chevron, drill-down table with checkboxes. Three-state model per specification above.
- New `buildDisplayAccordion(groupName, items)`: Chevron, drill-down table, no toggle, no checkboxes. Optional per-item Acknowledge/Skip.
- New `buildNotificationCard(item)`: Acknowledge button, collapsed state, persistence.
- `App.groupPriorState`: In-memory map storing per-item `include` values before a group toggle-off, keyed by group name. Used to restore row-level exceptions on toggle-on.

## Interaction with existing systems

### Autosave and rebuild

Output-affecting accordion toggles and per-item checkboxes update `include` fields and schedule autosave identically to current decision cards. No changes to debouncing, revision guard, or rebuild flow.

Notification card Acknowledge/Undo updates `acknowledged` fields and schedules autosave. The `acknowledged` field is written to and read from the snapshot JSON alongside `include`.

Display-only card Acknowledge/Skip updates `acknowledged` fields. No Containerfile or artifact effect.

### Section review states

All card types follow the same review state model:
- **Unreviewed:** Section not yet visited
- **In progress:** Admin has expanded at least one accordion, toggled a switch, checked/unchecked an item, or acknowledged a notification
- **Reviewed:** Admin clicks "Mark as reviewed"

**Reopen rule:** Any mutation in a reviewed section — group toggle, per-item checkbox, notification undo, display-only undo — reverts the section to "in progress." This matches the existing reopen behavior when a decision is changed after marking a section reviewed.

### Containerfile preview

Output-affecting accordion interactions trigger the same Containerfile preview updates as current decision cards. Display-only and notification card interactions do not trigger preview updates (they don't affect the Containerfile).

### Static mode

In static mode (file:// or no refine server), all accordions render **collapsed** (not expanded — expanding all would recreate the card wall this redesign eliminates). Toggles, checkboxes, and Acknowledge buttons are not rendered. Each accordion shows its collapsed summary. The existing static-mode banner applies: "Static report — run `inspectah refine` to enable interactive triage."

## Out of Scope

- **Fleet prevalence-driven grouping.** Fleet mode retains current card-per-item behavior. Follow-up spec for fleet-specific grouping with prevalence slider.
- **Dependency conflict detection.** When disabling a third-party repo breaks a dependency in an enabled repo. Valuable but requires dependency graph analysis not currently in the snapshot.
- **Package size in accordion headers.** Current `PackageEntry` / `TriageItem` do not expose package-size data. Accordion headers show item counts only. Size data can be added when the schema supports it.
- **Renderer parity for display-only surfaces.** Network connections, fstab entries, running containers, and groups remain display-only in this spec. Follow-up work to add per-item renderer output paths would promote these to output-affecting surfaces.
- **"+ New File" button in editor.** Deferred from editor redesign, remains deferred.

## Testing Strategy

### Go unit tests (classifier)

- Table-driven tests for each `classify*` function verifying `Group`, `CardType`, `DisplayOnly`, and `Acknowledged` fields
- Single-machine predicate: `FleetMetadata == nil` → groups populated; `FleetMetadata != nil` → groups empty
- BaseOS group identification and toggle suppression signal
- Third-party repo detection for badge
- Known-incompatible service detection (`dnf-makecache`, `packagekit`)
- Display-only surface classification: network connections, fstab entries, running containers without quadlet, identity groups
- Fstab risky-mount detection: `/`, `/boot`, `/var`, `/sysroot`, `/usr`, `/etc`, and unstable device paths

### Golden-file tests (renderer)

- Fragment golden for an output-affecting accordion (collapsed, expanded, partially excluded states)
- Fragment golden for a display-only accordion (collapsed, expanded)
- Fragment golden for a notification card (expanded and acknowledged states)
- Fragment golden for a packages section with mixed card types (accordion + decision + notification)
- Fragment golden confirming display-only surfaces do not produce Include/Exclude language

### Refine server contract tests

- Output-affecting accordion toggle: sets all group items' `include` to false, autosave fires
- Output-affecting accordion toggle restore: prior row-level exceptions restored after off/on cycle
- Per-item checkbox: updates single item's `include`, autosave fires
- Notification card acknowledge: sets `acknowledged=true` in snapshot, autosave fires
- Notification card undo: sets `acknowledged=false`, autosave fires
- Session resume after acknowledge: `GET /api/snapshot` returns `acknowledged=true`, card renders collapsed
- Session resume after group toggle with row exceptions: `GET /api/snapshot` returns correct per-item `include` values, group renders in partially-excluded state
- Display-only surfaces: toggling acknowledge does not trigger Containerfile preview update
- Rebuild after grouped decisions: response snapshot = working directory snapshot = tarball snapshot (three-way equality)
- Review state reopen: changing a group toggle or per-item checkbox in a reviewed section reverts it to "in progress"

### Manual browser smoke tests

| Scenario | Verify |
|----------|--------|
| Output accordion expand/collapse | Chevron rotates, drill-down table appears/hides |
| Output accordion toggle off | Card dims, all items excluded, autosave fires |
| Output accordion toggle on (restore) | Card restores, prior row-level exceptions preserved |
| Output accordion partially excluded | Summary shows "N of M included", toggle stays ON |
| Per-item checkbox uncheck | Single item excluded, summary updates |
| All items unchecked individually | Toggle reflects OFF state |
| Row controls while group off | Checkboxes disabled/grayed, not interactive |
| BaseOS accordion | No toggle switch, "always included" label |
| Third-party badge | Yellow "third-party" pill on EPEL/vendor repos |
| Display-only accordion | No toggle, no checkboxes, informational language |
| Display-only individual card | Acknowledge/Skip buttons, not Include/Exclude |
| Notification card acknowledge | Card collapses to name + warning + undo |
| Notification card undo | Card expands back to full view |
| Session resume (notification) | Acknowledged cards start collapsed after re-refine |
| Session resume (grouped) | Group states and row exceptions preserved |
| Review state reopen | Change in reviewed section reverts to "in progress" |
| Static mode | Accordions collapsed, no interactive controls, summary visible |
| Keyboard: output accordion | Tab to header (expand), Tab to toggle (switch), Tab through rows |
| Keyboard: display-only accordion | Tab to header (expand), no toggle stop |
| Screen reader: partial state | Announces "N of M included" for partially excluded groups |
| Theme toggle | Dark and light modes render all card types correctly |
