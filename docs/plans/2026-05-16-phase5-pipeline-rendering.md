# Phase 5: Pipeline Rendering & Triage Quality — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the flat NeedsReview attention model with baseline-aware three-tier classification, repo grouping with bulk actions, and Containerfile rendering fixes — reducing triage surface from ~734 to ~50-80 items.

**Architecture:** Two-pass classify-then-normalize in `inspectah-refine`, with `RepoIndex` for repo identity/cascade. Normalization materializes at session construction time into authoritative snapshot state. Pipeline fixes in `inspectah-pipeline`. Tiered UI in `inspectah-web`.

**Tech Stack:** Rust (inspectah-core, inspectah-refine, inspectah-pipeline, inspectah-web), React 19 + Vite + PatternFly 6 (web UI), Cargo test + Vitest + Playwright (testing)

**Spec:** `docs/specs/proposed/2026-05-16-phase5-pipeline-rendering-design.md` (approved after 3 review rounds)

**Ownership:**
- **Tang:** Tasks 1-10 (all Rust — types, attention, normalize, repo index, session, containerfile, web handlers)
- **Kit:** Tasks 11-17 (all React/TypeScript — layout, tier cards, repo grouping, config grouping, keyboard, responsive)
- **Integration:** Task 18 (E2E smoke test)

**Dependency chain:** Tasks 1-3 are foundational (types + classify). Task 4 (RepoIndex) depends on 1. Task 5 (normalize) depends on 2-3. Task 6 (ExcludeRepo) depends on 4-5. Tasks 7-10 depend on 1-6. Kit's tasks 11a-d (layout CSS) are independent. Kit's tasks 12-17 are blocked on Tang's pipeline landing.

**Build/test commands:**
```bash
export PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH"
# Rust tests
cargo test -p inspectah-core
cargo test -p inspectah-refine
cargo test -p inspectah-pipeline
cargo test -p inspectah-web
# UI tests
cd inspectah-web/ui && npm test
cd inspectah-web/ui && npx playwright test
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
    // Package reasons
    PackageBaselineMatch,
    PackageUserAdded,
    PackageVersionChanged,
    PackageProvenanceUnavailable,
    PackageLocalInstall,
    PackageNoRepoSource,
    // Config reasons
    ConfigDefault,
    ConfigBaselineMatch,
    ConfigModified,
    ConfigUnowned,
    ConfigOrphaned,
    // Cross-cutting
    SensitivePath,
    Custom(String),
}
```

This replaces `PackageNotInBaseline`, `PackageStateChanged`, `PackageNoRepo`. Old names are removed — the attention module is being rewritten so no migration needed.

- [ ] **Step 5: Add `RepoProvenance` enum**

In `inspectah-refine/src/types.rs`, add:

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

In `inspectah-refine/src/types.rs`, extend the enum:

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

- [ ] **Step 7: Extend `ChangesSummary` for repo tracking**

In `inspectah-refine/src/types.rs`:

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
```

- [ ] **Step 8: Update `RefineStats` for config count semantics**

In `inspectah-refine/src/types.rs`, replace `RefineStats`:

```rust
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
}
```

`package_managed_configs` counts Tier 1 configs with `include = false` from normalization — distinct from operator-excluded configs.

- [ ] **Step 9: Write serde round-trip tests for new variants**

In `inspectah-refine/tests/serde_test.rs`, add:

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

- [ ] **Step 10: Fix compilation errors from renamed reasons**

The attention module (`attention.rs`) uses old reason names. It will be rewritten in Task 2, but it needs to compile now. Temporarily map old names to new ones in `attention.rs` to keep the build green. (Task 2 replaces the entire function body.)

- [ ] **Step 11: Run all tests**

Run: `cargo test -p inspectah-core && cargo test -p inspectah-refine`
Expected: All existing tests pass. New serde tests pass.

- [ ] **Step 12: Commit**

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
    assert_eq!(pkgs.len(), 1);
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
    let snap = make_snap_with_package(
        "httpd", PackageState::Added, "appstream", None,
    );
    let pkgs = compute_package_attention(&snap);
    assert_eq!(pkgs[0].attention[0].level, AttentionLevel::Informational);
    assert_eq!(pkgs[0].attention[0].reason, AttentionReason::PackageProvenanceUnavailable);
}

#[test]
fn test_added_no_baseline_empty_repo_is_tier3() {
    let snap = make_snap_with_package(
        "mystery", PackageState::Added, "", None,
    );
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
        let snap = make_snap_with_package(
            "orphan", PackageState::NoRepo, "", baseline,
        );
        let pkgs = compute_package_attention(&snap);
        assert_eq!(pkgs[0].attention[0].level, AttentionLevel::NeedsReview);
        assert_eq!(pkgs[0].attention[0].reason, AttentionReason::PackageNoRepoSource);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-refine test_added_baseline`
