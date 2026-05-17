# Post-Leaf Bug Fix Run

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
explicitly installed them — the installer did.

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
the same gap — neither codebase filters anaconda artifacts from the
userinstalled set.

The graph-based fallback (used when `--userinstalled` has no overlap
with added packages) does not suffer from this bug because it classifies
based on dependency relationships, not install provenance.

### Proposed Fix

Add a post-filter step in `classify_leaf_auto()` after the
userinstalled intersection, before the leaf/auto split. The filter
demotes packages from leaf to auto when they match base-install
heuristics.

**Heuristic: baseline-present packages are not leaf.**

When baseline data is available (the common case after phase 6 ships),
any package in the `--userinstalled` set that also exists in the
baseline with `state: Added` (same name.arch, same or different
version) should be demoted to auto. The reasoning: if the base image
already contains the package, the user did not add it — the installer
or image builder did.

This requires threading `baseline_package_names` (already computed at
line ~260 in the RPM inspector) into `classify_leaf_auto()`.

**Implementation sketch:**

```rust
fn classify_leaf_auto(
    exec: &dyn Executor,
    packages_added: &[PackageEntry],
    baseline_names: &HashSet<String>,  // new parameter
) -> LeafClassification {
    let added_ids: HashSet<String> = packages_added.iter()
        .map(canonical_package_id).collect();

    let user_installed = query_user_installed(exec);
    // ... existing dependency graph logic ...

    // After computing initial leaf/auto split from userinstalled:
    if let Some(ref ui) = user_installed {
        let leaf_set: HashSet<&String> = ui.intersection(&added_ids).collect();
        if !leaf_set.is_empty() {
            // Demote baseline-present packages from leaf to auto.
            // If a package exists in the base image, anaconda/kickstart
            // put it there, not the user.
            let (demoted, true_leaf): (Vec<_>, Vec<_>) = leaf_set
                .into_iter()
                .partition(|id| baseline_names.contains(*id));

            // demoted → auto, true_leaf → leaf
        }
    }
    // ... rest unchanged ...
}
```

**No-baseline fallback:** When no baseline is available, the filter
cannot apply. The graph-based fallback already handles this case
reasonably well — packages depended on by other packages become auto
regardless of their userinstalled status. Accept the noise in the
no-baseline case; it self-corrects once baseline scanning is configured.

**Why not a static blocklist?** A hardcoded list of known
anaconda/kickstart packages (kernel, dosfstools, etc.) is fragile
across RHEL versions, CentOS variants, and custom kickstart profiles.
The baseline-present heuristic is version-agnostic and covers all
base-image packages without maintenance.

### Testing

- Unit test: `classify_leaf_auto` with baseline containing kernel,
  dosfstools → these should appear in auto, not leaf.
- Unit test: package in userinstalled but NOT in baseline → stays leaf.
- Unit test: no baseline available → existing behavior unchanged.
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
1. `state_changes` → items with "current_state → action" subtitle (correct — these are divergences)
2. `enabled_units` → items with "enabled" subtitle (noisy — includes stock defaults)
3. `disabled_units` → items with "disabled" subtitle (noisy — includes stock defaults)

The result is that items 2 and 3 dwarf item 1, and most of them are
services that match their preset defaults exactly.

### Proposed Fix

**Option A (recommended): Filter in the web handler.**

Modify `normalize_services()` to skip `enabled_units` and
`disabled_units` entries that also appear in `state_changes`. A unit
that diverges from its preset already has a `ServiceStateChange` entry
with full context (current state, default state, action). Showing it
again as a bare "enabled" or "disabled" item adds noise.

Additionally, suppress units whose state matches their preset default
entirely. Since the inspector already computed divergences, the
remaining enabled/disabled entries that are NOT in `state_changes` are
by definition matching their presets. They can be suppressed from the
context view.

**Concrete change to `normalize_services()`:**

Remove the `enabled_units` and `disabled_units` rendering loops
entirely. The `state_changes` loop already captures every unit that
diverges from its preset. The flat enabled/disabled lists serve the
Containerfile renderer, not the context UI. Their data is still in
the snapshot for renderers that need it.

After this change, the Services section shows:
- Units that diverge from presets (from `state_changes`)
- Drop-in overrides (from `drop_ins`)
- Nothing else — stock defaults are suppressed

**Option B (alternative): Filter in the collector.**

Add a `non_divergent_enabled` / `non_divergent_disabled` split in the
inspector itself, so `enabled_units` only contains units whose
enable state diverges from their preset. This is architecturally
cleaner but higher-risk because it changes the collector contract and
may affect Containerfile rendering which consumes these lists.

**Recommendation:** Option A. The web handler is the presentation layer.
The collector contract stays stable, renderers keep full unit lists, and
the context view shows only meaningful divergences.

