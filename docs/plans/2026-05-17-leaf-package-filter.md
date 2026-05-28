# Leaf Package Filter Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reduce the package list from ~477 to ~20-50 by filtering to user-intent (leaf) packages. The Containerfile `dnf install` line and the web UI both show only leaf packages; transitive dependencies are accessible via expand-on-demand in the UI.

**Architecture:** Two-part fix. (1) The RPM inspector at scan-time runs `dnf repoquery --userinstalled` and builds a dependency graph to split `packages_added` into `leaf_packages` (user-intent) and `auto_packages` (dependencies), plus a `leaf_dep_tree` mapping each leaf to its transitive deps. (2) The refine session at view-time filters the package list to leaf-only when `leaf_packages` is available, and feeds only leaf packages to the Containerfile renderer. The Rust schema already has the fields (`leaf_packages`, `auto_packages`, `leaf_dep_tree`) — they're just unpopulated.

**Tech Stack:** Rust (inspectah-collect, inspectah-refine, inspectah-pipeline), `dyn Executor` trait with `MockExecutor` for testing, Vitest for frontend.

**Testing constraint:** The scan-time code calls `dnf` and `rpm` which are only available on RHEL/CentOS hosts. All scan-time tasks use `MockExecutor` for unit tests. Real-host integration testing is a separate verification step.

**Reference:** Go implementation at `cmd/inspectah/internal/inspector/rpm.go` (lines 759-960) and `cmd/inspectah/internal/renderer/triage.go` (lines 329-357). Tang's criteria doc at `/Users/mrussell/PKA/marks-inbox/research/package-classification-criteria.md`.

---

### Task 1: Add `query_user_installed()` to RPM Inspector

**Files:**
- Modify: `inspectah-collect/src/inspectors/rpm/mod.rs`

Queries `dnf repoquery --userinstalled` to get the set of user-explicitly-installed package names. Returns `Option<HashSet<String>>` — `None` if dnf is unavailable (non-zero exit).

- [ ] **Step 1: Write failing test**

Add a test in the `#[cfg(test)]` module of `mod.rs`:

```rust
#[test]
fn query_user_installed_parses_dnf_output() {
    let exec = MockExecutor::new().with_command(
        "dnf",
        &["repoquery", "--userinstalled", "--queryformat", "%{name}\n"],
        ExecResult { exit_code: 0, stdout: "vim\nhtop\nnginx\n".into(), stderr: String::new() },
    );
    let result = query_user_installed(&exec);
    assert!(result.is_some());
    let names = result.unwrap();
    assert_eq!(names.len(), 3);
    assert!(names.contains("vim"));
    assert!(names.contains("htop"));
    assert!(names.contains("nginx"));
}

#[test]
fn query_user_installed_returns_none_on_failure() {
    let exec = MockExecutor::new().with_command(
        "dnf",
        &["repoquery", "--userinstalled", "--queryformat", "%{name}\n"],
        ExecResult { exit_code: 1, stdout: String::new(), stderr: "dnf not found".into() },
    );
    let result = query_user_installed(&exec);
    assert!(result.is_none());
}

#[test]
fn query_user_installed_skips_blank_lines() {
    let exec = MockExecutor::new().with_command(
        "dnf",
        &["repoquery", "--userinstalled", "--queryformat", "%{name}\n"],
        ExecResult { exit_code: 0, stdout: "vim\n\n  \nhtop\n".into(), stderr: String::new() },
    );
    let result = query_user_installed(&exec);
    let names = result.unwrap();
    assert_eq!(names.len(), 2);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-collect query_user_installed`
Expected: FAIL — function not defined

- [ ] **Step 3: Implement `query_user_installed`**

```rust
use std::collections::HashSet;

fn query_user_installed(exec: &dyn Executor) -> Option<HashSet<String>> {
    let result = exec.run("dnf", &["repoquery", "--userinstalled", "--queryformat", "%{name}\n"]);
    if result.exit_code != 0 {
        return None;
    }
    let names: HashSet<String> = result.stdout
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    Some(names)
}
```

