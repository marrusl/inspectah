# Fleet Leaf-Only Package Aggregation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Filter fleet `packages_added` to leaf-only packages at aggregation time, replacing the fragile "first host wins" heuristic with a principled intersection of all hosts' leaf classifications.

**Architecture:** The change centers on `merge_rpm_sections` (inspectah-core). The intersection computes an authoritative host subset, derives leaf/dep-tree/coverage data from it, then filters `packages_added` before repo-conflict detection. The service intent engine gets a fleet guard. Partial authority metadata is surfaced on three operator-facing surfaces: Containerfile comments, fleet report summary, and refine view API.

**Tech Stack:** Rust, serde, insta (snapshot tests), minijinja (report templates), std::collections (HashSet, BTreeSet, BTreeMap)

**Spec:** `/Users/mrussell/Work/bootc-migration/inspectah/process-docs/specs/proposed/2026-06-08-fleet-leaf-intersection.md`

**Verification convention:** All `cargo` commands in this plan run without pipe-to-tail. If output is long, the implementer may use context-mode tools to manage it, but the exit code must come from `cargo`, not from a downstream pipe.

---

### Task 1: Add coverage metadata fields and wire into merge constructor

**Files:**
- Modify: `inspectah-core/src/types/rpm.rs` (RpmSection struct)
- Modify: `inspectah-core/src/fleet/merge.rs` (RpmSection constructor in `merge_rpm_sections`)

Task 1 modifies both files in one slice because `merge_rpm_sections` constructs `RpmSection` exhaustively — adding fields to the struct without updating the constructor does not compile.

- [ ] **Step 1: Add the two new fields to RpmSection**

In `inspectah-core/src/types/rpm.rs`, add after the `no_baseline` field:

```rust
    /// Number of hosts with authoritative leaf classification data.
    /// Only meaningful for fleet-aggregated snapshots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leaf_authority_hosts: Option<u32>,
    /// Total number of hosts in the fleet.
    /// Only meaningful for fleet-aggregated snapshots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leaf_total_hosts: Option<u32>,
```

- [ ] **Step 2: Update the RpmSection constructor in merge_rpm_sections**

In `inspectah-core/src/fleet/merge.rs`, find the `Some((RpmSection { ... }` block at the end of `merge_rpm_sections`. Add `leaf_authority_hosts: None, leaf_total_hosts: None,` to the struct literal. These are placeholder values that Task 2 will replace with real computation.

- [ ] **Step 3: Verify compilation and tests**

Run: `cargo test --workspace`
Expected: All tests pass. The new fields have `serde(default)` so existing JSON fixtures deserialize correctly. The placeholder `None` values maintain current behavior.

- [ ] **Step 4: Run clippy**

Run: `cargo clippy --workspace -- -D warnings`
Expected: Clean.

- [ ] **Step 5: Commit**

