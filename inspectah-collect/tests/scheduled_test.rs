//! Integration tests for ScheduledTasksInspector.
//!
//! Runs the actual inspector on fixture data via MockExecutor
//! and verifies output is structurally correct.

use inspectah_collect::executor::mock::MockExecutor;
use inspectah_collect::inspectors::scheduled::ScheduledTasksInspector;
use inspectah_core::traits::inspector::{InspectionContext, Inspector, InspectorError, RpmState};
use inspectah_core::types::completeness::SectionData;
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::scheduled::ScheduledTaskSection;
use inspectah_core::types::system::SourceSystem;
use std::collections::HashSet;
use std::path::PathBuf;

// ── Fixtures ────────────────────────────────────────────────────────

const CRONTAB_SYSTEM: &str = include_str!("../../testdata/fixtures/scheduled/crontab-system");
const CRON_D_LOGROTATE: &str = include_str!("../../testdata/fixtures/scheduled/cron-d-logrotate");
const CRON_D_CUSTOM_BACKUP: &str =
    include_str!("../../testdata/fixtures/scheduled/cron-d-custom-backup");
const USER_CRONTAB: &str = include_str!("../../testdata/fixtures/scheduled/user-crontab");
const AT_JOB_SAMPLE: &str = include_str!("../../testdata/fixtures/scheduled/at-job-sample");
const CLEANUP_TIMER: &str = include_str!("../../testdata/fixtures/scheduled/cleanup-timer");
const CLEANUP_SERVICE: &str = include_str!("../../testdata/fixtures/scheduled/cleanup-service");

// ── Helpers ─────────────────────────────────────────────────────────

fn pkg_source() -> SourceSystem {
    SourceSystem::PackageBased {
        os_release: OsRelease {
            name: "Red Hat Enterprise Linux".into(),
            version_id: "9.4".into(),
            id: "rhel".into(),
            ..Default::default()
        },
    }
}

fn mock_rpm_state() -> RpmState {
    let mut owned = HashSet::new();
    // logrotate cron file is RPM-owned
    owned.insert(PathBuf::from("/etc/cron.d/logrotate"));
    // /etc/crontab is RPM-owned
    owned.insert(PathBuf::from("/etc/crontab"));
    RpmState {
        owned_paths: owned,
        ..Default::default()
    }
}

fn full_mock() -> MockExecutor {
    MockExecutor::new()
        // /etc/cron.d directory with two entries
        .with_dir("/etc/cron.d", vec!["logrotate", "custom-backup"])
        .with_file("/etc/cron.d/logrotate", CRON_D_LOGROTATE)
        .with_file("/etc/cron.d/custom-backup", CRON_D_CUSTOM_BACKUP)
        // /etc/crontab
        .with_file("/etc/crontab", CRONTAB_SYSTEM)
        // Period directories (only daily populated for test)
        .with_dir("/etc/cron.daily", vec!["rhsm"])
        .with_file("/etc/cron.daily/rhsm", "#!/bin/bash\n/usr/bin/rhsm-check\n")
        // User crontabs
        .with_dir("/var/spool/cron", vec!["appuser"])
        .with_file("/var/spool/cron/appuser", USER_CRONTAB)
        // Systemd timers
        .with_dir("/etc/systemd/system", vec!["cleanup.timer", "cleanup.service"])
        .with_file("/etc/systemd/system/cleanup.timer", CLEANUP_TIMER)
        .with_file("/etc/systemd/system/cleanup.service", CLEANUP_SERVICE)
        .with_dir("/usr/lib/systemd/system", vec![])
        // At jobs
        .with_dir("/var/spool/at", vec!["job001"])
        .with_file("/var/spool/at/job001", AT_JOB_SAMPLE)
}

// ── Tests ───────────────────────────────────────────────────────────

