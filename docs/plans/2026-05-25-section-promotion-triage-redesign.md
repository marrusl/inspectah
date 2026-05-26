# Section Promotion & Triage Model Redesign — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the attention-level triage system with action-oriented buckets (Baseline/Site/Investigate for single-host, Investigate/Divergent/Partial/Universal for fleet) and promote services, quadlets, flatpak provisioning, sysctls, and tuned profiles from read-only Reference to toggleable Review sections.

**Architecture:** Four sequential phases. Phase 0 replaces the core type system and classification logic. Phase 1 promotes services (proving ground for the pattern, includes parent-child drop-in toggles). Phase 2 promotes quadlets + flatpak. Phase 3 promotes sysctls + tuned. Each phase is independently compilable and testable. Tang owns Rust backend; Kit owns frontend. Frontend work in each phase can begin once the corresponding backend API is stable.

**Tech Stack:** Rust (inspectah-refine, inspectah-web), TypeScript/React (inspectah-web/ui), PatternFly 6

**Spec:** `docs/specs/proposed/2026-05-25-section-promotion-triage-redesign.md`

**UI/UX guidelines applied:** ui-ux-pro-max (touch targets 44x44px, 4.5:1 contrast, focus-visible rings, keyboard nav matches visual order, 150-300ms transitions, no emoji icons, cursor-pointer on clickables)

**Baseline prerequisite:** The current repo only captures baseline data for packages (via `BaselineData`). Non-package baseline comparison (services, sysctls, tuned) requires extending the baseline data model. Until that extension ships, non-package items that would be Baseline are classified as Site (safe default — everything starts enabled). The classification functions are written to accept `Option<baseline>` so they degrade gracefully. Baseline-aware classification for non-package sections is a follow-on task, not a blocker for this plan.

---

## Phase 0: Type System Foundation

**Owner:** Tang (Rust)
**Blocks:** All subsequent phases

### Task 1: Define new triage types ✅ DONE (b8b565f)

**Files:**
- Modify: `inspectah-refine/src/types.rs`
- Test: `inspectah-refine/src/types.rs` (inline `#[cfg(test)]`)

- [ ] **Step 1: Write serde round-trip tests for new types**

```rust
#[cfg(test)]
mod triage_tests {
    use super::*;

    #[test]
    fn triage_bucket_serde_roundtrip() {
        let buckets = vec![TriageBucket::Baseline, TriageBucket::Site, TriageBucket::Investigate];
        for b in buckets {
            let json = serde_json::to_string(&b).unwrap();
            let back: TriageBucket = serde_json::from_str(&json).unwrap();
            assert_eq!(b, back);
        }
    }

    #[test]
    fn fleet_triage_serde_roundtrip() {
        let ft = FleetTriage {
            bucket: FleetBucket::Divergent,
            prevalence: Prevalence { count: 42, total: 50 },
        };
        let json = serde_json::to_string(&ft).unwrap();
        let back: FleetTriage = serde_json::from_str(&json).unwrap();
        assert_eq!(ft.bucket, back.bucket);
        assert_eq!(ft.prevalence.count, 42);
        assert_eq!(ft.prevalence.total, 50);
    }

    #[test]
    fn triage_tag_with_annotations() {
        let tag = TriageTag {
            triage: Triage::SingleHost(TriageBucket::Investigate),
            primary_reason: TriageReason::PackageLocalInstall,
            annotations: vec![TriageAnnotation::SensitivePath],
        };
        let json = serde_json::to_string(&tag).unwrap();
        assert!(json.contains("investigate"));
        assert!(json.contains("sensitive_path"));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-refine triage_tests -- --nocapture`
Expected: FAIL — types not defined yet

- [ ] **Step 3: Add type definitions**

