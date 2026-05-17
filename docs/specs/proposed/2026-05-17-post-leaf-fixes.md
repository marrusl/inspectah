# Post-Leaf Bug Fix Run

> **Revision 3** (2026-05-17): Addresses round 2 findings. Item 2 now includes collector carrier (Mark's decision). Item 4 empty state and field casing fixed. Section-jump keyboard contract added.
>
> **Revision 2** (2026-05-17): Addresses round 1 review findings. See review summary for context.

## Summary

Four items from the roadmap, shipped as one batch. Two are bug fixes in
the collector and web handler (leaf classification noise, service
classification noise). Two are new UI features that surface existing
snapshot data (leaf dependency tree modal, version changes context
section).

**Audience:** Backend (Rust), frontend (React/PatternFly), interaction
design.

## Scope

**In scope:**
- Leaf classification false positives from anaconda/kickstart packages
- Service context section noise from dumping all enabled/disabled units
- Leaf dependency tree modal in the packages decision view
- Version changes as a new sidebar Context section
- Version info supplement on leaf package detail cards

**Out of scope:**
- Full package list view or "All Packages" toggle (deliberately removed)
- Baseline image selection workflow (separate spec)
- Fleet-aware service diffing
- Service include/exclude toggles (services remain read-only context)

---

## Item 1: Leaf Classification Quality

### Problem

On stock CentOS/RHEL systems, `dnf repoquery --userinstalled` reports
anaconda/kickstart-installed base packages as user-installed. Packages
like `kernel`, `dosfstools`, `efibootmgr`, `langpacks-en`, `lvm2`, and
`shim-aa64` appear as leaf packages even though the user never
explicitly installed them -- the installer did.

This inflates the leaf package count and forces users to triage packages
they have no intention of carrying forward.

### Root Cause

`classify_leaf_auto()` in `inspectah-collect/src/inspectors/rpm/mod.rs`
(line 419) trusts `query_user_installed()` output directly. The function
intersects the `--userinstalled` set with `packages_added` to produce
the leaf set. It has no heuristic to distinguish "user explicitly
installed this" from "anaconda/kickstart marked this during initial
system provisioning."

The Go implementation at `cmd/inspectah/internal/inspector/rpm.go` has
the same gap -- neither codebase filters anaconda artifacts from the
userinstalled set.

The graph-based fallback (used when `--userinstalled` has no overlap
with added packages) does not suffer from this bug because it classifies
based on dependency relationships, not install provenance.

### Proposed Fix

Add a post-filter step in `classify_leaf_auto()` after the
userinstalled intersection, before the leaf/auto split. The filter
**suppresses** baseline-present packages from the leaf triage and
install surfaces without altering `auto_packages` semantics.

**Key design principle:** Baseline presence proves target-image
membership, not dependency causality. The fix must NOT overload
`auto_packages` with baseline-membership semantics. `auto_packages`
remains strictly "dependency-derived/transitive packages" as defined
in `inspectah-core/src/types/rpm.rs`. Baseline-provided suppression
is a separate mechanism.

**Three concerns, kept separate:**

1. **Baseline-provided suppression** -- packages whose `name.arch`
   exists in the target baseline should not appear in leaf-only
   triage/install surfaces. This is the primary noise reduction.

2. **Dependency-derived `auto_packages` / `leaf_dep_tree`** -- these
   remain truthful: a package is `auto` only when the dependency graph
   proves it is a transitive dependency of another added package. No
   change to this contract.

3. **Baseline-present `Modified` packages** -- a package that exists
   in both the host and the baseline but with different versions has
   intentional version drift. These must NOT be suppressed from the
   leaf decision surface or from `RUN dnf install` output, because
   the version difference may be work the operator needs to recreate
   or consciously ignore.

**New field: `baseline_suppressed`**

Instead of demoting baseline-present packages into `auto_packages`,
introduce a new `Vec<String>` field on `RpmSection`:

```rust
/// Canonical `name.arch` identities for packages present in both the
/// host's userinstalled set and the target baseline with matching
/// version (i.e., `PackageState::Added` with identical EVR -- the
/// classifier treats same-EVR baseline matches as `Added`, not
/// `Modified`). These are suppressed from the leaf triage and install
/// surfaces but are NOT dependency-derived auto packages. `None` means
/// no baseline was available for suppression.
pub baseline_suppressed: Option<Vec<String>>,
```

**Implementation sketch:**

The suppression filter runs inside `classify_leaf_auto()` using
`ctx.baseline_data` (the authoritative baseline, not the compat
`baseline_package_names` field). It applies only to packages whose
classification state is NOT `Modified`:

```rust
fn classify_leaf_auto(
    exec: &dyn Executor,
    packages_added: &[PackageEntry],
    baseline: Option<&BaselineData>,  // from ctx.baseline_data
) -> LeafClassification {
    let added_ids: HashSet<String> = packages_added.iter()
        .map(canonical_package_id).collect();

    let user_installed = query_user_installed(exec);
    // ... existing dependency graph logic ...

    // After computing initial leaf/auto split from userinstalled:
    let mut baseline_suppressed = Vec::new();
    if let (Some(ref ui), Some(bl)) = (&user_installed, baseline) {
        let leaf_set: HashSet<&String> = ui.intersection(&added_ids).collect();
        if !leaf_set.is_empty() {
            let (suppressed, true_leaf): (Vec<_>, Vec<_>) = leaf_set
                .into_iter()
                .partition(|id| {
                    // Suppress only when the baseline has this package
                    // AND the host version matches (not Modified).
                    // Modified packages have version drift that matters.
                    bl.packages.contains_key(id.as_str())
                        && packages_added.iter().any(|p| {
                            canonical_package_id(p) == **id
                            && p.state != PackageState::Modified
                        })
                });

            baseline_suppressed = suppressed.into_iter().cloned().collect();
            // true_leaf -> leaf_packages
            // auto classification unchanged -- only dep-graph-derived
        }
    }
    // ... rest produces LeafClassification with new baseline_suppressed field
}
```

**Downstream consumers of `baseline_suppressed`:**

- `inspectah-refine/src/session.rs` -- the leaf filter (line ~the
  `is_fleet_snapshot` block) adds `baseline_suppressed` identities
  to the filter set alongside `auto_packages` when computing the
  visible package list. This keeps them out of the triage surface.

- `inspectah-pipeline/src/render/containerfile.rs` -- the
  `RUN dnf install` line already filters through `leaf_packages`.
  Baseline-suppressed packages are not in `leaf_packages` and thus
  not in the install line. But `Modified` baseline packages remain
  in `leaf_packages` because the suppression filter excluded them.

**Why `baseline_suppressed` instead of widening `auto_packages`:**
The current contract uses `auto_packages` and `leaf_dep_tree` as
paired fields: `leaf_dep_tree` maps each leaf to its auto deps. If
`auto_packages` gained non-dep-derived entries, `leaf_dep_tree` would
become inconsistent (no leaf would claim those packages as deps). A
separate field keeps both contracts truthful.

**No-baseline fallback:** When no baseline is available, the filter
cannot apply and `baseline_suppressed` is `None`. The graph-based
fallback already handles this case reasonably well. Accept the noise
in the no-baseline case; it self-corrects once baseline scanning is
configured.

**Why not a static blocklist?** A hardcoded list of known
anaconda/kickstart packages (kernel, dosfstools, etc.) is fragile
across RHEL versions, CentOS variants, and custom kickstart profiles.
The baseline-present heuristic is version-agnostic and covers all
base-image packages without maintenance.

### Testing

- Unit test: `classify_leaf_auto` with baseline containing kernel,
  dosfstools at matching version -> these should appear in
  `baseline_suppressed`, not in `leaf_packages` or `auto_packages`.
- Unit test: baseline-present package with `Modified` state (version
  drift) -> stays in `leaf_packages`, NOT suppressed.
- Unit test: package in userinstalled but NOT in baseline -> stays
  in `leaf_packages`.
- Unit test: no baseline available -> existing behavior unchanged,
  `baseline_suppressed` is `None`.
- End-to-end: collect -> refine -> view: suppressed packages absent
  from triage surface, `Modified` baseline packages still visible.
- End-to-end: collect -> render -> containerfile: suppressed packages
  absent from `RUN dnf install`, `Modified` baseline packages present.
- Regression: existing `classify_leaf_auto` tests must still pass.

---

## Item 2: Service Classification Noise

### Problem

The Services context section in the web UI shows every enabled and
disabled service unit on the system, not just those that differ from
preset defaults. On a stock CentOS 9 system, this produces 80-120
items, most of which are stock systemd defaults that the user never
changed.

### Root Cause

The Rust service inspector (`inspectah-collect/src/inspectors/services.rs`)
correctly computes divergences in `state_changes` (step 4, line ~120).
It only records a `ServiceStateChange` when a unit's current state
differs from its preset default. This is correct.

However, the inspector also populates `enabled_units` and
`disabled_units` as flat lists of ALL units in those states (lines
134-137), regardless of whether they diverge from presets. These lists
were originally meant for the Containerfile renderer (which needs to
know what to `systemctl enable/disable`), not for the context UI.

The web handler `normalize_services()` in `inspectah-web/src/handlers.rs`
(line 366) renders ALL three lists into `ContextItem`s:
1. `state_changes` -> items with "current_state -> action" subtitle (correct -- these are divergences)
2. `enabled_units` -> items with "enabled" subtitle (noisy -- includes stock defaults)
3. `disabled_units` -> items with "disabled" subtitle (noisy -- includes stock defaults)

The result is that items 2 and 3 dwarf item 1, and most of them are
services that match their preset defaults exactly.

**Critical nuance the original spec missed:** The current collector
does NOT support the claim that "anything outside `state_changes` is
by definition matching its preset." In
`inspectah-collect/src/inspectors/services.rs`, `state_changes` is
only emitted when a matching preset rule is known AND the current state
diverges. Units with no preset rule get no `state_changes` entry at all.
The collector tests (`inspectah-collect/tests/services_test.rs`) confirm
this: `httpd.service`, `libvirtd.service`, and `cups.service` are absent
from `state_changes` because preset truth is unavailable, while
`enabled_units` / `disabled_units` still carry their observed current
state. So removing those loops entirely would erase "current state
known, preset unknown" context.

### Proposed Fix

**Honest three-way contract via collector carrier + handler logic.**

The round 2 reviews identified that a handler-only approach cannot
truthfully implement a three-way split. The current collector in
`inspectah-collect/src/inspectors/services.rs` (step 4, lines ~120-170)
calls `resolve_preset()` for each unit, but only records the result
when the preset and current state **diverge** (pushing to
`state_changes`). When a preset matches the current state, the match
signal is silently discarded -- the unit still appears in
`enabled_units` or `disabled_units` with no indication that a preset
was consulted. The handler therefore cannot distinguish "preset matched"
from "preset unknown" using the current wire data.

**Mark's decision:** Ship the collector carrier now (not as a future
refinement). The three-way contract must be honest from day one.

