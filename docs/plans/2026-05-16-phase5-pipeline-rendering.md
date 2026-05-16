# Phase 5: Pipeline Rendering & Triage Quality — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the flat NeedsReview attention model with baseline-aware three-tier classification, repo grouping with bulk actions, and Containerfile rendering fixes — reducing triage surface from ~734 to ~50-80 items.

**Architecture:** Two-pass classify-then-normalize in `inspectah-refine`, with `RepoIndex` for repo identity/cascade. Normalization materializes at session construction time into authoritative snapshot state. Repo cascade lives in the projection path (`project_snapshot()`), not in view-only recomputation. Pipeline fixes in `inspectah-pipeline`. Tiered UI in `inspectah-web`.

**Tech Stack:** Rust (inspectah-core, inspectah-refine, inspectah-pipeline, inspectah-web), React 19 + Vite + PatternFly 6 (web UI), Cargo test + Vitest + Playwright (testing)

**Spec:** `docs/specs/proposed/2026-05-16-phase5-pipeline-rendering-design.md` (approved after 3 review rounds)

**Ownership:**
- **Tang:** Tasks 1-12 (all Rust — types, attention, normalize, repo index, session projection, cascade, view filtering, containerfile, source_repo, API contract, TS mirror)
- **Kit:** Tasks 13-19 (all React/TypeScript — layout, tier cards, repo grouping, config grouping, search auto-reveal, keyboard/responsive, E2E)

**Hard gates:**
- Kit Tasks 14-18 are blocked on Tang Tasks 1-11 landing (pipeline + API contract)
- Kit Task 13 (layout CSS) is independent — can ship immediately
- Task 8 (source_repo proof) must complete with a passing test on a real CentOS Stream 9 tarball before Tang proceeds to Task 10 (API contract) or Kit proceeds to repo grouping tasks

**Session state model (critical — read before implementing Tasks 6-7):**
- `snapshot()` → the original normalized baseline. Does NOT change after construction.
- `project_snapshot()` → replays ops from the undo/redo stack onto a clone of `snapshot()`. This is the current truth for rendering.
- `snapshot_projected()` → public accessor calling `project_snapshot()`.
- `recompute_view()` → calls `project_snapshot()`, then `compute_*_attention()`, then renders Containerfile preview, then caches as `RefinedView`.
- **Repo cascade ops must live in `project_snapshot()`**, not in `recompute_view()`. Tests assert via `session.view()` or `session.snapshot_projected()`, never via `session.snapshot()` for post-op state.

**Build/test commands:**
```bash
export PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH"
cargo test -p inspectah-core
cargo test -p inspectah-refine
cargo test -p inspectah-pipeline
cargo test -p inspectah-web
cd inspectah-web/ui && npm test
# E2E requires running server — see Task 19
```

---

## Tang Tasks (Rust Pipeline)

### Task 1: Core Type Additions

**Files:**
- Modify: `inspectah-core/src/types/config.rs`
- Modify: `inspectah-refine/src/types.rs`
- Test: `inspectah-core/src/types/config.rs` (inline `#[cfg(test)]`)
- Test: `inspectah-refine/tests/serde_test.rs`

- [ ] **Step 1: Add `BaselineMatch` to `ConfigFileKind`**

In `inspectah-core/src/types/config.rs`, add the variant with serde alias:

```rust
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigFileKind {
    RpmOwnedDefault,
    RpmOwnedModified,
    #[default]
    Unowned,
    Orphaned,
    #[serde(alias = "baseline_match")]
    BaselineMatch,
}
```

- [ ] **Step 2: Write serde round-trip test for `BaselineMatch`**

In the existing `#[cfg(test)] mod tests` in `config.rs`, add:

```rust
#[test]
fn test_baseline_match_roundtrip() {
    assert_eq!(
        serde_json::to_string(&ConfigFileKind::BaselineMatch).unwrap(),
        r#""baseline_match""#
    );
    let parsed: ConfigFileKind = serde_json::from_str(r#""baseline_match""#).unwrap();
    assert_eq!(parsed, ConfigFileKind::BaselineMatch);
}
```

- [ ] **Step 3: Run test**

Run: `cargo test -p inspectah-core test_baseline_match_roundtrip`
Expected: PASS

- [ ] **Step 4: Add new `AttentionReason` variants**

In `inspectah-refine/src/types.rs`, replace the current `AttentionReason` enum:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionReason {
    PackageBaselineMatch,
    PackageUserAdded,
    PackageVersionChanged,
    PackageProvenanceUnavailable,
    PackageLocalInstall,
    PackageNoRepoSource,
    ConfigDefault,
    ConfigBaselineMatch,
    ConfigModified,
    ConfigUnowned,
    ConfigOrphaned,
    SensitivePath,
    Custom(String),
}
```

- [ ] **Step 5: Add `RepoProvenance` enum**

In `inspectah-refine/src/types.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepoProvenance {
    Verified,
    Incomplete,
    Unknown,
}
```

- [ ] **Step 6: Add `ExcludeRepo` / `IncludeRepo` to `RefinementOp`**

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", content = "target")]
pub enum RefinementOp {
    ExcludePackage(PackageTarget),
    IncludePackage(PackageTarget),
    ExcludeConfig { path: PathBuf },
    IncludeConfig { path: PathBuf },
    ExcludeRepo { section_id: String },
    IncludeRepo { section_id: String },
}
```

- [ ] **Step 7: Extend `ChangesSummary` and `RefineStats`**

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangesSummary {
    pub packages_included: Vec<PackageTarget>,
    pub packages_excluded: Vec<PackageTarget>,
    pub configs_included: Vec<String>,
    pub configs_excluded: Vec<String>,
    pub repos_excluded: Vec<String>,
    pub is_dirty: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefineStats {
    pub total_packages: usize,
    pub included_packages: usize,
    pub excluded_packages: usize,
    pub total_configs: usize,
    pub included_configs: usize,
    pub package_managed_configs: usize,
    pub excluded_configs: usize,
    pub needs_review_count: usize,
    pub ops_applied: usize,
    pub can_undo: bool,
    pub can_redo: bool,
    pub baseline_available: bool,
}
```

`baseline_available` signals to the UI whether baseline data was present at import time. Kit uses this for the provenance completeness banner.

- [ ] **Step 8: Write serde round-trip tests for new variants**

In `inspectah-refine/tests/serde_test.rs`:

```rust
#[test]
fn test_exclude_repo_op_roundtrip() {
    let op = RefinementOp::ExcludeRepo { section_id: "epel".into() };
    let json = serde_json::to_string(&op).unwrap();
    let parsed: RefinementOp = serde_json::from_str(&json).unwrap();
    assert_eq!(op, parsed);
}

#[test]
fn test_new_attention_reasons_roundtrip() {
    let reasons = vec![
        AttentionReason::PackageBaselineMatch,
        AttentionReason::PackageUserAdded,
        AttentionReason::PackageVersionChanged,
        AttentionReason::PackageProvenanceUnavailable,
        AttentionReason::PackageNoRepoSource,
        AttentionReason::ConfigDefault,
        AttentionReason::ConfigBaselineMatch,
    ];
    for reason in &reasons {
        let json = serde_json::to_string(reason).unwrap();
        let parsed: AttentionReason = serde_json::from_str(&json).unwrap();
        assert_eq!(*reason, parsed);
    }
}

#[test]
fn test_repo_provenance_roundtrip() {
    for prov in &[RepoProvenance::Verified, RepoProvenance::Incomplete, RepoProvenance::Unknown] {
        let json = serde_json::to_string(prov).unwrap();
        let parsed: RepoProvenance = serde_json::from_str(&json).unwrap();
        assert_eq!(*prov, parsed);
    }
}
```

- [ ] **Step 9: Fix compilation from renamed reasons**

Update `attention.rs` to compile with new reason names (temporary — Task 2 rewrites the function bodies).

- [ ] **Step 10: Run all tests**

Run: `cargo test -p inspectah-core && cargo test -p inspectah-refine`
Expected: All PASS.

- [ ] **Step 11: Commit**

```bash
git add inspectah-core/src/types/config.rs inspectah-refine/src/types.rs inspectah-refine/tests/serde_test.rs inspectah-refine/src/attention.rs
git commit -m "feat(core): add BaselineMatch, new attention reasons, RepoProvenance, ExcludeRepo ops"
```

---

### Task 2: Package Classification (Classify Pass 1)

**Files:**
- Modify: `inspectah-refine/src/attention.rs`
- Test: `inspectah-refine/tests/attention_test.rs`

**Reference:** Spec Section 2, complete classification matrix.

- [ ] **Step 1: Write failing tests for the classification matrix**

In `inspectah-refine/tests/attention_test.rs`, add tests covering the exhaustive matrix. Each test builds a minimal `InspectionSnapshot` with specific `PackageState`, `baseline_package_names`, and `source_repo` values, then asserts the expected tier and reason.

```rust
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
use inspectah_refine::attention::compute_package_attention;
use inspectah_refine::types::{AttentionLevel, AttentionReason};

fn make_snap_with_package(
    name: &str,
    state: PackageState,
    source_repo: &str,
    baseline: Option<Vec<String>>,
) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: name.into(),
            arch: "x86_64".into(),
            state,
            source_repo: source_repo.into(),
            include: true,
            ..Default::default()
        }],
        baseline_package_names: baseline,
        ..Default::default()
    });
    snap
}

#[test]
fn test_added_baseline_match_is_tier1() {
    let snap = make_snap_with_package(
        "glibc", PackageState::Added, "baseos",
        Some(vec!["glibc".into()]),
    );
    let pkgs = compute_package_attention(&snap);
    assert_eq!(pkgs[0].attention[0].level, AttentionLevel::Routine);
    assert_eq!(pkgs[0].attention[0].reason, AttentionReason::PackageBaselineMatch);
}

#[test]
fn test_added_not_in_baseline_known_repo_is_tier2() {
    let snap = make_snap_with_package(
        "httpd", PackageState::Added, "appstream",
        Some(vec!["glibc".into()]),
    );
    let pkgs = compute_package_attention(&snap);
    assert_eq!(pkgs[0].attention[0].level, AttentionLevel::Informational);
    assert_eq!(pkgs[0].attention[0].reason, AttentionReason::PackageUserAdded);
}

#[test]
fn test_added_not_in_baseline_empty_repo_is_tier3() {
    let snap = make_snap_with_package(
        "mystery", PackageState::Added, "",
        Some(vec!["glibc".into()]),
    );
    let pkgs = compute_package_attention(&snap);
    assert_eq!(pkgs[0].attention[0].level, AttentionLevel::NeedsReview);
    assert_eq!(pkgs[0].attention[0].reason, AttentionReason::PackageNoRepoSource);
}

