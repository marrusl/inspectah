use inspectah_core::pipeline::{Collected, Pipeline};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::executor::Executor;
use inspectah_core::traits::inspector::{
    InspectionContext, Inspector, InspectorError, InspectorOutput, RpmState,
};
use inspectah_core::types::completeness::{Completeness, InspectorId, SectionData};
use inspectah_core::types::os::SystemType;
use inspectah_core::types::system::SourceSystem;
use inspectah_core::types::warnings::{Warning, WarningSeverity};

/// Run all inspectors in parallel via `std::thread::scope`, routing each
/// inspector's typed SectionData to the corresponding snapshot field.
///
/// Inspectors are filtered by applicability before spawning. Each inspector
/// runs in its own scoped thread; results are joined and routed serially
/// by the main thread.
///
/// Panics inside any inspector are contained — the panicking inspector is
/// recorded as Failed without tearing down other threads.
///
/// Returns a `Pipeline<Collected>` containing the populated snapshot.
pub fn collect(
    source: &SourceSystem,
    executor: &dyn Executor,
    inspectors: &[Box<dyn Inspector>],
) -> Pipeline<Collected> {
    let mut snapshot = InspectionSnapshot::new();
    let mut failed: Vec<InspectorId> = Vec::new();
    let mut degraded: Vec<InspectorId> = Vec::new();

    // Applicability gate: filter inspectors by source system kind.
    // Inapplicable inspectors are recorded as Skipped without spawning.
    let source_kind = source.kind();
    let mut applicable: Vec<&Box<dyn Inspector>> = Vec::new();

    for inspector in inspectors {
        if inspector.applicable_to().contains(&source_kind) {
            applicable.push(inspector);
        } else {
            snapshot.warnings.push(Warning {
                inspector: format!("{:?}", inspector.id()),
                message: format!("skipped: not applicable to {source_kind:?}"),
                severity: Some(WarningSeverity::Info),
                ..Default::default()
            });
        }
    }

    // ── Three-wave execution model ──────────────────────────────────
    //
    // Wave 1: RPM inspector + independent inspectors run in parallel
    //         with rpm_state: None.
    // Join:   Extract RpmState from RPM inspector output (if it ran).
    // Wave 2: Dependent inspectors run in parallel with enriched
    //         context containing rpm_state: Some(&rpm_state).
    //
    // For Slice 2a all non-RPM inspectors are independent (none need
    // rpm_state), so Wave 2 is empty. The partition and enrichment
    // path is exercised regardless — when Slice 2c adds dependent
    // inspectors, they will land in Wave 2 automatically.

    // Partition: RPM goes into Wave 1. Inspectors that need rpm_state
    // would go into Wave 2 (none yet in Slice 2a).
    // Partition applicable inspectors into Wave 1 (all for Slice 2a)
    // and Wave 2 (rpm_state-dependent, empty for Slice 2a).
    let mut wave1: Vec<&Box<dyn Inspector>> = Vec::new();
    let wave2: Vec<&Box<dyn Inspector>> = Vec::new();
    for insp in &applicable {
        // All inspectors are Wave 1 for Slice 2a. When an inspector
        // gains an rpm_state dependency, it moves to wave2.
        wave1.push(insp);
    }

    // Wave 1 base context — no rpm_state available yet.
    let base_ctx = InspectionContext {
        source_system: source,
        executor,
        rpm_state: None,
    };

    // Wave 1: parallel execution via std::thread::scope.
    let mut rpm_state = RpmState::default();

    std::thread::scope(|s| {
        let handles: Vec<_> = wave1
            .iter()
            .map(|inspector| s.spawn(|| inspector.inspect(&base_ctx)))
            .collect();

        for (inspector, handle) in wave1.iter().zip(handles) {
            handle_result(
                inspector.as_ref(),
                handle,
                &mut snapshot,
                &mut failed,
                &mut degraded,
                &mut rpm_state,
            );
        }
    });

    // Wave 2: dependent inspectors get enriched context with rpm_state.
    // Empty for Slice 2a — the API path is exercised, no inspectors run.
    if !wave2.is_empty() {
        let enriched_ctx = InspectionContext {
            source_system: source,
            executor,
            rpm_state: Some(&rpm_state),
        };

        // Wave 2 does not mutate rpm_state — it only reads it.
        // A separate mutable tracker is used for completeness bookkeeping.
        std::thread::scope(|s| {
            let handles: Vec<_> = wave2
                .iter()
                .map(|inspector| s.spawn(|| inspector.inspect(&enriched_ctx)))
                .collect();

            // Wave 2 inspectors don't produce RpmState, so pass a
            // throwaway — the real rpm_state is already finalized.
            let mut wave2_rpm = RpmState::default();
            for (inspector, handle) in wave2.iter().zip(handles) {
                handle_result(
                    inspector.as_ref(),
                    handle,
                    &mut snapshot,
                    &mut failed,
                    &mut degraded,
                    &mut wave2_rpm,
                );
            }
        });
    }

    // Set completeness based on inspector outcomes
    snapshot.completeness = if failed.is_empty() && degraded.is_empty() {
        Completeness::Complete
    } else if failed.is_empty() {
        // Only degraded — partial data available for all sections
        Completeness::Partial {
            degraded_sections: degraded,
            reason: "one or more inspectors returned degraded results".into(),
        }
    } else {
        Completeness::Incomplete {
            failed_sections: failed,
            degraded_sections: degraded,
            reason: "one or more inspectors failed or returned degraded results".into(),
        }
    };

    // Populate source identity so exported snapshots identify the host
    snapshot.os_release = Some(source.os_release().clone());
    snapshot.system_type = source_system_type(source);

    // Populate meta with provenance information
    let hostname = executor
        .read_file(std::path::Path::new("/etc/hostname"))
        .unwrap_or_default()
        .trim()
        .to_string();
    if !hostname.is_empty() {
        snapshot
            .meta
            .insert("hostname".into(), serde_json::Value::String(hostname));
    }
    snapshot.meta.insert(
        "timestamp".into(),
        serde_json::Value::String(chrono::Utc::now().to_rfc3339()),
    );
    snapshot.meta.insert(
        "inspectah_version".into(),
        serde_json::Value::String(env!("CARGO_PKG_VERSION").into()),
    );

    Pipeline {
        state: Collected { snapshot },
    }
}

