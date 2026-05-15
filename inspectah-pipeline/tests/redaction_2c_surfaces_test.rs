//! Redaction coverage tests for Slice 2c inspector surfaces.
//!
//! Tests planted secrets in config file content, .env file content, cron/at
//! commands, timer ExecStart, audit rules, PAM configs, and git remote URLs.
//! Includes both detection proofs (individual surface) and absence proofs
//! (secrets must not survive into any output artifact).

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::config::{ConfigFileEntry, ConfigSection};
use inspectah_core::types::nonrpm::{NonRpmItem, NonRpmSoftwareSection};
use inspectah_core::types::scheduled::{AtJob, GeneratedTimerUnit, ScheduledTaskSection, SystemdTimer};
use inspectah_core::types::selinux::SelinuxSection;
use inspectah_pipeline::redaction::engine::{redact, RedactOptions};

// ===================================================================
// Detection proofs — each surface produces a finding when planted
// ===================================================================

// ---------------------------------------------------------------------------
// Test 1: Config file content with password=
// ---------------------------------------------------------------------------

#[test]
fn test_redaction_config_content_password() {
    let mut snapshot = InspectionSnapshot::new();
    snapshot.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/myapp/db.conf".into(),
            content: "host=localhost\npassword=cfg_secret_42\nport=5432\n".into(),
            include: true,
            ..Default::default()
        }],
    });

    redact(&mut snapshot, &RedactOptions::default());

    let config = snapshot.config.as_ref().unwrap();
    assert!(
        !config.files[0].content.contains("cfg_secret_42"),
        "password value must be redacted from config content, got: {}",
        config.files[0].content
    );
    assert!(
        config.files[0].content.contains("REDACTED_"),
        "config content must contain redaction token, got: {}",
        config.files[0].content
    );
    assert!(
        !snapshot.redactions.is_empty(),
        "redactions must contain findings from config file"
    );
}

// ---------------------------------------------------------------------------
// Test 2: Config file content with api_key= (AWS-style)
// ---------------------------------------------------------------------------

#[test]
fn test_redaction_config_content_api_key() {
    let mut snapshot = InspectionSnapshot::new();
    snapshot.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/myapp/cloud.conf".into(),
            content: "region=us-east-1\napi_key=AKIAIOSFODNN7EXAMPLE\n".into(),
            include: true,
            ..Default::default()
        }],
    });

    redact(&mut snapshot, &RedactOptions::default());

    let config = snapshot.config.as_ref().unwrap();
    assert!(
        !config.files[0].content.contains("AKIAIOSFODNN7EXAMPLE"),
        "api_key value must be redacted from config content, got: {}",
        config.files[0].content
    );
    assert!(
        !snapshot.redactions.is_empty(),
        "redactions must contain findings from api_key config"
    );
}

// ---------------------------------------------------------------------------
// Test 3: .env file with DATABASE_URL containing credentials
// ---------------------------------------------------------------------------

#[test]
fn test_redaction_env_file_database_url() {
    let mut snapshot = InspectionSnapshot::new();
    snapshot.non_rpm_software = Some(NonRpmSoftwareSection {
        env_files: vec![ConfigFileEntry {
            path: "/opt/myapp/.env".into(),
            content: "NODE_ENV=production\nDATABASE_URL=postgres://appuser:env_secret_99@db.internal:5432/mydb\nPORT=3000\n".into(),
            include: true,
            ..Default::default()
        }],
        ..Default::default()
    });

    redact(&mut snapshot, &RedactOptions::default());

    let nrs = snapshot.non_rpm_software.as_ref().unwrap();
    assert!(
        !nrs.env_files[0].content.contains("env_secret_99"),
        "database URL password must be redacted from .env content, got: {}",
        nrs.env_files[0].content
    );
    assert!(
        !snapshot.redactions.is_empty(),
        "redactions must contain findings from .env file"
    );
}

// ---------------------------------------------------------------------------
// Test 4: Cron command with --password= flag
// ---------------------------------------------------------------------------

#[test]
fn test_redaction_cron_command_password() {
    let mut snapshot = InspectionSnapshot::new();
    snapshot.scheduled_tasks = Some(ScheduledTaskSection {
        generated_timer_units: vec![GeneratedTimerUnit {
            name: "backup.timer".into(),
            command: "/usr/bin/backup --host=db.local --password=cron_secret_88".into(),
            source_path: "/etc/cron.d/backup".into(),
            include: true,
            ..Default::default()
        }],
        ..Default::default()
    });

    redact(&mut snapshot, &RedactOptions::default());

    let sched = snapshot.scheduled_tasks.as_ref().unwrap();
    assert!(
        !sched.generated_timer_units[0].command.contains("cron_secret_88"),
        "password in cron command must be redacted, got: {}",
        sched.generated_timer_units[0].command
    );
    assert!(
        !snapshot.redactions.is_empty(),
        "redactions must contain findings from cron command"
    );
}

