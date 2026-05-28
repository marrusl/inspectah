# Fleet Refine Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add fleet-aware refinement to the inspectah refine engine: zone classification, fleet attention scoring, variant operations (select, edit, discard), diff computation, session persistence with auto-save/resume, and variant-aware export.

**Architecture:** Extends two existing crates. `inspectah-core` gets a `PrevalenceZone` enum and classifier. `inspectah-refine` gets a `fleet/` submodule (attention, variant ops, diffs) and `autosave.rs` for session persistence. All fleet variant state flows through one authoritative path: the op journal is replayed by `snapshot_projected()` → `recompute_view()`. View, export, undo/redo, and resume all derive from this single projection. No apply-time side state.

**Tech Stack:** Rust, `similar` crate (LCS diffs), `sha2` (already a dep), `serde_json` (session persistence)

**Spec:** `docs/specs/proposed/2026-05-20-fleet-refine-engine-spec.md` (7 review rounds, approved)

**Key seams in the current tree:**
- Session: `inspectah-refine/src/session.rs` — `RefineSession`, `snapshot_projected()`, `recompute_view()`, `render_refine_export()` (line 758)
- Tarball loader: `inspectah-refine/src/tarball.rs` — `from_tarball()`, extraction, provenance checks
- CLI entrypoint: `inspectah-cli/src/commands/refine.rs` — `run_refine()`, tarball open, server start
- Types: `inspectah-refine/src/types.rs` — `RefinementOp`, `RefinedView`, `AttentionLevel`
- Attention: `inspectah-refine/src/attention.rs` — `compute_package_attention()`, `compute_config_attention()`
- Core fleet types: `inspectah-core/src/types/fleet.rs` — `FleetPrevalence`, `VariantSelection`, `FleetSnapshotMeta`

---

## Task 1: PrevalenceZone + classify_zone (inspectah-core)

**Files:**
- Modify: `inspectah-core/src/types/fleet.rs`
- Modify: `inspectah-core/src/fleet/mod.rs`
- Test: `inspectah-core/tests/fleet_zone_test.rs`

- [ ] **Step 1: Write failing tests**

Create `inspectah-core/tests/fleet_zone_test.rs`:

```rust
use inspectah_core::types::fleet::{FleetPrevalence, PrevalenceZone};
use inspectah_core::fleet::classify_zone;

#[test]
fn consensus_when_all_hosts() {
    let fp = FleetPrevalence { count: 5, total: 5, hosts: vec![] };
    assert_eq!(classify_zone(&fp), PrevalenceZone::Consensus);
}

#[test]
fn near_consensus_at_exactly_half() {
    let fp = FleetPrevalence { count: 5, total: 10, hosts: vec![] };
    assert_eq!(classify_zone(&fp), PrevalenceZone::NearConsensus);
}

#[test]
fn near_consensus_above_half_odd() {
    // 3/5 = 60%, count*2=6 >= total=5 → NearConsensus
    let fp = FleetPrevalence { count: 3, total: 5, hosts: vec![] };
    assert_eq!(classify_zone(&fp), PrevalenceZone::NearConsensus);
}

#[test]
fn divergent_below_half() {
    // 2/5 = 40%, count*2=4 < total=5 → Divergent
    let fp = FleetPrevalence { count: 2, total: 5, hosts: vec![] };
    assert_eq!(classify_zone(&fp), PrevalenceZone::Divergent);
}

#[test]
fn divergent_single_host_of_twenty() {
    let fp = FleetPrevalence { count: 1, total: 20, hosts: vec![] };
    assert_eq!(classify_zone(&fp), PrevalenceZone::Divergent);
}

#[test]
fn consensus_when_count_equals_total_min_case() {
    let fp = FleetPrevalence { count: 1, total: 1, hosts: vec![] };
    assert_eq!(classify_zone(&fp), PrevalenceZone::Consensus);
}

#[test]
fn ord_divergent_less_than_near_consensus_less_than_consensus() {
    assert!(PrevalenceZone::Divergent < PrevalenceZone::NearConsensus);
    assert!(PrevalenceZone::NearConsensus < PrevalenceZone::Consensus);
}

#[test]
fn zone_serde_roundtrip() {
    for zone in [PrevalenceZone::Divergent, PrevalenceZone::NearConsensus, PrevalenceZone::Consensus] {
        let json = serde_json::to_string(&zone).unwrap();
        let parsed: PrevalenceZone = serde_json::from_str(&json).unwrap();
        assert_eq!(zone, parsed);
    }
}

#[test]
fn zone_is_hashable() {
    use std::collections::HashSet;
    let mut set = HashSet::new();
    set.insert(PrevalenceZone::Consensus);
    set.insert(PrevalenceZone::Consensus);
    assert_eq!(set.len(), 1);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-core --test fleet_zone_test -- --nocapture`
Expected: FAIL — `PrevalenceZone` and `classify_zone` don't exist

- [ ] **Step 3: Add PrevalenceZone enum**

In `inspectah-core/src/types/fleet.rs`, add after the existing types:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PrevalenceZone {
    Divergent,
    NearConsensus,
    Consensus,
}
```

Variant declaration order defines `Ord`: Divergent < NearConsensus < Consensus.

- [ ] **Step 4: Add classify_zone function**

In `inspectah-core/src/fleet/mod.rs`, add:

```rust
use crate::types::fleet::{FleetPrevalence, PrevalenceZone};

