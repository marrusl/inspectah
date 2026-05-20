# Fleet Aggregate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the `inspectah fleet` and `inspectah fleet init` commands that aggregate N single-host tarballs into a fleet tarball with prevalence metadata.

**Architecture:** Hybrid merge engine (generic `FleetMergeable` trait + thin per-section adapters) in `inspectah-core`. CLI commands in `inspectah-cli`. Fleet tarball inherits the scan tarball contract, adding `fleet/variants/` for content variant storage. The `VariantSelection` enum replaces `tie`/`tie_winner` bools on variant-capable types. `FleetSnapshotMeta` on `InspectionSnapshot` carries fleet-level metadata.

**Tech Stack:** Rust (2024 edition), serde, clap derive, toml, sha2, chrono, insta (snapshot tests), tar + flate2 (tarball packaging).

**Spec:** `docs/specs/proposed/2026-05-19-fleet-aggregate-spec.md`

**Repo:** `/Users/mrussell/Work/bootc-migration/inspectah/` (branch: `rust`)

**Cargo PATH:** `export PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH"`

---

## File Map

### New files

| File | Responsibility |
|------|---------------|
| `inspectah-core/src/fleet/mod.rs` | `merge_snapshots()` orchestrator, public API |
| `inspectah-core/src/fleet/merge.rs` | `FleetMergeable` trait, generic merge function, section adapters |
| `inspectah-core/src/fleet/validate.rs` | Pre-merge validation (hard errors + warnings) |
| `inspectah-core/src/fleet/manifest.rs` | TOML manifest parsing (`FleetManifest`) |
| `inspectah-core/tests/fleet_merge_test.rs` | Integration tests for merge engine |
| `inspectah-core/tests/fleet_validate_test.rs` | Integration tests for validation |
| `inspectah-cli/src/commands/fleet.rs` | `fleet` + `fleet init` CLI commands |

### Modified files

| File | Changes |
|------|---------|
| `inspectah-core/src/types/fleet.rs` | Add `VariantSelection` enum, `FleetSnapshotMeta` struct |
| `inspectah-core/src/types/config.rs` | Replace `tie`/`tie_winner` with `variant_selection: VariantSelection` on `ConfigFileEntry` |
| `inspectah-core/src/types/services.rs` | Replace `tie`/`tie_winner` with `variant_selection` on `SystemdDropIn` |
| `inspectah-core/src/types/containers.rs` | Replace `tie`/`tie_winner` with `variant_selection` on `QuadletUnit` and `ComposeFile` |
| `inspectah-core/src/types/kernelboot.rs` | Add `fleet` field to `KernelModule`, `SysctlOverride` |
| `inspectah-core/src/snapshot.rs` | Add `fleet_meta` field, bump `SCHEMA_VERSION` |
| `inspectah-core/src/lib.rs` | Register `fleet` module |
| `inspectah-core/Cargo.toml` | Add `sha2`, `toml`, `chrono` dependencies |
| `inspectah-cli/src/commands/mod.rs` | Register `fleet` subcommand |
| `inspectah-cli/src/main.rs` | Wire `Commands::Fleet` variant |
| `inspectah-pipeline/src/render/audit.rs` | Fleet summary section in audit report |

### Types NOT modified (already correct)

| File | Note |
|------|------|
| `inspectah-core/src/types/rpm.rs` | `RepoFile` has NO `tie`/`tie_winner` — not a variant type. `EnabledModuleStream` and `VersionLockEntry` already have `fleet`/`include`. |
| `inspectah-core/src/types/nonrpm.rs` | `NonRpmItem` already has `fleet: Option<FleetPrevalence>` and `content: String`. No changes needed. |

---

## Live Type Reference

Types that implement `FleetMergeable` (have both `fleet` and `include`):

| Type | Identity Key | Has Variants | Variant Source |
|------|-------------|-------------|----------------|
| `PackageEntry` | `name.arch` | No | — |
| `RepoFile` | `path` | No (no tie/tie_winner) | — |
| `ConfigFileEntry` | `path` | Yes | `content` field |
| `ServiceStateChange` | `unit` | No | — |
| `SystemdDropIn` | `path` | Yes | `content` field |
| `QuadletUnit` | `path` | Yes | `content` field |
| `ComposeFile` | `path` | Yes | hash of serialized `images` (no `content` field) |
| `EnabledModuleStream` | `module_name:stream` | No | — |
| `VersionLockEntry` | `name.arch` | No | — |
| `SelinuxPortLabel` | `protocol:port` | No | — |
| `FirewallZone` | `path` | No (has content but no tie/tie_winner) | — |
| `CronJob` | `path` | No | — |
| `NMConnection` | `path` | No | Note: `include` is `Option<bool>`, needs special handling |
| `NonRpmItem` | `name` | No (variant support deferred) | — |
| `KernelModule` | `name` | No | **Needs `fleet` field added** |
| `SysctlOverride` | `key` | No | **Needs `fleet` field added** |

Types handled by section adapters (no `fleet`/`include`):

| Type | Section | Strategy |
|------|---------|----------|
| `VersionChange` | `rpm` | Dedup by `name.arch` |
| `RpmVaEntry` | `rpm` | Dedup by identity |
| `FstabEntry` | `storage` | Dedup by identity |
| `MountPoint` | `storage` | Dedup by `target` |
| `LvmVolume` | `storage` | Dedup by identity |
| `VarDirectory` | `storage` | Dedup by identity |
| `CredentialRef` | `storage` | Dedup by identity |
| `SystemdTimer` | `scheduled_tasks` | Dedup by identity |
| `AtJob` | `scheduled_tasks` | Dedup by identity |
| `GeneratedTimerUnit` | `scheduled_tasks` | Dedup by `unit_name` |
| `ProxyEntry` | `network` | Dedup by `source` |
| `FirewallDirectRule` | `network` | Dedup by identity |
| `StaticRouteFile` | `network` | Dedup by identity |
| `CarryForwardFile` | `selinux` | Dedup by `path` |
| `UserGroupSection` | `users_groups` | JSON-level dedup (users/groups are `Vec<serde_json::Value>`) |

---

## Task 1: VariantSelection Enum

**Files:**
- Modify: `inspectah-core/src/types/fleet.rs`

- [ ] **Step 1: Write failing test for VariantSelection serde roundtrip**

Add to the existing `#[cfg(test)]` module in `fleet.rs`:

```rust
#[test]
fn test_variant_selection_default() {
    let vs = VariantSelection::default();
    assert_eq!(vs, VariantSelection::Only);
}

#[test]
fn test_variant_selection_serde_roundtrip() {
    for variant in [VariantSelection::Only, VariantSelection::Selected, VariantSelection::Alternative] {
        let json = serde_json::to_string(&variant).unwrap();
        let parsed: VariantSelection = serde_json::from_str(&json).unwrap();
        assert_eq!(variant, parsed);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-core -- test_variant_selection`
Expected: FAIL — `VariantSelection` not defined

- [ ] **Step 3: Implement VariantSelection enum**

Add to `inspectah-core/src/types/fleet.rs` before `FleetPrevalence`:

```rust
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum VariantSelection {
    #[default]
    Only,
    Selected,
    Alternative,
}
```

- [ ] **Step 4: Run tests, verify PASS**

Run: `cargo test -p inspectah-core -- test_variant_selection`

- [ ] **Step 5: Commit**

```bash
git add inspectah-core/src/types/fleet.rs
git commit -m "feat(core): add VariantSelection enum for fleet content variants"
```

---

## Task 2: Replace tie/tie_winner with VariantSelection

**Files:**
- Modify: `inspectah-core/src/types/config.rs` (ConfigFileEntry)
- Modify: `inspectah-core/src/types/services.rs` (SystemdDropIn)
- Modify: `inspectah-core/src/types/containers.rs` (QuadletUnit, ComposeFile)

**NOT modified:** `inspectah-core/src/types/rpm.rs` — `RepoFile` does not have `tie`/`tie_winner`.

This is a schema-breaking change per spec. The approach: replace the fields, update all consumers, update golden/fixture files. No serde-alias backward compat — the spec explicitly says "schema-breaking change without regard to Go compatibility."

- [ ] **Step 1: Find all tie/tie_winner usage across the codebase**

Run: `rg -n 'tie_winner|\.tie\b' --type rust | rg -v 'target/'`

Document every file that reads or writes `tie`/`tie_winner`. These all need updating.

- [ ] **Step 2: Replace fields on ConfigFileEntry**

In `inspectah-core/src/types/config.rs`, replace:
```rust
    #[serde(default)]
    pub tie: bool,
    #[serde(default)]
    pub tie_winner: bool,
```
with:
```rust
    #[serde(default)]
    pub variant_selection: VariantSelection,
```

Add: `use super::fleet::VariantSelection;`

- [ ] **Step 3: Replace fields on SystemdDropIn**

In `inspectah-core/src/types/services.rs`, same replacement on `SystemdDropIn`. Add the import.

- [ ] **Step 4: Replace fields on QuadletUnit and ComposeFile**

In `inspectah-core/src/types/containers.rs`, same replacement on both structs. Add the import.

- [ ] **Step 5: Fix all compilation errors across workspace**

Run: `cargo build --workspace 2>&1`

Replace patterns everywhere:
- `entry.tie_winner` → `entry.variant_selection == VariantSelection::Selected`
- `entry.tie && !entry.tie_winner` → `entry.variant_selection == VariantSelection::Alternative`
- `entry.tie = true; entry.tie_winner = true;` → `entry.variant_selection = VariantSelection::Selected;`
- `entry.tie = true; entry.tie_winner = false;` → `entry.variant_selection = VariantSelection::Alternative;`

Key files to check: `inspectah-pipeline/src/render/containerfile.rs`, `inspectah-pipeline/src/render/configtree.rs`, `inspectah-refine/src/session.rs`, `inspectah-web/src/handlers.rs`, and tests throughout.

- [ ] **Step 6: Update snapshot migration in `inspectah-core/src/snapshot.rs`**

The current `migrate()` function runs on `&mut InspectionSnapshot` post-deserialization. Since the old JSON has `"tie": true, "tie_winner": true` and the new struct has `"variant_selection": "Selected"`, deserialization of old snapshots will produce `variant_selection: Only` (the default) and silently drop the old bool fields.

Add a raw-JSON pre-patch in `InspectionSnapshot::load()` — the same pattern `inspectah-refine/src/normalize.rs::patch_missing_includes()` uses. Walk the raw `serde_json::Value` before typed deserialization and convert:
- `tie_winner: true` → insert `"variant_selection": "Selected"`, remove `tie`/`tie_winner`
- `tie: true, tie_winner: false` → insert `"variant_selection": "Alternative"`, remove `tie`/`tie_winner`
- neither → leave as-is (serde default produces `Only`)

Apply this patch to config files, drop-ins, quadlet units, and compose files.

- [ ] **Step 7: Update golden files and snapshot test fixtures**

Run: `cargo insta test --workspace` and review changes.

Update any JSON fixtures in `tests/` or `testdata/` that contain `"tie"` or `"tie_winner"` fields.

- [ ] **Step 8: Run full workspace tests**

Run: `cargo test --workspace`
Expected: PASS

- [ ] **Step 9: Commit**

```bash
git add inspectah-core/ inspectah-pipeline/ inspectah-refine/ inspectah-web/
git commit -m "refactor(core): replace tie/tie_winner bools with VariantSelection enum

Schema-breaking change. Adds raw-JSON pre-patch in load() to
migrate old tie/tie_winner bools to VariantSelection values.
Eliminates invalid state combinations (4 bool states to 3 enum
variants)."
```

---

## Task 3: FleetSnapshotMeta + fleet_meta on InspectionSnapshot

**Files:**
- Modify: `inspectah-core/src/types/fleet.rs`
- Modify: `inspectah-core/src/snapshot.rs`
- Modify: `inspectah-core/Cargo.toml`

- [ ] **Step 1: Add `chrono` dependency**

Add to `inspectah-core/Cargo.toml` under `[dependencies]`:
```toml
chrono = { version = "0.4", features = ["serde"] }
```

- [ ] **Step 2: Write failing test for FleetSnapshotMeta**

In `inspectah-core/src/types/fleet.rs` tests:

```rust
use std::collections::BTreeMap;

#[test]
fn test_fleet_snapshot_meta_roundtrip() {
    let meta = FleetSnapshotMeta {
        label: "web-servers".into(),
        host_count: 50,
        hostnames: vec!["host-a".into(), "host-b".into()],
        merged_at: "2026-05-20T12:00:00Z".into(),
        baseline_provisional: true,
        section_host_counts: BTreeMap::from([
            ("config".into(), 48usize),
            ("rpm".into(), 50),
        ]),
    };
    let json = serde_json::to_string(&meta).unwrap();
    let parsed: FleetSnapshotMeta = serde_json::from_str(&json).unwrap();
    assert_eq!(meta, parsed);
}
```