// ---------------------------------------------------------------------------
// Test 5: Timer ExecStart with --token= flag
// ---------------------------------------------------------------------------

#[test]
fn test_redaction_timer_execstart_token() {
    let mut snapshot = InspectionSnapshot::new();
    snapshot.scheduled_tasks = Some(ScheduledTaskSection {
        systemd_timers: vec![SystemdTimer {
            name: "deploy.timer".into(),
            exec_start: "/usr/local/bin/deploy --token=timer_secret_77 --env=prod".into(),
            source: "local".into(),
            path: "/etc/systemd/system/deploy.timer".into(),
            ..Default::default()
        }],
        ..Default::default()
    });

    redact(&mut snapshot, &RedactOptions::default());

    let sched = snapshot.scheduled_tasks.as_ref().unwrap();
    assert!(
        !sched.systemd_timers[0].exec_start.contains("timer_secret_77"),
        "token in timer ExecStart must be redacted, got: {}",
        sched.systemd_timers[0].exec_start
    );
    assert!(
        !snapshot.redactions.is_empty(),
        "redactions must contain findings from timer ExecStart"
    );
}

// ---------------------------------------------------------------------------
// Test 6: Git remote URL with embedded credentials
// ---------------------------------------------------------------------------

#[test]
fn test_redaction_git_url_credentials() {
    let mut snapshot = InspectionSnapshot::new();
    snapshot.non_rpm_software = Some(NonRpmSoftwareSection {
        items: vec![NonRpmItem {
            path: "/opt/myapp".into(),
            name: "myapp".into(),
            method: "git repo".into(),
            git_remote: "https://deploy:git_secret_66@github.com/corp/myapp.git".into(),
            include: true,
            ..Default::default()
        }],
        ..Default::default()
    });

    redact(&mut snapshot, &RedactOptions::default());

    let nrs = snapshot.non_rpm_software.as_ref().unwrap();
    assert!(
        !nrs.items[0].git_remote.contains("git_secret_66"),
        "credentials in git URL must be redacted, got: {}",
        nrs.items[0].git_remote
    );
    assert!(
        !snapshot.redactions.is_empty(),
        "redactions must contain findings from git remote URL"
    );
}

// ===================================================================
// Absence proofs — planted secrets must not survive into ANY artifact
// ===================================================================

/// Build a snapshot with planted secrets in ALL Slice 2c surfaces.
fn snapshot_with_all_planted_secrets() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();

    // Config file with password
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/myapp/db.conf".into(),
            content: "host=localhost\npassword=cfg_secret_42\nport=5432\n".into(),
            include: true,
            ..Default::default()
        }],
    });

    // .env file with database URL
    snap.non_rpm_software = Some(NonRpmSoftwareSection {
        env_files: vec![ConfigFileEntry {
            path: "/opt/myapp/.env".into(),
            content: "DATABASE_URL=postgres://user:env_secret_99@host/db\nPORT=3000\n".into(),
            include: true,
            ..Default::default()
        }],
        items: vec![NonRpmItem {
            path: "/opt/myapp".into(),
            name: "myapp".into(),
            method: "git repo".into(),
            git_remote: "https://deploy:git_secret_66@github.com/corp/myapp.git".into(),
            include: true,
            ..Default::default()
        }],
    });

    // Scheduled tasks: cron, at, timer
    snap.scheduled_tasks = Some(ScheduledTaskSection {
        generated_timer_units: vec![GeneratedTimerUnit {
            name: "backup.timer".into(),
            command: "/usr/bin/backup --password=cron_secret_88".into(),
            source_path: "/etc/cron.d/backup".into(),
            include: true,
            ..Default::default()
        }],
        at_jobs: vec![AtJob {
            file: "/var/spool/at/a00001".into(),
            command: "/usr/bin/sync --secret=atjob_secret_55".into(),
            user: "root".into(),
            ..Default::default()
        }],
        systemd_timers: vec![SystemdTimer {
            name: "deploy.timer".into(),
            exec_start: "/usr/local/bin/deploy --token=timer_secret_77".into(),
            source: "local".into(),
            path: "/etc/systemd/system/deploy.timer".into(),
            ..Default::default()
        }],
        ..Default::default()
    });

    // SELinux: audit rules and PAM configs with planted secrets
    snap.selinux = Some(SelinuxSection {
        audit_rules: vec![
            "# audit config password=audit_secret_33 for testing".into(),
        ],
        pam_configs: vec![
            "auth required pam_exec.so password=pam_secret_11".into(),
        ],
        ..Default::default()
    });

    snap
}

