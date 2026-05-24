# CLI Scan Progress UX Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the silent `inspectah scan` with a full progress checklist showing inspector states, sub-step detail, and a confidence-building completion summary.

**Architecture:** New `ProgressEvent` / `ProgressSink` types in inspectah-core define a typed event contract. `collect()` in inspectah-pipeline emits inspector lifecycle events through the sink. Three inspectors (RPM, Config, Non-RPM) emit sub-step events internally. The CLI provides a `TerminalProgress` renderer with three modes (rich/plain/flat). Exit codes reflect report trustworthiness via a `ScanOutcome` enum.

**Tech Stack:** Rust (edition 2024), existing workspace crates (inspectah-core, inspectah-collect, inspectah-pipeline, inspectah-cli). `terminal_size` (already a dep), `std::sync::atomic` for cancellation, `std::thread` for render tick.

**Spec:** `docs/specs/proposed/2026-05-24-cli-scan-progress-design.md` (approved revision 4)

---

### Task 1: Define Progress Event Types

**Files:**
- Create: `inspectah-core/src/types/progress.rs`
- Modify: `inspectah-core/src/types/mod.rs`

- [ ] **Step 1: Write the types module**

```rust
// inspectah-core/src/types/progress.rs

use crate::types::completeness::InspectorId;

/// Sub-step identity within an inspector's progress reporting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepId {
    // RPM sub-steps
    QueryingPackages,
    ClassifyingPackages,
    ResolvingSourceRepos,
    ResolvingDepTree,
    VerifyingIntegrity,
    MappingFileOwnership,
    // Config sub-steps
    ApplyingRpmVerification,
    WalkingFilesystem,
    ClassifyingConfigs,
}

/// Non-RPM ecosystem probe identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeId {
    ElfBinaries,
    PythonVenvs,
    PipPackages,
    NpmPackages,
    GemPackages,
    EnvFiles,
    GitRepos,
}

/// Typed metric kinds for count events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetricKind {
    PackagesFound,
    ReposMapped,
    ConfigsModified,
    UnitsFound,
    ContainersFound,
    TimersFound,
}

/// Terminal outcome for an inspector.
#[derive(Debug, Clone)]
pub enum InspectorOutcome {
    Complete,
    Degraded { reason: String },
    Skipped { reason: String },
    Failed { reason: String },
    Interrupted,
}

/// Terminal outcome for a sub-step.
#[derive(Debug, Clone)]
pub enum StepOutcome {
    Complete,
    Degraded { reason: String },
    Failed { reason: String },
    Skipped { reason: String },
    Interrupted,
}

/// Terminal outcome for a Non-RPM ecosystem probe.
#[derive(Debug, Clone)]
pub enum ProbeOutcome {
    Found { count: usize },
    Empty,
}

/// Typed progress event emitted by inspectors and the collector.
///
/// All events are owned (no borrowed lifetimes) to support buffering,
/// channel transport, and cross-thread aggregation.
#[derive(Debug, Clone)]
pub enum ProgressEvent {
    InspectorStarted(InspectorId),
    InspectorFinished {
        id: InspectorId,
        outcome: InspectorOutcome,
    },
    StepStarted {
        inspector: InspectorId,
        step: StepId,
    },
    StepFinished {
        inspector: InspectorId,
        step: StepId,
        outcome: StepOutcome,
    },
    Metric {
        inspector: InspectorId,
        kind: MetricKind,
        value: usize,
    },
    ProbeStarted {
        inspector: InspectorId,
        probe: ProbeId,
    },
    ProbeFinished {
        inspector: InspectorId,
        probe: ProbeId,
        outcome: ProbeOutcome,
    },
}
```

- [ ] **Step 2: Register the module**

Add `pub mod progress;` to `inspectah-core/src/types/mod.rs`.

- [ ] **Step 3: Verify it compiles**

Run: `cargo build -p inspectah-core`
Expected: compiles cleanly

- [ ] **Step 4: Commit**

```bash
git add inspectah-core/src/types/progress.rs inspectah-core/src/types/mod.rs
git commit -m "feat(core): add typed progress event model

StepId, ProbeId, MetricKind, InspectorOutcome, StepOutcome,
ProbeOutcome, and ProgressEvent enums for CLI scan progress.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 2: Define ProgressSink Trait + NullProgress + VecProgress

**Files:**
- Create: `inspectah-core/src/traits/progress.rs`
- Modify: `inspectah-core/src/traits/mod.rs`

- [ ] **Step 1: Write the failing test**

```rust
// at the bottom of inspectah-core/src/traits/progress.rs

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::completeness::InspectorId;
    use crate::types::progress::{InspectorOutcome, ProgressEvent};

    #[test]
    fn null_progress_accepts_events() {
        let sink = NullProgress;
        sink.emit(ProgressEvent::InspectorStarted(InspectorId::Rpm));
        // No panic, no output — just proves it compiles and runs.
    }

    #[test]
    fn vec_progress_collects_events() {
        let sink = VecProgress::new();
        sink.emit(ProgressEvent::InspectorStarted(InspectorId::Rpm));
        sink.emit(ProgressEvent::InspectorFinished {
            id: InspectorId::Rpm,
            outcome: InspectorOutcome::Complete,
        });
        let events = sink.events();
        assert_eq!(events.len(), 2);
        assert!(matches!(
            events[0],
            ProgressEvent::InspectorStarted(InspectorId::Rpm)
        ));
    }

    #[test]
    fn vec_progress_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<VecProgress>();
    }
}
```

- [ ] **Step 2: Write the trait and implementations**

```rust
// inspectah-core/src/traits/progress.rs

