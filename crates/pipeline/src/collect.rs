use std::sync::atomic::{AtomicBool, Ordering};

use inspectah_core::baseline::BaselineData;
use inspectah_core::machine_state::ScreenedExecutor;
use inspectah_core::pipeline::{Collected, Pipeline};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::executor::Executor;
use inspectah_core::traits::inspector::{
    InspectionContext, Inspector, InspectorError, InspectorOutput, RpmState,
};
use inspectah_core::traits::progress::ProgressSink;
use inspectah_core::types::completeness::{Completeness, InspectorId, SectionData};
use inspectah_core::types::config::{ConfigShadow, shadow_matching_config_entries};
use inspectah_core::types::destination::Destination;
use inspectah_core::types::os::SystemType;
use inspectah_core::types::progress::{InspectorOutcome, ProgressEvent};
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
    executor: &ScreenedExecutor<'_>,
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
                    Some((
                        inspector,
                        s.spawn(|| inspector.inspect(&enriched_ctx, progress)),
                    ))
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
    let hostname_source = match resolve_hostname(executor) {
        Some((hostname, source)) => {
            snapshot
                .meta
                .insert("hostname".into(), serde_json::Value::String(hostname));
            source
        }
        None => NO_HOSTNAME_SOURCE,
    };
    snapshot.meta.insert(
        "hostname_source".into(),
        serde_json::Value::String(hostname_source.into()),
    );
    snapshot.meta.insert(
        "timestamp".into(),
        serde_json::Value::String(inspectah_core::util::now_rfc3339_utc()),
    );
    snapshot.meta.insert(
        "inspectah_version".into(),
        serde_json::Value::String(env!("CARGO_PKG_VERSION").into()),
    );

    finalize_cross_inspector(&mut snapshot);

    Pipeline {
        state: Collected { snapshot },
    }
}

/// The static hostname, set by `hostnamectl set-hostname` or the installer.
const STATIC_HOSTNAME_PATH: &str = "/etc/hostname";

/// The kernel's current hostname: the transient name, which is what
/// `hostname` and `uname -n` print. On a DHCP-named host (a cloud
/// instance, a provisioned VM) this is the only name there is, and
/// `/etc/hostname` is empty.
const KERNEL_HOSTNAME_PATH: &str = "/proc/sys/kernel/hostname";

/// `meta["hostname_source"]` when no source names the host, so a scan that
/// found no name reads differently from a snapshot collected before the
/// source was recorded, which has no `hostname_source` at all.
const NO_HOSTNAME_SOURCE: &str = "none";

/// The kernel's hostname before anything sets one. It is not a valid
/// hostname, so no administrator can have chosen it.
const KERNEL_UNSET_HOSTNAME: &str = "(none)";

/// hostnamed's description of the host's names, as JSON: the `Describe()`
/// reply of `org.freedesktop.hostname1`, which `hostnamectl` relays.
const HOSTNAMECTL: &str = "hostnamectl";
const HOSTNAMECTL_ARGS: &[&str] = &["--json=short"];

/// hostnamed's `HostnameSource` for a kernel name that is systemd's
/// fallback (os-release's `DEFAULT_HOSTNAME` or the compiled-in default,
/// usually `localhost`) rather than a name anything set. The other
/// values are `static` and `transient`.
const HOSTNAME_SOURCE_DEFAULT: &str = "default";