pub fn classify_zone(prevalence: &FleetPrevalence) -> PrevalenceZone {
    if prevalence.count == prevalence.total {
        PrevalenceZone::Consensus
    } else if prevalence.count * 2 >= prevalence.total {
        PrevalenceZone::NearConsensus
    } else {
        PrevalenceZone::Divergent
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p inspectah-core --test fleet_zone_test -- --nocapture`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add inspectah-core/src/types/fleet.rs inspectah-core/src/fleet/mod.rs inspectah-core/tests/fleet_zone_test.rs
git commit -m "feat(core): add PrevalenceZone enum and classify_zone function"
```

---

## Task 2: ContentHash + ItemId + New Op Types (inspectah-refine)

**Files:**
- Modify: `inspectah-refine/src/types.rs`
- Modify: `inspectah-refine/Cargo.toml` (add `similar`)
- Test: `inspectah-refine/tests/fleet_types_test.rs`

- [ ] **Step 1: Add `similar` to Cargo.toml**

In `inspectah-refine/Cargo.toml` under `[dependencies]`, add:
```toml
similar = "2"
```

- [ ] **Step 2: Write failing tests**

Create `inspectah-refine/tests/fleet_types_test.rs`:

```rust
use inspectah_refine::types::{ContentHash, ItemId, RefinementOp};

#[test]
fn content_hash_valid_64_hex() {
    let hash = ContentHash::new("a".repeat(64)).unwrap();
    assert_eq!(hash.as_str(), "a".repeat(64));
}

#[test]
fn content_hash_rejects_63_chars() {
    assert!(ContentHash::new("a".repeat(63)).is_err());
}

#[test]
fn content_hash_rejects_non_hex() {
    assert!(ContentHash::new("z".repeat(64)).is_err());
}

#[test]
fn content_hash_from_content_produces_valid_hash() {
    let hash = ContentHash::from_content(b"hello world");
    assert_eq!(hash.as_str().len(), 64);
    assert!(hash.as_str().chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn content_hash_serde_roundtrip() {
    let hash = ContentHash::from_content(b"test");
    let json = serde_json::to_string(&hash).unwrap();
    let parsed: ContentHash = serde_json::from_str(&json).unwrap();
    assert_eq!(hash, parsed);
}

#[test]
fn content_hash_ord_for_btreemap() {
    use std::collections::BTreeMap;
    let mut map = BTreeMap::new();
    let h1 = ContentHash::from_content(b"aaa");
    let h2 = ContentHash::from_content(b"bbb");
    map.insert(h1.clone(), "first");
    map.insert(h2.clone(), "second");
    assert_eq!(map.len(), 2);
}

#[test]
fn item_id_config_serde_roundtrip() {
    let id = ItemId::Config { path: "/etc/nginx/nginx.conf".into() };
    let json = serde_json::to_string(&id).unwrap();
    assert!(json.contains("Config"));
    let parsed: ItemId = serde_json::from_str(&json).unwrap();
    assert_eq!(id, parsed);
}

#[test]
fn item_id_package_serde_roundtrip() {
    let id = ItemId::Package { name_arch: "httpd.x86_64".into() };
    let json = serde_json::to_string(&id).unwrap();
    let parsed: ItemId = serde_json::from_str(&json).unwrap();
    assert_eq!(id, parsed);
}

#[test]
fn select_variant_op_serde() {
    let hash = ContentHash::from_content(b"variant content");
    let op = RefinementOp::SelectVariant {
        item_id: ItemId::Config { path: "/etc/test.conf".into() },
        target: hash,
    };
    let json = serde_json::to_string(&op).unwrap();
    let parsed: RefinementOp = serde_json::from_str(&json).unwrap();
    assert_eq!(op, parsed);
}

#[test]
fn edit_variant_op_serde() {
    let op = RefinementOp::EditVariant {
        item_id: ItemId::DropIn { path: "/etc/systemd/system/httpd.service.d/override.conf".into() },
        content: "new content".into(),
        based_on: None,
    };
    let json = serde_json::to_string(&op).unwrap();
    let parsed: RefinementOp = serde_json::from_str(&json).unwrap();
    assert_eq!(op, parsed);
}

#[test]
fn discard_variant_op_serde() {
    let hash = ContentHash::from_content(b"discard me");
    let op = RefinementOp::DiscardVariant {
        item_id: ItemId::Config { path: "/etc/test.conf".into() },
        variant: hash,
    };
    let json = serde_json::to_string(&op).unwrap();
    let parsed: RefinementOp = serde_json::from_str(&json).unwrap();
    assert_eq!(op, parsed);
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p inspectah-refine --test fleet_types_test -- --nocapture`
Expected: FAIL

- [ ] **Step 4: Implement ContentHash**

In `inspectah-refine/src/types.rs`, add:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ContentHash(String);

impl ContentHash {
    pub fn new(s: impl Into<String>) -> Result<Self, String> {
        let s = s.into();
        if s.len() != 64 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!("invalid content hash: expected 64 hex chars, got {} chars", s.len()));
        }
        Ok(Self(s))
    }

    pub fn from_content(content: &[u8]) -> Self {
        use sha2::{Digest, Sha256};
        Self(format!("{:x}", Sha256::digest(content)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}
```

Note: `ContentHash` derives `Ord` so it can be used as a `BTreeMap` key in the batch diff API.

- [ ] **Step 5: Implement ItemId enum**

In `inspectah-refine/src/types.rs`, add the full `ItemId` enum with all 21 variants per the spec. Every variant carries a single `String` field matching the canonical `FleetMergeable::identity_key()` format.

- [ ] **Step 6: Add new RefinementOp variants**

In the existing `RefinementOp` enum, add `SelectVariant`, `EditVariant`, `DiscardVariant` per the spec.

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test -p inspectah-refine --test fleet_types_test -- --nocapture`
Expected: PASS

- [ ] **Step 8: Run full test suite for regressions**

Run: `cargo test -p inspectah-refine -- --nocapture`
Expected: All existing tests pass

- [ ] **Step 9: Commit**

```bash
git add inspectah-refine/src/types.rs inspectah-refine/tests/fleet_types_test.rs inspectah-refine/Cargo.toml
git commit -m "feat(refine): add ContentHash, ItemId, and fleet RefinementOp variants"
```

---

## Task 3: RefineMode + FleetContext + Auto-Detection

**Files:**
- Modify: `inspectah-refine/src/types.rs`
- Modify: `inspectah-refine/src/session.rs`
- Test: `inspectah-refine/tests/fleet_session_test.rs`

**Key design point:** Fleet-of-2 is `RefineMode::Fleet` with `zones_active: false` (zones suppressed, variant ops available). Fleet-of-3+ is `RefineMode::Fleet` with `zones_active: true`. Single-host snapshots (no `FleetSnapshotMeta`) are `RefineMode::SingleHost`. A fleet-of-1 tarball is not a realistic input.

- [ ] **Step 1: Write failing tests**

Create `inspectah-refine/tests/fleet_session_test.rs`:

```rust
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::fleet::{FleetSnapshotMeta, PrevalenceZone};
use inspectah_refine::session::RefineSession;
use std::collections::BTreeMap;

fn make_fleet_snapshot(host_count: usize) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::default();
    snap.fleet_meta = Some(FleetSnapshotMeta {
        label: "test".into(),
        host_count,
        hostnames: (0..host_count).map(|i| format!("host-{i}")).collect(),
        merged_at: "2026-05-20T00:00:00Z".into(),
        baseline_provisional: false,
        section_host_counts: BTreeMap::new(),
    });
    snap
}

#[test]
fn single_host_snapshot_has_no_fleet_context() {
    let session = RefineSession::new(InspectionSnapshot::default());
    assert!(session.fleet_context().is_none());
}

#[test]
fn fleet_of_five_has_fleet_context() {
    let session = RefineSession::new(make_fleet_snapshot(5));
    let ctx = session.fleet_context().unwrap();
    assert_eq!(ctx.total_hosts, 5);
}

#[test]
fn fleet_of_two_has_fleet_context_zones_suppressed() {
    let session = RefineSession::new(make_fleet_snapshot(2));
    let ctx = session.fleet_context().unwrap();
    assert_eq!(ctx.total_hosts, 2);
    assert!(!ctx.zones_active, "fleet-of-2 suppresses zones");
}

#[test]
fn fleet_of_three_has_zones_active() {
    let session = RefineSession::new(make_fleet_snapshot(3));
    let ctx = session.fleet_context().unwrap();
    assert!(ctx.zones_active, "fleet-of-3+ activates zones");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-refine --test fleet_session_test -- --nocapture`
Expected: FAIL

- [ ] **Step 3: Add FleetContext and RefineMode types**

In `inspectah-refine/src/types.rs`:

```rust
use inspectah_core::types::fleet::{FleetSnapshotMeta, PrevalenceZone};

pub struct FleetContext {
    pub fleet_meta: FleetSnapshotMeta,
    pub zones: HashMap<ItemId, PrevalenceZone>,
    pub total_hosts: usize,
    pub zones_active: bool,  // false for fleet-of-2, true for 3+
}

pub enum RefineMode {
    SingleHost,
    Fleet(FleetContext),
}
```

- [ ] **Step 4: Add refine_mode field and fleet detection to RefineSession**

In `session.rs`:
- Add `refine_mode: RefineMode` field to `RefineSession`
- In `RefineSession::new()`, check for `FleetSnapshotMeta` in the snapshot. If present:
  - Construct `FleetContext` with `zones_active = fleet_meta.host_count >= 3`
  - Iterate all prevalence-tracked items and call `classify_zone()` to populate the zone map
  - Items with `fleet: None` get `tracing::warn!` and are excluded from the zone map
- If no `FleetSnapshotMeta`, set `RefineMode::SingleHost`
- Add `pub fn fleet_context(&self) -> Option<&FleetContext>` accessor

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p inspectah-refine --test fleet_session_test -- --nocapture`
Expected: PASS

- [ ] **Step 6: Run full suite for regressions**

Run: `cargo test -p inspectah-refine -- --nocapture`
Expected: All pass

- [ ] **Step 7: Commit**

```bash
git add inspectah-refine/src/types.rs inspectah-refine/src/session.rs inspectah-refine/tests/fleet_session_test.rs
git commit -m "feat(refine): add RefineMode, FleetContext with zones_active, fleet auto-detection"
```

---

## Task 4: FleetAttention + AttentionScore + Scoring

**Files:**
- Create: `inspectah-refine/src/fleet/mod.rs`
- Create: `inspectah-refine/src/fleet/attention.rs`
- Modify: `inspectah-refine/src/lib.rs`
- Modify: `inspectah-refine/src/types.rs`
- Test: `inspectah-refine/tests/fleet_attention_test.rs`

- [ ] **Step 1: Write failing tests for FleetAttention Ord**

Create `inspectah-refine/tests/fleet_attention_test.rs`:

```rust
use inspectah_core::types::fleet::PrevalenceZone;
use inspectah_refine::types::{AttentionLevel, AttentionScore, FleetAttention};

#[test]
fn divergent_sorts_before_consensus_regardless_of_attention() {
    let a = FleetAttention {
        zone: PrevalenceZone::Divergent,
        attention: AttentionLevel::Informational,
        prevalence: 10,
    };
    let b = FleetAttention {
        zone: PrevalenceZone::Consensus,
        attention: AttentionLevel::NeedsReview,
        prevalence: 1,
    };
    assert!(a < b);
}

#[test]
fn within_zone_needs_review_before_informational() {
    let a = FleetAttention {
        zone: PrevalenceZone::Divergent,
        attention: AttentionLevel::NeedsReview,
        prevalence: 5,
    };
    let b = FleetAttention {
        zone: PrevalenceZone::Divergent,
        attention: AttentionLevel::Informational,
        prevalence: 5,
    };
    assert!(a < b);
}

#[test]
fn within_zone_and_attention_lower_prevalence_first() {
    let a = FleetAttention {
        zone: PrevalenceZone::Divergent,
        attention: AttentionLevel::NeedsReview,
        prevalence: 2,
    };
    let b = FleetAttention {
        zone: PrevalenceZone::Divergent,
        attention: AttentionLevel::NeedsReview,
        prevalence: 8,
    };
    assert!(a < b);
}

#[test]
fn sort_vec_of_fleet_attention_produces_correct_order() {
    let items = vec![
        FleetAttention { zone: PrevalenceZone::Consensus, attention: AttentionLevel::Informational, prevalence: 20 },
        FleetAttention { zone: PrevalenceZone::Divergent, attention: AttentionLevel::NeedsReview, prevalence: 1 },
        FleetAttention { zone: PrevalenceZone::NearConsensus, attention: AttentionLevel::NeedsReview, prevalence: 15 },
        FleetAttention { zone: PrevalenceZone::Divergent, attention: AttentionLevel::Informational, prevalence: 3 },
    ];
    let mut sorted = items.clone();
    sorted.sort();
    assert_eq!(sorted[0].prevalence, 1);  // Divergent, NeedsReview, lowest prevalence
    assert_eq!(sorted[1].prevalence, 3);  // Divergent, Informational
    assert_eq!(sorted[2].prevalence, 15); // NearConsensus
    assert_eq!(sorted[3].prevalence, 20); // Consensus
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-refine --test fleet_attention_test -- --nocapture`
Expected: FAIL

- [ ] **Step 3: Add FleetAttention and AttentionScore types**

In `inspectah-refine/src/types.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FleetAttention {
    pub zone: PrevalenceZone,
    pub attention: AttentionLevel,
    pub prevalence: u32,
}

impl Ord for FleetAttention {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.zone.cmp(&other.zone)
            .then(self.attention.cmp(&other.attention))
            .then(self.prevalence.cmp(&other.prevalence))
    }
}

impl PartialOrd for FleetAttention {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone)]
pub enum AttentionScore {
    SingleHost(AttentionLevel),
    Fleet(FleetAttention),
}
```

Ensure `AttentionLevel` derives `PartialOrd, Ord` with `NeedsReview` declared before `Informational`.

- [ ] **Step 4: Create fleet submodule and attention scoring**

Create `inspectah-refine/src/fleet/mod.rs`:
```rust
pub mod attention;
```

Register in `inspectah-refine/src/lib.rs`:
```rust
pub mod fleet;
```

Create `inspectah-refine/src/fleet/attention.rs` with fleet-aware scoring that composes `PrevalenceZone` (from `FleetContext.zones`) with `AttentionLevel` (from existing scoring logic) and raw `prevalence` count.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p inspectah-refine --test fleet_attention_test -- --nocapture`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add inspectah-refine/src/fleet/ inspectah-refine/src/lib.rs inspectah-refine/src/types.rs inspectah-refine/tests/fleet_attention_test.rs
git commit -m "feat(refine): add FleetAttention with Ord, AttentionScore, fleet attention scoring"
```

---

## Task 5: Diff Engine

**Files:**
- Create: `inspectah-refine/src/fleet/diff.rs`
- Modify: `inspectah-refine/src/fleet/mod.rs`
- Test: `inspectah-refine/tests/fleet_diff_test.rs`

- [ ] **Step 1: Write failing tests**

Create `inspectah-refine/tests/fleet_diff_test.rs`:

```rust
use inspectah_refine::fleet::diff::{compute_diff, compute_batch_diff, ChangeKind, DiffError};
use inspectah_refine::types::ContentHash;

#[test]
fn identical_content_empty_hunks() {
    let result = compute_diff("hello\nworld\n", "hello\nworld\n", 3).unwrap();
    assert!(result.hunks.is_empty());
    assert_eq!(result.stats.total_changes, 0);
    assert_eq!(result.stats.insertions, 0);
    assert_eq!(result.stats.deletions, 0);
}

#[test]
fn single_line_change_produces_hunk() {
    let result = compute_diff("a\nb\nc\n", "a\nB\nc\n", 3).unwrap();
    assert!(!result.hunks.is_empty());
    assert_eq!(result.stats.insertions, 1);
    assert_eq!(result.stats.deletions, 1);
}

#[test]
fn empty_base_all_inserts() {
    let result = compute_diff("", "line1\nline2\n", 3).unwrap();
    assert_eq!(result.stats.insertions, 2);
    assert_eq!(result.stats.deletions, 0);
}

#[test]
fn binary_content_rejected() {
    let result = compute_diff("hello\0world", "other", 3);
    assert!(matches!(result, Err(DiffError::BinaryContent)));
}

#[test]
fn binary_in_target_rejected() {
    let result = compute_diff("clean text", "has\0null", 3);
    assert!(matches!(result, Err(DiffError::BinaryContent)));
}

#[test]
fn input_too_large_rejected() {
    let large = "x\n".repeat(60_000); // >100KB
    let result = compute_diff(&large, "small", 3);
    assert!(matches!(result, Err(DiffError::InputTooLarge)));
}

#[test]
fn context_lines_trims_equal_runs() {
    let base = (0..100).map(|i| format!("line{i}\n")).collect::<String>();
    let target = base.replace("line50\n", "CHANGED\n");
    let result = compute_diff(&base, &target, 3).unwrap();
    let equal_count: usize = result.hunks.iter()
        .flat_map(|h| &h.changes)
        .filter(|c| c.kind == ChangeKind::Equal)
        .count();
    assert!(equal_count <= 7, "at most 3+3 context + boundary, got {equal_count}");
}

#[test]
fn batch_diff_multiple_targets() {
    let t1 = ContentHash::from_content(b"a\nB\nc\n");
    let t2 = ContentHash::from_content(b"a\nb\nC\n");
    let results = compute_batch_diff(
        "a\nb\nc\n",
        &[(t1.clone(), "a\nB\nc\n"), (t2.clone(), "a\nb\nC\n")],
        3,
    );
    assert_eq!(results.len(), 2);
    assert!(results[&t1].is_ok());
    assert!(results[&t2].is_ok());
}

#[test]
fn batch_diff_per_target_error() {
    let good = ContentHash::from_content(b"clean");
    let bad = ContentHash::from_content(b"has\0null");
    let results = compute_batch_diff(
        "base text",
        &[(good.clone(), "clean"), (bad.clone(), "has\0null")],
        3,
    );
    assert!(results[&good].is_ok());
    assert!(results[&bad].is_err());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-refine --test fleet_diff_test -- --nocapture`
Expected: FAIL

- [ ] **Step 3: Implement diff types and functions**

Create `inspectah-refine/src/fleet/diff.rs` with `DiffResult`, `DiffHunk`, `DiffChange`, `DiffStats`, `LineRange`, `ChangeKind`, `DiffError` types, `compute_diff()` using `similar::TextDiff::from_lines()` → `grouped_ops()`, and `compute_batch_diff()` returning `BTreeMap<ContentHash, Result<DiffResult, DiffError>>`. Include 100KB input cap and null-byte binary detection.

Register in `inspectah-refine/src/fleet/mod.rs`:
```rust
pub mod diff;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p inspectah-refine --test fleet_diff_test -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add inspectah-refine/src/fleet/diff.rs inspectah-refine/src/fleet/mod.rs inspectah-refine/tests/fleet_diff_test.rs
git commit -m "feat(refine): add diff engine with similar crate, batch API, and input guards"
```

---

## Task 6: Variant Operations via Projection

**Files:**
- Create: `inspectah-refine/src/fleet/variant_ops.rs`
- Modify: `inspectah-refine/src/fleet/mod.rs`
- Modify: `inspectah-refine/src/session.rs` (extend `snapshot_projected()` and `apply()`)
- Test: `inspectah-refine/tests/fleet_variant_ops_test.rs`

**Key design point:** Variant state is NOT maintained as apply-time side state. It is derived by `snapshot_projected()` — the same path that drives view recomputation, export, and resume. The in-memory `user_variants: HashMap<ItemId, HashMap<ContentHash, String>>` working map is built during projection by scanning the op journal.

- [ ] **Step 1: Write failing tests for SelectVariant**

Tests should create a fleet snapshot with config items having multiple variants, apply a `SelectVariant` op, call `snapshot_projected()`, and verify the projected snapshot has the correct `VariantSelection` values.

- [ ] **Step 2: Write failing tests for EditVariant**

Tests for: new variant creation, convergence detection (content matches existing host-sourced variant on same item), convergence with prior user-created variant on same item, `based_on` validation.

- [ ] **Step 3: Write failing tests for DiscardVariant**

Tests for: discard user-created variant, fallback to most-prevalent host-sourced on discard of Selected, error on discard of host-sourced, single-variant-after-discard becomes Only.

- [ ] **Step 4: Write failing tests for undo**

Tests for: undo SelectVariant restores previous selection, undo EditVariant removes user content from working map, undo converged EditVariant does NOT remove content, undo DiscardVariant restores variant.

- [ ] **Step 5: Run tests to verify they fail**

Run: `cargo test -p inspectah-refine --test fleet_variant_ops_test -- --nocapture`
Expected: FAIL

- [ ] **Step 6: Implement variant_ops.rs**

Create `inspectah-refine/src/fleet/variant_ops.rs` with functions that modify snapshot variant state. These functions are called by `snapshot_projected()` during its op-journal scan. The working `user_variants` map is populated during projection, not at apply time.

Register in `inspectah-refine/src/fleet/mod.rs`:
```rust
pub mod variant_ops;
```

- [ ] **Step 7: Extend snapshot_projected() in session.rs**

In `session.rs`, extend `snapshot_projected()` to handle `SelectVariant`, `EditVariant`, `DiscardVariant` ops by calling the variant_ops functions. This is the one authoritative projection path — view, export, undo/redo, and resume all flow through it.

- [ ] **Step 8: Extend apply() in session.rs**

Extend `apply()` to validate the new op types (item exists, hash exists, etc.) before adding to the op journal. Validation-only — no state mutation beyond appending to `ops`.

- [ ] **Step 9: Run tests to verify they pass**

Run: `cargo test -p inspectah-refine --test fleet_variant_ops_test -- --nocapture`
Expected: PASS

- [ ] **Step 10: Run full suite**

Run: `cargo test -p inspectah-refine -- --nocapture`
Expected: All pass

- [ ] **Step 11: Commit**

```bash
git add inspectah-refine/src/fleet/variant_ops.rs inspectah-refine/src/fleet/mod.rs inspectah-refine/src/session.rs inspectah-refine/tests/fleet_variant_ops_test.rs
git commit -m "feat(refine): implement variant ops via projection path with item-scoped variant pool"
```

---

## Task 7: Auto-Save Persistence

**Files:**
- Create: `inspectah-refine/src/autosave.rs`
- Modify: `inspectah-refine/src/lib.rs`
- Test: `inspectah-refine/tests/autosave_test.rs`

- [ ] **Step 1: Write failing tests**

Create `inspectah-refine/tests/autosave_test.rs`:

```rust
use inspectah_refine::autosave::{SessionState, save_session, load_session, session_file_path, compute_tarball_hash};
use inspectah_refine::types::ContentHash;
use std::path::PathBuf;

#[test]
fn session_file_path_strips_tar_gz() {
    let p = session_file_path(&PathBuf::from("/data/fleet-web-2026-05-20.tar.gz"));
    assert_eq!(p.file_name().unwrap(), ".inspectah-session-fleet-web-2026-05-20.json");
    assert_eq!(p.parent().unwrap(), std::path::Path::new("/data"));
}

#[test]
fn session_file_path_strips_tgz() {
    let p = session_file_path(&PathBuf::from("/tmp/fleet.tgz"));
    assert_eq!(p.file_name().unwrap(), ".inspectah-session-fleet.json");
}

#[test]
fn session_state_serde_roundtrip() {
    let state = SessionState {
        schema_version: 1,
        tarball_path: PathBuf::from("/tmp/test.tar.gz"),
        tarball_hash: ContentHash::from_content(b"tarball"),
        ops: vec![],
        cursor: 0,
        saved_at: "2026-05-20T00:00:00Z".into(),
    };
    let json = serde_json::to_string_pretty(&state).unwrap();
    let parsed: SessionState = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.schema_version, 1);
    assert_eq!(parsed.cursor, 0);
}

#[test]
fn atomic_save_and_load() {
    let dir = tempfile::tempdir().unwrap();
    let tarball = dir.path().join("test.tar.gz");
    std::fs::write(&tarball, b"fake tarball").unwrap();
    let state = SessionState {
        schema_version: 1,
        tarball_path: tarball.clone(),
        tarball_hash: ContentHash::from_content(b"fake tarball"),
        ops: vec![],
        cursor: 0,
        saved_at: "2026-05-20T00:00:00Z".into(),
    };
    save_session(&state, &tarball).unwrap();
    let loaded = load_session(&tarball).unwrap();
    assert!(loaded.is_some());
    assert_eq!(loaded.unwrap().cursor, 0);
}

#[test]
fn load_returns_none_when_no_session_file() {
    let dir = tempfile::tempdir().unwrap();
    let tarball = dir.path().join("nosession.tar.gz");
    assert!(load_session(&tarball).unwrap().is_none());
}

#[test]
fn rejects_unknown_schema_version() {
    let dir = tempfile::tempdir().unwrap();
    let session_path = dir.path().join(".inspectah-session-test.json");
    std::fs::write(&session_path, r#"{"schema_version":99,"tarball_path":"/tmp/x","tarball_hash":"a","ops":[],"cursor":0,"saved_at":"x"}"#).unwrap();
    let tarball = dir.path().join("test.tar.gz");
    let result = load_session(&tarball);
    assert!(result.is_err());
}

#[test]
fn compute_tarball_hash_produces_valid_hash() {
    let dir = tempfile::tempdir().unwrap();
    let tarball = dir.path().join("test.tar.gz");
    std::fs::write(&tarball, b"content").unwrap();
    let hash = compute_tarball_hash(&tarball).unwrap();
    assert_eq!(hash.as_str().len(), 64);
}

#[test]
fn stale_detection_different_hash() {
    let dir = tempfile::tempdir().unwrap();
    let tarball = dir.path().join("test.tar.gz");
    std::fs::write(&tarball, b"original").unwrap();
    let state = SessionState {
        schema_version: 1,
        tarball_path: tarball.clone(),
        tarball_hash: ContentHash::from_content(b"original"),
        ops: vec![],
        cursor: 0,
        saved_at: "2026-05-20T00:00:00Z".into(),
    };
    save_session(&state, &tarball).unwrap();
    std::fs::write(&tarball, b"modified").unwrap();
    let loaded = load_session(&tarball).unwrap().unwrap();
    let current_hash = compute_tarball_hash(&tarball).unwrap();
    assert_ne!(loaded.tarball_hash, current_hash); // stale!
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-refine --test autosave_test -- --nocapture`
Expected: FAIL

- [ ] **Step 3: Implement autosave.rs**

Create `inspectah-refine/src/autosave.rs` with `SessionState`, `session_file_path()`, `save_session()` (atomic write-then-rename), `load_session()` (with schema version check), `compute_tarball_hash()`.

Register in `inspectah-refine/src/lib.rs`:
```rust
pub mod autosave;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p inspectah-refine --test autosave_test -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add inspectah-refine/src/autosave.rs inspectah-refine/src/lib.rs inspectah-refine/tests/autosave_test.rs
git commit -m "feat(refine): add session persistence with atomic save, load, and stale detection"
```

---

## Task 8: Auto-Save Integration + Resume + CLI Wiring

**Files:**
- Modify: `inspectah-refine/src/session.rs` (auto-save on mutations, `resume_from()`, durability flag, `pending_changes()` extension)
- Modify: `inspectah-cli/src/commands/refine.rs` (resume prompt, `--fresh` flag)
- Test: `inspectah-refine/tests/autosave_integration_test.rs`

`from_tarball()` is NOT modified — it stays fresh-only. Session
discovery and reopen live in `RefineSession::resume_from()`. The CLI
calls `resume_from()` first; if no session or `--fresh`, it falls back
to `from_tarball()`.

- [ ] **Step 1: Write failing tests for auto-save on apply**

Create `inspectah-refine/tests/autosave_integration_test.rs`:

```rust
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_refine::autosave::{session_file_path, load_session};
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::{PackageTarget, RefinementOp};

fn make_session_with_tarball_path() -> (RefineSession, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let tarball = dir.path().join("test.tar.gz");
    std::fs::write(&tarball, b"fake").unwrap();
    let snap = InspectionSnapshot::default();
    let session = RefineSession::new_with_tarball(snap, tarball);
    (session, dir)
}

#[test]
fn session_file_created_after_first_apply() {
    let (mut session, dir) = make_session_with_tarball_path();
    let tarball = dir.path().join("test.tar.gz");
    let op = RefinementOp::ExcludePackage(PackageTarget {
        name: "httpd".into(), arch: "x86_64".into(),
    });
    session.apply(op).unwrap();
    let session_path = session_file_path(&tarball);
    assert!(session_path.exists(), "session file must exist after apply");
}

#[test]
fn session_file_updated_after_undo() {
    let (mut session, dir) = make_session_with_tarball_path();
    let tarball = dir.path().join("test.tar.gz");
    let op = RefinementOp::ExcludePackage(PackageTarget {
        name: "httpd".into(), arch: "x86_64".into(),
    });
    session.apply(op).unwrap();
    let before = load_session(&tarball).unwrap().unwrap();
    assert_eq!(before.cursor, 1);
    session.undo().unwrap();
    let after = load_session(&tarball).unwrap().unwrap();
    assert_eq!(after.cursor, 0, "cursor must be 0 after undo");
}

#[test]
fn session_file_updated_after_redo() {
    let (mut session, dir) = make_session_with_tarball_path();
    let tarball = dir.path().join("test.tar.gz");
    let op = RefinementOp::ExcludePackage(PackageTarget {
        name: "httpd".into(), arch: "x86_64".into(),
    });
    session.apply(op).unwrap();
    session.undo().unwrap();
    session.redo().unwrap();
    let state = load_session(&tarball).unwrap().unwrap();
    assert_eq!(state.cursor, 1, "cursor must be 1 after redo");
}

#[test]
fn replay_from_session_reconstructs_cursor() {
    let (mut session, dir) = make_session_with_tarball_path();
    let tarball = dir.path().join("test.tar.gz");
    let op1 = RefinementOp::ExcludePackage(PackageTarget {
        name: "httpd".into(), arch: "x86_64".into(),
    });
    let op2 = RefinementOp::ExcludePackage(PackageTarget {
        name: "nginx".into(), arch: "x86_64".into(),
    });
    session.apply(op1).unwrap();
    session.apply(op2).unwrap();
    session.undo().unwrap(); // cursor at 1, not 2
    drop(session);
    // Reload and replay
    let state = load_session(&tarball).unwrap().unwrap();
    assert_eq!(state.ops.len(), 2);
    assert_eq!(state.cursor, 1, "persisted cursor reflects undo");
}

#[test]
fn resumed_session_via_real_loader_reconstructs_visible_state() {
    // This test proves the full reopen path: tarball → from_tarball →
    // session discovery → replay → visible state matches pre-close.
    let dir = tempfile::tempdir().unwrap();
    let tarball = dir.path().join("test.tar.gz");

    // Build a real tarball with a fleet snapshot containing packages
    let snap = make_fleet_snapshot_with_packages(3);
    write_test_tarball(&tarball, &snap);

    // Session 1: apply two ops, undo one, close
    let mut session1 = RefineSession::new_with_tarball(
        load_snapshot_from_tarball(&tarball),
        tarball.clone(),
    );
    let op1 = RefinementOp::ExcludePackage(PackageTarget {
        name: "httpd".into(), arch: "x86_64".into(),
    });
    let op2 = RefinementOp::ExcludePackage(PackageTarget {
        name: "nginx".into(), arch: "x86_64".into(),
    });
    session1.apply(op1).unwrap();
    session1.apply(op2).unwrap();
    session1.undo().unwrap();
    // Visible state: op1 applied, op2 undone. Redo tail has op2.
    let view_before = session1.snapshot_projected();
    let httpd_excluded_before = !view_before.rpm.as_ref().unwrap()
        .packages_added.iter()
        .any(|p| p.name == "httpd" && p.include);
    let nginx_included_before = view_before.rpm.as_ref().unwrap()
        .packages_added.iter()
        .any(|p| p.name == "nginx" && p.include);
    assert!(httpd_excluded_before, "httpd should be excluded");
    assert!(nginx_included_before, "nginx should be included (undone)");
    drop(session1);

    // Session 2: reopen through the real loader path
    let session2 = RefineSession::resume_from(&tarball).unwrap()
        .expect("session file should exist");
    let view_after = session2.snapshot_projected();
    let httpd_excluded_after = !view_after.rpm.as_ref().unwrap()
        .packages_added.iter()
        .any(|p| p.name == "httpd" && p.include);
    let nginx_included_after = view_after.rpm.as_ref().unwrap()
        .packages_added.iter()
        .any(|p| p.name == "nginx" && p.include);
    assert!(httpd_excluded_after, "httpd still excluded after resume");
    assert!(nginx_included_after, "nginx still included after resume");

    // Redo tail survives: redo should re-exclude nginx
    let mut session2 = session2;
    session2.redo().unwrap();
    let view_redo = session2.snapshot_projected();
    let nginx_excluded_redo = !view_redo.rpm.as_ref().unwrap()
        .packages_added.iter()
        .any(|p| p.name == "nginx" && p.include);
    assert!(nginx_excluded_redo, "redo restores op2 — nginx excluded");
}
```

- [ ] **Step 2: Implement auto-save on every cursor-changing mutation**

In `session.rs`:
- Add `tarball_path: Option<PathBuf>` and `durability_degraded: bool` to `RefineSession`
- Add a private `fn try_autosave(&mut self)` that serializes `SessionState` (ops, cursor, tarball_path, tarball_hash, saved_at) and calls `save_session()`. On permanent failure (`EROFS`/`EACCES`), set `durability_degraded` and skip future saves. On transient failure, log and retry next time.
- Call `try_autosave()` after every cursor-changing mutation:
  - `apply()` — appends to ops and advances cursor
  - `undo()` — decrements cursor
  - `redo()` — increments cursor
- This ensures the persisted cursor always matches what the user last saw. Without this, undo/redo would change visible state without saving, and resume could reopen a state the user already backed out of.
- Add `pub fn durability_degraded(&self) -> bool` accessor

- [ ] **Step 3: Add `resume_from()` to RefineSession**

In `session.rs`, add `pub fn resume_from(tarball: &Path) -> Result<Option<Self>, RefineError>`:
- Check for adjacent session file via `session_file_path()`
- If no session file: return `Ok(None)`
- Load the session state, check `tarball_hash` for staleness
- Load snapshot from tarball via the existing `load_for_refine()` pipeline
- Construct `RefineSession::new_with_tarball(snapshot, tarball)`, then replay ops up to `cursor`
- Return `Ok(Some(session))`

This is the single engine-owned reopen entry point. `from_tarball()` remains unchanged — it always starts a fresh session.

- [ ] **Step 4: Add resume/fresh prompt to CLI**

In `inspectah-cli/src/commands/refine.rs`:
- Add `--fresh` flag to the CLI args
- Before calling `from_tarball()`, check `RefineSession::resume_from()`:
  - If `Ok(Some(session))` and NOT `--fresh`: prompt `[r] Resume  [f] Fresh start  [q] Quit`
  - If `--fresh`: confirm destructive discard, delete session file, call `from_tarball()`
  - If stale tarball (hash mismatch): warn and default to fresh start
  - If `Ok(None)`: no session file, call `from_tarball()` as today

- [ ] **Step 5: Extend `pending_changes()` for variant ops**

In `session.rs`, extend `pending_changes()` / `is_dirty()` to account for variant-only mutations. Currently `is_dirty` only checks include/exclude and repo deltas. Add:
- Count of `SelectVariant` ops where the selection differs from aggregate default
- Count of `EditVariant` ops (any user-created variant = dirty)
- Count of `DiscardVariant` ops
- Add `variants_changed: usize` to `ChangesSummary`
- `is_dirty` becomes true when any of the existing conditions OR `variants_changed > 0`

This ensures a session that only does variant ops still shows as dirty in `/api/changes` and the CLI exit warning.

- [ ] **Step 5: Run tests**

Run: `cargo test -p inspectah-refine --test autosave_integration_test -- --nocapture`
Expected: PASS

- [ ] **Step 6: Run full suite**

Run: `cargo test -p inspectah-refine -- --nocapture`
Run: `cargo test -p inspectah-cli -- --nocapture`
Expected: All pass

- [ ] **Step 7: Commit**

```bash
git add inspectah-refine/src/session.rs inspectah-refine/src/tarball.rs inspectah-cli/src/commands/refine.rs inspectah-refine/tests/autosave_integration_test.rs
git commit -m "feat(refine): wire auto-save into session lifecycle with resume, --fresh, and CLI prompt"
```

---

## Task 9: Variant-Aware Export

**Files:**
- Modify: `inspectah-refine/src/session.rs` (extend `render_refine_export()` at line ~758)
- Test: `inspectah-refine/tests/fleet_export_test.rs`

**Key design point:** Export uses the existing `render_refine_export()` in `session.rs`. Fleet refine extends this with one additive step: materializing `fleet/variants/` from the projected snapshot's variant data.

- [ ] **Step 1: Write failing tests**

Create `inspectah-refine/tests/fleet_export_test.rs`:

```rust
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::config::ConfigFileEntry;
use inspectah_core::types::fleet::{FleetPrevalence, FleetSnapshotMeta, VariantSelection};
use inspectah_refine::session::{RefineSession, render_refine_export};
use inspectah_refine::types::{ContentHash, ItemId, RefinementOp};
use std::collections::BTreeMap;

fn unpack_tarball(path: &std::path::Path) -> std::path::PathBuf {
    let dir = tempfile::tempdir().unwrap();
    let file = std::fs::File::open(path).unwrap();
    let gz = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);
    archive.unpack(dir.path()).unwrap();
    // flatten if prefixed
    let entries: Vec<_> = std::fs::read_dir(dir.path()).unwrap()
        .filter_map(|e| e.ok()).collect();
    if entries.len() == 1 && entries[0].file_type().unwrap().is_dir() {
        entries[0].path()
    } else {
        dir.into_path()
    }
}

fn make_fleet_snap_with_config_variants() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::default();
    snap.fleet_meta = Some(FleetSnapshotMeta {
        label: "test".into(), host_count: 3,
        hostnames: vec!["h1".into(), "h2".into(), "h3".into()],
        merged_at: "2026-05-20T00:00:00Z".into(),
        baseline_provisional: false,
        section_host_counts: BTreeMap::new(),
    });
    // Two config variants for /etc/nginx/nginx.conf
    let mut cfg = snap.config.get_or_insert_default();
    cfg.files.push(ConfigFileEntry {
        path: "/etc/nginx/nginx.conf".into(),
        content: "variant_a_content".into(),
        include: true,
        variant_selection: VariantSelection::Selected,
        fleet: Some(FleetPrevalence { count: 2, total: 3, hosts: vec!["h1".into(), "h2".into()] }),
        ..Default::default()
    });
    cfg.files.push(ConfigFileEntry {
        path: "/etc/nginx/nginx.conf".into(),
        content: "variant_b_content".into(),
        include: true,
        variant_selection: VariantSelection::Alternative,
        fleet: Some(FleetPrevalence { count: 1, total: 3, hosts: vec!["h3".into()] }),
        ..Default::default()
    });
    snap
}

#[test]
fn fleet_export_includes_fleet_variants_dir() {
    let snap = make_fleet_snap_with_config_variants();
    let dir = tempfile::tempdir().unwrap();
    let export_path = dir.path().join("export.tar.gz");
    render_refine_export(&snap, &export_path).unwrap();
    let unpacked = unpack_tarball(&export_path);
    assert!(unpacked.join("fleet/variants").exists(),
        "fleet/variants/ directory must exist in export");
}

#[test]
fn fleet_export_selected_variant_at_config_path() {
    let snap = make_fleet_snap_with_config_variants();
    let dir = tempfile::tempdir().unwrap();
    let export_path = dir.path().join("export.tar.gz");
    render_refine_export(&snap, &export_path).unwrap();
    let unpacked = unpack_tarball(&export_path);
    let snap_json = std::fs::read_to_string(unpacked.join("inspection-snapshot.json")).unwrap();
    let reloaded: InspectionSnapshot = serde_json::from_str(&snap_json).unwrap();
    let selected = reloaded.config.unwrap().files.iter()
        .find(|c| c.path == "/etc/nginx/nginx.conf" && c.variant_selection == VariantSelection::Selected)
        .unwrap();
    assert_eq!(selected.content, "variant_a_content");
}

#[test]
fn fleet_export_alternatives_in_fleet_variants() {
    let snap = make_fleet_snap_with_config_variants();
    let dir = tempfile::tempdir().unwrap();
    let export_path = dir.path().join("export.tar.gz");
    render_refine_export(&snap, &export_path).unwrap();
    let unpacked = unpack_tarball(&export_path);
    let variants_dir = unpacked.join("fleet/variants");
    let variant_files: Vec<_> = walkdir::WalkDir::new(&variants_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .collect();
    assert!(!variant_files.is_empty(),
        "alternative variant content must be materialized");
}

#[test]
fn single_host_export_has_no_fleet_variants() {
    let snap = InspectionSnapshot::default(); // no fleet_meta
    let dir = tempfile::tempdir().unwrap();
    let export_path = dir.path().join("export.tar.gz");
    render_refine_export(&snap, &export_path).unwrap();
    let unpacked = unpack_tarball(&export_path);
    assert!(!unpacked.join("fleet/variants").exists(),
        "single-host export must NOT have fleet/variants/");
}

#[test]
fn fleet_of_two_export_preserves_variants() {
    let mut snap = make_fleet_snap_with_config_variants();
    snap.fleet_meta.as_mut().unwrap().host_count = 2;
    snap.fleet_meta.as_mut().unwrap().hostnames = vec!["h1".into(), "h2".into()];
    let dir = tempfile::tempdir().unwrap();
    let export_path = dir.path().join("export.tar.gz");
    render_refine_export(&snap, &export_path).unwrap();
    let unpacked = unpack_tarball(&export_path);
    assert!(unpacked.join("fleet/variants").exists(),
        "fleet-of-2 still gets fleet/variants/");
}

#[test]
fn export_then_reimport_via_real_loader_preserves_selection() {
    let snap = make_fleet_snap_with_config_variants();
    let dir = tempfile::tempdir().unwrap();
    let export_path = dir.path().join("export.tar.gz");
    render_refine_export(&snap, &export_path).unwrap();
    // Reimport through the real loader, not raw JSON parse
    let reimported = inspectah_refine::tarball::from_tarball(&export_path).unwrap();
    let view = reimported.snapshot_projected();
    let configs = &view.config.unwrap().files;
    let selected_count = configs.iter()
        .filter(|c| c.path == "/etc/nginx/nginx.conf" && c.variant_selection == VariantSelection::Selected)
        .count();
    let alt_count = configs.iter()
        .filter(|c| c.path == "/etc/nginx/nginx.conf" && c.variant_selection == VariantSelection::Alternative)
        .count();
    assert_eq!(selected_count, 1, "exactly one Selected variant after reimport");
    assert_eq!(alt_count, 1, "exactly one Alternative variant after reimport");
    // Verify fleet mode detected on reimport
    assert!(reimported.fleet_context().is_some(), "reimported fleet tarball should detect fleet mode");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-refine --test fleet_export_test -- --nocapture`
Expected: FAIL

- [ ] **Step 3: Extend render_refine_export()**

In `session.rs` at `render_refine_export()` (~line 758):
- After the existing export pipeline produces its file set, check if the projected snapshot has fleet variant data (items with `VariantSelection::Alternative`)
- If yes, materialize `fleet/variants/<path>/<hash-prefix>` files with alternative content
- User-created variant content (from the projection's working `user_variants`) is merged into the snapshot before export — after export, it's indistinguishable from host-sourced content
- Single-host snapshots skip this step entirely

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p inspectah-refine --test fleet_export_test -- --nocapture`
Expected: PASS

- [ ] **Step 5: Run existing export contract test**

Run: `cargo test -p inspectah-refine --test export_contract_test -- --nocapture`
Expected: PASS (single-host export unchanged)

- [ ] **Step 6: Commit**

```bash
git add inspectah-refine/src/session.rs inspectah-refine/tests/fleet_export_test.rs
git commit -m "feat(refine): variant-aware export extending render_refine_export with fleet/variants/"
```

---

## Task 10: Variant Summary + End-to-End Integration

**Files:**
- Modify: `inspectah-refine/src/fleet/mod.rs`
- Create: `inspectah-refine/tests/fleet_e2e_test.rs`

- [ ] **Step 1: Write failing tests for variant_summary**

```rust
use inspectah_refine::fleet::variant_summary;
use inspectah_refine::session::RefineSession;

#[test]
fn summary_counts_paths_with_variants() {
    // Create fleet session with 3 config files, 2 have variants
    let session = make_fleet_session_with_variants();
    let summary = variant_summary(&session).unwrap();
    assert_eq!(summary.paths_with_variants, 2);
}

#[test]
fn summary_reports_host_split_sorted_descending() {
    // Config with 3 variants: 7 hosts, 2 hosts, 1 host
    let session = make_fleet_session_with_split_variants();
    let summary = variant_summary(&session).unwrap();
    let info = &summary.variant_distribution["/etc/nginx/nginx.conf"];
    assert_eq!(info.host_split, vec![7, 2, 1]);
}

#[test]
fn summary_none_for_single_host() {
    let session = RefineSession::new(InspectionSnapshot::default());
    assert!(variant_summary(&session).is_none());
}
```

- [ ] **Step 2: Implement variant_summary**

In `inspectah-refine/src/fleet/mod.rs`:

```rust
pub fn variant_summary(session: &RefineSession) -> Option<VariantSummary>
```

Returns `None` for non-fleet sessions. Iterates variant-capable items in the projected snapshot, counts content hashes per path, computes host splits from `FleetPrevalence`.

- [ ] **Step 3: Write end-to-end lifecycle test**

Create `inspectah-refine/tests/fleet_e2e_test.rs`:

```rust
#[test]
fn fleet_refine_full_lifecycle() {
    // 1. Build fleet snapshot: 5 hosts, configs with variants, mixed prevalence
    let snap = build_test_fleet_snapshot(5);

    // 2. Init session — verify fleet mode, zones computed
    let mut session = RefineSession::new(snap);
    let ctx = session.fleet_context().unwrap();
    assert!(ctx.zones_active);
    assert_eq!(ctx.total_hosts, 5);

    // 3. Check zone classification
    let zone = ctx.zones.get(&ItemId::Config { path: "/etc/nginx/nginx.conf".into() });
    assert!(zone.is_some());

    // 4. SelectVariant — pick variant B
    let variant_b_hash = ContentHash::from_content(b"variant B content");
    session.apply(RefinementOp::SelectVariant {
        item_id: ItemId::Config { path: "/etc/nginx/nginx.conf".into() },
        target: variant_b_hash.clone(),
    }).unwrap();

    // 5. EditVariant — create modified version
    session.apply(RefinementOp::EditVariant {
        item_id: ItemId::Config { path: "/etc/nginx/nginx.conf".into() },
        content: "edited content".into(),
        based_on: Some(variant_b_hash),
    }).unwrap();

    // 6. Compute diff between original and edited
    let edited_hash = ContentHash::from_content(b"edited content");
    let diff = compute_diff("variant B content", "edited content", 3).unwrap();
    assert!(!diff.hunks.is_empty());

    // 7. Variant summary
    let summary = variant_summary(&session).unwrap();
    assert!(summary.paths_with_variants > 0);

    // 8. Undo edit — edited variant removed
    session.undo().unwrap();

    // 9. Export — verify tarball has fleet/variants/ and reimports cleanly
    let export_dir = tempfile::tempdir().unwrap();
    let export_path = export_dir.path().join("export.tar.gz");
    let projected = session.snapshot_projected();
    render_refine_export(&projected, &export_path).unwrap();
    let unpacked = unpack_tarball(&export_path);
    assert!(unpacked.join("fleet/variants").exists(),
        "exported fleet tarball must contain fleet/variants/");
    assert!(unpacked.join("inspection-snapshot.json").exists(),
        "exported tarball must contain snapshot");

    // 10. Reimport via real loader — verify fleet mode and selection state
    let reimported = inspectah_refine::tarball::from_tarball(&export_path).unwrap();
    assert!(reimported.fleet_context().is_some(),
        "reimported fleet tarball should detect fleet mode");
    let reimport_view = reimported.snapshot_projected();
    let reimport_configs = &reimport_view.config.unwrap().files;
    assert!(reimport_configs.iter().any(|c|
        c.path == "/etc/nginx/nginx.conf"
        && c.variant_selection == VariantSelection::Selected),
        "reimported snapshot preserves variant selection");
}
```

- [ ] **Step 4: Run all tests**

Run: `cargo test -p inspectah-refine -- --nocapture`
Run: `cargo test -p inspectah-core -- --nocapture`
Expected: All pass

- [ ] **Step 5: Commit**

```bash
git add inspectah-refine/src/fleet/mod.rs inspectah-refine/tests/fleet_e2e_test.rs
git commit -m "feat(refine): add variant summary and fleet refine end-to-end integration tests"
```

---

## Scope Notes

### Compose EditVariant deferred
Compose files store variant data as a structured `images` list, not raw text. `SelectVariant` works (pick between existing host-sourced compose variants). `EditVariant` and diff on compose files are deferred — the serialize/parse/validate seam for the structured carrier is not defined in this plan. Compose items get zone classification, prevalence badges, and variant selection only.

### Variant-capable types in v1
Full variant ops (Select/Edit/Discard/Diff) apply to: Config, DropIn, Quadlet (all text-content based with `content` field). Compose gets SelectVariant only (structured `images` carrier — Edit/Diff deferred). Repo is not variant-capable at all. This matches the approved spec's Variant-Capable Types table.

### sha2 dependency
`sha2` is already a dependency of `inspectah-core` (used by `FleetMergeable::content_variant_key()`). `inspectah-refine` depends on `inspectah-core`, so `sha2` is available transitively. Task 2 adds it as a direct dependency of `inspectah-refine` for `ContentHash::from_content()`. `similar` is a new direct dependency added in Task 2.

---

## Self-Review Checklist

### Spec Coverage
| Spec Section | Task |
|-------------|------|
| Zone Classification | 1 |
| ContentHash + ItemId | 2 |
| New RefinementOp variants | 2 |
| RefineMode + FleetContext | 3 |
| Fleet-of-2 zones suppressed, single-host is SingleHost | 3 |
| FleetAttention + AttentionScore | 4 |
| Fleet attention scoring | 4 |
| Diff engine | 5 |
| SelectVariant | 6 |
| EditVariant + convergence | 6 |
| DiscardVariant + fallback | 6 |
| Variant state via projection path | 6 |
| Auto-save persistence | 7 |
| Autosave failure policy | 8 |
| Autosave on undo/redo (cursor-changing mutations) | 8 |
| Session resume + replay | 8 |
| CLI --fresh flag | 8 |
| Session sidecar discovery in from_tarball() | 8 |
| Variant-aware export via render_refine_export() | 9 |
| fleet/variants/ materialization | 9 |
| Variant summary | 10 |
| Content disjointness | 6 |

### Not Covered (by design)
- HTTP handler wiring (Spec 2)
- JSON wire format (Spec 2)
- UI rendering (Spec 2)
- Compose EditVariant/diff (deferred — structured carrier)

### Review History

#### Round 1
Panel: Tang, Collins, Thorn, Lens. Verdict: request-changes.

Fixes applied:
1. Resume wired to real entrypoints (`from_tarball()` + CLI `refine.rs`)
2. Export re-anchored to `render_refine_export()` in `session.rs`
3. Variant state flows through `snapshot_projected()`, no apply-time side state
4. Fleet-of-2 stays `RefineMode::Fleet` with `zones_active: false`; single-host is `SingleHost`
5. Placeholder tests replaced with executable assertions
6. Compose narrowed to SelectVariant only
7. `ContentHash` derives `Ord` for `BTreeMap` use in batch diff
8. `SessionState` lives in `autosave.rs` (consistent file ownership)

#### Round 2
Panel: Collins approve, Tang approve-with-nits, Thorn request-changes,
Lens request-changes.

Fixes applied:
1. **Autosave on undo/redo (must-fix):** `try_autosave()` called after
   every cursor-changing mutation (apply, undo, redo), not just apply.
   Persisted cursor always matches the user's last visible state.
   Tests explicitly verify cursor after undo and redo.
2. **Compose spec alignment (must-fix):** Spec updated to match plan —
   Compose is SelectVariant-only in v1, EditVariant/diff deferred.
   No plan/spec drift.
3. **Small-fleet spec alignment (should-fix):** Spec updated —
   FleetContext gains `zones_active: bool`. Fleet-of-2 is
   `RefineMode::Fleet` with `zones_active: false`. Single-host
   (no FleetSnapshotMeta) is `RefineMode::SingleHost`. Fleet-of-1
   removed as unrealistic. Plan and spec aligned.
4. **Placeholder tests replaced (should-fix):** Task 8 tests now have
   concrete assertions for session-file-after-apply, cursor-after-undo,
   cursor-after-redo, and replay-reconstructs-cursor.
5. **Tree fidelity (should-fix):** `project_snapshot()` → public
   `snapshot_projected()`. `sha2` dependency documented (transitive
   from core, added direct for ContentHash). `from_tarball()` callers
   noted.

#### Round 3
Panel: Collins approve, Tang approve-with-nits, Thorn request-changes,
Lens approve.

Fixes applied:
1. **Real reopen-path resume proof (must-fix):** Task 8 now includes
   `resumed_session_via_real_loader_reconstructs_visible_state()` —
   full test proving: apply two ops → undo one → close → reopen via
   `resume_from()` → verify projected snapshot matches pre-close state
   (httpd excluded, nginx included) → verify redo tail intact (redo
   re-excludes nginx). This is the concrete proof Thorn blocked on.
2. **Spec stale wording scrub (should-fix):** module layout removed
   nonexistent `export.rs`, autosave trigger updated to "every
   cursor-changing mutation" (not just apply), `AttentionScore`
   comment corrected to "single-host = no FleetSnapshotMeta" /
   "fleet-of-2+ = Fleet variant".
3. **Task 9 export tests replaced (should-fix):** all comment
   skeletons replaced with executable assertions — fleet/variants
   exists, selected variant at config path, alternatives materialized,
   single-host has no fleet/variants, fleet-of-2 preserves variants,
   export-reimport preserves selection state.
4. **Fleet-of-1 removed as unrealistic** from both plan and spec.