### Testing

- Unit test: `normalize_services` with 5 state_changes, 80
  enabled_units, 30 disabled_units → output should contain only the 5
  state_changes plus any standalone drop-ins.
- Unit test: enabled unit that IS in state_changes → should appear once
  (as the state_change item), not twice.
- Integration: verify Services section count in the sidebar drops from
  ~110 to ~5-15 on a stock CentOS snapshot.

---

## Item 3: Leaf Dependency Tree Modal

### Problem

The snapshot already contains `leaf_dep_tree` data — a map from each
leaf package to its transitive auto-dependency list. This data is
computed during collection but never surfaced in the web UI. Users
triaging leaf packages cannot see what other packages each leaf pulls
in, which matters for deciding whether to carry it forward.

### Design Decision

**Dependencies go in a modal popup, accessible via a button on the leaf
package card.** Not inline on the card, not a separate sidebar section.
A per-leaf modal that shows the transitive dependency tree from
`leaf_dep_tree`.

### Frontend Specification

**New component: `DependencyModal.tsx`**

A PatternFly `Modal` triggered by a "View Dependencies" button on the
`PackageDetail` component. The button appears only when:
1. The package is a leaf package (its canonical `name.arch` ID exists in
   the `leaf_dep_tree` keys).
2. The dependency list is non-empty.

Modal contents:
- Title: "Dependencies: {package name}"
- Body: sorted list of dependency package names (from `leaf_dep_tree[name.arch]`)
- Each dependency rendered as a simple text item showing name and arch
  (split from canonical `name.arch` format).
- Count in the modal header: "(N dependencies)"
- Close button (standard PatternFly modal dismiss).

**Modified component: `PackageDetail.tsx`**

Add a "View Dependencies (N)" button below the existing
DescriptionList. The button is a PatternFly `Button` variant="link"
that opens the `DependencyModal`.

**Data flow:**

The `leaf_dep_tree` data is already in the snapshot JSON. It needs to
be:
1. Included in the `/api/view` response (or a new lightweight endpoint).
2. Available to `PackageDetail` via props.

**Option A (recommended):** Add `leaf_dep_tree` to the `ViewResponse`
type. The refine session already has access to the snapshot's RPM
section. The web handler maps it through as a
`Record<string, string[]>` alongside the existing `packages` and
`config_files` arrays.

**Option B:** Separate `/api/snapshot/leaf-deps` endpoint. Unnecessary
given the data is small (typically <50 entries, each a short list of
package names).

### Backend Changes

**`inspectah-web/src/handlers.rs`:**
- Add `leaf_dep_tree: HashMap<String, Vec<String>>` to the view
  response struct (or a wrapper). The raw `serde_json::Value` from
  `RpmSection.leaf_dep_tree` needs to be deserialized into a typed map.

**`inspectah-refine/src/session.rs`:**
- Expose `leaf_dep_tree()` method on `RefineSession` that returns the
  snapshot's `rpm.leaf_dep_tree` value.

### Frontend Changes

**`api/types.ts`:**
```typescript
/** Added to ViewResponse */
export interface ViewResponse extends RefinedView {
  repo_groups: RepoGroupInfo[];
  leaf_dep_tree: Record<string, string[]>;  // new
}
```

**`components/DependencyModal.tsx`:** New component.
```typescript
export interface DependencyModalProps {
  packageId: string;       // canonical name.arch
  packageName: string;     // display name
  deps: string[];          // sorted dependency list
  isOpen: boolean;
  onClose: () => void;
}
```

**`components/PackageDetail.tsx`:** Add optional `leafDeps` prop.
When present and non-empty, render "View Dependencies (N)" button.

### Testing

- Unit test: `DependencyModal` renders sorted dependency list.
- Unit test: `PackageDetail` shows button when `leafDeps` is non-empty,
  hides it when empty or absent.
- Unit test: `PackageDetail` does not show button for non-leaf packages
  (leafDeps not provided).
- Backend: verify `/api/view` response includes `leaf_dep_tree` field.

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

### Design Decisions

1. **A new "Version Changes" sidebar section under the Context group.**
   Uses the existing `ContextList`/`ContextItem` pattern. Read-only.
   Shows packages where the running system has a different version than
   the base image.

2. **Version info in the leaf package card detail.** Users triaging leaf
   packages can see version information without navigating away. This is
   supplementary detail on the card, not an organizing principle.

3. **The Packages decision section stays leaf-only.** No toggle, no
   "All Packages" view. The Decisions/Full toggle was deliberately
   removed during the unified-repo-view refactor.

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