/// Happy path: all sub-collectors produce data.
#[test]
fn test_scheduled_inspector_happy_path() {
    let exec = full_mock();
    let source = pkg_source();
    let rpm_state = mock_rpm_state();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: Some(&rpm_state),
    };

    let output = ScheduledTasksInspector::new()
        .inspect(&ctx)
        .expect("scheduled inspector should succeed on full fixture set");

    let section = match &output.section {
        SectionData::ScheduledTasks(s) => s,
        other => panic!("expected SectionData::ScheduledTasks, got {:?}", other),
    };

    // Cron jobs: logrotate (rpm-owned), custom-backup, crontab, cron.daily/rhsm, user crontab
    assert!(
        !section.cron_jobs.is_empty(),
        "inspector must produce cron_jobs from fixture data"
    );

    // Verify RPM-owned classification
    let logrotate = section
        .cron_jobs
        .iter()
        .find(|j| j.path.contains("logrotate"));
    assert!(
        logrotate.is_some(),
        "should find logrotate cron job"
    );
    assert!(
        logrotate.unwrap().rpm_owned,
        "logrotate cron file should be marked rpm_owned"
    );

    let custom_backup = section
        .cron_jobs
        .iter()
        .find(|j| j.path.contains("custom-backup"));
    assert!(
        custom_backup.is_some(),
        "should find custom-backup cron job"
    );
    assert!(
        !custom_backup.unwrap().rpm_owned,
        "custom-backup should NOT be rpm_owned"
    );

    // Systemd timers
    assert!(
        !section.systemd_timers.is_empty(),
        "inspector must produce systemd_timers from fixture data"
    );
    let cleanup = section
        .systemd_timers
        .iter()
        .find(|t| t.name == "cleanup");
    assert!(cleanup.is_some(), "should find cleanup timer");
    assert!(
        !cleanup.unwrap().on_calendar.is_empty(),
        "cleanup timer should have on_calendar"
    );
    assert!(
        !cleanup.unwrap().exec_start.is_empty(),
        "cleanup timer should have exec_start"
    );

    // At jobs
    assert!(
        !section.at_jobs.is_empty(),
        "inspector must produce at_jobs from fixture data"
    );
    let at_job = &section.at_jobs[0];
    assert!(
        !at_job.command.is_empty(),
        "at job should have a command"
    );
    assert!(
        at_job.command.contains("run-migration.sh"),
        "at job command should contain the actual command"
    );

    // Generated timer units (from non-RPM-owned cron entries)
    assert!(
        !section.generated_timer_units.is_empty(),
        "inspector must generate timer units from non-RPM cron entries"
    );
}

/// No cron directories exist -- inspector still succeeds.
#[test]
fn test_scheduled_inspector_cron_not_found() {
    let exec = MockExecutor::new()
        // No cron dirs, no timers, no at jobs -- all dirs return NotFound
        .with_dir("/usr/lib/systemd/system", vec![]);

    let source = pkg_source();
    let rpm_state = mock_rpm_state();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: Some(&rpm_state),
    };

    let output = ScheduledTasksInspector::new()
        .inspect(&ctx)
        .expect("inspector should succeed even with no cron dirs");

    let section = match &output.section {
        SectionData::ScheduledTasks(s) => s,
        other => panic!("expected SectionData::ScheduledTasks, got {:?}", other),
    };

    assert!(
        section.cron_jobs.is_empty(),
        "no cron dirs means no cron_jobs"
    );
    assert!(
        section.at_jobs.is_empty(),
        "no at spool means no at_jobs"
    );
}

/// PermissionDenied on cron directories produces Degraded output.
#[test]
fn test_scheduled_inspector_degraded_permissions() {
    let exec = MockExecutor::new()
        .with_dir_error("/etc/cron.d", std::io::ErrorKind::PermissionDenied)
        .with_dir_error("/var/spool/at", std::io::ErrorKind::PermissionDenied)
        .with_dir("/usr/lib/systemd/system", vec![]);

    let source = pkg_source();
    let rpm_state = mock_rpm_state();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: Some(&rpm_state),
    };

    let result = ScheduledTasksInspector::new().inspect(&ctx);

    // The inspector should return Ok with warnings, or the warnings
    // should mention degraded/permission issues. The exact behavior
    // depends on whether other sub-collectors succeed.
    match result {
        Ok(output) => {
            // Some warnings should mention permission issues
            let has_perm_warning = output.warnings.iter().any(|w| {
                w.message.contains("permission denied")
                    || w.message.contains("degraded")
            });
            assert!(
                has_perm_warning,
                "should have warnings about permission issues"
            );
        }
        Err(InspectorError::Degraded { partial, reason }) => {
            assert!(
                reason.contains("permission denied"),
                "degraded reason should mention permission denied, got: {reason}"
            );
            // Partial output should still be valid
            match &partial.section {
                SectionData::ScheduledTasks(_) => {}
                other => panic!("expected SectionData::ScheduledTasks in partial, got {:?}", other),
            }
        }
        Err(other) => panic!("unexpected error: {other}"),
    }
}

/// Output serializes and deserializes cleanly.
#[test]
fn test_scheduled_inspector_json_roundtrip() {
    let exec = full_mock();
    let source = pkg_source();
    let rpm_state = mock_rpm_state();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: Some(&rpm_state),
    };

    let output = ScheduledTasksInspector::new()
        .inspect(&ctx)
        .expect("inspector should succeed");

    let section = match &output.section {
        SectionData::ScheduledTasks(s) => s,
        other => panic!("expected SectionData::ScheduledTasks, got {:?}", other),
    };

    let json = serde_json::to_string_pretty(section)
        .expect("section must serialize to JSON");
    let roundtrip: ScheduledTaskSection =
        serde_json::from_str(&json).expect("JSON must deserialize back");
    let roundtrip_json = serde_json::to_string_pretty(&roundtrip)
        .expect("roundtrip must serialize");

    assert_eq!(
        json, roundtrip_json,
        "inspector output must round-trip faithfully through ScheduledTaskSection"
    );
}
