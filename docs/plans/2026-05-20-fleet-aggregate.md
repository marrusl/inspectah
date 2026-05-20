# Fleet Aggregate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the `inspectah fleet` and `inspectah fleet init` commands that aggregate N single-host tarballs into a fleet tarball with prevalence metadata.

**Architecture:** Hybrid merge engine (generic `FleetMergeable` trait + thin per-section adapters) in `inspectah-core`. CLI commands in `inspectah-cli`. Fleet tarball inherits the scan tarball contract, adding `fleet/variants/` for content variant storage. The `VariantSelection` enum replaces `tie`/`tie_winner` bools. `FleetSnapshotMeta` on `InspectionSnapshot` carries fleet-level metadata.

**Tech Stack:** Rust (2024 edition), serde, clap derive, toml, sha2, insta (snapshot tests), tar + flate2 (tarball packaging).

**Spec:** `docs/specs/proposed/2026-05-19-fleet-aggregate-spec.md`

**Repo:** `/Users/mrussell/Work/bootc-migration/inspectah/` (branch: `rust`)

---

## File Map

### New files

| File | Responsibility |
|------|---------------|
| `inspectah-core/src/fleet/mod.rs` | `merge_snapshots()` orchestrator, public API |
| `inspectah-core/src/fleet/merge.rs` | Generic merge function + section adapters |
| `inspectah-core/src/fleet/validate.rs` | Pre-merge validation (hard errors + warnings) |
| `inspectah-core/src/fleet/manifest.rs` | TOML manifest parsing (`FleetManifest`) |
| `inspectah-core/tests/fleet_merge_test.rs` | Integration tests for merge engine |
| `inspectah-core/tests/fleet_validate_test.rs` | Integration tests for validation |
| `inspectah-cli/src/commands/fleet.rs` | `fleet` + `fleet init` CLI commands |

### Modified files

| File | Changes |
|------|---------|
| `inspectah-core/src/types/fleet.rs` | Add `VariantSelection` enum, `FleetSnapshotMeta` struct |
| `inspectah-core/src/types/config.rs` | Replace `tie`/`tie_winner` with `variant_selection: VariantSelection` |
| `inspectah-core/src/types/services.rs` | Replace `tie`/`tie_winner` on `SystemdDropIn` |
| `inspectah-core/src/types/containers.rs` | Replace `tie`/`tie_winner` on `QuadletUnit`, `ComposeFile` |
| `inspectah-core/src/types/rpm.rs` | Replace `tie`/`tie_winner` on `RepoFile` |
| `inspectah-core/src/types/kernelboot.rs` | Add `fleet` field to `KernelModule`, `SysctlOverride` |
| `inspectah-core/src/types/nonrpm.rs` | Add `fleet` field to `NonRpmItem` |
| `inspectah-core/src/snapshot.rs` | Add `fleet_meta` field, bump `SCHEMA_VERSION`, add migration |
| `inspectah-core/src/lib.rs` | Register `fleet` module |
| `inspectah-core/Cargo.toml` | Add `sha2`, `toml` dependencies |
| `inspectah-cli/src/commands/mod.rs` | Register `fleet` subcommand |
| `inspectah-cli/src/main.rs` | Wire fleet command |

---

## Task 1: VariantSelection Enum

**Files:**
- Modify: `inspectah-core/src/types/fleet.rs`
- Test: inline `#[cfg(test)]` module

This adds the `VariantSelection` enum alongside the existing `FleetPrevalence` and `FleetMeta` types. No migration yet — just the new type.

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