- [ ] **Step 3: Implement FleetSnapshotMeta**

```rust
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FleetSnapshotMeta {
    pub label: String,
    pub host_count: usize,
    pub hostnames: Vec<String>,
    pub merged_at: String,
    #[serde(default)]
    pub baseline_provisional: bool,
    #[serde(default)]
    pub section_host_counts: BTreeMap<String, usize>,
}
```

- [ ] **Step 4: Add fleet_meta to InspectionSnapshot**

In `inspectah-core/src/snapshot.rs`, add to the struct:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub fleet_meta: Option<crate::types::fleet::FleetSnapshotMeta>,
```

Bump `SCHEMA_VERSION` (currently 16 → 17).

Update `InspectionSnapshot::new()` to include `fleet_meta: None`.

Update the schema version range in `load()` (the `MIN_SCHEMA..=SCHEMA_VERSION` gate).

- [ ] **Step 5: Write snapshot roundtrip tests**

```rust
#[test]
fn test_snapshot_with_fleet_meta_roundtrip() {
    let mut snap = InspectionSnapshot::new();
    snap.fleet_meta = Some(FleetSnapshotMeta { /* ... */ });
    let json = serde_json::to_string(&snap).unwrap();
    let parsed: InspectionSnapshot = serde_json::from_str(&json).unwrap();
    assert_eq!(snap.fleet_meta, parsed.fleet_meta);
}

#[test]
fn test_snapshot_without_fleet_meta_omits_field() {
    let snap = InspectionSnapshot::new();
    let json = serde_json::to_string(&snap).unwrap();
    assert!(!json.contains("fleet_meta"));
}
```

- [ ] **Step 6: Run workspace tests, fix schema version gates**

Run: `cargo test --workspace`

The schema version bump may break `load_for_refine()` if its version range is hardcoded. Update accordingly. Also check `inspectah-core/tests/parity_gate.rs`.

- [ ] **Step 7: Commit**

```bash
git add inspectah-core/src/types/fleet.rs inspectah-core/src/snapshot.rs inspectah-core/Cargo.toml
git commit -m "feat(core): add FleetSnapshotMeta and fleet_meta field on InspectionSnapshot

Bumps SCHEMA_VERSION to 17. fleet_meta is None on single-host
snapshots and omitted from serialized JSON."
```

---

## Task 4: Add fleet field to types that need it

**Files:**
- Modify: `inspectah-core/src/types/kernelboot.rs`

Only two types need fleet added. `NonRpmItem` already has `fleet: Option<FleetPrevalence>`.

- [ ] **Step 1: Add fleet field to KernelModule and SysctlOverride**

In `inspectah-core/src/types/kernelboot.rs`, add import:
```rust
use super::fleet::FleetPrevalence;
```

Add to `KernelModule`:
```rust
    pub fleet: Option<FleetPrevalence>,
```

Add to `SysctlOverride`:
```rust
    pub fleet: Option<FleetPrevalence>,
```

- [ ] **Step 2: Run workspace tests**

Run: `cargo test --workspace`
Expected: PASS (new `Option` fields default to `None` via serde)

- [ ] **Step 3: Commit**

```bash
git add inspectah-core/src/types/kernelboot.rs
git commit -m "feat(core): add fleet prevalence field to KernelModule and SysctlOverride"
```

---

## Task 5: FleetMergeable Trait + Implementations

**Files:**
- Create: `inspectah-core/src/fleet/mod.rs`
- Create: `inspectah-core/src/fleet/merge.rs`
- Modify: `inspectah-core/src/lib.rs`
- Modify: `inspectah-core/Cargo.toml` (add `sha2`)
- Test: `inspectah-core/tests/fleet_merge_test.rs`

- [ ] **Step 1: Add sha2 dependency and create fleet module**

Add to `inspectah-core/Cargo.toml`:
```toml
sha2 = "0.10"
```

Create `inspectah-core/src/fleet/mod.rs`:
```rust
pub mod merge;
```

Register in `inspectah-core/src/lib.rs`:
```rust
pub mod fleet;
```

- [ ] **Step 2: Define the FleetMergeable trait**

Create `inspectah-core/src/fleet/merge.rs`:

```rust
use std::borrow::Cow;
use crate::types::fleet::{FleetPrevalence, VariantSelection};

pub trait FleetMergeable: Clone {
    fn identity_key(&self) -> Cow<'_, str>;
    fn fleet_mut(&mut self) -> &mut Option<FleetPrevalence>;
    fn set_include(&mut self, val: bool);
    fn variant_selection_mut(&mut self) -> Option<&mut VariantSelection> { None }
    fn content_variant_key(&self) -> Option<Cow<'_, str>> { None }
}
```

- [ ] **Step 3: Write failing test for PackageEntry**

Create `inspectah-core/tests/fleet_merge_test.rs`:

```rust
use inspectah_core::fleet::merge::FleetMergeable;
use inspectah_core::types::rpm::PackageEntry;

#[test]
fn test_package_entry_identity_key_is_name_dot_arch() {
    let pkg = PackageEntry {
        name: "httpd".into(),
        arch: "x86_64".into(),
        ..Default::default()
    };
    assert_eq!(pkg.identity_key().as_ref(), "httpd.x86_64");
}
```

- [ ] **Step 4: Implement FleetMergeable for all prevalence-tracked types**

Implement per the Live Type Reference table above. Key implementations:

```rust
// PackageEntry: identity = name.arch, no variants
impl FleetMergeable for PackageEntry {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Owned(format!("{}.{}", self.name, self.arch))
    }
    fn fleet_mut(&mut self) -> &mut Option<FleetPrevalence> { &mut self.fleet }
    fn set_include(&mut self, val: bool) { self.include = val; }
}

// ConfigFileEntry: identity = path, variants via content hash
impl FleetMergeable for ConfigFileEntry {
    fn identity_key(&self) -> Cow<'_, str> { Cow::Borrowed(&self.path) }
    fn fleet_mut(&mut self) -> &mut Option<FleetPrevalence> { &mut self.fleet }
    fn set_include(&mut self, val: bool) { self.include = val; }
    fn variant_selection_mut(&mut self) -> Option<&mut VariantSelection> {
        Some(&mut self.variant_selection)
    }
    fn content_variant_key(&self) -> Option<Cow<'_, str>> {
        use sha2::{Sha256, Digest};
        Some(Cow::Owned(format!("{:x}", Sha256::digest(self.content.as_bytes()))))
    }
}