**Three-way contract:**

1. **Known divergence** (unit has a preset, current state differs) --
   render from `state_changes` with full context. Subtitle format:
   `"current_state -> action (diverges from preset: default_state)"`.

2. **Known match** (unit has a preset, current state matches) --
   suppress entirely. These are stock defaults. The collector exports
   them in the new `preset_matched_units` field so the handler can
   distinguish them from preset-unknown units.

3. **Preset unknown** (unit has no matching preset rule, but its
   current state is observed in `enabled_units`/`disabled_units`) --
   keep visible with explicit labeling. Subtitle format:
   `"enabled (no preset rule)"` or `"disabled (no preset rule)"`.

#### Collector Change

**`inspectah-core/src/types/services.rs`:**

Add a new field to `ServiceSection`:

```rust
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ServiceSection {
    #[serde(default)]
    pub state_changes: Vec<ServiceStateChange>,
    #[serde(default)]
    pub enabled_units: Vec<String>,
    #[serde(default)]
    pub disabled_units: Vec<String>,
    #[serde(default)]
    pub drop_ins: Vec<SystemdDropIn>,
    /// Units where `resolve_preset()` found a matching rule AND the
    /// current enable/disable state matches the preset default.
    /// These are stock defaults that can be safely suppressed from
    /// the context UI. Empty when presets could not be read (degraded
    /// mode). Existing snapshots without this field deserialize as
    /// empty via `#[serde(default)]`.
    #[serde(default)]
    pub preset_matched_units: Vec<String>,
}
```

**`inspectah-collect/src/inspectors/services.rs`:**

In step 4 (the `for unit in &units` loop, lines ~120-170), the
current code only pushes to `state_changes` when a preset diverges.
Add a `preset_matched_units` accumulator that captures the match case:

```rust
        // 4. Compare state vs preset — build state_changes
        let mut state_changes = Vec::new();
        let mut enabled_units = Vec::new();
        let mut disabled_units = Vec::new();
        let mut preset_matched_units = Vec::new();  // NEW

        for unit in &units {
            if unit.unit.contains('@') || unit.state == "static" {
                continue;
            }

            match unit.state.as_str() {
                "enabled" => enabled_units.push(unit.unit.clone()),
                "disabled" => disabled_units.push(unit.unit.clone()),
                _ => {}
            }

            let default_state = resolve_preset(&unit.unit, &preset_rules);

            if let Some(ref default) = default_state {
                if *default != unit.state {
                    // Divergence — existing behavior
                    let action = if unit.state == "enabled" {
                        "enable"
                    } else {
                        "disable"
                    };
                    state_changes.push(ServiceStateChange {
                        unit: unit.unit.clone(),
                        current_state: unit.state.clone(),
                        default_state: default.clone(),
                        action: action.into(),
                        include: true,
                        owning_package: None,
                        fleet: None,
                        attention_reason: None,
                    });
                } else {
                    // Match — NEW: record so the handler can suppress
                    preset_matched_units.push(unit.unit.clone());
                }
            }
            // resolve_preset returned None → preset unknown, no action
        }