#[test]
fn test_added_no_baseline_known_repo_is_provenance_unavailable() {
    let snap = make_snap_with_package("httpd", PackageState::Added, "appstream", None);
    let pkgs = compute_package_attention(&snap);
    assert_eq!(pkgs[0].attention[0].level, AttentionLevel::Informational);
    assert_eq!(pkgs[0].attention[0].reason, AttentionReason::PackageProvenanceUnavailable);
}

#[test]
fn test_added_no_baseline_empty_repo_is_tier3() {
    let snap = make_snap_with_package("mystery", PackageState::Added, "", None);
    let pkgs = compute_package_attention(&snap);
    assert_eq!(pkgs[0].attention[0].level, AttentionLevel::NeedsReview);
    assert_eq!(pkgs[0].attention[0].reason, AttentionReason::PackageNoRepoSource);
}

#[test]
fn test_modified_baseline_match_is_tier1() {
    let snap = make_snap_with_package(
        "glibc", PackageState::Modified, "baseos",
        Some(vec!["glibc".into()]),
    );
    let pkgs = compute_package_attention(&snap);
    assert_eq!(pkgs[0].attention[0].level, AttentionLevel::Routine);
    assert_eq!(pkgs[0].attention[0].reason, AttentionReason::PackageBaselineMatch);
}

#[test]
fn test_modified_not_in_baseline_known_repo_is_version_changed() {
    let snap = make_snap_with_package(
        "httpd", PackageState::Modified, "appstream",
        Some(vec!["glibc".into()]),
    );
    let pkgs = compute_package_attention(&snap);
    assert_eq!(pkgs[0].attention[0].level, AttentionLevel::Informational);
    assert_eq!(pkgs[0].attention[0].reason, AttentionReason::PackageVersionChanged);
}

#[test]
fn test_local_install_always_tier3() {
    for baseline in [Some(vec!["glibc".into()]), None] {
        for repo in ["appstream", ""] {
            let snap = make_snap_with_package(
                "custom", PackageState::LocalInstall, repo, baseline.clone(),
            );
            let pkgs = compute_package_attention(&snap);
            assert_eq!(pkgs[0].attention[0].level, AttentionLevel::NeedsReview);
            assert_eq!(pkgs[0].attention[0].reason, AttentionReason::PackageLocalInstall);
        }
    }
}

#[test]
fn test_no_repo_always_tier3() {
    for baseline in [Some(vec!["glibc".into()]), None] {
        let snap = make_snap_with_package("orphan", PackageState::NoRepo, "", baseline);
        let pkgs = compute_package_attention(&snap);
        assert_eq!(pkgs[0].attention[0].level, AttentionLevel::NeedsReview);
        assert_eq!(pkgs[0].attention[0].reason, AttentionReason::PackageNoRepoSource);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-refine test_added_baseline`
Expected: FAIL

- [ ] **Step 3: Rewrite `compute_package_attention()`**

Replace the function body in `inspectah-refine/src/attention.rs`:

```rust
pub fn compute_package_attention(snap: &InspectionSnapshot) -> Vec<RefinedPackage> {
    let rpm = match &snap.rpm {
        Some(r) => r,
        None => return Vec::new(),
    };

    let baseline: Option<&[String]> = rpm.baseline_package_names.as_deref();

    rpm.packages_added
        .iter()
        .map(|entry| {
            let tag = classify_package(entry, baseline);
            let mut tags = vec![tag];

            if is_sensitive_path(&entry.name) {
                let primary_level = tags[0].level;
                let should_promote = match primary_level {
                    AttentionLevel::Informational => true,
                    AttentionLevel::Routine => baseline.is_none(),
                    AttentionLevel::NeedsReview => false,
                };
                if should_promote {
                    tags.push(AttentionTag {
                        level: AttentionLevel::NeedsReview,
                        reason: AttentionReason::SensitivePath,
                        detail: Some(entry.name.clone()),
                    });
                }
            }

            RefinedPackage { entry: entry.clone(), attention: tags }
        })
        .collect()
}

fn classify_package(entry: &PackageEntry, baseline: Option<&[String]>) -> AttentionTag {
    match entry.state {
        PackageState::LocalInstall => {
            return AttentionTag {
                level: AttentionLevel::NeedsReview,
                reason: AttentionReason::PackageLocalInstall,
                detail: None,
            };
        }
        PackageState::NoRepo => {
            return AttentionTag {
                level: AttentionLevel::NeedsReview,
                reason: AttentionReason::PackageNoRepoSource,
                detail: None,
            };
        }
        _ => {}
    }

    if entry.source_repo.is_empty() {
        return AttentionTag {
            level: AttentionLevel::NeedsReview,
            reason: AttentionReason::PackageNoRepoSource,
            detail: None,
        };
    }

    match baseline {
        Some(names) if names.iter().any(|n| n == &entry.name) => {
            AttentionTag {
                level: AttentionLevel::Routine,
                reason: AttentionReason::PackageBaselineMatch,
                detail: None,
            }
        }
        Some(_) => {
            let reason = match entry.state {
                PackageState::Modified => AttentionReason::PackageVersionChanged,
                _ => AttentionReason::PackageUserAdded,
            };
            AttentionTag { level: AttentionLevel::Informational, reason, detail: None }
        }
        None => {
            AttentionTag {
                level: AttentionLevel::Informational,
                reason: AttentionReason::PackageProvenanceUnavailable,
                detail: None,
            }
        }
    }
}
```

- [ ] **Step 4: Run all package classification tests**

Run: `cargo test -p inspectah-refine -- test_added test_modified test_local test_no_repo`
Expected: All PASS.

- [ ] **Step 5: Commit**

```bash
git add inspectah-refine/src/attention.rs inspectah-refine/tests/attention_test.rs
git commit -m "feat(refine): baseline-aware package classification with exhaustive matrix"
```

---

### Task 3: Config Classification (Classify Pass 1 continued)

**Files:**
- Modify: `inspectah-refine/src/attention.rs`
- Test: `inspectah-refine/tests/attention_test.rs`

- [ ] **Step 1: Write failing tests for config classification**

```rust
use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind, ConfigSection};

fn make_snap_with_config(path: &str, kind: ConfigFileKind) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: path.into(),
            kind,
            include: true,
            ..Default::default()
        }],
    });
    snap
}

#[test]
fn test_config_rpm_owned_default_is_tier1() {
    let snap = make_snap_with_config("/etc/httpd/conf/httpd.conf", ConfigFileKind::RpmOwnedDefault);
    let configs = compute_config_attention(&snap);
    assert_eq!(configs[0].attention[0].level, AttentionLevel::Routine);
    assert_eq!(configs[0].attention[0].reason, AttentionReason::ConfigDefault);
}

#[test]
fn test_config_baseline_match_is_tier1() {
    let snap = make_snap_with_config("/etc/sysconfig/network", ConfigFileKind::BaselineMatch);
    let configs = compute_config_attention(&snap);
    assert_eq!(configs[0].attention[0].level, AttentionLevel::Routine);
    assert_eq!(configs[0].attention[0].reason, AttentionReason::ConfigBaselineMatch);
}

#[test]
fn test_config_unowned_is_tier2() {
    let snap = make_snap_with_config("/etc/custom.conf", ConfigFileKind::Unowned);
    let configs = compute_config_attention(&snap);
    assert_eq!(configs[0].attention[0].level, AttentionLevel::Informational);
    assert_eq!(configs[0].attention[0].reason, AttentionReason::ConfigUnowned);
}

#[test]
fn test_config_rpm_owned_modified_is_tier3() {
    let snap = make_snap_with_config("/etc/ssh/sshd_config", ConfigFileKind::RpmOwnedModified);
    let configs = compute_config_attention(&snap);
    assert_eq!(configs[0].attention[0].level, AttentionLevel::NeedsReview);
    assert_eq!(configs[0].attention[0].reason, AttentionReason::ConfigModified);
}

#[test]
fn test_config_sensitive_path_promotes_tier2_only() {
    let snap = make_snap_with_config("/etc/ssh/custom_keys", ConfigFileKind::Unowned);
    let configs = compute_config_attention(&snap);
    assert!(configs[0].attention.iter().any(|t| t.reason == AttentionReason::SensitivePath));

    let snap2 = make_snap_with_config("/etc/pki/tls/cert.pem", ConfigFileKind::RpmOwnedDefault);
    let configs2 = compute_config_attention(&snap2);
    assert!(!configs2[0].attention.iter().any(|t| t.reason == AttentionReason::SensitivePath));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-refine test_config_`
Expected: FAIL

- [ ] **Step 3: Rewrite `compute_config_attention()`**

```rust
pub fn compute_config_attention(snap: &InspectionSnapshot) -> Vec<RefinedConfig> {
    let config = match &snap.config {
        Some(c) => c,
        None => return Vec::new(),
    };

    let mut configs: Vec<RefinedConfig> = config.files
        .iter()
        .map(|entry| {
            let tag = match entry.kind {
                ConfigFileKind::RpmOwnedDefault => AttentionTag {
                    level: AttentionLevel::Routine,
                    reason: AttentionReason::ConfigDefault,
                    detail: None,
                },
                ConfigFileKind::BaselineMatch => AttentionTag {
                    level: AttentionLevel::Routine,
                    reason: AttentionReason::ConfigBaselineMatch,
                    detail: None,
                },
                ConfigFileKind::Unowned => AttentionTag {
                    level: AttentionLevel::Informational,
                    reason: AttentionReason::ConfigUnowned,
                    detail: None,
                },
                ConfigFileKind::RpmOwnedModified => AttentionTag {
                    level: AttentionLevel::NeedsReview,
                    reason: AttentionReason::ConfigModified,
                    detail: None,
                },
                ConfigFileKind::Orphaned => AttentionTag {
                    level: AttentionLevel::Informational,
                    reason: AttentionReason::ConfigOrphaned,
                    detail: None,
                },
            };

            let mut tags = vec![tag];
            if is_sensitive_path(&entry.path) && tags[0].level == AttentionLevel::Informational {
                tags.push(AttentionTag {
                    level: AttentionLevel::NeedsReview,
                    reason: AttentionReason::SensitivePath,
                    detail: Some(entry.path.clone()),
                });
            }

            RefinedConfig { entry: entry.clone(), attention: tags }
        })
        .collect();

    if let Some(RedactionState::PartiallyRedacted { ref unresolved_hints, .. }) = snap.redaction_state {
        for hint in unresolved_hints {
            if let Some(cfg) = configs.iter_mut().find(|c| c.entry.path == hint.path) {
                cfg.attention.push(AttentionTag {
                    level: AttentionLevel::NeedsReview,
                    reason: AttentionReason::Custom("unresolved redaction hint".into()),
                    detail: Some(hint.reason.clone()),
                });
            }
        }
    }

    configs
}
```

- [ ] **Step 4: Run config tests**

Run: `cargo test -p inspectah-refine test_config_`
Expected: All PASS.

- [ ] **Step 5: Run full test suite**

Run: `cargo test -p inspectah-refine`
Expected: All PASS.

- [ ] **Step 6: Commit**

```bash
git add inspectah-refine/src/attention.rs inspectah-refine/tests/attention_test.rs
git commit -m "feat(refine): config classification with BaselineMatch, intentional RpmOwnedModified→Tier3"
```

---

### Task 4: RepoIndex Construction

**Files:**
- Create: `inspectah-refine/src/repo_index.rs`
- Modify: `inspectah-refine/src/lib.rs` (add `pub mod repo_index;`)
- Test: `inspectah-refine/tests/repo_index_test.rs`

- [ ] **Step 1: Write failing tests for RepoIndex**

Create `inspectah-refine/tests/repo_index_test.rs`. This file defines the shared `make_snap_with_repos()` helper that later tasks will also need. To share across integration test files, extract into a shared module later (Task 7), or inline in each test file.

```rust
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::rpm::{PackageEntry, PackageState, RepoFile, RpmSection};
use inspectah_refine::repo_index::RepoIndex;
use inspectah_refine::types::RepoProvenance;

pub fn make_snap_with_repos() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "httpd".into(), arch: "x86_64".into(),
                state: PackageState::Added, source_repo: "appstream".into(),
                include: true, ..Default::default()
            },
            PackageEntry {
                name: "epel-release".into(), arch: "noarch".into(),
                state: PackageState::Added, source_repo: "epel".into(),
                include: true, ..Default::default()
            },
        ],
        repo_files: vec![
            RepoFile {
                path: "/etc/yum.repos.d/centos.repo".into(),
                content: "[baseos]\nname=CentOS BaseOS\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-centosofficial\n\n[appstream]\nname=CentOS AppStream\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-centosofficial\n".into(),
                include: true, ..Default::default()
            },
            RepoFile {
                path: "/etc/yum.repos.d/epel.repo".into(),
                content: "[epel]\nname=EPEL 9\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9\n".into(),
                include: true, ..Default::default()
            },
        ],
        gpg_keys: vec![
            RepoFile {
                path: "/etc/pki/rpm-gpg/RPM-GPG-KEY-centosofficial".into(),
                content: "key-data".into(), include: true, ..Default::default()
            },
            RepoFile {
                path: "/etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9".into(),
                content: "key-data".into(), include: true, ..Default::default()
            },
        ],
        ..Default::default()
    });
    snap
}

