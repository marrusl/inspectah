# Refine & TUI UX Convergence — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Converge the refine web UI and TUI with the 8-group section model, add orphan full-shadow service rendering, tmpfiles.d-based /var provisioning, and fix TUI inventory toggle paths.

**Architecture:** Core type additions (Track A) flow through collection and pipeline (Track B) into web backend (Track C) and frontend (Track D). Tasks within a track are sequential; tracks A/B and C/D can overlap where dependencies allow. SDD task dependencies enforce ordering.

**Tech Stack:** Rust (core, collect, pipeline, refine, web, tui), React/TypeScript/PatternFly (web UI), Playwright (e2e tests)

**Spec:** `process-docs/specs/proposed/2026-07-06-refine-ux-convergence.md`

## Global Constraints

- Schema version stays at v21 — all new fields are `Option<T>` with `#[serde(default)]`
- `SectionGroup` in `crates/pipeline/src/section_group.rs` is the single source of truth for group membership, labels, and slugs
- `cargo clippy -- -W clippy::all` must pass with zero warnings on every commit
- `cargo fmt --check` must pass on every commit
- tmpfiles.d output goes under `/usr/lib/tmpfiles.d/` (vendor layer), never `/etc/tmpfiles.d/` (admin override)
- No inline comments in tmpfiles.d entries — comments on separate lines only
- Ownership in tmpfiles.d uses names when the account is guaranteed to exist in the target image, numeric UID:GID otherwise
- **Kit frontend tasks (T9–T14):** Invoke `/ui-ux-pro-max` skill before implementing UI components. Reference its accessibility, interaction, and PatternFly guidance for badge rendering, keyboard navigation, focus management, and component patterns
- **Tang TUI task (T5):** Invoke `/tui-design` skill before implementing TUI changes. Reference its ratatui patterns and interaction guidance

---

### Task 1: Extend SectionGroup mapping and slug contract

**Files:**
- Modify: `crates/pipeline/src/section_group.rs`

**Interfaces:**
- Produces: `SectionGroup::for_section()` covering all web-level section IDs; `SectionGroup::has_actionable_sections()` method; updated slugs

**blocked_by:** none

- [ ] **Step 1: Add missing section mappings and retired-ID contract**

Add `language_packages` to the `for_section()` match arm. Add an explicit `is_retired()` method that returns `true` for `system_tuning` and `version_changes`. All web/sidebar/batch-toggle paths must check `is_retired()` before using `for_section()` — retired IDs are excluded from routing, not silently remapped.

```rust
/// Section IDs that existed in prior sidebar versions but are no longer
/// live. They are not routed to any group in the web UI.
const RETIRED_SECTION_IDS: &[&str] = &["system_tuning", "version_changes"];

pub fn is_retired(section_id: &str) -> bool {
    RETIRED_SECTION_IDS.contains(&section_id)
}

pub fn for_section(section_name: &str) -> Self {
    match section_name {
        "rpm" | "packages" => Self::Packages,
        "config" | "configs" | "kernel_boot" | "selinux" => Self::SystemConfig,
        "services" | "scheduled_tasks" | "containers" | "compose" => Self::Services,
        "users_groups" => Self::Identity,
        "network" => Self::Network,
        "storage" => Self::Storage,
        "non_rpm_software" | "unmanaged_files" | "language_packages" => Self::Software,
        "secrets" | "subscription" => Self::Secrets,
        _ => Self::SystemConfig, // truly unknown IDs (not retired — retired IDs are caught by is_retired() before reaching this)
    }
}
```

- [ ] **Step 2: Add `has_actionable_sections()` method**

This method tells the frontend and batch-toggle handler which groups have triage sections.

```rust
pub fn has_actionable_sections(&self) -> bool {
    match self {
        Self::Packages | Self::SystemConfig | Self::Services
        | Self::Identity | Self::Software => true,
        Self::Network | Self::Storage | Self::Secrets => false,
    }
}
```

- [ ] **Step 3: Add tests for mapping completeness and actionable classification**

```rust
#[test]
fn web_section_ids_all_resolve() {
    let web_ids = [
        "packages", "configs", "kernel_boot", "selinux", "services",
        "containers", "scheduled_tasks", "compose", "users_groups",
        "network", "storage", "non_rpm_software", "unmanaged_files",
        "language_packages", "secrets", "subscription",
    ];
    for id in web_ids {
        let _ = SectionGroup::for_section(id);
    }
}

#[test]
fn retired_ids_are_explicitly_retired() {
    assert!(SectionGroup::is_retired("system_tuning"));
    assert!(SectionGroup::is_retired("version_changes"));
}

#[test]
fn live_section_ids_are_not_retired() {
    let live_ids = [
        "packages", "configs", "kernel_boot", "selinux", "services",
        "containers", "scheduled_tasks", "compose", "users_groups",
        "network", "storage", "non_rpm_software", "unmanaged_files",
        "language_packages", "secrets", "subscription",
    ];
    for id in live_ids {
        assert!(!SectionGroup::is_retired(id), "{id} should not be retired");
    }
}

#[test]
fn reference_only_groups_are_not_actionable() {
    assert!(!SectionGroup::Network.has_actionable_sections());
    assert!(!SectionGroup::Storage.has_actionable_sections());
    assert!(!SectionGroup::Secrets.has_actionable_sections());
}

#[test]
fn triage_groups_are_actionable() {
    assert!(SectionGroup::Packages.has_actionable_sections());
    assert!(SectionGroup::SystemConfig.has_actionable_sections());
    assert!(SectionGroup::Services.has_actionable_sections());
    assert!(SectionGroup::Identity.has_actionable_sections());
    assert!(SectionGroup::Software.has_actionable_sections());
}

#[test]
fn slugs_are_unique_and_stable() {
    let slugs: Vec<&str> = SectionGroup::all_in_order()
        .iter()
        .map(|g| g.slug())
        .collect();
    let mut deduped = slugs.clone();
    deduped.sort();
    deduped.dedup();
    assert_eq!(slugs.len(), deduped.len(), "slugs must be unique");
}
```

- [ ] **Step 4: Run tests, verify pass**

Run: `cargo test -p inspectah-pipeline -- section_group`

- [ ] **Step 5: Commit**