use crate::types::progress::ProgressEvent;
use std::sync::Mutex;

/// Sink for progress events emitted during scan collection.
///
/// Implementors must be `Send + Sync` because wave-2 inspectors
/// run in parallel via `std::thread::scope`.
pub trait ProgressSink: Send + Sync {
    fn emit(&self, event: ProgressEvent);
}

/// No-op progress sink. Default for library consumers and tests
/// that don't need progress reporting.
pub struct NullProgress;

impl ProgressSink for NullProgress {
    fn emit(&self, _event: ProgressEvent) {}
}

/// Test utility that collects all emitted events. Thread-safe via Mutex.
pub struct VecProgress {
    events: Mutex<Vec<ProgressEvent>>,
}

impl VecProgress {
    pub fn new() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
        }
    }

    pub fn events(&self) -> Vec<ProgressEvent> {
        self.events.lock().expect("VecProgress lock poisoned").clone()
    }
}

impl ProgressSink for VecProgress {
    fn emit(&self, event: ProgressEvent) {
        self.events
            .lock()
            .expect("VecProgress lock poisoned")
            .push(event);
    }
}
```

- [ ] **Step 3: Register the module**

Add `pub mod progress;` to `inspectah-core/src/traits/mod.rs`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p inspectah-core traits::progress`
Expected: 3 tests pass

- [ ] **Step 5: Commit**