#[test]
fn test_variant_selection_copy() {
    let a = VariantSelection::Selected;
    let b = a; // Copy
    assert_eq!(a, b);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-core -- test_variant_selection`
Expected: FAIL — `VariantSelection` not defined

- [ ] **Step 3: Implement VariantSelection enum**

Add to `inspectah-core/src/types/fleet.rs` before the existing `FleetPrevalence`:

```rust
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum VariantSelection {
    #[default]
    Only,
    Selected,
    Alternative,
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p inspectah-core -- test_variant_selection`
Expected: PASS (3 tests)

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
- Modify: `inspectah-core/src/types/rpm.rs` (RepoFile)
- Modify: `inspectah-core/src/snapshot.rs` (schema migration)

This is a schema-breaking change. Replace `tie: bool` + `tie_winner: bool` with `variant_selection: VariantSelection` on all content-variant types. Add schema migration for backward compat.

- [ ] **Step 1: Search for all tie/tie_winner usage across the codebase**

Run: `grep -rn 'tie_winner\|\.tie\b' --include='*.rs' | grep -v target/ | grep -v test`

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

Add the import: `use super::fleet::VariantSelection;`

- [ ] **Step 3: Replace fields on SystemdDropIn, QuadletUnit, ComposeFile, RepoFile**

Same replacement on each struct. Each file needs the `VariantSelection` import.

- `inspectah-core/src/types/services.rs` — `SystemdDropIn`
- `inspectah-core/src/types/containers.rs` — `QuadletUnit`, `ComposeFile`
- `inspectah-core/src/types/rpm.rs` — `RepoFile`

- [ ] **Step 4: Add schema migration in snapshot.rs**

In the `migrate()` function in `inspectah-core/src/snapshot.rs`, add migration logic for the previous schema version that maps `tie`/`tie_winner` to `VariantSelection`. Since this is a JSON-level migration (the old format has bool fields, the new format has an enum field), handle it in the raw JSON patching step or via a serde migration path.

The migration maps:
- `tie_winner: true` → `"variant_selection": "Selected"`
- `tie: true, tie_winner: false` → `"variant_selection": "Alternative"`
- neither set → `"variant_selection": "Only"` (default)

- [ ] **Step 5: Fix all compilation errors**

Run: `cargo build -p inspectah-core 2>&1 | head -50`

Fix every reference to `.tie` or `.tie_winner` across all crates. These will be in:
- `inspectah-pipeline/src/render/` (Containerfile renderer, config tree)
- `inspectah-refine/src/` (session.rs, handlers)
- `inspectah-web/src/` (handlers.rs)
- Tests throughout

Replace patterns:
- `entry.tie_winner` → `entry.variant_selection == VariantSelection::Selected`
- `entry.tie && !entry.tie_winner` → `entry.variant_selection == VariantSelection::Alternative`
- `entry.tie = true; entry.tie_winner = true;` → `entry.variant_selection = VariantSelection::Selected;`
- `entry.tie = true; entry.tie_winner = false;` → `entry.variant_selection = VariantSelection::Alternative;`

- [ ] **Step 6: Run full test suite**

Run: `cargo test --workspace`
Expected: PASS (all existing tests should work with the new field)

- [ ] **Step 7: Update golden files / snapshot tests if needed**

Run: `cargo insta review` if any snapshot tests have changed output.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "refactor(core): replace tie/tie_winner bools with VariantSelection enum

Schema-breaking change. Eliminates invalid state combinations
(4 bool states → 3 enum variants). Migration maps legacy
tie/tie_winner JSON to VariantSelection values."
```

---

## Task 3: FleetSnapshotMeta + fleet_meta on InspectionSnapshot

**Files:**
- Modify: `inspectah-core/src/types/fleet.rs`
- Modify: `inspectah-core/src/snapshot.rs`
- Modify: `inspectah-core/Cargo.toml` (add `toml` dependency if not present)

- [ ] **Step 1: Write failing test for FleetSnapshotMeta serde roundtrip**

Add to `inspectah-core/src/types/fleet.rs` tests:

```rust
#[test]
fn test_fleet_snapshot_meta_roundtrip() {
    let meta = FleetSnapshotMeta {
        label: "web-servers".into(),
        host_count: 50,
        hostnames: vec!["host-a".into(), "host-b".into()],
        merged_at: "2026-05-20T12:00:00Z".into(),
        baseline_provisional: true,
        section_host_counts: BTreeMap::from([
            ("config".into(), 48),
            ("rpm".into(), 50),
        ]),
    };
    let json = serde_json::to_string(&meta).unwrap();
    let parsed: FleetSnapshotMeta = serde_json::from_str(&json).unwrap();
    assert_eq!(meta, parsed);
}
```

- [ ] **Step 2: Implement FleetSnapshotMeta**

Add to `inspectah-core/src/types/fleet.rs`:

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

- [ ] **Step 3: Run FleetSnapshotMeta test**

Run: `cargo test -p inspectah-core -- test_fleet_snapshot_meta`
Expected: PASS

- [ ] **Step 4: Add fleet_meta to InspectionSnapshot**

In `inspectah-core/src/snapshot.rs`, add to the struct:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub fleet_meta: Option<crate::types::fleet::FleetSnapshotMeta>,
```

Bump `SCHEMA_VERSION` by 1.

Update the `InspectionSnapshot::new()` constructor to include `fleet_meta: None`.

- [ ] **Step 5: Write test for fleet_meta on snapshot**

```rust
#[test]
fn test_snapshot_with_fleet_meta_roundtrip() {
    let mut snap = InspectionSnapshot::new();
    snap.fleet_meta = Some(FleetSnapshotMeta {
        label: "test".into(),
        host_count: 5,
        hostnames: vec!["a".into(), "b".into()],
        merged_at: "2026-05-20T00:00:00Z".into(),
        baseline_provisional: false,
        section_host_counts: BTreeMap::new(),
    });
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

- [ ] **Step 6: Run tests, fix any schema version gate issues**

Run: `cargo test --workspace`

The schema version bump may break `load_for_refine()` if it has a hardcoded version range. Update `MIN_SCHEMA` or the range check as needed.

- [ ] **Step 7: Commit**

```bash
git add inspectah-core/src/types/fleet.rs inspectah-core/src/snapshot.rs
git commit -m "feat(core): add FleetSnapshotMeta and fleet_meta field on InspectionSnapshot

Bumps SCHEMA_VERSION. fleet_meta is None on single-host snapshots
and omitted from serialized JSON via skip_serializing_if."
```

---

## Task 4: Add fleet field to types that don't have it

**Files:**
- Modify: `inspectah-core/src/types/kernelboot.rs`
- Modify: `inspectah-core/src/types/nonrpm.rs`

Add `pub fleet: Option<FleetPrevalence>` to `KernelModule`, `SysctlOverride`, and `NonRpmItem`. These types currently have `include: bool` but no fleet tracking.

- [ ] **Step 1: Add fleet field to KernelModule and SysctlOverride**

In `inspectah-core/src/types/kernelboot.rs`:

```rust
use super::fleet::FleetPrevalence;
```

Add to both `KernelModule` and `SysctlOverride`:
```rust
    pub fleet: Option<FleetPrevalence>,
```

- [ ] **Step 2: Add fleet field to NonRpmItem**

In `inspectah-core/src/types/nonrpm.rs`:

```rust
use super::fleet::FleetPrevalence;
```

Add to `NonRpmItem`:
```rust
    pub fleet: Option<FleetPrevalence>,
```

- [ ] **Step 3: Run workspace tests**

Run: `cargo test --workspace`
Expected: PASS (new fields are `Option` with serde default)

- [ ] **Step 4: Commit**

```bash
git add inspectah-core/src/types/kernelboot.rs inspectah-core/src/types/nonrpm.rs
git commit -m "feat(core): add fleet prevalence field to KernelModule, SysctlOverride, NonRpmItem"
```

---

## Task 5: FleetMergeable Trait + Implementations

**Files:**
- Create: `inspectah-core/src/fleet/mod.rs`
- Create: `inspectah-core/src/fleet/merge.rs`
- Modify: `inspectah-core/src/lib.rs`
- Test: `inspectah-core/tests/fleet_merge_test.rs`

Define the `FleetMergeable` trait and implement it for all prevalence-tracked types from the coverage table.

- [ ] **Step 1: Create fleet module skeleton**

Create `inspectah-core/src/fleet/mod.rs`:

```rust
pub mod merge;
```

Register in `inspectah-core/src/lib.rs`:
```rust
pub mod fleet;
```

Create `inspectah-core/src/fleet/merge.rs` with the trait:

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

- [ ] **Step 2: Write failing test for PackageEntry identity key**

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

- [ ] **Step 3: Implement FleetMergeable for PackageEntry**

In `inspectah-core/src/fleet/merge.rs`:

```rust
use crate::types::rpm::PackageEntry;

impl FleetMergeable for PackageEntry {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Owned(format!("{}.{}", self.name, self.arch))
    }
    fn fleet_mut(&mut self) -> &mut Option<FleetPrevalence> { &mut self.fleet }
    fn set_include(&mut self, val: bool) { self.include = val; }
}
```

- [ ] **Step 4: Run test**

Run: `cargo test -p inspectah-core --test fleet_merge_test`
Expected: PASS

- [ ] **Step 5: Implement FleetMergeable for all remaining prevalence-tracked types**

Implement for each type per the coverage table. Identity keys:

| Type | Identity key expression |
|------|----------------------|
| `PackageEntry` | `format!("{}.{}", self.name, self.arch)` |
| `RepoFile` | `Cow::Borrowed(&self.path)` |
| `ConfigFileEntry` | `Cow::Borrowed(&self.path)` |
| `ServiceStateChange` | `Cow::Borrowed(&self.unit)` |
| `SystemdDropIn` | `Cow::Borrowed(&self.path)` |
| `QuadletUnit` | `Cow::Borrowed(&self.path)` |
| `ComposeFile` | `Cow::Borrowed(&self.path)` |
| `SelinuxPortLabel` | `format!("{}:{}", self.protocol, self.port)` |
| `KernelModule` | `Cow::Borrowed(&self.name)` |
| `SysctlOverride` | `Cow::Borrowed(&self.key)` |
| `NonRpmItem` | `Cow::Borrowed(&self.name)` |

For types with content variants (RepoFile, ConfigFileEntry, SystemdDropIn, QuadletUnit, ComposeFile), also implement `content_variant_key()` returning a SHA-256 hash of the `content` field, and `variant_selection_mut()` returning `Some(&mut self.variant_selection)`.

Use `sha2` crate for the content hash. Add `sha2 = "0.10"` to `inspectah-core/Cargo.toml`.

- [ ] **Step 6: Write tests for variant-capable types**

```rust
#[test]
fn test_config_file_entry_has_content_variant_key() {
    let entry = ConfigFileEntry {
        path: "/etc/foo.conf".into(),
        content: "setting=value".into(),
        ..Default::default()
    };
    let key = entry.content_variant_key();
    assert!(key.is_some());
    // Same content → same key
    let entry2 = ConfigFileEntry {
        path: "/etc/foo.conf".into(),
        content: "setting=value".into(),
        ..Default::default()
    };
    assert_eq!(key, entry2.content_variant_key());
}

#[test]
fn test_package_entry_has_no_content_variant_key() {
    let pkg = PackageEntry::default();
    assert!(pkg.content_variant_key().is_none());
}
```

- [ ] **Step 7: Run all fleet tests**

Run: `cargo test -p inspectah-core --test fleet_merge_test`
Expected: PASS

- [ ] **Step 8: Commit**

```bash
git add inspectah-core/src/fleet/ inspectah-core/src/lib.rs inspectah-core/Cargo.toml inspectah-core/tests/fleet_merge_test.rs
git commit -m "feat(core): FleetMergeable trait with impls for all prevalence-tracked types"
```

---

## Task 6: Generic Merge Function

**Files:**
- Modify: `inspectah-core/src/fleet/merge.rs`
- Test: `inspectah-core/tests/fleet_merge_test.rs`

The core merge function: takes items from N snapshots, groups by identity, computes prevalence, handles variants.

- [ ] **Step 1: Write failing test for basic prevalence merge**

```rust
#[test]
fn test_merge_items_prevalence_two_hosts_same_package() {
    let items: Vec<(usize, PackageEntry)> = vec![
        (0, PackageEntry { name: "httpd".into(), arch: "x86_64".into(), include: false, ..Default::default() }),
        (1, PackageEntry { name: "httpd".into(), arch: "x86_64".into(), include: false, ..Default::default() }),
    ];
    let hostnames = vec!["host-a".to_string(), "host-b".to_string()];
    let merged = merge_items(items, 2, &hostnames);
    assert_eq!(merged.len(), 1);
    let pkg = &merged[0];
    let fleet = pkg.fleet.as_ref().unwrap();
    assert_eq!(fleet.count, 2);
    assert_eq!(fleet.total, 2);
    assert!(pkg.include); // always true after aggregate
}
```

- [ ] **Step 2: Implement `merge_items<T: FleetMergeable>`**

In `inspectah-core/src/fleet/merge.rs`:

```rust
use std::collections::HashMap;

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
        // Sort group entries by host_idx for determinism
        group.sort_by_key(|(idx, _)| *idx);

        let hosts: Vec<String> = group.iter()
            .map(|(idx, _)| hostnames[*idx].clone())
            .collect();
        let count = hosts.len() as i32;

        // Check if this type has content variants
        let has_variants = group[0].1.content_variant_key().is_some();

        if has_variants {
            result.extend(merge_with_variants(group, total_hosts, &hosts));
        } else {
            // Take the most-prevalent representative (first by host order)
            let mut representative = group[0].1.clone();
            *representative.fleet_mut() = Some(FleetPrevalence {
                count,
                total: total_hosts as i32,
                hosts: {
                    let mut h = hosts;
                    h.sort();
                    h
                },
            });
            representative.set_include(true);
            result.push(representative);
        }
    }

    // Sort by identity key for deterministic output
    result.sort_by(|a, b| a.identity_key().cmp(&b.identity_key()));
    result
}
```

- [ ] **Step 3: Run test**

Run: `cargo test -p inspectah-core --test fleet_merge_test -- test_merge_items_prevalence`
Expected: PASS

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

    // Two variants: version_a (2 hosts) and version_b (1 host)
    assert_eq!(merged.len(), 2);
    let selected = merged.iter().find(|e| e.variant_selection == VariantSelection::Selected).unwrap();
    let alternative = merged.iter().find(|e| e.variant_selection == VariantSelection::Alternative).unwrap();

    assert_eq!(selected.content, "version_a"); // most prevalent
    assert_eq!(alternative.content, "version_b");
    assert_eq!(selected.fleet.as_ref().unwrap().count, 2);
    assert_eq!(alternative.fleet.as_ref().unwrap().count, 1);
}
```

- [ ] **Step 5: Implement `merge_with_variants`**

This is the content-variant merge path. Group by identity key, then subgroup by content hash. Most prevalent subgroup's representative becomes `Selected`, others become `Alternative`. Ties broken by lexicographic content hash.

```rust
fn merge_with_variants<T: FleetMergeable>(
    group: &mut [(usize, T)],
    total_hosts: usize,
    all_hosts: &[String],
) -> Vec<T> {
    use sha2::{Sha256, Digest};

    // Subgroup by content hash
    let mut subgroups: HashMap<String, Vec<(usize, &T)>> = HashMap::new();
    for (idx, item) in group.iter() {
        let hash = item.content_variant_key().unwrap().into_owned();
        subgroups.entry(hash).or_default().push((*idx, item));
    }

    // Find the winner: most hosts, tie-break by hash
    let mut ranked: Vec<(String, Vec<(usize, &T)>)> = subgroups.into_iter().collect();
    ranked.sort_by(|(hash_a, hosts_a), (hash_b, hosts_b)| {
        hosts_b.len().cmp(&hosts_a.len())
            .then_with(|| hash_a.cmp(hash_b))
    });

    let mut result = Vec::new();
    for (i, (content_hash, subgroup)) in ranked.iter().enumerate() {
        let hosts: Vec<String> = subgroup.iter()
            .map(|(idx, _)| all_hosts[*idx].clone())
            .collect();

        let mut item = subgroup[0].1.clone();
        *item.fleet_mut() = Some(FleetPrevalence {
            count: hosts.len() as i32,
            total: total_hosts as i32,
            hosts: { let mut h = hosts; h.sort(); h },
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

- [ ] **Step 6: Run variant test**

Run: `cargo test -p inspectah-core --test fleet_merge_test -- test_merge_items_variant`
Expected: PASS

- [ ] **Step 7: Write test for deterministic tie-breaking**

```rust
#[test]
fn test_merge_items_variant_tie_breaks_by_hash() {
    // Two content variants with equal host count — winner determined by hash
    let items: Vec<(usize, ConfigFileEntry)> = vec![
        (0, ConfigFileEntry { path: "/etc/foo.conf".into(), content: "aaa".into(), ..Default::default() }),
        (1, ConfigFileEntry { path: "/etc/foo.conf".into(), content: "zzz".into(), ..Default::default() }),
    ];
    let hostnames = vec!["h1".into(), "h2".into()];
    let merged = merge_items(items.clone(), 2, &hostnames);

    let selected = merged.iter().find(|e| e.variant_selection == VariantSelection::Selected).unwrap();
    // Run again with reversed input order — should pick the same winner
    let items_rev: Vec<(usize, ConfigFileEntry)> = vec![
        (0, ConfigFileEntry { path: "/etc/foo.conf".into(), content: "zzz".into(), ..Default::default() }),
        (1, ConfigFileEntry { path: "/etc/foo.conf".into(), content: "aaa".into(), ..Default::default() }),
    ];
    let merged_rev = merge_items(items_rev, 2, &hostnames);
    let selected_rev = merged_rev.iter().find(|e| e.variant_selection == VariantSelection::Selected).unwrap();

    assert_eq!(selected.content, selected_rev.content);
}
```

- [ ] **Step 8: Run all merge tests**

Run: `cargo test -p inspectah-core --test fleet_merge_test`
Expected: PASS

- [ ] **Step 9: Commit**

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

- [ ] **Step 1: Add `toml` dependency**

Add to `inspectah-core/Cargo.toml` under `[dependencies]`:
```toml
toml = "0.8"
```

- [ ] **Step 2: Write failing test for manifest parsing**

In `inspectah-core/src/fleet/manifest.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_basic_manifest() {
        let toml = r#"
            label = "web-servers"
            baseline = "registry.redhat.io/rhel9/rhel-bootc:9.6"
            sources = ["host1.tar.gz", "host2.tar.gz"]
        "#;
        let manifest = FleetManifest::parse(toml).unwrap();
        assert_eq!(manifest.label.as_deref(), Some("web-servers"));
        assert_eq!(manifest.baseline.as_deref(), Some("registry.redhat.io/rhel9/rhel-bootc:9.6"));
        assert_eq!(manifest.sources.len(), 2);
    }

    #[test]
    fn test_parse_minimal_manifest() {
        let toml = r#"sources = ["host1.tar.gz"]"#;
        let manifest = FleetManifest::parse(toml).unwrap();
        assert!(manifest.label.is_none());
        assert!(manifest.baseline.is_none());
        assert_eq!(manifest.sources.len(), 1);
    }

    #[test]
    fn test_parse_empty_sources_is_error() {
        let toml = r#"sources = []"#;
        assert!(FleetManifest::parse(toml).is_err());
    }
}
```

- [ ] **Step 3: Implement FleetManifest**

```rust
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
pub struct FleetManifest {
    pub label: Option<String>,
    pub baseline: Option<String>,
    pub sources: Vec<PathBuf>,
}

impl FleetManifest {
    pub fn parse(toml_str: &str) -> Result<Self, String> {
        let manifest: Self = toml::from_str(toml_str)
            .map_err(|e| format!("invalid fleet manifest: {e}"))?;
        if manifest.sources.is_empty() {
            return Err("manifest has no sources".into());
        }
        Ok(manifest)
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read manifest {}: {e}", path.display()))?;
        let mut manifest = Self::parse(&content)?;
        // Resolve sources relative to manifest parent dir
        if let Some(parent) = path.parent() {
            manifest.sources = manifest.sources.iter()
                .map(|s| parent.join(s))
                .collect();
        }
        Ok(manifest)
    }
}
```

- [ ] **Step 4: Register module and run tests**

Add `pub mod manifest;` to `inspectah-core/src/fleet/mod.rs`.

Run: `cargo test -p inspectah-core -- test_parse`
Expected: PASS

- [ ] **Step 5: Commit**

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

Implement all hard-error and warning checks from the spec's Validation section. Validation runs as a separate pass before merge, collecting all errors/warnings together.

- [ ] **Step 1: Define error and warning types**

In `inspectah-core/src/fleet/validate.rs`:

```rust
use crate::snapshot::InspectionSnapshot;

#[derive(Debug, Clone, PartialEq)]
pub enum FleetValidationError {
    SchemaVersionMismatch { versions: Vec<u32> },
    DuplicateHostname { hostname: String },
    ArchitectureMismatch { architectures: Vec<String> },
    EmptySnapshot { hostname: String },
    OsMajorVersionMismatch { versions: Vec<String> },
    TooFewSnapshots { count: usize },
}

#[derive(Debug, Clone, PartialEq)]
pub enum FleetWarning {
    StaleScanDates { oldest: String, newest: String, days_apart: u64 },
    UnparseableFile { path: String, error: String },
    BaselineConflict { distribution: Vec<(String, usize)> },
    MinorVersionSpread { versions: Vec<String> },
    SystemTypeMismatch { types: Vec<String> },
}

pub struct ValidationResult {
    pub errors: Vec<FleetValidationError>,
    pub warnings: Vec<FleetWarning>,
}

pub fn validate_snapshots(snapshots: &[InspectionSnapshot]) -> ValidationResult {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    // ... implement each check
    ValidationResult { errors, warnings }
}
```

- [ ] **Step 2: Write tests for each hard error**

Create `inspectah-core/tests/fleet_validate_test.rs` with tests for:
- Schema version mismatch across snapshots
- Duplicate hostnames
- Architecture mismatch (derive arch from packages or os_release)
- Empty/zero-package snapshot
- OS major version mismatch (RHEL 8 + RHEL 9)
- Minimum 2 snapshots

- [ ] **Step 3: Implement each validation check**

Each check iterates the snapshot list and collects violations:
- **Schema version:** compare `schema_version` across all inputs
- **Duplicate hostname:** extract from `meta["hostname"]`, check for duplicates
- **Architecture:** derive from os_release or package arches, check uniformity
- **Empty snapshot:** check `rpm.is_none()` or `rpm.packages_added.is_empty()`
- **OS major version:** parse from `os_release.version_id`, compare major component

- [ ] **Step 4: Write tests for each warning**

Test stale scan dates (>30 days apart), baseline conflicts (different `target_image` values), minor version spread, system type mismatch.

- [ ] **Step 5: Implement warning checks**

- **Stale scans:** compare `meta["scan_date"]` across inputs
- **Baseline conflicts:** compare `target_image` across inputs
- **Minor version:** parse minor from version_id, check spread
- **System type:** compare `system_type` across inputs

- [ ] **Step 6: Run all validation tests**

Run: `cargo test -p inspectah-core --test fleet_validate_test`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add inspectah-core/src/fleet/validate.rs inspectah-core/tests/fleet_validate_test.rs inspectah-core/src/fleet/mod.rs
git commit -m "feat(core): fleet pre-merge validation with hard errors and warnings"
```

---

## Task 9: Section Adapters

**Files:**
- Modify: `inspectah-core/src/fleet/merge.rs`
- Test: `inspectah-core/tests/fleet_merge_test.rs`

Implement thin adapter functions for each snapshot section. These extract fields, call `merge_items`, handle non-prevalence fields (string list dedup, pass-through), and reassemble section structs.

- [ ] **Step 1: Implement dedup helper for string lists**

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

- [ ] **Step 2: Implement most-prevalent-value helper**

```rust
pub(crate) fn most_prevalent_value<T: Clone + Eq + std::hash::Hash + Ord>(values: &[T]) -> T {
    let mut counts: HashMap<&T, usize> = HashMap::new();
    for v in values {
        *counts.entry(v).or_default() += 1;
    }
    let max_count = counts.values().max().copied().unwrap_or(0);
    let mut candidates: Vec<&&T> = counts.iter()
        .filter(|(_, &c)| c == max_count)
        .map(|(v, _)| v)
        .collect();
    candidates.sort();
    (**candidates[0]).clone()
}
```

- [ ] **Step 3: Implement RPM section adapter**

The RPM adapter is the most complex — it handles PackageEntry, RepoFile, ModuleStream, VersionChange, and several pass-through fields. Write the adapter as a private function that takes `Vec<Option<RpmSection>>` + host count + hostnames and returns `Option<RpmSection>`.

Key considerations:
- `baseline_package_names`: derive from the selected baseline only (the host whose target_image matches the merged target_image)
- `packages_added` and `base_image_only`: merge via `merge_items`
- `dnf_history_removed`: `dedup_strings`
- `version_changes`: dedup by `name.arch`, keep most common direction
- `leaf_packages` / `auto_packages` / `leaf_dep_tree`: pass through from the most prevalent host (these are per-host derived data; fleet recalculation is Spec 2)

- [ ] **Step 4: Write tests for RPM adapter**

Test with 3 host snapshots having overlapping packages, some shared configs, and different versions.

- [ ] **Step 5: Implement remaining section adapters**

Each adapter follows the same pattern:
- Config section: merge `files` (with variants), pass through section-level fields
- Services section: merge `state_changes` and `drop_ins` (variants), dedup `enabled_units`/`disabled_units`
- Network section: dedup `firewall_zones` by name, `nmconn_profiles` by filename, `proxy_entries` by env_var
- Storage section: dedup `mounts` by mountpoint, iscsi/nfs by identity
- Containers section: merge `quadlet_units` and `compose_files` (variants), skip `running_containers`
- SELinux section: merge `port_labels`, dedup string lists, most-prevalent for mode/fips_mode
- KernelBoot section: merge `sysctl_overrides` and `kernel_modules`, dedup snippets
- ScheduledTasks section: dedup by name/unit_name
- NonRpm section: merge `items` by name
- UsersGroups section: dedup users by name, groups by name, union membership lists

- [ ] **Step 6: Write at least one test per adapter**

Verify correct prevalence calculation, deduplication, and deterministic ordering for each section type.

- [ ] **Step 7: Run all tests**

Run: `cargo test -p inspectah-core --test fleet_merge_test`
Expected: PASS

- [ ] **Step 8: Commit**

```bash
git add inspectah-core/src/fleet/merge.rs inspectah-core/tests/fleet_merge_test.rs
git commit -m "feat(core): fleet section adapters for all round-1 sections"
```

---

## Task 10: merge_snapshots() Orchestrator

**Files:**
- Modify: `inspectah-core/src/fleet/mod.rs`
- Test: `inspectah-core/tests/fleet_merge_test.rs`

The top-level function that validates, merges all sections, populates snapshot-level fields, and returns the merged result.

- [ ] **Step 1: Write failing integration test**

```rust
#[test]
fn test_merge_snapshots_basic() {
    let snap1 = make_test_snapshot("host-a", vec![
        ("httpd", "x86_64", "2.4.51"),
        ("openssl", "x86_64", "3.0.7"),
    ]);
    let snap2 = make_test_snapshot("host-b", vec![
        ("httpd", "x86_64", "2.4.51"),
        ("vim", "x86_64", "9.0"),
    ]);
    let (merged, warnings) = merge_snapshots(vec![snap1, snap2], None).unwrap();

    assert!(merged.fleet_meta.is_some());
    let meta = merged.fleet_meta.as_ref().unwrap();
    assert_eq!(meta.host_count, 2);
    assert_eq!(meta.hostnames, vec!["host-a", "host-b"]);

    let rpm = merged.rpm.as_ref().unwrap();
    assert_eq!(rpm.packages_added.len(), 3); // httpd, openssl, vim
    let httpd = rpm.packages_added.iter().find(|p| p.name == "httpd").unwrap();
    assert_eq!(httpd.fleet.as_ref().unwrap().count, 2); // on both hosts
    let vim = rpm.packages_added.iter().find(|p| p.name == "vim").unwrap();
    assert_eq!(vim.fleet.as_ref().unwrap().count, 1); // on one host
}
```

Write a `make_test_snapshot` helper that builds an `InspectionSnapshot` with the given hostname and packages.

- [ ] **Step 2: Implement merge_snapshots()**

In `inspectah-core/src/fleet/mod.rs`:

```rust
pub fn merge_snapshots(
    snapshots: Vec<InspectionSnapshot>,
    manifest: Option<&FleetManifest>,
) -> Result<(InspectionSnapshot, Vec<FleetWarning>), Vec<FleetValidationError>> {
    // 1. Validate
    let validation = validate::validate_snapshots(&snapshots);
    if !validation.errors.is_empty() {
        return Err(validation.errors);
    }

    let total = snapshots.len();
    let hostnames = extract_sorted_hostnames(&snapshots);

    // 2. Merge each section via adapters
    // ... call each section adapter

    // 3. Populate snapshot-level fields per spec
    // target_image, baseline, completeness, etc.

    // 4. Build FleetSnapshotMeta
    let fleet_meta = FleetSnapshotMeta {
        label: derive_label(manifest, /* context */),
        host_count: total,
        hostnames: hostnames.clone(),
        merged_at: chrono::Utc::now().to_rfc3339(),
        baseline_provisional: /* ... */,
        section_host_counts: compute_section_host_counts(&snapshots),
    };

    // 5. Assemble merged snapshot
    let mut merged = InspectionSnapshot::new();
    merged.fleet_meta = Some(fleet_meta);
    // ... set all fields

    Ok((merged, validation.warnings))
}
```

- [ ] **Step 3: Implement snapshot-level field merging**

Per the spec's Existing Snapshot-Level Fields table:
- `target_image`: manifest baseline override, else most-common autodetected
- `baseline`: from the host matching selected target_image
- `no_baseline`: true if baseline is None after merge
- `completeness`: conservative merge of the Completeness enum
- `redaction_state`: None
- `sensitive_snapshot` / `preserved_credentials` / `preserved_ssh_keys`: OR across inputs
- `os_release`: from first host (sorted by hostname)
- `warnings`: deduplicated union + fleet-specific warnings
- `preflight`: default

- [ ] **Step 4: Write tests for snapshot-level field merging**

Test target_image selection, baseline truth, completeness merge (Complete + Partial → Partial), sensitive flag OR semantics.

- [ ] **Step 5: Run all tests**

Run: `cargo test -p inspectah-core --test fleet_merge_test`
Expected: PASS

- [ ] **Step 6: Commit**

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
- Modify: `inspectah-cli/Cargo.toml`

Implement the `inspectah fleet` command with all input modes and flags.

- [ ] **Step 1: Define the clap subcommand**

Create `inspectah-cli/src/commands/fleet.rs` with the clap derive structs:

```rust
use clap::{Args, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Subcommand)]
pub enum FleetCommands {
    /// Aggregate host tarballs into a fleet tarball
    #[command(name = "fleet")]
    Aggregate(FleetAggregateArgs),
    /// Generate a fleet manifest from a directory of tarballs
    Init(FleetInitArgs),
}

#[derive(Debug, Args)]
pub struct FleetAggregateArgs {
    /// Input tarballs or directory
    #[arg(required_unless_present = "manifest")]
    pub inputs: Vec<PathBuf>,

    /// TOML manifest file
    #[arg(long, conflicts_with = "inputs")]
    pub manifest: Option<PathBuf>,

    /// Baseline image override
    #[arg(long)]
    pub baseline: Option<String>,

    /// Write output to this directory
    #[arg(long)]
    pub output_dir: Option<PathBuf>,

    /// Explicit output tarball name
    #[arg(long)]
    pub output_file: Option<PathBuf>,

    /// Emit merged snapshot JSON only
    #[arg(long)]
    pub json_only: bool,

    /// Promote warnings to errors
    #[arg(long)]
    pub strict: bool,

    /// Verbose output
    #[arg(long, short)]
    pub verbose: bool,
}

#[derive(Debug, Args)]
pub struct FleetInitArgs {
    /// Directory of tarballs to scan
    pub directory: PathBuf,

    /// Output file path (default: fleet.toml in current directory)
    #[arg(long)]
    pub output: Option<PathBuf>,

    /// Overwrite existing fleet.toml
    #[arg(long)]
    pub overwrite: bool,
}
```

- [ ] **Step 2: Implement input resolution**

Write the logic that resolves input modes:
- Single directory arg → load all `.tar.gz` files from it
- Multiple file args → use as-is
- `--manifest` → parse TOML and resolve paths

- [ ] **Step 3: Implement --strict flag behavior**

In the aggregate handler, after calling `merge_snapshots()`, check if `--strict` is set. If so, treat any returned warnings as errors:

```rust
let (merged, warnings) = merge_snapshots(snapshots, manifest.as_ref())
    .map_err(|errors| /* format validation errors */)?;

if args.strict && !warnings.is_empty() {
    // Format warnings as errors and exit non-zero
    for w in &warnings {
        eprintln!("error (--strict): {w}");
    }
    anyhow::bail!("{} warning(s) promoted to errors by --strict", warnings.len());
}
```

- [ ] **Step 4: Implement the aggregate command handler**

Wire together: input resolution → tarball loading → snapshot extraction → `merge_snapshots()` → `--strict` check → render → package tarball.

Follow the existing scan command's pattern for calling `render_all()` and `tarball::create_tarball()`.

After `render_all()`, add fleet-specific steps:
- Prepend draft header to Containerfile
- Write `fleet/variants/` for any Alternative content variants

- [ ] **Step 4: Implement output formatting**

Per spec:
- Default: 3 lines (Fleet label + hosts, Merged counts, Output path)
- Warnings above summary on stderr
- `--json-only`: JSON to stdout (suppress summary), or to file
- `--verbose`: per-host counts, full prevalence breakdown

- [ ] **Step 5: Register the fleet subcommand**

In `inspectah-cli/src/commands/mod.rs`, add `pub mod fleet;` and register in the CLI enum.

In `inspectah-cli/src/main.rs`, wire the fleet commands.

- [ ] **Step 6: Build and verify CLI help**

Run: `cargo build -p inspectah-cli && ./target/debug/inspectah fleet --help`
Expected: shows fleet command with all flags

Run: `./target/debug/inspectah fleet init --help`
Expected: shows fleet init command

- [ ] **Step 7: Commit**

```bash
git add inspectah-cli/src/commands/fleet.rs inspectah-cli/src/commands/mod.rs inspectah-cli/src/main.rs inspectah-cli/Cargo.toml
git commit -m "feat(cli): inspectah fleet command with all input modes and flags"
```

---

## Task 12: Fleet Init Command

**Files:**
- Modify: `inspectah-cli/src/commands/fleet.rs`

Implement `inspectah fleet init <directory>` — scans tarballs and generates a commented `fleet.toml`.

- [ ] **Step 1: Implement tarball scanning**

Scan the directory for `.tar.gz` files, extract each snapshot's hostname and target_image from the embedded `inspection-snapshot.json`.

Reuse `inspectah-refine`'s tarball extraction code or the pipeline's tarball reading — check what's available. The key function: open tarball → find `inspection-snapshot.json` → parse just the metadata fields (hostname, target_image).

- [ ] **Step 2: Implement manifest generation**

Generate commented TOML:

```rust
fn generate_manifest_toml(
    label: &str,
    baseline: Option<&str>,
    sources: &[PathBuf],
    manifest_dir: &Path,
) -> String {
    let mut lines = vec![
        "# inspectah fleet manifest".to_string(),
        "# Edit label and baseline as needed. Sources are relative to this file.".to_string(),
        String::new(),
        format!("label = \"{}\"", label),
    ];
    if let Some(bl) = baseline {
        lines.push(format!("baseline = \"{}\"", bl));
    } else {
        lines.push("# baseline = \"registry.redhat.io/rhel9/rhel-bootc:9.6\"".to_string());
    }
    lines.push(String::new());
    lines.push("sources = [".to_string());
    for source in sources {
        let relative = pathdiff::diff_paths(source, manifest_dir)
            .unwrap_or_else(|| source.clone());
        lines.push(format!("  \"{}\",", relative.display()));
    }
    lines.push("]".to_string());
    lines.join("\n") + "\n"
}
```

Add `pathdiff = "0.2"` to `inspectah-cli/Cargo.toml` if not already present.

- [ ] **Step 3: Implement the init command handler**

- Check output path (default: `./fleet.toml`)
- Refuse if exists unless `--overwrite`
- Scan directory, extract metadata, report baseline conflicts on stderr
- Write manifest file
- Print summary on stderr

- [ ] **Step 4: Test manually with a directory of test tarballs**

If test tarballs exist in the repo, use those. Otherwise create a minimal test fixture.

- [ ] **Step 5: Commit**

```bash
git add inspectah-cli/src/commands/fleet.rs inspectah-cli/Cargo.toml
git commit -m "feat(cli): inspectah fleet init command for manifest generation"
```

---

## Task 13: Fleet-Aware Audit Report

**Files:**
- Modify: `inspectah-pipeline/src/render/audit.rs`

The spec requires the audit report to include a fleet summary section when `fleet_meta` is present. The base audit content comes from `render_all()` — this task adds fleet-specific augmentation.

- [ ] **Step 1: Check current audit renderer for fleet_meta awareness**

Run: `grep -n 'fleet_meta\|fleet' inspectah-pipeline/src/render/audit.rs`

If the audit renderer already checks for fleet data, extend it. If not, add a conditional section.

- [ ] **Step 2: Add fleet summary section to audit renderer**

When `snap.fleet_meta.is_some()`, prepend or append a fleet summary section:

```rust
if let Some(ref meta) = snap.fleet_meta {
    lines.push("## Fleet Aggregate Summary".into());
    lines.push(format!("- **Hosts:** {} ({})", meta.host_count,
        meta.hostnames.join(", ")));
    if meta.baseline_provisional {
        lines.push("- **Baseline:** auto-selected (provisional — confirm in refine)".into());
    }
    // Section coverage
    lines.push("- **Section coverage:**".into());
    for (section, count) in &meta.section_host_counts {
        lines.push(format!("  - {section}: {count}/{} hosts reported", meta.host_count));
    }
    lines.push(String::new());
}
```

- [ ] **Step 3: Run audit renderer tests**

Run: `cargo test -p inspectah-pipeline -- audit`
Expected: PASS (new code is additive, existing tests shouldn't break)

- [ ] **Step 4: Commit**

```bash
git add inspectah-pipeline/src/render/audit.rs
git commit -m "feat(render): fleet summary section in audit report"
```

---

## Task 14: Fleet Variant File Writing + Containerfile Header

**Files:**
- Modify: `inspectah-cli/src/commands/fleet.rs`

After `render_all()` produces the standard scan artifacts, the fleet command adds two things:
1. `fleet/variants/` directory with non-selected content variants
2. Draft header prepended to the Containerfile

- [ ] **Step 1: Implement variant file extraction**

```rust
fn write_fleet_variants(
    snap: &InspectionSnapshot,
    output_dir: &Path,
) -> Result<(), anyhow::Error> {
    use sha2::{Sha256, Digest};

    let variants_dir = output_dir.join("fleet").join("variants");

    // Walk all content-variant sections: config files, drop-ins, quadlets, compose, repo files
    // For each item with VariantSelection::Alternative, write content to
    // fleet/variants/{path}/{8-char-hash}.{ext}

    if let Some(config) = &snap.config {
        for entry in &config.files {
            if entry.variant_selection == VariantSelection::Alternative {
                let hash = format!("{:x}", Sha256::digest(entry.content.as_bytes()));
                let short_hash = &hash[..8];
                let variant_dir = variants_dir.join(
                    entry.path.trim_start_matches('/'));
                std::fs::create_dir_all(&variant_dir)?;
                let ext = Path::new(&entry.path)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("conf");
                std::fs::write(
                    variant_dir.join(format!("{short_hash}.{ext}")),
                    &entry.content,
                )?;
            }
        }
    }

    // Repeat for services.drop_ins, containers.quadlet_units,
    // containers.compose_files, rpm.repo_files

    Ok(())
}
```

- [ ] **Step 2: Implement Containerfile header prepending**

```rust
fn prepend_fleet_header(
    output_dir: &Path,
    meta: &FleetSnapshotMeta,
    target_image: Option<&str>,
) -> Result<(), anyhow::Error> {
    let cf_path = output_dir.join("Containerfile");
    let existing = std::fs::read_to_string(&cf_path)?;

    let mut header = format!(
        "# Fleet aggregate: {} ({} hosts)\n\
         # This is a draft — review before use\n",
        meta.label, meta.host_count
    );
    if let Some(img) = target_image {
        let provisional = if meta.baseline_provisional { " (auto-selected, provisional)" } else { "" };
        header.push_str(&format!("# Baseline: {img}{provisional}\n"));
    }

    std::fs::write(&cf_path, format!("{header}{existing}"))?;
    Ok(())
}
```

- [ ] **Step 3: Wire into the aggregate command handler**

After `render_all()` and before `create_tarball()`:
1. Call `write_fleet_variants()`
2. Call `prepend_fleet_header()`

- [ ] **Step 4: Test end-to-end with test fixtures**

Create a minimal integration test or manual test that:
1. Creates two test snapshots with config file variants
2. Runs fleet aggregate
3. Verifies the output tarball contains `fleet/variants/`
4. Verifies the Containerfile has the draft header

- [ ] **Step 5: Run `cargo clippy -- -W clippy::all` and fix warnings**

- [ ] **Step 6: Commit**

```bash
git add inspectah-cli/src/commands/fleet.rs
git commit -m "feat(cli): fleet variant file writing and Containerfile draft header"
```

---

## Task 15: End-to-End Integration Tests

**Files:**
- Create: `inspectah-core/tests/fleet_e2e_test.rs`

Write integration tests that exercise the full merge pipeline with realistic snapshot data.

- [ ] **Step 1: Write test helpers for building rich test snapshots**

Build helpers that create `InspectionSnapshot` with multiple sections populated: RPM packages, config files (with variants), services, selinux ports, etc.

- [ ] **Step 2: Write e2e test: 3 hosts, shared packages, config variants**

Test that:
- Packages appearing on all 3 hosts get count=3, total=3
- Packages on 1 host get count=1, total=3
- Config file with 2 variants: most-prevalent becomes Selected
- `fleet_meta` has correct host_count, hostnames (sorted), section_host_counts
- All items have `include: true`
- Output is deterministic (run twice with different input order, compare)

- [ ] **Step 3: Write e2e test: validation hard errors**

Test that `merge_snapshots` returns `Err` for:
- Mixed architectures
- Duplicate hostnames
- OS major version mismatch
- Empty snapshot

- [ ] **Step 4: Write e2e test: missing sections use global denominator**

Test with 3 hosts where only 2 have a config section. Verify config items have `total=3` (global), not `total=2` (per-section).

- [ ] **Step 5: Write e2e test: baseline selection**

Test with 3 hosts, 2 sharing one target_image and 1 with a different one. Verify the most-common target_image is selected and `baseline_provisional` is true.

- [ ] **Step 6: Run all tests**

Run: `cargo test --workspace`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add inspectah-core/tests/fleet_e2e_test.rs
git commit -m "test(core): fleet aggregate end-to-end integration tests"
```

---

## Task 16: Final Cleanup and Workspace Verification

- [ ] **Step 1: Run clippy across workspace**

Run: `cargo clippy --workspace -- -W clippy::all`
Expected: zero warnings

- [ ] **Step 2: Run fmt check**

Run: `cargo fmt --all --check`
Expected: no formatting issues

- [ ] **Step 3: Run full test suite**

Run: `cargo test --workspace`
Expected: all tests pass

- [ ] **Step 4: Verify the binary works**

Build: `cargo build -p inspectah-cli`

Test `--help`:
```bash
./target/debug/inspectah fleet --help
./target/debug/inspectah fleet init --help
```

- [ ] **Step 5: Update SCHEMA_VERSION in any snapshot test golden files**

If parity tests or golden files reference the old schema version, update them.

- [ ] **Step 6: Commit any remaining cleanup**

```bash
git add -A
git commit -m "chore: fleet aggregate final cleanup and workspace verification"
```
