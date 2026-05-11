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

use inspectah_collect::executor::mock::MockExecutor;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::executor::ExecResult;
use inspectah_core::traits::inspector::{
    InspectionContext, Inspector, InspectorError, InspectorOutput,
};
use inspectah_core::types::completeness::{
    Completeness, InspectorId, SectionData, SourceSystemKind,
};
use inspectah_core::types::config::{ConfigFileEntry, ConfigSection};
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::redaction::RedactionState;
use inspectah_core::types::services::{ServiceSection, SystemdDropIn};
use inspectah_core::types::system::SourceSystem;
use inspectah_pipeline::collect::collect;
use inspectah_pipeline::redaction::engine::{redact, RedactOptions};
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
fn snapshot_with_degraded_services() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.services = Some(ServiceSection {
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

    fn inspect(&self, _ctx: &InspectionContext<'_>) -> Result<InspectorOutput, InspectorError> {
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

    fn inspect(&self, _ctx: &InspectionContext<'_>) -> Result<InspectorOutput, InspectorError> {
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

    fn inspect(&self, _ctx: &InspectionContext<'_>) -> Result<InspectorOutput, InspectorError> {
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

    fn inspect(&self, _ctx: &InspectionContext<'_>) -> Result<InspectorOutput, InspectorError> {
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
        output.contains("systemctl enable httpd.service"),
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

    let pipeline = collect(&source, &exec, &inspectors);

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

    let pipeline = collect(&source, &exec, &inspectors);

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

    let pipeline = collect(&source, &exec, &inspectors);

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