/// All planted secret values that must be absent from every artifact.
const PLANTED_SECRETS: &[&str] = &[
    "cfg_secret_42",
    "env_secret_99",
    "timer_secret_77",
    "cron_secret_88",
    "atjob_secret_55",
    "audit_secret_33",
    "pam_secret_11",
    "git_secret_66",
];

// ---------------------------------------------------------------------------
// Absence Test 7: Snapshot JSON must not contain any planted secret
// ---------------------------------------------------------------------------

#[test]
fn test_planted_secret_absent_from_snapshot_json() {
    let mut snapshot = snapshot_with_all_planted_secrets();
    redact(&mut snapshot, &RedactOptions::default());

    let json = serde_json::to_string_pretty(&snapshot)
        .expect("snapshot must serialize to JSON");

    for secret in PLANTED_SECRETS {
        assert!(
            !json.contains(secret),
            "planted secret '{}' must not appear in snapshot JSON",
            secret
        );
    }
}

// ---------------------------------------------------------------------------
// Absence Test 8: Containerfile must not contain any planted secret
// ---------------------------------------------------------------------------

#[test]
fn test_planted_secret_absent_from_containerfile() {
    use inspectah_pipeline::render::containerfile::render_containerfile;

    let mut snapshot = snapshot_with_all_planted_secrets();
    redact(&mut snapshot, &RedactOptions::default());

    let containerfile = render_containerfile(&snapshot, None);

    for secret in PLANTED_SECRETS {
        assert!(
            !containerfile.contains(secret),
            "planted secret '{}' must not appear in Containerfile",
            secret
        );
    }
}

// ---------------------------------------------------------------------------
// Absence Test 9: Config tree files must not contain any planted secret
// ---------------------------------------------------------------------------

#[test]
fn test_planted_secret_absent_from_config_tree() {
    use inspectah_pipeline::render::configtree::write_config_tree;
    use std::fs;

    let mut snapshot = snapshot_with_all_planted_secrets();
    redact(&mut snapshot, &RedactOptions::default());

    let dir = tempfile::TempDir::new().unwrap();
    write_config_tree(&snapshot, dir.path()).unwrap();

    // Walk ALL files under the output directory and check each one
    fn check_dir_recursive(path: &std::path::Path, secrets: &[&str]) {
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    check_dir_recursive(&p, secrets);
                } else if p.is_file() {
                    let content = fs::read_to_string(&p).unwrap_or_default();
                    for secret in secrets {
                        assert!(
                            !content.contains(secret),
                            "planted secret '{}' must not appear in config tree file {:?}",
                            secret,
                            p
                        );
                    }
                }
            }
        }
    }

    check_dir_recursive(dir.path(), PLANTED_SECRETS);
}

// ---------------------------------------------------------------------------
// Absence Test 10: Audit report must not contain any planted secret
// ---------------------------------------------------------------------------

#[test]
fn test_planted_secret_absent_from_audit_report() {
    use inspectah_pipeline::render::audit::render_audit;

    let mut snapshot = snapshot_with_all_planted_secrets();
    redact(&mut snapshot, &RedactOptions::default());

    let audit = render_audit(&snapshot);

    for secret in PLANTED_SECRETS {
        assert!(
            !audit.contains(secret),
            "planted secret '{}' must not appear in audit report",
            secret
        );
    }
}

// ---------------------------------------------------------------------------
// Absence Test 11: Report HTML must not contain any planted secret
// ---------------------------------------------------------------------------

#[test]
fn test_planted_secret_absent_from_report_html() {
    use inspectah_core::traits::renderer::RenderContext;
    use inspectah_pipeline::render::report::render_report;

    let mut snapshot = snapshot_with_all_planted_secrets();
    redact(&mut snapshot, &RedactOptions::default());

    let html = render_report(&snapshot, &RenderContext { target: None });

    for secret in PLANTED_SECRETS {
        assert!(
            !html.contains(secret),
            "planted secret '{}' must not appear in report HTML",
            secret
        );
    }
}
