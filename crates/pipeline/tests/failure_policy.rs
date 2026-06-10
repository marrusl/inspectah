//! Failure & trust policy tests — verifies how degraded and failed sections
//! are handled differently across renderers and the pipeline.
//!
//! - Degraded: partial data present, included with FIXME annotations
//! - Failed: no data collected, excluded from artifacts entirely
//!
//! Tests cover:
//! 1. Degraded vs Failed distinction in Containerfile, audit, and secrets review
//! 2. Completeness aggregation from inspector outcomes
//! 3. Panic containment → Failed status
//! 4. Redaction state after pipeline execution

use std::sync::atomic::AtomicBool;

use inspectah_collect::executor::mock::MockExecutor;
use inspectah_collect::inspectors::config::ConfigInspector;
use inspectah_collect::inspectors::nonrpm::NonRpmInspector;
use inspectah_collect::inspectors::rpm::RpmInspector;
use inspectah_collect::inspectors::scheduled::ScheduledTasksInspector;
use inspectah_collect::inspectors::selinux::SelinuxInspector;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::executor::ExecResult;
use inspectah_core::traits::inspector::{
    InspectionContext, Inspector, InspectorError, InspectorOutput, RpmState,
};
use inspectah_core::traits::progress::{NullProgress, ProgressSink};
use inspectah_core::types::completeness::{
    Completeness, InspectorId, SectionData, SourceSystemKind,
};
use inspectah_core::types::config::{ConfigFileEntry, ConfigSection};
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::redaction::RedactionState;
use inspectah_core::types::services::{ServiceSection, ServiceStateChange, SystemdDropIn};
use inspectah_core::types::system::SourceSystem;
use inspectah_pipeline::collect::collect;
use inspectah_pipeline::redaction::engine::{RedactOptions, redact};
use inspectah_pipeline::render::{audit, containerfile, secrets};

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

/// Build a snapshot with services Degraded — partial data present.
/// Includes a state_change entry to verify degraded data still renders.
fn snapshot_with_degraded_services() -> InspectionSnapshot {
    use inspectah_core::types::services::{PresetDefault, ServiceUnitState};
    let mut snap = InspectionSnapshot::new();
    snap.services = Some(ServiceSection {
        state_changes: vec![ServiceStateChange {
            unit: "httpd.service".into(),
            current_state: ServiceUnitState::Enabled,
            default_state: Some(PresetDefault::Disable),
            include: true,
            locked: false,
            owning_package: None,
            fleet: None,
            attention_reason: None,
        }],
        enabled_units: vec!["httpd.service".into()],
        disabled_units: vec!["cups.service".into()],
        ..Default::default()
    });
    snap.completeness = Completeness::Partial {
        degraded_sections: vec![InspectorId::Services],
        reason: "services inspector returned partial data".into(),
    };
    snap
}

/// Build a snapshot with services Failed — no data present.
fn snapshot_with_failed_services() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    // services is None — failed inspector produced no data
    snap.completeness = Completeness::Incomplete {
        failed_sections: vec![InspectorId::Services],
        degraded_sections: vec![],
        reason: "services inspector failed: permission denied".into(),
    };
    snap
}

/// Build a snapshot with degraded services that have a drop-in containing
/// a secret value, to test that redaction still scans degraded content.
fn snapshot_with_degraded_services_and_secret() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.services = Some(ServiceSection {
        enabled_units: vec!["myapp.service".into()],
        drop_ins: vec![SystemdDropIn {
            unit: "myapp.service".into(),
            path: "etc/systemd/system/myapp.service.d/override.conf".into(),
            content: "[Service]\nEnvironment=DB_PASSWORD=supersecret123\n".into(),
            include: true,
            locked: false,
            ..Default::default()
        }],
        ..Default::default()
    });
    snap.completeness = Completeness::Partial {
        degraded_sections: vec![InspectorId::Services],
        reason: "services inspector returned partial data".into(),
    };
    snap
}

// ---------------------------------------------------------------------------
// Mock inspectors for pipeline-level tests (Approach B)
// ---------------------------------------------------------------------------

/// Inspector that succeeds with a given section.
struct SuccessInspector {
    id: InspectorId,
    section: SectionData,
}

