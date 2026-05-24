# Unified Package/Repo Management Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace divergent single-machine and fleet package views with a unified layout: repo control bar, sortable package list, and excluded zone.

**Architecture:** Backend-first — Rust data model changes (repo tier classification, fleet repo-conflict tracking, FleetViewResponse extension) are implemented and tested before frontend work. Frontend builds shared components consumed by both single-machine and fleet modes via props, replacing existing DecisionSections and FleetSection package rendering.

**Tech Stack:** Rust (inspectah-core, inspectah-refine, inspectah-web), TypeScript/React (inspectah-web/ui), PatternFly v6, Vitest, Playwright e2e

**Spec:** `docs/specs/proposed/2026-05-23-unified-package-repo-management-design.md`

---

## File Map

### Rust (create or modify)

| File | Responsibility |
|------|---------------|
| `inspectah-refine/src/repo_index.rs` | Remove CRB from DISTRO_REPOS, add `repo_tier()` classification method |
| `inspectah-refine/src/types.rs` | Add `RepoTier` enum |
| `inspectah-web/src/handlers.rs` | Add `tier` field to `RepoGroupInfo`, remove `leaf_dep_tree` from `ViewResponse` |
| `inspectah-web/src/fleet_handlers.rs` | Add `repo_groups` and `source_repo` + `repo_conflict` to fleet response |
| `inspectah-core/src/fleet/merge.rs` | Track per-repo host counts during package merge |
| `inspectah-core/src/types/fleet.rs` | Add `RepoSourceEntry` struct |
| `inspectah-web/tests/api_test.rs` | Update existing tests for new fields |
| `inspectah-web/tests/fleet_api_test.rs` | Add repo-conflict and repo_groups tests |

### TypeScript/React (create or modify)

| File | Responsibility |
|------|---------------|
| `inspectah-web/ui/src/api/types.ts` | Add `RepoTier`, `RepoSourceEntry`, update `RepoGroupInfo`, `FleetItem`, `ViewResponse` |
| `inspectah-web/ui/src/components/RepoBar.tsx` | New: two-row repo control bar (static text + toggleable pills) |
| `inspectah-web/ui/src/components/PackageList.tsx` | New: unified sortable package list with mode-aware columns |
| `inspectah-web/ui/src/components/SortHeader.tsx` | New: two-column sortable header with chevron indicators |
| `inspectah-web/ui/src/components/ExcludedZone.tsx` | New: dimmed excluded-packages section |
| `inspectah-web/ui/src/components/fleet/RepoConflictPopover.tsx` | New: button-triggered popover for repo-source warnings |
| `inspectah-web/ui/src/components/__tests__/RepoBar.test.tsx` | Tests for repo bar |
| `inspectah-web/ui/src/components/__tests__/PackageList.test.tsx` | Tests for unified package list |
| `inspectah-web/ui/src/components/__tests__/SortHeader.test.tsx` | Tests for sort headers |
| `inspectah-web/ui/src/components/__tests__/ExcludedZone.test.tsx` | Tests for excluded zone |
| `inspectah-web/ui/src/components/fleet/__tests__/RepoConflictPopover.test.tsx` | Tests for popover |

---

## Phase 1: Backend — Repo Tier Classification

### Task 1: Add RepoTier enum and reclassify CRB

**Files:**
- Modify: `inspectah-refine/src/types.rs`
- Modify: `inspectah-refine/src/repo_index.rs`

- [ ] **Step 1: Add RepoTier enum to types.rs**