```bash
git add inspectah-core/src/types/rpm.rs inspectah-core/src/fleet/merge.rs
git commit -m "feat(core): add leaf_authority_hosts and leaf_total_hosts to RpmSection

Coverage metadata for fleet leaf intersection. Tracks how many hosts
contributed authoritative leaf data vs total hosts in the fleet.
Placeholder None values in merge constructor — real computation in
next commit.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 2: Implement leaf intersection in merge_rpm_sections

**Files:**
- Modify: `inspectah-core/src/fleet/merge.rs` (replace first_host_option calls, add intersection logic, filter packages_added)

The implementation has a strict order of operations:
1. Compute authoritative host subset (hosts with `leaf_packages.is_some()`)
2. Compute leaf intersection from that subset
3. Derive dep tree from first authoritative host, filtered to intersection
4. Compute coverage metadata
5. Filter `packages_added` using the intersection
6. Repo-conflict detection runs on the filtered set

- [ ] **Step 1: Write the failing test**

In `inspectah-core/tests/fleet_merge_test.rs`, add:

```rust
#[test]
fn test_fleet_leaf_intersection_filters_packages_added() {
    let host_a = Some(RpmSection {
        packages_added: vec![
            PackageEntry { name: "git".into(), arch: "x86_64".into(), include: true, ..Default::default() },
            PackageEntry { name: "perl-libs".into(), arch: "x86_64".into(), include: true, ..Default::default() },
        ],
        leaf_packages: Some(vec!["git.x86_64".into()]),
        auto_packages: Some(vec!["perl-libs.x86_64".into()]),
        leaf_dep_tree: serde_json::json!({"git.x86_64": ["perl-libs.x86_64"]}),
        ..Default::default()
    });
    let host_b = Some(RpmSection {
        packages_added: vec![
            PackageEntry { name: "git".into(), arch: "x86_64".into(), include: true, ..Default::default() },
            PackageEntry { name: "perl-libs".into(), arch: "x86_64".into(), include: true, ..Default::default() },
        ],
        leaf_packages: Some(vec!["git.x86_64".into()]),
        auto_packages: Some(vec!["perl-libs.x86_64".into()]),
        leaf_dep_tree: serde_json::json!({"git.x86_64": ["perl-libs.x86_64"]}),
        ..Default::default()
    });

    let hostnames = vec!["host-a".into(), "host-b".into()];
    let (merged, _conflicts) =
        merge_rpm_sections(vec![host_a, host_b], 2, &hostnames, None).unwrap();

    // packages_added should contain ONLY the leaf package
    assert_eq!(merged.packages_added.len(), 1);
    assert_eq!(merged.packages_added[0].name, "git");

    // leaf_packages should be the intersection
    assert_eq!(merged.leaf_packages, Some(vec!["git.x86_64".into()]));

    // auto_packages should be None for fleet
    assert_eq!(merged.auto_packages, None);

    // leaf_dep_tree should only contain entries for intersection packages
    let tree = merged.leaf_dep_tree.as_object().unwrap();
    assert_eq!(tree.len(), 1);
    assert!(tree.contains_key("git.x86_64"));

    // coverage metadata — uses total_hosts param (2), not sections.len()
    assert_eq!(merged.leaf_authority_hosts, Some(2));
    assert_eq!(merged.leaf_total_hosts, Some(2));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --package inspectah-core --test fleet_merge_test test_fleet_leaf_intersection_filters_packages_added -- --exact`
Expected: FAIL — packages_added still contains both packages.

- [ ] **Step 3: Implement leaf intersection in merge_rpm_sections**

In `inspectah-core/src/fleet/merge.rs`, replace the three `first_host_option` calls for leaf fields (around line 946-950) with the following block. This must go BEFORE the `packages_added` filtering and BEFORE the repo-conflict detection.

```rust
    // --- Leaf intersection across authoritative hosts ---
    // An authoritative host has leaf_packages: Some(_).
    // Hosts with leaf_packages: None (degraded) are skipped entirely.
    let authoritative_indices: Vec<usize> = sections
        .iter()
        .enumerate()
        .filter(|(_, s)| {
            s.as_ref()
                .and_then(|s| s.leaf_packages.as_ref())
                .is_some()
        })
        .map(|(i, _)| i)
        .collect();

    let leaf_authority_hosts = Some(authoritative_indices.len() as u32);
    let leaf_total_hosts = Some(total_hosts as u32);

    let (leaf_packages, auto_packages, leaf_dep_tree) = if authoritative_indices.is_empty() {
        // All hosts degraded — no leaf truth available.
        (
            None,
            None,
            serde_json::Value::Object(serde_json::Map::new()),
        )
    } else {
        // Compute intersection of authoritative hosts' leaf sets.
        let mut leaf_sets: Vec<HashSet<String>> = authoritative_indices
            .iter()
            .map(|&i| {
                sections[i]
                    .as_ref()
                    .unwrap()
                    .leaf_packages
                    .as_ref()
                    .unwrap()
                    .iter()
                    .cloned()
                    .collect()
            })
            .collect();

        let mut intersection = leaf_sets.remove(0);
        for set in &leaf_sets {
            intersection.retain(|pkg| set.contains(pkg));
        }
        let mut sorted_leaf: Vec<String> = intersection.into_iter().collect();
        sorted_leaf.sort();

        // Fleet auto_packages: None (not independently meaningful).
        let auto = None;

        // Dep tree from first authoritative host (sorted by hostname),
        // filtered to intersection entries only.
        let leaf_ids: HashSet<&str> = sorted_leaf.iter().map(|s| s.as_str()).collect();
        let dep_tree = {
            let mut auth_pairs: Vec<(usize, &str)> = authoritative_indices
                .iter()
                .map(|&i| {
                    (
                        i,
                        hostnames.get(i).map(|s| s.as_str()).unwrap_or(""),
                    )
                })
                .collect();
            auth_pairs.sort_by_key(|(_, h)| *h);

            let donor_idx = auth_pairs[0].0;
            let donor_tree = &sections[donor_idx].as_ref().unwrap().leaf_dep_tree;

            if let Some(obj) = donor_tree.as_object() {
                let filtered: serde_json::Map<String, serde_json::Value> = obj
                    .iter()
                    .filter(|(k, _)| leaf_ids.contains(k.as_str()))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                serde_json::Value::Object(filtered)
            } else {
                serde_json::Value::Object(serde_json::Map::new())
            }
        };

        (Some(sorted_leaf), auto, dep_tree)
    };

    let versionlock_command_output =
        first_host_option(&sections, hostnames, |s| &s.versionlock_command_output);
```

- [ ] **Step 4: Filter packages_added to leaf-only**

Still in `merge_rpm_sections`, AFTER the leaf intersection block from Step 3 and BEFORE the repo-conflict detection block, add:

```rust
    // Filter packages_added to leaf-only when authoritative leaf data exists.
    let packages_added = if let Some(ref leaf_set) = leaf_packages {
        let leaf_ids: HashSet<&str> = leaf_set.iter().map(|s| s.as_str()).collect();
        packages_added
            .into_iter()
            .filter(|pkg| {
                let id = format!("{}.{}", pkg.name, pkg.arch);
                leaf_ids.contains(id.as_str())
            })
            .collect()
    } else {
        packages_added
    };
```

- [ ] **Step 5: Update the RpmSection constructor with real values**

In the `Some((RpmSection { ... }` block, replace the placeholder `None` values from Task 1:

```rust
            leaf_authority_hosts,
            leaf_total_hosts,
```

- [ ] **Step 6: Run test to verify it passes**

Run: `cargo test --package inspectah-core --test fleet_merge_test test_fleet_leaf_intersection_filters_packages_added -- --exact`
Expected: PASS.

- [ ] **Step 7: Run full test suite and clippy**

Run: `cargo test --workspace`
Expected: All pass. Some existing tests may need adjustment if they assert on `packages_added` counts for fleet merges — fix those by updating expectations to match leaf-only behavior.

Run: `cargo clippy --workspace -- -D warnings`
Expected: Clean.

- [ ] **Step 8: Commit**

```bash
git add inspectah-core/src/fleet/merge.rs inspectah-core/tests/fleet_merge_test.rs
git commit -m "feat(fleet): leaf intersection replaces first-host-wins heuristic

Compute intersection of all authoritative hosts' leaf classifications.
Filter fleet packages_added to leaf-only at aggregation time. Dep tree
donor selected from authoritative subset only. Auto packages set to
None for fleet. Coverage metadata tracks authority count vs total.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 3: Edge case tests

**Files:**
- Modify: `inspectah-core/tests/fleet_merge_test.rs`

- [ ] **Step 1: Test — package leaf on some hosts, auto on others → excluded**

```rust
#[test]
fn test_fleet_leaf_intersection_excludes_partial_leaf() {
    let host_a = Some(RpmSection {
        packages_added: vec![
            PackageEntry { name: "git".into(), arch: "x86_64".into(), include: true, ..Default::default() },
            PackageEntry { name: "htop".into(), arch: "x86_64".into(), include: true, ..Default::default() },
        ],
        leaf_packages: Some(vec!["git.x86_64".into(), "htop.x86_64".into()]),
        ..Default::default()
    });
    let host_b = Some(RpmSection {
        packages_added: vec![
            PackageEntry { name: "git".into(), arch: "x86_64".into(), include: true, ..Default::default() },
            PackageEntry { name: "htop".into(), arch: "x86_64".into(), include: true, ..Default::default() },
        ],
        leaf_packages: Some(vec!["git.x86_64".into()]),
        ..Default::default()
    });

    let hostnames = vec!["alpha".into(), "beta".into()];
    let (merged, _) = merge_rpm_sections(vec![host_a, host_b], 2, &hostnames, None).unwrap();

    let names: Vec<&str> = merged.packages_added.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, vec!["git"]);
    assert_eq!(merged.leaf_packages, Some(vec!["git.x86_64".into()]));
}
```

- [ ] **Step 2: Test — degraded hosts skipped, coverage metadata correct**

```rust
#[test]
fn test_fleet_leaf_intersection_skips_degraded_hosts() {
    let host_a = Some(RpmSection {
        packages_added: vec![
            PackageEntry { name: "vim".into(), arch: "x86_64".into(), include: true, ..Default::default() },
        ],
        leaf_packages: Some(vec!["vim.x86_64".into()]),
        ..Default::default()
    });
    let host_b = Some(RpmSection {
        packages_added: vec![
            PackageEntry { name: "vim".into(), arch: "x86_64".into(), include: true, ..Default::default() },
        ],
        leaf_packages: None,
        ..Default::default()
    });

    let hostnames = vec!["alpha".into(), "beta".into()];
    let (merged, _) = merge_rpm_sections(vec![host_a, host_b], 2, &hostnames, None).unwrap();

    assert_eq!(merged.leaf_packages, Some(vec!["vim.x86_64".into()]));
    assert_eq!(merged.leaf_authority_hosts, Some(1));
    assert_eq!(merged.leaf_total_hosts, Some(2));
}
```

- [ ] **Step 3: Test — all hosts degraded → full degraded triplet**

```rust
#[test]
fn test_fleet_leaf_intersection_all_degraded() {
    let host_a = Some(RpmSection {
        packages_added: vec![
            PackageEntry { name: "vim".into(), arch: "x86_64".into(), include: true, ..Default::default() },
        ],
        leaf_packages: None,
        ..Default::default()
    });
    let host_b = Some(RpmSection {
        packages_added: vec![
            PackageEntry { name: "vim".into(), arch: "x86_64".into(), include: true, ..Default::default() },
        ],
        leaf_packages: None,
        ..Default::default()
    });

    let hostnames = vec!["alpha".into(), "beta".into()];
    let (merged, _) = merge_rpm_sections(vec![host_a, host_b], 2, &hostnames, None).unwrap();

    assert_eq!(merged.leaf_packages, None);
    assert_eq!(merged.auto_packages, None);
    assert!(merged.leaf_dep_tree.is_object());
    assert_eq!(merged.leaf_dep_tree.as_object().unwrap().len(), 0);
    assert_eq!(merged.packages_added.len(), 1); // vim kept — no filtering
    assert_eq!(merged.leaf_authority_hosts, Some(0));
    assert_eq!(merged.leaf_total_hosts, Some(2));
}
```

- [ ] **Step 4: Test — Some([]) authoritative empty vs None degraded**

```rust
#[test]
fn test_fleet_leaf_intersection_authoritative_empty() {
    let host_a = Some(RpmSection {
        packages_added: vec![
            PackageEntry { name: "git".into(), arch: "x86_64".into(), include: true, ..Default::default() },
        ],
        leaf_packages: Some(vec!["git.x86_64".into()]),
        ..Default::default()
    });
    let host_b = Some(RpmSection {
        packages_added: vec![
            PackageEntry { name: "git".into(), arch: "x86_64".into(), include: true, ..Default::default() },
        ],
        leaf_packages: Some(vec![]),
        ..Default::default()
    });

    let hostnames = vec!["alpha".into(), "beta".into()];
    let (merged, _) = merge_rpm_sections(vec![host_a, host_b], 2, &hostnames, None).unwrap();

    assert_eq!(merged.leaf_packages, Some(vec![]));
    assert_eq!(merged.packages_added.len(), 0);
    assert_eq!(merged.leaf_authority_hosts, Some(2));
}
```

- [ ] **Step 5: Test — degraded host sorts first, authoritative host later → dep tree from authoritative host**

This is the bug the review caught: a degraded host with `leaf_dep_tree: {}` must not become the dep-tree donor.

```rust
#[test]
fn test_fleet_leaf_dep_tree_donor_from_authoritative_host() {
    // "alpha" sorts first but is degraded
    let host_a = Some(RpmSection {
        packages_added: vec![
            PackageEntry { name: "git".into(), arch: "x86_64".into(), include: true, ..Default::default() },
        ],
        leaf_packages: None, // degraded
        leaf_dep_tree: serde_json::Value::Object(serde_json::Map::new()), // empty — degraded
        ..Default::default()
    });
    // "beta" sorts second but is authoritative
    let host_b = Some(RpmSection {
        packages_added: vec![
            PackageEntry { name: "git".into(), arch: "x86_64".into(), include: true, ..Default::default() },
        ],
        leaf_packages: Some(vec!["git.x86_64".into()]),
        leaf_dep_tree: serde_json::json!({"git.x86_64": ["perl-libs.x86_64"]}),
        ..Default::default()
    });

    let hostnames = vec!["alpha".into(), "beta".into()];
    let (merged, _) = merge_rpm_sections(vec![host_a, host_b], 2, &hostnames, None).unwrap();

    // Dep tree must come from beta (authoritative), not alpha (degraded)
    let tree = merged.leaf_dep_tree.as_object().unwrap();
    assert_eq!(tree.len(), 1);
    assert!(tree.contains_key("git.x86_64"));
    assert_eq!(tree["git.x86_64"], serde_json::json!(["perl-libs.x86_64"]));
}
```

- [ ] **Step 6: Test — order independence**

```rust
#[test]
fn test_fleet_leaf_intersection_order_independent() {
    let make_host = || Some(RpmSection {
        packages_added: vec![
            PackageEntry { name: "zlib".into(), arch: "x86_64".into(), include: true, ..Default::default() },
            PackageEntry { name: "curl".into(), arch: "x86_64".into(), include: true, ..Default::default() },
            PackageEntry { name: "git".into(), arch: "x86_64".into(), include: true, ..Default::default() },
        ],
        leaf_packages: Some(vec!["zlib.x86_64".into(), "curl.x86_64".into(), "git.x86_64".into()]),
        ..Default::default()
    });

    let hostnames_ab = vec!["alpha".into(), "beta".into()];
    let hostnames_ba = vec!["beta".into(), "alpha".into()];

    let (merged_ab, _) = merge_rpm_sections(
        vec![make_host(), make_host()], 2, &hostnames_ab, None,
    ).unwrap();
    let (merged_ba, _) = merge_rpm_sections(
        vec![make_host(), make_host()], 2, &hostnames_ba, None,
    ).unwrap();

    assert_eq!(merged_ab.leaf_packages, merged_ba.leaf_packages);
    assert_eq!(
        merged_ab.leaf_packages,
        Some(vec!["curl.x86_64".into(), "git.x86_64".into(), "zlib.x86_64".into()])
    );
}
```

- [ ] **Step 7: Test — multiarch identity**

```rust
#[test]
fn test_fleet_leaf_intersection_multiarch_identity() {
    let host_a = Some(RpmSection {
        packages_added: vec![
            PackageEntry { name: "glibc".into(), arch: "x86_64".into(), include: true, ..Default::default() },
            PackageEntry { name: "glibc".into(), arch: "i686".into(), include: true, ..Default::default() },
        ],
        leaf_packages: Some(vec!["glibc.x86_64".into()]),
        ..Default::default()
    });
    let host_b = Some(RpmSection {
        packages_added: vec![
            PackageEntry { name: "glibc".into(), arch: "x86_64".into(), include: true, ..Default::default() },
            PackageEntry { name: "glibc".into(), arch: "i686".into(), include: true, ..Default::default() },
        ],
        leaf_packages: Some(vec!["glibc.x86_64".into()]),
        ..Default::default()
    });

    let hostnames = vec!["alpha".into(), "beta".into()];
    let (merged, _) = merge_rpm_sections(vec![host_a, host_b], 2, &hostnames, None).unwrap();

    assert_eq!(merged.packages_added.len(), 1);
    assert_eq!(merged.packages_added[0].arch, "x86_64");
    assert_eq!(merged.leaf_packages, Some(vec!["glibc.x86_64".into()]));
}
```

- [ ] **Step 8: Test — host-present vs host-absent leaf packages**

```rust
#[test]
fn test_fleet_leaf_intersection_host_absent_package() {
    // git is leaf on host_a but absent entirely on host_b
    let host_a = Some(RpmSection {
        packages_added: vec![
            PackageEntry { name: "git".into(), arch: "x86_64".into(), include: true, ..Default::default() },
            PackageEntry { name: "vim".into(), arch: "x86_64".into(), include: true, ..Default::default() },
        ],
        leaf_packages: Some(vec!["git.x86_64".into(), "vim.x86_64".into()]),
        ..Default::default()
    });
    let host_b = Some(RpmSection {
        packages_added: vec![
            PackageEntry { name: "vim".into(), arch: "x86_64".into(), include: true, ..Default::default() },
        ],
        leaf_packages: Some(vec!["vim.x86_64".into()]),
        ..Default::default()
    });

    let hostnames = vec!["alpha".into(), "beta".into()];
    let (merged, _) = merge_rpm_sections(vec![host_a, host_b], 2, &hostnames, None).unwrap();

    // git falls out of leaf intersection (not leaf on host_b — not present at all)
    assert_eq!(merged.leaf_packages, Some(vec!["vim.x86_64".into()]));
    let names: Vec<&str> = merged.packages_added.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"vim"));
    assert!(!names.contains(&"git"));
}
```

- [ ] **Step 9: Test — filtered packages absent from repo_conflicts**

```rust
#[test]
fn test_fleet_leaf_filtered_packages_absent_from_repo_conflicts() {
    let host_a = Some(RpmSection {
        packages_added: vec![
            PackageEntry { name: "git".into(), arch: "x86_64".into(), include: true, source_repo: "baseos".into(), ..Default::default() },
            PackageEntry { name: "perl-libs".into(), arch: "x86_64".into(), include: true, source_repo: "epel".into(), ..Default::default() },
        ],
        leaf_packages: Some(vec!["git.x86_64".into()]),
        ..Default::default()
    });
    let host_b = Some(RpmSection {
        packages_added: vec![
            PackageEntry { name: "git".into(), arch: "x86_64".into(), include: true, source_repo: "baseos".into(), ..Default::default() },
            PackageEntry { name: "perl-libs".into(), arch: "x86_64".into(), include: true, source_repo: "appstream".into(), ..Default::default() },
        ],
        leaf_packages: Some(vec!["git.x86_64".into()]),
        ..Default::default()
    });

    let hostnames = vec!["alpha".into(), "beta".into()];
    let (merged, conflicts) = merge_rpm_sections(vec![host_a, host_b], 2, &hostnames, None).unwrap();

    // perl-libs was filtered out as auto — must not appear in conflicts
    assert!(!conflicts.contains_key("perl-libs.x86_64"));
    // perl-libs must not be in packages_added
    assert!(!merged.packages_added.iter().any(|p| p.name == "perl-libs"));
}
```

- [ ] **Step 10: Test — full triplet coherence**

```rust
#[test]
fn test_fleet_leaf_triplet_coherence() {
    let host_a = Some(RpmSection {
        packages_added: vec![
            PackageEntry { name: "git".into(), arch: "x86_64".into(), include: true, ..Default::default() },
            PackageEntry { name: "vim".into(), arch: "x86_64".into(), include: true, ..Default::default() },
            PackageEntry { name: "perl-libs".into(), arch: "x86_64".into(), include: true, ..Default::default() },
        ],
        leaf_packages: Some(vec!["git.x86_64".into(), "vim.x86_64".into()]),
        auto_packages: Some(vec!["perl-libs.x86_64".into()]),
        leaf_dep_tree: serde_json::json!({
            "git.x86_64": ["perl-libs.x86_64"],
            "vim.x86_64": []
        }),
        ..Default::default()
    });
    let host_b = Some(RpmSection {
        packages_added: vec![
            PackageEntry { name: "git".into(), arch: "x86_64".into(), include: true, ..Default::default() },
            PackageEntry { name: "vim".into(), arch: "x86_64".into(), include: true, ..Default::default() },
            PackageEntry { name: "perl-libs".into(), arch: "x86_64".into(), include: true, ..Default::default() },
        ],
        leaf_packages: Some(vec!["git.x86_64".into()]), // vim is auto on host_b
        auto_packages: Some(vec!["perl-libs.x86_64".into(), "vim.x86_64".into()]),
        leaf_dep_tree: serde_json::json!({"git.x86_64": ["perl-libs.x86_64"]}),
        ..Default::default()
    });

    let hostnames = vec!["alpha".into(), "beta".into()];
    let (merged, _) = merge_rpm_sections(vec![host_a, host_b], 2, &hostnames, None).unwrap();

    // Intersection = git only
    assert_eq!(merged.leaf_packages, Some(vec!["git.x86_64".into()]));
    assert_eq!(merged.auto_packages, None);

    // Dep tree: only git entry
    let tree = merged.leaf_dep_tree.as_object().unwrap();
    assert!(tree.contains_key("git.x86_64"));
    assert!(!tree.contains_key("vim.x86_64"));

    // packages_added: only git
    assert_eq!(merged.packages_added.len(), 1);
    assert_eq!(merged.packages_added[0].name, "git");

    // Coherence: every package in packages_added is in leaf_packages
    let leaf_set: std::collections::HashSet<String> = merged
        .leaf_packages.as_ref().unwrap().iter().cloned().collect();
    for pkg in &merged.packages_added {
        let id = format!("{}.{}", pkg.name, pkg.arch);
        assert!(leaf_set.contains(&id), "{} not in leaf_packages", id);
    }
}
```

- [ ] **Step 11: Run all tests and clippy**

Run: `cargo test --workspace`
Run: `cargo clippy --workspace -- -D warnings`
Expected: All pass, clean clippy.

- [ ] **Step 12: Commit**

```bash
git add inspectah-core/tests/fleet_merge_test.rs
git commit -m "test(fleet): comprehensive leaf intersection edge cases

Covers: partial leaf, degraded hosts, all-degraded, authoritative empty,
degraded-donor-sorts-first, order independence, multiarch identity,
host-present-vs-absent, repo-conflict filtering, triplet coherence.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 4: Gate render_service_intent for fleet

**Files:**
- Modify: `inspectah-pipeline/src/render/service_intent.rs:294`
- Test: `inspectah-pipeline/tests/service_intent_test.rs`

- [ ] **Step 1: Write failing test**

In `inspectah-pipeline/tests/service_intent_test.rs`, add:

```rust
#[test]
fn test_fleet_snapshot_skips_service_omission_and_advisories() {
    use inspectah_core::snapshot::InspectionSnapshot;
    use inspectah_core::types::fleet::FleetSnapshotMeta;
    use inspectah_core::types::services::{
        ServiceSection, ServiceStateChange, ServiceUnitState, PresetDefault,
    };
    use inspectah_core::types::rpm::{PackageEntry, RpmSection};
    use inspectah_pipeline::render::service_intent::render_service_intent;

    let mut snap = InspectionSnapshot::default();
    snap.fleet_meta = Some(FleetSnapshotMeta {
        label: "test-fleet".into(),
        host_count: 2,
        hostnames: vec!["alpha".into(), "beta".into()],
        merged_at: "2026-06-09T00:00:00Z".into(),
        baseline_provisional: false,
        section_host_counts: Default::default(),
    });
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "git".into(),
                arch: "x86_64".into(),
                include: true,
                ..Default::default()
            },
        ],
        leaf_packages: Some(vec!["git.x86_64".into()]),
        baseline_package_names: Some(vec!["systemd".into()]),
        ..Default::default()
    });
    snap.services = Some(ServiceSection {
        state_changes: vec![ServiceStateChange {
            unit: "perl-related.service".into(),
            current_state: ServiceUnitState::Enabled,
            default_state: Some(PresetDefault::Disable),
            include: true,
            locked: false,
            owning_package: Some("perl-libs".into()),
            fleet: None,
            attention_reason: None,
        }],
        ..Default::default()
    });

    let plan = render_service_intent(&snap);

    // Fleet: no omissions, no advisories, service must be emitted
    assert!(plan.omissions.is_empty(), "fleet must not omit services");
    assert!(plan.advisories.is_empty(), "fleet must not emit package-derived advisories");
    assert!(
        plan.lines.iter().any(|l| l.contains("perl-related.service")),
        "fleet must emit perl-related.service"
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --package inspectah-pipeline --test service_intent_test test_fleet_snapshot_skips_service_omission_and_advisories -- --exact`
Expected: FAIL.

- [ ] **Step 3: Implement the fleet guard**

In `inspectah-pipeline/src/render/service_intent.rs`, in `render_service_intent()` (line 294), add `let is_fleet = snap.fleet_meta.is_some();` before the classify loop. Then replace the existing `match classify_service_presence(...)` with:

```rust
        let presence = if is_fleet {
            PresenceDecision::Emit { advisory_reasons: None }
        } else {
            classify_service_presence(sc, rpm, &target_packages, baseline_unavailable)
        };

        match presence {
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --package inspectah-pipeline --test service_intent_test test_fleet_snapshot_skips_service_omission_and_advisories -- --exact`
Expected: PASS.

- [ ] **Step 5: Run full suite and clippy**

Run: `cargo test --workspace`
Run: `cargo clippy --workspace -- -D warnings`
Expected: All pass, clean.

- [ ] **Step 6: Commit**

```bash
git add inspectah-pipeline/src/render/service_intent.rs inspectah-pipeline/tests/service_intent_test.rs
git commit -m "fix(service_intent): gate package-based omission for fleet

Fleet packages_added is leaf-only, so classify_service_presence would
incorrectly omit services owned by auto packages and emit stale
package-derived advisories. Skip classification entirely for fleet
and force all services to Emit.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 5: Surface partial authority metadata on operator-facing surfaces

**Files:**
- Modify: `inspectah-pipeline/src/render/containerfile.rs` — `packages_section_lines()` (line 263)
- Modify: `inspectah-pipeline/src/render/report.rs` — `render_report()` context variables (around line 756)
- Modify: `inspectah-pipeline/templates/report/fleet-summary.html` — add metadata line
- Modify: `inspectah-web/src/fleet_handlers.rs` — `FleetSummary` struct + `build_fleet_view_response()`

The spec requires surfacing "Leaf classification: N/M hosts" on three surfaces when `leaf_authority_hosts < leaf_total_hosts`.

#### 5a: Containerfile

The partial authority comment goes in `packages_section_lines()` (line 263 of `containerfile.rs`), NOT in `render_containerfile_inner()`. The `packages_section_lines` function owns the FROM + repos + packages block and returns `Vec<String>`.

- [ ] **Step 1: Add coverage comment in packages_section_lines**

In `inspectah-pipeline/src/render/containerfile.rs`, in `packages_section_lines()` (line 263), after the `FROM` line is pushed and the `rpm` is extracted (around line 276-278), add before the repo files section:

```rust
    // Partial leaf authority indicator for fleet snapshots
    if let (Some(auth), Some(total)) = (rpm.leaf_authority_hosts, rpm.leaf_total_hosts) {
        if auth < total {
            lines.push(format!(
                "# Leaf classification: {}/{} hosts (partial authority)",
                auth, total
            ));
        }
    }
```

- [ ] **Step 2: Write tests for Containerfile coverage comment**

In the test module of `containerfile.rs`:

```rust
#[test]
fn test_fleet_containerfile_partial_authority_comment() {
    let mut snap = InspectionSnapshot::default();
    snap.fleet_meta = Some(inspectah_core::types::fleet::FleetSnapshotMeta {
        label: "test".into(),
        host_count: 3,
        hostnames: vec!["a".into(), "b".into(), "c".into()],
        merged_at: "2026-06-09T00:00:00Z".into(),
        baseline_provisional: false,
        section_host_counts: Default::default(),
    });
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "git".into(),
            arch: "x86_64".into(),
            source_repo: "baseos".into(),
            include: true,
            ..Default::default()
        }],
        leaf_packages: Some(vec!["git.x86_64".into()]),
        leaf_authority_hosts: Some(2),
        leaf_total_hosts: Some(3),
        ..Default::default()
    });
    snap.services = Some(inspectah_core::types::services::ServiceSection::default());

    let output = render_containerfile(&snap, None);

    assert!(
        output.contains("Leaf classification: 2/3 hosts"),
        "partial authority comment must appear in Containerfile"
    );
}

#[test]
fn test_fleet_containerfile_full_authority_no_comment() {
    let mut snap = InspectionSnapshot::default();
    snap.fleet_meta = Some(inspectah_core::types::fleet::FleetSnapshotMeta {
        label: "test".into(),
        host_count: 2,
        hostnames: vec!["a".into(), "b".into()],
        merged_at: "2026-06-09T00:00:00Z".into(),
        baseline_provisional: false,
        section_host_counts: Default::default(),
    });
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "git".into(),
            arch: "x86_64".into(),
            source_repo: "baseos".into(),
            include: true,
            ..Default::default()
        }],
        leaf_packages: Some(vec!["git.x86_64".into()]),
        leaf_authority_hosts: Some(2),
        leaf_total_hosts: Some(2),
        ..Default::default()
    });
    snap.services = Some(inspectah_core::types::services::ServiceSection::default());

    let output = render_containerfile(&snap, None);

    assert!(
        !output.contains("Leaf classification"),
        "full authority must not show coverage comment"
    );
}
```

- [ ] **Step 3: Run Containerfile tests and clippy**

Run: `cargo test --package inspectah-pipeline test_fleet_containerfile_partial_authority_comment -- --exact`
Run: `cargo test --package inspectah-pipeline test_fleet_containerfile_full_authority_no_comment -- --exact`
Run: `cargo clippy --workspace -- -D warnings`
Expected: All pass, clean.

#### 5b: Fleet report summary

The report template uses scalar variables passed from `render_report()`, NOT an `rpm` object. The current pattern extracts fleet values from `snap.fleet_meta` as scalars (e.g., `fleet_label`, `fleet_host_count`). The leaf authority values live on `snap.rpm`, so new scalar variables must be added.

- [ ] **Step 4: Add context variables in render_report()**

In `inspectah-pipeline/src/render/report.rs`, find the fleet aggregate data section (around line 756, `// ── Fleet aggregate data`). Add two new scalar variables after the existing fleet variables:

```rust
    let fleet_leaf_authority_hosts = snap
        .rpm
        .as_ref()
        .and_then(|r| r.leaf_authority_hosts)
        .unwrap_or(0);
    let fleet_leaf_total_hosts = snap
        .rpm
        .as_ref()
        .and_then(|r| r.leaf_total_hosts)
        .unwrap_or(0);
    let fleet_leaf_partial = fleet_leaf_total_hosts > 0
        && fleet_leaf_authority_hosts < fleet_leaf_total_hosts;
```

Then add these to the template context (find where `is_fleet`, `fleet_label`, etc. are added to the context — around line 839):

```rust
        fleet_leaf_authority_hosts,
        fleet_leaf_total_hosts,
        fleet_leaf_partial,
```

- [ ] **Step 5: Add metadata line in fleet-summary.html**

In `inspectah-pipeline/templates/report/fleet-summary.html`, add after the `Baseline Status` `<dd>` and before the variant conflicts conditional:

```html
    {% if fleet_leaf_partial %}
    <dt>Leaf Classification</dt>
    <dd>{{ fleet_leaf_authority_hosts }}/{{ fleet_leaf_total_hosts }} hosts</dd>
    {% endif %}
```

- [ ] **Step 6: Write report regression test**

In `inspectah-pipeline`, find or create the appropriate test file for report rendering. Add:

```rust
#[test]
fn test_fleet_report_partial_authority_metadata() {
    let mut snap = InspectionSnapshot::default();
    snap.fleet_meta = Some(inspectah_core::types::fleet::FleetSnapshotMeta {
        label: "test".into(),
        host_count: 3,
        hostnames: vec!["a".into(), "b".into(), "c".into()],
        merged_at: "2026-06-09T00:00:00Z".into(),
        baseline_provisional: false,
        section_host_counts: Default::default(),
    });
    snap.rpm = Some(RpmSection {
        leaf_authority_hosts: Some(2),
        leaf_total_hosts: Some(3),
        ..Default::default()
    });

    let output = render_report(&snap, &RenderContext { target: None });

    assert!(
        output.contains("2/3 hosts"),
        "partial leaf authority must appear in fleet report"
    );
}
```

The implementer should verify the exact test module location and imports needed. If `render_report` tests are in a separate integration test file, place the test there.

#### 5c: Fleet refine view API

The fleet view API uses `FleetViewResponse` containing a `FleetSummary` struct (line 38 of `fleet_handlers.rs`). The coverage metadata must be added to `FleetSummary` so the frontend can display the `Leaf classification: N/M hosts` signal.

- [ ] **Step 7: Add fields to FleetSummary**

In `inspectah-web/src/fleet_handlers.rs`, add to the `FleetSummary` struct (line 38). The existing struct has `host_count`, `actionable_variant_items`, and `informational_variant_count`:

```rust
pub struct FleetSummary {
    pub host_count: usize,
    pub actionable_variant_items: Vec<ActionableVariantItem>,
    pub informational_variant_count: usize,
    pub leaf_authority_hosts: Option<u32>,
    pub leaf_total_hosts: Option<u32>,
}
```

- [ ] **Step 8: Wire fields in build_fleet_summary()**

The real construction seam is `build_fleet_summary()` (line 380), NOT `build_fleet_view_response()`. `build_fleet_summary` takes `snap: &InspectionSnapshot` and constructs the `FleetSummary`. Find where `FleetSummary { ... }` is returned and add the leaf authority fields:

```rust
    let (leaf_authority_hosts, leaf_total_hosts) = snap
        .rpm
        .as_ref()
        .map(|r| (r.leaf_authority_hosts, r.leaf_total_hosts))
        .unwrap_or((None, None));

    FleetSummary {
        host_count: ...,
        actionable_variant_items,
        informational_variant_count: ...,
        leaf_authority_hosts,
        leaf_total_hosts,
    }
```

- [ ] **Step 9: Write fleet API regression test**

In `inspectah-web/tests/fleet_api_test.rs`, follow the existing fleet API test patterns. Add a test that constructs a fleet snapshot with partial authority and asserts the view response JSON contains the metadata:

```rust
#[test]
fn test_fleet_view_response_includes_leaf_authority_metadata() {
    // Follow existing fleet_api_test.rs patterns for fixture construction.
    // Set snap.rpm leaf_authority_hosts = Some(2), leaf_total_hosts = Some(3).
    // Build the fleet view response via the existing test harness.
    // Parse the response JSON.
    // Assert: response.summary.leaf_authority_hosts == 2
    // Assert: response.summary.leaf_total_hosts == 3
}
```

The implementer should read the existing tests in `fleet_api_test.rs` to match the fixture construction and assertion patterns used there.

- [ ] **Step 10: Run all tests and clippy**

Run: `cargo test --workspace`
Run: `cargo clippy --workspace -- -D warnings`
Expected: All pass, clean.

- [ ] **Step 11: Commit**

```bash
git add inspectah-pipeline/src/render/containerfile.rs inspectah-pipeline/src/render/report.rs inspectah-pipeline/templates/report/fleet-summary.html inspectah-web/src/fleet_handlers.rs
git commit -m "feat(fleet): surface partial leaf authority on operator-facing surfaces

Containerfile: coverage comment in packages_section_lines() when
authority is partial. Fleet report: metadata line in fleet-summary.html
with scalar context variables from render_report(). Fleet API:
leaf_authority_hosts and leaf_total_hosts on FleetSummary DTO.
Only shown when leaf_authority_hosts < leaf_total_hosts.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 6: Containerfile fleet snapshot test

**Files:**
- Modify: `inspectah-pipeline/src/render/containerfile.rs` (test module)

- [ ] **Step 1: Write test — fleet Containerfile contains only leaf packages**

```rust
#[test]
fn test_fleet_containerfile_leaf_only_packages() {
    let mut snap = InspectionSnapshot::default();
    snap.fleet_meta = Some(inspectah_core::types::fleet::FleetSnapshotMeta {
        label: "test".into(),
        host_count: 2,
        hostnames: vec!["alpha".into(), "beta".into()],
        merged_at: "2026-06-09T00:00:00Z".into(),
        baseline_provisional: false,
        section_host_counts: Default::default(),
    });
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "git".into(),
            arch: "x86_64".into(),
            source_repo: "baseos".into(),
            include: true,
            ..Default::default()
        }],
        leaf_packages: Some(vec!["git.x86_64".into()]),
        leaf_authority_hosts: Some(2),
        leaf_total_hosts: Some(2),
        ..Default::default()
    });
    snap.services = Some(inspectah_core::types::services::ServiceSection::default());

    let output = render_containerfile(&snap, None);

    assert!(output.contains("git"), "leaf package must appear");
    assert!(!output.contains("perl-libs"), "auto package must not appear");
}
```

- [ ] **Step 2: Run test**

Run: `cargo test --package inspectah-pipeline test_fleet_containerfile_leaf_only_packages -- --exact`
Expected: PASS.

- [ ] **Step 3: Run full suite and clippy**

Run: `cargo test --workspace`
Run: `cargo clippy --workspace -- -D warnings`
Expected: All pass, clean.

- [ ] **Step 4: Commit**

```bash
git add inspectah-pipeline/src/render/containerfile.rs
git commit -m "test(containerfile): verify fleet install line is leaf-only