impl Inspector for SuccessInspector {
    fn id(&self) -> InspectorId {
        self.id
    }

    fn applicable_to(&self) -> &[SourceSystemKind] {
        &[SourceSystemKind::PackageBased]
    }

    fn inspect(
        &self,
        _ctx: &InspectionContext<'_>,
        _progress: &dyn ProgressSink,
    ) -> Result<InspectorOutput, InspectorError> {
        Ok(InspectorOutput {
            section: self.section.clone(),
            warnings: vec![],
            redaction_hints: vec![],
        })
    }
}

/// Inspector that returns Degraded with partial data.
struct DegradedServicesInspector;

impl Inspector for DegradedServicesInspector {
    fn id(&self) -> InspectorId {
        InspectorId::Services
    }

    fn applicable_to(&self) -> &[SourceSystemKind] {
        &[SourceSystemKind::PackageBased]
    }

    fn inspect(
        &self,
        _ctx: &InspectionContext<'_>,
        _progress: &dyn ProgressSink,
    ) -> Result<InspectorOutput, InspectorError> {
        Err(InspectorError::Degraded {
            partial: Box::new(InspectorOutput {
                section: SectionData::Services(ServiceSection {
                    enabled_units: vec!["httpd.service".into()],
                    ..Default::default()
                }),
                warnings: vec![],
                redaction_hints: vec![],
            }),
            reason: "partial data only".into(),
        })
    }
}

/// Inspector that returns Failed.
struct FailedServicesInspector;

impl Inspector for FailedServicesInspector {
    fn id(&self) -> InspectorId {
        InspectorId::Services
    }

    fn applicable_to(&self) -> &[SourceSystemKind] {
        &[SourceSystemKind::PackageBased]
    }

    fn inspect(
        &self,
        _ctx: &InspectionContext<'_>,
        _progress: &dyn ProgressSink,
    ) -> Result<InspectorOutput, InspectorError> {
        Err(InspectorError::Failed {
            reason: "permission denied".into(),
        })
    }
}

/// Inspector that panics — for containment testing.
struct PanickingInspector;

impl Inspector for PanickingInspector {
    fn id(&self) -> InspectorId {
        InspectorId::Network
    }

    fn applicable_to(&self) -> &[SourceSystemKind] {
        &[SourceSystemKind::PackageBased]
    }

    fn inspect(
        &self,
        _ctx: &InspectionContext<'_>,
        _progress: &dyn ProgressSink,
    ) -> Result<InspectorOutput, InspectorError> {
        panic!("intentional panic for failure policy test");
    }
}

// ===========================================================================
// Degraded vs Failed Distinction
// ===========================================================================

/// 1. Degraded section contributes to Containerfile with FIXME comment.
#[test]
fn degraded_section_contributes_to_containerfile_with_fixme() {
    let snap = snapshot_with_degraded_services();
    let output = containerfile::render_containerfile(&snap, None);

    // Services content IS present (partial data was routed)
    assert!(
        output.contains("systemctl enable") && output.contains("httpd.service"),
        "degraded services content must appear in Containerfile"
    );

    // FIXME annotation present for the degraded section
    assert!(
        output.contains("# FIXME: services data may be incomplete"),
        "degraded section must have FIXME comment in Containerfile"
    );
}

/// 2. Failed section excluded from Containerfile — no services content.
#[test]
fn failed_section_excluded_from_containerfile() {
    let snap = snapshot_with_failed_services();
    let output = containerfile::render_containerfile(&snap, None);

    // Services content must NOT appear (data is None)
    assert!(
        !output.contains("systemctl enable"),
        "failed services must not produce systemctl enable in Containerfile"
    );
    assert!(
        !output.contains("systemctl disable"),
        "failed services must not produce systemctl disable in Containerfile"
    );
    assert!(
        !output.contains("Service Enablement"),
        "failed services must not produce Service Enablement heading"
    );

    // The top-level incompleteness warning IS expected
    assert!(
        output.contains("WARNING"),
        "Containerfile must contain completeness warning for failed sections"
    );
}