/// The host's name and the file it was read from, recorded in the
/// snapshot as `meta["hostname"]` and `meta["hostname_source"]`.
///
/// The static name wins; an empty or whitespace-only `/etc/hostname`
/// falls back to the kernel's transient name. That name is dropped only
/// when it is the kernel's unset value, or when hostnamed reports it as
/// systemd's default: whether a kernel name was set is a matter of how
/// it got there, which its spelling cannot tell, so a `localhost` someone
/// set stays the host's name. When hostnamed cannot answer, the kernel
/// name is kept. `None` when neither file names the host, recorded as
/// [`NO_HOSTNAME_SOURCE`]. The source is written on every scan, so a
/// snapshot with no source predates this rule.
fn resolve_hostname(executor: &dyn Executor) -> Option<(String, &'static str)> {
    let read = |path: &str| {
        executor
            .read_file(std::path::Path::new(path))
            .ok()
            .map(|content| content.trim().to_string())
            .filter(|name| !name.is_empty())
    };
    if let Some(name) = read(STATIC_HOSTNAME_PATH) {
        return Some((name, STATIC_HOSTNAME_PATH));
    }
    read(KERNEL_HOSTNAME_PATH)
        .filter(|name| name != KERNEL_UNSET_HOSTNAME)
        .filter(|name| !is_systemd_default_hostname(executor, name))
        .map(|name| (name, KERNEL_HOSTNAME_PATH))
}

/// Whether hostnamed reports `kernel_name` as systemd's default hostname.
///
/// hostnamed (systemd 252 on RHEL 9, 257 on RHEL 10) calls the current
/// name `default` when it equals `/run/systemd/default-hostname`, which
/// PID 1 writes only when it fell back to the default at boot and which
/// hostnamed removes when a transient name is set. False when
/// `hostnamectl` is missing, fails, predates `--json`, prints something
/// unparseable, or describes a different name than the one read.
fn is_systemd_default_hostname(executor: &dyn Executor, kernel_name: &str) -> bool {
    let result = executor.run(HOSTNAMECTL, HOSTNAMECTL_ARGS);
    if !result.success() {
        return false;
    }
    let Ok(described) = serde_json::from_str::<serde_json::Value>(&result.stdout) else {
        return false;
    };
    described.get("Hostname").and_then(|v| v.as_str()) == Some(kernel_name)
        && described.get("HostnameSource").and_then(|v| v.as_str()) == Some(HOSTNAME_SOURCE_DEFAULT)
}

/// Reconcile facts that span two inspectors, once, after every inspector
/// has returned and before the snapshot leaves collection.
///
/// This boundary is the one every collection path crosses: the CLI scan
/// calls `collect()` directly and then `validate` and `redact`, never
/// `run_pipeline()`, and `run_pipeline()` calls this same `collect()`.
/// A pass placed in `orchestrate.rs` would be skipped by the production
/// scan.
///
/// Each pass is a separate function and every one of them is called
/// unconditionally. A pass's own missing-section checks are `return`s
/// inside that pass, never inside this dispatcher: a pass written into
/// the body of another pass would inherit that pass's early returns and
/// silently not run on hosts that take them, which no test or gate here
/// would catch.
pub fn finalize_cross_inspector(snapshot: &mut InspectionSnapshot) {
    shadow_unit_owned_paths(snapshot);
    shadow_repo_owned_paths(snapshot);
    crate::default_grub::settle_default_grub(snapshot);
}

/// A config file at the destination of an `rpm.repo_files` or
/// `rpm.gpg_keys` entry is decided by the repo toggle: the repo inventory
/// is what the stager writes. The pass, with the exclusion of what no repo
/// op can carry, is core's `settle_repo_ownership`, which the scan runs
/// again after redaction and an aggregate runs after its merge; its own
/// missing-section checks are its own returns.
fn shadow_repo_owned_paths(snapshot: &mut InspectionSnapshot) {
    inspectah_core::repo_ownership::settle_repo_ownership(snapshot);
}