```bash
git add inspectah-core/src/traits/progress.rs inspectah-core/src/traits/mod.rs
git commit -m "feat(core): add ProgressSink trait with NullProgress and VecProgress

Send + Sync trait for thread-safe progress reporting during parallel
inspector execution. NullProgress no-op for library consumers.
VecProgress Mutex-backed collector for test assertions.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 3: Change Inspector Trait to Accept ProgressSink

**Files:**
- Modify: `inspectah-core/src/traits/inspector.rs`
- Modify: every file implementing `Inspector` (11 inspectors + test mocks)

- [ ] **Step 1: Add progress parameter to Inspector trait**

In `inspectah-core/src/traits/inspector.rs`, change:

```rust
pub trait Inspector: Send + Sync {
    fn id(&self) -> InspectorId;
    fn applicable_to(&self) -> &[SourceSystemKind];
    fn inspect(&self, ctx: &InspectionContext<'_>) -> Result<InspectorOutput, InspectorError>;
}
```

to:

```rust
use crate::traits::progress::ProgressSink;

pub trait Inspector: Send + Sync {
    fn id(&self) -> InspectorId;
    fn applicable_to(&self) -> &[SourceSystemKind];
    fn inspect(
        &self,
        ctx: &InspectionContext<'_>,
        progress: &dyn ProgressSink,
    ) -> Result<InspectorOutput, InspectorError>;
}
```

- [ ] **Step 2: Fix all Inspector implementations**

Every inspector's `inspect` method needs the new parameter added. For
this task, add `_progress: &dyn ProgressSink` (unused) to each. The
implementations that emit events are Tasks 5-7.

Files to modify (search for `fn inspect(&self, ctx: &InspectionContext`):
- `inspectah-collect/src/inspectors/rpm/mod.rs`
- `inspectah-collect/src/inspectors/config/mod.rs`
- `inspectah-collect/src/inspectors/services.rs`
- `inspectah-collect/src/inspectors/containers.rs`
- `inspectah-collect/src/inspectors/kernelboot.rs`
- `inspectah-collect/src/inspectors/network.rs`
- `inspectah-collect/src/inspectors/storage.rs`
- `inspectah-collect/src/inspectors/selinux.rs`
- `inspectah-collect/src/inspectors/users.rs`
- `inspectah-collect/src/inspectors/scheduled.rs`
- `inspectah-collect/src/inspectors/nonrpm.rs`

Also fix all mock inspectors in test files:
- `inspectah-pipeline/src/collect.rs` (test mocks)
- Any other test files that implement `Inspector`

Add `use inspectah_core::traits::progress::ProgressSink;` where needed.

- [ ] **Step 3: Update collect() to pass progress**

In `inspectah-pipeline/src/collect.rs`, change `collect()` signature:

```rust
use inspectah_core::traits::progress::{NullProgress, ProgressSink};

pub fn collect(
    source: &SourceSystem,
    executor: &dyn Executor,
    inspectors: &[Box<dyn Inspector>],
    baseline: Option<&BaselineData>,
    progress: &dyn ProgressSink,
) -> Pipeline<Collected> {
```

Pass `progress` to each `inspector.inspect(&base_ctx, progress)` and
`inspector.inspect(&enriched_ctx, progress)` call.

Update the call site in `inspectah-cli/src/commands/scan.rs`:

```rust
use inspectah_core::traits::progress::NullProgress;

let collected = collect(&source, &executor, &inspectors, baseline_data.as_ref(), &NullProgress);
```

- [ ] **Step 4: Fix all tests that call collect()**

Add `&NullProgress` as the last argument to every `collect()` call in
test code. Search for `collect(&source` in test files.

- [ ] **Step 5: Build and test**

Run: `cargo build && cargo test`
Expected: all tests pass, no clippy warnings

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat(core): thread ProgressSink through Inspector trait and collect()

All inspectors accept &dyn ProgressSink. NullProgress used as default
throughout. No functional change — progress events not yet emitted.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 4: Emit Inspector Lifecycle Events from collect()

**Files:**
- Modify: `inspectah-pipeline/src/collect.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn test_collect_emits_inspector_lifecycle_events() {
    use inspectah_core::traits::progress::VecProgress;
    use inspectah_core::types::progress::{InspectorOutcome, ProgressEvent};

    let exec = build_test_mock();
    let source = SourceSystem::PackageBased {
        os_release: test_os_release(),
    };
    let progress = VecProgress::new();
    let inspectors: Vec<Box<dyn Inspector>> = vec![Box::new(RpmInspector::new())];
    let _pipeline = collect(&source, &exec, &inspectors, None, &progress);

    let events = progress.events();
    // Must have at least InspectorStarted and InspectorFinished for RPM
    assert!(
        events.iter().any(|e| matches!(e, ProgressEvent::InspectorStarted(InspectorId::Rpm))),
        "must emit InspectorStarted for RPM"
    );
    assert!(
        events.iter().any(|e| matches!(
            e,
            ProgressEvent::InspectorFinished { id: InspectorId::Rpm, outcome: InspectorOutcome::Complete }
        )),
        "must emit InspectorFinished for RPM"
    );
}

#[test]
fn test_collect_emits_skipped_for_inapplicable() {
    use inspectah_core::traits::progress::VecProgress;
    use inspectah_core::types::progress::{InspectorOutcome, ProgressEvent};

    let exec = build_test_mock();
    let source = SourceSystem::PackageBased {
        os_release: test_os_release(),
    };
    let progress = VecProgress::new();
    let inspectors: Vec<Box<dyn Inspector>> =
        vec![Box::new(RpmInspector::new()), Box::new(SkippedInspector)];
    let _pipeline = collect(&source, &exec, &inspectors, None, &progress);

    let events = progress.events();
    assert!(
        events.iter().any(|e| matches!(
            e,
            ProgressEvent::InspectorFinished { id: InspectorId::Storage, outcome: InspectorOutcome::Skipped { .. } }
        )),
        "inapplicable inspectors must emit Skipped"
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-pipeline test_collect_emits`
Expected: FAIL — no events emitted yet

- [ ] **Step 3: Add lifecycle event emission to collect()**

In the applicability gate (inapplicable inspectors), emit:

```rust
progress.emit(ProgressEvent::InspectorFinished {
    id: inspector.id(),
    outcome: InspectorOutcome::Skipped {
        reason: format!("not applicable to {source_kind:?}"),
    },
});
```

In the wave-1 and wave-2 spawn loops, before spawning:

```rust
progress.emit(ProgressEvent::InspectorStarted(inspector.id()));
```

In `handle_result()`, add a `progress` parameter and emit after join:

```rust
fn handle_result(
    inspector: &dyn Inspector,
    handle: std::thread::ScopedJoinHandle<'_, Result<InspectorOutput, InspectorError>>,
    snapshot: &mut InspectionSnapshot,
    failed: &mut Vec<InspectorId>,
    degraded: &mut Vec<InspectorId>,
    rpm_state: &mut RpmState,
    progress: &dyn ProgressSink,
) -> bool {
    // ... existing match logic ...
    // After each match arm, emit the appropriate InspectorFinished:

    // Ok(Ok(output)) arm:
    progress.emit(ProgressEvent::InspectorFinished {
        id: inspector.id(),
        outcome: InspectorOutcome::Complete,
    });

    // Skipped arm:
    progress.emit(ProgressEvent::InspectorFinished {
        id: inspector.id(),
        outcome: InspectorOutcome::Skipped { reason: reason.clone() },
    });

    // Degraded arm:
    progress.emit(ProgressEvent::InspectorFinished {
        id: inspector.id(),
        outcome: InspectorOutcome::Degraded { reason: reason.clone() },
    });

    // Failed arm:
    progress.emit(ProgressEvent::InspectorFinished {
        id: inspector.id(),
        outcome: InspectorOutcome::Failed { reason: reason.clone() },
    });

    // Panic arm:
    progress.emit(ProgressEvent::InspectorFinished {
        id: inspector.id(),
        outcome: InspectorOutcome::Failed { reason: "inspector panicked".into() },
    });
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p inspectah-pipeline`
Expected: all tests pass including new lifecycle tests

- [ ] **Step 5: Commit**

```bash
git add inspectah-pipeline/src/collect.rs
git commit -m "feat(pipeline): emit inspector lifecycle events from collect()

InspectorStarted before spawn, InspectorFinished after join with
typed outcome derived from InspectorError. Inapplicable inspectors
emit Skipped without spawning.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 5: Add Progress Events to RPM Inspector

**Files:**
- Modify: `inspectah-collect/src/inspectors/rpm/mod.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn test_rpm_inspector_emits_progress_events() {
    use inspectah_core::traits::progress::VecProgress;
    use inspectah_core::types::progress::{MetricKind, ProgressEvent, StepId};

    let exec = /* use existing build_test_mock or equivalent */;
    let source = SourceSystem::PackageBased {
        os_release: test_os_release(),
    };
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };
    let progress = VecProgress::new();
    let _output = RpmInspector::new().inspect(&ctx, &progress).unwrap();

    let events = progress.events();

    // Must see all 6 step pairs in order
    let step_ids: Vec<&StepId> = events
        .iter()
        .filter_map(|e| match e {
            ProgressEvent::StepStarted { step, .. } => Some(step),
            _ => None,
        })
        .collect();

    assert_eq!(step_ids, &[
        &StepId::QueryingPackages,
        &StepId::ClassifyingPackages,
        &StepId::ResolvingSourceRepos,
        &StepId::ResolvingDepTree,
        &StepId::VerifyingIntegrity,
        &StepId::MappingFileOwnership,
    ]);

    // Must see a PackagesFound metric
    assert!(events.iter().any(|e| matches!(
        e,
        ProgressEvent::Metric { kind: MetricKind::PackagesFound, .. }
    )));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p inspectah-collect test_rpm_inspector_emits_progress`
Expected: FAIL — no events emitted

- [ ] **Step 3: Add emit calls to RPM inspect()**

In `inspectah-collect/src/inspectors/rpm/mod.rs`, in the `inspect()`
method, add progress emissions at each phase boundary. The `progress`
parameter was added in Task 3 as `_progress` — rename it to `progress`.

```rust
fn inspect(&self, ctx: &InspectionContext<'_>, progress: &dyn ProgressSink)
    -> Result<InspectorOutput, InspectorError>
{
    let exec = ctx.executor;
    let inspector_id = InspectorId::Rpm;

    // 1. Query packages
    progress.emit(ProgressEvent::StepStarted {
        inspector: inspector_id,
        step: StepId::QueryingPackages,
    });
    let host_packages = self.query_packages(exec);
    if host_packages.is_empty() {
        return Err(InspectorError::Failed {
            reason: "rpm -qa returned no packages".into(),
        });
    }
    progress.emit(ProgressEvent::Metric {
        inspector: inspector_id,
        kind: MetricKind::PackagesFound,
        value: host_packages.len(),
    });
    progress.emit(ProgressEvent::StepFinished {
        inspector: inspector_id,
        step: StepId::QueryingPackages,
        outcome: StepOutcome::Complete,
    });

    // 2. Build baseline and classify
    progress.emit(ProgressEvent::StepStarted {
        inspector: inspector_id,
        step: StepId::ClassifyingPackages,
    });
    let baseline = self.build_baseline(ctx.baseline_data);
    let classification = classifier::classify_packages(&host_packages, &baseline);
    // ... existing code ...
    progress.emit(ProgressEvent::StepFinished {
        inspector: inspector_id,
        step: StepId::ClassifyingPackages,
        outcome: StepOutcome::Complete,
    });

    // 3b. Source repo attribution
    progress.emit(ProgressEvent::StepStarted {
        inspector: inspector_id,
        step: StepId::ResolvingSourceRepos,
    });
    if !packages_added.is_empty() {
        source_repos::populate_source_repos(exec, &mut packages_added);
    }
    // Count unique repos
    let repo_count = packages_added.iter()
        .map(|p| &p.source_repo)
        .filter(|r| !r.is_empty())
        .collect::<std::collections::HashSet<_>>()
        .len();
    progress.emit(ProgressEvent::Metric {
        inspector: inspector_id,
        kind: MetricKind::ReposMapped,
        value: repo_count,
    });
    progress.emit(ProgressEvent::StepFinished {
        inspector: inspector_id,
        step: StepId::ResolvingSourceRepos,
        outcome: StepOutcome::Complete,
    });

    // 5. Classify leaf vs auto
    progress.emit(ProgressEvent::StepStarted {
        inspector: inspector_id,
        step: StepId::ResolvingDepTree,
    });
    let leaf_classification = classify_leaf_auto(exec, &packages_added, &baseline_name_set);
    progress.emit(ProgressEvent::StepFinished {
        inspector: inspector_id,
        step: StepId::ResolvingDepTree,
        outcome: StepOutcome::Complete,
    });

    // 6. Supplementary data
    progress.emit(ProgressEvent::StepStarted {
        inspector: inspector_id,
        step: StepId::VerifyingIntegrity,
    });
    let supp = self.collect_supplementary(exec, ctx.source_system);
    progress.emit(ProgressEvent::StepFinished {
        inspector: inspector_id,
        step: StepId::VerifyingIntegrity,
        outcome: StepOutcome::Complete,
    });

    // 7. File ownership
    progress.emit(ProgressEvent::StepStarted {
        inspector: inspector_id,
        step: StepId::MappingFileOwnership,
    });
    let file_ownership = self.query_file_ownership(exec);
    progress.emit(ProgressEvent::StepFinished {
        inspector: inspector_id,
        step: StepId::MappingFileOwnership,
        outcome: StepOutcome::Complete,
    });

    // ... rest of method unchanged ...
}
```

Add the required imports at the top of the file:

```rust
use inspectah_core::traits::progress::ProgressSink;
use inspectah_core::types::progress::{
    MetricKind, ProgressEvent, StepId, StepOutcome,
};
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p inspectah-collect inspectors::rpm`
Expected: all tests pass including the new progress event test

- [ ] **Step 5: Commit**

```bash
git add inspectah-collect/src/inspectors/rpm/mod.rs
git commit -m "feat(collect): emit progress events from RPM inspector

6 StepStarted/StepFinished pairs covering package query, classify,
repo resolution, dep tree, integrity verification, file ownership.
PackagesFound and ReposMapped metrics.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 6: Add Progress Events to Config Inspector

**Files:**
- Modify: `inspectah-collect/src/inspectors/config/mod.rs`

- [ ] **Step 1: Write the failing test**

Test that Config emits 3 step pairs: `ApplyingRpmVerification`,
`WalkingFilesystem`, `ClassifyingConfigs`, plus a `ConfigsModified`
metric. Follow the same pattern as Task 5.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p inspectah-collect inspectors::config::tests::test_config_inspector_emits_progress`
Expected: FAIL

- [ ] **Step 3: Add emit calls to Config inspect()**

Rename `_progress` to `progress`. Add `StepStarted`/`StepFinished`
pairs around each phase:
1. `ApplyingRpmVerification` — around the rpm -Va results processing
2. `WalkingFilesystem` — around the filesystem walk
3. `ClassifyingConfigs` — around the classification pass

Add a `ConfigsModified` metric after classification completes.

Add required imports: `ProgressSink`, `ProgressEvent`, `StepId`,
`StepOutcome`, `MetricKind`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p inspectah-collect inspectors::config`
Expected: all tests pass

- [ ] **Step 5: Commit**

```bash
git add inspectah-collect/src/inspectors/config/mod.rs
git commit -m "feat(collect): emit progress events from Config inspector

3 StepStarted/StepFinished pairs. First sub-step is 'Applying RPM
verification results' to accurately reflect reuse of RPM inspector's
rpm -Va output rather than re-running verification.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 7: Add Probe Events to Non-RPM Inspector

**Files:**
- Modify: `inspectah-collect/src/inspectors/nonrpm.rs`

- [ ] **Step 1: Write the failing test**

Test that Non-RPM emits `ProbeStarted`/`ProbeFinished` for each
ecosystem check. Test both the found and empty cases:

```rust
#[test]
fn test_nonrpm_emits_probe_events() {
    use inspectah_core::traits::progress::VecProgress;
    use inspectah_core::types::progress::{ProbeId, ProbeOutcome, ProgressEvent};

    let exec = /* mock with no ecosystems found */;
    let progress = VecProgress::new();
    // ... run inspect ...

    let events = progress.events();
    // All 7 probes must have started and finished
    let probe_starts: Vec<&ProbeId> = events
        .iter()
        .filter_map(|e| match e {
            ProgressEvent::ProbeStarted { probe, .. } => Some(probe),
            _ => None,
        })
        .collect();
    assert_eq!(probe_starts.len(), 7);

    // All should be Empty when nothing is found
    let empties: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, ProgressEvent::ProbeFinished {
            outcome: ProbeOutcome::Empty, ..
        }))
        .collect();
    assert_eq!(empties.len(), 7);
}

#[test]
fn test_nonrpm_probe_found_has_count() {
    // Mock with pip packages found
    // ... assert ProbeFinished { probe: ProbeId::PipPackages, outcome: ProbeOutcome::Found { count: N } }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p inspectah-collect inspectors::nonrpm::tests::test_nonrpm_emits_probe`
Expected: FAIL

- [ ] **Step 3: Add probe emissions to Non-RPM inspect()**

Rename `_progress` to `progress`. Wrap each ecosystem scan with
`ProbeStarted`/`ProbeFinished`:

```rust
// ELF binaries
progress.emit(ProgressEvent::ProbeStarted {
    inspector: InspectorId::NonRpmSoftware,
    probe: ProbeId::ElfBinaries,
});
let pre_count = section.items.len();
scan_dirs(exec, &mut section, has_readelf, has_file);
let found = section.items.len() - pre_count;
progress.emit(ProgressEvent::ProbeFinished {
    inspector: InspectorId::NonRpmSoftware,
    probe: ProbeId::ElfBinaries,
    outcome: if found > 0 {
        ProbeOutcome::Found { count: found }
    } else {
        ProbeOutcome::Empty
    },
});
```

Repeat for all 7 ecosystem checks: `scan_dirs`, `scan_python_venvs`,
`scan_pip_packages`, `scan_npm_packages`, `scan_gem_packages`,
`collect_env_files`, `collect_git_repos`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p inspectah-collect inspectors::nonrpm`
Expected: all tests pass

- [ ] **Step 5: Commit**

```bash
git add inspectah-collect/src/inspectors/nonrpm.rs
git commit -m "feat(collect): emit probe lifecycle events from Non-RPM inspector

7 ProbeStarted/ProbeFinished pairs, one per ecosystem check.
ProbeOutcome::Found carries count, ProbeOutcome::Empty for misses.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 8: ScanOutcome Enum and Exit Codes

**Files:**
- Modify: `inspectah-cli/src/commands/scan.rs`
- Modify: `inspectah-cli/src/main.rs`

- [ ] **Step 1: Define ScanOutcome in scan.rs**

```rust
/// Maps snapshot completeness to process exit semantics.
enum ScanOutcome {
    /// Exit 0 — report is trustworthy (all complete, or skipped/degraded only).
    Clean,
    /// Exit 0 — report is trustworthy but has caveats.
    Degraded,
    /// Exit 2 — report has blind spots (at least one inspector failed).
    Incomplete,
    /// Exit 130 — user interrupted with SIGINT.
    Interrupted,
}

impl ScanOutcome {
    fn from_completeness(completeness: &Completeness) -> Self {
        match completeness {
            Completeness::Complete => ScanOutcome::Clean,
            Completeness::Partial { .. } => ScanOutcome::Degraded,
            Completeness::Incomplete { .. } => ScanOutcome::Incomplete,
        }
    }

    fn exit_code(&self) -> i32 {
        match self {
            ScanOutcome::Clean | ScanOutcome::Degraded => 0,
            ScanOutcome::Incomplete => 2,
            ScanOutcome::Interrupted => 130,
        }
    }
}
```

- [ ] **Step 2: Change run_scan() to return ScanOutcome**

Change `pub fn run_scan(args: &ScanArgs) -> Result<()>` to return
`Result<ScanOutcome>`. After `collect()` returns, derive the outcome:

```rust
let outcome = ScanOutcome::from_completeness(&collected.state.snapshot.completeness);
```

Return `Ok(outcome)` at the end instead of `Ok(())`.

- [ ] **Step 3: Update main.rs to use exit codes**

```rust
match commands::scan::run_scan(&args) {
    Ok(outcome) => {
        let code = outcome.exit_code();
        if code != 0 {
            std::process::exit(code);
        }
    }
    Err(e) => {
        eprintln!("Error: {e:#}");
        std::process::exit(1);
    }
}
```

- [ ] **Step 4: Build and test**

Run: `cargo build && cargo test -p inspectah-cli`
Expected: compiles and tests pass

- [ ] **Step 5: Commit**

```bash
git add inspectah-cli/src/commands/scan.rs inspectah-cli/src/main.rs
git commit -m "feat(cli): add ScanOutcome enum and exit code mapping

Exit 0 for clean/degraded (trustworthy report), exit 2 for incomplete
(blind spots), exit 130 for SIGINT, exit 1 for hard errors.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 9: TerminalProgress Renderer — Flat Mode

Start with flat mode (simplest — no ANSI, no cursor). This validates
the rendering pipeline before adding complexity.

**Files:**
- Create: `inspectah-cli/src/progress/mod.rs`
- Create: `inspectah-cli/src/progress/flat.rs`
- Modify: `inspectah-cli/src/main.rs` or `inspectah-cli/src/lib.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::types::completeness::InspectorId;
    use inspectah_core::types::progress::*;

    #[test]
    fn flat_mode_renders_inspector_lifecycle() {
        let mut output = Vec::new();
        let renderer = FlatRenderer::new(&mut output, 11);

        renderer.handle(ProgressEvent::InspectorStarted(InspectorId::Rpm));
        renderer.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::Rpm,
            outcome: InspectorOutcome::Complete,
        });

        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("[1/11] RPM packages..."));
        assert!(text.contains("[1/11] RPM packages... done"));
    }
}
```

- [ ] **Step 2: Implement FlatRenderer**

A struct that wraps a `Write` implementor and formats events as flat
sequential lines. Each `InspectorStarted` prints `[N/total] Name...`.
Each `InspectorFinished` prints `[N/total] Name... done (Xs)`.
Sub-steps and probes print indented lines.

Inspector display names and numbering are derived from `InspectorId`
via a display-order lookup table.

- [ ] **Step 3: Run tests**

Run: `cargo test -p inspectah-cli progress::flat`
Expected: pass

- [ ] **Step 4: Commit**

```bash
git add inspectah-cli/src/progress/
git commit -m "feat(cli): add flat-mode progress renderer

Non-TTY rendering: sequential numbered lines with no ANSI codes.
Inspector names and display order derived from InspectorId.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 10: TerminalProgress Renderer — Plain Mode

**Files:**
- Create: `inspectah-cli/src/progress/plain.rs`

- [ ] **Step 1: Write the failing test**

Test that plain mode emits started lines (`▸ prefix`), separate done
lines (`✓ prefix`), and never uses `\r` or cursor-up sequences.

- [ ] **Step 2: Implement PlainRenderer**

Append-only output with ANSI color (when `NO_COLOR` is not set).
Static `▸` arrow for started lines. Same state symbols as rich mode
for completion lines. No animation, no overwriting.

Under concurrent wave-2 events, started and done lines interleave.
This is correct — the transcript is chronological.

- [ ] **Step 3: Run tests**

Run: `cargo test -p inspectah-cli progress::plain`
Expected: pass

- [ ] **Step 4: Commit**

```bash
git add inspectah-cli/src/progress/plain.rs
git commit -m "feat(cli): add plain-mode progress renderer

Append-only TTY rendering with ANSI color. Static arrow prefix for
started lines, separate done lines. No cursor manipulation.
Accessible, durable transcript for screen readers and multiplexers.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 11: TerminalProgress Renderer — Rich Mode

**Files:**
- Create: `inspectah-cli/src/progress/rich.rs`

- [ ] **Step 1: Implement RichRenderer**

Full block-redraw checklist renderer. Key behaviors:

- Track all inspector rows and their states in an in-memory model
- On each progress event or periodic tick (~100ms), cursor-up to
  block start and rewrite all lines
- Braille spinner animation cycling on tick
- Elapsed timer on active inspectors (>3-4s threshold)
- Terminal overflow: truncate pending items if block exceeds
  terminal height minus 2, show `... and N more`
- Final scrollback: print the completed checklist as permanent
  output when scan finishes (no cursor-up on final render)

The tick is driven by a background thread that sends a `Tick` signal
to the renderer. The renderer's `handle()` method processes both
`ProgressEvent` and `Tick` variants.

- [ ] **Step 2: Test the state model**

Test the in-memory state model independently of terminal output:
inspector state transitions, sub-step tracking, overflow calculation.

- [ ] **Step 3: Test final scrollback output**

Capture the final render and assert it matches expected checklist
format with correct symbols, counts, and timing.

- [ ] **Step 4: Run tests**

Run: `cargo test -p inspectah-cli progress::rich`
Expected: pass

- [ ] **Step 5: Commit**

```bash
git add inspectah-cli/src/progress/rich.rs
git commit -m "feat(cli): add rich-mode progress renderer

Block-redraw checklist with braille spinners, elapsed timers,
cursor-up refresh, terminal overflow handling, and clean final
scrollback artifact. 100ms background tick for animation.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 12: TerminalProgress Dispatcher + Mode Detection

**Files:**
- Modify: `inspectah-cli/src/progress/mod.rs`

- [ ] **Step 1: Implement TerminalProgress**

The top-level `ProgressSink` implementor that detects the rendering
mode and dispatches events to the appropriate renderer.

```rust
pub struct TerminalProgress {
    inner: Box<dyn Renderer>,
}

impl TerminalProgress {
    pub fn new() -> Self {
        let mode = detect_mode();
        let inner: Box<dyn Renderer> = match mode {
            Mode::Rich => Box::new(RichRenderer::new()),
            Mode::Plain => Box::new(PlainRenderer::new()),
            Mode::Flat => Box::new(FlatRenderer::new()),
        };
        Self { inner }
    }
}

impl ProgressSink for TerminalProgress {
    fn emit(&self, event: ProgressEvent) {
        self.inner.handle(event);
    }
}
```

Mode detection:
- `INSPECTAH_PROGRESS=plain` or `--progress=plain` → Plain
- `!is_terminal(stderr)` or `$TERM == dumb` → Flat
- Otherwise → Rich

`NO_COLOR` strips color in Rich and Plain modes but does not change
the rendering mode.

- [ ] **Step 2: Test mode detection**

Test with env var overrides: `INSPECTAH_PROGRESS=plain`,
`NO_COLOR=1`, `TERM=dumb`.

- [ ] **Step 3: Commit**

```bash
git add inspectah-cli/src/progress/mod.rs
git commit -m "feat(cli): add TerminalProgress dispatcher with mode detection

Detects rich/plain/flat mode from TTY state, INSPECTAH_PROGRESS env,
and TERM value. NO_COLOR strips color without changing mode.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 13: Completion Output

**Files:**
- Modify: `inspectah-cli/src/commands/scan.rs`

- [ ] **Step 1: Replace the existing completion output**

Replace:
```rust
eprintln!("Scanning host {hostname}... done");
// ...
eprintln!("Output written to {}", tarball_path.display());
eprintln!("To view and edit results, run: inspectah refine {}", tarball_path.display());
```

With completion output that respects `ScanOutcome`:

```rust
fn print_completion(
    outcome: &ScanOutcome,
    elapsed: std::time::Duration,
    snapshot: &InspectionSnapshot,
    output_path: Option<&Path>,
    inspect_only: bool,
) {
    let secs = elapsed.as_secs_f64();
    let counts = build_summary_counts(snapshot);

    match outcome {
        ScanOutcome::Clean => {
            eprintln!("Scan complete ({secs:.1}s) — {counts}");
        }
        ScanOutcome::Degraded => {
            let n = /* count degraded from completeness */;
            eprintln!("Scan complete ({secs:.1}s) — {counts}");
            eprintln!("  {n} degraded (see report for details)");
        }
        ScanOutcome::Incomplete => {
            let (nf, nd) = /* count failed + degraded */;
            eprintln!("Scan complete ({secs:.1}s) — {counts}");
            eprintln!("  {nf} failed, {nd} degraded (see report for details)");
        }
        ScanOutcome::Interrupted => {
            eprintln!("Scan interrupted after {secs:.1}s — (partial)");
            eprintln!("No report written.");
            return;
        }
    }

    if let Some(path) = output_path {
        if inspect_only {
            eprintln!("Output: {}", path.display());
        } else {
            eprintln!("Report: {}", path.display());
            eprintln!("To review: inspectah refine {}", path.display());
        }
    }
}
```

- [ ] **Step 2: Add a timer around the scan**

Wrap the scan section with `std::time::Instant::now()` and pass
elapsed to `print_completion`.

- [ ] **Step 3: Build and test**

Run: `cargo build && cargo test -p inspectah-cli`
Expected: compiles and tests pass

- [ ] **Step 4: Commit**

```bash
git add inspectah-cli/src/commands/scan.rs
git commit -m "feat(cli): add structured completion output with summary counts

Shows key counts, degraded/failed warnings, report path, and
copy-pasteable refine command. Adapts for --inspect-only, export
failure, and interrupted scan paths.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 14: Wire TerminalProgress into Scan Command

**Files:**
- Modify: `inspectah-cli/src/commands/scan.rs`

- [ ] **Step 1: Replace NullProgress with TerminalProgress**

```rust
use crate::progress::TerminalProgress;

// Replace:
let collected = collect(&source, &executor, &inspectors, baseline_data.as_ref(), &NullProgress);

// With:
let progress = TerminalProgress::new();
let collected = collect(&source, &executor, &inspectors, baseline_data.as_ref(), &progress);
```

- [ ] **Step 2: Remove old eprintln! progress lines**

Remove:
```rust
eprintln!("Scanning host {hostname}...");
// ...
eprintln!("Scanning host {hostname}... done");
```

The `TerminalProgress` renderer now handles all progress output.
The hostname should be passed to `TerminalProgress::new()` so it
can render the header line (`Inspecting host rhel9-web01...`).

- [ ] **Step 3: Manual test**

Build and run on a real system (or in a VM) to verify the progress
output looks correct:

```bash
cargo build
sudo ./target/debug/inspectah scan
```

Verify:
- All 11 inspectors appear in the checklist
- RPM shows 6 sub-steps
- Config shows 3 sub-steps
- Non-RPM shows discoveries only
- Completion summary shows counts and report path
- `inspectah refine <path>` hint is copy-pasteable

- [ ] **Step 4: Test flat mode**

```bash
sudo ./target/debug/inspectah scan 2>progress.log
cat progress.log
```

Verify flat mode output with numbered sequential lines.

- [ ] **Step 5: Test plain mode**

```bash
INSPECTAH_PROGRESS=plain sudo ./target/debug/inspectah scan
```

Verify append-only output with `▸`/`✓` prefixes.

- [ ] **Step 6: Commit**

```bash
git add inspectah-cli/src/commands/scan.rs
git commit -m "feat(cli): wire TerminalProgress into scan command

Replace silent scan with full progress checklist. Removes old
eprintln! progress lines. Three rendering modes active based
on terminal detection.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 15: Pull Viewport Dynamic Height

**Files:**
- Modify: `inspectah-cli/src/commands/pull_progress.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn viewport_height_scales_with_terminal() {
    assert_eq!(viewport_height(80), 24);  // 80 * 0.3 = 24 -> capped at 16
    assert_eq!(viewport_height(50), 15);  // 50 * 0.3 = 15
    assert_eq!(viewport_height(24), 8);   // 24 * 0.3 = 7.2 -> floored at 8
    assert_eq!(viewport_height(10), 8);   // 10 * 0.3 = 3 -> floored at 8
}
```

- [ ] **Step 2: Implement viewport_height()**

```rust
/// Dynamic viewport height: 30% of terminal rows, floor 8, cap 16.
pub fn viewport_height(terminal_rows: usize) -> usize {
    let height = (terminal_rows as f64 * 0.3).round() as usize;
    height.clamp(8, 16)
}
```

- [ ] **Step 3: Replace VIEWPORT_LINES constant**

Change `const VIEWPORT_LINES: usize = 3;` to use the new function.
In the TTY viewport rendering path, get terminal height and compute:

```rust
let (term_width, term_height) = terminal_size::terminal_size()
    .map(|(w, h)| (w.0 as usize, h.0 as usize))
    .unwrap_or((80, 24));
let viewport_lines = viewport_height(term_height);
```

Update the ring buffer size to match `viewport_lines` instead of
the hardcoded 3. This may require changing the ring buffer from a
fixed-size array to a `Vec`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p inspectah-cli pull_progress`
Expected: pass

- [ ] **Step 5: Commit**

```bash
git add inspectah-cli/src/commands/pull_progress.rs
git commit -m "feat(cli): dynamic pull viewport height

30% of terminal height, floor 8 rows, cap 16. Replaces hardcoded
3-line viewport. Non-TTY skips viewport entirely.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 16: SIGINT Cancellation Token

**Files:**
- Modify: `inspectah-cli/src/commands/scan.rs`
- Modify: `inspectah-pipeline/src/collect.rs`

- [ ] **Step 1: Add cancellation token parameter to collect()**

```rust
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub fn collect(
    source: &SourceSystem,
    executor: &dyn Executor,
    inspectors: &[Box<dyn Inspector>],
    baseline: Option<&BaselineData>,
    progress: &dyn ProgressSink,
    cancelled: &AtomicBool,
) -> Pipeline<Collected> {
```

Check `cancelled` before wave-2 launch and before each inspector
spawn. If set, skip remaining inspectors.

After joining each handle, if `cancelled` was set before the handle
completed, discard the result.

- [ ] **Step 2: Install SIGINT handler in scan command**

```rust
let cancelled = Arc::new(AtomicBool::new(false));
let cancelled_clone = cancelled.clone();
ctrlc::set_handler(move || {
    cancelled_clone.store(true, Ordering::SeqCst);
}).expect("failed to install SIGINT handler");
```

Add `ctrlc = "3"` to `inspectah-cli/Cargo.toml` dependencies.

Pass `&cancelled` to `collect()`.

After `collect()` returns, if `cancelled` is set, return
`Ok(ScanOutcome::Interrupted)`.

- [ ] **Step 3: Emit Interrupted events for skipped inspectors**

After `collect()` returns, the CLI emits `InspectorFinished` with
`InspectorOutcome::Interrupted` for any inspectors that were not
started or whose results were discarded.

- [ ] **Step 4: Update all test call sites**

Add `&AtomicBool::new(false)` to every `collect()` call in tests.

- [ ] **Step 5: Build and test**

Run: `cargo build && cargo test`
Expected: pass

- [ ] **Step 6: Commit**

```bash
git add inspectah-cli/Cargo.toml inspectah-cli/src/commands/scan.rs inspectah-pipeline/src/collect.rs
git commit -m "feat(cli): add SIGINT cancellation with exit code 130

CLI owns the signal handler, passes AtomicBool token to collect().
Completed results before SIGINT are kept, in-flight discarded.
Interrupted inspectors get InspectorFinished::Interrupted events.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Self-Review Checklist

Spec coverage verified against `2026-05-24-cli-scan-progress-design.md`:

- [x] Full checklist with 11 inspectors — Task 9-12, 14
- [x] Nested sub-checklists: RPM (6) — Task 5, Config (3) — Task 6, Non-RPM (discoveries) — Task 7
- [x] Visual states: pending/active/complete/skipped/degraded/failed/interrupted — Tasks 1, 9-11
- [x] Three rendering modes: rich/plain/flat — Tasks 9-12
- [x] `NO_COLOR` strips color only — Task 12
- [x] Typed ProgressEvent model — Task 1
- [x] ProgressSink: Send + Sync — Task 2
- [x] Inspector trait change — Task 3
- [x] collect() lifecycle events — Task 4
- [x] Exit codes: 0/1/2/130 — Task 8
- [x] Completion output for all paths — Task 13
- [x] Pull viewport dynamic height — Task 15
- [x] SIGINT cancellation — Task 16
- [x] Two-wave parallel model acknowledged — Task 4, 11, 12
- [x] Reason strings on degraded/failed/skipped — Task 1 (types), Task 4 (emission)
- [x] Non-RPM per-mode probe behavior — Task 7 (events), Tasks 9-11 (rendering)
- [x] Periodic tick for rich mode — Task 11
