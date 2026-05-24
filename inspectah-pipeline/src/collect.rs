use std::sync::atomic::{AtomicBool, Ordering};

use inspectah_core::baseline::BaselineData;
use inspectah_core::pipeline::{Collected, Pipeline};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::executor::Executor;
use inspectah_core::traits::inspector::{
    InspectionContext, Inspector, InspectorError, InspectorOutput, RpmState,
};
use inspectah_core::traits::progress::ProgressSink;
use inspectah_core::types::completeness::{Completeness, InspectorId, SectionData};
use inspectah_core::types::progress::{InspectorOutcome, ProgressEvent};
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
    baseline: Option<&BaselineData>,
    progress: &dyn ProgressSink,
    cancelled: &AtomicBool,
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
            progress.emit(ProgressEvent::InspectorFinished {
                id: inspector.id(),
                outcome: InspectorOutcome::Skipped {
                    reason: format!("not applicable to {source_kind:?}"),
                },
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

    // Partition applicable inspectors into Wave 1 (independent) and
    // Wave 2 (rpm_state-dependent). RPM always runs in Wave 1.
    // Wave 2 inspectors receive enriched context with rpm_state.
    let mut wave1: Vec<&Box<dyn Inspector>> = Vec::new();
    let mut wave2: Vec<&Box<dyn Inspector>> = Vec::new();
    for insp in &applicable {
        if is_wave2(insp.id()) {
            wave2.push(insp);
        } else {
            wave1.push(insp);
        }
    }

    // Wave 1 base context — no rpm_state available yet.
    let base_ctx = InspectionContext {
        source_system: source,
        executor,
        rpm_state: None,
        baseline_data: baseline,
    };

    // Wave 1: parallel execution via std::thread::scope.
    let mut rpm_state = RpmState::default();
    let mut rpm_populated = false;

    std::thread::scope(|s| {
        let handles: Vec<_> = wave1
            .iter()
            .map(|inspector| {
                s.spawn(|| {
                    progress.emit(ProgressEvent::InspectorStarted(inspector.id()));
                    inspector.inspect(&base_ctx, progress)
                })
            })
            .collect();

        for (inspector, handle) in wave1.iter().zip(handles) {
            let was_rpm = handle_result(
                inspector.as_ref(),
                handle,
                &mut snapshot,
                &mut failed,
                &mut degraded,
                &mut rpm_state,
                progress,
            );
            if was_rpm {
                rpm_populated = true;
            }
        }
    });

    // Wave 2: dependent inspectors get enriched context with rpm_state.
    // rpm_state is Some when RPM succeeded (Ok or Degraded), None when
    // RPM failed entirely — Wave 2 inspectors use this to distinguish
    // "no data, can't classify" from "confirmed no RPM-owned paths."
    //
    // Cancellation check: if SIGINT arrived during wave 1, skip wave 2
    // entirely and emit Interrupted for all wave-2 inspectors.
    if !wave2.is_empty() && !cancelled.load(Ordering::SeqCst) {
        let wave2_rpm_state: Option<&RpmState> = if rpm_populated {
            Some(&rpm_state)
        } else {
            None
        };

        let enriched_ctx = InspectionContext {
            source_system: source,
            executor,
            rpm_state: wave2_rpm_state,
            baseline_data: baseline,
        };

        // Wave 2 does not mutate rpm_state — it only reads it.
        // A separate mutable tracker is used for completeness bookkeeping.
        //
        // Per-spawn cancellation: check `cancelled` before each spawn so
        // SIGINT between spawns prevents launching further inspectors.
        // Inspectors that were never started are simply omitted — the CLI
        // layer is responsible for emitting Interrupted for them.
        std::thread::scope(|s| {
            let handles: Vec<_> = wave2
                .iter()
                .filter_map(|inspector| {
                    if cancelled.load(Ordering::SeqCst) {
                        return None; // don't spawn
                    }
                    progress.emit(ProgressEvent::InspectorStarted(inspector.id()));
                    Some((inspector, s.spawn(|| inspector.inspect(&enriched_ctx, progress))))
                })
                .collect();

            // Wave 2 inspectors don't produce RpmState, so pass a
            // throwaway — the real rpm_state is already finalized.
            let mut wave2_rpm = RpmState::default();
            for (inspector, handle) in handles {
                handle_result(
                    inspector.as_ref(),
                    handle,
                    &mut snapshot,
                    &mut failed,
                    &mut degraded,
                    &mut wave2_rpm,
                    progress,
                );
            }
        });
    } else if !wave2.is_empty() {
        // Cancelled before wave 2 started — no action needed here.
        // The CLI layer emits Interrupted for inspectors not in the snapshot.
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
/// warnings/failures, collect redaction hints, and extract RpmState when
/// the RPM inspector succeeds.
///
/// Returns `true` when the RPM inspector's output was successfully
/// extracted into `rpm_state` (both Ok and Degraded paths).
fn handle_result(
    inspector: &dyn Inspector,
    handle: std::thread::ScopedJoinHandle<'_, Result<InspectorOutput, InspectorError>>,
    snapshot: &mut InspectionSnapshot,
    failed: &mut Vec<InspectorId>,
    degraded: &mut Vec<InspectorId>,
    rpm_state: &mut RpmState,
    progress: &dyn ProgressSink,
) -> bool {
    let mut rpm_extracted = false;

    match handle.join() {
        Ok(Ok(output)) => {
            // Extract RpmState from RPM inspector output before routing
            if inspector.id() == InspectorId::Rpm
                && let SectionData::Rpm(ref rpm) = output.section
            {
                extract_rpm_state(rpm, rpm_state);
                rpm_extracted = true;
            }
            route_section(snapshot, output.section);
            snapshot.warnings.extend(output.warnings);
            snapshot.redaction_hints.extend(output.redaction_hints);
            progress.emit(ProgressEvent::InspectorFinished {
                id: inspector.id(),
                outcome: InspectorOutcome::Complete,
            });
        }
        Ok(Err(InspectorError::Skipped { reason })) => {
            snapshot.warnings.push(Warning {
                inspector: format!("{:?}", inspector.id()),
                message: format!("skipped: {reason}"),
                severity: Some(WarningSeverity::Info),
                ..Default::default()
            });
            progress.emit(ProgressEvent::InspectorFinished {
                id: inspector.id(),
                outcome: InspectorOutcome::Skipped {
                    reason: reason.clone(),
                },
            });
        }
        Ok(Err(InspectorError::Degraded {
            partial, reason, ..
        })) => {
            // Extract RpmState from degraded RPM output too — partial
            // data is still valid for Wave 2 classification.
            if inspector.id() == InspectorId::Rpm
                && let SectionData::Rpm(ref rpm) = partial.section
            {
                extract_rpm_state(rpm, rpm_state);
                rpm_extracted = true;
            }
            route_section(snapshot, partial.section);
            snapshot.warnings.extend(partial.warnings);
            snapshot.redaction_hints.extend(partial.redaction_hints);
            snapshot.warnings.push(Warning {
                inspector: format!("{:?}", inspector.id()),
                message: format!("degraded: {reason}"),
                severity: Some(WarningSeverity::Warning),
                ..Default::default()
            });
            degraded.push(inspector.id());
            progress.emit(ProgressEvent::InspectorFinished {
                id: inspector.id(),
                outcome: InspectorOutcome::Degraded {
                    reason: reason.clone(),
                },
            });
        }
        Ok(Err(InspectorError::Failed { reason })) => {
            snapshot.warnings.push(Warning {
                inspector: format!("{:?}", inspector.id()),
                message: format!("failed: {reason}"),
                severity: Some(WarningSeverity::Error),
                ..Default::default()
            });
            failed.push(inspector.id());
            progress.emit(ProgressEvent::InspectorFinished {
                id: inspector.id(),
                outcome: InspectorOutcome::Failed {
                    reason: reason.clone(),
                },
            });
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
            progress.emit(ProgressEvent::InspectorFinished {
                id: inspector.id(),
                outcome: InspectorOutcome::Failed {
                    reason: "inspector panicked".into(),
                },
            });
        }
    }

    rpm_extracted
}

/// Populate RpmState from an RpmSection's data.
///
/// Extracts installed package names, full package list, verification
/// results, module streams, and file ownership. Builds `owned_paths`
/// (filtered to `/etc`) and `path_to_package` reverse index from the
/// file ownership data produced by the RPM inspector.
fn extract_rpm_state(rpm: &inspectah_core::types::rpm::RpmSection, state: &mut RpmState) {
    state.installed_packages = rpm.packages_added.iter().map(|p| p.name.clone()).collect();
    state.packages = rpm.packages_added.clone();
    state.verification_results = rpm.rpm_va.clone();
    state.module_streams = rpm.module_streams.clone();

    // Build owned_paths and path_to_package from file ownership data.
    // owned_paths: only /etc paths (matches Go's BuildRpmOwnedPaths filter).
    // path_to_package: maps each /etc path to its owning package's index
    // in state.packages.
    //
    // Also build a full path→package_name map (all paths, not just /etc)
    // for RpmVaEntry.package attribution below.
    let pkg_index: std::collections::HashMap<&str, usize> = state
        .packages
        .iter()
        .enumerate()
        .map(|(i, p)| (p.name.as_str(), i))
        .collect();

    let mut path_to_name: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();

    for entry in &rpm.file_ownership {
        let pkg_idx = pkg_index.get(entry.package_name.as_str()).copied();
        for path_str in &entry.paths {
            path_to_name.insert(path_str.as_str(), entry.package_name.as_str());
            if path_str.starts_with("/etc") {
                let path = std::path::PathBuf::from(path_str);
                state.owned_paths.insert(path.clone());
                if let Some(idx) = pkg_idx {
                    state.path_to_package.insert(path, idx);
                }
            }
        }
    }

    // Annotate RpmVaEntry.package from file ownership data.
    // rpm -Va output has paths but not package names; cross-reference
    // against the ownership index to fill in the owning package.
    for va in &mut state.verification_results {
        if va.package.is_none()
            && let Some(&pkg_name) = path_to_name.get(va.path.as_str())
        {
            va.package = Some(pkg_name.to_string());
        }
    }
}

/// Classify whether an inspector belongs to Wave 2 (depends on RPM state).
fn is_wave2(id: InspectorId) -> bool {
    !matches!(id, InspectorId::Rpm)
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
    use std::sync::atomic::AtomicBool;
    use inspectah_collect::executor::mock::MockExecutor;
    use inspectah_collect::inspectors::config::ConfigInspector;
    use inspectah_collect::inspectors::rpm::RpmInspector;
    use inspectah_core::traits::executor::ExecResult;
    use inspectah_core::traits::inspector::InspectorOutput;
    use inspectah_core::traits::progress::{NullProgress, ProgressSink, VecProgress};
    use inspectah_core::types::completeness::SourceSystemKind;
    use inspectah_core::types::config::ConfigFileKind;
    use inspectah_core::types::os::OsRelease;
    use inspectah_core::types::progress::{InspectorOutcome, ProgressEvent};
    use inspectah_core::types::redaction::{Confidence, RedactionHint};
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
        fn inspect(
            &self,
            _ctx: &InspectionContext<'_>,
            _progress: &dyn ProgressSink,
        ) -> Result<InspectorOutput, InspectorError> {
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
        fn inspect(
            &self,
            _ctx: &InspectionContext<'_>,
            _progress: &dyn ProgressSink,
        ) -> Result<InspectorOutput, InspectorError> {
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
        fn inspect(
            &self,
            _ctx: &InspectionContext<'_>,
            _progress: &dyn ProgressSink,
        ) -> Result<InspectorOutput, InspectorError> {
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
        let pipeline = collect(&source, &exec, &inspectors, None, &NullProgress, &AtomicBool::new(false));

        // Pipeline produced a Collected state with rpm data
        assert!(pipeline.state.snapshot.rpm.is_some());
        let rpm = pipeline.state.snapshot.rpm.unwrap();
        assert_eq!(rpm.packages_added.len(), 2);

        // Warnings should include the no-baseline warning from RpmInspector
        assert!(
            pipeline
                .state
                .snapshot
                .warnings
                .iter()
                .any(|w| w.message.contains("no baseline"))
        );
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
        let pipeline = collect(&source, &exec, &inspectors, None, &NullProgress, &AtomicBool::new(false));

        // rpm section should be None (failed)
        assert!(pipeline.state.snapshot.rpm.is_none());

        // Should have an error warning about the failure
        assert!(
            pipeline
                .state
                .snapshot
                .warnings
                .iter()
                .any(|w| w.message.contains("failed"))
        );
    }

    #[test]
    fn test_collect_routes_section_data_correctly() {
        let exec = build_test_mock();
        let source = SourceSystem::PackageBased {
            os_release: test_os_release(),
        };
        let inspectors: Vec<Box<dyn Inspector>> = vec![Box::new(RpmInspector::new())];
        let pipeline = collect(&source, &exec, &inspectors, None, &NullProgress, &AtomicBool::new(false));

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
        let pipeline = collect(&source, &exec, &inspectors, None, &NullProgress, &AtomicBool::new(false));
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
        let pipeline = collect(&source, &exec, &inspectors, None, &NullProgress, &AtomicBool::new(false));
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
        let pipeline = collect(&source, &exec, &inspectors, None, &NullProgress, &AtomicBool::new(false));

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
        let pipeline = collect(&source, &exec, &inspectors, None, &NullProgress, &AtomicBool::new(false));

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
        let pipeline = collect(&source, &exec, &inspectors, None, &NullProgress, &AtomicBool::new(false));

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
        let pipeline = collect(&source, &exec, &inspectors, None, &NullProgress, &AtomicBool::new(false));

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
        let pipeline = collect(&source, &exec, &inspectors, None, &NullProgress, &AtomicBool::new(false));

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

    // --- Redaction hints wiring tests ---

    /// Mock inspector that returns Ok with redaction hints.
    struct HintingInspector;
    impl Inspector for HintingInspector {
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
            Ok(InspectorOutput {
                section: SectionData::Services(Default::default()),
                warnings: vec![],
                redaction_hints: vec![RedactionHint {
                    path: "/etc/systemd/system/app.service.d/env.conf".into(),
                    reason: "Environment variable DB_PASSWORD may contain a secret".into(),
                    confidence: Some(Confidence::Medium),
                }],
            })
        }
    }

    /// Mock inspector that returns Degraded with redaction hints in partial output.
    struct DegradedWithHintsInspector;
    impl Inspector for DegradedWithHintsInspector {
        fn id(&self) -> InspectorId {
            InspectorId::KernelBoot
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
                    section: SectionData::KernelBoot(Default::default()),
                    warnings: vec![],
                    redaction_hints: vec![RedactionHint {
                        path: "/proc/cmdline".into(),
                        reason: "kernel cmdline contains password=".into(),
                        confidence: Some(Confidence::High),
                    }],
                }),
                reason: "partial data".into(),
            })
        }
    }

    #[test]
    fn test_hints_wired_from_ok_inspector() {
        let exec = build_test_mock();
        let source = SourceSystem::PackageBased {
            os_release: test_os_release(),
        };
        let inspectors: Vec<Box<dyn Inspector>> =
            vec![Box::new(RpmInspector::new()), Box::new(HintingInspector)];
        let pipeline = collect(&source, &exec, &inspectors, None, &NullProgress, &AtomicBool::new(false));

        assert_eq!(
            pipeline.state.snapshot.redaction_hints.len(),
            1,
            "redaction hints from Ok inspector must be wired to snapshot"
        );
        assert!(
            pipeline.state.snapshot.redaction_hints[0]
                .reason
                .contains("DB_PASSWORD"),
            "hint content must be preserved"
        );
    }

    #[test]
    fn test_hints_wired_from_degraded_inspector() {
        let exec = build_test_mock();
        let source = SourceSystem::PackageBased {
            os_release: test_os_release(),
        };
        let inspectors: Vec<Box<dyn Inspector>> = vec![
            Box::new(RpmInspector::new()),
            Box::new(DegradedWithHintsInspector),
        ];
        let pipeline = collect(&source, &exec, &inspectors, None, &NullProgress, &AtomicBool::new(false));

        assert_eq!(
            pipeline.state.snapshot.redaction_hints.len(),
            1,
            "redaction hints from Degraded inspector's partial output must be wired to snapshot"
        );
        assert!(
            pipeline.state.snapshot.redaction_hints[0]
                .reason
                .contains("password="),
            "hint content must be preserved from degraded output"
        );
    }

    // --- Wave 2 classifier and dispatch tests ---

    #[test]
    fn test_is_wave2_classifier() {
        // Only RPM is wave 1
        assert!(!is_wave2(InspectorId::Rpm));

        // All others are wave 2
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
    }

    /// Mock Wave 2 inspector that records whether it received rpm_state.
    /// Returns Ok with a ScheduledTasks section, capturing the rpm_state
    /// presence in a thread-safe flag.
    struct Wave2ProbeInspector {
        received_rpm_state: std::sync::Arc<std::sync::Mutex<Option<bool>>>,
    }

    impl Wave2ProbeInspector {
        fn new() -> (Self, std::sync::Arc<std::sync::Mutex<Option<bool>>>) {
            let flag = std::sync::Arc::new(std::sync::Mutex::new(None));
            (
                Self {
                    received_rpm_state: flag.clone(),
                },
                flag,
            )
        }
    }

    impl Inspector for Wave2ProbeInspector {
        fn id(&self) -> InspectorId {
            InspectorId::ScheduledTasks
        }
        fn applicable_to(&self) -> &[SourceSystemKind] {
            &[SourceSystemKind::PackageBased]
        }
        fn inspect(
            &self,
            ctx: &InspectionContext<'_>,
            _progress: &dyn ProgressSink,
        ) -> Result<InspectorOutput, InspectorError> {
            let has_state = ctx.rpm_state.is_some();
            *self.received_rpm_state.lock().unwrap() = Some(has_state);

            if let Some(rpm_state) = ctx.rpm_state {
                // Verify the rpm_state has actual data when present
                if !rpm_state.installed_packages().is_empty() {
                    Ok(InspectorOutput {
                        section: SectionData::ScheduledTasks(Default::default()),
                        warnings: vec![],
                        redaction_hints: vec![],
                    })
                } else {
                    Ok(InspectorOutput {
                        section: SectionData::ScheduledTasks(Default::default()),
                        warnings: vec![Warning {
                            inspector: "ScheduledTasks".into(),
                            message: "rpm_state present but empty".into(),
                            severity: Some(WarningSeverity::Info),
                            ..Default::default()
                        }],
                        redaction_hints: vec![],
                    })
                }
            } else {
                Err(InspectorError::Failed {
                    reason: "rpm_state is None — RPM failed".into(),
                })
            }
        }
    }

    #[test]
    fn test_wave2_receives_rpm_state() {
        let exec = build_test_mock();
        let source = SourceSystem::PackageBased {
            os_release: test_os_release(),
        };

        let (probe, flag) = Wave2ProbeInspector::new();
        let inspectors: Vec<Box<dyn Inspector>> =
            vec![Box::new(RpmInspector::new()), Box::new(probe)];
        let pipeline = collect(&source, &exec, &inspectors, None, &NullProgress, &AtomicBool::new(false));

        // The probe should have been called (it's a Wave 2 inspector)
        let received = flag.lock().unwrap();
        assert_eq!(
            *received,
            Some(true),
            "Wave 2 inspector must receive rpm_state when RPM succeeds"
        );

        // ScheduledTasks section should be routed
        assert!(
            pipeline.state.snapshot.scheduled_tasks.is_some(),
            "Wave 2 inspector output must be routed to snapshot"
        );
    }

    #[test]
    fn test_wave2_receives_none_when_rpm_fails() {
        // Mock executor that returns empty rpm output -> RPM inspector fails
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

        let (probe, flag) = Wave2ProbeInspector::new();
        let inspectors: Vec<Box<dyn Inspector>> =
            vec![Box::new(RpmInspector::new()), Box::new(probe)];
        let _pipeline = collect(&source, &exec, &inspectors, None, &NullProgress, &AtomicBool::new(false));

        // The probe should have received None (RPM failed)
        let received = flag.lock().unwrap();
        assert_eq!(
            *received,
            Some(false),
            "Wave 2 inspector must receive rpm_state=None when RPM fails"
        );
    }

    #[test]
    fn test_rpm_state_populated_with_packages() {
        let exec = build_test_mock();
        let source = SourceSystem::PackageBased {
            os_release: test_os_release(),
        };

        // Use a probe to verify rpm_state contents
        let (probe, _flag) = Wave2ProbeInspector::new();
        let inspectors: Vec<Box<dyn Inspector>> =
            vec![Box::new(RpmInspector::new()), Box::new(probe)];
        let pipeline = collect(&source, &exec, &inspectors, None, &NullProgress, &AtomicBool::new(false));

        // RPM section should be present with packages
        let rpm = pipeline.state.snapshot.rpm.as_ref().unwrap();
        assert_eq!(rpm.packages_added.len(), 2, "RPM should have 2 packages");

        // ScheduledTasks should succeed (rpm_state was populated)
        assert!(
            pipeline.state.snapshot.scheduled_tasks.is_some(),
            "Wave 2 inspector should succeed with populated rpm_state"
        );
    }

    // -----------------------------------------------------------------------
    // Regression: live collect path populates owned_paths from file ownership
    // -----------------------------------------------------------------------

    /// Mock Wave 2 inspector that captures the full RpmState for assertion.
    struct OwnershipProbeInspector {
        captured: std::sync::Arc<std::sync::Mutex<Option<RpmState>>>,
    }

    impl OwnershipProbeInspector {
        fn new() -> (Self, std::sync::Arc<std::sync::Mutex<Option<RpmState>>>) {
            let captured = std::sync::Arc::new(std::sync::Mutex::new(None));
            (
                Self {
                    captured: captured.clone(),
                },
                captured,
            )
        }
    }

    impl Inspector for OwnershipProbeInspector {
        fn id(&self) -> InspectorId {
            InspectorId::ScheduledTasks
        }
        fn applicable_to(&self) -> &[SourceSystemKind] {
            &[SourceSystemKind::PackageBased]
        }
        fn inspect(
            &self,
            ctx: &InspectionContext<'_>,
            _progress: &dyn ProgressSink,
        ) -> Result<InspectorOutput, InspectorError> {
            if let Some(rpm_state) = ctx.rpm_state {
                *self.captured.lock().unwrap() = Some(rpm_state.clone());
                Ok(InspectorOutput {
                    section: SectionData::ScheduledTasks(Default::default()),
                    warnings: vec![],
                    redaction_hints: vec![],
                })
            } else {
                Err(InspectorError::Failed {
                    reason: "rpm_state is None".into(),
                })
            }
        }
    }

    /// Build a MockExecutor with RPM data including file ownership for
    /// end-to-end regression testing.
    fn build_ownership_mock() -> MockExecutor {
        let rpm_qa_output = "\
0:bash-5.2.26-3.el9.x86_64
0:httpd-2.4.57-5.el9.x86_64
0:cronie-1.5.7-8.el9.x86_64
0:pam-1.5.1-14.el9.x86_64
";
        // File ownership: sentinel format (@@name header + paths).
        // Includes /etc paths (for owned_paths) and non-/etc (filtered out).
        let file_ownership_output = "\
@@bash
/etc/profile.d/bash_completion.sh
/usr/bin/bash
@@httpd
/etc/httpd/conf/httpd.conf
/etc/httpd/conf.d/ssl.conf
/usr/sbin/httpd
@@cronie
/etc/cron.d/0hourly
/etc/cron.daily/logrotate
/usr/sbin/crond
@@pam
/etc/pam.d/system-auth
/etc/pam.d/password-auth
/etc/security/limits.conf
";
        MockExecutor::new()
            .with_command(
                "rpm -qa --queryformat %{EPOCH}:%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\n",
                ExecResult {
                    stdout: rpm_qa_output.into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                "rpm -qa --queryformat @@%{NAME}\\n[%{FILENAMES}\\n]",
                ExecResult {
                    stdout: file_ownership_output.into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
    }

    #[test]
    fn test_collect_populates_owned_paths_from_file_ownership() {
        let exec = build_ownership_mock();
        let source = SourceSystem::PackageBased {
            os_release: test_os_release(),
        };

        let (probe, captured) = OwnershipProbeInspector::new();
        let inspectors: Vec<Box<dyn Inspector>> =
            vec![Box::new(RpmInspector::new()), Box::new(probe)];
        let pipeline = collect(&source, &exec, &inspectors, None, &NullProgress, &AtomicBool::new(false));

        // RPM section should contain file_ownership data
        let rpm = pipeline
            .state
            .snapshot
            .rpm
            .as_ref()
            .expect("RPM section present");
        assert!(
            !rpm.file_ownership.is_empty(),
            "file_ownership must be populated in production path"
        );

        // Wave 2 probe should have received populated RpmState
        let rpm_state = captured
            .lock()
            .unwrap()
            .clone()
            .expect("OwnershipProbe must receive RpmState");

        // owned_paths should contain /etc paths only
        assert!(
            rpm_state.is_rpm_owned(std::path::Path::new("/etc/httpd/conf/httpd.conf")),
            "httpd config must be RPM-owned"
        );
        assert!(
            rpm_state.is_rpm_owned(std::path::Path::new("/etc/cron.d/0hourly")),
            "cronie cron file must be RPM-owned"
        );
        assert!(
            rpm_state.is_rpm_owned(std::path::Path::new("/etc/pam.d/system-auth")),
            "pam config must be RPM-owned"
        );
        assert!(
            rpm_state.is_rpm_owned(std::path::Path::new("/etc/profile.d/bash_completion.sh")),
            "bash profile script must be RPM-owned"
        );

        // Non-/etc paths should NOT be in owned_paths
        assert!(
            !rpm_state.is_rpm_owned(std::path::Path::new("/usr/bin/bash")),
            "/usr paths must not be in owned_paths"
        );
        assert!(
            !rpm_state.is_rpm_owned(std::path::Path::new("/usr/sbin/httpd")),
            "/usr paths must not be in owned_paths"
        );

        // path_to_package should map /etc paths to correct package indices
        let httpd_pkg = rpm_state
            .package_for_path(std::path::Path::new("/etc/httpd/conf/httpd.conf"))
            .expect("httpd config must map to a package");
        assert_eq!(httpd_pkg.name, "httpd");

        let cronie_pkg = rpm_state
            .package_for_path(std::path::Path::new("/etc/cron.d/0hourly"))
            .expect("cronie cron must map to a package");
        assert_eq!(cronie_pkg.name, "cronie");

        let pam_pkg = rpm_state
            .package_for_path(std::path::Path::new("/etc/pam.d/system-auth"))
            .expect("pam config must map to a package");
        assert_eq!(pam_pkg.name, "pam");

        // Unowned path should return None
        assert!(
            rpm_state
                .package_for_path(std::path::Path::new("/etc/custom/app.conf"))
                .is_none()
        );
    }

    #[test]
    fn test_rpm_va_package_attribution_from_file_ownership() {
        // Build mock with rpm -Va output AND file ownership data,
        // then verify that extract_rpm_state populates RpmVaEntry.package.
        let rpm_qa_output = "\
0:httpd-2.4.57-5.el9.x86_64
0:bash-5.2.26-3.el9.x86_64
";
        let file_ownership_output = "\
@@httpd
/etc/httpd/conf/httpd.conf
/usr/sbin/httpd
@@bash
/etc/profile.d/bash_completion.sh
";
        let exec = MockExecutor::new()
            .with_command(
                "rpm -qa --queryformat %{EPOCH}:%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\n",
                ExecResult {
                    stdout: rpm_qa_output.into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                "rpm -qa --queryformat @@%{NAME}\\n[%{FILENAMES}\\n]",
                ExecResult {
                    stdout: file_ownership_output.into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                "rpm -Va",
                ExecResult {
                    stdout: "S.5....T.  c /etc/httpd/conf/httpd.conf\n".into(),
                    exit_code: 1,
                    ..Default::default()
                },
            );

        let source = SourceSystem::PackageBased {
            os_release: test_os_release(),
        };

        let (probe, captured) = OwnershipProbeInspector::new();
        let inspectors: Vec<Box<dyn Inspector>> =
            vec![Box::new(RpmInspector::new()), Box::new(probe)];
        let _pipeline = collect(&source, &exec, &inspectors, None, &NullProgress, &AtomicBool::new(false));

        let rpm_state = captured
            .lock()
            .unwrap()
            .clone()
            .expect("probe must capture RpmState");

        // rpm -Va entry for httpd.conf should have package attribution
        let httpd_va = rpm_state
            .verification_results()
            .iter()
            .find(|v| v.path == "/etc/httpd/conf/httpd.conf")
            .expect("httpd.conf should be in verification results");
        assert_eq!(
            httpd_va.package.as_deref(),
            Some("httpd"),
            "RpmVaEntry.package should be populated from file ownership"
        );
    }

    // -----------------------------------------------------------------------
    // End-to-end: RPM + Config inspectors through collect pipeline
    // -----------------------------------------------------------------------

    /// Build a MockExecutor that satisfies both RpmInspector and ConfigInspector,
    /// with a mix of RPM-owned and unowned files in /etc.
    fn build_e2e_config_mock() -> MockExecutor {
        let rpm_qa_output = "\
0:setup-2.13.7-10.el9.noarch
0:bash-5.2.26-3.el9.x86_64
0:httpd-2.4.57-5.el9.x86_64
";
        // File ownership: setup owns /etc/bashrc, /etc/profile, /etc/hosts, /etc/services.
        // httpd owns /etc/httpd/conf/httpd.conf.
        // bash owns /etc/profile.d/bash_completion.sh.
        // Non-/etc paths are included but should be filtered to owned_paths.
        let file_ownership_output = "\
@@setup
/etc/bashrc
/etc/profile
/etc/hosts
/etc/services
@@httpd
/etc/httpd/conf/httpd.conf
/usr/sbin/httpd
@@bash
/etc/profile.d/bash_completion.sh
/usr/bin/bash
";
        MockExecutor::new()
            // RPM package query
            .with_command(
                "rpm -qa --queryformat %{EPOCH}:%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\n",
                ExecResult {
                    stdout: rpm_qa_output.into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            // RPM file ownership query
            .with_command(
                "rpm -qa --queryformat @@%{NAME}\\n[%{FILENAMES}\\n]",
                ExecResult {
                    stdout: file_ownership_output.into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            // /etc directory tree: mix of RPM-owned and unowned files
            .with_dir(
                "/etc",
                vec![
                    "bashrc",
                    "profile",
                    "hosts",
                    "services",
                    "httpd",
                    "profile.d",
                    "custom-app.conf",
                    "myapp",
                ],
            )
            .with_dir("/etc/httpd", vec!["conf"])
            .with_dir("/etc/httpd/conf", vec!["httpd.conf"])
            .with_dir("/etc/profile.d", vec!["bash_completion.sh"])
            .with_dir("/etc/myapp", vec!["config.yaml"])
            // RPM-owned files (should be filtered OUT of config output)
            .with_file("/etc/bashrc", "# /etc/bashrc\n")
            .with_file("/etc/profile", "# /etc/profile\n")
            .with_file("/etc/hosts", "127.0.0.1 localhost\n")
            .with_file("/etc/services", "# /etc/services\n")
            .with_file("/etc/httpd/conf/httpd.conf", "ServerRoot /etc/httpd\n")
            .with_file("/etc/profile.d/bash_completion.sh", "# bash completion\n")
            // Genuinely unowned files (should REMAIN in config output)
            .with_file("/etc/custom-app.conf", "setting=value\n")
            .with_file("/etc/myapp/config.yaml", "key: value\n")
            // dnf history (config inspector needs this for orphan detection)
            .with_command(
                "dnf history list --reverse",
                ExecResult {
                    exit_code: 1,
                    ..Default::default()
                },
            )
    }

    #[test]
    fn test_e2e_config_filters_rpm_owned_files() {
        let exec = build_e2e_config_mock();
        let source = SourceSystem::PackageBased {
            os_release: test_os_release(),
        };

        let inspectors: Vec<Box<dyn Inspector>> = vec![
            Box::new(RpmInspector::new()),
            Box::new(ConfigInspector::new()),
        ];
        let pipeline = collect(&source, &exec, &inspectors, None, &NullProgress, &AtomicBool::new(false));
        let snap = &pipeline.state.snapshot;

        // RPM section must be present (Wave 1 succeeded)
        assert!(snap.rpm.is_some(), "RPM section must be populated");
        let rpm = snap.rpm.as_ref().unwrap();
        assert!(
            !rpm.file_ownership.is_empty(),
            "file_ownership must be populated for owned_paths extraction"
        );

        // Config section must be present (Wave 2 succeeded)
        assert!(snap.config.is_some(), "Config section must be populated");
        let config = snap.config.as_ref().unwrap();

        // Collect paths and kinds for assertions
        let config_paths: Vec<&str> = config.files.iter().map(|f| f.path.as_str()).collect();

        // RPM-owned files must NOT appear (they should be filtered by is_rpm_owned)
        assert!(
            !config_paths.contains(&"/etc/bashrc"),
            "/etc/bashrc is RPM-owned (setup) — must not appear as Unowned"
        );
        assert!(
            !config_paths.contains(&"/etc/profile"),
            "/etc/profile is RPM-owned (setup) — must not appear as Unowned"
        );
        assert!(
            !config_paths.contains(&"/etc/hosts"),
            "/etc/hosts is RPM-owned (setup) — must not appear as Unowned"
        );
        assert!(
            !config_paths.contains(&"/etc/services"),
            "/etc/services is RPM-owned (setup) — must not appear as Unowned"
        );
        assert!(
            !config_paths.contains(&"/etc/httpd/conf/httpd.conf"),
            "/etc/httpd/conf/httpd.conf is RPM-owned (httpd) — must not appear as Unowned"
        );
        assert!(
            !config_paths.contains(&"/etc/profile.d/bash_completion.sh"),
            "/etc/profile.d/bash_completion.sh is RPM-owned (bash) — must not appear as Unowned"
        );

        // Genuinely unowned files MUST appear with kind Unowned
        let custom_app = config
            .files
            .iter()
            .find(|f| f.path == "/etc/custom-app.conf")
            .expect("/etc/custom-app.conf must appear as unowned");
        assert_eq!(
            custom_app.kind,
            ConfigFileKind::Unowned,
            "custom-app.conf must be classified as Unowned"
        );

        let myapp_config = config
            .files
            .iter()
            .find(|f| f.path == "/etc/myapp/config.yaml")
            .expect("/etc/myapp/config.yaml must appear as unowned");
        assert_eq!(
            myapp_config.kind,
            ConfigFileKind::Unowned,
            "myapp/config.yaml must be classified as Unowned"
        );

        // Only unowned files should be in the output
        assert_eq!(
            config.files.len(),
            2,
            "only the 2 genuinely unowned files should appear (not the 6 RPM-owned ones)"
        );
    }

    // --- Inspector lifecycle event tests ---

    #[test]
    fn test_collect_emits_inspector_lifecycle_events() {
        let exec = build_test_mock();
        let source = SourceSystem::PackageBased {
            os_release: test_os_release(),
        };
        let progress = VecProgress::new();
        let inspectors: Vec<Box<dyn Inspector>> = vec![Box::new(RpmInspector::new())];
        let _pipeline = collect(&source, &exec, &inspectors, None, &progress, &AtomicBool::new(false));

        let events = progress.events();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ProgressEvent::InspectorStarted(InspectorId::Rpm))),
            "must emit InspectorStarted for Rpm"
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                ProgressEvent::InspectorFinished {
                    id: InspectorId::Rpm,
                    outcome: InspectorOutcome::Complete
                }
            )),
            "must emit InspectorFinished/Complete for Rpm"
        );
    }

    #[test]
    fn test_collect_emits_skipped_for_inapplicable() {
        let exec = build_test_mock();
        let source = SourceSystem::PackageBased {
            os_release: test_os_release(),
        };
        let progress = VecProgress::new();
        let inspectors: Vec<Box<dyn Inspector>> = vec![
            Box::new(RpmInspector::new()),
            Box::new(SkippedInspector),
        ];
        let _pipeline = collect(&source, &exec, &inspectors, None, &progress, &AtomicBool::new(false));

        let events = progress.events();
        assert!(
            events.iter().any(|e| matches!(
                e,
                ProgressEvent::InspectorFinished {
                    id: InspectorId::Storage,
                    outcome: InspectorOutcome::Skipped { .. }
                }
            )),
            "must emit InspectorFinished/Skipped for SkippedInspector"
        );
    }

    #[test]
    fn test_collect_emits_failed_outcome() {
        let exec = build_test_mock();
        let source = SourceSystem::PackageBased {
            os_release: test_os_release(),
        };
        let progress = VecProgress::new();
        let inspectors: Vec<Box<dyn Inspector>> = vec![
            Box::new(RpmInspector::new()),
            Box::new(FailingInspector),
        ];
        let _pipeline = collect(&source, &exec, &inspectors, None, &progress, &AtomicBool::new(false));

        let events = progress.events();
        assert!(
            events.iter().any(|e| matches!(
                e,
                ProgressEvent::InspectorFinished {
                    id: InspectorId::Config,
                    outcome: InspectorOutcome::Failed { .. }
                }
            )),
            "must emit InspectorFinished/Failed for FailingInspector"
        );
    }

    // --- SIGINT cancellation tests ---

    /// RPM inspector wrapper that sets a cancellation flag after completing.
    /// This simulates SIGINT arriving between wave 1 and wave 2.
    struct CancellingRpmInspector {
        inner: RpmInspector,
        flag: std::sync::Arc<AtomicBool>,
    }

    impl CancellingRpmInspector {
        fn new(flag: std::sync::Arc<AtomicBool>) -> Self {
            Self {
                inner: RpmInspector::new(),
                flag,
            }
        }
    }

    impl Inspector for CancellingRpmInspector {
        fn id(&self) -> InspectorId {
            InspectorId::Rpm
        }
        fn applicable_to(&self) -> &[SourceSystemKind] {
            self.inner.applicable_to()
        }
        fn inspect(
            &self,
            ctx: &InspectionContext<'_>,
            progress: &dyn ProgressSink,
        ) -> Result<InspectorOutput, InspectorError> {
            let result = self.inner.inspect(ctx, progress);
            // Simulate SIGINT arriving right after wave 1 completes
            self.flag.store(true, Ordering::SeqCst);
            result
        }
    }

    #[test]
    fn test_collect_skips_wave2_when_cancelled_between_waves() {
        use inspectah_collect::inspectors::services::ServicesInspector;

        let exec = build_test_mock();
        let source = SourceSystem::PackageBased {
            os_release: test_os_release(),
        };
        let cancelled = std::sync::Arc::new(AtomicBool::new(false));
        let progress = VecProgress::new();

        let inspectors: Vec<Box<dyn Inspector>> = vec![
            Box::new(CancellingRpmInspector::new(cancelled.clone())),
            Box::new(ServicesInspector::new()),
        ];
        let pipeline = collect(&source, &exec, &inspectors, None, &progress, &cancelled);

        // RPM completed in wave 1 (before flag was set)
        assert!(
            pipeline.state.snapshot.rpm.is_some(),
            "RPM ran in wave 1 before cancellation — must have data"
        );

        // Services is wave 2 — should have been skipped due to cancellation
        assert!(
            pipeline.state.snapshot.services.is_none(),
            "Services is wave 2 — must be skipped when cancelled between waves"
        );

        // collect() no longer emits Interrupted — the CLI layer handles
        // that for inspectors whose sections are absent from the snapshot.
        // Verify no InspectorStarted was emitted for Services (never spawned).
        let events = progress.events();
        assert!(
            !events.iter().any(|e| matches!(
                e,
                ProgressEvent::InspectorStarted(InspectorId::Services)
            )),
            "Services should not have been started when cancelled between waves"
        );
    }

    #[test]
    fn test_collect_no_cancellation_runs_normally() {
        use inspectah_collect::inspectors::services::ServicesInspector;

        let exec = build_test_mock();
        let source = SourceSystem::PackageBased {
            os_release: test_os_release(),
        };
        let cancelled = AtomicBool::new(false);
        let progress = VecProgress::new();

        let inspectors: Vec<Box<dyn Inspector>> = vec![
            Box::new(RpmInspector::new()),
            Box::new(ServicesInspector::new()),
        ];
        let pipeline = collect(&source, &exec, &inspectors, None, &progress, &cancelled);

        // Both waves should complete normally
        assert!(
            pipeline.state.snapshot.rpm.is_some(),
            "RPM must complete when not cancelled"
        );

        // No Interrupted events should be emitted
        let events = progress.events();
        assert!(
            !events.iter().any(|e| matches!(
                e,
                ProgressEvent::InspectorFinished {
                    outcome: InspectorOutcome::Interrupted,
                    ..
                }
            )),
            "no Interrupted events when not cancelled"
        );
    }
}