#[test]
fn test_repo_index_packages_by_repo() {
    let snap = make_snap_with_repos();
    let index = RepoIndex::build(&snap);
    assert_eq!(index.packages_by_repo.get("appstream").unwrap().len(), 1);
    assert_eq!(index.packages_by_repo.get("epel").unwrap().len(), 1);
}

#[test]
fn test_repo_index_multi_section_repo_file() {
    let snap = make_snap_with_repos();
    let index = RepoIndex::build(&snap);
    let baseos_files = index.repo_file_by_section.get("baseos").unwrap();
    let appstream_files = index.repo_file_by_section.get("appstream").unwrap();
    assert!(baseos_files.contains(&"/etc/yum.repos.d/centos.repo".to_string()));
    assert!(appstream_files.contains(&"/etc/yum.repos.d/centos.repo".to_string()));
}

#[test]
fn test_repo_index_gpg_shared_key() {
    let snap = make_snap_with_repos();
    let index = RepoIndex::build(&snap);
    let sections = index.sections_by_gpg_key
        .get("/etc/pki/rpm-gpg/RPM-GPG-KEY-centosofficial").unwrap();
    assert!(sections.contains("baseos"));
    assert!(sections.contains("appstream"));
}

#[test]
fn test_repo_index_provenance_verified() {
    let snap = make_snap_with_repos();
    let index = RepoIndex::build(&snap);
    assert_eq!(index.provenance("appstream"), RepoProvenance::Verified);
    assert_eq!(index.provenance("epel"), RepoProvenance::Verified);
}

#[test]
fn test_repo_index_provenance_incomplete() {
    let mut snap = make_snap_with_repos();
    snap.rpm.as_mut().unwrap().packages_added.push(PackageEntry {
        name: "custom-pkg".into(), arch: "x86_64".into(),
        state: PackageState::Added, source_repo: "custom-internal".into(),
        include: true, ..Default::default()
    });
    let index = RepoIndex::build(&snap);
    assert_eq!(index.provenance("custom-internal"), RepoProvenance::Incomplete);
}

#[test]
fn test_repo_index_provenance_unknown_empty_repo() {
    let index = RepoIndex::build(&make_snap_with_repos());
    assert_eq!(index.provenance(""), RepoProvenance::Unknown);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-refine test_repo_index`
Expected: FAIL — module doesn't exist.

- [ ] **Step 3: Implement `RepoIndex`**

Create `inspectah-refine/src/repo_index.rs`:

```rust
use std::collections::{BTreeMap, BTreeSet};
use inspectah_core::snapshot::InspectionSnapshot;
use crate::types::RepoProvenance;

pub const DISTRO_REPOS: &[&str] = &[
    "baseos", "appstream", "crb", "fedora", "updates", "anaconda",
];

pub struct RepoIndex {
    pub packages_by_repo: BTreeMap<String, Vec<String>>,
    pub repo_file_by_section: BTreeMap<String, Vec<String>>,
    pub gpg_keys_by_section: BTreeMap<String, Vec<String>>,
    pub sections_by_gpg_key: BTreeMap<String, BTreeSet<String>>,
    provenance_map: BTreeMap<String, RepoProvenance>,
}

impl RepoIndex {
    pub fn build(snap: &InspectionSnapshot) -> Self {
        let rpm = match &snap.rpm {
            Some(r) => r,
            None => return Self::empty(),
        };

        let mut repo_file_by_section: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut gpg_keys_by_section: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut sections_by_gpg_key: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

        for rf in &rpm.repo_files {
            for section in parse_repo_sections(&rf.content) {
                repo_file_by_section.entry(section.id.clone()).or_default().push(rf.path.clone());
                for key_path in &section.gpg_key_paths {
                    gpg_keys_by_section.entry(section.id.clone()).or_default().push(key_path.clone());
                    sections_by_gpg_key.entry(key_path.clone()).or_default().insert(section.id.clone());
                }
            }
        }

        let mut packages_by_repo: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for pkg in &rpm.packages_added {
            if !pkg.source_repo.is_empty() {
                packages_by_repo.entry(pkg.source_repo.clone()).or_default().push(pkg.name.clone());
            }
        }

        let mut provenance_map: BTreeMap<String, RepoProvenance> = BTreeMap::new();
        let all_ids: BTreeSet<String> = packages_by_repo.keys()
            .chain(repo_file_by_section.keys()).cloned().collect();
        for sid in &all_ids {
            if sid.is_empty() { provenance_map.insert(sid.clone(), RepoProvenance::Unknown); continue; }
            let has_repo_file = repo_file_by_section.contains_key(sid);
            provenance_map.insert(sid.clone(), if has_repo_file { RepoProvenance::Verified } else { RepoProvenance::Incomplete });
        }

        Self { packages_by_repo, repo_file_by_section, gpg_keys_by_section, sections_by_gpg_key, provenance_map }
    }

    pub fn provenance(&self, section_id: &str) -> RepoProvenance {
        if section_id.is_empty() { return RepoProvenance::Unknown; }
        self.provenance_map.get(section_id).copied().unwrap_or(RepoProvenance::Unknown)
    }

    pub fn is_distro_repo(section_id: &str) -> bool { DISTRO_REPOS.contains(&section_id) }

    fn empty() -> Self {
        Self { packages_by_repo: BTreeMap::new(), repo_file_by_section: BTreeMap::new(),
               gpg_keys_by_section: BTreeMap::new(), sections_by_gpg_key: BTreeMap::new(),
               provenance_map: BTreeMap::new() }
    }
}

struct RepoSection { id: String, gpg_key_paths: Vec<String> }

fn parse_repo_sections(content: &str) -> Vec<RepoSection> {
    let mut sections = Vec::new();
    let mut current_id: Option<String> = None;
    let mut current_keys: Vec<String> = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if let Some(id) = current_id.take() {
                sections.push(RepoSection { id, gpg_key_paths: current_keys.clone() });
                current_keys.clear();
            }
            current_id = Some(trimmed[1..trimmed.len()-1].to_string());
        } else if let Some(value) = trimmed.strip_prefix("gpgkey=") {
            for path in value.split([',', ' ']) {
                if let Some(file_path) = path.trim().strip_prefix("file://") {
                    current_keys.push(file_path.to_string());
                }
            }
        }
    }
    if let Some(id) = current_id {
        sections.push(RepoSection { id, gpg_key_paths: current_keys });
    }
    sections
}
```

- [ ] **Step 4: Register the module in `lib.rs`**

- [ ] **Step 5: Run tests**

Run: `cargo test -p inspectah-refine test_repo_index`
Expected: All PASS.

- [ ] **Step 6: Commit**

```bash
git add inspectah-refine/src/repo_index.rs inspectah-refine/src/lib.rs inspectah-refine/tests/repo_index_test.rs
git commit -m "feat(refine): RepoIndex with INI parsing, provenance computation, GPG ref counting"
```

---

### Task 5: Normalize Defaults (Pass 2)

**Files:**
- Modify: `inspectah-refine/src/normalize.rs`
- Test: `inspectah-refine/tests/normalize_test.rs`

- [ ] **Step 1: Write failing tests**

In `inspectah-refine/tests/normalize_test.rs`:

```rust
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind, ConfigSection};
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
use inspectah_refine::attention::{compute_config_attention, compute_package_attention};
use inspectah_refine::normalize::{normalize_config_defaults, normalize_package_defaults};

#[test]
fn test_tier1_packages_include_true() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "glibc".into(), arch: "x86_64".into(),
            state: PackageState::Added, source_repo: "baseos".into(),
            include: false, ..Default::default()
        }],
        baseline_package_names: Some(vec!["glibc".into()]),
        ..Default::default()
    });
    let pkgs = compute_package_attention(&snap);
    normalize_package_defaults(&mut snap, &pkgs);
    assert!(snap.rpm.as_ref().unwrap().packages_added[0].include);
}

