# Post-Leaf Bug Fix Run Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Revision 8** (2026-05-18): Addresses round 7 review. Epoch contract fully closed: `classify_packages()` now normalizes epochs via `norm_epoch()` (`""` → `"0"`) before `rpmvercmp` call, preventing spurious `Modified` for trivial epoch differences. Explicit step to delete contradictory `test_classify_baseline_none_epoch_defaults_to_empty()` test (line ~222) which asserts `Modified` for `"0"` vs `""`. Replacement test uses same fixture data (kernel, same EVR) with correct `Added` expectation.
>
> **Revision 7** (2026-05-18): Addresses round 6 review. Epoch classifier/render contract closed: added `test_classify_empty_vs_zero_epoch_is_not_drift` proving `rpmvercmp("", "0")` returns Equal so the classifier never emits a `VersionChange` for this case. Render-side normalization is defense-in-depth. Focus test tightened: `document.body` removed as passing state — `mainContent!.contains(document.activeElement)` must be `true`.
>
> **Revision 6** (2026-05-18): Addresses round 5 review. Epoch proof split into two tests: same-EVR epoch-only (`1:` vs `2:` with identical version-release) proves the dangerous case; `""` vs `"0"` normalization test proves trivial-epoch suppression. Both Rust and TypeScript sides have the same-EVR proof. Empty-section focus test replaced render-only assertion with real `document.activeElement` focus check through app-level key-4 navigation.
>
> **Revision 5** (2026-05-18): Addresses round 4 review. RoutineSummary proof snippets now use real `getByLabelText("Expand <name>")` affordance instead of clicking label text. Task 14 state literal fixed to `"modified"` (lowercase). Empty-section focus proof moved to app-level `FocusAndNavigation.test.tsx` with key-4 navigation. Epoch `format_evr_pair`/`formatEvrPair` tightened with normalization: `""` → `"0"` before comparing, show epoch when normalized values differ.
>
> **Revision 4** (2026-05-18): Addresses round 3 review. All placeholder test descriptions replaced with concrete, copy-pasteable test code. Task 6 session test uses real `test_snapshot()` + `RefineSession::new()` pattern. Task 11 adds initial-focus-on-close-button proof. Task 12/14 RoutineSummary path proofs have full render + assertion code. Task 13 Sidebar test has concrete render + badge + click assertions and empty-section focus landing proof. All "read the existing pattern" instructions eliminated.
>
> **Revision 3** (2026-05-18): Addresses round 2 review. Task 5 `BaselinePackageEntry.epoch` fixed to `Option<String>`, missing `glibc.x86_64` repoquery mock added. `format_evr()` replaced with paired `format_evr_pair()`/`formatEvrPair()` to handle `""` vs `"0"` edge case with explicit proof. Modal a11y proof expanded: Enter/Space open, Escape close, long-list scroll. RoutineSummary path proof tests added for both `leafDepTree` and `versionChange` threading. Concrete `4/5/9` remap assertions in `useKeyboard.test.ts`. ShortcutOverlay wording restored to approved `"Jump to section by index"`. `Sidebar.test.tsx` coverage added. Smoke fixture pinned to tracked path. Task 7 failure check split.
>
> **Revision 2** (2026-05-18): Addresses plan review findings. Item 1 rewritten against real `classify_leaf_auto`/`LeafClassification`/`recompute_view` seams. Tasks 5+8 merged into atomic commit. Epoch-aware `format_evr()` shared helper added. `PackageDetail` prop narrowed to card-local `versionChange?: VersionChangeEntry | null`. Frontend file map includes `RoutineSummary.tsx` and names existing proof suites. Verification gates use `set -o pipefail` and name specific proof-bearing tests. `BaselineData` fixtures are explicit (no `Default`). `PackageEntry.epoch` is `String` (not `Option`).

**Goal:** Implement the four post-leaf fixes from the approved spec (`docs/specs/proposed/2026-05-17-post-leaf-fixes.md`): service noise reduction, leaf classification quality, leaf dependency tree modal, and version changes context section.

**Architecture:** Five sequential implementation phases following the spec's dependency order: Item 2 (service noise) → Item 1 (leaf classification) → Item 4 backend (version changes collector + handler + attention exclusion, **atomic**) → Item 3 (dep-tree modal, backend + frontend) → Item 4 frontend (version changes sidebar + PackageDetail supplement). Backend changes land first so the API contract is stable before frontend work begins.

**Tech Stack:** Rust (inspectah-core, inspectah-collect, inspectah-refine, inspectah-web, inspectah-pipeline), TypeScript/React (inspectah-web/ui), PatternFly 6, Vitest

**Spec reference:** `docs/specs/proposed/2026-05-17-post-leaf-fixes.md` — read the full spec for design rationale and edge cases not repeated here.

---

## File Map

### Item 2: Service Classification Noise
- Modify: `inspectah-core/src/types/services.rs` — add `preset_matched_units` field to `ServiceSection`
- Modify: `inspectah-collect/src/inspectors/services.rs` — capture preset-match signal in collector
- Modify: `inspectah-web/src/handlers.rs` — three-way split in `normalize_services()`

### Item 1: Leaf Classification Quality
- Modify: `inspectah-core/src/types/rpm.rs` — add `baseline_suppressed` field to `RpmSection`
- Modify: `inspectah-collect/src/inspectors/rpm/mod.rs` — add `baseline` param to `classify_leaf_auto()`, add `baseline_suppressed` to `LeafClassification`, wire through `RpmSection` builder
- Modify: `inspectah-refine/src/session.rs` — exclude `baseline_suppressed` from `recompute_view()` leaf allowlist

### Item 4 Backend: Version Changes (**atomic: classifier + attention in single commit**)
- Modify: `inspectah-collect/src/inspectors/rpm/classifier.rs` — return `ClassificationResult` with `version_changes`
- Modify: `inspectah-collect/src/inspectors/rpm/mod.rs` — wire `ClassificationResult` into `RpmSection`
- Modify: `inspectah-refine/src/attention.rs` — gate baseline-present packages out of attention classification
- Modify: `inspectah-web/src/handlers.rs` — add `ContextSection` `empty_reason` field, `normalize_version_changes()`, `format_evr()` helper, `VersionChangeEntry` in ViewResponse

### Item 3: Dep-Tree Modal (Backend + Frontend)
- Modify: `inspectah-refine/src/session.rs` — expose `leaf_dep_tree()` method
- Modify: `inspectah-web/src/handlers.rs` — add `leaf_dep_tree` to `ViewResponse`
- Modify: `inspectah-web/ui/src/api/types.ts` — add `leaf_dep_tree`, `version_changes`, `VersionChangeEntry`, `empty_reason`
- Create: `inspectah-web/ui/src/components/DependencyModal.tsx` — modal component
- Create: `inspectah-web/ui/src/components/__tests__/DependencyModal.test.tsx`
- Modify: `inspectah-web/ui/src/components/PackageDetail.tsx` — add "View Dependencies" button
- Modify: `inspectah-web/ui/src/components/DecisionItem.tsx` — thread `leafDepTree` prop
- Modify: `inspectah-web/ui/src/components/DecisionList.tsx` — thread `leafDepTree` prop
- Modify: `inspectah-web/ui/src/components/RoutineSummary.tsx` — thread `leafDepTree` prop
- Modify: `inspectah-web/ui/src/components/MainContent.tsx` — pass `leafDepTree` from `viewData`

### Item 4 Frontend: Version Changes Section
- Modify: `inspectah-web/ui/src/components/Sidebar.tsx` — add `version_changes` to `CONTEXT_SECTIONS`
- Modify: `inspectah-web/ui/src/components/MainContent.tsx` — add `version_changes` to `contextSectionIds` + empty-state rendering
- Modify: `inspectah-web/ui/src/components/PackageDetail.tsx` — version change supplement (card-local `versionChange` prop)
- Modify: `inspectah-web/ui/src/components/DecisionItem.tsx` — resolve matching `VersionChangeEntry` and pass single `versionChange` to `PackageDetail`
- Modify: `inspectah-web/ui/src/components/DecisionList.tsx` — thread `versionChanges` array
- Modify: `inspectah-web/ui/src/components/RoutineSummary.tsx` — thread `versionChanges` array
- Modify: `inspectah-web/ui/src/hooks/useKeyboard.ts` — add `version_changes` to `SECTION_IDS`
- Modify: `inspectah-web/ui/src/components/ShortcutOverlay.tsx` — document updated key bindings

### Existing test suites that need updates for new `ViewResponse` fields
- `inspectah-web/ui/src/components/__tests__/EmptyStates.test.tsx` — `MOCK_VIEW` gains `leaf_dep_tree`, `version_changes`
- `inspectah-web/ui/src/components/__tests__/DecisionSections.test.tsx` — if it constructs `ViewResponse`
- `inspectah-web/ui/src/components/__tests__/ExportDialog.test.tsx` — if it constructs `ViewResponse`
- `inspectah-web/ui/src/hooks/__tests__/useView.test.ts` — if it constructs `ViewResponse`
- `inspectah-web/ui/src/components/__tests__/FocusAndNavigation.test.tsx` — section-jump key 4 shift
- `inspectah-web/ui/src/hooks/__tests__/useKeyboard.test.ts` — section index assertions

---

## Task 1: Add `preset_matched_units` field to `ServiceSection`

**Files:**
- Modify: `inspectah-core/src/types/services.rs`

- [ ] **Step 1: Write the failing test**

Add a test in the existing `#[cfg(test)]` module in `inspectah-core/src/types/services.rs` that verifies round-trip serialization of the new field and backward-compatible deserialization when the field is absent:

```rust
#[test]
fn test_preset_matched_units_roundtrip() {
    let section = ServiceSection {
        state_changes: Vec::new(),
        enabled_units: vec!["sshd.service".into()],
        disabled_units: Vec::new(),
        drop_ins: Vec::new(),
        preset_matched_units: vec!["chronyd.service".into(), "firewalld.service".into()],
    };
    let json = serde_json::to_string(&section).unwrap();
    let parsed: ServiceSection = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.preset_matched_units, vec!["chronyd.service", "firewalld.service"]);
}

#[test]
fn test_preset_matched_units_missing_deserializes_empty() {
    let json = r#"{"state_changes":[],"enabled_units":[],"disabled_units":[],"drop_ins":[]}"#;
    let parsed: ServiceSection = serde_json::from_str(json).unwrap();
    assert!(parsed.preset_matched_units.is_empty());
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
set -o pipefail
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-core test_preset_matched_units 2>&1 | tee /dev/stderr | tail -5
```

Expected: FAIL — `preset_matched_units` field does not exist on `ServiceSection`.

- [ ] **Step 3: Add the field to `ServiceSection`**