/// 3. Failed section appears in audit report with explanation.
#[test]
fn failed_section_appears_in_audit_with_explanation() {
    let snap = snapshot_with_failed_services();
    let md = audit::render_audit(&snap);

    // Must contain the Incomplete Sections heading
    assert!(
        md.contains("## Incomplete Sections"),
        "audit report must contain Incomplete Sections heading"
    );

    // Must explicitly mark services as failed
    assert!(
        md.contains("### Failed"),
        "audit report must distinguish failed sections with a Failed subheading"
    );
    assert!(
        md.contains("services"),
        "audit report must list the failed services section"
    );

    // Must include the reason
    assert!(
        md.contains("permission denied"),
        "audit report must include the failure reason"
    );
}

/// 4. Degraded section still scanned by secrets review.
#[test]
fn degraded_section_scanned_by_secrets_review() {
    let mut snap = snapshot_with_degraded_services_and_secret();

    // Run redaction engine to populate snap.redactions
    redact(&mut snap, &RedactOptions::default());

    // Render secrets review
    let md = secrets::render_secrets_review(&snap);

    // Must find the secret despite degradation
    assert!(
        !md.contains("No redactions recorded"),
        "degraded section with secret must produce redaction findings"
    );
    assert!(
        md.contains("myapp.service") || md.contains("DB_PASSWORD"),
        "secrets review must reference the finding from degraded services"
    );
}

/// 5. Failed section excluded from secrets review — no services content to scan.
#[test]
fn failed_section_excluded_from_secrets_review() {
    let mut snap = snapshot_with_failed_services();

    // Run redaction — nothing to find since services is None
    redact(&mut snap, &RedactOptions::default());

    // Render secrets review
    let md = secrets::render_secrets_review(&snap);

    // No services content means no services-related findings
    assert!(
        !md.contains("myapp.service"),
        "failed services must not appear in secrets review"
    );
    assert!(
        !md.contains("DB_PASSWORD"),
        "failed services must not produce DB_PASSWORD finding"
    );
}

// ===========================================================================
// Completeness Aggregation (pipeline-level)
// ===========================================================================

/// 6. All inspectors succeed → Complete.
#[test]
fn all_success_produces_complete() {
    let source = package_based_source();
    let exec = minimal_mock();

    let inspectors: Vec<Box<dyn Inspector>> = vec![
        Box::new(SuccessInspector {
            id: InspectorId::Services,
            section: SectionData::Services(Default::default()),
        }),
        Box::new(SuccessInspector {
            id: InspectorId::Storage,
            section: SectionData::Storage(Default::default()),
        }),
        Box::new(SuccessInspector {
            id: InspectorId::KernelBoot,
            section: SectionData::KernelBoot(Default::default()),
        }),
    ];

    let pipeline = collect(
        &source,
        &exec,
        &inspectors,
        None,
        &NullProgress,
        &AtomicBool::new(false),
    );

    assert_eq!(
        pipeline.state.snapshot.completeness,
        Completeness::Complete,
        "all inspectors Ok → completeness must be Complete"
    );
}

/// 7. One degraded → Partial.
#[test]
fn one_degraded_produces_partial() {
    let source = package_based_source();
    let exec = minimal_mock();

    let inspectors: Vec<Box<dyn Inspector>> = vec![
        Box::new(SuccessInspector {
            id: InspectorId::Storage,
            section: SectionData::Storage(Default::default()),
        }),
        Box::new(DegradedServicesInspector),
    ];

    let pipeline = collect(
        &source,
        &exec,
        &inspectors,
        None,
        &NullProgress,
        &AtomicBool::new(false),
    );

    match &pipeline.state.snapshot.completeness {
        Completeness::Partial {
            degraded_sections, ..
        } => {
            assert!(
                degraded_sections.contains(&InspectorId::Services),
                "Services must be in degraded_sections"
            );
        }
        other => panic!("expected Partial, got {other:?}"),
    }

    // Degraded partial data must be routed
    assert!(
        pipeline.state.snapshot.services.is_some(),
        "degraded services partial data must be routed to snapshot"
    );
}