/// Process a single inspector's join result: route section data, record
/// warnings/failures, and extract RpmState when the RPM inspector succeeds.
fn handle_result(
    inspector: &dyn Inspector,
    handle: std::thread::ScopedJoinHandle<'_, Result<InspectorOutput, InspectorError>>,
    snapshot: &mut InspectionSnapshot,
    failed: &mut Vec<InspectorId>,
    degraded: &mut Vec<InspectorId>,
    rpm_state: &mut RpmState,
) {
    match handle.join() {
        Ok(Ok(output)) => {
            // Extract RpmState from RPM inspector output before routing
            if inspector.id() == InspectorId::Rpm {
                if let SectionData::Rpm(ref rpm) = output.section {
                    rpm_state.installed_packages =
                        rpm.packages_added.iter().map(|p| p.name.clone()).collect();
                    // owned_paths populated in later slices
                }
            }
            route_section(snapshot, output.section);
            snapshot.warnings.extend(output.warnings);
        }
        Ok(Err(InspectorError::Skipped { reason })) => {
            snapshot.warnings.push(Warning {
                inspector: format!("{:?}", inspector.id()),
                message: format!("skipped: {reason}"),
                severity: Some(WarningSeverity::Info),
                ..Default::default()
            });
        }
        Ok(Err(InspectorError::Degraded {
            partial, reason, ..
        })) => {
            route_section(snapshot, partial.section);
            snapshot.warnings.extend(partial.warnings);
            snapshot.warnings.push(Warning {
                inspector: format!("{:?}", inspector.id()),
                message: format!("degraded: {reason}"),
                severity: Some(WarningSeverity::Warning),
                ..Default::default()
            });
            degraded.push(inspector.id());
        }
        Ok(Err(InspectorError::Failed { reason })) => {
            snapshot.warnings.push(Warning {
                inspector: format!("{:?}", inspector.id()),
                message: format!("failed: {reason}"),
                severity: Some(WarningSeverity::Error),
                ..Default::default()
            });
            failed.push(inspector.id());
        }
        Err(_panic) => {
            // Panic contained — record as Failed
            failed.push(inspector.id());
            snapshot.warnings.push(Warning {
                inspector: format!("{:?}", inspector.id()),
                message: "inspector panicked".into(),
                severity: Some(WarningSeverity::Error),
                ..Default::default()
            });
        }
    }
}