Expected: FAIL — current implementation doesn't check baseline.

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

            // Sensitive path overlay: promote Tier 2 → Tier 3.
            // Tier 1 only promoted if baseline is absent (can't verify
            // the sensitive file is an expected default).
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
    // LocalInstall and NoRepo are always Tier 3, regardless of baseline/repo
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

    // Empty source_repo with no baseline = Tier 3
    let has_repo = !entry.source_repo.is_empty();
    if !has_repo {
        return AttentionTag {
            level: AttentionLevel::NeedsReview,
            reason: AttentionReason::PackageNoRepoSource,
            detail: None,
        };
    }

    // Baseline-aware classification for Added and Modified
    match baseline {
        Some(names) if names.iter().any(|n| n == &entry.name) => {
            // In baseline → Tier 1
            AttentionTag {
                level: AttentionLevel::Routine,
                reason: AttentionReason::PackageBaselineMatch,
                detail: None,
            }
        }
        Some(_) => {
            // Baseline present, not in baseline, repo known → Tier 2
            let reason = match entry.state {
                PackageState::Modified => AttentionReason::PackageVersionChanged,
                _ => AttentionReason::PackageUserAdded,
            };
            AttentionTag {
                level: AttentionLevel::Informational,
                reason,
                detail: None,
            }
        }
        None => {
            // Baseline missing, repo known → Tier 2 with degraded provenance
            AttentionTag {
                level: AttentionLevel::Informational,
                reason: AttentionReason::PackageProvenanceUnavailable,
                detail: None,
            }
        }
    }
}
```

- [ ] **Step 4: Run package classification tests**

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
    // Tier 2 (Unowned) at a sensitive path → promoted to Tier 3
    let snap = make_snap_with_config("/etc/ssh/custom_keys", ConfigFileKind::Unowned);
    let configs = compute_config_attention(&snap);
    assert!(configs[0].attention.iter().any(|t| t.reason == AttentionReason::SensitivePath));

    // Tier 1 (RpmOwnedDefault) at a sensitive path → NOT promoted
    let snap2 = make_snap_with_config("/etc/pki/tls/cert.pem", ConfigFileKind::RpmOwnedDefault);
    let configs2 = compute_config_attention(&snap2);
    assert!(!configs2[0].attention.iter().any(|t| t.reason == AttentionReason::SensitivePath));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-refine test_config_`
Expected: FAIL — old classification.

- [ ] **Step 3: Rewrite `compute_config_attention()`**

Replace the function body in `inspectah-refine/src/attention.rs`:

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

            // Sensitive path overlay: promote Tier 2 → Tier 3.
            // Tier 1 is NOT promoted (base image ships these files).
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

    // Surface unresolved redaction hints as NeedsReview
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

- [ ] **Step 4: Run config classification tests**

Run: `cargo test -p inspectah-refine test_config_`
Expected: All PASS.

- [ ] **Step 5: Run full test suite**

Run: `cargo test -p inspectah-refine`
Expected: All PASS (existing tests may need attention reason updates).

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

Create `inspectah-refine/tests/repo_index_test.rs`:

```rust
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::rpm::{PackageEntry, PackageState, RepoFile, RpmSection};
use inspectah_refine::repo_index::RepoIndex;
use inspectah_refine::types::RepoProvenance;

fn make_snap_with_repos() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "appstream".into(),
                include: true,
                ..Default::default()
            },
            PackageEntry {
                name: "epel-release".into(),
                arch: "noarch".into(),
                state: PackageState::Added,
                source_repo: "epel".into(),
                include: true,
                ..Default::default()
            },
        ],
        repo_files: vec![
            RepoFile {
                path: "/etc/yum.repos.d/centos.repo".into(),
                content: "[baseos]\nname=CentOS Stream 9 - BaseOS\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-centosofficial\n\n[appstream]\nname=CentOS Stream 9 - AppStream\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-centosofficial\n".into(),
                include: true,
                ..Default::default()
            },
            RepoFile {
                path: "/etc/yum.repos.d/epel.repo".into(),
                content: "[epel]\nname=Extra Packages for Enterprise Linux 9\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9\n".into(),
                include: true,
                ..Default::default()
            },
        ],
        gpg_keys: vec![
            RepoFile {
                path: "/etc/pki/rpm-gpg/RPM-GPG-KEY-centosofficial".into(),
                content: "key-data".into(),
                include: true,
                ..Default::default()
            },
            RepoFile {
                path: "/etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9".into(),
                content: "key-data".into(),
                include: true,
                ..Default::default()
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
    // centos.repo has both baseos and appstream sections
    let baseos_files = index.repo_file_by_section.get("baseos").unwrap();
    let appstream_files = index.repo_file_by_section.get("appstream").unwrap();
    assert!(baseos_files.contains(&"/etc/yum.repos.d/centos.repo".to_string()));
    assert!(appstream_files.contains(&"/etc/yum.repos.d/centos.repo".to_string()));
}

#[test]
fn test_repo_index_gpg_shared_key() {
    let snap = make_snap_with_repos();
    let index = RepoIndex::build(&snap);
    // RPM-GPG-KEY-centosofficial is shared by baseos and appstream
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
    // Add a package from a repo with no matching repo file
    snap.rpm.as_mut().unwrap().packages_added.push(PackageEntry {
        name: "custom-pkg".into(),
        arch: "x86_64".into(),
        state: PackageState::Added,
        source_repo: "custom-internal".into(),
        include: true,
        ..Default::default()
    });
    let index = RepoIndex::build(&snap);
    assert_eq!(index.provenance("custom-internal"), RepoProvenance::Incomplete);
}

#[test]
fn test_repo_index_provenance_unknown() {
    let mut snap = make_snap_with_repos();
    snap.rpm.as_mut().unwrap().packages_added.push(PackageEntry {
        name: "mystery".into(),
        arch: "x86_64".into(),
        state: PackageState::Added,
        source_repo: "".into(),
        include: true,
        ..Default::default()
    });
    let index = RepoIndex::build(&snap);
    assert_eq!(index.provenance(""), RepoProvenance::Unknown);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-refine test_repo_index`