/// 8. One failed → Incomplete.
#[test]
fn one_failed_produces_incomplete() {
    let source = package_based_source();
    let exec = minimal_mock();

    let inspectors: Vec<Box<dyn Inspector>> = vec![
        Box::new(SuccessInspector {
            id: InspectorId::Storage,
            section: SectionData::Storage(Default::default()),
        }),
        Box::new(FailedServicesInspector),
    ];

    let pipeline = collect(
        &source,
        &exec,
        &inspectors,
        None,
        &NullProgress,
        &AtomicBool::new(false),
    );

    match &pipeline.state.snapshot.completeness {
        Completeness::Incomplete {
            failed_sections, ..
        } => {
            assert!(
                failed_sections.contains(&InspectorId::Services),
                "Services must be in failed_sections"
            );
        }
        other => panic!("expected Incomplete, got {other:?}"),
    }

    // Failed inspector produces no data
    assert!(
        pipeline.state.snapshot.services.is_none(),
        "failed services must not have data in snapshot"
    );
}

// ===========================================================================
// Panic Containment
// ===========================================================================

/// 9. Panicking inspector produces Failed status, others unaffected.
#[test]
fn panicking_inspector_produces_failed_status() {
    let source = package_based_source();
    let exec = minimal_mock();

    let inspectors: Vec<Box<dyn Inspector>> = vec![
        Box::new(SuccessInspector {
            id: InspectorId::Services,
            section: SectionData::Services(ServiceSection {
                enabled_units: vec!["httpd.service".into()],
                ..Default::default()
            }),
        }),
        Box::new(PanickingInspector), // InspectorId::Network
        Box::new(SuccessInspector {
            id: InspectorId::Storage,
            section: SectionData::Storage(Default::default()),
        }),
    ];

    let pipeline = collect(
        &source,
        &exec,
        &inspectors,
        None,
        &NullProgress,
        &AtomicBool::new(false),
    );

    // Non-panicking inspectors must succeed
    assert!(
        pipeline.state.snapshot.services.is_some(),
        "services must succeed despite another inspector panicking"
    );
    assert!(
        pipeline.state.snapshot.storage.is_some(),
        "storage must succeed despite another inspector panicking"
    );

    // Network (panicking) must be absent
    assert!(
        pipeline.state.snapshot.network.is_none(),
        "panicking inspector's section must be absent"
    );

    // Completeness must be Incomplete with Network in failed_sections
    match &pipeline.state.snapshot.completeness {
        Completeness::Incomplete {
            failed_sections, ..
        } => {
            assert!(
                failed_sections.contains(&InspectorId::Network),
                "Network (panicking) must be in failed_sections"
            );
        }
        other => panic!("expected Incomplete, got {other:?}"),
    }

    // Warning about the panic
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

// ===========================================================================
// Redaction State
// ===========================================================================

/// 10. Redaction state set after engine — not Raw.
#[test]
fn redaction_state_set_after_engine() {
    let mut snap = InspectionSnapshot::new();

    // Plant content with a secret that the redaction engine will detect
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/myapp/config".into(),
            content: "db_password = s3cretP@ss\n".into(),
            include: true,
            locked: false,
            ..Default::default()
        }],
    });

    // Redaction state starts as None (effectively Raw)
    assert!(
        snap.redaction_state.is_none(),
        "redaction_state must be None before engine runs"
    );

    // Run the redaction engine
    redact(&mut snap, &RedactOptions::default());

    // Redaction state must now be set
    assert!(
        snap.redaction_state.is_some(),
        "redaction_state must be set after engine runs"
    );

    match &snap.redaction_state {
        Some(RedactionState::FullyRedacted { redacted_by, .. }) => {
            assert!(
                redacted_by.contains("inspectah"),
                "redacted_by must identify inspectah"
            );
        }
        Some(RedactionState::PartiallyRedacted { redacted_by, .. }) => {
            assert!(
                redacted_by.contains("inspectah"),
                "redacted_by must identify inspectah"
            );
        }
        other => panic!("expected FullyRedacted or PartiallyRedacted, got {other:?}"),
    }

    // Content must have been redacted
    let config = snap.config.as_ref().unwrap();
    assert!(
        !config.files[0].content.contains("s3cretP@ss"),
        "secret must be redacted from content"
    );
    assert!(
        config.files[0].content.contains("REDACTED_"),
        "redacted content must contain REDACTED_ token"
    );
}

