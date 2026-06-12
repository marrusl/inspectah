//! Renderer smoke tests for Slice 2c sections: scheduled_tasks, config, selinux, non_rpm_software.
//!
//! These are SMOKE tests: they prove data REACHES the renderer, not that every
//! field is perfectly formatted. Each test builds a snapshot manually (no
//! inspector execution), calls the relevant renderer, and checks for key
//! markers in the output.
//!
//! Tests 1–5:   Containerfile renderer
//! Tests 6–7:   ConfigTree renderer
//! Tests 8–11:  Env-files renderer (NEW output path)
//! Tests 12–13: Kickstart / Readme renderers
//! Tests 14–17: Audit renderer (NEW sections)
//! Tests 18–21: Report renderer (NEW summary cards)
//! Tests 22–23: Rendered-output absence proofs
//! Tests 24–27: Negative contract tests

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::renderer::RenderContext;
use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind, ConfigSection};
use inspectah_core::types::nonrpm::{NonRpmItem, NonRpmSoftwareSection};
use inspectah_core::types::scheduled::{
    AtJob, CronJob, GeneratedTimerUnit, ScheduledTaskSection, SystemdTimer,
};
use inspectah_core::types::selinux::{CarryForwardFile, SelinuxPortLabel, SelinuxSection};
use inspectah_pipeline::render::{audit, configtree, containerfile, kickstart, readme, report};
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers: snapshot builders
// ---------------------------------------------------------------------------

fn snapshot_with_scheduled_tasks() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.scheduled_tasks = Some(ScheduledTaskSection {
        cron_jobs: vec![
            CronJob {
                path: "etc/cron.d/backup".into(),
                source: "cron.d".into(),
                include: true,
                locked: false,
                ..Default::default()
            },
            CronJob {
                // CronJob.source holds the collector source label (e.g. "cron.d"),
                // NOT the cron expression. The @reboot expression lives on
                // GeneratedTimerUnit.cron_expr.
                path: "etc/cron.d/reboot-task".into(),
                source: "cron.d".into(),
                include: true,
                locked: false,
                ..Default::default()
            },
        ],
        systemd_timers: vec![SystemdTimer {
            name: "logrotate".into(),
            source: "local".into(),
            timer_content: "[Timer]\nOnCalendar=daily".into(),
            service_content: "[Service]\nExecStart=/usr/sbin/logrotate".into(),
            include: true,
            locked: false,
            ..Default::default()
        }],
        generated_timer_units: vec![
            GeneratedTimerUnit {
                name: "backup-cron".into(),
                timer_content: "[Timer]\nOnCalendar=*-*-* 02:00:00".into(),
                service_content: "[Service]\nExecStart=/usr/bin/backup".into(),
                cron_expr: "0 2 * * *".into(),
                include: true,
                locked: false,
                ..Default::default()
            },
            // @reboot advisory: empty timer_content, boot-triggered service
            GeneratedTimerUnit {
                name: "cron-reboot-task".into(),
                timer_content: String::new(),
                service_content: "[Unit]\nDescription=Boot-triggered task\n\n\
                    [Service]\nType=oneshot\nExecStart=/usr/local/bin/startup.sh\n\n\
                    [Install]\nWantedBy=multi-user.target\n"
                    .into(),
                cron_expr: "@reboot".into(),
                source_path: "etc/cron.d/reboot-task".into(),
                command: "/usr/local/bin/startup.sh".into(),
                include: true,
                locked: false,
                ..Default::default()
            },
        ],
        at_jobs: vec![AtJob {
            file: "a00001".into(),
            command: "/tmp/scheduled-task.sh".into(),
            user: "root".into(),
            ..Default::default()
        }],
    });
    snap
}

fn snapshot_with_config_files() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        files: vec![
            ConfigFileEntry {
                path: "/etc/httpd/conf/httpd.conf".into(),
                content: "ServerRoot /etc/httpd".into(),
                kind: ConfigFileKind::RpmOwnedModified,
                include: true,
                locked: false,
                ..Default::default()
            },
            ConfigFileEntry {
                path: "/etc/sysconfig/network".into(),
                content: "NETWORKING=yes".into(),
                kind: ConfigFileKind::Unowned,
                include: true,
                locked: false,
                ..Default::default()
            },
        ],
    });
    snap
}