```

Wire into `ServiceSection` construction at the end of `inspect()`:

```rust
        Ok(InspectorOutput {
            section: SectionData::Services(ServiceSection {
                state_changes,
                enabled_units,
                disabled_units,
                drop_ins,
                preset_matched_units,  // NEW
            }),
            // ...
        })
```

**Backward compatibility:** The `#[serde(default)]` annotation on
`preset_matched_units` means existing snapshot JSON (which lacks this
field) deserializes with an empty vec. This gives handler-side
behavior identical to "all non-divergent units are preset-unknown"
-- the conservative fallback.

**Degraded mode:** When `read_preset_rules()` fails and the inspector
returns `Degraded`, `preset_matched_units` is empty (no preset truth
available). The handler treats all units as preset-unknown, which
is the correct conservative behavior.

#### Handler Change

**`inspectah-web/src/handlers.rs` -- `normalize_services()`:**

The handler now has three sets to classify each unit:

```rust
fn normalize_services(snap: &InspectionSnapshot) -> ContextSection {
    let mut items = Vec::new();
    if let Some(svc) = &snap.services {
        let dropin_by_unit: HashMap<&str, Vec<&str>> = /* existing logic */;

        // Units with known divergences (preset existed, state differs)
        let divergent_units: HashSet<&str> = svc.state_changes.iter()
            .map(|sc| sc.unit.as_str())
            .collect();

        // Units where preset matched (suppress from context UI)
        let matched_units: HashSet<&str> = svc.preset_matched_units.iter()
            .map(|u| u.as_str())
            .collect();

        // Render known divergences from state_changes
        for sc in &svc.state_changes {
            let dropins = dropin_by_unit.get(sc.unit.as_str());
            let mut detail_parts = vec![
                format!("Default: {}", sc.default_state),
            ];
            if let Some(drops) = dropins {
                detail_parts.push(format!("Drop-ins: {}", drops.join(", ")));
            }
            items.push(ContextItem {
                id: sc.unit.clone(),
                title: sc.unit.clone(),
                subtitle: Some(format!(
                    "{} -> {} (diverges from preset: {})",
                    sc.current_state, sc.action, sc.default_state
                )),
                detail: Some(detail_parts.join("\n")),
                searchable_text: format!(
                    "{} {} {} {}", sc.unit, sc.current_state,
                    sc.default_state, sc.action
                ),
            });
        }

        // For enabled/disabled units NOT in state_changes:
        // - If in preset_matched_units → suppress (known match)
        // - Otherwise → keep visible as preset-unknown
        for unit in &svc.enabled_units {
            if divergent_units.contains(unit.as_str()) {
                continue;  // already rendered from state_changes
            }
            if matched_units.contains(unit.as_str()) {
                continue;  // known match, suppress
            }
            items.push(ContextItem {
                id: unit.clone(),
                title: unit.clone(),
                subtitle: Some("enabled (no preset rule)".into()),
                detail: None,
                searchable_text: format!("{} enabled", unit),
            });
        }
        for unit in &svc.disabled_units {
            if divergent_units.contains(unit.as_str()) {
                continue;
            }
            if matched_units.contains(unit.as_str()) {
                continue;
            }
            items.push(ContextItem {
                id: unit.clone(),
                title: unit.clone(),
                subtitle: Some("disabled (no preset rule)".into()),
                detail: None,
                searchable_text: format!("{} disabled", unit),
            });
        }

        // Standalone drop-ins (no state_change or enabled/disabled entry)
        for di in &svc.drop_ins {
            if !divergent_units.contains(di.unit.as_str())
                && !svc.enabled_units.contains(&di.unit)
                && !svc.disabled_units.contains(&di.unit)
            {
                items.push(ContextItem {
                    id: di.unit.clone(),
                    title: di.unit.clone(),
                    subtitle: Some("drop-in override".into()),
                    detail: Some(format!("Path: {}", di.path)),
                    searchable_text: format!("{} drop-in {}", di.unit, di.path),
                });
            }
        }
    }
    ContextSection {
        id: "services".to_string(),
        display_name: "Services".to_string(),
        items,
    }
}
```

**UI labeling distinction:** The three-way contract is visible in the
UI through subtitle text:
- Divergence: `"enabled -> enable (diverges from preset: disabled)"`
- Preset unknown: `"enabled (no preset rule)"`
- Match: not rendered (suppressed)

This preserves truthful operator context while eliminating the stock-
default noise that dominated the old rendering.

### Testing

**Collector (`preset_matched_units` carrier):**
- Unit test: unit with `state: "enabled"` and matching preset
  `"enable"` -> appears in `preset_matched_units`, NOT in
  `state_changes`.
- Unit test: unit with `state: "disabled"` and matching preset
  `"disable"` -> appears in `preset_matched_units`.
- Unit test: unit with `state: "enabled"` and divergent preset
  `"disable"` -> appears in `state_changes`, NOT in
  `preset_matched_units`.