```
feat(pipeline): extend SectionGroup mapping for web convergence

Add language_packages, compose, and alternate IDs (packages, configs)
to for_section(). system_tuning and version_changes are retired (fall
through to default). Add has_actionable_sections() for batch-toggle
and sidebar menu gating. Add slug uniqueness test.
```

---

### Task 2: Core type additions — ServiceStateChange shadow fields + VarDirectory ownership/mode

**Files:**
- Modify: `crates/core/src/types/services.rs`
- Modify: `crates/core/src/types/storage.rs`
- Modify: all `VarDirectory` constructor sites (aggregate merge, fleet merge, degraded path, test constructors)

**Interfaces:**
- Produces: `ServiceStateChange.shadow_type: Option<ShadowType>`, `ServiceStateChange.shadow_rationale: Option<String>`; `VarDirectory.mode: Option<String>`, `VarDirectory.owner_name: Option<String>`, `VarDirectory.group_name: Option<String>`, `VarDirectory.owner_uid: Option<u32>`, `VarDirectory.owner_gid: Option<u32>`

**blocked_by:** none

- [ ] **Step 1: Add shadow fields to ServiceStateChange**

```rust
// In crates/core/src/types/services.rs, add to ServiceStateChange:
#[serde(default, skip_serializing_if = "Option::is_none")]
pub shadow_type: Option<ShadowType>,
#[serde(default, skip_serializing_if = "Option::is_none")]
pub shadow_rationale: Option<String>,
```

**`current_state` stays unchanged.** `ServiceStateChange.current_state` remains `ServiceUnitState` (the existing `Enabled` / `Disabled` / `Masked` enum), preserving the current service/session/export contract. Because that contract cannot represent "state unavailable" without a schema change, orphan full-shadow synthesis in Task 4 only creates synthetic `ServiceStateChange` entries when durable host state is already known from the existing service inventory (`enabled_units` or `disabled_units`). Orphan full-shadow units whose durable state is unavailable are intentionally not synthesized in this plan; supporting that case would require an explicit contract change beyond the approved scope.

- [ ] **Step 2: Add ownership/mode fields to VarDirectory**

```rust
// In crates/core/src/types/storage.rs, add to VarDirectory:
#[serde(default, skip_serializing_if = "Option::is_none")]
pub mode: Option<String>,
#[serde(default, skip_serializing_if = "Option::is_none")]
pub owner_name: Option<String>,
#[serde(default, skip_serializing_if = "Option::is_none")]
pub group_name: Option<String>,
#[serde(default, skip_serializing_if = "Option::is_none")]
pub owner_uid: Option<u32>,
#[serde(default, skip_serializing_if = "Option::is_none")]
pub owner_gid: Option<u32>,
```

- [ ] **Step 3: Fix all constructor sites**

Search for existing `VarDirectory {` constructor calls and add `..Default::default()` or explicit `None` fields. Key sites:
- `crates/core/src/aggregate/merge.rs`
- `crates/core/src/fleet/merge.rs`
- `crates/collect/src/inspectors/storage.rs` (degraded path)
- `crates/refine/src/projection/reference.rs` (test constructors)

For `ServiceStateChange`, search for constructor calls and add `shadow_type: None, shadow_rationale: None`. Key sites:
- `crates/collect/src/inspectors/services.rs`
- Any test constructors in `crates/refine/`

- [ ] **Step 4: Verify compilation**

Run: `cargo build --workspace`

- [ ] **Step 5: Add deserialization test for backward compatibility**

```rust
#[test]
fn var_directory_without_ownership_deserializes() {
    let json = r#"{"path":"/var/lib/test","size_estimate":"10M","recommendation":"review"}"#;
    let dir: VarDirectory = serde_json::from_str(json).unwrap();
    assert_eq!(dir.path, "/var/lib/test");
    assert!(dir.owner_uid.is_none());
    assert!(dir.mode.is_none());
}

#[test]
fn service_state_change_without_shadow_deserializes() {
    let json = r#"{"unit":"sshd.service","current_state":"enabled","default_state":null,"disposition":{"kind":"Actionable","include":true}}"#;
    let ssc: ServiceStateChange = serde_json::from_str(json).unwrap();
    assert_eq!(ssc.current_state, ServiceUnitState::Enabled);
    assert!(ssc.shadow_type.is_none());
}
```

- [ ] **Step 6: Run tests, commit**

Run: `cargo test --workspace`

```
feat(core): add shadow fields to ServiceStateChange and ownership/mode to VarDirectory

Optional fields with serde(default) — no schema version bump.
Backward-compatible with existing v21 snapshots.
```

---

### Task 3: /var ownership and mode collection

**Files:**
- Modify: `crates/collect/src/inspectors/storage.rs`

**Interfaces:**
- Consumes: `VarDirectory` fields from Task 2
- Produces: populated `mode`, `owner_name`, `group_name`, `owner_uid`, `owner_gid` on each `VarDirectory`

**blocked_by:** Task 2

- [ ] **Step 1: Modify `discover_var_directories()` to capture ownership and mode**

After each directory is discovered, call `stat` to capture mode and ownership in a single call:

```rust
let stat_result = exec.run("stat", &["-c", "%04a %u %g %U %G", &dir_path]);
if stat_result.exit_code == 0 {
    let parts: Vec<&str> = stat_result.stdout.trim().splitn(5, ' ').collect();
    if parts.len() == 5 {
        var_dir.mode = Some(parts[0].to_string());
        var_dir.owner_uid = parts[1].parse().ok();
        var_dir.owner_gid = parts[2].parse().ok();
        var_dir.owner_name = Some(parts[3].to_string());
        var_dir.group_name = Some(parts[4].to_string());
    }
} else {
    // stat failed — record degradation, leave fields as None
    // (renderer falls back to RUN mkdir)
}
```

- [ ] **Step 2: Add unit tests with mock executor**

Test cases: root-owned directory (0:0:root:root), packaged service account (26:26:postgres:postgres), non-default mode (2770), stat failure (degradation path).

```rust
#[test]
fn discover_var_directories_captures_ownership() {
    let mut mock = MockExecutor::new();
    // Set up mock to return stat output for a postgres-owned directory
    mock.set_output("stat -c %04a %u %g %U %G /var/lib/pgsql/data", "0750 26 26 postgres postgres");
    // ... run discover and verify fields populated
}
```

- [ ] **Step 3: Run tests, commit**

Run: `cargo test -p inspectah-collect -- storage`

```
feat(collect): capture /var directory ownership and mode via stat
```