Expected: FAIL — module doesn't exist yet.

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

        // 1. Parse repo files for INI sections
        let mut repo_file_by_section: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut gpg_keys_by_section: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut sections_by_gpg_key: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

        for rf in &rpm.repo_files {
            let sections = parse_repo_sections(&rf.content);
            for section in &sections {
                repo_file_by_section
                    .entry(section.id.clone())
                    .or_default()
                    .push(rf.path.clone());
                for key_path in &section.gpg_key_paths {
                    gpg_keys_by_section
                        .entry(section.id.clone())
                        .or_default()
                        .push(key_path.clone());
                    sections_by_gpg_key
                        .entry(key_path.clone())
                        .or_default()
                        .insert(section.id.clone());
                }
            }
        }

        // 2. Map packages by source_repo
        let mut packages_by_repo: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for pkg in &rpm.packages_added {
            if !pkg.source_repo.is_empty() {
                packages_by_repo
                    .entry(pkg.source_repo.clone())
                    .or_default()
                    .push(pkg.name.clone());
            }
        }

        // 3. Compute provenance per section ID
        let mut provenance_map: BTreeMap<String, RepoProvenance> = BTreeMap::new();
        let all_section_ids: BTreeSet<String> = packages_by_repo.keys()
            .chain(repo_file_by_section.keys())
            .cloned()
            .collect();

        for sid in &all_section_ids {
            if sid.is_empty() {
                provenance_map.insert(sid.clone(), RepoProvenance::Unknown);
                continue;
            }
            let has_repo_file = repo_file_by_section.contains_key(sid);
            let has_gpg = gpg_keys_by_section.contains_key(sid);
            let prov = if has_repo_file && has_gpg {
                RepoProvenance::Verified
            } else if has_repo_file {
                // Repo file exists but no gpgkey directive — still Verified
                // (some repos don't use GPG)
                RepoProvenance::Verified
            } else {
                RepoProvenance::Incomplete
            };
            provenance_map.insert(sid.clone(), prov);
        }

        Self {
            packages_by_repo,
            repo_file_by_section,
            gpg_keys_by_section,
            sections_by_gpg_key,
            provenance_map,
        }
    }

    pub fn provenance(&self, section_id: &str) -> RepoProvenance {
        if section_id.is_empty() {
            return RepoProvenance::Unknown;
        }
        self.provenance_map
            .get(section_id)
            .copied()
            .unwrap_or(RepoProvenance::Unknown)
    }

    pub fn is_distro_repo(section_id: &str) -> bool {
        DISTRO_REPOS.contains(&section_id)
    }

    fn empty() -> Self {
        Self {
            packages_by_repo: BTreeMap::new(),
            repo_file_by_section: BTreeMap::new(),
            gpg_keys_by_section: BTreeMap::new(),
            sections_by_gpg_key: BTreeMap::new(),
            provenance_map: BTreeMap::new(),
        }
    }
}

struct RepoSection {
    id: String,
    gpg_key_paths: Vec<String>,
}

fn parse_repo_sections(content: &str) -> Vec<RepoSection> {
    let mut sections = Vec::new();
    let mut current_id: Option<String> = None;
    let mut current_keys: Vec<String> = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            // Save previous section
            if let Some(id) = current_id.take() {
                sections.push(RepoSection { id, gpg_key_paths: current_keys.clone() });
                current_keys.clear();
            }
            current_id = Some(trimmed[1..trimmed.len()-1].to_string());
        } else if let Some(value) = trimmed.strip_prefix("gpgkey=") {
            // gpgkey can be comma-separated or space-separated
            for path in value.split([',', ' ']) {
                let path = path.trim();
                if let Some(file_path) = path.strip_prefix("file://") {
                    current_keys.push(file_path.to_string());
                }
            }
        }
    }
    // Save last section
    if let Some(id) = current_id {
        sections.push(RepoSection { id, gpg_key_paths: current_keys });
    }

    sections
}
```

- [ ] **Step 4: Register the module**

In `inspectah-refine/src/lib.rs`, add:

```rust
pub mod repo_index;
```

- [ ] **Step 5: Run RepoIndex tests**

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

The existing `normalize.rs` has the current normalize logic. This task replaces it with the tier-aware version that materializes into snapshot state.

- [ ] **Step 1: Write failing tests**

In `inspectah-refine/tests/normalize_test.rs`, add (or replace existing content):

```rust
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind, ConfigSection};
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
use inspectah_refine::attention::{compute_config_attention, compute_package_attention};
use inspectah_refine::normalize::{normalize_config_defaults, normalize_package_defaults};
use inspectah_refine::types::AttentionLevel;

#[test]
fn test_tier1_packages_include_true() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "glibc".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            source_repo: "baseos".into(),
            include: false, // starts false
            ..Default::default()
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
            name: "mystery".into(),
            arch: "x86_64".into(),
            state: PackageState::LocalInstall,
            source_repo: "".into(),
            include: true, // starts true
            ..Default::default()
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
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "appstream".into(),
                include: true,
                ..Default::default()
            },
            PackageEntry {
                name: "apr".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "appstream".into(),
                include: true,
                ..Default::default()
            },
        ],
        baseline_package_names: Some(vec![]),
        leaf_packages: Some(vec!["httpd".into()]),
        ..Default::default()
    });
    let pkgs = compute_package_attention(&snap);
    normalize_package_defaults(&mut snap, &pkgs);
    let rpm = snap.rpm.as_ref().unwrap();
    assert!(rpm.packages_added[0].include, "httpd is leaf, should be included");
    assert!(!rpm.packages_added[1].include, "apr is non-leaf, should be hidden");
}

#[test]
fn test_tier1_configs_include_false() {
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/httpd/conf/httpd.conf".into(),
            kind: ConfigFileKind::RpmOwnedDefault,
            include: true, // starts true
            ..Default::default()
        }],
    });
    let configs = compute_config_attention(&snap);
    normalize_config_defaults(&mut snap, &configs);
    assert!(!snap.config.as_ref().unwrap().files[0].include,
        "Tier 1 configs must NOT be copied — package manager handles them");
}