Add to `inspectah-refine/src/types.rs` (keep existing `AttentionLevel` temporarily — it will be removed in Task 5):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriageBucket {
    Baseline,
    Site,
    Investigate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FleetBucket {
    Investigate,
    Divergent,
    Partial,
    Universal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prevalence {
    pub count: u32,
    pub total: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetTriage {
    pub bucket: FleetBucket,
    pub prevalence: Prevalence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode")]
pub enum Triage {
    #[serde(rename = "single_host")]
    SingleHost(TriageBucket),
    #[serde(rename = "fleet")]
    Fleet(FleetTriage),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriageAnnotation {
    SensitivePath,
    FirstBootProvisioned,
    RequiresProjectedPackage { name: String },
    RuntimeOnlyObservation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriageReason {
    // Packages
    PackageBaselineMatch,
    PackageUserAdded,
    PackageVersionChanged,
    PackageProvenanceUnavailable,
    PackageLocalInstall,
    PackageNoRepoSource,
    PackageConfigCaptured,
    // Configs
    ConfigDefault,
    ConfigBaselineMatch,
    ConfigModified,
    ConfigUnowned,
    ConfigOrphaned,
    // Services
    ServiceBaselineMatch,
    ServiceNonDefaultState,
    ServiceUnknownOrigin,
    ServiceDropInPresent,
    // Containers
    QuadletUserDeployed,
    QuadletPresentInBaseImage,
    FlatpakProvisionedOnFirstBoot,
    FlatpakIncompleteProvenance,
    // Sysctls
    SysctlBaselineMatch,
    SysctlFileBackedOverride,
    SysctlNoBaseline,
    // Tuned
    TunedBaselineMatch,
    TunedNonDefaultProfile,
    TunedCustomProfile,
    TunedUnusualState,
    // General
    SensitivePath,
    Custom(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TriageTag {
    pub triage: Triage,
    pub primary_reason: TriageReason,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<TriageAnnotation>,
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p inspectah-refine triage_tests -- --nocapture`
Expected: PASS

- [ ] **Step 5: Add TriageReason display formatting**

```rust
impl TriageReason {
    pub fn display_string(&self) -> &'static str {
        match self {
            Self::PackageBaselineMatch => "Matches base image",
            Self::PackageUserAdded => "User-added package",
            Self::PackageVersionChanged => "Version changed from base image",
            Self::PackageProvenanceUnavailable => "Unknown origin \u{2014} no baseline available",
            Self::PackageLocalInstall => "Locally installed RPM \u{2014} not from a repository",
            Self::PackageNoRepoSource => "Unknown origin \u{2014} no repository source",
            Self::PackageConfigCaptured => "Contents captured via config files",
            Self::ConfigDefault => "RPM default \u{2014} unmodified",
            Self::ConfigBaselineMatch => "Matches base image",
            Self::ConfigModified => "Modified from RPM default",
            Self::ConfigUnowned => "Not owned by any installed package",
            Self::ConfigOrphaned => "Orphaned \u{2014} owning package removed",
            Self::ServiceBaselineMatch => "Matches base image service state",
            Self::ServiceNonDefaultState => "Non-default service state",
            Self::ServiceUnknownOrigin => "Service not from any installed RPM",
            Self::ServiceDropInPresent => "Drop-in override present",
            Self::QuadletUserDeployed => "User-deployed container workload",
            Self::QuadletPresentInBaseImage => "Quadlet present in base image",
            Self::FlatpakProvisionedOnFirstBoot => "Flatpak provisioned at first boot",
            Self::FlatpakIncompleteProvenance => "Incomplete provenance for manifest",
            Self::SysctlBaselineMatch => "Matches base image kernel parameter",
            Self::SysctlFileBackedOverride => "Non-default kernel parameter",
            Self::SysctlNoBaseline => "No baseline available for comparison",
            Self::TunedBaselineMatch => "Matches base image tuned profile",
            Self::TunedNonDefaultProfile => "Non-default tuned profile",
            Self::TunedCustomProfile => "Custom profile in /etc/tuned/",
            Self::TunedUnusualState => "Tuned in unusual state",
            Self::SensitivePath => "Security-sensitive path \u{2014} verify before including",
            Self::Custom(s) => "See detail",
        }
    }
}
```

- [ ] **Step 6: Run full crate tests and clippy**

Run: `cargo test -p inspectah-refine && cargo clippy -p inspectah-refine -- -D warnings`
Expected: PASS (existing tests still pass since old types are not yet removed)

- [ ] **Step 7: Commit**

```bash
git add inspectah-refine/src/types.rs
git commit -m "feat(refine): add triage bucket type system

New types: TriageBucket, FleetBucket, FleetTriage, Triage, TriageTag,
TriageReason, TriageAnnotation. Coexists with AttentionLevel temporarily.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 2: Collapse RefinementOp and restructure ItemId ✅ DONE (93107ab)

**Files:**
- Modify: `inspectah-refine/src/types.rs`
- Test: `inspectah-refine/src/types.rs` (inline)

- [ ] **Step 1: Write tests for SetInclude and new ItemId variants**

```rust
#[cfg(test)]
mod refinement_op_tests {
    use super::*;

    #[test]
    fn set_include_package_serde() {
        let op = RefinementOp::SetInclude {
            item_id: ItemId::Package { name: "httpd".into(), arch: "x86_64".into() },
            include: false,
        };
        let json = serde_json::to_string(&op).unwrap();
        assert!(json.contains("SetInclude"));
        assert!(json.contains("httpd"));
        let back: RefinementOp = serde_json::from_str(&json).unwrap();
        assert_eq!(op, back);
    }

    #[test]
    fn set_include_service_serde() {
        let op = RefinementOp::SetInclude {
            item_id: ItemId::Service { unit: "sshd.service".into() },
            include: true,
        };
        let json = serde_json::to_string(&op).unwrap();
        let back: RefinementOp = serde_json::from_str(&json).unwrap();
        assert_eq!(op, back);
    }

    #[test]
    fn set_include_sysctl_serde() {
        let op = RefinementOp::SetInclude {
            item_id: ItemId::Sysctl { key: "vm.swappiness".into() },
            include: true,
        };
        let json = serde_json::to_string(&op).unwrap();
        let back: RefinementOp = serde_json::from_str(&json).unwrap();
        assert_eq!(op, back);
    }

    #[test]
    fn legacy_exclude_package_deserializes() {
        let legacy = r#"{"op":"ExcludePackage","target":{"name":"httpd","arch":"x86_64"}}"#;
        let op: RefinementOp = serde_json::from_str(legacy).unwrap();
        match op {
            RefinementOp::SetInclude { item_id, include } => {
                assert_eq!(item_id, ItemId::Package { name: "httpd".into(), arch: "x86_64".into() });
                assert!(!include);
            }
            _ => panic!("expected SetInclude"),
        }
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-refine refinement_op_tests -- --nocapture`
Expected: FAIL

- [ ] **Step 3: Add new ItemId variants**

Update the `ItemId` enum in `types.rs`. Keep existing variants, add new ones:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "key")]
pub enum ItemId {
    Package { name: String, arch: String },
    Config { path: String },
    Repo { path: String },
    User { username: String },
    // Promoted:
    Service { unit: String },
    ServiceDropIn { unit: String, dropin_path: String },
    Quadlet { path: String },
    Flatpak { app_id: String, remote: String, branch: String },
    Sysctl { key: String },
    TunedSelection { profile: String },
    // Context-only:
    Compose { path: String },
    Fstab { mount_point: String },
    NonRpm { name: String },
}
```

Note: `ItemId::Package` changes from `{ name_arch: String }` to `{ name: String, arch: String }`. This requires updating all callsites that construct `ItemId::Package` — the compiler will find them.

- [ ] **Step 4: Add SetInclude to RefinementOp**

Add `SetInclude` variant. Keep legacy variants temporarily with `#[serde(alias)]` for deserialization compat. The legacy variants will be removed after autosave migration (Task 4).

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", content = "target")]
pub enum RefinementOp {
    SetInclude { item_id: ItemId, include: bool },

    // Legacy — kept for deserialization compat, removed after autosave migration
    ExcludePackage(PackageTarget),
    IncludePackage(PackageTarget),
    ExcludeConfig { path: PathBuf },
    IncludeConfig { path: PathBuf },
    ExcludeRepo { section_id: String },
    IncludeRepo { section_id: String },

    // Non-trivial payloads
    UserStrategy { username: String, strategy: UserContainerfileStrategy },
    UserPassword(UserPasswordOp),
    SelectVariant { item_id: ItemId, target: ContentHash },
    EditVariant { item_id: ItemId, content: String, based_on: Option<ContentHash> },
    DiscardVariant { item_id: ItemId, variant: ContentHash },
}
```

- [ ] **Step 5: Fix all ItemId::Package callsites**

Run: `cargo build -p inspectah-refine 2>&1 | head -50`

The compiler will list every callsite using the old `{ name_arch }` field. Fix each one. Common pattern:

```rust
// Before:
ItemId::Package { name_arch: format!("{}.{}", name, arch) }
// After:
ItemId::Package { name: name.clone(), arch: arch.clone() }
```

- [ ] **Step 6: Run tests**

Run: `cargo test -p inspectah-refine && cargo clippy -p inspectah-refine -- -D warnings`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add inspectah-refine/
git commit -m "feat(refine): collapse RefinementOp to SetInclude, restructure ItemId

SetInclude replaces per-section Exclude*/Include* variants. ItemId::Package
now carries structured name+arch fields. Legacy op variants kept temporarily
for deserialization compat.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 3: Generic ChangesSummary and RefineStats ✅ DONE (b2757db)

**Files:**
- Modify: `inspectah-refine/src/types.rs`
- Modify: `inspectah-refine/src/session.rs` (summary computation)
- Modify: `inspectah-web/src/handlers.rs` (changes endpoint)
- Test: existing tests + inline

- [ ] **Step 1: Define SectionKind, SectionStats, and new summary types**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionKind {
    Package,
    Config,
    Repo,
    User,
    Service,
    Quadlet,
    Flatpak,
    Sysctl,
    Tuned,
    ComposeContext,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SectionStats {
    pub kind: SectionKind,
    pub total: usize,
    pub included: usize,
    pub excluded: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SectionChangeSummary {
    pub kind: SectionKind,
    pub included: Vec<ItemId>,
    pub excluded: Vec<ItemId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChangesSummary {
    pub sections: Vec<SectionChangeSummary>,
    pub variants_changed: usize,
    pub is_dirty: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefineStats {
    pub sections: Vec<SectionStats>,
}
```

- [ ] **Step 2: Update session.rs to compute new summary format**

The `changes_summary()` and `stats()` methods on `RefineSession` need to produce the new types. The old per-field format (`packages_included`, `configs_included`, etc.) is replaced. Update the methods to iterate over section kinds and collect inclusions/exclusions per kind.

- [ ] **Step 3: Update handlers.rs changes endpoint**

The `/api/changes` endpoint in `handlers.rs:411` returns `ChangesSummary`. Update the response serialization to match the new struct.

- [ ] **Step 4: Run full workspace build**

Run: `cargo build --workspace`
Expected: Compiler errors in `inspectah-web` where old ChangesSummary/RefineStats fields are referenced. Fix each callsite.

- [ ] **Step 5: Run tests and clippy**

Run: `cargo test --workspace && cargo clippy --workspace -- -D warnings`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add inspectah-refine/ inspectah-web/
git commit -m "feat(refine): generic ChangesSummary and RefineStats

Replace per-section fields with Vec<SectionChangeSummary> and
Vec<SectionStats>. Scales to any number of sections without struct changes.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 4: Autosave migration (v1 → v2) ✅ DONE (900060c)

**Files:**
- Modify: `inspectah-refine/src/autosave.rs` — loader (`load_session`) and writer (`save_session` at line 51)
- Modify: `inspectah-refine/src/session.rs` — session init must accept both v1 and v2 state
- Test: `inspectah-refine/src/autosave.rs` (inline)

- [ ] **Step 1: Write dual-version load test**

```rust
#[test]
fn v1_autosave_loads_and_migrates() {
    let v1_json = r#"{
        "schema_version": 1,
        "ops": [
            {"op": "ExcludePackage", "target": {"name": "httpd", "arch": "x86_64"}},
            {"op": "IncludeConfig", "target": {"path": "/etc/nginx/nginx.conf"}},
            {"op": "ExcludeRepo", "target": {"section_id": "epel"}}
        ],
        "cursor": 2
    }"#;
    let state = load_session_state(v1_json.as_bytes()).unwrap();
    assert_eq!(state.schema_version, 2); // migrated to v2
    assert_eq!(state.ops.len(), 3);
    match &state.ops[0] {
        RefinementOp::SetInclude { item_id, include } => {
            assert_eq!(*item_id, ItemId::Package { name: "httpd".into(), arch: "x86_64".into() });
            assert!(!include);
        }
        other => panic!("expected SetInclude, got {:?}", other),
    }
    assert_eq!(state.cursor, 2); // cursor preserved
}

#[test]
fn v2_autosave_loads_directly() {
    let v2_json = r#"{
        "schema_version": 2,
        "ops": [
            {"op": "SetInclude", "target": {"item_id": {"kind": "Package", "key": {"name": "httpd", "arch": "x86_64"}}, "include": false}}
        ],
        "cursor": 1
    }"#;
    let state = load_session_state(v2_json.as_bytes()).unwrap();
    assert_eq!(state.schema_version, 2);
    assert_eq!(state.ops.len(), 1);
}

#[test]
fn v2_save_roundtrips() {
    let state = SessionState {
        schema_version: 2,
        ops: vec![RefinementOp::SetInclude {
            item_id: ItemId::Service { unit: "sshd.service".into() },
            include: true,
        }],
        cursor: 1,
    };
    let json = serde_json::to_string(&state).unwrap();
    let back = load_session_state(json.as_bytes()).unwrap();
    assert_eq!(back.schema_version, 2);
    assert_eq!(back.ops.len(), 1);
}
```

- [ ] **Step 2: Implement dual-version loader**

In `autosave.rs`, create `load_session_state()` that:
1. Deserializes into a raw `serde_json::Value` first
2. Checks `schema_version`
3. If v1: deserialize ops using the legacy enum, rewrite each to `SetInclude`, set `schema_version = 2`
4. If v2: deserialize directly into `SessionState`
5. Preserve `cursor` and op count through migration

- [ ] **Step 3: Bump writer to v2**

Update `save_session()` (line 51) to always write `schema_version: 2`.
Update the loader's version check (line 79: `if state.schema_version != 1`)
to accept both 1 and 2.

- [ ] **Step 4: Run tests**

Run: `cargo test -p inspectah-refine autosave -- --nocapture`
Expected: PASS — v1 loads and migrates, v2 loads directly, v2 save roundtrips

- [ ] **Step 5: Remove legacy RefinementOp variants**

Now that the dual-version loader handles v1 format, remove `ExcludePackage`,
`IncludePackage`, `ExcludeConfig`, `IncludeConfig`, `ExcludeRepo`,
`IncludeRepo` from the `RefinementOp` enum. Fix all match arms in
`session.rs` that handled these variants — they now handle `SetInclude`
with pattern matching on `item_id` kind.

- [ ] **Step 6: Verify session.rs init path**

Confirm that `RefineSession` construction (in `session.rs`) calls
`load_session_state()` and correctly replays migrated v2 ops through
`apply()`. Write a test that loads a v1 autosave, constructs a session,
and verifies the projected state matches.

- [ ] **Step 5: Fix session.rs apply() match arms**

The `apply()` method at `session.rs:329` has match arms for each legacy variant. Replace with a single `SetInclude` arm that dispatches on `item_id`:

```rust
RefinementOp::SetInclude { ref item_id, include } => {
    match item_id {
        ItemId::Package { name, arch } => {
            // existing package include/exclude logic
        }
        ItemId::Config { path } => {
            // existing config include/exclude logic
        }
        ItemId::Repo { path } => {
            // existing repo include/exclude logic
        }
        ItemId::Service { .. } | ItemId::ServiceDropIn { .. }
        | ItemId::Quadlet { .. } | ItemId::Flatpak { .. }
        | ItemId::Sysctl { .. } | ItemId::TunedSelection { .. } => {
            // Phase 1-3: not yet handled, return Ok(()) for now
        }
        _ => return Err(RefineError::InvalidOp("unsupported item kind".into())),
    }
}
```

- [ ] **Step 6: Run full workspace tests and clippy**

Run: `cargo test --workspace && cargo clippy --workspace -- -D warnings`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add inspectah-refine/
git commit -m "feat(refine): autosave v1→v2 migration, remove legacy RefinementOp variants

Legacy Exclude*/Include* ops are migrated to SetInclude on load. Legacy
enum variants removed. Session.apply() dispatches on ItemId kind.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 5: Replace attention classification with triage classification ✅ DONE (74c81cc)

**Files:**
- Modify: `inspectah-refine/src/attention.rs` → rename to `inspectah-refine/src/classify.rs`
- Modify: `inspectah-refine/src/fleet/attention.rs` → rename to `inspectah-refine/src/fleet/classify.rs`
- Modify: `inspectah-refine/src/lib.rs` (module rename)
- Test: existing tests migrate with the functions

- [ ] **Step 1: Rename attention.rs to classify.rs**

```bash
mv inspectah-refine/src/attention.rs inspectah-refine/src/classify.rs
mv inspectah-refine/src/fleet/attention.rs inspectah-refine/src/fleet/classify.rs
```

Update `lib.rs` and `fleet/mod.rs` module declarations.

- [ ] **Step 2: Rename functions and update return types**

- `compute_package_attention()` → `classify_packages()`, returns `Vec<RefinedPackage>` with `TriageTag` instead of `AttentionTag`
- `compute_config_attention()` → `classify_configs()`, same pattern
- `score_fleet_attention()` → `classify_fleet_bucket()`, returns `FleetTriage` instead of `AttentionScore`

**Fleet classification priority (Partial gates Divergent):**
```
if unknown_origin → Investigate
if prevalence < total → Partial (regardless of content divergence)
if prevalence == total && content diverges → Divergent
if prevalence == total && content agrees → Universal
```
Partial items with content variants still get the variant affordance (SelectVariant/EditVariant) once the user opts them in — the variant system is item-level, not bucket-level.

The single-host classification logic maps as follows:

| Old level | New bucket | Condition |
|-----------|-----------|-----------|
| `Routine` + `PackageBaselineMatch` | `Baseline` | Package in base image |
| `Routine` + `PackageUserAdded` | `Site` | Known repo, not in baseline |
| `Routine` + `PackageVersionChanged` (upgrade) | `Site` | Version changed |
| `NeedsReview` + `PackageVersionChanged` (downgrade) | `Investigate` | Downgrade |
| `NeedsReview` + `PackageLocalInstall` | `Investigate` | Local RPM |
| `NeedsReview` + `PackageNoRepoSource` | `Investigate` | No repo |
| `Informational` + `PackageProvenanceUnavailable` | `Investigate` | No baseline |
| `Routine` + `ConfigDefault` | `Baseline` | RPM default |
| `Routine` + `ConfigBaselineMatch` | `Baseline` | Matches baseline |
| `NeedsReview` + `ConfigModified` | `Site` | User customization |
| `Informational` + `ConfigUnowned` | `Site` | User-created file |
| `SensitivePath` overlay | `TriageAnnotation::SensitivePath` | Added as annotation, not bucket change |

- [ ] **Step 3: Update RefinedPackage and RefinedConfig structs**

Replace `attention: Vec<AttentionTag>` with `triage: TriageTag`. Replace `fleet_attention: Option<FleetAttention>` — the fleet triage is now inside `TriageTag.triage` (which is `Triage::Fleet(FleetTriage)` in fleet mode).

- [ ] **Step 4: Run all tests, fix failures**

Run: `cargo test --workspace`
Expected: Many test failures as old `AttentionLevel`/`AttentionTag` references break. Fix each one. The compiler and test failures guide the migration.

- [ ] **Step 5: Remove old AttentionLevel, AttentionReason, AttentionTag types**

Once all references are updated, delete the old types from `types.rs`.

- [ ] **Step 6: Run full workspace tests and clippy**

Run: `cargo test --workspace && cargo clippy --workspace -- -D warnings`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add inspectah-refine/ inspectah-web/
git commit -m "feat(refine): replace attention system with triage classification

AttentionLevel/AttentionReason/AttentionTag removed. New TriageBucket/
TriageReason/TriageTag system classifies items as Baseline/Site/Investigate.
SensitivePath becomes a TriageAnnotation overlay. Fleet classification uses
FleetBucket (Investigate/Divergent/Partial/Universal).

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 6: Update web handler and fleet handler for new types ✅ DONE (91fbc09)

**Files:**
- Modify: `inspectah-web/src/handlers.rs` (view endpoint, sections endpoint)
- Modify: `inspectah-web/src/fleet_handlers.rs` (fleet view)
- Test: existing handler tests

- [ ] **Step 1: Update ViewResponse to include triage tags**

The `/api/view` response currently returns `RefinedPackage` and `RefinedConfig` with `attention` fields. Update to use `triage: TriageTag` fields. The JSON wire format changes — frontend must be updated in the next phase.

- [ ] **Step 2: Update fleet_handlers.rs**

Replace `build_attention_dto()` with triage-aware equivalent. Fleet items get `Triage::Fleet(FleetTriage { bucket, prevalence })` instead of `AttentionScore::Fleet(FleetAttention { zone, attention, prevalence })`.

- [ ] **Step 3: Run workspace tests**

Run: `cargo test --workspace && cargo clippy --workspace -- -D warnings`
Expected: PASS (frontend tests will break until Task 7)

- [ ] **Step 4: Commit**

```bash
git add inspectah-web/
git commit -m "feat(web): update handlers for triage classification

View and fleet endpoints return TriageTag instead of AttentionTag.
Wire format change — frontend update required.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 7: Frontend type alignment ✅ DONE (f01a1bf, 3a2b43d, a83c693)

**Owner:** Kit (frontend)
**Files:**
- Modify: `inspectah-web/ui/src/api/types.ts`
- Modify: `inspectah-web/ui/src/components/DecisionItem.tsx`
- Modify: `inspectah-web/ui/src/components/DecisionList.tsx`
- Modify: `inspectah-web/ui/src/components/FleetApp.tsx`
- Modify: `inspectah-web/ui/src/components/MainContent.tsx`
- Modify: `inspectah-web/ui/src/components/Sidebar.tsx`
- Modify: `inspectah-web/ui/src/components/AttentionGroup.tsx` → rename/replace
- Test: `inspectah-web/ui/src/components/__tests__/DecisionSections.test.tsx`

- [ ] **Step 1: Update types.ts**

Replace `RefinementOp` union, add new types:

```typescript
// Triage types
export type TriageBucket = "baseline" | "site" | "investigate";
export type FleetBucket = "investigate" | "divergent" | "partial" | "universal";

export interface Prevalence { count: number; total: number; }

export interface FleetTriage {
  bucket: FleetBucket;
  prevalence: Prevalence;
}

export type Triage =
  | { mode: "single_host"; bucket: TriageBucket }
  | { mode: "fleet"; bucket: FleetBucket; prevalence: Prevalence };

export type TriageAnnotation =
  | "sensitive_path"
  | "first_boot_provisioned"
  | { requires_projected_package: { name: string } }
  | "runtime_only_observation";

export interface TriageTag {
  triage: Triage;
  primary_reason: string;
  annotations: TriageAnnotation[];
}

// Simplified RefinementOp
export type RefinementOp =
  | { op: "SetInclude"; target: { item_id: ItemId; include: boolean } }
  | { op: "UserStrategy"; target: { username: string; strategy: string } }
  | { op: "UserPassword"; target: UserPasswordOp }
  | { op: "SelectVariant"; target: { item_id: ItemId; target: string } }
  | { op: "EditVariant"; target: { item_id: ItemId; content: string; based_on: string | null } }
  | { op: "DiscardVariant"; target: { item_id: ItemId; variant: string } };

// New ItemId variants
export type ItemId =
  | { kind: "Package"; key: { name: string; arch: string } }
  | { kind: "Config"; key: { path: string } }
  | { kind: "Repo"; key: { path: string } }
  | { kind: "User"; key: { username: string } }
  | { kind: "Service"; key: { unit: string } }
  | { kind: "ServiceDropIn"; key: { unit: string; dropin_path: string } }
  | { kind: "Quadlet"; key: { path: string } }
  | { kind: "Flatpak"; key: { app_id: string; remote: string; branch: string } }
  | { kind: "Sysctl"; key: { key: string } }
  | { kind: "TunedSelection"; key: { profile: string } }
  | { kind: "Compose"; key: { path: string } }
  | { kind: "Fstab"; key: { mount_point: string } }
  | { kind: "NonRpm"; key: { name: string } };
```

- [ ] **Step 2: Collapse buildToggleOp functions**

In `DecisionItem.tsx` and `FleetApp.tsx`, replace section-specific toggle logic:

```typescript
function buildToggleOp(itemId: ItemId, include: boolean): RefinementOp {
  return { op: "SetInclude", target: { item_id: itemId, include } };
}
```

- [ ] **Step 3: Update DecisionItem to use TriageTag**

Replace `level` prop (which was `"needs_review" | "informational" | "routine"`) with `triageTag: TriageTag`. Update the left-border color logic:

```typescript
const BUCKET_BORDER: Record<string, string> = {
  investigate: "3px solid var(--pf-t--global--color--status--danger--default)",
  divergent: "3px solid var(--pf-t--global--color--status--warning--default)",
  site: "none",
  baseline: "none",
  partial: "3px solid var(--pf-t--global--color--status--custom--default)",
  universal: "none",
};
```

- [ ] **Step 4: Update AttentionGroup → TriageBucketGroup**

Rename `AttentionGroup.tsx` to `TriageBucketGroup.tsx`. Update to group items by triage bucket instead of attention level. Implement collapsible sections with smart defaults:

- Investigate + Divergent/Site: expanded by default
- Partial + Universal/Baseline: collapsed by default
- Sections with <3 items: always expanded
- Empty sections: show header with "(0)", disabled

- [ ] **Step 5: Add status bar component**

Create `TriageStatusBar.tsx` — passive chip display:

```typescript
export function TriageStatusBar({ bucketCounts }: { bucketCounts: Record<string, number> }) {
  return (
    <div className="inspectah-triage-status-bar">
      {Object.entries(bucketCounts).map(([bucket, count]) => (
        <span
          key={bucket}
          className={`inspectah-triage-chip inspectah-triage-chip--${bucket}`}
        >
          {bucketLabel(bucket)}: {count}
        </span>
      ))}
    </div>
  );
}
```

- [ ] **Step 6: Run frontend tests**

Run: `cd inspectah-web/ui && npx vitest run`
Expected: Some test failures from type changes. Fix each test.

- [ ] **Step 7: Commit**

```bash
git add inspectah-web/ui/
git commit -m "feat(ui): align frontend with triage bucket system

Replace AttentionLevel with TriageBucket/FleetBucket. Collapse buildToggleOp
to single SetInclude pattern. Add TriageBucketGroup with collapsible
sections and TriageStatusBar.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Phase 1: Services Promotion

**Owner:** Tang (backend) + Kit (frontend)
**Depends on:** Phase 0 complete

### Task 8: Service classification (Rust) ✅ DONE (6aae213)

**Files:**
- Modify: `inspectah-refine/src/classify.rs`
- Test: `inspectah-refine/src/classify.rs` (inline)

- [ ] **Step 1: Write classification tests**

Note: `ServiceStateChange` in `inspectah-core/src/types/services.rs` uses
typed fields: `current_state: ServiceUnitState` (enum: Enabled/Disabled/
Masked), `default_state: Option<PresetDefault>`, `owning_package: Option<String>`.
It already has `include: bool` and `fleet: Option<FleetPrevalence>`.
`SystemdDropIn` has `unit`, `path`, `content`, `include`, `variant_selection`,
`fleet`.

Non-package baseline data does not exist yet per the baseline prerequisite
caveat. The `default_state: Option<PresetDefault>` field gives us partial
baseline signal (systemd preset defaults), but full base-image comparison
is deferred. For v1, classify using `default_state` when available,
otherwise Site.

```rust
#[cfg(test)]
mod service_classification_tests {
    use super::*;
    use inspectah_core::types::services::{ServiceUnitState, PresetDefault};

    #[test]
    fn service_matching_preset_default_is_site_until_baseline() {
        // default_state gives partial signal, but true Baseline requires
        // base image comparison (deferred). For now, classify as Site.
        let change = ServiceStateChange {
            unit: "sshd.service".into(),
            current_state: ServiceUnitState::Enabled,
            default_state: Some(PresetDefault::Enabled),
            include: true,
            owning_package: Some("openssh-server".into()),
            ..Default::default()
        };
        let tag = classify_service(&change);
        assert_eq!(tag.triage, Triage::SingleHost(TriageBucket::Site));
    }

    #[test]
    fn service_differing_from_preset_is_site() {
        let change = ServiceStateChange {
            unit: "firewalld.service".into(),
            current_state: ServiceUnitState::Disabled,
            default_state: Some(PresetDefault::Enabled),
            include: true,
            ..Default::default()
        };
        let tag = classify_service(&change);
        assert_eq!(tag.triage, Triage::SingleHost(TriageBucket::Site));
        assert_eq!(tag.primary_reason, TriageReason::ServiceNonDefaultState);
    }

    #[test]
    fn service_without_owning_package_is_investigate() {
        let change = ServiceStateChange {
            unit: "custom-agent.service".into(),
            current_state: ServiceUnitState::Enabled,
            default_state: None,
            include: true,
            owning_package: None,
            ..Default::default()
        };
        let tag = classify_service(&change);
        assert_eq!(tag.triage, Triage::SingleHost(TriageBucket::Investigate));
        assert_eq!(tag.primary_reason, TriageReason::ServiceUnknownOrigin);
    }

    #[test]
    fn dropin_is_always_site() {
        let dropin = SystemdDropIn {
            unit: "sshd.service".into(),
            path: "/etc/systemd/system/sshd.service.d/override.conf".into(),
            include: true,
            ..Default::default()
        };
        let tag = classify_dropin(&dropin);
        assert_eq!(tag.triage, Triage::SingleHost(TriageBucket::Site));
        assert_eq!(tag.primary_reason, TriageReason::ServiceDropInPresent);
    }
}
```

- [ ] **Step 2: Implement classify_services()**

Note: Create `RefinedServiceState` and `RefinedDropIn` in `types.rs`
alongside `RefinedPackage` and `RefinedConfig`, each with `entry` +
`triage: TriageTag` fields.

Note: `ServiceSection` (in `inspectah-core/src/types/services.rs`) has
two sibling fields: `state_changes: Vec<ServiceStateChange>` and
`drop_ins: Vec<SystemdDropIn>`. Drop-ins are NOT nested inside state
changes. Match drop-ins to their parent service by the `unit` field on
`SystemdDropIn`.

```rust
pub fn classify_services(snap: &InspectionSnapshot) -> (Vec<RefinedServiceState>, Vec<RefinedDropIn>) {
    let services = match &snap.services {
        Some(s) => s,
        None => return (Vec::new(), Vec::new()),
    };

    let mut states = Vec::new();
    for change in &services.state_changes {
        let tag = classify_service(change);
        states.push(RefinedServiceState { entry: change.clone(), triage: tag });
    }

    let mut dropins = Vec::new();
    for dropin in &services.drop_ins {
        let tag = classify_dropin(dropin);
        dropins.push(RefinedDropIn { entry: dropin.clone(), triage: tag });
    }

    (states, dropins)
}

fn classify_service(change: &ServiceStateChange) -> TriageTag {
    // No owning package → Investigate (unknown origin)
    if change.owning_package.is_none() {
        return TriageTag {
            triage: Triage::SingleHost(TriageBucket::Investigate),
            primary_reason: TriageReason::ServiceUnknownOrigin,
            annotations: vec![],
        };
    }
    // All known services classify as Site until base-image baseline ships.
    // When baseline is available, compare current_state against the base
    // image's state and classify matching services as Baseline.
    TriageTag {
        triage: Triage::SingleHost(TriageBucket::Site),
        primary_reason: TriageReason::ServiceNonDefaultState,
        annotations: vec![],
    }
}

fn classify_dropin(dropin: &SystemdDropIn) -> TriageTag {
    TriageTag {
        triage: Triage::SingleHost(TriageBucket::Site),
        primary_reason: TriageReason::ServiceDropInPresent,
        annotations: vec![],
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p inspectah-refine service_classification -- --nocapture`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add inspectah-refine/
git commit -m "feat(refine): add service classification for triage buckets

classify_services() classifies service state changes and drop-ins into
Baseline/Site/Investigate based on base image comparison.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 9: Service refinement in session ✅ DONE (6aae213)

**Files:**
- Modify: `inspectah-refine/src/session.rs` (SetInclude handler for services)
- Test: session tests

- [ ] **Step 1: Add SetInclude handling for Service and ServiceDropIn in session.apply()**

In the `SetInclude` match arm, add:

```rust
ItemId::Service { ref unit } => {
    if let Some(services) = &mut self.snapshot.services {
        if let Some(svc) = services.state_changes.iter_mut()
            .find(|s| s.unit == *unit) {
            svc.include = include;
            // Symmetric cascade on the sibling drop_ins list
            for dropin in services.drop_ins.iter_mut()
                .filter(|d| d.unit == *unit) {
                dropin.include = include;
            }
        }
    }
}
ItemId::ServiceDropIn { ref unit, ref dropin_path } => {
    if let Some(services) = &mut self.snapshot.services {
        // Check parent service is included
        let parent_included = services.state_changes.iter()
            .any(|s| s.unit == *unit && s.include);
        if include && !parent_included {
            return Err(RefineError::InvalidOp(
                "cannot include drop-in when parent service is excluded".into()
            ));
        }
        if let Some(dropin) = services.drop_ins.iter_mut()
            .find(|d| d.unit == *unit && d.path == *dropin_path) {
            dropin.include = include;
        }
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p inspectah-refine -- --nocapture`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add inspectah-refine/
git commit -m "feat(refine): service refinement with drop-in cascade

Services and drop-ins support SetInclude. Excluding a service cascades to
its drop-ins. Re-including a service auto-re-includes its drop-ins.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 9a: Pipeline pruning for services ✅ DONE (dc4b607)

**Files:**
- Modify: `inspectah-pipeline/src/render/configtree.rs` — `write_config_tree()` (line 138) must skip materializing paths under `/etc/systemd/system/*.service.d/` — these are owned by the services renderer
- Modify: `inspectah-pipeline/src/render/service_intent.rs` — `render_service_intent()` (line 294) is the real services render authority. It produces a `ServiceRenderPlan` consumed by `containerfile.rs:services_section_lines()`. Must gate output on `include` field (already on `ServiceStateChange`).
- Modify: `inspectah-pipeline/src/render/containerfile.rs` — `services_section_lines()` (line 485) delegates to `render_service_intent()`. Ensure the delegation respects the `include` gate.

**Layering note:** `inspectah-pipeline` depends on `inspectah-core` (not
`inspectah-refine`). The renderer cannot see `TriageBucket` directly.
Rendering gates on the `include: bool` field already present on core
types (`ServiceStateChange.include`, `SystemdDropIn.include`, etc.).
The triage classification in `inspectah-refine` drives the UI grouping;
the core `include` field drives the renderer. Baseline no-output is
deferred until base-image baseline data exists — when it ships, the
refine layer will set a flag on core types (e.g., `baseline_match: bool`)
that the renderer can check without depending on refine.
- Test: render tests in both `configtree.rs` and `service_intent.rs`

- [ ] **Step 1: Write pruning test for configtree**

Test that a service drop-in path (`/etc/systemd/system/sshd.service.d/override.conf`) is NOT materialized in the generic config tree when the services renderer owns it.

- [ ] **Step 2: Write include-gating test for containerfile**

Test that a service with `include == false` generates no `systemctl` line.

- [ ] **Step 3: Implement pruning in write_config_tree()**

Add a path exclusion set for service-owned paths. The exclusion list is computed from the snapshot's services section:

```rust
fn service_owned_paths(snap: &InspectionSnapshot) -> HashSet<String> {
    let mut paths = HashSet::new();
    if let Some(services) = &snap.services {
        for change in &services.state_changes {
            for dropin in &services.drop_ins {
                paths.insert(dropin.path.clone());
            }
        }
    }
    paths
}
```

Skip any config entry whose path is in this set.

- [ ] **Step 4: Verify include gating in service_intent.rs**

Verify `render_service_intent()` skips services with `include == false`.
(Baseline no-output gating is deferred per the baseline prerequisite caveat.)

- [ ] **Step 5: Run tests**

Run: `cargo test -p inspectah-pipeline && cargo clippy -p inspectah-pipeline -- -D warnings`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add inspectah-pipeline/
git commit -m "feat(pipeline): ownership pruning for service drop-ins

Config tree skips service-owned paths. Containerfile renderer gates
service output on include state.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 9b: Web + fleet handlers for services ✅ DONE (edffbe0)

**Files:**
- Modify: `inspectah-web/src/handlers.rs` — replace `normalize_services()` (line 858) ContextSection projection with decision-item projection returning `Vec<RefinedService>` with `TriageTag`. Update `get_view()` (line 247) to include services in the view response.
- Modify: `inspectah-web/src/fleet_handlers.rs` — move services from `build_context_sections()` (line 622) to `build_fleet_sections()` (line 431). Wire up `TriageTag` and fleet prevalence for service items.
- Test: handler tests

- [ ] **Step 1: Update single-host handler**

Replace `normalize_services()` with a function that returns classified service items with `TriageTag` instead of `ContextItem`. Add to the view response alongside packages and configs.

- [ ] **Step 2: Update fleet handler**

Move services from context sections to fleet sections. Services get fleet classification (Universal/Divergent/Partial/Investigate) with prevalence data.

- [ ] **Step 3: Run tests**

Run: `cargo test -p inspectah-web && cargo clippy -p inspectah-web -- -D warnings`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add inspectah-web/
git commit -m "feat(web): services as decision items in single-host and fleet handlers

Services promoted from context sections to decision items in both
single-host view and fleet view endpoints.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 10: Services frontend ✅ DONE (127cae5)

**Owner:** Kit (frontend)
**Files:**
- Modify: `inspectah-web/ui/src/components/Sidebar.tsx`
- Modify: `inspectah-web/ui/src/components/MainContent.tsx`
- Create: `inspectah-web/ui/src/components/ServiceSection.tsx`
- Test: `inspectah-web/ui/src/components/__tests__/ServiceSection.test.tsx`

- [ ] **Step 1: Move Services from CONTEXT_SECTIONS to DECISION_SECTIONS**

```typescript
const DECISION_SECTIONS = [
  { id: "packages", label: "Packages" },
  { id: "configs", label: "Config Files" },
  { id: "users_groups", label: "Users & Groups" },
  { id: "services", label: "Services" },  // promoted
];
```

Remove `services` from `CONTEXT_SECTIONS`.

- [ ] **Step 2: Create ServiceSection component with parent-child toggle**

Build `ServiceSection.tsx` using `DecisionItem` for service rows. Drop-in rows render as indented children:

- 16px additional left padding
- Thin left border connecting to parent
- When parent is excluded: drop-in checkboxes disabled, opacity 0.55, "Service excluded" badge
- Masked vs disabled distinction shown in subtitle

Apply ui-ux-pro-max guidelines:
- Touch targets: 44x44px minimum for toggle areas
- Focus-visible rings on all interactive elements
- Keyboard: Tab through parent then children, Space on disabled drop-in is no-op
- Transitions: 150-300ms for toggle state changes
- Contrast: 4.5:1 minimum for all text including disabled state (opacity 0.55 must still meet contrast)

- [ ] **Step 3: Wire into MainContent**

Add services section routing in `MainContent.tsx` alongside packages and configs.

- [ ] **Step 4: Write tests**

Test parent-child cascade: excluding parent disables children. Test re-including parent re-enables children. Test excluding individual drop-in while parent stays included.

- [ ] **Step 5: Run tests**

Run: `cd inspectah-web/ui && npx vitest run`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add inspectah-web/ui/
git commit -m "feat(ui): promote services to Review with parent-child drop-in toggles

Services section moves from Reference to Review. Drop-ins render as
indented children with cascade disable when parent is excluded.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Phase 2: Quadlets + Flatpak Promotion

**Pattern:** Same as Phase 1 (classify → session handler → pipeline pruning → fleet handler → frontend).

### Task 11: Container classification (Rust) ✅ DONE (a707e7b)

- Quadlets are always `Site` (user-deployed). Rare exception: `Baseline` if a quadlet path exists in the base image.
- Flatpaks are always `Site` with `TriageAnnotation::FirstBootProvisioned`.
- Flatpak with incomplete provenance (missing remote/branch) → `Investigate`.
- Fleet: Divergent quadlets activate the variant system.
- `ItemId::Quadlet { path }`, `ItemId::Flatpak { app_id, remote, branch }`

### Task 11a: Flatpak merge/type update in inspectah-core ✅ DONE

**Files:**
- Modify: `inspectah-core/src/fleet/merge.rs` — flatpak dedup (line 1128) currently uses `app_id` only. Update to dedup by `(app_id, remote, branch)`. Carry `remote_url` as a field that can diverge across hosts (detected by content comparison, resolvable via variant selection).
- Modify: `inspectah-core/src/types/containers.rs` (or wherever `FlatpakApp` lives) — ensure `remote_url` is a field on the struct (render metadata, not identity).
- Test: fleet merge test proving two flatpak entries with same `app_id` but different `remote` are kept as separate items, not collapsed.

- [ ] **Step 1: Write merge test for flatpak identity**

```rust
#[test]
fn flatpak_different_remotes_not_collapsed() {
    // Same app_id, different remote → two distinct items
    // (previously collapsed by app_id-only dedup)
}
```

- [ ] **Step 2: Update merge dedup key**

Change line 1134 from `seen.insert(app.app_id.clone())` to
`seen.insert((app.app_id.clone(), app.remote.clone(), app.branch.clone()))`.

- [ ] **Step 3: Add remote_url divergence test**

Test that same `(app_id, remote, branch)` with different `remote_url`
across hosts produces a Divergent classification with resolvable variant.

- [ ] **Step 4: Run tests and commit**

```bash
cargo test -p inspectah-core && cargo clippy -p inspectah-core -- -D warnings
git add inspectah-core/
git commit -m "feat(core): flatpak merge uses (app_id, remote, branch) identity

Previously deduped by app_id only, which collapsed distinct flatpak
entries from different remotes. remote_url is render metadata that can
diverge across hosts.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 12: Container refinement in session ✅ DONE (2f0502d)

- SetInclude for `ItemId::Quadlet` and `ItemId::Flatpak`

### Task 12a: Pipeline pruning for containers ✅ DONE (5e38d7b)

**Files:**
- Modify: `inspectah-pipeline/src/render/configtree.rs` — `write_config_tree()` must skip materializing paths under `/etc/containers/systemd/` (owned by containers renderer)
- Modify: `inspectah-pipeline/src/render/containerfile.rs` — `render_containerfile()` must gate quadlet/flatpak output on `include` state
- Test: write render tests proving excluded quadlets do not appear in either the dedicated `quadlet/` tree or the generic `config/etc/` tree

### Task 12b: Fleet handler for containers ✅ DONE (f7fd90b)

**Files:**
- Modify: `inspectah-web/src/fleet_handlers.rs` — move containers from `build_context_sections()` (line 622) to `build_fleet_sections()` (line 431). Wire up `TriageTag` and variant system for quadlets. Add flatpak lifecycle badge to fleet DTOs.

### Task 13: Containers frontend ✅ DONE (62a1624)

- Move Containers from `CONTEXT_SECTIONS` to `DECISION_SECTIONS`
- Lifecycle badges: `Quadlet` + "Image content", `Flatpak` + "First boot", `Compose` + "Reference only"
- Compose items remain read-only `ContextItem` within the Containers section
- Apply ui-ux-pro-max: compact PatternFly Label for badges, neutral/gray `isCompact`
- Fleet: wire up variant display for divergent quadlets in `FleetApp.tsx`

---

## Phase 3: Sysctls + Tuned Promotion

### Task 14: Sysctl classification (Rust)

- Only file-backed overrides (source under `/etc/sysctl.d/` or `/etc/sysctl.conf`) are promoted
- Runtime-only observations stay as reference with `TriageAnnotation::RuntimeOnlyObservation`
- Deny list filters: `vm.drop_caches`, `vm.compact_memory`, `kernel.sysrq`
- Fleet Divergent: different values for the same key → variant display with human-readable values
- `ItemId::Sysctl { key }`

### Task 15: Tuned classification (Rust)

- One bundled `TunedSelection` per host (active profile + custom profile files as payload)
- Baseline: profile matches base image's tuned state
- Site: non-default profile or custom profile
- Investigate: tuned active but package not installed, or custom profile with missing files
- `RequiresProjectedPackage { name: "tuned" }` annotation when tuned package needs to be in the image
- `ItemId::TunedSelection { profile }`

### Task 16: Sysctl + Tuned refinement in session

- SetInclude for `ItemId::Sysctl` and `ItemId::TunedSelection`
- Included TunedSelection with custom profile bundles its `/etc/tuned/<name>/` directory

### Task 16a: Pipeline pruning for sysctls + tuned

**Files:**
- Modify: `inspectah-pipeline/src/render/configtree.rs` — `write_config_tree()` must:
  - Skip materializing original sysctl source files (e.g., `/etc/sysctl.d/99-custom.conf`) — these are owned by the sysctl renderer now
  - Skip materializing tuned profile paths under `/etc/tuned/` — owned by tuned renderer
- Modify: `inspectah-pipeline/src/render/containerfile.rs` — `render_containerfile()` must:
  - Synthesize `sysctl/etc/sysctl.d/99-inspectah-migrated.conf` from included keys only (not copying original source files wholesale)
  - Gate tuned output on `include` state, enforce `RequiresProjectedPackage` for tuned RPM
- Test: write render tests proving:
  - Excluding one sysctl key from a shared source file does not leak the original file
  - Custom tuned profile carries bundled files
  - Excluded tuned generates no output

### Task 16b: Fleet handler for sysctls + tuned

**Files:**
- Modify: `inspectah-web/src/fleet_handlers.rs` — move sysctl and tuned items from `build_context_sections()` to `build_fleet_sections()`. Wire up `TriageTag`. Sysctl divergent items use variant display with human-readable values (not content hashes).
- Modify: `inspectah-web/src/handlers.rs` — update single-host handler to project sysctls and tuned as decision items with `TriageTag`

### Task 17: Sysctls + Tuned frontend

- Move Kernel/Boot (sysctl portion) and Tuned to `DECISION_SECTIONS` as "Sysctls" and "Tuned Profiles"
- Sysctl fleet: variant display with human-readable values (`10 (45 hosts)` vs `60 (5 hosts)`)
- Tuned: usually 1-2 items, simple toggle. Stock profile list for display context.
- Apply ui-ux-pro-max: sections with <3 items default expanded
- Fleet: wire up sysctl variant display and tuned prevalence in `FleetApp.tsx`

---

## Phase 4: Integration Polish

### Task 18: Divergent review tracking (session layer — client-side)

Divergent review confirmation is session-layer state per the spec — not
in the triage struct, not a RefinementOp, not persisted in autosave. For
v1, this lives as client-side session state in the fleet React component.

**Files:**
- Modify: `inspectah-web/ui/src/components/FleetApp.tsx`

- [ ] **Step 1: Add confirmed set to fleet session state**

Add a `Set<string>` (keyed by JSON-serialized `ItemId`) to the fleet
component's state. Any `SetInclude` or `SelectVariant` mutation targeting
a Divergent item adds that item's serialized ID to the set.

- [ ] **Step 2: Wire confirmed count into status bar**

Divergent chip shows unconfirmed count: "Divergent: 3 (2 unconfirmed)".
Compute by diffing Divergent items against the confirmed set.

- [ ] **Step 3: Wire into progress**

Progress bar treats only confirmed + excluded items as resolved. A
Divergent item that is `include=true` but not in the confirmed set does
not count toward completion.

- [ ] **Step 4: Run tests and commit**

Run: `cd inspectah-web/ui && npx vitest run`

```bash
git add inspectah-web/ui/
git commit -m "feat(ui): divergent review tracking in fleet session state

Tracks which Divergent items the operator has interacted with. Unconfirmed
count shown in status bar. Progress excludes unconfirmed Divergent items.

Assisted-by: Claude Code (Opus 4.6)"
```

### Task 19: Package section repo-first exception

- Packages keep the existing `RepoBar` / `RepoGroup` layout as primary grouping
- Add `TriageTag` badge to each package row (Baseline/Site/Investigate)
- No structural change to packages — the triage model applies via badges, not via bucket grouping

### Task 20: End-to-end validation

### Task 19a: Export contract update for promoted roots

**Files:**
- Modify: `inspectah-pipeline/src/render/configtree.rs` — `write_config_tree()` and `config_copy_roots()` (line 462) must produce promoted-owned output roots: `drop-ins/` (service drop-in files), `quadlet/` (quadlet units), `flatpak/` (manifest + provisioning service), `sysctl/` (synthesized sysctl conf), `tuned/` (profile files + activation). These are new directories alongside the existing `config/` root.
- Modify: `inspectah-pipeline/src/render/tarball.rs` — tarball packaging must include the new output roots
- Modify: `inspectah-web/src/handlers.rs` — export endpoint (`export_tarball` at line 416) must include promoted roots in the exported archive
- Test: export test proving promoted-owned files appear in the tarball under their dedicated roots, not under `config/`

- [ ] **Step 1: Add promoted roots to configtree**

Update `write_config_tree()` to write promoted artifacts to dedicated directories:
- `drop-ins/etc/systemd/system/<unit>.d/` for service drop-ins
- `quadlet/etc/containers/systemd/` for quadlet units
- `flatpak/flatpak-install.json` + `flatpak/flatpak-provision.service`
- `sysctl/etc/sysctl.d/99-inspectah-migrated.conf`
- `tuned/etc/tuned/<profile>/` for custom profile files

- [ ] **Step 2: Update config_copy_roots()**

`config_copy_roots()` generates the `COPY` lines for the Containerfile. Add entries for the new roots.

- [ ] **Step 3: Update tarball packaging**

Ensure `tarball.rs` includes the new directories when packaging the output.

- [ ] **Step 4: Test export roundtrip**

Write test: create a snapshot with promoted items, run export, verify the tarball contains the new roots with correct content.

- [ ] **Step 5: Commit**

```bash
git add inspectah-pipeline/ inspectah-web/
git commit -m "feat(pipeline): export contract includes promoted-owned roots

Tarball export includes drop-ins/, quadlet/, flatpak/, sysctl/, tuned/
as dedicated output roots alongside config/.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 20: End-to-end validation

- [ ] Test each validation matrix case from the spec:
  - Excluded service drop-in absent from both `drop-ins/` and generic config output
  - Excluded service generates no `systemctl` line
  - Excluded quadlet generates no `quadlet/` materialization
  - Excluding one sysctl key from a shared source file does not leak the original file
  - Custom tuned profile carries bundled files
  - Autosave migration with active + inactive ops preserves replay semantics

- [ ] Visual regression check: run the dev server, verify all promoted sections appear in Review sidebar group, toggle items, verify Containerfile preview updates correctly

---

## Summary

| Phase | Tasks | Owner | Key deliverable |
|-------|-------|-------|----------------|
| 0 | 1-7 | Tang + Kit | Type system + classification rewrite + autosave v2 |
| 1 | 8, 9, 9a, 9b, 10 | Tang + Kit | Services promoted with cascade + pipeline pruning + fleet |
| 2 | 11, 11a, 12, 12a, 12b, 13 | Tang + Kit | Quadlets + Flatpak with merge fix + pipeline pruning + fleet |
| 3 | 14-15, 16, 16a, 16b, 17 | Tang + Kit | Sysctls + Tuned with synthesized output + pipeline pruning + fleet |
| 4 | 18, 19, 19a, 20 | Kit + Tang | Divergent tracking, repo exception, export contract, validation |

**Estimated total:** ~30 tasks, each 30-90 minutes. Each promotion phase covers four layers: classification (Tang), refine session (Tang), pipeline render/pruning (Tang), web+fleet handlers (Tang), frontend (Kit). Phases can overlap once the type system is stable.