Check the existing `Executor` trait's `run()` signature — it may use `&[&str]` or `Vec<String>` for args. Match the existing pattern used by other functions in `mod.rs` (e.g., `query_packages`).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-collect query_user_installed`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-collect/src/inspectors/rpm/mod.rs && git commit -m "feat(collect): add query_user_installed for leaf package detection

Queries dnf repoquery --userinstalled to identify user-intent packages.
Returns None if dnf is unavailable. Blank lines and whitespace handled.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 2: Add Dependency Graph Builder

**Files:**
- Modify: `inspectah-collect/src/inspectors/rpm/mod.rs`

Builds a dependency graph from `dnf repoquery --requires --resolve --recursive --installed` (with `rpm -qR` fallback). Returns which added packages depend on which other added packages.

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn classify_deps_dnf_builds_graph() {
    // vim depends on glibc (which is also in added_names)
    let mut exec = MockExecutor::new();
    // For each package, dnf repoquery --requires returns its deps
    exec = exec.with_command(
        "dnf",
        &["repoquery", "--requires", "--resolve", "--recursive", "--installed", "--queryformat", "%{name}\n", "vim"],
        ExecResult { exit_code: 0, stdout: "glibc\nncurses\n".into(), stderr: String::new() },
    );
    exec = exec.with_command(
        "dnf",
        &["repoquery", "--requires", "--resolve", "--recursive", "--installed", "--queryformat", "%{name}\n", "glibc"],
        ExecResult { exit_code: 0, stdout: "".into(), stderr: String::new() },
    );

    let added_names: HashSet<String> = ["vim", "glibc"].iter().map(|s| s.to_string()).collect();
    let (deps, ok) = classify_deps_dnf(&exec, &added_names);
    assert!(ok);
    // vim depends on glibc (glibc is in added_names)
    assert!(deps.get("vim").unwrap().contains("glibc"));
    // ncurses is NOT in added_names, so not tracked
    assert!(!deps.get("vim").unwrap().contains("ncurses"));
}

