# Unified Include-Default Model Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Unify inspectah's 25 toggleable item types into a clean three-source model: collectors set `include: true`, classifiers (RPM, config, tuned) override analytically, fleet aggregate narrows by strict universality, and semantic exclusions lock image-incompatible items.

**Architecture:** Changes flow bottom-up through the pipeline: core types first (locked field, Option→bool), then collectors (set true), then semantic exclusions (normalize layer), then fleet aggregate (merge.rs), then cleanup (delete session normalization + render overrides), then frontend (locked badges). Each phase is independently testable.

**Tech Stack:** Rust (inspectah-core, inspectah-collect, inspectah-refine, inspectah-pipeline, inspectah-web, inspectah-tui). Cargo workspace, `cargo clippy -- -D warnings`, `cargo test`.

**Spec:** `process-docs/specs/proposed/2026-06-02-unified-include-default-model.md`

**Owners:** Tang (Tasks 1-17, Rust), Kit (Tasks 18-19, frontend). Thorn checkpoints at Tasks 5, 9, 14, 20.

---

### Task 1: Add `default_true` serde helper to inspectah-core

**Files:**
- Modify: `inspectah-core/src/lib.rs`

- [ ] **Step 1: Add the helper function**

Add alongside the existing `is_false` helper:

```rust
pub fn default_true() -> bool {
    true
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p inspectah-core`
Expected: clean build

- [ ] **Step 3: Commit**

```bash
git add inspectah-core/src/lib.rs
git commit -m "feat(core): add default_true serde helper for include-default model"
```

---

### Task 2: Add `locked` field to core types

**Files:**
- Modify: `inspectah-core/src/types/services.rs`
- Modify: `inspectah-core/src/types/config.rs`
- Modify: `inspectah-core/src/types/storage.rs`
- Modify: `inspectah-core/src/types/containers.rs`
- Modify: `inspectah-core/src/types/scheduled.rs`
- Modify: `inspectah-core/src/types/network.rs`
- Modify: `inspectah-core/src/types/kernelboot.rs`
- Modify: `inspectah-core/src/types/selinux.rs`
- Modify: `inspectah-core/src/types/nonrpm.rs`

- [ ] **Step 1: Add `locked: bool` field to all item types that have `include`**

For each struct that has `pub include: bool` (or `Option<bool>`), add:

```rust
#[serde(default, skip_serializing_if = "crate::is_false")]
pub locked: bool,
```

Place it immediately after the `include` field. This applies to:
- `ServiceStateChange` (services.rs:69)
- `SystemdDropIn` (services.rs:104)
- `ConfigFileEntry` (config.rs:46)
- `FstabEntry` (storage.rs:15)
- `QuadletUnit` (containers.rs:29)
- `FlatpakApp` (containers.rs:56)
- `RunningContainer` (containers.rs:87)
- `ComposeFile` (containers.rs:103)
- `CronJob` (scheduled.rs:5)
- `SystemdTimer` (scheduled.rs:18)
- `AtJob` (scheduled.rs:42)
- `GeneratedTimerUnit` (scheduled.rs:58)
- `NMConnection` (network.rs:15)
- `FirewallZone` (network.rs:37)
- `FirewallDirectRule` (network.rs:54)
- `SysctlOverride` (kernelboot.rs:24)
- `KernelModule` (kernelboot.rs:37)
- `SelinuxPortLabel` (selinux.rs:13)
- `NonRpmItem` (nonrpm.rs:14)
- `PackageEntry` (rpm.rs:38)
- `RepoFile` (rpm.rs:72)
- `GpgKey` (rpm.rs:93)
- `EnabledModuleStream` (rpm.rs:145)
- `VersionLockEntry` — check rpm.rs for this struct

Do NOT add `locked` to `KernelBootSection.tuned_include` — tuned is a scalar, not a collection item. Its locked behavior is handled differently (via the stock-profile classifier).

**Narrow scope for `locked`:** Only types that are targets of semantic
exclusions strictly need `locked`. In practice, add it to all include-bearing
types for uniformity (the field is `skip_serializing_if = "is_false"` so it
costs nothing when unused), but the semantic exclusion code will only SET it
on: `ServiceStateChange`, `SystemdDropIn`, `ConfigFileEntry`, `FstabEntry`.

Also add `attention_reason: Option<String>` (with `skip_serializing_if =
"Option::is_none"`) to types that don't have it yet but will carry locked
reasons:
- `ConfigFileEntry` (config.rs) — for merge-hostile reason
- `FstabEntry` (storage.rs) — for "host state — not image-portable"
- `SystemdDropIn` (services.rs) — for "parent service image-incompatible"

`ServiceStateChange` already has `attention_reason`. Do not add it to types
that will never be semantically excluded (packages, repos, etc.).

- [ ] **Step 2: Fix any compilation errors**

The `locked` field defaults to `false` via serde, so no constructor changes are needed. But any struct literal in tests or production code that uses `..Default::default()` will be fine. Struct literals without `..Default::default()` will need `locked: false` added.

Run: `cargo build --workspace`

Fix all compilation errors by adding `locked: false` to struct literals.

- [ ] **Step 3: Run full test suite**