// ComposeFile: identity = path, variants via serialized images hash
// NOTE: ComposeFile has NO content field — variant key hashes serialized images
impl FleetMergeable for ComposeFile {
    fn identity_key(&self) -> Cow<'_, str> { Cow::Borrowed(&self.path) }
    fn fleet_mut(&mut self) -> &mut Option<FleetPrevalence> { &mut self.fleet }
    fn set_include(&mut self, val: bool) { self.include = val; }
    fn variant_selection_mut(&mut self) -> Option<&mut VariantSelection> {
        Some(&mut self.variant_selection)
    }
    fn content_variant_key(&self) -> Option<Cow<'_, str>> {
        use sha2::{Sha256, Digest};
        let serialized = serde_json::to_string(&self.images).unwrap_or_default();
        Some(Cow::Owned(format!("{:x}", Sha256::digest(serialized.as_bytes()))))
    }
}

// ServiceStateChange: identity = unit, no variants
// NOTE: current_state is ServiceUnitState enum, default_state is Option<PresetDefault>
impl FleetMergeable for ServiceStateChange {
    fn identity_key(&self) -> Cow<'_, str> { Cow::Borrowed(&self.unit) }
    fn fleet_mut(&mut self) -> &mut Option<FleetPrevalence> { &mut self.fleet }
    fn set_include(&mut self, val: bool) { self.include = val; }
}

// EnabledModuleStream: identity = module_name:stream
impl FleetMergeable for EnabledModuleStream {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Owned(format!("{}:{}", self.module_name, self.stream))
    }
    fn fleet_mut(&mut self) -> &mut Option<FleetPrevalence> { &mut self.fleet }
    fn set_include(&mut self, val: bool) { self.include = val; }
}

// VersionLockEntry: identity = name.arch
impl FleetMergeable for VersionLockEntry {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Owned(format!("{}.{}", self.name, self.arch))
    }
    fn fleet_mut(&mut self) -> &mut Option<FleetPrevalence> { &mut self.fleet }
    fn set_include(&mut self, val: bool) { self.include = val; }
}

// SelinuxPortLabel: identity = protocol:port
impl FleetMergeable for SelinuxPortLabel {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Owned(format!("{}:{}", self.protocol, self.port))
    }
    fn fleet_mut(&mut self) -> &mut Option<FleetPrevalence> { &mut self.fleet }
    fn set_include(&mut self, val: bool) { self.include = val; }
}

// NMConnection: identity = path
// NOTE: include is Option<bool>, not bool — needs special set_include
impl FleetMergeable for NMConnection {
    fn identity_key(&self) -> Cow<'_, str> { Cow::Borrowed(&self.path) }
    fn fleet_mut(&mut self) -> &mut Option<FleetPrevalence> { &mut self.fleet }
    fn set_include(&mut self, val: bool) { self.include = Some(val); }
}
```

Also implement for: `RepoFile` (identity: `path`), `SystemdDropIn` (identity: `path`, has variants), `QuadletUnit` (identity: `path`, has variants), `FirewallZone` (identity: `path`), `CronJob` (identity: `path`), `KernelModule` (identity: `name`), `SysctlOverride` (identity: `key`), `NonRpmItem` (identity: `name`).

- [ ] **Step 5: Write tests for variant-capable types**

```rust
#[test]
fn test_config_file_has_variant_key() {
    let entry = ConfigFileEntry {
        path: "/etc/foo.conf".into(),
        content: "val".into(),
        ..Default::default()
    };
    assert!(entry.content_variant_key().is_some());
}

#[test]
fn test_compose_file_variant_key_uses_images() {
    let cf = ComposeFile {
        path: "/opt/app/docker-compose.yml".into(),
        images: vec![],
        ..Default::default()
    };
    assert!(cf.content_variant_key().is_some());
}

#[test]
fn test_package_entry_has_no_variant_key() {
    assert!(PackageEntry::default().content_variant_key().is_none());
}