/// Map a SourceSystem variant to the corresponding SystemType for the snapshot.
fn source_system_type(source: &SourceSystem) -> SystemType {
    match source {
        SourceSystem::PackageBased { .. } => SystemType::PackageMode,
        SourceSystem::RpmOstree { .. } => SystemType::RpmOstree,
        SourceSystem::Bootc { .. } => SystemType::Bootc,
    }
}

/// Route a typed SectionData variant to the correct snapshot field.
fn route_section(snapshot: &mut InspectionSnapshot, section: SectionData) {
    match section {
        SectionData::Rpm(s) => snapshot.rpm = Some(s),
        SectionData::Config(s) => snapshot.config = Some(s),
        SectionData::Services(s) => snapshot.services = Some(s),
        SectionData::Network(s) => snapshot.network = Some(s),
        SectionData::Storage(s) => snapshot.storage = Some(s),
        SectionData::ScheduledTasks(s) => snapshot.scheduled_tasks = Some(s),
        SectionData::Containers(s) => snapshot.containers = Some(s),
        SectionData::NonRpmSoftware(s) => snapshot.non_rpm_software = Some(s),
        SectionData::KernelBoot(s) => snapshot.kernel_boot = Some(s),
        SectionData::Selinux(s) => snapshot.selinux = Some(s),
        SectionData::UsersGroups(s) => snapshot.users_groups = Some(s),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_collect::executor::mock::MockExecutor;
    use inspectah_collect::inspectors::rpm::RpmInspector;
    use inspectah_core::traits::executor::ExecResult;
    use inspectah_core::traits::inspector::InspectorOutput;
    use inspectah_core::types::completeness::SourceSystemKind;
    use inspectah_core::types::os::OsRelease;
    use inspectah_core::types::system::SourceSystem;

    /// Mock inspector that always returns Failed.
    struct FailingInspector;
    impl Inspector for FailingInspector {
        fn id(&self) -> InspectorId {
            InspectorId::Config
        }
        fn applicable_to(&self) -> &[SourceSystemKind] {
            &[SourceSystemKind::PackageBased]
        }
        fn inspect(&self, _ctx: &InspectionContext<'_>) -> Result<InspectorOutput, InspectorError> {
            Err(InspectorError::Failed {
                reason: "test failure".into(),
            })
        }
    }

    /// Mock inspector that returns Degraded with partial data.
    struct DegradedInspector;
    impl Inspector for DegradedInspector {
        fn id(&self) -> InspectorId {
            InspectorId::Network
        }
        fn applicable_to(&self) -> &[SourceSystemKind] {
            &[SourceSystemKind::PackageBased]
        }
        fn inspect(&self, _ctx: &InspectionContext<'_>) -> Result<InspectorOutput, InspectorError> {
            Err(InspectorError::Degraded {
                partial: Box::new(InspectorOutput {
                    section: SectionData::Network(Default::default()),
                    warnings: vec![],
                    redaction_hints: vec![],
                }),
                reason: "partial data only".into(),
            })
        }
    }

    /// Mock inspector that always returns Skipped.
    struct SkippedInspector;
    impl Inspector for SkippedInspector {
        fn id(&self) -> InspectorId {
            InspectorId::Storage
        }
        fn applicable_to(&self) -> &[SourceSystemKind] {
            &[SourceSystemKind::PackageBased]
        }
        fn inspect(&self, _ctx: &InspectionContext<'_>) -> Result<InspectorOutput, InspectorError> {
            Err(InspectorError::Skipped {
                reason: "not applicable".into(),
            })
        }
    }

    fn test_os_release() -> OsRelease {
        OsRelease {
            name: "Red Hat Enterprise Linux".into(),
            version_id: "9.4".into(),
            id: "rhel".into(),
            ..Default::default()
        }
    }

    fn build_test_mock() -> MockExecutor {
        let rpm_qa_output = "\
0:bash-5.2.26-3.el9.x86_64
0:httpd-2.4.57-5.el9.x86_64
";
        MockExecutor::new().with_command(
            "rpm -qa --queryformat %{EPOCH}:%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\n",
            ExecResult {
                stdout: rpm_qa_output.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
    }

    #[test]
    fn test_collect_produces_pipeline_collected() {
        let exec = build_test_mock();
        let source = SourceSystem::PackageBased {
            os_release: test_os_release(),
        };
        let inspectors: Vec<Box<dyn Inspector>> = vec![Box::new(RpmInspector::new())];
        let pipeline = collect(&source, &exec, &inspectors);

        // Pipeline produced a Collected state with rpm data
        assert!(pipeline.state.snapshot.rpm.is_some());
        let rpm = pipeline.state.snapshot.rpm.unwrap();
        assert_eq!(rpm.packages_added.len(), 2);

        // Warnings should include the no-baseline warning from RpmInspector
        assert!(pipeline
            .state
            .snapshot
            .warnings
            .iter()
            .any(|w| w.message.contains("no baseline")));
    }

    #[test]
    fn test_collect_handles_inspector_failure() {
        // Empty rpm output triggers a Failed error from RpmInspector
        let exec = MockExecutor::new().with_command(
            "rpm -qa --queryformat %{EPOCH}:%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\n",
            ExecResult {
                stdout: "".into(),
                exit_code: 0,
                ..Default::default()
            },
        );
        let source = SourceSystem::PackageBased {
            os_release: test_os_release(),
        };
        let inspectors: Vec<Box<dyn Inspector>> = vec![Box::new(RpmInspector::new())];
        let pipeline = collect(&source, &exec, &inspectors);

        // rpm section should be None (failed)
        assert!(pipeline.state.snapshot.rpm.is_none());

        // Should have an error warning about the failure
        assert!(pipeline
            .state
            .snapshot
            .warnings
            .iter()
            .any(|w| w.message.contains("failed")));
    }

    #[test]
    fn test_collect_routes_section_data_correctly() {
        let exec = build_test_mock();
        let source = SourceSystem::PackageBased {
            os_release: test_os_release(),
        };
        let inspectors: Vec<Box<dyn Inspector>> = vec![Box::new(RpmInspector::new())];
        let pipeline = collect(&source, &exec, &inspectors);

        // RPM routed correctly
        assert!(pipeline.state.snapshot.rpm.is_some());
        // Other sections remain None (no inspectors for them)
        assert!(pipeline.state.snapshot.config.is_none());
        assert!(pipeline.state.snapshot.services.is_none());
        assert!(pipeline.state.snapshot.network.is_none());
        assert!(pipeline.state.snapshot.storage.is_none());
    }

    #[test]
    fn test_collect_sets_source_identity() {
        let exec = build_test_mock().with_file("/etc/hostname", "testhost.example.com\n");
        let source = SourceSystem::PackageBased {
            os_release: test_os_release(),
        };
        let inspectors: Vec<Box<dyn Inspector>> = vec![Box::new(RpmInspector::new())];
        let pipeline = collect(&source, &exec, &inspectors);
        let snap = &pipeline.state.snapshot;

        // os_release must be populated from the source system
        assert!(snap.os_release.is_some(), "os_release must be set");
        let os = snap.os_release.as_ref().unwrap();
        assert_eq!(os.id, "rhel");
        assert_eq!(os.version_id, "9.4");

        // system_type must reflect the source
        assert_eq!(
            snap.system_type,
            inspectah_core::types::os::SystemType::PackageMode
        );

        // meta must contain hostname and inspectah_version
        assert!(
            snap.meta.contains_key("hostname"),
            "meta must contain hostname"
        );
        assert_eq!(
            snap.meta["hostname"].as_str().unwrap(),
            "testhost.example.com"
        );
        assert!(
            snap.meta.contains_key("inspectah_version"),
            "meta must contain inspectah_version"
        );
        assert!(
            snap.meta.contains_key("timestamp"),
            "meta must contain timestamp"
        );
    }

    #[test]
    fn test_collect_source_identity_without_hostname() {
        // No /etc/hostname file — hostname should be absent from meta
        let exec = build_test_mock();
        let source = SourceSystem::PackageBased {
            os_release: test_os_release(),
        };
        let inspectors: Vec<Box<dyn Inspector>> = vec![Box::new(RpmInspector::new())];
        let pipeline = collect(&source, &exec, &inspectors);
        let snap = &pipeline.state.snapshot;

        // os_release and system_type still set
        assert!(snap.os_release.is_some());
        assert_eq!(
            snap.system_type,
            inspectah_core::types::os::SystemType::PackageMode
        );

        // hostname absent when file doesn't exist
        assert!(
            !snap.meta.contains_key("hostname"),
            "hostname should be absent when /etc/hostname is missing"
        );
        // version and timestamp still present
        assert!(snap.meta.contains_key("inspectah_version"));
        assert!(snap.meta.contains_key("timestamp"));
    }

    // --- Completeness tracking tests ---

    #[test]
    fn test_completeness_complete_when_all_succeed() {
        let exec = build_test_mock();
        let source = SourceSystem::PackageBased {
            os_release: test_os_release(),
        };
        let inspectors: Vec<Box<dyn Inspector>> = vec![Box::new(RpmInspector::new())];
        let pipeline = collect(&source, &exec, &inspectors);

        assert_eq!(
            pipeline.state.snapshot.completeness,
            Completeness::Complete,
            "all inspectors succeeded -> completeness must be Complete"
        );
    }

    #[test]
    fn test_completeness_incomplete_when_inspector_fails() {
        let exec = build_test_mock();
        let source = SourceSystem::PackageBased {
            os_release: test_os_release(),
        };
        let inspectors: Vec<Box<dyn Inspector>> =
            vec![Box::new(RpmInspector::new()), Box::new(FailingInspector)];
        let pipeline = collect(&source, &exec, &inspectors);

        match &pipeline.state.snapshot.completeness {
            Completeness::Incomplete {
                failed_sections,
                reason,
                ..
            } => {
                assert!(
                    failed_sections.contains(&InspectorId::Config),
                    "Config inspector failed, must appear in failed_sections"
                );
                assert!(!reason.is_empty(), "reason must explain the incompleteness");
            }
            other => panic!("expected Incomplete, got {other:?}"),
        }
    }

    #[test]
    fn test_completeness_partial_when_inspector_degraded() {
        let exec = build_test_mock();
        let source = SourceSystem::PackageBased {
            os_release: test_os_release(),
        };
        let inspectors: Vec<Box<dyn Inspector>> =
            vec![Box::new(RpmInspector::new()), Box::new(DegradedInspector)];
        let pipeline = collect(&source, &exec, &inspectors);

        // Partial data should be routed
        assert!(
            pipeline.state.snapshot.network.is_some(),
            "degraded inspector's partial data must be routed"
        );

        // Completeness must reflect the degradation
        match &pipeline.state.snapshot.completeness {
            Completeness::Partial {
                degraded_sections, ..
            } => {
                assert!(
                    degraded_sections.contains(&InspectorId::Network),
                    "Network inspector degraded, must appear in degraded_sections"
                );
            }
            other => panic!("expected Partial, got {other:?}"),
        }
    }

    #[test]
    fn test_completeness_complete_when_inspector_skipped() {
        // Skipped is intentional (inapplicable) — should NOT affect completeness
        let exec = build_test_mock();
        let source = SourceSystem::PackageBased {
            os_release: test_os_release(),
        };
        let inspectors: Vec<Box<dyn Inspector>> =
            vec![Box::new(RpmInspector::new()), Box::new(SkippedInspector)];
        let pipeline = collect(&source, &exec, &inspectors);

        assert_eq!(
            pipeline.state.snapshot.completeness,
            Completeness::Complete,
            "skipped inspectors are intentional, completeness must still be Complete"
        );
    }

    #[test]
    fn test_completeness_incomplete_with_mixed_failures() {
        let exec = build_test_mock();
        let source = SourceSystem::PackageBased {
            os_release: test_os_release(),
        };
        let inspectors: Vec<Box<dyn Inspector>> = vec![
            Box::new(RpmInspector::new()),
            Box::new(FailingInspector),
            Box::new(DegradedInspector),
            Box::new(SkippedInspector),
        ];
        let pipeline = collect(&source, &exec, &inspectors);

        match &pipeline.state.snapshot.completeness {
            Completeness::Incomplete {
                failed_sections,
                degraded_sections,
                ..
            } => {
                assert_eq!(failed_sections.len(), 1, "one inspector failed");
                assert!(failed_sections.contains(&InspectorId::Config));
                assert_eq!(
                    degraded_sections.len(),
                    1,
                    "one inspector degraded (skipped is not degraded)"
                );
                assert!(degraded_sections.contains(&InspectorId::Network));
            }
            other => panic!("expected Incomplete, got {other:?}"),
        }
    }
}