Run: `cargo test --workspace`
Expected: all existing tests pass (locked defaults to false everywhere)

- [ ] **Step 4: Commit**

```bash
git add inspectah-core/
git commit -m "feat(core): add locked field to all toggleable item types"
```

---

### Task 3: Normalize `Option<bool>` include fields to `bool`

**Files:**
- Modify: `inspectah-core/src/types/storage.rs` (FstabEntry)
- Modify: `inspectah-core/src/types/network.rs` (NMConnection)
- Modify: `inspectah-core/src/types/scheduled.rs` (SystemdTimer, AtJob)
- Modify: `inspectah-core/src/types/containers.rs` (RunningContainer)
- Modify: `inspectah-core/src/fleet/merge.rs` (set_include impls)
- Potentially many other files that reference `.include` on these types

- [ ] **Step 1: Audit current `Option<bool>` include fields**

Run: `grep -rn 'include.*Option<bool>' inspectah-core/src/types/ --include='*.rs'`

Verify the list matches:
- `FstabEntry` (storage.rs)
- `NMConnection` (network.rs)
- `SystemdTimer` (scheduled.rs)
- `AtJob` (scheduled.rs)
- `RunningContainer` (containers.rs)

If there are others, include them.

- [ ] **Step 2: Change each `Option<bool>` to `bool` with `default_true`**

For each affected struct, change:

```rust
// Before
#[serde(default, skip_serializing_if = "Option::is_none")]
pub include: Option<bool>,

// After
#[serde(default = "crate::default_true")]
pub include: bool,
```

- [ ] **Step 3: Fix all compilation errors across the workspace**

Every reference to `.include` on these types changes from `Option<bool>` to `bool`. Common patterns to fix:

- `entry.include.unwrap_or(false)` → `entry.include`
- `entry.include.unwrap_or(true)` → `entry.include`
- `entry.include == Some(true)` → `entry.include`
- `entry.include == Some(false)` → `!entry.include`
- `entry.include = Some(val)` → `entry.include = val`
- `entry.include.is_none()` → remove the check (no longer optional)

In `inspectah-core/src/fleet/merge.rs`, the `set_include` impls for these types need `self.include = val` instead of `self.include = Some(val)`.

Run: `cargo build --workspace`

- [ ] **Step 4: Run full test suite, update snapshots**

Run: `cargo test --workspace`

Some insta snapshots may need updating if they serialize `"include": null` which will now be `"include": true`.

Run: `cargo insta review` if needed.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor(core): normalize Option<bool> include fields to bool with default_true"
```

---

### Task 4: Set `include: true` in all collectors

**Files:**
- Modify: multiple files in `inspectah-collect/src/inspectors/`

- [ ] **Step 1: Audit collectors that rely on serde default of `false`**

Two searches are needed — one for explicit `include:` settings, one for
struct literals that use `..Default::default()` and therefore inherit the
serde default of `false`:

```bash
# Find explicit include settings
grep -rn 'include:' inspectah-collect/src/inspectors/ --include='*.rs' | grep -v test | grep -v '//'

# Find struct literals using ..Default::default() for types that have include
grep -rn '\.\.Default::default()' inspectah-collect/src/inspectors/ --include='*.rs' | grep -v test
```

For each `..Default::default()` hit, check whether the struct type has an
`include` field. If it does and the literal doesn't set `include: true`
explicitly, that's a site that needs fixing. The grep-for-`include:` search
alone will miss these — they are the most common source of silent `false`
defaults.

Also identify any explicit `include: false` that is NOT in the RPM
classifier or config classifier.

The following inspectors should already set `include: true`:
- `kernelboot.rs` — sysctl_overrides, kernel_modules (lines 235, 288)
- `services.rs` — state_changes, drop_ins (lines 340, 370, 695)
- `selinux.rs` — port_labels (line 370)
- `users.rs` — users, groups (JSON `"include": true`)

The following may need changes (verify each):
- `nonrpm.rs` — NonRpmItem (currently uses serde default)
- `containers.rs` or wherever quadlets/flatpaks/compose are collected
- `network.rs` — NMConnection, FirewallZone, FirewallDirectRule
- `scheduled.rs` — CronJob, SystemdTimer, AtJob, GeneratedTimerUnit
- `storage.rs` — FstabEntry (set `include: true` even though refine ignores it)

Do NOT change:
- `config/mod.rs:149` — keeps `include: false` (classifier runs after)
- `rpm/classifier.rs` — keeps its classifier logic
- `kernelboot.rs:140` — keeps `is_stock_tuned_profile()` for tuned_include

- [ ] **Step 2: Set `include: true` in each identified collector**

For each inspector that doesn't explicitly set `include: true`, add it to the struct literal. Example:

```rust
// Before (relying on serde default = false)
NonRpmItem {
    path: path.to_string(),
    name: name.to_string(),
    ..Default::default()
}

