use inspectah_core::pipeline::{Collected, Pipeline};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::inspector::{InspectionContext, Inspector, InspectorError};
use inspectah_core::types::completeness::SectionData;
use inspectah_core::types::warnings::{Warning, WarningSeverity};

/// Run all inspectors against the given context, routing each inspector's
/// typed SectionData to the corresponding snapshot field.
///
/// Returns a `Pipeline<Collected>` containing the populated snapshot.
pub fn collect(
    ctx: &InspectionContext,
    inspectors: &[Box<dyn Inspector>],
) -> Pipeline<Collected> {
    let mut snapshot = InspectionSnapshot::new();

    for inspector in inspectors {
        match inspector.inspect(ctx) {
            Ok(output) => {
                route_section(&mut snapshot, output.section);
                snapshot.warnings.extend(output.warnings);
            }
            Err(InspectorError::Skipped { reason }) => {
                snapshot.warnings.push(Warning {
                    inspector: format!("{:?}", inspector.id()),
                    message: format!("skipped: {reason}"),
                    severity: Some(WarningSeverity::Info),
                    ..Default::default()
                });
            }
            Err(InspectorError::Degraded {
                partial, reason, ..
            }) => {
                // Route partial data and add a warning
                route_section(&mut snapshot, partial.section);
                snapshot.warnings.extend(partial.warnings);
                snapshot.warnings.push(Warning {
                    inspector: format!("{:?}", inspector.id()),
                    message: format!("degraded: {reason}"),
                    severity: Some(WarningSeverity::Warning),
                    ..Default::default()
                });
            }
            Err(InspectorError::Failed { reason }) => {
                snapshot.warnings.push(Warning {
                    inspector: format!("{:?}", inspector.id()),
                    message: format!("failed: {reason}"),
                    severity: Some(WarningSeverity::Error),
                    ..Default::default()
                });
            }
        }
    }

    Pipeline {
        state: Collected { snapshot },
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
    use inspectah_core::types::os::OsRelease;
    use inspectah_core::types::system::SourceSystem;

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
        let mock = build_test_mock();
        let ctx = InspectionContext {
            executor: Box::new(mock),
            source: SourceSystem::PackageBased {
                os_release: test_os_release(),
            },
            rpm_state: None,
        };
        let inspectors: Vec<Box<dyn Inspector>> = vec![Box::new(RpmInspector::new())];
        let pipeline = collect(&ctx, &inspectors);

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
        let mock = MockExecutor::new().with_command(
            "rpm -qa --queryformat %{EPOCH}:%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\n",
            ExecResult {
                stdout: "".into(),
                exit_code: 0,
                ..Default::default()
            },
        );
        let ctx = InspectionContext {
            executor: Box::new(mock),
            source: SourceSystem::PackageBased {
                os_release: test_os_release(),
            },
            rpm_state: None,
        };
        let inspectors: Vec<Box<dyn Inspector>> = vec![Box::new(RpmInspector::new())];
        let pipeline = collect(&ctx, &inspectors);

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
        let mock = build_test_mock();
        let ctx = InspectionContext {
            executor: Box::new(mock),
            source: SourceSystem::PackageBased {
                os_release: test_os_release(),
            },
            rpm_state: None,
        };
        let inspectors: Vec<Box<dyn Inspector>> = vec![Box::new(RpmInspector::new())];
        let pipeline = collect(&ctx, &inspectors);

        // RPM routed correctly
        assert!(pipeline.state.snapshot.rpm.is_some());
        // Other sections remain None (no inspectors for them)
        assert!(pipeline.state.snapshot.config.is_none());
        assert!(pipeline.state.snapshot.services.is_none());
        assert!(pipeline.state.snapshot.network.is_none());
        assert!(pipeline.state.snapshot.storage.is_none());
    }
}