Confirms renderer's leaf filter is a no-op for fleet snapshots
where packages_added is pre-filtered by the merge layer.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 7: Refine session regression

**Files:**
- Modify: `inspectah-refine/src/session.rs` (test module)

- [ ] **Step 1: Write test — pre-filtered fleet packages drive refine view**

The test must use the current API: `RefineSession::new(snapshot)` and `session.view().packages`.

```rust
#[test]
fn test_fleet_pre_filtered_packages_drive_refine_view() {
    let mut snap = test_snapshot(); // use existing test helper
    snap.fleet_meta = Some(inspectah_core::types::fleet::FleetSnapshotMeta {
        label: "test".into(),
        host_count: 2,
        hostnames: vec!["alpha".into(), "beta".into()],
        merged_at: "2026-06-09T00:00:00Z".into(),
        baseline_provisional: false,
        section_host_counts: Default::default(),
    });
    if let Some(ref mut rpm) = snap.rpm {
        // Simulate pre-filtered leaf-only packages_added
        rpm.packages_added.retain(|p| p.name == "git" || p.name == "vim");
        for pkg in &mut rpm.packages_added {
            pkg.fleet = Some(Default::default());
        }
        rpm.leaf_packages = Some(
            rpm.packages_added
                .iter()
                .map(|p| format!("{}.{}", p.name, p.arch))
                .collect(),
        );
        rpm.leaf_authority_hosts = Some(2);
        rpm.leaf_total_hosts = Some(2);
    }

    let session = RefineSession::new(snap);
    let view = session.view();

    // View should only contain the pre-filtered leaf packages
    let view_names: Vec<&str> = view.packages.iter().map(|p| p.entry.name.as_str()).collect();
    assert!(!view_names.is_empty(), "view should have packages");
    // All view packages should be in leaf_packages
    // (exact assertion depends on test_snapshot() fixture — adapt to match)
}
```