fn snapshot_with_selinux() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.selinux = Some(SelinuxSection {
        mode: "enforcing".into(),
        custom_modules: vec!["myapp".into(), "webapp".into()],
        boolean_overrides: vec![
            serde_json::json!({"name": "httpd_can_network_connect", "state": true}),
            serde_json::json!({"name": "container_manage_cgroup", "state": true}),
        ],
        fcontext_rules: vec!["/opt/app(/.*)?".into()],
        fips_mode: true,
        port_labels: vec![SelinuxPortLabel {
            protocol: "tcp".into(),
            port: "8443".into(),
            label_type: "http_port_t".into(),
            include: true,
            locked: false,
            ..Default::default()
        }],
        ..Default::default()
    });
    snap
}

fn snapshot_with_nonrpm() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.non_rpm_software = Some(NonRpmSoftwareSection {
        items: vec![
            NonRpmItem {
                path: "/opt/app/bin/myapp".into(),
                name: "myapp".into(),
                method: "binary".into(),
                confidence: "high".into(),
                include: true,
                locked: false,
                ..Default::default()
            },
            NonRpmItem {
                path: "/usr/local/lib/python3.9/site-packages/flask".into(),
                name: "flask".into(),
                method: "pip".into(),
                confidence: "high".into(),
                include: true,
                locked: false,
                ..Default::default()
            },
            NonRpmItem {
                path: "/usr/local/lib/node_modules/express".into(),
                name: "express".into(),
                method: "npm".into(),
                confidence: "high".into(),
                include: true,
                locked: false,
                ..Default::default()
            },
        ],
        env_files: vec![
            ConfigFileEntry {
                path: "/opt/app/.env".into(),
                content: "DB_HOST=localhost\nDB_PASS=secret123".into(),
                include: true,
                locked: false,
                ..Default::default()
            },
            ConfigFileEntry {
                path: "/srv/webapp/.env.production".into(),
                content: "API_KEY=abc123".into(),
                include: true,
                locked: false,
                ..Default::default()
            },
        ],
    });
    snap
}

fn snapshot_with_nonrpm_no_env() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.non_rpm_software = Some(NonRpmSoftwareSection {
        items: vec![NonRpmItem {
            path: "/opt/app/bin/myapp".into(),
            name: "myapp".into(),
            method: "binary".into(),
            confidence: "high".into(),
            include: true,
            locked: false,
            ..Default::default()
        }],
        env_files: vec![],
    });
    snap
}

// ---------------------------------------------------------------------------
// Tests 1–5: Containerfile renderer
// ---------------------------------------------------------------------------

#[test]
fn smoke_containerfile_scheduled() {
    let snap = snapshot_with_scheduled_tasks();
    let output = containerfile::render_containerfile(&snap, None, None);
    // Timer enable lines for included timers
    assert!(
        output.contains("systemctl enable") || output.contains("COPY config/etc/systemd/system/"),
        "must contain timer enable or COPY for timers"
    );
    // Cron-to-timer FIXME for generated timer units
    assert!(
        output.contains("backup-cron"),
        "must reference generated timer unit name"
    );
}

#[test]
fn smoke_containerfile_config() {
    let snap = snapshot_with_config_files();
    let output = containerfile::render_containerfile(&snap, None, None);
    // COPY comments for config files
    assert!(
        output.contains("COPY config/"),
        "must contain COPY line for config directory"
    );
}

#[test]
fn smoke_containerfile_selinux() {
    let snap = snapshot_with_selinux();
    let output = containerfile::render_containerfile(&snap, None, None);
    // Custom modules trigger FIXME + commented semodule lines
    assert!(
        output.contains("custom policy module"),
        "must reference custom policy modules"
    );
    assert!(
        output.contains("semodule"),
        "must contain semodule instruction"
    );
    // Port labels trigger semanage port
    assert!(
        output.contains("semanage port"),
        "must contain semanage port for custom port labels"
    );
}