#[test]
fn test_tier1_configs_absent_from_copy_roots() {
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        files: vec![
            ConfigFileEntry {
                path: "/etc/default.conf".into(),
                kind: ConfigFileKind::RpmOwnedDefault,
                include: true,
                ..Default::default()
            },
            ConfigFileEntry {
                path: "/etc/baseline.conf".into(),
                kind: ConfigFileKind::BaselineMatch,
                include: true,
                ..Default::default()
            },
            ConfigFileEntry {
                path: "/etc/custom.conf".into(),
                kind: ConfigFileKind::Unowned,
                include: true,
                ..Default::default()
            },
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
            path: "/etc/old-package.conf".into(),
            kind: ConfigFileKind::Orphaned,
            include: true,
            ..Default::default()
        }],
    });
    let configs = compute_config_attention(&snap);
    normalize_config_defaults(&mut snap, &configs);
    assert!(!snap.config.as_ref().unwrap().files[0].include);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-refine test_tier1 test_tier3 test_leaf test_orphaned`
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
            .map(|t| t.level)
            .unwrap_or(AttentionLevel::Routine);

        match primary_level {
            AttentionLevel::Routine => {
                // Tier 1: auto-include
                rpm.packages_added[i].include = true;
            }
            AttentionLevel::Informational => {
                // Tier 2: include if leaf (or no leaf data)
                let is_leaf = match &leaf_set {
                    Some(set) => set.contains(rpm.packages_added[i].name.as_str()),
                    None => true,
                };
                rpm.packages_added[i].include = is_leaf;
            }
            AttentionLevel::NeedsReview => {
                // Tier 3: exclude by default, operator opts in
                rpm.packages_added[i].include = false;
            }
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
            .map(|t| t.level)
            .unwrap_or(AttentionLevel::Routine);

        match primary_level {
            AttentionLevel::Routine => {
                // Tier 1: NOT copied — package manager handles these
                config.files[i].include = false;
            }
            AttentionLevel::Informational => {
                match config.files[i].kind {
                    inspectah_core::types::config::ConfigFileKind::Orphaned => {
                        // Orphaned: exclude by default
                        config.files[i].include = false;
                    }
                    _ => {
                        // Unowned: include (user-created)
                        config.files[i].include = true;
                    }
                }
            }
            AttentionLevel::NeedsReview => {
                // Tier 3 (RpmOwnedModified): include (user-customized)
                config.files[i].include = true;
            }
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

### Task 6: Session Integration — Normalize at Construction + ExcludeRepo

**Files:**
- Modify: `inspectah-refine/src/session.rs`
- Test: `inspectah-refine/tests/session_test.rs`

This is the heaviest task. It integrates RepoIndex into the session, materializes normalization at construction time, and adds ExcludeRepo/IncludeRepo cascade handling.

- [ ] **Step 1: Write failing tests for normalization at construction**

In `inspectah-refine/tests/session_test.rs`, add:

```rust
#[test]
fn test_session_normalizes_at_construction() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "glibc".into(), arch: "x86_64".into(),
                state: PackageState::Added, source_repo: "baseos".into(),
                include: false, ..Default::default()
            },
        ],
        baseline_package_names: Some(vec!["glibc".into()]),
        ..Default::default()
    });
    let session = RefineSession::new(snap);
    let view = session.view();
    // Tier 1 package should be auto-included after normalization
    assert!(view.packages[0].entry.include);
}

#[test]
fn test_session_preview_export_parity() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "httpd".into(), arch: "x86_64".into(),
                state: PackageState::Added, source_repo: "appstream".into(),
                include: false, ..Default::default()
            },
        ],
        baseline_package_names: Some(vec![]),
        ..Default::default()
    });
    let session = RefineSession::new(snap);
    let preview = &session.view().containerfile_preview;
    let snap_include = session.snapshot().rpm.as_ref().unwrap()
        .packages_added[0].include;
    // Preview and snapshot state must agree on include
    assert!(snap_include, "normalized snapshot must have include=true for Tier 2");
    assert!(preview.contains("httpd"), "preview must render included package");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-refine test_session_normalizes test_session_preview`
Expected: FAIL.

- [ ] **Step 3: Integrate normalization into `RefineSession::new()`**

In `inspectah-refine/src/session.rs`, modify the constructor:

```rust
use crate::repo_index::RepoIndex;
use crate::normalize::{normalize_package_defaults, normalize_config_defaults};

pub struct RefineSession {
    original: InspectionSnapshot,
    repo_index: RepoIndex,
    ops: Vec<RefinementOp>,
    cursor: usize,
    cached_view: Option<RefinedView>,
    generation: u64,
    viewed: HashSet<String>,
}

impl RefineSession {
    pub fn new(mut snapshot: InspectionSnapshot) -> Self {
        // Build repo index from raw snapshot
        let repo_index = RepoIndex::build(&snapshot);

        // Classify
        let pkgs = compute_package_attention(&snapshot);
        let configs = compute_config_attention(&snapshot);

        // Normalize — materializes into snapshot state
        normalize_package_defaults(&mut snapshot, &pkgs);
        normalize_config_defaults(&mut snapshot, &configs);

        let mut session = Self {
            original: snapshot,
            repo_index,
            ops: Vec::new(),
            cursor: 0,
            cached_view: None,
            generation: 0,
            viewed: HashSet::new(),
        };
        session.recompute_view();
        session
    }

    pub fn repo_index(&self) -> &RepoIndex {
        &self.repo_index
    }

    // ... rest unchanged
}
```

- [ ] **Step 4: Run normalization tests**

Run: `cargo test -p inspectah-refine test_session_normalizes test_session_preview`
Expected: PASS.

- [ ] **Step 5: Write failing tests for ExcludeRepo**

```rust
#[test]
fn test_exclude_repo_cascades_packages() {
    let snap = make_snap_with_repos(); // reuse from repo_index_test
    let mut session = RefineSession::new(snap);
    session.apply(RefinementOp::ExcludeRepo { section_id: "epel".into() }).unwrap();
    let view = session.view();
    let epel_pkg = view.packages.iter().find(|p| p.entry.name == "epel-release");
    assert!(epel_pkg.is_some());
    assert!(!epel_pkg.unwrap().entry.include, "epel package must be excluded");
}