/// A config file whose path appears in `units.drop_ins[].path` is
/// materialized by the units section, under that entry's own decision.
/// That list holds every path the units section owns under
/// `/etc/systemd/system`: drop-ins of every kind, including those under a
/// name that does not parse, and full shadows (§ 7.7 of the units spec).
/// A mask has no file, and a storage unit's file is a config entry, so
/// neither is in it. The config tree subtracts each listed path
/// unconditionally (`render::configtree::service_owned_paths`). Two live toggles over
/// one carriage decision let an included config entry ship nothing and
/// an excluded one ship anyway; marking the config entry inventory
/// leaves exactly one decision and makes that subtraction correct by
/// construction rather than by accident.
///
/// The criterion is the drop-in list, not the services inspector's
/// degraded flag: the services inspector returns `Err` when its
/// collection fails, so a path can appear here only when the entry that
/// owns it was really collected.
fn shadow_unit_owned_paths(snapshot: &mut InspectionSnapshot) {
    let Some(services) = snapshot.units.as_ref() else {
        return;
    };
    let owned: std::collections::BTreeSet<Destination> = services
        .drop_ins
        .iter()
        .filter_map(|d| Destination::new(&d.path).ok())
        .collect();
    if owned.is_empty() {
        return;
    }
    shadow_matching_config_entries(snapshot.config.as_mut(), &owned, ConfigShadow::Units);
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
    state.verification_results = rpm.rpm_va_complete.then(|| rpm.rpm_va.clone());
    state.module_streams = rpm.module_streams.clone();

    // Build owned_paths and path_to_package from file ownership data.
    // owned_paths: only /etc paths (config tree scope).
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
    for va in state.verification_results.iter_mut().flatten() {
        if va.package.is_none()
            && let Some(&pkg_name) = path_to_name.get(va.path.as_str())
        {
            va.package = Some(pkg_name.to_string());
        }
    }
}