#[test]
fn test_tier3_packages_include_false() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "mystery".into(), arch: "x86_64".into(),
            state: PackageState::LocalInstall, source_repo: "".into(),
            include: true, ..Default::default()
        }],
        ..Default::default()
    });
    let pkgs = compute_package_attention(&snap);
    normalize_package_defaults(&mut snap, &pkgs);
    assert!(!snap.rpm.as_ref().unwrap().packages_added[0].include);
}

#[test]
fn test_leaf_filtering_hides_non_leaf_tier2() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry { name: "httpd".into(), arch: "x86_64".into(),
                state: PackageState::Added, source_repo: "appstream".into(),
                include: true, ..Default::default() },
            PackageEntry { name: "apr".into(), arch: "x86_64".into(),
                state: PackageState::Added, source_repo: "appstream".into(),
                include: true, ..Default::default() },
        ],
        baseline_package_names: Some(vec![]),
        leaf_packages: Some(vec!["httpd".into()]),
        ..Default::default()
    });
    let pkgs = compute_package_attention(&snap);
    normalize_package_defaults(&mut snap, &pkgs);
    let rpm = snap.rpm.as_ref().unwrap();
    assert!(rpm.packages_added[0].include, "httpd is leaf");
    assert!(!rpm.packages_added[1].include, "apr is non-leaf, hidden");
}

#[test]
fn test_tier1_configs_include_false_not_copied() {
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        files: vec![
            ConfigFileEntry { path: "/etc/default.conf".into(),
                kind: ConfigFileKind::RpmOwnedDefault, include: true, ..Default::default() },
            ConfigFileEntry { path: "/etc/baseline.conf".into(),
                kind: ConfigFileKind::BaselineMatch, include: true, ..Default::default() },
            ConfigFileEntry { path: "/etc/custom.conf".into(),
                kind: ConfigFileKind::Unowned, include: true, ..Default::default() },
        ],
    });
    let configs = compute_config_attention(&snap);
    normalize_config_defaults(&mut snap, &configs);
    let files = &snap.config.as_ref().unwrap().files;
    assert!(!files[0].include, "RpmOwnedDefault must not be copied");
    assert!(!files[1].include, "BaselineMatch must not be copied");
    assert!(files[2].include, "Unowned must be copied");
}

#[test]
fn test_orphaned_configs_include_false() {
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/old.conf".into(), kind: ConfigFileKind::Orphaned,
            include: true, ..Default::default()
        }],
    });
    let configs = compute_config_attention(&snap);
    normalize_config_defaults(&mut snap, &configs);
    assert!(!snap.config.as_ref().unwrap().files[0].include);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-refine -- test_tier1 test_tier3 test_leaf test_orphaned`
Expected: FAIL.

- [ ] **Step 3: Implement normalize functions**

Replace `inspectah-refine/src/normalize.rs`:

```rust
use inspectah_core::snapshot::InspectionSnapshot;
use crate::types::{AttentionLevel, RefinedConfig, RefinedPackage};

pub fn normalize_package_defaults(
    snapshot: &mut InspectionSnapshot,
    packages: &[RefinedPackage],
) {
    let rpm = match snapshot.rpm.as_mut() {
        Some(r) => r,
        None => return,
    };

    let leaf_set: Option<std::collections::HashSet<&str>> = rpm.leaf_packages
        .as_ref()
        .map(|lp| lp.iter().map(|s| s.as_str()).collect());

    for (i, refined) in packages.iter().enumerate() {
        if i >= rpm.packages_added.len() { break; }
        let primary_level = refined.attention.first()
            .map(|t| t.level).unwrap_or(AttentionLevel::Routine);
        match primary_level {
            AttentionLevel::Routine => { rpm.packages_added[i].include = true; }
            AttentionLevel::Informational => {
                let is_leaf = match &leaf_set {
                    Some(set) => set.contains(rpm.packages_added[i].name.as_str()),
                    None => true,
                };
                rpm.packages_added[i].include = is_leaf;
            }
            AttentionLevel::NeedsReview => { rpm.packages_added[i].include = false; }
        }
    }
}

pub fn normalize_config_defaults(
    snapshot: &mut InspectionSnapshot,
    configs: &[RefinedConfig],
) {
    let config = match snapshot.config.as_mut() {
        Some(c) => c,
        None => return,
    };
    for (i, refined) in configs.iter().enumerate() {
        if i >= config.files.len() { break; }
        let primary_level = refined.attention.first()
            .map(|t| t.level).unwrap_or(AttentionLevel::Routine);
        match primary_level {
            AttentionLevel::Routine => { config.files[i].include = false; }
            AttentionLevel::Informational => {
                config.files[i].include = !matches!(
                    config.files[i].kind,
                    inspectah_core::types::config::ConfigFileKind::Orphaned
                );
            }
            AttentionLevel::NeedsReview => { config.files[i].include = true; }
        }
    }
}
```

- [ ] **Step 4: Run normalize tests**

Run: `cargo test -p inspectah-refine -- test_tier1 test_tier3 test_leaf test_orphaned`
Expected: All PASS.

- [ ] **Step 5: Commit**

```bash
git add inspectah-refine/src/normalize.rs inspectah-refine/tests/normalize_test.rs
git commit -m "feat(refine): tier-aware normalize with Tier1 config omission, leaf filtering"
```

---

### Task 6: Session Construction — Normalize at Import + RepoIndex

**Files:**
- Modify: `inspectah-refine/src/session.rs`
- Test: `inspectah-refine/tests/session_test.rs`

This task integrates normalization and RepoIndex into the session constructor. Repo cascade ops are Task 7 (separate, smaller slice).

- [ ] **Step 1: Write failing tests**

In `inspectah-refine/tests/session_test.rs`:

```rust
#[test]
fn test_session_normalizes_at_construction() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "glibc".into(), arch: "x86_64".into(),
            state: PackageState::Added, source_repo: "baseos".into(),
            include: false, ..Default::default()
        }],
        baseline_package_names: Some(vec!["glibc".into()]),
        ..Default::default()
    });
    let session = RefineSession::new(snap);
    // View should reflect normalized state (Tier 1 = include true)
    let view = session.view();
    assert!(view.packages[0].entry.include);
    // Original snapshot also has normalized include (materialized at construction)
    assert!(session.snapshot().rpm.as_ref().unwrap().packages_added[0].include);
}

#[test]
fn test_session_preview_export_parity() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "httpd".into(), arch: "x86_64".into(),
            state: PackageState::Added, source_repo: "appstream".into(),
            include: false, ..Default::default()
        }],
        baseline_package_names: Some(vec![]),
        ..Default::default()
    });
    let session = RefineSession::new(snap);
    // Both preview and projected snapshot agree on include state
    assert!(session.view().packages[0].entry.include);
    assert!(session.snapshot_projected().rpm.as_ref().unwrap().packages_added[0].include);
    assert!(session.view().containerfile_preview.contains("httpd"));
}

#[test]
fn test_session_baseline_available_in_stats() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        baseline_package_names: Some(vec!["glibc".into()]),
        ..Default::default()
    });
    let session = RefineSession::new(snap);
    assert!(session.view().stats.baseline_available);

    let snap_no_baseline = InspectionSnapshot::new();
    let session2 = RefineSession::new(snap_no_baseline);
    assert!(!session2.view().stats.baseline_available);
}

#[test]
fn test_tier1_configs_not_in_containerfile() {
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        files: vec![
            ConfigFileEntry { path: "/etc/default.conf".into(),
                kind: ConfigFileKind::RpmOwnedDefault, include: true, ..Default::default() },
            ConfigFileEntry { path: "/etc/custom.conf".into(),
                kind: ConfigFileKind::Unowned, include: true,
                content: "custom content".into(), ..Default::default() },
        ],
    });
    let session = RefineSession::new(snap);
    let preview = &session.view().containerfile_preview;
    assert!(!preview.contains("default.conf"), "Tier 1 config must not appear in Containerfile");
    assert!(preview.contains("custom.conf") || preview.contains("config/etc"),
        "Tier 2 config must appear in Containerfile");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-refine test_session_normalizes test_session_preview test_session_baseline test_tier1_configs_not`
Expected: FAIL.

- [ ] **Step 3: Integrate normalization and RepoIndex into constructor**

In `inspectah-refine/src/session.rs`:

```rust
use crate::repo_index::RepoIndex;
use crate::normalize::{normalize_package_defaults, normalize_config_defaults};

pub struct RefineSession {
    original: InspectionSnapshot,
    repo_index: RepoIndex,
    baseline_available: bool,
    ops: Vec<RefinementOp>,
    cursor: usize,
    cached_view: Option<RefinedView>,
    generation: u64,
    viewed: HashSet<String>,
}

impl RefineSession {
    pub fn new(mut snapshot: InspectionSnapshot) -> Self {
        let repo_index = RepoIndex::build(&snapshot);
        let baseline_available = snapshot.rpm.as_ref()
            .and_then(|r| r.baseline_package_names.as_ref())
            .is_some();

        // Classify then normalize — materializes into snapshot state
        let pkgs = compute_package_attention(&snapshot);
        let configs = compute_config_attention(&snapshot);
        normalize_package_defaults(&mut snapshot, &pkgs);
        normalize_config_defaults(&mut snapshot, &configs);

        let mut session = Self {
            original: snapshot,
            repo_index,
            baseline_available,
            ops: Vec::new(),
            cursor: 0,
            cached_view: None,
            generation: 0,
            viewed: HashSet::new(),
        };
        session.recompute_view();
        session
    }

    pub fn repo_index(&self) -> &RepoIndex { &self.repo_index }
    // ...existing methods unchanged...
}
```

Update `recompute_view()` to set `stats.baseline_available` and `stats.package_managed_configs`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p inspectah-refine test_session_normalizes test_session_preview test_session_baseline test_tier1_configs_not`
Expected: All PASS.

- [ ] **Step 5: Run full suite**

Run: `cargo test -p inspectah-refine`
Expected: All PASS.

- [ ] **Step 6: Commit**

```bash
git add inspectah-refine/src/session.rs inspectah-refine/tests/session_test.rs
git commit -m "feat(refine): normalize at session construction, RepoIndex integration, baseline_available"
```

---

### Task 7: ExcludeRepo / IncludeRepo Cascade in Projection Path

**Files:**
- Modify: `inspectah-refine/src/session.rs`
- Create: `inspectah-refine/tests/helpers/mod.rs` (shared test fixture)
- Test: `inspectah-refine/tests/session_test.rs`

This is the load-bearing cascade task. All repo ops live in `project_snapshot()`, and all tests assert via `view()` or `snapshot_projected()`.

- [ ] **Step 1: Create shared test fixture module**

Create `inspectah-refine/tests/helpers/mod.rs` with `make_snap_with_repos()` and `make_snap_with_multi_section_third_party()` helpers. Register it from test files via `mod helpers;`.