// ===========================================================================
// Wave 2 Inspector Failure Policy Tests (Slice 2c)
// ===========================================================================

/// Helper: build a MockExecutor with minimal RPM data for Wave 2 pipeline
/// tests. Provides responses for the RPM inspector's `rpm -qa`, `rpm -Va`,
/// and file ownership commands so Wave 1 completes successfully and
/// populates RpmState with owned_paths.
fn wave2_rpm_mock(exec: MockExecutor) -> MockExecutor {
    exec.with_command(
        "rpm -qa --queryformat %{EPOCH}:%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\n",
        ExecResult {
            stdout: "0:bash-5.2.26-3.el9.x86_64\n".into(),
            exit_code: 0,
            ..Default::default()
        },
    )
    .with_command(
        "rpm -Va",
        ExecResult {
            stdout: String::new(),
            exit_code: 0,
            ..Default::default()
        },
    )
    .with_command(
        "rpm -qa --queryformat [%{NAME}\\t%{FILENAMES}\\n]",
        ExecResult {
            stdout: "bash\t/etc/profile.d/bash_completion.sh\nbash\t/usr/bin/bash\n".into(),
            exit_code: 0,
            ..Default::default()
        },
    )
}

// ---------------------------------------------------------------------------
// 1. Scheduled: PermissionDenied on cron spool → Degraded
// ---------------------------------------------------------------------------

#[test]
fn test_scheduled_permission_denied_degraded() {
    let exec = wave2_rpm_mock(MockExecutor::new())
        .with_dir_error("/etc/cron.d", std::io::ErrorKind::PermissionDenied);

    let source = package_based_source();
    let inspectors: Vec<Box<dyn Inspector>> = vec![
        Box::new(RpmInspector::new()),
        Box::new(ScheduledTasksInspector::new()),
    ];

    let pipeline = collect(
        &source,
        &exec,
        &inspectors,
        None,
        &NullProgress,
        &AtomicBool::new(false),
    );
    let snapshot = &pipeline.state.snapshot;

    match &snapshot.completeness {
        Completeness::Partial {
            degraded_sections, ..
        } => {
            assert!(
                degraded_sections.contains(&InspectorId::ScheduledTasks),
                "ScheduledTasks should be degraded when cron dir is unreadable"
            );
        }
        other => panic!(
            "Expected Completeness::Partial for PermissionDenied cron dir, got {:?}",
            other
        ),
    }

    // Section should still be present (degraded carries partial data).
    assert!(
        snapshot.scheduled_tasks.is_some(),
        "ScheduledTasks section should be present even when degraded"
    );
}

// ---------------------------------------------------------------------------
// 2. Scheduled: cron dirs NotFound → no error, empty section
// ---------------------------------------------------------------------------

#[test]
fn test_scheduled_not_found_silent() {
    // No cron/timer/at directories registered → all return NotFound.
    // The inspector should return Ok with an empty section, not error.
    let exec = wave2_rpm_mock(MockExecutor::new());

    let source = package_based_source();
    let inspectors: Vec<Box<dyn Inspector>> = vec![
        Box::new(RpmInspector::new()),
        Box::new(ScheduledTasksInspector::new()),
    ];

    let pipeline = collect(
        &source,
        &exec,
        &inspectors,
        None,
        &NullProgress,
        &AtomicBool::new(false),
    );
    let snapshot = &pipeline.state.snapshot;

    assert!(
        matches!(snapshot.completeness, Completeness::Complete),
        "ScheduledTasks with all dirs NotFound should be Complete, got {:?}",
        snapshot.completeness
    );

    assert!(
        snapshot.scheduled_tasks.is_some(),
        "ScheduledTasks section should be present with empty data"
    );

    if let Some(ref section) = snapshot.scheduled_tasks {
        assert!(section.cron_jobs.is_empty(), "no cron jobs expected");
        assert!(section.systemd_timers.is_empty(), "no timers expected");
        assert!(section.at_jobs.is_empty(), "no at jobs expected");
    }
}