- Unit test: unit with no matching preset rule -> absent from both
  `state_changes` and `preset_matched_units`.
- Unit test: degraded mode (preset files unreadable) ->
  `preset_matched_units` is empty.
- Backward compat: deserialize snapshot JSON without
  `preset_matched_units` field -> empty vec (serde default).

**Handler (`normalize_services` three-way split):**
- Unit test: `normalize_services` with 5 `state_changes`,
  40 `preset_matched_units`, 30 remaining enabled/disabled units
  -> output should contain the 5 divergences plus the 30
  preset-unknown units, NOT the 40 matched units.
- Unit test: enabled unit in `state_changes` -> appears once (as
  the divergence item), not twice.
- Unit test: enabled unit in `preset_matched_units` -> suppressed,
  does not appear in output.
- Unit test: enabled unit NOT in `state_changes` AND NOT in
  `preset_matched_units` -> appears with
  `"enabled (no preset rule)"` subtitle.
- Unit test: subtitle text for divergence items includes both current
  state and preset default.
- Integration: verify Services section count in the sidebar drops
  significantly on a stock CentOS snapshot (from ~110 to the number of
  divergences + preset-unknown units).
- Regression: verify that the full `enabled_units` and
  `disabled_units` data is still present in the raw snapshot for
  Containerfile renderer consumption.

---

## Item 3: Leaf Dependency Tree Modal

### Problem

The snapshot already contains `leaf_dep_tree` data -- a map from each
leaf package to its transitive auto-dependency list. This data is
computed during collection but never surfaced in the web UI. Users
triaging leaf packages cannot see what other packages each leaf pulls
in, which matters for deciding whether to carry it forward.

### Design Decision

**Dependencies go in a modal popup, accessible via a button on the leaf
package card.** Not inline on the card, not a separate sidebar section.
A per-leaf modal that shows the flat dependency list from
`leaf_dep_tree`.

**This is a flat dependency list, not a nested tree.** The
`leaf_dep_tree` data maps each leaf package to its list of transitive
auto-dependencies. The modal shows this as a flat, sorted list of
`name.arch` identities. No tree visualization, no nesting.

### Scope Guard

**The modal inherits the existing single-host / non-fleet scope guard.**

Current codebase already treats leaf truth as single-host-only:
- `inspectah-refine/src/session.rs` skips leaf-only package filtering
  when any RPM package carries fleet prevalence (`pkg.fleet.is_some()`).
- `inspectah-pipeline/src/render/containerfile.rs` applies the same
  guard for preview/export rendering.

The dependency modal must apply the same rule. When the snapshot is a
fleet/merged snapshot (any package has `fleet` data), the "View
Dependencies" button is not rendered and `leaf_dep_tree` is not
included in the `ViewResponse`.

**Implementation:** The `ViewResponse` construction in the web handler
already has access to the projected snapshot. Gate `leaf_dep_tree`
inclusion on the same `is_fleet_snapshot` check:

```rust
let is_fleet = projected.rpm.as_ref()
    .map_or(false, |rpm| rpm.packages_added.iter().any(|p| p.fleet.is_some()));

let leaf_dep_tree = if is_fleet {
    HashMap::new()  // empty = modal not available
} else {
    // deserialize from serde_json::Value
    parse_leaf_dep_tree(&projected.rpm)
};
```

### Accessibility Contract

This is the **first interactive control inside `PackageDetail`** --
the detail pane is currently read-only. Adding a triggerable button
changes the focus model and requires explicit accessibility behavior.

**Keyboard reachability:** When a package row has focus and the user
opens the detail pane (existing `Enter`/expand behavior), `Tab` from
the detail pane content must reach the "View Dependencies" button.
The button is the only focusable element in the detail pane, so a
single `Tab` press from the detail content reaches it.

**Modal focus management:**
- **Trigger:** `Enter` or `Space` on the "View Dependencies" button
  opens the modal.
- **Initial focus:** The modal's close button (PatternFly `Modal`
  default behavior -- `FocusTrap` places initial focus on the first
  focusable element, which is the close "X" button).
- **Focus trap:** PatternFly `Modal` provides `FocusTrap` by default.
  `Tab` cycles within the modal (close button -> dependency list ->
  close button). `Shift+Tab` cycles in reverse.
- **Close:** `Escape` or close button dismisses the modal.
- **Focus return:** On close, focus returns to the "View Dependencies"
  button that triggered the modal. PatternFly `Modal` handles this
  via `onClose` callback + `appendTo` behavior. The implementation
  must verify this works correctly in the `PackageDetail` context.

**Long-list behavior:** When the dependency list exceeds the modal's
viewport height, the list container scrolls. Use PatternFly's
`Modal` body with `overflow-y: auto`. No virtualization needed for
the expected list sizes (typically 5-50 dependencies, worst case a
few hundred).

**Visible `name.arch` identity:** Each dependency in the list renders
the full `name.arch` format (e.g., `glibc.x86_64`), not just the
package name. This is consistent with the package identity model used
throughout the triage surface and avoids ambiguity on multilib hosts.

### Frontend Specification

**New component: `DependencyModal.tsx`**

A PatternFly `Modal` triggered by a "View Dependencies" button on the
`PackageDetail` component. The button appears only when:
1. The package is a leaf package (its canonical `name.arch` ID exists in
   the `leaf_dep_tree` keys).
2. The dependency list is non-empty.
3. The snapshot is not a fleet/merged snapshot (enforced by
   `leaf_dep_tree` being empty in the `ViewResponse` for fleet data).

Modal contents:
- Title: `"Dependencies: {name}.{arch}"` (full `name.arch` identity)
- Body: sorted flat list of dependency `name.arch` identities
- Each dependency rendered as a read-only list item showing full
  `name.arch` (e.g., `glibc.x86_64`, `ncurses-libs.x86_64`)
- Count in the modal header: `"(N dependencies)"`
- Close button (standard PatternFly modal dismiss)
- List container with `overflow-y: auto` for long lists
- ARIA: `aria-label="Dependency list for {name}.{arch}"`

**Modified component: `PackageDetail.tsx`**

Add a "View Dependencies (N)" button below the existing
DescriptionList. The button is a PatternFly `Button` variant="link"
that opens the `DependencyModal`. The button must be focusable via
`Tab` from the detail pane content.

**Data flow:**