// After
NonRpmItem {
    path: path.to_string(),
    name: name.to_string(),
    include: true,
    ..Default::default()
}
```

- [ ] **Step 3: Write tests verifying collector defaults**

For each changed inspector, add or update a test asserting that collected items have `include: true`:

```rust
#[test]
fn collected_items_default_to_include_true() {
    // ... set up inspector with mock executor ...
    let result = inspector.inspect(&executor);
    for item in &result.items {
        assert!(item.include, "item {} should default to include: true", item.name);
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p inspectah-collect`
Expected: all pass

- [ ] **Step 5: Commit**

```bash
git add inspectah-collect/
git commit -m "feat(collect): set include: true in all collectors per unified default model"
```

---

### Task 5: Thorn checkpoint 1

**Scope:** Tasks 1-4 (type foundation + collector defaults)

- [ ] **Step 1: Run full workspace checks**

```bash
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

- [ ] **Step 2: Review for regressions**

Verify:
- All existing tests pass
- `locked` defaults to `false` everywhere
- `Option<bool>` → `bool` migration didn't change any existing behavior
- Collectors that should NOT change (RPM classifier, config classifier, tuned classifier) are untouched

---

### Task 6: Add `normalize_merge_hostile_configs()`

**Files:**
- Modify: `inspectah-refine/src/normalize.rs`

- [ ] **Step 1: Write tests for merge-hostile normalization**

```rust
#[test]
fn merge_hostile_fstab_locked() {
    let mut snap = test_snapshot();
    snap.storage = Some(StorageSection {
        fstab_entries: vec![FstabEntry {
            mount_point: "/data".into(),
            device: "/dev/sda1".into(),
            include: true,
            locked: false,
            ..Default::default()
        }],
        ..Default::default()
    });
    normalize_merge_hostile_configs(&mut snap);
    let entry = &snap.storage.as_ref().unwrap().fstab_entries[0];
    assert!(!entry.include, "fstab should be excluded");
    assert!(entry.locked, "fstab should be locked");
}

#[test]
fn merge_hostile_crypttab_locked() {
    let mut snap = test_snapshot();
    // Add a config file entry for /etc/crypttab
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/crypttab".into(),
            include: true,
            locked: false,
            ..Default::default()
        }],
        ..Default::default()
    });
    normalize_merge_hostile_configs(&mut snap);
    let entry = &snap.config.as_ref().unwrap().files[0];
    assert!(!entry.include, "crypttab should be excluded");
    assert!(entry.locked, "crypttab should be locked");
}

#[test]
fn non_hostile_config_untouched() {
    let mut snap = test_snapshot();
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/httpd/conf/httpd.conf".into(),
            include: true,
            locked: false,
            ..Default::default()
        }],
        ..Default::default()
    });
    normalize_merge_hostile_configs(&mut snap);
    let entry = &snap.config.as_ref().unwrap().files[0];
    assert!(entry.include, "non-hostile config should stay included");
    assert!(!entry.locked, "non-hostile config should not be locked");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-refine normalize_merge_hostile -v`
Expected: FAIL — function doesn't exist yet

- [ ] **Step 3: Implement `normalize_merge_hostile_configs()`**

Add to `inspectah-refine/src/normalize.rs`, following the `normalize_incompatible_services()` pattern:

```rust
const MERGE_HOSTILE_PATHS: &[&str] = &[
    "/etc/fstab",
    "/etc/crypttab",
];

pub fn normalize_merge_hostile_configs(snapshot: &mut InspectionSnapshot) {
    // Lock all fstab entries — host state, not image-portable
    if let Some(ref mut storage) = snapshot.storage {
        for entry in &mut storage.fstab_entries {
            entry.include = false;
            entry.locked = true;
            entry.attention_reason =
                Some("host state — not image-portable".into());
        }
    }

    // Lock /etc/crypttab in config files
    if let Some(ref mut config) = snapshot.config {
        for file in &mut config.files {
            if MERGE_HOSTILE_PATHS.contains(&file.path.as_str()) {
                file.include = false;
                file.locked = true;
                file.attention_reason =
                    Some("merge-hostile — fights bootc /etc 3-way merge".into());
            }
        }
    }
}
```

- [ ] **Step 4: Wire into ALL session construction paths**

Semantic exclusions must fire wherever sessions are constructed — not just
`load_for_refine()`. Audit the codebase for every path that creates a
`RefineSession`:

```bash
grep -rn 'RefineSession::new\|RefineSession::resume\|load_for_refine' inspectah-refine/src/ inspectah-web/src/ --include='*.rs'
```

Add `normalize_merge_hostile_configs(&mut snapshot)` at each construction
site, alongside the existing `normalize_incompatible_services()` call. If
some paths construct sessions from in-memory snapshots or fleet paths rather
than tarballs, they need the same treatment. The invariant is: **no
`RefineSession` ever exists without semantic exclusions applied.**

If all paths already converge through a single constructor, wiring it there
is sufficient. But verify — don't assume.

- [ ] **Step 5: Run tests**

Run: `cargo test -p inspectah-refine`
Expected: all pass including new tests

- [ ] **Step 6: Commit**

```bash
git add inspectah-refine/
git commit -m "feat(refine): add merge-hostile config normalization with locked enforcement"
```

---

### Task 7: Update `normalize_incompatible_services()` for locked + drop-ins

**Files:**
- Modify: `inspectah-refine/src/normalize.rs`

- [ ] **Step 1: Write tests for locked enforcement and drop-in coverage**

```rust
#[test]
fn incompatible_service_is_locked() {
    let mut snap = test_snapshot();
    // Set up a snapshot with dnf-makecache.service
    snap.services = Some(ServicesSection {
        state_changes: vec![ServiceStateChange {
            unit: "dnf-makecache.service".into(),
            include: true,
            locked: false,
            ..Default::default()
        }],
        ..Default::default()
    });
    normalize_incompatible_services(&mut snap);
    let sc = &snap.services.as_ref().unwrap().state_changes[0];
    assert!(!sc.include);
    assert!(sc.locked, "incompatible service should be locked");
}

#[test]
fn incompatible_service_dropin_is_locked() {
    let mut snap = test_snapshot();
    snap.services = Some(ServicesSection {
        state_changes: vec![ServiceStateChange {
            unit: "dnf-makecache.service".into(),
            include: true,
            locked: false,
            ..Default::default()
        }],
        drop_ins: vec![SystemdDropIn {
            unit: "dnf-makecache.service".into(),
            name: "override.conf".into(),
            include: true,
            locked: false,
            ..Default::default()
        }],
        ..Default::default()
    });
    normalize_incompatible_services(&mut snap);
    let di = &snap.services.as_ref().unwrap().drop_ins[0];
    assert!(!di.include, "drop-in of incompatible service should be excluded");
    assert!(di.locked, "drop-in of incompatible service should be locked");
}

#[test]
fn unrelated_dropin_not_locked() {
    let mut snap = test_snapshot();
    snap.services = Some(ServicesSection {
        drop_ins: vec![SystemdDropIn {
            unit: "httpd.service".into(),
            name: "custom.conf".into(),
            include: true,
            locked: false,
            ..Default::default()
        }],
        ..Default::default()
    });
    normalize_incompatible_services(&mut snap);
    let di = &snap.services.as_ref().unwrap().drop_ins[0];
    assert!(di.include, "unrelated drop-in should stay included");
    assert!(!di.locked, "unrelated drop-in should not be locked");
}
```

- [ ] **Step 2: Run tests to verify new ones fail**

Run: `cargo test -p inspectah-refine incompatible -v`
Expected: new tests fail (locked not set, drop-ins not covered)

- [ ] **Step 3: Update `normalize_incompatible_services()`**

Add `locked = true` to the existing service exclusion loop. Add a new loop for drop-ins:

```rust
pub fn normalize_incompatible_services(snapshot: &mut InspectionSnapshot) {
    let services = match snapshot.services.as_mut() {
        Some(s) => s,
        None => return,
    };

    let incompatible_units: Vec<&str> = INCOMPATIBLE_SERVICES.iter().map(|e| e.unit).collect();

    // Lock incompatible services
    for sc in &mut services.state_changes {
        if incompatible_units.contains(&sc.unit.as_str()) {
            sc.include = false;
            sc.locked = true;
            sc.attention_reason = Some("service-image-mode-incompatible".into());
        }
    }

    // Lock drop-ins owned by incompatible services
    for di in &mut services.drop_ins {
        if incompatible_units.contains(&di.unit.as_str()) {
            di.include = false;
            di.locked = true;
            di.attention_reason =
                Some("parent service image-mode incompatible".into());
        }
    }

    // Remove from enabled_units (existing behavior)
    services.enabled_units.retain(|u| !incompatible_units.contains(&u.as_str()));
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p inspectah-refine`
Expected: all pass

- [ ] **Step 5: Commit**

```bash
git add inspectah-refine/
git commit -m "feat(refine): add locked enforcement and drop-in coverage to incompatible services"
```

---

### Task 8: Enforce `locked` in session toggle and export clamp

**Files:**
- Modify: `inspectah-refine/src/session.rs`

- [ ] **Step 1: Write tests for locked toggle rejection**

```rust
#[test]
fn set_include_on_locked_item_is_rejected() {
    // Build a session with a locked service
    let mut session = test_session();
    // ... set up a service with locked: true, include: false ...
    // apply() should succeed as a silent no-op — not an error
    session.apply(RefinementOp::SetInclude {
        item_id: ItemId::Service { unit: "dnf-makecache.service".into() },
        include: true,
    });
    // Verify the item is still include: false
    let projected = session.snapshot_projected();
    let sc = projected.services.as_ref().unwrap()
        .state_changes.iter()
        .find(|s| s.unit == "dnf-makecache.service")
        .unwrap();
    assert!(!sc.include, "locked item should not be re-included");
}
```

- [ ] **Step 2: Write test for recompute_view locked enforcement**

```rust
#[test]
fn recompute_view_skips_locked_set_include_ops() {
    // Build a session, add a SetInclude(true) op for a locked item,
    // then call recompute_view and verify the item stays excluded
    // ...
}
```

- [ ] **Step 3: Short-circuit locked ops in `apply()` BEFORE recording**

The silent no-op must happen before the op is appended to session history.
If the skip only happens during replay/clamp, the op gets recorded and
replayed on every future `recompute_view()` — wasteful and misleading.

Find the `apply()` method (or equivalent op-recording entry point) in
`session.rs`. Add a pre-record guard:

```rust
pub fn apply(&mut self, op: RefinementOp) {
    // Short-circuit: locked items cannot be re-included
    if let RefinementOp::SetInclude { ref item_id, include: true } = op {
        if is_item_locked(&self.snapshot, item_id) {
            return; // silent no-op — op never recorded
        }
    }
    // ... existing logic: record op, advance cursor, etc. ...
}
```

Implement `is_item_locked()` as a helper that checks the `locked` field on
the relevant item type based on `ItemId`.

- [ ] **Step 3a: Also guard in `recompute_view()` replay as defense-in-depth**

In case ops from pre-locked sessions exist in autosaved history, add the
same guard in the `recompute_view()` op replay loop (around line 1339):

```rust
RefinementOp::SetInclude { item_id, include } => {
    if *include && is_item_locked(&snapshot, item_id) {
        continue; // skip stale op from pre-locked autosave
    }
    // ... existing match on item_id ...
}
```

- [ ] **Step 4: Add export/render clamp in `snapshot_projected()`**

Find `snapshot_projected()` or the function that produces the projected snapshot for renderers. After computing the projected snapshot, add a clamping pass:

```rust
fn clamp_locked_items(snapshot: &mut InspectionSnapshot) {
    if let Some(ref mut services) = snapshot.services {
        for sc in &mut services.state_changes {
            if sc.locked { sc.include = false; }
        }
        for di in &mut services.drop_ins {
            if di.locked { di.include = false; }
        }
    }
    if let Some(ref mut config) = snapshot.config {
        for f in &mut config.files {
            if f.locked { f.include = false; }
        }
    }
    if let Some(ref mut storage) = snapshot.storage {
        for e in &mut storage.fstab_entries {
            if e.locked { e.include = false; }
        }
    }
    // ... repeat for all types with locked field ...
}
```

Call this after the projection is built, before returning.

- [ ] **Step 5: Run tests**

Run: `cargo test -p inspectah-refine`
Expected: all pass

- [ ] **Step 6: Commit**

```bash
git add inspectah-refine/
git commit -m "feat(refine): enforce locked field in session toggle and export clamp"
```

---

### Task 9: Thorn checkpoint 2

**Scope:** Tasks 6-8 (semantic exclusions)

- [ ] **Step 1: Run full workspace checks**

```bash
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

- [ ] **Step 2: Verify semantic exclusion guarantees**

Test manually or via targeted tests:
- A locked item cannot be toggled to `include: true` via `apply()`
- A locked item cannot be re-included via session resume (autosave replay)
- The export clamp ensures locked items are `false` in the projected snapshot
- Drop-ins of incompatible services are also locked
- Merge-hostile paths (fstab, crypttab) are locked

---

### Task 10: Wire flatpaks into fleet prevalence system

**Files:**
- Modify: `inspectah-core/src/types/containers.rs`
- Modify: `inspectah-core/src/fleet/merge.rs`

`FlatpakApp` is currently NOT wired into the fleet prevalence system. It has
no `fleet: Option<FleetPrevalence>` field, no `FleetMergeable` impl, and
its merge is a hand-rolled HashSet dedup instead of the generic
`merge_items()` engine. This task plugs it in.

- [ ] **Step 1: Add `fleet` field to `FlatpakApp`**

In `inspectah-core/src/types/containers.rs`, add to the `FlatpakApp` struct:

```rust
#[serde(skip_serializing_if = "Option::is_none")]
pub fleet: Option<FleetPrevalence>,
```

- [ ] **Step 2: Implement `FleetMergeable` for `FlatpakApp`**

In `inspectah-core/src/fleet/merge.rs`, add:

```rust
impl FleetMergeable for FlatpakApp {
    fn identity_key(&self) -> String {
        format!("{}.{}.{}", self.app_id, self.remote, self.branch)
    }

    fn fleet_mut(&mut self) -> &mut Option<FleetPrevalence> {
        &mut self.fleet
    }

    fn set_include(&mut self, val: bool) {
        self.include = val;
    }
}
```

No `content_variant_key` needed — flatpaks don't have content variants.

- [ ] **Step 3: Replace hand-rolled dedup with `merge_items()`**

Replace the HashSet-based dedup block at `merge.rs:1128-1149` with:

```rust
let flatpak_apps = merge_items(
    collect_items(&sections, |s| &s.flatpak_apps),
    total_hosts,
    hostnames,
);
```

Same pattern as the quadlet merge at line ~1114.

- [ ] **Step 4: Fix compilation errors and update tests**

Run: `cargo build -p inspectah-core`

Update tests at `merge.rs:2088-2206` to assert on `fleet` values
(count/total) in addition to dedup counts.

- [ ] **Step 5: Run tests**

Run: `cargo test -p inspectah-core`
Expected: all pass

- [ ] **Step 6: Commit**

```bash
git add inspectah-core/
git commit -m "feat(fleet): wire FlatpakApp into generic prevalence merge engine"
```

---

### Task 11: Move fleet narrowing to aggregate (`merge.rs`)

**Files:**
- Modify: `inspectah-core/src/fleet/merge.rs`
- Test: existing fleet merge tests in `inspectah-core/tests/`

- [ ] **Step 1: Write tests for fleet aggregate narrowing**

```rust
#[test]
fn fleet_merge_non_universal_item_excluded() {
    // Merge 3 snapshots where a quadlet appears on 2 of 3 hosts
    // Verify the merged snapshot has include: false for that quadlet
}

#[test]
fn fleet_merge_universal_item_included() {
    // Merge 3 snapshots where a quadlet appears on all 3 hosts
    // Verify the merged snapshot has include: true
}

#[test]
fn fleet_merge_tuned_universal_stock_excluded() {
    // All 3 hosts have virtual-guest (stock profile)
    // Even though universal, tuned_include should be false (classifier)
}

#[test]
fn fleet_merge_tuned_universal_custom_included() {
    // All 3 hosts have my-custom-profile
    // Universal + non-stock = tuned_include: true
}

#[test]
fn fleet_merge_tuned_non_universal_custom_excluded() {
    // 2 of 3 hosts have my-custom-profile
    // Non-universal = tuned_include: false (even though custom)
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-core fleet_merge_non_universal -v`
Expected: FAIL — narrowing not yet in merge

- [ ] **Step 3: Add narrowing to fleet merge**

In each `merge_*_section` function in `merge.rs`, after computing prevalence, add the universality check. The `set_include` method is already wired up via `MergeWith`. The pattern:

```rust
// After prevalence is computed for each item:
if item.fleet.as_ref().is_some_and(|f| f.count < f.total) {
    item.set_include(false);
}
```

This must be added to every section merger that handles prevalence-tracked
items, **including flatpaks** (wired into the generic engine in Task 10).

Implement the narrowing as a generic post-merge pass rather than scattering
per-section checks: after each `merge_*_section` returns, walk its items
and apply `if count < total { set_include(false) }`. This keeps the logic
in one place and prevents missing a section. Tuned is the one explicit
exception — it composes classifier + universality (see below).

For tuned specifically, the existing logic in the kernelboot merge (around line 1500-1515) already handles stock-profile exclusion. Add universality narrowing ON TOP of the classifier:

```rust
let tuned_include = if tuned_active.is_empty() {
    false
} else if is_stock_tuned_profile(&tuned_active) {
    false // stock profile excluded by classifier
} else {
    // Non-stock: include only if universal across all hosts
    is_scalar_universal(&sections, |s| &s.tuned_active, &tuned_active)
};
```

Add a helper for scalar universality checking:

```rust
fn is_scalar_universal<T, F>(sections: &[T], accessor: F, winner: &str) -> bool
where
    F: Fn(&T) -> &str,
{
    sections.iter().all(|s| accessor(s) == winner)
}
```

This checks whether `most_prevalent_scalar`'s winner was present on ALL
hosts (equivalent to `count == total` for collection items). If any host
had a different tuned profile, the winner is non-universal and excluded.

- [ ] **Step 4: Run tests**

Run: `cargo test -p inspectah-core`
Expected: new tests pass, existing fleet merge tests may need updating

- [ ] **Step 5: Commit**

```bash
git add inspectah-core/
git commit -m "feat(fleet): move include narrowing to aggregate merge pass"
```

---

### Task 12: Refactor `fleet_handlers.rs` — remove `fleet_include_default`

**Files:**
- Modify: `inspectah-web/src/fleet_handlers.rs`

- [ ] **Step 1: Audit all `fleet_include_default(fp)` call sites**

Run: `grep -n 'fleet_include_default' inspectah-web/src/fleet_handlers.rs`

There are ~9 occurrences (lines 940, 971, 986, 1011, 1035, 1050, 1081, 1106, 1137). Each one computes inclusion from prevalence instead of reading the stored value.

- [ ] **Step 2: Replace each call with stored `.include`**

For each occurrence, find the corresponding entry that holds the stored `.include` value and use it. The pattern differs by section:

For items that have a direct `entry.include`:
```rust
// Before
include: fleet_include_default(fp),

// After
include: entry.include,
```

For items where the stored include comes from a different path (e.g., fstab entries via storage), trace the data source and read from it.

- [ ] **Step 3: Remove the `fleet_include_default` function**

After all call sites are replaced, delete the function (line 1213-1215):

```rust
// DELETE
fn fleet_include_default(fp: Option<&inspectah_core::types::fleet::FleetPrevalence>) -> bool {
    fp.is_some_and(|f| f.count > 0 && f.count == f.total)
}
```

- [ ] **Step 4: Write test asserting stored-value passthrough**

```rust
#[test]
fn fleet_handlers_use_stored_include_not_recomputed() {
    // Build a fleet snapshot where an item has include: true
    // but prevalence is NOT universal (count < total).
    // Verify the fleet handler returns include: true (stored value)
    // rather than false (which fleet_include_default would have returned).
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p inspectah-web`
Expected: all pass

- [ ] **Step 6: Commit**

```bash
git add inspectah-web/
git commit -m "refactor(web): fleet handlers consume stored include values, remove fleet_include_default"
```

---

### Task 13: Add fleet tuned interaction tests

**Files:**
- Test: `inspectah-core/tests/` or `inspectah-refine/tests/`

This task addresses spec Implementation Note #1 (tuned fleet merge interaction).

- [ ] **Step 1: Write targeted tests**

```rust
#[test]
fn fleet_tuned_universal_stock_remains_excluded() {
    // All hosts have virtual-guest.
    // Merge produces tuned_include: false (stock classifier wins).
    // Universality alone does NOT override the classifier.
}

#[test]
fn fleet_tuned_non_universal_custom_excluded_by_universality() {
    // 2 of 3 hosts have my-custom-profile.
    // Even though custom (classifier would include), non-universal
    // means tuned_include: false.
}

#[test]
fn fleet_tuned_universal_custom_included() {
    // All 3 hosts have my-custom-profile.
    // Custom + universal = tuned_include: true.
    // This is the only case where tuned is included in fleet mode.
}
```

- [ ] **Step 2: Run and verify**

Run: `cargo test -p inspectah-core fleet_tuned -v`
Expected: all pass (if Task 10 was implemented correctly)

- [ ] **Step 3: Commit**

```bash
git add inspectah-core/
git commit -m "test(fleet): add tuned interaction tests for classifier + universality composition"
```

---

### Task 14: Thorn checkpoint 3

**Scope:** Tasks 10-13 (flatpak wiring + fleet aggregate narrowing)

- [ ] **Step 1: Run full workspace checks**

```bash
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

- [ ] **Step 2: Verify fleet invariants**

- Flatpaks are wired into the generic `merge_items()` engine with prevalence
- Fleet merge sets `include: false` on non-universal items (including flatpaks)
- Fleet merge respects tuned stock-profile classifier (universal stock = excluded)
- Tuned universality uses `is_scalar_universal()` helper
- Fleet handlers read stored `.include`, never recompute
- `fleet_include_default` function is deleted

---

### Task 15: Delete session single-host normalization block

**Files:**
- Modify: `inspectah-refine/src/session.rs`

- [ ] **Step 1: Locate and delete the normalization block**

Find the block starting around line 337 with the comment about "Single-host defaults." Delete the entire `if matches!(refine_mode, RefineMode::SingleHost)` block that sets `include = true` on ~12 item types.

- [ ] **Step 2: Delete or update associated tests**

Find tests like `single_host_quadlets_default_to_included`, `single_host_tuned_defaults_to_included`, etc. These tested the normalization block — delete them. The behavior they tested is now handled by the collector (Task 4).

- [ ] **Step 3: Run tests**

Run: `cargo test -p inspectah-refine`
Expected: all pass (collectors now set true, so normalization was already redundant)

- [ ] **Step 4: Commit**

```bash
git add inspectah-refine/
git commit -m "refactor(refine): delete single-host normalization block, now handled by collectors"
```

---

### Task 16: Delete session fleet prevalence gate

**Files:**
- Modify: `inspectah-refine/src/session.rs`

- [ ] **Step 1: Locate and delete the fleet prevalence gate**

Find the block around lines 182-327 that iterates over fleet prevalence data and sets `include = false` on non-universal items. This is the per-section loop that checks `fp.count < fp.total`.

Delete the entire block. Fleet narrowing is now handled in `merge.rs` (Task 11).

- [ ] **Step 2: Update or delete associated tests**

Tests that verified the fleet prevalence gate behavior need updating. The behavior still exists — it just moved to the merge layer.

- [ ] **Step 3: Run tests**

Run: `cargo test --workspace`
Expected: all pass

- [ ] **Step 4: Commit**

```bash
git add inspectah-refine/
git commit -m "refactor(refine): delete fleet prevalence gate, now handled by aggregate merge"
```

---

### Task 17: Delete render layer `is_single_host` overrides

**Files:**
- Modify: `inspectah-pipeline/src/render/containerfile.rs`
- Modify: `inspectah-pipeline/src/render/configtree.rs`

- [ ] **Step 1: Audit ALL render-layer include overrides and fallbacks**

First, find every `is_single_host`, `fleet_meta.is_none()`, and any other
conditional include logic in both renderers:

```bash
grep -n 'is_single_host\|fleet_meta\|\.include\b' inspectah-pipeline/src/render/containerfile.rs inspectah-pipeline/src/render/configtree.rs | head -60
```

Known occurrences (6 across 2 files):

In `containerfile.rs` (3 around lines 891, 896, 901 — quadlets, flatpaks):
```rust
// Before
.filter(|u| u.include || is_single_host)

// After
.filter(|u| u.include)
```

In `configtree.rs` (3 around lines 420, 423, 436 — quadlets, flatpaks):
Same pattern — remove `|| is_single_host`.

**Also check for:**
- **Tuned render fallback:** `containerfile.rs` has a tuned include check
  (around the `kb.tuned_include || ...` line). Remove any
  `fleet_meta.is_none()` fallback — `tuned_include` is now authoritative.
- **systemd_timers in configtree.rs:** Check if `configtree.rs` materializes
  local `systemd_timers` without checking `.include`. If it does, add the
  `.include` filter — the include flag must be authoritative everywhere.
- Any other section that renders content without gating on `.include`.

The goal: after this task, every render path in both files gates exclusively
on `.include`. No overrides, no fallbacks, no special cases.

Remove `let is_single_host = snap.fleet_meta.is_none();` if it becomes unused.

- [ ] **Step 2: Update tests**

Tests like `test_single_host_quadlet_included_by_default` that use `fleet_meta = None` to trigger the override path need rewriting. They should now set `include: true` directly on the items (matching the new collector behavior).

Tests like `test_excluded_quadlet_not_in_quadlet_dir` that use `fleet_meta = Some(...)` to test fleet exclusion should keep their `fleet_meta` but remove any dependency on the render-layer override.

- [ ] **Step 3: Run tests**

Run: `cargo test -p inspectah-pipeline`
Expected: all pass

- [ ] **Step 4: Commit**

```bash
git add inspectah-pipeline/
git commit -m "refactor(pipeline): delete is_single_host render overrides, include flag is now authoritative"
```

---

### Task 18: Web adapter — locked + reason badge (Kit)

**Files:**
- Modify: `inspectah-web/src/` (adapter and handler files)
- Modify: frontend components that render decision items

- [ ] **Step 1: Pass `locked` and `attention_reason` through the API response DTO**

Find the DTO structs that represent decision items in the web adapter. Add:

```rust
pub locked: bool,
#[serde(skip_serializing_if = "Option::is_none")]
pub attention_reason: Option<String>,
```

Wire them from the projected snapshot's item fields.

- [ ] **Step 2: Frontend — render locked items as visible-but-excluded**

For items where `locked: true`:
- Show the item in the list (not hidden)
- Render the toggle as disabled/grayed out
- Show a reason badge from `attention_reason` (e.g., "image-mode incompatible" or "host state — not image-portable")
- The toggle must not be clickable

- [ ] **Step 3: Test in browser**

Start the dev server and verify:
- Incompatible services appear grayed out with reason badge
- Merge-hostile configs (fstab, crypttab) appear as reference-only
- Clicking a locked toggle does nothing

- [ ] **Step 4: Commit**

```bash
git add inspectah-web/
git commit -m "feat(web): render locked items as visible-but-excluded with reason badges"
```

---

### Task 19: TUI — locked display (Kit)

**Files:**
- Modify: `inspectah-tui/src/` (relevant section renderers)

- [ ] **Step 1: Respect `locked` in TUI toggle rendering**

In the TUI section renderers, when displaying items with `locked: true`:
- Show the item (don't hide it)
- Display a lock indicator or "LOCKED" badge
- Show the reason inline
- Prevent toggling via keyboard

- [ ] **Step 2: Test in terminal**

Run the TUI and verify locked items display correctly.

- [ ] **Step 3: Commit**

```bash
git add inspectah-tui/
git commit -m "feat(tui): display locked items with reason, prevent toggling"
```

---

### Task 20: Thorn checkpoint 4 (final)

**Scope:** Full implementation (Tasks 1-19)

- [ ] **Step 1: Run full workspace checks**

```bash
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

- [ ] **Step 2: Negative artifact tests — locked items stay out of rendered output**

Write tests that prove semantic exclusions survive all the way through
rendering:

```rust
#[test]
fn crypttab_does_not_appear_in_rendered_artifacts() {
    // Build snapshot with /etc/crypttab include: true, locked: true
    // Run through normalize + Containerfile render + configtree materialization
    // Assert NEITHER the Containerfile text NOR the config tree contains crypttab
}

#[test]
fn incompatible_service_dropin_not_in_configtree() {
    // Build snapshot with dnf-makecache drop-in, locked: true
    // Run configtree materialization
    // Assert drop-in file is NOT in the output tree
}

#[test]
fn stock_tuned_profile_not_in_containerfile_after_cleanup() {
    // Build single-host snapshot with virtual-guest tuned profile
    // Run through normalize + render (NO is_single_host override anymore)
    // Assert output does NOT contain "tuned" section
}
```

- [ ] **Step 3: Regression guard — validate include:false ownership**

Write or run a test that:
1. Collects a single-host snapshot
2. Verifies every item has `include: true` EXCEPT:
   - Items excluded by RPM classifier (base-image deps)
   - Items excluded by config classifier (baseline, orphaned)
   - Items excluded by tuned classifier (stock profiles)
   - Items locked by semantic exclusions (incompatible services, merge-hostile configs)
3. Any `include: false` not in those categories is a regression

- [ ] **Step 3: Fleet end-to-end test**

1. Merge multiple host snapshots
2. Verify non-universal items have `include: false`
3. Verify universal items have `include: true`
4. Verify fleet handlers pass through stored values
5. Verify tuned classifier + universality compose correctly

- [ ] **Step 4: Verify render layer is clean**

Grep for any remaining `is_single_host`, `fleet_meta.is_none()`, or `fleet_include_default` references:

```bash
grep -rn 'is_single_host\|fleet_include_default\|fleet_meta\.is_none' inspectah-pipeline/ inspectah-web/ inspectah-refine/src/session.rs
```

Expected: zero results (all three patterns should be eliminated)

- [ ] **Step 5: Final commit if any cleanup needed**

```bash
git add -A
git commit -m "test: add regression guards for unified include-default model"
```