---

### Task 4: Orphan full-shadow synthesis and lifecycle

**Files:**
- Modify: `crates/refine/src/classify.rs`
- Modify: `crates/refine/src/session.rs` (if synthesis must happen before session construction)

**Interfaces:**
- Consumes: `ServiceStateChange` shadow fields from Task 2, `ServiceSection.drop_ins` and `ServiceSection.state_changes`
- Produces: synthetic `ServiceStateChange` entries for orphan full-shadow services, present in `state_changes` BEFORE `RefineSession::new()` is called, so they are real toggle targets through the full lifecycle

**blocked_by:** Task 2

- [ ] **Step 1: Add synthesis function in classify.rs**

Add `synthesize_orphan_shadows()` as a public function in `crates/refine/src/classify.rs`. It mutates the snapshot's `services.state_changes` in place. The existing session construction path already calls `classify_services()` from this module — `synthesize_orphan_shadows()` is called from within `classify_services()` at the top, before building the `states` vec. This keeps all classification/synthesis logic at the classify boundary without touching session construction call sites.

```rust
/// Inject synthetic ServiceStateChange entries for orphan full-shadow
/// services. Called at the top of classify_services() so the session
/// sees these as real toggle targets.
pub fn synthesize_orphan_shadows(services: &mut ServiceSection) {
    let existing_units: HashSet<&str> = services
        .state_changes
        .iter()
        .map(|s| s.unit.as_str())
        .collect();

    let mut synthetics = Vec::new();
    for dropin in &services.drop_ins {
        if dropin.shadow_type == Some(ShadowType::FullShadow)
            && !existing_units.contains(dropin.unit.as_str())
        {
            // Derive durable host state from the existing service inventory.
            // Only synthesize when the current ServiceStateChange contract can
            // represent that state faithfully.
            let current_state = if services.disabled_units.contains(&dropin.unit) {
                Some(ServiceUnitState::Disabled)
            } else if services.enabled_units.contains(&dropin.unit) {
                Some(ServiceUnitState::Enabled)
            } else {
                None
            };

            let Some(current_state) = current_state else {
                // The current contract cannot encode "state unavailable" without
                // changing ServiceStateChange.current_state, so skip synthesis for
                // orphan full-shadow units whose durable state is not known.
                continue;
            };

            synthetics.push(ServiceStateChange {
                unit: dropin.unit.clone(),
                current_state,
                default_state: None,
                disposition: FindingKind::included(),
                locked: false,
                owning_package: None,
                aggregate: None,
                attention_reason: None,
                shadow_type: Some(ShadowType::FullShadow),
                shadow_rationale: Some(
                    "base image updates to this unit will be silently ignored".to_string(),
                ),
            });
        }
    }
    services.state_changes.extend(synthetics);
}
```

- [ ] **Step 2: Call synthesis from classify_services()**

At the top of `classify_services()`, call `synthesize_orphan_shadows()` on the snapshot's services before building the states vec. Since `classify_services()` takes `&InspectionSnapshot` (immutable), change it to take `&mut InspectionSnapshot` so the synthesis can mutate `state_changes`. Update the single call site in the session construction path to pass `&mut snap`.

```rust
pub fn classify_services(
    snap: &mut InspectionSnapshot,
) -> (Vec<RefinedServiceState>, Vec<RefinedDropIn>) {
    let services = match snap.services.as_mut() {
        Some(s) => s,
        None => return (Vec::new(), Vec::new()),
    };

    // Synthesize orphan full-shadow entries before classification.
    synthesize_orphan_shadows(services);

    let states: Vec<RefinedServiceState> = services
        .state_changes
        .iter()
        .map(|change| RefinedServiceState {
            entry: change.clone(),
            triage: classify_service(change),
        })
        .collect();
    // ... rest unchanged
}
```

- [ ] **Step 3: Add unit test for synthesis**

```rust
#[test]
fn orphan_full_shadow_gets_synthetic_state_change() {
    let mut snap = test_snapshot();
    snap.services = Some(ServiceSection {
        state_changes: vec![],
        drop_ins: vec![SystemdDropIn {
            unit: "sshd.service".into(),
            path: "/etc/systemd/system/sshd.service".into(),
            shadow_type: Some(ShadowType::FullShadow),
            shadow_rationale: Some("base image updates...".into()),
            ..Default::default()
        }],
        ..Default::default()
    });
    synthesize_orphan_shadows(&mut snap);
    let sc = &snap.services.unwrap().state_changes;
    assert_eq!(sc.len(), 1);
    assert_eq!(sc[0].unit, "sshd.service");
    assert_eq!(sc[0].shadow_type, Some(ShadowType::FullShadow));
}

#[test]
fn non_orphan_full_shadow_not_duplicated() {
    let mut snap = test_snapshot();
    snap.services = Some(ServiceSection {
        state_changes: vec![ServiceStateChange {
            unit: "sshd.service".into(),
            ..test_service_state_change()
        }],
        drop_ins: vec![SystemdDropIn {
            unit: "sshd.service".into(),
            shadow_type: Some(ShadowType::FullShadow),
            ..Default::default()
        }],
        ..Default::default()
    });
    synthesize_orphan_shadows(&mut snap);
    assert_eq!(snap.services.unwrap().state_changes.len(), 1, "should not duplicate");
}
```

- [ ] **Step 4: Add unavailable-state boundary test**

Document the current contract boundary: if an orphan full-shadow unit appears in neither `enabled_units` nor `disabled_units`, do not synthesize a `ServiceStateChange`.

```rust
#[test]
fn orphan_shadow_without_durable_state_is_not_synthesized() {
    let mut snap = test_snapshot();
    snap.services = Some(ServiceSection {
        state_changes: vec![],
        enabled_units: vec![],
        disabled_units: vec![],
        drop_ins: vec![SystemdDropIn {
            unit: "sshd.service".into(),
            shadow_type: Some(ShadowType::FullShadow),
            ..Default::default()
        }],
        ..Default::default()
    });

    let (states, _) = classify_services(&mut snap);
    assert!(states.is_empty(), "skip synthesis when durable state is unavailable");
}
```

- [ ] **Step 5: Add full lifecycle round-trip test (known-state shadow)**

Prove the synthetic entry survives the complete lifecycle:

```rust
#[test]
fn orphan_shadow_survives_full_lifecycle() {
    let mut snap = test_snapshot_with_orphan_shadow("sshd.service");
    let (states, _) = classify_services(&mut snap);
    assert_eq!(states.len(), 1, "synthetic entry exists after classify");

    // 1. Session construction — validates synthetic entry
    let session = RefineSession::new(snap.clone()).unwrap();

    // 2. Toggle mutation — exclude the shadow service
    let op = RefinementOp::SetInclude {
        item_id: ItemId::Service { unit: "sshd.service".into() },
        include: false,
    };
    let session = session.apply(op).unwrap();

    // 3. Autosave — serialize session to JSON
    let json = serde_json::to_string(&session.to_saveable()).unwrap();

    // 4. Reload — deserialize and reconstruct
    let reloaded = RefineSession::from_saved(&json, snap.clone()).unwrap();
    assert!(!reloaded.is_included(&ItemId::Service { unit: "sshd.service".into() }),
        "exclude decision must persist across save/reload");
}
```

- [ ] **Step 6: Add export omission test (THIS TASK OWNS IT)**

Prove that excluding a synthetic orphan full-shadow keeps it out of the Containerfile output. This is the export contract test — no other task covers it.

```rust
#[test]
fn excluded_orphan_shadow_omitted_from_containerfile() {
    let mut snap = test_snapshot_with_orphan_shadow("sshd.service");
    classify_services(&mut snap);

    let session = RefineSession::new(snap.clone()).unwrap();
    let session = session.apply(RefinementOp::SetInclude {
        item_id: ItemId::Service { unit: "sshd.service".into() },
        include: false,
    }).unwrap();

    let projected = session.project();
    let containerfile = render_containerfile(&projected, None, None);
    assert!(!containerfile.contains("sshd.service"),
        "excluded shadow must not appear in Containerfile");
}
```

- [ ] **Step 7: Add stale-session reconciliation pruning test**

Prove that when the shadow file disappears on re-scan, stale session decisions referencing it are pruned during session/snapshot reconciliation — not just that synthesis stops.

```rust
#[test]
fn stale_shadow_decision_pruned_on_rescan() {
    // First scan: orphan shadow exists, user excludes it
    let mut snap1 = test_snapshot_with_orphan_shadow("sshd.service");
    classify_services(&mut snap1);
    let session = RefineSession::new(snap1).unwrap();
    let session = session.apply(RefinementOp::SetInclude {
        item_id: ItemId::Service { unit: "sshd.service".into() },
        include: false,
    }).unwrap();
    let saved = serde_json::to_string(&session.to_saveable()).unwrap();

    // Re-scan: shadow file is gone
    let mut snap2 = test_snapshot(); // no shadow drop-ins
    snap2.services = Some(ServiceSection::default());
    classify_services(&mut snap2);
    assert!(snap2.services.as_ref().unwrap().state_changes.is_empty(),
        "no synthesis when shadow is gone");

    // Reload saved session against new snapshot — stale decision pruned
    let reloaded = RefineSession::from_saved(&saved, snap2).unwrap();
    // The stale "sshd.service" exclude decision should be pruned
    // because the unit no longer exists in state_changes.
    // Verify it does not appear in the projected view.
    let projected = reloaded.project();
    assert!(projected.services.is_none()
        || projected.services.as_ref().unwrap().state_changes.is_empty(),
        "stale shadow decision must be pruned on reconciliation");
}
```

- [ ] **Step 8: Run tests, commit**

Run: `cargo test -p inspectah-refine -- classify && cargo test -p inspectah-refine -- session`

```
feat(refine): synthesize orphan full-shadow services before session construction

Orphan full-shadows get synthetic ServiceStateChange entries injected
into the snapshot before RefineSession::new(). Survives toggle,
autosave, reload, and export. Pruned on re-scan when shadow disappears.
```

---

### Task 5: TUI inventory-row modeling fix

**Files:**
- Modify: `crates/tui/src/screen/single_host.rs`
- Modify: `crates/tui/src/app.rs`
- Modify: `crates/refine/src/session.rs`

**Interfaces:**
- Consumes: `ListItem::inventory()` constructor (existing)
- Produces: network items rendered as non-toggleable inventory rows; session rejects inventory ItemId variants

**blocked_by:** none (independent)

- [ ] **Step 1: Switch network items from `RawItem::new()` to `ListItem::inventory()`**

In `single_host.rs`, the network section (`SectionId::Network`) builds items using `RawItem::new()`. Replace each `RawItem::new()` call within the `SectionId::Network` match arm with `RawItem::inventory()`. `RawItem::inventory()` is the TUI-layer builder that produces a `ListItem` with `is_inventory: true` via the downstream `ListItem::inventory()` constructor. The ifcfg deprecation advisory stays as `RawItem::advisory()`.

Items to convert: connections, firewall zones, static routes, IP routes, IP rules, resolv entries, hosts entries, proxy config. All use `RawItem::inventory()` — no exceptions within the network section except the ifcfg advisory.

- [ ] **Step 2: Add `is_inventory` to toggle guard in app.rs**

Find the toggle guard that checks `is_advisory` and add the inventory check:

```rust
if item.is_advisory || item.is_inventory {
    return; // non-toggleable
}
```

- [ ] **Step 3: Add session-level rejection for inventory ItemId variants**

In `session.rs`, first add a new variant to `RefineError`:

```rust
// In the RefineError enum (uses thiserror):
#[error("inventory item is not toggleable: {0}")]
InventoryNotToggleable(String),
```

Then in `validate_target()`, add rejection for inventory-type ItemIds (e.g., `ItemId::NMConnection`, `ItemId::FirewallZone`):

```rust
ItemId::NMConnection { .. } | ItemId::FirewallZone { .. } => {
    return Err(RefineError::InventoryNotToggleable(
        format!("{:?}", item_id),
    ));
}
```

- [ ] **Step 4: Add tests**

Test the toggle guard rejects inventory items. Test session validation rejects inventory ItemId variants.

- [ ] **Step 5: Run tests, commit**

Run: `cargo test -p inspectah-tui && cargo test -p inspectah-refine -- session`

```
fix(tui): use ListItem::inventory() for network items

Network items are display-only inventory — not toggleable.
Adds is_inventory guard in app.rs and session-level rejection
for inventory ItemId variants.
```

---

### Task 6: tmpfiles.d renderer and config tree staging

