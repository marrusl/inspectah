# CLI Scan Progress UX Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the silent `inspectah scan` with a full progress checklist showing inspector states, sub-step detail, and a confidence-building completion summary.

**Architecture:** New `ProgressEvent` / `ProgressSink` types in inspectah-core define a typed event contract. `collect()` in inspectah-pipeline emits inspector lifecycle events through the sink. Three inspectors (RPM, Config, Non-RPM) emit sub-step events internally. The CLI provides a `TerminalProgress` renderer with three modes (rich/plain/flat). Exit codes reflect report trustworthiness via a `ScanOutcome` enum.

**Tech Stack:** Rust (edition 2024), existing workspace crates (inspectah-core, inspectah-collect, inspectah-pipeline, inspectah-cli). `terminal_size` (already a dep), `std::sync::atomic` for cancellation, `std::thread` for render tick.

**Spec:** `docs/specs/proposed/2026-05-24-cli-scan-progress-design.md` (approved revision 4)

**Implementer:** Tang (Rust Systems Engineer). All tasks are Tang's
lane — this is pure Rust/CLI work across inspectah-core,
inspectah-collect, inspectah-pipeline, and inspectah-cli.

**Dispatch rule:** When dispatching Tang for any task, READ the full
contents of `team/tang.md` AND `team/context.md` and include them in
the agent prompt. Do not summarize or reference by name only. Tang's
persona defines code standards, clippy rules, testing strategy, and
architectural decision-making rules that govern implementation.

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

### Task 4: Align Collector Wave Model to Spec

**Files:**
- Modify: `inspectah-pipeline/src/collect.rs`