#[test]
fn test_exclude_repo_rejects_distro_repo() {
    let snap = make_snap_with_repos();
    let mut session = RefineSession::new(snap);
    let result = session.apply(RefinementOp::ExcludeRepo { section_id: "baseos".into() });
    assert!(result.is_err(), "distro repos cannot be excluded");
}

#[test]
fn test_exclude_repo_rejects_incomplete_provenance() {
    let mut snap = make_snap_with_repos();
    snap.rpm.as_mut().unwrap().packages_added.push(PackageEntry {
        name: "custom".into(), arch: "x86_64".into(),
        state: PackageState::Added, source_repo: "no-repo-file".into(),
        include: true, ..Default::default()
    });
    let mut session = RefineSession::new(snap);
    let result = session.apply(RefinementOp::ExcludeRepo { section_id: "no-repo-file".into() });
    assert!(result.is_err(), "incomplete provenance repos cannot be excluded");
}

#[test]
fn test_exclude_repo_is_dirty() {
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
    // centos.repo has baseos + appstream. Excluding appstream alone
    // must NOT exclude the repo file.
    let mut snap = make_snap_with_repos();
    // Make appstream non-distro for this test (distro repos can't be excluded)
    // Instead test with a multi-section third-party repo file
    snap.rpm.as_mut().unwrap().repo_files.push(RepoFile {
        path: "/etc/yum.repos.d/custom-multi.repo".into(),
        content: "[custom-a]\nname=Custom A\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-custom\n\n[custom-b]\nname=Custom B\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-custom\n".into(),
        include: true,
        ..Default::default()
    });
    snap.rpm.as_mut().unwrap().gpg_keys.push(RepoFile {
        path: "/etc/pki/rpm-gpg/RPM-GPG-KEY-custom".into(),
        content: "key-data".into(),
        include: true,
        ..Default::default()
    });
    snap.rpm.as_mut().unwrap().packages_added.push(PackageEntry {
        name: "pkg-a".into(), arch: "x86_64".into(),
        state: PackageState::Added, source_repo: "custom-a".into(),
        include: true, ..Default::default()
    });
    snap.rpm.as_mut().unwrap().packages_added.push(PackageEntry {
        name: "pkg-b".into(), arch: "x86_64".into(),
        state: PackageState::Added, source_repo: "custom-b".into(),
        include: true, ..Default::default()
    });

    let mut session = RefineSession::new(snap);
    // Exclude custom-a only
    session.apply(RefinementOp::ExcludeRepo { section_id: "custom-a".into() }).unwrap();
    let snap_after = session.snapshot();
    let repo_file = snap_after.rpm.as_ref().unwrap().repo_files.iter()
        .find(|rf| rf.path.contains("custom-multi")).unwrap();
    assert!(repo_file.include, "shared repo file must stay included while custom-b is enabled");
    let gpg = snap_after.rpm.as_ref().unwrap().gpg_keys.iter()
        .find(|k| k.path.contains("RPM-GPG-KEY-custom")).unwrap();
    assert!(gpg.include, "shared GPG key must stay included while custom-b is enabled");

    // Now exclude custom-b too
    session.apply(RefinementOp::ExcludeRepo { section_id: "custom-b".into() }).unwrap();
    let snap_after2 = session.snapshot();
    let gpg2 = snap_after2.rpm.as_ref().unwrap().gpg_keys.iter()
        .find(|k| k.path.contains("RPM-GPG-KEY-custom")).unwrap();
    assert!(!gpg2.include, "GPG key must be excluded once all referencing sections are excluded");
}

#[test]
fn test_exclude_repo_undo_redo() {
    let snap = make_snap_with_repos();
    let mut session = RefineSession::new(snap);
    session.apply(RefinementOp::ExcludeRepo { section_id: "epel".into() }).unwrap();
    assert!(!session.snapshot().rpm.as_ref().unwrap().packages_added
        .iter().find(|p| p.name == "epel-release").unwrap().include);
    session.undo().unwrap();
    assert!(session.snapshot().rpm.as_ref().unwrap().packages_added
        .iter().find(|p| p.name == "epel-release").unwrap().include);
    session.redo().unwrap();
    assert!(!session.snapshot().rpm.as_ref().unwrap().packages_added
        .iter().find(|p| p.name == "epel-release").unwrap().include);
}
```

- [ ] **Step 6: Run tests to verify they fail**

Run: `cargo test -p inspectah-refine test_exclude_repo test_shared_repo`
Expected: FAIL.

- [ ] **Step 7: Implement ExcludeRepo / IncludeRepo in session**

Add the cascade logic to `session.rs`. This involves:
1. Adding repo ops to `validate_target()` — check provenance and distro guard
2. Adding repo ops to `is_op_noop()`
3. Adding repo cascade to the replay loop in `recompute_view()`
4. Extending `pending_changes()` to track repo exclusions

The implementation must use the `RepoIndex` for reference counting: when excluding a section, only flip a GPG key's `include` to false if ALL sections referencing that key are now excluded.

(Full implementation code omitted for brevity — the engineer implements the cascade logic following the test contracts above. The key function is a `apply_repo_cascade()` helper that takes a section_id and desired include state, walks `packages_by_repo`, `repo_file_by_section`, and `sections_by_gpg_key` from the RepoIndex, and flips include flags with reference counting.)

- [ ] **Step 8: Run all repo tests**

Run: `cargo test -p inspectah-refine test_exclude_repo test_shared_repo`
Expected: All PASS.

- [ ] **Step 9: Run full test suite**

Run: `cargo test -p inspectah-refine`
Expected: All PASS.

- [ ] **Step 10: Commit**

```bash
git add inspectah-refine/src/session.rs inspectah-refine/tests/session_test.rs
git commit -m "feat(refine): normalize at construction, ExcludeRepo/IncludeRepo with cascade and ref counting"
```

---

### Task 7: Containerfile Renderer Fixes

**Files:**
- Modify: `inspectah-pipeline/src/render/containerfile.rs`
- Test: existing inline tests in same file

- [ ] **Step 1: Write failing test for GPG batching**

In the `#[cfg(test)] mod tests` section of `containerfile.rs`:

```rust
#[test]
fn test_gpg_standard_dir_single_copy() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        gpg_keys: vec![
            RepoFile {
                path: "/etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9".into(),
                content: "key1".into(), include: true, ..Default::default()
            },
            RepoFile {
                path: "/etc/pki/rpm-gpg/RPM-GPG-KEY-centosofficial".into(),
                content: "key2".into(), include: true, ..Default::default()
            },
        ],
        ..Default::default()
    });
    let output = render_containerfile(&snap, None);
    // Should be one COPY line for the directory, no per-key rpm --import
    let copy_lines: Vec<_> = output.lines()
        .filter(|l| l.contains("COPY") && l.contains("rpm-gpg"))
        .collect();
    assert_eq!(copy_lines.len(), 1, "standard dir keys should be single COPY");
    assert!(!output.contains("rpm --import"), "standard dir keys should not have explicit imports");
}
```

- [ ] **Step 2: Write failing test for service formatting**

```rust
#[test]
fn test_service_backslash_continuation_over_3() {
    let mut snap = InspectionSnapshot::new();
    snap.services = Some(inspectah_core::types::services::ServiceSection {
        enabled_units: vec![
            "httpd.service".into(), "sshd.service".into(),
            "chronyd.service".into(), "firewalld.service".into(),
        ],
        ..Default::default()
    });
    let output = render_containerfile(&snap, None);
    assert!(output.contains("systemctl enable \\"),
        "4+ services should use backslash continuation");
    assert!(output.contains("    httpd.service \\"),
        "each service on its own indented line");
}

#[test]
fn test_service_single_line_under_4() {
    let mut snap = InspectionSnapshot::new();
    snap.services = Some(inspectah_core::types::services::ServiceSection {
        enabled_units: vec!["httpd.service".into(), "sshd.service".into()],
        ..Default::default()
    });
    let output = render_containerfile(&snap, None);
    assert!(output.contains("systemctl enable httpd.service sshd.service"),
        "3 or fewer services should be single line");
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p inspectah-pipeline test_gpg_standard test_service_backslash test_service_single`
Expected: FAIL.

- [ ] **Step 4: Implement GPG batching**

Modify the GPG section in `containerfile.rs`. When all included GPG keys share the `/etc/pki/rpm-gpg/` directory, emit a single `COPY config/etc/pki/rpm-gpg/ /etc/pki/rpm-gpg/` with no `rpm --import` lines. For keys outside that directory, keep the per-key pattern.

- [ ] **Step 5: Implement service backslash continuation**

Modify `services_section_lines()` in `containerfile.rs`. When `safe_enabled.len() > 3`, format as:

```rust
if safe_enabled.len() > 3 {
    lines.push("RUN systemctl enable \\".into());
    for (i, u) in safe_enabled.iter().enumerate() {
        if i < safe_enabled.len() - 1 {
            lines.push(format!("    {} \\", u));
        } else {
            lines.push(format!("    {}", u));
        }
    }
} else {
    lines.push(format!("RUN systemctl enable {}", safe_enabled.join(" ")));
}
```

Same for `safe_disabled`.

- [ ] **Step 6: Run renderer tests**

Run: `cargo test -p inspectah-pipeline`
Expected: All PASS (update golden files if needed).

- [ ] **Step 7: Commit**

```bash
git add inspectah-pipeline/src/render/containerfile.rs
git commit -m "feat(pipeline): GPG key batching for standard dir, service backslash continuation"
```

---

### Task 8: source_repo Investigation and Fix

**Files:**
- Investigate: `inspectah-collect/src/inspectors/rpm/mod.rs` (Rust scanner)
- Investigate: Go source `cmd/inspectah/internal/inspectors/rpm.go` (Go scanner `populateSourceRepos`)
- Possibly modify: Rust RPM inspector or snapshot serialization

This task is investigative — the root cause of the "Unknown" repo display is not yet known.

- [ ] **Step 1: Check Go scanner for `source_repo` population**

Read `cmd/inspectah/internal/inspectors/rpm.go` and search for `populateSourceRepos` or `source_repo` or `SourceRepo`. Document how the Go scanner determines and sets this field.

- [ ] **Step 2: Check a real Go-generated tarball**

Examine the CentOS Stream 9 scan tarball's `snapshot.json` for `source_repo` values on packages. Are they populated or empty?

- [ ] **Step 3: Check Rust RPM inspector**

Read `inspectah-collect/src/inspectors/rpm/mod.rs` for any `source_repo` population logic. If absent, this is the gap.

- [ ] **Step 4: Determine root cause and fix**

Based on investigation:
- If Go populates but field name differs (e.g., `sourceRepo` vs `source_repo`): fix serde alias in Rust types
- If Go populates but Rust scanner doesn't: port the `populateSourceRepos` logic to Rust
- If Go doesn't populate for this scan: the scan predates the feature, re-scan needed

- [ ] **Step 5: Write a test proving `source_repo` is populated**