**Files:**
- Modify: `crates/pipeline/src/render/containerfile.rs`
- Modify: `crates/pipeline/src/render/mod.rs`
- Modify: `crates/pipeline/src/render/configtree.rs` (if tmpfiles.d is staged via config tree)
- Modify: `crates/pipeline/src/render/readme.rs`

**Interfaces:**
- Consumes: `VarDirectory` ownership/mode fields from Task 2, user materialization output from `render/users.rs`
- Produces: `render_tmpfiles_conf()` function; staged `config/usr/lib/tmpfiles.d/inspectah-var.conf`; updated Containerfile COPY directive

**blocked_by:** Task 2, Task 3

- [ ] **Step 1: Write `render_tmpfiles_conf()` function**

In `containerfile.rs`, add a function that generates the tmpfiles.d conf file content. The function processes each unbacked `VarDirectory` independently — **per-directory mixed output** is required. Directories with complete ownership/mode data get tmpfiles entries; directories missing that data are skipped (and handled by `var_dir_section_lines()` as `RUN mkdir -p` fallback). This means a single snapshot can produce BOTH a tmpfiles.d conf AND `RUN mkdir` lines for different directories.

**Ownership resolution inputs:** The function receives the full `InspectionSnapshot`, which provides:
- `snap.rpm.packages_added` — the list of RPMs being replicated (for case 3: package scriptlets create the account)
- User materialization output is derived from `snap.users_groups` — the same data `render/users.rs` uses to emit `useradd`/`groupadd` (for case 2)

Follow the spec's ownership resolution order:
1. Root (UID 0) → `root root`
2. Account present in user materialization output → name
3. Account owned by an RPM being replicated → name + `# created by package:` comment
4. Name available but not guaranteed → numeric UID:GID + `# account 'X' not guaranteed` comment
5. Name unavailable → numeric

Comments go on separate lines above the entry. No inline comments.

```rust
fn render_tmpfiles_conf(snap: &InspectionSnapshot) -> Option<String> {
    let storage = snap.storage.as_ref()?;
    let materialized_users = collect_materialized_usernames(snap);
    let replicated_rpms = collect_replicated_rpm_names(snap);
    let mut lines = Vec::new();

    for d in &storage.var_directories {
        if d.backing != Some(VarDirBacking::Unbacked) {
            continue;
        }
        // Per-directory: skip directories missing mode/ownership data.
        // These get RUN mkdir fallback in var_dir_section_lines().
        let mode = match &d.mode {
            Some(m) => m.as_str(),
            None => continue,
        };
        let (owner, group, comment) = resolve_tmpfiles_ownership(
            d, &materialized_users, &replicated_rpms,
        );
        if !comment.is_empty() {
            lines.push(format!("# {}", comment));
        }
        lines.push(format!("d {} {} {} {} -", d.path, mode, owner, group));
    }

    if lines.is_empty() {
        return None;
    }

    Some(lines.join("\n") + "\n")
}
```

- [ ] **Step 2: Update `var_dir_section_lines()` for per-directory mixed output**

The existing `var_dir_section_lines()` now emits `RUN mkdir -p` ONLY for unbacked directories that are NOT covered by the tmpfiles.d conf (i.e., those with `mode == None`). Directories with complete metadata are handled by the tmpfiles.d file and should NOT also get `RUN mkdir`. If a tmpfiles.d file was generated and staged, emit a `COPY` directive for it at the top of the section.

```rust
fn var_dir_section_lines(snap: &InspectionSnapshot, tmpfiles_staged: bool) -> Vec<String> {
    let storage = match &snap.storage {
        Some(s) => s,
        None => return Vec::new(),
    };
    let mut body = Vec::new();

    if tmpfiles_staged {
        body.push("# /var directories with known ownership/mode provisioned via tmpfiles.d".into());
        body.push("COPY config/usr/lib/tmpfiles.d/inspectah-var.conf /usr/lib/tmpfiles.d/inspectah-var.conf".into());
    }

    // Fallback: RUN mkdir for unbacked dirs WITHOUT ownership/mode data
    for d in &storage.var_directories {
        if d.backing == Some(VarDirBacking::Unbacked) && d.mode.is_none() {
            body.push(format!("RUN mkdir -p {}", d.path));
        }
    }

    section("/var directory provisioning", body)
}
```

- [ ] **Step 3: Stage the file in `render_all()`**

In `mod.rs`, after `configtree::write_config_tree()` and before `containerfile::render_containerfile()`, generate the tmpfiles.d content and write it to the staged path:

```rust
// 8c. tmpfiles.d — /var directory provisioning (before Containerfile rendering)
let tmpfiles_staged = if let Some(tmpfiles_content) = containerfile::render_tmpfiles_conf(snap) {
    let tmpfiles_dir = output_dir.join("config/usr/lib/tmpfiles.d");
    std::fs::create_dir_all(&tmpfiles_dir)?;
    std::fs::write(tmpfiles_dir.join("inspectah-var.conf"), &tmpfiles_content)?;
    true
} else {
    false
};
// Pass tmpfiles_staged to render_containerfile so var_dir_section_lines()
// knows whether to emit COPY or only RUN mkdir fallback.
```

- [ ] **Step 4: Update README generation**

In `readme.rs`, add a line documenting the tmpfiles.d artifact when it exists.

- [ ] **Step 5: Add unit tests**

Test `render_tmpfiles_conf()` output format: separate-line comments, correct field ordering, ownership resolution for each case (root, packaged, non-system, unresolvable, stat failure). Test that `None` mode/ownership produces no tmpfiles.d file. Test non-default modes (0700, 2770, 1777). **Critical mixed-data test:** snapshot with 3 unbacked dirs — one with full metadata (→ tmpfiles entry), one with mode but no owner (→ tmpfiles with numeric fallback), one with no mode at all (→ `RUN mkdir` fallback). Verify the tmpfiles conf has 2 entries and `var_dir_section_lines()` emits 1 `RUN mkdir` line.

- [ ] **Step 6: Run tests, commit**

Run: `cargo test -p inspectah-pipeline -- containerfile && cargo test -p inspectah-pipeline -- render`

```
feat(pipeline): emit tmpfiles.d for /var directory provisioning

Generates /usr/lib/tmpfiles.d/inspectah-var.conf for unbacked /var
directories with ownership and mode data. Staged as config tree
artifact. Falls back to RUN mkdir for snapshots without ownership data.
```

---