#[test]
fn classify_deps_dnf_returns_false_on_failure() {
    let exec = MockExecutor::new().with_command(
        "dnf",
        &["repoquery", "--requires", "--resolve", "--recursive", "--installed", "--queryformat", "%{name}\n", "vim"],
        ExecResult { exit_code: 1, stdout: String::new(), stderr: "error".into() },
    );
    let added_names: HashSet<String> = ["vim"].iter().map(|s| s.to_string()).collect();
    let (_, ok) = classify_deps_dnf(&exec, &added_names);
    assert!(!ok);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-collect classify_deps`
Expected: FAIL

- [ ] **Step 3: Implement `classify_deps_dnf`**

```rust
use std::collections::HashMap;

fn classify_deps_dnf(
    exec: &dyn Executor,
    added_names: &HashSet<String>,
) -> (HashMap<String, HashSet<String>>, bool) {
    if added_names.is_empty() {
        return (HashMap::new(), true);
    }

    let mut sorted_names: Vec<&str> = added_names.iter().map(|s| s.as_str()).collect();
    sorted_names.sort();

    // Probe with first package to check if dnf repoquery works
    let first = &sorted_names[0];
    let probe = exec.run("dnf", &[
        "repoquery", "--requires", "--resolve", "--recursive",
        "--installed", "--queryformat", "%{name}\n", first,
    ]);
    if probe.exit_code != 0 {
        return (HashMap::new(), false);
    }

    let mut depends_on: HashMap<String, HashSet<String>> = HashMap::new();
    for name in added_names {
        depends_on.insert(name.clone(), HashSet::new());
    }

    parse_dnf_deps(&probe.stdout, first, added_names, &mut depends_on);

    for pkg_name in &sorted_names[1..] {
        let result = exec.run("dnf", &[
            "repoquery", "--requires", "--resolve", "--recursive",
            "--installed", "--queryformat", "%{name}\n", pkg_name,
        ]);
        if result.exit_code != 0 {
            continue;
        }
        parse_dnf_deps(&result.stdout, pkg_name, added_names, &mut depends_on);
    }

    (depends_on, true)
}

fn parse_dnf_deps(
    stdout: &str,
    pkg_name: &str,
    added_names: &HashSet<String>,
    depends_on: &mut HashMap<String, HashSet<String>>,
) {
    for line in stdout.lines() {
        let dep = line.trim();
        if !dep.is_empty() && added_names.contains(dep) && dep != pkg_name {
            depends_on.entry(pkg_name.to_string())
                .or_default()
                .insert(dep.to_string());
        }
    }
}
```

Note: Check the `Executor::run()` signature for how args are passed. The Go code runs one `dnf repoquery` per package. This is O(n) calls but each is fast. Match the Go behavior exactly for now; optimization is future work.

- [ ] **Step 4: Run tests to verify they pass**

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-collect/src/inspectors/rpm/mod.rs && git commit -m "feat(collect): add dependency graph builder for leaf classification

Queries dnf repoquery --requires --resolve --recursive for each added
package. Builds inter-package dependency map scoped to added packages.
Returns false if dnf repoquery is unavailable.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 3: Add `classify_leaf_auto()` and Wire into RPM Inspector

**Files:**
- Modify: `inspectah-collect/src/inspectors/rpm/mod.rs`

Orchestrates `query_user_installed()` + `classify_deps_dnf()` to split `packages_added` into leaf and auto sets, and builds the per-leaf dependency tree. Wires into the main RPM inspector `inspect()` method to populate `leaf_packages`, `auto_packages`, and `leaf_dep_tree`.

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn classify_leaf_auto_splits_by_user_installed() {
    let mut exec = MockExecutor::new();
    // dnf --userinstalled returns vim (but not glibc)
    exec = exec.with_command(
        "dnf",
        &["repoquery", "--userinstalled", "--queryformat", "%{name}\n"],
        ExecResult { exit_code: 0, stdout: "vim\n".into(), stderr: String::new() },
    );
    // vim depends on glibc
    exec = exec.with_command(
        "dnf",
        &["repoquery", "--requires", "--resolve", "--recursive", "--installed", "--queryformat", "%{name}\n", "glibc"],
        ExecResult { exit_code: 0, stdout: "".into(), stderr: String::new() },
    );
    exec = exec.with_command(
        "dnf",
        &["repoquery", "--requires", "--resolve", "--recursive", "--installed", "--queryformat", "%{name}\n", "vim"],
        ExecResult { exit_code: 0, stdout: "glibc\n".into(), stderr: String::new() },
    );

    let added = vec![
        make_test_entry("vim"),
        make_test_entry("glibc"),
    ];

    let (leaf, auto, dep_tree) = classify_leaf_auto(&exec, &added);

    assert_eq!(leaf, vec!["vim".to_string()]);
    assert_eq!(auto, vec!["glibc".to_string()]);
    // dep_tree: vim -> [glibc]
    let vim_deps = dep_tree.get("vim").unwrap().as_array().unwrap();
    assert_eq!(vim_deps.len(), 1);
    assert_eq!(vim_deps[0].as_str().unwrap(), "glibc");
}

#[test]
fn classify_leaf_auto_falls_back_to_graph_when_userinstalled_empty() {
    let mut exec = MockExecutor::new();
    // dnf --userinstalled returns empty (intersection with added is empty)
    exec = exec.with_command(
        "dnf",
        &["repoquery", "--userinstalled", "--queryformat", "%{name}\n"],
        ExecResult { exit_code: 0, stdout: "unrelated-pkg\n".into(), stderr: String::new() },
    );
    // glibc depends on nothing in added_names, vim depends on glibc
    exec = exec.with_command(
        "dnf",
        &["repoquery", "--requires", "--resolve", "--recursive", "--installed", "--queryformat", "%{name}\n", "glibc"],
        ExecResult { exit_code: 0, stdout: "".into(), stderr: String::new() },
    );
    exec = exec.with_command(
        "dnf",
        &["repoquery", "--requires", "--resolve", "--recursive", "--installed", "--queryformat", "%{name}\n", "vim"],
        ExecResult { exit_code: 0, stdout: "glibc\n".into(), stderr: String::new() },
    );

    let added = vec![
        make_test_entry("vim"),
        make_test_entry("glibc"),
    ];

    let (leaf, auto, _) = classify_leaf_auto(&exec, &added);

    // Graph-based: glibc is depended on by vim, so glibc is auto. vim is leaf.
    assert_eq!(leaf, vec!["vim".to_string()]);
    assert_eq!(auto, vec!["glibc".to_string()]);
}
```

Add a `make_test_entry` helper if one doesn't exist:
```rust
fn make_test_entry(name: &str) -> PackageEntry {
    PackageEntry {
        name: name.to_string(),
        ..PackageEntry::default()
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

- [ ] **Step 3: Implement `classify_leaf_auto`**

Port the Go logic from `classifyLeafAuto()` (rpm.go:761-862):

```rust
fn classify_leaf_auto(
    exec: &dyn Executor,
    packages_added: &[PackageEntry],
) -> (Vec<String>, Vec<String>, serde_json::Value) {
    let added_names: HashSet<String> = packages_added.iter()
        .map(|p| p.name.clone())
        .collect();

    let user_installed = query_user_installed(exec);
    let (depends_on, _transitive) = classify_deps_dnf(exec, &added_names);

    let (mut leaf, mut auto): (Vec<String>, Vec<String>) = if let Some(ref ui) = user_installed {
        let leaf_set: HashSet<&String> = ui.intersection(&added_names).collect();
        if leaf_set.is_empty() && !added_names.is_empty() {
            // Fallback to graph-based
            graph_based_split(&added_names, &depends_on)
        } else {
            let mut l = Vec::new();
            let mut a = Vec::new();
            for name in &added_names {
                if leaf_set.contains(name) {
                    l.push(name.clone());
                } else {
                    a.push(name.clone());
                }
            }
            (l, a)
        }
    } else {
        graph_based_split(&added_names, &depends_on)
    };

    leaf.sort();
    auto.sort();

    // Build per-leaf dep tree
    let auto_set: HashSet<&str> = auto.iter().map(|s| s.as_str()).collect();
    let mut dep_tree = serde_json::Map::new();
    for lf in &leaf {
        let mut filtered: Vec<String> = depends_on.get(lf)
            .map(|deps| deps.iter()
                .filter(|d| auto_set.contains(d.as_str()))
                .cloned()
                .collect())
            .unwrap_or_default();
        filtered.sort();
        dep_tree.insert(lf.clone(), serde_json::json!(filtered));
    }

    (leaf, auto, serde_json::Value::Object(dep_tree))
}

fn graph_based_split(
    added_names: &HashSet<String>,
    depends_on: &HashMap<String, HashSet<String>>,
) -> (Vec<String>, Vec<String>) {
    let mut depended_on: HashSet<String> = HashSet::new();
    for deps in depends_on.values() {
        for dep in deps {
            depended_on.insert(dep.clone());
        }
    }
    let mut leaf = Vec::new();
    let mut auto = Vec::new();
    for name in added_names {
        if depended_on.contains(name) {
            auto.push(name.clone());
        } else {
            leaf.push(name.clone());
        }
    }
    (leaf, auto)
}
```

- [ ] **Step 4: Wire into RPM inspector `inspect()` method**

Find where `RpmSection` is constructed in the inspector's `inspect()` method. After the existing `packages_added` population, add:

```rust
let (leaf_packages, auto_packages, leaf_dep_tree) = classify_leaf_auto(exec, &section.packages_added);
section.leaf_packages = Some(leaf_packages);
section.auto_packages = Some(auto_packages);
section.leaf_dep_tree = leaf_dep_tree;
```

Check the actual field assignment pattern in `mod.rs` — the section may be built as a struct literal or via mutation. Match the existing pattern.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-collect classify_leaf`
Expected: PASS

- [ ] **Step 6: Run full inspectah-collect tests**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-collect`
Expected: PASS (no regressions)

- [ ] **Step 7: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-collect/src/inspectors/rpm/mod.rs && git commit -m "feat(collect): classify leaf vs auto packages in RPM inspector

Splits packages_added into leaf (user-intent via dnf --userinstalled)
and auto (transitive dependencies). Falls back to graph-based analysis
when dnf userinstalled returns empty intersection. Builds per-leaf
dependency tree for expand-on-demand UI.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 4: Add Leaf Filter to View Projection

**Files:**
- Modify: `inspectah-refine/src/session.rs`

When `leaf_packages` is available in the snapshot and the snapshot is not a fleet merge, filter the view's package list to only include leaf packages. This is the primary fix that reduces 477 → ~20-50 visible packages.

- [ ] **Step 1: Write failing test**

Find the test module in `session.rs` (or create one). Add a test that constructs a `RefineSession` with mock snapshot data including `leaf_packages`, and verifies the view only contains leaf packages.

```rust
#[test]
fn view_filters_to_leaf_packages_when_available() {
    // Build a snapshot with 3 packages but only 1 is a leaf
    let mut snap = test_snapshot();
    snap.rpm.packages_added = vec![
        PackageEntry { name: "vim".into(), ..Default::default() },
        PackageEntry { name: "glibc".into(), ..Default::default() },
        PackageEntry { name: "ncurses".into(), ..Default::default() },
    ];
    snap.rpm.leaf_packages = Some(vec!["vim".into()]);
    snap.rpm.auto_packages = Some(vec!["glibc".into(), "ncurses".into()]);

    let session = RefineSession::new(snap);
    let view = session.view();

    // View should only contain the leaf package
    assert_eq!(view.packages.len(), 1);
    assert_eq!(view.packages[0].entry.name, "vim");
}

#[test]
fn view_shows_all_packages_when_leaf_data_unavailable() {
    let mut snap = test_snapshot();
    snap.rpm.packages_added = vec![
        PackageEntry { name: "vim".into(), ..Default::default() },
        PackageEntry { name: "glibc".into(), ..Default::default() },
    ];
    snap.rpm.leaf_packages = None; // No leaf data

    let session = RefineSession::new(snap);
    let view = session.view();

    // All packages visible (degraded mode)
    assert_eq!(view.packages.len(), 2);
}
```

Adapt `test_snapshot()` to whatever existing test helper exists for constructing snapshots in the refine crate.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-refine view_filters`
Expected: FAIL

- [ ] **Step 3: Implement leaf filter in view projection**

In the `recompute()` method of `RefineSession` (around line 578-630 of session.rs), after computing the view packages but before building `RefinedView`, add leaf filtering:

```rust
// Filter to leaf packages when available (non-fleet snapshots only)
let filtered_packages = if let Some(ref leaf_names) = projected.rpm.leaf_packages {
    let leaf_set: HashSet<&str> = leaf_names.iter().map(|s| s.as_str()).collect();
    view_packages.into_iter()
        .filter(|pkg| leaf_set.contains(pkg.entry.name.as_str()))
        .collect()
} else {
    view_packages
};
```

Then use `filtered_packages` instead of `view_packages` when building the `RefinedView`.

- [ ] **Step 4: Run tests to verify they pass**

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-refine/src/session.rs && git commit -m "feat(refine): filter view to leaf packages when available

When leaf_packages is present in the snapshot, the view only shows
user-intent packages. Transitive dependencies are excluded from the
package list. Degrades gracefully to showing all packages when leaf
data is unavailable.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 5: Filter Containerfile Rendering to Leaf Packages

**Files:**
- Modify: `inspectah-refine/src/session.rs` (or `inspectah-pipeline/src/render/containerfile.rs` — check which owns the `dnf install` line construction)

The Containerfile's `dnf install` line should only include leaf packages. When leaf data is available, filter `packages_added` in the projected snapshot before passing to `render_containerfile()`.

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn containerfile_preview_only_includes_leaf_packages() {
    let mut snap = test_snapshot();
    snap.rpm.packages_added = vec![
        PackageEntry { name: "vim".into(), include: true, ..Default::default() },
        PackageEntry { name: "glibc".into(), include: true, ..Default::default() },
    ];
    snap.rpm.leaf_packages = Some(vec!["vim".into()]);

    let session = RefineSession::new(snap);
    let view = session.view();

    // Containerfile should contain vim but not glibc
    assert!(view.containerfile_preview.contains("vim"));
    assert!(!view.containerfile_preview.contains("glibc"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

- [ ] **Step 3: Implement leaf filter before containerfile rendering**

In `recompute()`, before the `render_containerfile(&projected, ...)` call, apply the leaf filter to the projected snapshot's `packages_added`:

```rust
// Filter projected snapshot to leaf packages for containerfile rendering
let mut containerfile_snap = projected.clone();
if let Some(ref leaf_names) = containerfile_snap.rpm.leaf_packages {
    let leaf_set: HashSet<&str> = leaf_names.iter().map(|s| s.as_str()).collect();
    containerfile_snap.rpm.packages_added.retain(|pkg| leaf_set.contains(pkg.name.as_str()));
}
let containerfile_preview = render_containerfile(&containerfile_snap, Some(&materialized_roots));
```

Check if cloning the snapshot is expensive. If so, consider filtering `packages_added` in-place on a mutable reference, or pass the leaf set to `render_containerfile()` as a filter parameter. The clone approach is simplest and correct; optimize later if profiling shows it matters.

- [ ] **Step 4: Run tests to verify they pass**

- [ ] **Step 5: Run full refine tests**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-refine`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add -u && git commit -m "feat(refine): filter containerfile dnf install to leaf packages

Containerfile preview only includes user-intent (leaf) packages in the
dnf install line. Transitive dependencies are omitted. Degrades to
including all packages when leaf data is unavailable.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 6: Update View Stats and Frontend Package Count

**Files:**
- Modify: `inspectah-refine/src/session.rs` (stats computation)
- Modify: `inspectah-web/ui/src/components/StatsBar.tsx` (if needed — verify the stats bar shows package counts)

The stats in `RefinedView` (total_packages, included_packages, etc.) should reflect the filtered (leaf-only) count, not the raw 477 total. The user sees "23 packages" not "477 packages."

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn view_stats_reflect_leaf_filtered_count() {
    let mut snap = test_snapshot();
    snap.rpm.packages_added = vec![
        PackageEntry { name: "vim".into(), include: true, ..Default::default() },
        PackageEntry { name: "glibc".into(), include: true, ..Default::default() },
        PackageEntry { name: "ncurses".into(), include: true, ..Default::default() },
    ];
    snap.rpm.leaf_packages = Some(vec!["vim".into()]);

    let session = RefineSession::new(snap);
    let view = session.view();

    assert_eq!(view.stats.total_packages, 1); // only leaf
    assert_eq!(view.stats.included_packages, 1);
}
```

- [ ] **Step 2: Run test, verify fail**

- [ ] **Step 3: Update stats computation**

In the stats computation section of `recompute()`, use the filtered package list length instead of `packages_added.len()`.

- [ ] **Step 4: Run tests to verify they pass**

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add -u && git commit -m "feat(refine): view stats reflect leaf-filtered package counts

Total and included package counts show leaf packages only when leaf
data is available. Matches the filtered view the user sees.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 7: Full Test Suite and Clippy

**Files:**
- All modified files from Tasks 1-6

- [ ] **Step 1: Run full Rust test suite**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test --workspace`
Expected: PASS

- [ ] **Step 2: Run clippy**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && cargo clippy --workspace -- -W clippy::all`
Expected: No warnings

- [ ] **Step 3: Run frontend tests**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run`
Expected: PASS (frontend should work with fewer packages — no changes needed, just verify)

- [ ] **Step 4: Fix any issues found**

- [ ] **Step 5: Commit if any fixes were needed**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add -u && git commit -m "chore: fix clippy warnings and test regressions from leaf filter

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Verification

### Local verification (macOS)
1. Run `cargo test --workspace` — all Rust tests pass
2. Run `cargo clippy --workspace` — no warnings
3. Run `npx vitest run` in `inspectah-web/ui/` — all frontend tests pass

### Host verification (RHEL/CentOS VM)
1. Build the binary: `cargo build --release`
2. Run `inspectah scan` on a test system
3. Verify the snapshot `.tar.gz` contains `leaf_packages`, `auto_packages`, `leaf_dep_tree` in the RPM section (inspect with `tar xzf *.tar.gz -O snapshot.json | jq '.rpm.leaf_packages'`)
4. Run `inspectah refine <tarball>` and open the web UI
5. Verify the package list shows ~20-50 packages (not 477)
6. Verify the Containerfile preview has a short `dnf install` line with only user-intent packages
7. Expand a package card and verify dependencies are accessible (if dep-tree UI is wired — may be future task)