Add a test that deserializes a known-good snapshot and verifies `source_repo` is non-empty for packages from standard repos.

- [ ] **Step 6: Commit**

```bash
git commit -m "fix(collect): populate source_repo for packages from RPM metadata"
```

---

### Task 9: Health Endpoint — Policy Field

**Files:**
- Modify: `inspectah-web/src/handlers.rs`
- Test: existing inline tests in same file

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn health_includes_policy_distro_repos() {
    let snap = InspectionSnapshot::new();
    let session = RefineSession::new(snap);
    let state = Arc::new(AppState {
        session: Arc::new(Mutex::new(session)),
        sections_cache: OnceLock::new(),
    });
    // Call health handler and check for policy.distro_repos
    let rt = tokio::runtime::Runtime::new().unwrap();
    let response = rt.block_on(health(State(state)));
    let json = response.0;
    let distro_repos = json["policy"]["distro_repos"].as_array().unwrap();
    assert!(distro_repos.iter().any(|v| v == "baseos"));
    assert!(distro_repos.iter().any(|v| v == "appstream"));
}
```

- [ ] **Step 2: Add policy field to health response**

In `handlers.rs`, modify the `health()` handler to include:

```rust
use inspectah_refine::repo_index::DISTRO_REPOS;

// In the health handler's json! macro:
"policy": {
    "distro_repos": DISTRO_REPOS,
}
```

- [ ] **Step 3: Run test**

Run: `cargo test -p inspectah-web health_includes_policy`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add inspectah-web/src/handlers.rs
git commit -m "feat(web): add policy.distro_repos to health endpoint for UI repo classification"
```

---

### Task 10: Expose RepoIndex Data via API

**Files:**
- Modify: `inspectah-web/src/handlers.rs`

The UI needs repo provenance data to determine which repos get toggle controls. Add repo metadata to the refined view response.

- [ ] **Step 1: Add repo metadata to the view endpoint**

Extend the `/api/snapshot/view` response (or `/api/snapshot/refined`) to include repo grouping data:

```rust
#[derive(Serialize)]
pub struct RepoGroupInfo {
    pub section_id: String,
    pub provenance: RepoProvenance,
    pub is_distro: bool,
    pub package_count: usize,
}
```

Include `Vec<RepoGroupInfo>` in the view response, built from `session.repo_index()`.

- [ ] **Step 2: Write test**

Verify the response includes repo groups with correct provenance and distro flags.

- [ ] **Step 3: Commit**

```bash
git add inspectah-web/src/handlers.rs
git commit -m "feat(web): expose repo group metadata with provenance in view response"
```

---

## Kit Tasks (Web UI)

### Task 11: Layout Fixes (Independent — No Pipeline Dependency)

**Files:**
- Modify: `inspectah-web/ui/src/App.css`
- Modify: `inspectah-web/ui/src/components/Sidebar.tsx`
- Modify: `inspectah-web/ui/src/components/ContainerfilePanel.tsx`

These are independent of the pipeline changes and can ship immediately.

- [ ] **Step 1: Full-width layout**

In `App.css`, strip PatternFly Page padding:

```css
.pf-v6-c-page__main {
  padding: 0;
}
```

- [ ] **Step 2: Nav spacing fix**

In `App.css`, remove `flex: 1` from sidebar nav so items top-align:

```css
.inspectah-sidebar nav {
  flex: initial;
}
```

- [ ] **Step 3: Move hostname to top of sidebar**

In `Sidebar.tsx`, move the `inspectah-sidebar__host` block above the `<Nav>` element. Change from subtle small text to bold hostname with OS info below. Add bottom border instead of top.

- [ ] **Step 4: Fix panel collapse icon direction**

In `ContainerfilePanel.tsx`, flip the icon so it points right when collapsed (indicating "expand right") and left when open (indicating "collapse left").

- [ ] **Step 5: Verify visually**

Run: `cd inspectah-web/ui && npm run dev`
Open browser, verify: full-width layout, top-aligned nav, hostname at top, correct collapse icon direction.

- [ ] **Step 6: Run existing tests**

Run: `cd inspectah-web/ui && npm test`
Expected: All PASS (update selectors if needed).

- [ ] **Step 7: Commit**

```bash
git add inspectah-web/ui/src/App.css inspectah-web/ui/src/components/Sidebar.tsx inspectah-web/ui/src/components/ContainerfilePanel.tsx
git commit -m "fix(web): full-width layout, nav spacing, hostname to top, panel collapse icon"
```

---

### Task 12: Tier-Aware Card Treatment

**Files:**
- Modify: `inspectah-web/ui/src/components/DecisionSections.tsx`
- Modify: `inspectah-web/ui/src/components/PackageDetail.tsx`
- Modify: `inspectah-web/ui/src/App.css`
- Test: `inspectah-web/ui/src/components/__tests__/DecisionSections.test.tsx`

Depends on Tang's pipeline landing (Tasks 1-6).

- [ ] **Step 1: Render Tier 1 as collapsed summary**

In `DecisionSections.tsx`, detect `AttentionLevel::Routine` items and render them as a collapsed summary group: "N baseline packages (auto-included)" with an expand toggle.

When expanded, render a compact list (name only, muted text, no cards, no checkboxes).

- [ ] **Step 2: Render Tier 2 with info-level styling**

Change the left border color from warning to info (blue) for `AttentionLevel::Informational` items. Badge shows `source_repo` value when provenance is `Verified`, or "baseline unavailable" when `PackageProvenanceUnavailable`.

- [ ] **Step 3: Add provenance completeness banner**

When the API response indicates baseline data is unavailable, show a banner at the top of the Packages section: "Baseline data unavailable — classification confidence reduced."

- [ ] **Step 4: Write tests**