### Task 7: Batch-toggle handler slug migration

**Files:**
- Modify: `crates/web/src/handlers.rs`
- Modify: `crates/web/src/lib.rs` (route registration)

**Interfaces:**
- Consumes: `SectionGroup::slug()`, `SectionGroup::has_actionable_sections()` from Task 1
- Produces: batch-toggle routes keyed by `SectionGroup::slug()` values; reference-only groups rejected

**blocked_by:** Task 1

- [ ] **Step 1: Migrate batch-toggle handler to use SectionGroup slugs**

Replace the ad-hoc group name matching with `SectionGroup` lookup. The route parameter is the slug value. Reference-only groups are not registered as routes (return 404).

```rust
pub async fn batch_toggle_group(
    Path(group_slug): Path<String>,
    State(state): State<AppState>,
    Json(payload): Json<BatchTogglePayload>,
) -> Result<Json<ViewResponse>, StatusCode> {
    let group = SectionGroup::all_in_order()
        .iter()
        .find(|g| g.slug() == group_slug)
        .ok_or(StatusCode::NOT_FOUND)?;

    if !group.has_actionable_sections() {
        return Err(StatusCode::NOT_FOUND);
    }

    // ... toggle logic per group
}
```

- [ ] **Step 2: Update route registration**

In `lib.rs`, change the route from the current path to use the slug-based pattern:
```rust
.route("/api/batch-toggle/:group_slug", post(handlers::batch_toggle_group))
```

- [ ] **Step 3: Expand group → ItemId mapping for all actionable groups**

Add `ItemId` iteration for Identity (users_groups), Software (language_packages, unmanaged_files), and any remaining groups that lack a mapping.

- [ ] **Step 4: Add tests**

Edge-case test matrix:
- Each actionable group slug (`packages-group`, `system-config`, `services-scheduling`, `identity`, `software`) returns 200
- Reference-only slugs (`network-group`, `storage-group`, `secrets`) return 404
- Unknown slugs (e.g., `foobar`, `system_tuning`, `version_changes`) return 404
- Slug uniqueness: assert all slugs from `SectionGroup::all_in_order()` are distinct
- Zero-actionable triage group (all items locked): returns 200 with count of 0 toggled items (not an error — the toggle succeeded, it just had no effect)

- [ ] **Step 5: Run tests, commit**

Run: `cargo test -p inspectah-web -- batch_toggle`

```
feat(web): migrate batch-toggle handler to SectionGroup slug routing

Routes now use SectionGroup::slug() values. Reference-only groups
(network, storage, secrets) return 404 — no batch operations on
non-actionable groups.
```

---

### Task 8: Web adapter — shadow wiring, orphan service DTOs, ifcfg note

**Files:**
- Modify: `crates/web/src/adapter.rs`
- Modify: `crates/web/src/web_types.rs`
- Modify: `crates/refine/src/projection/types.rs`

**Interfaces:**
- Consumes: `ServiceStateChange` shadow fields from Task 2, orphan synthesis from Task 4, `IFCFG_DEPRECATION_NOTE` from `crates/core/src/types/network.rs`
- Produces: `RefServiceItem.shadow_type`, `RefServiceItem.shadow_rationale`; `has_ifcfg` + `ifcfg_note` on network DTO; shadow fields propagated through to `ServiceDecisionDto`

**blocked_by:** Task 2, Task 4

- [ ] **Step 1: Add shadow fields to RefServiceItem**

```rust
// In crates/refine/src/projection/types.rs:
pub struct RefServiceItem {
    pub unit: String,
    pub current_state: ServiceUnitState,
    pub default_state: Option<PresetDefault>,
    pub owning_package: Option<String>,
    pub dropin_contents: Vec<String>,
    pub shadow_type: Option<ShadowType>,
    pub shadow_rationale: Option<String>,
}
```

- [ ] **Step 2: Wire shadow fields through projection**

In the projection code that builds `RefServiceItem`, copy shadow fields from the `ServiceStateChange`:

```rust
RefServiceItem {
    // ... existing fields
    shadow_type: change.shadow_type.clone(),
    shadow_rationale: change.shadow_rationale.clone(),
}
```

- [ ] **Step 3: Wire shadow fields through adapter to ServiceDecisionDto**

The `ServiceDecisionDto` already has `shadow_type` and `shadow_rationale` fields. Wire them from the `RefServiceItem`:

```rust
ServiceDecisionDto {
    // ... existing fields
    shadow_type: item.shadow_type.as_ref().map(|s| format!("{:?}", s).to_lowercase()),
    shadow_rationale: item.shadow_rationale.clone(),
}
```

- [ ] **Step 4: Add ifcfg note to network section DTO**

Add `has_ifcfg: bool` and `ifcfg_note: Option<String>` to the network section response in `web_types.rs`. The ifcfg signal should be threaded from the snapshot model, not detected via path-string heuristic. Check if `RefNetwork` or `NetworkSection` already carries a `has_ifcfg` flag from the collector. If it does, use it directly. If not, add `has_ifcfg: bool` to `NetworkSection` in the collector (set during network inspection when any connection path contains `network-scripts`), and thread it through `RefNetwork` → adapter → DTO. The note text comes from `IFCFG_DEPRECATION_NOTE` constant — never hardcode it in the adapter.

- [ ] **Step 5: Add API group metadata endpoint**

Add a `GET /api/groups` endpoint (or a `groups` field on the existing `/api/health` response) that returns the ordered group list. The response is built entirely from `SectionGroup`:

```rust
#[derive(Serialize)]
pub struct GroupMetaDto {
    pub slug: String,
    pub label: String,
    pub sections: Vec<SectionMetaDto>,
    pub has_actionable_sections: bool,
}

#[derive(Serialize)]
pub struct SectionMetaDto {
    pub id: String,
    pub label: String,
    pub is_triage: bool,
}
```

The section list and triage classification are derived from `SectionGroup` — the frontend sidebar consumes this response and does not maintain its own section lists. The endpoint iterates `SectionGroup::all_in_order()`, and for each group, lists its member sections with labels and triage/reference classification. This is the single path from `SectionGroup` to the frontend.

- [ ] **Step 6: Run tests, commit**

Run: `cargo test -p inspectah-web && cargo test -p inspectah-refine`

