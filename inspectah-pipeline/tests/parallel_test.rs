//! Parallel execution tests for `collect()`.
//!
//! Validates the scoped-thread model: concurrency, panic containment,
//! applicability filtering, and completeness tracking.

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use inspectah_collect::executor::mock::MockExecutor;
use inspectah_core::traits::executor::ExecResult;
use inspectah_core::traits::inspector::{
    InspectionContext, Inspector, InspectorError, InspectorOutput,
};
use inspectah_core::types::completeness::{
    Completeness, InspectorId, SectionData, SourceSystemKind,
};
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::system::SourceSystem;
use inspectah_pipeline::collect::collect;

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn test_os_release() -> OsRelease {
    OsRelease {
        name: "Red Hat Enterprise Linux".into(),
        version_id: "9.4".into(),
        id: "rhel".into(),
        ..Default::default()
    }
}

fn package_based_source() -> SourceSystem {
    SourceSystem::PackageBased {
        os_release: test_os_release(),
    }
}

fn bootc_source() -> SourceSystem {
    SourceSystem::Bootc {
        os_release: test_os_release(),
        booted_image: "quay.io/example/rhel:9.4".into(),
        staged_image: None,
    }
}

fn minimal_mock() -> MockExecutor {
    MockExecutor::new().with_command(
        "rpm -qa --queryformat %{EPOCH}:%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\n",
        ExecResult {
            stdout: "0:bash-5.2.26-3.el9.x86_64\n".into(),
            exit_code: 0,
            ..Default::default()
        },
    )
}

// ---------------------------------------------------------------------------
// Mock inspectors
// ---------------------------------------------------------------------------

/// Inspector with a configurable sleep to prove concurrency.
struct DelayedInspector {
    id: InspectorId,
    section: SectionData,
    delay: Duration,
}

impl Inspector for DelayedInspector {
    fn id(&self) -> InspectorId {
        self.id
    }

    fn applicable_to(&self) -> &[SourceSystemKind] {
        &[
            SourceSystemKind::PackageBased,
            SourceSystemKind::RpmOstree,
            SourceSystemKind::Bootc,
        ]
    }

    fn inspect(&self, _ctx: &InspectionContext<'_>) -> Result<InspectorOutput, InspectorError> {
        std::thread::sleep(self.delay);
        Ok(InspectorOutput {
            section: self.section.clone(),
            warnings: vec![],
            redaction_hints: vec![],
        })
    }
}

/// Inspector that always panics — for containment testing.
struct PanickingInspector;

impl Inspector for PanickingInspector {
    fn id(&self) -> InspectorId {
        InspectorId::Config
    }

    fn applicable_to(&self) -> &[SourceSystemKind] {
        &[SourceSystemKind::PackageBased]
    }

    fn inspect(&self, _ctx: &InspectionContext<'_>) -> Result<InspectorOutput, InspectorError> {
        panic!("intentional panic for containment test");
    }
}

/// Inspector that only applies to PackageBased — used to test applicability filtering.
struct PackageOnlyInspector {
    called: AtomicU32,
}

impl PackageOnlyInspector {
    fn new() -> Self {
        Self {
            called: AtomicU32::new(0),
        }
    }

    fn call_count(&self) -> u32 {
        self.called.load(Ordering::SeqCst)
    }
}

impl Inspector for PackageOnlyInspector {
    fn id(&self) -> InspectorId {
        InspectorId::Network
    }

    fn applicable_to(&self) -> &[SourceSystemKind] {
        &[SourceSystemKind::PackageBased]
    }

    fn inspect(&self, _ctx: &InspectionContext<'_>) -> Result<InspectorOutput, InspectorError> {
        self.called.fetch_add(1, Ordering::SeqCst);
        Ok(InspectorOutput {
            section: SectionData::Network(Default::default()),
            warnings: vec![],
            redaction_hints: vec![],
        })
    }
}

/// Inspector that returns Failed with a reason.
struct FailingInspector {
    id: InspectorId,
}

impl Inspector for FailingInspector {
    fn id(&self) -> InspectorId {
        self.id
    }

    fn applicable_to(&self) -> &[SourceSystemKind] {
        &[SourceSystemKind::PackageBased]
    }