#[test]
fn smoke_containerfile_nonrpm() {
    // The containerfile renderer only emits non-RPM items with
    // review_status == "migration_planned". Build a snapshot with that status.
    let mut snap = InspectionSnapshot::new();
    snap.non_rpm_software = Some(NonRpmSoftwareSection {
        items: vec![
            NonRpmItem {
                path: "/opt/app/bin/myapp".into(),
                name: "myapp".into(),
                method: "binary".into(),
                confidence: "high".into(),
                include: true,
                locked: false,
                review_status: "migration_planned".into(),
                ..Default::default()
            },
            NonRpmItem {
                path: "/usr/local/lib/python3.9/site-packages/flask".into(),
                name: "flask".into(),
                method: "pip dist-info".into(),
                confidence: "high".into(),
                include: true,
                locked: false,
                review_status: "migration_planned".into(),
                version: "2.3.0".into(),
                ..Default::default()
            },
        ],
        env_files: vec![],
    });
    let output = containerfile::render_containerfile(&snap, None, None);
    // Non-RPM migration stubs
    assert!(
        output.contains("Non-RPM Software"),
        "must contain Non-RPM Software header"
    );
    // pip provisioning
    assert!(
        output.contains("pip install flask"),
        "must reference pip install for migration_planned pip packages"
    );
}

#[test]
fn smoke_containerfile_env_fixme() {
    // The containerfile renderer does not currently emit env-file COPY lines.
    // Verify that .env files do NOT produce uncommented COPY lines (they are
    // handled by the separate env-files/ output path, not the Containerfile).
    let snap = snapshot_with_nonrpm();
    let output = containerfile::render_containerfile(&snap, None, None);
    assert!(
        !output.contains("COPY env-files/"),
        "containerfile must NOT contain uncommented COPY env-files/ line"
    );
}

// ---------------------------------------------------------------------------
// Tests 6–7: ConfigTree renderer
// ---------------------------------------------------------------------------

#[test]
fn smoke_configtree_config_files() {
    let snap = snapshot_with_config_files();
    let dir = TempDir::new().unwrap();
    configtree::write_config_tree(&snap, dir.path()).unwrap();
    // Config files materialized under config/
    assert!(
        dir.path().join("config/etc/httpd/conf/httpd.conf").exists(),
        "must materialize httpd.conf under config/"
    );
    assert!(
        dir.path().join("config/etc/sysconfig/network").exists(),
        "must materialize network config under config/"
    );
}

#[test]
fn smoke_configtree_timers() {
    let snap = snapshot_with_scheduled_tasks();
    let dir = TempDir::new().unwrap();
    configtree::write_config_tree(&snap, dir.path()).unwrap();
    // Normal timer units under config/etc/systemd/system/
    assert!(
        dir.path()
            .join("config/etc/systemd/system/backup-cron.timer")
            .exists(),
        "must materialize generated timer unit"
    );
    assert!(
        dir.path()
            .join("config/etc/systemd/system/backup-cron.service")
            .exists(),
        "must materialize generated service unit"
    );
    // @reboot advisory: service materialized, NO timer file
    assert!(
        dir.path()
            .join("config/etc/systemd/system/cron-reboot-task.service")
            .exists(),
        "@reboot must materialize boot-triggered service unit"
    );
    assert!(
        !dir.path()
            .join("config/etc/systemd/system/cron-reboot-task.timer")
            .exists(),
        "@reboot must NOT materialize a timer file (empty timer_content)"
    );
}

// ---------------------------------------------------------------------------
// Tests 8–11: Env-files renderer (NEW output path)
// ---------------------------------------------------------------------------

#[test]
fn smoke_env_files_materialized() {
    let snap = snapshot_with_nonrpm();
    let dir = TempDir::new().unwrap();
    configtree::write_env_files(&snap, dir.path()).unwrap();
    // .env files under env-files/
    assert!(
        dir.path().join("env-files/opt/app/.env").exists(),
        "must materialize .env file under env-files/"
    );
    assert!(
        dir.path()
            .join("env-files/srv/webapp/.env.production")
            .exists(),
        "must materialize .env.production under env-files/"
    );
}

#[test]
fn smoke_env_files_not_in_config() {
    let snap = snapshot_with_nonrpm();
    let dir = TempDir::new().unwrap();
    configtree::write_config_tree(&snap, dir.path()).unwrap();
    // .env files NOT under config/
    assert!(
        !dir.path().join("config/opt/app/.env").exists(),
        ".env files must NOT be under config/"
    );
    assert!(
        !dir.path()
            .join("config/srv/webapp/.env.production")
            .exists(),
        ".env.production must NOT be under config/"
    );
}