```rust
// inspectah-refine/tests/helpers/mod.rs
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::rpm::{PackageEntry, PackageState, RepoFile, RpmSection};

pub fn make_snap_with_repos() -> InspectionSnapshot {
    // Same fixture from Task 4 tests — baseos/appstream in centos.repo, epel in epel.repo
    // (duplicate the full fixture here — Rust integration tests don't share across files
    // without an explicit mod)
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry { name: "httpd".into(), arch: "x86_64".into(),
                state: PackageState::Added, source_repo: "appstream".into(),
                include: true, ..Default::default() },
            PackageEntry { name: "epel-release".into(), arch: "noarch".into(),
                state: PackageState::Added, source_repo: "epel".into(),
                include: true, ..Default::default() },
        ],
        repo_files: vec![
            RepoFile { path: "/etc/yum.repos.d/centos.repo".into(),
                content: "[baseos]\nname=CentOS BaseOS\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-centosofficial\n\n[appstream]\nname=CentOS AppStream\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-centosofficial\n".into(),
                include: true, ..Default::default() },
            RepoFile { path: "/etc/yum.repos.d/epel.repo".into(),
                content: "[epel]\nname=EPEL 9\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9\n".into(),
                include: true, ..Default::default() },
        ],
        gpg_keys: vec![
            RepoFile { path: "/etc/pki/rpm-gpg/RPM-GPG-KEY-centosofficial".into(),
                content: "key-data".into(), include: true, ..Default::default() },
            RepoFile { path: "/etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9".into(),
                content: "key-data".into(), include: true, ..Default::default() },
        ],
        baseline_package_names: Some(vec![]),
        ..Default::default()
    });
    snap
}

pub fn make_snap_with_multi_section_third_party() -> InspectionSnapshot {
    let mut snap = make_snap_with_repos();
    let rpm = snap.rpm.as_mut().unwrap();
    rpm.repo_files.push(RepoFile {
        path: "/etc/yum.repos.d/custom-multi.repo".into(),
        content: "[custom-a]\nname=Custom A\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-custom\n\n[custom-b]\nname=Custom B\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-custom\n".into(),
        include: true, ..Default::default(),
    });
    rpm.gpg_keys.push(RepoFile {
        path: "/etc/pki/rpm-gpg/RPM-GPG-KEY-custom".into(),
        content: "key-data".into(), include: true, ..Default::default(),
    });
    rpm.packages_added.push(PackageEntry {
        name: "pkg-a".into(), arch: "x86_64".into(),
        state: PackageState::Added, source_repo: "custom-a".into(),
        include: true, ..Default::default(),
    });
    rpm.packages_added.push(PackageEntry {
        name: "pkg-b".into(), arch: "x86_64".into(),
        state: PackageState::Added, source_repo: "custom-b".into(),
        include: true, ..Default::default(),
    });
    snap
}
```

- [ ] **Step 2: Write failing tests for ExcludeRepo (assert via view/projected state)**

In `inspectah-refine/tests/session_test.rs`:

```rust
mod helpers;
use helpers::*;

#[test]
fn test_exclude_repo_cascades_packages_in_view() {
    let snap = make_snap_with_repos();
    let mut session = RefineSession::new(snap);
    session.apply(RefinementOp::ExcludeRepo { section_id: "epel".into() }).unwrap();
    let epel_pkg = session.view().packages.iter()
        .find(|p| p.entry.name == "epel-release").unwrap();
    assert!(!epel_pkg.entry.include, "epel package must be excluded in view");
}

#[test]
fn test_exclude_repo_cascades_in_projected_snapshot() {
    let snap = make_snap_with_repos();
    let mut session = RefineSession::new(snap);
    session.apply(RefinementOp::ExcludeRepo { section_id: "epel".into() }).unwrap();
    let projected = session.snapshot_projected();
    let epel_pkg = projected.rpm.as_ref().unwrap().packages_added.iter()
        .find(|p| p.name == "epel-release").unwrap();
    assert!(!epel_pkg.include, "epel package must be excluded in projected snapshot");
    // Original snapshot is unchanged
    let orig_pkg = session.snapshot().rpm.as_ref().unwrap().packages_added.iter()
        .find(|p| p.name == "epel-release").unwrap();
    assert!(orig_pkg.include, "original snapshot must be unchanged");
}

#[test]
fn test_exclude_repo_rejects_distro_repo() {
    let snap = make_snap_with_repos();
    let mut session = RefineSession::new(snap);
    let result = session.apply(RefinementOp::ExcludeRepo { section_id: "baseos".into() });
    assert!(result.is_err());
}

#[test]
fn test_exclude_repo_rejects_incomplete_provenance() {
    let mut snap = make_snap_with_repos();
    snap.rpm.as_mut().unwrap().packages_added.push(PackageEntry {
        name: "custom".into(), arch: "x86_64".into(),
        state: PackageState::Added, source_repo: "no-repo-file".into(),
        include: true, ..Default::default(),
    });
    let mut session = RefineSession::new(snap);
    let result = session.apply(RefinementOp::ExcludeRepo { section_id: "no-repo-file".into() });
    assert!(result.is_err());
}

#[test]
fn test_exclude_repo_is_dirty_with_repo_tracking() {
    let snap = make_snap_with_repos();
    let mut session = RefineSession::new(snap);
    assert!(!session.is_dirty());
    session.apply(RefinementOp::ExcludeRepo { section_id: "epel".into() }).unwrap();
    assert!(session.is_dirty());
    let changes = session.pending_changes();
    assert!(changes.repos_excluded.contains(&"epel".to_string()));
}

#[test]
fn test_shared_repo_file_retained_until_last_section() {
    let snap = make_snap_with_multi_section_third_party();
    let mut session = RefineSession::new(snap);

    session.apply(RefinementOp::ExcludeRepo { section_id: "custom-a".into() }).unwrap();
    let projected = session.snapshot_projected();
    let repo_file = projected.rpm.as_ref().unwrap().repo_files.iter()
        .find(|rf| rf.path.contains("custom-multi")).unwrap();
    assert!(repo_file.include, "shared repo file must stay while custom-b is enabled");
    let gpg = projected.rpm.as_ref().unwrap().gpg_keys.iter()
        .find(|k| k.path.contains("RPM-GPG-KEY-custom")).unwrap();
    assert!(gpg.include, "shared GPG key must stay while custom-b is enabled");

    session.apply(RefinementOp::ExcludeRepo { section_id: "custom-b".into() }).unwrap();
    let projected2 = session.snapshot_projected();
    let gpg2 = projected2.rpm.as_ref().unwrap().gpg_keys.iter()
        .find(|k| k.path.contains("RPM-GPG-KEY-custom")).unwrap();
    assert!(!gpg2.include, "GPG key excluded once all sections excluded");
}

#[test]
fn test_exclude_repo_then_per_package_then_include_repo() {
    let snap = make_snap_with_repos();
    let mut session = RefineSession::new(snap);
    // 1. Exclude epel
    session.apply(RefinementOp::ExcludeRepo { section_id: "epel".into() }).unwrap();
    assert!(!session.view().packages.iter()
        .find(|p| p.entry.name == "epel-release").unwrap().entry.include);

    // 2. Re-include the specific package manually
    session.apply(RefinementOp::IncludePackage(PackageTarget {
        name: "epel-release".into(), arch: "noarch".into(),
    })).unwrap();
    assert!(session.view().packages.iter()
        .find(|p| p.entry.name == "epel-release").unwrap().entry.include);

    // 3. Include repo again — this sets include=true on ALL packages
    //    The per-package override is now in the op stack history.
    //    IncludeRepo re-enables everything; the per-package op was already
    //    applied and is still in the stack but gets overridden.
    session.apply(RefinementOp::IncludeRepo { section_id: "epel".into() }).unwrap();
    assert!(session.view().packages.iter()
        .find(|p| p.entry.name == "epel-release").unwrap().entry.include);

    // 4. Undo the IncludeRepo — should restore per-package state
    session.undo().unwrap();
    assert!(session.view().packages.iter()
        .find(|p| p.entry.name == "epel-release").unwrap().entry.include,
        "per-package include is still active after undoing repo include");
}

#[test]
fn test_exclude_repo_undo_redo() {
    let snap = make_snap_with_repos();
    let mut session = RefineSession::new(snap);
    session.apply(RefinementOp::ExcludeRepo { section_id: "epel".into() }).unwrap();
    assert!(!session.view().packages.iter()
        .find(|p| p.entry.name == "epel-release").unwrap().entry.include);
    session.undo().unwrap();
    assert!(session.view().packages.iter()
        .find(|p| p.entry.name == "epel-release").unwrap().entry.include);
    session.redo().unwrap();
    assert!(!session.view().packages.iter()
        .find(|p| p.entry.name == "epel-release").unwrap().entry.include);
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p inspectah-refine test_exclude_repo test_shared_repo`
Expected: FAIL.

- [ ] **Step 4: Implement repo cascade in `project_snapshot()`**

In `session.rs`, modify `project_snapshot()` to handle `ExcludeRepo` / `IncludeRepo` ops. The cascade logic:
- For `ExcludeRepo { section_id }`: set `include = false` on all packages where `source_repo == section_id`. For repo files: check if any other ENABLED section still uses this file (via `repo_index.repo_file_by_section`); if not, set `include = false`. For GPG keys: check `sections_by_gpg_key` ref count — only flip when ALL referencing sections are excluded.
- For `IncludeRepo { section_id }`: reverse — set `include = true` on all matching packages, re-enable repo files and GPG keys.

Add `validate_target()` guard for repo ops: reject if `RepoIndex::is_distro_repo(section_id)` or if `repo_index.provenance(section_id) != Verified`.

Add `is_op_noop()` for repo ops.

Extend `pending_changes()` to track `repos_excluded`.

- [ ] **Step 5: Run all repo cascade tests**

Run: `cargo test -p inspectah-refine test_exclude_repo test_shared_repo`
Expected: All PASS.

- [ ] **Step 6: Run full suite**

Run: `cargo test -p inspectah-refine`
Expected: All PASS.

- [ ] **Step 7: Commit**

```bash
git add inspectah-refine/src/session.rs inspectah-refine/tests/session_test.rs inspectah-refine/tests/helpers/mod.rs
git commit -m "feat(refine): ExcludeRepo/IncludeRepo cascade in projection path with ref counting"
```

---

### Task 8: Non-Leaf Tier 2 View Filtering

**Files:**
- Modify: `inspectah-refine/src/session.rs` (in `recompute_view()`)
- Test: `inspectah-refine/tests/session_test.rs`