/// Classify whether an inspector belongs to Wave 2 (depends on RPM state).
fn is_wave2(id: InspectorId) -> bool {
    !matches!(id, InspectorId::Rpm | InspectorId::Subscription)
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
        SectionData::Units(s) => snapshot.units = Some(s),
        SectionData::Network(s) => snapshot.network = Some(s),
        SectionData::Storage(s) => snapshot.storage = Some(s),
        SectionData::ScheduledTasks(s) => snapshot.scheduled_tasks = Some(s),
        SectionData::Containers(s) => snapshot.containers = Some(s),
        SectionData::NonRpmSoftware(s) => snapshot.non_rpm_software = Some(s),
        SectionData::KernelBoot(s) => snapshot.kernel_boot = Some(s),
        SectionData::Selinux(s) => snapshot.selinux = Some(s),
        SectionData::UsersGroups(s) => snapshot.users_groups = Some(s),
        SectionData::Subscription(s) => snapshot.subscription = Some(s),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    use std::sync::atomic::AtomicBool;

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
        let pipeline = collect(
            &source,
            &inspectah_core::machine_state::ScreenedExecutor::new(&exec),
            &inspectors,
            None,
            &NullProgress,
            &AtomicBool::new(false),
        );

        // Pipeline produced a Collected state with rpm data
        assert!(pipeline.state.snapshot.rpm.is_some());
        let rpm = pipeline.state.snapshot.rpm.unwrap();
        assert_eq!(rpm.packages_added.len(), 2);
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
        let pipeline = collect(
            &source,
            &inspectah_core::machine_state::ScreenedExecutor::new(&exec),
            &inspectors,
            None,
            &NullProgress,
            &AtomicBool::new(false),
        );

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
        let pipeline = collect(
            &source,
            &inspectah_core::machine_state::ScreenedExecutor::new(&exec),
            &inspectors,
            None,
            &NullProgress,
            &AtomicBool::new(false),
        );

        // RPM routed correctly
        assert!(pipeline.state.snapshot.rpm.is_some());
        // Other sections remain None (no inspectors for them)
        assert!(pipeline.state.snapshot.config.is_none());
        assert!(pipeline.state.snapshot.units.is_none());
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
        let pipeline = collect(
            &source,
            &inspectah_core::machine_state::ScreenedExecutor::new(&exec),
            &inspectors,
            None,
            &NullProgress,
            &AtomicBool::new(false),
        );
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
        let pipeline = collect(
            &source,
            &inspectah_core::machine_state::ScreenedExecutor::new(&exec),
            &inspectors,
            None,
            &NullProgress,
            &AtomicBool::new(false),
        );
        let snap = &pipeline.state.snapshot;

        // os_release and system_type still set
        assert!(snap.os_release.is_some());
        assert_eq!(
            snap.system_type,
            inspectah_core::types::os::SystemType::PackageMode
        );

        // hostname absent when no file names the host, and the source says
        // so, which a snapshot from before the source was recorded cannot
        assert!(
            !snap.meta.contains_key("hostname"),
            "hostname should be absent when /etc/hostname is missing"
        );
        assert_eq!(
            snap.meta.get("hostname_source"),
            Some(&serde_json::json!("none")),
        );
        // version and timestamp still present
        assert!(snap.meta.contains_key("inspectah_version"));
        assert!(snap.meta.contains_key("timestamp"));
    }

    /// The hostname falls back from the static name to the kernel's
    /// (transient) name, and `meta["hostname_source"]` names the file that
    /// supplied it. A host whose `/etc/hostname` is empty and whose name
    /// comes from DHCP is the measured real-host case (`rhel9`). A kernel
    /// name is dropped only when hostnamed says it is systemd's default,
    /// never for its spelling: a `localhost` someone set is a name.
    #[test]
    fn test_collect_hostname_source_order() {
        const STATIC: &str = "/etc/hostname";
        const KERNEL: &str = "/proc/sys/kernel/hostname";
        /// hostnamed's answer for a kernel name and a source.
        fn describe(hostname: &str, source: &str) -> Option<(i32, String)> {
            Some((
                0,
                serde_json::json!({"Hostname": hostname, "StaticHostname": null, "HostnameSource": source})
                    .to_string(),
            ))
        }
        /// (case, /etc/hostname, /proc/sys/kernel/hostname,
        ///  `hostnamectl --json=short` exit code and stdout, expected (hostname, source))
        type Case<'a> = (
            &'a str,
            Option<&'a str>,
            Option<&'a str>,
            Option<(i32, String)>,
            Option<(&'a str, &'a str)>,
        );
        let cases: Vec<Case> = vec![
            (
                "static present wins over kernel",
                Some("web-01\n"),
                Some("dhcp-10-0-0-5\n"),
                describe("web-01", "static"),
                Some(("web-01", STATIC)),
            ),
            (
                "static present, no kernel file",
                Some("web-01\n"),
                None,
                None,
                Some(("web-01", STATIC)),
            ),
            (
                "static localhost is a name",
                Some("localhost\n"),
                Some("localhost\n"),
                describe("localhost", "default"),
                Some(("localhost", STATIC)),
            ),
            (
                "empty static falls back to kernel",
                Some(""),
                Some("rhel9\n"),
                describe("rhel9", "transient"),
                Some(("rhel9", KERNEL)),
            ),
            (
                "whitespace-only static falls back to kernel",
                Some("  \n\t\n"),
                Some("rhel9\n"),
                describe("rhel9", "transient"),
                Some(("rhel9", KERNEL)),
            ),
            (
                "missing static falls back to kernel",
                None,
                Some("ip-10-0-0-5.ec2.internal\n"),
                describe("ip-10-0-0-5.ec2.internal", "transient"),
                Some(("ip-10-0-0-5.ec2.internal", KERNEL)),
            ),
            ("both absent", None, None, None, None),
            (
                "empty static, empty kernel",
                Some(""),
                Some("\n"),
                None,
                None,
            ),
            (
                "kernel default name is not a hostname",
                Some(""),
                Some("(none)\n"),
                None,
                None,
            ),
            (
                "systemd default name is not a hostname",
                Some(""),
                Some("localhost\n"),
                describe("localhost", "default"),
                None,
            ),
            (
                "a distribution default name is not a hostname",
                None,
                Some("fedora\n"),
                describe("fedora", "default"),
                None,
            ),
            (
                "a transient localhost someone set is a name",
                Some(""),
                Some("localhost\n"),
                describe("localhost", "transient"),
                Some(("localhost", KERNEL)),
            ),
            (
                "a transient localhost.localdomain someone set is a name",
                None,
                Some("localhost.localdomain\n"),
                describe("localhost.localdomain", "transient"),
                Some(("localhost.localdomain", KERNEL)),
            ),
            (
                "hostnamectl missing keeps the kernel name",
                Some(""),
                Some("localhost\n"),
                None,
                Some(("localhost", KERNEL)),
            ),
            (
                "hostnamectl without --json keeps the kernel name",
                Some(""),
                Some("localhost\n"),
                Some((1, String::new())),
                Some(("localhost", KERNEL)),
            ),
            (
                "unparseable hostnamectl output keeps the kernel name",
                Some(""),
                Some("localhost\n"),
                Some((0, "not json".into())),
                Some(("localhost", KERNEL)),
            ),
            (
                "a default verdict about another name keeps the kernel name",
                Some(""),
                Some("rhel9\n"),
                describe("localhost", "default"),
                Some(("rhel9", KERNEL)),
            ),
        ];

        for (case, static_name, kernel_name, hostnamectl, expected) in cases {
            let mut exec = MockExecutor::new();
            if let Some(content) = static_name {
                exec = exec.with_file(STATIC, content);
            }
            if let Some(content) = kernel_name {
                exec = exec.with_file(KERNEL, content);
            }
            if let Some((exit_code, stdout)) = hostnamectl {
                exec = exec.with_command(
                    "hostnamectl --json=short",
                    ExecResult {
                        stdout,
                        stderr: String::new(),
                        exit_code,
                    },
                );
            }
            let source = SourceSystem::PackageBased {
                os_release: test_os_release(),
            };
            let pipeline = collect(
                &source,
                &inspectah_core::machine_state::ScreenedExecutor::new(&exec),
                &[],
                None,
                &NullProgress,
                &AtomicBool::new(false),
            );
            let meta = &pipeline.state.snapshot.meta;
            let actual = meta
                .get("hostname")
                .and_then(|v| v.as_str())
                .map(|h| (h, meta.get("hostname_source").and_then(|v| v.as_str())));
            let expected = expected.map(|(h, s)| (h, Some(s)));
            assert_eq!(actual, expected, "case: {case}");
            if expected.is_none() {
                assert_eq!(
                    meta.get("hostname_source"),
                    Some(&serde_json::json!("none")),
                    "case: {case}: an unnamed host records that no source named it"
                );
            }
        }
    }

    // --- Completeness tracking tests ---

    #[test]
    fn test_completeness_complete_when_all_succeed() {
        let exec = build_test_mock();
        let source = SourceSystem::PackageBased {
            os_release: test_os_release(),
        };
        let inspectors: Vec<Box<dyn Inspector>> = vec![Box::new(RpmInspector::new())];
        let pipeline = collect(
            &source,
            &inspectah_core::machine_state::ScreenedExecutor::new(&exec),
            &inspectors,
            None,
            &NullProgress,
            &AtomicBool::new(false),
        );

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
        let pipeline = collect(
            &source,
            &inspectah_core::machine_state::ScreenedExecutor::new(&exec),
            &inspectors,
            None,
            &NullProgress,
            &AtomicBool::new(false),
        );

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
        let pipeline = collect(
            &source,
            &inspectah_core::machine_state::ScreenedExecutor::new(&exec),
            &inspectors,
            None,
            &NullProgress,
            &AtomicBool::new(false),
        );

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
        let pipeline = collect(
            &source,
            &inspectah_core::machine_state::ScreenedExecutor::new(&exec),
            &inspectors,
            None,
            &NullProgress,
            &AtomicBool::new(false),
        );

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
        let pipeline = collect(
            &source,
            &inspectah_core::machine_state::ScreenedExecutor::new(&exec),
            &inspectors,
            None,
            &NullProgress,
            &AtomicBool::new(false),
        );

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
            InspectorId::Units
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
                section: SectionData::Units(Default::default()),
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
        let pipeline = collect(
            &source,
            &inspectah_core::machine_state::ScreenedExecutor::new(&exec),
            &inspectors,
            None,
            &NullProgress,
            &AtomicBool::new(false),
        );

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
        let pipeline = collect(
            &source,
            &inspectah_core::machine_state::ScreenedExecutor::new(&exec),
            &inspectors,
            None,
            &NullProgress,
            &AtomicBool::new(false),
        );

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
        // Wave 1: RPM and Subscription (standalone, no dependencies)
        assert!(!is_wave2(InspectorId::Rpm));
        assert!(!is_wave2(InspectorId::Subscription));

        // All others are wave 2
        assert!(is_wave2(InspectorId::Units));
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
        let pipeline = collect(
            &source,
            &inspectah_core::machine_state::ScreenedExecutor::new(&exec),
            &inspectors,
            None,
            &NullProgress,
            &AtomicBool::new(false),
        );

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
        let _pipeline = collect(
            &source,
            &inspectah_core::machine_state::ScreenedExecutor::new(&exec),
            &inspectors,
            None,
            &NullProgress,
            &AtomicBool::new(false),
        );

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
        let pipeline = collect(
            &source,
            &inspectah_core::machine_state::ScreenedExecutor::new(&exec),
            &inspectors,
            None,
            &NullProgress,
            &AtomicBool::new(false),
        );

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
        let pipeline = collect(
            &source,
            &inspectah_core::machine_state::ScreenedExecutor::new(&exec),
            &inspectors,
            None,
            &NullProgress,
            &AtomicBool::new(false),
        );

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
        let _pipeline = collect(
            &source,
            &inspectah_core::machine_state::ScreenedExecutor::new(&exec),
            &inspectors,
            None,
            &NullProgress,
            &AtomicBool::new(false),
        );

        let rpm_state = captured
            .lock()
            .unwrap()
            .clone()
            .expect("probe must capture RpmState");

        // rpm -Va entry for httpd.conf should have package attribution
        let httpd_va = rpm_state
            .verification_results()
            .expect("rpm -Va ran to the end")
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
        let pipeline = collect(
            &source,
            &inspectah_core::machine_state::ScreenedExecutor::new(&exec),
            &inspectors,
            None,
            &NullProgress,
            &AtomicBool::new(false),
        );
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
        let _pipeline = collect(
            &source,
            &inspectah_core::machine_state::ScreenedExecutor::new(&exec),
            &inspectors,
            None,
            &progress,
            &AtomicBool::new(false),
        );

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
        let inspectors: Vec<Box<dyn Inspector>> =
            vec![Box::new(RpmInspector::new()), Box::new(SkippedInspector)];
        let _pipeline = collect(
            &source,
            &inspectah_core::machine_state::ScreenedExecutor::new(&exec),
            &inspectors,
            None,
            &progress,
            &AtomicBool::new(false),
        );

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
        let inspectors: Vec<Box<dyn Inspector>> =
            vec![Box::new(RpmInspector::new()), Box::new(FailingInspector)];
        let _pipeline = collect(
            &source,
            &inspectah_core::machine_state::ScreenedExecutor::new(&exec),
            &inspectors,
            None,
            &progress,
            &AtomicBool::new(false),
        );

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
        use inspectah_collect::inspectors::units::UnitsInspector;

        let exec = build_test_mock();
        let source = SourceSystem::PackageBased {
            os_release: test_os_release(),
        };
        let cancelled = std::sync::Arc::new(AtomicBool::new(false));
        let progress = VecProgress::new();

        let inspectors: Vec<Box<dyn Inspector>> = vec![
            Box::new(CancellingRpmInspector::new(cancelled.clone())),
            Box::new(UnitsInspector::new()),
        ];
        let pipeline = collect(
            &source,
            &inspectah_core::machine_state::ScreenedExecutor::new(&exec),
            &inspectors,
            None,
            &progress,
            &cancelled,
        );

        // RPM completed in wave 1 (before flag was set)
        assert!(
            pipeline.state.snapshot.rpm.is_some(),
            "RPM ran in wave 1 before cancellation — must have data"
        );

        // Services is wave 2 — should have been skipped due to cancellation
        assert!(
            pipeline.state.snapshot.units.is_none(),
            "Services is wave 2 — must be skipped when cancelled between waves"
        );

        // collect() no longer emits Interrupted — the CLI layer handles
        // that for inspectors whose sections are absent from the snapshot.
        // Verify no InspectorStarted was emitted for Services (never spawned).
        let events = progress.events();
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, ProgressEvent::InspectorStarted(InspectorId::Units))),
            "Services should not have been started when cancelled between waves"
        );
    }

    #[test]
    fn test_collect_no_cancellation_runs_normally() {
        use inspectah_collect::inspectors::units::UnitsInspector;

        let exec = build_test_mock();
        let source = SourceSystem::PackageBased {
            os_release: test_os_release(),
        };
        let cancelled = AtomicBool::new(false);
        let progress = VecProgress::new();

        let inspectors: Vec<Box<dyn Inspector>> = vec![
            Box::new(RpmInspector::new()),
            Box::new(UnitsInspector::new()),
        ];
        let pipeline = collect(
            &source,
            &inspectah_core::machine_state::ScreenedExecutor::new(&exec),
            &inspectors,
            None,
            &progress,
            &cancelled,
        );

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

#[cfg(test)]
mod finalize_tests {
    use super::*;
    use inspectah_core::types::AdvisoryType;
    use inspectah_core::types::FindingKind;
    use inspectah_core::types::config::{ConfigFileEntry, ConfigSection};
    use inspectah_core::types::finding::ShadowType;
    use inspectah_core::types::units::{SystemdDropIn, UnitSection};

    fn only_entry(snap: &InspectionSnapshot) -> &ConfigFileEntry {
        &snap.config.as_ref().expect("config section").files[0]
    }

    const DROP_IN: &str = "/etc/systemd/system/httpd.service.d/limits.conf";

    /// A service-owned config path with the given disposition.
    fn service_owned_snapshot(disposition: FindingKind) -> InspectionSnapshot {
        let mut snap = InspectionSnapshot::new();
        snap.units = Some(UnitSection {
            drop_ins: vec![SystemdDropIn {
                unit: "httpd.service".into(),
                path: DROP_IN.into(),
                ..Default::default()
            }],
            ..Default::default()
        });
        snap.config = Some(ConfigSection {
            withheld: Vec::new(),
            files: vec![ConfigFileEntry {
                path: DROP_IN.into(),
                disposition,
                ..Default::default()
            }],
        });
        snap
    }

    #[test]
    fn an_actionable_service_owned_path_becomes_inventory() {
        let mut snap = service_owned_snapshot(FindingKind::included());
        finalize_cross_inspector(&mut snap);
        let entry = only_entry(&snap);
        assert_eq!(entry.shadowed_by, Some(ConfigShadow::Units));
        assert!(entry.disposition.is_inventory());
    }

    /// An advisory is the walker's warning that the file points outside
    /// `/etc`, and it lives in the same field the shadow would overwrite.
    /// Shadowing it would buy nothing, since an advisory entry is already
    /// never materialized, and would cost the operator the warning.
    #[test]
    fn an_advisory_service_owned_path_keeps_its_advisory() {
        let mut snap = service_owned_snapshot(FindingKind::advisory(
            AdvisoryType::CrossTreeSymlink,
            "symlink crosses into /var",
        ));
        finalize_cross_inspector(&mut snap);
        let entry = only_entry(&snap);
        assert!(
            entry.disposition.is_advisory(),
            "the advisory must survive the shadow pass, got {:?}",
            entry.disposition
        );
        assert_eq!(
            entry.shadowed_by, None,
            "an entry that is not shadowed must not claim to be"
        );
    }

    /// `units.drop_ins` holds full unit shadows as well as
    /// `*.service.d/*.conf`, which is why the variant is named for the
    /// owning set rather than for the field.
    #[test]
    fn a_full_unit_shadow_is_service_owned_too() {
        const FULL_SHADOW: &str = "/etc/systemd/system/custom.service";
        let mut snap = InspectionSnapshot::new();
        snap.units = Some(UnitSection {
            drop_ins: vec![SystemdDropIn {
                unit: "custom.service".into(),
                path: FULL_SHADOW.into(),
                shadow_type: Some(ShadowType::FullShadow),
                ..Default::default()
            }],
            ..Default::default()
        });
        snap.config = Some(ConfigSection {
            withheld: Vec::new(),
            files: vec![ConfigFileEntry {
                path: FULL_SHADOW.into(),
                disposition: FindingKind::included(),
                ..Default::default()
            }],
        });

        finalize_cross_inspector(&mut snap);

        assert_eq!(only_entry(&snap).shadowed_by, Some(ConfigShadow::Units));
    }

    /// The pass follows the drop-in list, not a unit kind: a drop-in of a
    /// socket, of a `.device`, and under a name that does not parse each
    /// shadows the config entry at its path.
    #[test]
    fn a_drop_in_of_any_kind_shadows_its_config_entry() {
        for unit in ["cockpit.socket", "dev-sda.device", "foo.servic"] {
            let path = format!("/etc/systemd/system/{unit}.d/override.conf");
            let mut snap = InspectionSnapshot::new();
            snap.units = Some(UnitSection {
                drop_ins: vec![SystemdDropIn {
                    unit: unit.into(),
                    path: path.clone(),
                    ..Default::default()
                }],
                ..Default::default()
            });
            snap.config = Some(ConfigSection {
                withheld: Vec::new(),
                files: vec![ConfigFileEntry {
                    path,
                    disposition: FindingKind::included(),
                    ..Default::default()
                }],
            });

            finalize_cross_inspector(&mut snap);

            assert_eq!(
                only_entry(&snap).shadowed_by,
                Some(ConfigShadow::Units),
                "{unit}"
            );
            assert!(only_entry(&snap).disposition.is_inventory(), "{unit}");
        }
    }

    const REPO: &str = "/etc/yum.repos.d/custom.repo";

    /// A host with a repo file and its config twin, and no units section,
    /// so the units pass returns early.
    fn repo_owned_snapshot() -> InspectionSnapshot {
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(inspectah_core::types::rpm::RpmSection {
            repo_files: vec![inspectah_core::types::rpm::RepoFile {
                path: REPO.into(),
                content: "[custom]\nbaseurl=https://custom.example/\n".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            }],
            ..Default::default()
        });
        snap.config = Some(ConfigSection {
            withheld: Vec::new(),
            files: vec![ConfigFileEntry {
                // An ostree scan's relative spelling of the same file.
                path: REPO[1..].into(),
                disposition: FindingKind::included(),
                ..Default::default()
            }],
        });
        snap
    }

    #[test]
    fn the_repo_pass_runs_where_the_units_pass_returns_early() {
        let mut snap = repo_owned_snapshot();
        assert!(snap.units.is_none());
        finalize_cross_inspector(&mut snap);
        let entry = only_entry(&snap);
        assert_eq!(entry.shadowed_by, Some(ConfigShadow::Repositories));
        assert!(entry.disposition.is_inventory());
    }

    #[test]
    fn the_units_pass_runs_where_the_repo_pass_returns_early() {
        let mut snap = service_owned_snapshot(FindingKind::included());
        assert!(snap.rpm.is_none());
        finalize_cross_inspector(&mut snap);
        assert_eq!(only_entry(&snap).shadowed_by, Some(ConfigShadow::Units));
    }

    /// An empty `path` is not a match for every config entry that also
    /// has one. The services inspector writes `path` from a real
    /// `read_dir` result, but a merged or hand-edited snapshot need not.
    #[test]
    fn an_empty_drop_in_path_shadows_nothing() {
        let mut snap = InspectionSnapshot::new();
        snap.units = Some(UnitSection {
            drop_ins: vec![SystemdDropIn::default()],
            ..Default::default()
        });
        snap.config = Some(ConfigSection {
            withheld: Vec::new(),
            files: vec![ConfigFileEntry {
                path: String::new(),
                disposition: FindingKind::included(),
                ..Default::default()
            }],
        });

        finalize_cross_inspector(&mut snap);

        assert_eq!(only_entry(&snap).shadowed_by, None);
        assert!(only_entry(&snap).disposition.is_included());
    }
}