#[test]
fn smoke_env_files_content_preserved() {
    let snap = snapshot_with_nonrpm();
    let dir = TempDir::new().unwrap();
    configtree::write_env_files(&snap, dir.path()).unwrap();
    let content = std::fs::read_to_string(dir.path().join("env-files/opt/app/.env")).unwrap();
    assert!(
        content.contains("DB_HOST=localhost"),
        "must preserve .env content"
    );
    assert!(
        content.contains("DB_PASS=secret123"),
        "must preserve all env vars"
    );
}

#[test]
fn smoke_env_files_empty_section() {
    let snap = snapshot_with_nonrpm_no_env();
    let dir = TempDir::new().unwrap();
    configtree::write_env_files(&snap, dir.path()).unwrap();
    // No env-files/ directory when no .env files
    assert!(
        !dir.path().join("env-files").exists(),
        "env-files/ directory must not be created when there are no .env files"
    );
}

// ---------------------------------------------------------------------------
// Tests 12–13: Kickstart / Readme renderers
// ---------------------------------------------------------------------------

#[test]
fn smoke_kickstart_scheduled() {
    // Kickstart currently doesn't render scheduled tasks directly,
    // but verify no panic and basic structure when scheduled data is present
    let snap = snapshot_with_scheduled_tasks();
    let ks = kickstart::render_kickstart(&snap);
    assert!(
        ks.contains("#version="),
        "must contain version header even with scheduled data"
    );
}

#[test]
fn smoke_readme_findings() {
    let mut snap = snapshot_with_nonrpm();
    snap.scheduled_tasks = snapshot_with_scheduled_tasks().scheduled_tasks;
    let md = readme::render_readme(&snap);
    assert!(
        md.contains("## Findings summary"),
        "must contain findings summary heading"
    );
    // Non-RPM items should appear in the summary table
    assert!(
        md.contains("Non-RPM"),
        "findings summary must include Non-RPM items"
    );
}

// ---------------------------------------------------------------------------
// Tests 14–17: Audit renderer (NEW sections)
// ---------------------------------------------------------------------------

#[test]
fn audit_scheduled_section() {
    let snap = snapshot_with_scheduled_tasks();
    let md = audit::render_audit(&snap);
    assert!(
        md.contains("## Scheduled Tasks"),
        "must contain Scheduled Tasks heading"
    );
    assert!(md.contains("Cron jobs:"), "must show cron job count");
    assert!(md.contains("Systemd timers:"), "must show timer count");
    assert!(md.contains("At jobs:"), "must show at job count");
    // @reboot warning
    assert!(md.contains("@reboot"), "must warn about @reboot cron jobs");
}

#[test]
fn audit_config_section() {
    let snap = snapshot_with_config_files();
    let md = audit::render_audit(&snap);
    assert!(
        md.contains("## Configuration Files"),
        "must contain Configuration Files heading"
    );
    assert!(md.contains("httpd.conf"), "must list modified config file");
}

#[test]
fn audit_selinux_section() {
    let snap = snapshot_with_selinux();
    let md = audit::render_audit(&snap);
    assert!(
        md.contains("## Security & Access Control"),
        "must contain Security & Access Control heading"
    );
    assert!(md.contains("enforcing"), "must show SELinux mode");
    assert!(
        md.contains("Custom modules:"),
        "must show custom module count"
    );
    assert!(md.contains("FIPS mode:"), "must show FIPS status");
}

#[test]
fn audit_nonrpm_section() {
    let snap = snapshot_with_nonrpm();
    let md = audit::render_audit(&snap);
    assert!(
        md.contains("## Non-RPM Software"),
        "must contain Non-RPM Software heading"
    );
    assert!(md.contains("Items (3)"), "must show item count");
    // Method breakdown
    assert!(md.contains("binary"), "must show binary method");
    assert!(md.contains("pip"), "must show pip method");
    assert!(md.contains("npm"), "must show npm method");
    // .env warning
    assert!(md.contains(".env"), "must warn about .env files");
}

// ---------------------------------------------------------------------------
// Tests 18–21: Report renderer (NEW summary cards)
// ---------------------------------------------------------------------------

#[test]
#[ignore = "summary cards deferred to T6; un-ignore when summary card template lands"]
fn report_scheduled_section() {
    let snap = snapshot_with_scheduled_tasks();
    let html = report::render_report(&snap, &RenderContext { target: None });
    assert!(
        html.contains("Scheduled Tasks"),
        "must contain Scheduled Tasks summary card"
    );
    // Total: 2 cron + 1 timer + 2 generated + 1 at = 6
    assert!(html.contains(">6<"), "scheduled count must be 6");
}