Normalization flips `include = false` on non-leaf Tier 2 packages, but the view still returns them. This task filters them out of the `RefinedView.packages` list so the triage count actually drops.

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn test_non_leaf_tier2_excluded_from_view() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry { name: "httpd".into(), arch: "x86_64".into(),
                state: PackageState::Added, source_repo: "appstream".into(),
                include: true, ..Default::default() },
            PackageEntry { name: "apr".into(), arch: "x86_64".into(),
                state: PackageState::Added, source_repo: "appstream".into(),
                include: true, ..Default::default() },
        ],
        baseline_package_names: Some(vec![]),
        leaf_packages: Some(vec!["httpd".into()]),
        ..Default::default()
    });
    let session = RefineSession::new(snap);
    let view = session.view();
    // Only leaf package should appear in the view
    assert!(view.packages.iter().any(|p| p.entry.name == "httpd"));
    assert!(!view.packages.iter().any(|p| p.entry.name == "apr"),
        "non-leaf Tier 2 package must be filtered from view");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p inspectah-refine test_non_leaf_tier2`
Expected: FAIL — apr still appears.

- [ ] **Step 3: Filter in `recompute_view()`**

In the `recompute_view()` method, after computing attention, filter out packages where `include == false` AND the package is Tier 2 (non-leaf dependency). These are not operator-excluded items — they're hidden dependencies. Keep Tier 3 `include == false` in the view because those need operator decision.

The filter: only include packages in the view where `entry.include == true` OR where the primary attention level is `NeedsReview` (Tier 3 items that default to exclude but need operator attention).

- [ ] **Step 4: Run test**

Run: `cargo test -p inspectah-refine test_non_leaf_tier2`
Expected: PASS.

- [ ] **Step 5: Verify triage count semantics**

The `needs_review_count` in stats should only count items visible in the view, not hidden dependencies.

- [ ] **Step 6: Commit**

```bash
git add inspectah-refine/src/session.rs inspectah-refine/tests/session_test.rs
git commit -m "feat(refine): filter non-leaf Tier 2 from triage view, update needs_review_count"
```

---

### Task 9: Containerfile Renderer Fixes

**Files:**
- Modify: `inspectah-pipeline/src/render/containerfile.rs`
- Test: inline `#[cfg(test)]` in same file

- [ ] **Step 1: Write failing test for GPG batching**

```rust
#[test]
fn test_gpg_standard_dir_single_copy() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        gpg_keys: vec![
            RepoFile { path: "/etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9".into(),
                content: "key1".into(), include: true, ..Default::default() },
            RepoFile { path: "/etc/pki/rpm-gpg/RPM-GPG-KEY-centosofficial".into(),
                content: "key2".into(), include: true, ..Default::default() },
        ],
        ..Default::default()
    });
    let output = render_containerfile(&snap, None);
    let copy_lines: Vec<_> = output.lines()
        .filter(|l| l.contains("COPY") && l.contains("rpm-gpg")).collect();
    assert_eq!(copy_lines.len(), 1, "standard dir keys should be single COPY");
    assert!(!output.contains("rpm --import"), "no explicit imports for standard dir");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p inspectah-pipeline test_gpg_standard`
Expected: FAIL.

- [ ] **Step 3: Implement GPG batching**

When all included GPG keys share `/etc/pki/rpm-gpg/`, emit single `COPY config/etc/pki/rpm-gpg/ /etc/pki/rpm-gpg/` with no `rpm --import`. For keys outside that directory, keep per-key pattern.

- [ ] **Step 4: Write failing test for service continuation**

```rust
#[test]
fn test_service_backslash_continuation_over_3() {
    let mut snap = InspectionSnapshot::new();
    snap.services = Some(inspectah_core::types::services::ServiceSection {
        enabled_units: vec!["httpd.service".into(), "sshd.service".into(),
            "chronyd.service".into(), "firewalld.service".into()],
        ..Default::default()
    });
    let output = render_containerfile(&snap, None);
    assert!(output.contains("systemctl enable \\"), "4+ services should use continuation");
}

#[test]
fn test_service_single_line_under_4() {
    let mut snap = InspectionSnapshot::new();
    snap.services = Some(inspectah_core::types::services::ServiceSection {
        enabled_units: vec!["httpd.service".into(), "sshd.service".into()],
        ..Default::default()
    });
    let output = render_containerfile(&snap, None);
    assert!(output.contains("systemctl enable httpd.service sshd.service"));
}
```

- [ ] **Step 5: Run tests to verify they fail**

Run: `cargo test -p inspectah-pipeline test_service_backslash test_service_single`
Expected: FAIL.

- [ ] **Step 6: Implement service continuation**

When `safe_enabled.len() > 3`, format with backslash continuation. Same for `safe_disabled`.

- [ ] **Step 7: Run all pipeline tests**

Run: `cargo test -p inspectah-pipeline`
Expected: All PASS (update golden files as needed).

- [ ] **Step 8: Commit**

```bash
git add inspectah-pipeline/src/render/containerfile.rs
git commit -m "feat(pipeline): GPG key batching for standard dir, service backslash continuation"
```

---

### Task 10: source_repo Investigation and Fix

**Files:**
- Investigate: Go source `cmd/inspectah/internal/inspectors/rpm.go`
- Investigate: Rust source `inspectah-collect/src/inspectors/rpm/mod.rs`
- Investigate: CentOS Stream 9 scan tarball

**HARD GATE:** This task must produce a passing test proving `source_repo` is populated on a real CentOS Stream 9 tarball before Tang proceeds to Task 11 (API contract) or Kit starts repo grouping work.

- [ ] **Step 1: Check Go scanner**

Read `cmd/inspectah/internal/inspectors/rpm.go`, search for `populateSourceRepos` or `source_repo`. Document how Go determines the field.

- [ ] **Step 2: Check actual tarball**

Examine `snapshot.json` from the CentOS Stream 9 tarball. Are `source_repo` values populated or empty?

- [ ] **Step 3: Check Rust RPM inspector**

Read `inspectah-collect/src/inspectors/rpm/mod.rs` for `source_repo` population.

- [ ] **Step 4: Implement fix**

Based on root cause (serde mismatch, missing Rust logic, or stale tarball).

- [ ] **Step 5: Write proof test**

```rust
#[test]
fn test_source_repo_populated_from_real_tarball() {
    // Deserialize the CentOS Stream 9 snapshot
    let snap: InspectionSnapshot = load_test_tarball("testdata/centos-stream-9.tar.gz");
    let rpm = snap.rpm.as_ref().unwrap();
    let packages_with_repo: Vec<_> = rpm.packages_added.iter()
        .filter(|p| !p.source_repo.is_empty()).collect();
    assert!(packages_with_repo.len() > 0, "at least some packages must have source_repo");
    let known_repo = packages_with_repo.iter()
        .any(|p| ["baseos", "appstream", "epel"].contains(&p.source_repo.as_str()));
    assert!(known_repo, "at least one package must have a recognized repo name");
}
```

- [ ] **Step 6: Run proof test**

Run: `cargo test -p inspectah-refine test_source_repo_populated`
Expected: PASS — this is the hard gate.

- [ ] **Step 7: Commit**

```bash
git commit -m "fix(collect): populate source_repo for packages from RPM metadata"
```

---

### Task 11: API Contract for Kit — RefinedView + RepoGroups + TS Mirror

**Files:**
- Modify: `inspectah-web/src/handlers.rs`
- Modify: `inspectah-web/ui/src/api/types.ts`
- Modify: `inspectah-web/ui/src/components/attentionUtils.ts`
- Test: inline tests in `handlers.rs`

This task pins the exact browser-facing contract that Kit codes against. Tang owns the TS mirror updates.

- [ ] **Step 1: Define `RepoGroupInfo` in Rust**

In `inspectah-web/src/handlers.rs`:

```rust
#[derive(Serialize, Clone, Debug)]
pub struct RepoGroupInfo {
    pub section_id: String,
    pub provenance: inspectah_refine::types::RepoProvenance,
    pub is_distro: bool,
    pub package_count: usize,
    pub enabled: bool,
}
```

`enabled` is `true` when no `ExcludeRepo` for this section is active in the projected state. Kit uses this to render toggle state.

- [ ] **Step 2: Extend health endpoint with `policy`**

```rust
use inspectah_refine::repo_index::DISTRO_REPOS;

// In health handler:
"policy": {
    "distro_repos": DISTRO_REPOS,
}
```

- [ ] **Step 3: Extend view response with `repo_groups` and `baseline_available`**

The existing view endpoint returns `session.view()` which is a `RefinedView`. Extend the handler to wrap it:

```rust
#[derive(Serialize)]
pub struct ViewResponse {
    #[serde(flatten)]
    pub view: RefinedView,
    pub repo_groups: Vec<RepoGroupInfo>,
}
```

Build `repo_groups` from `session.repo_index()` plus projected state to determine `enabled`. Include an entry for empty `source_repo` packages with `provenance: Unknown`, `is_distro: false`, `enabled: true` (no toggle).

- [ ] **Step 4: Write handler test**

```rust
#[test]
fn test_view_response_includes_repo_groups() {
    // Build session with known repos, call view handler, verify repo_groups
    // has entries for appstream (distro, verified, no toggle) and epel (third-party, verified, toggle)
}

#[test]
fn test_health_includes_policy_distro_repos() {
    // Verify policy.distro_repos contains the expected list
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p inspectah-web`
Expected: PASS.

- [ ] **Step 6: Update TypeScript mirror**

In `inspectah-web/ui/src/api/types.ts`, add:

```typescript
// --- New Phase 5 types ---

export type RepoProvenance = "verified" | "incomplete" | "unknown";

export interface RepoGroupInfo {
  section_id: string;
  provenance: RepoProvenance;
  is_distro: boolean;
  package_count: number;
  enabled: boolean;
}

// Update AttentionReason to include new variants
export type AttentionReason =
  | "package_baseline_match"
  | "package_user_added"
  | "package_version_changed"
  | "package_provenance_unavailable"
  | "package_local_install"
  | "package_no_repo_source"
  | "config_default"
  | "config_baseline_match"
  | "config_modified"
  | "config_unowned"
  | "config_orphaned"
  | "sensitive_path"
  | { custom: string };

// Update RefineStats
export interface RefineStats {
  total_packages: number;
  included_packages: number;
  excluded_packages: number;
  total_configs: number;
  included_configs: number;
  package_managed_configs: number;
  excluded_configs: number;
  needs_review_count: number;
  ops_applied: number;
  can_undo: boolean;
  can_redo: boolean;
  baseline_available: boolean;
}

// Update HealthResponse
export interface HealthResponse {
  status: string;
  host: { hostname: string; os_name: string; os_version: string; os_id: string; system_type: string; schema_version: number; };
  completeness: string;
  policy: { distro_repos: string[] };
}

// View response now includes repo_groups
export interface ViewResponse extends RefinedView {
  repo_groups: RepoGroupInfo[];
}
```