```
feat(web): wire shadow fields, ifcfg note, and group metadata through adapter

Shadow data flows from ServiceStateChange through RefServiceItem
to ServiceDecisionDto. Network section includes has_ifcfg flag.
API response includes group metadata for sidebar rendering.
```

---

### Task 9: Sidebar overhaul — NavExpandable structure

**Files:**
- Modify: `crates/web/ui/src/components/Sidebar.tsx`
- Modify: `crates/web/ui/src/api/types.ts`
- Modify: `crates/web/ui/src/components/MainContent.tsx`

**Interfaces:**
- Consumes: group metadata from Task 8 API response
- Produces: 8-group collapsible sidebar, retired `system_tuning` and `version_changes` sections

**blocked_by:** Task 8

- [ ] **Step 1: Define TypeScript types for group metadata**

```typescript
interface SectionGroupMeta {
  slug: string;
  label: string;
  sections: { id: string; label: string; isTriage: boolean }[];
  hasActionableSections: boolean;
}
```

- [ ] **Step 2: Replace BASE_REVIEW_SECTIONS / REFERENCE_SECTIONS with group-driven rendering**

Remove the hardcoded section arrays. Fetch group metadata from the API. Render each group as a `NavExpandable` (multi-section) or plain `NavItem` (singleton). Singleton triage groups (Packages, Users & Identity) render as a `NavItem` with a right-aligned kebab menu for batch actions (same menu as multi-section group headings, just on the NavItem row itself). Retire `system_tuning` — its contents become separate sections under System Configuration. Retire `version_changes` — it becomes a collapsible panel inside the Packages content view.

**Retired ID handling:** If saved session state references `system_tuning` or `version_changes` as the active section, ignore the stale value and default to the first section (packages). If expansion state references a retired group slug, ignore it. No migration step — stale keys are silently dropped on load.

- [ ] **Step 3: Implement collapsed/expanded state persistence**

Store expansion state as `Record<string, boolean>` keyed by group slug. Persist as a field on the web-layer session payload (sent to backend on autosave alongside refine decisions). This is web-only state — it does not live in the refine session's core JSON, only in the web session wrapper. Default: all groups expanded on first load.

- [ ] **Step 4: Update MainContent.tsx**

Remove flat `if (activeSection === ...)` branches for retired sections. Content area behavior unchanged — one section per view. Add a "Version Changes" collapsible panel inside the packages view.

- [ ] **Step 5: Add Playwright test for group structure**

Test: 8 groups rendered, expand/collapse works, singleton groups have no chevron.

- [ ] **Step 6: Run tests, commit**

Run: `cd crates/web/ui && npm test && npx playwright test`

```
feat(web-ui): replace Review/Reference sidebar with 8-group NavExpandable

Groups derived from API metadata, not hardcoded constants.
system_tuning and version_changes retired. Collapsed state
persisted to session.
```

---

### Task 10: Sidebar badges and cleared-state signaling

**Files:**
- Modify: `crates/web/ui/src/components/Sidebar.tsx`

**Interfaces:**
- Consumes: sidebar structure from Task 9, view data item counts
- Produces: blue badges on triage sections (with count), grey badges on reference sections (no count), textual "0" cleared state with aria-live announcement

**blocked_by:** Task 9

- [ ] **Step 1: Implement badge differentiation**

Triage sections: `<Badge>{count}</Badge>` (PatternFly default = blue). Reference sections: `<Badge isRead>` with no count text.

- [ ] **Step 2: Implement cleared-state signaling**

When all actionable items in a triage section are excluded, badge text changes to `"0"`. Add `aria-live="polite"` region that announces `"{section}: 0 decisions remaining"` when state changes.

- [ ] **Step 3: Add tests**

Test: triage sections show blue badges with counts, reference sections show grey badges, excluded-all state shows "0" badge.

- [ ] **Step 4: Run tests, commit**

```
feat(web-ui): add blue/grey badge differentiation and cleared-state signaling
```

---

### Task 11: Sidebar keyboard navigation

**Files:**
- Modify: `crates/web/ui/src/hooks/useKeyboard.ts`
- Modify: `crates/web/ui/src/components/Sidebar.tsx`

**Interfaces:**
- Consumes: sidebar structure from Task 9, group metadata
- Produces: number-key 1-8 group jumps per spec interaction table, arrow key navigation, focus restoration on collapse, `aria-current="page"` contract

**blocked_by:** Task 9

- [ ] **Step 1: Implement number-key shortcuts**

Number keys 1-8 map to groups in display order. Behavior per group type (see spec §1 interaction table):
- Singleton triage/reference: focus the entry, load section, set `aria-current="page"`
- Multi-section triage: expand if collapsed, focus first triage child, load it
- Multi-section reference: expand if collapsed, focus first child, load it

- [ ] **Step 2: Implement `aria-current` contract**

`aria-current="page"` stays on the active section's NavItem even when its parent is collapsed. Group headings never get `aria-current`.

- [ ] **Step 3: Implement focus restoration on collapse**

When a group is collapsed while its child is active: keyboard focus moves to the group heading, `aria-current` stays on the hidden child, content area keeps showing the active section.

- [ ] **Step 4: Implement heading row Tab order**

Group label → kebab menu button (if present) → no more tab stops on heading row. Arrow Down moves to first child if expanded.

- [ ] **Step 5: Add Playwright tests**

Test: number-key jumps, focus restoration, aria-current stays on hidden child.

- [ ] **Step 6: Run tests, commit**

```
feat(web-ui): add group-aware keyboard navigation and aria-current contract
```

---

### Task 12: Batch-toggle action menu in sidebar

**Files:**
- Modify: `crates/web/ui/src/components/Sidebar.tsx`
- Modify: `crates/web/ui/src/api/client.ts`

**Interfaces:**
- Consumes: sidebar structure from Task 9, `batchToggleGroup` backend route from Task 7
- Produces: kebab action menu on triage group headings, `batchToggleGroup()` client method, Packages confirmation dialog, Ctrl+Shift+A/X shortcuts, aria-live announcements

**blocked_by:** Task 7, Task 9

- [ ] **Step 1: Add TypeScript client method**

```typescript
export async function batchToggleGroup(
  groupSlug: string,
  include: boolean,
): Promise<ViewResponse> {
  return postJson(`/api/batch-toggle/${groupSlug}`, { include });
}
```

- [ ] **Step 2: Add kebab menu to triage group headings**