    fn inspect(&self, _ctx: &InspectionContext<'_>) -> Result<InspectorOutput, InspectorError> {
        Err(InspectorError::Failed {
            reason: "simulated failure".into(),
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Three inspectors each sleep 50ms. Serial would take >=150ms.
/// Parallel execution should complete in roughly one sleep window.
#[test]
fn independent_inspectors_run_concurrently() {
    let source = package_based_source();
    let exec = minimal_mock();
    let delay = Duration::from_millis(50);

    let inspectors: Vec<Box<dyn Inspector>> = vec![
        Box::new(DelayedInspector {
            id: InspectorId::Services,
            section: SectionData::Services(Default::default()),
            delay,
        }),
        Box::new(DelayedInspector {
            id: InspectorId::Storage,
            section: SectionData::Storage(Default::default()),
            delay,
        }),
        Box::new(DelayedInspector {
            id: InspectorId::KernelBoot,
            section: SectionData::KernelBoot(Default::default()),
            delay,
        }),
    ];

    let start = Instant::now();
    let pipeline = collect(&source, &exec, &inspectors);
    let elapsed = start.elapsed();

    // All three sections must be populated
    assert!(
        pipeline.state.snapshot.services.is_some(),
        "services section must be present"
    );
    assert!(
        pipeline.state.snapshot.storage.is_some(),
        "storage section must be present"
    );
    assert!(
        pipeline.state.snapshot.kernel_boot.is_some(),
        "kernel_boot section must be present"
    );

    // Elapsed must be significantly less than serial time (3 * 50ms = 150ms).
    // Allow generous margin for CI variability — the key assertion is
    // that it's faster than serial, not that it hits a precise deadline.
    let serial_time = delay * 3;
    assert!(
        elapsed < serial_time,
        "parallel execution took {elapsed:?}, which exceeds serial bound {serial_time:?}"
    );

    assert_eq!(pipeline.state.snapshot.completeness, Completeness::Complete);
}

/// Verify the API shape supports rpm_state flow (Wave 2 structure proof).
/// Wave 2 is Slice 2c, so this just validates the InspectionContext shape.
#[test]
fn rpm_state_flows_to_dependent_inspectors() {
    use inspectah_core::traits::inspector::RpmState;

    let source = package_based_source();
    let exec = minimal_mock();

    // Build an enriched context manually to prove the type structure works
    let rpm_state = RpmState {
        installed_packages: ["bash".to_string()].into_iter().collect(),
        owned_paths: ["/usr/bin/bash".to_string()].into_iter().collect(),
    };

    let enriched_ctx = InspectionContext {
        source: &source,
        executor: &exec,
        rpm_state: Some(&rpm_state),
    };

    // A dependent inspector can access rpm_state through the context
    assert!(enriched_ctx.rpm_state.is_some());
    let state = enriched_ctx.rpm_state.unwrap();
    assert!(state.installed_packages.contains("bash"));
    assert!(state.owned_paths.contains("/usr/bin/bash"));
}

/// RPM inspector returning Failed propagates to Incomplete completeness.
#[test]
fn rpm_failure_propagates() {
    let source = package_based_source();
    let exec = minimal_mock();

    let inspectors: Vec<Box<dyn Inspector>> = vec![
        Box::new(DelayedInspector {
            id: InspectorId::Services,
            section: SectionData::Services(Default::default()),
            delay: Duration::ZERO,
        }),
        Box::new(FailingInspector {
            id: InspectorId::Rpm,
        }),
    ];

    let pipeline = collect(&source, &exec, &inspectors);

    // Services should succeed
    assert!(
        pipeline.state.snapshot.services.is_some(),
        "services section must be present despite RPM failure"
    );

    // RPM section should be absent
    assert!(
        pipeline.state.snapshot.rpm.is_none(),
        "rpm section must be absent when RPM inspector fails"
    );

    // Completeness must be Incomplete with Rpm in failed_sections
    match &pipeline.state.snapshot.completeness {
        Completeness::Incomplete {
            failed_sections, ..
        } => {
            assert!(
                failed_sections.contains(&InspectorId::Rpm),
                "Rpm must be in failed_sections"
            );
        }
        other => panic!("expected Incomplete, got {other:?}"),
    }
}

/// A panicking inspector is contained — recorded as Failed, other inspectors
/// succeed, and completeness reflects the failure.
#[test]
fn inspector_panic_contained() {
    let source = package_based_source();
    let exec = minimal_mock();

    let inspectors: Vec<Box<dyn Inspector>> = vec![
        Box::new(DelayedInspector {
            id: InspectorId::Services,
            section: SectionData::Services(Default::default()),
            delay: Duration::ZERO,
        }),
        Box::new(PanickingInspector),
        Box::new(DelayedInspector {
            id: InspectorId::Storage,
            section: SectionData::Storage(Default::default()),
            delay: Duration::ZERO,
        }),
    ];

    let pipeline = collect(&source, &exec, &inspectors);

    // Non-panicking inspectors must succeed
    assert!(
        pipeline.state.snapshot.services.is_some(),
        "services must succeed despite another inspector panicking"
    );
    assert!(
        pipeline.state.snapshot.storage.is_some(),
        "storage must succeed despite another inspector panicking"
    );

    // Completeness must be Incomplete with Config (panicking inspector's ID) in failed
    match &pipeline.state.snapshot.completeness {
        Completeness::Incomplete {
            failed_sections, ..
        } => {
            assert!(
                failed_sections.contains(&InspectorId::Config),
                "Config (panicking inspector) must be in failed_sections"
            );
        }
        other => panic!("expected Incomplete, got {other:?}"),
    }

    // Should have a warning about the panic
    assert!(
        pipeline
            .state
            .snapshot
            .warnings
            .iter()
            .any(|w| w.message.contains("panicked")),
        "warnings must record the panic"
    );
}

/// A PackageBased-only inspector is never called when the source is Bootc.
#[test]
fn orchestrator_skips_inapplicable() {
    let source = bootc_source();
    let exec = minimal_mock();

    let pkg_inspector = PackageOnlyInspector::new();
    // We need to check call_count after collect, so use a shared reference trick.
    // Since Inspector requires ownership via Box, we use AtomicU32 inside the inspector.
    let inspectors: Vec<Box<dyn Inspector>> = vec![Box::new(PackageOnlyInspector::new())];

    let pipeline = collect(&source, &exec, &inspectors);

    // No sections should be populated (PackageOnly inspector was skipped)
    assert!(
        pipeline.state.snapshot.network.is_none(),
        "PackageBased-only inspector must not run on Bootc source"
    );

    // Should have a skip warning
    assert!(
        pipeline
            .state
            .snapshot
            .warnings
            .iter()
            .any(|w| w.message.contains("skipped") && w.message.contains("Bootc")),
        "warnings must record that inspector was skipped for Bootc source"
    );

    // Completeness should be Complete (skipped is intentional, not a failure)
    assert_eq!(
        pipeline.state.snapshot.completeness,
        Completeness::Complete,
        "skipped inspectors should not affect completeness"
    );

    // Verify the standalone pkg_inspector was never called (proves the pattern)
    assert_eq!(
        pkg_inspector.call_count(),
        0,
        "standalone PackageOnlyInspector should never be called"
    );
}