- [ ] **Step 7: Update `attentionUtils.ts`**

In `inspectah-web/ui/src/components/attentionUtils.ts`, update `formatReasonText()` and `attentionLabelColor()` to handle new reason values. Key additions:

```typescript
export function formatReasonText(reason: AttentionReason): string {
  if (typeof reason === "object" && "custom" in reason) return reason.custom;
  const map: Record<string, string> = {
    package_baseline_match: "Baseline",
    package_user_added: "User Added",
    package_version_changed: "Version Changed",
    package_provenance_unavailable: "Baseline Unavailable",
    package_local_install: "Local Install",
    package_no_repo_source: "No Repo Source",
    config_default: "Package Default",
    config_baseline_match: "Baseline Match",
    config_modified: "Modified",
    config_unowned: "Unowned",
    config_orphaned: "Orphaned",
    sensitive_path: "Sensitive Path",
  };
  return map[reason] ?? reason.split("_").map(w => w.charAt(0).toUpperCase() + w.slice(1)).join(" ");
}
```

- [ ] **Step 8: Run UI tests to verify TS compiles and existing tests pass**

Run: `cd inspectah-web/ui && npm test`
Expected: PASS (some tests may need attention reason updates in fixtures).

- [ ] **Step 9: Commit**

```bash
git add inspectah-web/src/handlers.rs inspectah-web/ui/src/api/types.ts inspectah-web/ui/src/components/attentionUtils.ts
git commit -m "feat(web): pin ViewResponse+RepoGroupInfo contract, update TS mirror and attentionUtils"
```

---

### Task 12: Add ExcludeRepo / IncludeRepo API Endpoint

**Files:**
- Modify: `inspectah-web/src/handlers.rs`
- Modify: `inspectah-web/ui/src/api/client.ts`

- [ ] **Step 1: Add handler for repo ops**

The existing `/api/snapshot/apply` handler dispatches `RefinementOp`. Since `ExcludeRepo` / `IncludeRepo` are now variants of `RefinementOp`, verify the handler already handles them through the existing `session.apply(op)` path. If the handler deserializes the op correctly (it should via serde `tag`/`content`), no new endpoint is needed — the existing apply handler works.

- [ ] **Step 2: Write test proving repo ops work through the apply endpoint**

```rust
#[test]
fn test_apply_exclude_repo_via_handler() {
    // POST {"op": "ExcludeRepo", "target": {"section_id": "epel"}} to /api/snapshot/apply
    // Verify response includes updated view with repo excluded
}
```

- [ ] **Step 3: Run test**

Run: `cargo test -p inspectah-web test_apply_exclude_repo`
Expected: PASS.

- [ ] **Step 4: Update client.ts**

Add typed helper in `client.ts`:

```typescript
export async function excludeRepo(sectionId: string, generation: number): Promise<ViewResponse> {
  return applyOp({ op: "ExcludeRepo", target: { section_id: sectionId } }, generation);
}

export async function includeRepo(sectionId: string, generation: number): Promise<ViewResponse> {
  return applyOp({ op: "IncludeRepo", target: { section_id: sectionId } }, generation);
}
```

- [ ] **Step 5: Commit**

```bash
git add inspectah-web/src/handlers.rs inspectah-web/ui/src/api/client.ts
git commit -m "feat(web): wire ExcludeRepo/IncludeRepo through apply endpoint and TS client"
```

---

## Kit Tasks (Web UI)

### Task 13: Layout Fixes (Independent — Ship Now)

**Files:**
- Modify: `inspectah-web/ui/src/App.css`
- Modify: `inspectah-web/ui/src/components/MainContent.tsx` (for `PageSection` padding)
- Modify: `inspectah-web/ui/src/components/Sidebar.tsx`
- Modify: `inspectah-web/ui/src/components/ContainerfilePanel.tsx`
- Test: `inspectah-web/ui/src/components/__tests__/Sidebar.test.tsx`
- Test: `inspectah-web/ui/src/components/__tests__/ContainerfilePanel.test.tsx`
- Test: `inspectah-web/ui/src/components/__tests__/ResponsiveLayout.test.tsx`

No pipeline dependency. Can ship in parallel with Tang's work.

- [ ] **Step 1: Write failing test for hostname position**

In `Sidebar.test.tsx`, add a test verifying the hostname element appears before nav items in the DOM:

```typescript
it("renders hostname above nav groups", () => {
  render(<Sidebar {...defaultProps} />);
  const host = screen.getByText(defaultProps.health.host.hostname);
  const firstNav = screen.getByRole("navigation");
  // hostname should precede nav in DOM order
  expect(host.compareDocumentPosition(firstNav) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd inspectah-web/ui && npx vitest run --reporter=verbose -- Sidebar`
Expected: FAIL — hostname currently below nav.

- [ ] **Step 3: Move hostname to top of sidebar**

In `Sidebar.tsx`, move the `inspectah-sidebar__host` block above the `<Nav>` element. Bold hostname, OS below. Bottom border separating from nav.

- [ ] **Step 4: Run test to verify it passes**

Run: `cd inspectah-web/ui && npx vitest run --reporter=verbose -- Sidebar`
Expected: PASS.

- [ ] **Step 5: Full-width layout**

In `App.css`, strip PF Page padding. In `MainContent.tsx`, verify `PageSection` doesn't add extra padding.

- [ ] **Step 6: Nav spacing fix**

In `App.css`, remove `flex: 1` from sidebar nav.

- [ ] **Step 7: Panel collapse icon direction**

In `ContainerfilePanel.tsx`, flip icon. Write test in `ContainerfilePanel.test.tsx` verifying icon direction matches panel state.

- [ ] **Step 8: Run all affected tests**

Run: `cd inspectah-web/ui && npx vitest run --reporter=verbose -- Sidebar ContainerfilePanel ResponsiveLayout`
Expected: All PASS.

- [ ] **Step 9: Visual verification**

Run: `cd inspectah-web/ui && npm run dev`
Open browser, verify: full-width, top-aligned nav, hostname at top, correct collapse icon.

- [ ] **Step 10: Commit**

```bash
git add inspectah-web/ui/src/App.css inspectah-web/ui/src/components/Sidebar.tsx inspectah-web/ui/src/components/ContainerfilePanel.tsx inspectah-web/ui/src/components/MainContent.tsx
git commit -m "fix(web): full-width layout, nav spacing, hostname to top, panel collapse icon"
```

---

### Task 14: Tier-Aware Card Treatment

**Files:**
- Modify: `inspectah-web/ui/src/components/AttentionGroup.tsx`
- Modify: `inspectah-web/ui/src/components/DecisionItem.tsx`
- Modify: `inspectah-web/ui/src/components/DecisionList.tsx`
- Modify: `inspectah-web/ui/src/components/PackageDetail.tsx`
- Modify: `inspectah-web/ui/src/App.css`
- Test: `inspectah-web/ui/src/components/__tests__/DecisionSections.test.tsx`

Blocked on Tang Tasks 1-11.

- [ ] **Step 1: Write failing test for Tier 1 collapsed summary**

In `DecisionSections.test.tsx`:

```typescript
it("renders Tier 1 packages as collapsed summary", () => {
  const view = makeView({
    packages: [
      makePkg({ source_repo: "baseos", attention: [{ level: "routine", reason: "package_baseline_match", detail: null }] }),
    ],
  });
  render(<MainContent {...defaultProps} view={view} />);
  expect(screen.getByText(/baseline packages/i)).toBeInTheDocument();
  expect(screen.queryByText("glibc")).not.toBeInTheDocument(); // collapsed
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd inspectah-web/ui && npx vitest run --reporter=verbose -- DecisionSections`
Expected: FAIL.

- [ ] **Step 3: Implement Tier 1 collapsed summary**

In `AttentionGroup.tsx` (or `DecisionList.tsx`), detect Routine-level items and render as collapsed summary: "N baseline packages (auto-included)" with expand toggle. When expanded, compact list (name only, muted text).

- [ ] **Step 4: Write failing test for Tier 2 badge text**

```typescript
it("shows repo source badge for verified Tier 2", () => {
  const view = makeView({
    packages: [
      makePkg({ source_repo: "appstream", attention: [{ level: "informational", reason: "package_user_added", detail: null }] }),
    ],
  });
  render(<MainContent {...defaultProps} view={view} />);
  expect(screen.getByText("appstream")).toBeInTheDocument();
});

it("shows 'Baseline Unavailable' for provenance-unavailable Tier 2", () => {
  const view = makeView({
    packages: [
      makePkg({ attention: [{ level: "informational", reason: "package_provenance_unavailable", detail: null }] }),
    ],
  });
  render(<MainContent {...defaultProps} view={view} />);
  expect(screen.getByText(/baseline unavailable/i)).toBeInTheDocument();
});
```

- [ ] **Step 5: Run tests to verify they fail**

Run: `cd inspectah-web/ui && npx vitest run --reporter=verbose -- DecisionSections`
Expected: FAIL.

- [ ] **Step 6: Implement badge text in PackageDetail.tsx**

Update `PackageDetail.tsx` to show `source_repo` as badge when reason is `package_user_added`, or "Baseline Unavailable" when `package_provenance_unavailable`.

- [ ] **Step 7: Implement provenance completeness banner**

When `stats.baseline_available === false`, show banner at top of Packages section.

- [ ] **Step 8: Run all tests**

Run: `cd inspectah-web/ui && npx vitest run --reporter=verbose -- DecisionSections`
Expected: All PASS.

- [ ] **Step 9: Commit**

```bash
git commit -m "feat(web): tier-aware cards with collapsed Tier 1, provenance badges, baseline banner"
```

---

### Task 15: Repo Group Headers and Bulk Toggle

**Files:**
- Create: `inspectah-web/ui/src/components/RepoGroupHeader.tsx`
- Modify: `inspectah-web/ui/src/components/DecisionList.tsx`
- Modify: `inspectah-web/ui/src/hooks/useMutation.ts`
- Test: `inspectah-web/ui/src/components/__tests__/DecisionSections.test.tsx`

- [ ] **Step 1: Write failing test for repo group rendering**