Render a PatternFly `Dropdown` with kebab toggle on group rows where `hasActionableSections` is true. For multi-section groups, the menu lives on the `NavExpandable` heading row. For singleton triage groups (Packages, Users & Identity), the menu lives on the `NavItem` row itself (right-aligned, same kebab pattern). Two items: "Include all", "Exclude all". No menu on reference-only groups (singleton or multi-section).

- [ ] **Step 3: Add Packages confirmation dialog**

When "Exclude all" is selected on the Packages group, show a `Modal` confirmation: "This will exclude all packages, producing an image missing critical runtime dependencies. Continue?"

- [ ] **Step 4: Add keyboard shortcuts**

Ctrl+Shift+A / Ctrl+Shift+X when a group heading (multi-section) or singleton triage NavItem (Packages, Users & Identity) has focus. No-op on reference-only groups regardless of whether they are singleton or multi-section.

- [ ] **Step 5: Add aria-live announcements**

On toggle completion: "12 items included in Services & Scheduling". Partial success: "8 of 12 items included — 4 locked".

- [ ] **Step 6: Add tests**

Playwright test: menu appears on triage groups only, Packages confirmation dialog, toggle calls API.

- [ ] **Step 7: Run tests, commit**

```
feat(web-ui): add group-level batch-toggle action menu with Packages safety gate
```

---

### Task 13: ServiceSection full-shadow rendering

**Files:**
- Modify: `crates/web/ui/src/components/ServiceSection.tsx`
- Modify: `crates/web/ui/src/components/ServiceSection.css` (or inline styles)

**Interfaces:**
- Consumes: `ServiceDecisionDto.shadow_type`, `ServiceDecisionDto.shadow_rationale` from Task 8
- Produces: warning amber border-left on full-shadow rows, "Shadow override" badge, aria-describedby on toggle control, shadow count in section header

**blocked_by:** Task 8

- [ ] **Step 1: Add warning amber border-left for full-shadow rows**

When `svc.shadow_type === "full_shadow"`:
- Default: `border-left: 4px solid var(--pf-v5-global--warning-color--100)`
- Conflict with triage border: triage border wins left edge, shadow row gets `background-color: var(--pf-v5-global--warning-color--100)` at 10% opacity

- [ ] **Step 2: Add "Shadow override" badge**

`<Label color="gold" isCompact>Shadow override</Label>` after the triage badge, before locked badge. Badge order: triage → shadow → locked.

- [ ] **Step 3: Wire aria-describedby to toggle control**

Helper text element gets `id={`shadow-rationale-${svc.unit}`}`. The checkbox/toggle gets `aria-describedby` referencing this ID. When both shadow and locked apply, reference both IDs space-separated.

- [ ] **Step 4: Add shadow count to section header**

When shadow services exist: "12 services (3 shadow overrides)" in the section header.

- [ ] **Step 5: Add component tests**

Test: amber border renders for full-shadow, badge appears, aria-describedby on toggle, combined shadow+locked description, shadow count in header, non-shadow rows unaffected.

- [ ] **Step 6: Run tests, commit**

```
feat(web-ui): add full-shadow service rendering with warning border and accessibility
```

---

### Task 14: ifcfg deprecation banner in network view

**Files:**
- Modify: `crates/web/ui/src/components/MainContent.tsx` (or a new NetworkSection component)

**Interfaces:**
- Consumes: `has_ifcfg` and `ifcfg_note` from Task 8 network section DTO
- Produces: PatternFly Alert banner at top of Network section

**blocked_by:** Task 8, Task 9

- [ ] **Step 1: Render ifcfg Alert banner**

When the network section DTO has `has_ifcfg: true`, render a PatternFly `Alert` (variant: info, inline) at the top of the Network section content. Text comes from the `ifcfg_note` field (sourced from `IFCFG_DEPRECATION_NOTE` constant).

```tsx
{hasIfcfg && (
  <Alert variant="info" isInline title="ifcfg Deprecation">
    {ifcfgNote}
  </Alert>
)}
```

- [ ] **Step 2: Add component test**

Test: banner appears when `has_ifcfg` is true, hidden when false, text matches constant.

- [ ] **Step 3: Run tests, commit**

```
feat(web-ui): add ifcfg deprecation banner in network section
```

---

### Task 15: Display surfaces for /var ownership and mode

**Files:**
- Modify: `crates/pipeline/templates/report/storage.html`
- Modify: `crates/tui/src/screen/single_host.rs` (storage section)
- Modify: `crates/web/src/adapter.rs` (storage section DTO)

**Interfaces:**
- Consumes: `VarDirectory` ownership/mode fields from Task 2
- Produces: ownership and mode info shown in HTML report, TUI, and refine web storage view

**blocked_by:** Task 2

- [ ] **Step 1: Update HTML report template**

Show ownership and mode alongside path for unbacked /var directories: `/var/lib/pgsql/data (0750 postgres:postgres)`.

- [ ] **Step 2: Update TUI storage section**

Add ownership/mode to the advisory item detail text.

- [ ] **Step 3: Update web adapter storage section**

Include ownership/mode in the `ContextItem` detail for unbacked /var paths in `web_storage_section()`.

- [ ] **Step 4: Add tests**

Test each surface renders ownership/mode when present, falls back gracefully when absent.

- [ ] **Step 5: Run tests, commit**

```
feat: display /var ownership and mode in HTML report, TUI, and refine web
```

---

## Task Dependency Graph

```
T1 (SectionGroup) ──────────────┬──→ T7 (batch-toggle handler)
                                │
T2 (core types) ───┬──→ T3 (/var collection) ──→ T6 (tmpfiles.d renderer)
                   ├──→ T4 (shadow synthesis) ──→ T8 (web adapter) ──┬──→ T9 (sidebar structure) ──┬──→ T10 (badges)
                   │                                                  │                             ├──→ T11 (keyboard)
                   │                                                  │                             ├──→ T12 (batch-toggle UI) ←── T7
                   │                                                  ├──→ T13 (shadow rendering)   └──→ T14 (ifcfg banner)
                   └──→ T15 (/var display surfaces)                   │
                                                                      └──→ T14 (ifcfg banner)
T5 (TUI inventory) — independent
```

**Parallelism:** T1, T2, T5 can start simultaneously. T3+T4 can parallel after T2. T7 can parallel with T3/T4 (only needs T1). T15 can run anytime after T2. Frontend track (T9-T14) is sequential but starts once T8 lands.