#[test]
fn test_repo_file_has_no_variant_key() {
    assert!(RepoFile::default().content_variant_key().is_none());
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test -p inspectah-core --test fleet_merge_test`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add inspectah-core/src/fleet/ inspectah-core/src/lib.rs inspectah-core/Cargo.toml inspectah-core/tests/fleet_merge_test.rs
git commit -m "feat(core): FleetMergeable trait with impls for all prevalence-tracked types"
```

---

## Task 6: Generic Merge Function

**Files:**
- Modify: `inspectah-core/src/fleet/merge.rs`
- Test: `inspectah-core/tests/fleet_merge_test.rs`

- [ ] **Step 1: Write failing test for basic prevalence merge**

```rust
#[test]
fn test_merge_items_two_hosts_same_package() {
    let items: Vec<(usize, PackageEntry)> = vec![
        (0, PackageEntry { name: "httpd".into(), arch: "x86_64".into(), ..Default::default() }),
        (1, PackageEntry { name: "httpd".into(), arch: "x86_64".into(), ..Default::default() }),
    ];
    let hostnames = vec!["host-a".to_string(), "host-b".to_string()];
    let merged = merge_items(items, 2, &hostnames);
    assert_eq!(merged.len(), 1);
    let fleet = merged[0].fleet.as_ref().unwrap();
    assert_eq!(fleet.count, 2);
    assert_eq!(fleet.total, 2);
    assert_eq!(fleet.hosts, vec!["host-a", "host-b"]); // sorted
    assert!(merged[0].include);
}
```

- [ ] **Step 2: Implement merge_items**

```rust
pub fn merge_items<T: FleetMergeable>(
    items: Vec<(usize, T)>,
    total_hosts: usize,
    hostnames: &[String],
) -> Vec<T> {
    let mut groups: HashMap<String, Vec<(usize, T)>> = HashMap::new();
    for (host_idx, item) in items {
        let key = item.identity_key().into_owned();
        groups.entry(key).or_default().push((host_idx, item));
    }

    let mut result: Vec<T> = Vec::new();
    for (_key, group) in &mut groups {
        group.sort_by_key(|(idx, _)| *idx);

        let mut hosts: Vec<String> = group.iter()
            .map(|(idx, _)| hostnames[*idx].clone())
            .collect();
        hosts.sort();
        hosts.dedup();
        let count = hosts.len() as i32;

        let has_variants = group[0].1.content_variant_key().is_some();

        if has_variants {
            result.extend(merge_with_variants(group, total_hosts, &hosts));
        } else {
            let mut representative = group[0].1.clone();
            *representative.fleet_mut() = Some(FleetPrevalence {
                count,
                total: total_hosts as i32,
                hosts,
            });
            representative.set_include(true);
            result.push(representative);
        }
    }

    result.sort_by(|a, b| a.identity_key().cmp(&b.identity_key()));
    result
}
```

Note: host dedup is important — the same host index can appear multiple times if a snapshot has duplicate items within a section.

- [ ] **Step 3: Run test, verify PASS**

- [ ] **Step 4: Write failing test for variant merge**

```rust
#[test]
fn test_merge_items_variant_selection() {
    let items: Vec<(usize, ConfigFileEntry)> = vec![
        (0, ConfigFileEntry { path: "/etc/foo.conf".into(), content: "version_a".into(), ..Default::default() }),
        (1, ConfigFileEntry { path: "/etc/foo.conf".into(), content: "version_a".into(), ..Default::default() }),
        (2, ConfigFileEntry { path: "/etc/foo.conf".into(), content: "version_b".into(), ..Default::default() }),
    ];
    let hostnames = vec!["h1".into(), "h2".into(), "h3".into()];
    let merged = merge_items(items, 3, &hostnames);
    assert_eq!(merged.len(), 2);
    let selected = merged.iter().find(|e| e.variant_selection == VariantSelection::Selected).unwrap();
    let alt = merged.iter().find(|e| e.variant_selection == VariantSelection::Alternative).unwrap();
    assert_eq!(selected.content, "version_a");
    assert_eq!(selected.fleet.as_ref().unwrap().count, 2);
    assert_eq!(alt.content, "version_b");
    assert_eq!(alt.fleet.as_ref().unwrap().count, 1);
}

#[test]
fn test_merge_items_single_variant_is_only() {
    let items: Vec<(usize, ConfigFileEntry)> = vec![
        (0, ConfigFileEntry { path: "/etc/foo.conf".into(), content: "same".into(), ..Default::default() }),
        (1, ConfigFileEntry { path: "/etc/foo.conf".into(), content: "same".into(), ..Default::default() }),
    ];
    let hostnames = vec!["h1".into(), "h2".into()];
    let merged = merge_items(items, 2, &hostnames);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].variant_selection, VariantSelection::Only);
}
```

- [ ] **Step 5: Implement merge_with_variants**

Key behavior: subgroup by content hash. If only one subgroup exists → `Only` (not `Selected`). If multiple → most-prevalent is `Selected`, rest are `Alternative`. Ties broken by lexicographic content hash.

```rust
fn merge_with_variants<T: FleetMergeable>(
    group: &mut [(usize, T)],
    total_hosts: usize,
    all_hosts_sorted: &[String],
) -> Vec<T> {
    use sha2::{Sha256, Digest};

    let mut subgroups: HashMap<String, Vec<(usize, &T)>> = HashMap::new();
    for (idx, item) in group.iter() {
        let hash = item.content_variant_key().unwrap().into_owned();
        subgroups.entry(hash).or_default().push((*idx, item));
    }

    // Single content version across all hosts → Only, not Selected
    if subgroups.len() == 1 {
        let (_, subgroup) = subgroups.into_iter().next().unwrap();
        let mut item = subgroup[0].1.clone();
        let mut hosts: Vec<String> = subgroup.iter()
            .map(|(idx, _)| all_hosts_sorted[*idx].clone())
            .collect();
        hosts.sort();
        hosts.dedup();
        *item.fleet_mut() = Some(FleetPrevalence {
            count: hosts.len() as i32,
            total: total_hosts as i32,
            hosts,
        });
        item.set_include(true);
        // variant_selection stays Only (default)
        return vec![item];
    }

    // Multiple content versions — rank by prevalence, tie-break by hash
    let mut ranked: Vec<(String, Vec<(usize, &T)>)> = subgroups.into_iter().collect();
    ranked.sort_by(|(hash_a, hosts_a), (hash_b, hosts_b)| {
        hosts_b.len().cmp(&hosts_a.len())
            .then_with(|| hash_a.cmp(hash_b))
    });

    let mut result = Vec::new();
    for (i, (_hash, subgroup)) in ranked.iter().enumerate() {
        let mut hosts: Vec<String> = subgroup.iter()
            .map(|(idx, _)| all_hosts_sorted[*idx].clone())
            .collect();
        hosts.sort();
        hosts.dedup();

        let mut item = subgroup[0].1.clone();
        *item.fleet_mut() = Some(FleetPrevalence {
            count: hosts.len() as i32,
            total: total_hosts as i32,
            hosts,
        });
        item.set_include(true);
        if let Some(vs) = item.variant_selection_mut() {
            *vs = if i == 0 { VariantSelection::Selected } else { VariantSelection::Alternative };
        }
        result.push(item);
    }
    result
}
```

- [ ] **Step 6: Write deterministic tie-break test**

Test that reversing input order produces the same Selected winner.

- [ ] **Step 7: Run all merge tests**

Run: `cargo test -p inspectah-core --test fleet_merge_test`
Expected: PASS

- [ ] **Step 8: Commit**

```bash
git add inspectah-core/src/fleet/merge.rs inspectah-core/tests/fleet_merge_test.rs
git commit -m "feat(core): generic fleet merge function with prevalence and variant handling"
```

---

## Task 7: FleetManifest TOML Parsing

**Files:**
- Create: `inspectah-core/src/fleet/manifest.rs`
- Modify: `inspectah-core/src/fleet/mod.rs`
- Modify: `inspectah-core/Cargo.toml`

- [ ] **Step 1: Add toml dependency**

Add to `inspectah-core/Cargo.toml`:
```toml
toml = "0.8"
```

- [ ] **Step 2: Write tests and implement FleetManifest**

See original plan Task 7 — the manifest struct and tests are correct as written. The type is:

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct FleetManifest {
    pub label: Option<String>,
    pub baseline: Option<String>,
    pub sources: Vec<PathBuf>,
}
```

With `parse()` and `load()` methods. `load()` resolves `sources` paths relative to the manifest file's parent directory.

- [ ] **Step 3: Register module, run tests, commit**

```bash
git add inspectah-core/src/fleet/manifest.rs inspectah-core/src/fleet/mod.rs inspectah-core/Cargo.toml
git commit -m "feat(core): FleetManifest TOML parsing with path resolution"
```

---

## Task 8: Fleet Validation

**Files:**
- Create: `inspectah-core/src/fleet/validate.rs`
- Modify: `inspectah-core/src/fleet/mod.rs`
- Test: `inspectah-core/tests/fleet_validate_test.rs`

**Important seam note:** `validate_snapshots()` takes `&[InspectionSnapshot]` — already-parsed snapshots. It CANNOT detect unparseable files because those never become snapshots. The `UnparseableFile` warning is emitted by the CLI layer during tarball loading (Task 11), not by core validation.

- [ ] **Step 1: Define error and warning types**

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum FleetValidationError {
    TooFewSnapshots { count: usize },
    SchemaVersionMismatch { versions: Vec<u32> },
    DuplicateHostname { hostname: String },
    ArchitectureMismatch { architectures: Vec<String> },
    EmptySnapshot { hostname: String },
    OsMajorVersionMismatch { versions: Vec<String> },
}

#[derive(Debug, Clone, PartialEq)]
pub enum FleetWarning {
    StaleScanDates { spread_description: String },
    BaselineConflict { distribution: Vec<(String, usize)>, selected: String },
    MinorVersionSpread { versions: Vec<String> },
    SystemTypeMismatch { types: Vec<String> },
}
```

Note: `StaleScanDates` uses a description string rather than parsed timestamps because the Rust collector may not populate `meta["timestamp"]`. The implementer should check what metadata keys are available on real snapshots and adapt. Tarball file modification times are a fallback.

- [ ] **Step 2: Write tests for each hard error and warning**

Tests should construct `InspectionSnapshot` instances with the relevant fields set and verify validation catches the problems.

For hostname extraction: check `meta.get("hostname")`. If unavailable, derive from tarball filename (CLI layer responsibility, passed to validation as a separate hostname list).

- [ ] **Step 3: Implement validation, run tests, commit**

```bash
git add inspectah-core/src/fleet/validate.rs inspectah-core/tests/fleet_validate_test.rs inspectah-core/src/fleet/mod.rs
git commit -m "feat(core): fleet pre-merge validation with hard errors and warnings"
```

---

## Task 9: Section Adapters

**Files:**
- Modify: `inspectah-core/src/fleet/merge.rs`
- Test: `inspectah-core/tests/fleet_merge_test.rs`

Thin adapter functions per section. Each extracts fields, calls `merge_items` for prevalence-tracked types, and handles non-prevalence types with dedup helpers.

- [ ] **Step 1: Implement dedup helpers**

```rust
pub(crate) fn dedup_strings(lists: Vec<Vec<String>>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for list in lists {
        for item in list {
            if seen.insert(item.clone()) {
                result.push(item);
            }
        }
    }
    result.sort();
    result
}
```

- [ ] **Step 2: Implement RPM section adapter**

Handles: `packages_added`, `base_image_only` (merge via `merge_items`), `repo_files`, `gpg_keys` (merge via `merge_items` — no variants, RepoFile has no tie/tie_winner), `module_streams` (merge via `merge_items`), `version_locks` (merge via `merge_items`), `dnf_history_removed` / `module_stream_conflicts` / `multiarch_packages` / `duplicate_packages` (dedup strings), `version_changes` (dedup by `name.arch`).

Pass-through fields from selected baseline: `baseline_package_names`, `baseline_suppressed`, `no_baseline`, `base_image`. These are derived from the merged `target_image` selection (handled in Task 10's orchestrator, not here).

Pass-through from most-prevalent host: `leaf_packages`, `auto_packages`, `leaf_dep_tree`, `baseline_module_streams`, `versionlock_command_output`, `rpm_va`, `ostree_overrides`, `ostree_removals`, `repo_providing_packages`, `file_ownership`.

- [ ] **Step 3: Implement remaining section adapters**

Each adapter takes `Vec<Option<SectionType>>` (one per host, `None` if host didn't have that section) + host count + hostnames, returns `Option<SectionType>`.

**Config:** merge `files` via `merge_items` (has variants). Pass through section-level fields.

**Services:** merge `state_changes` via `merge_items` (no variants — typed enum fields: `current_state: ServiceUnitState`, `default_state: Option<PresetDefault>`), merge `drop_ins` via `merge_items` (has variants), dedup `enabled_units`/`disabled_units`.

**Containers:** merge `quadlet_units` (has variants), `compose_files` (has variants — variant key hashes serialized `images`, not a content field). Skip `running_containers` (runtime state, not config).

**Network:** merge `firewall_zones` via `merge_items` (no variants despite having content — no tie/tie_winner), merge `nm_connections` via `merge_items` (note: `include` is `Option<bool>`), dedup `proxy_entries` by `source`, dedup `direct_rules`/`static_routes` by identity.

**Storage:** Dedup `fstab_entries` by identity, `mount_points` by `target`, `lvm_info`/`var_directories`/`credential_refs` by identity. No types have fleet/include.

**Scheduled Tasks:** merge `cron_jobs` via `merge_items`, dedup `systemd_timers`/`at_jobs` by identity, dedup `generated_timer_units` by `unit_name`.

**SELinux:** merge `port_labels` via `merge_items`, dedup `custom_modules`/`fcontext_rules` (string lists), dedup `boolean_overrides` (JSON equality), dedup `audit_rules`/`pam_configs` (`CarryForwardFile`) by `path`. Most-prevalent for `mode`/`fips_mode`.

**KernelBoot:** merge `kernel_modules`/`sysctl_overrides` via `merge_items`, dedup `modules_load_d`/`modprobe_d` (`ConfigSnippet`) by `path`, dedup `alternatives` by `name`. Most-prevalent for `cmdline`/`grub_defaults`.

**NonRpm:** merge `items` via `merge_items` (no variants despite having content — deferred per spec), merge `env_files` (these are `ConfigFileEntry` — has variants).

**UsersGroups:** JSON-level dedup. `users` and `groups` are `Vec<serde_json::Value>`. Extract `"name"` from each JSON object, dedup by name, union membership lists. Dedup `sudoers_rules`, `passwd_entries`, `shadow_entries`, `group_entries`, `gshadow_entries`, `subuid_entries`, `subgid_entries` as string lists. Dedup `ssh_authorized_keys_refs` by JSON equality.

- [ ] **Step 4: Write at least one test per adapter**

Focus on: correct prevalence calculation, deterministic ordering, string list dedup, variant handling where applicable.

- [ ] **Step 5: Run tests, commit**

```bash
git add inspectah-core/src/fleet/merge.rs inspectah-core/tests/fleet_merge_test.rs
git commit -m "feat(core): fleet section adapters for all round-1 sections"
```

---

## Task 10: merge_snapshots() Orchestrator

**Files:**
- Modify: `inspectah-core/src/fleet/mod.rs`
- Test: `inspectah-core/tests/fleet_merge_test.rs`

- [ ] **Step 1: Implement merge_snapshots()**

```rust
pub fn merge_snapshots(
    snapshots: Vec<InspectionSnapshot>,
    manifest: Option<&FleetManifest>,
) -> Result<(InspectionSnapshot, Vec<FleetWarning>), Vec<FleetValidationError>> {
    let validation = validate::validate_snapshots(&snapshots);
    if !validation.errors.is_empty() {
        return Err(validation.errors);
    }

    let total = snapshots.len();
    let hostnames = extract_sorted_hostnames(&snapshots);
    let section_host_counts = compute_section_host_counts(&snapshots);

    // Merge each section
    let rpm = merge::merge_rpm_section(/* ... */);
    let config = merge::merge_config_section(/* ... */);
    // ... all sections

    // Snapshot-level field merging per spec
    let target_image = select_target_image(&snapshots, manifest);
    let baseline = select_baseline(&snapshots, &target_image);
    let completeness = merge_completeness(&snapshots);

    let fleet_meta = FleetSnapshotMeta {
        label: manifest.and_then(|m| m.label.clone())
            .unwrap_or_else(|| "fleet".into()),
        host_count: total,
        hostnames: hostnames.clone(),
        merged_at: chrono::Utc::now().to_rfc3339(),
        baseline_provisional: /* true if baseline was auto-selected from conflicts */,
        section_host_counts,
    };

    let mut merged = InspectionSnapshot::new();
    merged.schema_version = SCHEMA_VERSION;
    merged.fleet_meta = Some(fleet_meta);
    merged.target_image = target_image;
    merged.baseline = baseline;
    merged.no_baseline = merged.baseline.is_none();
    merged.completeness = completeness;
    merged.redaction_state = None;
    merged.sensitive_snapshot = snapshots.iter().any(|s| s.sensitive_snapshot);
    merged.preserved_credentials = snapshots.iter().any(|s| s.preserved_credentials);
    merged.preserved_ssh_keys = snapshots.iter().any(|s| s.preserved_ssh_keys);
    merged.os_release = snapshots.iter()
        .min_by_key(|s| extract_hostname(s))
        .and_then(|s| s.os_release.clone());
    // ... set all section fields, warnings, etc.

    Ok((merged, validation.warnings))
}
```

- [ ] **Step 2: Implement `select_target_image`**

When manifest provides a baseline override, construct `TargetImageIdentity` with `strategy: ResolutionStrategy::CliOverride` and the override ref as `image_ref`. Otherwise, find the most-common `target_image` across inputs. Ties broken by lexicographic `image_ref`.

- [ ] **Step 3: Implement `select_baseline`**

Find the input whose `target_image` matches the selected one. Use the first match sorted by hostname. Copy its `baseline` data. Derive `baseline_package_names` and `baseline_suppressed` from this selected baseline only.

- [ ] **Step 4: Implement `merge_completeness`**

```rust
fn merge_completeness(snapshots: &[InspectionSnapshot]) -> Completeness {
    // If all Complete → Complete
    // If any Incomplete → Incomplete (union failed + degraded sections)
    // If any Partial → Partial (union degraded sections)
}
```

- [ ] **Step 5: Write integration tests**

Test basic merge, snapshot-level field selection, completeness merge, baseline provisionality.

- [ ] **Step 6: Run tests, commit**

```bash
git add inspectah-core/src/fleet/mod.rs inspectah-core/tests/fleet_merge_test.rs
git commit -m "feat(core): merge_snapshots() orchestrator with snapshot-level field merging"
```

---

## Task 11: Fleet CLI Command

**Files:**
- Create: `inspectah-cli/src/commands/fleet.rs`
- Modify: `inspectah-cli/src/commands/mod.rs`
- Modify: `inspectah-cli/src/main.rs`

The current CLI uses a flat `Commands` enum in `main.rs`:
```rust
enum Commands {
    Scan(ScanArgs),
    Refine(RefineArgs),
    Version,
}
```

Fleet needs a top-level `Fleet` variant with its own subcommands:

- [ ] **Step 1: Define the Fleet subcommand tree**

In `inspectah-cli/src/commands/fleet.rs`:

```rust
use clap::{Args, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Args)]
pub struct FleetArgs {
    #[command(subcommand)]
    pub command: FleetSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum FleetSubcommand {
    /// Aggregate host tarballs into a fleet tarball
    Aggregate(FleetAggregateArgs),
    /// Generate a fleet manifest from a directory of tarballs
    Init(FleetInitArgs),
}

#[derive(Debug, Args)]
pub struct FleetAggregateArgs {
    /// Input tarballs or directory
    pub inputs: Vec<PathBuf>,
    #[arg(long)]
    pub manifest: Option<PathBuf>,
    #[arg(long)]
    pub baseline: Option<String>,
    #[arg(long)]
    pub output_dir: Option<PathBuf>,
    #[arg(long)]
    pub output_file: Option<PathBuf>,
    #[arg(long)]
    pub json_only: bool,
    #[arg(long)]
    pub strict: bool,
    #[arg(long, short)]
    pub verbose: bool,
}

#[derive(Debug, Args)]
pub struct FleetInitArgs {
    pub directory: PathBuf,
    #[arg(long)]
    pub output: Option<PathBuf>,
    #[arg(long)]
    pub overwrite: bool,
}
```

- [ ] **Step 2: Register in command tree**

In `inspectah-cli/src/commands/mod.rs`:
```rust
pub mod fleet;
```

In `inspectah-cli/src/main.rs`, add to `Commands`:
```rust
Fleet(commands::fleet::FleetArgs),
```

And in the match:
```rust
Commands::Fleet(args) => commands::fleet::run_fleet(args),
```

- [ ] **Step 3: Implement input resolution**

In the aggregate handler, resolve inputs:
- If `--manifest` is set and `inputs` is non-empty → error
- If `--manifest` is set → parse manifest, resolve paths
- If single input is a directory → list `.tar.gz` files in it, label defaults to dir name
- If multiple inputs → use as tarball paths, label defaults to `"fleet"`

During tarball loading, collect `UnparseableFile` warnings for files that fail to load — these are CLI-layer warnings, not core validation warnings.

- [ ] **Step 4: Implement --strict promotion**

After `merge_snapshots()` returns, merge CLI-layer warnings (unparseable files) with core warnings. If `--strict`, treat any warning as an error:

```rust
let all_warnings = [loader_warnings, merge_warnings].concat();
if args.strict && !all_warnings.is_empty() {
    for w in &all_warnings {
        eprintln!("error (--strict): {w}");
    }
    anyhow::bail!("{} warning(s) promoted to errors by --strict", all_warnings.len());
}
```

- [ ] **Step 5: Implement render + tarball packaging**

Follow the scan command's pattern:
1. Save `inspection-snapshot.json` to temp dir
2. Call `render_all()` with the merged snapshot
3. Write `fleet/variants/` (Task 13)
4. Prepend Containerfile header (Task 13)
5. Package into `.tar.gz`

- [ ] **Step 6: Implement output formatting**

Default (to stderr):
```
Fleet: {label} ({N} hosts)
Merged: {pkg_count} packages, {config_count} config files, {svc_count} services
Output: {tarball_path}
```

Warnings above summary. `--json-only`: JSON to stdout or file per behavior table (warnings to stderr always). `--verbose`: add per-host counts.

- [ ] **Step 7: Build and verify CLI help**

Run:
```bash
cargo build -p inspectah-cli
./target/debug/inspectah fleet --help
./target/debug/inspectah fleet aggregate --help
./target/debug/inspectah fleet init --help
```

- [ ] **Step 8: Commit**

```bash
git add inspectah-cli/src/commands/fleet.rs inspectah-cli/src/commands/mod.rs inspectah-cli/src/main.rs
git commit -m "feat(cli): inspectah fleet command with aggregate and init subcommands"
```

---

## Task 12: Fleet Init Command

**Files:**
- Modify: `inspectah-cli/src/commands/fleet.rs`
- Modify: `inspectah-cli/Cargo.toml` (add `pathdiff`)

- [ ] **Step 1: Add pathdiff dependency**

Add to `inspectah-cli/Cargo.toml`:
```toml
pathdiff = "0.2"
```

- [ ] **Step 2: Implement tarball scanning**

Scan directory for `.tar.gz` files. For each, extract `inspection-snapshot.json`, parse just `meta` (hostname) and `target_image` fields. Use `inspectah-refine`'s tarball extraction helpers or re-implement minimally.

- [ ] **Step 3: Implement manifest generation**

Generate commented TOML with source paths relative to the manifest file's parent directory using `pathdiff::diff_paths()`.

- [ ] **Step 4: Implement init command handler**

- Default output: `fleet.toml` in cwd
- Refuse if exists (unless `--overwrite`)
- `--output` overrides path
- Baseline conflicts on stderr, most-common image written
- Summary on stderr: `wrote fleet.toml (N sources, baseline: ...)`

- [ ] **Step 5: Commit**

```bash
git add inspectah-cli/src/commands/fleet.rs inspectah-cli/Cargo.toml
git commit -m "feat(cli): inspectah fleet init command for manifest generation"
```

---

## Task 13: Fleet Variant Files + Containerfile Header

**Files:**
- Modify: `inspectah-cli/src/commands/fleet.rs`

- [ ] **Step 1: Implement variant file extraction**

Walk all variant-capable sections in the merged snapshot. For each item with `VariantSelection::Alternative`, write its content to `fleet/variants/{path}/{8-char-sha256-hash}.{ext}`.

For `ComposeFile` (no `content` field), serialize `images` to JSON and write that as the variant file.

- [ ] **Step 2: Implement Containerfile header prepending**

Read the rendered Containerfile, prepend the draft header (with provisionality note when `baseline_provisional` is true), write back.

- [ ] **Step 3: Wire into aggregate handler between render_all() and tarball packaging**

- [ ] **Step 4: Commit**

```bash
git add inspectah-cli/src/commands/fleet.rs
git commit -m "feat(cli): fleet variant file writing and Containerfile draft header"
```

---

## Task 14: Fleet-Aware Audit Report

**Files:**
- Modify: `inspectah-pipeline/src/render/audit.rs`

- [ ] **Step 1: Add fleet summary section to audit renderer**

When `snap.fleet_meta.is_some()`, add a "Fleet Aggregate Summary" section:
- Host count and hostname list
- Baseline selection method and provisionality
- Section coverage from `section_host_counts`
- Variant conflicts (count of paths with multiple content versions)

- [ ] **Step 2: Run audit tests**

Run: `cargo test -p inspectah-pipeline -- audit`

- [ ] **Step 3: Commit**

```bash
git add inspectah-pipeline/src/render/audit.rs
git commit -m "feat(render): fleet summary section in audit report"
```

---

## Task 15: End-to-End Integration Tests

**Files:**
- Create: `inspectah-core/tests/fleet_e2e_test.rs`

- [ ] **Step 1: Build rich test snapshot helpers**

Create helpers that build `InspectionSnapshot` with multiple populated sections.

- [ ] **Step 2: Write e2e tests**

- 3 hosts, shared packages, config variants → correct prevalence, variant selection, fleet_meta
- Validation hard errors (mixed arch, duplicate hostname, OS major mismatch)
- Missing sections use global denominator
- Baseline selection with provisionality
- Deterministic output (reversed input order → same result except `merged_at`)

- [ ] **Step 3: Run, commit**

```bash
git add inspectah-core/tests/fleet_e2e_test.rs
git commit -m "test(core): fleet aggregate end-to-end integration tests"
```

---

## Task 16: Final Cleanup

- [ ] **Step 1: Run clippy**

Run: `cargo clippy --workspace -- -W clippy::all`
Expected: zero warnings

- [ ] **Step 2: Run fmt**

Run: `cargo fmt --all --check`

- [ ] **Step 3: Run full test suite**

Run: `cargo test --workspace`

- [ ] **Step 4: Verify CLI**

```bash
cargo build -p inspectah-cli
./target/debug/inspectah fleet --help
./target/debug/inspectah fleet init --help
```

- [ ] **Step 5: Commit any remaining cleanup**

```bash
git add inspectah-core/ inspectah-cli/ inspectah-pipeline/
git commit -m "chore: fleet aggregate final cleanup"
```