The `leaf_dep_tree` data is already in the snapshot JSON. It needs to
be:
1. Included in the `/api/view` response (gated by fleet check).
2. Available to `PackageDetail` via props.

**Option A (recommended):** Add `leaf_dep_tree` to the `ViewResponse`
type. The refine session already has access to the snapshot's RPM
section. The web handler maps it through as a
`Record<string, string[]>` alongside the existing `packages` and
`config_files` arrays, gated by the fleet scope check.

### Backend Changes

**`inspectah-web/src/handlers.rs`:**
- Add `leaf_dep_tree: HashMap<String, Vec<String>>` to the view
  response struct. The raw `serde_json::Value` from
  `RpmSection.leaf_dep_tree` is deserialized into a typed map.
- Gate inclusion on the same fleet check used by session.rs leaf
  filtering: if any package has `fleet` data, return empty map.

**`inspectah-refine/src/session.rs`:**
- Expose `leaf_dep_tree()` method on `RefineSession` that returns the
  snapshot's `rpm.leaf_dep_tree` value.

### Frontend Changes

**`api/types.ts`:**
```typescript
/** Added to ViewResponse */
export interface ViewResponse extends RefinedView {
  repo_groups: RepoGroupInfo[];
  leaf_dep_tree: Record<string, string[]>;  // new, empty for fleet
}
```

**`components/DependencyModal.tsx`:** New component.
```typescript
export interface DependencyModalProps {
  packageId: string;       // canonical name.arch
  deps: string[];          // sorted dependency list (name.arch format)
  isOpen: boolean;
  onClose: () => void;
}
```

**`components/PackageDetail.tsx`:** Add optional `leafDeps` prop.
When present and non-empty, render "View Dependencies (N)" button.
Button receives `tabIndex={0}` and proper `aria-label`.

### Testing

- Unit test: `DependencyModal` renders sorted dependency list with
  full `name.arch` identity.
- Unit test: `DependencyModal` long list (50+ items) scrolls within
  modal body.
- Unit test: `PackageDetail` shows button when `leafDeps` is non-empty,
  hides it when empty or absent.
- Unit test: `PackageDetail` does not show button for non-leaf packages
  (leafDeps not provided).
- Unit test: keyboard flow -- `Tab` reaches button, `Enter` opens
  modal, `Escape` closes, focus returns to button.
- Unit test: modal initial focus is on close button (PatternFly
  FocusTrap default).
- Backend: verify `/api/view` response includes `leaf_dep_tree` for
  single-host snapshots, empty map for fleet/merged snapshots.
- Backend: verify fleet scope guard matches the existing
  `is_fleet_snapshot` logic in session.rs.

---

## Item 4: Version Changes Context Section

### Problem

Packages with `state: "modified"` (running system has a different
version than the base image) exist in the snapshot data but are not
surfaced anywhere in the web UI. Users cannot see version drift without
examining the raw snapshot JSON.

Additionally, the Rust collector does not populate the
`version_changes` field in `RpmSection`. The Go code computes
`VersionChange` entries during baseline classification (comparing host
EVR against baseline EVR for each `Modified` package), but the Rust
port uses `..Default::default()` which leaves `version_changes` as an
empty vec.

### Cross-Surface Contract

**This is not just a new UI feature.** Populating `version_changes` in
the collector is a cross-surface contract change that affects three
existing consumers:

1. **`inspectah-refine/src/attention.rs`** -- already consumes
   `rpm.version_changes` to classify modified packages as upgrade vs
   downgrade. Upgrades stay `Routine`; downgrades become `NeedsReview`.
   Currently, with `version_changes` empty, all `Modified` packages
   default to upgrade-like attention. Once the collector populates
   this field, downgrades will shift to `NeedsReview`, changing
   `needs_review_count` and package grouping on the triage surface.

2. **`inspectah-pipeline/src/render/audit.rs`** -- already renders a
   "Version Changes" markdown table (columns: Package, Host Version,
   Base Version, Direction) whenever `version_changes` is non-empty.
   Once populated, audit exports will include this table.

3. **New: `inspectah-web/src/handlers.rs`** -- the new Context section
   and `PackageDetail` supplement described below.

The spec must prove that all three consumers produce correct output
after the collector starts emitting real data.

### Design Decisions

1. **A new "Version Changes" sidebar section under the Context group.**
   This is a separate read-only Context section (decided -- Ember and
   Fern debated integrated vs. separate placement; Mark chose separate).
   Shows packages where the running system has a different version than
   the base image.

2. **A typed `VersionChange` carrier for the web/UI boundary.**
   The frontend needs structured data, not parsed subtitle strings.
   The handler emits `ContextItem`s for the sidebar list, but
   `PackageDetail` receives a typed struct for version-change display.
   These are two distinct render paths from the same underlying data.

3. **The Packages decision section stays leaf-only.** No toggle, no
   "All Packages" view.

### Backend Changes

**`inspectah-collect/src/inspectors/rpm/classifier.rs`:**

Port the version change detection from the Go code. During
`classify_packages()`, when a package matches a baseline entry but has
a different EVR (the `Modified` state branch), emit a `VersionChange`
entry:

```rust
PackageState::Modified => {
    // Existing: set state to Modified
    // New: also record the version change
    version_changes.push(VersionChange {
        name: pkg.name.clone(),
        arch: pkg.arch.clone(),
        host_version: format!("{}-{}", pkg.version, pkg.release),
        base_version: format!("{}-{}", base.version, base.release),
        host_epoch: pkg.epoch.clone(),
        base_epoch: base.epoch.clone(),
        direction: if rpmvercmp_evr(pkg, base) == Ordering::Greater {
            VersionChangeDirection::Upgrade
        } else {
            VersionChangeDirection::Downgrade
        },
    });
}
```

The `classify_packages` return type changes from `Vec<PackageEntry>` to
a struct carrying both the classified packages and the version changes
list:

```rust
pub struct ClassificationResult {
    pub packages: Vec<PackageEntry>,
    pub version_changes: Vec<VersionChange>,
}
```

**`inspectah-collect/src/inspectors/rpm/mod.rs`:**

Wire the `version_changes` from `ClassificationResult` into the
`RpmSection` builder (step 9, line ~307), replacing the
`..Default::default()` that currently zeroes the field.

**`inspectah-web/src/handlers.rs`:**

Two render paths from `version_changes`:

**Path 1: Context section (sidebar list)**

Add a `normalize_version_changes()` function that maps
`RpmSection.version_changes` to a `ContextSection` with
`ContextItem`s. These items use `name.arch` visible identity and
epoch-aware rendering:

```rust
fn normalize_version_changes(snap: &InspectionSnapshot) -> ContextSection {
    let mut items = Vec::new();
    if let Some(rpm) = &snap.rpm {
        for vc in &rpm.version_changes {
            let direction_label = match vc.direction {
                VersionChangeDirection::Upgrade => "upgrade",
                VersionChangeDirection::Downgrade => "downgrade",
            };
            items.push(ContextItem {
                id: format!("{}.{}", vc.name, vc.arch),
                title: format!("{}.{}", vc.name, vc.arch),  // name.arch visible identity
                subtitle: Some(format!(
                    "{} -> {} ({})",
                    format_evr(&vc.base_epoch, &vc.base_version),
                    format_evr(&vc.host_epoch, &vc.host_version),
                    direction_label
                )),
                detail: None,
                searchable_text: format!(
                    "{} {} {} {} {} {}",
                    vc.name, vc.arch, vc.base_version,
                    vc.host_version, direction_label,
                    format!("{}.{}", vc.name, vc.arch)
                ),
            });
        }
    }
    ContextSection {
        id: "version_changes".to_string(),
        display_name: "Version Changes".to_string(),
        items,
    }
}
```

**Path 2: Typed carrier for `PackageDetail`**

Add a `version_changes` field to the `ViewResponse` that carries
structured data for the frontend, separate from the `ContextItem`
display strings:

```rust
/// Wire-format version change for the frontend typed API.
#[derive(Serialize)]
pub struct VersionChangeEntry {
    pub name: String,
    pub arch: String,
    pub host_version: String,
    pub base_version: String,
    pub host_epoch: String,
    pub base_epoch: String,
    pub direction: String,  // "upgrade" | "downgrade"
}
```

The `ViewResponse` gains:
```rust
pub version_changes: Vec<VersionChangeEntry>,
```

This lets `PackageDetail` look up version-change data by `name.arch`
without parsing subtitle strings from `ContextItem`.

Add the new context section to `normalize_for_context()`. Position
after "Services".

**Epoch-aware rendering:**

The `format_evr` helper renders epoch only when it is meaningful:

```rust
fn format_evr(epoch: &str, version_release: &str) -> String {
    let epoch_num: u32 = epoch.parse().unwrap_or(0);
    if epoch_num > 0 {
        format!("{}:{}", epoch, version_release)
    } else {
        version_release.to_string()
    }
}
```

**Cross-epoch change detection:** When the epoch changes between
host and base (even if version-release is identical), the rendered
display must show both epochs to avoid the misleading
`1.2-3 -> 1.2-3 (upgrade)` appearance. The `format_evr` function
handles this: if either epoch is > 0, it is included in the display.
Supplementary rule: if `host_epoch != base_epoch`, always show both
epochs regardless of their numeric value:

```rust
fn format_evr_pair(
    host_epoch: &str, host_vr: &str,
    base_epoch: &str, base_vr: &str,
) -> (String, String) {
    let show_epoch = host_epoch != base_epoch
        || host_epoch.parse::<u32>().unwrap_or(0) > 0
        || base_epoch.parse::<u32>().unwrap_or(0) > 0;
    if show_epoch {
        (format!("{}:{}", base_epoch, base_vr),
         format!("{}:{}", host_epoch, host_vr))
    } else {
        (base_vr.to_string(), host_vr.to_string())
    }
}
```

**Multiarch `name.arch` visible identity:** Both the Context section
rows and the `PackageDetail` supplement display `name.arch` (e.g.,
`glibc.x86_64`, `glibc.i686`). This prevents ambiguous rows on
multilib hosts where the same package name may have different version
drift on different architectures.

### Frontend Changes

**Field casing convention:** The existing project convention is
snake_case on the wire (Rust serde defaults, no `rename_all`) and
snake_case in TypeScript types (matching `api/types.ts` where fields
like `source_repo`, `fleet`, `diff_against_rpm`, `display_name` are
all snake_case). The `VersionChangeEntry` type follows this convention.

**`api/types.ts`:**

```typescript
/** Typed version change from the ViewResponse.
 *  Field names are snake_case matching Rust serde output. */
export interface VersionChangeEntry {
  name: string;
  arch: string;
  host_version: string;
  base_version: string;
  host_epoch: string;
  base_epoch: string;
  direction: "upgrade" | "downgrade";
}

/** Added to ViewResponse */
export interface ViewResponse extends RefinedView {
  repo_groups: RepoGroupInfo[];
  leaf_dep_tree: Record<string, string[]>;
  version_changes: VersionChangeEntry[];  // new typed carrier
}
```

**`components/Sidebar.tsx`:**

Add `version_changes` to the `CONTEXT_SECTIONS` array:

```typescript
const CONTEXT_SECTIONS = [
  { id: "services", label: "Services" },
  { id: "version_changes", label: "Version Changes" },  // new
  { id: "containers", label: "Containers" },
  // ... rest unchanged
];
```

**`components/MainContent.tsx`:**

Add `"version_changes"` to `SECTION_LABELS` and to the
`contextSectionIds` array:

```typescript
const SECTION_LABELS: Record<string, string> = {
  // ... existing entries ...
  version_changes: "Version Changes",  // new
};
```

```typescript
const contextSectionIds = [
  "services",
  "version_changes",  // new -- must match Sidebar order
  "containers",
  // ... rest unchanged
];
```

The existing routing logic in `MainContent` handles context sections
uniformly: when `contextSectionIds.includes(activeSection)`, it finds
the section in the `sections` array and renders via `ContextList`.
When the section is not found, it falls back to
`"Section data not available."`. This fallback behavior is
insufficient for `version_changes` because it cannot distinguish
zero-drift from no-baseline. The empty-state contract below replaces
it for this section.

#### Empty-State Contract

The handler **always emits** the `version_changes` section from
`normalize_for_context()`, regardless of whether a baseline exists or
whether there are version changes. The section carries a typed
`empty_reason` field that the frontend uses to render section-specific
empty-state copy.

**Wire contract:**

The `ContextSection` struct gains an optional `empty_reason` field:

```rust
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ContextSection {
    pub id: String,
    pub display_name: String,
    pub items: Vec<ContextItem>,
    /// Present only on sections that need custom empty-state copy.
    /// `None` when items is non-empty (normal rendering).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub empty_reason: Option<String>,
}
```

**Handler behavior in `normalize_version_changes()`:**

```rust
fn normalize_version_changes(snap: &InspectionSnapshot) -> ContextSection {
    let mut items = Vec::new();
    let empty_reason;

    if snap.baseline.is_none() && snap.rpm.as_ref()
        .map_or(true, |r| r.version_changes.is_empty())
    {
        // No baseline was available during collection
        empty_reason = Some("no_baseline".to_string());
    } else if let Some(rpm) = &snap.rpm {
        for vc in &rpm.version_changes {
            // ... existing ContextItem construction ...
            items.push(/* ... */);
        }
        if items.is_empty() {
            // Baseline existed but all packages match
            empty_reason = Some("zero_drift".to_string());
        } else {
            empty_reason = None;
        }
    } else {
        empty_reason = Some("no_baseline".to_string());
    }

    ContextSection {
        id: "version_changes".to_string(),
        display_name: "Version Changes".to_string(),
        items,
        empty_reason,
    }
}
```

**TypeScript type update:**

```typescript
export interface ContextSection {
  id: string;
  display_name: string;
  items: ContextItem[];
  empty_reason?: "zero_drift" | "no_baseline" | null;
}
```

**Frontend rendering in `MainContent.tsx`:**

When `activeSection` is `"version_changes"` and the section has zero
items, check `section.empty_reason` before falling through to
`ContextList` (which would show the generic
`"No Version Changes data in this snapshot"` message):

```typescript
if (activeSection === "version_changes" && section.items.length === 0) {
  const copy = section.empty_reason === "no_baseline"
    ? "Version comparison requires a baseline. Run with --baseline to enable."
    : "All packages match the target baseline versions.";
  return (
    <PageSection>
      <Content><h2>{label}</h2></Content>
      <EmptyState
        titleText={copy}
        icon={CubesIcon}
        headingLevel="h3"
      />
    </PageSection>
  );
}
```

This check runs before the `ContextList` render path, so zero-item
sections with a typed reason get custom copy while all other sections
keep the existing generic empty state from `ContextList`.

**Sidebar behavior:** The `version_changes` entry is always present
in the static `CONTEXT_SECTIONS` array. Its badge count reflects
`section.items.length` (zero for both empty-state cases). This is
consistent: the section is always navigable, and when selected, the
main content area shows the appropriate explanation.

#### Section-Jump Keyboard Contract

The current `useKeyboard.ts` hook maps keys `1-9` to sections in the
`SECTION_IDS` array (display order):

```typescript
// Current SECTION_IDS (11 entries, keys 1-9 reach first 9):
const SECTION_IDS = [
  "packages",          // 1
  "configs",           // 2
  "services",          // 3
  "containers",        // 4
  "users_groups",      // 5
  "network",           // 6
  "storage",           // 7
  "scheduled_tasks",   // 8
  "non_rpm_software",  // 9
  "kernel_boot",       // (no key -- already beyond 9)
  "selinux",           // (no key -- already beyond 9)
];
```

`version_changes` is inserted after `"services"` (position index 3)
to match the sidebar order. This shifts `"containers"` from key `4`
to key `5`, and cascades through the rest. The mapping becomes:

```typescript
const SECTION_IDS = [
  "packages",          // 1
  "configs",           // 2
  "services",          // 3
  "version_changes",   // 4  (NEW)
  "containers",        // 5  (was 4)
  "users_groups",      // 6  (was 5)
  "network",           // 7  (was 6)
  "storage",           // 8  (was 7)
  "scheduled_tasks",   // 9  (was 8)
  "non_rpm_software",  // (was 9 -- loses key reachability)
  "kernel_boot",       // (no key)
  "selinux",           // (no key)
];
```

**Trade-off:** `non_rpm_software` loses its `9` key binding. This is
acceptable because:
- The section is low-traffic (typically 0-5 items)
- `kernel_boot` and `selinux` were already unreachable via number keys
- The alternative (expanding beyond `1-9` to `0` or modifier keys)
  would break the intuitive single-digit model

**`ShortcutOverlay.tsx`:** The overlay currently documents `1-9` as
`"Jump to section by index"`. This description remains accurate since
the model is still `1-9` mapped to the first 9 entries in
`SECTION_IDS`. No change needed to the overlay text itself.

**Section-local search/filter:** For the expected 20-80 row range of
version changes, the existing `ContextList` search behavior (global
`searchable_text` matching) is sufficient. The `searchable_text`
field includes `name`, `arch`, `name.arch`, both versions, and the
direction label, so users can find entries by any of these terms.

No additional section-local filter control is needed in this slice.
If the list regularly exceeds 80 rows in practice, a follow-up can
add direction-based filtering (show upgrades only / downgrades only).

**`components/PackageDetail.tsx`:**

Add version info to the detail card for packages with
`state: "modified"`. The parent component looks up the package's
`name.arch` in the typed `version_changes` array from `ViewResponse`
and passes a `VersionChangeEntry` (or null) as a prop.

Display below the existing NEVRA and State fields:
- Label: "Base Version"
- Value: epoch-aware formatted base version
- Label: "Direction"
- Value: "Upgrade" or "Downgrade" with appropriate visual indicator

The `PackageDetail` component receives this as a typed prop, NOT by
parsing `ContextItem` subtitle strings:

```typescript
interface PackageDetailProps {
  // ... existing props ...
  versionChange?: VersionChangeEntry | null;
}
```

### Testing

**Collector (cross-surface contract proof):**
- Unit test: `classify_packages` with baseline that has different EVR
  -> `version_changes` populated with correct direction.
- Unit test: same EVR -> no version change entry.
- Unit test: epoch-only change (same version-release, different epoch)
  -> `version_changes` entry with correct direction.
- Unit test: multiarch -- same package name, different arches with
  different version drift -> separate `VersionChange` entries for each.

**Refine attention (cross-surface proof):**
- Unit test: `classify_package` in `attention.rs` with populated
  `version_changes` containing a downgrade -> package gets
  `NeedsReview` attention.
- Unit test: populated `version_changes` with upgrade -> package stays
  `Routine`.