The approved spec says wave 1 is RPM alone, wave 2 is all other
inspectors. The current `is_wave2()` classifier puts only
ScheduledTasks, Config, Selinux, and NonRpmSoftware in wave 2 —
everything else (Services, Storage, Kernel, Network, Containers,
Users) runs alongside RPM in wave 1. This is a spec mismatch.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn test_wave_partition_rpm_alone_in_wave1() {
    // All non-RPM inspectors must be wave 2
    assert!(is_wave2(InspectorId::Services));
    assert!(is_wave2(InspectorId::Storage));
    assert!(is_wave2(InspectorId::KernelBoot));
    assert!(is_wave2(InspectorId::Network));
    assert!(is_wave2(InspectorId::Containers));
    assert!(is_wave2(InspectorId::UsersGroups));
    assert!(is_wave2(InspectorId::ScheduledTasks));
    assert!(is_wave2(InspectorId::Config));
    assert!(is_wave2(InspectorId::Selinux));
    assert!(is_wave2(InspectorId::NonRpmSoftware));

    // Only RPM is wave 1
    assert!(!is_wave2(InspectorId::Rpm));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p inspectah-pipeline test_wave_partition_rpm_alone`
Expected: FAIL — Services, Storage, etc. are currently wave 1

- [ ] **Step 3: Change is_wave2() to make RPM the only wave-1 inspector**

```rust
fn is_wave2(id: InspectorId) -> bool {
    !matches!(id, InspectorId::Rpm)
}
```

- [ ] **Step 4: Update the existing is_wave2 classifier test**

Replace the old `test_is_wave2_classifier` test with the new
assertions from step 1. Remove the assertions that Services,
Network, Storage, etc. are wave-1.

- [ ] **Step 5: Run all tests**

Run: `cargo test -p inspectah-pipeline`
Expected: all tests pass. Wave-2 inspectors that don't need
`rpm_state` (Services, Storage, etc.) will now receive
`rpm_state: Some(...)` — this is correct, they simply ignore it.

- [ ] **Step 6: Commit**

```bash
git add inspectah-pipeline/src/collect.rs
git commit -m "feat(pipeline): RPM alone in wave 1, all others wave 2

Aligns collector wave model to approved scan progress spec.
Non-RPM-dependent inspectors now also run in wave 2, receiving
enriched context with rpm_state. They ignore it — no behavior change.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 5: Emit Inspector Lifecycle Events from collect()

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

### Task 6: Add Progress Events to RPM Inspector

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

- [ ] **Step 4: Write degraded sub-step test**

Test that when `dnf` is unavailable (dep tree resolution degrades),
the `ResolvingDepTree` step emits `StepFinished` with
`StepOutcome::Degraded { reason }` rather than `Complete`:

```rust
#[test]
fn test_rpm_degraded_dep_tree_emits_degraded_step() {
    // Build mock where dnf repoquery --userinstalled fails
    let exec = build_test_mock()
        .with_command(
            "dnf repoquery --userinstalled --queryformat %{name}.%{arch}\n",
            ExecResult { exit_code: 1, ..Default::default() },
        );
    let progress = VecProgress::new();
    // ... run inspect ...

    let events = progress.events();
    assert!(events.iter().any(|e| matches!(
        e,
        ProgressEvent::StepFinished {
            step: StepId::ResolvingDepTree,
            outcome: StepOutcome::Degraded { .. },
            ..
        }
    )));
}
```

- [ ] **Step 5: Add degraded emission to RPM sub-steps**

In the dep-tree classification code, when `classify_leaf_auto` returns
a degraded result (leaf_packages is None), emit:

```rust
progress.emit(ProgressEvent::StepFinished {
    inspector: inspector_id,
    step: StepId::ResolvingDepTree,
    outcome: StepOutcome::Degraded {
        reason: "dnf unavailable, dependency tree incomplete".into(),
    },
});
```

Apply the same pattern to other sub-steps that can degrade:
- `ResolvingSourceRepos` — when dnf repoquery fails and falls back to rpm -qi
- `VerifyingIntegrity` — when rpm -Va is not available

- [ ] **Step 6: Run tests**

Run: `cargo test -p inspectah-collect inspectors::rpm`
Expected: all tests pass including happy-path and degraded-path tests

- [ ] **Step 7: Commit**

```bash
git add inspectah-collect/src/inspectors/rpm/mod.rs
git commit -m "feat(collect): emit progress events from RPM inspector

6 StepStarted/StepFinished pairs covering package query, classify,
repo resolution, dep tree, integrity verification, file ownership.
PackagesFound and ReposMapped metrics.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 7: Add Progress Events to Config Inspector

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

### Task 8: Add Probe Events to Non-RPM Inspector

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

### Task 9: ScanOutcome Enum and Exit Codes

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

### Task 10: TerminalProgress Renderer — Flat Mode

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

### Task 11: TerminalProgress Renderer — Plain Mode

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

### Task 12: TerminalProgress Renderer — Rich Mode

**Files:**
- Create: `inspectah-cli/src/progress/rich.rs`

- [ ] **Step 1: Define the state model (separate from rendering)**

```rust
/// In-memory state model for the checklist. Updated by events,
/// read by the renderer. All access goes through a Mutex.
struct ChecklistState {
    inspectors: Vec<InspectorRow>,
    scan_start: Instant,
}

struct InspectorRow {
    id: InspectorId,
    display_name: &'static str,
    display_order: usize,
    state: RowState,
    started_at: Option<Instant>,
    sub_steps: Vec<SubStepRow>,  // RPM/Config: populated upfront
    probes: Vec<ProbeRow>,       // Non-RPM: populated on discovery
}

enum RowState {
    Pending,
    Active,
    Complete { detail: String, elapsed: Duration },
    Skipped { reason: String },
    Degraded { reason: String, detail: String, elapsed: Duration },
    Failed { reason: String },
    Interrupted,
}
```

- [ ] **Step 2: Implement the concurrency model**

```rust
/// RichRenderer owns a Mutex<ChecklistState> and a Mutex<Stderr>.
/// Two sources write to it:
/// 1. Inspector threads (via ProgressSink::emit) — update state
/// 2. Tick thread — triggers redraw
///
/// Locking order (must always acquire in this order):
/// 1. state_lock (Mutex<ChecklistState>)
/// 2. stderr_lock (Mutex<Stderr>)
///
/// The emit() method: lock state, update, lock stderr, redraw, unlock both.
/// The tick thread:   lock state (read), lock stderr, redraw, unlock both.
///
/// This is safe because both paths acquire locks in the same order.
struct RichRenderer {
    state: Mutex<ChecklistState>,
    stderr: Mutex<std::io::Stderr>,
    use_color: bool,
    tick_handle: Option<std::thread::JoinHandle<()>>,
    stop_tick: Arc<AtomicBool>,
}
```

The tick thread runs a loop: sleep 100ms, check `stop_tick`, lock
state + stderr, call `redraw()`. When `finalize()` is called (scan
complete), set `stop_tick`, join the tick thread, print the final
scrollback render.

- [ ] **Step 3: Implement block redraw**

The `redraw()` method:
1. Read `ChecklistState`
2. Compute block height (inspectors + expanded sub-steps/probes)
3. If block > terminal height - 2, truncate pending items
4. Cursor-up to block start (`\x1b[{n}A`)
5. Print all lines (clear each line first with `\x1b[2K`)
6. Cursor at bottom of block

Elapsed timer: for active rows where `started_at.elapsed() > 3.5s`,
append `(Ns)` to the line.

Spinner: cycle through braille frames `['⠋','⠙','⠹','⠸','⠼','⠴','⠦','⠧','⠇','⠏']`
based on `tick_count % 10`.

- [ ] **Step 4: Implement finalize**

```rust
fn finalize(&mut self) {
    // Stop tick thread
    self.stop_tick.store(true, Ordering::SeqCst);
    if let Some(handle) = self.tick_handle.take() {
        handle.join().ok();
    }
    // Final scrollback: print completed state as permanent output
    // No cursor-up — this is the durable artifact
    let state = self.state.lock().unwrap();
    let mut stderr = self.stderr.lock().unwrap();
    // Clear the in-progress block first (cursor-up + clear lines)
    // Then print final state
    render_final(&state, &mut stderr, self.use_color);
}
```

- [ ] **Step 5: Test the state model (no terminal)**

```rust
#[test]
fn test_state_model_transitions() {
    let mut state = ChecklistState::new(/* 11 inspectors */);
    state.handle_event(ProgressEvent::InspectorStarted(InspectorId::Rpm));
    assert!(matches!(state.inspectors[0].state, RowState::Active));

    state.handle_event(ProgressEvent::InspectorFinished {
        id: InspectorId::Rpm,
        outcome: InspectorOutcome::Complete,
    });
    assert!(matches!(state.inspectors[0].state, RowState::Complete { .. }));
}

#[test]
fn test_overflow_truncates_pending() {
    let state = ChecklistState::new(/* 11 inspectors */);
    // With terminal height 10 and 11 inspectors + sub-steps,
    // overflow should hide pending items
    let lines = state.render_lines(10);
    assert!(lines.last().unwrap().contains("... and"));
}
```

- [ ] **Step 6: Test final scrollback output**

Capture the final render into a `Vec<u8>` and assert format.

- [ ] **Step 7: Run tests**

Run: `cargo test -p inspectah-cli progress::rich`
Expected: pass

- [ ] **Step 8: Commit**

```bash
git add inspectah-cli/src/progress/rich.rs
git commit -m "feat(cli): add rich-mode progress renderer

Block-redraw checklist with Mutex<State> + Mutex<Stderr> locking
model. Braille spinners, elapsed timers, cursor-up refresh,
terminal overflow, and clean final scrollback artifact. 100ms
background tick thread with stop flag.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 13: TerminalProgress Dispatcher + Mode Detection + CLI Flag

**Files:**
- Modify: `inspectah-cli/src/progress/mod.rs`
- Modify: `inspectah-cli/src/commands/scan.rs` (add `--progress` flag)
- Modify: `inspectah-cli/src/commands/pull_progress.rs` (respect mode)

- [ ] **Step 1: Add --progress flag to ScanArgs**

```rust
/// Progress display mode: rich (default TTY), plain (durable), flat (non-TTY)
#[arg(long, value_name = "MODE")]
pub progress: Option<ProgressMode>,

#[derive(Clone, Debug, clap::ValueEnum)]
pub enum ProgressMode {
    Rich,
    Plain,
    Flat,
}
```

- [ ] **Step 2: Implement mode detection with flag + env + TTY**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Rich,
    Plain,
    Flat,
}

/// Resolve rendering mode from CLI flag, env var, and TTY detection.
/// Priority: --progress flag > INSPECTAH_PROGRESS env > auto-detect.
pub fn detect_mode(cli_flag: Option<&ProgressMode>) -> Mode {
    // CLI flag takes precedence
    if let Some(flag) = cli_flag {
        return match flag {
            ProgressMode::Rich => Mode::Rich,
            ProgressMode::Plain => Mode::Plain,
            ProgressMode::Flat => Mode::Flat,
        };
    }

    // Env var
    if let Ok(val) = std::env::var("INSPECTAH_PROGRESS") {
        return match val.to_lowercase().as_str() {
            "plain" => Mode::Plain,
            "flat" => Mode::Flat,
            "rich" => Mode::Rich,
            _ => Mode::Rich,  // unknown value → default
        };
    }

    // Auto-detect from TTY
    let is_tty = std::io::IsTerminal::is_terminal(&std::io::stderr());
    let is_dumb = std::env::var("TERM")
        .map(|t| t == "dumb")
        .unwrap_or(false);

    if !is_tty || is_dumb {
        Mode::Flat
    } else {
        Mode::Rich
    }
}

/// Whether to use ANSI color (independent of mode).
pub fn use_color() -> bool {
    std::env::var("NO_COLOR").is_err()
}
```

- [ ] **Step 3: Implement TerminalProgress**

```rust
pub struct TerminalProgress {
    inner: Box<dyn Renderer + Send + Sync>,
}

impl TerminalProgress {
    pub fn new(mode: Mode, use_color: bool) -> Self {
        let inner: Box<dyn Renderer + Send + Sync> = match mode {
            Mode::Rich => Box::new(RichRenderer::new(use_color)),
            Mode::Plain => Box::new(PlainRenderer::new(use_color)),
            Mode::Flat => Box::new(FlatRenderer::new()),
        };
        Self { inner }
    }

    /// Expose the resolved mode so pull_progress can use the same decision.
    pub fn mode(&self) -> Mode { /* stored during construction */ }
}

impl ProgressSink for TerminalProgress {
    fn emit(&self, event: ProgressEvent) {
        self.inner.handle(event);
    }
}
```

- [ ] **Step 4: Thread mode into pull_progress**

The resolved `Mode` from scan must also govern pull viewport behavior:
- `Mode::Rich` → TTY viewport with dynamic height
- `Mode::Plain` → sequential pull lines (no viewport)
- `Mode::Flat` → sequential pull lines (no viewport)

Pass `mode` to the pull-progress rendering path so `--progress=plain`
disables the pull viewport consistently with scan progress.

- [ ] **Step 5: Test mode detection**

```rust
#[test]
fn test_mode_detection_cli_flag_overrides_env() {
    std::env::set_var("INSPECTAH_PROGRESS", "flat");
    let mode = detect_mode(Some(&ProgressMode::Plain));
    assert_eq!(mode, Mode::Plain);
    std::env::remove_var("INSPECTAH_PROGRESS");
}

#[test]
fn test_mode_detection_env_overrides_tty() {
    std::env::set_var("INSPECTAH_PROGRESS", "plain");
    let mode = detect_mode(None);
    assert_eq!(mode, Mode::Plain);
    std::env::remove_var("INSPECTAH_PROGRESS");
}
```

- [ ] **Step 6: Commit**

```bash
git add inspectah-cli/src/progress/mod.rs inspectah-cli/src/commands/scan.rs inspectah-cli/src/commands/pull_progress.rs
git commit -m "feat(cli): add --progress flag and unified mode detection

CLI flag > INSPECTAH_PROGRESS env > TTY auto-detect. Mode governs
both scan progress and pull viewport rendering. NO_COLOR is
independent of mode — strips color only.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 14: Completion Output

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
            let partial = build_summary_counts(snapshot);
            if partial.is_empty() {
                eprintln!("Scan interrupted after {secs:.1}s");
            } else {
                eprintln!("Scan interrupted after {secs:.1}s — {partial} (partial)");
            }
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

### Task 15: Wire TerminalProgress into Scan Command

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

### Task 16: Pull Viewport Dynamic Height

**Files:**
- Modify: `inspectah-cli/src/commands/pull_progress.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn viewport_height_scales_with_terminal() {
    assert_eq!(viewport_height(80), 16);  // 80 * 0.3 = 24 -> capped at 16
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

### Task 17: SIGINT Cancellation Token

**Files:**
- Modify: `inspectah-cli/src/commands/scan.rs`
- Modify: `inspectah-pipeline/src/collect.rs`
- Modify: `inspectah-cli/Cargo.toml`

The approved spec requires: CLI owns signal handler, `collect()`
receives cancellation token, completed results before SIGINT are
kept, in-flight results are discarded. The cutoff rule must be
deterministic: a result is "completed before cancel" if and only if
its thread's `join()` returned `Ok` AND `cancelled` was `false` at
the moment `join()` returned.

- [ ] **Step 1: Add cancellation token to collect() signature**

```rust
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

- [ ] **Step 2: Add cancellation checks to wave execution**

```rust
// Between wave 1 and wave 2:
if cancelled.load(Ordering::SeqCst) {
    // Skip wave 2 entirely. Emit Interrupted for all wave-2 inspectors.
    for insp in &wave2 {
        progress.emit(ProgressEvent::InspectorFinished {
            id: insp.id(),
            outcome: InspectorOutcome::Interrupted,
        });
    }
    // Jump to completeness computation
} else {
    // Wave 2: spawn all, then join
    std::thread::scope(|s| {
        let handles: Vec<_> = wave2
            .iter()
            .map(|inspector| {
                progress.emit(ProgressEvent::InspectorStarted(inspector.id()));
                s.spawn(|| inspector.inspect(&enriched_ctx, progress))
            })
            .collect();

        for (inspector, handle) in wave2.iter().zip(handles) {
            let result = handle.join();
            // Cutoff rule: check cancelled AFTER join returns.
            // If cancelled is true, discard this result regardless
            // of whether the thread finished "in time."
            if cancelled.load(Ordering::SeqCst) {
                progress.emit(ProgressEvent::InspectorFinished {
                    id: inspector.id(),
                    outcome: InspectorOutcome::Interrupted,
                });
                continue; // don't route to snapshot
            }
            // Not cancelled: handle normally
            handle_result(
                inspector.as_ref(), result,
                &mut snapshot, &mut failed, &mut degraded,
                &mut wave2_rpm, progress,
            );
        }
    });
}
```

**Why check after join, not before spawn:** Threads are already
running via `thread::scope`. We can't un-spawn them. The only
reliable cutoff is at join time. If `cancelled` is true when we
join a handle, we discard that result even if the thread finished
before the signal — this is simpler and deterministic. The cost is
potentially discarding one or two already-complete results, which
is acceptable for a SIGINT path.

- [ ] **Step 3: Install SIGINT handler in scan command**

```rust
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

let cancelled = Arc::new(AtomicBool::new(false));
let cancelled_clone = cancelled.clone();
ctrlc::set_handler(move || {
    cancelled_clone.store(true, Ordering::SeqCst);
}).expect("failed to install SIGINT handler");
```

Add `ctrlc = "3"` to `inspectah-cli/Cargo.toml`.

After `collect()` returns, check `cancelled`:
```rust
if cancelled.load(Ordering::SeqCst) {
    print_completion(&ScanOutcome::Interrupted, elapsed, &snapshot, None, false);
    return Ok(ScanOutcome::Interrupted);
}
```

- [ ] **Step 4: Write cancellation test**

```rust
#[test]
fn test_collect_respects_cancellation_between_waves() {
    let exec = build_test_mock();
    let source = SourceSystem::PackageBased {
        os_release: test_os_release(),
    };
    let cancelled = AtomicBool::new(false);
    let progress = VecProgress::new();

    // Set cancelled before wave 2 would run
    // (RPM is wave 1 — it will complete; wave 2 should be skipped)
    // Use a mock inspector that sets cancelled during wave 1
    struct CancellingRpm {
        flag: *const AtomicBool,
    }
    unsafe impl Send for CancellingRpm {}
    unsafe impl Sync for CancellingRpm {}
    impl Inspector for CancellingRpm {
        fn id(&self) -> InspectorId { InspectorId::Rpm }
        fn applicable_to(&self) -> &[SourceSystemKind] {
            &[SourceSystemKind::PackageBased]
        }
        fn inspect(&self, ctx: &InspectionContext<'_>, _progress: &dyn ProgressSink)
            -> Result<InspectorOutput, InspectorError>
        {
            // Set cancel flag during RPM execution
            unsafe { &*self.flag }.store(true, Ordering::SeqCst);
            // Return a valid RPM output
            RpmInspector::new().inspect(ctx, _progress)
        }
    }

    let cancelling = CancellingRpm { flag: &cancelled };
    let inspectors: Vec<Box<dyn Inspector>> = vec![
        Box::new(cancelling),
        Box::new(ServicesInspector::new()),
    ];
    let pipeline = collect(&source, &exec, &inspectors, None, &progress, &cancelled);

    // RPM should have completed (wave 1)
    assert!(pipeline.state.snapshot.rpm.is_some());

    // Services should NOT have run (wave 2 skipped)
    let events = progress.events();
    assert!(events.iter().any(|e| matches!(
        e,
        ProgressEvent::InspectorFinished {
            id: InspectorId::Services,
            outcome: InspectorOutcome::Interrupted,
        }
    )));
}
```

- [ ] **Step 5: Update all test call sites**

Add `&AtomicBool::new(false)` to every `collect()` call in tests.

- [ ] **Step 6: Build and test**

Run: `cargo build && cargo test`
Expected: pass

- [ ] **Step 7: Commit**

```bash
git add inspectah-cli/Cargo.toml inspectah-cli/src/commands/scan.rs inspectah-pipeline/src/collect.rs
git commit -m "feat(cli): add SIGINT cancellation with exit code 130

CLI owns the signal handler, passes AtomicBool token to collect().
Deterministic cutoff: check cancelled after join(), discard if set.
Wave-2 skipped entirely if cancelled between waves.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Self-Review Checklist

Spec coverage verified against `2026-05-24-cli-scan-progress-design.md`:

- [x] Full checklist with 11 inspectors — Tasks 10-13, 15
- [x] Nested sub-checklists: RPM (6) — Task 6, Config (3) — Task 7, Non-RPM (discoveries) — Task 8
- [x] Visual states: pending/active/complete/skipped/degraded/failed/interrupted — Tasks 1, 6-7 (degraded sub-steps), 10-12
- [x] Three rendering modes: rich/plain/flat — Tasks 10-13
- [x] `NO_COLOR` strips color only — Task 13
- [x] Typed ProgressEvent model — Task 1
- [x] ProgressSink: Send + Sync — Task 2
- [x] Inspector trait change — Task 3
- [x] Wave model: RPM alone in wave 1 — Task 4
- [x] collect() lifecycle events — Task 5
- [x] Exit codes: 0/1/2/130 — Task 9
- [x] Completion output for all paths including interrupted partial counts — Task 14
- [x] Pull viewport dynamic height — Task 16
- [x] SIGINT cancellation with deterministic cutoff — Task 17
- [x] --progress CLI flag threaded through scan and pull-progress — Task 13
- [x] Reason strings on degraded/failed/skipped — Task 1 (types), Task 5 (emission)
- [x] Non-RPM per-mode probe behavior — Task 8 (events), Tasks 10-12 (rendering)
- [x] Periodic tick for rich mode — Task 12
- [x] Rich mode concurrency: Mutex<State> + Mutex<Stderr> with defined lock order — Task 12
- [x] Sub-step degraded/failed tests for RPM — Task 6
- [x] Viewport test assertions match spec (cap 16) — Task 16