#[test]
fn report_config_section() {
    let snap = snapshot_with_config_files();
    let html = report::render_report(&snap, &RenderContext { target: None });
    assert!(
        html.contains("Configuration Files"),
        "must contain Configuration Files summary card"
    );
    // 2 included config files
    assert!(html.contains(">2<"), "config count must be 2");
}

#[test]
fn report_selinux_section() {
    let snap = snapshot_with_selinux();
    let html = report::render_report(&snap, &RenderContext { target: None });
    assert!(
        html.contains("Security"),
        "must contain Security summary card"
    );
    assert!(html.contains("enforcing"), "security card must show mode");
}

#[test]
#[ignore = "summary cards deferred to T6; un-ignore when summary card template lands"]
fn report_nonrpm_section() {
    let snap = snapshot_with_nonrpm();
    let html = report::render_report(&snap, &RenderContext { target: None });
    assert!(
        html.contains("Non-RPM Items"),
        "must contain Non-RPM Items summary card"
    );
    assert!(html.contains(">3<"), "non-RPM count must be 3");
}

// ---------------------------------------------------------------------------
// Test: @reboot audit detection from GeneratedTimerUnit.cron_expr
// ---------------------------------------------------------------------------

/// Regression: @reboot detection comes from GeneratedTimerUnit.cron_expr,
/// NOT from CronJob.source. CronJob.source holds collector labels like
/// "cron.d" or "crontab" — never the cron expression.
#[test]
fn audit_reboot_detected_from_generated_units() {
    // Build a snapshot with collector-shaped CronJob (source="cron.d")
    // and a GeneratedTimerUnit with cron_expr="@reboot"
    let mut snap = InspectionSnapshot::new();
    snap.scheduled_tasks = Some(ScheduledTaskSection {
        cron_jobs: vec![CronJob {
            path: "etc/cron.d/startup".into(),
            source: "cron.d".into(), // collector-shaped, never contains @reboot
            include: true,
            locked: false,
            ..Default::default()
        }],
        generated_timer_units: vec![GeneratedTimerUnit {
            name: "cron-startup".into(),
            timer_content: String::new(), // no fake timer
            service_content: "[Service]\nType=oneshot\nExecStart=/opt/init.sh".into(),
            cron_expr: "@reboot".into(),
            include: true,
            locked: false,
            ..Default::default()
        }],
        ..Default::default()
    });

    let md = audit::render_audit(&snap);
    assert!(
        md.contains("@reboot"),
        "audit must detect @reboot from GeneratedTimerUnit.cron_expr"
    );
    assert!(
        md.contains("manual handling"),
        "audit must warn about manual handling for @reboot"
    );
}

/// Negative regression: CronJob.source="cron.d" must NOT trigger
/// the @reboot warning on its own.
#[test]
fn audit_no_false_reboot_from_cronjob_source() {
    let mut snap = InspectionSnapshot::new();
    snap.scheduled_tasks = Some(ScheduledTaskSection {
        cron_jobs: vec![CronJob {
            path: "etc/cron.d/backup".into(),
            source: "cron.d".into(),
            include: true,
            locked: false,
            ..Default::default()
        }],
        generated_timer_units: vec![GeneratedTimerUnit {
            name: "backup-cron".into(),
            timer_content: "[Timer]\nOnCalendar=*-*-* 02:00:00".into(),
            service_content: "[Service]\nExecStart=/usr/bin/backup".into(),
            cron_expr: "0 2 * * *".into(),
            include: true,
            locked: false,
            ..Default::default()
        }],
        ..Default::default()
    });

    let md = audit::render_audit(&snap);
    assert!(
        !md.contains("@reboot"),
        "audit must NOT warn about @reboot when no generated unit has cron_expr=@reboot"
    );
}

// ---------------------------------------------------------------------------
// Tests 22–23: Rendered-output absence proofs
// ---------------------------------------------------------------------------

#[test]
fn audit_no_content_for_missing_scheduled() {
    let snap = InspectionSnapshot::new();
    let md = audit::render_audit(&snap);
    assert!(
        !md.contains("## Scheduled Tasks"),
        "audit must not produce Scheduled Tasks heading when section is None"
    );
}