Add a `normalize_version_changes()` function (following the pattern of
`normalize_services()`, `normalize_network()`, etc.) that maps
`RpmSection.version_changes` to a `ContextSection`:

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
                title: vc.name.clone(),
                subtitle: Some(format!(
                    "{} -> {} ({})",
                    vc.base_version, vc.host_version, direction_label
                )),
                detail: None,
                searchable_text: format!(
                    "{} {} {} {} {}",
                    vc.name, vc.arch, vc.base_version,
                    vc.host_version, direction_label
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

Add the new section to the `normalize_for_context()` return vec.
Position it after "Services" — version changes are high-signal context
that relates to the packages decision surface.

**Sections cache invalidation:** The `OnceLock<Vec<ContextSection>>`
in `AppState` (line 23 of handlers.rs) is initialized once. Since
version changes come from the snapshot (immutable during a session),
this is fine — the cache captures the correct data on first access.

### Frontend Changes

**`api/types.ts`:**

No new types needed. The `ContextSection` and `ContextItem` interfaces
already cover the wire format. The `VersionChangeDirection` type is
not needed on the frontend — the direction is encoded in the subtitle
string.

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

Add `"version_changes"` to `SECTION_LABELS`:

```typescript
const SECTION_LABELS: Record<string, string> = {
  // ... existing entries ...
  version_changes: "Version Changes",  // new
};
```

No routing changes needed — `MainContent` already renders any
non-decision section via `ContextList` when a matching section exists
in the `sections` array.

**`components/PackageDetail.tsx`:**

Add version info to the detail card for packages with
`state: "modified"`. Display base version and change direction below
the existing NEVRA and State fields.

This requires either:
- Passing the `version_changes` data as a lookup prop to
  `PackageDetail`, or
- Including base version info directly in the `PackageEntry` type (a
  larger change).

**Recommended approach:** Pass a `versionChange` prop of type
`VersionChangeInfo | null` to `PackageDetail`:

```typescript
interface VersionChangeInfo {
  baseVersion: string;
  direction: string;  // "upgrade" | "downgrade"
}
```

The parent (`DecisionItem` or `DecisionList`) looks up the package's
`name.arch` in the `version_changes` context data and passes it down.

### Testing

- Backend unit test: `classify_packages` with baseline that has
  different EVR → `version_changes` populated with correct direction.
- Backend unit test: same EVR → no version change entry.
- Backend unit test: `normalize_version_changes` produces correct
  ContextSection with subtitle format.
- Frontend unit test: Version Changes section renders in sidebar with
  correct count.
- Frontend unit test: `PackageDetail` shows base version when
  `versionChange` prop is provided.
- Frontend unit test: `PackageDetail` omits version info when prop is
  null.

---

## Cross-Cutting Concerns

### Data Flow Summary

```
Collector (Rust)
  ├─ RPM inspector
  │   ├─ classify_packages() → version_changes  [Item 4 backend]
  │   └─ classify_leaf_auto() → filtered leaf_packages  [Item 1]
  └─ Services inspector → state_changes, enabled/disabled, drop_ins
                                                          [unchanged]

Web Handler (Rust)
  ├─ /api/view → includes leaf_dep_tree  [Item 3 backend]
  ├─ /api/snapshot/sections
  │   ├─ normalize_services() → suppresses non-divergent units  [Item 2]
  │   └─ normalize_version_changes() → new section  [Item 4 backend]
  └─ unchanged endpoints

Frontend (React)
  ├─ Sidebar → adds "Version Changes" entry  [Item 4]
  ├─ MainContent → routes version_changes to ContextList  [Item 4]
  ├─ PackageDetail → "View Dependencies" button + version info  [Items 3, 4]
  └─ DependencyModal → new component  [Item 3]
```

### Dependency Order

Items 1 and 2 are independent bug fixes with no cross-dependencies.

Item 3 (dep tree modal) depends only on `leaf_dep_tree` data already in
the snapshot — no collector changes needed.

Item 4 (version changes) has a backend prerequisite: the Rust classifier
must populate `version_changes` before the frontend can render them.
The backend change (classifier + handler) ships first; the frontend
follows.

Recommended implementation order:
1. Item 2 (service noise fix) — smallest change, immediate UX win
2. Item 1 (leaf classification) — bug fix, needs baseline threading
3. Item 3 (dep tree modal) — frontend-only, no backend prerequisite
4. Item 4 (version changes) — backend then frontend

### API Contract Changes

| Endpoint | Change | Breaking? |
|---|---|---|
| `/api/view` | Add `leaf_dep_tree` field | No (additive) |
| `/api/snapshot/sections` | Add `version_changes` section; reduce services item count | No (additive + reduction) |
| Snapshot JSON | `version_changes` populated instead of empty | No (field already in schema) |