```typescript
it("groups Tier 2 packages by repo with header", () => {
  const view = makeView({
    packages: [
      makePkg({ name: "httpd", source_repo: "appstream", attention: [{ level: "informational", reason: "package_user_added", detail: null }] }),
      makePkg({ name: "epel-release", source_repo: "epel", attention: [{ level: "informational", reason: "package_user_added", detail: null }] }),
    ],
    repo_groups: [
      { section_id: "appstream", provenance: "verified", is_distro: true, package_count: 1, enabled: true },
      { section_id: "epel", provenance: "verified", is_distro: false, package_count: 1, enabled: true },
    ],
  });
  render(<MainContent {...defaultProps} view={view} />);
  expect(screen.getByText(/appstream/)).toBeInTheDocument();
  expect(screen.getByText(/Distro/)).toBeInTheDocument();
  expect(screen.getByText(/Third-party/)).toBeInTheDocument();
});

it("shows toggle for verified third-party, no toggle for distro", () => {
  // Same fixture as above
  const distroGroup = screen.getByText(/appstream/).closest("[data-repo-group]");
  expect(within(distroGroup).queryByRole("switch")).not.toBeInTheDocument();
  const thirdPartyGroup = screen.getByText(/epel/).closest("[data-repo-group]");
  expect(within(thirdPartyGroup).getByRole("switch")).toBeInTheDocument();
});

it("does not show toggle for unverified provenance", () => {
  const view = makeView({
    repo_groups: [
      { section_id: "mystery", provenance: "incomplete", is_distro: false, package_count: 2, enabled: true },
    ],
  });
  render(<MainContent {...defaultProps} view={view} />);
  expect(screen.getByText(/Unverified/)).toBeInTheDocument();
  expect(screen.queryByRole("switch")).not.toBeInTheDocument();
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd inspectah-web/ui && npx vitest run --reporter=verbose -- DecisionSections`
Expected: FAIL.

- [ ] **Step 3: Create `RepoGroupHeader.tsx`**

Renders: repo label, badge ("Distro"/"Third-party"/"Unverified"/"Unknown"), package count, conditional toggle. Toggle only for third-party + `Verified`. Badges abbreviate to "D"/"3P" at <768px with `aria-label`.

- [ ] **Step 4: Wire toggle to ExcludeRepo/IncludeRepo**

In `DecisionList.tsx` / `useMutation.ts`, wire toggle to call `excludeRepo()` / `includeRepo()` from the client. Optimistic UI: flip immediately, show undo toast via `role="status"` on success, revert + `role="alert"` error banner on failure.

- [ ] **Step 5: Run tests**

Run: `cd inspectah-web/ui && npx vitest run --reporter=verbose -- DecisionSections`
Expected: All PASS.

- [ ] **Step 6: Commit**

```bash
git commit -m "feat(web): repo group headers with bulk toggle, distro/third-party badges"
```

---

### Task 16: Config Kind Grouping

**Files:**
- Modify: `inspectah-web/ui/src/components/AttentionGroup.tsx` or `DecisionList.tsx`
- Modify: `inspectah-web/ui/src/components/ConfigDetail.tsx`
- Test: `inspectah-web/ui/src/components/__tests__/DecisionSections.test.tsx`

- [ ] **Step 1: Write failing test for config Tier 1 summary**

```typescript
it("renders Tier 1 configs as 'managed by packages (not copied)' summary", () => {
  const view = makeView({
    config_files: [
      makeConfig({ path: "/etc/default.conf", kind: "rpm_owned_default",
        attention: [{ level: "routine", reason: "config_default", detail: null }] }),
    ],
  });
  render(<MainContent {...defaultProps} view={view} />);
  expect(screen.getByText(/managed by packages/i)).toBeInTheDocument();
  expect(screen.queryByText("/etc/default.conf")).not.toBeInTheDocument();
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd inspectah-web/ui && npx vitest run --reporter=verbose -- DecisionSections`
Expected: FAIL.

- [ ] **Step 3: Implement config kind grouping**

Tier 1 collapsed: "N configs managed by packages (not copied)". Tier 2 shown as cards. Tier 3 shown with attention badge and "View diff" link when `diff_against_rpm` is available.

- [ ] **Step 4: Write test for diff indicator**

```typescript
it("shows View diff link when diff_against_rpm is available", () => {
  const view = makeView({
    config_files: [
      makeConfig({ path: "/etc/ssh/sshd_config", kind: "rpm_owned_modified",
        diff_against_rpm: "--- a\n+++ b\n@@ -1 +1 @@\n-old\n+new",
        attention: [{ level: "needs_review", reason: "config_modified", detail: null }] }),
    ],
  });
  render(<MainContent {...defaultProps} view={view} />);
  expect(screen.getByText(/view diff/i)).toBeInTheDocument();
});
```

- [ ] **Step 5: Run tests**

Run: `cd inspectah-web/ui && npx vitest run --reporter=verbose -- DecisionSections`
Expected: All PASS.

- [ ] **Step 6: Commit**

```bash
git commit -m "feat(web): config kind grouping with Tier 1 collapsed, diff indicator"
```

---

### Task 17: Search Auto-Reveal for Collapsed Groups

**Files:**
- Modify: `inspectah-web/ui/src/App.tsx` (search/focus orchestration lives here)
- Modify: `inspectah-web/ui/src/components/GlobalSearch.tsx`
- Test: `inspectah-web/ui/src/components/__tests__/GlobalSearch.test.tsx`

- [ ] **Step 1: Write failing test**

```typescript
it("auto-expands collapsed group when search selects item inside it", () => {
  // Render with Tier 1 group collapsed, search for a Tier 1 package name
  // Verify: group expands, item is visible, has flash highlight class
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd inspectah-web/ui && npx vitest run --reporter=verbose -- GlobalSearch`
Expected: FAIL.

- [ ] **Step 3: Implement auto-reveal**

In `App.tsx` search handler: when search result targets an item inside a collapsed group, expand the group, scroll into view, apply `inspectah-search-highlight` class (2s CSS animation), focus on item's primary action.

- [ ] **Step 4: Run test**

Run: `cd inspectah-web/ui && npx vitest run --reporter=verbose -- GlobalSearch`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git commit -m "feat(web): search auto-reveal for collapsed tier/repo groups"
```

---

### Task 18: Keyboard Traversal and Responsive Behavior

**Files:**
- Modify: `inspectah-web/ui/src/App.tsx` (keyboard handling)
- Modify: `inspectah-web/ui/src/App.css`
- Test: `inspectah-web/ui/src/components/__tests__/FocusAndNavigation.test.tsx`
- Test: `inspectah-web/ui/src/components/__tests__/ResponsiveLayout.test.tsx`

- [ ] **Step 1: Write failing test for group header keyboard stops**

```typescript
it("Tab moves from group header to toggle to first item", () => {
  // Render with repo group, focus header, Tab → toggle, Tab → first item
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd inspectah-web/ui && npx vitest run --reporter=verbose -- FocusAndNavigation`
Expected: FAIL.

- [ ] **Step 3: Implement keyboard traversal**

Group headers as Tab stops. Tab: header → toggle → first item. Arrow/j/k within group.

- [ ] **Step 4: Write failing test for responsive badges**

```typescript
it("abbreviates badges at narrow width with aria-label", () => {
  // Render at 600px, verify "D" visible, aria-label="Distro"
});
```

- [ ] **Step 5: Run test to verify it fails**

Run: `cd inspectah-web/ui && npx vitest run --reporter=verbose -- ResponsiveLayout`
Expected: FAIL.

- [ ] **Step 6: Implement responsive rules**

Badge abbreviation at <768px with `aria-label`. Repo headers stack at <1024px.

- [ ] **Step 7: Run all tests**

Run: `cd inspectah-web/ui && npx vitest run --reporter=verbose -- FocusAndNavigation ResponsiveLayout`
Expected: All PASS.

- [ ] **Step 8: Commit**

```bash
git commit -m "feat(web): keyboard traversal for repo groups, responsive badge abbreviation"
```

---

### Task 19: E2E Smoke Tests

**Files:**
- Modify: `inspectah-web/ui/e2e/triage.spec.ts`
- Test fixtures: `testdata/` (CentOS Stream 9 tarball)

E2E tests require a running `inspectah refine` server with a test tarball. See `inspectah-web/ui/e2e/README.md` for server setup.

- [ ] **Step 1: Write E2E test for triage count reduction**

In `inspectah-web/ui/e2e/triage.spec.ts`:

```typescript
test("Phase 5: triage surface reduced from ~734 to <100", async ({ page }) => {
  // Navigate to refine UI, verify needs_review count < 100
  // Verify Tier 1 section shows "baseline packages" collapsed summary
  // Verify repo groups are visible
});
```

- [ ] **Step 2: Write E2E test for ExcludeRepo flow**

```typescript
test("ExcludeRepo removes packages and shows undo toast", async ({ page }) => {
  // Find third-party repo toggle, click it
  // Verify packages disappear from triage
  // Verify Containerfile preview updates
  // Verify undo toast appears
  // Click undo, verify restoration
});
```

- [ ] **Step 3: Write E2E test for no-toggle on unverified repo**

```typescript
test("unverified repo shows label but no toggle", async ({ page }) => {
  // Verify repo with incomplete provenance has no switch element
});
```

- [ ] **Step 4: Write E2E test for Tier 1 config "not copied"**

```typescript
test("Tier 1 configs show 'managed by packages' and are not in Containerfile", async ({ page }) => {
  // Verify config section shows "managed by packages" summary
  // Open Containerfile panel, verify no COPY directives for default configs
});
```

- [ ] **Step 5: Run E2E tests**

Start server: `cargo run -p inspectah-cli -- refine testdata/centos-stream-9.tar.gz &`
Run: `cd inspectah-web/ui && npx playwright test e2e/triage.spec.ts`
Expected: All PASS.

- [ ] **Step 6: Commit**

```bash
git add inspectah-web/ui/e2e/triage.spec.ts
git commit -m "test(web): E2E for tiered triage, repo exclusion, provenance, config omission"
```

---

## Task Dependency Summary

```
Task 1 (types) ──┬──→ Task 2 (pkg classify) ──┬──→ Task 5 (normalize) ──→ Task 6 (session construction)
                  ├──→ Task 3 (cfg classify) ──┘                           ↓
                  └──→ Task 4 (repo index) ────────────────────────→ Task 7 (ExcludeRepo cascade)
                                                                     ↓
                                                               Task 8 (non-leaf view filter)
                                                                     ↓
                                                               Task 9 (containerfile fixes)
                                                                     ↓
                                                               Task 10 (source_repo ◆ HARD GATE)
                                                                     ↓
                                                               Task 11 (API contract + TS mirror)
                                                                     ↓
                                                               Task 12 (repo op endpoint)

Task 13 (layout CSS) ─── independent, ship anytime

Tasks 14-18 (tier cards, repo groups, config groups, search, keyboard) ── blocked on Tasks 1-12

Task 19 (E2E) ── blocked on all above
```