The implementer should adapt this test to the actual `test_snapshot()` fixture contents and verify the view contains only the expected leaf packages.

- [ ] **Step 2: Write test — partial authority metadata accessible from snapshot**

```rust
#[test]
fn test_fleet_leaf_authority_metadata_on_snapshot() {
    let mut snap = test_snapshot();
    snap.fleet_meta = Some(inspectah_core::types::fleet::FleetSnapshotMeta {
        label: "test".into(),
        host_count: 3,
        hostnames: vec!["a".into(), "b".into(), "c".into()],
        merged_at: "2026-06-09T00:00:00Z".into(),
        baseline_provisional: false,
        section_host_counts: Default::default(),
    });
    if let Some(ref mut rpm) = snap.rpm {
        rpm.leaf_authority_hosts = Some(2);
        rpm.leaf_total_hosts = Some(3);
    }

    let session = RefineSession::new(snap);
    let rpm = session.snapshot().rpm.as_ref().unwrap();
    assert_eq!(rpm.leaf_authority_hosts, Some(2));
    assert_eq!(rpm.leaf_total_hosts, Some(3));
}
```

- [ ] **Step 3: Run tests and clippy**

Run: `cargo test --workspace`
Run: `cargo clippy --workspace -- -D warnings`
Expected: All pass, clean.