Test that Tier 1 items render as collapsed summary, Tier 2 items show correct badge text for verified vs. provenance-unavailable, and Tier 3 items render unchanged.

- [ ] **Step 5: Run tests**

Run: `cd inspectah-web/ui && npm test`
Expected: All PASS.

- [ ] **Step 6: Commit**

```bash
git commit -m "feat(web): tier-aware card treatment with collapsed Tier 1, info-level Tier 2, provenance banner"
```

---

### Task 13: Repo Group Headers and Bulk Toggle

**Files:**
- Create: `inspectah-web/ui/src/components/RepoGroupHeader.tsx`
- Modify: `inspectah-web/ui/src/components/DecisionSections.tsx`
- Test: `inspectah-web/ui/src/components/__tests__/RepoGroupHeader.test.tsx`

- [ ] **Step 1: Create `RepoGroupHeader` component**

Renders: repo label, distro/third-party/unverified/unknown badge, package count, and conditional enable/disable toggle. Toggle only shown for third-party repos with `Verified` provenance.

Props: `sectionId`, `provenance`, `isDistro`, `packageCount`, `enabled`, `onToggle`.

Badge labels: "Distro" / "Third-party" / "Unverified" / "Unknown". Abbreviate to "D" / "3P" at <768px with `aria-label` preserving full text.

- [ ] **Step 2: Group Tier 2 packages by repo in DecisionSections**

Read repo group data from the API response. Render each repo as a group with `RepoGroupHeader` followed by the group's package cards.

- [ ] **Step 3: Wire toggle to ExcludeRepo / IncludeRepo API call**

Optimistic UI: flip immediately, send API request, show undo toast via `role="status"` live region on success, revert + error banner via `role="alert"` on failure.

- [ ] **Step 4: Keyboard traversal**

Group headers are Tab stops. Within a header, Tab moves to the toggle (if present), then to the first item. Arrow/j/k navigate within a group.

- [ ] **Step 5: Write tests**

Test header rendering for each provenance state, toggle visibility rules, and keyboard focus order.

- [ ] **Step 6: Run tests**

Run: `cd inspectah-web/ui && npm test`
Expected: All PASS.

- [ ] **Step 7: Commit**

```bash
git commit -m "feat(web): repo group headers with bulk toggle, distro/third-party badges, keyboard nav"
```

---

### Task 14: Config Grouping

**Files:**
- Modify: `inspectah-web/ui/src/components/DecisionSections.tsx` (or create `ConfigGroup.tsx`)

- [ ] **Step 1: Group configs by kind**

Tier 1 collapsed: "N configs managed by packages (not copied)". Tier 2 (Unowned) shown as reviewable cards grouped by parent directory. Tier 3 (RpmOwnedModified) shown with attention badge.

- [ ] **Step 2: Config diff indicator**

When `diff_against_rpm` is available on a config entry, show a "View diff" link. Clicking opens inline diff below the card. No indicator when no diff data.

- [ ] **Step 3: Write tests and commit**

```bash
git commit -m "feat(web): config kind grouping with Tier 1 collapsed, diff indicator"
```

---

### Task 15: Search Auto-Reveal for Collapsed Groups

**Files:**
- Modify: `inspectah-web/ui/src/components/GlobalSearch.tsx` (or equivalent)
- Modify: `inspectah-web/ui/src/components/DecisionSections.tsx`

- [ ] **Step 1: Implement auto-reveal**

When global search selects an item inside a collapsed group:
1. Auto-expand the containing group
2. Scroll item into view
3. Flash highlight (2-second CSS animation)
4. Focus lands on the item's primary action control

- [ ] **Step 2: Write test and commit**

```bash
git commit -m "feat(web): search auto-reveal for collapsed tier/repo groups"
```

---

### Task 16: Responsive Behavior

**Files:**
- Modify: `inspectah-web/ui/src/App.css`

- [ ] **Step 1: Responsive repo headers**

At <1024px: label + count on first line, toggle on second. At <768px: badge abbreviates to "D" / "3P" with `aria-label`.

- [ ] **Step 2: Verify and commit**

```bash
git commit -m "feat(web): responsive repo headers and badge abbreviation"
```

---

## Integration

### Task 17: E2E Smoke Test with CentOS Stream 9 Tarball

**Files:**
- Test: `inspectah-web/ui/tests/e2e/` (Playwright)

- [ ] **Step 1: Add E2E test for triage count reduction**

Create a Playwright test that loads the CentOS Stream 9 tarball, verifies the triage counter shows ~50-80 items (not ~734), verifies Tier 1 packages are collapsed, and verifies repo grouping is visible.

- [ ] **Step 2: Add E2E test for ExcludeRepo flow**

Test: click a third-party repo toggle, verify packages disappear from triage and Containerfile, verify undo toast appears, click undo, verify restoration.

- [ ] **Step 3: Run E2E tests**

Run: `cd inspectah-web/ui && npx playwright test`
Expected: All PASS.

- [ ] **Step 4: Commit**

```bash
git commit -m "test(web): E2E smoke tests for tiered triage and repo exclusion"
```

---

## Task Dependency Summary

```
Task 1 (types) ─────┬──→ Task 2 (pkg classify) ──→ Task 5 (normalize) ──→ Task 6 (session) ──→ Task 7 (containerfile)
                     ├──→ Task 3 (cfg classify) ──→ Task 5                                  ──→ Task 8 (source_repo)
                     └──→ Task 4 (repo index) ────→ Task 6                                  ──→ Task 9 (health endpoint)
                                                                                             ──→ Task 10 (view endpoint)

Task 11 (layout CSS) ─── independent, ship anytime

Tasks 12-16 (UI tiers, repo groups, config groups, search, responsive) ── blocked on Tasks 1-10

Task 17 (E2E) ── blocked on all above
```