// ---------------------------------------------------------------------------
// 3. Config: rpm -Va empty + file read failure → Degraded
// ---------------------------------------------------------------------------

#[test]
fn test_config_rpm_va_failure_degraded() {
    // Config inspector receives RpmState with empty verification_results
    // (simulating upstream rpm -Va failure). An /etc file that exists but
    // can't be read pushes to degraded_reasons, making config Degraded.
    let exec = MockExecutor::new()
        // Provide an /etc directory with one file
        .with_dir("/etc", vec!["myapp.conf"])
        // The file exists (walk finds it) but reading content fails
        .with_file_error("/etc/myapp.conf", std::io::ErrorKind::PermissionDenied);

    let rpm_state = RpmState::default(); // empty — simulates rpm -Va failure upstream
    let source = package_based_source();
    let inspector = ConfigInspector::new();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: Some(&rpm_state),
        baseline_data: None,
    };

    let result = inspector.inspect(&ctx, &NullProgress);
    match result {
        Err(InspectorError::Degraded { reason, .. }) => {
            assert!(
                reason.contains("degraded"),
                "expected 'degraded' in reason, got: {reason}"
            );
        }
        other => panic!("expected Degraded for config with file read failure, got: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 4. Config: PermissionDenied on /etc subdir → Degraded
// ---------------------------------------------------------------------------

#[test]
fn test_config_etc_permission_denied_degraded() {
    // /etc exists and is listable, but a subdirectory has PermissionDenied.
    // walk_etc_recursive encounters the error and pushes to degraded_reasons.
    // An unowned file that can't be read also contributes to degradation.
    let exec = wave2_rpm_mock(MockExecutor::new())
        // /etc is readable at top level (walk starts here)
        .with_dir("/etc", vec!["httpd", "myapp.conf"])
        // /etc/httpd is PermissionDenied → walk_recursive_inner skips it
        .with_dir_error("/etc/httpd", std::io::ErrorKind::PermissionDenied)
        // /etc/myapp.conf exists but content read fails → degraded_reasons
        .with_file_error("/etc/myapp.conf", std::io::ErrorKind::PermissionDenied);

    let source = package_based_source();
    let inspectors: Vec<Box<dyn Inspector>> = vec![
        Box::new(RpmInspector::new()),
        Box::new(ConfigInspector::new()),
    ];

    let pipeline = collect(
        &source,
        &exec,
        &inspectors,
        None,
        &NullProgress,
        &AtomicBool::new(false),
    );
    let snapshot = &pipeline.state.snapshot;

    match &snapshot.completeness {
        Completeness::Partial {
            degraded_sections, ..
        } => {
            assert!(
                degraded_sections.contains(&InspectorId::Config),
                "Config should be degraded when /etc subdirs are unreadable"
            );
        }
        other => panic!(
            "Expected Completeness::Partial for /etc PermissionDenied, got {:?}",
            other
        ),
    }

    // Config section should still be present (degraded carries partial data).
    assert!(
        snapshot.config.is_some(),
        "Config section should be present even when degraded"
    );
}

// ---------------------------------------------------------------------------
// 5. SELinux: semanage unavailable → Degraded with sysfs fallback
// ---------------------------------------------------------------------------

#[test]
fn test_selinux_semanage_unavailable_degraded() {
    // semanage commands all fail (exit 127). getenforce also fails.
    // sysfs fallback also unavailable. This triggers degraded because
    // SELinux mode detection fails (both getenforce and sysfs).
    let exec = wave2_rpm_mock(MockExecutor::new())
        // getenforce fails
        .with_command(
            "getenforce",
            ExecResult {
                exit_code: 127,
                stderr: "command not found".into(),
                ..Default::default()
            },
        )
        // sysfs fallback not available (file not registered → NotFound)
        // semanage boolean -l fails (chroot / semanage boolean -l)
        .with_command(
            "chroot / semanage boolean -l",
            ExecResult {
                exit_code: 127,
                stderr: "command not found".into(),
                ..Default::default()
            },
        )
        .with_command(
            "chroot / semanage fcontext -l -C",
            ExecResult {
                exit_code: 127,
                stderr: "command not found".into(),
                ..Default::default()
            },
        )
        .with_command(
            "chroot / semanage port -l -C",
            ExecResult {
                exit_code: 127,
                stderr: "command not found".into(),
                ..Default::default()
            },
        );

    let source = package_based_source();
    let inspectors: Vec<Box<dyn Inspector>> = vec![
        Box::new(RpmInspector::new()),
        Box::new(SelinuxInspector::new()),
    ];

    let pipeline = collect(
        &source,
        &exec,
        &inspectors,
        None,
        &NullProgress,
        &AtomicBool::new(false),
    );
    let snapshot = &pipeline.state.snapshot;

    match &snapshot.completeness {
        Completeness::Partial {
            degraded_sections, ..
        } => {
            assert!(
                degraded_sections.contains(&InspectorId::Selinux),
                "Selinux should be degraded when semanage and sysfs are unavailable"
            );
        }
        other => panic!(
            "Expected Completeness::Partial for semanage unavailable, got {:?}",
            other
        ),
    }

    // Section should still be present with partial data.
    assert!(
        snapshot.selinux.is_some(),
        "Selinux section should be present even when degraded"
    );
}

// ---------------------------------------------------------------------------
// 6. SELinux: PermissionDenied on audit rules → Degraded
// ---------------------------------------------------------------------------

#[test]
fn test_selinux_audit_permission_denied_degraded() {
    // Audit rules dir exists but is unreadable → Degraded.
    let exec = wave2_rpm_mock(MockExecutor::new())
        // getenforce succeeds so mode detection is fine
        .with_command(
            "getenforce",
            ExecResult {
                stdout: "Enforcing\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_file(
            "/etc/selinux/config",
            "SELINUX=enforcing\nSELINUXTYPE=targeted\n",
        )
        // semanage boolean works
        .with_command(
            "chroot / semanage boolean -l",
            ExecResult {
                stdout: String::new(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_command(
            "chroot / semanage fcontext -l -C",
            ExecResult {
                stdout: String::new(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_command(
            "chroot / semanage port -l -C",
            ExecResult {
                stdout: String::new(),
                exit_code: 0,
                ..Default::default()
            },
        )
        // Audit rules dir: PermissionDenied
        .with_dir_error("/etc/audit/rules.d", std::io::ErrorKind::PermissionDenied)
        // FIPS mode check
        .with_file("/proc/sys/crypto/fips_enabled", "0\n");

    let source = package_based_source();
    let inspectors: Vec<Box<dyn Inspector>> = vec![
        Box::new(RpmInspector::new()),
        Box::new(SelinuxInspector::new()),
    ];

    let pipeline = collect(
        &source,
        &exec,
        &inspectors,
        None,
        &NullProgress,
        &AtomicBool::new(false),
    );
    let snapshot = &pipeline.state.snapshot;

    match &snapshot.completeness {
        Completeness::Partial {
            degraded_sections, ..
        } => {
            assert!(
                degraded_sections.contains(&InspectorId::Selinux),
                "Selinux should be degraded when audit rules dir is unreadable"
            );
        }
        other => panic!(
            "Expected Completeness::Partial for audit PermissionDenied, got {:?}",
            other
        ),
    }

    assert!(
        snapshot.selinux.is_some(),
        "Selinux section should be present even when degraded"
    );
}

// ---------------------------------------------------------------------------
// 7. NonRPM: readelf unavailable → Complete with warning (not degraded)
// ---------------------------------------------------------------------------

#[test]
fn test_nonrpm_readelf_unavailable_completes() {
    // readelf returns exit code 127 (not found). Missing tools are not a
    // scan failure — the inspector reports what it found with a warning.
    let exec = wave2_rpm_mock(MockExecutor::new())
        .with_command(
            "readelf --version",
            ExecResult {
                exit_code: 127,
                stderr: "command not found".into(),
                ..Default::default()
            },
        )
        .with_dir("/opt", vec!["app"])
        .with_dir("/opt/app", vec![".env"])
        .with_file("/opt/app/.env", "DATABASE_URL=postgres://localhost/mydb\n");

    let source = package_based_source();
    let inspectors: Vec<Box<dyn Inspector>> = vec![
        Box::new(RpmInspector::new()),
        Box::new(NonRpmInspector::new()),
    ];

    let pipeline = collect(
        &source,
        &exec,
        &inspectors,
        None,
        &NullProgress,
        &AtomicBool::new(false),
    );
    let snapshot = &pipeline.state.snapshot;

    assert_eq!(
        snapshot.completeness,
        Completeness::Complete,
        "missing readelf should not degrade the scan"
    );
    assert!(
        snapshot
            .warnings
            .iter()
            .any(|w| w.message.contains("readelf")),
        "should have a warning about readelf being unavailable"
    );

    assert!(
        snapshot.non_rpm_software.is_some(),
        "NonRpmSoftware section should be present with partial data"
    );
}

// ---------------------------------------------------------------------------
// 8. NonRPM: /opt not found → no error, no items
// ---------------------------------------------------------------------------

#[test]
fn test_nonrpm_scan_dir_not_found_silent() {
    // No /opt, /srv, /usr/local registered → all return NotFound.
    // readelf is available. Inspector should return Ok with empty section.
    let exec = wave2_rpm_mock(MockExecutor::new())
        .with_command(
            "readelf --version",
            ExecResult {
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_command(
            "file --version",
            ExecResult {
                exit_code: 0,
                ..Default::default()
            },
        );

    let source = package_based_source();
    let inspectors: Vec<Box<dyn Inspector>> = vec![
        Box::new(RpmInspector::new()),
        Box::new(NonRpmInspector::new()),
    ];

    let pipeline = collect(
        &source,
        &exec,
        &inspectors,
        None,
        &NullProgress,
        &AtomicBool::new(false),
    );
    let snapshot = &pipeline.state.snapshot;

    assert!(
        matches!(snapshot.completeness, Completeness::Complete),
        "NonRpmSoftware with all scan dirs NotFound should be Complete, got {:?}",
        snapshot.completeness
    );

    assert!(
        snapshot.non_rpm_software.is_some(),
        "NonRpmSoftware section should be present with empty data"
    );

    if let Some(ref section) = snapshot.non_rpm_software {
        assert!(section.items.is_empty(), "no items expected");
        assert!(section.env_files.is_empty(), "no env files expected");
    }
}

// ---------------------------------------------------------------------------
// 9. Wave 2 RPM unavailable → all 4 dependents return Failed
// ---------------------------------------------------------------------------

#[test]
fn test_wave2_rpm_unavailable_fails_all_dependents() {
    // When ctx.rpm_state is None, all 4 Wave 2 inspectors MUST return
    // Err(InspectorError::Failed). This proves the None vs Some(empty)
    // distinction: RPM failure is fatal to all dependents.
    let exec = MockExecutor::new();
    let source = package_based_source();

    let inspectors: Vec<(&str, Box<dyn Inspector>)> = vec![
        ("ScheduledTasks", Box::new(ScheduledTasksInspector::new())),
        ("Config", Box::new(ConfigInspector::new())),
        ("Selinux", Box::new(SelinuxInspector::new())),
        ("NonRpmSoftware", Box::new(NonRpmInspector::new())),
    ];

    for (name, inspector) in &inspectors {
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        match result {
            Err(InspectorError::Failed { reason }) => {
                assert!(
                    reason.to_lowercase().contains("rpm")
                        || reason.to_lowercase().contains("prerequisite"),
                    "{name}: expected RPM-related failure reason, got: {reason}"
                );
            }
            other => {
                panic!("{name}: expected InspectorError::Failed for None rpm_state, got: {other:?}")
            }
        }
    }

    // Verify that Some(empty) does NOT fail — it produces Ok or Degraded,
    // but never Failed. This is the critical None vs Some(empty) distinction.
    let empty_rpm_state = RpmState::default();
    for (name, inspector) in &inspectors {
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&empty_rpm_state),
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        if let Err(InspectorError::Failed { reason }) = &result {
            panic!(
                "{name}: Some(empty) rpm_state should NOT produce Failed, \
                 but got Failed {{ reason: \"{reason}\" }}"
            );
        }
        // Ok or Degraded are both acceptable — the point is it's not Failed.
    }
}