- End-to-end: collect with real baseline -> refine -> verify
  `needs_review_count` increases for downgraded packages vs. the
  current behavior (empty `version_changes` defaults to routine).

**Audit export (cross-surface proof):**
- Unit test: `render_audit` with populated `version_changes` ->
  output contains "Version Changes" table with correct columns.
- Unit test: empty `version_changes` -> no "Version Changes" table
  in audit output (existing behavior preserved).

**Web handler:**
- Unit test: `normalize_version_changes` produces `ContextSection`
  with `name.arch` in title and epoch-aware subtitle.
- Unit test: epoch-only change renders both epochs in subtitle to
  avoid ambiguous `1.2-3 -> 1.2-3 (upgrade)`.
- Unit test: `ViewResponse` includes typed `version_changes` array.

**Frontend:**
- Unit test: Version Changes section renders in sidebar with correct
  count.
- Unit test: zero-drift empty state -- section with zero items and
  `empty_reason: "zero_drift"` shows
  `"All packages match the target baseline versions."`.
- Unit test: no-baseline empty state -- section with zero items and
  `empty_reason: "no_baseline"` shows
  `"Version comparison requires a baseline. Run with --baseline to enable."`.
- Unit test: section always present in sidebar (both empty-state cases
  show with badge count 0, normal case shows item count).
- Unit test: `PackageDetail` shows base version and direction when
  `versionChange` prop is provided.
- Unit test: `PackageDetail` omits version info when prop is null.
- Unit test: multiarch -- `glibc.x86_64` and `glibc.i686` render as
  distinct rows with visible `name.arch` identity.
- Unit test: `VersionChangeEntry` fields use snake_case matching Rust
  serde output (`host_version`, `base_version`, `host_epoch`,
  `base_epoch`).

**Section-jump keyboard regression:**
- Unit test: `SECTION_IDS` array in `useKeyboard.ts` contains
  `"version_changes"` at index 3 (key `4`).
- Unit test: key `4` navigates to Version Changes section.
- Unit test: key `5` navigates to Containers (shifted from key `4`).
- Unit test: key `9` navigates to Scheduled Tasks (shifted from
  key `8`).
- Unit test: `ShortcutOverlay` still documents `1-9` as
  `"Jump to section by index"`.

---

## Cross-Cutting Concerns

### Data Flow Summary

```
Collector (Rust)
  |-- RPM inspector
  |   |-- classify_packages() -> ClassificationResult
  |   |   |-- packages: Vec<PackageEntry>
  |   |   +-- version_changes: Vec<VersionChange>      [Item 4 backend]
  |   +-- classify_leaf_auto() -> LeafClassification
  |       |-- leaf_packages: Option<Vec<String>>
  |       |-- auto_packages: Option<Vec<String>>       [unchanged semantics]
  |       |-- leaf_dep_tree: serde_json::Value
  |       +-- baseline_suppressed: Option<Vec<String>>  [Item 1 - NEW]
  +-- Services inspector -> state_changes, enabled/disabled, drop_ins,
  |                         preset_matched_units         [Item 2 - NEW]

Web Handler (Rust)
  |-- /api/view
  |   |-- leaf_dep_tree (gated by fleet check)          [Item 3]
  |   +-- version_changes: Vec<VersionChangeEntry>      [Item 4 typed carrier]
  |-- /api/snapshot/sections
  |   |-- normalize_services() -> three-way contract    [Item 2]
  |   |   (uses preset_matched_units from collector)
  |   +-- normalize_version_changes() -> new section    [Item 4]
  |       (always emitted, with empty_reason for empty states)
  +-- unchanged endpoints

Refine (Rust, existing consumers affected by Item 4)
  |-- attention.rs: version_changes -> upgrade/downgrade attention
  +-- session.rs: baseline_suppressed added to leaf filter set  [Item 1]

Pipeline (Rust, existing consumer affected by Item 4)
  +-- audit.rs: version_changes -> "Version Changes" table

Frontend (React)
  |-- Sidebar -> adds "Version Changes" entry             [Item 4]
  |-- MainContent -> routes version_changes to ContextList [Item 4]
  |-- PackageDetail
  |   |-- "View Dependencies" button (fleet-gated)        [Item 3]
  |   +-- version info from typed VersionChangeEntry      [Item 4]
  +-- DependencyModal -> new component (a11y contract)     [Item 3]
```

### Dependency Order

Items 1 and 2 are independent bug fixes with no cross-dependencies.
Item 2 now includes a collector change (`preset_matched_units` on
`ServiceSection`) that must land before the handler change can use it.

Item 3 (dep tree modal) depends only on `leaf_dep_tree` data already in
the snapshot -- no collector changes needed. Requires the fleet scope
guard in the handler.

Item 4 (version changes) is a cross-surface contract change. The
collector change affects attention.rs and audit.rs immediately. The
backend change (classifier + handler) ships first; the frontend follows.
**Critical: the attention and audit behavior changes must be verified
before the frontend work begins.**

Recommended implementation order:
1. Item 2 (service noise fix) -- collector carrier + handler change
2. Item 1 (leaf classification) -- bug fix, needs baseline threading
   and new `baseline_suppressed` field
3. Item 4 backend (classifier + handler + cross-surface proof) --
   verify attention and audit behavior before frontend
4. Item 3 (dep tree modal) -- frontend, needs fleet scope guard
5. Item 4 frontend (sidebar section + PackageDetail supplement)

### API Contract Changes

| Endpoint | Change | Breaking? |
|---|---|---|
| `/api/view` | Add `leaf_dep_tree` field (fleet-gated) | No (additive) |
| `/api/view` | Add `version_changes` typed array (snake_case fields) | No (additive) |
| `/api/snapshot/sections` | Add `version_changes` section (always emitted, with `empty_reason`); three-way services contract | No (additive + reduction) |
| `/api/snapshot/sections` | `ContextSection` gains optional `empty_reason` field | No (additive, `skip_serializing_if`) |
| Snapshot JSON | `version_changes` populated instead of empty | No (field already in schema) |
| Snapshot JSON | New `baseline_suppressed` field on `RpmSection` | No (additive, `Option`) |
| Snapshot JSON | New `preset_matched_units` field on `ServiceSection` | No (additive, `serde(default)`) |
| Refine | `needs_review_count` may change for downgraded packages | Behavioral (correct) |
| Audit | "Version Changes" table appears when field populated | Behavioral (correct) |