Find the existing `RepoProvenance` enum in `inspectah-refine/src/types.rs` and add `RepoTier` nearby:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepoTier {
    Distro,
    OfficialOptional,
    ThirdParty,
}
```

- [ ] **Step 2: Remove CRB from DISTRO_REPOS in repo_index.rs**

Change the `DISTRO_REPOS` constant from:
```rust
pub const DISTRO_REPOS: &[&str] = &[
    "baseos",
    "appstream",
    "crb",
    "fedora",
    "updates",
    "anaconda",
];
```
to (removes CRB, adds `updates-testing` and `extras` per accepted spec):
```rust
pub const DISTRO_REPOS: &[&str] = &[
    "baseos",
    "appstream",
    "fedora",
    "updates",
    "updates-testing",
    "extras",
    "anaconda",
];
```

- [ ] **Step 3: Add OFFICIAL_OPTIONAL_REPOS constant and repo_tier() method**

Below the `DISTRO_REPOS` constant, add:
```rust
pub const OFFICIAL_OPTIONAL_REPOS: &[&str] = &["crb", "codeready-builder", "rhel-extensions"];
```

Add a public method to `RepoIndex`. **Case-insensitive** — lowercase the
input before matching, consistent with how `RepoIndex::build()` normalizes
section IDs:
```rust
pub fn repo_tier(section_id: &str) -> RepoTier {
    let lower = section_id.to_lowercase();
    let id = lower.as_str();
    if DISTRO_REPOS.contains(&id) {
        RepoTier::Distro
    } else if OFFICIAL_OPTIONAL_REPOS.contains(&id) {
        RepoTier::OfficialOptional
    } else {
        RepoTier::ThirdParty
    }
}
```

Also update `is_distro_repo()` to be case-insensitive for consistency:
```rust
pub fn is_distro_repo(section_id: &str) -> bool {
    DISTRO_REPOS.contains(&section_id.to_lowercase().as_str())
}
```

- [ ] **Step 4: Update is_distro_repo test**

The existing `test_is_distro_repo` test asserts CRB is distro. Update it:
```rust
#[test]
fn test_is_distro_repo() {
    assert!(RepoIndex::is_distro_repo("baseos"));
    assert!(RepoIndex::is_distro_repo("appstream"));
    assert!(RepoIndex::is_distro_repo("BaseOS")); // case-insensitive
    assert!(RepoIndex::is_distro_repo("updates-testing"));
    assert!(RepoIndex::is_distro_repo("extras"));
    assert!(!RepoIndex::is_distro_repo("epel"));
    assert!(!RepoIndex::is_distro_repo("custom-internal"));
    assert!(!RepoIndex::is_distro_repo("crb")); // CRB is now official-optional
}
```

Add a new test for `repo_tier`:
```rust
#[test]
fn test_repo_tier() {
    assert_eq!(RepoIndex::repo_tier("baseos"), RepoTier::Distro);
    assert_eq!(RepoIndex::repo_tier("appstream"), RepoTier::Distro);
    assert_eq!(RepoIndex::repo_tier("AppStream"), RepoTier::Distro); // case-insensitive
    assert_eq!(RepoIndex::repo_tier("updates-testing"), RepoTier::Distro);
    assert_eq!(RepoIndex::repo_tier("extras"), RepoTier::Distro);
    assert_eq!(RepoIndex::repo_tier("crb"), RepoTier::OfficialOptional);
    assert_eq!(RepoIndex::repo_tier("CRB"), RepoTier::OfficialOptional); // case-insensitive
    assert_eq!(RepoIndex::repo_tier("rhel-extensions"), RepoTier::OfficialOptional);
    assert_eq!(RepoIndex::repo_tier("epel"), RepoTier::ThirdParty);
    assert_eq!(RepoIndex::repo_tier("copr:mytools"), RepoTier::ThirdParty);
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p inspectah-refine -- repo`
Expected: All repo-related tests pass, including updated CRB classification.

- [ ] **Step 6: Commit**

```bash
git add inspectah-refine/src/types.rs inspectah-refine/src/repo_index.rs
git commit -m "feat(refine): add RepoTier enum, reclassify CRB as official-optional"
```

---

### Task 2: Add tier to RepoGroupInfo and remove leaf_dep_tree

**Files:**
- Modify: `inspectah-web/src/handlers.rs`

- [ ] **Step 1: Add tier field to RepoGroupInfo**

Change `RepoGroupInfo` from:
```rust
#[derive(Serialize, Clone, Debug)]
pub struct RepoGroupInfo {
    pub section_id: String,
    pub provenance: RepoProvenance,
    pub is_distro: bool,
    pub package_count: usize,
    pub enabled: bool,
}
```
to:
```rust
#[derive(Serialize, Clone, Debug)]
pub struct RepoGroupInfo {
    pub section_id: String,
    pub provenance: RepoProvenance,
    pub is_distro: bool,
    pub tier: RepoTier,
    pub package_count: usize,
    pub enabled: bool,
}
```

Add the import at the top of handlers.rs:
```rust
use inspectah_refine::types::RepoTier;
```

- [ ] **Step 2: Populate tier in build_repo_groups()**

In the `build_repo_groups()` function, find where `RepoGroupInfo` is constructed and add the `tier` field:

```rust
RepoGroupInfo {
    section_id: section_id.clone(),
    provenance,
    is_distro,
    tier: RepoIndex::repo_tier(&section_id),
    package_count,
    enabled,
}
```

- [ ] **Step 3: Remove leaf_dep_tree from ViewResponse**

Change `ViewResponse` to remove the `leaf_dep_tree` field:
```rust
#[derive(Serialize)]
pub struct ViewResponse {
    #[serde(flatten)]
    pub view: RefinedView,
    pub repo_groups: Vec<RepoGroupInfo>,
    pub baseline_summary: Option<BaselineSummary>,
    pub version_changes: Vec<VersionChangeEntry>,
    pub users_groups_decisions: Vec<serde_json::Value>,
    pub session_is_sensitive: bool,
}
```

Update `build_view_response()` to remove the `leaf_dep_tree` computation and field.

- [ ] **Step 4: Fix compilation and update existing tests**

Run: `cargo test -p inspectah-web`

Fix any tests that reference `leaf_dep_tree` in the view response — they should no longer expect this field. Fix any tests that construct `RepoGroupInfo` without the `tier` field.

- [ ] **Step 5: Add test for tier in repo_groups**

In `inspectah-web/tests/api_test.rs`, find the existing `view_response_includes_repo_groups` test. Add an assertion for the new `tier` field:

```rust
let appstream = groups.iter().find(|g| g.section_id == "appstream").unwrap();
assert_eq!(appstream.tier, "distro");

let epel = groups.iter().find(|g| g.section_id == "epel").unwrap();
assert_eq!(epel.tier, "third_party");
```

Run: `cargo test -p inspectah-web`
Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add inspectah-web/src/handlers.rs inspectah-web/tests/api_test.rs
git commit -m "feat(web): add tier to RepoGroupInfo, remove leaf_dep_tree from ViewResponse"
```

---

## Phase 2: Backend — Fleet Repo Conflict Tracking

### Task 3: Track repo-source conflicts during fleet merge

**Files:**
- Modify: `inspectah-core/src/types/fleet.rs`
- Modify: `inspectah-core/src/fleet/merge.rs`

- [ ] **Step 1: Add RepoSourceEntry struct to fleet types**

In `inspectah-core/src/types/fleet.rs`, add:
```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoSourceEntry {
    pub repo: String,
    pub host_count: usize,
}
```

- [ ] **Step 2: Write failing test for repo-conflict tracking**

In `inspectah-core/src/fleet/merge.rs` (in the `#[cfg(test)]` module), add:

```rust
#[test]
fn test_package_merge_tracks_repo_conflict() {
    use crate::types::rpm::{PackageEntry, PackageState, RpmSection};

    let host_a_rpm = RpmSection {
        packages_added: vec![PackageEntry {
            name: "nginx".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            include: true,
            source_repo: "epel".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let host_b_rpm = RpmSection {
        packages_added: vec![PackageEntry {
            name: "nginx".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            include: true,
            source_repo: "appstream".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let host_c_rpm = RpmSection {
        packages_added: vec![PackageEntry {
            name: "nginx".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            include: true,
            source_repo: "epel".into(),
            ..Default::default()
        }],
        ..Default::default()
    };

    let hostnames = vec!["host-a".into(), "host-b".into(), "host-c".into()];
    let merged = merge_rpm_sections(
        vec![Some(host_a_rpm), Some(host_b_rpm), Some(host_c_rpm)],
        3,
        &hostnames,
        None,
    )
    .unwrap();

    let nginx = merged
        .packages_added
        .iter()
        .find(|p| p.name == "nginx")
        .unwrap();

    // Majority repo wins
    assert_eq!(nginx.source_repo, "epel");

    // After Step 3, merge_rpm_sections returns a tuple:
    // (RpmSection, HashMap<String, Vec<RepoSourceEntry>>)
    // Destructure and verify conflict data:
    let (merged, repo_conflicts) = merge_rpm_sections(
        vec![Some(host_a_rpm), Some(host_b_rpm), Some(host_c_rpm)],
        3,
        &hostnames,
        None,
    )
    .unwrap();

    let nginx = merged
        .packages_added
        .iter()
        .find(|p| p.name == "nginx")
        .unwrap();
    assert_eq!(nginx.source_repo, "epel"); // majority wins

    // Conflict data from the merge
    assert!(repo_conflicts.contains_key("nginx.x86_64"));
    let conflict = &repo_conflicts["nginx.x86_64"];
    assert_eq!(conflict.len(), 2);
    assert_eq!(conflict[0].repo, "epel");
    assert_eq!(conflict[0].host_count, 2);
    assert_eq!(conflict[1].repo, "appstream");
    assert_eq!(conflict[1].host_count, 1);
}
```

Run: `cargo test -p inspectah-core -- test_package_merge_tracks_repo_conflict`
Expected: FAIL until Step 3 is implemented (return type changes).

- [ ] **Step 3: Compute repo conflicts inside merge_rpm_sections()**

**Why inside the merge layer:** The web handler (`fleet_handlers.rs`)
cannot call a post-merge scan because `FleetContext` does not retain
per-host RPM sections after `merge_snapshots()`. The merge function is
the only point where per-host sections are available. Compute conflicts
here and return them alongside the merged output.

Change `merge_rpm_sections()` return type from `Option<RpmSection>` to
`Option<(RpmSection, HashMap<String, Vec<RepoSourceEntry>>)>`. The
conflict map keys are `name.arch` identity keys; values are the distinct
repos with their host counts (only entries with 2+ distinct repos).

Add the conflict-detection logic inside `merge_rpm_sections()`, after
`merge_items()` produces `packages_added` but before the function
returns. At this point, the original `sections: Vec<Option<RpmSection>>`
parameter is still in scope:

```rust
let repo_conflicts = {
    let mut conflicts: HashMap<String, Vec<RepoSourceEntry>> = HashMap::new();
    for pkg in &packages_added {
        let key = format!("{}.{}", pkg.name, pkg.arch);
        let mut repo_counts: HashMap<String, usize> = HashMap::new();
        for section in sections.iter().flatten() {
            for host_pkg in &section.packages_added {
                if host_pkg.name == pkg.name
                    && host_pkg.arch == pkg.arch
                    && !host_pkg.source_repo.is_empty()
                {
                    *repo_counts
                        .entry(host_pkg.source_repo.to_lowercase())
                        .or_insert(0) += 1;
                }
            }
        }
        if repo_counts.len() >= 2 {
            let mut entries: Vec<RepoSourceEntry> = repo_counts
                .into_iter()
                .map(|(repo, host_count)| RepoSourceEntry { repo, host_count })
                .collect();
            entries.sort_by(|a, b| {
                b.host_count.cmp(&a.host_count)
                    .then_with(|| a.repo.cmp(&b.repo)) // alpha tie-break
            });
            conflicts.insert(key, entries);
        }
    }
    conflicts
};
```

Return `Some((merged_rpm_section, repo_conflicts))`.

**Explicit carrier chain (one path, no either/or):**

1. `inspectah-core/src/fleet/merge.rs` — `merge_rpm_sections()` return
   type changes from `Option<RpmSection>` to
   `Option<(RpmSection, HashMap<String, Vec<RepoSourceEntry>>)>`.

2. `inspectah-core/src/fleet/mod.rs` — `merge_snapshots()` destructures
   the tuple. The conflict map is stored on a **new field** on the
   returned `InspectionSnapshot`:
   `pub rpm_repo_conflicts: HashMap<String, Vec<RepoSourceEntry>>`.
   This field defaults to an empty map for single-machine snapshots.
   This is the carrier across the core→refine boundary — not a
   side-channel, not a separate return value.

3. `inspectah-refine/src/session.rs` — `RefineSession::new()` reads
   `snapshot.rpm_repo_conflicts` and copies it into `FleetContext`.
   This happens in the existing fleet-detection branch of `new()`
   (the same block that populates `fleet_meta`, `fleet_context`, etc.).
   No new constructor needed.

4. `inspectah-refine/src/types.rs` — `FleetContext` gains:
   `pub repo_conflicts: HashMap<String, Vec<RepoSourceEntry>>`.
   Populated by step 3. Immutable after session construction — not
   modified by refinement operations.

5. `inspectah-web/src/fleet_handlers.rs` — `build_fleet_view_response()`
   reads `ctx.repo_conflicts` via `session.fleet_context()`. For each
   RPM `FleetItem`, looks up `name.arch` in the map. Present → set
   `repo_conflict` to `Some(entries)`. Absent → `None`. Counts
   `repo_conflicts.len()` for `repo_conflict_count`. No conflict
   computation at this layer.

6. `inspectah-web/tests/fleet_api_test.rs` — Proves the full vertical.
   **Test setup must start from per-host snapshots**, not a
   handcrafted merged fixture. Build individual `InspectionSnapshot`
   values for each host (with different `source_repo` values on the
   same package), pass them through `merge_snapshots()`, construct
   `RefineSession::new()` from the merged result, build `AppState`,
   then GET `/api/fleet/view` and assert `source_repo`,
   `repo_conflict`, `repo_conflict_count` on the JSON response.

- [ ] **Step 4: Write test for non-conflicting and tie-breaking behavior**

```rust
#[test]
fn test_merge_no_conflict_single_repo() {
    use crate::types::rpm::{PackageEntry, PackageState, RpmSection};

    let sections = vec![
        Some(RpmSection {
            packages_added: vec![PackageEntry {
                name: "bash".into(),
                arch: "x86_64".into(),
                source_repo: "baseos".into(),
                state: PackageState::Added,
                include: true,
                ..Default::default()
            }],
            ..Default::default()
        }),
        Some(RpmSection {
            packages_added: vec![PackageEntry {
                name: "bash".into(),
                arch: "x86_64".into(),
                source_repo: "baseos".into(),
                state: PackageState::Added,
                include: true,
                ..Default::default()
            }],
            ..Default::default()
        }),
    ];

    let hostnames = vec!["host-a".into(), "host-b".into()];
    let (merged, conflicts) =
        merge_rpm_sections(sections, 2, &hostnames, None).unwrap();

    assert_eq!(merged.packages_added[0].source_repo, "baseos");
    // No conflict — same repo on both hosts
    assert!(conflicts.is_empty());
}

#[test]
fn test_merge_repo_conflict_tie() {
    use crate::types::rpm::{PackageEntry, PackageState, RpmSection};

    // 50/50 split — tie-break: count desc, then repo name alphabetical
    let sections = vec![
        Some(RpmSection {
            packages_added: vec![PackageEntry {
                name: "nginx".into(),
                arch: "x86_64".into(),
                source_repo: "epel".into(),
                state: PackageState::Added,
                include: true,
                ..Default::default()
            }],
            ..Default::default()
        }),
        Some(RpmSection {
            packages_added: vec![PackageEntry {
                name: "nginx".into(),
                arch: "x86_64".into(),
                source_repo: "appstream".into(),
                state: PackageState::Added,
                include: true,
                ..Default::default()
            }],
            ..Default::default()
        }),
    ];

    let hostnames = vec!["host-a".into(), "host-b".into()];
    let (merged, conflicts) =
        merge_rpm_sections(sections, 2, &hostnames, None).unwrap();

    // Merged row's source_repo: merge_items picks first-seen by sorted
    // hostname, so host-a's "epel" wins the merged PackageEntry
    let nginx = merged.packages_added.iter()
        .find(|p| p.name == "nginx").unwrap();
    assert_eq!(nginx.source_repo, "epel");

    // Conflict entries: equal count, alphabetical tie-break
    let conflict = &conflicts["nginx.x86_64"];
    assert_eq!(conflict.len(), 2);
    assert_eq!(conflict[0].repo, "appstream"); // alpha first at equal count
    assert_eq!(conflict[0].host_count, 1);
    assert_eq!(conflict[1].repo, "epel");
    assert_eq!(conflict[1].host_count, 1);
}
```

Run: `cargo test -p inspectah-core -- test_merge_no_conflict test_merge_repo_conflict_tie`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add inspectah-core/src/types/fleet.rs inspectah-core/src/fleet/merge.rs
git commit -m "feat(core): add repo-conflict detection during fleet merge"
```

---

### Task 4: Add repo_groups and repo_conflict to FleetViewResponse

**Files:**
- Modify: `inspectah-web/src/fleet_handlers.rs`
- Modify: `inspectah-web/tests/fleet_api_test.rs`

- [ ] **Step 1: Add repo_conflict field to FleetItem**

In `fleet_handlers.rs`, add a new DTO struct:
```rust
#[derive(Clone, Serialize)]
pub struct RepoSourceEntryDto {
    pub repo: String,
    pub host_count: usize,
}
```

Add `repo_conflict` and `source_repo` fields to `FleetItem`:
```rust
#[derive(Clone, Serialize)]
pub struct FleetItem {
    pub item_id: ItemId,
    pub include: bool,
    pub attention: FleetAttentionDto,
    pub prevalence: FleetPrevalenceDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variants: Option<FleetVariants>,
    pub source_repo: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_conflict: Option<Vec<RepoSourceEntryDto>>,
}
```

- [ ] **Step 2: Add repo_groups and conflict_count to FleetViewResponse**

```rust
#[derive(Serialize)]
pub struct FleetViewResponse {
    pub generation: u64,
    pub can_undo: bool,
    pub can_redo: bool,
    pub containerfile_preview: String,
    pub session_is_sensitive: bool,
    pub summary: FleetSummary,
    pub sections: Vec<FleetSection>,
    pub repo_groups: Vec<crate::handlers::RepoGroupInfo>,
    pub repo_conflict_count: usize,
}
```

- [ ] **Step 3: Populate the new fields in build_fleet_view_response()**

**Canonical data flow (one path, no handler-side computation):**
1. `merge_rpm_sections()` (inspectah-core) computes repo conflicts
2. `merge_snapshots()` stores them on `FleetContext.repo_conflicts`
3. `build_fleet_view_response()` (inspectah-web) reads from `FleetContext`
4. Handler only MAPS data — no conflict detection at the web layer

Import `build_repo_groups` from handlers (make it `pub` if needed).

In `build_fleet_view_response()`:
1. Call `build_repo_groups(session)` to get repo groups
2. Read `ctx.repo_conflicts` from `FleetContext` — this was populated
   during merge (Task 3). No `detect_repo_conflicts()` call here.
3. Pass the conflict map through to `build_fleet_sections()` so each
   `FleetItem` for an RPM package can look up its `repo_conflict` by
   `name.arch` key
4. Count `repo_conflicts.len()` for `repo_conflict_count`

Populate `source_repo` on each `FleetItem` by reading
`PackageEntry.source_repo` from the projected snapshot's rpm packages.

- [ ] **Step 4: Write integration test for fleet repo_groups and conflicts**

In `inspectah-web/tests/fleet_api_test.rs`:

```rust
#[tokio::test]
async fn fleet_view_includes_repo_groups() {
    let state = fleet_state_with_packages();
    let app = app(state);
    let (status, json) = get_json(&app, "/api/fleet/view").await;

    assert_eq!(status, StatusCode::OK);
    let repo_groups = json.get("repo_groups").unwrap().as_array().unwrap();
    assert!(!repo_groups.is_empty());
}
```

Create a helper `fleet_state_with_packages()` that builds a fleet snapshot with packages from multiple repos, including at least one repo-conflict package.

Run: `cargo test -p inspectah-web -- fleet_view_includes_repo_groups`
Expected: PASS

- [ ] **Step 5: Write ExcludeRepo/IncludeRepo round-trip test for fleet**

This test proves that repo disable/re-enable correctly flows through to
the fleet view's `FleetItem.include` state. The fleet handler must source
include state from the projected/refined snapshot, not from raw prevalence.

```rust
#[tokio::test]
async fn fleet_exclude_repo_round_trip() {
    let state = fleet_state_with_packages(); // includes epel packages
    let app = app(state);

    // Helper: extract fleet items with a given source_repo from response
    fn fleet_items_by_repo(json: &serde_json::Value, repo: &str) -> Vec<&serde_json::Value> {
        json["sections"].as_array().unwrap()
            .iter()
            .flat_map(|s| {
                let items = s.get("items").and_then(|i| i.as_array());
                let zone_items = s.get("zones").map(|z| {
                    ["consensus", "near_consensus", "divergent"].iter()
                        .flat_map(|k| z[k]["items"].as_array().unwrap_or(&vec![]).clone())
                        .collect::<Vec<_>>()
                }).unwrap_or_default();
                items.cloned().unwrap_or_default().into_iter().chain(zone_items)
            })
            .filter(|item| item["source_repo"].as_str() == Some(repo))
            .collect()
    }

    // 1. Initial view — epel packages included, repo enabled
    let (_, initial) = get_json(&app, "/api/fleet/view").await;
    let epel_items = fleet_items_by_repo(&initial, "epel");
    assert!(!epel_items.is_empty(), "should have epel packages");
    for item in &epel_items {
        assert_eq!(item["include"], true);
    }
    let epel_group = initial["repo_groups"].as_array().unwrap()
        .iter().find(|g| g["section_id"] == "epel").unwrap();
    assert_eq!(epel_group["enabled"], true);

    // 2. ExcludeRepo
    let (status, _) = post_json(&app, "/api/op", json!({
        "op": "ExcludeRepo",
        "target": { "section_id": "epel" }
    })).await;
    assert_eq!(status, StatusCode::OK);

    // 3. After exclude — FleetItem.include=false AND repo_groups.enabled=false
    let (_, after_exclude) = get_json(&app, "/api/fleet/view").await;
    let epel_items = fleet_items_by_repo(&after_exclude, "epel");
    for item in &epel_items {
        assert_eq!(item["include"], false, "epel packages should be excluded");
    }
    let epel_group = after_exclude["repo_groups"].as_array().unwrap()
        .iter().find(|g| g["section_id"] == "epel").unwrap();
    assert_eq!(epel_group["enabled"], false, "epel repo should be disabled");

    // 4. IncludeRepo
    let (status, _) = post_json(&app, "/api/op", json!({
        "op": "IncludeRepo",
        "target": { "section_id": "epel" }
    })).await;
    assert_eq!(status, StatusCode::OK);

    // 5. After include — all back to include=true, repo enabled=true
    let (_, after_include) = get_json(&app, "/api/fleet/view").await;
    let epel_items = fleet_items_by_repo(&after_include, "epel");
    for item in &epel_items {
        assert_eq!(item["include"], true, "epel packages should be re-included");
    }
    let epel_group = after_include["repo_groups"].as_array().unwrap()
        .iter().find(|g| g["section_id"] == "epel").unwrap();
    assert_eq!(epel_group["enabled"], true, "epel repo should be re-enabled");
}
```

Run: `cargo test -p inspectah-web -- fleet_exclude_repo_round_trip`
Expected: PASS. If it fails, the `fleet_handlers.rs` include-state
sourcing needs to be fixed to read from the projected/refined snapshot
(via `session.snapshot_projected()`) rather than raw prevalence.

- [ ] **Step 6: Commit**

```bash
git add inspectah-web/src/fleet_handlers.rs inspectah-web/src/handlers.rs inspectah-web/tests/fleet_api_test.rs
git commit -m "feat(web): add repo_groups, source_repo, and repo_conflict to fleet response"
```

---

## Phase 3: Frontend — TypeScript Types

### Task 5: Update TypeScript API types

**Files:**
- Modify: `inspectah-web/ui/src/api/types.ts`

- [ ] **Step 1: Add new types and update existing ones**

Add `RepoTier` type:
```typescript
export type RepoTier = "distro" | "official_optional" | "third_party";
```

Add `RepoSourceEntry`:
```typescript
export interface RepoSourceEntry {
  repo: string;
  host_count: number;
}
```

Update `RepoGroupInfo` to include `tier`:
```typescript
export interface RepoGroupInfo {
  section_id: string;
  provenance: RepoProvenance;
  is_distro: boolean;
  tier: RepoTier;
  package_count: number;
  enabled: boolean;
}
```

Update `FleetItem` to include `source_repo` and `repo_conflict`:
```typescript
export interface FleetItem {
  item_id: ItemId;
  include: boolean;
  attention: FleetAttention;
  prevalence: FleetItemPrevalence;
  variants?: FleetVariants;
  source_repo: string;
  repo_conflict?: RepoSourceEntry[];
}
```

Update `FleetViewResponse` to include `repo_groups` and `repo_conflict_count`:
```typescript
export interface FleetViewResponse {
  generation: number;
  can_undo: boolean;
  can_redo: boolean;
  containerfile_preview: string;
  session_is_sensitive: boolean;
  summary: FleetSummary;
  sections: FleetSection[];
  repo_groups: RepoGroupInfo[];
  repo_conflict_count: number;
}
```

Remove `leaf_dep_tree` from `ViewResponse`:
```typescript
export interface ViewResponse extends RefinedView {
  repo_groups: RepoGroupInfo[];
  version_changes: VersionChangeEntry[];
  users_groups_decisions: UserDecision[];
  session_is_sensitive: boolean;
}
```

- [ ] **Step 2: Run type check**

Run: `cd inspectah-web/ui && npx tsc --noEmit`
Expected: Type errors in components that reference `leaf_dep_tree` or the old `FleetItem` shape. List the errors — these will be fixed in subsequent tasks.

- [ ] **Step 3: Commit**

```bash
git add inspectah-web/ui/src/api/types.ts
git commit -m "feat(ui): update API types for unified package/repo management"
```

---

## Phase 4: Frontend — Shared Components

### Task 6: RepoBar component

**Files:**
- Create: `inspectah-web/ui/src/components/RepoBar.tsx`
- Create: `inspectah-web/ui/src/components/__tests__/RepoBar.test.tsx`

- [ ] **Step 1: Write failing tests**

```typescript
import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { RepoBar } from "../RepoBar";
import type { RepoGroupInfo } from "../../api/types";

const mockRepos: RepoGroupInfo[] = [
  { section_id: "baseos", provenance: "verified", is_distro: true, tier: "distro", package_count: 12, enabled: true },
  { section_id: "appstream", provenance: "verified", is_distro: true, tier: "distro", package_count: 28, enabled: true },
  { section_id: "crb", provenance: "verified", is_distro: false, tier: "official_optional", package_count: 4, enabled: true },
  { section_id: "epel", provenance: "incomplete", is_distro: false, tier: "third_party", package_count: 8, enabled: true },
];

describe("RepoBar", () => {
  it("renders distro repos as plain text in row 1", () => {
    render(<RepoBar repos={mockRepos} onToggle={vi.fn()} />);
    expect(screen.getByText(/baseos/)).toBeInTheDocument();
    expect(screen.getByText(/appstream/)).toBeInTheDocument();
  });

  it("renders toggleable repos as pills in row 2", () => {
    render(<RepoBar repos={mockRepos} onToggle={vi.fn()} />);
    const crbPill = screen.getByRole("switch", { name: /crb/i });
    expect(crbPill).toBeInTheDocument();
    const epelPill = screen.getByRole("switch", { name: /epel/i });
    expect(epelPill).toBeInTheDocument();
  });

  it("distro repos have no toggle", () => {
    render(<RepoBar repos={mockRepos} onToggle={vi.fn()} />);
    expect(screen.queryByRole("switch", { name: /baseos/i })).not.toBeInTheDocument();
  });

  it("calls onToggle with section_id when pill is clicked", () => {
    const onToggle = vi.fn();
    render(<RepoBar repos={mockRepos} onToggle={onToggle} />);
    fireEvent.click(screen.getByRole("switch", { name: /epel/i }));
    expect(onToggle).toHaveBeenCalledWith("epel");
  });

  it("shows conflict count badge when provided", () => {
    render(<RepoBar repos={mockRepos} onToggle={vi.fn()} conflictCount={3} dismissedCount={0} onRestoreDismissed={vi.fn()} />);
    expect(screen.getByText(/3 conflicts/i)).toBeInTheDocument();
  });

  it("shows 'Show N dismissed' restore button when dismissedCount > 0", () => {
    const onRestore = vi.fn();
    render(<RepoBar repos={mockRepos} onToggle={vi.fn()} conflictCount={3} dismissedCount={2} onRestoreDismissed={onRestore} />);
    const restoreBtn = screen.getByRole("button", { name: /show 2 dismissed/i });
    expect(restoreBtn).toBeInTheDocument();
    fireEvent.click(restoreBtn);
    expect(onRestore).toHaveBeenCalled();
  });

  it("hides restore button when dismissedCount is 0", () => {
    render(<RepoBar repos={mockRepos} onToggle={vi.fn()} conflictCount={3} dismissedCount={0} onRestoreDismissed={vi.fn()} />);
    expect(screen.queryByRole("button", { name: /show.*dismissed/i })).not.toBeInTheDocument();
  });
});
```

Run: `cd inspectah-web/ui && npx vitest run src/components/__tests__/RepoBar.test.tsx`
Expected: FAIL — component doesn't exist yet.

- [ ] **Step 2: Implement RepoBar**

Create `inspectah-web/ui/src/components/RepoBar.tsx`. Implement the two-row layout with static text (row 1) and toggle pills (row 2). Use `role="switch"` with `aria-checked` on pills. Include optional `conflictCount` prop for the badge.

- [ ] **Step 3: Run tests**

Run: `cd inspectah-web/ui && npx vitest run src/components/__tests__/RepoBar.test.tsx`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add inspectah-web/ui/src/components/RepoBar.tsx inspectah-web/ui/src/components/__tests__/RepoBar.test.tsx
git commit -m "feat(ui): add RepoBar component with two-row distro/toggleable layout"
```

---

### Task 7: SortHeader component

**Files:**
- Create: `inspectah-web/ui/src/components/SortHeader.tsx`
- Create: `inspectah-web/ui/src/components/__tests__/SortHeader.test.tsx`

- [ ] **Step 1: Write failing tests**

```typescript
import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { SortHeader } from "../SortHeader";

describe("SortHeader", () => {
  it("renders two column headers", () => {
    render(
      <SortHeader
        leftLabel="Packages"
        rightLabel="Repo"
        activeColumn="left"
        direction="asc"
        onSort={vi.fn()}
      />
    );
    expect(screen.getByRole("columnheader", { name: /packages/i })).toBeInTheDocument();
    expect(screen.getByRole("columnheader", { name: /repo/i })).toBeInTheDocument();
  });

  it("shows chevron on active column only", () => {
    const { container } = render(
      <SortHeader
        leftLabel="Packages"
        rightLabel="Prevalence"
        activeColumn="left"
        direction="asc"
        onSort={vi.fn()}
      />
    );
    const left = screen.getByRole("columnheader", { name: /packages/i });
    expect(left.textContent).toContain("▲");
    const right = screen.getByRole("columnheader", { name: /prevalence/i });
    expect(right.textContent).not.toContain("▲");
    expect(right.textContent).not.toContain("▼");
  });

  it("calls onSort when clicked", () => {
    const onSort = vi.fn();
    render(
      <SortHeader
        leftLabel="Packages"
        rightLabel="Repo"
        activeColumn="left"
        direction="asc"
        onSort={onSort}
      />
    );
    fireEvent.click(screen.getByRole("columnheader", { name: /repo/i }));
    expect(onSort).toHaveBeenCalledWith("right");
  });

  it("has correct aria-sort attributes", () => {
    render(
      <SortHeader
        leftLabel="Packages"
        rightLabel="Repo"
        activeColumn="left"
        direction="asc"
        onSort={vi.fn()}
      />
    );
    expect(screen.getByRole("columnheader", { name: /packages/i })).toHaveAttribute("aria-sort", "ascending");
    expect(screen.getByRole("columnheader", { name: /repo/i })).toHaveAttribute("aria-sort", "none");
  });
});
```

Run: `cd inspectah-web/ui && npx vitest run src/components/__tests__/SortHeader.test.tsx`
Expected: FAIL

- [ ] **Step 2: Implement SortHeader**

Create `inspectah-web/ui/src/components/SortHeader.tsx`. Two `<button>` elements inside a `<div role="row">`, each with `role="columnheader"`. Support Left/Right arrow navigation with wrap. Two-state cycle per column (asc → desc → asc).

- [ ] **Step 3: Run tests, commit**

Run: `cd inspectah-web/ui && npx vitest run src/components/__tests__/SortHeader.test.tsx`
Expected: PASS

```bash
git add inspectah-web/ui/src/components/SortHeader.tsx inspectah-web/ui/src/components/__tests__/SortHeader.test.tsx
git commit -m "feat(ui): add SortHeader component with two-column sortable headers"
```

---

### Task 8: ExcludedZone component

**Files:**
- Create: `inspectah-web/ui/src/components/ExcludedZone.tsx`
- Create: `inspectah-web/ui/src/components/__tests__/ExcludedZone.test.tsx`

- [ ] **Step 1: Write failing tests**

```typescript
import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { ExcludedZone } from "../ExcludedZone";

describe("ExcludedZone", () => {
  it("renders nothing when never toggled", () => {
    const { container } = render(
      <ExcludedZone packages={[]} hasEverToggled={false} />
    );
    expect(container.firstChild).toBeNull();
  });

  it("shows empty state after toggle and re-enable", () => {
    render(<ExcludedZone packages={[]} hasEverToggled={true} />);
    expect(screen.getByText(/no excluded packages/i)).toBeInTheDocument();
  });

  it("shows excluded packages with strikethrough", () => {
    const pkgs = [
      { name: "nginx", repo: "epel" },
      { name: "jq", repo: "epel" },
    ];
    render(<ExcludedZone packages={pkgs} hasEverToggled={true} />);
    expect(screen.getByText("nginx")).toBeInTheDocument();
    expect(screen.getByText("jq")).toBeInTheDocument();
    expect(screen.getByText(/excluded · 2 packages/i)).toBeInTheDocument();
  });

  it("collapses when 50+ packages with expander", () => {
    const pkgs = Array.from({ length: 55 }, (_, i) => ({
      name: `pkg-${i}`,
      repo: "epel",
    }));
    render(<ExcludedZone packages={pkgs} hasEverToggled={true} />);
    expect(screen.getByText(/show 55 excluded/i)).toBeInTheDocument();
  });
});
```

Run: `cd inspectah-web/ui && npx vitest run src/components/__tests__/ExcludedZone.test.tsx`
Expected: FAIL

- [ ] **Step 2: Implement ExcludedZone**

Create `inspectah-web/ui/src/components/ExcludedZone.tsx`. Three visibility states: never-shown, visible-with-content, visible-but-empty. Collapse at 50+ with `aria-expanded` expander.

- [ ] **Step 3: Run tests, commit**

```bash
git add inspectah-web/ui/src/components/ExcludedZone.tsx inspectah-web/ui/src/components/__tests__/ExcludedZone.test.tsx
git commit -m "feat(ui): add ExcludedZone component with three visibility states"
```

---

### Task 9: PackageList component

**Files:**
- Create: `inspectah-web/ui/src/components/PackageList.tsx`
- Create: `inspectah-web/ui/src/components/__tests__/PackageList.test.tsx`

- [ ] **Step 1: Write failing tests**

Test the unified package list with mode-aware rendering: single-machine shows repo as right column, fleet shows repo inline-left with prevalence right. Test sort behavior, checkbox toggling, and the excluded zone integration.

Key test cases:
- Renders package name + repo text for each package
- Single-machine: repo in right column
- Fleet: repo inline, prevalence in right column
- Sort by package name (both modes)
- Sort by repo tier-first (single-machine)
- Sort by prevalence ascending — rarest first (fleet default)
- Checkbox toggle calls onToggle
- Excluded packages shown in ExcludedZone when repos disabled
- Repo text color: muted for distro, green for official-optional, amber for third-party
- Non-distro repos: dotted underline (official-optional), solid underline (third-party)

- [ ] **Step 2: Implement PackageList**

Create `inspectah-web/ui/src/components/PackageList.tsx`. A mode-aware component that:
- Accepts `mode: "single" | "fleet"`, `packages`, `repoGroups`, `onToggle`, `onRepoToggle`
- Manages sort state internally (default: alpha for single, prevalence-asc for fleet)
- Renders `SortHeader` with mode-appropriate labels
- Renders package rows with checkbox + name + repo text (mode-aware positioning)
- Computes excluded packages from disabled repos
- Renders `ExcludedZone`

**Dismissed-state ownership (canonical):** `PackageList` owns a
`dismissedConflicts: Set<string>` state (identity keys of dismissed
warnings). This is the single source of truth. Threading:
- `PackageList` passes `dismissedConflicts` down to each
  `RepoConflictPopover` instance (which checks if its key is in the set)
- `PackageList` passes `onDismiss(key: string)` down to each popover
- `PackageList` computes `dismissedCount = dismissedConflicts.size` and
  passes it UP to the parent as a render prop or via callback so the
  parent can pass `dismissedCount` and `onRestoreDismissed` to `RepoBar`
- `onRestoreDismissed` clears the `dismissedConflicts` set
- Fleet mode sort with conflicts: within each prevalence group, packages
  with `repo_conflict` (that are NOT dismissed) sort before packages
  without conflicts. This ensures conflict-first surfacing under the
  default prevalence-ascending sort.

- [ ] **Step 3: Run tests, commit**

```bash
git add inspectah-web/ui/src/components/PackageList.tsx inspectah-web/ui/src/components/__tests__/PackageList.test.tsx
git commit -m "feat(ui): add unified PackageList component with mode-aware sort and layout"
```

---

## Phase 5: Frontend — Fleet-Specific Components

### Task 10: RepoConflictPopover component

**Files:**
- Create: `inspectah-web/ui/src/components/fleet/RepoConflictPopover.tsx`
- Create: `inspectah-web/ui/src/components/fleet/__tests__/RepoConflictPopover.test.tsx`

- [ ] **Step 1: Write failing tests**

Key test cases:
- Renders warning button when `repo_conflict` is present
- Does not render when `repo_conflict` is undefined
- Popover opens on click with repo + host count details
- Popover opens on Enter/Space
- Dismiss button inside popover hides the warning
- Escape closes popover without dismissing
- Focus returns to trigger on close, moves to next element on dismiss
- `aria-haspopup="dialog"` on trigger
- `role="dialog"` on popover

- [ ] **Step 2: Implement RepoConflictPopover**

A button-triggered popover disclosure with complete interaction contract:

**Trigger:** Native `<button>` with warning icon. Attributes:
`aria-haspopup="dialog"`, `aria-expanded="true|false"`. Accessible name:
"Repo conflict for {packageName} — {N} sources".

**Popover:** `role="dialog"`, `aria-label="Repo source conflict for
{packageName}"`. Content: repo names with host counts, one per line.
Dismiss button inside.

**Focus landing:** When popover opens, focus moves to the dismiss button
inside the popover (first interactive element).

**Close without dismiss (Escape):** Popover closes, focus returns to
the trigger button. Warning icon remains visible.

**Dismiss:** Dismiss button inside popover. On activation: popover closes,
trigger button is removed from DOM, focus moves to the next focusable
element in the row (the package checkbox). Warning is hidden for the
session.

**Session-scoped dismissed state:** Lifted to the parent `PackageList`
component as `Set<string>` of dismissed identity keys (name.arch).
Passed down as prop. Not persisted beyond the session.

**"Show N dismissed" restore control:** Rendered in the `RepoBar`
component next to the conflict-count badge. When activated, clears the
dismissed set. All previously dismissed warnings reappear. Accessible
name: "Show {N} dismissed repo conflict warnings". Standard toggle
button.

**Conflict-first surfacing:** When the PackageList is in fleet mode with
default prevalence-ascending sort, packages with `repo_conflict` sort
to the top of their prevalence group. This ensures consensus-but-repo-
split packages are not buried below divergent packages.

- [ ] **Step 3: Run tests, commit**

```bash
git add inspectah-web/ui/src/components/fleet/RepoConflictPopover.tsx inspectah-web/ui/src/components/fleet/__tests__/RepoConflictPopover.test.tsx
git commit -m "feat(ui): add RepoConflictPopover with button-triggered disclosure pattern"
```

---

## Phase 6: Integration — Wire Up Both Modes

### Task 11: Wire unified components into fleet and single-machine views

**Files:**
- Modify: `inspectah-web/ui/src/components/MainContent.tsx` (or equivalent single-machine container)
- Modify: `inspectah-web/ui/src/components/fleet/FleetApp.tsx` (or equivalent fleet container)
- Modify: `inspectah-web/ui/src/App.tsx`

This is the largest task. It replaces existing package rendering in both modes with the new shared components (RepoBar, PackageList, SortHeader, ExcludedZone).

- [ ] **Step 1: Identify and map current package rendering code paths**

**Single-machine path (to be replaced):**
- `MainContent.tsx` — container, renders package sections
- `DecisionList.tsx` — renders attention-grouped package items
- `PackageDetail.tsx` — per-package detail view
- `DependencyModal.tsx` — dep tree modal (removed entirely)
- `AttentionSummary.tsx` — attention level indicators (removed from this view)
- `RepoGroup.tsx` — existing repo group accordion (replaced by RepoBar)

**Fleet path (to be replaced):**
- `FleetApp.tsx` — fleet container
- `fleet/FleetSection.tsx` — renders fleet sections with zone groups
- `fleet/FleetItemRow.tsx` — individual fleet item rendering
- `fleet/ZoneGroup.tsx` — divergent/near-consensus/consensus grouping (removed)

**Legacy tests to update or remove:**
- `__tests__/DecisionSections.test.tsx` — update or remove
- `__tests__/DependencyModal.test.tsx` — remove
- `__tests__/AttentionSummary.test.tsx` — remove from package context
- `__tests__/RepoGroup.test.tsx` — remove (replaced by RepoBar tests)
- `fleet/__tests__/FleetSection.test.tsx` — update
- `fleet/__tests__/ZoneGroup.test.tsx` — remove

**Components to KEEP unchanged:**
- `AppShell.tsx` — outer shell, sidebar, navigation
- `ContainerfilePanel.tsx` — containerfile preview
- `fleet/FleetBanner.tsx` — fleet summary banner
- `fleet/FleetSidebar.tsx` — fleet selection sidebar
- `fleet/DiffDrawer.tsx` — variant diff comparison

**Fleet integration seam decision:** Package rendering moves OUT of
`FleetSection.tsx` / `FleetSectionContent` and into the shared
`PackageList` component. `FleetApp.tsx` renders `RepoBar` + `PackageList`
directly for the RPM section, bypassing `FleetSection` for packages.
Non-package fleet sections (config, services, etc.) continue to use
`FleetSection` unchanged. This means:
- `FleetSection.tsx` loses its RPM package rendering path but keeps
  config/service/quadlet rendering
- Nav/filter/reveal behaviors in `FleetApp` that currently route through
  `FleetSection` for packages must be re-pointed to `PackageList`
- Fleet tests that assert package rendering through `FleetSection` need
  to be updated to test through `PackageList` instead
- Focus management: the fleet section-level keyboard nav must include
  the new `PackageList` in its focus group

- [ ] **Step 2: Wire RepoBar + PackageList into single-machine view**

Replace the existing package section (likely `DecisionSections` or similar) with:
```tsx
<RepoBar repos={viewData.repo_groups} onToggle={handleRepoToggle} />
<PackageList
  mode="single"
  packages={viewData.packages}
  repoGroups={viewData.repo_groups}
  onToggle={handlePackageToggle}
  onRepoToggle={handleRepoToggle}
/>
```

- [ ] **Step 3: Wire RepoBar + PackageList into fleet view**

Replace the existing fleet package section with:
```tsx
<RepoBar
  repos={fleetData.repo_groups}
  onToggle={handleRepoToggle}
  conflictCount={fleetData.repo_conflict_count}
/>
<PackageList
  mode="fleet"
  packages={fleetPackages}
  repoGroups={fleetData.repo_groups}
  onToggle={handlePackageToggle}
  onRepoToggle={handleRepoToggle}
/>
```

- [ ] **Step 4: Remove DependencyModal references**

Remove or comment out imports and rendering of the `DependencyModal` component. The dep tree is no longer surfaced.

- [ ] **Step 5: Run full test suite**

Run: `cd inspectah-web/ui && npx vitest run`
Expected: Some existing tests may break due to removed components. Fix or update tests for the new component structure.

Run: `cargo test -p inspectah-web`
Expected: Rust tests pass.

- [ ] **Step 6: Commit**

```bash
git add -A inspectah-web/ui/src/
git commit -m "feat(ui): wire unified package/repo components into both modes"
```

---

### Task 12: Build and verify

- [ ] **Step 1: Build the UI**

Run: `cd inspectah-web/ui && npm run build`
Expected: Clean build, no TypeScript errors, production bundle generated.

- [ ] **Step 2: Run full Rust test suite**

Run: `cargo test`
Expected: All tests pass across all crates.

- [ ] **Step 3: Run full UI test suite**

Run: `cd inspectah-web/ui && npx vitest run`
Expected: All tests pass.

- [ ] **Step 4: Vertical round-trip assertions**

All vertical proofs are Rust API integration tests in
`inspectah-web/tests/fleet_api_test.rs` (fleet) and
`inspectah-web/tests/api_test.rs` (single-machine). These prove the
full merge → session → handler → JSON response chain.

**ExcludeRepo/IncludeRepo round-trip** (`fleet_api_test.rs`):
1. Single-machine (`api_test.rs`): ExcludeRepo("epel") → GET `/api/view`
   → assert epel packages have `include=false` AND
   `repo_groups[epel].enabled=false` → IncludeRepo("epel") → assert
   all back to `include=true` AND `enabled=true`
2. Fleet (`fleet_api_test.rs`): same round-trip via `/api/fleet/view`
   (already written in Task 4 Step 5)

**source_repo / repo_conflict / repo_conflict_count** (`fleet_api_test.rs`):
1. Construct fleet snapshot with: `nginx` from epel on 2 hosts +
   appstream on 1 host, `bash` from baseos on all 3 hosts
2. GET `/api/fleet/view` → assert:
   - `nginx` FleetItem has `source_repo: "epel"` (majority)
   - `nginx` FleetItem has `repo_conflict` array with 2 entries
   - `bash` FleetItem has `source_repo: "baseos"`
   - `bash` FleetItem has `repo_conflict: null`
   - Top-level `repo_conflict_count == 1`

**Excluded zone three visibility states:**
1. Fresh session → excluded zone not rendered
2. Disable repo → zone appears with packages
3. Re-enable repo → zone shows "No excluded packages"

**Dismissed warning badge update:**
1. Fleet with 3 conflicts → `repo_conflict_count=3`
2. Dismiss 1 warning → "Show 1 dismissed" restore control appears
3. Restore → all 3 warnings visible again

**Conflict-first ordering within equal prevalence:**
1. Fleet with 3 packages at prevalence 4/5: two without conflict, one
   with conflict → the conflicted package sorts first within that group
2. Verify via rendered DOM order in a Vitest test

**Popover focus landing:**
1. Click repo-conflict trigger button → popover opens → verify
   `document.activeElement` is the dismiss button inside the popover
2. Press Escape → popover closes → verify `document.activeElement` is
   the trigger button
3. Click dismiss → popover closes, trigger removed → verify
   `document.activeElement` is the package checkbox in the same row

- [ ] **Step 5: Accessibility walkthrough**

Verify the following keyboard/screen-reader contracts (manual or e2e):

**Tab order:** Repo bar toggle pills → sort column headers → package
checkboxes → repo-conflict popover triggers → excluded zone expander.
Distro text (row 1) is skipped.

**Focus restoration:**
- Repo-conflict popover Escape → focus returns to trigger button
- Repo-conflict popover dismiss → focus moves to package checkbox
- Repo toggle → focus stays on toggle pill

**Live announcements (aria-live="polite"):**
- Repo disable: "N packages excluded from epel"
- Repo enable: "epel enabled. N packages restored"
- Excluded zone count updates on toggle
- Conflict count badge updates on dismiss/restore

**Non-color cues:**
- Distro repo text: no underline
- Official-optional repo text: dotted underline
- Third-party repo text: solid underline
- Prevalence: N/M numeric count present (not color-only)

**Reduced motion:**
- With `prefers-reduced-motion: reduce` active, verify no transitions
  on sort reorder or excluded zone movement

- [ ] **Step 6: Manual smoke test**

Start the refine server with a test snapshot and verify:
- Single-machine: repo bar shows, package list renders with repo column, sort works
- Fleet: repo bar shows, prevalence column renders, default sort is rarest-first
- Repo toggle: disabling a repo moves packages to excluded zone
- Re-enabling: packages return, all set to included
- Repo-conflict warning: visible on split packages, popover opens, dismiss works

- [ ] **Step 7: Final commit**

```bash
git add -A
git commit -m "chore: build and verify unified package/repo management"
```