#[test]
#[ignore = "summary cards deferred to T6; un-ignore when summary card template lands"]
fn report_no_card_for_missing_nonrpm() {
    let snap = InspectionSnapshot::new();
    let html = report::render_report(&snap, &RenderContext { target: None });
    // Non-RPM card should still be present (with 0) since the summary grid
    // always renders all cards. But verify the count is 0.
    assert!(
        html.contains("Non-RPM Items"),
        "Non-RPM card is always rendered in the summary grid"
    );
    assert!(
        html.contains(">0<") || html.contains(">n/a<"),
        "Non-RPM count must be 0 or n/a when section is None"
    );
}

// ---------------------------------------------------------------------------
// Tests 24–27: Negative contract tests
// ---------------------------------------------------------------------------

#[test]
fn configtree_vendor_timers_not_copied() {
    // Vendor timers from /usr/lib/systemd/system/ MUST NOT appear in config tree
    let mut snap = InspectionSnapshot::new();
    snap.scheduled_tasks = Some(ScheduledTaskSection {
        systemd_timers: vec![SystemdTimer {
            name: "fstrim".into(),
            source: "vendor".into(),
            path: "/usr/lib/systemd/system/fstrim.timer".into(),
            timer_content: "[Timer]\nOnCalendar=weekly".into(),
            service_content: "[Service]\nExecStart=/usr/sbin/fstrim -a".into(),
            include: false, // vendor timers excluded
            ..Default::default()
        }],
        ..Default::default()
    });
    let dir = TempDir::new().unwrap();
    configtree::write_config_tree(&snap, dir.path()).unwrap();
    assert!(
        !dir.path()
            .join("config/usr/lib/systemd/system/fstrim.timer")
            .exists(),
        "vendor timer from /usr/lib/systemd/system/ must NOT appear in config tree"
    );
}

#[test]
fn configtree_cron_spool_not_materialized() {
    // Cron spool from /var MUST NOT be materialized into config tree
    let mut snap = InspectionSnapshot::new();
    snap.scheduled_tasks = Some(ScheduledTaskSection {
        cron_jobs: vec![CronJob {
            path: "/var/spool/cron/root".into(),
            source: "spool".into(),
            include: true,
            locked: false,
            ..Default::default()
        }],
        ..Default::default()
    });
    let dir = TempDir::new().unwrap();
    configtree::write_config_tree(&snap, dir.path()).unwrap();
    // Cron jobs are advisory metadata, not materialized files
    assert!(
        !dir.path().join("config/var/spool/cron/root").exists(),
        "cron spool from /var must NOT be materialized in config tree"
    );
}

#[test]
fn configtree_audit_rules_materialized() {
    // Audit rules from SELinux inspector ARE materialized in config tree
    let mut snap = InspectionSnapshot::new();
    snap.selinux = Some(SelinuxSection {
        audit_rules: vec![CarryForwardFile {
            path: "etc/audit/rules.d/custom-compliance.rules".into(),
            content: "-w /etc/shadow -p wa -k shadow_changes".into(),
        }],
        ..Default::default()
    });
    let dir = TempDir::new().unwrap();
    configtree::write_config_tree(&snap, dir.path()).unwrap();
    let rule_path = dir
        .path()
        .join("config/etc/audit/rules.d/custom-compliance.rules");
    assert!(
        rule_path.exists(),
        "audit rules from SELinux must be materialized in config tree"
    );
    let content = std::fs::read_to_string(&rule_path).unwrap();
    assert_eq!(
        content, "-w /etc/shadow -p wa -k shadow_changes",
        "audit rule content must be preserved"
    );
}

#[test]
fn configtree_pam_materialized() {
    // PAM configs from SELinux inspector ARE materialized in config tree
    let mut snap = InspectionSnapshot::new();
    snap.selinux = Some(SelinuxSection {
        pam_configs: vec![CarryForwardFile {
            path: "etc/pam.d/custom-faillock".into(),
            content: "auth required pam_faillock.so".into(),
        }],
        ..Default::default()
    });
    let dir = TempDir::new().unwrap();
    configtree::write_config_tree(&snap, dir.path()).unwrap();
    let pam_path = dir.path().join("config/etc/pam.d/custom-faillock");
    assert!(
        pam_path.exists(),
        "PAM configs from SELinux must be materialized in config tree"
    );
    let content = std::fs::read_to_string(&pam_path).unwrap();
    assert_eq!(
        content, "auth required pam_faillock.so",
        "PAM config content must be preserved"
    );
}