- [ ] **Step 4: Commit**

```bash
git add inspectah-refine/src/session.rs
git commit -m "test(refine): fleet pre-filtered packages and authority metadata

Proves refine view uses merge-layer's leaf-filtered packages_added
and that partial authority metadata is accessible on the snapshot.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 8: Final verification

- [ ] **Step 1: Full test suite**

Run: `cargo test --workspace`
Expected: All tests pass.

- [ ] **Step 2: Clippy clean**

Run: `cargo clippy --workspace -- -D warnings`
Expected: Zero warnings.

- [ ] **Step 3: Verify degraded-state JSON contract**

Add a test (can go in `fleet_merge_test.rs` or the rpm types tests):

```rust
#[test]
fn test_fleet_degraded_state_json_contract() {
    let rpm = RpmSection {
        leaf_packages: None,
        auto_packages: None,
        leaf_dep_tree: serde_json::Value::Object(serde_json::Map::new()),
        leaf_authority_hosts: Some(0),
        leaf_total_hosts: Some(3),
        ..Default::default()
    };
    let json = serde_json::to_value(&rpm).unwrap();
    assert!(json["leaf_packages"].is_null());
    assert!(json["auto_packages"].is_null());
    assert_eq!(json["leaf_dep_tree"], serde_json::json!({}));
    assert_eq!(json["leaf_authority_hosts"], 0);
    assert_eq!(json["leaf_total_hosts"], 3);
}
```

Run: `cargo test --workspace`

- [ ] **Step 4: Review git log**

Run: `git log --oneline -10`
Verify commits are clean, focused, and correctly attributed.