In `inspectah-core/src/types/services.rs`, add the field to the `ServiceSection` struct after `drop_ins`:

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
    #[serde(default)]
    pub preset_matched_units: Vec<String>,
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
set -o pipefail
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-core test_preset_matched_units 2>&1 | tee /dev/stderr | tail -5
```

Expected: PASS — both tests green.

- [ ] **Step 5: Fix any compilation errors in other crates**

The new field changes the struct construction. Grep for `ServiceSection {` across the workspace:

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && grep -rn 'ServiceSection {' --include='*.rs' | grep -v target | grep -v test
```

For each match, add `preset_matched_units: Vec::new()` (or the appropriate value). Key locations:
- `inspectah-collect/src/inspectors/services.rs` — the degraded-mode fallback (~line 110) and the main section builder (~line 190)

- [ ] **Step 6: Run full workspace build**

```bash
set -o pipefail
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo build 2>&1 | tee /dev/stderr | tail -5
```

Expected: `Finished` with no errors.

- [ ] **Step 7: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-core/src/types/services.rs inspectah-collect/src/inspectors/services.rs && git commit -m "feat(core): add preset_matched_units field to ServiceSection

Adds a new serde(default) field that carries unit names where the
current enable/disable state matches the systemd preset default.
Existing snapshots without this field deserialize with an empty vec.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 2: Populate `preset_matched_units` in the services collector

**Files:**
- Modify: `inspectah-collect/src/inspectors/services.rs`

- [ ] **Step 1: Write the failing test**

In `inspectah-collect/tests/services_test.rs` (or in the inline `#[cfg(test)]` module in the inspector), add a test that verifies the collector populates `preset_matched_units` when a unit's state matches its preset. Read the existing test file first to understand the `MockExecutor` pattern used there:

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && head -80 inspectah-collect/tests/services_test.rs
```

Using the existing mock pattern, write tests with these key assertions:

- Unit with state `"enabled"` + preset `"enable"` → in `preset_matched_units`, NOT in `state_changes`
- Unit with state `"disabled"` + preset `"disable"` → in `preset_matched_units`, NOT in `state_changes`
- Unit with state `"enabled"` + preset `"disable"` → in `state_changes`, NOT in `preset_matched_units`
- Unit with no preset rule → in neither `state_changes` nor `preset_matched_units`
- Degraded mode (preset unreadable) → `preset_matched_units` is empty

- [ ] **Step 2: Run test to verify it fails**

```bash
set -o pipefail
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-collect test_preset_match 2>&1 | tee /dev/stderr | tail -5
```

Expected: FAIL — collector does not populate the field yet.

- [ ] **Step 3: Modify the collector to populate `preset_matched_units`**

In `inspectah-collect/src/inspectors/services.rs`, step 4 (the `for unit in &units` loop, approximately lines 130-175):

1. Add a `let mut preset_matched_units = Vec::new();` accumulator alongside the existing `state_changes`, `enabled_units`, `disabled_units`.

2. In the match block where `resolve_preset()` returns `Some(ref default)` and `*default == unit.state` (the current no-op branch that falls through), add: `preset_matched_units.push(unit.unit.clone());`

The current code only records a `ServiceStateChange` when `*default != unit.state`. The matching branch is currently an implicit no-op. Change it to:

```rust
if let Some(ref default) = default_state {
    if *default != unit.state {
        // Divergence — existing code
        let action = if unit.state == "enabled" { "enable" } else { "disable" };
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
        // Match — capture for handler suppression
        preset_matched_units.push(unit.unit.clone());
    }
}
```

3. Wire into the `ServiceSection` construction (~line 190):

```rust
let section = ServiceSection {
    state_changes,
    enabled_units,
    disabled_units,
    drop_ins,
    preset_matched_units,
};
```

4. In the degraded-mode fallback (~line 110), ensure `preset_matched_units: Vec::new()`.

- [ ] **Step 4: Run tests**

```bash
set -o pipefail
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-collect 2>&1 | tee /dev/stderr | tail -10
```

Expected: All tests pass, including the new ones.

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-collect/src/inspectors/services.rs && git commit -m "feat(collect): populate preset_matched_units in services inspector

When a unit's current enable/disable state matches its systemd preset
default, the unit name is recorded in preset_matched_units instead of
being silently discarded. This gives the handler the signal it needs
to implement the three-way services contract.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 3: Implement three-way `normalize_services()` handler and add `empty_reason` to `ContextSection`

**Files:**
- Modify: `inspectah-web/src/handlers.rs`

- [ ] **Step 1: Add `empty_reason` to `ContextSection`**

Update the `ContextSection` struct (needed now for this task's return type and for Task 9):

```rust
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ContextSection {
    pub id: String,
    pub display_name: String,
    pub items: Vec<ContextItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub empty_reason: Option<String>,
}
```

Then add `empty_reason: None,` to **every** existing `normalize_*` function's return `ContextSection { ... }` block. There are 9 functions: `normalize_services`, `normalize_containers`, `normalize_users_groups`, `normalize_network`, `normalize_storage`, `normalize_scheduled_tasks`, `normalize_non_rpm_software`, `normalize_kernel_boot`, `normalize_selinux`. Also update any existing test assertions that construct `ContextSection` values.

- [ ] **Step 2: Write the failing tests for three-way services**

In the `#[cfg(test)]` module at the bottom of `handlers.rs`:

```rust
#[test]
fn test_normalize_services_three_way_split() {
    let mut snap = InspectionSnapshot::default();
    snap.services = Some(ServiceSection {
        state_changes: vec![
            ServiceStateChange {
                unit: "httpd.service".into(),
                current_state: "enabled".into(),
                default_state: "disable".into(),
                action: "enable".into(),
                include: true,
                owning_package: None,
                fleet: None,
                attention_reason: None,
            },
        ],
        enabled_units: vec![
            "httpd.service".into(),    // divergent (in state_changes)
            "chronyd.service".into(),  // matched (in preset_matched_units)
            "oddjobd.service".into(),  // preset-unknown
        ],
        disabled_units: vec![
            "cups.service".into(),     // preset-unknown
        ],
        drop_ins: Vec::new(),
        preset_matched_units: vec!["chronyd.service".into()],
    });

    let section = normalize_services(&snap);

    // chronyd.service should NOT appear (matched preset, no drop-in)
    assert!(!section.items.iter().any(|i| i.id == "chronyd.service"),
        "preset-matched unit should be suppressed");

    // httpd.service appears once (as divergence)
    assert_eq!(section.items.iter().filter(|i| i.id == "httpd.service").count(), 1);

    // oddjobd.service appears with "no preset rule" subtitle
    let oddjobd = section.items.iter().find(|i| i.id == "oddjobd.service").unwrap();
    assert!(oddjobd.subtitle.as_ref().unwrap().contains("no preset rule"));

    // cups.service appears with "no preset rule" subtitle
    let cups = section.items.iter().find(|i| i.id == "cups.service").unwrap();
    assert!(cups.subtitle.as_ref().unwrap().contains("no preset rule"));
}

#[test]
fn test_normalize_services_matched_with_dropin_visible() {
    let mut snap = InspectionSnapshot::default();
    snap.services = Some(ServiceSection {
        state_changes: Vec::new(),
        enabled_units: vec!["sshd.service".into()],
        disabled_units: Vec::new(),
        drop_ins: vec![SystemdDropIn {
            unit: "sshd.service".into(),
            path: "/etc/systemd/system/sshd.service.d/override.conf".into(),
            content: "[Service]\nTimeoutStartSec=90".into(),
            include: true,
            tie: false,
            tie_winner: false,
            fleet: None,
        }],
        preset_matched_units: vec!["sshd.service".into()],
    });

    let section = normalize_services(&snap);
    let sshd = section.items.iter().find(|i| i.id == "sshd.service");
    assert!(sshd.is_some(), "matched unit with drop-in should remain visible");
    assert!(sshd.unwrap().subtitle.as_ref().unwrap().contains("matches preset"));
    assert!(sshd.unwrap().subtitle.as_ref().unwrap().contains("drop-in"));
}

#[test]
fn test_normalize_services_legacy_snapshot_no_preset_matched() {
    let mut snap = InspectionSnapshot::default();
    snap.services = Some(ServiceSection {
        state_changes: Vec::new(),
        enabled_units: vec!["chronyd.service".into()],
        disabled_units: Vec::new(),
        drop_ins: Vec::new(),
        preset_matched_units: Vec::new(), // legacy — no signal
    });

    let section = normalize_services(&snap);
    let chronyd = section.items.iter().find(|i| i.id == "chronyd.service").unwrap();
    assert!(chronyd.subtitle.as_ref().unwrap().contains("no preset rule"));
}
```

- [ ] **Step 3: Run tests to verify they fail**

```bash
set -o pipefail
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-web test_normalize_services_three_way 2>&1 | tee /dev/stderr | tail -5
```

Expected: FAIL — current handler doesn't filter by `preset_matched_units`.

- [ ] **Step 4: Rewrite `normalize_services()` with three-way logic**

Replace the body of `normalize_services()` in `handlers.rs` (starting at line ~366). The new logic:

1. **Divergence items** (from `state_changes`) — render with subtitle `"{current_state} (diverges from preset: {default_state})"`.
2. **Preset-matched with drop-in** — units in `preset_matched_units` that ALSO have a drop-in in `drop_ins`. Subtitle: `"enabled (matches preset, has drop-in override)"`. Include drop-in content as detail.
3. **Preset-matched without drop-in** — suppressed entirely. No `ContextItem` emitted.
4. **Preset-unknown items** — enabled/disabled units NOT in `state_changes` AND NOT in `preset_matched_units`. Subtitle: `"enabled (no preset rule)"` or `"disabled (no preset rule)"`.
5. **Standalone drop-ins** — units with drop-in overrides not covered by any of the above.

```rust
fn normalize_services(snap: &InspectionSnapshot) -> ContextSection {
    let mut items = Vec::new();

    if let Some(svc) = &snap.services {
        let matched_set: std::collections::HashSet<&str> = svc
            .preset_matched_units
            .iter()
            .map(|s| s.as_str())
            .collect();
        let divergent_set: std::collections::HashSet<&str> = svc
            .state_changes
            .iter()
            .map(|sc| sc.unit.as_str())
            .collect();

        // Collect drop-in data for lookup
        let mut dropin_by_unit: std::collections::HashMap<&str, Vec<&str>> =
            std::collections::HashMap::new();
        let mut standalone_dropins = Vec::new();
        for d in &svc.drop_ins {
            if divergent_set.contains(d.unit.as_str()) || matched_set.contains(d.unit.as_str()) {
                dropin_by_unit
                    .entry(d.unit.as_str())
                    .or_default()
                    .push(&d.content);
            } else {
                standalone_dropins.push(d);
            }
        }

        // 1. Divergence items (state_changes)
        for sc in &svc.state_changes {
            let dropin_detail = dropin_by_unit
                .get(sc.unit.as_str())
                .map(|contents| contents.join("\n---\n"));
            let mut search = format!(
                "{} {} {} {}",
                sc.unit, sc.current_state, sc.default_state, sc.action
            );
            if let Some(pkg) = &sc.owning_package {
                search.push(' ');
                search.push_str(pkg);
            }
            items.push(ContextItem {
                id: sc.unit.clone(),
                title: sc.unit.clone(),
                subtitle: Some(format!(
                    "{} (diverges from preset: {})",
                    sc.current_state, sc.default_state
                )),
                detail: dropin_detail,
                searchable_text: search,
            });
        }

        // 2. Preset-matched with drop-in (stays visible)
        for unit_name in &svc.preset_matched_units {
            if let Some(dropin_contents) = dropin_by_unit.get(unit_name.as_str()) {
                let state = if svc.enabled_units.contains(unit_name) {
                    "enabled"
                } else {
                    "disabled"
                };
                items.push(ContextItem {
                    id: unit_name.clone(),
                    title: unit_name.clone(),
                    subtitle: Some(format!(
                        "{} (matches preset, has drop-in override)", state
                    )),
                    detail: Some(dropin_contents.join("\n---\n")),
                    searchable_text: format!("{} {} drop-in override", unit_name, state),
                });
            }
            // Matched without drop-in → suppressed (no item emitted)
        }

        // 3. Preset-unknown items
        for unit_name in &svc.enabled_units {
            if !divergent_set.contains(unit_name.as_str())
                && !matched_set.contains(unit_name.as_str())
            {
                let dropin_detail = dropin_by_unit
                    .get(unit_name.as_str())
                    .map(|contents| contents.join("\n---\n"));
                items.push(ContextItem {
                    id: unit_name.clone(),
                    title: unit_name.clone(),
                    subtitle: Some("enabled (no preset rule)".into()),
                    detail: dropin_detail,
                    searchable_text: format!("{} enabled no preset rule", unit_name),
                });
            }
        }
        for unit_name in &svc.disabled_units {
            if !divergent_set.contains(unit_name.as_str())
                && !matched_set.contains(unit_name.as_str())
            {
                items.push(ContextItem {
                    id: unit_name.clone(),
                    title: unit_name.clone(),
                    subtitle: Some("disabled (no preset rule)".into()),
                    detail: None,
                    searchable_text: format!("{} disabled no preset rule", unit_name),
                });
            }
        }

        // 4. Standalone drop-ins
        for d in &standalone_dropins {
            items.push(ContextItem {
                id: format!("dropin-{}", d.unit),
                title: format!("{} (drop-in)", d.unit),
                subtitle: Some("drop-in override".into()),
                detail: Some(d.content.clone()),
                searchable_text: format!("{} drop-in", d.unit),
            });
        }
    }

    ContextSection {
        id: "services".to_string(),
        display_name: "Services".to_string(),
        items,
        empty_reason: None,
    }
}
```

- [ ] **Step 5: Run tests**

```bash
set -o pipefail
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-web 2>&1 | tee /dev/stderr | tail -10
```

Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-web/src/handlers.rs && git commit -m "feat(web): implement three-way services contract in normalize_services

Divergences: shown with preset default context.
Preset-matched without drop-in: suppressed.
Preset-matched with drop-in: shown with override detail.
Preset-unknown: shown with 'no preset rule' label.

Also adds empty_reason field to ContextSection for Item 4.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 4: Add `baseline_suppressed` field to `RpmSection`

**Files:**
- Modify: `inspectah-core/src/types/rpm.rs`

- [ ] **Step 1: Write the failing test**

In the `#[cfg(test)]` module in `rpm.rs`:

```rust
#[test]
fn test_baseline_suppressed_roundtrip() {
    let mut rpm = RpmSection::default();
    rpm.baseline_suppressed = Some(vec!["kernel.x86_64".into(), "dosfstools.x86_64".into()]);
    let json = serde_json::to_value(&rpm).unwrap();
    assert_eq!(
        json["baseline_suppressed"],
        serde_json::json!(["kernel.x86_64", "dosfstools.x86_64"])
    );
}

#[test]
fn test_baseline_suppressed_none_when_absent() {
    let json = r#"{"packages_added":[],"version_changes":[],"leaf_dep_tree":{}}"#;
    let parsed: RpmSection = serde_json::from_str(json).unwrap();
    assert!(parsed.baseline_suppressed.is_none());
}

#[test]
fn test_baseline_suppressed_some_empty_when_baseline_exists_but_nothing_suppressed() {
    // Distinct from None: baseline was available but no packages matched
    let mut rpm = RpmSection::default();
    rpm.baseline_suppressed = Some(Vec::new());
    let json = serde_json::to_value(&rpm).unwrap();
    // Some([]) serializes to [] (not omitted), distinguishing from None
    assert_eq!(json["baseline_suppressed"], serde_json::json!([]));
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
set -o pipefail
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-core test_baseline_suppressed 2>&1 | tee /dev/stderr | tail -5
```

Expected: FAIL — field does not exist.

- [ ] **Step 3: Add the field to `RpmSection`**

In `inspectah-core/src/types/rpm.rs`, add to the `RpmSection` struct after `auto_packages`:

```rust
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_suppressed: Option<Vec<String>>,
```

**Semantics:** `None` = no baseline was available (cannot suppress). `Some([])` = baseline was available but no leaf candidates matched it. `Some([...])` = these packages were suppressed. This distinction matters for downstream consumers deciding whether the suppression logic ran.

- [ ] **Step 4: Run tests**

```bash
set -o pipefail
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-core test_baseline_suppressed 2>&1 | tee /dev/stderr | tail -5
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-core/src/types/rpm.rs && git commit -m "feat(core): add baseline_suppressed field to RpmSection

Optional field carrying name.arch identities of packages present in
the baseline that should be suppressed from the decision surface.
None = no baseline. Some([]) = baseline present, nothing suppressed.
Separate from auto_packages to keep leaf_dep_tree consistent.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 5: Thread `baseline_suppressed` through `classify_leaf_auto()` and `LeafClassification`

**Files:**
- Modify: `inspectah-collect/src/inspectors/rpm/mod.rs`

This is the critical seam fix. The current `classify_leaf_auto()` signature is:
```rust
fn classify_leaf_auto(exec: &dyn Executor, packages_added: &[PackageEntry]) -> LeafClassification
```
It has no access to baseline data. The `LeafClassification` struct is:
```rust
struct LeafClassification {
    leaf_packages: Option<Vec<String>>,
    auto_packages: Option<Vec<String>>,
    leaf_dep_tree: serde_json::Value,
}
```
It has no `baseline_suppressed` field. Both need to change.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn test_classify_leaf_auto_suppresses_baseline_present_packages() {
    use inspectah_core::baseline::{BaselineData, BaselinePackageEntry};
    use std::collections::HashMap;

    let exec = MockExecutor::new()
        .with_command(
            "dnf repoquery --userinstalled --queryformat %{name}.%{arch}\n",
            ExecResult {
                exit_code: 0,
                stdout: "vim.x86_64\nkernel.x86_64\n".into(),
                stderr: String::new(),
            },
        )
        .with_command(
            "dnf repoquery --requires --resolve --recursive --installed --queryformat %{name}.%{arch}\n vim.x86_64",
            ExecResult { exit_code: 0, stdout: "glibc.x86_64\n".into(), stderr: String::new() },
        )
        .with_command(
            "dnf repoquery --requires --resolve --recursive --installed --queryformat %{name}.%{arch}\n kernel.x86_64",
            ExecResult { exit_code: 0, stdout: "".into(), stderr: String::new() },
        )
        .with_command(
            "dnf repoquery --requires --resolve --recursive --installed --queryformat %{name}.%{arch}\n glibc.x86_64",
            ExecResult { exit_code: 0, stdout: "".into(), stderr: String::new() },
        );

    let added = vec![
        make_test_entry("vim", "x86_64"),
        make_test_entry("kernel", "x86_64"),
        make_test_entry("glibc", "x86_64"),
    ];

    let mut baseline_packages = HashMap::new();
    baseline_packages.insert("kernel.x86_64".into(), BaselinePackageEntry {
        name: "kernel".into(),
        arch: "x86_64".into(),
        version: "5.14.0".into(),
        release: "362.el9".into(),
        epoch: Some("0".into()),
    });

    let baseline = BaselineData {
        image_digest: "sha256:abc123".into(),
        packages: baseline_packages,
        extracted_at: "2026-01-01T00:00:00Z".into(),
    };

    let classification = classify_leaf_auto(&exec, &added, Some(&baseline));

    // kernel.x86_64 is in baseline → suppressed, not in leaf_packages
    assert_eq!(classification.leaf_packages, Some(vec!["vim.x86_64".to_string()]));
    assert_eq!(classification.baseline_suppressed, Some(vec!["kernel.x86_64".to_string()]));
    // auto_packages unchanged — only dep-graph-derived
    assert_eq!(classification.auto_packages, Some(vec!["glibc.x86_64".to_string()]));
}

#[test]
fn test_classify_leaf_auto_no_baseline_suppressed_is_none() {
    // Same as existing tests but verify baseline_suppressed is None
    let exec = MockExecutor::new()
        .with_command(
            "dnf repoquery --userinstalled --queryformat %{name}.%{arch}\n",
            ExecResult { exit_code: 0, stdout: "vim.x86_64\n".into(), stderr: String::new() },
        )
        .with_command(
            "dnf repoquery --requires --resolve --recursive --installed --queryformat %{name}.%{arch}\n vim.x86_64",
            ExecResult { exit_code: 0, stdout: "glibc.x86_64\n".into(), stderr: String::new() },
        )
        .with_command(
            "dnf repoquery --requires --resolve --recursive --installed --queryformat %{name}.%{arch}\n glibc.x86_64",
            ExecResult { exit_code: 0, stdout: "".into(), stderr: String::new() },
        );

    let added = vec![
        make_test_entry("vim", "x86_64"),
        make_test_entry("glibc", "x86_64"),
    ];

    let classification = classify_leaf_auto(&exec, &added, None);
    assert!(classification.baseline_suppressed.is_none());
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
set -o pipefail
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-collect test_classify_leaf_auto_suppresses 2>&1 | tee /dev/stderr | tail -5
```

Expected: FAIL — `classify_leaf_auto` doesn't accept a `baseline` parameter.

- [ ] **Step 3: Add `baseline_suppressed` to `LeafClassification`**

```rust
#[derive(Debug, Clone, PartialEq)]
struct LeafClassification {
    leaf_packages: Option<Vec<String>>,
    auto_packages: Option<Vec<String>>,
    leaf_dep_tree: serde_json::Value,
    baseline_suppressed: Option<Vec<String>>,  // NEW
}

impl LeafClassification {
    fn authoritative(
        leaf_packages: Vec<String>,
        auto_packages: Vec<String>,
        leaf_dep_tree: serde_json::Value,
        baseline_suppressed: Option<Vec<String>>,  // NEW
    ) -> Self {
        Self {
            leaf_packages: Some(leaf_packages),
            auto_packages: Some(auto_packages),
            leaf_dep_tree,
            baseline_suppressed,
        }
    }

    fn unavailable() -> Self {
        Self {
            leaf_packages: None,
            auto_packages: None,
            leaf_dep_tree: empty_leaf_dep_tree(),
            baseline_suppressed: None,  // No leaf data → no suppression
        }
    }
}
```

- [ ] **Step 4: Add `baseline` parameter to `classify_leaf_auto()`**

Change the signature:

```rust
fn classify_leaf_auto(
    exec: &dyn Executor,
    packages_added: &[PackageEntry],
    baseline: Option<&BaselineData>,  // NEW
) -> LeafClassification {
```

Add the suppression logic after computing the initial `leaf`/`auto` split from `user_installed`:

```rust
    // After: let (mut leaf, mut auto) = ...;
    // Before: leaf.sort(); auto.sort();

    // Suppress baseline-present packages from leaf set
    let baseline_suppressed: Option<Vec<String>> = baseline.map(|bl| {
        let mut suppressed = Vec::new();
        leaf.retain(|id| {
            if bl.packages.contains_key(id) {
                suppressed.push(id.clone());
                false  // remove from leaf
            } else {
                true   // keep in leaf
            }
        });
        suppressed.sort();
        suppressed
    });

    leaf.sort();
    auto.sort();

    // ... existing dep_tree construction ...

    LeafClassification::authoritative(leaf, auto, serde_json::Value::Object(dep_tree), baseline_suppressed)
```

- [ ] **Step 5: Update the call site in `RpmInspector::inspect()`**

In the `inspect()` method (~line 265), change:

```rust
// Before:
let leaf_classification = classify_leaf_auto(exec, &packages_added);

// After:
let leaf_classification = classify_leaf_auto(exec, &packages_added, ctx.baseline_data);
```

- [ ] **Step 6: Wire `baseline_suppressed` into `RpmSection` builder**

In the `RpmSection` construction (~line 300):

```rust
let section = RpmSection {
    packages_added,
    base_image_only,
    // ... existing fields ...
    leaf_packages: leaf_classification.leaf_packages,
    auto_packages: leaf_classification.auto_packages,
    leaf_dep_tree: leaf_classification.leaf_dep_tree,
    baseline_suppressed: leaf_classification.baseline_suppressed,  // NEW
    ..Default::default()
};
```

- [ ] **Step 7: Update existing tests that call `classify_leaf_auto`**

All existing tests pass `None` for the new `baseline` parameter since they predate baseline suppression:

```rust
// Before:
let classification = classify_leaf_auto(&exec, &added);
// After:
let classification = classify_leaf_auto(&exec, &added, None);
```

Also update existing assertions if `LeafClassification::authoritative` call sites need the new param.

- [ ] **Step 8: Run full test suite**

```bash
set -o pipefail
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test 2>&1 | tee /dev/stderr | tail -15
```

Expected: All tests pass.

- [ ] **Step 9: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-collect/src/inspectors/rpm/mod.rs && git commit -m "feat(collect): thread baseline through classify_leaf_auto for suppression

classify_leaf_auto now accepts an optional BaselineData reference.
Leaf candidates whose name.arch exists in the baseline are moved from
leaf_packages to baseline_suppressed. LeafClassification carries the
new field through to RpmSection. None = no baseline available.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 6: Exclude `baseline_suppressed` from `recompute_view()` leaf allowlist

**Files:**
- Modify: `inspectah-refine/src/session.rs`

The current leaf filter in `recompute_view()` (~line 614) builds a `leaf_set` from `rpm.leaf_packages` and keeps a package if it's in `leaf_set`, has `NeedsReview` attention, or has an operator include-delta. Baseline-suppressed packages already won't be in `leaf_set` (Task 5 removed them), **but** they could still leak through via the `NeedsReview` exception. Task 8 (attention gating) prevents that. This task adds belt-and-suspenders exclusion.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn test_baseline_suppressed_excluded_from_view_even_if_needs_review() {
    let mut snap = test_snapshot();
    let rpm = snap.rpm.as_mut().unwrap();
    rpm.packages_added = vec![
        PackageEntry {
            name: "httpd".into(),
            arch: "x86_64".into(),
            include: true,
            source_repo: "appstream".into(),
            ..Default::default()
        },
        PackageEntry {
            name: "kernel".into(),
            arch: "x86_64".into(),
            include: true,
            source_repo: "baseos".into(),
            state: PackageState::Modified,
            ..Default::default()
        },
    ];
    rpm.leaf_packages = Some(vec!["httpd.x86_64".into()]);
    rpm.auto_packages = Some(Vec::new());
    rpm.baseline_suppressed = Some(vec!["kernel.x86_64".into()]);

    let session = RefineSession::new(snap);
    let view = session.view();

    // kernel.x86_64 is baseline-suppressed — must not appear
    assert_eq!(view.packages.len(), 1);
    assert_eq!(view.packages[0].entry.name, "httpd");
    assert!(!view.packages.iter().any(|p| p.entry.name == "kernel"),
        "baseline-suppressed package must not appear in view");
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
set -o pipefail
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-refine test_baseline_suppressed_excluded 2>&1 | tee /dev/stderr | tail -5
```

- [ ] **Step 3: Add baseline_suppressed exclusion to `recompute_view()`**

In `session.rs`, find the leaf filter closure (~line 620). The current filter keeps a package if:
```rust
leaf_set.contains(package_id.as_str())
    || matches!(primary_level, Some(AttentionLevel::NeedsReview))
    || pkg.entry.include != original_include
```

Add a `baseline_suppressed_set` and exclude those packages **before** the NeedsReview exception can admit them:

```rust
let baseline_suppressed_set: std::collections::HashSet<&str> = rpm
    .baseline_suppressed
    .as_ref()
    .map(|v| v.iter().map(|s| s.as_str()).collect())
    .unwrap_or_default();

// In the filter closure:
packages.into_iter().filter(|pkg| {
    let package_id = canonical_package_id(pkg.entry.name.as_str(), pkg.entry.arch.as_str());

    // Baseline-suppressed packages never appear on the decision surface
    if baseline_suppressed_set.contains(package_id.as_str()) {
        return false;
    }

    // ... existing leaf_set / NeedsReview / include-delta logic unchanged ...
}).collect()
```

- [ ] **Step 4: Add proof test for `needs_review_count` stability**

```rust
#[test]
fn test_needs_review_count_excludes_baseline_suppressed_downgrades() {
    // Build a session with:
    // - 1 LocalInstall package (genuinely NeedsReview)
    // - 3 baseline-suppressed packages with downgrade version_changes
    // The view's needs_review_count should be exactly 1, not 4
}
```

- [ ] **Step 5: Add proof test for `RUN dnf install` suppression**

```rust
#[test]
fn test_containerfile_preview_excludes_baseline_suppressed() {
    // Build a session with:
    // - leaf_packages: ["httpd.x86_64"]
    // - baseline_suppressed: Some(["kernel.x86_64"])
    // The containerfile_preview should contain "httpd" but NOT "kernel"
}
```

- [ ] **Step 6: Run tests**

```bash
set -o pipefail
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-refine 2>&1 | tee /dev/stderr | tail -10
```

Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-refine/src/session.rs && git commit -m "feat(refine): exclude baseline_suppressed from view and containerfile

Baseline-present packages are excluded from the decision surface
unconditionally, even if they have NeedsReview attention. This is
defense-in-depth alongside the attention gating in the next commit.
Proves needs_review_count stability and RUN dnf install suppression.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 7: Populate `version_changes` in classifier AND gate attention (**ATOMIC**)

**Files:**
- Modify: `inspectah-collect/src/inspectors/rpm/classifier.rs`
- Modify: `inspectah-collect/src/inspectors/rpm/mod.rs`
- Modify: `inspectah-refine/src/attention.rs`

**Why atomic:** Populating `version_changes` without simultaneously gating attention creates a broken intermediate state where baseline-present downgrades become `NeedsReview`, violating the context-only drift model. Both changes land in one commit.

- [ ] **Step 1: Write the classifier tests**

In `classifier.rs` `#[cfg(test)]` module. Note: `PackageEntry.epoch` is `String` (not `Option`).

```rust
#[test]
fn test_classify_modified_emits_version_change() {
    let host = vec![pkg("bash", "5.2.26", "4.el9")];
    let baseline = baseline_with(&[("bash", "5.2.26", "3.el9")]);
    let result = classify_packages(&host, &baseline);
    assert_eq!(result.version_changes.len(), 1);
    assert_eq!(result.version_changes[0].name, "bash");
    assert_eq!(result.version_changes[0].host_version, "5.2.26-4.el9");
    assert_eq!(result.version_changes[0].base_version, "5.2.26-3.el9");
    assert!(matches!(result.version_changes[0].direction, VersionChangeDirection::Upgrade));
}

#[test]
fn test_classify_modified_downgrade_emits_version_change() {
    let host = vec![pkg("bash", "5.2.26", "3.el9")];
    let baseline = baseline_with(&[("bash", "5.2.26", "4.el9")]);
    let result = classify_packages(&host, &baseline);
    assert_eq!(result.version_changes.len(), 1);
    assert!(matches!(result.version_changes[0].direction, VersionChangeDirection::Downgrade));
}

#[test]
fn test_classify_same_evr_no_version_change() {
    let host = vec![pkg("bash", "5.2.26", "3.el9")];
    let baseline = baseline_with(&[("bash", "5.2.26", "3.el9")]);
    let result = classify_packages(&host, &baseline);
    assert!(result.version_changes.is_empty());
}

#[test]
fn test_classify_added_no_baseline_no_version_change() {
    let host = vec![pkg("httpd", "2.4.57", "5.el9")];
    let result = classify_packages(&host, &HashMap::new());
    assert!(result.version_changes.is_empty());
}

#[test]
fn test_classify_epoch_change_emits_version_change() {
    let mut host_pkg = pkg("glibc", "2.34", "100.el9");
    host_pkg.epoch = "1".into();
    let mut base_pkg = pkg("glibc", "2.34", "100.el9");
    base_pkg.epoch = "0".into();
    let baseline = HashMap::from([("glibc.x86_64".to_string(), base_pkg)]);
    let result = classify_packages(&[host_pkg], &baseline);
    assert_eq!(result.version_changes.len(), 1);
    assert_eq!(result.version_changes[0].host_epoch, "1");
    assert_eq!(result.version_changes[0].base_epoch, "0");
    assert!(matches!(result.version_changes[0].direction, VersionChangeDirection::Upgrade));
}

#[test]
fn test_classify_empty_vs_zero_epoch_is_not_drift() {
    // Replaces the old test_classify_baseline_none_epoch_defaults_to_empty()
    // at classifier.rs:222, which expected Modified for "" vs "0".
    //
    // After adding norm_epoch() in classify_packages(), "" and "0" are
    // normalized to "0" before rpmvercmp, so same-version packages with
    // only a trivial epoch difference are correctly classified as
    // baseline-match (Added), not drift (Modified).
    let mut host_pkg = pkg("kernel", "5.14.0", "503.el9");
    host_pkg.epoch = "0".into(); // from rpm -qa (always emits epoch)
    let mut base_pkg = PackageEntry {
        name: "kernel".into(),
        epoch: String::new(), // from baseline (None.unwrap_or_default() = "")
        version: "5.14.0".into(),
        release: "503.el9".into(),
        arch: "x86_64".into(),
        state: PackageState::BaseImageOnly,
        include: false,
        ..Default::default()
    };
    let baseline = HashMap::from([("kernel.x86_64".to_string(), base_pkg)]);
    let result = classify_packages(&[host_pkg], &baseline);
    // After epoch normalization: "0" == "0", same EVR → Added, no VersionChange
    assert_eq!(result.packages[0].state, PackageState::Added);
    assert!(result.version_changes.is_empty(),
        "'0' vs '' epoch must not produce a VersionChange after normalization");
}
```

- [ ] **Step 1b: Delete the contradictory existing test**

In `classifier.rs`, delete or replace `test_classify_baseline_none_epoch_defaults_to_empty()` (line ~222). This test currently asserts `PackageState::Modified` for `epoch "0"` vs `epoch ""` — the exact case the new `norm_epoch()` fix now classifies as `Added`. The new `test_classify_empty_vs_zero_epoch_is_not_drift` test above covers the same scenario with the correct expectation.

```rust
// DELETE this test from classifier.rs (lines 219-255):
// fn test_classify_baseline_none_epoch_defaults_to_empty()
// It asserts Modified for "0" vs "" which is now intentionally Added.
```

- [ ] **Step 2: Write the attention gating test**

In `attention.rs` `#[cfg(test)]` module:

```rust
#[test]
fn test_baseline_suppressed_package_gets_routine_not_needs_review() {
    let mut snap = InspectionSnapshot::default();
    let mut rpm = RpmSection::default();
    rpm.packages_added = vec![PackageEntry {
        name: "bash".into(),
        arch: "x86_64".into(),
        version: "5.2.26".into(),
        release: "3.el9".into(),
        epoch: String::new(),
        state: PackageState::Modified,
        include: true,
        source_repo: "baseos".into(),
        ..Default::default()
    }];
    rpm.version_changes = vec![VersionChange {
        name: "bash".into(),
        arch: "x86_64".into(),
        host_version: "5.2.26-3.el9".into(),
        base_version: "5.2.26-4.el9".into(),
        host_epoch: String::new(),
        base_epoch: String::new(),
        direction: VersionChangeDirection::Downgrade,
    }];
    rpm.baseline_suppressed = Some(vec!["bash.x86_64".into()]);
    snap.rpm = Some(rpm);

    let result = compute_package_attention(&snap);
    let bash = result.iter().find(|p| p.entry.name == "bash").unwrap();
    assert_eq!(bash.attention[0].level, AttentionLevel::Routine);
    assert_eq!(bash.attention[0].reason, AttentionReason::PackageBaselineMatch);
}

#[test]
fn test_non_suppressed_downgrade_still_gets_needs_review() {
    // A Modified downgrade NOT in baseline_suppressed should still get NeedsReview
    let mut snap = InspectionSnapshot::default();
    let mut rpm = RpmSection::default();
    rpm.packages_added = vec![PackageEntry {
        name: "httpd".into(),
        arch: "x86_64".into(),
        version: "2.4.57".into(),
        release: "4.el9".into(),
        epoch: String::new(),
        state: PackageState::Modified,
        include: true,
        source_repo: "appstream".into(),
        ..Default::default()
    }];
    rpm.version_changes = vec![VersionChange {
        name: "httpd".into(),
        arch: "x86_64".into(),
        host_version: "2.4.57-4.el9".into(),
        base_version: "2.4.57-5.el9".into(),
        host_epoch: String::new(),
        base_epoch: String::new(),
        direction: VersionChangeDirection::Downgrade,
    }];
    rpm.baseline_suppressed = Some(Vec::new()); // baseline present, but httpd not suppressed
    snap.rpm = Some(rpm);
    snap.baseline = Some(inspectah_core::baseline::BaselineData {
        image_digest: "sha256:test".into(),
        packages: std::collections::HashMap::new(),
        extracted_at: "2026-01-01T00:00:00Z".into(),
    });

    let result = compute_package_attention(&snap);
    let httpd = result.iter().find(|p| p.entry.name == "httpd").unwrap();
    assert_eq!(httpd.attention[0].level, AttentionLevel::NeedsReview);
}
```

- [ ] **Step 3a: Run classifier tests to verify they fail**

```bash
set -o pipefail
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-collect test_classify_modified_emits 2>&1 | tee /dev/stderr | tail -5
```

Expected: FAIL — `classify_packages` returns `Vec<PackageEntry>`, not `ClassificationResult`.

- [ ] **Step 3b: Run attention tests to verify they fail**

```bash
set -o pipefail
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-refine test_baseline_suppressed_package 2>&1 | tee /dev/stderr | tail -5
```

Expected: FAIL — no baseline_suppressed gating in `compute_package_attention`.

- [ ] **Step 4: Create `ClassificationResult` and refactor `classify_packages`**

In `classifier.rs`:

```rust
use inspectah_core::types::rpm::{PackageEntry, PackageState, VersionChange, VersionChangeDirection};
use std::cmp::Ordering;

pub struct ClassificationResult {
    pub packages: Vec<PackageEntry>,
    pub version_changes: Vec<VersionChange>,
}

pub fn classify_packages(
    host: &[PackageEntry],
    baseline: &HashMap<String, PackageEntry>,
) -> ClassificationResult {
    let mut version_changes = Vec::new();

    // Normalize epoch: "" and "0" are semantically equal in RPM.
    // Without this, rpmvercmp("0", "") returns Greater, causing
    // spurious Modified classification for packages where the host
    // rpm -qa emits "0" but the baseline carries "".
    let norm_epoch = |e: &str| -> &str { if e.is_empty() { "0" } else { e } };

    let packages = host.iter()
        .map(|pkg| {
            let key = format!("{}.{}", pkg.name, pkg.arch);
            let state = match baseline.get(&key) {
                None => PackageState::Added,
                Some(base) => {
                    let epoch_cmp = rpmvercmp(norm_epoch(&pkg.epoch), norm_epoch(&base.epoch));
                    let ver_cmp = rpmvercmp(&pkg.version, &base.version);
                    let rel_cmp = rpmvercmp(&pkg.release, &base.release);

                    if epoch_cmp == Ordering::Equal
                        && ver_cmp == Ordering::Equal
                        && rel_cmp == Ordering::Equal
                    {
                        PackageState::Added
                    } else {
                        let direction = if epoch_cmp != Ordering::Equal {
                            if epoch_cmp == Ordering::Greater {
                                VersionChangeDirection::Upgrade
                            } else {
                                VersionChangeDirection::Downgrade
                            }
                        } else if ver_cmp != Ordering::Equal {
                            if ver_cmp == Ordering::Greater {
                                VersionChangeDirection::Upgrade
                            } else {
                                VersionChangeDirection::Downgrade
                            }
                        } else if rel_cmp == Ordering::Greater {
                            VersionChangeDirection::Upgrade
                        } else {
                            VersionChangeDirection::Downgrade
                        };

                        version_changes.push(VersionChange {
                            name: pkg.name.clone(),
                            arch: pkg.arch.clone(),
                            host_version: format!("{}-{}", pkg.version, pkg.release),
                            base_version: format!("{}-{}", base.version, base.release),
                            host_epoch: pkg.epoch.clone(),
                            base_epoch: base.epoch.clone(),
                            direction,
                        });
                        PackageState::Modified
                    }
                }
            };
            PackageEntry {
                state,
                include: true,
                ..pkg.clone()
            }
        })
        .collect();

    ClassificationResult { packages, version_changes }
}
```

Note: `pkg.epoch` is `String`, not `Option<String>`. No `.as_deref().unwrap_or("0")` needed — compare the strings directly. The existing `rpmvercmp` handles empty strings vs "0" correctly.

Update all existing tests to use `.packages` on the result:
```rust
// Before:
let result = classify_packages(&host, &baseline);
assert_eq!(result[0].state, PackageState::Added);
// After:
let result = classify_packages(&host, &baseline);
assert_eq!(result.packages[0].state, PackageState::Added);
```

- [ ] **Step 5: Wire `version_changes` into `RpmSection` builder**

In `mod.rs`, at the `classify_packages` call site:

```rust
// Before:
let classified = classify_packages(&host_packages, &baseline_map);
// ... later:
packages_added: classified,

// After:
let classification = classify_packages(&host_packages, &baseline_map);
// ... later:
packages_added: classification.packages,
version_changes: classification.version_changes,
```

Remove the `..Default::default()` that zeroes `version_changes` in the `RpmSection` builder.

- [ ] **Step 6: Add baseline_suppressed gating to `compute_package_attention()`**

In `attention.rs`, in `compute_package_attention()`:

```rust
pub fn compute_package_attention(snap: &InspectionSnapshot) -> Vec<RefinedPackage> {
    let rpm = match &snap.rpm {
        Some(r) => r,
        None => return Vec::new(),
    };

    let baseline_names: Option<Vec<String>> = snap
        .baseline
        .as_ref()
        .map(|b| b.packages.keys().cloned().collect());
    let baseline: Option<&[String]> = baseline_names.as_deref();

    // Build baseline_suppressed set for fast lookup
    let suppressed_set: std::collections::HashSet<&str> = rpm
        .baseline_suppressed
        .as_ref()
        .map(|v| v.iter().map(|s| s.as_str()).collect())
        .unwrap_or_default();

    rpm.packages_added
        .iter()
        .map(|entry| {
            let canonical_id = format!("{}.{}", entry.name, entry.arch);

            // Baseline-suppressed: Routine(PackageBaselineMatch) regardless of version drift
            if suppressed_set.contains(canonical_id.as_str()) {
                return RefinedPackage {
                    entry: entry.clone(),
                    attention: vec![AttentionTag {
                        level: AttentionLevel::Routine,
                        reason: AttentionReason::PackageBaselineMatch,
                        detail: None,
                    }],
                };
            }

            let tag = classify_package(entry, baseline, &rpm.version_changes);
            let mut tags = vec![tag];
            // ... rest of existing sensitive-path logic unchanged ...
```

- [ ] **Step 7: Run full test suite**

```bash
set -o pipefail
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test 2>&1 | tee /dev/stderr | tail -15
```

Expected: All tests pass.

- [ ] **Step 8: Commit — SINGLE ATOMIC COMMIT for both changes**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add \
    inspectah-collect/src/inspectors/rpm/classifier.rs \
    inspectah-collect/src/inspectors/rpm/mod.rs \
    inspectah-refine/src/attention.rs \
&& git commit -m "feat(collect,refine): populate version_changes and gate attention atomically

classify_packages now returns ClassificationResult with version_changes.
Modified packages get a VersionChange with direction computed via
rpmvercmp. Simultaneously, compute_package_attention excludes
baseline-suppressed packages from attention classification, ensuring
the context-only drift invariant is never violated.

These changes MUST land together — populating version_changes without
attention gating would cause baseline-present downgrades to become
NeedsReview, violating the approved spec.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 8: Add `normalize_version_changes()` handler, `format_evr()`, and `VersionChangeEntry`

**Files:**
- Modify: `inspectah-web/src/handlers.rs`

- [ ] **Step 1: Add shared `format_evr_pair()` helper**

This is the single epoch-aware formatting path used by both the context section and `PackageDetail` supplement. It uses **paired** rendering with epoch normalization:

1. Normalize both epochs: `""` → `"0"` (semantically equal in RPM).
2. Compare the normalized values. If they differ, show epoch prefix on both sides.
3. If both are `"0"` after normalization, suppress epoch prefix on both sides.

This handles the real `base_epoch=""` vs `host_epoch="0"` edge case: after normalization both are `"0"`, so no prefix is shown (correct — they're semantically equal). But `base_epoch="0"` vs `host_epoch="1"` shows `0:` and `1:` on both sides.

```rust
/// Format a version change pair with epoch-awareness.
///
/// Normalizes "" to "0" before comparing. When normalized epochs
/// differ, both sides render with epoch prefix. When both are "0"
/// (or empty), neither side shows epoch.
fn format_evr_pair(
    base_epoch: &str, base_version: &str,
    host_epoch: &str, host_version: &str,
) -> (String, String) {
    let norm = |e: &str| -> &str { if e.is_empty() { "0" } else { e } };
    let base_norm = norm(base_epoch);
    let host_norm = norm(host_epoch);
    let show_epoch = base_norm != host_norm || (base_norm != "0");

    let fmt = |epoch: &str, version: &str| -> String {
        if show_epoch {
            let e = if epoch.is_empty() { "0" } else { epoch };
            format!("{}:{}", e, version)
        } else {
            version.to_string()
        }
    };

    (fmt(base_epoch, base_version), fmt(host_epoch, host_version))
}
```

- [ ] **Step 2: Write the failing tests**

```rust
#[test]
fn test_normalize_version_changes_downgrades_first() {
    use inspectah_core::baseline::{BaselineData, BaselinePackageEntry};

    let mut snap = InspectionSnapshot::default();
    let mut rpm = RpmSection::default();
    rpm.version_changes = vec![
        VersionChange {
            name: "vim".into(), arch: "x86_64".into(),
            host_version: "9.0.2-1.el9".into(), base_version: "9.0.1-1.el9".into(),
            host_epoch: String::new(), base_epoch: String::new(),
            direction: VersionChangeDirection::Upgrade,
        },
        VersionChange {
            name: "bash".into(), arch: "x86_64".into(),
            host_version: "5.2.26-3.el9".into(), base_version: "5.2.26-4.el9".into(),
            host_epoch: String::new(), base_epoch: String::new(),
            direction: VersionChangeDirection::Downgrade,
        },
    ];
    snap.rpm = Some(rpm);
    snap.baseline = Some(BaselineData {
        image_digest: "sha256:test".into(),
        packages: std::collections::HashMap::new(),
        extracted_at: "2026-01-01T00:00:00Z".into(),
    });

    let section = normalize_version_changes(&snap);
    assert_eq!(section.id, "version_changes");
    assert_eq!(section.items.len(), 2);
    // Downgrades sort first
    assert!(section.items[0].title.starts_with('\u{25BC}')); // ▼
    assert!(section.items[0].title.contains("bash"));
    // Upgrades after
    assert!(!section.items[1].title.starts_with('\u{25BC}'));
    assert!(section.empty_reason.is_none());
}

#[test]
fn test_normalize_version_changes_epoch_aware_subtitle() {
    let mut snap = InspectionSnapshot::default();
    let mut rpm = RpmSection::default();
    rpm.version_changes = vec![VersionChange {
        name: "glibc".into(), arch: "x86_64".into(),
        host_version: "2.34-100.el9".into(), base_version: "2.34-100.el9".into(),
        host_epoch: "1".into(), base_epoch: "0".into(),
        direction: VersionChangeDirection::Upgrade,
    }];
    snap.rpm = Some(rpm);
    snap.baseline = Some(BaselineData {
        image_digest: "sha256:test".into(),
        packages: std::collections::HashMap::new(),
        extracted_at: "2026-01-01T00:00:00Z".into(),
    });

    let section = normalize_version_changes(&snap);
    let item = &section.items[0];
    // Epoch-only change: subtitle must show epoch prefix so versions don't look identical
    assert!(item.subtitle.as_ref().unwrap().contains("1:"));
    // Paired rendering: base side also gets epoch prefix
    assert!(item.subtitle.as_ref().unwrap().contains("0:"));
}

#[test]
fn test_normalize_version_changes_epoch_only_same_evr() {
    // The dangerous case: same version-release but different epochs.
    // Without paired epoch rendering, both sides render as identical
    // "2.34-100.el9 → 2.34-100.el9" with no visible distinction.
    // The paired helper must show epoch prefix on both sides.
    let mut snap = InspectionSnapshot::default();
    let mut rpm = RpmSection::default();
    rpm.version_changes = vec![VersionChange {
        name: "glibc".into(), arch: "x86_64".into(),
        host_version: "2.34-100.el9".into(), base_version: "2.34-100.el9".into(),
        host_epoch: "2".into(), base_epoch: "1".into(),
        direction: VersionChangeDirection::Upgrade,
    }];
    snap.rpm = Some(rpm);
    snap.baseline = Some(BaselineData {
        image_digest: "sha256:test".into(),
        packages: std::collections::HashMap::new(),
        extracted_at: "2026-01-01T00:00:00Z".into(),
    });

    let section = normalize_version_changes(&snap);
    let item = &section.items[0];
    let subtitle = item.subtitle.as_ref().unwrap();
    // Both sides must show epoch prefix so they're visually distinct
    assert!(subtitle.contains("1:2.34-100.el9"), "base side must show epoch: {}", subtitle);
    assert!(subtitle.contains("2:2.34-100.el9"), "host side must show epoch: {}", subtitle);
}

#[test]
fn test_normalize_version_changes_empty_vs_zero_epoch_normalized() {
    // base_epoch="" and host_epoch="0" are semantically equal in RPM.
    // rpmvercmp("", "0") returns Equal, so the classifier will NOT
    // emit a VersionChange for this case (it's not drift). But if one
    // somehow reaches the renderer, normalization ensures no spurious
    // epoch prefix appears. This test uses different version-release
    // to make it a valid VersionChange entry.
    let mut snap = InspectionSnapshot::default();
    let mut rpm = RpmSection::default();
    rpm.version_changes = vec![VersionChange {
        name: "bash".into(), arch: "x86_64".into(),
        host_version: "5.2.26-4.el9".into(), base_version: "5.2.26-3.el9".into(),
        host_epoch: "0".into(), base_epoch: String::new(),
        direction: VersionChangeDirection::Upgrade,
    }];
    snap.rpm = Some(rpm);
    snap.baseline = Some(BaselineData {
        image_digest: "sha256:test".into(),
        packages: std::collections::HashMap::new(),
        extracted_at: "2026-01-01T00:00:00Z".into(),
    });

    let section = normalize_version_changes(&snap);
    let item = &section.items[0];
    let subtitle = item.subtitle.as_ref().unwrap();
    // After normalization, both epochs are "0" — no prefix shown
    assert!(!subtitle.contains("0:"), "normalized trivial epochs should not render: {}", subtitle);
    assert!(subtitle.contains("5.2.26-3.el9"));
    assert!(subtitle.contains("5.2.26-4.el9"));
}

#[test]
fn test_normalize_version_changes_no_baseline() {
    let mut snap = InspectionSnapshot::default();
    snap.rpm = Some(RpmSection::default());
    let section = normalize_version_changes(&snap);
    assert_eq!(section.empty_reason, Some("no_baseline".to_string()));
    assert!(section.items.is_empty());
}

#[test]
fn test_normalize_version_changes_zero_drift() {
    let mut snap = InspectionSnapshot::default();
    snap.rpm = Some(RpmSection::default());
    snap.baseline = Some(BaselineData {
        image_digest: "sha256:test".into(),
        packages: std::collections::HashMap::new(),
        extracted_at: "2026-01-01T00:00:00Z".into(),
    });
    let section = normalize_version_changes(&snap);
    assert_eq!(section.empty_reason, Some("zero_drift".to_string()));
}

#[test]
fn test_normalize_version_changes_no_rpm() {
    let snap = InspectionSnapshot::default();
    let section = normalize_version_changes(&snap);
    assert_eq!(section.empty_reason, Some("data_unavailable".to_string()));
}
```

- [ ] **Step 3: Run tests to verify they fail**

```bash
set -o pipefail
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-web test_normalize_version_changes 2>&1 | tee /dev/stderr | tail -5
```

- [ ] **Step 4: Implement `normalize_version_changes()`**

```rust
fn normalize_version_changes(snap: &InspectionSnapshot) -> ContextSection {
    let mut items = Vec::new();
    let empty_reason;

    match &snap.rpm {
        Some(rpm) => {
            let mut downgrades = Vec::new();
            let mut upgrades = Vec::new();

            for vc in &rpm.version_changes {
                let id = format!("{}.{}", vc.name, vc.arch);
                let is_downgrade = vc.direction == VersionChangeDirection::Downgrade;
                let title = if is_downgrade {
                    format!("\u{25BC} {}", id)
                } else {
                    id.clone()
                };
                let (base_evr, host_evr) = format_evr_pair(
                    &vc.base_epoch, &vc.base_version,
                    &vc.host_epoch, &vc.host_version,
                );
                let subtitle = format!(
                    "{} \u{2192} {} ({})",
                    base_evr,
                    host_evr,
                    if is_downgrade { "downgrade" } else { "upgrade" }
                );
                let item = ContextItem {
                    id: id.clone(),
                    title,
                    subtitle: Some(subtitle),
                    detail: None,
                    searchable_text: format!(
                        "{} {} {} {} {}",
                        vc.name, vc.arch, host_evr, base_evr,
                        if is_downgrade { "downgrade" } else { "upgrade" }
                    ),
                };
                if is_downgrade {
                    downgrades.push(item);
                } else {
                    upgrades.push(item);
                }
            }

            items.extend(downgrades);
            items.extend(upgrades);

            if !items.is_empty() {
                empty_reason = None;
            } else if snap.baseline.is_some() {
                empty_reason = Some("zero_drift".to_string());
            } else {
                empty_reason = Some("no_baseline".to_string());
            }
        }
        None => {
            empty_reason = Some("data_unavailable".to_string());
        }
    }

    ContextSection {
        id: "version_changes".to_string(),
        display_name: "Version Changes".to_string(),
        items,
        empty_reason,
    }
}
```

- [ ] **Step 5: Add `normalize_version_changes` to `normalize_for_context()`**

Insert after `normalize_services(snap)`:

```rust
pub fn normalize_for_context(snap: &InspectionSnapshot) -> Vec<ContextSection> {
    vec![
        normalize_services(snap),
        normalize_version_changes(snap),  // NEW — after services
        normalize_containers(snap),
        // ... rest unchanged
    ]
}
```

- [ ] **Step 6: Add `VersionChangeEntry` to `ViewResponse` and `build_view_response()`**

```rust
#[derive(Serialize)]
pub struct VersionChangeEntry {
    pub name: String,
    pub arch: String,
    pub host_version: String,
    pub base_version: String,
    pub host_epoch: String,
    pub base_epoch: String,
    pub direction: String,
}
```

Add `pub version_changes: Vec<VersionChangeEntry>` to the `ViewResponse` struct.

In `build_view_response()`:

```rust
let version_changes: Vec<VersionChangeEntry> = session.snapshot()
    .rpm.as_ref()
    .map(|rpm| rpm.version_changes.iter().map(|vc| VersionChangeEntry {
        name: vc.name.clone(),
        arch: vc.arch.clone(),
        host_version: vc.host_version.clone(),
        base_version: vc.base_version.clone(),
        host_epoch: vc.host_epoch.clone(),
        base_epoch: vc.base_epoch.clone(),
        direction: match vc.direction {
            VersionChangeDirection::Upgrade => "upgrade".to_string(),
            VersionChangeDirection::Downgrade => "downgrade".to_string(),
        },
    }).collect())
    .unwrap_or_default();
```

- [ ] **Step 7: Add audit-render proof test**

In `inspectah-pipeline` tests:

```rust
#[test]
fn test_audit_renders_version_changes_table_when_populated() {
    let mut snap = InspectionSnapshot::default();
    let mut rpm = RpmSection::default();
    rpm.version_changes = vec![VersionChange {
        name: "bash".into(), arch: "x86_64".into(),
        host_version: "5.2.26-4.el9".into(), base_version: "5.2.26-3.el9".into(),
        host_epoch: String::new(), base_epoch: String::new(),
        direction: VersionChangeDirection::Upgrade,
    }];
    snap.rpm = Some(rpm);
    let report = render_audit(&snap);
    assert!(report.contains("Version Changes"));
    assert!(report.contains("bash"));
}

#[test]
fn test_audit_omits_version_changes_table_when_empty() {
    let mut snap = InspectionSnapshot::default();
    snap.rpm = Some(RpmSection::default());
    let report = render_audit(&snap);
    assert!(!report.contains("Version Changes"));
}
```

- [ ] **Step 8: Run all tests**

```bash
set -o pipefail
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test 2>&1 | tee /dev/stderr | tail -15
```

Expected: All tests pass.

- [ ] **Step 9: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add \
    inspectah-web/src/handlers.rs \
    inspectah-pipeline/src/render/audit.rs \
&& git commit -m "feat(web): add normalize_version_changes with epoch-aware rendering

New context section with downgrade-first sort, epoch-aware format_evr()
helper, three-state empty reason, and typed VersionChangeEntry in
ViewResponse. Audit-render proof tests verify table visibility.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 9: Add `leaf_dep_tree` to ViewResponse and expose from session

**Files:**
- Modify: `inspectah-refine/src/session.rs`
- Modify: `inspectah-web/src/handlers.rs`

- [ ] **Step 1: Add `leaf_dep_tree()` method to `RefineSession`**

In `session.rs`:

```rust
pub fn leaf_dep_tree(&self) -> serde_json::Value {
    self.snapshot()
        .rpm
        .as_ref()
        .map(|rpm| rpm.leaf_dep_tree.clone())
        .unwrap_or(serde_json::json!({}))
}
```

- [ ] **Step 2: Add `leaf_dep_tree` to `ViewResponse`**

Add to the `ViewResponse` struct:

```rust
pub leaf_dep_tree: std::collections::HashMap<String, Vec<String>>,
```

In `build_view_response()`:

```rust
let is_fleet = session.snapshot().rpm.as_ref()
    .map(|rpm| rpm.packages_added.iter().any(|p| p.fleet.is_some()))
    .unwrap_or(false);

let leaf_dep_tree: std::collections::HashMap<String, Vec<String>> = if is_fleet {
    std::collections::HashMap::new()
} else {
    serde_json::from_value(session.leaf_dep_tree()).unwrap_or_default()
};
```

- [ ] **Step 3: Write backend proof test**

```rust
#[test]
fn test_view_response_leaf_dep_tree_fleet_gated() {
    // Non-fleet snapshot → leaf_dep_tree populated
    // Fleet snapshot (any package has fleet data) → empty map
}
```

- [ ] **Step 4: Run tests**

```bash
set -o pipefail
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test 2>&1 | tee /dev/stderr | tail -10
```

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-refine/src/session.rs inspectah-web/src/handlers.rs && git commit -m "feat(web): expose leaf_dep_tree in ViewResponse

Fleet-gated: returns empty map for fleet/merged snapshots.
Deserialized from serde_json::Value to typed HashMap for the frontend.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## VERIFICATION GATE: Backend complete — verify before frontend

**Critical checkpoint.** All commands use `set -o pipefail` and name specific tests.

- [ ] **V1: Full Rust test suite passes**

```bash
set -o pipefail
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test 2>&1 | tee /dev/stderr | grep -E 'test result|FAILED'
```

Expected: `test result: ok` for all crates. Zero failures.

- [ ] **V2: Clippy clean**

```bash
set -o pipefail
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo clippy -- -W clippy::all 2>&1 | tee /dev/stderr | tail -5
```

Expected: Zero warnings.

- [ ] **V3: Attention invariant — baseline-suppressed downgrades are Routine**

```bash
set -o pipefail
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-refine test_baseline_suppressed_package_gets_routine -- --nocapture 2>&1 | tee /dev/stderr | tail -5
```

Expected: PASS.

- [ ] **V4: `needs_review_count` proof**

```bash
set -o pipefail
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-refine test_needs_review_count_excludes -- --nocapture 2>&1 | tee /dev/stderr | tail -5
```

Expected: PASS.

- [ ] **V5: Containerfile suppression proof**

```bash
set -o pipefail
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-refine test_containerfile_preview_excludes_baseline -- --nocapture 2>&1 | tee /dev/stderr | tail -5
```

Expected: PASS.

- [ ] **V6: Audit render proof**

```bash
set -o pipefail
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-pipeline test_audit_renders_version_changes -- --nocapture 2>&1 | tee /dev/stderr | tail -5
```

Expected: PASS.

- [ ] **V7: Services three-way proof**

```bash
set -o pipefail
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-web test_normalize_services_three_way -- --nocapture 2>&1 | tee /dev/stderr | tail -5
```

Expected: PASS.

---

## Task 10: Update TypeScript API types

**Files:**
- Modify: `inspectah-web/ui/src/api/types.ts`

- [ ] **Step 1: Add `empty_reason` to `ContextSection`**

```typescript
export interface ContextSection {
  id: string;
  display_name: string;
  items: ContextItem[];
  empty_reason?: string;
}
```

- [ ] **Step 2: Add `VersionChangeEntry`, `leaf_dep_tree`, `version_changes` to `ViewResponse`**

```typescript
export interface VersionChangeEntry {
  name: string;
  arch: string;
  host_version: string;
  base_version: string;
  host_epoch: string;
  base_epoch: string;
  direction: "upgrade" | "downgrade";
}

export interface ViewResponse extends RefinedView {
  repo_groups: RepoGroupInfo[];
  leaf_dep_tree: Record<string, string[]>;
  version_changes: VersionChangeEntry[];
}
```

- [ ] **Step 3: Update `MOCK_VIEW` in existing test helpers**

Search for `ViewResponse` constructions across test files and add the new required fields:

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && grep -rn 'MOCK_VIEW\|ViewResponse' src --include='*.ts' --include='*.tsx' | grep -v node_modules
```

For each `ViewResponse` construction (in `EmptyStates.test.tsx`, `DecisionSections.test.tsx`, etc.), add:

```typescript
leaf_dep_tree: {},
version_changes: [],
```

- [ ] **Step 4: Run TypeScript type check**

```bash
set -o pipefail
cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx tsc --noEmit 2>&1 | tee /dev/stderr | tail -10
```

Expected: No errors.

- [ ] **Step 5: Run full Vitest suite to catch broken mocks**

```bash
set -o pipefail
cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run 2>&1 | tee /dev/stderr | tail -15
```

Expected: All existing tests pass.

- [ ] **Step 6: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-web/ui/src && git commit -m "feat(ui): add VersionChangeEntry, leaf_dep_tree, empty_reason types

Typed API contract for post-leaf features. Updates existing test mocks
to include the new required ViewResponse fields.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 11: Create `DependencyModal` component

**Files:**
- Create: `inspectah-web/ui/src/components/DependencyModal.tsx`
- Create: `inspectah-web/ui/src/components/__tests__/DependencyModal.test.tsx`

- [ ] **Step 1: Write the test**

```tsx
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi } from "vitest";
import { DependencyModal } from "../DependencyModal";

describe("DependencyModal", () => {
  const deps = ["glibc.x86_64", "ncurses-libs.x86_64", "apr.x86_64"];

  it("renders sorted dependency list", () => {
    render(
      <DependencyModal
        packageId="httpd.x86_64"
        dependencies={deps}
        isOpen={true}
        onClose={vi.fn()}
      />
    );
    expect(screen.getByText("Dependencies: httpd.x86_64")).toBeInTheDocument();
    expect(screen.getByText("(3 dependencies)")).toBeInTheDocument();
    const items = screen.getAllByRole("listitem");
    expect(items[0]).toHaveTextContent("apr.x86_64");
    expect(items[1]).toHaveTextContent("glibc.x86_64");
    expect(items[2]).toHaveTextContent("ncurses-libs.x86_64");
  });

  it("has distinct ARIA labels for dialog and list", () => {
    render(
      <DependencyModal
        packageId="httpd.x86_64"
        dependencies={deps}
        isOpen={true}
        onClose={vi.fn()}
      />
    );
    // Dialog and list have DIFFERENT aria-labels
    expect(screen.getByRole("dialog", { name: /dependencies.*httpd/i })).toBeInTheDocument();
    expect(screen.getByRole("list", { name: /dependency list.*httpd/i })).toBeInTheDocument();
  });

  it("calls onClose when close button clicked", async () => {
    const onClose = vi.fn();
    render(
      <DependencyModal
        packageId="httpd.x86_64"
        dependencies={deps}
        isOpen={true}
        onClose={onClose}
      />
    );
    await userEvent.click(screen.getByLabelText("Close"));
    expect(onClose).toHaveBeenCalled();
  });

  it("renders nothing when not open", () => {
    const { container } = render(
      <DependencyModal
        packageId="httpd.x86_64"
        dependencies={deps}
        isOpen={false}
        onClose={vi.fn()}
      />
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("places initial focus on the close button when opened", () => {
    render(
      <DependencyModal
        packageId="httpd.x86_64"
        dependencies={deps}
        isOpen={true}
        onClose={vi.fn()}
      />
    );
    // PatternFly Modal's FocusTrap places initial focus on the first
    // focusable element, which is the close "X" button
    const closeButton = screen.getByLabelText("Close");
    expect(closeButton).toHaveFocus();
  });

  it("opens via Enter key on trigger button", async () => {
    // This tests the trigger-side contract, not the modal itself.
    // Render a button that opens the modal (simulates PackageDetail usage).
    const Wrapper = () => {
      const [open, setOpen] = useState(false);
      return (
        <>
          <button onClick={() => setOpen(true)}>View Dependencies (3)</button>
          <DependencyModal
            packageId="httpd.x86_64"
            dependencies={deps}
            isOpen={open}
            onClose={() => setOpen(false)}
          />
        </>
      );
    };
    render(<Wrapper />);
    const trigger = screen.getByText("View Dependencies (3)");
    trigger.focus();
    await userEvent.keyboard("{Enter}");
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("opens via Space key on trigger button", async () => {
    const Wrapper = () => {
      const [open, setOpen] = useState(false);
      return (
        <>
          <button onClick={() => setOpen(true)}>View Dependencies (3)</button>
          <DependencyModal
            packageId="httpd.x86_64"
            dependencies={deps}
            isOpen={open}
            onClose={() => setOpen(false)}
          />
        </>
      );
    };
    render(<Wrapper />);
    const trigger = screen.getByText("View Dependencies (3)");
    trigger.focus();
    await userEvent.keyboard(" ");
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("closes on Escape key", async () => {
    const onClose = vi.fn();
    render(
      <DependencyModal
        packageId="httpd.x86_64"
        dependencies={deps}
        isOpen={true}
        onClose={onClose}
      />
    );
    await userEvent.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalled();
  });

  it("scrolls long dependency lists", () => {
    const longDeps = Array.from({ length: 60 }, (_, i) => `dep-${String(i).padStart(3, "0")}.x86_64`);
    render(
      <DependencyModal
        packageId="httpd.x86_64"
        dependencies={longDeps}
        isOpen={true}
        onClose={vi.fn()}
      />
    );
    expect(screen.getByText("(60 dependencies)")).toBeInTheDocument();
    const list = screen.getByRole("list");
    expect(list).toHaveStyle({ overflowY: "auto", maxHeight: "60vh" });
  });
});
```

Note: The `Enter`/`Space` open tests and the `useState` import need `import { useState } from "react";` at the top of the test file.

- [ ] **Step 2: Create `DependencyModal.tsx`**

```tsx
import { Modal, ModalBody, ModalHeader } from "@patternfly/react-core";

export interface DependencyModalProps {
  packageId: string;
  dependencies: string[];
  isOpen: boolean;
  onClose: () => void;
}

export function DependencyModal({
  packageId,
  dependencies,
  isOpen,
  onClose,
}: DependencyModalProps) {
  if (!isOpen) return null;

  const sorted = [...dependencies].sort();

  return (
    <Modal
      isOpen={isOpen}
      onClose={onClose}
      aria-label={`Dependencies for ${packageId}`}
      variant="medium"
      data-testid="dependency-modal"
    >
      <ModalHeader title={`Dependencies: ${packageId}`} />
      <ModalBody>
        <p style={{ marginBottom: "var(--pf-t--global--spacer--sm)" }}>
          ({sorted.length} dependencies)
        </p>
        <ul
          role="list"
          aria-label={`Dependency list for ${packageId}`}
          style={{
            listStyle: "none",
            padding: 0,
            maxHeight: "60vh",
            overflowY: "auto",
          }}
        >
          {sorted.map((dep) => (
            <li
              key={dep}
              style={{
                padding: "var(--pf-t--global--spacer--xs) var(--pf-t--global--spacer--sm)",
                fontFamily: "var(--pf-t--global--font--family--mono)",
                borderBottom: "1px solid var(--pf-t--global--border--color--default)",
              }}
            >
              {dep}
            </li>
          ))}
        </ul>
      </ModalBody>
    </Modal>
  );
}
```

Note: **Distinct ARIA labels** — dialog gets `"Dependencies for {id}"`, inner list gets `"Dependency list for {id}"`. This avoids ambiguous accessibility queries.

- [ ] **Step 3: Run tests**

```bash
set -o pipefail
cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run src/components/__tests__/DependencyModal.test.tsx 2>&1 | tee /dev/stderr | tail -10
```

- [ ] **Step 4: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-web/ui/src/components/DependencyModal.tsx inspectah-web/ui/src/components/__tests__/DependencyModal.test.tsx && git commit -m "feat(ui): add DependencyModal for leaf package dep trees

Sorted flat list of name.arch dependencies in a PatternFly Modal.
Distinct ARIA labels for dialog vs inner list. Full a11y contract.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 12: Add "View Dependencies" button to `PackageDetail` and thread through render chain

**Files:**
- Modify: `inspectah-web/ui/src/components/PackageDetail.tsx`
- Modify: `inspectah-web/ui/src/components/DecisionItem.tsx` (3 render sites at lines ~200, ~282, ~347)
- Modify: `inspectah-web/ui/src/components/DecisionList.tsx`
- Modify: `inspectah-web/ui/src/components/RoutineSummary.tsx`
- Modify: `inspectah-web/ui/src/components/MainContent.tsx`

The `PackageDetail` component is rendered at 3 sites in `DecisionItem.tsx` (lines ~200, ~282, ~347) and `DecisionItem` is rendered from both `DecisionList` and `RoutineSummary`. The prop must thread all the way from `MainContent` (which has `viewData`).

- [ ] **Step 1: Write tests**

Create or extend `inspectah-web/ui/src/components/__tests__/PackageDetail.test.tsx`:

```tsx
describe("PackageDetail dependency button", () => {
  const basePkg = {
    entry: {
      name: "httpd", arch: "x86_64", version: "2.4.57", release: "5.el9",
      epoch: "0", state: "added", include: true, source_repo: "appstream",
      fleet: null,
    },
    attention: [],
  };

  it("shows View Dependencies button when leafDepTree has entry", () => {
    const deps = { "httpd.x86_64": ["apr.x86_64", "glibc.x86_64"] };
    render(<PackageDetail pkg={basePkg as any} leafDepTree={deps} />);
    expect(screen.getByText(/View Dependencies \(2\)/)).toBeInTheDocument();
  });

  it("hides button when package not in leafDepTree", () => {
    render(<PackageDetail pkg={basePkg as any} leafDepTree={{}} />);
    expect(screen.queryByText(/View Dependencies/)).not.toBeInTheDocument();
  });

  it("hides button when leafDepTree not provided", () => {
    render(<PackageDetail pkg={basePkg as any} />);
    expect(screen.queryByText(/View Dependencies/)).not.toBeInTheDocument();
  });

  it("opens modal on click and returns focus on close", async () => {
    const deps = { "httpd.x86_64": ["apr.x86_64"] };
    render(<PackageDetail pkg={basePkg as any} leafDepTree={deps} />);
    const button = screen.getByText(/View Dependencies/);
    await userEvent.click(button);
    expect(screen.getByText("Dependencies: httpd.x86_64")).toBeInTheDocument();
    // Close modal
    await userEvent.click(screen.getByLabelText("Close"));
    // Focus returns to button
    expect(button).toHaveFocus();
  });
});
```

- [ ] **Step 2: Add `leafDepTree` prop to `PackageDetail`**

```tsx
import { useState } from "react";
import { Button } from "@patternfly/react-core";
import { DependencyModal } from "./DependencyModal";
import type { VersionChangeEntry } from "../api/types";

export interface PackageDetailProps {
  pkg: RefinedPackage;
  leafDepTree?: Record<string, string[]>;
  versionChange?: VersionChangeEntry | null;  // for Task 14 — card-local, not whole array
}
```

Render the button + modal:

```tsx
const canonicalId = `${pkg.entry.name}.${pkg.entry.arch}`;
const deps = leafDepTree?.[canonicalId];
const hasDeps = deps && deps.length > 0;

// ... existing DescriptionList ...

{hasDeps && (
  <>
    <Button
      variant="link"
      onClick={() => setDepModalOpen(true)}
      style={{ marginTop: "var(--pf-t--global--spacer--sm)" }}
    >
      View Dependencies ({deps.length})
    </Button>
    <DependencyModal
      packageId={canonicalId}
      dependencies={deps}
      isOpen={depModalOpen}
      onClose={() => setDepModalOpen(false)}
    />
  </>
)}
```

- [ ] **Step 3: Thread `leafDepTree` through the render chain**

The threading path is: `MainContent` → `DecisionList` → `DecisionItem` → `PackageDetail` AND `MainContent` → `DecisionList` → `RoutineSummary` → `DecisionItem` → `PackageDetail`.

1. `MainContent`: Pass `viewData.leaf_dep_tree` to `DecisionList` as a prop.
2. `DecisionList`: Accept `leafDepTree` prop, pass to `RoutineSummary` and `DecisionItem`.
3. `RoutineSummary`: Accept `leafDepTree` prop, pass to `DecisionItem`.
4. `DecisionItem`: Accept `leafDepTree` prop, pass to `PackageDetail` at all 3 render sites (~lines 200, 282, 347).

- [ ] **Step 4: Add RoutineSummary path proof test**

Add to `inspectah-web/ui/src/components/__tests__/RoutineSummary.test.tsx`:

```tsx
it("threads leafDepTree through to PackageDetail View Dependencies button", async () => {
  const items: DecisionItemKind[] = [{
    type: "package",
    data: {
      entry: {
        name: "httpd", epoch: "0", version: "2.4.57", release: "5.el9",
        arch: "x86_64", state: "added", include: true, source_repo: "appstream",
        fleet: null,
      },
      attention: [ROUTINE_TAG],
    },
  }];
  const leafDepTree = { "httpd.x86_64": ["apr.x86_64", "glibc.x86_64"] };

  render(
    <RoutineSummary
      items={items}
      forceExpanded={true}
      onToggleInclude={vi.fn()}
      onMarkViewed={vi.fn()}
      viewedIds={new Set()}
      isPending={false}
      leafDepTree={leafDepTree}
    />,
  );

  // Expand detail via the real expand affordance (aria-label button)
  const expandBtn = screen.getByLabelText("Expand httpd.x86_64");
  await userEvent.click(expandBtn);

  // leafDepTree survived RoutineSummary → DecisionItem → PackageDetail
  expect(screen.getByText(/View Dependencies \(2\)/)).toBeInTheDocument();
});
```

- [ ] **Step 5: Run tests**

```bash
set -o pipefail
cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run 2>&1 | tee /dev/stderr | tail -15
```

Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-web/ui/src/components/ && git commit -m "feat(ui): add View Dependencies button to leaf package cards

Button on leaf packages with non-empty dep trees. Opens DependencyModal.
Threaded through full render chain including RoutineSummary path.
Focus-return and RoutineSummary path proof tests verify a11y and threading.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 13: Add Version Changes sidebar entry and empty-state rendering

**Files:**
- Modify: `inspectah-web/ui/src/components/Sidebar.tsx`
- Modify: `inspectah-web/ui/src/components/MainContent.tsx`

- [ ] **Step 1: Add `version_changes` to `CONTEXT_SECTIONS` in Sidebar**

```typescript
const CONTEXT_SECTIONS = [
  { id: "services", label: "Services" },
  { id: "version_changes", label: "Version Changes" },
  { id: "containers", label: "Containers" },
  // ... rest unchanged
];
```

- [ ] **Step 2: Add `version_changes` to `SECTION_LABELS` and `contextSectionIds` in MainContent**

```typescript
const SECTION_LABELS: Record<string, string> = {
  // ... existing ...
  version_changes: "Version Changes",
};

const contextSectionIds = [
  "services",
  "version_changes",
  "containers",
  // ... rest
];
```

- [ ] **Step 3: Add empty-state rendering before generic context section block**

In `MainContent.tsx`, add a `version_changes` handler before the generic `contextSectionIds.includes(activeSection)` block:

```tsx
import { CubesIcon } from "@patternfly/react-icons";

// ... before the generic context block:
if (activeSection === "version_changes") {
  const section = sections?.find((s) => s.id === "version_changes");
  if (!section) {
    return (
      <PageSection>
        <Content><h2>{label}</h2></Content>
        <p>Section data not available.</p>
      </PageSection>
    );
  }

  if (section.items.length === 0 && section.empty_reason) {
    const copyMap: Record<string, string> = {
      no_baseline: "Version comparison requires a baseline. Run with --baseline to enable.",
      zero_drift: "All packages match the target baseline versions.",
      data_unavailable: "Version change data is not available for this snapshot.",
    };
    const copy = copyMap[section.empty_reason] ?? copyMap.data_unavailable;
    return (
      <PageSection>
        <Content><h2>{label}</h2></Content>
        <EmptyState titleText={copy} icon={CubesIcon} headingLevel="h3" />
      </PageSection>
    );
  }

  return (
    <PageSection>
      <Content><h2>{label}</h2></Content>
      <ContextList section={section} />
    </PageSection>
  );
}
```

- [ ] **Step 4: Write empty-state proof tests**

Add to `EmptyStates.test.tsx`:

```tsx
describe("Version Changes empty states", () => {
  it("renders no_baseline empty state", async () => {
    const { MainContent } = await import("../MainContent");
    const sections: ContextSection[] = [{
      id: "version_changes",
      display_name: "Version Changes",
      items: [],
      empty_reason: "no_baseline",
    }];
    render(
      <MainContent
        activeSection="version_changes"
        loading={false}
        viewData={{ ...MOCK_VIEW, leaf_dep_tree: {}, version_changes: [] }}
        sections={sections}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
        sectionSearchOpen={false}
        onSectionSearchClose={vi.fn()}
      />
    );
    expect(screen.getByText(/requires a baseline/)).toBeInTheDocument();
  });

  it("renders zero_drift empty state", async () => {
    const { MainContent } = await import("../MainContent");
    const sections: ContextSection[] = [{
      id: "version_changes",
      display_name: "Version Changes",
      items: [],
      empty_reason: "zero_drift",
    }];
    render(
      <MainContent
        activeSection="version_changes"
        loading={false}
        viewData={{ ...MOCK_VIEW, leaf_dep_tree: {}, version_changes: [] }}
        sections={sections}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
        sectionSearchOpen={false}
        onSectionSearchClose={vi.fn()}
      />
    );
    expect(screen.getByText(/All packages match/)).toBeInTheDocument();
  });

  it("renders data_unavailable empty state", async () => {
    const { MainContent } = await import("../MainContent");
    const sections: ContextSection[] = [{
      id: "version_changes",
      display_name: "Version Changes",
      items: [],
      empty_reason: "data_unavailable",
    }];
    render(
      <MainContent
        activeSection="version_changes"
        loading={false}
        viewData={{ ...MOCK_VIEW, leaf_dep_tree: {}, version_changes: [] }}
        sections={sections}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
        sectionSearchOpen={false}
        onSectionSearchClose={vi.fn()}
      />
    );
    expect(screen.getByText(/not available/)).toBeInTheDocument();
  });
});
```

- [ ] **Step 5: Add Sidebar.test.tsx coverage for always-present section**

In `inspectah-web/ui/src/components/__tests__/Sidebar.test.tsx`:

First, update `MOCK_SECTIONS` to include `version_changes`:
```tsx
const MOCK_SECTIONS: ContextSection[] = [
  { id: "services", display_name: "Services", items: [{ id: "s1", title: "sshd", subtitle: null, detail: null, searchable_text: "sshd" }] },
  { id: "version_changes", display_name: "Version Changes", items: [] },  // NEW — 0 items
  { id: "containers", display_name: "Containers", items: [] },
  // ... rest unchanged
];
```

Update the existing "renders all N section items" test count (11 → 12).

Then add the explicit proof:
```tsx
it("renders Version Changes in sidebar Context group with badge", () => {
  render(
    <Sidebar
      activeSection="packages"
      onSelect={vi.fn()}
      stats={MOCK_STATS}
      sections={MOCK_SECTIONS}
      health={MOCK_HEALTH}
    />,
  );

  expect(screen.getByText("Version Changes")).toBeInTheDocument();
  // Badge shows "0" for the empty section
  const vcItem = screen.getByText("Version Changes").closest("a, button, li");
  expect(vcItem).toHaveTextContent("0");
});

it("calls onSelect with version_changes when clicked", async () => {
  const onSelect = vi.fn();
  render(
    <Sidebar
      activeSection="packages"
      onSelect={onSelect}
      stats={MOCK_STATS}
      sections={MOCK_SECTIONS}
      health={MOCK_HEALTH}
    />,
  );

  await userEvent.click(screen.getByText("Version Changes"));
  expect(onSelect).toHaveBeenCalledWith("version_changes");
});
```

- [ ] **Step 6: Add app-level empty-section focus landing proof**

In `inspectah-web/ui/src/components/__tests__/FocusAndNavigation.test.tsx`, add a test that navigates to the empty `version_changes` section via the app-level section-change path and verifies focus lands correctly. This test must use the `App`-level render (matching the existing `FocusAndNavigation` pattern) to prove focus through the real navigation path, not just `MainContent` in isolation.

Update the test file's `MOCK_SECTIONS` to include `version_changes`:
```tsx
const MOCK_SECTIONS = [
  {
    id: "services",
    display_name: "Services",
    items: [{ id: "svc-1", title: "httpd.service", searchable_text: "httpd service" }],
  },
  {
    id: "version_changes",
    display_name: "Version Changes",
    items: [],
    empty_reason: "zero_drift",
  },
  // ... rest of existing mock sections
];
```

Update `MOCK_VIEW` to include the new required fields:
```tsx
const MOCK_VIEW = {
  // ... existing fields ...
  leaf_dep_tree: {},
  version_changes: [],
};
```

Add the test:
```tsx
it("navigates to empty version_changes section via key 4 and focus lands on main content", async () => {
  render(<App />);

  await waitFor(() => {
    expect(screen.getByText("Packages")).toBeInTheDocument();
  });

  // Press key 4 to navigate to version_changes
  await act(async () => {
    fireEvent.keyDown(document, { key: "4" });
  });

  // The section heading renders
  await waitFor(() => {
    expect(screen.getByText("Version Changes")).toBeInTheDocument();
  });

  // The empty-state copy is visible
  expect(screen.getByText(/All packages match/)).toBeInTheDocument();

  // Focus must land inside the main content area, not on document.body
  // or stuck in the sidebar. This is the real app-level focus proof.
  const mainContent = document.querySelector("[data-testid='main-content']")
    ?? document.querySelector("main")
    ?? document.querySelector(".pf-v6-c-page__main-section");
  expect(mainContent).not.toBeNull();
  expect(mainContent!.contains(document.activeElement)).toBe(true);
});
```

- [ ] **Step 7: Run tests**

```bash
set -o pipefail
cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run 2>&1 | tee /dev/stderr | tail -15
```

- [ ] **Step 8: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-web/ui/src/components/ && git commit -m "feat(ui): add Version Changes sidebar section with typed empty states

Three-state empty reason rendering with proof tests for each:
no_baseline, zero_drift, data_unavailable. Sidebar.test.tsx proves
section is always navigable. Empty-section focus landing verified.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 14: Add version change supplement to PackageDetail (card-local prop)

**Files:**
- Modify: `inspectah-web/ui/src/components/PackageDetail.tsx`
- Modify: `inspectah-web/ui/src/components/DecisionItem.tsx`
- Modify: `inspectah-web/ui/src/components/DecisionList.tsx`
- Modify: `inspectah-web/ui/src/components/RoutineSummary.tsx`
- Modify: `inspectah-web/ui/src/components/MainContent.tsx`

**Card-local boundary:** `DecisionItem` resolves the matching `VersionChangeEntry` from the array and passes a single `versionChange?: VersionChangeEntry | null` to `PackageDetail`. The card does NOT receive the whole `version_changes` array.

- [ ] **Step 1: Write the test**

```tsx
it("shows version change info for Modified package", () => {
  const pkg = {
    entry: {
      name: "bash", arch: "x86_64", version: "5.2.26", release: "3.el9",
      epoch: "0", state: "modified", include: true, source_repo: "baseos",
      fleet: null,
    },
    attention: [],
  };
  const vc = {
    name: "bash", arch: "x86_64",
    host_version: "5.2.26-3.el9", base_version: "5.2.26-4.el9",
    host_epoch: "", base_epoch: "",
    direction: "downgrade" as const,
  };
  render(<PackageDetail pkg={pkg as any} versionChange={vc} />);
  expect(screen.getByText(/5\.2\.26-3\.el9/)).toBeInTheDocument();
  expect(screen.getByText(/5\.2\.26-4\.el9/)).toBeInTheDocument();
  expect(screen.getByText(/downgrade/i)).toBeInTheDocument();
});

it("shows both epoch prefixes for epoch-only same-EVR change", () => {
  const pkg = {
    entry: {
      name: "glibc", arch: "x86_64", version: "2.34", release: "100.el9",
      epoch: "2", state: "modified", include: true, source_repo: "baseos",
      fleet: null,
    },
    attention: [],
  };
  const vc = {
    name: "glibc", arch: "x86_64",
    host_version: "2.34-100.el9", base_version: "2.34-100.el9",
    host_epoch: "2", base_epoch: "1",
    direction: "upgrade" as const,
  };
  render(<PackageDetail pkg={pkg as any} versionChange={vc} />);
  // Same version-release — epoch prefix is the ONLY visual distinction
  expect(screen.getByText(/1:2\.34-100\.el9/)).toBeInTheDocument();
  expect(screen.getByText(/2:2\.34-100\.el9/)).toBeInTheDocument();
});

it("does not show version change when versionChange is null", () => {
  const pkg = {
    entry: {
      name: "httpd", arch: "x86_64", version: "2.4.57", release: "5.el9",
      epoch: "0", state: "added", include: true, source_repo: "appstream",
      fleet: null,
    },
    attention: [],
  };
  render(<PackageDetail pkg={pkg as any} versionChange={null} />);
  expect(screen.queryByText("Version Change")).not.toBeInTheDocument();
});
```

- [ ] **Step 1b: Add `""` vs `"0"` epoch edge case proof**

```tsx
it("does not show epoch prefix when both sides are trivial (empty vs 0)", () => {
  const pkg = {
    entry: {
      name: "bash", arch: "x86_64", version: "5.2.26", release: "4.el9",
      epoch: "0", state: "modified", include: true, source_repo: "baseos",
      fleet: null,
    },
    attention: [],
  };
  const vc = {
    name: "bash", arch: "x86_64",
    host_version: "5.2.26-4.el9", base_version: "5.2.26-3.el9",
    host_epoch: "0", base_epoch: "",
    direction: "upgrade" as const,
  };
  render(<PackageDetail pkg={pkg as any} versionChange={vc} />);
  // Neither side shows epoch prefix (both are trivial)
  expect(screen.queryByText(/0:/)).not.toBeInTheDocument();
  expect(screen.getByText(/5\.2\.26-3\.el9/)).toBeInTheDocument();
  expect(screen.getByText(/5\.2\.26-4\.el9/)).toBeInTheDocument();
});
```

- [ ] **Step 2: Add paired epoch-aware display helper to PackageDetail**

Uses paired rendering with epoch normalization (same logic as the Rust `format_evr_pair`):

1. Normalize `""` → `"0"` on both sides.
2. If normalized epochs differ, show prefix on both. If both are `"0"`, suppress.

```tsx
function formatEvrPair(
  baseEpoch: string, baseVersion: string,
  hostEpoch: string, hostVersion: string,
): [string, string] {
  const norm = (e: string): string => (e === "" ? "0" : e);
  const baseNorm = norm(baseEpoch);
  const hostNorm = norm(hostEpoch);
  const showEpoch = baseNorm !== hostNorm || baseNorm !== "0";

  const fmt = (epoch: string, version: string): string => {
    if (showEpoch) {
      const e = epoch === "" ? "0" : epoch;
      return `${e}:${version}`;
    }
    return version;
  };

  return [fmt(baseEpoch, baseVersion), fmt(hostEpoch, hostVersion)];
}
```

- [ ] **Step 3: Add version change display to PackageDetail**

```tsx
{versionChange && (() => {
  const [baseEvr, hostEvr] = formatEvrPair(
    versionChange.base_epoch, versionChange.base_version,
    versionChange.host_epoch, versionChange.host_version,
  );
  return (
  <DescriptionListGroup>
    <DescriptionListTerm>Version Change</DescriptionListTerm>
    <DescriptionListDescription>
      <Content component="small">
        {baseEvr}
        {" → "}
        {hostEvr}
        {" "}
        <Label color={versionChange.direction === "downgrade" ? "red" : "blue"}>
          {versionChange.direction}
        </Label>
      </Content>
    </DescriptionListDescription>
  </DescriptionListGroup>
  );
})()}
```

- [ ] **Step 4: Thread through render chain (card-local resolution)**

The `versionChanges` array flows from `MainContent` → `DecisionList` → `RoutineSummary` → `DecisionItem`. But `DecisionItem` resolves the match and passes a single `versionChange` to `PackageDetail`:

In `DecisionItem.tsx`:

```tsx
// Resolve matching version change for this package
const canonicalId = `${item.data.entry.name}.${item.data.entry.arch}`;
const matchingVc = versionChanges?.find(
  (vc) => vc.name === item.data.entry.name && vc.arch === item.data.entry.arch
) ?? null;

// Then at each PackageDetail render site:
<PackageDetail pkg={item.data as RefinedPackage} leafDepTree={leafDepTree} versionChange={matchingVc} />
```

- [ ] **Step 5: Add RoutineSummary path proof for versionChange threading**

Add to `RoutineSummary.test.tsx`:

```tsx
it("threads versionChanges through to PackageDetail version change display", async () => {
  const items: DecisionItemKind[] = [{
    type: "package",
    data: {
      entry: {
        name: "bash", epoch: "0", version: "5.2.26", release: "3.el9",
        arch: "x86_64", state: "modified", include: true, source_repo: "baseos",
        fleet: null,
      },
      attention: [ROUTINE_TAG],
    },
  }];
  const versionChanges = [{
    name: "bash", arch: "x86_64",
    host_version: "5.2.26-3.el9", base_version: "5.2.26-4.el9",
    host_epoch: "", base_epoch: "",
    direction: "downgrade" as const,
  }];

  render(
    <RoutineSummary
      items={items}
      forceExpanded={true}
      onToggleInclude={vi.fn()}
      onMarkViewed={vi.fn()}
      viewedIds={new Set()}
      isPending={false}
      versionChanges={versionChanges}
    />,
  );

  // Expand detail via the real expand affordance
  const expandBtn = screen.getByLabelText("Expand bash.x86_64");
  await userEvent.click(expandBtn);

  // versionChanges survived RoutineSummary → DecisionItem → PackageDetail
  expect(screen.getByText("Version Change")).toBeInTheDocument();
  expect(screen.getByText(/downgrade/i)).toBeInTheDocument();
});
```

- [ ] **Step 6: Run tests**

```bash
set -o pipefail
cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run 2>&1 | tee /dev/stderr | tail -15
```

- [ ] **Step 7: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-web/ui/src/components/ && git commit -m "feat(ui): show version change info on package detail cards

Card-local prop: DecisionItem resolves matching VersionChangeEntry
and passes single versionChange to PackageDetail. Paired epoch-aware
formatEvrPair helper. Threaded through RoutineSummary path with proof.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 15: Update keyboard shortcuts for `version_changes` section

**Files:**
- Modify: `inspectah-web/ui/src/hooks/useKeyboard.ts`
- Modify: `inspectah-web/ui/src/components/ShortcutOverlay.tsx`

- [ ] **Step 1: Update `SECTION_IDS` in useKeyboard**

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
  "non_rpm_software",  // (was 9 — loses key)
  "kernel_boot",       // (no key)
  "selinux",           // (no key)
];
```

- [ ] **Step 2: Keep ShortcutOverlay wording aligned with approved spec**

The existing wording is `"Jump to section by index"`. Do **not** change it to `"Jump to section (4 = Version Changes)"` — the approved spec keeps the generic wording. Leave ShortcutOverlay unchanged.

```typescript
// KEEP EXISTING — no change:
{ keys: "1-9", description: "Jump to section by index" },
```

- [ ] **Step 3: Add concrete remap assertions in `useKeyboard.test.ts`**

In `inspectah-web/ui/src/hooks/__tests__/useKeyboard.test.ts`, the existing test `"calls onSectionChange with correct section on 1-9"` asserts keys `1`, `2`, `3`. Extend it (or add a new test) with explicit `4`, `5`, and `9` assertions:

```typescript
it("maps key 4 to version_changes after insertion", () => {
  const opts = makeOptions();
  renderHook(() => useKeyboard(opts));

  fireEvent.keyDown(document, { key: "4" });
  expect(opts.onSectionChange).toHaveBeenCalledWith("version_changes");
});

it("maps key 5 to containers (shifted from 4)", () => {
  const opts = makeOptions();
  renderHook(() => useKeyboard(opts));

  fireEvent.keyDown(document, { key: "5" });
  expect(opts.onSectionChange).toHaveBeenCalledWith("containers");
});

it("maps key 9 to scheduled_tasks (shifted from 8)", () => {
  const opts = makeOptions();
  renderHook(() => useKeyboard(opts));

  fireEvent.keyDown(document, { key: "9" });
  expect(opts.onSectionChange).toHaveBeenCalledWith("scheduled_tasks");
});
```

- [ ] **Step 4: Update `ShortcutOverlay.test.tsx` if it pins shortcut descriptions**

Check `inspectah-web/ui/src/components/__tests__/ShortcutOverlay.test.tsx` for assertions about the "1-9" shortcut description. Since we're keeping the wording unchanged, no update should be needed — but verify.

- [ ] **Step 5: Run tests**

```bash
set -o pipefail
cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run 2>&1 | tee /dev/stderr | tail -15
```

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-web/ui/src/hooks/ inspectah-web/ui/src/components/ShortcutOverlay.tsx && git commit -m "feat(ui): add version_changes to section-jump keyboard bindings

Key 4 now jumps to Version Changes. Containers shifts to 5, cascading
through the rest. non_rpm_software drops off the 1-9 range. Updated
existing keyboard test assertions for new mapping.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 16: Final verification

- [ ] **Step 1: Full Rust test suite**

```bash
set -o pipefail
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test 2>&1 | tee /dev/stderr | grep -E 'test result|FAILED'
```

Expected: `test result: ok` for all crates.

- [ ] **Step 2: Clippy clean**

```bash
set -o pipefail
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo clippy -- -W clippy::all 2>&1 | tee /dev/stderr | tail -5
```

Expected: Zero warnings.

- [ ] **Step 3: Full frontend test suite**

```bash
set -o pipefail
cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run 2>&1 | tee /dev/stderr | grep -E 'Tests|fail'
```

Expected: All tests pass.

- [ ] **Step 4: TypeScript type check**

```bash
set -o pipefail
cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx tsc --noEmit 2>&1 | tee /dev/stderr | tail -10
```

Expected: No errors.

- [ ] **Step 5: Start dev server and smoke test**

Use the tracked snapshot fixture at:
```
/Users/mrussell/Work/bootc-migration/inspectah/input-20260323-133834/inspection-snapshot.json
```

If this path does not exist (snapshot was cleaned up), generate a fresh one:
```bash
set -o pipefail
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo run -p inspectah-cli -- scan --output /tmp/inspectah-smoke-fixture 2>&1 | tee /dev/stderr | tail -5
```
Then use `/tmp/inspectah-smoke-fixture/inspection-snapshot.json`.

Start the dev server:
```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo run -p inspectah-web -- --snapshot /Users/mrussell/Work/bootc-migration/inspectah/input-20260323-133834/inspection-snapshot.json
```

Verify in browser:
1. Services section count is lower (three-way split working)
2. Version Changes section appears in sidebar with correct badge count
3. Version Changes shows downgrades first with ▼ prefix
4. Empty states display correct messages for no-baseline / zero-drift
5. Clicking a leaf package shows "View Dependencies" button
6. Dep modal opens, shows sorted list, closes with Escape, focus returns to button
7. Modified packages show version change info in card detail (epoch-aware)
8. Key `4` jumps to Version Changes

- [ ] **Step 6: Move spec to implemented**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && mv docs/specs/proposed/2026-05-17-post-leaf-fixes.md docs/specs/implemented/
```

- [ ] **Step 7: Update ROADMAP.md**

Mark "Post-Leaf Bug Fix Run" as COMPLETE with today's date.
